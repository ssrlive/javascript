/// $262.agent support for test262 multi-agent (multi-threaded) tests.
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};

thread_local! {
    static IS_AGENT_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static LAST_BROADCAST_SEEN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[derive(Clone)]
pub struct BroadcastInfo {
    pub shared_buffer_id: u64,
}

struct SharedWaiter {
    state: Mutex<WaiterState>,
    cv: Condvar,
}

#[derive(Default)]
struct WaiterState {
    notified: bool,
    complete: bool,
}

struct SharedBuffer {
    bytes: Mutex<Vec<u8>>,
    waiters: Mutex<HashMap<usize, VecDeque<Arc<SharedWaiter>>>>,
}

static AGENT_REPORTS: LazyLock<Mutex<VecDeque<String>>> = LazyLock::new(|| Mutex::new(VecDeque::new()));
static AGENT_HANDLES: LazyLock<Mutex<Vec<thread::JoinHandle<()>>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static SHARED_BUFFERS: LazyLock<Mutex<HashMap<u64, Arc<SharedBuffer>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_SHARED_BUFFER_ID: AtomicU64 = AtomicU64::new(1);
static BROADCAST_DATA: LazyLock<Mutex<Option<BroadcastInfo>>> = LazyLock::new(|| Mutex::new(None));
static BROADCAST_CV: LazyLock<(Mutex<u32>, Condvar)> = LazyLock::new(|| (Mutex::new(0), Condvar::new()));
static AGENT_COUNT: AtomicU32 = AtomicU32::new(0);
static START_TIME: LazyLock<Instant> = LazyLock::new(Instant::now);

pub fn is_agent_thread() -> bool {
    IS_AGENT_THREAD.with(|c| c.get())
}

pub fn monotonic_now_ms() -> f64 {
    START_TIME.elapsed().as_secs_f64() * 1000.0
}

pub fn sleep_ms(ms: f64) {
    let millis = if !ms.is_finite() || ms <= 0.0 { 0 } else { ms.trunc() as u64 };
    thread::sleep(Duration::from_millis(millis));
}

pub fn push_report(report: String) {
    AGENT_REPORTS.lock().unwrap().push_back(report);
}

pub fn pop_report() -> Option<String> {
    AGENT_REPORTS.lock().unwrap().pop_front()
}

pub fn register_shared_buffer(initial_bytes: Vec<u8>) -> u64 {
    let id = NEXT_SHARED_BUFFER_ID.fetch_add(1, Ordering::SeqCst);
    let buffer = Arc::new(SharedBuffer {
        bytes: Mutex::new(initial_bytes),
        waiters: Mutex::new(HashMap::new()),
    });
    SHARED_BUFFERS.lock().unwrap().insert(id, buffer);
    id
}

fn get_shared_buffer(id: u64) -> Option<Arc<SharedBuffer>> {
    SHARED_BUFFERS.lock().unwrap().get(&id).cloned()
}

pub fn shared_buffer_len(id: u64) -> Option<usize> {
    let buffer = get_shared_buffer(id)?;
    Some(buffer.bytes.lock().unwrap().len())
}

pub fn shared_buffer_snapshot(id: u64) -> Option<Vec<u8>> {
    let buffer = get_shared_buffer(id)?;
    Some(buffer.bytes.lock().unwrap().clone())
}

pub fn shared_buffer_read(id: u64, base: usize, len: usize) -> Option<Vec<u8>> {
    let buffer = get_shared_buffer(id)?;
    let bytes = buffer.bytes.lock().unwrap();
    if base.checked_add(len)? > bytes.len() {
        return None;
    }
    Some(bytes[base..base + len].to_vec())
}

pub fn shared_buffer_write(id: u64, base: usize, data: &[u8]) -> bool {
    let Some(buffer) = get_shared_buffer(id) else {
        return false;
    };
    let mut bytes = buffer.bytes.lock().unwrap();
    let Some(end) = base.checked_add(data.len()) else {
        return false;
    };
    if end > bytes.len() {
        return false;
    }
    bytes[base..end].copy_from_slice(data);
    true
}

pub fn shared_buffer_compare_exchange(id: u64, base: usize, expected: &[u8], replacement: &[u8]) -> Option<Vec<u8>> {
    if expected.len() != replacement.len() {
        return None;
    }
    let buffer = get_shared_buffer(id)?;
    let mut bytes = buffer.bytes.lock().unwrap();
    let end = base.checked_add(expected.len())?;
    if end > bytes.len() {
        return None;
    }
    let current = bytes[base..end].to_vec();
    if current == expected {
        bytes[base..end].copy_from_slice(replacement);
    }
    Some(current)
}

pub fn atomics_wait(id: u64, byte_index: usize, timeout_ms: Option<f64>) -> Option<bool> {
    let buffer = get_shared_buffer(id)?;
    let waiter = Arc::new(SharedWaiter {
        state: Mutex::new(WaiterState::default()),
        cv: Condvar::new(),
    });

    {
        let mut waiters = buffer.waiters.lock().unwrap();
        waiters.entry(byte_index).or_default().push_back(waiter.clone());
    }

    let mut state = waiter.state.lock().unwrap();
    if let Some(timeout) = timeout_ms.filter(|v| v.is_finite()) {
        let duration = if timeout <= 0.0 {
            Duration::from_millis(0)
        } else {
            Duration::from_secs_f64(timeout / 1000.0)
        };
        let (new_state, _) = waiter.cv.wait_timeout_while(state, duration, |s| !s.notified).unwrap();
        state = new_state;
    } else {
        while !state.notified {
            state = waiter.cv.wait(state).unwrap();
        }
    }
    let was_notified = state.notified;
    state.complete = true;
    drop(state);

    let mut waiters = buffer.waiters.lock().unwrap();
    if let Some(queue) = waiters.get_mut(&byte_index) {
        queue.retain(|entry| !Arc::ptr_eq(entry, &waiter));
        if queue.is_empty() {
            waiters.remove(&byte_index);
        }
    }
    Some(was_notified)
}

pub fn atomics_notify(id: u64, byte_index: usize, count: usize) -> Option<usize> {
    let buffer = get_shared_buffer(id)?;
    let mut notified = 0usize;
    let mut waiters = buffer.waiters.lock().unwrap();
    let queue = waiters.get_mut(&byte_index)?;
    let wake_limit = if count == usize::MAX { queue.len() } else { count };

    for waiter in queue.iter() {
        if notified >= wake_limit {
            break;
        }
        let mut state = waiter.state.lock().unwrap();
        if state.complete || state.notified {
            continue;
        }
        state.notified = true;
        state.complete = true;
        notified += 1;
        waiter.cv.notify_one();
    }

    queue.retain(|entry| {
        let state = entry.state.lock().unwrap();
        !state.complete
    });
    if queue.is_empty() {
        waiters.remove(&byte_index);
    }
    Some(notified)
}

pub fn broadcast_shared_buffer(shared_buffer_id: u64) -> bool {
    let Some(_byte_length) = shared_buffer_len(shared_buffer_id) else {
        return false;
    };
    {
        let mut data = BROADCAST_DATA.lock().unwrap();
        *data = Some(BroadcastInfo { shared_buffer_id });
    }
    let (lock, cv) = &*BROADCAST_CV;
    let mut generation = lock.lock().unwrap();
    *generation = generation.wrapping_add(1);
    cv.notify_all();
    true
}

pub fn wait_for_broadcast() -> Option<BroadcastInfo> {
    let (lock, cv) = &*BROADCAST_CV;
    let mut generation = lock.lock().unwrap();
    let last_seen = LAST_BROADCAST_SEEN.with(|cell| cell.get());
    while *generation <= last_seen {
        generation = cv.wait(generation).unwrap();
    }
    let seen_generation = *generation;
    LAST_BROADCAST_SEEN.with(|cell| cell.set(seen_generation));
    BROADCAST_DATA.lock().unwrap().clone()
}

pub fn start_agent(script: String) {
    AGENT_COUNT.fetch_add(1, Ordering::SeqCst);
    let handle = thread::spawn(move || {
        IS_AGENT_THREAD.with(|c| c.set(true));
        LAST_BROADCAST_SEEN.with(|c| c.set(0));
        let result = crate::core::evaluate_script_with_unwrap(script, false, Option::<&std::path::Path>::None, false);
        if let Err(err) = result {
            push_report(format!("__agent_error__:{err}"));
        }
    });
    AGENT_HANDLES.lock().unwrap().push(handle);
}

pub fn reset_agent_state() {
    let mut handles = AGENT_HANDLES.lock().unwrap();
    let pending = std::mem::take(&mut *handles);
    drop(handles);
    for handle in pending {
        let _ = handle.join();
    }

    AGENT_REPORTS.lock().unwrap().clear();
    SHARED_BUFFERS.lock().unwrap().clear();
    *BROADCAST_DATA.lock().unwrap() = None;
    AGENT_COUNT.store(0, Ordering::SeqCst);
    NEXT_SHARED_BUFFER_ID.store(1, Ordering::SeqCst);
    let (lock, _) = &*BROADCAST_CV;
    *lock.lock().unwrap() = 0;
}
