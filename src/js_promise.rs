//! # JavaScript Promise Implementation
//!
//! This module implements JavaScript Promise functionality in Rust, including:
//! - Promise constructor and basic lifecycle (pending → fulfilled/rejected)
//! - Instance methods: then(), catch(), finally()
//! - Static methods: all(), race(), allSettled(), any()
//! - Asynchronous execution via event loop and task queue
//!
//! ## Architecture Overview
//!
//! The implementation uses several key components:
//!
//! 1. **JSPromise**: Core promise structure with state management
//! 2. **Task Queue**: Global asynchronous task execution system
//! 3. **Event Loop**: Processes queued tasks to enable async behavior
//! 4. **Internal Callbacks**: Helper functions for static method coordination
//!
//! ## Complexity Issues Addressed
//!
//! This implementation has evolved to handle complex scenarios like Promise.allSettled,
//! which requires coordinating multiple promises and maintaining shared state.
//! The current implementation uses JS objects for shared state, which adds complexity.
//!
//! Future refactoring will introduce dedicated Rust structures for better type safety.

use crate::core::{
    ClosureData, DestructuringElement, EvalError, Expr, JSGenerator, JSObjectData, JSObjectDataPtr, JSPromise, PromiseState, Statement,
    StatementKind, Value, env_set, evaluate_statements, extract_closure_from_value, generate_unique_id, object_get_key_value,
    object_set_key_value, prepare_closure_call_env, prepare_function_call_env, value_to_string,
};
use crate::core::{Gc, GcCell, GcPtr, MutationContext, new_gc_cell_ptr};
use crate::error::JSError;
use crate::unicode::utf8_to_utf16;
use crate::{new_js_object_data, utf16_to_utf8};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

pub fn stmt_expr(expr: Expr) -> Statement {
    Statement::from(StatementKind::Expr(expr))
}

// #[derive(Debug)]
#[allow(dead_code)]
enum Task<'gc> {
    /// Task to execute fulfilled callbacks with the resolved value
    Resolution {
        promise: GcPtr<'gc, JSPromise<'gc>>,
        callbacks: Vec<(Value<'gc>, GcPtr<'gc, JSPromise<'gc>>, Option<JSObjectDataPtr<'gc>>)>,
    },
    /// Task to execute rejected callbacks with the rejection reason
    Rejection {
        promise: GcPtr<'gc, JSPromise<'gc>>,
        callbacks: Vec<(Value<'gc>, GcPtr<'gc, JSPromise<'gc>>, Option<JSObjectDataPtr<'gc>>)>,
    },
    /// Task to execute an async function body asynchronously
    ExecuteClosure {
        function: Value<'gc>,
        args: Vec<Value<'gc>>,
        resolve: Value<'gc>, // Promise resolve function
        reject: Value<'gc>,  // Promise reject function
        this_val: Option<Value<'gc>>,
        env: JSObjectDataPtr<'gc>,
    },
    /// Task to attach handlers to a promise when a direct borrow fails (deferral mechanism)
    AttachHandlers {
        promise: GcPtr<'gc, JSPromise<'gc>>,
        on_fulfilled: Option<Value<'gc>>,
        on_rejected: Option<Value<'gc>>,
        result_promise: Option<GcPtr<'gc, JSPromise<'gc>>>,
        env: JSObjectDataPtr<'gc>,
    },
    /// Task to resolve a promise asynchronously with a value
    ResolvePromise {
        promise: GcPtr<'gc, JSPromise<'gc>>,
        value: Value<'gc>,
        env: JSObjectDataPtr<'gc>,
    },
    /// Task to resume an async function generator step
    AsyncStep {
        generator: GcPtr<'gc, JSGenerator<'gc>>,
        resolve: Value<'gc>,
        reject: Value<'gc>,
        result: Value<'gc>,
        is_reject: bool,
        env: JSObjectDataPtr<'gc>,
    },
    /// Task to execute a setTimeout callback
    Timeout {
        id: usize,
        callback: Value<'gc>,
        args: Vec<Value<'gc>>,
        target_time: Instant,
    },
    /// Task to execute a setInterval callback
    Interval {
        id: usize,
        callback: Value<'gc>,
        args: Vec<Value<'gc>>,
        target_time: Instant,
        interval: Duration,
    },
    /// Task to check for unhandled rejection after potential handler attachment
    /// `insertion_tick` records the CURRENT_TICK when the check was first scheduled
    UnhandledCheck {
        promise: GcPtr<'gc, JSPromise<'gc>>,
        reason: Value<'gc>,
        insertion_tick: usize,
        env: JSObjectDataPtr<'gc>,
        force: bool,
    },
    // Previously this variant represented a queued unhandled-check task.
    // Unhandled checks are now tracked separately in `PENDING_UNHANDLED_CHECKS`.
    // NOTE: Unhandled checks are now tracked in `PENDING_UNHANDLED_CHECKS`
    // rather than as queued tasks. Keeping the task enum slimmer avoids
    // accidental re-processing within the same run. The pending list is
    // processed once when the outermost `run_event_loop` finishes.
}

// Unhandled rejection accessors are runtime-backed (see below)

/// Return the current number of queued tasks in the global task queue.
pub fn create_promise_capability<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(GcPtr<'gc, JSPromise<'gc>>, Value<'gc>, Value<'gc>), JSError> {
    let promise = new_gc_cell_ptr(mc, JSPromise::new());

    // Create resolve/reject functions
    let resolve = create_resolve_function_direct(mc, promise, env);
    let reject = create_reject_function_direct(mc, promise, env);

    Ok((promise, resolve, reject))
}

pub fn call_function<'gc>(
    mc: &MutationContext<'gc>,
    func: &Value<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match func {
        Value::Closure(cl) => crate::core::call_closure(mc, cl, None, args, env, None),
        Value::Function(name) => {
            if let Some(res) = crate::core::call_native_function(mc, name, None, args, env)? {
                Ok(res)
            } else {
                crate::js_function::handle_global_function(mc, name, args, env)
            }
        }
        Value::Object(obj) => {
            if let Some(cl_ptr) = obj.borrow().get_closure()
                && let Value::Closure(cl) = &*cl_ptr.borrow()
            {
                return crate::core::call_closure(mc, cl, None, args, env, None);
            }
            Err(raise_type_error!("Not a function").into())
        }
        _ => Err(raise_type_error!("Not a function").into()),
    }
}

pub fn call_function_with_this<'gc>(
    mc: &MutationContext<'gc>,
    func: &Value<'gc>,
    this_val: Option<Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match func {
        Value::Closure(cl) => crate::core::call_closure(mc, cl, this_val, args, env, None),
        Value::Function(name) => {
            if let Some(res) = crate::core::call_native_function(mc, name, this_val.clone(), args, env)? {
                Ok(res)
            } else if let Some(this) = this_val {
                let call_env = crate::core::new_js_object_data(mc);
                // Use the existing env as the parent scope loop up
                call_env.borrow_mut(mc).prototype = Some(*env);
                call_env.borrow_mut(mc).is_function_scope = true;
                object_set_key_value(mc, &call_env, "this", this.clone())?;
                crate::js_function::handle_global_function(mc, name, args, &call_env)
            } else {
                crate::js_function::handle_global_function(mc, name, args, env)
            }
        }
        Value::Object(obj) => {
            if let Some(cl_ptr) = obj.borrow().get_closure()
                && let Value::Closure(cl) = &*cl_ptr.borrow()
            {
                return crate::core::call_closure(mc, cl, this_val, args, env, None);
            }
            Err(raise_type_error!("Not a function").into())
        }
        _ => Err(raise_type_error!("Not a function").into()),
    }
}

/// Walk up to the global environment object (top of prototype chain)
fn get_global_env<'gc>(_mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> JSObjectDataPtr<'gc> {
    // climb prototypes until none
    let mut global_env = *env;
    loop {
        let next = global_env.borrow().prototype;
        if let Some(parent) = next {
            global_env = parent;
        } else {
            break;
        }
    }
    global_env
}

/// Ensure a GC-rooted runtime JS object is stored on the global env under
/// the hidden property `__promise_runtime`. This object will hold arrays
/// for pending unhandled checks and allSettled state so they are arena-rooted.
fn ensure_promise_runtime<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let global = get_global_env(mc, env);
    if let Some(rc) = object_get_key_value(&global, "__promise_runtime")
        && let Value::Object(obj) = &*rc.borrow()
    {
        return Ok(*obj);
    }
    // create runtime object and set it
    let runtime = new_js_object_data(mc);
    object_set_key_value(mc, &global, "__promise_runtime", Value::Object(runtime))?;
    Ok(runtime)
}

/// Get (or create) a runtime array property on the runtime object
fn get_runtime_array<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, name: &str) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let runtime = ensure_promise_runtime(mc, env)?;
    if let Some(arr_rc) = object_get_key_value(&runtime, name)
        && let Value::Object(arr_obj) = &*arr_rc.borrow()
    {
        return Ok(*arr_obj);
    }
    // create array and set
    let arr = crate::js_array::create_array(mc, &runtime)?;
    object_set_key_value(mc, &runtime, name, Value::Object(arr))?;
    Ok(arr)
}

/// Push an entry into pending unhandled checks (stored as JS objects in runtime)
fn runtime_push_pending_unhandled<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    promise: GcPtr<'gc, JSPromise<'gc>>,
    reason: Value<'gc>,
    tick: usize,
) -> Result<(), JSError> {
    let arr = get_runtime_array(mc, env, "__pending_unhandled")?;
    let entry = new_js_object_data(mc);
    object_set_key_value(mc, &entry, "promise", Value::Promise(promise))?;
    object_set_key_value(mc, &entry, "reason", reason)?;
    object_set_key_value(mc, &entry, "tick", Value::Number(tick as f64))?;
    // Also store the env where this rejection was observed so later processing can re-create a proper UnhandledCheck task
    object_set_key_value(mc, &entry, "env", Value::Object(*env))?;

    // append to array
    let idx = crate::core::object_get_length(&arr).unwrap_or(0);
    object_set_key_value(mc, &arr, idx, Value::Object(entry))?;
    // Debug: log pending count to help diagnose why entries may never mature
    log::debug!(
        "runtime_push_pending_unhandled: added entry for promise ptr={:p} tick={} pending_count={}",
        Gc::as_ptr(promise),
        tick,
        pending_unhandled_count(mc, env)
    );
    Ok(())
}

/// Peek and take unhandled rejection stored on runtime (as stringified reason)
pub fn take_unhandled_rejection<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Option<Value<'gc>> {
    if let Ok(runtime) = ensure_promise_runtime(mc, env)
        && let Some(rc) = object_get_key_value(&runtime, "__unhandled_rejection")
        && let Value::String(s) = &*rc.borrow()
    {
        // consume it
        object_set_key_value(mc, &runtime, "__unhandled_rejection", Value::Undefined).unwrap();
        return Some(Value::String(s.clone()));
    }
    None
}

/// Clear any recorded runtime unhandled rejection if it was caused by `ptr`.
pub fn clear_runtime_unhandled_for_promise<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, ptr: usize) -> Result<(), JSError> {
    if let Ok(runtime) = ensure_promise_runtime(mc, env)
        && let Some(rc) = object_get_key_value(&runtime, "__unhandled_rejection_promise_ptr")
        && let Value::Number(n) = &*rc.borrow()
        && *n as usize == ptr
    {
        // Clear both the reason and the pointer
        object_set_key_value(mc, &runtime, "__unhandled_rejection", Value::Undefined)?;
        object_set_key_value(mc, &runtime, "__unhandled_rejection_promise_ptr", Value::Undefined)?;
    }
    Ok(())
}

/// Remove any pending __pending_unhandled entries for `ptr` from the runtime array
pub fn runtime_remove_pending_unhandled_for_promise<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    ptr: usize,
) -> Result<(), JSError> {
    if let Ok(runtime) = ensure_promise_runtime(mc, env)
        && let Some(rc) = object_get_key_value(&runtime, "__pending_unhandled")
        && let Value::Object(arr) = &*rc.borrow()
    {
        let len = crate::core::object_get_length(arr).unwrap_or(0);
        let new_arr = crate::js_array::create_array(mc, &runtime)?;
        let mut write_idx = 0usize;
        for i in 0..len {
            if let Some(entry_rc) = object_get_key_value(arr, i)
                && let Value::Object(entry) = &*entry_rc.borrow()
            {
                let mut keep = true;
                if let Some(p_rc) = object_get_key_value(entry, "promise")
                    && let Value::Promise(p) = &*p_rc.borrow()
                    && (Gc::as_ptr(*p) as usize) == ptr
                {
                    keep = false;
                }
                if keep {
                    object_set_key_value(mc, &new_arr, write_idx, Value::Object(*entry))?;
                    write_idx += 1;
                }
            }
        }
        object_set_key_value(mc, &runtime, "__pending_unhandled", Value::Object(new_arr))?;
    }
    Ok(())
}

/// Process pending runtime unhandled checks and enqueue UnhandledCheck tasks when their grace window elapsed.
/// Returns `Ok(true)` if at least one task was enqueued.
pub fn process_runtime_pending_unhandled<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, force: bool) -> Result<bool, JSError> {
    let mut queued_any = false;
    if let Ok(runtime) = ensure_promise_runtime(mc, env)
        && let Some(rc) = object_get_key_value(&runtime, "__pending_unhandled")
        && let Value::Object(arr) = &*rc.borrow()
    {
        let len = crate::core::object_get_length(arr).unwrap_or(0);
        let new_arr = crate::js_array::create_array(mc, &runtime)?;
        let mut write_idx = 0usize;
        for i in 0..len {
            if let Some(entry_rc) = object_get_key_value(arr, i)
                && let Value::Object(entry) = &*entry_rc.borrow()
            {
                // Read insertion tick
                let mut insertion_tick = 0usize;
                if let Some(t_rc) = object_get_key_value(entry, "tick")
                    && let Value::Number(n) = &*t_rc.borrow()
                {
                    insertion_tick = *n as usize;
                }
                let current = CURRENT_TICK.load(Ordering::SeqCst);
                if force || current >= insertion_tick + UNHANDLED_GRACE {
                    // Enqueue UnhandledCheck task for this entry
                    if let Some(p_rc) = object_get_key_value(entry, "promise") {
                        if let Value::Promise(promise_gc) = &*p_rc.borrow() {
                            let reason = if let Some(r_rc) = object_get_key_value(entry, "reason") {
                                r_rc.borrow().clone()
                            } else {
                                Value::Undefined
                            };
                            // env saved in entry
                            let entry_env = if let Some(e_rc) = object_get_key_value(entry, "env") {
                                if let Value::Object(o) = &*e_rc.borrow() { *o } else { *env }
                            } else {
                                *env
                            };

                            log::debug!(
                                "process_runtime_pending_unhandled: scheduling UnhandledCheck for promise ptr={:p} insertion_tick={} force={}",
                                Gc::as_ptr(*promise_gc),
                                insertion_tick,
                                force
                            );

                            queue_task(
                                mc,
                                Task::UnhandledCheck {
                                    promise: *promise_gc,
                                    reason,
                                    insertion_tick,
                                    env: entry_env,
                                    force,
                                },
                            );
                            queued_any = true;
                        } else {
                            // Not a Promise? keep it
                            object_set_key_value(mc, &new_arr, write_idx, Value::Object(*entry))?;
                            write_idx += 1;
                        }
                    } else {
                        // malformed entry: keep it
                        object_set_key_value(mc, &new_arr, write_idx, Value::Object(*entry))?;
                        write_idx += 1;
                    }
                } else {
                    // Not yet matured: keep it for later
                    object_set_key_value(mc, &new_arr, write_idx, Value::Object(*entry))?;
                    write_idx += 1;
                }
            }
        }
        object_set_key_value(mc, &runtime, "__pending_unhandled", Value::Object(new_arr))?;
        // Debug: report how many pending entries remain after processing
        log::debug!(
            "process_runtime_pending_unhandled: queued_any={} pending_count={}",
            queued_any,
            pending_unhandled_count(mc, env)
        );
    }
    Ok(queued_any)
}

/// Mark a promise as handled and clear any pending unhandled rejection checks.
pub fn mark_promise_handled<'gc>(
    mc: &MutationContext<'gc>,
    promise: GcPtr<'gc, JSPromise<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    let ptr = Gc::as_ptr(promise) as usize;
    promise.borrow_mut(mc).handled = true;
    remove_unhandled_checks_for_promise(ptr);
    clear_runtime_unhandled_for_promise(mc, env, ptr)?;
    // Also remove any pending runtime __pending_unhandled entries for this promise
    runtime_remove_pending_unhandled_for_promise(mc, env, ptr)?;
    Ok(())
}

pub fn pending_unhandled_count<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> usize {
    if let Ok(arr) = get_runtime_array(mc, env, "__pending_unhandled") {
        crate::core::object_get_length(&arr).unwrap_or(0)
    } else {
        0
    }
}

/// Configure whether `evaluate_script` should keep the event loop alive while
/// active timers/intervals exist. Default: false. Exposed via a public setter
/// so examples or tests can enable the Node-like behavior when appropriate.
pub fn set_wait_for_active_handles(enabled: bool) {
    WAIT_FOR_ACTIVE_HANDLES.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

pub fn wait_for_active_handles() -> bool {
    WAIT_FOR_ACTIVE_HANDLES.load(std::sync::atomic::Ordering::SeqCst)
}

/// Returns true if there are any active timers or intervals registered on this
/// thread's timer registry.
pub fn has_active_timers() -> bool {
    TIMER_REGISTRY.with(|reg| !reg.borrow().is_empty())
}

/// Peek the reason information for the first pending UnhandledCheck task, if any.
/// Returns (message, Option<(line, column)>) to avoid GC lifetime issues when reporting
/// an unhandled rejection back to the caller.
pub fn peek_pending_unhandled_info<'gc>(
    _mc: &MutationContext<'gc>,
    _env: &JSObjectDataPtr<'gc>,
) -> Option<(String, Option<(usize, usize)>)> {
    GLOBAL_TASK_QUEUE.with(|q| {
        let queue = q.borrow();
        for t in queue.iter() {
            if let Task::UnhandledCheck {
                promise,
                reason,
                insertion_tick,
                ..
            } = t
            {
                // Only report unhandled checks whose grace window has passed.
                // Within the grace window we *only* allow immediate reporting for
                // explicit Error-like objects (they carry line/column info and
                // should surface immediately). This avoids recording non-Error
                // rejections in the same tick where a handler may still be
                // attached synchronously.
                let current = CURRENT_TICK.load(std::sync::atomic::Ordering::SeqCst);
                if current < *insertion_tick + UNHANDLED_GRACE {
                    // Within grace window: allow only if the reason looks like an Error
                    let mut allow_immediate = false;
                    if let Value::Object(obj) = reason {
                        // __is_error flag or presence of __line__ indicates Error
                        if let Some(is_err_rc) = object_get_key_value(obj, "__is_error")
                            && let Value::Boolean(true) = &*is_err_rc.borrow()
                        {
                            allow_immediate = true;
                        }
                        if !allow_immediate && object_get_key_value(obj, "__line__").is_some() {
                            allow_immediate = true;
                        }
                    }
                    if !allow_immediate {
                        continue;
                    }
                }

                // Only consider it unhandled if the promise currently has no rejection handlers
                // and there are no queued Rejection tasks for this promise. It's possible
                // a handler attached synchronously to an already-rejected promise has
                // queued a Rejection task (which will execute asynchronously). In that
                // case, treat the promise as handled.
                // Also check the explicit `handled` flag.
                let promise_borrow = promise.borrow();
                if !promise_borrow.on_rejected.is_empty() || promise_borrow.handled {
                    continue;
                }

                // Check for queued Rejection tasks targeting the same promise
                let ptr = Gc::as_ptr(*promise) as usize;
                let mut has_queued_rejection = false;
                for t2 in queue.iter() {
                    if let Task::Rejection { promise: p2, .. } = t2
                        && (Gc::as_ptr(*p2) as usize) == ptr
                    {
                        has_queued_rejection = true;
                        break;
                    }
                }
                if has_queued_rejection {
                    continue;
                }

                // Extract string message and optional line/column from reason
                match reason {
                    Value::Object(obj) => {
                        // Try message
                        if let Some(msg_rc) = object_get_key_value(obj, "message")
                            && let Value::String(s_utf16) = &*msg_rc.borrow()
                        {
                            let msg = utf16_to_utf8(s_utf16);
                            // Try __line__ and __column__
                            let mut loc: Option<(usize, usize)> = None;
                            if let Some(line_rc) = object_get_key_value(obj, "__line__")
                                && let Value::Number(line_num) = &*line_rc.borrow()
                            {
                                let col = if let Some(col_rc) = object_get_key_value(obj, "__column__") {
                                    if let Value::Number(col_num) = &*col_rc.borrow() {
                                        *col_num as usize
                                    } else {
                                        0
                                    }
                                } else {
                                    0
                                };
                                loc = Some((*line_num as usize, col));
                            }
                            return Some((msg, loc));
                        }
                        // Fallback to value_to_string
                        return Some((crate::core::value_to_string(reason), None));
                    }
                    _ => return Some((crate::core::value_to_string(reason), None)),
                }
            }
        }
        None
    })
}

thread_local! {
    /// Global task queue for asynchronous promise operations.
    /// Uses thread-local storage to maintain separate queues per thread.
    /// This enables proper asynchronous execution of promise callbacks.
    static GLOBAL_TASK_QUEUE: std::cell::RefCell<Vec<Task<'static>>> = const { std::cell::RefCell::new(Vec::new()) };

    // Track consecutive Resolution/Rejection tasks to improve microtask fairness.
    static RESOLUTION_STREAK: AtomicUsize = const { AtomicUsize::new(0) };

    /// Counter for generating unique timeout IDs
    static NEXT_TIMEOUT_ID: std::cell::RefCell<usize> = const { std::cell::RefCell::new(1) };

    /// Registry of active timers for the current thread/arena.
    /// Stores (callback, args, optional interval) as 'static coerced values.
    #[allow(clippy::type_complexity)]
    static TIMER_REGISTRY: std::cell::RefCell<std::collections::HashMap<usize, (Value<'static>, Vec<Value<'static>>, Option<std::time::Duration>)>> = std::cell::RefCell::new(std::collections::HashMap::new());

    /// Parallel queue of task ids corresponding to entries in GLOBAL_TASK_QUEUE.
    /// This allows correlating dequeued tasks with their assigned compact id.
    static GLOBAL_TASK_ID_QUEUE: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

use crate::timer_thread::{TimerCommand, spawn_timer_thread};
use crossbeam_channel::{Receiver, Sender};
use std::sync::OnceLock;

struct TimerThreadHandle {
    cmd_tx: Sender<TimerCommand>,
    expired_rx: Receiver<usize>,
}

static TIMER_THREAD_HANDLE: OnceLock<TimerThreadHandle> = OnceLock::new();

fn ensure_timer_thread() -> &'static TimerThreadHandle {
    TIMER_THREAD_HANDLE.get_or_init(|| {
        let (cmd_tx, expired_rx) = spawn_timer_thread();
        TimerThreadHandle { cmd_tx, expired_rx }
    })
}

/// Reset global promise runtime state between arena runs (for test isolation).
/// This clears the global task queue, resets the monotonic tick, and
/// resets the timeout id counter so each JsArena starts with a clean state.
pub fn reset_global_state() {
    GLOBAL_TASK_QUEUE.with(|q| q.borrow_mut().clear());
    GLOBAL_TASK_ID_QUEUE.with(|ids| ids.borrow_mut().clear());
    CURRENT_TICK.store(0, Ordering::SeqCst);
    NEXT_TIMEOUT_ID.with(|id| *id.borrow_mut() = 1);
}

/// Tracks how many nested invocations of the promise event loop are active.
/// When >1 we are in a nested/inline run and should defer UnhandledCheck
/// processing to the outermost loop to avoid premature unhandled reports.
static RUN_LOOP_NESTING: AtomicUsize = AtomicUsize::new(0);

// If true, `evaluate_script` (CLI / examples) will keep the event loop alive
// while there are active timers/intervals registered. Defaults to false so
// tests don't block waiting for long-running handles.
static WAIT_FOR_ACTIVE_HANDLES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Monotonic tick counter advanced once per outermost idle event-loop tick.
/// Pending unhandled checks record the insertion tick and are considered
/// unhandled only when `CURRENT_TICK >= insertion_tick + UNHANDLED_GRACE`.
static CURRENT_TICK: AtomicUsize = AtomicUsize::new(0);

/// Monotonic task id counter for queued asynchronous tasks. This provides a
/// compact stable id to correlate queue/processing logs during debugging.
static TASK_COUNTER: AtomicUsize = AtomicUsize::new(1);

/// Number of outermost idle ticks to wait before treating a rejection as
/// unhandled. This provides a small grace window for handlers to attach.
/// Increased to give harnesses additional time to attach handlers in
/// high-latency or deeply-nested synchronous scenarios.
const UNHANDLED_GRACE: usize = 6;

use std::sync::atomic::AtomicU64;
use std::sync::{Condvar, Mutex};

/// Threshold (ms) under which timers are considered "short" and are
/// handled synchronously by `evaluate_script` to allow small test timers
/// to fire before returning. Default is 20 ms.
static SHORT_TIMER_WAIT_MS: AtomicU64 = AtomicU64::new(20);

/// Set the short-timer threshold (milliseconds). Public so examples can
/// configure runtime behavior via CLI flags.
pub fn set_short_timer_threshold_ms(ms: u64) {
    SHORT_TIMER_WAIT_MS.store(ms, Ordering::SeqCst);
}

/// Read the current short-timer threshold in milliseconds.
pub fn short_timer_threshold_ms() -> u64 {
    SHORT_TIMER_WAIT_MS.load(Ordering::SeqCst)
}

/// Event-loop wake primitive used by `evaluate_script` to wait for short timers
/// and to be notified when new tasks are queued. Lazily initialized on first use.
static EVENT_LOOP_WAKE: OnceLock<(Mutex<bool>, Condvar)> = OnceLock::new();

pub(crate) fn get_event_loop_wake() -> &'static (Mutex<bool>, Condvar) {
    EVENT_LOOP_WAKE.get_or_init(|| (Mutex::new(false), Condvar::new()))
}

/// Global task repetition counters to debug infinite loops
static TASK_REPETITION_COUNTERS: OnceLock<Mutex<std::collections::HashMap<String, usize>>> = OnceLock::new();

pub(crate) fn get_task_repetition_counters() -> &'static Mutex<std::collections::HashMap<String, usize>> {
    TASK_REPETITION_COUNTERS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// Add a task to the global task queue for later execution.
///
/// # Arguments
/// * `task` - The task to queue (Resolution or Rejection)
fn queue_task<'gc>(_mc: &MutationContext<'gc>, task: Task<'gc>) {
    // Assign a compact task id to help correlate enqueue -> process logs
    let task_id = TASK_COUNTER.fetch_add(1, Ordering::SeqCst);

    let task_key = match &task {
        Task::Resolution { promise, callbacks } => {
            format!("Resolution promise_ptr={:p} callbacks={}", Gc::as_ptr(*promise), callbacks.len())
        }
        Task::Rejection { promise, callbacks } => format!("Rejection promise_ptr={:p} callbacks={}", Gc::as_ptr(*promise), callbacks.len()),
        Task::ExecuteClosure { function, .. } => format!("ExecuteClosure function={:?}", function),
        Task::AttachHandlers { promise, .. } => format!("AttachHandlers promise_ptr={:p}", Gc::as_ptr(*promise)),
        Task::ResolvePromise { promise, .. } => format!("ResolvePromise promise_ptr={:p}", Gc::as_ptr(*promise)),
        Task::AsyncStep { generator, is_reject, .. } => {
            format!("AsyncStep gen_ptr={:p} is_reject={}", Gc::as_ptr(*generator), is_reject)
        }
        Task::Timeout { id, .. } => format!("Timeout id={}", id),
        Task::Interval { id, .. } => format!("Interval id={}", id),
        Task::UnhandledCheck {
            promise, insertion_tick, ..
        } => format!(
            "UnhandledCheck promise_ptr={:p} insertion_tick={}",
            Gc::as_ptr(*promise),
            insertion_tick
        ),
    };

    if let Ok(mut counters) = get_task_repetition_counters().lock() {
        *counters.entry(task_key.clone()).or_insert(0) += 1;
    }

    // Log a compact summary of the task being queued to aid tracing
    let task_summary = format!("id={} {}", task_id, task_key);
    log::debug!("queue_task: enqueuing task -> {}", task_summary);

    GLOBAL_TASK_QUEUE.with(|q| {
        q.borrow_mut().push({
            // We can transmute lifetime here because GLOBAL_TASK_QUEUE stores Task<'static>.
            // This is safe in the existing design where tasks are processed within the arena
            // lifetime used by mutation events. Use of Gc values across ticks should be
            // done carefully; for now we coerce the lifetime.
            let t = task;
            unsafe { std::mem::transmute::<Task<'gc>, Task<'static>>(t) }
        });
        // Maintain the parallel id queue so dequeues can be correlated with enqueue logs
        GLOBAL_TASK_ID_QUEUE.with(|ids| ids.borrow_mut().push(task_id));
        let len = q.borrow().len();
        log::debug!("queue_task: id={} queue_len after push = {}", task_id, len);
    });

    // Wake anyone waiting for short timers / new tasks so they can process immediately.
    let (lock, cv) = get_event_loop_wake();
    let mut guard = lock.lock().unwrap();
    *guard = true;
    cv.notify_all();
}

/// Add a task to the front of the global task queue for later execution.
///
/// This is used to ensure certain tasks (like initial async steps) run
/// before already-queued promise reactions.
fn queue_task_front<'gc>(_mc: &MutationContext<'gc>, task: Task<'gc>) {
    // Assign a compact task id to help correlate enqueue -> process logs
    let task_id = TASK_COUNTER.fetch_add(1, Ordering::SeqCst);

    let task_key = match &task {
        Task::Resolution { promise, callbacks } => {
            format!("Resolution promise_ptr={:p} callbacks={}", Gc::as_ptr(*promise), callbacks.len())
        }
        Task::Rejection { promise, callbacks } => {
            format!("Rejection promise_ptr={:p} callbacks={}", Gc::as_ptr(*promise), callbacks.len())
        }
        Task::ExecuteClosure { function, .. } => format!("ExecuteClosure function={:?}", function),
        Task::AttachHandlers { promise, .. } => format!("AttachHandlers promise_ptr={:p}", Gc::as_ptr(*promise)),
        Task::ResolvePromise { promise, .. } => format!("ResolvePromise promise_ptr={:p}", Gc::as_ptr(*promise)),
        Task::AsyncStep { generator, is_reject, .. } => {
            format!("AsyncStep gen_ptr={:p} is_reject={}", Gc::as_ptr(*generator), is_reject)
        }
        Task::Timeout { id, .. } => format!("Timeout id={}", id),
        Task::Interval { id, .. } => format!("Interval id={}", id),
        Task::UnhandledCheck {
            promise, insertion_tick, ..
        } => format!(
            "UnhandledCheck promise_ptr={:p} insertion_tick={}",
            Gc::as_ptr(*promise),
            insertion_tick
        ),
    };

    if let Ok(mut counters) = get_task_repetition_counters().lock() {
        *counters.entry(task_key.clone()).or_insert(0) += 1;
    }

    let task_summary = format!("id={} {}", task_id, task_key);
    log::debug!("queue_task_front: enqueuing task -> {}", task_summary);

    GLOBAL_TASK_QUEUE.with(|q| {
        q.borrow_mut().insert(0, {
            let t = task;
            unsafe { std::mem::transmute::<Task<'gc>, Task<'static>>(t) }
        });
        GLOBAL_TASK_ID_QUEUE.with(|ids| ids.borrow_mut().insert(0, task_id));
        let len = q.borrow().len();
        log::debug!("queue_task_front: id={} queue_len after insert = {}", task_id, len);
    });

    let (lock, cv) = get_event_loop_wake();
    let mut guard = lock.lock().unwrap();
    *guard = true;
    cv.notify_all();
}

pub fn queue_async_step<'gc>(
    mc: &MutationContext<'gc>,
    generator: GcPtr<'gc, JSGenerator<'gc>>,
    resolve: Value<'gc>,
    reject: Value<'gc>,
    result: Value<'gc>,
    is_reject: bool,
    env: &JSObjectDataPtr<'gc>,
) {
    queue_task_front(
        mc,
        Task::AsyncStep {
            generator,
            resolve,
            reject,
            result,
            is_reject,
            env: *env,
        },
    );
}

/// Remove any pending UnhandledCheck tasks for the given promise from the global queue.
/// This helps avoid the situation where an UnhandledCheck is queued before a handler
/// was attached and repeatedly re-queues itself, preventing the event loop from
/// advancing the tick.
fn remove_unhandled_checks_for_promise(ptr: usize) {
    GLOBAL_TASK_QUEUE.with(|q| {
        match q.try_borrow_mut() {
            Ok(mut borrow) => {
                let before = borrow.len();
                // Rebuild both the task queue and the parallel id queue to remove matches
                let orig = std::mem::take(&mut *borrow);
                let mut new_tasks: Vec<Task<'static>> = Vec::new();
                let mut new_ids: Vec<usize> = Vec::new();
                GLOBAL_TASK_ID_QUEUE.with(|ids| {
                    let id_borrow = ids.borrow();
                    for (i, task) in orig.into_iter().enumerate() {
                        let keep = match &task {
                            Task::UnhandledCheck { promise: p, .. } => (Gc::as_ptr(*p) as usize) != ptr,
                            _ => true,
                        };
                        if keep {
                            new_tasks.push(task);
                            new_ids.push(id_borrow[i]);
                        }
                    }
                });
                *borrow = new_tasks;
                GLOBAL_TASK_ID_QUEUE.with(|ids| *ids.borrow_mut() = new_ids);
                let after = borrow.len();
                log::trace!(
                    "remove_unhandled_checks_for_promise ptr={} removed={}",
                    ptr,
                    before.saturating_sub(after)
                );
            }
            Err(_) => {
                // If the queue is currently borrowed elsewhere, skip removal to avoid panic.
                // The handled flag on the promise will prevent incorrect reporting in most cases.
                log::trace!("remove_unhandled_checks_for_promise ptr={} skipped due to active borrow", ptr);
            }
        }
    });
}

/// Execute the event loop to process all queued asynchronous tasks.
///
/// This function simulates JavaScript's event loop for promises. It processes
/// tasks in FIFO order, executing promise callbacks asynchronously.
///
/// # Returns
/// Result of polling the event loop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PollResult {
    /// A task was executed.
    Executed,
    /// No tasks were ready, but there are pending timers.
    /// The caller should wait for the specified duration.
    Wait(Duration),
    /// The queue is empty and there are no pending timers.
    Empty,
}

/// Process a single task.
fn process_task<'gc>(mc: &MutationContext<'gc>, task_id: usize, task: Task<'gc>) -> Result<(), JSError> {
    // Short summary of task for tracing including the assigned task id
    let task_summary = match &task {
        Task::Resolution { promise, callbacks } => format!(
            "id={} Resolution promise_ptr={:p} callbacks={}",
            task_id,
            Gc::as_ptr(*promise),
            callbacks.len()
        ),
        Task::Rejection { promise, callbacks } => format!(
            "id={} Rejection promise_ptr={:p} callbacks={}",
            task_id,
            Gc::as_ptr(*promise),
            callbacks.len()
        ),
        Task::ExecuteClosure { function, .. } => format!("id={} ExecuteClosure function={:?}", task_id, function),
        Task::AttachHandlers { promise, .. } => format!("id={} AttachHandlers promise_ptr={:p}", task_id, Gc::as_ptr(*promise)),
        Task::ResolvePromise { promise, .. } => format!("id={} ResolvePromise promise_ptr={:p}", task_id, Gc::as_ptr(*promise)),
        Task::AsyncStep { generator, is_reject, .. } => {
            format!(
                "id={} AsyncStep gen_ptr={:p} is_reject={}",
                task_id,
                Gc::as_ptr(*generator),
                is_reject
            )
        }
        Task::Timeout { id, .. } => format!("id={} Timeout id={}", task_id, id),
        Task::Interval { id, .. } => format!("id={} Interval id={}", task_id, id),
        Task::UnhandledCheck {
            promise,
            insertion_tick,
            force,
            ..
        } => format!(
            "id={} UnhandledCheck promise_ptr={:p} insertion_tick={} force={}",
            task_id,
            Gc::as_ptr(*promise),
            insertion_tick,
            force
        ),
    };
    log::debug!("process_task: executing task -> {}", task_summary);

    let is_resolution_task = matches!(task, Task::Resolution { .. } | Task::Rejection { .. } | Task::AsyncStep { .. });

    match task {
        Task::Resolution { promise, callbacks } => {
            log::trace!("Processing Resolution task with {} callbacks", callbacks.len());
            let p_val = promise.borrow().value.clone(); // unwrap_or(Value::Undefined);
            log::trace!("process_task Resolution. p_val={:?}", p_val);

            for (callback, new_promise, caller_env_opt) in callbacks {
                log::trace!(
                    "process_task invoking callback for new_promise ptr={:p} callback={:?} caller_env={:p}",
                    Gc::as_ptr(new_promise),
                    callback,
                    caller_env_opt.as_ref().map(|e| e as *const _).unwrap_or(std::ptr::null())
                );
                let args = vec![promise.borrow().value.clone().unwrap_or(Value::Undefined)];

                let mut handled = false;
                match &callback {
                    Value::Closure(cl) => {
                        let tmp_env = new_js_object_data(mc);
                        let call_env = caller_env_opt.as_ref().or(cl.env.as_ref()).unwrap_or(&tmp_env);
                        handled = true;
                        match crate::core::call_closure(mc, cl, None, &args, call_env, None) {
                            Ok(result) => {
                                resolve_promise(mc, &new_promise, result, call_env);
                            }
                            Err(e) => {
                                if let crate::core::EvalError::Throw(value, ..) = e {
                                    reject_promise(mc, &new_promise, value.clone(), call_env);
                                } else {
                                    reject_promise(mc, &new_promise, Value::String(utf8_to_utf16(&format!("{:?}", e))), call_env);
                                }
                            }
                        }
                    }
                    Value::AsyncClosure(cl) => {
                        let tmp_env = new_js_object_data(mc);
                        let call_env = caller_env_opt.as_ref().or(cl.env.as_ref()).unwrap_or(&tmp_env);
                        handled = true;
                        match crate::js_async::handle_async_closure_call(mc, cl, None, &args, call_env, None) {
                            Ok(result) => resolve_promise(mc, &new_promise, result, call_env),
                            Err(e) => reject_promise(mc, &new_promise, Value::String(utf8_to_utf16(&e.message())), call_env),
                        }
                    }
                    Value::Object(obj) => {
                        if let Some(cl_ptr) = obj.borrow().get_closure() {
                            match &*cl_ptr.borrow() {
                                Value::Closure(cl) => {
                                    let tmp_env = new_js_object_data(mc);
                                    let call_env = caller_env_opt.as_ref().or(cl.env.as_ref()).unwrap_or(&tmp_env);
                                    handled = true;
                                    match crate::core::call_closure(mc, cl, None, &args, call_env, Some(*obj)) {
                                        Ok(result) => resolve_promise(mc, &new_promise, result, call_env),
                                        Err(e) => {
                                            if let crate::core::EvalError::Throw(value, ..) = e {
                                                reject_promise(mc, &new_promise, value.clone(), call_env);
                                            } else {
                                                reject_promise(mc, &new_promise, Value::String(utf8_to_utf16(&format!("{e:?}"))), call_env);
                                            }
                                        }
                                    }
                                }
                                Value::AsyncClosure(cl) => {
                                    let tmp_env = new_js_object_data(mc);
                                    let call_env = caller_env_opt.as_ref().or(cl.env.as_ref()).unwrap_or(&tmp_env);
                                    handled = true;
                                    match crate::js_async::handle_async_closure_call(mc, cl, None, &args, call_env, Some(*obj)) {
                                        Ok(result) => resolve_promise(mc, &new_promise, result, call_env),
                                        Err(e) => reject_promise(mc, &new_promise, Value::String(utf8_to_utf16(&e.message())), call_env),
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }

                if handled {
                    continue;
                }

                // Call the callback and resolve the new promise with the result
                if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                    // Debug: show callback param count and captured_env pointer for diagnosis
                    log::trace!(
                        "[promise] invoking callback - params_len={} captured_env_ptr={:p}",
                        params.len(),
                        Gc::as_ptr(captured_env)
                    );
                    log::trace!("callback args={:?}", args);
                    let func_env = prepare_closure_call_env(mc, Some(&captured_env), Some(&params[..]), &args, caller_env_opt.as_ref())?;
                    match evaluate_statements(mc, &func_env, &body) {
                        Ok(result) => {
                            log::trace!(
                                "callback result={:?} -> resolving new_promise ptr={:p}",
                                result,
                                Gc::as_ptr(new_promise)
                            );
                            log::trace!("Callback executed successfully, resolving promise");
                            resolve_promise(mc, &new_promise, result, &func_env);
                        }
                        Err(e) => {
                            log::trace!("Callback execution failed: {:?}", e);
                            if let crate::core::EvalError::Throw(value, ..) = e {
                                reject_promise(mc, &new_promise, value.clone(), &func_env);
                            } else {
                                reject_promise(mc, &new_promise, Value::String(utf8_to_utf16(&format!("{:?}", e))), &func_env);
                            }
                        }
                    }
                } else {
                    // If callback is a native function (Value::Function) or an object with
                    // a callable closure, attempt to call it. Otherwise forward the value.
                    let original_val = promise.borrow().value.clone().unwrap_or(Value::Undefined);

                    // Determine whether we should forward (i.e. callback is not callable)
                    let should_forward = match &callback {
                        Value::Undefined => true,
                        Value::Function(_) => false,
                        Value::Object(obj) => obj.borrow().get_closure().is_none(),
                        _ => true,
                    };

                    if should_forward {
                        // Forward the original value to the chained promise
                        if let Some(env) = caller_env_opt.as_ref() {
                            resolve_promise(mc, &new_promise, original_val, env);
                        } else {
                            // Fallback env in the unlikely case none was provided
                            let tmp_env = new_js_object_data(mc);
                            resolve_promise(mc, &new_promise, original_val, &tmp_env);
                        }
                    } else {
                        // Callback looks callable — attempt to call it using the provided env
                        if let Some(env) = caller_env_opt.as_ref() {
                            match crate::js_promise::call_function(mc, &callback, std::slice::from_ref(&original_val), env) {
                                Ok(res) => {
                                    resolve_promise(mc, &new_promise, res, env);
                                }
                                Err(e) => {
                                    log::trace!("Callback execution failed: {:?}", e);
                                    reject_promise(mc, &new_promise, Value::String(utf8_to_utf16(&e.message())), env);
                                }
                            }
                        } else {
                            // No caller env — create a temporary env and try
                            let tmp_env = new_js_object_data(mc);
                            match crate::js_promise::call_function(mc, &callback, std::slice::from_ref(&original_val), &tmp_env) {
                                Ok(res) => {
                                    resolve_promise(mc, &new_promise, res, &tmp_env);
                                }
                                Err(e) => {
                                    log::trace!("Callback execution failed: {:?}", e);
                                    reject_promise(mc, &new_promise, Value::String(utf8_to_utf16(&e.message())), &tmp_env);
                                }
                            }
                        }
                    }
                }
            }
        }
        Task::Rejection { promise, callbacks } => {
            log::trace!("Processing Rejection task with {} callbacks", callbacks.len());
            // Ensure any pending UnhandledCheck entries or runtime-recorded unhandled
            // are cleared for this promise now that a handler is being executed.
            let ptr = Gc::as_ptr(promise) as usize;
            remove_unhandled_checks_for_promise(ptr);
            // Mark the promise as handled so subsequent scans skip it
            promise.borrow_mut(mc).handled = true;
            // Try to clear the runtime recorded unhandled using any available env from callbacks
            for (_cb, _np, caller_env_opt) in callbacks.iter() {
                if let Some(env) = caller_env_opt.as_ref() {
                    clear_runtime_unhandled_for_promise(mc, env, ptr)?;
                    break;
                }
            }

            for (callback, new_promise, caller_env_opt) in callbacks {
                let args = vec![promise.borrow().value.clone().unwrap_or(Value::Undefined)];

                let mut handled = false;
                match &callback {
                    Value::Closure(cl) => {
                        let tmp_env = new_js_object_data(mc);
                        let call_env = caller_env_opt.as_ref().or(cl.env.as_ref()).unwrap_or(&tmp_env);
                        handled = true;
                        match crate::core::call_closure(mc, cl, None, &args, call_env, None) {
                            Ok(result) => resolve_promise(mc, &new_promise, result, call_env),
                            Err(e) => {
                                if let crate::core::EvalError::Throw(value, ..) = e {
                                    reject_promise(mc, &new_promise, value.clone(), call_env);
                                } else {
                                    reject_promise(mc, &new_promise, Value::String(utf8_to_utf16(&format!("{:?}", e))), call_env);
                                }
                            }
                        }
                    }
                    Value::AsyncClosure(cl) => {
                        let tmp_env = new_js_object_data(mc);
                        let call_env = caller_env_opt.as_ref().or(cl.env.as_ref()).unwrap_or(&tmp_env);
                        handled = true;
                        match crate::js_async::handle_async_closure_call(mc, cl, None, &args, call_env, None) {
                            Ok(result) => resolve_promise(mc, &new_promise, result, call_env),
                            Err(e) => reject_promise(mc, &new_promise, Value::String(utf8_to_utf16(&e.message())), call_env),
                        }
                    }
                    Value::Object(obj) => {
                        if let Some(cl_ptr) = obj.borrow().get_closure() {
                            match &*cl_ptr.borrow() {
                                Value::Closure(cl) => {
                                    let tmp_env = new_js_object_data(mc);
                                    let call_env = caller_env_opt.as_ref().or(cl.env.as_ref()).unwrap_or(&tmp_env);
                                    handled = true;
                                    match crate::core::call_closure(mc, cl, None, &args, call_env, Some(*obj)) {
                                        Ok(result) => resolve_promise(mc, &new_promise, result, call_env),
                                        Err(e) => {
                                            if let crate::core::EvalError::Throw(value, ..) = e {
                                                reject_promise(mc, &new_promise, value.clone(), call_env);
                                            } else {
                                                reject_promise(
                                                    mc,
                                                    &new_promise,
                                                    Value::String(utf8_to_utf16(&format!("{:?}", e))),
                                                    call_env,
                                                );
                                            }
                                        }
                                    }
                                }
                                Value::AsyncClosure(cl) => {
                                    let tmp_env = new_js_object_data(mc);
                                    let call_env = caller_env_opt.as_ref().or(cl.env.as_ref()).unwrap_or(&tmp_env);
                                    handled = true;
                                    match crate::js_async::handle_async_closure_call(mc, cl, None, &args, call_env, Some(*obj)) {
                                        Ok(result) => resolve_promise(mc, &new_promise, result, call_env),
                                        Err(e) => reject_promise(mc, &new_promise, Value::String(utf8_to_utf16(&e.message())), call_env),
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }

                if handled {
                    continue;
                }

                // Call the callback and resolve the new promise with the result
                if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                    let func_env = prepare_closure_call_env(mc, Some(&captured_env), Some(&params[..]), &args, caller_env_opt.as_ref())?;
                    match evaluate_statements(mc, &func_env, &body) {
                        Ok(result) => {
                            resolve_promise(mc, &new_promise, result, &func_env);
                        }
                        Err(e) => {
                            if let crate::core::EvalError::Throw(value, ..) = e {
                                reject_promise(mc, &new_promise, value.clone(), &func_env);
                            } else {
                                reject_promise(mc, &new_promise, Value::String(utf8_to_utf16(&format!("{:?}", e))), &func_env);
                            }
                        }
                    }
                } else {
                    // If callback is a native function or Function object, call it; otherwise forward the rejection
                    let original_reason = promise.borrow().value.clone().unwrap_or(Value::Undefined);

                    // Determine whether we should forward the rejection (callback not callable)
                    let should_forward = match &callback {
                        Value::Undefined => true,
                        Value::Function(_) => false,
                        Value::Object(obj) => obj.borrow().get_closure().is_none(),
                        _ => true,
                    };

                    if should_forward {
                        if let Some(env) = caller_env_opt.as_ref() {
                            reject_promise(mc, &new_promise, original_reason, env);
                        } else {
                            let tmp_env = new_js_object_data(mc);
                            reject_promise(mc, &new_promise, original_reason, &tmp_env);
                        }
                    } else if let Some(env) = caller_env_opt.as_ref() {
                        match crate::js_promise::call_function(mc, &callback, std::slice::from_ref(&original_reason), env) {
                            Ok(res) => {
                                resolve_promise(mc, &new_promise, res, env);
                            }
                            Err(e) => {
                                log::trace!("Callback execution failed: {:?}", e);
                                reject_promise(mc, &new_promise, Value::String(utf8_to_utf16(&e.message())), env);
                            }
                        }
                    } else {
                        let tmp_env = new_js_object_data(mc);
                        match crate::js_promise::call_function(mc, &callback, std::slice::from_ref(&original_reason), &tmp_env) {
                            Ok(res) => {
                                resolve_promise(mc, &new_promise, res, &tmp_env);
                            }
                            Err(e) => {
                                log::trace!("Callback execution failed: {:?}", e);
                                reject_promise(mc, &new_promise, Value::String(utf8_to_utf16(&e.message())), &tmp_env);
                            }
                        }
                    }
                }
            }
        }
        Task::ExecuteClosure {
            function,
            args,
            resolve,
            reject,
            this_val,
            env,
        } => {
            log::trace!("Processing ExecuteClosure task");
            // Call the closure/function
            let result = match &function {
                Value::Closure(cl) => crate::core::call_closure(mc, cl, this_val, &args, &env, None),
                Value::Function(_) => call_function_with_this(mc, &function, this_val, &args, &env),
                _ => call_function_with_this(mc, &function, this_val, &args, &env),
            };

            match result {
                Ok(val) => {
                    let _ = call_function(mc, &resolve, &[val], &env);
                }
                Err(e) => {
                    // DEBUG: print exception info to help trace unhandled rejections
                    match &e {
                        EvalError::Throw(v, ..) => {
                            log::debug!("ExecuteClosure threw (Throw): {:?}", v);
                        }
                        EvalError::Js(j) => {
                            log::debug!("ExecuteClosure threw (Js): {}", j.message());
                        }
                    }
                    let val = match e {
                        EvalError::Throw(v, ..) => v,
                        EvalError::Js(j) => Value::String(utf8_to_utf16(&j.message())),
                    };
                    let _ = call_function(mc, &reject, &[val], &env);
                }
            }
        }
        Task::ResolvePromise { promise, value, env } => {
            log::trace!("Processing ResolvePromise task");
            resolve_promise(mc, &promise, value, &env);
        }
        Task::AsyncStep {
            generator,
            resolve,
            reject,
            result,
            is_reject,
            env,
        } => {
            let step_result = if is_reject { Err(result) } else { Ok(result) };
            crate::js_async::continue_async_step_direct(mc, generator, resolve, reject, step_result, &env)?;
        }
        Task::Timeout { id, callback, args, .. } => {
            log::trace!("Processing Timeout task");
            // Call the callback with the provided args
            if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                // Distinguish arrow vs normal functions so `this` semantics match Node:
                // - Arrow functions inherit lexical `this` from creation time (use closure semantics)
                // - Non-arrow functions should be called with the global object as `this` when
                //   invoked by timers (i.e., plain function call semantics)
                let mut is_arrow = false;
                match &callback {
                    Value::Closure(cl) => is_arrow = cl.is_arrow,
                    Value::AsyncClosure(cl) => is_arrow = cl.is_arrow,
                    Value::Object(obj) => {
                        if let Some(closure_prop) = obj.borrow().get_closure() {
                            match &*closure_prop.borrow() {
                                Value::Closure(c) => is_arrow = c.is_arrow,
                                Value::AsyncClosure(c) => is_arrow = c.is_arrow,
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }

                if is_arrow {
                    // Arrow functions: use closure semantics so bound_this is respected
                    let func_env = prepare_closure_call_env(mc, Some(&captured_env), Some(&params[..]), &args, None)?;
                    let _ = evaluate_statements(mc, &func_env, &body)?;
                } else {
                    // Non-arrow function: follow strict-mode semantics when applicable.
                    // Our runtime is strict-only, so the `this` value for a plain function
                    // call should be `undefined` (not the global object).
                    let this_val = Some(Value::Undefined);
                    let func_env = prepare_function_call_env(mc, Some(&captured_env), this_val, Some(&params[..]), &args, None, None)?;
                    let _ = evaluate_statements(mc, &func_env, &body)?;
                }
            }

            // One-shot timeouts should be removed from the registry so that any
            // late expired notifications from the timer thread are ignored.
            TIMER_REGISTRY.with(|reg| {
                reg.borrow_mut().remove(&id);
            });
        }
        Task::Interval {
            id,
            callback,
            args,
            interval,
            ..
        } => {
            log::trace!("Processing Interval task");
            // Call the callback with the provided args
            if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                let _this_val_opt = if let Value::Object(_) = callback {
                    let mut global_env = captured_env;
                    loop {
                        let next = global_env.borrow().prototype;
                        if let Some(parent) = next {
                            global_env = parent;
                        } else {
                            break;
                        }
                    }
                    Some(Value::Object(global_env))
                } else {
                    None
                };

                // Distinguish arrow vs normal functions like above
                let mut is_arrow = false;
                match &callback {
                    Value::Closure(cl) => is_arrow = cl.is_arrow,
                    Value::AsyncClosure(cl) => is_arrow = cl.is_arrow,
                    Value::Object(obj) => {
                        if let Some(closure_prop) = obj.borrow().get_closure() {
                            match &*closure_prop.borrow() {
                                Value::Closure(c) => is_arrow = c.is_arrow,
                                Value::AsyncClosure(c) => is_arrow = c.is_arrow,
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }

                if is_arrow {
                    let func_env = prepare_closure_call_env(mc, Some(&captured_env), Some(&params[..]), &args, None)?;
                    let _ = evaluate_statements(mc, &func_env, &body)?;
                } else {
                    // Strict-mode: use undefined as `this` for plain function calls
                    let this_val = Some(Value::Undefined);
                    let func_env = prepare_function_call_env(mc, Some(&captured_env), this_val, Some(&params[..]), &args, None, None)?;
                    let _ = evaluate_statements(mc, &func_env, &body)?;
                }

                // Re-schedule the next interval tick
                queue_task(
                    mc,
                    Task::Interval {
                        id,
                        callback: callback.clone(),
                        args: args.clone(),
                        target_time: Instant::now() + interval,
                        interval,
                    },
                );
            }
        }
        Task::UnhandledCheck {
            promise,
            reason,
            insertion_tick,
            env,
            force,
        } => {
            log::trace!(
                "Processing UnhandledCheck task for promise ptr={:p} insertion_tick={} force={}",
                Gc::as_ptr(promise),
                insertion_tick,
                force
            );
            // Check if the promise still has no rejection handlers
            let promise_borrow = promise.borrow();
            if promise_borrow.on_rejected.is_empty() && !promise_borrow.handled {
                // If the grace window has passed, record as unhandled
                let current = CURRENT_TICK.load(Ordering::SeqCst);
                log::trace!(
                    "UnhandledCheck: current_tick={} insertion_tick={} grace={} force={} on_rejected_empty=true",
                    current,
                    insertion_tick,
                    UNHANDLED_GRACE,
                    force
                );
                if force || current >= insertion_tick + UNHANDLED_GRACE {
                    log::debug!(
                        "UnhandledCheck: grace elapsed (or forced), recording unhandled rejection for promise ptr={:p}",
                        Gc::as_ptr(promise)
                    );
                    // Store the stringified reason into runtime property for later pick-up
                    let s = utf8_to_utf16(&value_to_string(&reason));
                    if let Ok(runtime) = ensure_promise_runtime(mc, &env) {
                        object_set_key_value(mc, &runtime, "__unhandled_rejection", Value::String(s))?;
                        // Record which promise ptr caused this so it can be cleared if a handler attaches later
                        let ptr_num = (Gc::as_ptr(promise) as usize) as f64;
                        object_set_key_value(mc, &runtime, "__unhandled_rejection_promise_ptr", Value::Number(ptr_num))?;
                    }
                } else {
                    // Not yet elapsed: defer to the runtime pending list to avoid tight requeue loops
                    log::trace!(
                        "UnhandledCheck: grace not elapsed, deferring to runtime pending for promise ptr={:p}",
                        Gc::as_ptr(promise)
                    );
                    // Store in runtime pending list rather than re-queueing immediately
                    runtime_push_pending_unhandled(mc, &env, promise, reason.clone(), insertion_tick)?;
                }
            } else {
                log::trace!(
                    "UnhandledCheck: handlers attached, skipping unhandled recording for promise ptr={:p}",
                    Gc::as_ptr(promise)
                );
            }
        }
        Task::AttachHandlers {
            promise,
            on_fulfilled,
            on_rejected,
            result_promise,
            env,
        } => {
            log::trace!("AttachHandlers task executed for promise={:p}", Gc::as_ptr(promise));
            // Safe to borrow mutably here because we are running in the event loop
            let mut state = promise.borrow_mut(mc);
            let rp = result_promise.unwrap_or_else(|| new_gc_cell_ptr(mc, JSPromise::new()));
            match state.state {
                PromiseState::Pending => {
                    let before_len = state.on_rejected.len();
                    state.on_fulfilled.push((on_fulfilled.unwrap_or(Value::Undefined), rp, Some(env)));
                    state.on_rejected.push((on_rejected.unwrap_or(Value::Undefined), rp, Some(env)));
                    let after_len = state.on_rejected.len();
                    if before_len == 0 && after_len > 0 {
                        state.handled = true;
                        let ptr = Gc::as_ptr(promise) as usize;
                        drop(state);
                        remove_unhandled_checks_for_promise(ptr);
                        clear_runtime_unhandled_for_promise(mc, &env, ptr)?;
                    }
                }
                PromiseState::Fulfilled(_) => {
                    drop(state);
                    queue_task(
                        mc,
                        Task::Resolution {
                            promise,
                            callbacks: vec![(on_fulfilled.unwrap_or(Value::Undefined), rp, Some(env))],
                        },
                    );
                }
                PromiseState::Rejected(_) => {
                    state.handled = true;
                    let ptr = Gc::as_ptr(promise) as usize;
                    drop(state);
                    remove_unhandled_checks_for_promise(ptr);
                    clear_runtime_unhandled_for_promise(mc, &env, ptr)?;
                    queue_task(
                        mc,
                        Task::Rejection {
                            promise,
                            callbacks: vec![(on_rejected.unwrap_or(Value::Undefined), rp, Some(env))],
                        },
                    );
                }
            }
        }
    }

    if is_resolution_task {
        RESOLUTION_STREAK.with(|streak| {
            streak.fetch_add(1, Ordering::SeqCst);
        });
    } else {
        RESOLUTION_STREAK.with(|streak| {
            streak.store(0, Ordering::SeqCst);
        });
    }
    Ok(())
}

/// Poll the event loop for a single task.
///
/// This function checks the task queue and executes the first ready task.
/// If no tasks are ready but timers are pending, it returns `PollResult::Wait`.
/// If the queue is empty, it returns `PollResult::Empty`.
pub fn poll_event_loop<'gc>(mc: &MutationContext<'gc>) -> Result<PollResult, JSError> {
    // Debug: print top 5 repeated tasks every 100 tasks
    let current_task_count = TASK_COUNTER.load(Ordering::SeqCst);
    if current_task_count % 100 < 20 {
        // Print strictly periodically (approx)
        if let Ok(counters) = get_task_repetition_counters().lock() {
            // Let's print even if empty to confirm this runs
            if !counters.is_empty() {
                let mut sorted: Vec<_> = counters.iter().collect();
                sorted.sort_by_key(|a| std::cmp::Reverse(a.1));
                let top_5: Vec<_> = sorted.into_iter().take(5).collect();
                log::warn!("Top 5 repeated tasks (total queued={}): {:?}", current_task_count, top_5);
            } else {
                log::warn!("No tasks in repetition counters yet (total queued={})", current_task_count);
            }
        }
    }

    let now = Instant::now();

    // Drain any expired timer notifications from the timer thread and enqueue
    // corresponding tasks on the main event loop. This converts cross-thread
    // timer expirations (ids) back into `Task::Timeout` or `Task::Interval`
    // so that callbacks run on the main thread where GC-managed Values are
    // valid.
    if let Some(handle) = TIMER_THREAD_HANDLE.get() {
        // Try to receive expired ids without blocking; process all available.
        while let Ok(id) = handle.expired_rx.try_recv() {
            TIMER_REGISTRY.with(|reg| {
                let reg_borrow = reg.borrow_mut();
                if let Some((cb_static, args_static, interval_opt)) = reg_borrow.get(&id).cloned() {
                    // Convert stored 'static values back into the current arena lifetime
                    let cb_gc: Value<'gc> = unsafe { std::mem::transmute::<Value<'static>, Value<'gc>>(cb_static) };
                    let args_gc: Vec<Value<'gc>> = args_static
                        .into_iter()
                        .map(|a| unsafe { std::mem::transmute::<Value<'static>, Value<'gc>>(a) })
                        .collect();

                    // Remove any placeholder task for this id so we don't double-enqueue
                    GLOBAL_TASK_QUEUE.with(|queue| {
                        let mut queue_borrow = queue.borrow_mut();
                        // Rebuild tasks and ids to remove any Timeout/Interval tasks with matching id
                        let orig = std::mem::take(&mut *queue_borrow);
                        let mut new_tasks: Vec<Task<'static>> = Vec::new();
                        let mut new_ids: Vec<usize> = Vec::new();
                        GLOBAL_TASK_ID_QUEUE.with(|ids| {
                            let id_borrow = ids.borrow();
                            for (i, task) in orig.into_iter().enumerate() {
                                let keep = !matches!(&task, Task::Timeout { id: task_id, .. } if *task_id == id)
                                    && !matches!(&task, Task::Interval { id: task_id, .. } if *task_id == id);
                                if keep {
                                    new_tasks.push(task);
                                    new_ids.push(id_borrow[i]);
                                }
                            }
                        });
                        *queue_borrow = new_tasks;
                        GLOBAL_TASK_ID_QUEUE.with(|ids| *ids.borrow_mut() = new_ids);
                    });

                    if let Some(interval) = interval_opt {
                        queue_task(
                            mc,
                            Task::Interval {
                                id,
                                callback: cb_gc,
                                args: args_gc,
                                target_time: now,
                                interval,
                            },
                        );
                        // Reschedule the interval occurrence with the timer thread
                        let _ = handle.cmd_tx.send(TimerCommand::Schedule {
                            id,
                            when: Instant::now() + interval,
                        });
                    } else {
                        queue_task(
                            mc,
                            Task::Timeout {
                                id,
                                callback: cb_gc,
                                args: args_gc,
                                target_time: now,
                            },
                        );
                    }
                }
            });
        }
    }

    // Debug: print queue summary to help diagnose hanging loops
    GLOBAL_TASK_QUEUE.with(|queue| {
        let q = queue.borrow();
        if !q.is_empty() {
            let mut counts = std::collections::HashMap::new();
            for t in q.iter() {
                let k = match t {
                    Task::Resolution { .. } => "Resolution",
                    Task::Rejection { .. } => "Rejection",
                    Task::ExecuteClosure { .. } => "ExecuteClosure",
                    Task::Timeout { .. } => "Timeout",
                    Task::Interval { .. } => "Interval",
                    Task::UnhandledCheck { .. } => "UnhandledCheck",
                    Task::AttachHandlers { .. } => "AttachHandlers",
                    Task::ResolvePromise { .. } => "ResolvePromise",
                    Task::AsyncStep { .. } => "AsyncStep",
                };
                *counts.entry(k).or_insert(0usize) += 1;
            }
            log::trace!("poll_event_loop queue_len={} counts={:?}", q.len(), counts);
        }
    });

    // Select the next task to process and also return its assigned task id so we can correlate logs.
    let (task_with_id, should_sleep) = GLOBAL_TASK_QUEUE.with(|queue| {
        let mut queue_borrow = queue.borrow_mut();
        if queue_borrow.is_empty() {
            return (None, None);
        }

        let mut min_wait_time: Option<Duration> = None;
        let mut ready_index: Option<usize> = None;
        let mut first_resolution: Option<usize> = None;
        let mut first_async_step: Option<usize> = None;
        let mut first_resolve_promise: Option<usize> = None;

        for (i, task) in queue_borrow.iter().enumerate() {
            match task {
                Task::Resolution { .. } | Task::Rejection { .. } => {
                    if first_resolution.is_none() {
                        first_resolution = Some(i);
                    }
                }
                Task::AsyncStep { .. } => {
                    if first_async_step.is_none() {
                        first_async_step = Some(i);
                    }
                }
                Task::ResolvePromise { .. } => {
                    if first_resolve_promise.is_none() {
                        first_resolve_promise = Some(i);
                    }
                }
                Task::Timeout { target_time, .. } | Task::Interval { target_time, .. } => {
                    if *target_time <= now {
                        if ready_index.is_none() {
                            ready_index = Some(i);
                        }
                    } else {
                        let wait = *target_time - now;
                        min_wait_time = Some(min_wait_time.map_or(wait, |m| m.min(wait)));
                    }
                }
                Task::UnhandledCheck { insertion_tick, force, .. } => {
                    let current = CURRENT_TICK.load(Ordering::SeqCst);
                    if *force || current >= *insertion_tick + UNHANDLED_GRACE && ready_index.is_none() {
                        ready_index = Some(i);
                    }
                }
                _ => {
                    if ready_index.is_none() {
                        ready_index = Some(i);
                    }
                }
            }
        }

        if let Some(async_idx) = first_async_step {
            ready_index = Some(async_idx);
        } else if let Some(resolve_idx) = first_resolve_promise
            && RESOLUTION_STREAK.with(|streak| streak.load(Ordering::SeqCst)) >= 2
        {
            ready_index = Some(resolve_idx);
        } else if let Some(res_idx) = first_resolution {
            ready_index = Some(res_idx);
        } else if let Some(resolve_idx) = first_resolve_promise {
            ready_index = Some(resolve_idx);
        }

        if let Some(index) = ready_index {
            let t = queue_borrow.remove(index);
            let id = GLOBAL_TASK_ID_QUEUE.with(|ids| ids.borrow_mut().remove(index));
            let t_gc: Task<'gc> = unsafe { std::mem::transmute(t) };
            (Some((id, t_gc)), None)
        } else {
            (None, min_wait_time)
        }
    });

    if let Some((task_id, task)) = task_with_id {
        process_task(mc, task_id, task)?;
        let q_len = GLOBAL_TASK_QUEUE.with(|q| q.borrow().len());
        log::trace!("poll_event_loop: processed a task id={} ; queue_len={}", task_id, q_len);
        Ok(PollResult::Executed)
    } else if let Some(wait) = should_sleep {
        Ok(PollResult::Wait(wait))
    } else {
        Ok(PollResult::Empty)
    }
}

/// Execute the event loop to process all queued asynchronous tasks.
///
/// This function simulates JavaScript's event loop for promises. It processes
/// tasks in FIFO order, executing promise callbacks asynchronously.
///
/// # Returns
/// * `Result<PollResult, JSError>` - The result of the poll operation
pub fn run_event_loop<'gc>(mc: &MutationContext<'gc>) -> Result<PollResult, JSError> {
    log::trace!("run_event_loop called");
    // Mark that we're entering an event-loop run (may be nested).
    let nesting_before = RUN_LOOP_NESTING.fetch_add(1, Ordering::SeqCst);
    log::debug!(
        "run_event_loop: incremented RUN_LOOP_NESTING from {} to {}",
        nesting_before,
        nesting_before + 1
    );

    let result = poll_event_loop(mc)?;
    let processed_any = matches!(result, PollResult::Executed);

    // If this was the outermost run and we didn't process any tasks, process
    // any pending unhandled checks. Only counting down on idle outermost
    // ticks prevents consuming the grace window while work is actively
    // being performed (which may attach handlers).
    if nesting_before == 0 && !processed_any {
        // We are leaving the outermost run and the loop was idle.
        // Advance the monotonic tick and process pending entries which
        // were recorded earlier with an insertion tick. Treat an entry
        // as unhandled only when the current tick has advanced by
        // `UNHANDLED_GRACE` since insertion.
        let prev_tick = CURRENT_TICK.load(Ordering::SeqCst);
        let current = CURRENT_TICK.fetch_add(1, Ordering::SeqCst) + 1;
        log::debug!("CURRENT_TICK advanced from {} to {}", prev_tick, current);
        // Unhandled checks are now performed by the individual UnhandledCheck task
        // which re-queues itself until the grace window has passed. No pending
        // list processing is required here.
    }

    // Leaving this run: decrement nesting
    RUN_LOOP_NESTING.fetch_sub(1, Ordering::SeqCst);
    Ok(result)
}

fn create_resolve_function_direct<'gc>(
    mc: &MutationContext<'gc>,
    promise: GcPtr<'gc, JSPromise<'gc>>,
    global_env: &JSObjectDataPtr<'gc>,
) -> Value<'gc> {
    let env = make_promise_js_object(mc, promise, None).unwrap();

    // Set the prototype to the global environment to allow access to global functions
    env.borrow_mut(mc).prototype = Some(*global_env);

    let body = vec![stmt_expr(Expr::Call(
        Box::new(Expr::Var("__internal_promise_resolve_captured".to_string(), None, None)),
        vec![Expr::Var("value".to_string(), None, None)],
    ))];

    Value::Closure(Gc::new(
        mc,
        ClosureData::new(&[DestructuringElement::Variable("value".to_string(), None)], &body, Some(env), None),
    ))
}

fn create_reject_function_direct<'gc>(
    mc: &MutationContext<'gc>,
    promise: GcPtr<'gc, JSPromise<'gc>>,
    global_env: &JSObjectDataPtr<'gc>,
) -> Value<'gc> {
    let env = make_promise_js_object(mc, promise, None).unwrap();

    // Set the prototype to the global environment to allow access to global functions
    env.borrow_mut(mc).prototype = Some(*global_env);

    let body = vec![stmt_expr(Expr::Call(
        Box::new(Expr::Var("__internal_promise_reject_captured".to_string(), None, None)),
        vec![Expr::Var("reason".to_string(), None, None)],
    ))];

    Value::Closure(Gc::new(
        mc,
        ClosureData::new(
            &[DestructuringElement::Variable("reason".to_string(), None)],
            &body,
            Some(env),
            None,
        ),
    ))
}

/// Helper to handle the actual resolution from the captured environment
pub fn __internal_promise_resolve_captured<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let val = if !args.is_empty() { args[0].clone() } else { Value::Undefined };
    log::trace!("__internal_promise_resolve_captured args_len={} val={:?}", args.len(), val);

    if let Some(promise) = get_promise_from_js_object(env) {
        resolve_promise(mc, &promise, val, env);
    }
    Ok(Value::Undefined)
}

pub fn __internal_promise_reject_captured<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let val = if !args.is_empty() { args[0].clone() } else { Value::Undefined };
    if let Some(promise) = get_promise_from_js_object(env) {
        reject_promise(mc, &promise, val, env);
    }
    Ok(Value::Undefined)
}

// Look up Promise.prototype
fn get_promise_prototype_from_env<'gc>(env: JSObjectDataPtr<'gc>) -> Option<JSObjectDataPtr<'gc>> {
    if let Some(ctor_val) = crate::core::env_get(&env, "Promise")
        && let Value::Object(ctor_obj) = &*ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(ctor_obj, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        return Some(*proto_obj);
    }
    None
}

/// Create a new JavaScript Promise object with prototype methods.
///
/// This function creates a JS object that wraps a JSPromise instance and
/// attaches the standard Promise prototype methods (then, catch, finally).
///
/// # Returns
/// * `Result<JSObjectDataPtr, JSError>` - The promise object or creation error
pub fn make_promise_js_object<'gc>(
    mc: &MutationContext<'gc>,
    promise: GcPtr<'gc, JSPromise<'gc>>,
    prototype: Option<JSObjectDataPtr<'gc>>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let promise_obj = new_js_object_data(mc);

    // Try to set prototype from Promise.prototype if env is available
    if let Some(prototype) = prototype
        && let Some(proto) = get_promise_prototype_from_env(prototype)
    {
        promise_obj.borrow_mut(mc).prototype = Some(proto);
    }

    // Add then method
    let then_func = Value::Function("Promise.prototype.then".to_string());
    object_set_key_value(mc, &promise_obj, "then", then_func)?;

    // Add catch method
    let catch_func = Value::Function("Promise.prototype.catch".to_string());
    object_set_key_value(mc, &promise_obj, "catch", catch_func)?;
    // Add finally method
    let finally_func = Value::Function("Promise.prototype.finally".to_string());
    object_set_key_value(mc, &promise_obj, "finally", finally_func)?;

    // Assign a stable object-side id for debugging/tracking
    let id = generate_unique_id();
    object_set_key_value(mc, &promise_obj, "__promise_obj_id", Value::Number(id as f64))?;

    object_set_key_value(mc, &promise_obj, "__promise", Value::Promise(promise))?;
    Ok(promise_obj)
}

pub fn get_promise_from_js_object<'gc>(obj: &JSObjectDataPtr<'gc>) -> Option<GcPtr<'gc, JSPromise<'gc>>> {
    if let Some(promise_val) = object_get_key_value(obj, "__promise")
        && let Value::Promise(promise) = &*promise_val.borrow()
    {
        return Some(*promise);
    }
    None
}

/// Direct then handler that operates on JSPromise directly
///
/// # Arguments
/// * `promise` - The promise to attach handlers to
/// * `args` - Method arguments (onFulfilled, onRejected callbacks)
/// * `env` - Current execution environment
///
/// # Returns
/// * `Result<Value, JSError>` - New promise for chaining or error
pub fn perform_promise_then<'gc>(
    mc: &MutationContext<'gc>,
    promise: Gc<'gc, GcCell<JSPromise<'gc>>>,
    on_fulfilled: Option<Value<'gc>>,
    on_rejected: Option<Value<'gc>>,
    result_promise: Option<Gc<'gc, GcCell<JSPromise<'gc>>>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    // Try to acquire a mutable borrow; if another borrow is active, defer attachment
    match promise.try_borrow_mut(mc) {
        Ok(mut promise_state) => {
            let rp = result_promise.unwrap_or_else(|| new_gc_cell_ptr(mc, JSPromise::new()));

            match promise_state.state {
                PromiseState::Pending => {
                    let before_len = promise_state.on_rejected.len();
                    promise_state
                        .on_fulfilled
                        .push((on_fulfilled.unwrap_or(Value::Undefined), rp, Some(*env)));
                    promise_state
                        .on_rejected
                        .push((on_rejected.unwrap_or(Value::Undefined), rp, Some(*env)));
                    let after_len = promise_state.on_rejected.len();
                    // If we transitioned from zero rejection handlers to >0, remove any pending UnhandledCheck tasks
                    if before_len == 0 && after_len > 0 {
                        // Mark as handled so subsequent unhandled scans skip it
                        promise_state.handled = true;
                        let ptr = Gc::as_ptr(promise) as usize;
                        drop(promise_state);
                        remove_unhandled_checks_for_promise(ptr);
                        // Also clear any already-recorded runtime unhandled rejection for this promise
                        clear_runtime_unhandled_for_promise(mc, env, ptr)?;
                    }
                }
                PromiseState::Fulfilled(_) => {
                    drop(promise_state);
                    queue_task(
                        mc,
                        Task::Resolution {
                            promise,
                            callbacks: vec![(on_fulfilled.unwrap_or(Value::Undefined), rp, Some(*env))],
                        },
                    );
                }
                PromiseState::Rejected(_) => {
                    // Mark as handled because an explicit rejection handler is being scheduled
                    promise_state.handled = true;
                    drop(promise_state);
                    queue_task(
                        mc,
                        Task::Rejection {
                            promise,
                            callbacks: vec![(on_rejected.unwrap_or(Value::Undefined), rp, Some(*env))],
                        },
                    );
                }
            }
        }
        Err(_) => {
            log::trace!("perform_promise_then: deferring handler attachment due to active borrow");
            // Defer attaching handlers to the event loop where we can safely mutate the promise
            queue_task(
                mc,
                Task::AttachHandlers {
                    promise,
                    on_fulfilled: on_fulfilled.clone(),
                    on_rejected: on_rejected.clone(),
                    result_promise,
                    env: *env,
                },
            );
        }
    }

    Ok(())
}

pub fn resolve_promise<'gc>(
    mc: &MutationContext<'gc>,
    promise: &Gc<'gc, GcCell<JSPromise<'gc>>>,
    value: Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) {
    log::trace!("resolve_promise called");
    // Diagnostic: print promise ptr, value, and calling env frame (if available)
    let mut frame_name: Option<String> = None;
    if let Some(frame_rc) = object_get_key_value(env, "__frame")
        && let Value::String(s) = &*frame_rc.borrow()
    {
        frame_name = Some(crate::unicode::utf16_to_utf8(s));
    }
    log::debug!(
        "resolve_promise called for promise ptr={:p} id={} value={:?} env_ptr={:p} frame={:?}",
        Gc::as_ptr(*promise),
        promise.borrow().id,
        value,
        env,
        frame_name
    );

    // If value is Undefined, capture a backtrace to find the caller
    if matches!(value, Value::Undefined) {
        log::debug!("resolve_promise received Undefined — capturing backtrace to find caller");
        let bt = std::backtrace::Backtrace::capture();
        log::debug!("resolve_promise backtrace:\n{:?}", bt);
    }

    let mut promise_borrow = promise.borrow_mut(mc);
    // println!("DEBUG: resolve_promise called for promise id={} value={:?}", promise_borrow.id, value);
    if let PromiseState::Pending = promise_borrow.state {
        // Check if value is a promise object for flattening
        if let Value::Object(obj) = &value
            && let Some(other_promise) = get_promise_from_js_object(obj)
        {
            // Adopt the state of the other promise
            let current_promise = *promise;

            let then_callback = Value::Closure(Gc::new(
                mc,
                ClosureData::new(
                    &[DestructuringElement::Variable("val".to_string(), None)],
                    &[stmt_expr(Expr::Call(
                        Box::new(Expr::Var("__internal_resolve_promise".to_string(), None, None)),
                        vec![
                            Expr::Var("__current_promise".to_string(), None, None),
                            Expr::Var("val".to_string(), None, None),
                        ],
                    ))],
                    {
                        let new_env = new_js_object_data(mc);
                        // Ensure the helper names and globals are reachable via prototype chain by
                        // setting the new env's prototype to the caller's env
                        new_env.borrow_mut(mc).prototype = Some(*env);
                        // Bind current promise on the env so the helper can access it
                        if let Err(e) = env_set(mc, &new_env, "__current_promise", Value::Promise(current_promise)) {
                            log::error!("resolve_promise: failed to set __current_promise in env: {}", e);
                        }
                        Some(new_env)
                    },
                    None,
                ),
            ));

            let catch_callback = Value::Closure(Gc::new(
                mc,
                ClosureData::new(
                    &[DestructuringElement::Variable("reason".to_string(), None)],
                    &[stmt_expr(Expr::Call(
                        Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                        vec![
                            Expr::Var("__current_promise".to_string(), None, None),
                            Expr::Var("reason".to_string(), None, None),
                        ],
                    ))],
                    {
                        let new_env = new_js_object_data(mc);
                        // Ensure the helper names and globals are reachable via prototype chain by
                        // setting the new env's prototype to the caller's env
                        new_env.borrow_mut(mc).prototype = Some(*env);
                        if let Err(e) = env_set(mc, &new_env, "__current_promise", Value::Promise(current_promise)) {
                            log::error!("resolve_promise: failed to set __current_promise in env: {}", e);
                        }
                        Some(new_env)
                    },
                    None,
                ),
            ));

            let other_promise_borrow = other_promise.borrow();
            match &other_promise_borrow.state {
                PromiseState::Fulfilled(val) => {
                    // Already fulfilled, resolve immediately with the value
                    drop(promise_borrow);
                    resolve_promise(mc, promise, val.clone(), env);
                    return;
                }
                PromiseState::Rejected(reason) => {
                    // Already rejected, reject immediately with the reason
                    // Also mark the other promise as handled since we are consuming its rejection
                    // into a new promise chain.
                    let reason = reason.clone();
                    drop(other_promise_borrow);

                    let mut other_promise_mut = other_promise.borrow_mut(mc);
                    if !other_promise_mut.handled {
                        other_promise_mut.handled = true;
                        let ptr = Gc::as_ptr(other_promise) as usize;
                        remove_unhandled_checks_for_promise(ptr);
                        clear_runtime_unhandled_for_promise(mc, env, ptr).ok();
                    }
                    drop(other_promise_mut);
                    drop(promise_borrow);

                    reject_promise(mc, promise, reason, env);
                    return;
                }
                PromiseState::Pending => {
                    // Still pending, attach callbacks
                    drop(other_promise_borrow);
                    let mut other_promise_mut = other_promise.borrow_mut(mc);
                    let before_len = other_promise_mut.on_rejected.len();
                    other_promise_mut.on_fulfilled.push((then_callback, *promise, None));
                    other_promise_mut.on_rejected.push((catch_callback, *promise, None));
                    let after_len = other_promise_mut.on_rejected.len();

                    if before_len == 0 && after_len > 0 {
                        other_promise_mut.handled = true;
                        let ptr = Gc::as_ptr(other_promise) as usize;
                        remove_unhandled_checks_for_promise(ptr);
                        clear_runtime_unhandled_for_promise(mc, env, ptr).ok();
                    }
                    return;
                }
            }
        }

        // Normal resolve
        log::trace!("resolve_promise setting promise ptr={:p} value = {:?}", Gc::as_ptr(*promise), value);
        promise_borrow.state = PromiseState::Fulfilled(value.clone());
        promise_borrow.value = Some(value);

        // Queue task to execute fulfilled callbacks asynchronously
        let callbacks = promise_borrow.on_fulfilled.clone();
        promise_borrow.on_fulfilled.clear();
        if !callbacks.is_empty() {
            log::trace!("resolve_promise: queuing {} callbacks", callbacks.len());
            log::debug!(
                "resolve_promise: scheduling Resolution task for promise ptr={:p} id={}",
                Gc::as_ptr(*promise),
                promise_borrow.id
            );
            queue_task(
                mc,
                Task::Resolution {
                    promise: *promise,
                    callbacks,
                },
            );
        }
    }
}

/// Reject a promise with a reason, transitioning it to rejected state.
///
/// This function changes the promise state from Pending to Rejected and
/// queues all registered rejection callbacks for asynchronous execution.
///
/// # Arguments
/// * `promise` - The promise to reject
/// * `reason` - The reason for rejection
///
/// # Behavior
/// - Only works if promise is currently in Pending state
/// - Sets promise state to Rejected and stores the reason
/// - Queues all on_rejected callbacks for async execution
/// - Clears the callback list after queuing
pub fn reject_promise<'gc>(
    mc: &MutationContext<'gc>,
    promise: &Gc<'gc, GcCell<JSPromise<'gc>>>,
    reason: Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) {
    let mut promise_borrow = promise.borrow_mut(mc);
    // Helpful debug logging for rejected promises (especially when rejecting
    // with JS Error-like objects) to help track unhandled rejections.
    if let Value::Object(obj) = &reason {
        if let Some(ctor_rc) = object_get_key_value(obj, "constructor") {
            log::debug!("reject_promise: rejecting with object whose constructor = {:?}", ctor_rc.borrow());
        } else {
            log::debug!("reject_promise: rejecting with object ptr={:p}", Gc::as_ptr(*obj));
        }
    } else {
        log::debug!("reject_promise: rejecting with value={}", value_to_string(&reason));
    }
    log::trace!("reject_promise callbacks count = {}", promise_borrow.on_rejected.len());
    if let PromiseState::Pending = promise_borrow.state {
        promise_borrow.state = PromiseState::Rejected(reason.clone());
        promise_borrow.value = Some(reason.clone());

        // Queue task to execute rejected callbacks asynchronously
        let callbacks = promise_borrow.on_rejected.clone();
        promise_borrow.on_rejected.clear();
        if !callbacks.is_empty() {
            log::debug!(
                "reject_promise: scheduling Rejection task for promise ptr={:p} id={} callbacks={}",
                Gc::as_ptr(*promise),
                promise_borrow.id,
                callbacks.len()
            );
            queue_task(
                mc,
                Task::Rejection {
                    promise: *promise,
                    callbacks,
                },
            );
        } else {
            // No callbacks now: queue a task to check for unhandled rejection
            // after potential handler attachment (avoids race with synchronous .then/.catch)
            log::trace!(
                "reject_promise: scheduling UnhandledCheck for promise ptr={:p} id={}",
                Gc::as_ptr(*promise),
                promise_borrow.id
            );
            queue_task(
                mc,
                Task::UnhandledCheck {
                    promise: *promise,
                    reason: reason.clone(),
                    insertion_tick: CURRENT_TICK.load(Ordering::SeqCst),
                    env: *env,
                    force: false,
                },
            );
        }
    }
}

/// Internal function for Promise.allSettled resolve callback
///
/// Called when an individual promise in Promise.allSettled resolves.
/// Creates a "fulfilled" result object and stores it in the shared state.
///
/// # Arguments
/// * `idx` - Index of the promise in the original array
/// * `value` - The resolved value
/// * `shared_state` - Shared state object containing results array and completion tracking
///
/// # Behavior
/// - Creates a result object with status="fulfilled" and the resolved value
/// - Stores the result in the results array at the specified index
/// - Increments the completion counter
/// - Resolves the main Promise.allSettled promise when all promises have settled
pub fn __internal_promise_allsettled_resolve<'gc>(
    mc: &MutationContext<'gc>,
    idx: f64,
    value: Value<'gc>,
    shared_state: Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    if let Value::Object(shared_state_obj) = shared_state {
        // Get results array
        if let Some(results_val_rc) = object_get_key_value(&shared_state_obj, "results")
            && let Value::Object(results_obj) = &*results_val_rc.borrow()
        {
            // Create settled result
            let settled = new_gc_cell_ptr(mc, JSObjectData::new());
            object_set_key_value(mc, &settled, "status", Value::String(utf8_to_utf16("fulfilled")))?;
            object_set_key_value(mc, &settled, "value", value)?;
            // Add to results array at idx
            object_set_key_value(mc, results_obj, idx as usize, Value::Object(settled))?;
        }

        // Increment completed
        if let Some(completed_val_rc) = object_get_key_value(&shared_state_obj, "completed")
            && let Value::Number(completed) = &*completed_val_rc.borrow()
        {
            let new_completed = completed + 1.0;
            object_set_key_value(mc, &shared_state_obj, "completed", Value::Number(new_completed))?;

            // Check if all completed
            if let Some(total_val_rc) = object_get_key_value(&shared_state_obj, "total")
                && let Value::Number(total) = &*total_val_rc.borrow()
                && new_completed == *total
            {
                // Resolve result promise
                if let Some(promise_val_rc) = object_get_key_value(&shared_state_obj, "result_promise")
                    && let Value::Promise(result_promise) = &*promise_val_rc.borrow()
                    && let Some(results_val_rc) = object_get_key_value(&shared_state_obj, "results")
                    && let Value::Object(results_obj) = &*results_val_rc.borrow()
                {
                    resolve_promise(mc, result_promise, Value::Object(*results_obj), env);
                }
            }
        }
    }
    Ok(())
}

/// Internal function for Promise.allSettled reject callback
///
/// Called when an individual promise in Promise.allSettled rejects.
/// Creates a "rejected" result object and stores it in the shared state.
///
/// # Arguments
/// * `idx` - Index of the promise in the original array
/// * `reason` - The rejection reason
/// * `shared_state` - Shared state object containing results array and completion tracking
///
/// # Behavior
/// - Creates a result object with status="rejected" and the rejection reason
/// - Stores the result in the results array at the specified index
/// - Increments the completion counter
/// - Resolves the main Promise.allSettled promise when all promises have settled
pub fn __internal_promise_allsettled_reject<'gc>(
    mc: &MutationContext<'gc>,
    idx: f64,
    reason: Value<'gc>,
    shared_state: Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    if let Value::Object(shared_state_obj) = shared_state {
        // Get results array
        if let Some(results_val_rc) = object_get_key_value(&shared_state_obj, "results")
            && let Value::Object(results_obj) = &*results_val_rc.borrow()
        {
            // Create settled result
            let settled = new_gc_cell_ptr(mc, JSObjectData::new());
            object_set_key_value(mc, &settled, "status", Value::String(utf8_to_utf16("rejected")))?;
            object_set_key_value(mc, &settled, "reason", reason)?;

            // Add to results array at idx
            object_set_key_value(mc, results_obj, idx as usize, Value::Object(settled))?;
        }

        // Increment completed
        if let Some(completed_val_rc) = object_get_key_value(&shared_state_obj, "completed")
            && let Value::Number(completed) = &*completed_val_rc.borrow()
        {
            let new_completed = completed + 1.0;
            object_set_key_value(mc, &shared_state_obj, "completed", Value::Number(new_completed))?;

            // Check if all completed
            if let Some(total_val_rc) = object_get_key_value(&shared_state_obj, "total")
                && let Value::Number(total) = &*total_val_rc.borrow()
                && new_completed == *total
            {
                // Resolve result promise
                if let Some(promise_val_rc) = object_get_key_value(&shared_state_obj, "result_promise")
                    && let Value::Promise(result_promise) = &*promise_val_rc.borrow()
                    && let Some(results_val_rc) = object_get_key_value(&shared_state_obj, "results")
                    && let Value::Object(results_obj) = &*results_val_rc.borrow()
                {
                    resolve_promise(mc, result_promise, Value::Object(*results_obj), env);
                }
            }
        }
    }
    Ok(())
}

/// Internal function for Promise.any resolve callback
///
/// Called when any promise in Promise.any resolves.
/// Immediately resolves the main Promise.any promise with the fulfilled value.
///
/// # Arguments
/// * `value` - The resolved value from the first fulfilled promise
/// * `result_promise` - The main Promise.any promise to resolve
pub fn __internal_promise_any_resolve<'gc>(
    mc: &MutationContext<'gc>,
    value: Value<'gc>,
    result_promise: Gc<'gc, GcCell<JSPromise<'gc>>>,
    env: &JSObjectDataPtr<'gc>,
) {
    resolve_promise(mc, &result_promise, value, env);
}

/// Internal function for Promise.any reject callback
///
/// Called when individual promises in Promise.any reject.
/// Tracks rejections and creates an AggregateError when all promises reject.
///
/// # Arguments
/// * `idx` - Index of the rejected promise
/// * `reason` - The rejection reason
/// * `rejections` - Vector storing all rejection reasons
/// * `rejected_count` - Counter of rejected promises
/// * `total` - Total number of promises
/// * `result_promise` - The main Promise.any promise
///
/// # Behavior
/// - Stores the rejection reason
/// - Increments rejection counter
/// - When all promises reject, creates AggregateError with all rejection reasons
#[allow(clippy::too_many_arguments)]
pub fn __internal_promise_any_reject<'gc>(
    mc: &MutationContext<'gc>,
    idx: f64,
    reason: Value<'gc>,
    rejections: Gc<'gc, GcCell<Vec<Option<Value<'gc>>>>>,
    rejected_count: GcPtr<'gc, usize>,
    total: usize,
    result_promise: Gc<'gc, GcCell<JSPromise<'gc>>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    let idx = idx as usize;
    rejections.borrow_mut(mc)[idx] = Some(reason);
    *rejected_count.borrow_mut(mc) += 1;

    if *rejected_count.borrow() == total {
        // All promises rejected, create AggregateError
        let aggregate_error = new_gc_cell_ptr(mc, JSObjectData::new());
        object_set_key_value(mc, &aggregate_error, "name", Value::String(utf8_to_utf16("AggregateError"))).unwrap();
        object_set_key_value(
            mc,
            &aggregate_error,
            "message",
            Value::String(utf8_to_utf16("All promises were rejected")),
        )?;

        let errors_array = new_gc_cell_ptr(mc, JSObjectData::new());
        let rejections_vec = rejections.borrow();
        for (i, rejection) in rejections_vec.iter().enumerate() {
            if let Some(err) = rejection {
                object_set_key_value(mc, &errors_array, i, err.clone())?;
            }
        }
        object_set_key_value(mc, &aggregate_error, "errors", Value::Object(errors_array))?;

        crate::js_promise::reject_promise(mc, &result_promise, Value::Object(aggregate_error), env);
    }
    Ok(())
}

/// Internal function for Promise.race resolve callback
///
/// Called when any promise in Promise.race resolves.
/// Immediately resolves the main Promise.race promise with the value.
///
/// # Arguments
/// * `value` - The resolved/rejected value from the first settled promise
/// * `result_promise` - The main Promise.race promise to resolve/reject
pub fn __internal_promise_race_resolve<'gc>(
    mc: &MutationContext<'gc>,
    value: Value<'gc>,
    result_promise: Gc<'gc, GcCell<JSPromise<'gc>>>,
    env: &JSObjectDataPtr<'gc>,
) {
    resolve_promise(mc, &result_promise, value, env);
}

/// Internal function for Promise.race reject callback
///
/// Called when any promise in Promise.race rejects.
/// Immediately rejects the main Promise.race promise with the reason.
///
/// # Arguments
/// * `reason` - The rejection reason from the first rejected promise
/// * `result_promise` - The main Promise.race promise to reject
pub fn __internal_promise_race_reject<'gc>(
    mc: &MutationContext<'gc>,
    reason: Value<'gc>,
    result_promise: Gc<'gc, GcCell<JSPromise<'gc>>>,
    env: &JSObjectDataPtr<'gc>,
) {
    reject_promise(mc, &result_promise, reason, env);
}

/// Internal function for AllSettledState resolve callback
///
/// Called when an individual promise in Promise.allSettled resolves.
/// Records the fulfillment in the AllSettledState stored in global storage.
///
/// # Arguments
/// * `state_index` - Index of the AllSettledState in global storage
/// * `index` - Index of the promise in the original array
/// * `value` - The resolved value
pub fn __internal_allsettled_state_record_fulfilled_env<'gc>(
    mc: &MutationContext<'gc>,
    state_env: Value<'gc>,
    index: f64,
    value: Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    log::trace!("__internal_allsettled_state_record_fulfilled_env called: idx={index}, val={value:?}");
    let index = index as usize;
    if let Value::Object(state_obj) = &state_env {
        if let Some(results_rc) = object_get_key_value(state_obj, "__results")
            && let Value::Object(results_arr) = &*results_rc.borrow()
        {
            // create result object
            let result_obj = new_gc_cell_ptr(mc, JSObjectData::new());
            object_set_key_value(mc, &result_obj, "status", Value::String(utf8_to_utf16("fulfilled")))?;
            object_set_key_value(mc, &result_obj, "value", value.clone())?;
            object_set_key_value(mc, results_arr, index, Value::Object(result_obj))?;
        }
        // increment completed
        if let Some(comp_rc) = object_get_key_value(state_obj, "__completed")
            && let Value::Number(n) = &*comp_rc.borrow()
        {
            object_set_key_value(mc, state_obj, "__completed", Value::Number(n + 1.0))?;
            // check for completion
            if let Some(total_rc) = object_get_key_value(state_obj, "__total")
                && let Value::Number(total) = &*total_rc.borrow()
                && (n + 1.0) == *total
                && let Some(promise_rc) = object_get_key_value(state_obj, "__result_promise")
                && let Value::Promise(result_promise_ref) = &*promise_rc.borrow()
            {
                // get results array
                if let Some(results_rc2) = object_get_key_value(state_obj, "__results")
                    && let Value::Object(results_arr2) = &*results_rc2.borrow()
                {
                    resolve_promise(mc, result_promise_ref, Value::Object(*results_arr2), env);
                }
            }
        }
    }
    Ok(())
}

/// Internal function for AllSettledState reject callback
///
/// Called when an individual promise in Promise.allSettled rejects.
/// Records the rejection in the AllSettledState stored in global storage.
///
/// # Arguments
/// * `state_index` - Index of the AllSettledState in global storage
/// * `index` - Index of the promise in the original array
/// * `reason` - The rejection reason
pub fn __internal_allsettled_state_record_rejected_env<'gc>(
    mc: &MutationContext<'gc>,
    state_env: Value<'gc>,
    index: f64,
    reason: Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    log::trace!("__internal_allsettled_state_record_rejected_env called: idx={index}, reason={reason:?}");
    let index = index as usize;
    if let Value::Object(state_obj) = &state_env {
        if let Some(results_rc) = object_get_key_value(state_obj, "__results")
            && let Value::Object(results_arr) = &*results_rc.borrow()
        {
            // create result object
            let result_obj = new_gc_cell_ptr(mc, JSObjectData::new());
            object_set_key_value(mc, &result_obj, "status", Value::String(utf8_to_utf16("rejected")))?;
            object_set_key_value(mc, &result_obj, "reason", reason.clone())?;
            object_set_key_value(mc, results_arr, index, Value::Object(result_obj))?;
        }
        // increment completed
        if let Some(comp_rc) = object_get_key_value(state_obj, "__completed")
            && let Value::Number(n) = &*comp_rc.borrow()
        {
            object_set_key_value(mc, state_obj, "__completed", Value::Number(n + 1.0))?;
            // check for completion
            if let Some(total_rc) = object_get_key_value(state_obj, "__total")
                && let Value::Number(total) = &*total_rc.borrow()
                && (n + 1.0) == *total
                && let Some(promise_rc) = object_get_key_value(state_obj, "__result_promise")
                && let Value::Promise(result_promise_ref) = &*promise_rc.borrow()
            {
                // get results array
                if let Some(results_rc2) = object_get_key_value(state_obj, "__results")
                    && let Value::Object(results_arr2) = &*results_rc2.borrow()
                {
                    resolve_promise(mc, result_promise_ref, Value::Object(*results_arr2), env);
                }
            }
        }
    }
    Ok(())
}

/// Create a resolve callback function for Promise.allSettled
///
/// Creates a closure that calls the internal function to record fulfillment
/// in the AllSettledState stored in global storage.
///
/// # Arguments
/// * `state_index` - Index of the AllSettledState in global storage
/// * `index` - Index of the promise in the original array
///
/// # Returns
/// A Value::Closure that can be used as a then callback
fn create_allsettled_resolve_callback<'gc>(
    mc: &MutationContext<'gc>,
    state_env: JSObjectDataPtr<'gc>,
    index: usize,
    parent_env: JSObjectDataPtr<'gc>,
) -> Value<'gc> {
    Value::Closure(Gc::new(
        mc,
        ClosureData::new(
            &[DestructuringElement::Variable("value".to_string(), None)],
            &[stmt_expr(Expr::Call(
                Box::new(Expr::Var(
                    "__internal_allsettled_state_record_fulfilled_env".to_string(),
                    None,
                    None,
                )),
                vec![
                    Expr::Var("__state_env".to_string(), None, None),
                    Expr::Number(index as f64),
                    Expr::Var("value".to_string(), None, None),
                ],
            ))],
            {
                let env = new_js_object_data(mc);
                env.borrow_mut(mc).prototype = Some(parent_env);
                env_set(mc, &env, "__state_env", Value::Object(state_env)).unwrap();
                Some(env)
            },
            None,
        ),
    ))
}

/// Create a reject callback function for Promise.allSettled
///
/// Creates a closure that calls the internal function to record rejection
/// in the state env stored on the closure's environment.
fn create_allsettled_reject_callback<'gc>(
    mc: &MutationContext<'gc>,
    state_env: JSObjectDataPtr<'gc>,
    index: usize,
    parent_env: JSObjectDataPtr<'gc>,
) -> Value<'gc> {
    Value::Closure(Gc::new(
        mc,
        ClosureData::new(
            &[DestructuringElement::Variable("reason".to_string(), None)],
            &[stmt_expr(Expr::Call(
                Box::new(Expr::Var("__internal_allsettled_state_record_rejected_env".to_string(), None, None)),
                vec![
                    Expr::Var("__state_env".to_string(), None, None),
                    Expr::Number(index as f64),
                    Expr::Var("reason".to_string(), None, None),
                ],
            ))],
            {
                let env = new_js_object_data(mc);
                env.borrow_mut(mc).prototype = Some(parent_env);
                env_set(mc, &env, "__state_env", Value::Object(state_env)).unwrap();
                Some(env)
            },
            None,
        ),
    ))
}

// Value-based wrappers for timer functions (used by global function dispatch)

pub fn handle_set_timeout_val<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if args.is_empty() {
        return Err(raise_eval_error!("setTimeout requires at least one argument"));
    }

    let callback = args[0].clone();
    let delay = if args.len() > 1 {
        match &args[1] {
            Value::Number(n) => n.max(0.0) as u64,
            _ => 0,
        }
    } else {
        0
    };

    let mut timeout_args = Vec::new();
    for arg in args.iter().skip(2) {
        timeout_args.push(arg.clone());
    }

    let id = NEXT_TIMEOUT_ID.with(|counter| {
        let mut id = counter.borrow_mut();
        let current_id = *id;
        *id += 1;
        current_id
    });

    // For small delays, schedule directly on the main thread to avoid
    // cross-thread latency for short timers used by tests.
    let when = Instant::now() + Duration::from_millis(delay);
    if delay <= short_timer_threshold_ms() {
        queue_task(
            mc,
            Task::Timeout {
                id,
                callback: callback.clone(),
                args: timeout_args,
                target_time: when,
            },
        );
        return Ok(Value::Number(id as f64));
    }

    // Store callback + args + optional interval in the thread-local timer registry for long timers.
    let cb_static = unsafe { std::mem::transmute::<Value<'gc>, Value<'static>>(callback.clone()) };
    let args_static: Vec<Value<'static>> = timeout_args
        .iter()
        .cloned()
        .map(|a| unsafe { std::mem::transmute::<Value<'gc>, Value<'static>>(a) })
        .collect();

    TIMER_REGISTRY.with(|reg| {
        reg.borrow_mut().insert(id, (cb_static, args_static, None));
    });

    // Schedule with timer thread
    let handle = ensure_timer_thread();
    let _ = handle.cmd_tx.send(TimerCommand::Schedule { id, when });

    // Also enqueue a placeholder task so the main event loop knows a timer is pending
    queue_task(
        mc,
        Task::Timeout {
            id,
            callback: callback.clone(),
            args: timeout_args,
            target_time: when,
        },
    );

    Ok(Value::Number(id as f64))
}

pub fn handle_clear_timeout_val<'gc>(
    _mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if args.is_empty() {
        return Ok(Value::Undefined);
    }

    let id = match &args[0] {
        Value::Number(n) => *n as usize,
        _ => return Ok(Value::Undefined),
    };

    // Remove from local registry if present
    TIMER_REGISTRY.with(|reg| {
        reg.borrow_mut().remove(&id);
    });

    // Tell timer thread to cancel
    if let Some(handle) = TIMER_THREAD_HANDLE.get() {
        let _ = handle.cmd_tx.send(TimerCommand::Cancel(id));
    }

    // Also remove from any queued tasks if present and keep ids in sync
    GLOBAL_TASK_QUEUE.with(|queue| {
        let mut queue_borrow = queue.borrow_mut();
        let orig = std::mem::take(&mut *queue_borrow);
        let mut new_tasks: Vec<Task<'static>> = Vec::new();
        let mut new_ids: Vec<usize> = Vec::new();
        GLOBAL_TASK_ID_QUEUE.with(|ids| {
            let id_borrow = ids.borrow();
            for (i, task) in orig.into_iter().enumerate() {
                let keep = !matches!(&task, Task::Timeout { id: task_id, .. } if *task_id == id);
                if keep {
                    new_tasks.push(task);
                    new_ids.push(id_borrow[i]);
                }
            }
        });
        *queue_borrow = new_tasks;
        GLOBAL_TASK_ID_QUEUE.with(|ids| *ids.borrow_mut() = new_ids);
    });

    Ok(Value::Undefined)
}

pub fn handle_set_interval_val<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if args.is_empty() {
        return Err(raise_eval_error!("setInterval requires at least one argument"));
    }

    let callback = args[0].clone();
    let delay = if args.len() > 1 {
        match &args[1] {
            Value::Number(n) => n.max(0.0) as u64,
            _ => 0,
        }
    } else {
        0
    };

    let mut interval_args = Vec::new();
    for arg in args.iter().skip(2) {
        interval_args.push(arg.clone());
    }

    let id = NEXT_TIMEOUT_ID.with(|counter| {
        let mut id = counter.borrow_mut();
        let current_id = *id;
        *id += 1;
        current_id
    });

    let interval = Duration::from_millis(delay);

    let when = Instant::now() + interval;
    if delay <= short_timer_threshold_ms() {
        // Small intervals: schedule locally and rely on local rescheduling for subsequent ticks.
        queue_task(
            mc,
            Task::Interval {
                id,
                callback: callback.clone(),
                args: interval_args.clone(),
                target_time: when,
                interval,
            },
        );
        return Ok(Value::Number(id as f64));
    }

    // Store in registry so the timer thread can manage long sleeps and expiry
    let cb_static = unsafe { std::mem::transmute::<Value<'gc>, Value<'static>>(callback.clone()) };
    let args_static: Vec<Value<'static>> = interval_args
        .iter()
        .cloned()
        .map(|a| unsafe { std::mem::transmute::<Value<'gc>, Value<'static>>(a) })
        .collect();

    TIMER_REGISTRY.with(|reg| {
        reg.borrow_mut().insert(id, (cb_static, args_static, Some(interval)));
    });

    // Schedule with timer thread
    let handle = ensure_timer_thread();
    let _ = handle.cmd_tx.send(TimerCommand::Schedule { id, when });

    // Enqueue placeholder interval task so the main event loop observes the pending timer
    queue_task(
        mc,
        Task::Interval {
            id,
            callback: callback.clone(),
            args: interval_args.clone(),
            target_time: when,
            interval,
        },
    );

    Ok(Value::Number(id as f64))
}

pub fn handle_clear_interval_val<'gc>(
    _mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if args.is_empty() {
        return Ok(Value::Undefined);
    }

    let id = match &args[0] {
        Value::Number(n) => *n as usize,
        _ => return Ok(Value::Undefined),
    };

    // Remove from local registry if present
    TIMER_REGISTRY.with(|reg| {
        reg.borrow_mut().remove(&id);
    });

    // Tell timer thread to cancel
    if let Some(handle) = TIMER_THREAD_HANDLE.get() {
        let _ = handle.cmd_tx.send(TimerCommand::Cancel(id));
    }

    // Also remove from any queued tasks if present
    GLOBAL_TASK_QUEUE.with(|queue| {
        let mut queue_borrow = queue.borrow_mut();
        queue_borrow.retain(|task| !matches!(task, Task::Interval { id: task_id, .. } if *task_id == id));
    });

    Ok(Value::Undefined)
}

/// Initialize Promise constructor and prototype
pub fn initialize_promise<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let promise_ctor = new_js_object_data(mc);
    object_set_key_value(mc, &promise_ctor, "__is_constructor", Value::Boolean(true))?;
    object_set_key_value(mc, &promise_ctor, "__native_ctor", Value::String(utf8_to_utf16("Promise")))?;
    object_set_key_value(mc, &promise_ctor, "name", Value::String(utf8_to_utf16("Promise")))?;

    // Setup prototype
    let promise_proto = new_js_object_data(mc);
    object_set_key_value(mc, &promise_ctor, "prototype", Value::Object(promise_proto))?;
    object_set_key_value(mc, &promise_proto, "constructor", Value::Object(promise_ctor))?;

    // Static methods
    let static_methods = vec!["all", "race", "any", "allSettled", "resolve", "reject"];
    for method in static_methods {
        object_set_key_value(mc, &promise_ctor, method, Value::Function(format!("Promise.{}", method)))?;
    }

    // Prototype methods
    let methods = vec!["then", "catch", "finally"];
    for method in methods {
        object_set_key_value(mc, &promise_proto, method, Value::Function(format!("Promise.prototype.{}", method)))?;
    }

    // Symbol.toStringTag
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(s) = &*tag_sym.borrow()
    {
        object_set_key_value(mc, &promise_proto, s, Value::String(utf8_to_utf16("Promise")))?;
    }

    crate::core::env_set(mc, env, "Promise", Value::Object(promise_ctor))?;

    // Internal helpers for resolution/rejection captures
    crate::core::env_set(
        mc,
        env,
        "__internal_promise_resolve_captured",
        Value::Function("__internal_promise_resolve_captured".to_string()),
    )?;
    crate::core::env_set(
        mc,
        env,
        "__internal_promise_reject_captured",
        Value::Function("__internal_promise_reject_captured".to_string()),
    )?;

    // Register finally internal helpers so closures can call into them by name
    crate::core::env_set(
        mc,
        env,
        "__internal_promise_finally_resolve",
        Value::Function("__internal_promise_finally_resolve".to_string()),
    )?;
    crate::core::env_set(
        mc,
        env,
        "__internal_promise_finally_reject",
        Value::Function("__internal_promise_finally_reject".to_string()),
    )?;

    // Register Promise.all internal helpers
    crate::core::env_set(
        mc,
        env,
        "__internal_promise_all_resolve",
        Value::Function("__internal_promise_all_resolve".to_string()),
    )?;
    crate::core::env_set(
        mc,
        env,
        "__internal_promise_all_reject",
        Value::Function("__internal_promise_all_reject".to_string()),
    )?;

    Ok(())
}

// Missing helpers for Promise.all
pub fn __internal_promise_all_resolve<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if args.len() < 3 {
        return Ok(Value::Undefined);
    }
    let index_val = &args[0];
    let value = &args[1];
    let state_val = &args[2];

    if let Value::Object(state_obj) = state_val
        && let Some(results_val_rc) = object_get_key_value(state_obj, "results")
        && let Value::Object(results_obj) = &*results_val_rc.borrow()
    {
        let idx_str = match index_val {
            Value::Number(n) => n.to_string(),
            _ => return Ok(Value::Undefined),
        };
        object_set_key_value(mc, results_obj, &idx_str, value.clone())?;

        if let Some(completed_val_rc) = object_get_key_value(state_obj, "completed")
            && let Value::Number(completed) = &*completed_val_rc.borrow()
        {
            let new_completed = completed + 1.0;
            object_set_key_value(mc, state_obj, "completed", Value::Number(new_completed))?;

            if let Some(total_val_rc) = object_get_key_value(state_obj, "total")
                && let Value::Number(total) = &*total_val_rc.borrow()
                && new_completed == *total
                && let Some(promise_val_rc) = object_get_key_value(state_obj, "result_promise")
                && let Value::Promise(promise_ref) = &*promise_val_rc.borrow()
            {
                resolve_promise(mc, promise_ref, Value::Object(*results_obj), env);
            }
        }
    }
    Ok(Value::Undefined)
}

pub fn __internal_promise_all_reject<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if args.len() < 2 {
        return Ok(Value::Undefined);
    }
    let reason = &args[0];
    let state_val = &args[1];

    if let Value::Object(state_obj) = state_val
        && let Some(promise_val_rc) = object_get_key_value(state_obj, "result_promise")
        && let Value::Promise(promise_ref) = &*promise_val_rc.borrow()
    {
        reject_promise(mc, promise_ref, reason.clone(), env);
    }
    Ok(Value::Undefined)
}

pub fn handle_promise_static_method_val<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    match method {
        "resolve" => {
            let val = if args.is_empty() { Value::Undefined } else { args[0].clone() };
            // If the argument is already a Promise object we should return it directly (per spec)
            if let Value::Object(obj) = &val
                && get_promise_from_js_object(obj).is_some()
            {
                // It's already a Promise object; return it as-is
                return Ok(val.clone());
            }
            // Manually creating promise object and resolving it
            // We can't easily call `make_promise_js_object` from here if it relies on "Promise" in env.
            // `make_promise_js_object` uses object_get_key_value(env, "Promise") to get prototype.
            // If Promise is not yet initialized in env fully this might be tricky, but handle_promise_static_method_val runs dynamically.
            let promise = new_gc_cell_ptr(mc, JSPromise::new());
            let promise_obj = make_promise_js_object(mc, promise, Some(*env))?;
            resolve_promise(mc, &promise, val, env);
            Ok(Value::Object(promise_obj))
        }
        "reject" => {
            let val = if args.is_empty() { Value::Undefined } else { args[0].clone() };
            let promise = new_gc_cell_ptr(mc, JSPromise::new());
            let promise_obj = make_promise_js_object(mc, promise, Some(*env))?;
            reject_promise(mc, &promise, val, env);
            Ok(Value::Object(promise_obj))
        }
        "allSettled" => {
            // Runtime (evaluated args) version of Promise.allSettled
            log::trace!("handle_promise_static_method_val - allSettled entered with args={:?}", args);
            if args.is_empty() {
                return Err(crate::raise_eval_error!("Promise.allSettled requires at least one argument"));
            }
            let iterable = args[0].clone();
            log::trace!("allSettled iterable={:?}", iterable);
            let promises = match iterable {
                Value::Object(arr) => {
                    let mut promises = Vec::new();
                    let mut i = 0_usize;
                    while let Some(val_rc) = object_get_key_value(&arr, i) {
                        promises.push((*val_rc).borrow().clone());
                        i += 1;
                    }
                    promises
                }
                _ => return Err(crate::raise_eval_error!("Promise.allSettled argument must be iterable")),
            };

            let result_promise = new_gc_cell_ptr(mc, JSPromise::new());
            let result_promise_obj = make_promise_js_object(mc, result_promise, Some(*env))?;

            let num_promises = promises.len();
            if num_promises == 0 {
                let result_arr = crate::js_array::create_array(mc, env)?;
                resolve_promise(mc, &result_promise, Value::Object(result_arr), env);
                return Ok(Value::Object(result_promise_obj));
            }

            let results_array = crate::js_array::create_array(mc, env)?;
            let state_env = crate::core::new_js_object_data(mc);
            object_set_key_value(mc, &state_env, "__results", Value::Object(results_array))?;
            object_set_key_value(mc, &state_env, "__completed", Value::Number(0.0))?;
            object_set_key_value(mc, &state_env, "__total", Value::Number(num_promises as f64))?;
            object_set_key_value(mc, &state_env, "__result_promise", Value::Promise(result_promise))?;

            for (idx, promise_val) in promises.into_iter().enumerate() {
                match promise_val {
                    Value::Object(obj) => {
                        if let Some(promise_ref) = get_promise_from_js_object(&obj) {
                            let promise_state = &promise_ref.borrow().state;
                            match promise_state {
                                PromiseState::Fulfilled(val) => {
                                    let result_obj = new_gc_cell_ptr(mc, JSObjectData::new());
                                    object_set_key_value(mc, &result_obj, "status", Value::String(utf8_to_utf16("fulfilled")))?;
                                    object_set_key_value(mc, &result_obj, "value", val.clone())?;
                                    object_set_key_value(mc, &results_array, idx, Value::Object(result_obj))?;
                                    if let Some(comp_rc) = object_get_key_value(&state_env, "__completed")
                                        && let Value::Number(n) = &*comp_rc.borrow()
                                    {
                                        object_set_key_value(mc, &state_env, "__completed", Value::Number(n + 1.0))?;
                                        if let Some(total_rc) = object_get_key_value(&state_env, "__total")
                                            && let Value::Number(total) = &*total_rc.borrow()
                                            && (n + 1.0) == *total
                                        {
                                            resolve_promise(mc, &result_promise, Value::Object(results_array), env);
                                            return Ok(Value::Object(result_promise_obj));
                                        }
                                    }
                                }
                                PromiseState::Rejected(reason) => {
                                    let result_obj = new_gc_cell_ptr(mc, JSObjectData::new());
                                    object_set_key_value(mc, &result_obj, "status", Value::String(utf8_to_utf16("rejected")))?;
                                    object_set_key_value(mc, &result_obj, "reason", reason.clone())?;
                                    object_set_key_value(mc, &results_array, idx, Value::Object(result_obj))?;
                                    if let Some(comp_rc) = object_get_key_value(&state_env, "__completed")
                                        && let Value::Number(n) = &*comp_rc.borrow()
                                    {
                                        object_set_key_value(mc, &state_env, "__completed", Value::Number(n + 1.0))?;
                                    }
                                }
                                PromiseState::Pending => {
                                    let then_callback = create_allsettled_resolve_callback(mc, state_env, idx, *env);
                                    let catch_callback = create_allsettled_reject_callback(mc, state_env, idx, *env);
                                    perform_promise_then(
                                        mc,
                                        promise_ref,
                                        Some(then_callback),
                                        Some(catch_callback),
                                        None, // Do not tie result to result_promise
                                        env,
                                    )?;
                                }
                            }
                        } else {
                            let result_obj = new_gc_cell_ptr(mc, JSObjectData::new());
                            object_set_key_value(mc, &result_obj, "status", Value::String(utf8_to_utf16("fulfilled")))?;
                            object_set_key_value(mc, &result_obj, "value", Value::Object(obj))?;
                            object_set_key_value(mc, &results_array, idx, Value::Object(result_obj))?;
                            if let Some(comp_rc) = object_get_key_value(&state_env, "__completed")
                                && let Value::Number(n) = &*comp_rc.borrow()
                            {
                                object_set_key_value(mc, &state_env, "__completed", Value::Number(n + 1.0))?;
                            }
                        }
                    }
                    val => {
                        let result_obj = new_gc_cell_ptr(mc, JSObjectData::new());
                        object_set_key_value(mc, &result_obj, "status", Value::String(utf8_to_utf16("fulfilled")))?;
                        object_set_key_value(mc, &result_obj, "value", val.clone())?;
                        object_set_key_value(mc, &results_array, idx, Value::Object(result_obj))?;
                        if let Some(comp_rc) = object_get_key_value(&state_env, "__completed")
                            && let Value::Number(n) = &*comp_rc.borrow()
                        {
                            object_set_key_value(mc, &state_env, "__completed", Value::Number(n + 1.0))?;
                        }
                    }
                }
            }

            if let Some(comp_rc) = object_get_key_value(&state_env, "__completed")
                && let Value::Number(n) = &*comp_rc.borrow()
                && (*n as usize) == num_promises
            {
                resolve_promise(mc, &result_promise, Value::Object(results_array), env);
                return Ok(Value::Object(result_promise_obj));
            }

            Ok(Value::Object(result_promise_obj))
        }
        "all" => {
            // Runtime (evaluated args) version of Promise.all
            if args.is_empty() {
                return Err(crate::raise_eval_error!("Promise.all requires at least one argument"));
            }
            let iterable = args[0].clone();
            let promises = match iterable {
                Value::Object(arr) => {
                    let mut promises = Vec::new();
                    let mut i = 0_usize;
                    while let Some(val_rc) = object_get_key_value(&arr, i) {
                        promises.push((*val_rc).borrow().clone());
                        i += 1;
                    }
                    promises
                }
                _ => return Err(crate::raise_eval_error!("Promise.all argument must be iterable")),
            };

            let result_promise = new_gc_cell_ptr(mc, JSPromise::new());
            let result_promise_obj = make_promise_js_object(mc, result_promise, Some(*env))?;

            let num_promises = promises.len();
            if num_promises == 0 {
                let result_arr = crate::js_array::create_array(mc, env)?;
                resolve_promise(mc, &result_promise, Value::Object(result_arr), env);
                return Ok(Value::Object(result_promise_obj));
            }

            // Create state object for coordination
            let state_obj = new_js_object_data(mc);
            let results_obj = crate::js_array::create_array(mc, env)?;
            object_set_key_value(mc, &state_obj, "results", Value::Object(results_obj))?;
            object_set_key_value(mc, &state_obj, "completed", Value::Number(0.0))?;
            object_set_key_value(mc, &state_obj, "total", Value::Number(num_promises as f64))?;
            object_set_key_value(mc, &state_obj, "result_promise", Value::Promise(result_promise))?;

            for (idx, promise_val) in promises.into_iter().enumerate() {
                let state_obj_clone = state_obj;

                match promise_val {
                    Value::Object(obj) => {
                        if let Some(promise_ref) = get_promise_from_js_object(&obj) {
                            // Check if promise is already settled
                            let promise_state = &promise_ref.borrow().state;
                            match promise_state {
                                PromiseState::Fulfilled(val) => {
                                    object_set_key_value(mc, &results_obj, idx, val.clone())?;
                                    if let Some(completed_val_rc) = object_get_key_value(&state_obj, "completed")
                                        && let Value::Number(completed) = &*completed_val_rc.borrow()
                                    {
                                        let new_completed = completed + 1.0;
                                        object_set_key_value(mc, &state_obj, "completed", Value::Number(new_completed))?;
                                        if let Some(total_val_rc) = object_get_key_value(&state_obj, "total")
                                            && let Value::Number(total) = &*total_val_rc.borrow()
                                            && new_completed == *total
                                            && let Some(promise_val_rc) = object_get_key_value(&state_obj, "result_promise")
                                            && let Value::Promise(result_promise_ref) = &*promise_val_rc.borrow()
                                        {
                                            resolve_promise(mc, result_promise_ref, Value::Object(results_obj), env);
                                        }
                                    }
                                }
                                PromiseState::Rejected(reason) => {
                                    if let Some(promise_val_rc) = object_get_key_value(&state_obj, "result_promise")
                                        && let Value::Promise(result_promise_ref) = &*promise_val_rc.borrow()
                                    {
                                        reject_promise(mc, result_promise_ref, reason.clone(), env);
                                    }
                                    remove_unhandled_checks_for_promise(Gc::as_ptr(promise_ref) as usize);
                                    let noop_env = new_js_object_data(mc);
                                    let noop_closure = Value::Closure(Gc::new(mc, ClosureData::new(&[], &[], Some(noop_env), None)));
                                    perform_promise_then(mc, promise_ref, None, Some(noop_closure), None, env)?;
                                    return Ok(Value::Object(result_promise_obj));
                                }
                                PromiseState::Pending => {
                                    let then_callback = Value::Closure(Gc::new(
                                        mc,
                                        ClosureData::new(
                                            &[DestructuringElement::Variable("value".to_string(), None)],
                                            &[stmt_expr(Expr::Call(
                                                Box::new(Expr::Var("__internal_promise_all_resolve".to_string(), None, None)),
                                                vec![
                                                    Expr::Number(idx as f64),
                                                    Expr::Var("value".to_string(), None, None),
                                                    Expr::Var("__state".to_string(), None, None),
                                                ],
                                            ))],
                                            {
                                                let new_env = *env;
                                                object_set_key_value(mc, &new_env, "__state", Value::Object(state_obj_clone))?;
                                                Some(new_env)
                                            },
                                            None,
                                        ),
                                    ));

                                    let catch_callback = Value::Closure(Gc::new(
                                        mc,
                                        ClosureData::new(
                                            &[DestructuringElement::Variable("reason".to_string(), None)],
                                            &[stmt_expr(Expr::Call(
                                                Box::new(Expr::Var("__internal_promise_all_reject".to_string(), None, None)),
                                                vec![
                                                    Expr::Var("reason".to_string(), None, None),
                                                    Expr::Var("__state".to_string(), None, None),
                                                ],
                                            ))],
                                            {
                                                let new_env = *env;
                                                object_set_key_value(mc, &new_env, "__state", Value::Object(state_obj_clone))?;
                                                Some(new_env)
                                            },
                                            None,
                                        ),
                                    ));

                                    perform_promise_then(mc, promise_ref, Some(then_callback), None, None, env)?;
                                    perform_promise_then(mc, promise_ref, None, Some(catch_callback), None, env)?;
                                }
                            }
                        } else {
                            let val = Value::Object(obj);
                            object_set_key_value(mc, &results_obj, idx, val)?;
                            if let Some(completed_val_rc) = object_get_key_value(&state_obj, "completed")
                                && let Value::Number(completed) = &*completed_val_rc.borrow()
                            {
                                object_set_key_value(mc, &state_obj, "completed", Value::Number(completed + 1.0))?;
                                // We'll check for completion below
                            }
                        }
                    }
                    val => {
                        object_set_key_value(mc, &results_obj, idx, val.clone())?;
                        if let Some(completed_val_rc) = object_get_key_value(&state_obj, "completed")
                            && let Value::Number(completed) = &*completed_val_rc.borrow()
                        {
                            object_set_key_value(mc, &state_obj, "completed", Value::Number(completed + 1.0))?;
                        }
                    }
                }
            }

            if let Some(completed_val_rc) = object_get_key_value(&state_obj, "completed")
                && let Value::Number(completed) = &*completed_val_rc.borrow()
                && (*completed as usize) == num_promises
                && let Some(promise_val_rc) = object_get_key_value(&state_obj, "result_promise")
                && let Value::Promise(result_promise_ref) = &*promise_val_rc.borrow()
            {
                resolve_promise(mc, result_promise_ref, Value::Object(results_obj), env);
            }

            Ok(Value::Object(result_promise_obj))
        }
        "race" => {
            // Runtime (evaluated args) version of Promise.race
            if args.is_empty() {
                return Err(crate::raise_eval_error!("Promise.race requires at least one argument"));
            }
            let iterable = args[0].clone();
            let promises = match iterable {
                Value::Object(arr) => {
                    let mut promises = Vec::new();
                    let mut i = 0_usize;
                    while let Some(val_rc) = object_get_key_value(&arr, i) {
                        promises.push((*val_rc).borrow().clone());
                        i += 1;
                    }
                    promises
                }
                _ => return Err(crate::raise_eval_error!("Promise.race argument must be iterable")),
            };

            let result_promise = new_gc_cell_ptr(mc, JSPromise::new());
            let result_promise_obj = make_promise_js_object(mc, result_promise, Some(*env))?;

            for promise_val in promises {
                let result_promise_clone = result_promise;

                match promise_val {
                    Value::Object(obj) => {
                        if let Some(promise_ref) = get_promise_from_js_object(&obj) {
                            let promise_state = &promise_ref.borrow().state;
                            match promise_state {
                                PromiseState::Fulfilled(val) => {
                                    resolve_promise(mc, &result_promise, val.clone(), env);
                                    return Ok(Value::Object(result_promise_obj));
                                }
                                PromiseState::Rejected(reason) => {
                                    reject_promise(mc, &result_promise, reason.clone(), env);
                                    return Ok(Value::Object(result_promise_obj));
                                }
                                PromiseState::Pending => {
                                    let then_callback = Value::Closure(Gc::new(
                                        mc,
                                        ClosureData::new(
                                            &[DestructuringElement::Variable("value".to_string(), None)],
                                            &[stmt_expr(Expr::Call(
                                                Box::new(Expr::Var("__internal_promise_race_resolve".to_string(), None, None)),
                                                vec![
                                                    Expr::Var("value".to_string(), None, None),
                                                    Expr::Var("__result_promise".to_string(), None, None),
                                                ],
                                            ))],
                                            {
                                                let new_env = *env;
                                                object_set_key_value(
                                                    mc,
                                                    &new_env,
                                                    "__result_promise",
                                                    Value::Promise(result_promise_clone),
                                                )?;
                                                Some(new_env)
                                            },
                                            None,
                                        ),
                                    ));

                                    let catch_callback = Value::Closure(Gc::new(
                                        mc,
                                        ClosureData::new(
                                            &[DestructuringElement::Variable("reason".to_string(), None)],
                                            &[stmt_expr(Expr::Call(
                                                Box::new(Expr::Var("__internal_promise_race_reject".to_string(), None, None)),
                                                vec![
                                                    Expr::Var("reason".to_string(), None, None),
                                                    Expr::Var("__result_promise".to_string(), None, None),
                                                ],
                                            ))],
                                            {
                                                let new_env = *env;
                                                object_set_key_value(
                                                    mc,
                                                    &new_env,
                                                    "__result_promise",
                                                    Value::Promise(result_promise_clone),
                                                )?;
                                                Some(new_env)
                                            },
                                            None,
                                        ),
                                    ));

                                    perform_promise_then(mc, promise_ref, Some(then_callback), None, Some(result_promise), env)?;
                                    perform_promise_then(mc, promise_ref, None, Some(catch_callback), Some(result_promise), env)?;
                                }
                            }
                        } else {
                            resolve_promise(mc, &result_promise, Value::Object(obj), env);
                            return Ok(Value::Object(result_promise_obj));
                        }
                    }
                    val => {
                        resolve_promise(mc, &result_promise, val.clone(), env);
                        return Ok(Value::Object(result_promise_obj));
                    }
                }
            }

            Ok(Value::Object(result_promise_obj))
        }
        _ => Err(crate::raise_eval_error!(format!(
            "Static method Promise.{} is not yet wired to receive evaluated arguments.",
            method
        ))),
    }
}

pub fn handle_promise_prototype_method<'gc>(
    mc: &MutationContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let promise = get_promise_from_js_object(obj).ok_or(crate::raise_type_error!("Method called on incompatible receiver"))?;

    match method {
        "then" => {
            let on_fulfilled = args.first().cloned();
            let on_rejected = args.get(1).cloned();
            handle_promise_then_val(mc, promise, on_fulfilled, on_rejected, env)
        }
        "catch" => {
            let on_rejected = args.first().cloned();
            handle_promise_then_val(mc, promise, None, on_rejected, env)
        }
        "finally" => {
            let on_finally = args.first().cloned();
            handle_promise_finally_val(mc, promise, on_finally, env)
        }
        _ => Err(crate::raise_eval_error!(format!("Unknown Promise method {method}"))),
    }
}

pub fn handle_promise_constructor_val<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.is_empty() {
        return Err(raise_eval_error!("Promise constructor requires an executor function").into());
    }
    let executor = &args[0];

    let promise = new_gc_cell_ptr(mc, JSPromise::new());
    let promise_obj = make_promise_js_object(mc, promise, Some(*env))?;
    // Also store the internal id on the object for correlation
    object_set_key_value(mc, &promise_obj, "__promise_internal_id", Value::Number(promise.borrow().id as f64))?;

    let resolve_func = create_resolve_function_direct(mc, promise, env);
    let reject_func = create_reject_function_direct(mc, promise, env);

    if let Some((params, body, captured_env)) = crate::core::extract_closure_from_value(executor) {
        let executor_args = vec![resolve_func.clone(), reject_func.clone()];
        let executor_env = if params.is_empty() {
            prepare_closure_call_env(mc, Some(&captured_env), None, &[], None)?
        } else {
            prepare_closure_call_env(mc, Some(&captured_env), Some(&params[..]), &executor_args, None)?
        };
        log::trace!("Promise executor params={:?}", params);
        match crate::core::evaluate_statements(mc, &executor_env, &body) {
            Ok(_) => {}
            Err(e) => {
                // Executor threw synchronously — reject the created promise instead of propagating
                // Extract a JS reason value when possible
                let reason = match e {
                    EvalError::Throw(val, _line, _col) => val,
                    EvalError::Js(js_err) => Value::String(utf8_to_utf16(&js_err.message())),
                };
                // Call reject callback with the reason
                match &reject_func {
                    Value::Closure(cl) => {
                        let _ = crate::core::call_closure(mc, cl, None, std::slice::from_ref(&reason), &executor_env, None);
                    }
                    Value::Function(name) => {
                        if let Ok(call_env) = crate::js_class::prepare_call_env_with_this(
                            mc,
                            Some(&executor_env),
                            None,
                            None,
                            std::slice::from_ref(&reason),
                            None,
                            Some(&executor_env),
                            None,
                        ) {
                            let _ = crate::core::evaluate_call_dispatch(
                                mc,
                                &call_env,
                                Value::Function(name.clone()),
                                None,
                                vec![reason.clone()],
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    } else {
        return Err(raise_type_error!("Promise executor must be a function").into());
    }
    Ok(Value::Object(promise_obj))
}

pub fn handle_promise_then_val<'gc>(
    mc: &MutationContext<'gc>,
    promise: Gc<'gc, GcCell<JSPromise<'gc>>>,
    on_fulfilled: Option<Value<'gc>>,
    on_rejected: Option<Value<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let new_promise = new_gc_cell_ptr(mc, JSPromise::new());
    let new_promise_obj = make_promise_js_object(mc, new_promise, Some(*env))?;

    perform_promise_then(mc, promise, on_fulfilled, on_rejected, Some(new_promise), env)?;

    Ok(Value::Object(new_promise_obj))
}

pub fn __internal_promise_finally_resolve<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // args: (value, on_finally, result_promise)
    if args.len() < 3 {
        return Ok(Value::Undefined);
    }
    let orig_value = args[0].clone();
    let on_finally = args[1].clone();
    let result_promise_val = args[2].clone();

    let result_promise = if let Value::Promise(p) = &result_promise_val {
        *p
    } else {
        return Ok(Value::Undefined);
    };

    // If on_finally is callable, call it and react to its result
    match on_finally {
        Value::Closure(ref cl) => {
            match crate::core::call_closure(mc, cl, None, &[], env, None) {
                Ok(ret) => {
                    // If ret is a Promise, chain it
                    if let Value::Object(o) = ret
                        && let Some(inner_p) = get_promise_from_js_object(&o)
                    {
                        // When inner resolves -> resolve result_promise with original value
                        // When inner rejects -> reject result_promise with that reason
                        perform_promise_then(
                            mc,
                            inner_p,
                            Some(Value::Closure(Gc::new(
                                mc,
                                ClosureData::new(
                                    &[],
                                    &[stmt_expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_resolve_promise".to_string(), None, None)),
                                        vec![
                                            Expr::Var("__result_promise".to_string(), None, None),
                                            Expr::Var("__orig_value".to_string(), None, None),
                                        ],
                                    ))],
                                    {
                                        let new_env = *env;
                                        object_set_key_value(mc, &new_env, "__result_promise", Value::Promise(result_promise))?;
                                        object_set_key_value(mc, &new_env, "__orig_value", orig_value.clone())?;
                                        Some(new_env)
                                    },
                                    None,
                                ),
                            ))),
                            Some(Value::Closure(Gc::new(
                                mc,
                                ClosureData::new(
                                    &[],
                                    &[stmt_expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                                        vec![
                                            Expr::Var("__result_promise".to_string(), None, None),
                                            Expr::Var("__reason".to_string(), None, None),
                                        ],
                                    ))],
                                    {
                                        let new_env = *env;
                                        object_set_key_value(mc, &new_env, "__result_promise", Value::Promise(result_promise))?;
                                        Some(new_env)
                                    },
                                    None,
                                ),
                            ))),
                            Some(result_promise),
                            env,
                        )?;
                        return Ok(Value::Undefined);
                    }
                    // Not a promise: pass through original value
                    resolve_promise(mc, &result_promise, orig_value, env);
                }
                Err(e) => match e {
                    crate::core::EvalError::Throw(val, _, _) => {
                        reject_promise(mc, &result_promise, val, env);
                    }
                    _ => return Err(JSError::from(e)),
                },
            }
        }
        Value::Object(ref obj) => {
            // Support function objects that wrap a closure (from evaluate_expr Function -> Object)
            if let Some(cl_rc) = obj.borrow().get_closure()
                && let Value::Closure(cl) = &*cl_rc.borrow()
            {
                match crate::core::call_closure(mc, cl, None, &[], env, None) {
                    Ok(ret) => {
                        if let Value::Object(o) = ret
                            && let Some(inner_p) = get_promise_from_js_object(&o)
                        {
                            perform_promise_then(
                                mc,
                                inner_p,
                                Some(Value::Closure(Gc::new(
                                    mc,
                                    ClosureData::new(
                                        &[],
                                        &[stmt_expr(Expr::Call(
                                            Box::new(Expr::Var("__internal_resolve_promise".to_string(), None, None)),
                                            vec![
                                                Expr::Var("__result_promise".to_string(), None, None),
                                                Expr::Var("__orig_value".to_string(), None, None),
                                            ],
                                        ))],
                                        {
                                            let new_env = *env;
                                            object_set_key_value(mc, &new_env, "__result_promise", Value::Promise(result_promise))?;
                                            object_set_key_value(mc, &new_env, "__orig_value", orig_value.clone())?;
                                            Some(new_env)
                                        },
                                        None,
                                    ),
                                ))),
                                Some(Value::Closure(Gc::new(
                                    mc,
                                    ClosureData::new(
                                        &[],
                                        &[stmt_expr(Expr::Call(
                                            Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                                            vec![
                                                Expr::Var("__result_promise".to_string(), None, None),
                                                Expr::Var("__reason".to_string(), None, None),
                                            ],
                                        ))],
                                        {
                                            let new_env = *env;
                                            object_set_key_value(mc, &new_env, "__result_promise", Value::Promise(result_promise))?;
                                            Some(new_env)
                                        },
                                        None,
                                    ),
                                ))),
                                Some(result_promise),
                                env,
                            )?;
                            return Ok(Value::Undefined);
                        }
                        resolve_promise(mc, &result_promise, orig_value.clone(), env);
                    }
                    Err(e) => match e {
                        crate::core::EvalError::Throw(val, _, _) => {
                            reject_promise(mc, &result_promise, val, env);
                        }
                        _ => return Err(JSError::from(e)),
                    },
                }
            }
            // Not callable object: pass-through
            resolve_promise(mc, &result_promise, orig_value.clone(), env);
        }
        Value::Function(ref name) => {
            // call builtin function
            match crate::js_function::handle_global_function(mc, name, &[], env) {
                Ok(ret) => {
                    if let Value::Object(o) = ret
                        && let Some(inner_p) = get_promise_from_js_object(&o)
                    {
                        perform_promise_then(
                            mc,
                            inner_p,
                            Some(Value::Closure(Gc::new(
                                mc,
                                ClosureData::new(
                                    &[],
                                    &[stmt_expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_resolve_promise".to_string(), None, None)),
                                        vec![
                                            Expr::Var("__result_promise".to_string(), None, None),
                                            Expr::Var("__orig_value".to_string(), None, None),
                                        ],
                                    ))],
                                    {
                                        let new_env = *env;
                                        object_set_key_value(mc, &new_env, "__result_promise", Value::Promise(result_promise))?;
                                        object_set_key_value(mc, &new_env, "__orig_value", orig_value.clone())?;
                                        Some(new_env)
                                    },
                                    None,
                                ),
                            ))),
                            Some(Value::Closure(Gc::new(
                                mc,
                                ClosureData::new(
                                    &[],
                                    &[stmt_expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                                        vec![
                                            Expr::Var("__result_promise".to_string(), None, None),
                                            Expr::Var("__reason".to_string(), None, None),
                                        ],
                                    ))],
                                    {
                                        let new_env = *env;
                                        object_set_key_value(mc, &new_env, "__result_promise", Value::Promise(result_promise))?;
                                        Some(new_env)
                                    },
                                    None,
                                ),
                            ))),
                            Some(result_promise),
                            env,
                        )?;
                        return Ok(Value::Undefined);
                    }
                    resolve_promise(mc, &result_promise, orig_value, env);
                }
                Err(e) => return Err(e.into()),
            }
        }
        _ => {
            // not callable: pass-through
            resolve_promise(mc, &result_promise, orig_value, env);
        }
    }

    Ok(Value::Undefined)
}

pub fn __internal_promise_finally_reject<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // args: (reason, on_finally, result_promise)
    if args.len() < 3 {
        return Ok(Value::Undefined);
    }
    let orig_reason = args[0].clone();
    let on_finally = args[1].clone();
    let result_promise_val = args[2].clone();

    let result_promise = if let Value::Promise(p) = &result_promise_val {
        *p
    } else {
        return Ok(Value::Undefined);
    };

    match on_finally {
        Value::Closure(ref cl) => {
            match crate::core::call_closure(mc, cl, None, &[], env, None) {
                Ok(ret) => {
                    if let Value::Object(o) = ret
                        && let Some(inner_p) = get_promise_from_js_object(&o)
                    {
                        // When inner resolves -> pass-through original rejection
                        // When inner rejects -> reject with that reason
                        perform_promise_then(
                            mc,
                            inner_p,
                            Some(Value::Closure(Gc::new(
                                mc,
                                ClosureData::new(
                                    &[],
                                    &[stmt_expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                                        vec![
                                            Expr::Var("__result_promise".to_string(), None, None),
                                            Expr::Var("__orig_reason".to_string(), None, None),
                                        ],
                                    ))],
                                    {
                                        let new_env = *env;
                                        object_set_key_value(mc, &new_env, "__result_promise", Value::Promise(result_promise))?;
                                        object_set_key_value(mc, &new_env, "__orig_reason", orig_reason.clone())?;
                                        Some(new_env)
                                    },
                                    None,
                                ),
                            ))),
                            Some(Value::Closure(Gc::new(
                                mc,
                                ClosureData::new(
                                    &[],
                                    &[stmt_expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                                        vec![
                                            Expr::Var("__result_promise".to_string(), None, None),
                                            Expr::Var("__reason".to_string(), None, None),
                                        ],
                                    ))],
                                    {
                                        let new_env = *env;
                                        object_set_key_value(mc, &new_env, "__result_promise", Value::Promise(result_promise))?;
                                        Some(new_env)
                                    },
                                    None,
                                ),
                            ))),
                            Some(result_promise),
                            env,
                        )?;
                        return Ok(Value::Undefined);
                    }
                    // Not a promise: pass-through original reason
                    reject_promise(mc, &result_promise, orig_reason, env);
                }
                Err(e) => match e {
                    crate::core::EvalError::Throw(val, _, _) => {
                        reject_promise(mc, &result_promise, val, env);
                    }
                    _ => return Err(JSError::from(e)),
                },
            }
        }
        Value::Function(ref name) => match crate::js_function::handle_global_function(mc, name, &[], env) {
            Ok(ret) => {
                if let Value::Object(o) = ret
                    && let Some(inner_p) = get_promise_from_js_object(&o)
                {
                    perform_promise_then(
                        mc,
                        inner_p,
                        Some(Value::Closure(Gc::new(
                            mc,
                            ClosureData::new(
                                &[],
                                &[stmt_expr(Expr::Call(
                                    Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                                    vec![
                                        Expr::Var("__result_promise".to_string(), None, None),
                                        Expr::Var("__orig_reason".to_string(), None, None),
                                    ],
                                ))],
                                {
                                    let new_env = *env;
                                    object_set_key_value(mc, &new_env, "__result_promise", Value::Promise(result_promise))?;
                                    object_set_key_value(mc, &new_env, "__orig_reason", orig_reason.clone())?;
                                    Some(new_env)
                                },
                                None,
                            ),
                        ))),
                        Some(Value::Closure(Gc::new(
                            mc,
                            ClosureData::new(
                                &[],
                                &[stmt_expr(Expr::Call(
                                    Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                                    vec![
                                        Expr::Var("__result_promise".to_string(), None, None),
                                        Expr::Var("__reason".to_string(), None, None),
                                    ],
                                ))],
                                {
                                    let new_env = *env;
                                    object_set_key_value(mc, &new_env, "__result_promise", Value::Promise(result_promise))?;
                                    Some(new_env)
                                },
                                None,
                            ),
                        ))),
                        Some(result_promise),
                        env,
                    )?;
                    return Ok(Value::Undefined);
                }
                reject_promise(mc, &result_promise, orig_reason, env);
            }
            Err(e) => return Err(e.into()),
        },
        _ => {
            reject_promise(mc, &result_promise, orig_reason, env);
        }
    }

    Ok(Value::Undefined)
}

pub fn handle_promise_finally_val<'gc>(
    mc: &MutationContext<'gc>,
    promise: Gc<'gc, GcCell<JSPromise<'gc>>>,
    on_finally: Option<Value<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Create a new promise for chaining
    let new_promise = new_gc_cell_ptr(mc, JSPromise::new());
    let new_promise_obj = make_promise_js_object(mc, new_promise, Some(*env))?;

    // Prepare closure wrappers that will invoke the internal helpers
    let on_finally_val = on_finally.unwrap_or(Value::Undefined);

    let then_callback = Value::Closure(Gc::new(
        mc,
        ClosureData::new(
            &[DestructuringElement::Variable("value".to_string(), None)],
            &[stmt_expr(Expr::Call(
                Box::new(Expr::Var("__internal_promise_finally_resolve".to_string(), None, None)),
                vec![
                    Expr::Var("value".to_string(), None, None),
                    Expr::Var("__on_finally".to_string(), None, None),
                    Expr::Var("__result_promise".to_string(), None, None),
                ],
            ))],
            {
                let new_env = *env;
                object_set_key_value(mc, &new_env, "__on_finally", on_finally_val.clone())?;
                object_set_key_value(mc, &new_env, "__result_promise", Value::Promise(new_promise))?;
                Some(new_env)
            },
            None,
        ),
    ));

    let catch_callback = Value::Closure(Gc::new(
        mc,
        ClosureData::new(
            &[DestructuringElement::Variable("reason".to_string(), None)],
            &[stmt_expr(Expr::Call(
                Box::new(Expr::Var("__internal_promise_finally_reject".to_string(), None, None)),
                vec![
                    Expr::Var("reason".to_string(), None, None),
                    Expr::Var("__on_finally".to_string(), None, None),
                    Expr::Var("__result_promise".to_string(), None, None),
                ],
            ))],
            {
                let new_env = *env;
                object_set_key_value(mc, &new_env, "__on_finally", on_finally_val.clone())?;
                object_set_key_value(mc, &new_env, "__result_promise", Value::Promise(new_promise))?;
                Some(new_env)
            },
            None,
        ),
    ));

    perform_promise_then(mc, promise, Some(then_callback), Some(catch_callback), Some(new_promise), env)?;

    Ok(Value::Object(new_promise_obj))
}
