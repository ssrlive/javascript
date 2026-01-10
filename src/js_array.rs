#![allow(warnings)]

use crate::core::MutationContext;
use crate::{
    core::{JSObjectDataPtr, PropertyKey, env_set, js_error::EvalError, new_js_object_data},
    error::JSError,
    raise_eval_error, raise_range_error,
    unicode::{utf8_to_utf16, utf16_to_utf8},
};

use crate::core::{
    Expr, Value, evaluate_expr, evaluate_statements, get_own_property, obj_get_key_value, obj_set_key_value, obj_set_rc,
    value_to_sort_string, values_equal,
};

pub fn initialize_array<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let array_ctor = new_js_object_data(mc);
    obj_set_key_value(mc, &array_ctor, &"__is_constructor".into(), Value::Boolean(true))?;
    obj_set_key_value(mc, &array_ctor, &"__native_ctor".into(), Value::String(utf8_to_utf16("Array")))?;

    // Get Object.prototype
    let object_proto = if let Some(obj_val) = obj_get_key_value(env, &"Object".into())?
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = obj_get_key_value(obj_ctor, &"prototype".into())?
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else {
        None
    };

    let array_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        array_proto.borrow_mut(mc).prototype = Some(proto);
    }

    obj_set_key_value(mc, &array_ctor, &"prototype".into(), Value::Object(array_proto))?;
    obj_set_key_value(mc, &array_proto, &"constructor".into(), Value::Object(array_ctor))?;
    // Make constructor non-enumerable
    array_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from("constructor"));

    // Register static methods
    obj_set_key_value(mc, &array_ctor, &"isArray".into(), Value::Function("Array.isArray".to_string()))?;
    obj_set_key_value(mc, &array_ctor, &"from".into(), Value::Function("Array.from".to_string()))?;
    obj_set_key_value(mc, &array_ctor, &"of".into(), Value::Function("Array.of".to_string()))?;

    // Register instance methods
    let methods = vec![
        "at",
        "push",
        "pop",
        "length",
        "join",
        "slice",
        "splice",
        "shift",
        "unshift",
        "concat",
        "reverse",
        "sort",
        "includes",
        "indexOf",
        "lastIndexOf",
        "forEach",
        "map",
        "filter",
        "reduce",
        "reduceRight",
        "some",
        "every",
        "find",
        "findIndex",
        "findLast",
        "findLastIndex",
        "flat",
        "flatMap",
        "fill",
        "copyWithin",
        "entries",
        "keys",
        "values",
        "toString",
        "toLocaleString",
    ];

    for method in methods {
        let val = Value::Function(format!("Array.prototype.{method}"));
        obj_set_key_value(mc, &array_proto, &method.into(), val)?;

        // Methods on prototypes should be non-enumerable so for..in doesn't list them
        array_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from(method));
    }

    env_set(mc, env, "Array", Value::Object(array_ctor))?;
    Ok(())
}

/// Handle Array static method calls (Array.isArray, Array.from, Array.of)
pub(crate) fn handle_array_static_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "isArray" => {
            if args.len() != 1 {
                return Err(EvalError::Js(raise_eval_error!("Array.isArray requires exactly one argument")));
            }
            // let arg = evaluate_expr(mc, env, &args[0])?;
            let arg = args[0].clone();
            let is_array = match arg {
                Value::Object(object) => is_array(mc, &object),
                _ => false,
            };
            Ok(Value::Boolean(is_array))
        }
        "from" => {
            // Array.from(iterable, mapFn?, thisArg?)
            if args.is_empty() {
                return Err(EvalError::Js(raise_eval_error!("Array.from requires at least one argument")));
            }

            // let iterable = evaluate_expr(mc, env, &args[0])?;
            // let map_fn = if args.len() > 1 {
            //     Some(evaluate_expr(mc, env, &args[1])?)
            // } else {
            //     None
            // };

            let iterable = args[0].clone();
            let map_fn = if args.len() > 1 { Some(args[1].clone()) } else { None };

            let mut result = Vec::new();

            // Handle different types of iterables
            match iterable {
                Value::Set(set) => {
                    // Handle Set iteration
                    for val in &set.borrow().values {
                        if let Some(ref fn_val) = map_fn {
                            let call_args = vec![val.clone(), val.clone()];
                            let mapped = match fn_val {
                                Value::Closure(cl) => {
                                    crate::core::call_closure(mc, &*cl, None, &call_args, env)?
                                }
                                Value::Function(name) => {
                                    crate::js_function::handle_global_function(mc, name, &call_args, env).map_err(EvalError::Js)?
                                }
                                _ => return Err(EvalError::Js(raise_eval_error!("Array.from map function must be a function"))),
                            };
                            result.push(mapped);
                        } else {
                            result.push(val.clone());
                        }
                    }
                }
                Value::Object(object) => {
                    // let maybe_set = {
                    //     let borrow = object.borrow();
                    //     borrow.get(&PropertyKey::String("__set__".to_string()))
                    // };

                    // let maybe_map = if maybe_set.is_none() {
                    //     let borrow = object.borrow();
                    //     borrow.get(&PropertyKey::String("__map__".to_string()))
                    // } else {
                    //     None
                    // };

                    // if let Some(set_val) = maybe_set {
                    //     if let Value::Set(set) = &*set_val.borrow() {
                    //         for (i, val) in set.borrow().values.iter().enumerate() {
                    //             if let Some(ref fn_val) = map_fn {
                    //                 if let Some((params, body, captured_env)) = extract_closure_from_value(fn_val) {
                    //                     let args = vec![val.clone(), Value::Number(i as f64)];
                    //                     let func_env = prepare_closure_call_env(&captured_env, Some(&params), &args, Some(env))?;
                    //                     let mut body_clone = body.clone();
                    //                     let mapped = evaluate_statements(mc, &func_env, &mut body_clone)?;
                    //                     result.push(mapped);
                    //                 } else {
                    //                     return Err(raise_eval_error!("Array.from map function must be a function"));
                    //                 }
                    //             } else {
                    //                 result.push(val.clone());
                    //             }
                    //         }
                    //     }
                    // } else if let Some(map_val) = maybe_map {
                    //     if let Value::Map(map) = &*map_val.borrow() {
                    //         for (i, (key, val)) in map.borrow().entries.iter().enumerate() {
                    //             let entry_obj = create_array(mc, env)?;
                    //             set_array_length(mc, &entry_obj, 2)?;
                    //             obj_set_key_value(mc, &entry_obj, &"0".into(), key.clone())?;
                    //             obj_set_key_value(mc, &entry_obj, &"1".into(), val.clone())?;
                    //             let entry_val = Value::Object(entry_obj);

                    //             if let Some(ref fn_val) = map_fn {
                    //                 if let Some((params, body, captured_env)) = extract_closure_from_value(fn_val) {
                    //                     let args = vec![entry_val.clone(), Value::Number(i as f64)];
                    //                     let func_env = prepare_closure_call_env(&captured_env, Some(&params), &args, Some(env))?;
                    //                     let mut body_clone = body.clone();
                    //                     let mapped = evaluate_statements(mc, &func_env, &mut body_clone)?;
                    //                     result.push(mapped);
                    //                 } else {
                    //                     return Err(raise_eval_error!("Array.from map function must be a function"));
                    //                 }
                    //             } else {
                    //                 result.push(entry_val);
                    //             }
                    //         }
                    //     }
                    // } else if let Some(len) = get_array_length(mc, &object) {

                    if let Some(len) = get_array_length(mc, &object) {
                        for i in 0..len {
                            let val_opt = obj_get_key_value(&object, &i.to_string().into())?;
                            let element = if let Some(val) = val_opt {
                                val.borrow().clone()
                            } else {
                                Value::Undefined
                            };

                            if let Some(ref fn_val) = map_fn {
                                // if let Some((params, body, captured_env)) = extract_closure_from_value(fn_val) {
                                //     let args = vec![element, Value::Number(i as f64)];
                                //     let func_env = prepare_closure_call_env(&captured_env, Some(&params), &args, Some(env))?;
                                //     let mut body_clone = body.clone();
                                //     let mapped = evaluate_statements(mc, &func_env, &mut body_clone).map_err(|e| match e {
                                //         EvalError::Js(e) => e,
                                //         EvalError::Throw(v, ..) => JSError::new(
                                //             crate::error::JSErrorKind::RuntimeError {
                                //                 message: value_to_string(&v),
                                //             },
                                //             "array.rs".to_string(),
                                //             0,
                                //             "handle_array_static_method".to_string(),
                                //         ),
                                //     })?;
                                //     result.push(mapped);
                                // } else {
                                //     return Err(EvalError::Js(raise_eval_error!("Array.from map function must be a function")));
                                // }
                                todo!()
                            } else {
                                result.push(element);
                            }
                        }
                    } else {
                        return Err(EvalError::Js(raise_eval_error!("Array.from iterable must be array-like")));
                    }
                }
                _ => {
                    return Err(EvalError::Js(raise_eval_error!("Array.from iterable must be array-like")));
                }
            }

            let new_array = create_array(mc, env)?;
            set_array_length(mc, &new_array, result.len())?;
            for (i, val) in result.into_iter().enumerate() {
                obj_set_key_value(mc, &new_array, &i.to_string().into(), val)?;
            }
            Ok(Value::Object(new_array))
        }
        "of" => {
            // Array.of(...elements)
            let new_array = create_array(mc, env)?;
            for (i, arg) in args.iter().enumerate() {
                // let val = evaluate_expr(mc, env, arg)?;
                let val = arg.clone();
                obj_set_key_value(mc, &new_array, &i.to_string().into(), val)?;
            }
            set_array_length(mc, &new_array, args.len())?;
            Ok(Value::Object(new_array))
        }
        _ => Err(EvalError::Js(raise_eval_error!(format!("Array.{method} is not implemented")))),
    }
}

/// Handle Array constructor calls
pub(crate) fn handle_array_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.is_empty() {
        // Array() - create empty array
        let array_obj = create_array(mc, env)?;
        set_array_length(mc, &array_obj, 0)?;
        Ok(Value::Object(array_obj))
    } else if args.len() == 1 {
        // Array(length) or Array(element)
        // let arg_val = evaluate_expr(mc, env, &args[0])?;
        let arg_val = args[0].clone();
        match arg_val {
            Value::Number(n) => {
                if n.is_nan() {
                    return Err(EvalError::Js(raise_range_error!("Invalid array length")));
                }
                if n.fract() != 0.0 {
                    return Err(EvalError::Js(raise_range_error!("Invalid array length")));
                }
                if n < 0.0 {
                    return Err(EvalError::Js(raise_range_error!("Invalid array length")));
                }
                if n > u32::MAX as f64 {
                    return Err(EvalError::Js(raise_range_error!("Invalid array length")));
                }
                // Array(length) - create array with specified length
                let array_obj = create_array(mc, env)?;
                set_array_length(mc, &array_obj, n as usize)?;
                Ok(Value::Object(array_obj))
            }
            _ => {
                // Array(element) - create array with single element
                let array_obj = create_array(mc, env)?;
                obj_set_key_value(mc, &array_obj, &"0".into(), arg_val)?;
                set_array_length(mc, &array_obj, 1)?;
                Ok(Value::Object(array_obj))
            }
        }
    } else {
        // Array(element1, element2, ...) - create array with multiple elements
        let array_obj = create_array(mc, env)?;
        for (i, arg) in args.iter().enumerate() {
            // let arg_val = evaluate_expr(mc, env, arg)?;
            let arg_val = arg.clone();
            obj_set_key_value(mc, &array_obj, &i.to_string().into(), arg_val)?;
        }
        set_array_length(mc, &array_obj, args.len())?;
        Ok(Value::Object(array_obj))
    }
}

/// Handle Array instance method calls
pub(crate) fn handle_array_instance_method<'gc>(
    mc: &MutationContext<'gc>,
    object: &JSObjectDataPtr<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "at" => {
            let index = if !args.is_empty() {
                // match evaluate_expr(mc, env, &args[0])? {
                match args[0].clone() {
                    Value::Number(n) => n as i64,
                    _ => 0,
                }
            } else {
                0
            };

            let len = get_array_length(mc, object).unwrap_or(0) as i64;
            let k = if index >= 0 { index } else { len + index };

            if k < 0 || k >= len {
                Ok(Value::Undefined)
            } else {
                let val_opt = obj_get_key_value(object, &k.to_string().into())?;
                Ok(val_opt.map(|v| v.borrow().clone()).unwrap_or(Value::Undefined))
            }
        }
        "push" => {
            if !args.is_empty() {
                // Try to mutate the original object in the environment when possible
                // so that push is chainable (returns the array) and mutations persist.
                // Evaluate all args and append them.
                // First determine current length from the local object
                let mut current_len = get_array_length(mc, object).unwrap_or(0);

                // Helper closure to push a value into a map
                fn push_into_map<'gc>(
                    mc: &MutationContext<'gc>,
                    map: &JSObjectDataPtr<'gc>,
                    val: Value<'gc>,
                    current_len: &mut usize,
                ) -> Result<(), JSError> {
                    obj_set_key_value(mc, map, &current_len.to_string().into(), val)?;
                    *current_len += 1;
                    Ok(())
                }

                // Fallback: mutate the local object copy
                for arg in args {
                    let val = arg.clone();
                    push_into_map(mc, object, val, &mut current_len)?;
                }
                set_array_length(mc, object, current_len)?;
                // Return the array object (chainable)
                Ok(Value::Object(object.clone()))
            } else {
                Err(EvalError::Js(raise_eval_error!("Array.push expects at least one argument")))
            }
        }
        "pop" => {
            let current_len = get_array_length(mc, object).unwrap_or(0);
            if current_len > 0 {
                let last_idx = (current_len - 1).to_string();
                let val = object.borrow_mut(mc).properties.shift_remove(&PropertyKey::from(last_idx));
                set_array_length(mc, object, current_len - 1)?;
                Ok(val.map(|v| v.borrow().clone()).unwrap_or(Value::Undefined))
            } else {
                Ok(Value::Undefined)
            }
        }
        "length" => {
            let length = Value::Number(get_array_length(mc, object).unwrap_or(0) as f64);
            Ok(length)
        }
        "join" => {
            let separator = if !args.is_empty() {
                // match evaluate_expr(mc, env, &args[0])? {
                match args[0].clone() {
                    Value::String(s) => utf16_to_utf8(&s),
                    Value::Number(n) => n.to_string(),
                    _ => ",".to_string(),
                }
            } else {
                ",".to_string()
            };

            let current_len = get_array_length(mc, object).unwrap_or(0);

            let mut result = String::new();
            for i in 0..current_len {
                if i > 0 {
                    result.push_str(&separator);
                }
                if let Some(val) = obj_get_key_value(object, &i.to_string().into())? {
                    match &*val.borrow() {
                        Value::Undefined | Value::Null => {} // push nothing for null and undefined
                        Value::String(s) => result.push_str(&utf16_to_utf8(s)),
                        Value::Number(n) => result.push_str(&n.to_string()),
                        Value::Boolean(b) => result.push_str(&b.to_string()),
                        Value::BigInt(b) => result.push_str(&format!("{}n", b)),
                        _ => result.push_str("[object Object]"),
                    }
                }
            }
            Ok(Value::String(utf8_to_utf16(&result)))
        }
        "slice" => {
            let start = if !args.is_empty() {
                // match evaluate_expr(mc, env, &args[0])? {
                match args[0].clone() {
                    Value::Number(n) => n as isize,
                    _ => 0isize,
                }
            } else {
                0isize
            };

            let current_len = get_array_length(mc, object).unwrap_or(0);

            let end = if args.len() >= 2 {
                // match evaluate_expr(mc, env, &args[1])? {
                match args[1].clone() {
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

            let new_array = create_array(mc, env)?;
            let mut idx = 0;
            for i in start..end {
                if let Some(val) = obj_get_key_value(object, &i.to_string().into())? {
                    obj_set_key_value(mc, &new_array, &idx.to_string().into(), val.borrow().clone())?;
                    idx += 1;
                }
            }
            set_array_length(mc, &new_array, idx)?;
            Ok(Value::Object(new_array))
        }
        "forEach" => {
            if !args.is_empty() {
                // Evaluate the callback expression
                // let callback_val = evaluate_expr(mc, env, &args[0])?;
                let callback_val = args[0].clone();
                let current_len = get_array_length(mc, object).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(val_rc) = obj_get_key_value(object, &i.to_string().into())? {
                        let val = val_rc.borrow().clone();
                        let call_args = vec![val, Value::Number(i as f64), Value::Object(object.clone())];
                        
                        let actual_func = if let Value::Object(obj) = &callback_val {
                            if let Ok(Some(prop)) = obj_get_key_value(obj, &"__closure__".into()) {
                                prop.borrow().clone()
                            } else {
                                callback_val.clone()
                            }
                        } else {
                            callback_val.clone()
                        };

                        match &actual_func {
                            Value::Closure(cl) => {
                                crate::core::call_closure(mc, &*cl, None, &call_args, env)?;
                            }
                            Value::Function(name) => {
                                crate::js_function::handle_global_function(mc, name, &call_args, env).map_err(EvalError::Js)?;
                            }
                            _ => return Err(EvalError::Js(raise_eval_error!("Array.forEach callback must be a function"))),
                        }
                    }
                }
                Ok(Value::Undefined)
            } else {
                Err(EvalError::Js(raise_eval_error!("Array.forEach expects at least one argument")))
            }
        }
        "map" => {
            if !args.is_empty() {
                // let callback_val = evaluate_expr(mc, env, &args[0])?;
                let callback_val = args[0].clone();
                let current_len = get_array_length(mc, object).unwrap_or(0);

                let new_array = create_array(mc, env)?;
                set_array_length(mc, &new_array, current_len)?;

                for i in 0..current_len {
                    if let Some(val_rc) = obj_get_key_value(object, &i.to_string().into())? {
                        let val = val_rc.borrow().clone();
                        let call_args = vec![val, Value::Number(i as f64), Value::Object(object.clone())];
                        let res = match &callback_val {
                            Value::Closure(cl) => {
                                crate::core::call_closure(mc, &*cl, None, &call_args, env)?
                            }
                            Value::Function(name) => {
                                crate::js_function::handle_global_function(mc, name, &call_args, env).map_err(EvalError::Js)?
                            }
                            _ => return Err(EvalError::Js(raise_eval_error!("Array.map callback must be a function"))),
                        };
                        obj_set_key_value(mc, &new_array, &i.to_string().into(), res)?;
                    }
                }
                Ok(Value::Object(new_array))
            } else {
                Err(EvalError::Js(raise_eval_error!("Array.map expects at least one argument")))
            }
        }
        "filter" => {
            if !args.is_empty() {
                // let callback_val = evaluate_expr(mc, env, &args[0])?;
                let callback_val = args[0].clone();
                let current_len = get_array_length(mc, object).unwrap_or(0);

                let new_array = create_array(mc, env)?;
                let mut idx = 0;
                for i in 0..current_len {
                    if let Some(val) = obj_get_key_value(object, &i.to_string().into())? {
                        // if let Some((params, body, captured_env)) = extract_closure_from_value(&callback_val) {
                        //     let args = vec![val.borrow().clone(), Value::Number(i as f64), Value::Object(object.clone())];
                        //     let func_env = prepare_closure_call_env(&captured_env, Some(&params), &args, Some(env))?;

                        //     let res = evaluate_statements(mc, &func_env, &mut body.clone())?;
                        //     // truthy check
                        //     let include = match res {
                        //         Value::Boolean(b) => b,
                        //         Value::Number(n) => n != 0.0,
                        //         Value::String(ref s) => !s.is_empty(),
                        //         Value::Object(_) => true,
                        //         Value::Undefined => false,
                        //         _ => false,
                        //     };
                        //     if include {
                        //         obj_set_key_value(mc, &new_array, &idx.to_string().into(), val.borrow().clone())?;
                        //         idx += 1;
                        //     }
                        // } else {
                        //     return Err(EvalError::Js(raise_eval_error!("Array.filter expects a function")));
                        // }
                        todo!()
                    }
                }
                set_array_length(mc, &new_array, idx)?;
                Ok(Value::Object(new_array))
            } else {
                Err(EvalError::Js(raise_eval_error!("Array.filter expects at least one argument")))
            }
        }
        "reduce" => {
            if !args.is_empty() {
                // let callback_val = evaluate_expr(mc, env, &args[0])?;
                // let initial_value = if args.len() >= 2 {
                //     Some(evaluate_expr(mc, env, &args[1])?)
                // } else {
                //     None
                // };

                let callback_val = args[0].clone();
                let initial_value = if args.len() >= 2 { Some(args[1].clone()) } else { None };

                let current_len = get_array_length(mc, object).unwrap_or(0);

                if current_len == 0 && initial_value.is_none() {
                    return Err(EvalError::Js(raise_eval_error!(
                        "Array.reduce called on empty array with no initial value"
                    )));
                }

                let mut accumulator: Value = if let Some(ref val) = initial_value {
                    val.clone()
                } else if let Some(val) = obj_get_key_value(object, &"0".into())? {
                    val.borrow().clone()
                } else {
                    Value::Undefined
                };

                let start_idx = if initial_value.is_some() { 0 } else { 1 };
                for i in start_idx..current_len {
                    if let Some(val) = obj_get_key_value(object, &i.to_string().into())? {
                        // if let Some((params, body, captured_env)) = extract_closure_from_value(&callback_val) {
                        //     // build args for callback: first acc, then current element
                        //     let args = vec![
                        //         accumulator.clone(),
                        //         val.borrow().clone(),
                        //         Value::Number(i as f64),
                        //         Value::Object(object.clone()),
                        //     ];
                        //     let func_env = prepare_closure_call_env(&captured_env, Some(&params), &args, Some(env))?;
                        //     let res = evaluate_statements(mc, &func_env, &mut body.clone())?;
                        //     accumulator = res;
                        // } else {
                        //     return Err(EvalError::Js(raise_eval_error!("Array.reduce expects a function")));
                        // }
                        todo!()
                    }
                }
                Ok(accumulator)
            } else {
                Err(EvalError::Js(raise_eval_error!("Array.reduce expects at least one argument")))
            }
        }
        "reduceRight" => {
            if !args.is_empty() {
                // let callback_val = evaluate_expr(mc, env, &args[0])?;
                // let initial_value = if args.len() >= 2 {
                //     Some(evaluate_expr(mc, env, &args[1])?)
                // } else {
                //     None
                // };

                let callback_val = args[0].clone();
                let initial_value = if args.len() >= 2 { Some(args[1].clone()) } else { None };

                let current_len = get_array_length(mc, object).unwrap_or(0);

                if current_len == 0 && initial_value.is_none() {
                    return Err(EvalError::Js(raise_eval_error!(
                        "Array.reduceRight called on empty array with no initial value"
                    )));
                }

                let mut accumulator: Value;
                let mut start_idx_rev = 0; // How many items to skip from the end

                if let Some(ref val) = initial_value {
                    accumulator = val.clone();
                    start_idx_rev = 0;
                } else {
                    // Find the last present element
                    let mut found = false;
                    accumulator = Value::Undefined; // Placeholder
                    for i in (0..current_len).rev() {
                        if let Some(val) = obj_get_key_value(object, &i.to_string().into())? {
                            accumulator = val.borrow().clone();
                            start_idx_rev = current_len - i;
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return Err(EvalError::Js(raise_eval_error!(
                            "Array.reduceRight called on empty array with no initial value"
                        )));
                    }
                }

                // Iterate backwards
                // If we found an initial value at index `last_idx`, we start loop from `last_idx - 1`.
                // `start_idx_rev` is `current_len - last_idx`.
                // So we want to iterate from `current_len - 1 - start_idx_rev` down to 0?
                // No, if initial value was provided, start_idx_rev is 0. We iterate from `current_len - 1` down to 0.
                // If initial value was from array at `last_idx`, we want to start from `last_idx - 1`.
                // `last_idx` was `current_len - start_idx_rev`.
                // So we start from `current_len - start_idx_rev - 1`.

                let start_loop = current_len.saturating_sub(start_idx_rev);

                for i in (0..start_loop).rev() {
                    if let Some(val) = obj_get_key_value(object, &i.to_string().into())? {
                        // if let Some((params, body, captured_env)) = extract_closure_from_value(&callback_val) {
                        //     // build args for callback: first acc, then current element
                        //     let args = vec![
                        //         accumulator.clone(),
                        //         val.borrow().clone(),
                        //         Value::Number(i as f64),
                        //         Value::Object(object.clone()),
                        //     ];
                        //     let func_env = prepare_closure_call_env(&captured_env, Some(&params), &args, Some(env))?;
                        //     let res = evaluate_statements(mc, &func_env, &mut body.clone())?;
                        //     accumulator = res;
                        // } else {
                        //     return Err(EvalError::Js(raise_eval_error!("Array.reduceRight expects a function")));
                        // }
                        todo!()
                    }
                }
                Ok(accumulator)
            } else {
                Err(EvalError::Js(raise_eval_error!("Array.reduceRight expects at least one argument")))
            }
        }
        "find" => {
            if !args.is_empty() {
                let callback = args[0].clone();
                let current_len = get_array_length(mc, object).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(value) = obj_get_key_value(object, &i.to_string().into())? {
                        // if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                        //     let element = value.borrow().clone();
                        //     let index_val = Value::Number(i as f64);

                        //     // Create new environment for callback
                        //     let args = vec![element.clone(), index_val, Value::Object(object.clone())];
                        //     let func_env = prepare_closure_call_env(&captured_env, Some(&params), &args, Some(env))?;

                        //     let res = evaluate_statements(mc, &func_env, &mut body.clone())?;
                        //     // truthy check
                        //     let is_truthy = match res {
                        //         Value::Boolean(b) => b,
                        //         Value::Number(n) => n != 0.0,
                        //         Value::String(ref s) => !s.is_empty(),
                        //         Value::Object(_) => true,
                        //         Value::Undefined => false,
                        //         _ => false,
                        //     };
                        //     if is_truthy {
                        //         return Ok(element);
                        //     }
                        // } else {
                        //     return Err(EvalError::Js(raise_eval_error!("Array.find expects a function")));
                        // }
                        todo!()
                    }
                }
                Ok(Value::Undefined)
            } else {
                Err(EvalError::Js(raise_eval_error!("Array.find expects at least one argument")))
            }
        }
        "findIndex" => {
            if !args.is_empty() {
                let callback = args[0].clone();
                let current_len = get_array_length(mc, object).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(value) = obj_get_key_value(object, &i.to_string().into())? {
                        // if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                        //     let element = value.borrow().clone();
                        //     let index_val = Value::Number(i as f64);

                        //     let args = vec![element.clone(), index_val, Value::Object(object.clone())];
                        //     let func_env = prepare_closure_call_env(&captured_env, Some(&params), &args, Some(env))?;

                        //     let res = evaluate_statements(mc, &func_env, &mut body.clone())?;
                        //     // truthy check
                        //     let is_truthy = match res {
                        //         Value::Boolean(b) => b,
                        //         Value::Number(n) => n != 0.0,
                        //         Value::String(ref s) => !s.is_empty(),
                        //         Value::Object(_) => true,
                        //         Value::Undefined => false,
                        //         _ => false,
                        //     };
                        //     if is_truthy {
                        //         return Ok(Value::Number(i as f64));
                        //     }
                        // } else {
                        //     return Err(EvalError::Js(raise_eval_error!("Array.findIndex expects a function")));
                        // }
                        todo!()
                    }
                }
                Ok(Value::Number(-1.0))
            } else {
                Err(EvalError::Js(raise_eval_error!("Array.findIndex expects at least one argument")))
            }
        }
        "some" => {
            if !args.is_empty() {
                let callback = args[0].clone();
                let current_len = get_array_length(mc, object).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(value) = obj_get_key_value(object, &i.to_string().into())? {
                        // if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                        //     let element = value.borrow().clone();
                        //     let index_val = Value::Number(i as f64);

                        //     let args = vec![element.clone(), index_val, Value::Object(object.clone())];
                        //     let func_env = prepare_closure_call_env(&captured_env, Some(&params), &args, Some(env))?;

                        //     let res = evaluate_statements(mc, &func_env, &mut body.clone())?;
                        //     // truthy check
                        //     let is_truthy = match res {
                        //         Value::Boolean(b) => b,
                        //         Value::Number(n) => n != 0.0,
                        //         Value::String(ref s) => !s.is_empty(),
                        //         Value::Object(_) => true,
                        //         Value::Undefined => false,
                        //         _ => false,
                        //     };
                        //     if is_truthy {
                        //         return Ok(Value::Boolean(true));
                        //     }
                        // } else {
                        //     return Err(EvalError::Js(raise_eval_error!("Array.some expects a function")));
                        // }
                        todo!()
                    }
                }
                Ok(Value::Boolean(false))
            } else {
                Err(EvalError::Js(raise_eval_error!("Array.some expects at least one argument")))
            }
        }
        "every" => {
            if !args.is_empty() {
                let callback = args[0].clone();
                let current_len = get_array_length(mc, object).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(value) = obj_get_key_value(object, &i.to_string().into())? {
                        // if let Some((params, body, captured_env)) = extract_closure_from_value(&callback) {
                        //     let element = value.borrow().clone();
                        //     let index_val = Value::Number(i as f64);

                        //     let args = vec![element.clone(), index_val, Value::Object(object.clone())];
                        //     let func_env = prepare_closure_call_env(&captured_env, Some(&params), &args, Some(env))?;

                        //     let res = evaluate_statements(mc, &func_env, &mut body.clone())?;
                        //     // truthy check
                        //     let is_truthy = match res {
                        //         Value::Boolean(b) => b,
                        //         Value::Number(n) => n != 0.0,
                        //         Value::String(ref s) => !s.is_empty(),
                        //         Value::Object(_) => true,
                        //         Value::Undefined => false,
                        //         _ => false,
                        //     };
                        //     if !is_truthy {
                        //         return Ok(Value::Boolean(false));
                        //     }
                        // } else {
                        //     return Err(EvalError::Js(raise_eval_error!("Array.every expects a function")));
                        // }
                        todo!()
                    }
                }
                Ok(Value::Boolean(true))
            } else {
                Err(EvalError::Js(raise_eval_error!("Array.every expects at least one argument")))
            }
        }
        "concat" => {
            let result = create_array(mc, env)?;

            // First, copy all elements from current array
            let current_len = get_array_length(mc, object).unwrap_or(0);

            let mut new_index = 0;
            for i in 0..current_len {
                if let Some(val) = obj_get_key_value(object, &i.to_string().into())? {
                    obj_set_key_value(mc, &result, &new_index.to_string().into(), val.borrow().clone())?;
                    new_index += 1;
                }
            }

            // Then, append all arguments
            for arg in args {
                let arg_val = arg.clone();
                match arg_val {
                    Value::Object(arg_obj) => {
                        // If argument is an array-like object, copy its elements
                        let arg_len = get_array_length(mc, &arg_obj).unwrap_or(0);
                        for i in 0..arg_len {
                            if let Some(val) = obj_get_key_value(&arg_obj, &i.to_string().into())? {
                                obj_set_rc(mc, &result, &new_index.to_string().into(), val.clone())?;
                                new_index += 1;
                            }
                        }
                    }
                    _ => {
                        // If argument is not an array, append it directly
                        obj_set_key_value(mc, &result, &new_index.to_string().into(), arg_val)?;
                        new_index += 1;
                    }
                }
            }

            set_array_length(mc, &result, new_index)?;
            Ok(Value::Object(result))
        }
        "indexOf" => {
            if args.is_empty() {
                return Err(EvalError::Js(raise_eval_error!("Array.indexOf expects at least one argument")));
            }

            let search_element = args[0].clone();
            let from_index = if args.len() > 1 {
                match args[1].clone() {
                    Value::Number(n) => n as isize,
                    _ => 0isize,
                }
            } else {
                0isize
            };

            let current_len = get_array_length(mc, object).unwrap_or(0);

            let start = if from_index < 0 {
                (current_len as isize + from_index).max(0) as usize
            } else {
                from_index as usize
            };

            for i in start..current_len {
                if let Some(val) = obj_get_key_value(object, &i.to_string().into())?
                    && values_equal(mc, &val.borrow(), &search_element)
                {
                    return Ok(Value::Number(i as f64));
                }
            }

            Ok(Value::Number(-1.0))
        }
        "includes" => {
            if args.is_empty() {
                return Err(EvalError::Js(raise_eval_error!("Array.includes expects at least one argument")));
            }

            let search_element = args[0].clone();
            let from_index = if args.len() > 1 {
                match args[1].clone() {
                    Value::Number(n) => n as isize,
                    _ => 0isize,
                }
            } else {
                0isize
            };

            let current_len = get_array_length(mc, object).unwrap_or(0);

            let start = if from_index < 0 {
                (current_len as isize + from_index).max(0) as usize
            } else {
                from_index as usize
            };

            for i in start..current_len {
                if let Some(val) = obj_get_key_value(object, &i.to_string().into())?
                    && values_equal(mc, &val.borrow(), &search_element)
                {
                    return Ok(Value::Boolean(true));
                }
            }

            Ok(Value::Boolean(false))
        }
        "sort" => {
            let current_len = get_array_length(mc, object).unwrap_or(0);

            // Extract array elements for sorting
            // Note: This implementation uses O(n) extra space for simplicity.
            // For better memory efficiency with large arrays, an in-place sort
            // could be implemented, but it would be more complex with the current
            // object storage model.
            let mut elements: Vec<(String, Value<'gc>)> = Vec::new();
            for i in 0..current_len {
                if let Some(val) = obj_get_key_value(object, &i.to_string().into())? {
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
                let compare_fn = args[0].clone();
                // if let Some((params, body, captured_env)) = extract_closure_from_value(&compare_fn) {
                //     elements.sort_by(|a, b| {
                //         // Create function environment for comparison (fresh frame whose prototype is captured_env)
                //         let args = vec![a.1.clone(), b.1.clone()];
                //         let func_env = match prepare_closure_call_env(mc, &captured_env, Some(&params), &args, Some(env)) {
                //             Ok(e) => e,
                //             Err(_) => return std::cmp::Ordering::Equal,
                //         };

                //         match evaluate_statements(mc, &func_env, &body) {
                //             Ok(Value::Number(n)) => {
                //                 if n < 0.0 {
                //                     std::cmp::Ordering::Less
                //                 } else if n > 0.0 {
                //                     std::cmp::Ordering::Greater
                //                 } else {
                //                     std::cmp::Ordering::Equal
                //                 }
                //             }
                //             _ => std::cmp::Ordering::Equal,
                //         }
                //     });
                // } else {
                //     return Err(EvalError::Js(raise_eval_error!(
                //         "Array.sort expects a function as compare function"
                //     )));
                // }
                todo!()
            }

            // Update the array with sorted elements
            for (new_index, (_old_key, value)) in elements.into_iter().enumerate() {
                obj_set_key_value(mc, object, &new_index.to_string().into(), value)?;
            }

            Ok(Value::Object(object.clone()))
        }
        "reverse" => {
            let current_len = get_array_length(mc, object).unwrap_or(0);

            // Reverse elements in place
            let mut left = 0;
            let mut right = current_len.saturating_sub(1);

            while left < right {
                let left_key = left.to_string();
                let right_key = right.to_string();

                let left_val = obj_get_key_value(object, &left_key.clone().into())?.map(|v| v.borrow().clone());
                let right_val = obj_get_key_value(object, &right_key.clone().into())?.map(|v| v.borrow().clone());

                if let Some(val) = right_val {
                    obj_set_key_value(mc, object, &left_key.clone().into(), val)?;
                } else {
                    object.borrow_mut(mc).properties.shift_remove(&PropertyKey::from(left_key.clone()));
                }

                if let Some(val) = left_val {
                    obj_set_key_value(mc, object, &right_key.clone().into(), val)?;
                } else {
                    object.borrow_mut(mc).properties.shift_remove(&PropertyKey::from(right_key.clone()));
                }

                left += 1;
                right -= 1;
            }

            Ok(Value::Object(object.clone()))
        }
        "splice" => {
            // array.splice(start, deleteCount, ...items)
            let current_len = get_array_length(mc, object).unwrap_or(0);

            let start = if !args.is_empty() {
                match args[0].clone() {
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
                match args[1].clone() {
                    Value::Number(n) => n as usize,
                    _ => 0,
                }
            } else {
                current_len
            };

            // Collect elements to be deleted
            let mut deleted_elements = Vec::new();
            for i in start..(start + delete_count).min(current_len) {
                if let Some(val) = obj_get_key_value(object, &i.to_string().into())? {
                    deleted_elements.push(val.borrow().clone());
                }
            }

            // Create new array for deleted elements
            let deleted_array = create_array(mc, env)?;
            for (i, val) in deleted_elements.iter().enumerate() {
                obj_set_key_value(mc, &deleted_array, &i.to_string().into(), val.clone())?;
            }
            set_array_length(mc, &deleted_array, deleted_elements.len())?;

            // Collect tail elements (elements that need to be shifted)
            // We must collect them before we start writing new items to avoid overwriting them
            let mut tail_elements = Vec::new();
            let shift_start = start + delete_count;
            for i in shift_start..current_len {
                let val_opt = obj_get_key_value(object, &i.to_string().into())?;
                tail_elements.push(val_opt.map(|v| v.borrow().clone()));
            }

            // Insert new items at start position
            let mut write_idx = start;
            for item in args.iter().skip(2) {
                let item_val = item.clone();
                obj_set_key_value(mc, object, &write_idx.to_string().into(), item_val)?;
                write_idx += 1;
            }

            // Write tail elements back
            for val_opt in tail_elements {
                if let Some(val) = val_opt {
                    obj_set_key_value(mc, object, &write_idx.to_string().into(), val)?;
                } else {
                    // If the element was a hole (or missing), ensure the destination is also a hole
                    object
                        .borrow_mut(mc)
                        .properties
                        .shift_remove(&PropertyKey::from(write_idx.to_string()));
                }
                write_idx += 1;
            }

            // If the array shrank, remove the remaining properties at the end
            for i in write_idx..current_len {
                object.borrow_mut(mc).properties.shift_remove(&PropertyKey::from(i.to_string()));
            }

            // Update length
            set_array_length(mc, object, write_idx)?;

            Ok(Value::Object(deleted_array))
        }
        "shift" => {
            let current_len = get_array_length(mc, object).unwrap_or(0);

            if current_len > 0 {
                // Get the first element
                // Fallback: mutate the local object copy
                let first_element = obj_get_key_value(object, &"0".into())?.map(|v| v.borrow().clone());
                for i in 1..current_len {
                    let val_rc_opt = obj_get_key_value(object, &i.to_string().into())?;
                    if let Some(val_rc) = val_rc_opt {
                        obj_set_rc(mc, object, &(i - 1).to_string().into(), val_rc);
                    } else {
                        object
                            .borrow_mut(mc)
                            .properties
                            .shift_remove(&PropertyKey::from((i - 1).to_string()));
                    }
                }
                object
                    .borrow_mut(mc)
                    .properties
                    .shift_remove(&PropertyKey::from((current_len - 1).to_string()));
                set_array_length(mc, object, current_len - 1)?;
                Ok(first_element.unwrap_or(Value::Undefined))
            } else {
                Ok(Value::Undefined)
            }
        }
        "unshift" => {
            let current_len = get_array_length(mc, object).unwrap_or(0);
            if args.is_empty() {
                return Ok(Value::Number(current_len as f64));
            }

            // Fallback: mutate local copy (shift right by number of new elements)
            for i in (0..current_len).rev() {
                let dest = (i + args.len()).to_string();
                let val_rc_opt = obj_get_key_value(object, &i.to_string().into())?;
                if let Some(val_rc) = val_rc_opt {
                    obj_set_rc(mc, object, &dest.into(), val_rc);
                } else {
                    object.borrow_mut(mc).properties.shift_remove(&PropertyKey::from(dest));
                }
            }
            for (i, arg) in args.iter().enumerate() {
                let val = arg.clone();
                obj_set_key_value(mc, object, &i.to_string().into(), val)?;
            }
            let new_len = current_len + args.len();
            set_array_length(mc, object, new_len)?;
            Ok(Value::Number(new_len as f64))
        }
        "fill" => {
            if args.is_empty() {
                return Ok(Value::Object(object.clone()));
            }

            let fill_value = args[0].clone();

            let current_len = get_array_length(mc, object).unwrap_or(0);

            let start = if args.len() >= 2 {
                match args[1].clone() {
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
                match args[2].clone() {
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
                obj_set_key_value(mc, object, &i.to_string().into(), fill_value.clone())?;
            }

            Ok(Value::Object(object.clone()))
        }
        "lastIndexOf" => {
            if args.is_empty() {
                return Ok(Value::Number(-1.0));
            }

            let search_element = args[0].clone();

            let current_len = get_array_length(mc, object).unwrap_or(0);

            let from_index = if args.len() >= 2 {
                match args[1].clone() {
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
                if let Some(val) = obj_get_key_value(object, &i.to_string().into())?
                    && values_equal(mc, &val.borrow(), &search_element)
                {
                    return Ok(Value::Number(i as f64));
                }
            }

            Ok(Value::Number(-1.0))
        }
        "toString" => {
            // Array.prototype.toString() is equivalent to join(",")
            let current_len = get_array_length(mc, object).unwrap_or(0);

            let mut result = String::new();
            for i in 0..current_len {
                if i > 0 {
                    result.push(',');
                }
                if let Some(val) = obj_get_key_value(object, &i.to_string().into())? {
                    match &*val.borrow() {
                        Value::Undefined | Value::Null => {} // push nothing for null and undefined
                        Value::String(s) => result.push_str(&utf16_to_utf8(s)),
                        Value::Number(n) => result.push_str(&n.to_string()),
                        Value::Boolean(b) => result.push_str(&b.to_string()),
                        Value::BigInt(b) => result.push_str(&format!("{}n", b)),
                        _ => result.push_str("[object Object]"),
                    }
                }
            }
            Ok(Value::String(utf8_to_utf16(&result)))
        }
        "flat" => {
            let depth = if !args.is_empty() {
                match args[0].clone() {
                    Value::Number(n) => n as usize,
                    _ => 1,
                }
            } else {
                1
            };

            let mut result = Vec::new();
            flatten_array(mc, object, &mut result, depth)?;

            let new_array = create_array(mc, env)?;
            set_array_length(mc, &new_array, result.len())?;
            for (i, val) in result.into_iter().enumerate() {
                obj_set_key_value(mc, &new_array, &i.to_string().into(), val)?;
            }
            Ok(Value::Object(new_array))
        }
        "flatMap" => {
            if args.is_empty() {
                return Err(EvalError::Js(raise_eval_error!("Array.flatMap expects at least one argument")));
            }

            let callback_val = args[0].clone();
            let current_len = get_array_length(mc, object).unwrap_or(0);

            let mut result = Vec::new();
            for i in 0..current_len {
                if let Some(val) = obj_get_key_value(object, &i.to_string().into())? {
                    // if let Some((params, body, captured_env)) = extract_closure_from_value(&callback_val) {
                    //     let args = vec![val.borrow().clone(), Value::Number(i as f64), Value::Object(object.clone())];
                    //     let func_env = prepare_closure_call_env(mc, &captured_env, Some(&params), &args, Some(env))?;
                    //     let mapped_val = evaluate_statements(mc, &func_env, &mut body.clone())?;
                    //     flatten_single_value(mc, mapped_val, &mut result, 1)?;
                    // } else {
                    //     return Err(EvalError::Js(raise_eval_error!("Array.flatMap expects a function")));
                    // }
                    todo!()
                }
            }

            let new_array = create_array(mc, env)?;
            set_array_length(mc, &new_array, result.len())?;
            for (i, val) in result.into_iter().enumerate() {
                obj_set_key_value(mc, &new_array, &i.to_string().into(), val)?;
            }
            Ok(Value::Object(new_array))
        }
        "copyWithin" => {
            let current_len = get_array_length(mc, object).unwrap_or(0);

            if args.is_empty() {
                return Ok(Value::Object(object.clone()));
            }

            let target = match args[0].clone() {
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
                match args[1].clone() {
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
                match args[2].clone() {
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
                return Ok(Value::Object(object.clone()));
            }

            let mut temp_values = Vec::new();
            for i in start..end.min(current_len) {
                if let Some(val) = obj_get_key_value(object, &i.to_string().into())? {
                    temp_values.push(val.borrow().clone());
                }
            }

            for (i, val) in temp_values.into_iter().enumerate() {
                let dest_idx = target + i;
                if dest_idx < current_len {
                    obj_set_key_value(mc, object, &dest_idx.to_string().into(), val)?;
                }
            }

            Ok(Value::Object(object.clone()))
        }
        "entries" => {
            let length = get_array_length(mc, object).unwrap_or(0);

            let result = create_array(mc, env)?;
            set_array_length(mc, &result, length)?;
            for i in 0..length {
                if let Some(val) = obj_get_key_value(object, &i.to_string().into())? {
                    // Create entry [i, value]
                    let entry = create_array(mc, env)?;
                    obj_set_key_value(mc, &entry, &"0".into(), Value::Number(i as f64))?;
                    obj_set_key_value(mc, &entry, &"1".into(), val.borrow().clone())?;
                    set_array_length(mc, &entry, 2)?;
                    obj_set_key_value(mc, &result, &i.to_string().into(), Value::Object(entry))?;
                }
            }
            Ok(Value::Object(result))
        }
        "findLast" => {
            if !args.is_empty() {
                let callback = args[0].clone();
                match callback {
                    Value::Closure(data) => {
                        let params = &data.params;
                        let body = &data.body;
                        let captured_env = &data.env;
                        let current_len = get_array_length(mc, object).unwrap_or(0);

                        // Search from the end
                        for i in (0..current_len).rev() {
                            if let Some(value) = obj_get_key_value(object, &i.to_string().into())? {
                                let element = value.borrow().clone();
                                let index_val = Value::Number(i as f64);

                                let args = vec![element.clone(), index_val, Value::Object(object.clone())];
                                // let func_env = prepare_closure_call_env(mc, captured_env, Some(params), &args, Some(env))?;

                                // let res = evaluate_statements(mc, &func_env, &mut body.clone())?;
                                // // truthy check
                                // let is_truthy = match res {
                                //     Value::Boolean(b) => b,
                                //     Value::Number(n) => n != 0.0,
                                //     Value::String(ref s) => !s.is_empty(),
                                //     Value::Object(_) => true,
                                //     Value::Undefined => false,
                                //     _ => false,
                                // };
                                // if is_truthy {
                                //     return Ok(element);
                                // }
                                todo!()
                            }
                        }
                        Ok(Value::Undefined)
                    }
                    _ => Err(EvalError::Js(raise_eval_error!("Array.findLast expects a function"))),
                }
            } else {
                Err(EvalError::Js(raise_eval_error!("Array.findLast expects at least one argument")))
            }
        }
        "findLastIndex" => {
            if !args.is_empty() {
                let callback = args[0].clone();
                match callback {
                    Value::Closure(data) => {
                        let params = &data.params;
                        let body = &data.body;
                        let captured_env = &data.env;
                        let current_len = get_array_length(mc, object).unwrap_or(0);

                        // Search from the end
                        for i in (0..current_len).rev() {
                            if let Some(value) = obj_get_key_value(object, &i.to_string().into())? {
                                let element = value.borrow().clone();
                                let index_val = Value::Number(i as f64);

                                let args = vec![element.clone(), index_val, Value::Object(object.clone())];
                                // let func_env = prepare_closure_call_env(mc, captured_env, Some(params), &args, Some(env))?;

                                // let res = evaluate_statements(mc, &func_env, &mut body.clone())?;
                                // // truthy check
                                // let is_truthy = match res {
                                //     Value::Boolean(b) => b,
                                //     Value::Number(n) => n != 0.0,
                                //     Value::String(ref s) => !s.is_empty(),
                                //     Value::Object(_) => true,
                                //     Value::Undefined => false,
                                //     _ => false,
                                // };
                                // if is_truthy {
                                //     return Ok(Value::Number(i as f64));
                                // }
                                todo!()
                            }
                        }
                        Ok(Value::Number(-1.0))
                    }
                    _ => Err(EvalError::Js(raise_eval_error!("Array.findLastIndex expects a function"))),
                }
            } else {
                Err(EvalError::Js(raise_eval_error!(
                    "Array.findLastIndex expects at least one argument"
                )))
            }
        }
        _ => Err(EvalError::Js(raise_eval_error!(format!("Array.{method} not found")))),
    }
}

// Helper functions for array flattening
fn flatten_array<'gc>(
    mc: &MutationContext<'gc>,
    object: &JSObjectDataPtr<'gc>,
    result: &mut Vec<Value<'gc>>,
    depth: usize,
) -> Result<(), JSError> {
    let current_len = get_array_length(mc, object).unwrap_or(0);

    for i in 0..current_len {
        if let Some(val) = obj_get_key_value(object, &i.to_string().into())? {
            let value = val.borrow().clone();
            flatten_single_value(mc, value, result, depth)?;
        }
    }
    Ok(())
}

fn flatten_single_value<'gc>(
    mc: &MutationContext<'gc>,
    value: Value<'gc>,
    result: &mut Vec<Value<'gc>>,
    depth: usize,
) -> Result<(), JSError> {
    if depth == 0 {
        result.push(value);
        return Ok(());
    }

    match value {
        Value::Object(obj) => {
            // Check if it's an array-like object
            let is_arr = { is_array(mc, &obj) };
            if is_arr {
                flatten_array(mc, &obj, result, depth - 1)?;
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

/// Check if an object is an Array
pub(crate) fn is_array<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>) -> bool {
    if let Some(val) = get_own_property(obj, &"__is_array".into())
        && let Value::Boolean(b) = *val.borrow()
    {
        return b;
    }
    false
}

pub(crate) fn get_array_length<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>) -> Option<usize> {
    if let Some(length_rc) = get_own_property(obj, &"length".into())
        && let Value::Number(len) = *length_rc.borrow()
        && len >= 0.0
        && len == len.floor()
    {
        return Some(len as usize);
    }
    None
}

pub(crate) fn set_array_length<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>, new_length: usize) -> Result<(), JSError> {
    obj_set_key_value(mc, obj, &"length".into(), Value::Number(new_length as f64))?;
    obj.borrow_mut(mc).non_enumerable.insert("length".into());
    Ok(())
}

pub(crate) fn create_array<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let arr = new_js_object_data(mc);
    set_array_length(mc, &arr, 0)?;

    obj_set_key_value(mc, &arr, &"__is_array".into(), Value::Boolean(true))?;
    arr.borrow_mut(mc).non_enumerable.insert("__is_array".into());

    // Set prototype
    let mut root_env_opt = Some(env.clone());
    while let Some(r) = root_env_opt.clone() {
        let proto_opt = r.borrow().prototype.clone();
        if let Some(proto_rc) = proto_opt {
            root_env_opt = Some(proto_rc);
        } else {
            break;
        }
    }
    if let Some(root_env) = root_env_opt {
        // Try to set prototype to Array.prototype
        if crate::core::set_internal_prototype_from_constructor(mc, &arr, &root_env, "Array").is_err() {
            // Fallback to Object.prototype
            let _ = crate::core::set_internal_prototype_from_constructor(mc, &arr, &root_env, "Object");
        }
    }

    Ok(arr)
}
