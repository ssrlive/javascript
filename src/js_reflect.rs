use crate::core::{
    Expr, JSObjectDataPtr, PropertyKey, Value, evaluate_expr, new_js_object_data, obj_delete, obj_get_key_value, obj_set_key_value,
};
use crate::error::JSError;
use crate::unicode::utf8_to_utf16;
use std::cell::RefCell;
use std::rc::Rc;

/// Create the Reflect object with all reflection methods
pub fn make_reflect_object() -> Result<JSObjectDataPtr, JSError> {
    let reflect_obj = new_js_object_data();
    obj_set_key_value(&reflect_obj, &"apply".into(), Value::Function("Reflect.apply".to_string()))?;
    obj_set_key_value(&reflect_obj, &"construct".into(), Value::Function("Reflect.construct".to_string()))?;
    obj_set_key_value(
        &reflect_obj,
        &"defineProperty".into(),
        Value::Function("Reflect.defineProperty".to_string()),
    )?;
    obj_set_key_value(
        &reflect_obj,
        &"deleteProperty".into(),
        Value::Function("Reflect.deleteProperty".to_string()),
    )?;
    obj_set_key_value(&reflect_obj, &"get".into(), Value::Function("Reflect.get".to_string()))?;
    obj_set_key_value(
        &reflect_obj,
        &"getOwnPropertyDescriptor".into(),
        Value::Function("Reflect.getOwnPropertyDescriptor".to_string()),
    )?;
    obj_set_key_value(
        &reflect_obj,
        &"getPrototypeOf".into(),
        Value::Function("Reflect.getPrototypeOf".to_string()),
    )?;
    obj_set_key_value(&reflect_obj, &"has".into(), Value::Function("Reflect.has".to_string()))?;
    obj_set_key_value(
        &reflect_obj,
        &"isExtensible".into(),
        Value::Function("Reflect.isExtensible".to_string()),
    )?;
    obj_set_key_value(&reflect_obj, &"ownKeys".into(), Value::Function("Reflect.ownKeys".to_string()))?;
    obj_set_key_value(
        &reflect_obj,
        &"preventExtensions".into(),
        Value::Function("Reflect.preventExtensions".to_string()),
    )?;
    obj_set_key_value(&reflect_obj, &"set".into(), Value::Function("Reflect.set".to_string()))?;
    obj_set_key_value(
        &reflect_obj,
        &"setPrototypeOf".into(),
        Value::Function("Reflect.setPrototypeOf".to_string()),
    )?;
    Ok(reflect_obj)
}

/// Handle Reflect object method calls
pub fn handle_reflect_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "apply" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.apply requires at least 2 arguments"));
            }
            let _target = evaluate_expr(env, &args[0])?;
            let _this_arg = evaluate_expr(env, &args[1])?;
            let _arguments_list = if args.len() > 2 {
                evaluate_expr(env, &args[2])?
            } else {
                Value::Undefined
            };

            // Implement Reflect.apply: call the target with given thisArg and argument list
            let target = _target;
            let this_arg = _this_arg;
            let arguments_list = _arguments_list;

            // Build argument Expr list from array-like arguments_list
            let mut arg_exprs: Vec<Expr> = Vec::new();
            match arguments_list {
                Value::Object(arr_obj) => {
                    // Expect an array-like object
                    if crate::js_array::is_array(&arr_obj) {
                        if let Some(len) = crate::js_array::get_array_length(&arr_obj) {
                            for i in 0..len {
                                if let Some(val_rc) = obj_get_key_value(&arr_obj, &i.to_string().into())? {
                                    arg_exprs.push(Expr::Value(val_rc.borrow().clone()));
                                } else {
                                    arg_exprs.push(Expr::Value(Value::Undefined));
                                }
                            }
                        }
                    } else {
                        return Err(raise_type_error!("Reflect.apply argumentsList must be an array-like object"));
                    }
                }
                Value::Undefined => {}
                _ => return Err(raise_type_error!("Reflect.apply argumentsList must be an array-like object")),
            }

            // If target is a closure or function, invoke appropriately
            match target {
                Value::Closure(params, body, captured_env, _) => {
                    // Create function environment and bind 'this'
                    let func_env = new_js_object_data();
                    func_env.borrow_mut().prototype = Some(captured_env.clone());
                    func_env.borrow_mut().is_function_scope = true;
                    obj_set_key_value(&func_env, &"this".into(), this_arg)?;

                    // Collect all arguments, expanding spreads
                    let mut evaluated_args = Vec::new();
                    crate::core::expand_spread_in_call_args(env, &arg_exprs, &mut evaluated_args)?;

                    // Bind parameters
                    crate::core::bind_function_parameters(&func_env, &params, &evaluated_args)?;

                    // Execute function body
                    crate::core::evaluate_statements(&func_env, &body)
                }
                Value::AsyncClosure(params, body, captured_env, _) => {
                    // Similar handling to async closures in evaluate_call: return a Promise object
                    let mut evaluated_args = Vec::new();
                    for ae in &arg_exprs {
                        evaluated_args.push(evaluate_expr(env, ae)?);
                    }
                    let promise = Rc::new(RefCell::new(crate::js_promise::JSPromise::default()));
                    let promise_obj = Value::Object(new_js_object_data());
                    if let Value::Object(obj) = &promise_obj {
                        obj.borrow_mut()
                            .insert("__promise".into(), Rc::new(RefCell::new(Value::Promise(promise.clone()))));
                    }

                    let func_env = new_js_object_data();
                    func_env.borrow_mut().prototype = Some(captured_env.clone());
                    func_env.borrow_mut().is_function_scope = true;
                    obj_set_key_value(&func_env, &"this".into(), this_arg)?;
                    // Bind parameters
                    crate::core::bind_function_parameters(&func_env, &params, &evaluated_args)?;
                    let result = crate::core::evaluate_statements(&func_env, &body);
                    match result {
                        Ok(val) => {
                            crate::js_promise::resolve_promise(&promise, val);
                        }
                        Err(e) => match e.kind() {
                            crate::JSErrorKind::Throw { value } => {
                                crate::js_promise::reject_promise(&promise, value.clone());
                            }
                            _ => {
                                crate::js_promise::reject_promise(&promise, Value::String(utf8_to_utf16(&format!("{}", e))));
                            }
                        },
                    }
                    Ok(promise_obj)
                }
                Value::Function(func_name) => {
                    // For native/global functions, build Expr args and call handler
                    let expr_args: Vec<Expr> = arg_exprs.into_iter().collect();
                    crate::js_function::handle_global_function(&func_name, &expr_args, env)
                }
                Value::Object(obj_map) => {
                    // If this object wraps an internal closure (function-object),
                    // invoke that closure with `this` bound to `this_arg` and
                    // the provided argument list. This preserves the correct
                    // `this` binding for `Reflect.apply` when the target is a
                    // script-defined function stored as an object.
                    if let Some(cl_rc) = obj_get_key_value(&obj_map, &"__closure__".into())? {
                        match &*cl_rc.borrow() {
                            Value::Closure(params, body, captured_env, _) => {
                                // Evaluate argument expressions to Values
                                let mut evaluated_args: Vec<Value> = Vec::new();
                                for ae in &arg_exprs {
                                    evaluated_args.push(evaluate_expr(env, ae)?);
                                }

                                // Prepare function environment and bind `this`
                                let func_env = new_js_object_data();
                                func_env.borrow_mut().prototype = Some(captured_env.clone());
                                func_env.borrow_mut().is_function_scope = true;
                                // Use env_set so function scope semantics apply
                                crate::core::env_set(&func_env, "this", this_arg)?;

                                // Bind parameters
                                crate::core::bind_function_parameters(&func_env, params, &evaluated_args)?;

                                // Execute function body
                                return crate::core::evaluate_statements(&func_env, body);
                            }
                            Value::AsyncClosure(params, body, captured_env, _) => {
                                // Evaluate argument expressions to Values
                                let mut evaluated_args: Vec<Value> = Vec::new();
                                for ae in &arg_exprs {
                                    evaluated_args.push(evaluate_expr(env, ae)?);
                                }

                                // Create promise and wrapper object
                                let promise = Rc::new(RefCell::new(crate::js_promise::JSPromise::default()));
                                let promise_obj = Value::Object(new_js_object_data());
                                if let Value::Object(obj) = &promise_obj {
                                    obj.borrow_mut()
                                        .insert("__promise".into(), Rc::new(RefCell::new(Value::Promise(promise.clone()))));
                                }

                                // Prepare function environment and bind `this`
                                let func_env = new_js_object_data();
                                func_env.borrow_mut().prototype = Some(captured_env.clone());
                                func_env.borrow_mut().is_function_scope = true;
                                crate::core::env_set(&func_env, "this", this_arg)?;

                                // Bind parameters
                                // Bind parameters
                                crate::core::bind_function_parameters(&func_env, params, &evaluated_args)?;

                                // Execute function body and resolve/reject promise
                                let result = crate::core::evaluate_statements(&func_env, body);
                                match result {
                                    Ok(val) => {
                                        promise.borrow_mut().state = crate::js_promise::PromiseState::Fulfilled(val);
                                    }
                                    Err(e) => match e.kind() {
                                        crate::JSErrorKind::Throw { value } => {
                                            promise.borrow_mut().state = crate::js_promise::PromiseState::Rejected(value.clone());
                                        }
                                        _ => {
                                            promise.borrow_mut().state =
                                                crate::js_promise::PromiseState::Rejected(Value::String(utf8_to_utf16(&format!("{}", e))));
                                        }
                                    },
                                }
                                return Ok(promise_obj);
                            }
                            _ => {
                                // Not callable - fall through to generic error below
                            }
                        }
                    }

                    // If not an internal closure, fall back to building a call expression
                    let call_expr = Expr::Call(Box::new(Expr::Value(Value::Object(obj_map.clone()))), arg_exprs);
                    crate::core::evaluate_expr(env, &call_expr)
                }
                _ => Err(raise_type_error!("Reflect.apply target is not callable")),
            }
        }
        "construct" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.construct requires at least 1 argument"));
            }
            let target = evaluate_expr(env, &args[0])?;
            let _arguments_list = if args.len() > 1 {
                evaluate_expr(env, &args[1])?
            } else {
                Value::Undefined
            };
            let _new_target = if args.len() > 2 {
                evaluate_expr(env, &args[2])?
            } else {
                target.clone()
            };

            // Implement Reflect.construct: use evaluate_new by building Expr::Value for constructor and argument list
            let mut arg_exprs: Vec<Expr> = Vec::new();
            match _arguments_list {
                Value::Object(arr_obj) => {
                    if crate::js_array::is_array(&arr_obj) {
                        if let Some(len) = crate::js_array::get_array_length(&arr_obj) {
                            for i in 0..len {
                                if let Some(val_rc) = obj_get_key_value(&arr_obj, &i.to_string().into())? {
                                    arg_exprs.push(Expr::Value(val_rc.borrow().clone()));
                                } else {
                                    arg_exprs.push(Expr::Value(Value::Undefined));
                                }
                            }
                        }
                    } else {
                        return Err(raise_type_error!("Reflect.construct argumentsList must be an array-like object"));
                    }
                }
                Value::Undefined => {}
                _ => return Err(raise_type_error!("Reflect.construct argumentsList must be an array-like object")),
            }

            // Call evaluate_new with Expr::Value(target)
            let ctor_expr = Expr::Value(target.clone());
            crate::js_class::evaluate_new(env, &ctor_expr, &arg_exprs)
        }
        "defineProperty" => {
            if args.len() < 3 {
                return Err(raise_type_error!("Reflect.defineProperty requires 3 arguments"));
            }
            let target = evaluate_expr(env, &args[0])?;
            let property_key = evaluate_expr(env, &args[1])?;
            let attributes = evaluate_expr(env, &args[2])?;

            match target {
                Value::Object(obj) => {
                    // For now, just set the property with the value from attributes
                    // This is a simplified implementation
                    if let Value::Object(attr_obj) = &attributes {
                        if let Some(value_rc) = obj_get_key_value(attr_obj, &"value".into())? {
                            let prop_key = match property_key {
                                Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                                Value::Number(n) => PropertyKey::String(n.to_string()),
                                _ => return Err(raise_type_error!("Invalid property key")),
                            };
                            obj_set_key_value(&obj, &prop_key, value_rc.borrow().clone())?;
                            Ok(Value::Boolean(true))
                        } else {
                            Ok(Value::Boolean(false))
                        }
                    } else {
                        Ok(Value::Boolean(false))
                    }
                }
                _ => Err(raise_type_error!("Reflect.defineProperty target must be an object")),
            }
        }
        "deleteProperty" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.deleteProperty requires 2 arguments"));
            }
            let target = evaluate_expr(env, &args[0])?;
            let property_key = evaluate_expr(env, &args[1])?;

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(n.to_string()),
                        _ => return Err(raise_type_error!("Invalid property key")),
                    };
                    // For now, always return true as we don't have configurable properties
                    obj_delete(&obj, &prop_key)?;
                    Ok(Value::Boolean(true))
                }
                _ => Err(raise_type_error!("Reflect.deleteProperty target must be an object")),
            }
        }
        "get" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.get requires at least 2 arguments"));
            }
            let target = evaluate_expr(env, &args[0])?;
            let property_key = evaluate_expr(env, &args[1])?;
            let _receiver = if args.len() > 2 {
                evaluate_expr(env, &args[2])?
            } else {
                target.clone()
            };

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(n.to_string()),
                        _ => return Err(raise_type_error!("Invalid property key")),
                    };
                    if let Some(value_rc) = obj_get_key_value(&obj, &prop_key)? {
                        Ok(value_rc.borrow().clone())
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Err(raise_type_error!("Reflect.get target must be an object")),
            }
        }
        "getOwnPropertyDescriptor" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.getOwnPropertyDescriptor requires 2 arguments"));
            }
            let target = evaluate_expr(env, &args[0])?;
            let property_key = evaluate_expr(env, &args[1])?;

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(n.to_string()),
                        _ => return Err(raise_type_error!("Invalid property key")),
                    };
                    if let Some(value_rc) = obj_get_key_value(&obj, &prop_key)? {
                        // Create a descriptor object
                        let descriptor = new_js_object_data();
                        obj_set_key_value(&descriptor, &"value".into(), value_rc.borrow().clone())?;
                        obj_set_key_value(&descriptor, &"writable".into(), Value::Boolean(true))?;
                        obj_set_key_value(&descriptor, &"enumerable".into(), Value::Boolean(true))?;
                        obj_set_key_value(&descriptor, &"configurable".into(), Value::Boolean(true))?;
                        Ok(Value::Object(descriptor))
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Err(raise_type_error!("Reflect.getOwnPropertyDescriptor target must be an object")),
            }
        }
        "getPrototypeOf" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.getPrototypeOf requires 1 argument"));
            }
            let target = evaluate_expr(env, &args[0])?;

            match target {
                Value::Object(obj) => {
                    if let Some(proto) = &obj.borrow().prototype {
                        Ok(Value::Object(proto.clone()))
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Err(raise_type_error!("Reflect.getPrototypeOf target must be an object")),
            }
        }
        "has" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.has requires 2 arguments"));
            }
            let target = evaluate_expr(env, &args[0])?;
            let property_key = evaluate_expr(env, &args[1])?;

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(n.to_string()),
                        _ => return Err(raise_type_error!("Invalid property key")),
                    };
                    let has_prop = obj_get_key_value(&obj, &prop_key)?.is_some();
                    Ok(Value::Boolean(has_prop))
                }
                _ => Err(raise_type_error!("Reflect.has target must be an object")),
            }
        }
        "isExtensible" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.isExtensible requires 1 argument"));
            }
            let target = evaluate_expr(env, &args[0])?;

            match target {
                Value::Object(_) => {
                    // For now, all objects are extensible
                    Ok(Value::Boolean(true))
                }
                _ => Err(raise_type_error!("Reflect.isExtensible target must be an object")),
            }
        }
        "ownKeys" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.ownKeys requires 1 argument"));
            }
            let target = evaluate_expr(env, &args[0])?;

            match target {
                Value::Object(obj) => {
                    let mut keys = Vec::new();
                    for key in obj.borrow().keys() {
                        if let PropertyKey::String(s) = key {
                            keys.push(Value::String(utf8_to_utf16(s)));
                        }
                    }
                    let keys_len = keys.len();
                    // Create an array-like object for keys
                    let result_obj = new_js_object_data();
                    for (i, key) in keys.into_iter().enumerate() {
                        obj_set_key_value(&result_obj, &i.to_string().into(), key)?;
                    }
                    // Set length property
                    obj_set_key_value(&result_obj, &"length".into(), Value::Number(keys_len as f64))?;
                    Ok(Value::Object(result_obj))
                }
                _ => Err(raise_type_error!("Reflect.ownKeys target must be an object")),
            }
        }
        "preventExtensions" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.preventExtensions requires 1 argument"));
            }
            let target = evaluate_expr(env, &args[0])?;

            match target {
                Value::Object(_) => {
                    // For now, just return true (we don't implement extensibility control yet)
                    Ok(Value::Boolean(true))
                }
                _ => Err(raise_type_error!("Reflect.preventExtensions target must be an object")),
            }
        }
        "set" => {
            if args.len() < 3 {
                return Err(raise_type_error!("Reflect.set requires at least 3 arguments"));
            }
            let target = evaluate_expr(env, &args[0])?;
            let property_key = evaluate_expr(env, &args[1])?;
            let value = evaluate_expr(env, &args[2])?;
            let _receiver = if args.len() > 3 {
                evaluate_expr(env, &args[3])?
            } else {
                target.clone()
            };

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(n.to_string()),
                        _ => return Err(raise_type_error!("Invalid property key")),
                    };
                    obj_set_key_value(&obj, &prop_key, value)?;
                    Ok(Value::Boolean(true))
                }
                _ => Err(raise_type_error!("Reflect.set target must be an object")),
            }
        }
        "setPrototypeOf" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.setPrototypeOf requires 2 arguments"));
            }
            let target = evaluate_expr(env, &args[0])?;
            let prototype = evaluate_expr(env, &args[1])?;

            match target {
                Value::Object(obj) => match prototype {
                    Value::Object(proto_obj) => {
                        obj.borrow_mut().prototype = Some(proto_obj);
                        Ok(Value::Boolean(true))
                    }
                    Value::Undefined => {
                        obj.borrow_mut().prototype = None;
                        Ok(Value::Boolean(true))
                    }
                    _ => Err(raise_type_error!("Reflect.setPrototypeOf prototype must be an object or null")),
                },
                _ => Err(raise_type_error!("Reflect.setPrototypeOf target must be an object")),
            }
        }
        _ => Err(raise_eval_error!("Unknown Reflect method")),
    }
}
