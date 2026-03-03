/// $262.agent support for test262 multi-agent (multi-threaded) tests.
///
/// This module implements the test262 agent API which allows spawning
/// "agents" (conceptually separate JS execution threads) that share
/// SharedArrayBuffer memory. Communication between main thread and agents
/// happens through:
///   - broadcast: main sends a SharedArrayBuffer to all agents
///   - report queue: agents send string messages back to main
///
/// Architecture:
///   - Each agent runs in its own OS thread with its own GC arena
///   - SharedArrayBuffer sharing works because the backing store is
///     `Arc<Mutex<Vec<u8>>>` which is Send+Sync
///   - Global statics coordinate broadcast/report passing
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::Instant;

// Thread-local flag to mark agent threads (so evaluate_program skips reset_agent_state).
thread_local! {
    static IS_AGENT_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Returns true if the current thread is an agent thread.
pub fn is_agent_thread() -> bool {
    IS_AGENT_THREAD.with(|c| c.get())
}

// ─── Global agent communication state ──────────────────────────────────────

/// Report queue: agents push string reports, main thread pops them.
static AGENT_REPORTS: LazyLock<Mutex<VecDeque<String>>> = LazyLock::new(|| Mutex::new(VecDeque::new()));

/// Broadcast channel: main thread stores the SAB data + metadata here,
/// then notifies all waiting agents.
/// Tuple: (data Arc, byte_length, shared flag, broadcast_generation)
static BROADCAST_DATA: LazyLock<Mutex<Option<BroadcastInfo>>> = LazyLock::new(|| Mutex::new(None));

/// Condition variable for agents waiting to receive a broadcast.
static BROADCAST_CV: LazyLock<(Mutex<u64>, Condvar)> = LazyLock::new(|| (Mutex::new(0), Condvar::new()));

/// Number of agents started (for coordination).
static AGENT_COUNT: AtomicU32 = AtomicU32::new(0);

/// Monotonic clock epoch (set once at first use).
static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Generation counter for thread-local "already seen this broadcast" tracking.
static BROADCAST_GENERATION: AtomicU32 = AtomicU32::new(0);

#[derive(Clone)]
struct BroadcastInfo {
    data: Arc<Mutex<Vec<u8>>>,
    byte_length: usize,
    generation: u32,
}

// ─── Public API called from the evaluator ──────────────────────────────────

/// Reset global agent state. Called at the start of each test to ensure isolation.
pub fn reset_agent_state() {
    {
        let mut q = AGENT_REPORTS.lock().unwrap();
        q.clear();
    }
    {
        let mut b = BROADCAST_DATA.lock().unwrap();
        *b = None;
    }
    AGENT_COUNT.store(0, Ordering::SeqCst);
    BROADCAST_GENERATION.store(0, Ordering::SeqCst);
    // Reset the broadcast CV generation counter
    {
        let (lock, _cv) = &*BROADCAST_CV;
        let mut generation = lock.lock().unwrap();
        *generation = 0;
    }
}

/// `__agent_start(script)` — spawn an agent thread that runs the given script.
/// The script will have access to `__agent_receiveBroadcast()`,
/// `__agent_report(val)`, `__agent_leaving()`, `__agent_sleep(ms)`,
/// `__agent_monotonicNow()`.
pub fn agent_start(script: String) {
    AGENT_COUNT.fetch_add(1, Ordering::SeqCst);

    // The agent script is wrapped with a preamble that defines $262.agent.*
    // pointing to native __agent_* hooks.
    let agent_script = format!(
        r#"
var $262 = typeof $262 !== "undefined" ? $262 : {{}};
if (!$262.agent) $262.agent = {{}};
$262.agent.receiveBroadcast = function(cb) {{
    var sab = __agent_receiveBroadcast();
    cb(sab);
}};
$262.agent.report = function(val) {{ __agent_report(String(val)); }};
$262.agent.leaving = function() {{ __agent_leaving(); }};
$262.agent.sleep = function(ms) {{ __agent_sleep(ms); }};
$262.agent.monotonicNow = function() {{ return __agent_monotonicNow(); }};
{script}"#
    );

    let builder = std::thread::Builder::new()
        .name("test262-agent".into())
        .stack_size(16 * 1024 * 1024);

    builder
        .spawn(move || {
            // Mark this thread as an agent so evaluate_program skips reset_agent_state
            IS_AGENT_THREAD.with(|c| c.set(true));
            match crate::evaluate_script(&agent_script, Option::<&str>::None) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("[agent] error: {e}");
                }
            }
            // Ensure agent count is decremented when the agent exits
            // (in case __agent_leaving was not called)
            // We use a saturating sub to avoid underflow.
            let _ = AGENT_COUNT.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| if v > 0 { Some(v - 1) } else { Some(0) });
        })
        .expect("Failed to spawn agent thread");
}

/// `__agent_broadcast(sab_data_arc, byte_length)` — called from the evaluator
/// when JS calls `$262.agent.broadcast(sab)`. Extracts the Arc backing store
/// and makes it available to all agent threads.
pub fn agent_broadcast(data: Arc<Mutex<Vec<u8>>>, byte_length: usize) {
    let generation = BROADCAST_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    {
        let mut b = BROADCAST_DATA.lock().unwrap();
        *b = Some(BroadcastInfo {
            data,
            byte_length,
            generation,
        });
    }

    // Wake all agents waiting for a broadcast
    let (lock, cv) = &*BROADCAST_CV;
    let mut g = lock.lock().unwrap();
    *g = generation as u64;
    cv.notify_all();
}

/// `__agent_receiveBroadcast()` — called from within an agent thread.
/// Blocks until a broadcast is available, then returns the broadcast info
/// (data Arc + byte_length) so the evaluator can create a SharedArrayBuffer.
pub fn agent_receive_broadcast() -> (Arc<Mutex<Vec<u8>>>, usize) {
    let (lock, cv) = &*BROADCAST_CV;
    let mut generation = lock.lock().unwrap();

    // Wait until a broadcast is available
    loop {
        {
            let b = BROADCAST_DATA.lock().unwrap();
            if let Some(ref info) = *b {
                let info_gen = info.generation as u64;
                let cur_gen = *generation;
                if info_gen > 0 && info_gen >= cur_gen {
                    let result = (info.data.clone(), info.byte_length);
                    drop(b);
                    *generation = info_gen;
                    return result;
                }
            }
        }
        generation = cv.wait(generation).unwrap();
    }
}

/// `__agent_report(value)` — called from within an agent thread.
/// Pushes a string report to the shared queue.
pub fn agent_report(value: String) {
    let mut q = AGENT_REPORTS.lock().unwrap();
    q.push_back(value);
}

/// `__agent_getReport()` — called from the main thread.
/// Returns the next report string, or None if the queue is empty.
pub fn agent_get_report() -> Option<String> {
    let mut q = AGENT_REPORTS.lock().unwrap();
    q.pop_front()
}

/// `__agent_sleep(ms)` — block the current thread for the given milliseconds.
pub fn agent_sleep(ms: f64) {
    if ms > 0.0 {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }
}

/// `__agent_leaving()` — signal that the agent is leaving.
/// Decrements the agent count.
pub fn agent_leaving() {
    let _ = AGENT_COUNT.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| if v > 0 { Some(v - 1) } else { Some(0) });
}

/// `__agent_monotonicNow()` — return milliseconds since epoch.
pub fn agent_monotonic_now() -> f64 {
    EPOCH.elapsed().as_secs_f64() * 1000.0
}
