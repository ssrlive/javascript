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
    ClosureData, DestructuringElement, Expr, JSObjectDataPtr, Statement, StatementKind, Value, env_set, evaluate_expr, evaluate_statements,
    extract_closure_from_value, prepare_function_call_env, value_to_string,
};
use crate::core::{new_js_object_data, obj_get_key_value, obj_set_key_value};
use crate::error::JSError;

fn stmt_expr(expr: Expr) -> Statement {
    Statement::from(StatementKind::Expr(expr))
}

fn stmt_return(expr: Option<Expr>) -> Statement {
    Statement::from(StatementKind::Return(expr))
}

use crate::js_array::set_array_length;
use crate::unicode::utf8_to_utf16;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Asynchronous task types for the promise event loop.
///
/// Tasks represent deferred callback execution to maintain JavaScript's
/// asynchronous behavior where promise callbacks are always executed
/// asynchronously, even when the promise is already settled.
#[derive(Clone, Debug)]
enum Task {
    /// Task to execute fulfilled callbacks with the resolved value
    Resolution {
        promise: Rc<RefCell<JSPromise>>,
        callbacks: Vec<(Value, Rc<RefCell<JSPromise>>, Option<JSObjectDataPtr>)>,
    },
    /// Task to execute rejected callbacks with the rejection reason
    Rejection {
        promise: Rc<RefCell<JSPromise>>,
        callbacks: Vec<(Value, Rc<RefCell<JSPromise>>, Option<JSObjectDataPtr>)>,
    },
    /// Task to execute a setTimeout callback
    Timeout {
        id: usize,
        callback: Value,
        args: Vec<Value>,
        target_time: Instant,
    },
    /// Task to execute a setInterval callback
    Interval {
        id: usize,
        callback: Value,
        args: Vec<Value>,
        target_time: Instant,
        interval: Duration,
    },
    /// Task to check for unhandled rejection after potential handler attachment
    UnhandledCheck { promise: Rc<RefCell<JSPromise>>, reason: Value },
    // Previously this variant represented a queued unhandled-check task.
    // Unhandled checks are now tracked separately in `PENDING_UNHANDLED_CHECKS`.
    // NOTE: Unhandled checks are now tracked in `PENDING_UNHANDLED_CHECKS`
    // rather than as queued tasks. Keeping the task enum slimmer avoids
    // accidental re-processing within the same run. The pending list is
    // processed once when the outermost `run_event_loop` finishes.
}

/// Take any recorded unhandled rejection, consuming it from the thread-local
/// storage. Returns `Some(Value)` if an unhandled rejection was recorded.
pub fn take_unhandled_rejection() -> Option<Value> {
    UNHANDLED_REJECTION.with(|slot| slot.borrow_mut().take())
}

/// Peek at any recorded unhandled rejection without consuming it.
pub fn peek_unhandled_rejection() -> Option<Value> {
    UNHANDLED_REJECTION.with(|slot| slot.borrow().clone())
}

/// Return the number of pending unhandled checks awaiting processing.
pub fn pending_unhandled_count() -> usize {
    // Count entries in the pending-unhandled list
    PENDING_UNHANDLED_CHECKS.with(|q| q.borrow().len())
}

/// Return the current number of queued tasks in the global task queue.
pub fn task_queue_len() -> usize {
    GLOBAL_TASK_QUEUE.with(|q| q.borrow().len())
}

/// Return the current monotonic tick value (for debugging/inspection)
pub fn current_tick() -> usize {
    CURRENT_TICK.load(Ordering::SeqCst)
}
thread_local! {
    /// Global task queue for asynchronous promise operations.
    /// Uses thread-local storage to maintain separate queues per thread.
    /// This enables proper asynchronous execution of promise callbacks.
    static GLOBAL_TASK_QUEUE: RefCell<Vec<Task>> = const { RefCell::new(Vec::new()) };

    /// Global storage for AllSettledState instances during Promise.allSettled execution
    static ALLSETTLED_STATES: RefCell<Vec<Rc<RefCell<AllSettledState>>>> = const { RefCell::new(Vec::new()) };

    /// Counter for generating unique timeout IDs
    static NEXT_TIMEOUT_ID: RefCell<usize> = const { RefCell::new(1) };
    /// Storage for an unhandled rejection detected by the UnhandledCheck task
    static UNHANDLED_REJECTION: RefCell<Option<Value>> = const { RefCell::new(None) };
    /// Pending unhandled checks queued by `reject_promise` when there are no
    /// attached rejection handlers at the time of rejection. Each entry
    /// stores the tuple `(promise, reason, insertion_tick)` where
    /// `insertion_tick` is the value of `CURRENT_TICK` when the rejection
    /// was recorded. The pending list is processed only once per outermost
    /// idle tick; an entry is treated as unhandled when
    /// `CURRENT_TICK >= insertion_tick + UNHANDLED_GRACE`.
    #[allow(clippy::type_complexity)]
    static PENDING_UNHANDLED_CHECKS: RefCell<Vec<(Rc<RefCell<JSPromise>>, Value, usize)>> = const { RefCell::new(Vec::new()) };
}

/// Tracks how many nested invocations of the promise event loop are active.
/// When >1 we are in a nested/inline run and should defer UnhandledCheck
/// processing to the outermost loop to avoid premature unhandled reports.
static RUN_LOOP_NESTING: AtomicUsize = AtomicUsize::new(0);

/// Monotonic tick counter advanced once per outermost idle event-loop tick.
/// Pending unhandled checks record the insertion tick and are considered
/// unhandled only when `CURRENT_TICK >= insertion_tick + UNHANDLED_GRACE`.
static CURRENT_TICK: AtomicUsize = AtomicUsize::new(0);

/// Number of outermost idle ticks to wait before treating a rejection as
/// unhandled. This provides a small grace window for handlers to attach.
/// Increased to give harnesses additional time to attach handlers in
/// high-latency or deeply-nested synchronous scenarios.
const UNHANDLED_GRACE: usize = 6;

/// Add a task to the global task queue for later execution.
///
/// # Arguments
/// * `task` - The task to queue (Resolution or Rejection)
fn queue_task(task: Task) {
    log::debug!("queue_task called with {:?}", task);
    // Also log current run-loop nesting for correlation
    let nesting = RUN_LOOP_NESTING.load(Ordering::SeqCst);
    log::trace!("queue_task: current RUN_LOOP_NESTING={}", nesting);
    // Log tick and current queue length to help debug ordering with console.log
    log::debug!("queue_task: CURRENT_TICK={} task_queue_len={}", current_tick(), task_queue_len());
    GLOBAL_TASK_QUEUE.with(|queue| {
        queue.borrow_mut().push(task);
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
fn process_task(task: Task) -> Result<(), JSError> {
    match task {
        Task::Resolution { promise, callbacks } => {
            log::trace!("Processing Resolution task with {} callbacks", callbacks.len());
            for (callback, new_promise, caller_env_opt) in callbacks {
                // Call the callback and resolve the new promise with the result
                if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                    let args = vec![promise.borrow().value.clone().unwrap_or(Value::Undefined)];
                    let func_env =
                        prepare_function_call_env(Some(&captured_env), None, Some(&params), &args, None, caller_env_opt.as_ref())?;
                    match evaluate_statements(&func_env, &body) {
                        Ok(result) => {
                            log::trace!("Callback executed successfully, resolving promise");
                            resolve_promise(&new_promise, result);
                        }
                        Err(e) => {
                            log::trace!("Callback execution failed: {:?}", e);
                            // If the callback threw a JS value, propagate that value
                            // as the rejection reason. Otherwise fall back to stringifying
                            // the error for the rejection reason.
                            if let crate::error::JSErrorKind::Throw { value } = e.kind() {
                                reject_promise(&new_promise, value.clone());
                            } else {
                                reject_promise(&new_promise, Value::String(utf8_to_utf16(&format!("{:?}", e))));
                            }
                        }
                    }
                } else {
                    // If callback is not a function, resolve with undefined
                    log::trace!("Callback is not a function, resolving with undefined");
                    resolve_promise(&new_promise, Value::Undefined);
                }
            }
        }
        Task::Rejection { promise, callbacks } => {
            log::trace!("Processing Rejection task with {} callbacks", callbacks.len());
            for (callback, new_promise, caller_env_opt) in callbacks {
                // Call the callback and resolve the new promise with the result
                if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                    let args = vec![promise.borrow().value.clone().unwrap_or(Value::Undefined)];
                    let func_env =
                        prepare_function_call_env(Some(&captured_env), None, Some(&params), &args, None, caller_env_opt.as_ref())?;
                    match evaluate_statements(&func_env, &body) {
                        Ok(result) => {
                            resolve_promise(&new_promise, result);
                        }
                        Err(e) => {
                            if let crate::error::JSErrorKind::Throw { value } = e.kind() {
                                reject_promise(&new_promise, value.clone());
                            } else {
                                reject_promise(&new_promise, Value::String(utf8_to_utf16(&format!("{:?}", e))));
                            }
                        }
                    }
                } else {
                    // If callback is not a function, resolve with undefined
                    resolve_promise(&new_promise, Value::Undefined);
                }
            }
        }
        Task::Timeout { id: _, callback, args, .. } => {
            log::trace!("Processing Timeout task");
            // Call the callback with the provided args
            if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                // If callback is a standard function (Value::Object), bind `this` to global.
                // Arrow functions (Value::Closure) should inherit `this` from captured_env.
                let this_val_opt = if let Value::Object(_) = callback {
                    let mut global_env = captured_env.clone();
                    while let Some(proto) = global_env.clone().borrow().prototype.clone() {
                        global_env = proto;
                    }
                    Some(Value::Object(global_env))
                } else {
                    None
                };

                let func_env = prepare_function_call_env(Some(&captured_env), this_val_opt, Some(&params), &args, None, None)?;
                let _ = evaluate_statements(&func_env, &body)?;
            }
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
                let this_val_opt = if let Value::Object(_) = callback {
                    let mut global_env = captured_env.clone();
                    while let Some(proto) = global_env.clone().borrow().prototype.clone() {
                        global_env = proto;
                    }
                    Some(Value::Object(global_env))
                } else {
                    None
                };

                let func_env = prepare_function_call_env(Some(&captured_env), this_val_opt, Some(&params), &args, None, None)?;
                let _ = evaluate_statements(&func_env, &body)?;

                // Re-queue the interval task
                queue_task(Task::Interval {
                    id,
                    callback: callback.clone(),
                    args: args.clone(),
                    target_time: Instant::now() + interval,
                    interval,
                });
            }
        }
        Task::UnhandledCheck { promise, reason } => {
            log::trace!("Processing UnhandledCheck task for promise ptr={:p}", Rc::as_ptr(&promise));
            // Check if the promise still has no rejection handlers
            let promise_borrow = promise.borrow();
            if promise_borrow.on_rejected.is_empty() {
                // Still no handlers: record insertion tick for later processing
                let insertion_tick = CURRENT_TICK.load(Ordering::SeqCst);
                log::trace!(
                    "UnhandledCheck: adding to PENDING_UNHANDLED_CHECKS for promise ptr={:p} insertion_tick={}",
                    Rc::as_ptr(&promise),
                    insertion_tick
                );
                PENDING_UNHANDLED_CHECKS.with(|pending| {
                    pending.borrow_mut().push((promise.clone(), reason, insertion_tick));
                });
            } else {
                log::trace!("UnhandledCheck: handlers attached, skipping unhandled recording");
            }
        }
    }
    Ok(())
}

/// Poll the event loop for a single task.
///
/// This function checks the task queue and executes the first ready task.
/// If no tasks are ready but timers are pending, it returns `PollResult::Wait`.
/// If the queue is empty, it returns `PollResult::Empty`.
pub fn poll_event_loop() -> Result<PollResult, JSError> {
    let now = Instant::now();
    let (task, should_sleep) = GLOBAL_TASK_QUEUE.with(|queue| {
        let mut queue_borrow = queue.borrow_mut();
        if queue_borrow.is_empty() {
            return (None, None);
        }

        let mut ready_index = None;
        let mut min_wait_time: Option<Duration> = None;

        for (i, task) in queue_borrow.iter().enumerate() {
            match task {
                Task::Timeout { target_time, .. } | Task::Interval { target_time, .. } => {
                    if *target_time <= now {
                        ready_index = Some(i);
                        break;
                    } else {
                        let wait = *target_time - now;
                        min_wait_time = Some(min_wait_time.map_or(wait, |m| m.min(wait)));
                    }
                }
                _ => {
                    ready_index = Some(i);
                    break;
                }
            }
        }

        if let Some(index) = ready_index {
            (Some(queue_borrow.remove(index)), None)
        } else {
            (None, min_wait_time)
        }
    });

    if let Some(task) = task {
        process_task(task)?;
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
pub fn run_event_loop() -> Result<PollResult, JSError> {
    log::trace!("run_event_loop called");
    // Mark that we're entering an event-loop run (may be nested).
    let nesting_before = RUN_LOOP_NESTING.fetch_add(1, Ordering::SeqCst);
    log::debug!(
        "run_event_loop: incremented RUN_LOOP_NESTING from {} to {}",
        nesting_before,
        nesting_before + 1
    );

    let result = poll_event_loop()?;
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
        PENDING_UNHANDLED_CHECKS.with(|pending| {
            let mut pending_borrow = pending.borrow_mut();
            if !pending_borrow.is_empty() {
                log::trace!(
                    "Processing PENDING_UNHANDLED_CHECKS: len={} current={}",
                    pending_borrow.len(),
                    current
                );
                let mut new_pending: Vec<(Rc<RefCell<JSPromise>>, Value, usize)> = Vec::new();
                // Drain current list and decide whether to record or re-queue
                for (promise, reason, insertion_tick) in pending_borrow.drain(..) {
                    let promise_ptr = Rc::as_ptr(&promise);
                    log::trace!(
                        "pending entry: promise ptr={:p} insertion_tick={} expires_at={}",
                        promise_ptr,
                        insertion_tick,
                        insertion_tick + UNHANDLED_GRACE
                    );
                    let promise_b = promise.borrow();
                    match &promise_b.state {
                        PromiseState::Rejected(_val) => {
                            if !promise_b.on_rejected.is_empty() {
                                // Handler attached; do not record or re-queue
                                log::trace!("handler attached for promise ptr={:p}, ignoring", promise_ptr);
                                continue;
                            }
                            if current >= insertion_tick + UNHANDLED_GRACE {
                                log::debug!("pending expired -> recording unhandled for promise ptr={:p}", promise_ptr);
                                // Record the unhandled rejection if slot empty
                                UNHANDLED_REJECTION.with(|slot| {
                                    let mut s = slot.borrow_mut();
                                    if s.is_none() {
                                        *s = Some(reason.clone());
                                    }
                                });
                            } else {
                                // Not yet timed out; keep for later
                                log::trace!("pending not yet expired -> requeue promise ptr={:p}", promise_ptr);
                                new_pending.push((promise.clone(), reason.clone(), insertion_tick));
                            }
                        }
                        _ => {
                            // Not rejected anymore; ignore
                            log::trace!("promise ptr={:p} no longer rejected, ignoring", promise_ptr);
                        }
                    }
                }
                *pending_borrow = new_pending;
            }
        });
    }

    // Leaving this run: decrement nesting
    RUN_LOOP_NESTING.fetch_sub(1, Ordering::SeqCst);
    Ok(result)
}

/// Represents the current state of a JavaScript Promise.
///
/// Promises transition through these states exactly once:
/// Pending → Fulfilled (with a value), or
/// Pending → Rejected (with a reason)
#[derive(Clone, Debug, Default)]
pub enum PromiseState {
    #[default]
    Pending,
    Fulfilled(Value),
    Rejected(Value),
}

/// Core JavaScript Promise structure.
///
/// Maintains the promise's current state and manages callback queues
/// for then/catch/finally chaining.
#[derive(Clone, Default)]
pub struct JSPromise {
    pub state: PromiseState,
    pub value: Option<Value>, // The resolved value or rejection reason
    pub on_fulfilled: Vec<(Value, Rc<RefCell<JSPromise>>, Option<JSObjectDataPtr>)>, // Callbacks and their chaining promises + optional caller env
    pub on_rejected: Vec<(Value, Rc<RefCell<JSPromise>>, Option<JSObjectDataPtr>)>, // Callbacks and their chaining promises + optional caller env
}

/// Represents the result of a settled promise in Promise.allSettled
#[derive(Clone, Debug)]
pub enum SettledResult {
    /// Promise was fulfilled with a value
    Fulfilled(Value),
    /// Promise was rejected with a reason
    Rejected(Value),
}

/// Dedicated state structure for Promise.allSettled coordination
///
/// This replaces the previous shared JS object approach with a type-safe
/// Rust structure, eliminating the need for string-based property access
/// and providing better compile-time guarantees.
///
/// # Fields
/// * `results` - Array of settled results, indexed by original promise position
/// * `completed` - Number of promises that have settled (fulfilled or rejected)
/// * `total` - Total number of promises being tracked
/// * `result_promise` - The main Promise.allSettled promise to resolve when all settle
#[derive(Clone, Debug)]
pub struct AllSettledState {
    pub results: Vec<Option<SettledResult>>,
    pub completed: usize,
    pub total: usize,
    pub result_promise: Rc<RefCell<JSPromise>>,
    pub env: JSObjectDataPtr,
}

impl AllSettledState {
    /// Create a new AllSettledState for tracking multiple promises
    ///
    /// # Arguments
    /// * `total` - Number of promises to track
    /// * `result_promise` - The promise to resolve when all promises settle
    /// * `env` - The environment to create arrays in
    pub fn new(total: usize, result_promise: Rc<RefCell<JSPromise>>, env: JSObjectDataPtr) -> Self {
        AllSettledState {
            results: vec![None; total],
            completed: 0,
            total,
            result_promise,
            env,
        }
    }

    /// Record that a promise at the given index has been fulfilled
    ///
    /// # Arguments
    /// * `index` - Index of the promise in the original array
    /// * `value` - The fulfilled value
    pub fn record_fulfilled(&mut self, index: usize, value: Value) -> Result<(), JSError> {
        if index < self.results.len() {
            self.results[index] = Some(SettledResult::Fulfilled(value));
            self.completed += 1;
            self.check_completion()?;
        }
        Ok(())
    }

    /// Record that a promise at the given index has been rejected
    ///
    /// # Arguments
    /// * `index` - Index of the promise in the original array
    /// * `reason` - The rejection reason
    pub fn record_rejected(&mut self, index: usize, reason: Value) -> Result<(), JSError> {
        if index < self.results.len() {
            self.results[index] = Some(SettledResult::Rejected(reason));
            self.completed += 1;
            self.check_completion()?;
        }
        Ok(())
    }

    /// Check if all promises have settled and resolve the result promise if so
    fn check_completion(&self) -> Result<(), JSError> {
        log::trace!("check_completion: completed={}, total={}", self.completed, self.total);
        if self.completed == self.total {
            log::trace!("All promises settled, resolving result promise");
            // All promises have settled, create the result array
            let result_array = crate::js_array::create_array(&self.env)?;

            for (i, result) in self.results.iter().enumerate() {
                if let Some(settled_result) = result {
                    let result_obj = Rc::new(RefCell::new(crate::core::JSObjectData::new()));

                    match settled_result {
                        SettledResult::Fulfilled(value) => {
                            obj_set_key_value(&result_obj, &"status".into(), Value::String(utf8_to_utf16("fulfilled")))?;
                            obj_set_key_value(&result_obj, &"value".into(), value.clone())?;
                        }
                        SettledResult::Rejected(reason) => {
                            obj_set_key_value(&result_obj, &"status".into(), Value::String(utf8_to_utf16("rejected")))?;
                            obj_set_key_value(&result_obj, &"reason".into(), reason.clone())?;
                        }
                    }

                    obj_set_key_value(&result_array, &i.to_string().into(), Value::Object(result_obj))?;
                }
            }

            // Set the length property for array compatibility
            set_array_length(&result_array, self.total)?;

            // Resolve the main promise with the results array
            log::trace!("Resolving allSettled result promise");
            resolve_promise(&self.result_promise, Value::Object(result_array));
        }
        Ok(())
    }
}

impl JSPromise {
    /// Create a new promise in the pending state.
    pub fn new() -> Self {
        JSPromise {
            state: PromiseState::Pending,
            value: None,
            on_fulfilled: Vec::new(),
            on_rejected: Vec::new(),
        }
    }
}

impl std::fmt::Debug for JSPromise {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "JSPromise {{ state: {:?}, on_fulfilled: {}, on_rejected: {} }}",
            self.state,
            self.on_fulfilled.len(),
            self.on_rejected.len()
        )
    }
}

/// Create a new JavaScript Promise object with prototype methods.
///
/// This function creates a JS object that wraps a JSPromise instance and
/// attaches the standard Promise prototype methods (then, catch, finally).
///
/// # Returns
/// * `Result<JSObjectDataPtr, JSError>` - The promise object or creation error
pub fn make_promise_object() -> Result<JSObjectDataPtr, JSError> {
    let promise_obj = Rc::new(RefCell::new(crate::core::JSObjectData::new()));

    // Add then method
    let then_func = Value::Function("Promise.prototype.then".to_string());
    obj_set_key_value(&promise_obj, &"then".into(), then_func)?;

    // Add catch method
    let catch_func = Value::Function("Promise.prototype.catch".to_string());
    obj_set_key_value(&promise_obj, &"catch".into(), catch_func)?;
    // Add finally method
    let finally_func = Value::Function("Promise.prototype.finally".to_string());
    obj_set_key_value(&promise_obj, &"finally".into(), finally_func)?;

    Ok(promise_obj)
}

/// Handle JavaScript Promise constructor calls (new Promise(executor)).
///
/// Creates a new promise and executes the executor function with resolve/reject
/// functions. The executor typically initiates asynchronous operations.
///
/// # Arguments
/// * `args` - Constructor arguments (should contain executor function)
/// * `env` - Current execution environment
///
/// # Returns
/// * `Result<Value, JSError>` - The promise object or construction error
///
/// # Example
/// ```javascript
/// new Promise((resolve, reject) => {
///   setTimeout(() => resolve("done"), 1000);
/// });
/// ```
pub fn handle_promise_constructor(args: &[crate::core::Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    handle_promise_constructor_direct(args, env)
}

/// Direct promise constructor that operates without abstraction layers
///
/// # Arguments
/// * `args` - Constructor arguments (should contain executor function)
/// * `env` - Current execution environment
///
/// # Returns
/// * `Result<Value, JSError>` - The promise object or construction error
pub fn handle_promise_constructor_direct(args: &[crate::core::Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        return Err(raise_eval_error!("Promise constructor requires an executor function"));
    }

    let executor = evaluate_expr(env, &args[0])?;
    let (params, captured_env) = if let Some((p, _body, c)) = extract_closure_from_value(&executor) {
        (p.clone(), c.clone())
    } else {
        return Err(raise_eval_error!("Promise constructor requires a function as executor"));
    };

    // Create the promise directly
    let promise = Rc::new(RefCell::new(JSPromise::new()));
    let promise_obj = make_promise_object()?;
    obj_set_key_value(&promise_obj, &"__promise".into(), Value::Promise(promise.clone()))?;

    // Create resolve and reject functions directly
    let resolve_func = create_resolve_function_direct(promise.clone());
    let reject_func = create_reject_function_direct(promise.clone());

    // Create executor function environment and bind resolve/reject into params
    let executor_args = vec![resolve_func.clone(), reject_func.clone()];
    let executor_env = if params.is_empty() {
        crate::core::prepare_function_call_env(Some(&captured_env), None, None, &[], None, None)?
    } else {
        crate::core::prepare_function_call_env(Some(&captured_env), None, Some(&params), &executor_args, None, None)?
    };

    log::trace!("About to call executor function");
    // Execute the executor function by calling it
    let call_expr = Expr::Call(
        Box::new(Expr::Value(executor)),
        vec![Expr::Value(resolve_func), Expr::Value(reject_func)],
    );
    match evaluate_expr(&executor_env, &call_expr) {
        Ok(_) => {}
        Err(e) => {
            // If the executor threw a JS value, the Promise constructor must
            // reject the newly created promise with that value instead of
            // re-throwing to the host.
            if let crate::error::JSErrorKind::Throw { value } = e.kind() {
                crate::js_promise::reject_promise(&promise, value.clone());
            } else {
                return Err(e);
            }
        }
    }
    log::trace!("Executor function called");

    Ok(Value::Object(promise_obj))
}

/// Create a resolve function for Promise executor (direct version).
///
/// This function creates a closure that, when called, will resolve the promise
/// with the provided value. It's passed to the executor function as the first parameter.
///
/// # Arguments
/// * `promise` - The promise to resolve
///
/// # Returns
/// * `Value` - A closure that resolves the promise when called
fn create_resolve_function_direct(promise: Rc<RefCell<JSPromise>>) -> Value {
    log::trace!("create_resolve_function_direct called");
    let closure_data = ClosureData::new(
        &[DestructuringElement::Variable("value".to_string(), None)],
        &[stmt_expr(Expr::Call(
            Box::new(Expr::Var("__internal_resolve_promise".to_string(), None, None)),
            vec![
                Expr::Var("__captured_promise".to_string(), None, None),
                Expr::Var("value".to_string(), None, None),
            ],
        ))],
        &{
            let closure_env = new_js_object_data();
            env_set(&closure_env, "__captured_promise", Value::Promise(promise)).unwrap();
            closure_env
        },
        None,
    );
    Value::Closure(Rc::new(closure_data))
}

/// Create a reject function for Promise executor (direct version).
///
/// This function creates a closure that, when called, will reject the promise
/// with the provided reason. It's passed to the executor function as the second parameter.
///
/// # Arguments
/// * `promise` - The promise to reject
///
/// # Returns
/// * `Value` - A closure that rejects the promise when called
fn create_reject_function_direct(promise: Rc<RefCell<JSPromise>>) -> Value {
    log::trace!("create_reject_function_direct called");

    let closure_data = ClosureData::new(
        &[DestructuringElement::Variable("reason".to_string(), None)],
        &[stmt_expr(Expr::Call(
            Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
            vec![
                Expr::Var("__captured_promise".to_string(), None, None),
                Expr::Var("reason".to_string(), None, None),
            ],
        ))],
        &{
            let env = new_js_object_data();
            env_set(&env, "__captured_promise", Value::Promise(promise)).unwrap();
            env
        },
        None,
    );
    Value::Closure(Rc::new(closure_data))
}

/// Attaches fulfillment and rejection handlers to the promise, returning a new
/// promise that resolves/rejects based on the callback return values.
///
/// # Arguments
/// * `promise_obj` - The promise object to attach handlers to
/// * `args` - Method arguments (onFulfilled, onRejected callbacks)
/// * `env` - Current execution environment
///
/// # Returns
/// * `Result<Value, JSError>` - New promise for chaining or error
///
/// # Behavior
/// - If promise is already fulfilled, queues callback for async execution
/// - If promise is already rejected, does nothing (catch handles this)
/// - Returns a new promise that resolves with callback return value
pub fn handle_promise_then(promise_obj: &JSObjectDataPtr, args: &[crate::core::Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Get the underlying promise
    let promise_val = obj_get_key_value(promise_obj, &"__promise".into())?;
    let promise = match promise_val {
        Some(val_rc) => {
            let val = val_rc.borrow();
            match &*val {
                Value::Promise(p) => p.clone(),
                _ => {
                    return Err(raise_eval_error!("Invalid promise object"));
                }
            }
        }
        _ => {
            return Err(raise_eval_error!("Invalid promise object"));
        }
    };

    handle_promise_then_direct(promise, args, env)
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
pub fn handle_promise_then_direct(promise: Rc<RefCell<JSPromise>>, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Create a new promise for chaining
    let new_promise = Rc::new(RefCell::new(JSPromise::new()));
    let new_promise_obj = make_promise_object()?;
    obj_set_key_value(&new_promise_obj, &"__promise".into(), Value::Promise(new_promise.clone()))?;

    // Get the onFulfilled callback
    let on_fulfilled = if !args.is_empty() {
        Some(evaluate_expr(env, &args[0])?)
    } else {
        None
    };

    // Get the onRejected callback
    let on_rejected = if args.len() > 1 {
        Some(evaluate_expr(env, &args[1])?)
    } else {
        None
    };

    // Add to the promise's callback lists
    let mut promise_borrow = promise.borrow_mut();
    if let Some(ref callback) = on_fulfilled {
        promise_borrow
            .on_fulfilled
            .push((callback.clone(), new_promise.clone(), Some(env.clone())));
    } else {
        // Add pass-through for fulfillment
        let closure_data = ClosureData::new(
            &[DestructuringElement::Variable("value".to_string(), None)],
            &[stmt_expr(Expr::Call(
                Box::new(Expr::Var("__internal_resolve_promise".to_string(), None, None)),
                vec![
                    Expr::Var("__new_promise".to_string(), None, None),
                    Expr::Var("value".to_string(), None, None),
                ],
            ))],
            &{
                let env = new_js_object_data();
                env_set(&env, "__new_promise", Value::Promise(new_promise.clone())).unwrap();
                env
            },
            None,
        );
        let pass_through_fulfill = Value::Closure(Rc::new(closure_data));
        promise_borrow
            .on_fulfilled
            .push((pass_through_fulfill, new_promise.clone(), Some(env.clone())));
    }

    if let Some(ref callback) = on_rejected {
        promise_borrow
            .on_rejected
            .push((callback.clone(), new_promise.clone(), Some(env.clone())));
    } else {
        // Add pass-through for rejection
        let closure_data = ClosureData::new(
            &[DestructuringElement::Variable("reason".to_string(), None)],
            &[stmt_expr(Expr::Call(
                Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                vec![
                    Expr::Var("__new_promise".to_string(), None, None),
                    Expr::Var("reason".to_string(), None, None),
                ],
            ))],
            &{
                let env = new_js_object_data();
                env_set(&env, "__new_promise", Value::Promise(new_promise.clone())).unwrap();
                env
            },
            None,
        );

        let pass_through_reject = Value::Closure(Rc::new(closure_data));
        promise_borrow
            .on_rejected
            .push((pass_through_reject, new_promise.clone(), Some(env.clone())));
    }

    // If promise is already settled, queue task to execute callback asynchronously
    match &promise_borrow.state {
        PromiseState::Fulfilled(val) => {
            if let Some(ref callback) = on_fulfilled {
                // Queue task to execute callback asynchronously
                queue_task(Task::Resolution {
                    promise: promise.clone(),
                    callbacks: vec![(callback.clone(), new_promise.clone(), Some(env.clone()))],
                });
            } else {
                // No callback, resolve with the original value
                resolve_promise(&new_promise, val.clone());
            }
        }
        PromiseState::Rejected(val) => {
            if let Some(ref callback) = on_rejected {
                // Queue task to execute callback asynchronously
                queue_task(Task::Rejection {
                    promise: promise.clone(),
                    callbacks: vec![(callback.clone(), new_promise.clone(), Some(env.clone()))],
                });
            } else {
                // No callback, reject with the original reason
                reject_promise(&new_promise, val.clone());
            }
        }
        _ => {}
    }

    Ok(Value::Object(new_promise_obj))
}

/// Handle Promise.prototype.catch() method calls.
///
/// Attaches a rejection handler to the promise, returning a new promise.
/// Unlike then(), catch() only handles rejections and passes through fulfillments.
///
/// # Arguments
/// * `promise_obj` - The promise object to attach handler to
/// * `args` - Method arguments (onRejected callback)
/// * `env` - Current execution environment
///
/// # Returns
/// * `Result<Value, JSError>` - New promise for chaining or error
///
/// # Behavior
/// - If promise is already rejected, queues callback for async execution
/// - If promise is already fulfilled, resolves new promise with original value
/// - Returns a new promise that resolves with callback return value
pub fn handle_promise_catch(promise_obj: &JSObjectDataPtr, args: &[crate::core::Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Get the underlying promise
    let promise_val = obj_get_key_value(promise_obj, &"__promise".into())?;
    let promise = match promise_val {
        Some(val_rc) => {
            let val = val_rc.borrow();
            match &*val {
                Value::Promise(p) => p.clone(),
                _ => {
                    return Err(raise_eval_error!("Invalid promise object"));
                }
            }
        }
        _ => {
            return Err(raise_eval_error!("Invalid promise object"));
        }
    };

    handle_promise_catch_direct(promise, args, env)
}

/// Direct catch handler that operates on JSPromise directly
///
/// # Arguments
/// * `promise` - The promise to attach handler to
/// * `args` - Method arguments (onRejected callback)
/// * `env` - Current execution environment
///
/// # Returns
/// * `Result<Value, JSError>` - New promise for chaining or error
pub fn handle_promise_catch_direct(
    promise: Rc<RefCell<JSPromise>>,
    args: &[crate::core::Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    // Create a new promise for chaining
    let new_promise = Rc::new(RefCell::new(JSPromise::new()));
    let new_promise_obj = make_promise_object()?;
    obj_set_key_value(&new_promise_obj, &"__promise".into(), Value::Promise(new_promise.clone()))?;

    // Get the onRejected callback
    let on_rejected = if !args.is_empty() {
        Some(evaluate_expr(env, &args[0])?)
    } else {
        None
    };

    // Add to the promise's callback lists
    let mut promise_borrow = promise.borrow_mut();
    // Add pass-through for fulfillment
    let closure_data = ClosureData::new(
        &[DestructuringElement::Variable("value".to_string(), None)],
        &[stmt_expr(Expr::Call(
            Box::new(Expr::Var("__internal_resolve_promise".to_string(), None, None)),
            vec![
                Expr::Var("__new_promise".to_string(), None, None),
                Expr::Var("value".to_string(), None, None),
            ],
        ))],
        &{
            let env = new_js_object_data();
            env_set(&env, "__new_promise", Value::Promise(new_promise.clone())).unwrap();
            env
        },
        None,
    );

    let pass_through_fulfill = Value::Closure(Rc::new(closure_data));
    promise_borrow
        .on_fulfilled
        .push((pass_through_fulfill, new_promise.clone(), Some(env.clone())));

    if let Some(ref callback) = on_rejected {
        promise_borrow
            .on_rejected
            .push((callback.clone(), new_promise.clone(), Some(env.clone())));
    } else {
        // Add pass-through for rejection
        let closure_data = ClosureData::new(
            &[DestructuringElement::Variable("reason".to_string(), None)],
            &[stmt_expr(Expr::Call(
                Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                vec![
                    Expr::Var("__new_promise".to_string(), None, None),
                    Expr::Var("reason".to_string(), None, None),
                ],
            ))],
            &{
                let env = new_js_object_data();
                env_set(&env, "__new_promise", Value::Promise(new_promise.clone())).unwrap();
                env
            },
            None,
        );
        let pass_through_reject = Value::Closure(Rc::new(closure_data));
        promise_borrow
            .on_rejected
            .push((pass_through_reject, new_promise.clone(), Some(env.clone())));
    }

    // If promise is already settled, queue task to execute callback asynchronously
    match &promise_borrow.state {
        PromiseState::Rejected(val) => {
            if let Some(ref callback) = on_rejected {
                // Queue task to execute callback asynchronously
                queue_task(Task::Rejection {
                    promise: promise.clone(),
                    callbacks: vec![(callback.clone(), new_promise.clone(), Some(env.clone()))],
                });
            } else {
                // No callback, reject the new promise with the same reason
                reject_promise(&new_promise, val.clone());
            }
        }
        PromiseState::Fulfilled(val) => {
            // For catch, if already fulfilled, resolve the new promise with the value
            resolve_promise(&new_promise, val.clone());
        }
        _ => {}
    }

    Ok(Value::Object(new_promise_obj))
}

/// Handle Promise.prototype.finally() method calls.
///
/// Attaches a cleanup handler that executes regardless of promise outcome.
/// The finally callback receives no arguments and its return value is ignored.
///
/// # Arguments
/// * `promise_obj` - The promise object to attach handler to
/// * `args` - Method arguments (onFinally callback)
/// * `env` - Current execution environment
///
/// # Returns
/// * `Result<Value, JSError>` - New promise for chaining or error
///
/// # Behavior
/// - Creates a callback that executes finally handler then returns original value
/// - Attaches same callback to both fulfillment and rejection queues
/// - If promise already settled, queues callback for async execution
pub fn handle_promise_finally(promise_obj: &JSObjectDataPtr, args: &[crate::core::Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Get the underlying promise
    let promise_val = obj_get_key_value(promise_obj, &"__promise".into())?;
    let promise = match promise_val {
        Some(val_rc) => {
            let val = val_rc.borrow();
            match &*val {
                Value::Promise(p) => p.clone(),
                _ => {
                    return Err(raise_eval_error!("Invalid promise object"));
                }
            }
        }
        _ => {
            return Err(raise_eval_error!("Invalid promise object"));
        }
    };

    handle_promise_finally_direct(promise, args, env)
}

/// Direct finally handler that operates on JSPromise directly
///
/// # Arguments
/// * `promise` - The promise to attach handler to
/// * `args` - Method arguments (onFinally callback)
/// * `env` - Current execution environment
///
/// # Returns
/// * `Result<Value, JSError>` - New promise for chaining or error
pub fn handle_promise_finally_direct(
    promise: Rc<RefCell<JSPromise>>,
    args: &[crate::core::Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    // Create a new promise for chaining
    let new_promise = Rc::new(RefCell::new(JSPromise::new()));
    let new_promise_obj = make_promise_object()?;
    obj_set_key_value(&new_promise_obj, &"__promise".into(), Value::Promise(new_promise.clone()))?;

    // Get the onFinally callback
    let on_finally = if !args.is_empty() {
        Some(evaluate_expr(env, &args[0])?)
    } else {
        None
    };

    // Create a closure that executes finally and returns the original value
    let closure_data = ClosureData::new(
        &[DestructuringElement::Variable("value".to_string(), None)],
        &[
            // Execute finally callback if provided (no arguments)
            stmt_expr(Expr::Call(Box::new(Expr::Var("finally_func".to_string(), None, None)), vec![])),
            // Return the original value
            stmt_return(Some(Expr::Var("value".to_string(), None, None))),
        ],
        &{
            let new_env = env.clone();
            // Add the finally callback to the environment
            if let Some(callback) = on_finally {
                obj_set_key_value(&new_env, &"finally_func".into(), callback)?;
            } else {
                // No-op if no callback provided
                let closure_data = ClosureData::new(&[], &[], &new_env, None);
                let noop = Value::Closure(Rc::new(closure_data));
                obj_set_key_value(&new_env, &"finally_func".into(), noop)?;
            }
            new_env
        },
        None,
    );
    let finally_callback = Value::Closure(Rc::new(closure_data));

    // Add the same callback to both fulfilled and rejected lists
    let mut promise_borrow = promise.borrow_mut();
    promise_borrow
        .on_fulfilled
        .push((finally_callback.clone(), new_promise.clone(), Some(env.clone())));
    promise_borrow
        .on_rejected
        .push((finally_callback.clone(), new_promise.clone(), Some(env.clone())));

    // If promise is already settled, queue task to execute callback asynchronously
    match &promise_borrow.state {
        PromiseState::Fulfilled(_) => {
            queue_task(Task::Resolution {
                promise: promise.clone(),
                callbacks: vec![(finally_callback.clone(), new_promise.clone(), Some(env.clone()))],
            });
        }
        PromiseState::Rejected(_) => {
            queue_task(Task::Rejection {
                promise: promise.clone(),
                callbacks: vec![(finally_callback.clone(), new_promise.clone(), Some(env.clone()))],
            });
        }
        _ => {}
    }

    Ok(Value::Object(new_promise_obj))
}

/// Resolve a promise with a value, transitioning it to fulfilled state.
///
/// This function changes the promise state from Pending to Fulfilled and
/// queues all registered fulfillment callbacks for asynchronous execution.
/// If the value is itself a promise, it adopts the state of that promise (flattening).
///
/// # Arguments
/// * `promise` - The promise to resolve
/// * `value` - The value to resolve the promise with
///
/// # Behavior
/// - Only works if promise is currently in Pending state
/// - If value is a promise object, adopts its state instead of resolving to the object
/// - Sets promise state to Fulfilled and stores the value
/// - Queues all on_fulfilled callbacks for async execution
/// - Clears the callback list after queuing
pub fn resolve_promise(promise: &Rc<RefCell<JSPromise>>, value: Value) {
    log::trace!("resolve_promise called");
    let mut promise_borrow = promise.borrow_mut();
    if let PromiseState::Pending = promise_borrow.state {
        // Check if value is a promise object for flattening
        if let Value::Object(obj) = &value
            && let Ok(Some(promise_val_rc)) = obj_get_key_value(obj, &"__promise".into())
            && let Value::Promise(other_promise) = &*promise_val_rc.borrow()
        {
            // Adopt the state of the other promise
            let current_promise = promise.clone();

            let then_callback = Value::Closure(Rc::new(ClosureData::new(
                &[DestructuringElement::Variable("val".to_string(), None)],
                &[stmt_expr(Expr::Call(
                    Box::new(Expr::Var("__internal_resolve_promise".to_string(), None, None)),
                    vec![
                        Expr::Var("__current_promise".to_string(), None, None),
                        Expr::Var("val".to_string(), None, None),
                    ],
                ))],
                &{
                    let env = new_js_object_data();
                    env_set(&env, "__current_promise", Value::Promise(current_promise.clone())).unwrap();
                    env
                },
                None,
            )));

            let catch_callback = Value::Closure(Rc::new(ClosureData::new(
                &[DestructuringElement::Variable("reason".to_string(), None)],
                &[stmt_expr(Expr::Call(
                    Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                    vec![
                        Expr::Var("__current_promise".to_string(), None, None),
                        Expr::Var("reason".to_string(), None, None),
                    ],
                ))],
                &{
                    let env = new_js_object_data();
                    env_set(&env, "__current_promise", Value::Promise(current_promise)).unwrap();
                    env
                },
                None,
            )));

            let other_promise_borrow = other_promise.borrow();
            match &other_promise_borrow.state {
                PromiseState::Fulfilled(val) => {
                    // Already fulfilled, resolve immediately with the value
                    drop(promise_borrow);
                    resolve_promise(promise, val.clone());
                    return;
                }
                PromiseState::Rejected(reason) => {
                    // Already rejected, reject immediately with the reason
                    drop(promise_borrow);
                    reject_promise(promise, reason.clone());
                    return;
                }
                PromiseState::Pending => {
                    // Still pending, attach callbacks
                    drop(other_promise_borrow);
                    let mut other_promise_mut = other_promise.borrow_mut();
                    other_promise_mut.on_fulfilled.push((then_callback, promise.clone(), None));
                    other_promise_mut.on_rejected.push((catch_callback, promise.clone(), None));
                    return;
                }
            }
        }

        // Normal resolve
        promise_borrow.state = PromiseState::Fulfilled(value.clone());
        promise_borrow.value = Some(value);

        // Queue task to execute fulfilled callbacks asynchronously
        let callbacks = promise_borrow.on_fulfilled.clone();
        promise_borrow.on_fulfilled.clear();
        if !callbacks.is_empty() {
            log::trace!("resolve_promise: queuing {} callbacks", callbacks.len());
            queue_task(Task::Resolution {
                promise: promise.clone(),
                callbacks,
            });
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
pub fn reject_promise(promise: &Rc<RefCell<JSPromise>>, reason: Value) {
    let mut promise_borrow = promise.borrow_mut();
    // Helpful debug logging for rejected promises (especially when rejecting
    // with JS Error-like objects) to help track unhandled rejections.
    if let Value::Object(obj) = &reason {
        if let Ok(Some(ctor_rc)) = obj_get_key_value(obj, &"constructor".into()) {
            log::debug!("reject_promise: rejecting with object whose constructor = {:?}", ctor_rc.borrow());
        } else {
            log::debug!("reject_promise: rejecting with object ptr={:p}", Rc::as_ptr(obj));
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
            queue_task(Task::Rejection {
                promise: promise.clone(),
                callbacks,
            });
        } else {
            // No callbacks now: queue a task to check for unhandled rejection
            // after potential handler attachment (avoids race with synchronous .then/.catch)
            log::trace!(
                "reject_promise: queuing UnhandledCheck task for promise ptr={:p}",
                Rc::as_ptr(promise)
            );
            queue_task(Task::UnhandledCheck {
                promise: promise.clone(),
                reason: reason.clone(),
            });
        }
    }
}

/// Check if a JavaScript object represents a Promise.
///
/// # Arguments
/// * `obj` - The object to check
///
/// # Returns
/// * `bool` - True if the object contains a promise, false otherwise
#[allow(dead_code)]
pub fn is_promise(obj: &JSObjectDataPtr) -> bool {
    if let Ok(Some(val_rc)) = obj_get_key_value(obj, &"__promise".into()) {
        matches!(&*val_rc.borrow(), Value::Promise(_))
    } else {
        false
    }
}

/// Handle Promise static methods like Promise.all, Promise.race, Promise.allSettled, Promise.any
///
/// These methods coordinate multiple promises and return a new promise that
/// resolves based on the collective outcome of the input promises.
///
/// # Arguments
/// * `method` - The static method name ("all", "race", "allSettled", "any")
/// * `args` - Method arguments (typically an iterable of promises)
/// * `env` - Current execution environment
///
/// # Returns
/// * `Result<Value, JSError>` - Result promise or error
///
/// # Supported Methods
/// - `Promise.all(iterable)` - Resolves when all promises resolve, rejects on first rejection
/// - `Promise.race(iterable)` - Resolves/rejects with the first settled promise
/// - `Promise.allSettled(iterable)` - Resolves when all promises settle (fulfill or reject)
/// - `Promise.any(iterable)` - Resolves with first fulfillment, rejects only if all reject
pub fn handle_promise_static_method(method: &str, args: &[crate::core::Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "all" => {
            // Promise.all(iterable) - resolves when all promises resolve, rejects on first rejection
            if args.is_empty() {
                return Err(raise_eval_error!("Promise.all requires at least one argument"));
            }

            let iterable = evaluate_expr(env, &args[0])?;
            let promises = match iterable {
                Value::Object(arr) => {
                    // Assume it's an array-like object
                    let mut promises = Vec::new();
                    let mut i = 0;
                    loop {
                        let key = i.to_string();
                        if let Some(val) = obj_get_key_value(&arr, &key.into())? {
                            promises.push((*val).borrow().clone());
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    promises
                }
                _ => {
                    return Err(raise_eval_error!("Promise.all argument must be iterable"));
                }
            };

            // Create a new promise that resolves when all promises resolve
            let result_promise = Rc::new(RefCell::new(JSPromise::new()));
            let result_promise_obj = make_promise_object()?;
            obj_set_key_value(&result_promise_obj, &"__promise".into(), Value::Promise(result_promise.clone()))?;

            let num_promises = promises.len();
            if num_promises == 0 {
                // Empty array, resolve immediately with empty array
                let result_arr = crate::js_array::create_array(env)?;
                resolve_promise(&result_promise, Value::Object(result_arr));
                return Ok(Value::Object(result_promise_obj));
            }

            // Create state object for coordination
            let state_obj = new_js_object_data();
            let results_obj = crate::js_array::create_array(env)?;
            obj_set_key_value(&state_obj, &"results".into(), Value::Object(results_obj.clone()))?;
            obj_set_key_value(&state_obj, &"completed".into(), Value::Number(0.0))?;
            obj_set_key_value(&state_obj, &"total".into(), Value::Number(num_promises as f64))?;
            obj_set_key_value(&state_obj, &"result_promise".into(), Value::Promise(result_promise.clone()))?;

            for (idx, promise_val) in promises.into_iter().enumerate() {
                let state_obj_clone = state_obj.clone();

                match promise_val {
                    Value::Object(obj) => {
                        if let Some(promise_rc) = obj_get_key_value(&obj, &"__promise".into())? {
                            if let Value::Promise(promise_ref) = &*promise_rc.borrow() {
                                // Check if promise is already settled
                                let promise_state = &promise_ref.borrow().state;
                                match promise_state {
                                    PromiseState::Fulfilled(val) => {
                                        // Promise already fulfilled, record synchronously
                                        obj_set_key_value(&results_obj, &idx.to_string().into(), val.clone())?;
                                        // Increment completed
                                        if let Some(completed_val_rc) = obj_get_key_value(&state_obj, &"completed".into())?
                                            && let Value::Number(completed) = &*completed_val_rc.borrow()
                                        {
                                            let new_completed = completed + 1.0;
                                            obj_set_key_value(&state_obj, &"completed".into(), Value::Number(new_completed))?;
                                            // Check if all completed
                                            if let Some(total_val_rc) = obj_get_key_value(&state_obj, &"total".into())?
                                                && let Value::Number(total) = &*total_val_rc.borrow()
                                                && new_completed == *total
                                            {
                                                // Resolve result_promise with results array
                                                if let Some(promise) = obj_get_key_value(&state_obj, &"result_promise".into())?
                                                    && let Value::Promise(result_promise_ref) = &*promise.borrow()
                                                {
                                                    resolve_promise(result_promise_ref, Value::Object(results_obj.clone()));
                                                }
                                            }
                                        }
                                    }
                                    PromiseState::Rejected(reason) => {
                                        // Promise already rejected, reject result promise immediately
                                        if let Some(promise_val_rc) = obj_get_key_value(&state_obj, &"result_promise".into())?
                                            && let Value::Promise(result_promise_ref) = &*promise_val_rc.borrow()
                                        {
                                            reject_promise(result_promise_ref, reason.clone());
                                        }
                                        return Ok(Value::Object(result_promise_obj));
                                    }
                                    PromiseState::Pending => {
                                        // Promise still pending, attach callbacks
                                        let then_callback = Value::Closure(Rc::new(ClosureData::new(
                                            &[DestructuringElement::Variable("value".to_string(), None)],
                                            &[stmt_expr(Expr::Call(
                                                Box::new(Expr::Var("__internal_promise_all_resolve".to_string(), None, None)),
                                                vec![
                                                    Expr::Number(idx as f64),
                                                    Expr::Var("value".to_string(), None, None),
                                                    Expr::Var("__state".to_string(), None, None),
                                                ],
                                            ))],
                                            &{
                                                let new_env = env.clone();
                                                obj_set_key_value(&new_env, &"__state".into(), Value::Object(state_obj_clone.clone()))?;
                                                new_env
                                            },
                                            None,
                                        )));

                                        let catch_callback = Value::Closure(Rc::new(ClosureData::new(
                                            &[DestructuringElement::Variable("reason".to_string(), None)],
                                            &[stmt_expr(Expr::Call(
                                                Box::new(Expr::Var("__internal_promise_all_reject".to_string(), None, None)),
                                                vec![
                                                    Expr::Var("reason".to_string(), None, None),
                                                    Expr::Var("__state".to_string(), None, None),
                                                ],
                                            ))],
                                            &{
                                                let new_env = env.clone();
                                                obj_set_key_value(&new_env, &"__state".into(), Value::Object(state_obj_clone))?;
                                                new_env
                                            },
                                            None,
                                        )));

                                        // Attach then and catch to the promise
                                        handle_promise_then(&obj, &[Expr::Value(then_callback)], env)?;
                                        handle_promise_catch(&obj, &[Expr::Value(catch_callback)], env)?;
                                    }
                                }
                            } else {
                                // Not a promise, treat as resolved value
                                obj_set_key_value(&results_obj, &idx.to_string().into(), Value::Object(obj.clone()))?;
                                // Increment completed
                                if let Some(completed_val_rc) = obj_get_key_value(&state_obj, &"completed".into())?
                                    && let Value::Number(completed) = &*completed_val_rc.borrow()
                                {
                                    let new_completed = completed + 1.0;
                                    obj_set_key_value(&state_obj, &"completed".into(), Value::Number(new_completed))?;
                                    // Check if all completed
                                    if let Some(total_val_rc) = obj_get_key_value(&state_obj, &"total".into())?
                                        && let Value::Number(total) = &*total_val_rc.borrow()
                                        && new_completed == *total
                                    {
                                        // Resolve result_promise with results array
                                        if let Some(promise_val_rc) = obj_get_key_value(&state_obj, &"result_promise".into())?
                                            && let Value::Promise(result_promise_ref) = &*promise_val_rc.borrow()
                                        {
                                            resolve_promise(result_promise_ref, Value::Object(results_obj.clone()));
                                        }
                                    }
                                }
                            }
                        } else {
                            // Not a promise, treat as resolved value
                            obj_set_key_value(&results_obj, &idx.to_string().into(), Value::Object(obj.clone()))?;
                            // Increment completed
                            if let Some(completed_val_rc) = obj_get_key_value(&state_obj, &"completed".into())?
                                && let Value::Number(completed) = &*completed_val_rc.borrow()
                            {
                                let new_completed = completed + 1.0;
                                obj_set_key_value(&state_obj, &"completed".into(), Value::Number(new_completed))?;
                                // Check if all completed
                                if let Some(total_val_rc) = obj_get_key_value(&state_obj, &"total".into())?
                                    && let Value::Number(total) = &*total_val_rc.borrow()
                                    && new_completed == *total
                                {
                                    // Resolve result_promise with results array
                                    if let Some(promise_val_rc) = obj_get_key_value(&state_obj, &"result_promise".into())?
                                        && let Value::Promise(result_promise_ref) = &*promise_val_rc.borrow()
                                    {
                                        resolve_promise(result_promise_ref, Value::Object(results_obj.clone()));
                                    }
                                }
                            }
                        }
                    }
                    val => {
                        // Non-object value, treat as resolved
                        obj_set_key_value(&results_obj, &idx.to_string().into(), val.clone())?;
                        // Increment completed
                        if let Some(completed_val_rc) = obj_get_key_value(&state_obj, &"completed".into())?
                            && let Value::Number(completed) = &*completed_val_rc.borrow()
                        {
                            let new_completed = completed + 1.0;
                            obj_set_key_value(&state_obj, &"completed".into(), Value::Number(new_completed))?;
                            // Check if all completed
                            if let Some(total_val_rc) = obj_get_key_value(&state_obj, &"total".into())?
                                && let Value::Number(total) = &*total_val_rc.borrow()
                                && new_completed == *total
                            {
                                // Resolve result_promise with results array
                                if let Some(promise_val_rc) = obj_get_key_value(&state_obj, &"result_promise".into())?
                                    && let Value::Promise(result_promise_ref) = &*promise_val_rc.borrow()
                                {
                                    resolve_promise(result_promise_ref, Value::Object(results_obj.clone()));
                                }
                            }
                        }
                    }
                }
            }

            Ok(Value::Object(result_promise_obj))
        }
        "allSettled" => {
            // Promise.allSettled(iterable) - resolves when all promises settle (fulfill or reject)
            // PHASE 2: Now using dedicated AllSettledState struct for better type safety
            if args.is_empty() {
                return Err(raise_eval_error!("Promise.allSettled requires at least one argument"));
            }

            let iterable = evaluate_expr(env, &args[0])?;
            let promises = match iterable {
                Value::Object(arr) => {
                    // Assume it's an array-like object
                    let mut promises = Vec::new();
                    let mut i = 0;
                    loop {
                        let key = i.to_string();
                        if let Some(val) = obj_get_key_value(&arr, &key.into())? {
                            promises.push((*val).borrow().clone());
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    promises
                }
                _ => {
                    return Err(raise_eval_error!("Promise.allSettled argument must be iterable"));
                }
            };

            let result_promise = Rc::new(RefCell::new(JSPromise::new()));
            let result_promise_obj = make_promise_object()?;
            obj_set_key_value(&result_promise_obj, &"__promise".into(), Value::Promise(result_promise.clone()))?;

            let num_promises = promises.len();
            if num_promises == 0 {
                let result_arr = crate::js_array::create_array(env)?;
                resolve_promise(&result_promise, Value::Object(result_arr));
                return Ok(Value::Object(result_promise_obj));
            }

            // Create dedicated state structure for coordination
            let state = Rc::new(RefCell::new(AllSettledState::new(
                num_promises,
                result_promise.clone(),
                env.clone(),
            )));

            // Store state in global storage and get its index
            let state_index = ALLSETTLED_STATES.with(|states| {
                let mut states_borrow = states.borrow_mut();
                let index = states_borrow.len();
                states_borrow.push(state.clone());
                index
            });

            for (idx, promise_val) in promises.into_iter().enumerate() {
                match promise_val {
                    Value::Object(obj) => {
                        if let Some(promise_rc) = obj_get_key_value(&obj, &"__promise".into())? {
                            if let Value::Promise(promise_ref) = &*promise_rc.borrow() {
                                // Check if promise is already settled
                                let promise_state = &promise_ref.borrow().state;
                                match promise_state {
                                    PromiseState::Fulfilled(val) => {
                                        // Promise already fulfilled, record synchronously
                                        state.borrow_mut().record_fulfilled(idx, val.clone())?;
                                    }
                                    PromiseState::Rejected(reason) => {
                                        // Promise already rejected, record synchronously
                                        state.borrow_mut().record_rejected(idx, reason.clone())?;
                                    }
                                    PromiseState::Pending => {
                                        // Promise still pending, attach callbacks
                                        let then_callback = create_allsettled_resolve_callback(state_index, idx);
                                        let catch_callback = create_allsettled_reject_callback(state_index, idx);
                                        handle_promise_then(&obj, &[Expr::Value(then_callback)], env)?;
                                        handle_promise_catch(&obj, &[Expr::Value(catch_callback)], env)?;
                                    }
                                }
                            } else {
                                // Not a promise, treat as resolved value
                                state.borrow_mut().record_fulfilled(idx, Value::Object(obj.clone()))?;
                            }
                        } else {
                            // Not a promise, treat as resolved value
                            state.borrow_mut().record_fulfilled(idx, Value::Object(obj.clone()))?;
                        }
                    }
                    val => {
                        // Non-object, treat as resolved value
                        state.borrow_mut().record_fulfilled(idx, val)?;
                    }
                }
            }

            Ok(Value::Object(result_promise_obj))
        }
        "any" => {
            // Promise.any(iterable)
            if args.is_empty() {
                return Err(raise_eval_error!("Promise.any requires at least one argument"));
            }

            let iterable = evaluate_expr(env, &args[0])?;
            let promises = match iterable {
                Value::Object(arr) => {
                    let mut promises = Vec::new();
                    let mut i = 0;
                    loop {
                        let key = i.to_string();
                        if let Some(val) = obj_get_key_value(&arr, &key.into())? {
                            promises.push((*val).borrow().clone());
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    promises
                }
                _ => {
                    return Err(raise_eval_error!("Promise.any argument must be iterable"));
                }
            };

            let result_promise = Rc::new(RefCell::new(JSPromise::new()));
            let result_promise_obj = make_promise_object()?;
            obj_set_key_value(&result_promise_obj, &"__promise".into(), Value::Promise(result_promise.clone()))?;

            let num_promises = promises.len();
            if num_promises == 0 {
                // Empty array, reject with AggregateError
                let aggregate_error = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
                obj_set_key_value(&aggregate_error, &"name".into(), Value::String(utf8_to_utf16("AggregateError")))?;
                obj_set_key_value(
                    &aggregate_error,
                    &"message".into(),
                    Value::String(utf8_to_utf16("All promises were rejected")),
                )?;
                reject_promise(&result_promise, Value::Object(aggregate_error));
                return Ok(Value::Object(result_promise_obj));
            }

            let rejections = Rc::new(RefCell::new(vec![None::<Value>; num_promises]));
            let rejected_count = Rc::new(RefCell::new(0));

            for (idx, promise_val) in promises.into_iter().enumerate() {
                let _rejections_clone = rejections.clone();
                let rejected_count_clone = rejected_count.clone();
                let result_promise_clone = result_promise.clone();

                match promise_val {
                    Value::Object(obj) => {
                        if let Some(promise_rc) = obj_get_key_value(&obj, &"__promise".into())? {
                            if let Value::Promise(_p) = &*promise_rc.borrow() {
                                let then_callback = Value::Closure(Rc::new(ClosureData::new(
                                    &[DestructuringElement::Variable("value".to_string(), None)],
                                    &[stmt_expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_promise_any_resolve".to_string(), None, None)),
                                        vec![
                                            Expr::Var("value".to_string(), None, None),
                                            Expr::Var("__result_promise".to_string(), None, None),
                                        ],
                                    ))],
                                    &{
                                        let new_env = env.clone();
                                        obj_set_key_value(
                                            &new_env,
                                            &"__result_promise".into(),
                                            Value::Promise(result_promise_clone.clone()),
                                        )?;
                                        new_env
                                    },
                                    None,
                                )));

                                let catch_callback = Value::Closure(Rc::new(ClosureData::new(
                                    &[DestructuringElement::Variable("reason".to_string(), None)],
                                    &[stmt_expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_promise_any_reject".to_string(), None, None)),
                                        vec![
                                            Expr::Number(idx as f64),
                                            Expr::Var("reason".to_string(), None, None),
                                            Expr::Var("__rejections".to_string(), None, None),
                                            Expr::Var("__rejected_count".to_string(), None, None),
                                            Expr::Var("__total".to_string(), None, None),
                                            Expr::Var("__result_promise".to_string(), None, None),
                                        ],
                                    ))],
                                    &{
                                        let new_env = env.clone();
                                        obj_set_key_value(
                                            &new_env,
                                            &"__rejected_count".into(),
                                            Value::Number(*rejected_count_clone.borrow() as f64),
                                        )?;
                                        obj_set_key_value(&new_env, &"__total".into(), Value::Number(num_promises as f64))?;
                                        obj_set_key_value(&new_env, &"__result_promise".into(), Value::Promise(result_promise_clone))?;
                                        new_env
                                    },
                                    None,
                                )));

                                handle_promise_then(&obj, &[Expr::Value(then_callback)], env)?;
                                handle_promise_catch(&obj, &[Expr::Value(catch_callback)], env)?;
                            } else {
                                // Not a promise, resolve immediately
                                resolve_promise(&result_promise, Value::Object(obj.clone()));
                                return Ok(Value::Object(result_promise_obj));
                            }
                        } else {
                            resolve_promise(&result_promise, Value::Object(obj.clone()));
                            return Ok(Value::Object(result_promise_obj));
                        }
                    }
                    val => {
                        // Non-object, resolve immediately
                        resolve_promise(&result_promise, val.clone());
                        return Ok(Value::Object(result_promise_obj));
                    }
                }
            }

            Ok(Value::Object(result_promise_obj))
        }
        "race" => {
            // Promise.race(iterable) - asynchronous implementation
            if args.is_empty() {
                return Err(raise_eval_error!("Promise.race requires at least one argument"));
            }

            let iterable = evaluate_expr(env, &args[0])?;
            let promises = match iterable {
                Value::Object(arr) => {
                    let mut promises = Vec::new();
                    let mut i = 0;
                    loop {
                        let key = i.to_string();
                        if let Some(val) = obj_get_key_value(&arr, &key.into())? {
                            promises.push((*val).borrow().clone());
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    promises
                }
                _ => {
                    return Err(raise_eval_error!("Promise.race argument must be iterable"));
                }
            };

            let result_promise = Rc::new(RefCell::new(JSPromise::new()));
            let result_promise_obj = make_promise_object()?;
            obj_set_key_value(&result_promise_obj, &"__promise".into(), Value::Promise(result_promise.clone()))?;

            for promise_val in promises {
                let result_promise_clone = result_promise.clone();

                match promise_val {
                    Value::Object(obj) => {
                        if let Some(promise_rc) = obj_get_key_value(&obj, &"__promise".into())? {
                            if let Value::Promise(promise_ref) = &*promise_rc.borrow() {
                                // Check if promise is already settled
                                let promise_state = &promise_ref.borrow().state;
                                match promise_state {
                                    PromiseState::Fulfilled(val) => {
                                        // Promise already fulfilled, resolve result immediately
                                        resolve_promise(&result_promise, val.clone());
                                        return Ok(Value::Object(result_promise_obj));
                                    }
                                    PromiseState::Rejected(reason) => {
                                        // Promise already rejected, reject result immediately
                                        reject_promise(&result_promise, reason.clone());
                                        return Ok(Value::Object(result_promise_obj));
                                    }
                                    PromiseState::Pending => {
                                        // Promise still pending, attach callbacks
                                        let then_callback = Value::Closure(Rc::new(ClosureData::new(
                                            &[DestructuringElement::Variable("value".to_string(), None)],
                                            &[stmt_expr(Expr::Call(
                                                Box::new(Expr::Var("__internal_promise_race_resolve".to_string(), None, None)),
                                                vec![
                                                    Expr::Var("value".to_string(), None, None),
                                                    Expr::Var("__result_promise".to_string(), None, None),
                                                ],
                                            ))],
                                            &{
                                                let new_env = env.clone();
                                                obj_set_key_value(
                                                    &new_env,
                                                    &"__result_promise".into(),
                                                    Value::Promise(result_promise_clone.clone()),
                                                )?;
                                                new_env
                                            },
                                            None,
                                        )));

                                        let catch_callback = Value::Closure(Rc::new(ClosureData::new(
                                            &[DestructuringElement::Variable("reason".to_string(), None)],
                                            &[stmt_expr(Expr::Call(
                                                Box::new(Expr::Var("__internal_promise_race_reject".to_string(), None, None)),
                                                vec![
                                                    Expr::Var("reason".to_string(), None, None),
                                                    Expr::Var("__result_promise".to_string(), None, None),
                                                ],
                                            ))],
                                            &{
                                                let new_env = env.clone();
                                                obj_set_key_value(
                                                    &new_env,
                                                    &"__result_promise".into(),
                                                    Value::Promise(result_promise_clone),
                                                )?;
                                                new_env
                                            },
                                            None,
                                        )));

                                        handle_promise_then(&obj, &[Expr::Value(then_callback)], env)?;
                                        handle_promise_catch(&obj, &[Expr::Value(catch_callback)], env)?;
                                    }
                                }
                            } else {
                                // Not a promise, resolve immediately
                                resolve_promise(&result_promise, Value::Object(obj.clone()));
                                return Ok(Value::Object(result_promise_obj));
                            }
                        } else {
                            resolve_promise(&result_promise, Value::Object(obj.clone()));
                            return Ok(Value::Object(result_promise_obj));
                        }
                    }
                    val => {
                        // Non-object, resolve immediately
                        resolve_promise(&result_promise, val.clone());
                        return Ok(Value::Object(result_promise_obj));
                    }
                }
            }

            Ok(Value::Object(result_promise_obj))
        }
        "resolve" => {
            // Promise.resolve(value) - return the value wrapped in a resolved promise
            let value = if args.is_empty() {
                Value::Undefined
            } else {
                evaluate_expr(env, &args[0])?
            };

            // If the value is already a promise object, return it directly
            if let Value::Object(obj) = &value
                && let Some(promise_rc) = obj_get_key_value(obj, &"__promise".into())?
                && let Value::Promise(_) = &*promise_rc.borrow()
            {
                return Ok(Value::Object(obj.clone()));
            }

            // Otherwise create a new resolved promise holding the value
            let result_promise = Rc::new(RefCell::new(JSPromise::new()));
            {
                let mut p = result_promise.borrow_mut();
                p.state = PromiseState::Fulfilled(value.clone());
                p.value = Some(value.clone());
            }
            let result_promise_obj = make_promise_object()?;
            obj_set_key_value(&result_promise_obj, &"__promise".into(), Value::Promise(result_promise.clone()))?;
            Ok(Value::Object(result_promise_obj))
        }
        "reject" => {
            // Promise.reject(reason) - return a rejected promise
            let reason = if args.is_empty() {
                Value::Undefined
            } else {
                evaluate_expr(env, &args[0])?
            };

            let result_promise = Rc::new(RefCell::new(JSPromise::new()));
            {
                let mut p = result_promise.borrow_mut();
                p.state = PromiseState::Rejected(reason.clone());
                p.value = Some(reason.clone());
            }
            let result_promise_obj = make_promise_object()?;
            obj_set_key_value(&result_promise_obj, &"__promise".into(), Value::Promise(result_promise.clone()))?;
            Ok(Value::Object(result_promise_obj))
        }
        _ => Err(raise_eval_error!(format!("Promise has no static method '{method}'"))),
    }
}

/// Handle Promise instance method calls
pub fn handle_promise_method(object: &JSObjectDataPtr, method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "then" => crate::js_promise::handle_promise_then(object, args, env),
        "catch" => crate::js_promise::handle_promise_catch(object, args, env),
        "finally" => crate::js_promise::handle_promise_finally(object, args, env),
        _ => Err(raise_eval_error!(format!("Promise has no method '{method}'"))),
    }
}

// Internal callback functions for Promise static methods
// These functions are called when individual promises in Promise.allSettled resolve/reject

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
pub fn __internal_promise_allsettled_resolve(idx: f64, value: Value, shared_state: Value) -> Result<(), JSError> {
    if let Value::Object(shared_state_obj) = shared_state {
        // Get results array
        if let Some(results_val_rc) = obj_get_key_value(&shared_state_obj, &"results".into())?
            && let Value::Object(results_obj) = &*results_val_rc.borrow()
        {
            // Create settled result
            let settled = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
            obj_set_key_value(&settled, &"status".into(), Value::String(utf8_to_utf16("fulfilled")))?;
            obj_set_key_value(&settled, &"value".into(), value)?;
            // Add to results array at idx
            obj_set_key_value(results_obj, &idx.to_string().into(), Value::Object(settled))?;
        }

        // Increment completed
        if let Some(completed_val_rc) = obj_get_key_value(&shared_state_obj, &"completed".into())?
            && let Value::Number(completed) = &*completed_val_rc.borrow()
        {
            let new_completed = completed + 1.0;
            obj_set_key_value(&shared_state_obj, &"completed".into(), Value::Number(new_completed))?;

            // Check if all completed
            if let Some(total_val_rc) = obj_get_key_value(&shared_state_obj, &"total".into())?
                && let Value::Number(total) = &*total_val_rc.borrow()
                && new_completed == *total
            {
                // Resolve result promise
                if let Some(promise_val_rc) = obj_get_key_value(&shared_state_obj, &"result_promise".into())?
                    && let Value::Promise(result_promise) = &*promise_val_rc.borrow()
                    && let Some(results_val_rc) = obj_get_key_value(&shared_state_obj, &"results".into())?
                    && let Value::Object(results_obj) = &*results_val_rc.borrow()
                {
                    resolve_promise(result_promise, Value::Object(results_obj.clone()));
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
pub fn __internal_promise_allsettled_reject(idx: f64, reason: Value, shared_state: Value) -> Result<(), JSError> {
    if let Value::Object(shared_state_obj) = shared_state {
        // Get results array
        if let Some(results_val_rc) = obj_get_key_value(&shared_state_obj, &"results".into())?
            && let Value::Object(results_obj) = &*results_val_rc.borrow()
        {
            // Create settled result
            let settled = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
            obj_set_key_value(&settled, &"status".into(), Value::String(utf8_to_utf16("rejected")))?;
            obj_set_key_value(&settled, &"reason".into(), reason)?;

            // Add to results array at idx
            obj_set_key_value(results_obj, &idx.to_string().into(), Value::Object(settled))?;
        }

        // Increment completed
        if let Some(completed_val_rc) = obj_get_key_value(&shared_state_obj, &"completed".into())?
            && let Value::Number(completed) = &*completed_val_rc.borrow()
        {
            let new_completed = completed + 1.0;
            obj_set_key_value(&shared_state_obj, &"completed".into(), Value::Number(new_completed))?;

            // Check if all completed
            if let Some(total_val_rc) = obj_get_key_value(&shared_state_obj, &"total".into())?
                && let Value::Number(total) = &*total_val_rc.borrow()
                && new_completed == *total
            {
                // Resolve result promise
                if let Some(promise_val_rc) = obj_get_key_value(&shared_state_obj, &"result_promise".into())?
                    && let Value::Promise(result_promise) = &*promise_val_rc.borrow()
                    && let Some(results_val_rc) = obj_get_key_value(&shared_state_obj, &"results".into())?
                    && let Value::Object(results_obj) = &*results_val_rc.borrow()
                {
                    resolve_promise(result_promise, Value::Object(results_obj.clone()));
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
pub fn __internal_promise_any_resolve(value: Value, result_promise: Rc<RefCell<JSPromise>>) {
    resolve_promise(&result_promise, value);
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
pub fn __internal_promise_any_reject(
    idx: f64,
    reason: Value,
    rejections: Rc<RefCell<Vec<Option<Value>>>>,
    rejected_count: Rc<RefCell<usize>>,
    total: usize,
    result_promise: Rc<RefCell<JSPromise>>,
) -> Result<(), JSError> {
    let idx = idx as usize;
    rejections.borrow_mut()[idx] = Some(reason);
    *rejected_count.borrow_mut() += 1;

    if *rejected_count.borrow() == total {
        // All promises rejected, create AggregateError
        let aggregate_error = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
        obj_set_key_value(&aggregate_error, &"name".into(), Value::String(utf8_to_utf16("AggregateError"))).unwrap();
        obj_set_key_value(
            &aggregate_error,
            &"message".into(),
            Value::String(utf8_to_utf16("All promises were rejected")),
        )?;

        let errors_array = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
        let rejections_vec = rejections.borrow();
        for (i, rejection) in rejections_vec.iter().enumerate() {
            if let Some(err) = rejection {
                obj_set_key_value(&errors_array, &i.to_string().into(), err.clone())?;
            }
        }
        obj_set_key_value(&aggregate_error, &"errors".into(), Value::Object(errors_array))?;

        reject_promise(&result_promise, Value::Object(aggregate_error));
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
pub fn __internal_promise_race_resolve(value: Value, result_promise: Rc<RefCell<JSPromise>>) {
    resolve_promise(&result_promise, value);
}

/// Internal function for Promise.race reject callback
///
/// Called when any promise in Promise.race rejects.
/// Immediately rejects the main Promise.race promise with the reason.
///
/// # Arguments
/// * `reason` - The rejection reason from the first rejected promise
/// * `result_promise` - The main Promise.race promise to reject
pub fn __internal_promise_race_reject(reason: Value, result_promise: Rc<RefCell<JSPromise>>) {
    reject_promise(&result_promise, reason);
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
pub fn __internal_allsettled_state_record_fulfilled(state_index: f64, index: f64, value: Value) -> Result<(), JSError> {
    log::trace!("__internal_allsettled_state_record_fulfilled called: state_idx={state_index}, idx={index}, val={value:?}");
    let state_index = state_index as usize;
    let index = index as usize;

    ALLSETTLED_STATES.with(|states| {
        let states_borrow = states.borrow();
        if state_index < states_borrow.len() {
            let state = &states_borrow[state_index];
            log::trace!("Recording fulfilled for index {} in state {}", index, state_index);
            state.borrow_mut().record_fulfilled(index, value)?;
        } else {
            log::trace!("Invalid state_index {} (len={})", state_index, states_borrow.len());
        }
        Ok::<(), JSError>(())
    })?;
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
pub fn __internal_allsettled_state_record_rejected(state_index: f64, index: f64, reason: Value) -> Result<(), JSError> {
    log::trace!("__internal_allsettled_state_record_rejected called: state_index={state_index}, index={index}, reason={reason:?}");
    let state_index = state_index as usize;
    let index = index as usize;

    ALLSETTLED_STATES.with(|states| {
        let states_borrow = states.borrow();
        if state_index < states_borrow.len() {
            let state = &states_borrow[state_index];
            log::trace!("Recording rejected for index {} in state {}", index, state_index);
            state.borrow_mut().record_rejected(index, reason)?;
        } else {
            log::trace!("Invalid state_index {} (len={})", state_index, states_borrow.len());
        }
        Ok::<(), JSError>(())
    })?;
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
fn create_allsettled_resolve_callback(state_index: usize, index: usize) -> Value {
    Value::Closure(Rc::new(ClosureData::new(
        &[DestructuringElement::Variable("value".to_string(), None)],
        &[stmt_expr(Expr::Call(
            Box::new(Expr::Var("__internal_allsettled_state_record_fulfilled".to_string(), None, None)),
            vec![
                Expr::Number(state_index as f64),
                Expr::Number(index as f64),
                Expr::Var("value".to_string(), None, None),
            ],
        ))],
        &Rc::new(RefCell::new(crate::core::JSObjectData::new())), // Empty environment
        None,
    )))
}

/// Create a reject callback function for Promise.allSettled
///
/// Creates a closure that calls the internal function to record rejection
/// in the AllSettledState stored in global storage.
///
/// # Arguments
/// * `state_index` - Index of the AllSettledState in global storage
/// * `index` - Index of the promise in the original array
///
/// # Returns
/// A Value::Closure that can be used as a catch callback
fn create_allsettled_reject_callback(state_index: usize, index: usize) -> Value {
    Value::Closure(Rc::new(ClosureData::new(
        &[DestructuringElement::Variable("reason".to_string(), None)],
        &[stmt_expr(Expr::Call(
            Box::new(Expr::Var("__internal_allsettled_state_record_rejected".to_string(), None, None)),
            vec![
                Expr::Number(state_index as f64),
                Expr::Number(index as f64),
                Expr::Var("reason".to_string(), None, None),
            ],
        ))],
        &Rc::new(RefCell::new(crate::core::JSObjectData::new())), // Empty environment
        None,
    )))
}

/// Handle setTimeout function calls.
///
/// Schedules a callback to be executed asynchronously after a delay.
/// In this implementation, the delay is ignored and the callback is queued
/// for execution in the next event loop iteration.
///
/// # Arguments
/// * `args` - Function arguments: callback function and optional delay/args
/// * `env` - Current execution environment
///
/// # Returns
/// * `Result<Value, JSError>` - A numeric timeout ID
///
/// # Example
/// ```javascript
/// let id = setTimeout(() => console.log("Hello"), 1000);
/// ```
pub fn handle_set_timeout(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        return Err(raise_eval_error!("setTimeout requires at least one argument"));
    }

    let callback = evaluate_expr(env, &args[0])?;
    let delay = if args.len() > 1 {
        match evaluate_expr(env, &args[1])? {
            Value::Number(n) => n.max(0.0) as u64,
            _ => 0,
        }
    } else {
        0
    };
    let mut timeout_args = Vec::new();

    // Additional arguments to pass to the callback
    for arg in args.iter().skip(2) {
        timeout_args.push(evaluate_expr(env, arg)?);
    }

    // Generate a unique timeout ID
    let id = NEXT_TIMEOUT_ID.with(|counter| {
        let mut id = counter.borrow_mut();
        let current_id = *id;
        *id += 1;
        current_id
    });

    // Queue the timeout task
    queue_task(Task::Timeout {
        id,
        callback,
        args: timeout_args,
        target_time: Instant::now() + Duration::from_millis(delay),
    });

    // Return the timeout ID
    Ok(Value::Number(id as f64))
}

/// Handle clearTimeout function calls.
///
/// Cancels a scheduled timeout. Removes the timeout task from the queue
/// if it hasn't been executed yet.
///
/// # Arguments
/// * `args` - Function arguments: timeout ID to cancel
/// * `_env` - Current execution environment (unused)
///
/// # Returns
/// * `Result<Value, JSError>` - Undefined
pub fn handle_clear_timeout(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        return Ok(Value::Undefined);
    }

    let id_val = evaluate_expr(env, &args[0])?;
    let id = match id_val {
        Value::Number(n) => n as usize,
        _ => return Ok(Value::Undefined),
    };

    // Remove the timeout task with the matching ID
    GLOBAL_TASK_QUEUE.with(|queue| {
        let mut queue_borrow = queue.borrow_mut();
        queue_borrow.retain(|task| !matches!(task, Task::Timeout { id: task_id, .. } if *task_id == id));
    });

    Ok(Value::Undefined)
}

/// Handle setInterval function calls.
pub fn handle_set_interval(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        return Err(raise_eval_error!("setInterval requires at least one argument"));
    }

    let callback = evaluate_expr(env, &args[0])?;
    let delay = if args.len() > 1 {
        match evaluate_expr(env, &args[1])? {
            Value::Number(n) => n.max(0.0) as u64,
            _ => 0,
        }
    } else {
        0
    };
    let mut interval_args = Vec::new();

    // Additional arguments to pass to the callback
    for arg in args.iter().skip(2) {
        interval_args.push(evaluate_expr(env, arg)?);
    }

    // Generate a unique timeout ID (shared with timeouts)
    let id = NEXT_TIMEOUT_ID.with(|counter| {
        let mut id = counter.borrow_mut();
        let current_id = *id;
        *id += 1;
        current_id
    });

    let interval = Duration::from_millis(delay);
    // Queue the interval task
    queue_task(Task::Interval {
        id,
        callback,
        args: interval_args,
        target_time: Instant::now() + interval,
        interval,
    });

    // Return the interval ID
    Ok(Value::Number(id as f64))
}

/// Handle clearInterval function calls.
pub fn handle_clear_interval(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        return Ok(Value::Undefined);
    }

    let id_val = evaluate_expr(env, &args[0])?;
    let id = match id_val {
        Value::Number(n) => n as usize,
        _ => return Ok(Value::Undefined),
    };

    // Remove the interval task with the matching ID
    GLOBAL_TASK_QUEUE.with(|queue| {
        let mut queue_borrow = queue.borrow_mut();
        queue_borrow.retain(|task| !matches!(task, Task::Interval { id: task_id, .. } if *task_id == id));
    });

    Ok(Value::Undefined)
}
