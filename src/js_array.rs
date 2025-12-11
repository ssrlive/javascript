use crate::{
    core::{JSObjectDataPtr, PropertyKey, extract_closure_from_value, new_js_object_data},
    error::JSError,
    raise_eval_error,
    unicode::utf8_to_utf16,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::core::{
    Expr, Value, env_get, env_set, evaluate_expr, evaluate_statements, get_own_property, obj_get_value, obj_set_rc, obj_set_value,
    value_to_sort_string, values_equal,
};

/// Handle Array static method calls (Array.isArray, Array.from, Array.of)
pub(crate) fn handle_array_static_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "isArray" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("Array.isArray requires exactly one argument"));
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
                return Err(raise_eval_error!("Array.from requires at least one argument"));
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
                            if let Some(val) = obj_get_value(&obj_map, &i.to_string().into())? {
                                let element = val.borrow().clone();
                                if let Some(ref fn_val) = map_fn {
                                    if let Some((params, body, captured_env)) = extract_closure_from_value(fn_val) {
                                        let func_env = new_js_object_data();
                                        func_env.borrow_mut().prototype = Some(captured_env.clone());
                                        if !params.is_empty() {
                                            env_set(&func_env, params[0].as_str(), element)?;
                                        }
                                        if params.len() >= 2 {
                                            env_set(&func_env, params[1].as_str(), Value::Number(i as f64))?;
                                        }
                                        let mapped = evaluate_statements(&func_env, &body)?;
                                        result.push(mapped);
                                    } else {
                                        return Err(raise_eval_error!("Array.from map function must be a function"));
                                    }
                                } else {
                                    result.push(element);
                                }
                            }
                        }
                    } else {
                        return Err(raise_eval_error!("Array.from iterable must be array-like"));
                    }
                }
                _ => {
                    return Err(raise_eval_error!("Array.from iterable must be array-like"));
                }
            }

            let new_array = new_js_object_data();
            set_array_length(&new_array, result.len())?;
            for (i, val) in result.into_iter().enumerate() {
                obj_set_value(&new_array, &i.to_string().into(), val)?;
            }
            Ok(Value::Object(new_array))
        }
        "of" => {
            // Array.of(...elements)
            let new_array = new_js_object_data();
            for (i, arg) in args.iter().enumerate() {
                let val = evaluate_expr(env, arg)?;
                obj_set_value(&new_array, &i.to_string().into(), val)?;
            }
            set_array_length(&new_array, args.len())?;
            Ok(Value::Object(new_array))
        }
        _ => Err(raise_eval_error!(format!("Array.{method} is not implemented"))),
    }
}

/// Handle Array constructor calls
pub(crate) fn handle_array_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        // Array() - create empty array
        let array_obj = new_js_object_data();
        set_array_length(&array_obj, 0)?;
        Ok(Value::Object(array_obj))
    } else if args.len() == 1 {
        // Array(length) or Array(element)
        let arg_val = evaluate_expr(env, &args[0])?;
        match arg_val {
            Value::Number(n) => {
                if n.is_nan() {
                    return Err(raise_type_error!("Invalid array length"));
                }
                if n.fract() != 0.0 {
                    return Err(raise_type_error!("Array length must be an integer"));
                }
                if n < 0.0 {
                    return Err(raise_type_error!("Array length cannot be negative"));
                }
                if n > u32::MAX as f64 {
                    return Err(raise_type_error!("Array length too large"));
                }
                // Array(length) - create array with specified length
                let array_obj = new_js_object_data();
                set_array_length(&array_obj, n as usize)?;
                Ok(Value::Object(array_obj))
            }
            _ => {
                // Array(element) - create array with single element
                let array_obj = new_js_object_data();
                obj_set_value(&array_obj, &"0".into(), arg_val)?;
                set_array_length(&array_obj, 1)?;
                Ok(Value::Object(array_obj))
            }
        }
    } else {
        // Array(element1, element2, ...) - create array with multiple elements
        let array_obj = new_js_object_data();
        for (i, arg) in args.iter().enumerate() {
            let arg_val = evaluate_expr(env, arg)?;
            obj_set_value(&array_obj, &i.to_string().into(), arg_val)?;
        }
        set_array_length(&array_obj, args.len())?;
        Ok(Value::Object(array_obj))
    }
}

/// Handle Array instance method calls
pub(crate) fn handle_array_instance_method(
    obj_map: &JSObjectDataPtr,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr,
    obj_expr: &Expr,
) -> Result<Value, JSError> {
    match method {
        "push" => {
            if !args.is_empty() {
                // Try to mutate the original object in the environment when possible
                // so that push is chainable (returns the array) and mutations persist.
                // Evaluate all args and append them.
                // First determine current length from the local obj_map
                let mut current_len = get_array_length(obj_map).unwrap_or(0);

                // Helper closure to push a value into a map
                fn push_into_map(map: &JSObjectDataPtr, val: Value, current_len: &mut usize) -> Result<(), JSError> {
                    obj_set_value(map, &current_len.to_string().into(), val)?;
                    *current_len += 1;
                    Ok(())
                }

                // If obj_expr is a variable referring to an object stored in env,
                // mutate that stored object directly so changes persist.
                if let Expr::Var(varname) = obj_expr
                    && let Some(rc_val) = env_get(env, varname)
                {
                    let mut borrowed = rc_val.borrow_mut();
                    if let Value::Object(ref mut map) = *borrowed {
                        for arg in args {
                            let val = evaluate_expr(env, arg)?;
                            push_into_map(map, val, &mut current_len)?;
                        }
                        set_array_length(map, current_len)?;

                        // Return the original object
                        return Ok(Value::Object(map.clone()));
                    }
                }

                // Fallback: mutate the local obj_map copy
                for arg in args {
                    let val = evaluate_expr(env, arg)?;
                    push_into_map(obj_map, val, &mut current_len)?;
                }
                set_array_length(obj_map, current_len)?;
                // Return the array object (chainable)
                Ok(Value::Object(obj_map.clone()))
            } else {
                Err(raise_eval_error!("Array.push expects at least one argument"))
            }
        }
        "pop" => {
            let current_len = get_array_length(obj_map).unwrap_or(0);
            if current_len > 0 {
                let last_idx = (current_len - 1).to_string();
                let val = obj_map.borrow_mut().remove(&last_idx.into());
                set_array_length(obj_map, current_len - 1)?;
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
            let separator = if !args.is_empty() {
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
                if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
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
            let start = if !args.is_empty() {
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

            let new_array = new_js_object_data();
            let mut idx = 0;
            for i in start..end {
                if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
                    obj_set_value(&new_array, &idx.to_string().into(), val.borrow().clone())?;
                    idx += 1;
                }
            }
            set_array_length(&new_array, idx)?;
            Ok(Value::Object(new_array))
        }
        "forEach" => {
            if !args.is_empty() {
                // Evaluate the callback expression
                let callback_val = evaluate_expr(env, &args[0])?;
                let current_len = get_array_length(obj_map).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
                        if let Some((params, body, captured_env)) = extract_closure_from_value(&callback_val) {
                            // Prepare function environment
                            let func_env = new_js_object_data();
                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                            // Map params: (element, index, array)
                            if !params.is_empty() {
                                env_set(&func_env, params[0].as_str(), val.borrow().clone())?;
                            }
                            if params.len() >= 2 {
                                env_set(&func_env, params[1].as_str(), Value::Number(i as f64))?;
                            }
                            if params.len() >= 3 {
                                env_set(&func_env, params[2].as_str(), Value::Object(obj_map.clone()))?;
                            }
                            evaluate_statements(&func_env, &body)?;
                        } else {
                            return Err(raise_eval_error!("Array.forEach expects a function"));
                        }
                    }
                }
                Ok(Value::Undefined)
            } else {
                Err(raise_eval_error!("Array.forEach expects at least one argument"))
            }
        }
        "map" => {
            if !args.is_empty() {
                let callback_val = evaluate_expr(env, &args[0])?;
                let current_len = get_array_length(obj_map).unwrap_or(0);

                let new_array = new_js_object_data();
                let mut idx = 0;
                for i in 0..current_len {
                    if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
                        if let Some((params, body, captured_env)) = extract_closure_from_value(&callback_val) {
                            // Prepare function environment
                            let func_env = new_js_object_data();
                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                            if !params.is_empty() {
                                env_set(&func_env, params[0].as_str(), val.borrow().clone())?;
                            }
                            if params.len() >= 2 {
                                env_set(&func_env, params[1].as_str(), Value::Number(i as f64))?;
                            }
                            if params.len() >= 3 {
                                env_set(&func_env, params[2].as_str(), Value::Object(obj_map.clone()))?;
                            }
                            let res = evaluate_statements(&func_env, &body)?;
                            obj_set_value(&new_array, &idx.to_string().into(), res)?;
                            idx += 1;
                        } else {
                            return Err(raise_eval_error!("Array.map expects a function"));
                        }
                    }
                }
                set_array_length(&new_array, idx)?;
                Ok(Value::Object(new_array))
            } else {
                Err(raise_eval_error!("Array.map expects at least one argument"))
            }
        }
        "filter" => {
            if !args.is_empty() {
                let callback_val = evaluate_expr(env, &args[0])?;
                let current_len = get_array_length(obj_map).unwrap_or(0);

                let new_array = new_js_object_data();
                let mut idx = 0;
                for i in 0..current_len {
                    if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
                        if let Some((params, body, captured_env)) = extract_closure_from_value(&callback_val) {
                            let func_env = new_js_object_data();
                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                            if !params.is_empty() {
                                env_set(&func_env, params[0].as_str(), val.borrow().clone())?;
                            }
                            if params.len() >= 2 {
                                env_set(&func_env, params[1].as_str(), Value::Number(i as f64))?;
                            }
                            if params.len() >= 3 {
                                env_set(&func_env, params[2].as_str(), Value::Object(obj_map.clone()))?;
                            }
                            let res = evaluate_statements(&func_env, &body)?;
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
                                obj_set_value(&new_array, &idx.to_string().into(), val.borrow().clone())?;
                                idx += 1;
                            }
                        } else {
                            return Err(raise_eval_error!("Array.filter expects a function"));
                        }
                    }
                }
                set_array_length(&new_array, idx)?;
                Ok(Value::Object(new_array))
            } else {
                Err(raise_eval_error!("Array.filter expects at least one argument"))
            }
        }
        "reduce" => {
            if !args.is_empty() {
                let callback_val = evaluate_expr(env, &args[0])?;
                let initial_value = if args.len() >= 2 {
                    Some(evaluate_expr(env, &args[1])?)
                } else {
                    None
                };

                let current_len = get_array_length(obj_map).unwrap_or(0);

                if current_len == 0 && initial_value.is_none() {
                    return Err(raise_eval_error!("Array.reduce called on empty array with no initial value"));
                }

                let mut accumulator: Value = if let Some(ref val) = initial_value {
                    val.clone()
                } else if let Some(val) = obj_get_value(obj_map, &"0".into())? {
                    val.borrow().clone()
                } else {
                    Value::Undefined
                };

                let start_idx = if initial_value.is_some() { 0 } else { 1 };
                for i in start_idx..current_len {
                    if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
                        if let Some((params, body, captured_env)) = extract_closure_from_value(&callback_val) {
                            let func_env = new_js_object_data();
                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                            // build args for callback: first acc, then current element
                            if !params.is_empty() {
                                env_set(&func_env, params[0].as_str(), accumulator.clone())?;
                            }
                            if params.len() >= 2 {
                                env_set(&func_env, params[1].as_str(), val.borrow().clone())?;
                            }
                            if params.len() >= 3 {
                                env_set(&func_env, params[2].as_str(), Value::Number(i as f64))?;
                            }
                            if params.len() >= 4 {
                                env_set(&func_env, params[3].as_str(), Value::Object(obj_map.clone()))?;
                            }
                            let res = evaluate_statements(&func_env, &body)?;
                            accumulator = res;
                        } else {
                            return Err(raise_eval_error!("Array.reduce expects a function"));
                        }
                    }
                }
                Ok(accumulator)
            } else {
                Err(raise_eval_error!("Array.reduce expects at least one argument"))
            }
        }
        "find" => {
            if !args.is_empty() {
                let callback = evaluate_expr(env, &args[0])?;
                let current_len = get_array_length(obj_map).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(value) = obj_get_value(obj_map, &i.to_string().into())? {
                        if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                            let element = value.borrow().clone();
                            let index_val = Value::Number(i as f64);

                            // Create new environment for callback
                            let func_env = new_js_object_data();
                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                            if !params.is_empty() {
                                env_set(&func_env, params[0].as_str(), element.clone())?;
                            }
                            if params.len() >= 2 {
                                env_set(&func_env, params[1].as_str(), index_val)?;
                            }
                            if params.len() > 2 {
                                env_set(&func_env, params[2].as_str(), Value::Object(obj_map.clone()))?;
                            }

                            let res = evaluate_statements(&func_env, &body)?;
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
                        } else {
                            return Err(raise_eval_error!("Array.find expects a function"));
                        }
                    }
                }
                Ok(Value::Undefined)
            } else {
                Err(raise_eval_error!("Array.find expects at least one argument"))
            }
        }
        "findIndex" => {
            if !args.is_empty() {
                let callback = evaluate_expr(env, &args[0])?;
                let current_len = get_array_length(obj_map).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(value) = obj_get_value(obj_map, &i.to_string().into())? {
                        if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                            let element = value.borrow().clone();
                            let index_val = Value::Number(i as f64);

                            // Create new environment for callback
                            let func_env = new_js_object_data();
                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                            if !params.is_empty() {
                                env_set(&func_env, params[0].as_str(), element.clone())?;
                            }
                            if params.len() >= 2 {
                                env_set(&func_env, params[1].as_str(), index_val)?;
                            }
                            if params.len() > 2 {
                                env_set(&func_env, params[2].as_str(), Value::Object(obj_map.clone()))?;
                            }

                            let res = evaluate_statements(&func_env, &body)?;
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
                        } else {
                            return Err(raise_eval_error!("Array.findIndex expects a function"));
                        }
                    }
                }
                Ok(Value::Number(-1.0))
            } else {
                Err(raise_eval_error!("Array.findIndex expects at least one argument"))
            }
        }
        "some" => {
            if !args.is_empty() {
                let callback = evaluate_expr(env, &args[0])?;
                let current_len = get_array_length(obj_map).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(value) = obj_get_value(obj_map, &i.to_string().into())? {
                        if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                            let element = value.borrow().clone();
                            let index_val = Value::Number(i as f64);

                            // Create new environment for callback (fresh frame whose prototype is captured_env)
                            let func_env = new_js_object_data();
                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                            if !params.is_empty() {
                                env_set(&func_env, params[0].as_str(), element.clone())?;
                            }
                            if params.len() >= 2 {
                                env_set(&func_env, params[1].as_str(), index_val)?;
                            }
                            if params.len() > 2 {
                                env_set(&func_env, params[2].as_str(), Value::Object(obj_map.clone()))?;
                            }

                            let res = evaluate_statements(&func_env, &body)?;
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
                        } else {
                            return Err(raise_eval_error!("Array.some expects a function"));
                        }
                    }
                }
                Ok(Value::Boolean(false))
            } else {
                Err(raise_eval_error!("Array.some expects at least one argument"))
            }
        }
        "every" => {
            if !args.is_empty() {
                let callback = evaluate_expr(env, &args[0])?;
                let current_len = get_array_length(obj_map).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(value) = obj_get_value(obj_map, &i.to_string().into())? {
                        if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                            let element = value.borrow().clone();
                            let index_val = Value::Number(i as f64);

                            // Create new environment for callback (fresh frame whose prototype is captured_env)
                            let func_env = new_js_object_data();
                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                            if !params.is_empty() {
                                env_set(&func_env, params[0].as_str(), element.clone())?;
                            }
                            if params.len() >= 2 {
                                env_set(&func_env, params[1].as_str(), index_val)?;
                            }
                            if params.len() > 2 {
                                env_set(&func_env, params[2].as_str(), Value::Object(obj_map.clone()))?;
                            }

                            let res = evaluate_statements(&func_env, &body)?;
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
                        } else {
                            return Err(raise_eval_error!("Array.every expects a function"));
                        }
                    }
                }
                Ok(Value::Boolean(true))
            } else {
                Err(raise_eval_error!("Array.every expects at least one argument"))
            }
        }
        "concat" => {
            let result = new_js_object_data();

            // First, copy all elements from current array
            let current_len = get_array_length(obj_map).unwrap_or(0);

            let mut new_index = 0;
            for i in 0..current_len {
                if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
                    obj_set_value(&result, &new_index.to_string().into(), val.borrow().clone())?;
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
                            if let Some(val) = obj_get_value(&arg_obj, &i.to_string().into())? {
                                obj_set_rc(&result, &new_index.to_string().into(), val.clone());
                                new_index += 1;
                            }
                        }
                    }
                    _ => {
                        // If argument is not an array, append it directly
                        obj_set_value(&result, &new_index.to_string().into(), arg_val)?;
                        new_index += 1;
                    }
                }
            }

            set_array_length(&result, new_index)?;
            Ok(Value::Object(result))
        }
        "indexOf" => {
            if args.is_empty() {
                return Err(raise_eval_error!("Array.indexOf expects at least one argument"));
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
                if let Some(val) = obj_get_value(obj_map, &i.to_string().into())?
                    && values_equal(&val.borrow(), &search_element)
                {
                    return Ok(Value::Number(i as f64));
                }
            }

            Ok(Value::Number(-1.0))
        }
        "includes" => {
            if args.is_empty() {
                return Err(raise_eval_error!("Array.includes expects at least one argument"));
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
                if let Some(val) = obj_get_value(obj_map, &i.to_string().into())?
                    && values_equal(&val.borrow(), &search_element)
                {
                    return Ok(Value::Boolean(true));
                }
            }

            Ok(Value::Boolean(false))
        }
        "sort" => {
            let current_len = get_array_length(obj_map).unwrap_or(0);

            // Extract array elements for sorting
            // Note: This implementation uses O(n) extra space for simplicity.
            // For better memory efficiency with large arrays, an in-place sort
            // could be implemented, but it would be more complex with the current
            // object storage model.
            let mut elements: Vec<(String, Value)> = Vec::new();
            for i in 0..current_len {
                if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
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
                if let Some((params, body, captured_env)) = extract_closure_from_value(&compare_fn) {
                    elements.sort_by(|a, b| {
                        // Create function environment for comparison (fresh frame whose prototype is captured_env)
                        let func_env = new_js_object_data();
                        func_env.borrow_mut().prototype = Some(captured_env.clone());
                        let mut param_set = true;
                        if !params.is_empty() && env_set(&func_env, params[0].as_str(), a.1.clone()).is_err() {
                            param_set = false;
                        }
                        if params.len() >= 2 && param_set && env_set(&func_env, params[1].as_str(), b.1.clone()).is_err() {
                            param_set = false;
                        }

                        if !param_set {
                            return std::cmp::Ordering::Equal;
                        }

                        match evaluate_statements(&func_env, &body) {
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
                    return Err(raise_eval_error!("Array.sort expects a function as compare function"));
                }
            }

            // Update the array with sorted elements
            for (new_index, (_old_key, value)) in elements.into_iter().enumerate() {
                obj_set_value(obj_map, &new_index.to_string().into(), value)?;
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

                let left_val = obj_get_value(obj_map, &left_key.clone().into())?.map(|v| v.borrow().clone());
                let right_val = obj_get_value(obj_map, &right_key.clone().into())?.map(|v| v.borrow().clone());

                if let Some(val) = right_val {
                    obj_set_value(obj_map, &left_key.clone().into(), val)?;
                } else {
                    obj_map.borrow_mut().remove(&left_key.clone().into());
                }

                if let Some(val) = left_val {
                    obj_set_value(obj_map, &right_key.clone().into(), val)?;
                } else {
                    obj_map.borrow_mut().remove(&right_key.clone().into());
                }

                left += 1;
                right -= 1;
            }

            Ok(Value::Object(obj_map.clone()))
        }
        "splice" => {
            // array.splice(start, deleteCount, ...items)
            let current_len = get_array_length(obj_map).unwrap_or(0);

            let start = if !args.is_empty() {
                match evaluate_expr(env, &args[0])? {
                    Value::Number(n) => {
                        let mut idx = n as isize;
                        if idx < 0 {
                            idx += current_len as isize;
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
                if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
                    deleted_elements.push(val.borrow().clone());
                }
            }

            // Create new array for deleted elements
            let deleted_array = new_js_object_data();
            for (i, val) in deleted_elements.iter().enumerate() {
                obj_set_value(&deleted_array, &i.to_string().into(), val.clone())?;
            }
            set_array_length(&deleted_array, deleted_elements.len())?;

            // Remove deleted elements and shift remaining elements
            let mut new_len = start;

            // Copy elements before start (no change needed)

            // Insert new items at start position
            for item in args.iter().skip(2) {
                let item_val = evaluate_expr(env, item)?;
                obj_set_value(obj_map, &new_len.to_string().into(), item_val)?;
                new_len += 1;
            }

            // Shift remaining elements after deleted section
            let shift_start = start + delete_count;
            for i in shift_start..current_len {
                if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
                    let value = val.borrow().clone();
                    obj_set_value(obj_map, &new_len.to_string().into(), value)?;
                    new_len += 1;
                }
            }

            // Remove old elements that are now beyond new length
            let mut keys_to_remove = Vec::new();
            for key in obj_map.borrow().keys() {
                if let PropertyKey::String(key_str) = key
                    && let Ok(idx) = key_str.parse::<usize>()
                    && idx >= new_len
                {
                    keys_to_remove.push(key.clone());
                }
            }
            for key in keys_to_remove {
                obj_map.borrow_mut().remove(&key);
            }

            // Update length
            set_array_length(obj_map, new_len)?;

            Ok(Value::Object(deleted_array))
        }
        "shift" => {
            let current_len = get_array_length(obj_map).unwrap_or(0);

            if current_len > 0 {
                // Get the first element
                // Try to mutate the env-stored object when possible (chainable behavior)
                if let Expr::Var(varname) = obj_expr
                    && let Some(rc_val) = env_get(env, varname)
                {
                    let mut borrowed = rc_val.borrow_mut();
                    if let Value::Object(ref mut map) = *borrowed {
                        let first_element = obj_get_value(map, &"0".into())?.map(|v| v.borrow().clone());
                        // Shift left
                        for i in 1..current_len {
                            let val_rc_opt = obj_get_value(map, &i.to_string().into())?;
                            if let Some(val_rc) = val_rc_opt {
                                obj_set_rc(map, &(i - 1).to_string().into(), val_rc);
                            } else {
                                map.borrow_mut().remove(&(i - 1).to_string().into());
                            }
                        }
                        map.borrow_mut().remove(&(current_len - 1).to_string().into());
                        set_array_length(map, current_len - 1)?;
                        return Ok(first_element.unwrap_or(Value::Undefined));
                    }
                }

                // Fallback: mutate the local obj_map copy
                let first_element = obj_get_value(obj_map, &"0".into())?.map(|v| v.borrow().clone());
                for i in 1..current_len {
                    let val_rc_opt = obj_get_value(obj_map, &i.to_string().into())?;
                    if let Some(val_rc) = val_rc_opt {
                        obj_set_rc(obj_map, &(i - 1).to_string().into(), val_rc);
                    } else {
                        obj_map.borrow_mut().remove(&(i - 1).to_string().into());
                    }
                }
                obj_map.borrow_mut().remove(&(current_len - 1).to_string().into());
                set_array_length(obj_map, current_len - 1)?;
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
            if let Expr::Var(varname) = obj_expr
                && let Some(rc_val) = env_get(env, varname)
            {
                let mut borrowed = rc_val.borrow_mut();
                if let Value::Object(ref mut map) = *borrowed {
                    // Shift right by number of new elements
                    for i in (0..current_len).rev() {
                        let dest = (i + args.len()).to_string();
                        let val_rc_opt = obj_get_value(map, &i.to_string().into())?;
                        if let Some(val_rc) = val_rc_opt {
                            obj_set_rc(map, &dest.into(), val_rc);
                        } else {
                            map.borrow_mut().remove(&dest.into());
                        }
                    }
                    // Insert new elements
                    for (i, arg) in args.iter().enumerate() {
                        let val = evaluate_expr(env, arg)?;
                        obj_set_value(map, &i.to_string().into(), val)?;
                    }
                    let new_len = current_len + args.len();
                    set_array_length(map, new_len)?;
                    return Ok(Value::Number(new_len as f64));
                }
            }

            // Fallback: mutate local copy (shift right by number of new elements)
            for i in (0..current_len).rev() {
                let dest = (i + args.len()).to_string();
                let val_rc_opt = obj_get_value(obj_map, &i.to_string().into())?;
                if let Some(val_rc) = val_rc_opt {
                    obj_set_rc(obj_map, &dest.into(), val_rc);
                } else {
                    obj_map.borrow_mut().remove(&dest.into());
                }
            }
            for (i, arg) in args.iter().enumerate() {
                let val = evaluate_expr(env, arg)?;
                obj_set_value(obj_map, &i.to_string().into(), val)?;
            }
            let new_len = current_len + args.len();
            set_array_length(obj_map, new_len)?;
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
                            idx += current_len as isize;
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
                            idx += current_len as isize;
                        }
                        idx.max(0) as usize
                    }
                    _ => current_len,
                }
            } else {
                current_len
            };

            for i in start..end.min(current_len) {
                let val = Rc::new(RefCell::new(fill_value.clone()));
                obj_map.borrow_mut().insert(PropertyKey::String(i.to_string()), val);
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
                            idx += current_len as isize;
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
                if let Some(val) = obj_get_value(obj_map, &i.to_string().into())?
                    && values_equal(&val.borrow(), &search_element)
                {
                    return Ok(Value::Number(i as f64));
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
                if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
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
        "flat" => {
            let depth = if !args.is_empty() {
                match evaluate_expr(env, &args[0])? {
                    Value::Number(n) => n as usize,
                    _ => 1,
                }
            } else {
                1
            };

            let mut result = Vec::new();
            flatten_array(obj_map, &mut result, depth)?;

            let new_array = new_js_object_data();
            set_array_length(&new_array, result.len())?;
            for (i, val) in result.into_iter().enumerate() {
                obj_set_value(&new_array, &i.to_string().into(), val)?;
            }
            Ok(Value::Object(new_array))
        }
        "flatMap" => {
            if args.is_empty() {
                return Err(raise_eval_error!("Array.flatMap expects at least one argument"));
            }

            let callback_val = evaluate_expr(env, &args[0])?;
            let current_len = get_array_length(obj_map).unwrap_or(0);

            let mut result = Vec::new();
            for i in 0..current_len {
                if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
                    if let Some((params, body, captured_env)) = extract_closure_from_value(&callback_val) {
                        let func_env = new_js_object_data();
                        func_env.borrow_mut().prototype = Some(captured_env.clone());
                        if !params.is_empty() {
                            env_set(&func_env, params[0].as_str(), val.borrow().clone())?;
                        }
                        if params.len() >= 2 {
                            env_set(&func_env, params[1].as_str(), Value::Number(i as f64))?;
                        }
                        if params.len() >= 3 {
                            env_set(&func_env, params[2].as_str(), Value::Object(obj_map.clone()))?;
                        }
                        let mapped_val = evaluate_statements(&func_env, &body)?;
                        flatten_single_value(mapped_val, &mut result, 1)?;
                    } else {
                        return Err(raise_eval_error!("Array.flatMap expects a function"));
                    }
                }
            }

            let new_array = new_js_object_data();
            set_array_length(&new_array, result.len())?;
            for (i, val) in result.into_iter().enumerate() {
                obj_set_value(&new_array, &i.to_string().into(), val)?;
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
                        idx += current_len as isize;
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
                            idx += current_len as isize;
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
                            idx += current_len as isize;
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
                if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
                    temp_values.push(val.borrow().clone());
                }
            }

            for (i, val) in temp_values.into_iter().enumerate() {
                let dest_idx = target + i;
                if dest_idx < current_len {
                    obj_set_value(obj_map, &dest_idx.to_string().into(), val)?;
                }
            }

            Ok(Value::Object(obj_map.clone()))
        }
        "entries" => {
            let length = get_array_length(obj_map).unwrap_or(0);

            let result = new_js_object_data();
            set_array_length(&result, length)?;
            for i in 0..length {
                if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
                    // Create entry [i, value]
                    let entry = new_js_object_data();
                    obj_set_value(&entry, &"0".into(), Value::Number(i as f64))?;
                    obj_set_value(&entry, &"1".into(), val.borrow().clone())?;
                    set_array_length(&entry, 2)?;
                    obj_set_value(&result, &i.to_string().into(), Value::Object(entry))?;
                }
            }
            Ok(Value::Object(result))
        }
        "findLast" => {
            if !args.is_empty() {
                let callback = evaluate_expr(env, &args[0])?;
                match callback {
                    Value::Closure(params, body, captured_env) => {
                        let current_len = get_array_length(obj_map).unwrap_or(0);

                        // Search from the end
                        for i in (0..current_len).rev() {
                            if let Some(value) = obj_get_value(obj_map, &i.to_string().into())? {
                                let element = value.borrow().clone();
                                let index_val = Value::Number(i as f64);

                                // Create new environment for callback (fresh frame whose prototype is captured_env)
                                let func_env = new_js_object_data();
                                func_env.borrow_mut().prototype = Some(captured_env.clone());
                                if !params.is_empty() {
                                    env_set(&func_env, params[0].as_str(), element.clone())?;
                                }
                                if params.len() >= 2 {
                                    env_set(&func_env, params[1].as_str(), index_val)?;
                                }
                                if params.len() > 2 {
                                    env_set(&func_env, params[2].as_str(), Value::Object(obj_map.clone()))?;
                                }

                                let res = evaluate_statements(&func_env, &body)?;
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
                    _ => Err(raise_eval_error!("Array.findLast expects a function")),
                }
            } else {
                Err(raise_eval_error!("Array.findLast expects at least one argument"))
            }
        }
        "findLastIndex" => {
            if !args.is_empty() {
                let callback = evaluate_expr(env, &args[0])?;
                match callback {
                    Value::Closure(params, body, captured_env) => {
                        let current_len = get_array_length(obj_map).unwrap_or(0);

                        // Search from the end
                        for i in (0..current_len).rev() {
                            if let Some(value) = obj_get_value(obj_map, &i.to_string().into())? {
                                let element = value.borrow().clone();
                                let index_val = Value::Number(i as f64);

                                // Create new environment for callback (fresh frame whose prototype is captured_env)
                                let func_env = new_js_object_data();
                                func_env.borrow_mut().prototype = Some(captured_env.clone());
                                if !params.is_empty() {
                                    env_set(&func_env, params[0].as_str(), element.clone())?;
                                }
                                if params.len() >= 2 {
                                    env_set(&func_env, params[1].as_str(), index_val)?;
                                }
                                if params.len() > 2 {
                                    env_set(&func_env, params[2].as_str(), Value::Object(obj_map.clone()))?;
                                }

                                let res = evaluate_statements(&func_env, &body)?;
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
                    _ => Err(raise_eval_error!("Array.findLastIndex expects a function")),
                }
            } else {
                Err(raise_eval_error!("Array.findLastIndex expects at least one argument"))
            }
        }
        _ => Err(raise_eval_error!(format!("Array.{method} not found"))),
    }
}

// Helper functions for array flattening
fn flatten_array(obj_map: &JSObjectDataPtr, result: &mut Vec<Value>, depth: usize) -> Result<(), JSError> {
    let current_len = get_array_length(obj_map).unwrap_or(0);

    for i in 0..current_len {
        if let Some(val) = obj_get_value(obj_map, &i.to_string().into())? {
            let value = val.borrow().clone();
            flatten_single_value(value, result, depth)?;
        }
    }
    Ok(())
}

fn flatten_single_value(value: Value, result: &mut Vec<Value>, depth: usize) -> Result<(), JSError> {
    if depth == 0 {
        result.push(value);
        return Ok(());
    }

    match value {
        Value::Object(obj) => {
            // Check if it's an array-like object
            let is_arr = { is_array(&obj) };
            if is_arr {
                flatten_array(&obj, result, depth - 1)?;
            } else {
                result.push(Value::Object(obj));
            }
        }
        _ => {
            result.push(value);
        }
    }
    Ok(())
}

/// Check if an object looks like an array (has length and consecutive numeric indices)
pub(crate) fn is_array(obj: &JSObjectDataPtr) -> bool {
    if let Some(length_rc) = get_own_property(obj, &"length".into()) {
        if let Value::Number(len) = *length_rc.borrow() {
            let len = len as usize;
            // Check if all indices from 0 to len-1 exist
            for i in 0..len {
                if get_own_property(obj, &i.to_string().into()).is_none() {
                    return false;
                }
            }
            // Check that there are no extra numeric keys beyond len
            for key in obj.borrow().keys() {
                if let PropertyKey::String(key_str) = key
                    && let Ok(idx) = key_str.parse::<usize>()
                    && idx >= len
                {
                    return false;
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

pub(crate) fn get_array_length(obj: &JSObjectDataPtr) -> Option<usize> {
    if let Some(length_rc) = get_own_property(obj, &"length".into())
        && let Value::Number(len) = *length_rc.borrow()
        && len >= 0.0
        && len == len.floor()
    {
        return Some(len as usize);
    }
    None
}

pub(crate) fn set_array_length(obj: &JSObjectDataPtr, new_length: usize) -> Result<(), JSError> {
    obj_set_value(obj, &"length".into(), Value::Number(new_length as f64))?;
    Ok(())
}
