/// `$262.agent` support for Test262 multi-agent tests.
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};

thread_local! {
    static CURRENT_CONTEXT: std::cell::RefCell<Option<(u64, Arc<AgentContext>)>> = const { std::cell::RefCell::new(None) };
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

struct AgentContext {
    reports: Mutex<VecDeque<String>>,
    handles: Mutex<Vec<thread::JoinHandle<()>>>,
    shared_buffers: Mutex<HashMap<u64, Arc<SharedBuffer>>>,
    broadcast_data: Mutex<Option<BroadcastInfo>>,
    broadcast_cv: (Mutex<u32>, Condvar),
    agent_count: AtomicU32,
    start_time: Instant,
    #[cfg(feature = "jit")]
    jit_stats: Mutex<crate::runtime::JitStats>,
}

impl AgentContext {
    fn new() -> Self {
        Self {
            reports: Mutex::new(VecDeque::new()),
            handles: Mutex::new(Vec::new()),
            shared_buffers: Mutex::new(HashMap::new()),
            broadcast_data: Mutex::new(None),
            broadcast_cv: (Mutex::new(0), Condvar::new()),
            agent_count: AtomicU32::new(0),
            start_time: Instant::now(),
            #[cfg(feature = "jit")]
            jit_stats: Mutex::new(crate::runtime::JitStats::default()),
        }
    }
}

static CONTEXTS: LazyLock<Mutex<HashMap<u64, Arc<AgentContext>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_CONTEXT_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_SHARED_BUFFER_ID: AtomicU64 = AtomicU64::new(1);

fn context_for(id: u64) -> Arc<AgentContext> {
    let mut contexts = CONTEXTS.lock().unwrap();
    contexts.entry(id).or_insert_with(|| Arc::new(AgentContext::new())).clone()
}

fn current_context() -> Arc<AgentContext> {
    CURRENT_CONTEXT.with(|current| {
        current
            .borrow()
            .as_ref()
            .map(|(_, context)| context.clone())
            .unwrap_or_else(|| context_for(0))
    })
}

pub(crate) fn new_context_id() -> u64 {
    NEXT_CONTEXT_ID.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn current_context_id() -> u64 {
    CURRENT_CONTEXT.with(|current| current.borrow().as_ref().map(|(id, _)| *id).unwrap_or(0))
}

pub(crate) fn with_context<T>(id: u64, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<(u64, Arc<AgentContext>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT_CONTEXT.with(|current| *current.borrow_mut() = self.0.take());
        }
    }

    let context = context_for(id);
    let previous = CURRENT_CONTEXT.with(|current| current.borrow_mut().replace((id, context)));
    let _restore = Restore(previous);
    f()
}

pub fn is_agent_thread() -> bool {
    IS_AGENT_THREAD.with(|c| c.get())
}

pub fn monotonic_now_ms() -> f64 {
    current_context().start_time.elapsed().as_secs_f64() * 1000.0
}

pub fn sleep_ms(ms: f64) {
    let millis = if !ms.is_finite() || ms <= 0.0 { 0 } else { ms.trunc() as u64 };
    thread::sleep(Duration::from_millis(millis));
}

pub fn push_report(report: String) {
    current_context().reports.lock().unwrap().push_back(report);
}

pub fn pop_report() -> Option<String> {
    current_context().reports.lock().unwrap().pop_front()
}

pub fn register_shared_buffer(initial_bytes: Vec<u8>) -> u64 {
    let context = current_context();
    let id = NEXT_SHARED_BUFFER_ID.fetch_add(1, Ordering::Relaxed);
    let buffer = Arc::new(SharedBuffer {
        bytes: Mutex::new(initial_bytes),
        waiters: Mutex::new(HashMap::new()),
    });
    context.shared_buffers.lock().unwrap().insert(id, buffer);
    id
}

fn get_shared_buffer(id: u64) -> Option<Arc<SharedBuffer>> {
    current_context().shared_buffers.lock().unwrap().get(&id).cloned()
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
    let context = current_context();
    if shared_buffer_len(shared_buffer_id).is_none() {
        return false;
    }
    *context.broadcast_data.lock().unwrap() = Some(BroadcastInfo { shared_buffer_id });
    let (lock, cv) = &context.broadcast_cv;
    let mut generation = lock.lock().unwrap();
    *generation = generation.wrapping_add(1);
    cv.notify_all();
    true
}

pub fn wait_for_broadcast() -> Option<BroadcastInfo> {
    let context = current_context();
    let (lock, cv) = &context.broadcast_cv;
    let mut generation = lock.lock().unwrap();
    let last_seen = LAST_BROADCAST_SEEN.with(std::cell::Cell::get);
    while *generation <= last_seen {
        generation = cv.wait(generation).unwrap();
    }
    let seen_generation = *generation;
    LAST_BROADCAST_SEEN.with(|cell| cell.set(seen_generation));
    context.broadcast_data.lock().unwrap().clone()
}

#[cfg(feature = "jit")]
pub(crate) fn jit_stats(context_id: u64) -> crate::runtime::JitStats {
    *context_for(context_id).jit_stats.lock().unwrap()
}

pub fn start_agent(script: String, jit_required: bool) {
    let context_id = current_context_id();
    let context = context_for(context_id);
    context.agent_count.fetch_add(1, Ordering::SeqCst);
    let handle = thread::spawn(move || {
        with_context(context_id, || {
            IS_AGENT_THREAD.with(|c| c.set(true));
            LAST_BROADCAST_SEEN.with(|c| c.set(0));
            use crate::core::DEFAULT_GC_OPCODE_BUDGET as B;
            let result = crate::runtime::JsRuntime::with_gc_opcode_budget(B).and_then(|mut runtime| {
                #[cfg(feature = "jit")]
                runtime.set_jit_required(jit_required);
                #[cfg(not(feature = "jit"))]
                let _ = jit_required;
                let result = runtime.eval_script_with_unwrap(script, false, Option::<&std::path::Path>::None, false);
                #[cfg(feature = "jit")]
                {
                    // This context also owns nested agents; count only this VM here.
                    let stats = runtime.mutate_root(|_, vm| vm.jit_stats());
                    let context = current_context();
                    let mut total = context.jit_stats.lock().unwrap();
                    total.baseline_opcodes += stats.baseline_opcodes;
                    total.interpreter_opcodes += stats.interpreter_opcodes;
                    total.optimized_calls += stats.optimized_calls;
                }
                result
            });
            if let Err(err) = result {
                push_report(format!("__agent_error__:{err}"));
            }
        });
    });
    context.handles.lock().unwrap().push(handle);
}

pub fn reset_agent_state() {
    let context_id = current_context_id();
    let context = context_for(context_id);
    let pending = {
        let mut handles = context.handles.lock().unwrap();
        std::mem::take(&mut *handles)
    };
    for handle in pending {
        let _ = handle.join();
    }

    context.reports.lock().unwrap().clear();
    context.shared_buffers.lock().unwrap().clear();
    *context.broadcast_data.lock().unwrap() = None;
    context.agent_count.store(0, Ordering::SeqCst);
    #[cfg(feature = "jit")]
    {
        *context.jit_stats.lock().unwrap() = crate::runtime::JitStats::default();
    }
    let (lock, cv) = &context.broadcast_cv;
    *lock.lock().unwrap() = 0;
    cv.notify_all();
}

pub(crate) fn release_context(id: u64) {
    if id != 0
        && let Some(context) = CONTEXTS.lock().unwrap().remove(&id)
    {
        drop(std::mem::take(&mut *context.handles.lock().unwrap()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "jit")]
    #[test]
    fn agents_inherit_required_jit_without_interpreter_fallback() {
        for required in [false, true] {
            let context_id = new_context_id();
            with_context(context_id, || {
                start_agent(
                    r#"
                    function* values() { yield { value: 42 }; }
                    __agent_report(String(values().next().value.value));
                    "#
                    .to_string(),
                    required,
                );
                let context = current_context();
                let handles = std::mem::take(&mut *context.handles.lock().unwrap());
                for handle in handles {
                    handle.join().unwrap();
                }
                assert_eq!(pop_report().as_deref(), Some("42"));
                assert_eq!(pop_report(), None);
                let stats = jit_stats(context_id);
                if required {
                    assert!(stats.baseline_opcodes > 0, "{stats:?}");
                    assert_eq!(stats.interpreter_opcodes, 0, "{stats:?}");
                } else {
                    assert!(stats.interpreter_opcodes > 0, "{stats:?}");
                }
                reset_agent_state();
                assert_eq!(jit_stats(context_id), crate::runtime::JitStats::default());
            });
            release_context(context_id);
        }
    }

    #[cfg(feature = "jit")]
    #[test]
    fn nested_agent_counters_are_recorded_once() {
        let context_id = new_context_id();
        with_context(context_id, || {
            start_agent(
                r#"
                __agent_start("function twice(value) { return value * 2; } twice(21);");
                function twice(value) { return value * 2; }
                twice(21);
                "#
                .to_string(),
                true,
            );
            let context = current_context();
            loop {
                let handles = std::mem::take(&mut *context.handles.lock().unwrap());
                if handles.is_empty() {
                    break;
                }
                for handle in handles {
                    handle.join().unwrap();
                }
            }
            assert_eq!(pop_report(), None);
            let stats = jit_stats(context_id);
            assert_eq!(stats.interpreter_opcodes, 0, "{stats:?}");
            assert_eq!(stats.optimized_calls, 2, "{stats:?}");
        });
        release_context(context_id);
    }

    #[test]
    fn agent_state_is_scoped_to_runtime_context() {
        let first = new_context_id();
        let second = new_context_id();
        let first_buffer = with_context(first, || register_shared_buffer(vec![1]));
        let second_buffer = with_context(second, || register_shared_buffer(vec![2]));

        assert_eq!(with_context(first, || shared_buffer_read(first_buffer, 0, 1)), Some(vec![1]));
        assert_eq!(with_context(second, || shared_buffer_read(second_buffer, 0, 1)), Some(vec![2]));
        assert_eq!(with_context(first, || shared_buffer_read(second_buffer, 0, 1)), None);
        assert_eq!(with_context(second, || shared_buffer_read(first_buffer, 0, 1)), None);

        with_context(first, || push_report("first".to_string()));
        with_context(second, || push_report("second".to_string()));
        assert_eq!(with_context(first, pop_report), Some("first".to_string()));
        assert_eq!(with_context(second, pop_report), Some("second".to_string()));

        release_context(first);
        release_context(second);
    }
}
