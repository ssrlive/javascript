use crate::{error::JSError, quickjs::JSObjectData};
use std::cell::RefCell;
use std::rc::Rc;

use super::quickjs::{
    env_get, env_set, evaluate_expr, evaluate_statements, obj_get, obj_set_rc, obj_set_val, utf8_to_utf16, value_to_sort_string,
    values_equal, Expr, Value,
};

/// Handle Array static method calls (Array.isArray, Array.from, Array.of)
pub(crate) fn handle_array_static_method(method: &str, args: &[Expr], env: &JSObjectData) -> Result<Value, JSError> {
    match method {
        "isArray" => {
            if args.len() != 1 {
                return Err(JSError::EvaluationError {
                    message: "Array.isArray requires exactly one argument".to_string(),
                });
            }

            let arg = evaluate_expr(env, &args[0])?;
            let is_array = match arg {
                Value::Object(obj_map) => is_array(&obj_map),
                _ => false,
            };
            Ok(Value::Boolean(is_array))
        }
        "from" => {
            // Array.from(iterable, mapFn?, thisArg?)
            if args.is_empty() {
                return Err(JSError::EvaluationError {
                    message: "Array.from requires at least one argument".to_string(),
                });
            }

            let iterable = evaluate_expr(env, &args[0])?;
            let map_fn = if args.len() > 1 {
                Some(evaluate_expr(env, &args[1])?)
            } else {
                None
            };

            let mut result = Vec::new();

            // Handle different types of iterables
            match iterable {
                Value::Object(obj_map) => {
                    // If it's an array-like object
                    if is_array(&obj_map) {
                        let len = get_array_length(&obj_map).unwrap_or(0);

                        for i in 0..len {
                            if let Some(val) = obj_get(&obj_map, &i.to_string()) {
                                let element = val.borrow().clone();
                                if let Some(ref fn_val) = map_fn {
                                    match fn_val {
                                        Value::Closure(params, body, captured_env) => {
                                            let mut func_env = captured_env.clone();
                                            if params.len() >= 1 {
                                                env_set(&mut func_env, params[0].as_str(), element);
                                            }
                                            if params.len() >= 2 {
                                                env_set(&mut func_env, params[1].as_str(), Value::Number(i as f64));
                                            }
                                            let mapped = evaluate_statements(&mut func_env, &body)?;
                                            result.push(mapped);
                                        }
                                        _ => {
                                            return Err(JSError::EvaluationError {
                                                message: "Array.from map function must be a function".to_string(),
                                            });
                                        }
                                    }
                                } else {
                                    result.push(element);
                                }
                            }
                        }
                    } else {
                        return Err(JSError::EvaluationError {
                            message: "Array.from iterable must be array-like".to_string(),
                        });
                    }
                }
                _ => {
                    return Err(JSError::EvaluationError {
                        message: "Array.from iterable must be array-like".to_string(),
                    });
                }
            }

            let mut new_array = JSObjectData::new();
            set_array_length(&mut new_array, result.len());
            for (i, val) in result.into_iter().enumerate() {
                obj_set_val(&mut new_array, &i.to_string(), val);
            }
            Ok(Value::Object(new_array))
        }
        "of" => {
            // Array.of(...elements)
            let mut new_array = JSObjectData::new();
            for (i, arg) in args.iter().enumerate() {
                let val = evaluate_expr(env, arg)?;
                obj_set_val(&mut new_array, &i.to_string(), val);
            }
            set_array_length(&mut new_array, args.len());
            Ok(Value::Object(new_array))
        }
        _ => Err(JSError::EvaluationError {
            message: format!("Array.{} is not implemented", method),
        }),
    }
}

/// Handle Array constructor calls
pub(crate) fn handle_array_constructor(args: &[Expr], env: &JSObjectData) -> Result<Value, JSError> {
    if args.is_empty() {
        // Array() - create empty array
        let mut array_obj = JSObjectData::new();
        set_array_length(&mut array_obj, 0);
        Ok(Value::Object(array_obj))
    } else if args.len() == 1 {
        // Array(length) or Array(element)
        let arg_val = evaluate_expr(env, &args[0])?;
        match arg_val {
            Value::Number(n) => {
                if n.fract() == 0.0 && n >= 0.0 && n <= u32::MAX as f64 {
                    // Array(length) - create array with specified length
                    let mut array_obj = JSObjectData::new();
                    set_array_length(&mut array_obj, n as usize);
                    Ok(Value::Object(array_obj))
                } else {
                    // Invalid length
                    Ok(Value::Undefined)
                }
            }
            _ => {
                // Array(element) - create array with single element
                let mut array_obj = JSObjectData::new();
                obj_set_val(&mut array_obj, "0", arg_val);
                set_array_length(&mut array_obj, 1);
                Ok(Value::Object(array_obj))
            }
        }
    } else {
        // Array(element1, element2, ...) - create array with multiple elements
        let mut array_obj = JSObjectData::new();
        for (i, arg) in args.iter().enumerate() {
            let arg_val = evaluate_expr(env, arg)?;
            obj_set_val(&mut array_obj, &i.to_string(), arg_val);
        }
        set_array_length(&mut array_obj, args.len());
        Ok(Value::Object(array_obj))
    }
}

/// Handle Array instance method calls
pub(crate) fn handle_array_instance_method(
    obj_map: &mut JSObjectData,
    method: &str,
    args: &[Expr],
    env: &JSObjectData,
    obj_expr: &Expr,
) -> Result<Value, JSError> {
    match method {
        "push" => {
            if args.len() >= 1 {
                // Try to mutate the original object in the environment when possible
                // so that push is chainable (returns the array) and mutations persist.
                // Evaluate all args and append them.
                // First determine current length from the local obj_map
                let mut current_len = get_array_length(obj_map).unwrap_or(0);

                // Helper closure to push a value into a map
                let mut push_into_map = |map: &mut JSObjectData, val: Value| {
                    obj_set_val(map, &current_len.to_string(), val);
                    current_len += 1;
                };

                // If obj_expr is a variable referring to an object stored in env,
                // mutate that stored object directly so changes persist.
                if let Expr::Var(varname) = obj_expr {
                    if let Some(rc_val) = env_get(env, varname) {
                        let mut borrowed = rc_val.borrow_mut();
                        if let Value::Object(ref mut map) = *borrowed {
                            for arg in args {
                                let val = evaluate_expr(env, arg)?;
                                push_into_map(map, val);
                            }
                            set_array_length(map, current_len);

                            // Return the original object
                            return Ok(Value::Object(map.clone()));
                        }
                    }
                }

                // Fallback: mutate the local obj_map copy
                for arg in args {
                    let val = evaluate_expr(env, arg)?;
                    push_into_map(obj_map, val);
                }
                set_array_length(obj_map, current_len);
                // Return the array object (chainable)
                Ok(Value::Object(obj_map.clone()))
            } else {
                Err(JSError::EvaluationError {
                    message: "Array.push expects at least one argument".to_string(),
                })
            }
        }
        "pop" => {
            let current_len = get_array_length(obj_map).unwrap_or(0);
            if current_len > 0 {
                let last_idx = (current_len - 1).to_string();
                let val = obj_map.remove(&last_idx);
                set_array_length(obj_map, current_len - 1);
                Ok(val.map(|v| v.borrow().clone()).unwrap_or(Value::Undefined))
            } else {
                Ok(Value::Undefined)
            }
        }
        "length" => {
            let length = Value::Number(get_array_length(obj_map).unwrap_or(0) as f64);
            Ok(length)
        }
        "join" => {
            let separator = if args.len() >= 1 {
                match evaluate_expr(env, &args[0])? {
                    Value::String(s) => String::from_utf16_lossy(&s),
                    Value::Number(n) => n.to_string(),
                    _ => ",".to_string(),
                }
            } else {
                ",".to_string()
            };

            let current_len = get_array_length(obj_map).unwrap_or(0);

            let mut result = String::new();
            for i in 0..current_len {
                if i > 0 {
                    result.push_str(&separator);
                }
                if let Some(val) = obj_get(obj_map, &i.to_string()) {
                    match &*val.borrow() {
                        Value::String(s) => result.push_str(&String::from_utf16_lossy(s)),
                        Value::Number(n) => result.push_str(&n.to_string()),
                        Value::Boolean(b) => result.push_str(&b.to_string()),
                        _ => result.push_str("[object Object]"),
                    }
                }
            }
            Ok(Value::String(utf8_to_utf16(&result)))
        }
        "slice" => {
            let start = if args.len() >= 1 {
                match evaluate_expr(env, &args[0])? {
                    Value::Number(n) => n as isize,
                    _ => 0isize,
                }
            } else {
                0isize
            };

            let current_len = get_array_length(obj_map).unwrap_or(0);

            let end = if args.len() >= 2 {
                match evaluate_expr(env, &args[1])? {
                    Value::Number(n) => n as isize,
                    _ => current_len as isize,
                }
            } else {
                current_len as isize
            };

            let len = current_len as isize;
            let start = if start < 0 { len + start } else { start };
            let end = if end < 0 { len + end } else { end };

            let start = start.max(0).min(len) as usize;
            let end = end.max(0).min(len) as usize;

            let mut new_array = JSObjectData::new();
            let mut idx = 0;
            for i in start..end {
                if let Some(val) = obj_get(obj_map, &i.to_string()) {
                    obj_set_val(&mut new_array, &idx.to_string(), val.borrow().clone());
                    idx += 1;
                }
            }
            set_array_length(&mut new_array, idx);
            Ok(Value::Object(new_array))
        }
        "forEach" => {
            if args.len() >= 1 {
                // Evaluate the callback expression
                let callback_val = evaluate_expr(env, &args[0])?;
                let current_len = get_array_length(obj_map).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(val) = obj_get(obj_map, &i.to_string()) {
                        match &callback_val {
                            Value::Closure(params, body, captured_env) => {
                                // Prepare function environment
                                let mut func_env = captured_env.clone();
                                // Map params: (element, index, array)
                                if params.len() >= 1 {
                                    env_set(&mut func_env, params[0].as_str(), val.borrow().clone());
                                }
                                if params.len() >= 2 {
                                    env_set(&mut func_env, params[1].as_str(), Value::Number(i as f64));
                                }
                                if params.len() >= 3 {
                                    env_set(&mut func_env, params[2].as_str(), Value::Object(obj_map.clone()));
                                }
                                let _ = evaluate_statements(&mut func_env, &body)?;
                            }
                            _ => {
                                return Err(JSError::EvaluationError {
                                    message: "Array.forEach expects a function".to_string(),
                                })
                            }
                        }
                    }
                }
                Ok(Value::Undefined)
            } else {
                Err(JSError::EvaluationError {
                    message: "Array.forEach expects at least one argument".to_string(),
                })
            }
        }
        "map" => {
            if args.len() >= 1 {
                let callback_val = evaluate_expr(env, &args[0])?;
                let current_len = get_array_length(obj_map).unwrap_or(0);

                let mut new_array = JSObjectData::new();
                let mut idx = 0;
                for i in 0..current_len {
                    if let Some(val) = obj_get(obj_map, &i.to_string()) {
                        match &callback_val {
                            Value::Closure(params, body, captured_env) => {
                                // Prepare function environment
                                let mut func_env = captured_env.clone();
                                if params.len() >= 1 {
                                    env_set(&mut func_env, params[0].as_str(), val.borrow().clone());
                                }
                                if params.len() >= 2 {
                                    env_set(&mut func_env, params[1].as_str(), Value::Number(i as f64));
                                }
                                if params.len() >= 3 {
                                    env_set(&mut func_env, params[2].as_str(), Value::Object(obj_map.clone()));
                                }
                                let res = evaluate_statements(&mut func_env, &body)?;
                                obj_set_val(&mut new_array, &idx.to_string(), res);
                                idx += 1;
                            }
                            _ => {
                                return Err(JSError::EvaluationError {
                                    message: "Array.map expects a function".to_string(),
                                })
                            }
                        }
                    }
                }
                set_array_length(&mut new_array, idx);
                Ok(Value::Object(new_array))
            } else {
                Err(JSError::EvaluationError {
                    message: "Array.map expects at least one argument".to_string(),
                })
            }
        }
        "filter" => {
            if args.len() >= 1 {
                let callback_val = evaluate_expr(env, &args[0])?;
                let current_len = get_array_length(obj_map).unwrap_or(0);

                let mut new_array = JSObjectData::new();
                let mut idx = 0;
                for i in 0..current_len {
                    if let Some(val) = obj_get(obj_map, &i.to_string()) {
                        match &callback_val {
                            Value::Closure(params, body, captured_env) => {
                                let mut func_env = captured_env.clone();
                                if params.len() >= 1 {
                                    env_set(&mut func_env, params[0].as_str(), val.borrow().clone());
                                }
                                if params.len() >= 2 {
                                    env_set(&mut func_env, params[1].as_str(), Value::Number(i as f64));
                                }
                                if params.len() >= 3 {
                                    env_set(&mut func_env, params[2].as_str(), Value::Object(obj_map.clone()));
                                }
                                let res = evaluate_statements(&mut func_env, &body)?;
                                // truthy check
                                let include = match res {
                                    Value::Boolean(b) => b,
                                    Value::Number(n) => n != 0.0,
                                    Value::String(ref s) => !s.is_empty(),
                                    Value::Object(_) => true,
                                    Value::Undefined => false,
                                    _ => false,
                                };
                                if include {
                                    obj_set_val(&mut new_array, &idx.to_string(), val.borrow().clone());
                                    idx += 1;
                                }
                            }
                            _ => {
                                return Err(JSError::EvaluationError {
                                    message: "Array.filter expects a function".to_string(),
                                })
                            }
                        }
                    }
                }
                set_array_length(&mut new_array, idx);
                Ok(Value::Object(new_array))
            } else {
                Err(JSError::EvaluationError {
                    message: "Array.filter expects at least one argument".to_string(),
                })
            }
        }
        "reduce" => {
            if args.len() >= 1 {
                let callback_val = evaluate_expr(env, &args[0])?;
                let initial_value = if args.len() >= 2 {
                    Some(evaluate_expr(env, &args[1])?)
                } else {
                    None
                };

                let current_len = get_array_length(obj_map).unwrap_or(0);

                if current_len == 0 && initial_value.is_none() {
                    return Err(JSError::EvaluationError {
                        message: "Array.reduce called on empty array with no initial value".to_string(),
                    });
                }

                let mut accumulator: Value = if let Some(ref val) = initial_value {
                    val.clone()
                } else if let Some(val) = obj_get(obj_map, &0.to_string()) {
                    val.borrow().clone()
                } else {
                    Value::Undefined
                };

                let start_idx = if initial_value.is_some() { 0 } else { 1 };
                for i in start_idx..current_len {
                    if let Some(val) = obj_get(obj_map, &i.to_string()) {
                        match &callback_val {
                            Value::Closure(params, body, captured_env) => {
                                let mut func_env = captured_env.clone();
                                // build args for callback: first acc, then current element
                                if params.len() >= 1 {
                                    env_set(&mut func_env, params[0].as_str(), accumulator.clone());
                                }
                                if params.len() >= 2 {
                                    env_set(&mut func_env, params[1].as_str(), val.borrow().clone());
                                }
                                if params.len() >= 3 {
                                    env_set(&mut func_env, params[2].as_str(), Value::Number(i as f64));
                                }
                                if params.len() >= 4 {
                                    env_set(&mut func_env, params[3].as_str(), Value::Object(obj_map.clone()));
                                }
                                let res = evaluate_statements(&mut func_env, &body)?;
                                accumulator = res;
                            }
                            _ => {
                                return Err(JSError::EvaluationError {
                                    message: "Array.reduce expects a function".to_string(),
                                })
                            }
                        }
                    }
                }
                Ok(accumulator)
            } else {
                Err(JSError::EvaluationError {
                    message: "Array.reduce expects at least one argument".to_string(),
                })
            }
        }
        "find" => {
            if !args.is_empty() {
                let callback = evaluate_expr(env, &args[0])?;
                match callback {
                    Value::Closure(params, body, captured_env) => {
                        let current_len = get_array_length(obj_map).unwrap_or(0);

                        for i in 0..current_len {
                            if let Some(value) = obj_get(obj_map, &i.to_string()) {
                                let element = value.borrow().clone();
                                let index_val = Value::Number(i as f64);

                                // Create new environment for callback
                                let mut func_env = captured_env.clone();
                                if params.len() > 0 {
                                    env_set(&mut func_env, params[0].as_str(), element.clone());
                                }
                                if params.len() > 1 {
                                    env_set(&mut func_env, params[1].as_str(), index_val);
                                }
                                if params.len() > 2 {
                                    env_set(&mut func_env, params[2].as_str(), Value::Object(obj_map.clone()));
                                }

                                let res = evaluate_statements(&mut func_env, &body)?;
                                // truthy check
                                let is_truthy = match res {
                                    Value::Boolean(b) => b,
                                    Value::Number(n) => n != 0.0,
                                    Value::String(ref s) => !s.is_empty(),
                                    Value::Object(_) => true,
                                    Value::Undefined => false,
                                    _ => false,
                                };
                                if is_truthy {
                                    return Ok(element);
                                }
                            }
                        }
                        Ok(Value::Undefined)
                    }
                    _ => Err(JSError::EvaluationError {
                        message: "Array.find expects a function".to_string(),
                    }),
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Array.find expects at least one argument".to_string(),
                })
            }
        }
        "findIndex" => {
            if !args.is_empty() {
                let callback = evaluate_expr(env, &args[0])?;
                match callback {
                    Value::Closure(params, body, captured_env) => {
                        let current_len = get_array_length(obj_map).unwrap_or(0);

                        for i in 0..current_len {
                            if let Some(value) = obj_get(obj_map, &i.to_string()) {
                                let element = value.borrow().clone();
                                let index_val = Value::Number(i as f64);

                                // Create new environment for callback
                                let mut func_env = captured_env.clone();
                                if params.len() > 0 {
                                    env_set(&mut func_env, params[0].as_str(), element.clone());
                                }
                                if params.len() > 1 {
                                    env_set(&mut func_env, params[1].as_str(), index_val);
                                }
                                if params.len() > 2 {
                                    env_set(&mut func_env, params[2].as_str(), Value::Object(obj_map.clone()));
                                }

                                let res = evaluate_statements(&mut func_env, &body)?;
                                // truthy check
                                let is_truthy = match res {
                                    Value::Boolean(b) => b,
                                    Value::Number(n) => n != 0.0,
                                    Value::String(ref s) => !s.is_empty(),
                                    Value::Object(_) => true,
                                    Value::Undefined => false,
                                    _ => false,
                                };
                                if is_truthy {
                                    return Ok(Value::Number(i as f64));
                                }
                            }
                        }
                        Ok(Value::Number(-1.0))
                    }
                    _ => Err(JSError::EvaluationError {
                        message: "Array.findIndex expects a function".to_string(),
                    }),
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Array.findIndex expects at least one argument".to_string(),
                })
            }
        }
        "some" => {
            if !args.is_empty() {
                let callback = evaluate_expr(env, &args[0])?;
                match callback {
                    Value::Closure(params, body, captured_env) => {
                        let current_len = get_array_length(obj_map).unwrap_or(0);

                        for i in 0..current_len {
                            if let Some(value) = obj_get(obj_map, &i.to_string()) {
                                let element = value.borrow().clone();
                                let index_val = Value::Number(i as f64);

                                // Create new environment for callback
                                let mut func_env = captured_env.clone();
                                if params.len() > 0 {
                                    env_set(&mut func_env, params[0].as_str(), element.clone());
                                }
                                if params.len() > 1 {
                                    env_set(&mut func_env, params[1].as_str(), index_val);
                                }
                                if params.len() > 2 {
                                    env_set(&mut func_env, params[2].as_str(), Value::Object(obj_map.clone()));
                                }

                                let res = evaluate_statements(&mut func_env, &body)?;
                                // truthy check
                                let is_truthy = match res {
                                    Value::Boolean(b) => b,
                                    Value::Number(n) => n != 0.0,
                                    Value::String(ref s) => !s.is_empty(),
                                    Value::Object(_) => true,
                                    Value::Undefined => false,
                                    _ => false,
                                };
                                if is_truthy {
                                    return Ok(Value::Boolean(true));
                                }
                            }
                        }
                        Ok(Value::Boolean(false))
                    }
                    _ => Err(JSError::EvaluationError {
                        message: "Array.some expects a function".to_string(),
                    }),
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Array.some expects at least one argument".to_string(),
                })
            }
        }
        "every" => {
            if !args.is_empty() {
                let callback = evaluate_expr(env, &args[0])?;
                match callback {
                    Value::Closure(params, body, captured_env) => {
                        let current_len = get_array_length(obj_map).unwrap_or(0);

                        for i in 0..current_len {
                            if let Some(value) = obj_get(obj_map, &i.to_string()) {
                                let element = value.borrow().clone();
                                let index_val = Value::Number(i as f64);

                                // Create new environment for callback
                                let mut func_env = captured_env.clone();
                                if params.len() > 0 {
                                    env_set(&mut func_env, params[0].as_str(), element.clone());
                                }
                                if params.len() > 1 {
                                    env_set(&mut func_env, params[1].as_str(), index_val);
                                }
                                if params.len() > 2 {
                                    env_set(&mut func_env, params[2].as_str(), Value::Object(obj_map.clone()));
                                }

                                let res = evaluate_statements(&mut func_env, &body)?;
                                // truthy check
                                let is_truthy = match res {
                                    Value::Boolean(b) => b,
                                    Value::Number(n) => n != 0.0,
                                    Value::String(ref s) => !s.is_empty(),
                                    Value::Object(_) => true,
                                    Value::Undefined => false,
                                    _ => false,
                                };
                                if !is_truthy {
                                    return Ok(Value::Boolean(false));
                                }
                            }
                        }
                        Ok(Value::Boolean(true))
                    }
                    _ => Err(JSError::EvaluationError {
                        message: "Array.every expects a function".to_string(),
                    }),
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Array.every expects at least one argument".to_string(),
                })
            }
        }
        "concat" => {
            let mut result = JSObjectData::new();

            // First, copy all elements from current array
            let current_len = get_array_length(obj_map).unwrap_or(0);

            let mut new_index = 0;
            for i in 0..current_len {
                if let Some(val) = obj_get(obj_map, &i.to_string()) {
                    obj_set_val(&mut result, &new_index.to_string(), val.borrow().clone());
                    new_index += 1;
                }
            }

            // Then, append all arguments
            for arg in args {
                let arg_val = evaluate_expr(env, arg)?;
                match arg_val {
                    Value::Object(arg_obj) => {
                        // If argument is an array-like object, copy its elements
                        let arg_len = get_array_length(&arg_obj).unwrap_or(0);
                        for i in 0..arg_len {
                            if let Some(val) = arg_obj.get(&i.to_string()) {
                                obj_set_rc(&mut result, &new_index.to_string(), val.clone());
                                new_index += 1;
                            }
                        }
                    }
                    _ => {
                        // If argument is not an array, append it directly
                        obj_set_val(&mut result, &new_index.to_string(), arg_val);
                        new_index += 1;
                    }
                }
            }

            set_array_length(&mut result, new_index);
            Ok(Value::Object(result))
        }
        "indexOf" => {
            if args.is_empty() {
                return Err(JSError::EvaluationError {
                    message: "Array.indexOf expects at least one argument".to_string(),
                });
            }

            let search_element = evaluate_expr(env, &args[0])?;
            let from_index = if args.len() > 1 {
                match evaluate_expr(env, &args[1])? {
                    Value::Number(n) => n as isize,
                    _ => 0isize,
                }
            } else {
                0isize
            };

            let current_len = get_array_length(obj_map).unwrap_or(0);

            let start = if from_index < 0 {
                (current_len as isize + from_index).max(0) as usize
            } else {
                from_index as usize
            };

            for i in start..current_len {
                if let Some(val) = obj_get(obj_map, &i.to_string()) {
                    if values_equal(&val.borrow(), &search_element) {
                        return Ok(Value::Number(i as f64));
                    }
                }
            }

            Ok(Value::Number(-1.0))
        }
        "includes" => {
            if args.is_empty() {
                return Err(JSError::EvaluationError {
                    message: "Array.includes expects at least one argument".to_string(),
                });
            }

            let search_element = evaluate_expr(env, &args[0])?;
            let from_index = if args.len() > 1 {
                match evaluate_expr(env, &args[1])? {
                    Value::Number(n) => n as isize,
                    _ => 0isize,
                }
            } else {
                0isize
            };

            let current_len = get_array_length(obj_map).unwrap_or(0);

            let start = if from_index < 0 {
                (current_len as isize + from_index).max(0) as usize
            } else {
                from_index as usize
            };

            for i in start..current_len {
                if let Some(val) = obj_get(obj_map, &i.to_string()) {
                    if values_equal(&val.borrow(), &search_element) {
                        return Ok(Value::Boolean(true));
                    }
                }
            }

            Ok(Value::Boolean(false))
        }
        "sort" => {
            let current_len = get_array_length(obj_map).unwrap_or(0);

            // Extract array elements for sorting
            let mut elements: Vec<(String, Value)> = Vec::new();
            for i in 0..current_len {
                if let Some(val) = obj_get(obj_map, &i.to_string()) {
                    elements.push((i.to_string(), val.borrow().clone()));
                }
            }

            // Sort elements
            if args.is_empty() {
                // Default sort (string comparison)
                elements.sort_by(|a, b| {
                    let a_str = value_to_sort_string(&a.1);
                    let b_str = value_to_sort_string(&b.1);
                    a_str.cmp(&b_str)
                });
            } else {
                // Custom sort with compare function
                let compare_fn = evaluate_expr(env, &args[0])?;
                if let Value::Closure(params, body, captured_env) = compare_fn {
                    elements.sort_by(|a, b| {
                        // Create function environment for comparison
                        let mut func_env = captured_env.clone();
                        if params.len() > 0 {
                            env_set(&mut func_env, params[0].as_str(), a.1.clone());
                        }
                        if params.len() > 1 {
                            env_set(&mut func_env, params[1].as_str(), b.1.clone());
                        }

                        match evaluate_statements(&mut func_env, &body) {
                            Ok(Value::Number(n)) => {
                                if n < 0.0 {
                                    std::cmp::Ordering::Less
                                } else if n > 0.0 {
                                    std::cmp::Ordering::Greater
                                } else {
                                    std::cmp::Ordering::Equal
                                }
                            }
                            _ => std::cmp::Ordering::Equal,
                        }
                    });
                } else {
                    return Err(JSError::EvaluationError {
                        message: "Array.sort expects a function as compare function".to_string(),
                    });
                }
            }

            // Update the array with sorted elements
            for (new_index, (_old_key, value)) in elements.into_iter().enumerate() {
                obj_set_val(obj_map, &new_index.to_string(), value);
            }

            Ok(Value::Object(obj_map.clone()))
        }
        "reverse" => {
            let current_len = get_array_length(obj_map).unwrap_or(0);

            // Reverse elements in place
            let mut left = 0;
            let mut right = current_len.saturating_sub(1);

            while left < right {
                let left_key = left.to_string();
                let right_key = right.to_string();

                let left_val = obj_get(obj_map, &left_key).map(|v| v.borrow().clone());
                let right_val = obj_get(obj_map, &right_key).map(|v| v.borrow().clone());

                if let Some(val) = right_val {
                    obj_set_val(obj_map, &left_key, val);
                } else {
                    obj_map.remove(&left_key);
                }

                if let Some(val) = left_val {
                    obj_set_val(obj_map, &right_key, val);
                } else {
                    obj_map.remove(&right_key);
                }

                left += 1;
                right -= 1;
            }

            Ok(Value::Object(obj_map.clone()))
        }
        "splice" => {
            // array.splice(start, deleteCount, ...items)
            let current_len = get_array_length(obj_map).unwrap_or(0);

            let start = if args.len() >= 1 {
                match evaluate_expr(env, &args[0])? {
                    Value::Number(n) => {
                        let mut idx = n as isize;
                        if idx < 0 {
                            idx = current_len as isize + idx;
                        }
                        idx.max(0).min(current_len as isize) as usize
                    }
                    _ => 0,
                }
            } else {
                0
            };

            let delete_count = if args.len() >= 2 {
                match evaluate_expr(env, &args[1])? {
                    Value::Number(n) => n as usize,
                    _ => 0,
                }
            } else {
                current_len
            };

            // Collect elements to be deleted
            let mut deleted_elements = Vec::new();
            for i in start..(start + delete_count).min(current_len) {
                if let Some(val) = obj_get(obj_map, &i.to_string()) {
                    deleted_elements.push(val.borrow().clone());
                }
            }

            // Create new array for deleted elements
            let mut deleted_array = JSObjectData::new();
            for (i, val) in deleted_elements.iter().enumerate() {
                obj_set_val(&mut deleted_array, &i.to_string(), val.clone());
            }
            set_array_length(&mut deleted_array, deleted_elements.len());

            // Remove deleted elements and shift remaining elements
            let mut new_len = start;

            // Copy elements before start (no change needed)

            // Insert new items at start position
            for i in 2..args.len() {
                let item = evaluate_expr(env, &args[i])?;
                obj_set_val(obj_map, &new_len.to_string(), item);
                new_len += 1;
            }

            // Shift remaining elements after deleted section
            let shift_start = start + delete_count;
            for i in shift_start..current_len {
                if let Some(val) = obj_get(obj_map, &i.to_string()) {
                    let value = val.borrow().clone();
                    obj_set_val(obj_map, &(new_len).to_string(), value);
                    new_len += 1;
                }
            }

            // Remove old elements that are now beyond new length
            let mut keys_to_remove = Vec::new();
            for key in obj_map.keys() {
                if let Ok(idx) = key.parse::<usize>() {
                    if idx >= new_len {
                        keys_to_remove.push(key.clone());
                    }
                }
            }
            for key in keys_to_remove {
                obj_map.remove(&key);
            }

            // Update length
            set_array_length(obj_map, new_len);

            Ok(Value::Object(deleted_array))
        }
        "shift" => {
            let current_len = get_array_length(obj_map).unwrap_or(0);

            if current_len > 0 {
                // Get the first element
                // Try to mutate the env-stored object when possible (chainable behavior)
                if let Expr::Var(varname) = obj_expr {
                    if let Some(rc_val) = env_get(env, varname) {
                        let mut borrowed = rc_val.borrow_mut();
                        if let Value::Object(ref mut map) = *borrowed {
                            let first_element = obj_get(map, &0.to_string()).map(|v| v.borrow().clone());
                            // Shift left
                            for i in 1..current_len {
                                let val_rc_opt = obj_get(map, &i.to_string());
                                if let Some(val_rc) = val_rc_opt {
                                    obj_set_rc(map, &(i - 1).to_string(), val_rc);
                                } else {
                                    map.remove(&(i - 1).to_string());
                                }
                            }
                            map.remove(&(current_len - 1).to_string());
                            set_array_length(map, current_len - 1);
                            return Ok(first_element.unwrap_or(Value::Undefined));
                        }
                    }
                }

                // Fallback: mutate the local obj_map copy
                let first_element = obj_get(obj_map, &0.to_string()).map(|v| v.borrow().clone());
                for i in 1..current_len {
                    let val_rc_opt = obj_get(obj_map, &i.to_string());
                    if let Some(val_rc) = val_rc_opt {
                        obj_set_rc(obj_map, &(i - 1).to_string(), val_rc);
                    } else {
                        obj_map.remove(&(i - 1).to_string());
                    }
                }
                obj_map.remove(&(current_len - 1).to_string());
                set_array_length(obj_map, current_len - 1);
                Ok(first_element.unwrap_or(Value::Undefined))
            } else {
                Ok(Value::Undefined)
            }
        }
        "unshift" => {
            let current_len = get_array_length(obj_map).unwrap_or(0);
            if args.is_empty() {
                return Ok(Value::Number(current_len as f64));
            }

            // Try to mutate env-stored object when possible
            if let Expr::Var(varname) = obj_expr {
                if let Some(rc_val) = env_get(env, varname) {
                    let mut borrowed = rc_val.borrow_mut();
                    if let Value::Object(ref mut map) = *borrowed {
                        // Shift right by number of new elements
                        for i in (0..current_len).rev() {
                            let dest = (i + args.len()).to_string();
                            let val_rc_opt = obj_get(map, &i.to_string());
                            if let Some(val_rc) = val_rc_opt {
                                obj_set_rc(map, &dest, val_rc);
                            } else {
                                map.remove(&dest);
                            }
                        }
                        // Insert new elements
                        for (i, arg) in args.iter().enumerate() {
                            let val = evaluate_expr(env, arg)?;
                            obj_set_val(map, &i.to_string(), val);
                        }
                        let new_len = current_len + args.len();
                        set_array_length(map, new_len);
                        return Ok(Value::Number(new_len as f64));
                    }
                }
            }

            // Fallback: mutate local copy (shift right by number of new elements)
            for i in (0..current_len).rev() {
                let dest = (i + args.len()).to_string();
                let val_rc_opt = obj_get(obj_map, &i.to_string());
                if let Some(val_rc) = val_rc_opt {
                    obj_set_rc(obj_map, &dest, val_rc);
                } else {
                    obj_map.remove(&dest);
                }
            }
            for (i, arg) in args.iter().enumerate() {
                let val = evaluate_expr(env, arg)?;
                obj_set_val(obj_map, &i.to_string(), val);
            }
            let new_len = current_len + args.len();
            set_array_length(obj_map, new_len);
            Ok(Value::Number(new_len as f64))
        }
        "fill" => {
            if args.is_empty() {
                return Ok(Value::Object(obj_map.clone()));
            }

            let fill_value = evaluate_expr(env, &args[0])?;

            let current_len = get_array_length(obj_map).unwrap_or(0);

            let start = if args.len() >= 2 {
                match evaluate_expr(env, &args[1])? {
                    Value::Number(n) => {
                        let mut idx = n as isize;
                        if idx < 0 {
                            idx = current_len as isize + idx;
                        }
                        idx.max(0) as usize
                    }
                    _ => 0,
                }
            } else {
                0
            };

            let end = if args.len() >= 3 {
                match evaluate_expr(env, &args[2])? {
                    Value::Number(n) => {
                        let mut idx = n as isize;
                        if idx < 0 {
                            idx = current_len as isize + idx;
                        }
                        idx.max(0) as usize
                    }
                    _ => current_len,
                }
            } else {
                current_len
            };

            for i in start..end.min(current_len) {
                obj_map.insert(i.to_string(), Rc::new(RefCell::new(fill_value.clone())));
            }

            Ok(Value::Object(obj_map.clone()))
        }
        "lastIndexOf" => {
            if args.is_empty() {
                return Ok(Value::Number(-1.0));
            }

            let search_element = evaluate_expr(env, &args[0])?;

            let current_len = get_array_length(obj_map).unwrap_or(0);

            let from_index = if args.len() >= 2 {
                match evaluate_expr(env, &args[1])? {
                    Value::Number(n) => {
                        let mut idx = n as isize;
                        if idx < 0 {
                            idx = current_len as isize + idx;
                        }
                        (idx as usize).min(current_len.saturating_sub(1))
                    }
                    _ => current_len.saturating_sub(1),
                }
            } else {
                current_len.saturating_sub(1)
            };

            // Search from from_index backwards
            for i in (0..=from_index).rev() {
                if let Some(val) = obj_get(obj_map, &i.to_string()) {
                    if values_equal(&val.borrow(), &search_element) {
                        return Ok(Value::Number(i as f64));
                    }
                }
            }

            Ok(Value::Number(-1.0))
        }
        "toString" => {
            let current_len = get_array_length(obj_map).unwrap_or(0);

            let mut result = String::new();
            for i in 0..current_len {
                if i > 0 {
                    result.push(',');
                }
                if let Some(val) = obj_get(obj_map, &i.to_string()) {
                    match &*val.borrow() {
                        Value::String(ref s) => result.push_str(&String::from_utf16_lossy(s)),
                        Value::Number(ref n) => result.push_str(&n.to_string()),
                        Value::Boolean(ref b) => result.push_str(&b.to_string()),
                        _ => result.push_str("[object Object]"),
                    }
                }
            }
            Ok(Value::String(utf8_to_utf16(&result)))
        }
        "flat" => {
            let depth = if args.len() >= 1 {
                match evaluate_expr(env, &args[0])? {
                    Value::Number(n) => n as usize,
                    _ => 1,
                }
            } else {
                1
            };

            let mut result = Vec::new();
            flatten_array(obj_map, &mut result, depth);

            let mut new_array = JSObjectData::new();
            set_array_length(&mut new_array, result.len());
            for (i, val) in result.into_iter().enumerate() {
                obj_set_val(&mut new_array, &i.to_string(), val);
            }
            Ok(Value::Object(new_array))
        }
        "flatMap" => {
            if args.is_empty() {
                return Err(JSError::EvaluationError {
                    message: "Array.flatMap expects at least one argument".to_string(),
                });
            }

            let callback_val = evaluate_expr(env, &args[0])?;
            let current_len = get_array_length(obj_map).unwrap_or(0);

            let mut result = Vec::new();
            for i in 0..current_len {
                if let Some(val) = obj_get(obj_map, &i.to_string()) {
                    match &callback_val {
                        Value::Closure(params, body, captured_env) => {
                            let mut func_env = captured_env.clone();
                            if params.len() >= 1 {
                                env_set(&mut func_env, params[0].as_str(), val.borrow().clone());
                            }
                            if params.len() >= 2 {
                                env_set(&mut func_env, params[1].as_str(), Value::Number(i as f64));
                            }
                            if params.len() >= 3 {
                                env_set(&mut func_env, params[2].as_str(), Value::Object(obj_map.clone()));
                            }
                            let mapped_val = evaluate_statements(&mut func_env, &body)?;
                            flatten_single_value(mapped_val, &mut result, 1);
                        }
                        _ => {
                            return Err(JSError::EvaluationError {
                                message: "Array.flatMap expects a function".to_string(),
                            })
                        }
                    }
                }
            }

            let mut new_array = JSObjectData::new();
            set_array_length(&mut new_array, result.len());
            for (i, val) in result.into_iter().enumerate() {
                obj_set_val(&mut new_array, &i.to_string(), val);
            }
            Ok(Value::Object(new_array))
        }
        "copyWithin" => {
            let current_len = get_array_length(obj_map).unwrap_or(0);

            if args.is_empty() {
                return Ok(Value::Object(obj_map.clone()));
            }

            let target = match evaluate_expr(env, &args[0])? {
                Value::Number(n) => {
                    let mut idx = n as isize;
                    if idx < 0 {
                        idx = current_len as isize + idx;
                    }
                    idx.max(0) as usize
                }
                _ => 0,
            };

            let start = if args.len() >= 2 {
                match evaluate_expr(env, &args[1])? {
                    Value::Number(n) => {
                        let mut idx = n as isize;
                        if idx < 0 {
                            idx = current_len as isize + idx;
                        }
                        idx.max(0) as usize
                    }
                    _ => 0,
                }
            } else {
                0
            };

            let end = if args.len() >= 3 {
                match evaluate_expr(env, &args[2])? {
                    Value::Number(n) => {
                        let mut idx = n as isize;
                        if idx < 0 {
                            idx = current_len as isize + idx;
                        }
                        idx.max(0) as usize
                    }
                    _ => current_len,
                }
            } else {
                current_len
            };

            if target >= current_len || start >= end {
                return Ok(Value::Object(obj_map.clone()));
            }

            let mut temp_values = Vec::new();
            for i in start..end.min(current_len) {
                if let Some(val) = obj_get(obj_map, &i.to_string()) {
                    temp_values.push(val.borrow().clone());
                }
            }

            for (i, val) in temp_values.into_iter().enumerate() {
                let dest_idx = target + i;
                if dest_idx < current_len {
                    obj_set_val(obj_map, &dest_idx.to_string(), val);
                }
            }

            Ok(Value::Object(obj_map.clone()))
        }
        "entries" => {
            let length = get_array_length(obj_map).unwrap_or(0);

            let mut entries = Vec::new();
            for i in 0..length {
                if let Some(val) = obj_get(obj_map, &i.to_string()) {
                    let entry = vec![Value::Number(i as f64), val.borrow().clone()];
                    let mut entry_obj = JSObjectData::new();
                    obj_set_val(&mut entry_obj, &0.to_string(), entry[0].clone());
                    obj_set_val(&mut entry_obj, &1.to_string(), entry[1].clone());
                    set_array_length(&mut entry_obj, 2);
                    entries.push(Value::Object(entry_obj));
                }
            }

            let mut iterator = JSObjectData::new();
            set_array_length(&mut iterator, entries.len());
            for (i, entry) in entries.into_iter().enumerate() {
                obj_set_val(&mut iterator, &i.to_string(), entry);
            }
            Ok(Value::Object(iterator))
        }
        _ => Err(JSError::EvaluationError {
            message: "error".to_string(),
        }), // array method not found
    }
}

// Helper functions for array flattening
fn flatten_array(obj_map: &JSObjectData, result: &mut Vec<Value>, depth: usize) {
    let current_len = get_array_length(obj_map).unwrap_or(0);

    for i in 0..current_len {
        if let Some(val) = obj_get(obj_map, &i.to_string()) {
            let value = val.borrow().clone();
            flatten_single_value(value, result, depth);
        }
    }
}

fn flatten_single_value(value: Value, result: &mut Vec<Value>, depth: usize) {
    if depth == 0 {
        result.push(value);
        return;
    }

    match value {
        Value::Object(obj) => {
            // Check if it's an array-like object
            if is_array(&obj) {
                flatten_array(&obj, result, depth - 1);
            } else {
                result.push(Value::Object(obj));
            }
        }
        _ => {
            result.push(value);
        }
    }
}

/// Check if an object looks like an array (has length and consecutive numeric indices)
pub(crate) fn is_array(obj: &JSObjectData) -> bool {
    if let Some(length_rc) = obj.get("length") {
        if let Value::Number(len) = *length_rc.borrow() {
            let len = len as usize;
            // Check if all indices from 0 to len-1 exist
            for i in 0..len {
                if !obj.contains_key(&i.to_string()) {
                    return false;
                }
            }
            // Check that there are no extra numeric keys beyond len
            for key in obj.keys() {
                if let Ok(idx) = key.parse::<usize>() {
                    if idx >= len {
                        return false;
                    }
                }
            }
            true
        } else {
            false
        }
    } else {
        false
    }
}

pub(crate) fn get_array_length(obj: &JSObjectData) -> Option<usize> {
    if let Some(length_rc) = obj.get("length") {
        if let Value::Number(len) = *length_rc.borrow() {
            if len >= 0.0 && len == len.floor() {
                return Some(len as usize);
            }
        }
    }
    None
}

pub(crate) fn set_array_length(obj: &mut JSObjectData, new_length: usize) {
    obj_set_val(obj, "length", Value::Number(new_length as f64));
}
