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
    ClosureData, DestructuringElement, EvalError, Expr, InternalSlot, JSGenerator, JSObjectData, JSObjectDataPtr, JSPromise, PromiseState,
    Statement, StatementKind, Value, evaluate_statements, extract_closure_from_value, generate_unique_id, object_get_key_value,
    object_set_key_value, prepare_closure_call_env, prepare_function_call_env, slot_get, slot_get_chained, slot_set, value_to_string,
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
    /// Host task to perform dynamic import and settle its promise
    DynamicImport {
        promise: GcPtr<'gc, JSPromise<'gc>>,
        module_specifier: Value<'gc>,
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
    /// PromiseResolveThenableJob: call thenable.then(resolve, reject) asynchronously
    ResolveThenableJob {
        promise: GcPtr<'gc, JSPromise<'gc>>,
        thenable: Value<'gc>,
        then_fn: Value<'gc>,
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
    // Unwrap Value::Property to its inner value before dispatch
    if let Value::Property { value: Some(v), .. } = func {
        return call_function(mc, &v.borrow(), args, env);
    }
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
            if let Some(cl_ptr) = obj.borrow().get_closure() {
                match &*cl_ptr.borrow() {
                    Value::Closure(cl) => {
                        return crate::core::call_closure(mc, cl, None, args, env, None);
                    }
                    Value::Function(name) => {
                        if let Some(res) = crate::core::call_native_function(mc, name, None, args, env)? {
                            return Ok(res);
                        }
                        return crate::js_function::handle_global_function(mc, name, args, env);
                    }
                    Value::AsyncClosure(cl) => {
                        return Ok(crate::js_async::handle_async_closure_call(mc, cl, None, args, env, None)?);
                    }
                    _ => {}
                }
            }
            Err(raise_type_error!("Not a function").into())
        }
        _ => Err(raise_type_error!("Not a function").into()),
    }
}

pub fn call_function_with_this<'gc>(
    mc: &MutationContext<'gc>,
    func: &Value<'gc>,
    this_val: Option<&Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Unwrap Value::Property to its inner value before dispatch
    if let Value::Property { value: Some(v), .. } = func {
        return call_function_with_this(mc, &v.borrow(), this_val, args, env);
    }
    match func {
        Value::Closure(cl) => crate::core::call_closure(mc, cl, this_val, args, env, None),
        Value::Function(name) => {
            if let Some(res) = crate::core::call_native_function(mc, name, this_val, args, env)? {
                Ok(res)
            } else if let Some(this) = this_val {
                let call_env = crate::core::new_js_object_data(mc);
                call_env.borrow_mut(mc).prototype = Some(*env);
                call_env.borrow_mut(mc).is_function_scope = true;
                object_set_key_value(mc, &call_env, "this", this)?;
                crate::js_function::handle_global_function(mc, name, args, &call_env)
            } else {
                crate::js_function::handle_global_function(mc, name, args, env)
            }
        }
        Value::Object(obj) => {
            if let Some(cl_ptr) = obj.borrow().get_closure() {
                match &*cl_ptr.borrow() {
                    Value::Closure(cl) => {
                        return crate::core::call_closure(mc, cl, this_val, args, env, None);
                    }
                    Value::Function(name) => {
                        if let Some(res) = crate::core::call_native_function(mc, name, this_val, args, env)? {
                            return Ok(res);
                        }
                        if let Some(this) = this_val {
                            let call_env = crate::core::new_js_object_data(mc);
                            call_env.borrow_mut(mc).prototype = Some(*env);
                            call_env.borrow_mut(mc).is_function_scope = true;
                            crate::core::object_set_key_value(mc, &call_env, "this", this)?;
                            return crate::js_function::handle_global_function(mc, name, args, &call_env);
                        } else {
                            return crate::js_function::handle_global_function(mc, name, args, env);
                        }
                    }
                    Value::AsyncClosure(cl) => {
                        return Ok(crate::js_async::handle_async_closure_call(mc, cl, this_val, args, env, None)?);
                    }
                    _ => {}
                }
            }
            Err(raise_type_error!("Not a function").into())
        }
        _ => Err(raise_type_error!("Not a function").into()),
    }
}

/// Walk up to the global environment object (top of prototype chain)
fn get_global_env<'gc>(_mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> JSObjectDataPtr<'gc> {
    // Climb prototypes until we find the global env.
    // The global env is identified by having the "globalThis" property.
    // We must NOT walk past it into Object.prototype or similar built-in prototypes.
    let mut global_env = *env;
    loop {
        if global_env
            .borrow()
            .properties
            .contains_key(&crate::core::PropertyKey::String("globalThis".to_string()))
        {
            break;
        }
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
    if let Some(rc) = slot_get_chained(&global, &InternalSlot::PromiseRuntime)
        && let Value::Object(obj) = &*rc.borrow()
    {
        return Ok(*obj);
    }
    // create runtime object and set it
    let runtime = new_js_object_data(mc);
    slot_set(mc, &global, InternalSlot::PromiseRuntime, &Value::Object(runtime));
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
    object_set_key_value(mc, &runtime, name, &Value::Object(arr))?;
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
    object_set_key_value(mc, &entry, "promise", &Value::Promise(promise))?;
    object_set_key_value(mc, &entry, "reason", &reason)?;
    object_set_key_value(mc, &entry, "tick", &Value::Number(tick as f64))?;
    // Also store the env where this rejection was observed so later processing can re-create a proper UnhandledCheck task
    object_set_key_value(mc, &entry, "env", &Value::Object(*env))?;

    // append to array
    let idx = crate::core::object_get_length(&arr).unwrap_or(0);
    object_set_key_value(mc, &arr, idx, &Value::Object(entry))?;
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
        && let Some(rc) = slot_get_chained(&runtime, &InternalSlot::UnhandledRejection)
        && let Value::String(s) = &*rc.borrow()
    {
        // consume it
        slot_set(mc, &runtime, InternalSlot::UnhandledRejection, &Value::Undefined);
        return Some(Value::String(s.clone()));
    }
    None
}

/// Clear any recorded runtime unhandled rejection if it was caused by `ptr`.
pub fn clear_runtime_unhandled_for_promise<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, ptr: usize) -> Result<(), JSError> {
    if let Ok(runtime) = ensure_promise_runtime(mc, env)
        && let Some(rc) = slot_get(&runtime, &InternalSlot::UnhandledRejectionPromisePtr)
        && let Value::Number(n) = &*rc.borrow()
        && *n as usize == ptr
    {
        // Clear both the reason and the pointer
        slot_set(mc, &runtime, InternalSlot::UnhandledRejection, &Value::Undefined);
        slot_set(mc, &runtime, InternalSlot::UnhandledRejectionPromisePtr, &Value::Undefined);
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
        && let Some(rc) = slot_get_chained(&runtime, &InternalSlot::PendingUnhandled)
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
                    object_set_key_value(mc, &new_arr, write_idx, &Value::Object(*entry))?;
                    write_idx += 1;
                }
            }
        }
        slot_set(mc, &runtime, InternalSlot::PendingUnhandled, &Value::Object(new_arr));
    }
    Ok(())
}

/// Process pending runtime unhandled checks and enqueue UnhandledCheck tasks when their grace window elapsed.
/// Returns `Ok(true)` if at least one task was enqueued.
pub fn process_runtime_pending_unhandled<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, force: bool) -> Result<bool, JSError> {
    let mut queued_any = false;
    if let Ok(runtime) = ensure_promise_runtime(mc, env)
        && let Some(rc) = slot_get_chained(&runtime, &InternalSlot::PendingUnhandled)
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
                            object_set_key_value(mc, &new_arr, write_idx, &Value::Object(*entry))?;
                            write_idx += 1;
                        }
                    } else {
                        // malformed entry: keep it
                        object_set_key_value(mc, &new_arr, write_idx, &Value::Object(*entry))?;
                        write_idx += 1;
                    }
                } else {
                    // Not yet matured: keep it for later
                    object_set_key_value(mc, &new_arr, write_idx, &Value::Object(*entry))?;
                    write_idx += 1;
                }
            }
        }
        slot_set(mc, &runtime, InternalSlot::PendingUnhandled, &Value::Object(new_arr));
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
                        if let Some(is_err_rc) = slot_get_chained(obj, &InternalSlot::IsError)
                            && let Value::Boolean(true) = &*is_err_rc.borrow()
                        {
                            allow_immediate = true;
                        }
                        if !allow_immediate && slot_get_chained(obj, &InternalSlot::Line).is_some() {
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
                            if let Some(line_rc) = slot_get_chained(obj, &InternalSlot::Line)
                                && let Value::Number(line_num) = &*line_rc.borrow()
                            {
                                let col = if let Some(col_rc) = slot_get_chained(obj, &InternalSlot::Column) {
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
        Task::ResolveThenableJob { promise, .. } => format!("ResolveThenableJob promise_ptr={:p}", Gc::as_ptr(*promise)),
        Task::AttachHandlers { promise, .. } => format!("AttachHandlers promise_ptr={:p}", Gc::as_ptr(*promise)),
        Task::ResolvePromise { promise, .. } => format!("ResolvePromise promise_ptr={:p}", Gc::as_ptr(*promise)),
        Task::DynamicImport { promise, .. } => format!("DynamicImport promise_ptr={:p}", Gc::as_ptr(*promise)),
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
        Task::ResolveThenableJob { promise, .. } => format!("ResolveThenableJob promise_ptr={:p}", Gc::as_ptr(*promise)),
        Task::AttachHandlers { promise, .. } => format!("AttachHandlers promise_ptr={:p}", Gc::as_ptr(*promise)),
        Task::ResolvePromise { promise, .. } => format!("ResolvePromise promise_ptr={:p}", Gc::as_ptr(*promise)),
        Task::DynamicImport { promise, .. } => format!("DynamicImport promise_ptr={:p}", Gc::as_ptr(*promise)),
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
    resolve: &Value<'gc>,
    reject: &Value<'gc>,
    result: &Value<'gc>,
    is_reject: bool,
    env: &JSObjectDataPtr<'gc>,
) {
    queue_task_front(
        mc,
        Task::AsyncStep {
            generator,
            resolve: resolve.clone(),
            reject: reject.clone(),
            result: result.clone(),
            is_reject,
            env: *env,
        },
    );
}

pub fn queue_dynamic_import<'gc>(
    mc: &MutationContext<'gc>,
    promise: GcPtr<'gc, JSPromise<'gc>>,
    module_specifier: Value<'gc>,
    env: JSObjectDataPtr<'gc>,
) {
    queue_task(
        mc,
        Task::DynamicImport {
            promise,
            module_specifier,
            env,
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
        Task::ResolveThenableJob { promise, .. } => format!("id={} ResolveThenableJob promise_ptr={:p}", task_id, Gc::as_ptr(*promise)),
        Task::AttachHandlers { promise, .. } => format!("id={} AttachHandlers promise_ptr={:p}", task_id, Gc::as_ptr(*promise)),
        Task::ResolvePromise { promise, .. } => format!("id={} ResolvePromise promise_ptr={:p}", task_id, Gc::as_ptr(*promise)),
        Task::DynamicImport { promise, .. } => format!("id={} DynamicImport promise_ptr={:p}", task_id, Gc::as_ptr(*promise)),
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
                Value::Closure(cl) => crate::core::call_closure(mc, cl, this_val.as_ref(), &args, &env, None),
                Value::Function(_) => call_function_with_this(mc, &function, this_val.as_ref(), &args, &env),
                _ => call_function_with_this(mc, &function, this_val.as_ref(), &args, &env),
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
        Task::ResolveThenableJob {
            promise,
            thenable,
            then_fn,
            env,
        } => {
            log::trace!("Processing ResolveThenableJob task");
            let resolve = create_resolve_function_direct(mc, promise, &env);
            let reject = create_reject_function_direct(mc, promise, &env);
            if let Err(err) = call_function_with_this(mc, &then_fn, Some(&thenable), &[resolve, reject], &env) {
                let reason = match err {
                    EvalError::Throw(v, ..) => v,
                    EvalError::Js(js_err) => crate::core::js_error_to_value(mc, &env, &js_err),
                };
                reject_promise(mc, &promise, reason, &env);
            }
        }
        Task::DynamicImport {
            promise,
            module_specifier,
            env,
        } => {
            log::trace!("Processing DynamicImport task");
            fn import_result<'gc>(
                mc: &MutationContext<'gc>,
                module_specifier: &Value<'gc>,
                env: &JSObjectDataPtr<'gc>,
            ) -> Result<Value<'gc>, EvalError<'gc>> {
                let prim = crate::core::to_primitive(mc, module_specifier, "string", env)?;
                let module_name = match prim {
                    Value::Symbol(_) => {
                        return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
                    }
                    _ => value_to_string(&prim),
                };

                let base_path = if let Some(cell) = crate::core::slot_get_chained(env, &InternalSlot::Filepath)
                    && let Value::String(s) = cell.borrow().clone()
                {
                    Some(utf16_to_utf8(&s))
                } else {
                    None
                };

                crate::js_module::load_module_for_dynamic_import(mc, &module_name, base_path.as_deref(), env)
            }

            match import_result(mc, &module_specifier, &env) {
                Ok(module_value) => resolve_promise(mc, &promise, module_value, &env),
                Err(err) => {
                    let reason = match err {
                        EvalError::Throw(val, _line, _column) => val,
                        EvalError::Js(js_err) => crate::core::js_error_to_value(mc, &env, &js_err),
                    };
                    reject_promise(mc, &promise, reason, &env);
                }
            }
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
            crate::js_async::continue_async_step_direct(mc, generator, &resolve, &reject, &step_result, &env)?;
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
                    let func_env =
                        prepare_function_call_env(mc, Some(&captured_env), this_val.as_ref(), Some(&params[..]), &args, None, None)?;
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
                    let func_env =
                        prepare_function_call_env(mc, Some(&captured_env), this_val.as_ref(), Some(&params[..]), &args, None, None)?;
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
                        slot_set(mc, &runtime, InternalSlot::UnhandledRejection, &Value::String(s));
                        // Record which promise ptr caused this so it can be cleared if a handler attaches later
                        let ptr_num = (Gc::as_ptr(promise) as usize) as f64;
                        slot_set(mc, &runtime, InternalSlot::UnhandledRejectionPromisePtr, &Value::Number(ptr_num));
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
                    Task::ResolveThenableJob { .. } => "ResolveThenableJob",
                    Task::Timeout { .. } => "Timeout",
                    Task::Interval { .. } => "Interval",
                    Task::UnhandledCheck { .. } => "UnhandledCheck",
                    Task::AttachHandlers { .. } => "AttachHandlers",
                    Task::ResolvePromise { .. } => "ResolvePromise",
                    Task::DynamicImport { .. } => "DynamicImport",
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

        if ready_index.is_none() {
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

    let closure = Value::Closure(Gc::new(
        mc,
        ClosureData::new(&[DestructuringElement::Variable("value".to_string(), None)], &body, Some(env), None),
    ));

    // Wrap as a proper function object with length=1, name="" per spec
    match wrap_closure_as_fn_obj(mc, global_env, closure, "", 1.0) {
        Ok(fn_obj) => Value::Object(fn_obj),
        Err(_) => Value::Undefined, // should not happen
    }
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

    let closure = Value::Closure(Gc::new(
        mc,
        ClosureData::new(
            &[DestructuringElement::Variable("reason".to_string(), None)],
            &body,
            Some(env),
            None,
        ),
    ));

    // Wrap as a proper function object with length=1, name="" per spec
    match wrap_closure_as_fn_obj(mc, global_env, closure, "", 1.0) {
        Ok(fn_obj) => Value::Object(fn_obj),
        Err(_) => Value::Undefined, // should not happen
    }
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
    if let Some(proto_val) = crate::core::slot_get_chained(&env, &InternalSlot::IntrinsicPromiseProto)
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        return Some(*proto_obj);
    }
    if let Some(ctor_val) = crate::core::env_get(&env, "Promise")
        && let Value::Object(ctor_obj) = &*ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(ctor_obj, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        return Some(*proto_obj);
    }
    None
}

/// Create a new JavaScript Promise object linked to Promise.prototype.
///
/// This function creates a JS object that wraps a JSPromise instance and
/// relies on the prototype chain for standard Promise methods.
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

    // Assign a stable object-side id for debugging/tracking
    let id = generate_unique_id();
    slot_set(mc, &promise_obj, InternalSlot::PromiseObjId, &Value::Number(id as f64));

    slot_set(mc, &promise_obj, InternalSlot::Promise, &Value::Promise(promise));
    Ok(promise_obj)
}

pub fn get_promise_from_js_object<'gc>(obj: &JSObjectDataPtr<'gc>) -> Option<GcPtr<'gc, JSPromise<'gc>>> {
    if let Some(promise_val) = slot_get_chained(obj, &InternalSlot::Promise)
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
    let mut promise_borrow = promise.borrow_mut(mc);
    if let PromiseState::Pending = promise_borrow.state {
        // §27.2.1.3.2 Step 6: If SameValue(resolution, promise) is true,
        // reject with TypeError
        if let Value::Object(obj) = &value
            && let Some(other_promise) = get_promise_from_js_object(obj)
            && std::ptr::eq(Gc::as_ptr(*promise), Gc::as_ptr(other_promise))
        {
            // Self-resolution: reject with TypeError
            drop(promise_borrow);
            let type_error_js = crate::raise_type_error!("Chaining cycle detected for promise");
            let type_error_val = crate::core::js_error_to_value(mc, env, &type_error_js);
            reject_promise(mc, promise, type_error_val, env);
            return;
        }

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
                        slot_set(mc, &new_env, InternalSlot::CurrentPromise, &Value::Promise(current_promise));
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
                        slot_set(mc, &new_env, InternalSlot::CurrentPromise, &Value::Promise(current_promise));
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

        // Thenable assimilation: Promise resolve must adopt objects with a callable `then`.
        if let Value::Object(obj) = &value {
            let then_value = match crate::core::get_property_with_accessors(mc, env, obj, "then") {
                Ok(v) => v,
                Err(err) => {
                    let reason = match err {
                        EvalError::Throw(v, ..) => v,
                        EvalError::Js(js_err) => crate::core::js_error_to_value(mc, env, &js_err),
                    };
                    drop(promise_borrow);
                    reject_promise(mc, promise, reason, env);
                    return;
                }
            };

            let then_callable = match &then_value {
                Value::Function(_) | Value::Closure(_) => true,
                Value::Object(o) => o.borrow().get_closure().is_some(),
                _ => false,
            };

            if !matches!(then_value, Value::Undefined | Value::Null) && then_callable {
                drop(promise_borrow);

                // §27.2.1.3.2 Step 14-15: Enqueue PromiseResolveThenableJob as a microtask
                queue_task(
                    mc,
                    Task::ResolveThenableJob {
                        promise: *promise,
                        thenable: Value::Object(*obj),
                        then_fn: then_value,
                        env: *env,
                    },
                );
                return;
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
            object_set_key_value(mc, &settled, "status", &Value::String(utf8_to_utf16("fulfilled")))?;
            object_set_key_value(mc, &settled, "value", &value)?;
            // Add to results array at idx
            object_set_key_value(mc, results_obj, idx as usize, &Value::Object(settled))?;
        }

        // Increment completed
        if let Some(completed_val_rc) = object_get_key_value(&shared_state_obj, "completed")
            && let Value::Number(completed) = &*completed_val_rc.borrow()
        {
            let new_completed = completed + 1.0;
            object_set_key_value(mc, &shared_state_obj, "completed", &Value::Number(new_completed))?;

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
            object_set_key_value(mc, &settled, "status", &Value::String(utf8_to_utf16("rejected")))?;
            object_set_key_value(mc, &settled, "reason", &reason)?;

            // Add to results array at idx
            object_set_key_value(mc, results_obj, idx as usize, &Value::Object(settled))?;
        }

        // Increment completed
        if let Some(completed_val_rc) = object_get_key_value(&shared_state_obj, "completed")
            && let Value::Number(completed) = &*completed_val_rc.borrow()
        {
            let new_completed = completed + 1.0;
            object_set_key_value(mc, &shared_state_obj, "completed", &Value::Number(new_completed))?;

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
        object_set_key_value(mc, &aggregate_error, "name", &Value::String(utf8_to_utf16("AggregateError"))).unwrap();
        object_set_key_value(
            mc,
            &aggregate_error,
            "message",
            &Value::String(utf8_to_utf16("All promises were rejected")),
        )?;

        let errors_array = new_gc_cell_ptr(mc, JSObjectData::new());
        let rejections_vec = rejections.borrow();
        for (i, rejection) in rejections_vec.iter().enumerate() {
            if let Some(err) = rejection {
                object_set_key_value(mc, &errors_array, i, &err.clone())?;
            }
        }
        object_set_key_value(mc, &aggregate_error, "errors", &Value::Object(errors_array))?;

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
        if let Some(results_rc) = slot_get_chained(state_obj, &InternalSlot::Results)
            && let Value::Object(results_arr) = &*results_rc.borrow()
        {
            // create result object
            let result_obj = new_gc_cell_ptr(mc, JSObjectData::new());
            object_set_key_value(mc, &result_obj, "status", &Value::String(utf8_to_utf16("fulfilled")))?;
            object_set_key_value(mc, &result_obj, "value", &value.clone())?;
            object_set_key_value(mc, results_arr, index, &Value::Object(result_obj))?;
        }
        // increment completed
        if let Some(comp_rc) = slot_get_chained(state_obj, &InternalSlot::Completed)
            && let Value::Number(n) = &*comp_rc.borrow()
        {
            slot_set(mc, state_obj, InternalSlot::Completed, &Value::Number(n + 1.0));
            // check for completion
            if let Some(total_rc) = slot_get_chained(state_obj, &InternalSlot::Total)
                && let Value::Number(total) = &*total_rc.borrow()
                && (n + 1.0) == *total
                && let Some(promise_rc) = slot_get_chained(state_obj, &InternalSlot::ResultPromise)
                && let Value::Promise(result_promise_ref) = &*promise_rc.borrow()
            {
                // get results array
                if let Some(results_rc2) = slot_get_chained(state_obj, &InternalSlot::Results)
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
        if let Some(results_rc) = slot_get_chained(state_obj, &InternalSlot::Results)
            && let Value::Object(results_arr) = &*results_rc.borrow()
        {
            // create result object
            let result_obj = new_gc_cell_ptr(mc, JSObjectData::new());
            object_set_key_value(mc, &result_obj, "status", &Value::String(utf8_to_utf16("rejected")))?;
            object_set_key_value(mc, &result_obj, "reason", &reason.clone())?;
            object_set_key_value(mc, results_arr, index, &Value::Object(result_obj))?;
        }
        // increment completed
        if let Some(comp_rc) = slot_get_chained(state_obj, &InternalSlot::Completed)
            && let Value::Number(n) = &*comp_rc.borrow()
        {
            slot_set(mc, state_obj, InternalSlot::Completed, &Value::Number(n + 1.0));
            // check for completion
            if let Some(total_rc) = slot_get_chained(state_obj, &InternalSlot::Total)
                && let Value::Number(total) = &*total_rc.borrow()
                && (n + 1.0) == *total
                && let Some(promise_rc) = slot_get_chained(state_obj, &InternalSlot::ResultPromise)
                && let Value::Promise(result_promise_ref) = &*promise_rc.borrow()
            {
                // get results array
                if let Some(results_rc2) = slot_get_chained(state_obj, &InternalSlot::Results)
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
#[allow(dead_code)]
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
                slot_set(mc, &env, InternalSlot::StateEnv, &Value::Object(state_env));
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
#[allow(dead_code)]
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
                slot_set(mc, &env, InternalSlot::StateEnv, &Value::Object(state_env));
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
/// Helper: get Function.prototype from the global env
fn get_function_proto<'gc>(env: &JSObjectDataPtr<'gc>) -> Option<JSObjectDataPtr<'gc>> {
    if let Some(func_rc) = object_get_key_value(env, "Function")
        && let Value::Object(func_obj) = &*func_rc.borrow()
        && let Some(proto_rc) = object_get_key_value(func_obj, "prototype")
        && let Value::Object(proto_obj) = &*proto_rc.borrow()
    {
        Some(*proto_obj)
    } else {
        None
    }
}

/// Helper: create a proper native function object (Value::Object with closure)
/// with spec-compliant length, name, and [[Prototype]] = Function.prototype.
fn make_native_fn_obj<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    display_name: &str,
    length: f64,
    internal_dispatch: &str,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let fn_obj = new_js_object_data(mc);
    fn_obj
        .borrow_mut(mc)
        .set_closure(Some(new_gc_cell_ptr(mc, Value::Function(internal_dispatch.to_string()))));
    if let Some(fp) = get_function_proto(env) {
        fn_obj.borrow_mut(mc).prototype = Some(fp);
    }
    // name property: non-writable, non-enumerable, configurable
    let name_d = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16(display_name)), false, false, true)?;
    crate::js_object::define_property_internal(mc, &fn_obj, "name", &name_d)?;
    // length property: non-writable, non-enumerable, configurable
    let len_d = crate::core::create_descriptor_object(mc, &Value::Number(length), false, false, true)?;
    crate::js_object::define_property_internal(mc, &fn_obj, "length", &len_d)?;
    Ok(fn_obj)
}

/// Helper: wrap an arbitrary closure in a function object with length and name.
#[allow(dead_code)]
fn wrap_closure_as_fn_obj<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    closure: Value<'gc>,
    display_name: &str,
    length: f64,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let fn_obj = new_js_object_data(mc);
    slot_set(mc, &fn_obj, InternalSlot::Callable, &Value::Boolean(true));
    // Mark the closure as arrow-like (non-constructible) — spec built-in functions are not constructors
    let marked_closure = match closure {
        Value::Closure(cl) => {
            let mut data = (*cl).clone();
            data.is_arrow = true; // prevents `new` from succeeding (object_has_construct returns false)
            Value::Closure(Gc::new(mc, data))
        }
        other => other,
    };
    fn_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, marked_closure)));
    if let Some(fp) = get_function_proto(env) {
        fn_obj.borrow_mut(mc).prototype = Some(fp);
    }
    // name and length — spec order is: length first, then name
    let len_d = crate::core::create_descriptor_object(mc, &Value::Number(length), false, false, true)?;
    crate::js_object::define_property_internal(mc, &fn_obj, "length", &len_d)?;
    let name_d = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16(display_name)), false, false, true)?;
    crate::js_object::define_property_internal(mc, &fn_obj, "name", &name_d)?;
    Ok(fn_obj)
}

/// Check whether a Value is callable (function, closure, or callable object).
#[allow(dead_code)]
fn is_callable_val<'gc>(v: &Value<'gc>) -> bool {
    match v {
        Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) => true,
        Value::Object(o) => {
            o.borrow().get_closure().is_some()
                || slot_get_chained(o, &InternalSlot::Callable).is_some()
                || slot_get_chained(o, &InternalSlot::IsConstructor).is_some()
                || o.borrow().class_def.is_some()
                || slot_get_chained(o, &InternalSlot::NativeCtor).is_some()
        }
        _ => false,
    }
}

/// Check whether a Value is a constructor.
#[allow(dead_code)]
fn is_constructor_val<'gc>(v: &Value<'gc>) -> bool {
    match v {
        // Plain function declarations/expressions are constructors
        Value::Function(_) => true,
        // Class definitions are constructors
        Value::ClassDefinition(_) => true,
        // Closures are NOT constructors (arrow functions, etc.)
        Value::Closure(_) | Value::AsyncClosure(_) => false,
        Value::Object(o) => {
            // Objects with closures that are not explicitly non-constructor (arrow functions)
            // are constructors — this covers function declarations/expressions stored as objects
            if let Some(cl_ptr) = o.borrow().get_closure() {
                // Check the ClosureData.is_arrow flag
                let is_arrow_closure = match &*cl_ptr.borrow() {
                    Value::Closure(cl) | Value::AsyncClosure(cl) => cl.is_arrow,
                    Value::GeneratorFunction(..) | Value::AsyncGeneratorFunction(..) => false,
                    _ => false,
                };
                if is_arrow_closure {
                    return false;
                }
                // Also check the InternalSlot::IsArrowFunction marker
                if let Some(arrow_rc) = slot_get_chained(o, &InternalSlot::IsArrowFunction)
                    && matches!(*arrow_rc.borrow(), Value::Boolean(true))
                {
                    return false;
                }
                return true;
            }
            slot_get_chained(o, &InternalSlot::IsConstructor).is_some()
                || o.borrow().class_def.is_some()
                || slot_get_chained(o, &InternalSlot::NativeCtor).is_some()
        }
        _ => false,
    }
}

/// Check whether a Value has Object type in the ECMAScript sense.
/// In JS spec, functions are objects. Our engine represents them differently.
fn is_object_type<'gc>(v: &Value<'gc>) -> bool {
    matches!(
        v,
        Value::Object(_)
            | Value::Function(_)
            | Value::Closure(_)
            | Value::AsyncClosure(_)
            | Value::ClassDefinition(_)
            | Value::GeneratorFunction(..)
            | Value::AsyncGeneratorFunction(..)
    )
}

/// Check whether a Value is the built-in Promise constructor (not a subclass).
fn is_default_promise_ctor<'gc>(v: &Value<'gc>, env: &JSObjectDataPtr<'gc>) -> bool {
    if let Value::Object(obj) = v {
        // Use slot_get (own property only) — NOT slot_get_chained, because
        // subclasses that extend Promise would inherit NativeCtor through the chain
        if let Some(nc_rc) = slot_get(obj, &InternalSlot::NativeCtor)
            && let Value::String(s) = &*nc_rc.borrow()
        {
            let name = crate::unicode::utf16_to_utf8(s);
            return name == "Promise";
        }
        // Also check identity with the global Promise constructor
        let default_ctor = get_default_promise_ctor(env);
        if let Value::Object(def_obj) = &default_ctor {
            return std::ptr::eq(&*obj.borrow() as *const _, &*def_obj.borrow() as *const _);
        }
    }
    false
}

/// SpeciesConstructor(O, defaultConstructor) — §7.3.20
/// Looks up O.constructor[Symbol.species], falling back to defaultConstructor.
fn get_species_constructor<'gc>(
    mc: &MutationContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    // §7.3.20 SpeciesConstructor(O, defaultConstructor)
    // Step 1: Let C = Get(O, "constructor") — must trigger getters
    let ctor_val = crate::core::get_property_with_accessors(mc, env, obj, "constructor")?;

    // Step 2: If C is undefined, return defaultConstructor
    if matches!(ctor_val, Value::Undefined) {
        return Ok(None);
    }

    // Step 3: If Type(C) is not Object, throw TypeError
    if !is_object_type(&ctor_val) {
        return Err(raise_type_error!("Species constructor: constructor is not an object").into());
    }

    let ctor_obj = match &ctor_val {
        Value::Object(o) => o,
        // For Value::Function/ClassDefinition etc (which are object types), just return as-is
        _ => return Ok(Some(ctor_val)),
    };

    // Step 4: Let S = Get(C, @@species)
    // Look up the well-known Symbol.species the same way as ArraySpeciesCreate
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_val.borrow()
        && let Some(species_sym_val) = object_get_key_value(sym_obj, "species")
        && let Value::Symbol(species_sym) = &*species_sym_val.borrow()
    {
        let species = crate::core::get_property_with_accessors(mc, env, ctor_obj, *species_sym)?;
        // Step 5: If S is undefined or null, return defaultConstructor
        match species {
            Value::Null | Value::Undefined => return Ok(None),
            other => {
                // Step 6: If IsConstructor(S), return S
                if is_constructor_val(&other) {
                    return Ok(Some(other));
                }
                // Step 7: Throw TypeError
                return Err(raise_type_error!("Species constructor: species is not a constructor").into());
            }
        }
    }
    // Fallback: try string key "Symbol.species"
    if let Ok(species) = crate::core::get_property_with_accessors(mc, env, ctor_obj, "Symbol.species") {
        match species {
            Value::Null | Value::Undefined => return Ok(None),
            other => {
                if is_constructor_val(&other) {
                    return Ok(Some(other));
                }
                return Err(raise_type_error!("Species constructor: species is not a constructor").into());
            }
        }
    }
    Ok(None)
}

/// NewPromiseCapability(C) per ECMAScript spec §27.2.1.5.
/// Creates a GetCapabilitiesExecutor, calls `new C(executor)`, and returns
/// (promiseObj, resolveFunction, rejectFunction).
#[allow(dead_code)]
pub fn new_promise_capability<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    constructor: &Value<'gc>,
) -> Result<(Value<'gc>, Value<'gc>, Value<'gc>), EvalError<'gc>> {
    // Step 1: Check IsConstructor(C)
    if !is_constructor_val(constructor) {
        return Err(raise_type_error!("Promise.all/resolve/etc: this is not a constructor").into());
    }

    // Step 2: Create the PromiseCapability record (as an env object)
    let cap_env = new_js_object_data(mc);
    cap_env.borrow_mut(mc).prototype = Some(*env); // inherit globals for __internal_capability_executor lookup
    // Self-reference so the executor closure's body can find it via env chain lookup
    object_set_key_value(mc, &cap_env, "__cap_env_ref", &Value::Object(cap_env))?;

    // Step 3: Create the GetCapabilitiesExecutor function
    // Body: calls __internal_capability_executor(__cap_res_arg, __cap_rej_arg)
    let executor_closure = Value::Closure(Gc::new(
        mc,
        ClosureData::new(
            &[
                DestructuringElement::Variable("__cap_res_arg".to_string(), None),
                DestructuringElement::Variable("__cap_rej_arg".to_string(), None),
            ],
            &[stmt_expr(Expr::Call(
                Box::new(Expr::Var("__internal_capability_executor".to_string(), None, None)),
                vec![
                    Expr::Var("__cap_res_arg".to_string(), None, None),
                    Expr::Var("__cap_rej_arg".to_string(), None, None),
                ],
            ))],
            Some(cap_env),
            None,
        ),
    ));
    // Wrap as a function object with length=2, name=""
    let executor_fn = wrap_closure_as_fn_obj(mc, env, executor_closure, "", 2.0)?;

    // Step 4: Call new C(executor)
    let promise_obj = crate::js_class::evaluate_new(mc, env, constructor, &[Value::Object(executor_fn)], None)?;

    // Steps 5-6: Retrieve captured resolve/reject
    let resolve = object_get_key_value(&cap_env, "__cap_resolve")
        .map(|rc| rc.borrow().clone())
        .unwrap_or(Value::Undefined);
    let reject = object_get_key_value(&cap_env, "__cap_reject")
        .map(|rc| rc.borrow().clone())
        .unwrap_or(Value::Undefined);

    // Steps 7-8: Validate callability
    if !is_callable_val(&resolve) {
        return Err(raise_type_error!("Promise capability: resolve is not callable").into());
    }
    if !is_callable_val(&reject) {
        return Err(raise_type_error!("Promise capability: reject is not callable").into());
    }

    Ok((promise_obj, resolve, reject))
}

/// Handler for the GetCapabilitiesExecutor internal function.
/// Called when the capability executor closure body runs.
/// Reads args[0]=resolve, args[1]=reject and stores them into the cap_env object
/// found via `__cap_env_ref` in the current env chain.
pub fn __internal_capability_executor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let resolve_arg = args.first().cloned().unwrap_or(Value::Undefined);
    let reject_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

    if let Some(cap_ref_rc) = crate::core::env_get(env, "__cap_env_ref") {
        let cap_val = cap_ref_rc.borrow().clone();
        if let Value::Object(cap_obj) = cap_val {
            // Check: if [[Resolve]] is already non-undefined, throw
            let resolve_already_non_undef = object_get_key_value(&cap_obj, "__cap_resolve")
                .map(|rc| !matches!(*rc.borrow(), Value::Undefined))
                .unwrap_or(false);
            if resolve_already_non_undef {
                return Err(raise_type_error!("GetCapabilitiesExecutor: [[Resolve]] already set").into());
            }
            // Check: if [[Reject]] is already non-undefined, throw
            let reject_already_non_undef = object_get_key_value(&cap_obj, "__cap_reject")
                .map(|rc| !matches!(*rc.borrow(), Value::Undefined))
                .unwrap_or(false);
            if reject_already_non_undef {
                return Err(raise_type_error!("GetCapabilitiesExecutor: [[Reject]] already set").into());
            }
            // Set both (even if undefined)
            object_set_key_value(mc, &cap_obj, "__cap_resolve", &resolve_arg)?;
            object_set_key_value(mc, &cap_obj, "__cap_reject", &reject_arg)?;
        }
    }
    Ok(Value::Undefined)
}

/// Get the `this` value from the current execution environment.
#[allow(dead_code)]
fn get_static_this<'gc>(env: &JSObjectDataPtr<'gc>) -> Value<'gc> {
    crate::core::env_get(env, "this")
        .map(|rc| rc.borrow().clone())
        .unwrap_or(Value::Undefined)
}

/// Get the default Promise constructor from the global env.
#[allow(dead_code)]
/// Convert an EvalError into a JS Value for use in IfAbruptRejectPromise patterns.
fn eval_error_to_value<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, err: EvalError<'gc>) -> Value<'gc> {
    match err {
        EvalError::Throw(v, ..) => v,
        EvalError::Js(j) => crate::core::js_error_to_value(mc, env, &j),
    }
}

fn get_default_promise_ctor<'gc>(env: &JSObjectDataPtr<'gc>) -> Value<'gc> {
    if let Some(rc) = slot_get_chained(env, &InternalSlot::IntrinsicPromiseCtor) {
        rc.borrow().clone()
    } else {
        object_get_key_value(env, "Promise")
            .map(|rc| rc.borrow().clone())
            .unwrap_or(Value::Undefined)
    }
}

/// Call `.then(onFulfilled, onRejected)` on a thenable value.
/// Falls back to native perform_promise_then if it's a native JSPromise.
#[allow(dead_code)]
fn call_then_on_thenable<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    thenable: &Value<'gc>,
    on_fulfilled: Value<'gc>,
    on_rejected: Value<'gc>,
) -> Result<(), JSError> {
    if let Value::Object(obj) = thenable {
        if let Some(promise_ref) = get_promise_from_js_object(obj) {
            // Fast path: native JSPromise
            perform_promise_then(mc, promise_ref, Some(on_fulfilled), Some(on_rejected), None, env)?;
            return Ok(());
        }
        // Slow path: dynamic .then() call
        let then_fn = crate::core::get_property_with_accessors(mc, env, obj, "then")?;
        if is_callable_val(&then_fn) {
            let _ = call_function_with_this(mc, &then_fn, Some(thenable), &[on_fulfilled, on_rejected], env)?;
        }
    }
    Ok(())
}

pub fn initialize_promise<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let promise_ctor = new_js_object_data(mc);
    slot_set(mc, &promise_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(
        mc,
        &promise_ctor,
        InternalSlot::NativeCtor,
        &Value::String(utf8_to_utf16("Promise")),
    );
    // Set [[Prototype]] = Function.prototype
    if let Some(fp) = get_function_proto(env) {
        promise_ctor.borrow_mut(mc).prototype = Some(fp);
    }

    // Setup prototype (inherits from Object.prototype)
    let promise_proto = new_js_object_data(mc);
    let _ = crate::core::set_internal_prototype_from_constructor(mc, &promise_proto, env, "Object");

    // Promise.prototype — non-writable, non-enumerable, non-configurable
    {
        let desc = crate::core::create_descriptor_object(mc, &Value::Object(promise_proto), false, false, false)?;
        crate::js_object::define_property_internal(mc, &promise_ctor, "prototype", &desc)?;
    }
    // Promise.prototype.constructor — writable, non-enumerable, configurable
    {
        let desc = crate::core::create_descriptor_object(mc, &Value::Object(promise_ctor), true, false, true)?;
        crate::js_object::define_property_internal(mc, &promise_proto, "constructor", &desc)?;
    }

    // Promise.length = 1 — non-writable, non-enumerable, configurable
    {
        let desc = crate::core::create_descriptor_object(mc, &Value::Number(1.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &promise_ctor, "length", &desc)?;
    }
    // Promise.name = "Promise" — non-writable, non-enumerable, configurable
    {
        let desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("Promise")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &promise_ctor, "name", &desc)?;
    }

    // Static methods — writable, non-enumerable, configurable
    let static_methods: &[(&str, f64)] = &[
        ("all", 1.0),
        ("allSettled", 1.0),
        ("any", 1.0),
        ("race", 1.0),
        ("resolve", 1.0),
        ("reject", 1.0),
    ];
    for (method, arity) in static_methods {
        let fn_obj = make_native_fn_obj(mc, env, method, *arity, &format!("Promise.{}", method))?;
        let desc = crate::core::create_descriptor_object(mc, &Value::Object(fn_obj), true, false, true)?;
        crate::js_object::define_property_internal(mc, &promise_ctor, *method, &desc)?;
    }

    // Symbol.species accessor on the constructor — get [Symbol.species]() { return this; }
    if let Some(sym_rc) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_rc.borrow()
        && let Some(species_rc) = object_get_key_value(sym_obj, "species")
        && let Value::Symbol(species_sym) = &*species_rc.borrow()
    {
        let getter_fn = make_native_fn_obj(mc, env, "get [Symbol.species]", 0.0, "Promise.species")?;
        let desc = new_js_object_data(mc);
        object_set_key_value(mc, &desc, "get", &Value::Object(getter_fn))?;
        object_set_key_value(mc, &desc, "enumerable", &Value::Boolean(false))?;
        object_set_key_value(mc, &desc, "configurable", &Value::Boolean(true))?;
        crate::js_object::define_property_internal(mc, &promise_ctor, crate::core::PropertyKey::Symbol(*species_sym), &desc)?;
    }

    // Prototype methods — writable, non-enumerable, configurable
    let proto_methods: &[(&str, f64)] = &[("then", 2.0), ("catch", 1.0), ("finally", 1.0)];
    for (method, arity) in proto_methods {
        let fn_obj = make_native_fn_obj(mc, env, method, *arity, &format!("Promise.prototype.{}", method))?;
        let desc = crate::core::create_descriptor_object(mc, &Value::Object(fn_obj), true, false, true)?;
        crate::js_object::define_property_internal(mc, &promise_proto, *method, &desc)?;
    }

    // Promise.prototype[Symbol.toStringTag] = "Promise" — non-writable, non-enumerable, configurable
    if let Some(sym_rc) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_rc.borrow()
        && let Some(tag_rc) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_rc.borrow()
    {
        let desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("Promise")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &promise_proto, crate::core::PropertyKey::Symbol(*tag_sym), &desc)?;
    }

    crate::core::env_set(mc, env, "Promise", &Value::Object(promise_ctor))?;

    // Preserve intrinsic references for internal use
    slot_set(mc, env, InternalSlot::IntrinsicPromiseCtor, &Value::Object(promise_ctor));
    slot_set(mc, env, InternalSlot::IntrinsicPromiseProto, &Value::Object(promise_proto));

    // Internal helpers registered in global env
    let helpers = [
        "__internal_promise_resolve_captured",
        "__internal_promise_reject_captured",
        "__internal_promise_finally_resolve",
        "__internal_promise_finally_reject",
        "__internal_promise_all_resolve",
        "__internal_promise_all_reject",
        "__internal_allsettled_resolve",
        "__internal_allsettled_reject",
        "__internal_any_resolve",
        "__internal_any_reject",
        "__internal_capability_executor",
    ];
    for h in helpers {
        crate::core::env_set(mc, env, h, &Value::Function(h.to_string()))?;
    }

    Ok(())
}

// Missing helpers for Promise.all
pub fn __internal_promise_all_resolve<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
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
        // Check [[AlreadyCalled]] guard for this index
        let already_key = format!(
            "__already_{}",
            match index_val {
                Value::Number(n) => *n as usize,
                _ => return Ok(Value::Undefined),
            }
        );
        if let Some(already_rc) = object_get_key_value(state_obj, &already_key)
            && matches!(*already_rc.borrow(), Value::Boolean(true))
        {
            return Ok(Value::Undefined);
        }
        object_set_key_value(mc, state_obj, &already_key, &Value::Boolean(true))?;

        let idx_str = match index_val {
            Value::Number(n) => n.to_string(),
            _ => return Ok(Value::Undefined),
        };
        object_set_key_value(mc, results_obj, &idx_str, &value.clone())?;
        // Also set length on results array
        let idx_num = match index_val {
            Value::Number(n) => *n as usize,
            _ => 0,
        };
        if let Some(len_rc) = object_get_key_value(results_obj, "length")
            && let Value::Number(cur_len) = &*len_rc.borrow()
            && (idx_num + 1) as f64 > *cur_len
        {
            object_set_key_value(mc, results_obj, "length", &Value::Number((idx_num + 1) as f64))?;
        }

        // Decrement remainingElementsCount
        if let Some(remaining_rc) = object_get_key_value(state_obj, "remaining")
            && let Value::Number(remaining) = &*remaining_rc.borrow()
        {
            let new_remaining = remaining - 1.0;
            object_set_key_value(mc, state_obj, "remaining", &Value::Number(new_remaining))?;

            if new_remaining == 0.0 {
                // Use capability resolve
                if let Some(cap_resolve_rc) = object_get_key_value(state_obj, "cap_resolve") {
                    let cap_resolve = cap_resolve_rc.borrow().clone();
                    call_function(mc, &cap_resolve, &[Value::Object(*results_obj)], env)?;
                }
            }
        }
    }
    Ok(Value::Undefined)
}

pub fn __internal_promise_all_reject<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.len() < 2 {
        return Ok(Value::Undefined);
    }
    let reason = &args[0];
    let state_val = &args[1];

    if let Value::Object(state_obj) = state_val {
        // Use capability reject (spec: Call(promiseCapability.[[Reject]], undefined, «reason»))
        if let Some(cap_reject_rc) = object_get_key_value(state_obj, "cap_reject") {
            let cap_reject = cap_reject_rc.borrow().clone();
            call_function(mc, &cap_reject, std::slice::from_ref(reason), env)?;
        }
    }
    Ok(Value::Undefined)
}

/// Internal resolve callback for Promise.allSettled — creates {status:"fulfilled", value} record
pub fn __internal_allsettled_resolve<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
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
        let idx = match index_val {
            Value::Number(n) => *n as usize,
            _ => return Ok(Value::Undefined),
        };

        // [[AlreadyCalled]] guard
        let already_key = format!("already_{}", idx);
        if let Some(already_rc) = object_get_key_value(state_obj, &already_key)
            && matches!(*already_rc.borrow(), Value::Boolean(true))
        {
            return Ok(Value::Undefined);
        }
        object_set_key_value(mc, state_obj, &already_key, &Value::Boolean(true))?;

        // Build {status: "fulfilled", value} record
        let result_obj = new_gc_cell_ptr(mc, JSObjectData::new());
        object_set_key_value(mc, &result_obj, "status", &Value::String(utf8_to_utf16("fulfilled")))?;
        object_set_key_value(mc, &result_obj, "value", &value.clone())?;
        object_set_key_value(mc, results_obj, idx, &Value::Object(result_obj))?;

        // Decrement remainingElementsCount
        if let Some(remaining_rc) = object_get_key_value(state_obj, "remaining")
            && let Value::Number(remaining) = &*remaining_rc.borrow()
        {
            let new_remaining = remaining - 1.0;
            object_set_key_value(mc, state_obj, "remaining", &Value::Number(new_remaining))?;
            if new_remaining == 0.0
                && let Some(cap_resolve_rc) = object_get_key_value(state_obj, "cap_resolve")
            {
                let cap_resolve = cap_resolve_rc.borrow().clone();
                call_function(mc, &cap_resolve, &[Value::Object(*results_obj)], env)?;
            }
        }
    }
    Ok(Value::Undefined)
}

/// Internal reject callback for Promise.allSettled — creates {status:"rejected", reason} record
pub fn __internal_allsettled_reject<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.len() < 3 {
        return Ok(Value::Undefined);
    }
    let index_val = &args[0];
    let reason = &args[1];
    let state_val = &args[2];

    if let Value::Object(state_obj) = state_val
        && let Some(results_val_rc) = object_get_key_value(state_obj, "results")
        && let Value::Object(results_obj) = &*results_val_rc.borrow()
    {
        let idx = match index_val {
            Value::Number(n) => *n as usize,
            _ => return Ok(Value::Undefined),
        };

        // [[AlreadyCalled]] guard
        let already_key = format!("already_{}", idx);
        if let Some(already_rc) = object_get_key_value(state_obj, &already_key)
            && matches!(*already_rc.borrow(), Value::Boolean(true))
        {
            return Ok(Value::Undefined);
        }
        object_set_key_value(mc, state_obj, &already_key, &Value::Boolean(true))?;

        // Build {status: "rejected", reason} record
        let result_obj = new_gc_cell_ptr(mc, JSObjectData::new());
        object_set_key_value(mc, &result_obj, "status", &Value::String(utf8_to_utf16("rejected")))?;
        object_set_key_value(mc, &result_obj, "reason", &reason.clone())?;
        object_set_key_value(mc, results_obj, idx, &Value::Object(result_obj))?;

        // Decrement remainingElementsCount
        if let Some(remaining_rc) = object_get_key_value(state_obj, "remaining")
            && let Value::Number(remaining) = &*remaining_rc.borrow()
        {
            let new_remaining = remaining - 1.0;
            object_set_key_value(mc, state_obj, "remaining", &Value::Number(new_remaining))?;
            if new_remaining == 0.0
                && let Some(cap_resolve_rc) = object_get_key_value(state_obj, "cap_resolve")
            {
                let cap_resolve = cap_resolve_rc.borrow().clone();
                call_function(mc, &cap_resolve, &[Value::Object(*results_obj)], env)?;
            }
        }
    }
    Ok(Value::Undefined)
}

/// Internal resolve callback for Promise.any — first fulfilled value resolves the capability
pub fn __internal_any_resolve<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.is_empty() {
        return Ok(Value::Undefined);
    }
    let value = &args[0];
    let state_val = if args.len() > 1 { &args[1] } else { return Ok(Value::Undefined) };

    if let Value::Object(state_obj) = state_val
        && let Some(cap_resolve_rc) = object_get_key_value(state_obj, "cap_resolve")
    {
        let cap_resolve = cap_resolve_rc.borrow().clone();
        call_function(mc, &cap_resolve, std::slice::from_ref(value), env)?;
    }
    Ok(Value::Undefined)
}

/// Internal reject callback for Promise.any — stores reason, creates AggregateError when all rejected
pub fn __internal_any_reject<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.len() < 3 {
        return Ok(Value::Undefined);
    }
    let index_val = &args[0];
    let reason = &args[1];
    let state_val = &args[2];

    if let Value::Object(state_obj) = state_val
        && let Some(errors_val_rc) = object_get_key_value(state_obj, "errors")
        && let Value::Object(errors_obj) = &*errors_val_rc.borrow()
    {
        let idx = match index_val {
            Value::Number(n) => *n as usize,
            _ => return Ok(Value::Undefined),
        };

        // [[AlreadyCalled]] guard
        let already_key = format!("already_{}", idx);
        if let Some(already_rc) = object_get_key_value(state_obj, &already_key)
            && matches!(*already_rc.borrow(), Value::Boolean(true))
        {
            return Ok(Value::Undefined);
        }
        object_set_key_value(mc, state_obj, &already_key, &Value::Boolean(true))?;

        object_set_key_value(mc, errors_obj, idx, &reason.clone())?;

        // Decrement remainingElementsCount
        if let Some(remaining_rc) = object_get_key_value(state_obj, "remaining")
            && let Value::Number(remaining) = &*remaining_rc.borrow()
        {
            let new_remaining = remaining - 1.0;
            object_set_key_value(mc, state_obj, "remaining", &Value::Number(new_remaining))?;
            if new_remaining == 0.0 {
                // All rejected — create AggregateError
                let aggregate_error = new_gc_cell_ptr(mc, JSObjectData::new());
                object_set_key_value(mc, &aggregate_error, "name", &Value::String(utf8_to_utf16("AggregateError")))?;
                object_set_key_value(
                    mc,
                    &aggregate_error,
                    "message",
                    &Value::String(utf8_to_utf16("All promises were rejected")),
                )?;
                object_set_key_value(mc, &aggregate_error, "errors", &Value::Object(*errors_obj))?;
                // Set prototype to Error.prototype chain
                if let Some(agg_ctor_rc) = crate::core::env_get(env, "AggregateError") {
                    let agg_ctor = agg_ctor_rc.borrow().clone();
                    if let Value::Object(agg_obj) = &agg_ctor
                        && let Some(proto_rc) = object_get_key_value(agg_obj, "prototype")
                    {
                        let proto = proto_rc.borrow().clone();
                        if let Value::Object(proto_obj) = proto {
                            aggregate_error.borrow_mut(mc).prototype = Some(proto_obj);
                        }
                    }
                }

                if let Some(cap_reject_rc) = object_get_key_value(state_obj, "cap_reject") {
                    let cap_reject = cap_reject_rc.borrow().clone();
                    call_function(mc, &cap_reject, &[Value::Object(aggregate_error)], env)?;
                }
            }
        }
    }
    Ok(Value::Undefined)
}

pub fn handle_promise_static_method_val<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    this_val: Option<&Value<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Determine the constructor C from `this` — default to built-in Promise
    let default_ctor = get_default_promise_ctor(env);
    let c = this_val.unwrap_or(&default_ctor);

    match method {
        "resolve" => {
            let val = if args.is_empty() { Value::Undefined } else { args[0].clone() };

            // Step 2: If Type(C) is not Object, throw TypeError
            // In the spec, functions are objects; our engine represents them as Value::Function/Closure/Object
            if !is_object_type(c) {
                return Err(raise_type_error!("Promise.resolve requires that 'this' be an Object").into());
            }

            // Step 3: If IsPromise(x) and x.constructor === C, return x
            if let Value::Object(obj) = &val
                && get_promise_from_js_object(obj).is_some()
            {
                // Check if val.constructor === C
                if let Some(ctor_rc) = object_get_key_value(obj, "constructor") {
                    let ctor_val = ctor_rc.borrow().normalize_slot();
                    // Use referential equality for objects
                    let same = match (&ctor_val, c) {
                        (Value::Object(a), Value::Object(b)) => std::ptr::eq(&*a.borrow() as *const _, &*b.borrow() as *const _),
                        _ => false,
                    };
                    if same {
                        return Ok(val.clone());
                    }
                }
            }

            // Step 4: NewPromiseCapability(C)
            if is_constructor_val(c) && !is_default_promise_ctor(c, env) {
                let (promise_obj, resolve_fn, _reject_fn) = new_promise_capability(mc, env, c)?;
                call_function(mc, &resolve_fn, &[val], env)?;
                return Ok(promise_obj);
            }

            // Fast path: C is the built-in Promise constructor
            let promise = new_gc_cell_ptr(mc, JSPromise::new());
            let promise_obj = make_promise_js_object(mc, promise, Some(*env))?;
            resolve_promise(mc, &promise, val, env);
            Ok(Value::Object(promise_obj))
        }
        "reject" => {
            // Step 1: Let C be the this value
            // Step 2: If Type(C) is not Object, throw TypeError
            if !is_object_type(c) {
                return Err(raise_type_error!("Promise.reject requires that 'this' be an Object").into());
            }

            let val = if args.is_empty() { Value::Undefined } else { args[0].clone() };

            // Step 3: NewPromiseCapability(C)
            if is_constructor_val(c) && !is_default_promise_ctor(c, env) {
                let (promise_obj, _resolve_fn, reject_fn) = new_promise_capability(mc, env, c)?;
                call_function(mc, &reject_fn, &[val], env)?;
                return Ok(promise_obj);
            }

            // Fast path: C is the built-in Promise constructor
            let promise = new_gc_cell_ptr(mc, JSPromise::new());
            let promise_obj = make_promise_js_object(mc, promise, Some(*env))?;
            reject_promise(mc, &promise, val, env);
            Ok(Value::Object(promise_obj))
        }
        "allSettled" => {
            // §27.2.4.2 Promise.allSettled ( iterable )
            if !is_object_type(c) {
                return Err(raise_type_error!("Promise.allSettled requires that 'this' be an Object").into());
            }

            let (cap_promise, cap_resolve, cap_reject) = new_promise_capability(mc, env, c)?;

            let reject_cap = |mc2: &MutationContext<'gc>, e: EvalError<'gc>| -> Value<'gc> {
                let reason = eval_error_to_value(mc2, env, e);
                let _ = call_function(mc2, &cap_reject, &[reason], env);
                cap_promise.clone()
            };

            let promise_resolve = match c {
                Value::Object(c_obj) => match crate::core::get_property_with_accessors(mc, env, c_obj, "resolve") {
                    Ok(v) => v,
                    Err(e) => return Ok(reject_cap(mc, e)),
                },
                _ => Value::Undefined,
            };
            if !is_callable_val(&promise_resolve) {
                let reason = crate::core::js_error_to_value(mc, env, &raise_type_error!("Promise.allSettled: resolve is not a function"));
                let _ = call_function(mc, &cap_reject, &[reason], env);
                return Ok(cap_promise);
            }

            let iterable = if args.is_empty() { Value::Undefined } else { args[0].clone() };
            let (iter_obj, next_fn) = match crate::js_map::get_iterator(mc, env, &iterable) {
                Ok(v) => v,
                Err(e) => return Ok(reject_cap(mc, e)),
            };

            let results_obj = match crate::js_array::create_array(mc, env) {
                Ok(r) => r,
                Err(e) => return Ok(reject_cap(mc, e.into())),
            };
            let state_obj = new_js_object_data(mc);
            object_set_key_value(mc, &state_obj, "results", &Value::Object(results_obj))?;
            object_set_key_value(mc, &state_obj, "remaining", &Value::Number(1.0))?;
            object_set_key_value(mc, &state_obj, "cap_resolve", &cap_resolve)?;
            object_set_key_value(mc, &state_obj, "cap_reject", &cap_reject)?;

            let mut index: usize = 0;

            loop {
                let next_result = match crate::js_map::call_iterator_next(mc, env, &iter_obj, &next_fn) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(reject_cap(mc, e));
                    }
                };
                let done = match crate::js_map::get_iterator_done(mc, env, &next_result) {
                    Ok(d) => d,
                    Err(e) => {
                        return Ok(reject_cap(mc, e));
                    }
                };
                if done {
                    if let Some(rem_rc) = object_get_key_value(&state_obj, "remaining")
                        && let Value::Number(rem) = &*rem_rc.borrow()
                    {
                        let new_rem = rem - 1.0;
                        object_set_key_value(mc, &state_obj, "remaining", &Value::Number(new_rem))?;
                        if new_rem == 0.0 {
                            let _ = call_function(mc, &cap_resolve, &[Value::Object(results_obj)], env);
                        }
                    }
                    return Ok(cap_promise);
                }

                let next_value = match crate::js_map::get_iterator_value(mc, env, &next_result) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(reject_cap(mc, e));
                    }
                };

                object_set_key_value(mc, &results_obj, index, &Value::Undefined)?;
                object_set_key_value(mc, &results_obj, "length", &Value::Number((index + 1) as f64))?;

                let next_promise = match call_function_with_this(mc, &promise_resolve, Some(c), &[next_value], env) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                        return Ok(reject_cap(mc, e));
                    }
                };

                // Create onFulfilled: stores {status:"fulfilled", value} and decrements remaining
                let then_cb = {
                    let cb_env = new_js_object_data(mc);
                    cb_env.borrow_mut(mc).prototype = Some(*env);
                    object_set_key_value(mc, &cb_env, "pstate", &Value::Object(state_obj))?;
                    let raw_cl = Value::Closure(Gc::new(
                        mc,
                        ClosureData::new(
                            &[DestructuringElement::Variable("value".to_string(), None)],
                            &[stmt_expr(Expr::Call(
                                Box::new(Expr::Var("__internal_allsettled_resolve".to_string(), None, None)),
                                vec![
                                    Expr::Number(index as f64),
                                    Expr::Var("value".to_string(), None, None),
                                    Expr::Var("pstate".to_string(), None, None),
                                ],
                            ))],
                            Some(cb_env),
                            None,
                        ),
                    ));
                    Value::Object(wrap_closure_as_fn_obj(mc, env, raw_cl, "", 1.0)?)
                };
                // Create onRejected: stores {status:"rejected", reason} and decrements remaining
                let catch_cb = {
                    let cb_env = new_js_object_data(mc);
                    cb_env.borrow_mut(mc).prototype = Some(*env);
                    object_set_key_value(mc, &cb_env, "pstate", &Value::Object(state_obj))?;
                    let raw_cl = Value::Closure(Gc::new(
                        mc,
                        ClosureData::new(
                            &[DestructuringElement::Variable("reason".to_string(), None)],
                            &[stmt_expr(Expr::Call(
                                Box::new(Expr::Var("__internal_allsettled_reject".to_string(), None, None)),
                                vec![
                                    Expr::Number(index as f64),
                                    Expr::Var("reason".to_string(), None, None),
                                    Expr::Var("pstate".to_string(), None, None),
                                ],
                            ))],
                            Some(cb_env),
                            None,
                        ),
                    ));
                    Value::Object(wrap_closure_as_fn_obj(mc, env, raw_cl, "", 1.0)?)
                };

                if let Some(rem_rc) = object_get_key_value(&state_obj, "remaining")
                    && let Value::Number(rem) = &*rem_rc.borrow()
                {
                    object_set_key_value(mc, &state_obj, "remaining", &Value::Number(rem + 1.0))?;
                }

                let then_fn = match &next_promise {
                    Value::Object(np_obj) => match crate::core::get_property_with_accessors(mc, env, np_obj, "then") {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                            return Ok(reject_cap(mc, e));
                        }
                    },
                    _ => Value::Undefined,
                };
                if is_callable_val(&then_fn) {
                    match call_function_with_this(mc, &then_fn, Some(&next_promise), &[then_cb, catch_cb], env) {
                        Ok(_) => {}
                        Err(e) => {
                            let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                            return Ok(reject_cap(mc, e));
                        }
                    }
                }

                index += 1;
            }
        }
        "all" => {
            // §27.2.4.1 Promise.all ( iterable )
            // Step 1-2: Let C = this, check Type(C) is Object
            if !is_object_type(c) {
                return Err(raise_type_error!("Promise.all requires that 'this' be an Object").into());
            }

            // Step 3: Let promiseCapability be ? NewPromiseCapability(C)
            let (cap_promise, cap_resolve, cap_reject) = new_promise_capability(mc, env, c)?;

            // IfAbruptRejectPromise helper closure
            let reject_cap = |mc2: &MutationContext<'gc>, e: EvalError<'gc>| -> Value<'gc> {
                let reason = eval_error_to_value(mc2, env, e);
                let _ = call_function(mc2, &cap_reject, &[reason], env);
                cap_promise.clone()
            };

            // Step 4: Let promiseResolve be GetPromiseResolve(C) — IfAbruptRejectPromise
            let promise_resolve = match c {
                Value::Object(c_obj) => match crate::core::get_property_with_accessors(mc, env, c_obj, "resolve") {
                    Ok(v) => v,
                    Err(e) => return Ok(reject_cap(mc, e)),
                },
                _ => Value::Undefined,
            };
            // Step 5: If IsCallable(promiseResolve) is false — IfAbruptRejectPromise
            if !is_callable_val(&promise_resolve) {
                let reason = crate::core::js_error_to_value(mc, env, &raise_type_error!("Promise.all: resolve is not a function"));
                let _ = call_function(mc, &cap_reject, &[reason], env);
                return Ok(cap_promise);
            }

            // Step 6: Let iteratorRecord be GetIterator(iterable) — IfAbruptRejectPromise
            let iterable = if args.is_empty() { Value::Undefined } else { args[0].clone() };
            let (iter_obj, next_fn) = match crate::js_map::get_iterator(mc, env, &iterable) {
                Ok(v) => v,
                Err(e) => return Ok(reject_cap(mc, e)),
            };

            // §27.2.4.1.2 PerformPromiseAll
            // Step 1: Let values be new empty List (as JS array)
            let results_obj = match crate::js_array::create_array(mc, env) {
                Ok(r) => r,
                Err(e) => return Ok(reject_cap(mc, e.into())),
            };
            // Step 2: Let remainingElementsCount be Record { [[Value]]: 1 }
            let state_obj = new_js_object_data(mc);
            object_set_key_value(mc, &state_obj, "results", &Value::Object(results_obj))?;
            object_set_key_value(mc, &state_obj, "remaining", &Value::Number(1.0))?;
            object_set_key_value(mc, &state_obj, "cap_resolve", &cap_resolve)?;
            object_set_key_value(mc, &state_obj, "cap_reject", &cap_reject)?;

            // Step 3: Let index be 0
            let mut index: usize = 0;

            loop {
                // Step 4a: Let next be Completion(IteratorStep(iteratorRecord))
                let next_result = match crate::js_map::call_iterator_next(mc, env, &iter_obj, &next_fn) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(reject_cap(mc, e));
                    }
                };

                // Step 4b: If next is done
                let done = match crate::js_map::get_iterator_done(mc, env, &next_result) {
                    Ok(d) => d,
                    Err(e) => {
                        return Ok(reject_cap(mc, e));
                    }
                };
                if done {
                    // Decrement remainingElementsCount by 1
                    if let Some(rem_rc) = object_get_key_value(&state_obj, "remaining")
                        && let Value::Number(rem) = &*rem_rc.borrow()
                    {
                        let new_rem = rem - 1.0;
                        object_set_key_value(mc, &state_obj, "remaining", &Value::Number(new_rem))?;
                        if new_rem == 0.0 {
                            // Resolve with values array
                            if call_function(mc, &cap_resolve, &[Value::Object(results_obj)], env).is_ok() {}
                        }
                    }
                    return Ok(cap_promise);
                }

                // Step 4c: Let nextValue be Completion(IteratorValue(next))
                let next_value = match crate::js_map::get_iterator_value(mc, env, &next_result) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(reject_cap(mc, e));
                    }
                };

                // Step 4d: Append undefined to values
                object_set_key_value(mc, &results_obj, index, &Value::Undefined)?;
                object_set_key_value(mc, &results_obj, "length", &Value::Number((index + 1) as f64))?;

                // Step 4e: Let nextPromise be Call(promiseResolve, C, «nextValue»)
                let next_promise = match call_function_with_this(mc, &promise_resolve, Some(c), &[next_value], env) {
                    Ok(v) => v,
                    Err(e) => {
                        // IfAbruptRejectPromise — close iterator first
                        let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                        return Ok(reject_cap(mc, e));
                    }
                };

                // Step 4f-g: Create resolve element function
                let then_cb = {
                    let cb_env = new_js_object_data(mc);
                    cb_env.borrow_mut(mc).prototype = Some(*env);
                    object_set_key_value(mc, &cb_env, "pstate", &Value::Object(state_obj))?;
                    let raw_closure = Value::Closure(Gc::new(
                        mc,
                        ClosureData::new(
                            &[DestructuringElement::Variable("value".to_string(), None)],
                            &[stmt_expr(Expr::Call(
                                Box::new(Expr::Var("__internal_promise_all_resolve".to_string(), None, None)),
                                vec![
                                    Expr::Number(index as f64),
                                    Expr::Var("value".to_string(), None, None),
                                    Expr::Var("pstate".to_string(), None, None),
                                ],
                            ))],
                            Some(cb_env),
                            None,
                        ),
                    ));
                    // Wrap as function object with name="", length=1, non-constructor (spec §27.2.4.1.3)
                    Value::Object(wrap_closure_as_fn_obj(mc, env, raw_closure, "", 1.0)?)
                };

                // Step 4h: remainingElementsCount.[[Value]] += 1
                if let Some(rem_rc) = object_get_key_value(&state_obj, "remaining")
                    && let Value::Number(rem) = &*rem_rc.borrow()
                {
                    object_set_key_value(mc, &state_obj, "remaining", &Value::Number(rem + 1.0))?;
                }

                // Step 4i: Invoke(nextPromise, "then", «resolveElement, resultCapability.[[Reject]]»)
                // = Get(nextPromise, "then") then Call(thenFn, nextPromise, args)
                let then_fn = match &next_promise {
                    Value::Object(np_obj) => match crate::core::get_property_with_accessors(mc, env, np_obj, "then") {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                            return Ok(reject_cap(mc, e));
                        }
                    },
                    _ => Value::Undefined,
                };
                if is_callable_val(&then_fn) {
                    match call_function_with_this(mc, &then_fn, Some(&next_promise), &[then_cb, cap_reject.clone()], env) {
                        Ok(_) => {}
                        Err(e) => {
                            let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                            return Ok(reject_cap(mc, e));
                        }
                    }
                }

                // Step 4j: index += 1
                index += 1;
            }
        }
        "race" => {
            // §27.2.4.5 Promise.race ( iterable )
            if !is_object_type(c) {
                return Err(raise_type_error!("Promise.race requires that 'this' be an Object").into());
            }

            let (cap_promise, cap_resolve, cap_reject) = new_promise_capability(mc, env, c)?;

            let reject_cap = |mc2: &MutationContext<'gc>, e: EvalError<'gc>| -> Value<'gc> {
                let reason = eval_error_to_value(mc2, env, e);
                let _ = call_function(mc2, &cap_reject, &[reason], env);
                cap_promise.clone()
            };

            // GetPromiseResolve(C) — IfAbruptRejectPromise
            let promise_resolve = match c {
                Value::Object(c_obj) => match crate::core::get_property_with_accessors(mc, env, c_obj, "resolve") {
                    Ok(v) => v,
                    Err(e) => return Ok(reject_cap(mc, e)),
                },
                _ => Value::Undefined,
            };
            if !is_callable_val(&promise_resolve) {
                let reason = crate::core::js_error_to_value(mc, env, &raise_type_error!("Promise.race: resolve is not a function"));
                let _ = call_function(mc, &cap_reject, &[reason], env);
                return Ok(cap_promise);
            }

            // GetIterator(iterable) — IfAbruptRejectPromise
            let iterable = if args.is_empty() { Value::Undefined } else { args[0].clone() };
            let (iter_obj, next_fn) = match crate::js_map::get_iterator(mc, env, &iterable) {
                Ok(v) => v,
                Err(e) => return Ok(reject_cap(mc, e)),
            };

            loop {
                // IteratorStep
                let next_result = match crate::js_map::call_iterator_next(mc, env, &iter_obj, &next_fn) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(reject_cap(mc, e));
                    }
                };

                let done = match crate::js_map::get_iterator_done(mc, env, &next_result) {
                    Ok(d) => d,
                    Err(e) => {
                        return Ok(reject_cap(mc, e));
                    }
                };
                if done {
                    return Ok(cap_promise);
                }

                let next_value = match crate::js_map::get_iterator_value(mc, env, &next_result) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(reject_cap(mc, e));
                    }
                };

                // Call(promiseResolve, C, «nextValue»)
                let next_promise = match call_function_with_this(mc, &promise_resolve, Some(c), &[next_value], env) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                        return Ok(reject_cap(mc, e));
                    }
                };

                // Invoke(nextPromise, "then", «resultCapability.[[Resolve]], resultCapability.[[Reject]]»)
                let then_fn = match &next_promise {
                    Value::Object(np_obj) => match crate::core::get_property_with_accessors(mc, env, np_obj, "then") {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                            return Ok(reject_cap(mc, e));
                        }
                    },
                    _ => Value::Undefined,
                };
                if is_callable_val(&then_fn) {
                    match call_function_with_this(mc, &then_fn, Some(&next_promise), &[cap_resolve.clone(), cap_reject.clone()], env) {
                        Ok(_) => {}
                        Err(e) => {
                            let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                            return Ok(reject_cap(mc, e));
                        }
                    }
                }
            }
        }
        "any" => {
            // §27.2.4.3 Promise.any ( iterable )
            if !is_object_type(c) {
                return Err(raise_type_error!("Promise.any requires that 'this' be an Object").into());
            }

            let (cap_promise, cap_resolve, cap_reject) = new_promise_capability(mc, env, c)?;

            let reject_cap = |mc2: &MutationContext<'gc>, e: EvalError<'gc>| -> Value<'gc> {
                let reason = eval_error_to_value(mc2, env, e);
                let _ = call_function(mc2, &cap_reject, &[reason], env);
                cap_promise.clone()
            };

            let promise_resolve = match c {
                Value::Object(c_obj) => match crate::core::get_property_with_accessors(mc, env, c_obj, "resolve") {
                    Ok(v) => v,
                    Err(e) => return Ok(reject_cap(mc, e)),
                },
                _ => Value::Undefined,
            };
            if !is_callable_val(&promise_resolve) {
                let reason = crate::core::js_error_to_value(mc, env, &raise_type_error!("Promise.any: resolve is not a function"));
                let _ = call_function(mc, &cap_reject, &[reason], env);
                return Ok(cap_promise);
            }

            let iterable = if args.is_empty() { Value::Undefined } else { args[0].clone() };
            let (iter_obj, next_fn) = match crate::js_map::get_iterator(mc, env, &iterable) {
                Ok(v) => v,
                Err(e) => return Ok(reject_cap(mc, e)),
            };

            let errors_obj = match crate::js_array::create_array(mc, env) {
                Ok(r) => r,
                Err(e) => return Ok(reject_cap(mc, e.into())),
            };
            let state_obj = new_js_object_data(mc);
            object_set_key_value(mc, &state_obj, "errors", &Value::Object(errors_obj))?;
            object_set_key_value(mc, &state_obj, "remaining", &Value::Number(1.0))?;
            object_set_key_value(mc, &state_obj, "cap_resolve", &cap_resolve)?;
            object_set_key_value(mc, &state_obj, "cap_reject", &cap_reject)?;

            let mut index: usize = 0;

            loop {
                let next_result = match crate::js_map::call_iterator_next(mc, env, &iter_obj, &next_fn) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(reject_cap(mc, e));
                    }
                };
                let done = match crate::js_map::get_iterator_done(mc, env, &next_result) {
                    Ok(d) => d,
                    Err(e) => {
                        return Ok(reject_cap(mc, e));
                    }
                };
                if done {
                    if let Some(rem_rc) = object_get_key_value(&state_obj, "remaining")
                        && let Value::Number(rem) = &*rem_rc.borrow()
                    {
                        let new_rem = rem - 1.0;
                        object_set_key_value(mc, &state_obj, "remaining", &Value::Number(new_rem))?;
                        if new_rem == 0.0 {
                            // All rejected — create AggregateError and reject
                            let aggregate_error = new_gc_cell_ptr(mc, JSObjectData::new());
                            object_set_key_value(mc, &aggregate_error, "name", &Value::String(utf8_to_utf16("AggregateError")))?;
                            object_set_key_value(
                                mc,
                                &aggregate_error,
                                "message",
                                &Value::String(utf8_to_utf16("All promises were rejected")),
                            )?;
                            object_set_key_value(mc, &aggregate_error, "errors", &Value::Object(errors_obj))?;
                            if let Some(agg_ctor_rc) = crate::core::env_get(env, "AggregateError") {
                                let agg_ctor = agg_ctor_rc.borrow().clone();
                                if let Value::Object(agg_obj) = &agg_ctor
                                    && let Some(proto_rc) = object_get_key_value(agg_obj, "prototype")
                                {
                                    let proto = proto_rc.borrow().clone();
                                    if let Value::Object(proto_obj) = proto {
                                        aggregate_error.borrow_mut(mc).prototype = Some(proto_obj);
                                    }
                                }
                            }
                            let _ = call_function(mc, &cap_reject, &[Value::Object(aggregate_error)], env);
                        }
                    }
                    return Ok(cap_promise);
                }

                let next_value = match crate::js_map::get_iterator_value(mc, env, &next_result) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(reject_cap(mc, e));
                    }
                };

                object_set_key_value(mc, &errors_obj, index, &Value::Undefined)?;
                object_set_key_value(mc, &errors_obj, "length", &Value::Number((index + 1) as f64))?;

                let next_promise = match call_function_with_this(mc, &promise_resolve, Some(c), &[next_value], env) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                        return Ok(reject_cap(mc, e));
                    }
                };

                // onFulfilled: immediately resolves the capability
                let resolve_cb = {
                    let cb_env = new_js_object_data(mc);
                    cb_env.borrow_mut(mc).prototype = Some(*env);
                    object_set_key_value(mc, &cb_env, "pstate", &Value::Object(state_obj))?;
                    let raw_cl = Value::Closure(Gc::new(
                        mc,
                        ClosureData::new(
                            &[DestructuringElement::Variable("value".to_string(), None)],
                            &[stmt_expr(Expr::Call(
                                Box::new(Expr::Var("__internal_any_resolve".to_string(), None, None)),
                                vec![
                                    Expr::Var("value".to_string(), None, None),
                                    Expr::Var("pstate".to_string(), None, None),
                                ],
                            ))],
                            Some(cb_env),
                            None,
                        ),
                    ));
                    Value::Object(wrap_closure_as_fn_obj(mc, env, raw_cl, "", 1.0)?)
                };

                // onRejected: stores reason and decrements remaining
                let reject_cb = {
                    let cb_env = new_js_object_data(mc);
                    cb_env.borrow_mut(mc).prototype = Some(*env);
                    object_set_key_value(mc, &cb_env, "pstate", &Value::Object(state_obj))?;
                    let raw_cl = Value::Closure(Gc::new(
                        mc,
                        ClosureData::new(
                            &[DestructuringElement::Variable("reason".to_string(), None)],
                            &[stmt_expr(Expr::Call(
                                Box::new(Expr::Var("__internal_any_reject".to_string(), None, None)),
                                vec![
                                    Expr::Number(index as f64),
                                    Expr::Var("reason".to_string(), None, None),
                                    Expr::Var("pstate".to_string(), None, None),
                                ],
                            ))],
                            Some(cb_env),
                            None,
                        ),
                    ));
                    Value::Object(wrap_closure_as_fn_obj(mc, env, raw_cl, "", 1.0)?)
                };

                if let Some(rem_rc) = object_get_key_value(&state_obj, "remaining")
                    && let Value::Number(rem) = &*rem_rc.borrow()
                {
                    object_set_key_value(mc, &state_obj, "remaining", &Value::Number(rem + 1.0))?;
                }

                let then_fn = match &next_promise {
                    Value::Object(np_obj) => match crate::core::get_property_with_accessors(mc, env, np_obj, "then") {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                            return Ok(reject_cap(mc, e));
                        }
                    },
                    _ => Value::Undefined,
                };
                if is_callable_val(&then_fn) {
                    match call_function_with_this(mc, &then_fn, Some(&next_promise), &[resolve_cb, reject_cb], env) {
                        Ok(_) => {}
                        Err(e) => {
                            let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                            return Ok(reject_cap(mc, e));
                        }
                    }
                }

                index += 1;
            }
        }
        _ => Err(crate::raise_eval_error!(format!(
            "Static method Promise.{} is not yet wired to receive evaluated arguments.",
            method
        ))
        .into()),
    }
}

pub fn handle_promise_prototype_method<'gc>(
    mc: &MutationContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let promise = get_promise_from_js_object(obj)
        .ok_or_else(|| -> EvalError<'gc> { crate::raise_type_error!("Method called on incompatible receiver").into() })?;

    match method {
        "then" => {
            let on_fulfilled = args.first().cloned();
            let on_rejected = args.get(1).cloned();
            handle_promise_then_val(mc, promise, on_fulfilled, on_rejected, Some(obj), env)
        }
        "finally" => {
            let on_finally = args.first().cloned();
            Ok(handle_promise_finally_val(mc, promise, on_finally, env)?)
        }
        _ => Err(crate::raise_eval_error!(format!("Unknown Promise method {method}")).into()),
    }
}

pub fn handle_promise_constructor_val<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.is_empty() {
        return Err(raise_type_error!("Promise constructor requires an executor function").into());
    }
    let executor = &args[0];

    let promise = new_gc_cell_ptr(mc, JSPromise::new());
    let promise_obj = make_promise_js_object(mc, promise, Some(*env))?;
    // Also store the internal id on the object for correlation
    object_set_key_value(
        mc,
        &promise_obj,
        "__promise_internal_id",
        &Value::Number(promise.borrow().id as f64),
    )?;

    let resolve_func = create_resolve_function_direct(mc, promise, env);
    let reject_func = create_reject_function_direct(mc, promise, env);

    if let Some((params, body, captured_env)) = crate::core::extract_closure_from_value(executor) {
        let executor_args = vec![resolve_func.clone(), reject_func.clone()];
        let executor_env = prepare_closure_call_env(mc, Some(&captured_env), Some(&params[..]), &executor_args, None)?;
        // §27.2.3.1 step 9: Call(executor, undefined, ...) — set this to undefined
        object_set_key_value(mc, &executor_env, "this", &Value::Undefined)?;
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
                let _ = call_function(mc, &reject_func, std::slice::from_ref(&reason), &executor_env);
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
    promise_obj: Option<&JSObjectDataPtr<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // SpeciesConstructor(promise, %Promise%)
    // Look up promise.constructor[Symbol.species] to determine result constructor
    let species_ctor = if let Some(obj) = promise_obj {
        get_species_constructor(mc, obj, env)?
    } else {
        None
    };

    if let Some(ref ctor) = species_ctor
        && !is_default_promise_ctor(ctor, env)
    {
        // Subclass path: use NewPromiseCapability(C)
        let (cap_promise, _cap_resolve, _cap_reject) = new_promise_capability(mc, env, ctor)?;
        // Extract the internal promise from the capability result if it's a promise object
        if let Value::Object(cap_obj) = &cap_promise
            && let Some(cap_internal) = get_promise_from_js_object(cap_obj)
        {
            perform_promise_then(mc, promise, on_fulfilled, on_rejected, Some(cap_internal), env)?;
            return Ok(cap_promise);
        }
        // Fallback: the capability promise is not a native JSPromise
        // Just return it with then handlers attached via the task queue
        let new_promise = new_gc_cell_ptr(mc, JSPromise::new());
        perform_promise_then(mc, promise, on_fulfilled, on_rejected, Some(new_promise), env)?;
        return Ok(cap_promise);
    }

    // Default path: create a plain Promise
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
                                        slot_set(mc, &new_env, InternalSlot::ResultPromise, &Value::Promise(result_promise));
                                        slot_set(mc, &new_env, InternalSlot::OrigValue, &orig_value.clone());
                                        Some(new_env)
                                    },
                                    None,
                                ),
                            ))),
                            Some(Value::Closure(Gc::new(
                                mc,
                                ClosureData::new(
                                    &[DestructuringElement::Variable("__reason".to_string(), None)],
                                    &[stmt_expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                                        vec![
                                            Expr::Var("__result_promise".to_string(), None, None),
                                            Expr::Var("__reason".to_string(), None, None),
                                        ],
                                    ))],
                                    {
                                        let new_env = *env;
                                        slot_set(mc, &new_env, InternalSlot::ResultPromise, &Value::Promise(result_promise));
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
                                            slot_set(mc, &new_env, InternalSlot::ResultPromise, &Value::Promise(result_promise));
                                            slot_set(mc, &new_env, InternalSlot::OrigValue, &orig_value.clone());
                                            Some(new_env)
                                        },
                                        None,
                                    ),
                                ))),
                                Some(Value::Closure(Gc::new(
                                    mc,
                                    ClosureData::new(
                                        &[DestructuringElement::Variable("__reason".to_string(), None)],
                                        &[stmt_expr(Expr::Call(
                                            Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                                            vec![
                                                Expr::Var("__result_promise".to_string(), None, None),
                                                Expr::Var("__reason".to_string(), None, None),
                                            ],
                                        ))],
                                        {
                                            let new_env = *env;
                                            slot_set(mc, &new_env, InternalSlot::ResultPromise, &Value::Promise(result_promise));
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
                                        slot_set(mc, &new_env, InternalSlot::ResultPromise, &Value::Promise(result_promise));
                                        slot_set(mc, &new_env, InternalSlot::OrigValue, &orig_value.clone());
                                        Some(new_env)
                                    },
                                    None,
                                ),
                            ))),
                            Some(Value::Closure(Gc::new(
                                mc,
                                ClosureData::new(
                                    &[DestructuringElement::Variable("__reason".to_string(), None)],
                                    &[stmt_expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                                        vec![
                                            Expr::Var("__result_promise".to_string(), None, None),
                                            Expr::Var("__reason".to_string(), None, None),
                                        ],
                                    ))],
                                    {
                                        let new_env = *env;
                                        slot_set(mc, &new_env, InternalSlot::ResultPromise, &Value::Promise(result_promise));
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
                                        slot_set(mc, &new_env, InternalSlot::ResultPromise, &Value::Promise(result_promise));
                                        slot_set(mc, &new_env, InternalSlot::OrigReason, &orig_reason.clone());
                                        Some(new_env)
                                    },
                                    None,
                                ),
                            ))),
                            Some(Value::Closure(Gc::new(
                                mc,
                                ClosureData::new(
                                    &[DestructuringElement::Variable("__reason".to_string(), None)],
                                    &[stmt_expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                                        vec![
                                            Expr::Var("__result_promise".to_string(), None, None),
                                            Expr::Var("__reason".to_string(), None, None),
                                        ],
                                    ))],
                                    {
                                        let new_env = *env;
                                        slot_set(mc, &new_env, InternalSlot::ResultPromise, &Value::Promise(result_promise));
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
                                    slot_set(mc, &new_env, InternalSlot::ResultPromise, &Value::Promise(result_promise));
                                    slot_set(mc, &new_env, InternalSlot::OrigReason, &orig_reason.clone());
                                    Some(new_env)
                                },
                                None,
                            ),
                        ))),
                        Some(Value::Closure(Gc::new(
                            mc,
                            ClosureData::new(
                                &[DestructuringElement::Variable("__reason".to_string(), None)],
                                &[stmt_expr(Expr::Call(
                                    Box::new(Expr::Var("__internal_reject_promise".to_string(), None, None)),
                                    vec![
                                        Expr::Var("__result_promise".to_string(), None, None),
                                        Expr::Var("__reason".to_string(), None, None),
                                    ],
                                ))],
                                {
                                    let new_env = *env;
                                    slot_set(mc, &new_env, InternalSlot::ResultPromise, &Value::Promise(result_promise));
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
                slot_set(mc, &new_env, InternalSlot::OnFinally, &on_finally_val.clone());
                slot_set(mc, &new_env, InternalSlot::ResultPromise, &Value::Promise(new_promise));
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
                slot_set(mc, &new_env, InternalSlot::OnFinally, &on_finally_val.clone());
                slot_set(mc, &new_env, InternalSlot::ResultPromise, &Value::Promise(new_promise));
                Some(new_env)
            },
            None,
        ),
    ));

    perform_promise_then(mc, promise, Some(then_callback), Some(catch_callback), Some(new_promise), env)?;

    Ok(Value::Object(new_promise_obj))
}
