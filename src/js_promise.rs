use crate::core::{Expr, JSObjectDataPtr, Statement, Value, env_set, evaluate_expr, evaluate_statements, utf8_to_utf16};
use crate::error::JSError;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

#[derive(Clone)]
pub enum Task {
    Resolution {
        promise: Rc<RefCell<JSPromise>>,
        callbacks: Vec<(Value, Rc<RefCell<JSPromise>>)>,
    },
    Rejection {
        promise: Rc<RefCell<JSPromise>>,
        callbacks: Vec<(Value, Rc<RefCell<JSPromise>>)>,
    },
}

thread_local! {
    pub static GLOBAL_TASK_QUEUE: RefCell<VecDeque<Task>> = const { RefCell::new(VecDeque::new()) };
}

pub fn queue_task(task: Task) {
    GLOBAL_TASK_QUEUE.with(|queue| {
        queue.borrow_mut().push_back(task);
    });
}

// Function to run the event loop and process asynchronous tasks
pub fn run_event_loop() -> Result<(), JSError> {
    loop {
        let task = GLOBAL_TASK_QUEUE.with(|queue| queue.borrow_mut().pop_front());

        match task {
            Some(Task::Resolution { promise, callbacks }) => {
                for (callback, new_promise) in callbacks {
                    // Call the callback and resolve the new promise with the result
                    match &callback {
                        Value::Closure(params, body, captured_env) => {
                            let func_env = captured_env.clone();
                            if !params.is_empty() {
                                env_set(&func_env, &params[0], promise.borrow().value.clone().unwrap_or(Value::Undefined))?;
                            }
                            match evaluate_statements(&func_env, body) {
                                Ok(result) => {
                                    resolve_promise(&new_promise, result);
                                }
                                Err(e) => {
                                    reject_promise(&new_promise, Value::String(utf8_to_utf16(&format!("{:?}", e))));
                                }
                            }
                        }
                        _ => {
                            // If callback is not a function, resolve with undefined
                            resolve_promise(&new_promise, Value::Undefined);
                        }
                    }
                }
            }
            Some(Task::Rejection { promise, callbacks }) => {
                for (callback, new_promise) in callbacks {
                    // Call the callback and resolve the new promise with the result
                    match &callback {
                        Value::Closure(params, body, captured_env) => {
                            let func_env = captured_env.clone();
                            if !params.is_empty() {
                                env_set(&func_env, &params[0], promise.borrow().value.clone().unwrap_or(Value::Undefined))?;
                            }
                            match evaluate_statements(&func_env, body) {
                                Ok(result) => {
                                    resolve_promise(&new_promise, result);
                                }
                                Err(e) => {
                                    reject_promise(&new_promise, Value::String(utf8_to_utf16(&format!("{:?}", e))));
                                }
                            }
                        }
                        _ => {
                            // If callback is not a function, resolve with undefined
                            resolve_promise(&new_promise, Value::Undefined);
                        }
                    }
                }
            }
            None => break, // No more tasks
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub enum PromiseState {
    #[default]
    Pending,
    Fulfilled(Value),
    Rejected(Value),
}

#[derive(Clone, Default)]
pub struct JSPromise {
    pub state: PromiseState,
    pub value: Option<Value>,                               // The resolved value or rejection reason
    pub on_fulfilled: Vec<(Value, Rc<RefCell<JSPromise>>)>, // Callbacks and their chaining promises
    pub on_rejected: Vec<(Value, Rc<RefCell<JSPromise>>)>,  // Callbacks and their chaining promises
}

impl JSPromise {
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

/// Create a new Promise object
pub fn make_promise_object() -> Result<JSObjectDataPtr, JSError> {
    let promise_obj = Rc::new(RefCell::new(crate::core::JSObjectData::new()));

    // Add then method
    let then_func = Value::Function("Promise.prototype.then".to_string());
    crate::core::obj_set_value(&promise_obj, "then", then_func)?;

    // Add catch method
    let catch_func = Value::Function("Promise.prototype.catch".to_string());
    crate::core::obj_set_value(&promise_obj, "catch", catch_func)?;

    // Add finally method
    let finally_func = Value::Function("Promise.prototype.finally".to_string());
    crate::core::obj_set_value(&promise_obj, "finally", finally_func)?;

    Ok(promise_obj)
}

/// Handle Promise constructor calls
pub fn handle_promise_constructor(args: &[crate::core::Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        return Err(JSError::EvaluationError {
            message: "Promise constructor requires an executor function".to_string(),
        });
    }

    let executor = evaluate_expr(env, &args[0])?;
    match executor {
        Value::Closure(params, body, captured_env) => {
            if params.len() != 2 {
                return Err(JSError::EvaluationError {
                    message: "Promise executor function must take exactly 2 parameters (resolve, reject)".to_string(),
                });
            }

            // Create the promise
            let promise = Rc::new(RefCell::new(JSPromise::new()));
            let promise_obj = make_promise_object()?;
            crate::core::obj_set_value(&promise_obj, "__promise", Value::Promise(promise.clone()))?;

            // Create resolve and reject functions
            let resolve_func = create_resolve_function(promise.clone(), env);
            let reject_func = create_reject_function(promise.clone(), env);

            // Call the executor with resolve and reject functions
            let executor_env = captured_env.clone();
            env_set(&executor_env, &params[0], resolve_func)?;
            env_set(&executor_env, &params[1], reject_func)?;

            // Execute the executor function
            // Note: In a real implementation, this should be asynchronous
            let _result = evaluate_statements(&executor_env, &body);

            Ok(Value::Object(promise_obj))
        }
        _ => Err(JSError::EvaluationError {
            message: "Promise constructor requires a function as executor".to_string(),
        }),
    }
}

/// Create a resolve function for Promise executor
fn create_resolve_function(promise: Rc<RefCell<JSPromise>>, env: &JSObjectDataPtr) -> Value {
    // Create a closure environment that captures the promise
    let closure_env = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
    closure_env.borrow_mut().prototype = Some(env.clone());
    env_set(&closure_env, "__captured_promise", Value::Promise(promise)).unwrap();

    Value::Closure(
        vec!["value".to_string()],
        vec![Statement::Expr(Expr::Call(
            Box::new(Expr::Var("__internal_resolve_promise".to_string())),
            vec![Expr::Var("__captured_promise".to_string()), Expr::Var("value".to_string())],
        ))],
        closure_env,
    )
}

/// Create a reject function for Promise executor
fn create_reject_function(promise: Rc<RefCell<JSPromise>>, env: &JSObjectDataPtr) -> Value {
    // Create a closure environment that captures the promise
    let closure_env = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
    closure_env.borrow_mut().prototype = Some(env.clone());
    env_set(&closure_env, "__captured_promise", Value::Promise(promise)).unwrap();

    Value::Closure(
        vec!["reason".to_string()],
        vec![Statement::Expr(Expr::Call(
            Box::new(Expr::Var("__internal_reject_promise".to_string())),
            vec![Expr::Var("__captured_promise".to_string()), Expr::Var("reason".to_string())],
        ))],
        closure_env,
    )
}

/// Handle Promise.prototype.then calls
pub fn handle_promise_then(promise_obj: &JSObjectDataPtr, args: &[crate::core::Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Get the underlying promise
    let promise_val = crate::core::obj_get_value(promise_obj, "__promise")?;
    let promise = match promise_val {
        Some(val_rc) => {
            let val = val_rc.borrow();
            match &*val {
                Value::Promise(p) => p.clone(),
                _ => {
                    return Err(JSError::EvaluationError {
                        message: "Invalid promise object".to_string(),
                    });
                }
            }
        }
        _ => {
            return Err(JSError::EvaluationError {
                message: "Invalid promise object".to_string(),
            });
        }
    };

    // Create a new promise for chaining
    let new_promise = Rc::new(RefCell::new(JSPromise::new()));
    let new_promise_obj = make_promise_object()?;
    crate::core::obj_set_value(&new_promise_obj, "__promise", Value::Promise(new_promise.clone()))?;

    // Get the onFulfilled callback
    let on_fulfilled = if !args.is_empty() {
        Some(evaluate_expr(env, &args[0])?)
    } else {
        None
    };

    // Add to the promise's callback lists
    let mut promise_borrow = promise.borrow_mut();
    if let Some(ref callback) = on_fulfilled {
        promise_borrow.on_fulfilled.push((callback.clone(), new_promise.clone()));
    }

    // If promise is already resolved, queue task to execute callback asynchronously
    if let PromiseState::Fulfilled(val) = &promise_borrow.state {
        if let Some(ref callback) = on_fulfilled {
            // Queue task to execute callback asynchronously
            queue_task(Task::Resolution {
                promise: promise.clone(),
                callbacks: vec![(callback.clone(), new_promise.clone())],
            });
        } else {
            // No callback, resolve with the original value
            resolve_promise(&new_promise, val.clone());
        }
    }

    Ok(Value::Object(new_promise_obj))
}

/// Handle Promise.prototype.catch calls
pub fn handle_promise_catch(promise_obj: &JSObjectDataPtr, args: &[crate::core::Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Similar to then, but only for rejection
    handle_promise_then(promise_obj, args, env)
}

/// Handle Promise.prototype.finally calls
pub fn handle_promise_finally(promise_obj: &JSObjectDataPtr, args: &[crate::core::Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Get the underlying promise
    let promise_val = crate::core::obj_get_value(promise_obj, "__promise")?;
    let promise = match promise_val {
        Some(val_rc) => {
            let val = val_rc.borrow();
            match &*val {
                Value::Promise(p) => p.clone(),
                _ => {
                    return Err(JSError::EvaluationError {
                        message: "Invalid promise object".to_string(),
                    });
                }
            }
        }
        _ => {
            return Err(JSError::EvaluationError {
                message: "Invalid promise object".to_string(),
            });
        }
    };

    // Create a new promise for chaining
    let new_promise = Rc::new(RefCell::new(JSPromise::new()));
    let new_promise_obj = make_promise_object()?;
    crate::core::obj_set_value(&new_promise_obj, "__promise", Value::Promise(new_promise.clone()))?;

    // Get the onFinally callback
    let on_finally = if !args.is_empty() {
        Some(evaluate_expr(env, &args[0])?)
    } else {
        None
    };

    // Create a closure that executes finally and returns the original value
    let finally_callback = Value::Closure(
        vec!["value".to_string()],
        vec![
            // Execute finally callback if provided (no arguments)
            Statement::Expr(Expr::Call(Box::new(Expr::Var("finally_func".to_string())), vec![])),
            // Return the original value
            Statement::Return(Some(Expr::Var("value".to_string()))),
        ],
        {
            let new_env = env.clone();
            // Add the finally callback to the environment
            if let Some(callback) = on_finally {
                crate::core::obj_set_value(&new_env, "finally_func", callback)?;
            } else {
                // No-op if no callback provided
                let noop = Value::Closure(vec![], vec![], env.clone());
                crate::core::obj_set_value(&new_env, "finally_func", noop)?;
            }
            new_env
        },
    );

    // Add the same callback to both fulfilled and rejected lists
    let mut promise_borrow = promise.borrow_mut();
    promise_borrow.on_fulfilled.push((finally_callback.clone(), new_promise.clone()));
    promise_borrow.on_rejected.push((finally_callback.clone(), new_promise.clone()));

    // If promise is already settled, queue task to execute callback asynchronously
    match &promise_borrow.state {
        PromiseState::Fulfilled(_) => {
            queue_task(Task::Resolution {
                promise: promise.clone(),
                callbacks: vec![(finally_callback.clone(), new_promise.clone())],
            });
        }
        PromiseState::Rejected(_) => {
            queue_task(Task::Rejection {
                promise: promise.clone(),
                callbacks: vec![(finally_callback.clone(), new_promise.clone())],
            });
        }
        _ => {}
    }

    Ok(Value::Object(new_promise_obj))
}

/// Resolve a promise with a value
pub fn resolve_promise(promise: &Rc<RefCell<JSPromise>>, value: Value) {
    let mut promise_borrow = promise.borrow_mut();
    if let PromiseState::Pending = promise_borrow.state {
        promise_borrow.state = PromiseState::Fulfilled(value.clone());
        promise_borrow.value = Some(value);

        // Queue task to execute fulfilled callbacks asynchronously
        let callbacks = promise_borrow.on_fulfilled.clone();
        promise_borrow.on_fulfilled.clear();
        if !callbacks.is_empty() {
            queue_task(Task::Resolution {
                promise: promise.clone(),
                callbacks,
            });
        }
    }
}

/// Reject a promise with a reason
pub fn reject_promise(promise: &Rc<RefCell<JSPromise>>, reason: Value) {
    let mut promise_borrow = promise.borrow_mut();
    if let PromiseState::Pending = promise_borrow.state {
        promise_borrow.state = PromiseState::Rejected(reason.clone());
        promise_borrow.value = Some(reason);

        // Queue task to execute rejected callbacks asynchronously
        let callbacks = promise_borrow.on_rejected.clone();
        promise_borrow.on_rejected.clear();
        if !callbacks.is_empty() {
            queue_task(Task::Rejection {
                promise: promise.clone(),
                callbacks,
            });
        }
    }
}

/// Check if an object is a promise
#[allow(dead_code)]
pub fn is_promise(obj: &JSObjectDataPtr) -> bool {
    if let Ok(Some(val_rc)) = crate::core::obj_get_value(obj, "__promise") {
        matches!(&*val_rc.borrow(), Value::Promise(_))
    } else {
        false
    }
}

/// Handle Promise static methods like Promise.all, Promise.race
pub fn handle_promise_static_method(method: &str, args: &[crate::core::Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "all" => {
            // Promise.all(iterable) - simplified synchronous implementation
            if args.is_empty() {
                return Err(JSError::EvaluationError {
                    message: "Promise.all requires at least one argument".to_string(),
                });
            }

            // Evaluate the iterable argument
            let iterable = evaluate_expr(env, &args[0])?;
            let promises = match iterable {
                Value::Object(arr) => {
                    // Assume it's an array-like object
                    let mut promises = Vec::new();
                    let mut i = 0;
                    loop {
                        let key = i.to_string();
                        if let Some(val) = crate::core::obj_get_value(&arr, &key)? {
                            promises.push((*val).clone());
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    promises
                }
                _ => {
                    return Err(JSError::EvaluationError {
                        message: "Promise.all argument must be iterable".to_string(),
                    });
                }
            };

            // Create a new promise that resolves when all promises resolve
            let result_promise = Rc::new(RefCell::new(JSPromise::new()));
            let result_promise_obj = make_promise_object()?;
            crate::core::obj_set_value(&result_promise_obj, "__promise", Value::Promise(result_promise.clone()))?;

            // For now, check if all values are already resolved (synchronous implementation)
            let mut all_resolved = true;
            let mut results = Vec::new();
            let mut rejection_reason = None;

            for promise_val in promises {
                match &*promise_val.borrow() {
                    Value::Object(obj) => {
                        if let Some(promise_rc) = crate::core::obj_get_value(obj, "__promise")? {
                            if let Value::Promise(p) = &*promise_rc.borrow() {
                                match &p.borrow().state {
                                    PromiseState::Fulfilled(value) => {
                                        results.push(value.clone());
                                    }
                                    PromiseState::Rejected(reason) => {
                                        rejection_reason = Some(reason.clone());
                                        all_resolved = false;
                                        break;
                                    }
                                    PromiseState::Pending => {
                                        all_resolved = false;
                                        break;
                                    }
                                }
                            } else {
                                results.push(Value::Object(obj.clone()));
                            }
                        } else {
                            results.push(Value::Object(obj.clone()));
                        }
                    }
                    val => {
                        results.push(val.clone());
                    }
                }
            }

            if all_resolved {
                if let Some(reason) = rejection_reason {
                    result_promise.borrow_mut().state = PromiseState::Rejected(reason);
                } else {
                    // Create result array
                    let result_arr = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
                    for (idx, val) in results.iter().enumerate() {
                        crate::core::obj_set_value(&result_arr, idx.to_string(), val.clone())?;
                    }
                    result_promise.borrow_mut().state = PromiseState::Fulfilled(Value::Object(result_arr));
                }
            }
            // If not all resolved, the promise remains pending

            Ok(Value::Object(result_promise_obj))
        }
        "race" => {
            // Promise.race(iterable) - simplified synchronous implementation
            if args.is_empty() {
                return Err(JSError::EvaluationError {
                    message: "Promise.race requires at least one argument".to_string(),
                });
            }

            // Evaluate the iterable argument
            let iterable = evaluate_expr(env, &args[0])?;
            let promises = match iterable {
                Value::Object(arr) => {
                    // Assume it's an array-like object
                    let mut promises = Vec::new();
                    let mut i = 0;
                    loop {
                        let key = i.to_string();
                        if let Some(val) = crate::core::obj_get_value(&arr, &key)? {
                            promises.push((*val).clone());
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    promises
                }
                _ => {
                    return Err(JSError::EvaluationError {
                        message: "Promise.race argument must be iterable".to_string(),
                    });
                }
            };

            // Create a new promise that resolves/rejects when the first promise settles
            let result_promise = Rc::new(RefCell::new(JSPromise::new()));
            let result_promise_obj = make_promise_object()?;
            crate::core::obj_set_value(&result_promise_obj, "__promise", Value::Promise(result_promise.clone()))?;

            // For now, check if any value is already settled (synchronous implementation)
            for promise_val in promises {
                match &*promise_val.borrow() {
                    Value::Object(obj) => {
                        if let Some(promise_rc) = crate::core::obj_get_value(obj, "__promise")?
                            && let Value::Promise(p) = &*promise_rc.borrow()
                        {
                            match &p.borrow().state {
                                PromiseState::Fulfilled(value) => {
                                    result_promise.borrow_mut().state = PromiseState::Fulfilled(value.clone());
                                    return Ok(Value::Object(result_promise_obj));
                                }
                                PromiseState::Rejected(reason) => {
                                    result_promise.borrow_mut().state = PromiseState::Rejected(reason.clone());
                                    return Ok(Value::Object(result_promise_obj));
                                }
                                PromiseState::Pending => {
                                    // Continue checking other promises
                                }
                            }
                        }
                    }
                    val => {
                        // Non-promise values resolve immediately
                        result_promise.borrow_mut().state = PromiseState::Fulfilled(val.clone());
                        return Ok(Value::Object(result_promise_obj));
                    }
                }
            }

            // If no promises were settled, return the pending promise
            Ok(Value::Object(result_promise_obj))
        }
        _ => Err(JSError::EvaluationError {
            message: format!("Promise has no static method '{}'", method),
        }),
    }
}

/// Handle Promise instance method calls
pub fn handle_promise_method(obj_map: &JSObjectDataPtr, method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "then" => crate::js_promise::handle_promise_then(obj_map, args, env),
        "catch" => crate::js_promise::handle_promise_catch(obj_map, args, env),
        "finally" => crate::js_promise::handle_promise_finally(obj_map, args, env),
        _ => Err(JSError::EvaluationError {
            message: format!("Promise has no method '{}'", method),
        }),
    }
}
