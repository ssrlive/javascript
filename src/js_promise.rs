use crate::core::{Expr, JSObjectDataPtr, Statement, Value, env_set, evaluate_expr, evaluate_statements, utf8_to_utf16};
use crate::error::JSError;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

#[derive(Clone)]
enum Task {
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

fn queue_task(task: Task) {
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

    // Get the onRejected callback
    let on_rejected = if !args.is_empty() {
        Some(evaluate_expr(env, &args[0])?)
    } else {
        None
    };

    // Add to the promise's callback lists
    let mut promise_borrow = promise.borrow_mut();
    if let Some(ref callback) = on_rejected {
        promise_borrow.on_rejected.push((callback.clone(), new_promise.clone()));
    }

    // If promise is already settled, queue task to execute callback asynchronously
    match &promise_borrow.state {
        PromiseState::Rejected(val) => {
            if let Some(ref callback) = on_rejected {
                // Queue task to execute callback asynchronously
                queue_task(Task::Rejection {
                    promise: promise.clone(),
                    callbacks: vec![(callback.clone(), new_promise.clone())],
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

/// Handle Promise static methods like Promise.all, Promise.race, Promise.allSettled, Promise.any
pub fn handle_promise_static_method(method: &str, args: &[crate::core::Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "all" => {
            // Promise.all(iterable) - asynchronous implementation
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
                            promises.push((*val).borrow().clone());
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

            let num_promises = promises.len();
            if num_promises == 0 {
                // Empty array, resolve immediately with empty array
                let result_arr = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
                resolve_promise(&result_promise, Value::Object(result_arr));
                return Ok(Value::Object(result_promise_obj));
            }

            let results = Rc::new(RefCell::new(vec![Value::Undefined; num_promises]));
            let completed = Rc::new(RefCell::new(0));

            for (idx, promise_val) in promises.into_iter().enumerate() {
                let results_clone = results.clone();
                let completed_clone = completed.clone();
                let _result_promise_clone = result_promise.clone();

                match promise_val {
                    Value::Object(obj) => {
                        if let Some(promise_rc) = crate::core::obj_get_value(&obj, "__promise")? {
                            if let Value::Promise(_) = &*promise_rc.borrow() {
                                // It's a promise, attach then/catch
                                let then_callback = Value::Closure(
                                    vec!["value".to_string()],
                                    vec![Statement::Expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_resolve".to_string())),
                                        vec![Expr::Number(idx as f64), Expr::Var("value".to_string())],
                                    ))],
                                    env.clone(),
                                );

                                let catch_callback = Value::Closure(
                                    vec!["reason".to_string()],
                                    vec![Statement::Expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_reject".to_string())),
                                        vec![Expr::Var("reason".to_string())],
                                    ))],
                                    env.clone(),
                                );

                                // Attach then and catch to the promise
                                handle_promise_then(&obj, &[Expr::Value(then_callback)], env)?;
                                handle_promise_catch(&obj, &[Expr::Value(catch_callback)], env)?;
                            } else {
                                // Not a promise, treat as resolved value
                                results_clone.borrow_mut()[idx] = Value::Object(obj.clone());
                                *completed_clone.borrow_mut() += 1;
                                if *completed_clone.borrow() == num_promises {
                                    let final_results: Vec<Value> = results_clone.borrow().iter().cloned().collect();
                                    let result_arr = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
                                    for (i, val) in final_results.iter().enumerate() {
                                        crate::core::obj_set_value(&result_arr, i.to_string(), val.clone())?;
                                    }
                                    resolve_promise(&result_promise, Value::Object(result_arr));
                                }
                            }
                        } else {
                            // Not a promise, treat as resolved value
                            results_clone.borrow_mut()[idx] = Value::Object(obj.clone());
                            *completed_clone.borrow_mut() += 1;
                            if *completed_clone.borrow() == num_promises {
                                let final_results: Vec<Value> = results_clone.borrow().iter().cloned().collect();
                                let result_arr = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
                                for (i, val) in final_results.iter().enumerate() {
                                    crate::core::obj_set_value(&result_arr, i.to_string(), val.clone())?;
                                }
                                resolve_promise(&result_promise, Value::Object(result_arr));
                            }
                        }
                    }
                    val => {
                        // Non-object value, treat as resolved
                        results_clone.borrow_mut()[idx] = val.clone();
                        *completed_clone.borrow_mut() += 1;
                        if *completed_clone.borrow() == num_promises {
                            let final_results: Vec<Value> = results_clone.borrow().iter().cloned().collect();
                            let result_arr = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
                            for (i, val) in final_results.iter().enumerate() {
                                crate::core::obj_set_value(&result_arr, i.to_string(), val.clone())?;
                            }
                            resolve_promise(&result_promise, Value::Object(result_arr));
                        }
                    }
                }
            }

            Ok(Value::Object(result_promise_obj))
        }
        "allSettled" => {
            // Promise.allSettled(iterable) - asynchronous implementation using internal callbacks
            if args.is_empty() {
                return Err(JSError::EvaluationError {
                    message: "Promise.allSettled requires at least one argument".to_string(),
                });
            }

            let iterable = evaluate_expr(env, &args[0])?;
            let promises = match iterable {
                Value::Object(arr) => {
                    let mut promises = Vec::new();
                    let mut i = 0;
                    loop {
                        let key = i.to_string();
                        if let Some(val) = crate::core::obj_get_value(&arr, &key)? {
                            promises.push((*val).borrow().clone());
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    promises
                }
                _ => {
                    return Err(JSError::EvaluationError {
                        message: "Promise.allSettled argument must be iterable".to_string(),
                    });
                }
            };

            let result_promise = Rc::new(RefCell::new(JSPromise::new()));
            let result_promise_obj = make_promise_object()?;
            crate::core::obj_set_value(&result_promise_obj, "__promise", Value::Promise(result_promise.clone()))?;

            let num_promises = promises.len();
            if num_promises == 0 {
                let result_arr = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
                resolve_promise(&result_promise, Value::Object(result_arr));
                return Ok(Value::Object(result_promise_obj));
            }

            // Create shared state for all promises
            let shared_state = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
            let results_array = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
            crate::core::obj_set_value(&results_array, "length", Value::Number(num_promises as f64))?;
            crate::core::obj_set_value(&shared_state, "results", Value::Object(results_array))?;
            crate::core::obj_set_value(&shared_state, "completed", Value::Number(0.0))?;
            crate::core::obj_set_value(&shared_state, "total", Value::Number(num_promises as f64))?;
            crate::core::obj_set_value(&shared_state, "result_promise", Value::Promise(result_promise.clone()))?;

            for (idx, promise_val) in promises.into_iter().enumerate() {
                let shared_state_clone = shared_state.clone();

                match promise_val {
                    Value::Object(obj) => {
                        if let Some(promise_rc) = crate::core::obj_get_value(&obj, "__promise")? {
                            if let Value::Promise(_p) = &*promise_rc.borrow() {
                                // Use internal callback functions
                                let then_callback = Value::Closure(
                                    vec!["value".to_string()],
                                    vec![Statement::Expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_promise_allsettled_resolve".to_string())),
                                        vec![
                                            Expr::Number(idx as f64),
                                            Expr::Var("value".to_string()),
                                            Expr::Var("__shared_state".to_string()),
                                        ],
                                    ))],
                                    {
                                        let new_env = env.clone();
                                        crate::core::obj_set_value(&new_env, "__shared_state", Value::Object(shared_state_clone.clone()))?;
                                        new_env
                                    },
                                );

                                let catch_callback = Value::Closure(
                                    vec!["reason".to_string()],
                                    vec![Statement::Expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_promise_allsettled_reject".to_string())),
                                        vec![
                                            Expr::Number(idx as f64),
                                            Expr::Var("reason".to_string()),
                                            Expr::Var("__shared_state".to_string()),
                                        ],
                                    ))],
                                    {
                                        let new_env = env.clone();
                                        crate::core::obj_set_value(&new_env, "__shared_state", Value::Object(shared_state_clone))?;
                                        new_env
                                    },
                                );

                                handle_promise_then(&obj, &[Expr::Value(then_callback)], env)?;
                                handle_promise_catch(&obj, &[Expr::Value(catch_callback)], env)?;
                            } else {
                                // Not a promise, treat as resolved value
                                __internal_promise_allsettled_resolve(
                                    idx as f64,
                                    Value::Object(obj.clone()),
                                    Value::Object(shared_state.clone()),
                                );
                            }
                        } else {
                            // Not a promise, treat as resolved value
                            __internal_promise_allsettled_resolve(
                                idx as f64,
                                Value::Object(obj.clone()),
                                Value::Object(shared_state.clone()),
                            );
                        }
                    }
                    val => {
                        // Non-object, treat as resolved value
                        __internal_promise_allsettled_resolve(idx as f64, val.clone(), Value::Object(shared_state.clone()));
                    }
                }
            }

            // Run the event loop to process any synchronously resolved promises
            run_event_loop()?;

            Ok(Value::Object(result_promise_obj))
        }
        "any" => {
            // Promise.any(iterable)
            if args.is_empty() {
                return Err(JSError::EvaluationError {
                    message: "Promise.any requires at least one argument".to_string(),
                });
            }

            let iterable = evaluate_expr(env, &args[0])?;
            let promises = match iterable {
                Value::Object(arr) => {
                    let mut promises = Vec::new();
                    let mut i = 0;
                    loop {
                        let key = i.to_string();
                        if let Some(val) = crate::core::obj_get_value(&arr, &key)? {
                            promises.push((*val).borrow().clone());
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    promises
                }
                _ => {
                    return Err(JSError::EvaluationError {
                        message: "Promise.any argument must be iterable".to_string(),
                    });
                }
            };

            let result_promise = Rc::new(RefCell::new(JSPromise::new()));
            let result_promise_obj = make_promise_object()?;
            crate::core::obj_set_value(&result_promise_obj, "__promise", Value::Promise(result_promise.clone()))?;

            let num_promises = promises.len();
            if num_promises == 0 {
                // Empty array, reject with AggregateError
                let aggregate_error = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
                crate::core::obj_set_value(&aggregate_error, "name", Value::String(utf8_to_utf16("AggregateError")))?;
                crate::core::obj_set_value(
                    &aggregate_error,
                    "message",
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
                        if let Some(promise_rc) = crate::core::obj_get_value(&obj, "__promise")? {
                            if let Value::Promise(_p) = &*promise_rc.borrow() {
                                let then_callback = Value::Closure(
                                    vec!["value".to_string()],
                                    vec![Statement::Expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_promise_any_resolve".to_string())),
                                        vec![Expr::Var("value".to_string()), Expr::Var("__result_promise".to_string())],
                                    ))],
                                    {
                                        let new_env = env.clone();
                                        crate::core::obj_set_value(
                                            &new_env,
                                            "__result_promise",
                                            Value::Promise(result_promise_clone.clone()),
                                        )?;
                                        new_env
                                    },
                                );

                                let catch_callback = Value::Closure(
                                    vec!["reason".to_string()],
                                    vec![Statement::Expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_promise_any_reject".to_string())),
                                        vec![
                                            Expr::Number(idx as f64),
                                            Expr::Var("reason".to_string()),
                                            Expr::Var("__rejections".to_string()),
                                            Expr::Var("__rejected_count".to_string()),
                                            Expr::Var("__total".to_string()),
                                            Expr::Var("__result_promise".to_string()),
                                        ],
                                    ))],
                                    {
                                        let new_env = env.clone();
                                        crate::core::obj_set_value(
                                            &new_env,
                                            "__rejected_count",
                                            Value::Number(*rejected_count_clone.borrow() as f64),
                                        )?;
                                        crate::core::obj_set_value(&new_env, "__total", Value::Number(num_promises as f64))?;
                                        crate::core::obj_set_value(&new_env, "__result_promise", Value::Promise(result_promise_clone))?;
                                        new_env
                                    },
                                );

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
                return Err(JSError::EvaluationError {
                    message: "Promise.race requires at least one argument".to_string(),
                });
            }

            let iterable = evaluate_expr(env, &args[0])?;
            let promises = match iterable {
                Value::Object(arr) => {
                    let mut promises = Vec::new();
                    let mut i = 0;
                    loop {
                        let key = i.to_string();
                        if let Some(val) = crate::core::obj_get_value(&arr, &key)? {
                            promises.push((*val).borrow().clone());
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

            let result_promise = Rc::new(RefCell::new(JSPromise::new()));
            let result_promise_obj = make_promise_object()?;
            crate::core::obj_set_value(&result_promise_obj, "__promise", Value::Promise(result_promise.clone()))?;

            for promise_val in promises {
                let result_promise_clone = result_promise.clone();

                match promise_val {
                    Value::Object(obj) => {
                        if let Some(promise_rc) = crate::core::obj_get_value(&obj, "__promise")? {
                            if let Value::Promise(_p) = &*promise_rc.borrow() {
                                let then_callback = Value::Closure(
                                    vec!["value".to_string()],
                                    vec![Statement::Expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_promise_race_resolve".to_string())),
                                        vec![Expr::Var("value".to_string()), Expr::Var("__result_promise".to_string())],
                                    ))],
                                    {
                                        let new_env = env.clone();
                                        crate::core::obj_set_value(
                                            &new_env,
                                            "__result_promise",
                                            Value::Promise(result_promise_clone.clone()),
                                        )?;
                                        new_env
                                    },
                                );

                                let catch_callback = Value::Closure(
                                    vec!["reason".to_string()],
                                    vec![Statement::Expr(Expr::Call(
                                        Box::new(Expr::Var("__internal_promise_race_reject".to_string())),
                                        vec![Expr::Var("reason".to_string()), Expr::Var("__result_promise".to_string())],
                                    ))],
                                    {
                                        let new_env = env.clone();
                                        crate::core::obj_set_value(&new_env, "__result_promise", Value::Promise(result_promise_clone))?;
                                        new_env
                                    },
                                );

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

// Internal callback functions for Promise static methods
// These functions are called when individual promises in Promise.allSettled resolve/reject

/// Internal function for Promise.allSettled resolve callback
pub fn __internal_promise_allsettled_resolve(idx: f64, value: Value, shared_state: Value) {
    if let Value::Object(shared_state_obj) = shared_state {
        // Get results array
        if let Some(results_val_rc) = crate::core::obj_get_value(&shared_state_obj, "results").unwrap()
            && let Value::Object(results_obj) = &*results_val_rc.borrow()
        {
            // Create settled result
            let settled = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
            crate::core::obj_set_value(&settled, "status", Value::String(utf8_to_utf16("fulfilled"))).unwrap();
            crate::core::obj_set_value(&settled, "value", value).unwrap();

            // Add to results array at idx
            crate::core::obj_set_value(results_obj, idx.to_string(), Value::Object(settled)).unwrap();
        }

        // Increment completed
        if let Some(completed_val_rc) = crate::core::obj_get_value(&shared_state_obj, "completed").unwrap()
            && let Value::Number(completed) = &*completed_val_rc.borrow()
        {
            let new_completed = completed + 1.0;
            crate::core::obj_set_value(&shared_state_obj, "completed", Value::Number(new_completed)).unwrap();

            // Check if all completed
            if let Some(total_val_rc) = crate::core::obj_get_value(&shared_state_obj, "total").unwrap()
                && let Value::Number(total) = &*total_val_rc.borrow()
                && new_completed == *total
            {
                // Resolve result promise
                if let Some(promise_val_rc) = crate::core::obj_get_value(&shared_state_obj, "result_promise").unwrap()
                    && let Value::Promise(result_promise) = &*promise_val_rc.borrow()
                    && let Some(results_val_rc) = crate::core::obj_get_value(&shared_state_obj, "results").unwrap()
                    && let Value::Object(results_obj) = &*results_val_rc.borrow()
                {
                    resolve_promise(result_promise, Value::Object(results_obj.clone()));
                }
            }
        }
    }
}

/// Internal function for Promise.allSettled reject callback
pub fn __internal_promise_allsettled_reject(idx: f64, reason: Value, shared_state: Value) {
    if let Value::Object(shared_state_obj) = shared_state {
        // Get results array
        if let Some(results_val_rc) = crate::core::obj_get_value(&shared_state_obj, "results").unwrap()
            && let Value::Object(results_obj) = &*results_val_rc.borrow()
        {
            // Create settled result
            let settled = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
            crate::core::obj_set_value(&settled, "status", Value::String(utf8_to_utf16("rejected"))).unwrap();
            crate::core::obj_set_value(&settled, "reason", reason).unwrap();

            // Add to results array at idx
            crate::core::obj_set_value(results_obj, idx.to_string(), Value::Object(settled)).unwrap();
        }

        // Increment completed
        if let Some(completed_val_rc) = crate::core::obj_get_value(&shared_state_obj, "completed").unwrap()
            && let Value::Number(completed) = &*completed_val_rc.borrow()
        {
            let new_completed = completed + 1.0;
            crate::core::obj_set_value(&shared_state_obj, "completed", Value::Number(new_completed)).unwrap();

            // Check if all completed
            if let Some(total_val_rc) = crate::core::obj_get_value(&shared_state_obj, "total").unwrap()
                && let Value::Number(total) = &*total_val_rc.borrow()
                && new_completed == *total
            {
                // Resolve result promise
                if let Some(promise_val_rc) = crate::core::obj_get_value(&shared_state_obj, "result_promise").unwrap()
                    && let Value::Promise(result_promise) = &*promise_val_rc.borrow()
                    && let Some(results_val_rc) = crate::core::obj_get_value(&shared_state_obj, "results").unwrap()
                    && let Value::Object(results_obj) = &*results_val_rc.borrow()
                {
                    resolve_promise(result_promise, Value::Object(results_obj.clone()));
                }
            }
        }
    }
}

/// Internal function for Promise.any resolve callback
pub fn __internal_promise_any_resolve(value: Value, result_promise: Rc<RefCell<JSPromise>>) {
    resolve_promise(&result_promise, value);
}

/// Internal function for Promise.any reject callback
pub fn __internal_promise_any_reject(
    idx: f64,
    reason: Value,
    rejections: Rc<RefCell<Vec<Option<Value>>>>,
    rejected_count: Rc<RefCell<usize>>,
    total: usize,
    result_promise: Rc<RefCell<JSPromise>>,
) {
    let idx = idx as usize;
    rejections.borrow_mut()[idx] = Some(reason);
    *rejected_count.borrow_mut() += 1;

    if *rejected_count.borrow() == total {
        // All promises rejected, create AggregateError
        let aggregate_error = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
        crate::core::obj_set_value(&aggregate_error, "name", Value::String(utf8_to_utf16("AggregateError"))).unwrap();
        crate::core::obj_set_value(
            &aggregate_error,
            "message",
            Value::String(utf8_to_utf16("All promises were rejected")),
        )
        .unwrap();

        let errors_array = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
        let rejections_vec = rejections.borrow();
        for (i, rejection) in rejections_vec.iter().enumerate() {
            if let Some(err) = rejection {
                crate::core::obj_set_value(&errors_array, i.to_string(), err.clone()).unwrap();
            }
        }
        crate::core::obj_set_value(&aggregate_error, "errors", Value::Object(errors_array)).unwrap();

        reject_promise(&result_promise, Value::Object(aggregate_error));
    }
}

/// Internal function for Promise.race resolve callback
pub fn __internal_promise_race_resolve(value: Value, result_promise: Rc<RefCell<JSPromise>>) {
    resolve_promise(&result_promise, value);
}

/// Internal function for Promise.race reject callback
pub fn __internal_promise_race_reject(reason: Value, result_promise: Rc<RefCell<JSPromise>>) {
    reject_promise(&result_promise, reason);
}
