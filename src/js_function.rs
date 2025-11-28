use crate::core::{
    evaluate_expr, obj_get_value, obj_set_value, utf8_to_utf16, Expr, JSObjectData, JSObjectDataPtr, PromiseState, Statement, Value,
};
use crate::error::JSError;
use crate::js_array::handle_array_constructor;
use crate::js_date::handle_date_constructor;
use std::cell::RefCell;
use std::rc::Rc;

pub fn handle_global_function(func_name: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match func_name {
        "std.sprintf" => crate::sprintf::handle_sprintf_call(env, args),
        "String" => {
            // String() constructor
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                match arg_val {
                    Value::Number(n) => Ok(Value::String(utf8_to_utf16(&n.to_string()))),
                    Value::String(s) => Ok(Value::String(s.clone())),
                    Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
                    Value::Undefined => Ok(Value::String(utf8_to_utf16("undefined"))),
                    Value::Object(_) => Ok(Value::String(utf8_to_utf16("[object Object]"))),
                    Value::Function(name) => Ok(Value::String(utf8_to_utf16(&format!("[Function: {name}]")))),
                    Value::Closure(_, _, _) => Ok(Value::String(utf8_to_utf16("[Function]"))),
                    Value::ClassDefinition(_) => Ok(Value::String(utf8_to_utf16("[Class]"))),
                    Value::Getter(_, _) => Ok(Value::String(utf8_to_utf16("[Getter]"))),
                    Value::Setter(_, _, _) => Ok(Value::String(utf8_to_utf16("[Setter]"))),
                    Value::Property { .. } => Ok(Value::String(utf8_to_utf16("[Property]"))),
                    Value::Promise(_) => Ok(Value::String(utf8_to_utf16("[object Promise]"))),
                }
            } else {
                Ok(Value::String(Vec::new())) // String() with no args returns empty string
            }
        }

        "parseInt" => {
            if args.is_empty() {
                return Err(JSError::TypeError {
                    message: "parseInt requires at least one argument".to_string(),
                });
            }
            let arg_val = evaluate_expr(env, &args[0])?;
            match arg_val {
                Value::String(s) => {
                    let str_val = String::from_utf16_lossy(&s);
                    // Parse integer from the beginning of the string
                    let trimmed = str_val.trim();
                    if trimmed.is_empty() {
                        return Ok(Value::Number(f64::NAN));
                    }
                    let mut end_pos = 0;
                    let mut chars = trimmed.chars();
                    if let Some(first_char) = chars.next() {
                        if first_char == '-' || first_char == '+' || first_char.is_ascii_digit() {
                            end_pos = 1;
                            for ch in chars {
                                if ch.is_ascii_digit() {
                                    end_pos += 1;
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                    if end_pos == 0 {
                        return Ok(Value::Number(f64::NAN));
                    }
                    let num_str = &trimmed[0..end_pos];
                    match num_str.parse::<i32>() {
                        Ok(n) => Ok(Value::Number(n as f64)),
                        Err(_) => Ok(Value::Number(f64::NAN)), // This shouldn't happen with our validation
                    }
                }
                Value::Number(n) => Ok(Value::Number(n.trunc())),
                Value::Boolean(b) => Ok(Value::Number(if b { 1.0 } else { 0.0 })),
                Value::Undefined => Ok(Value::Number(f64::NAN)),
                _ => {
                    // Convert to string first, then parse
                    let str_val = match arg_val {
                        Value::Object(_) => "[object Object]".to_string(),
                        Value::Function(name) => format!("[Function: {}]", name),
                        Value::Closure(_, _, _) => "[Function]".to_string(),
                        _ => unreachable!(), // All cases covered above
                    };
                    match str_val.parse::<i32>() {
                        Ok(n) => Ok(Value::Number(n as f64)),
                        Err(_) => Ok(Value::Number(f64::NAN)),
                    }
                }
            }
        }
        "parseFloat" => {
            if args.is_empty() {
                return Err(JSError::TypeError {
                    message: "parseFloat requires at least one argument".to_string(),
                });
            }
            let arg_val = evaluate_expr(env, &args[0])?;
            match arg_val {
                Value::String(s) => {
                    let str_val = String::from_utf16_lossy(&s);
                    let trimmed = str_val.trim();
                    if trimmed.is_empty() {
                        return Ok(Value::Number(f64::NAN));
                    }
                    match trimmed.parse::<f64>() {
                        Ok(n) => Ok(Value::Number(n)),
                        Err(_) => Ok(Value::Number(f64::NAN)),
                    }
                }
                Value::Number(n) => Ok(Value::Number(n)),
                Value::Boolean(b) => Ok(Value::Number(if b { 1.0 } else { 0.0 })),
                Value::Undefined => Ok(Value::Number(f64::NAN)),
                _ => {
                    // Convert to string first, then parse
                    let str_val = match arg_val {
                        Value::Object(_) => "[object Object]".to_string(),
                        Value::Function(name) => format!("[Function: {}]", name),
                        Value::Closure(_, _, _) => "[Function]".to_string(),
                        _ => unreachable!(), // All cases covered above
                    };
                    match str_val.parse::<f64>() {
                        Ok(n) => Ok(Value::Number(n)),
                        Err(_) => Ok(Value::Number(f64::NAN)),
                    }
                }
            }
        }
        "isNaN" => {
            if args.is_empty() {
                return Err(JSError::TypeError {
                    message: "isNaN requires at least one argument".to_string(),
                });
            }
            let arg_val = evaluate_expr(env, &args[0])?;
            match arg_val {
                Value::Number(n) => Ok(Value::Boolean(n.is_nan())),
                Value::String(s) => {
                    let str_val = String::from_utf16_lossy(&s);
                    match str_val.trim().parse::<f64>() {
                        Ok(n) => Ok(Value::Boolean(n.is_nan())),
                        Err(_) => Ok(Value::Boolean(true)), // Non-numeric strings are NaN when parsed
                    }
                }
                Value::Boolean(_) => Ok(Value::Boolean(false)), // Booleans are never NaN
                Value::Undefined => Ok(Value::Boolean(true)),   // undefined is NaN
                _ => Ok(Value::Boolean(false)),                 // Objects, functions, etc. are not NaN
            }
        }
        "isFinite" => {
            if args.is_empty() {
                return Err(JSError::TypeError {
                    message: "isFinite requires at least one argument".to_string(),
                });
            }
            let arg_val = evaluate_expr(env, &args[0])?;
            match arg_val {
                Value::Number(n) => Ok(Value::Boolean(n.is_finite())),
                Value::String(s) => {
                    let str_val = String::from_utf16_lossy(&s);
                    match str_val.trim().parse::<f64>() {
                        Ok(n) => Ok(Value::Boolean(n.is_finite())),
                        Err(_) => Ok(Value::Boolean(false)), // Non-numeric strings are not finite
                    }
                }
                Value::Boolean(_) => Ok(Value::Boolean(true)), // Booleans are finite
                Value::Undefined => Ok(Value::Boolean(false)), // undefined is not finite
                _ => Ok(Value::Boolean(false)),                // Objects, functions, etc. are not finite
            }
        }
        "encodeURIComponent" => {
            if !args.is_empty() {
                let arg_val = evaluate_expr(env, &args[0])?;
                match arg_val {
                    Value::String(s) => {
                        let str_val = String::from_utf16_lossy(&s);
                        // Simple URI encoding - replace spaces with %20 and some special chars
                        let encoded = str_val
                            .replace("%", "%25")
                            .replace(" ", "%20")
                            .replace("\"", "%22")
                            .replace("'", "%27")
                            .replace("<", "%3C")
                            .replace(">", "%3E")
                            .replace("&", "%26");
                        Ok(Value::String(utf8_to_utf16(&encoded)))
                    }
                    _ => {
                        // For non-string values, convert to string first
                        let str_val = match arg_val {
                            Value::Number(n) => n.to_string(),
                            Value::Boolean(b) => b.to_string(),
                            _ => "[object Object]".to_string(),
                        };
                        Ok(Value::String(utf8_to_utf16(&str_val)))
                    }
                }
            } else {
                Ok(Value::String(Vec::new()))
            }
        }
        "decodeURIComponent" => {
            if !args.is_empty() {
                let arg_val = evaluate_expr(env, &args[0])?;
                match arg_val {
                    Value::String(s) => {
                        let str_val = String::from_utf16_lossy(&s);
                        // Simple URI decoding - replace %20 with spaces and some special chars
                        let decoded = str_val
                            .replace("%20", " ")
                            .replace("%22", "\"")
                            .replace("%27", "'")
                            .replace("%3C", "<")
                            .replace("%3E", ">")
                            .replace("%26", "&")
                            .replace("%25", "%");
                        Ok(Value::String(utf8_to_utf16(&decoded)))
                    }
                    _ => {
                        // For non-string values, convert to string first
                        let str_val = match arg_val {
                            Value::Number(n) => n.to_string(),
                            Value::Boolean(b) => b.to_string(),
                            _ => "[object Object]".to_string(),
                        };
                        Ok(Value::String(utf8_to_utf16(&str_val)))
                    }
                }
            } else {
                Ok(Value::String(Vec::new()))
            }
        }
        "Array" => handle_array_constructor(args, env),
        "Number" => {
            // Number constructor
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                match arg_val {
                    Value::Number(n) => Ok(Value::Number(n)),
                    Value::String(s) => {
                        let str_val = String::from_utf16_lossy(&s);
                        match str_val.trim().parse::<f64>() {
                            Ok(n) => Ok(Value::Number(n)),
                            Err(_) => Ok(Value::Number(f64::NAN)),
                        }
                    }
                    Value::Boolean(b) => Ok(Value::Number(if b { 1.0 } else { 0.0 })),
                    _ => Ok(Value::Number(f64::NAN)),
                }
            } else {
                Ok(Value::Number(0.0)) // Number() with no args returns 0
            }
        }
        "Boolean" => {
            // Boolean constructor
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                let bool_val = match arg_val {
                    Value::Boolean(b) => b,
                    Value::Number(n) => n != 0.0 && !n.is_nan(),
                    Value::String(s) => !s.is_empty(),
                    Value::Object(_) => true,
                    Value::Undefined => false,
                    _ => false,
                };
                Ok(Value::Boolean(bool_val))
            } else {
                Ok(Value::Boolean(false)) // Boolean() with no args returns false
            }
        }
        "Date" => {
            // Date constructor - create a Date object
            handle_date_constructor(args, env)
        }
        "new" => {
            // Handle new expressions: new Constructor(args)
            if args.len() == 1 {
                if let Expr::Call(constructor_expr, constructor_args) = &args[0] {
                    if let Expr::Var(constructor_name) = &**constructor_expr {
                        match constructor_name.as_str() {
                            "RegExp" => return crate::js_regexp::handle_regexp_constructor(constructor_args, env),
                            "Array" => return crate::js_array::handle_array_constructor(constructor_args, env),
                            "Date" => return crate::js_date::handle_date_constructor(constructor_args, env),
                            "Promise" => return handle_promise_constructor(constructor_args, env),
                            _ => {
                                return Err(JSError::EvaluationError {
                                    message: format!("Constructor {constructor_name} not implemented"),
                                })
                            }
                        }
                    }
                }
            }
            Err(JSError::EvaluationError {
                message: "Invalid new expression".to_string(),
            })
        }
        "eval" => {
            // eval function - execute the code
            if !args.is_empty() {
                let arg_val = evaluate_expr(env, &args[0])?;
                match arg_val {
                    Value::String(s) => {
                        let code = String::from_utf16_lossy(&s);
                        crate::core::evaluate_script(&code)
                    }
                    _ => Ok(arg_val),
                }
            } else {
                Ok(Value::Undefined)
            }
        }
        "encodeURI" => {
            if !args.is_empty() {
                let arg_val = evaluate_expr(env, &args[0])?;
                match arg_val {
                    Value::String(s) => {
                        let str_val = String::from_utf16_lossy(&s);
                        // Simple URI encoding - replace spaces with %20
                        let encoded = str_val.replace(" ", "%20");
                        Ok(Value::String(utf8_to_utf16(&encoded)))
                    }
                    _ => {
                        let str_val = match arg_val {
                            Value::Number(n) => n.to_string(),
                            Value::Boolean(b) => b.to_string(),
                            _ => "[object Object]".to_string(),
                        };
                        Ok(Value::String(utf8_to_utf16(&str_val)))
                    }
                }
            } else {
                Ok(Value::String(Vec::new()))
            }
        }
        "decodeURI" => {
            if !args.is_empty() {
                let arg_val = evaluate_expr(env, &args[0])?;
                match arg_val {
                    Value::String(s) => {
                        let str_val = String::from_utf16_lossy(&s);
                        // Simple URI decoding - replace %20 with spaces
                        let decoded = str_val.replace("%20", " ");
                        Ok(Value::String(utf8_to_utf16(&decoded)))
                    }
                    _ => {
                        let str_val = match arg_val {
                            Value::Number(n) => n.to_string(),
                            Value::Boolean(b) => b.to_string(),
                            _ => "[object Object]".to_string(),
                        };
                        Ok(Value::String(utf8_to_utf16(&str_val)))
                    }
                }
            } else {
                Ok(Value::String(Vec::new()))
            }
        }
        "__resolve_promise_internal" => {
            // Internal function to resolve a promise by ID asynchronously
            if args.len() < 2 {
                return Err(JSError::TypeError {
                    message: "__resolve_promise_internal requires promise ID and value".to_string(),
                });
            }
            let id_val = evaluate_expr(env, &args[0])?;
            let value = evaluate_expr(env, &args[1])?;

            match id_val {
                Value::String(id_utf16) => {
                    let promise_id = String::from_utf16_lossy(&id_utf16);
                    let promise_key = format!("__promise_{}", promise_id);

                    if let Some(promise_val) = crate::core::obj_get_value(env, &promise_key)? {
                        if let Value::Promise(promise_rc) = &*promise_val.borrow() {
                            // Queue asynchronous resolution task
                            crate::core::queue_task(crate::core::Task::PromiseResolve {
                                promise: promise_rc.clone(),
                                value,
                            });
                        }
                    }
                    Ok(Value::Undefined)
                }
                _ => Err(JSError::TypeError {
                    message: "Invalid promise ID".to_string(),
                }),
            }
        }
        "__reject_promise_internal" => {
            // Internal function to reject a promise by ID asynchronously
            if args.len() < 2 {
                return Err(JSError::TypeError {
                    message: "__reject_promise_internal requires promise ID and reason".to_string(),
                });
            }
            let id_val = evaluate_expr(env, &args[0])?;
            let reason = evaluate_expr(env, &args[1])?;

            match id_val {
                Value::String(id_utf16) => {
                    let promise_id = String::from_utf16_lossy(&id_utf16);
                    let promise_key = format!("__promise_{}", promise_id);

                    if let Some(promise_val) = crate::core::obj_get_value(env, &promise_key)? {
                        if let Value::Promise(promise_rc) = &*promise_val.borrow() {
                            // Queue asynchronous rejection task
                            crate::core::queue_task(crate::core::Task::PromiseReject {
                                promise: promise_rc.clone(),
                                reason,
                            });
                        }
                    }
                    Ok(Value::Undefined)
                }
                _ => Err(JSError::TypeError {
                    message: "Invalid promise ID".to_string(),
                }),
            }
        }
        _ => Err(JSError::EvaluationError {
            message: format!("Global function {} is not implemented", func_name),
        }),
    }
}

pub fn handle_promise_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Promise constructor
    if args.len() != 1 {
        return Err(JSError::TypeError {
            message: "Promise constructor requires exactly one argument".to_string(),
        });
    }

    let executor = evaluate_expr(env, &args[0])?;
    match executor {
        Value::Closure(params, body, captured_env) => {
            if params.len() != 2 {
                return Err(JSError::TypeError {
                    message: "Promise executor function must accept exactly 2 parameters (resolve, reject)".to_string(),
                });
            }

            // Create a new Promise object
            let promise = Rc::new(RefCell::new(crate::core::JSPromise::new()));
            let promise_obj = Rc::new(RefCell::new(crate::core::JSObjectData::new()));

            // Set the __promise marker
            crate::core::obj_set_value(&promise_obj, "__promise", Value::Promise(promise.clone()))?;

            // Generate a unique ID for this promise
            use std::time::{SystemTime, UNIX_EPOCH};
            let promise_id = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos().to_string();

            // Store the promise in a global registry (simplified - in real implementation use a proper registry)
            // For now, we'll use the environment to store promises by ID
            crate::core::env_set(env, format!("__promise_{}", promise_id), Value::Promise(promise.clone()))?;

            // Create resolve function as a closure that captures the promise
            let resolve_func = Value::Closure(
                vec!["value".to_string()],
                vec![Statement::Return(Some(Expr::Call(
                    Box::new(Expr::Var("__resolve_promise_internal".to_string())),
                    vec![
                        Expr::Value(Value::String(crate::core::utf8_to_utf16(&promise_id))),
                        Expr::Var("value".to_string()),
                    ],
                )))],
                env.clone(),
            );

            // Create reject function as a closure that captures the promise
            let reject_func = Value::Closure(
                vec!["reason".to_string()],
                vec![Statement::Return(Some(Expr::Call(
                    Box::new(Expr::Var("__reject_promise_internal".to_string())),
                    vec![
                        Expr::Value(Value::String(crate::core::utf8_to_utf16(&promise_id))),
                        Expr::Var("reason".to_string()),
                    ],
                )))],
                env.clone(),
            );

            // Execute the executor function synchronously for now
            let func_env = captured_env.clone();
            crate::core::env_set(&func_env, &params[0], resolve_func)?;
            crate::core::env_set(&func_env, &params[1], reject_func)?;
            crate::core::env_set(&func_env, "__promise_id", Value::String(crate::core::utf8_to_utf16(&promise_id)))?;

            // Execute the executor
            let _ = crate::core::evaluate_statements(&func_env, &body);

            Ok(Value::Object(promise_obj))
        }
        _ => Err(JSError::TypeError {
            message: "Promise constructor requires a function as argument".to_string(),
        }),
    }
}

pub fn handle_promise_static_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "all" => {
            // Promise.all(iterable) - simplified synchronous implementation
            if args.is_empty() {
                return Err(JSError::TypeError {
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
                        if let Some(val) = obj_get_value(&arr, &key)? {
                            promises.push((*val).clone());
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    promises
                }
                _ => {
                    return Err(JSError::TypeError {
                        message: "Promise.all argument must be iterable".to_string(),
                    });
                }
            };

            // Create a new promise that resolves when all promises resolve
            let result_promise = Rc::new(RefCell::new(crate::core::JSPromise::new()));
            let result_promise_obj = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
            obj_set_value(&result_promise_obj, "__promise", Value::Promise(result_promise.clone()))?;

            // For now, check if all values are already resolved (synchronous implementation)
            let mut all_resolved = true;
            let mut results = Vec::new();
            let mut rejection_reason = None;

            for promise_val in promises {
                match &*promise_val.borrow() {
                    Value::Object(obj) => {
                        if let Some(promise_rc) = obj_get_value(obj, "__promise")? {
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
                    let result_arr = Rc::new(RefCell::new(JSObjectData::new()));
                    for (idx, val) in results.iter().enumerate() {
                        obj_set_value(&result_arr, idx.to_string(), val.clone())?;
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
                return Err(JSError::TypeError {
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
                        if let Some(val) = obj_get_value(&arr, &key)? {
                            promises.push((*val).clone());
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    promises
                }
                _ => {
                    return Err(JSError::TypeError {
                        message: "Promise.race argument must be iterable".to_string(),
                    });
                }
            };

            // Create a new promise that resolves/rejects when the first promise settles
            let result_promise = Rc::new(RefCell::new(crate::core::JSPromise::new()));
            let result_promise_obj = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
            obj_set_value(&result_promise_obj, "__promise", Value::Promise(result_promise.clone()))?;

            // For now, check if any value is already settled (synchronous implementation)
            for promise_val in promises {
                match &*promise_val.borrow() {
                    Value::Object(obj) => {
                        if let Some(promise_rc) = obj_get_value(obj, "__promise")? {
                            if let Value::Promise(p) = &*promise_rc.borrow() {
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
