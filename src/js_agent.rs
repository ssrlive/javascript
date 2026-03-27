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
use std::sync::{Condvar, LazyLock, Mutex};
thread_local! {
    static IS_AGENT_THREAD : std::cell::Cell < bool > = const {
    std::cell::Cell::new(false) };
}
/// Returns true if the current thread is an agent thread.
pub fn is_agent_thread() -> bool {
    IS_AGENT_THREAD.with(|c| c.get())
}
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
/// Generation counter for thread-local "already seen this broadcast" tracking.
static BROADCAST_GENERATION: AtomicU32 = AtomicU32::new(0);
#[derive(Clone)]
struct BroadcastInfo;
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
    {
        let (lock, _cv) = &*BROADCAST_CV;
        let mut generation = lock.lock().unwrap();
        *generation = 0;
    }
}
