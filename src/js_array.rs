#![allow(warnings)]

use crate::core::{MutationContext, object_get_length, object_set_length};
use crate::{
    core::{EvalError, JSObjectDataPtr, PropertyKey, env_set, evaluate_call_dispatch, new_js_object_data},
    error::JSError,
    raise_eval_error, raise_range_error,
    unicode::{utf8_to_utf16, utf16_to_utf8},
};

use crate::core::{
    Expr, Value, evaluate_expr, evaluate_statements, get_own_property, object_get_key_value, object_set_key_value, value_to_sort_string,
    values_equal,
};

pub fn initialize_array<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let array_ctor = new_js_object_data(mc);
    object_set_key_value(mc, &array_ctor, "__is_constructor", &Value::Boolean(true))?;
    object_set_key_value(mc, &array_ctor, "__native_ctor", &Value::String(utf8_to_utf16("Array")))?;

    // Get Object.prototype
    let object_proto = if let Some(obj_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
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

    object_set_key_value(mc, &array_ctor, "prototype", &Value::Object(array_proto))?;
    object_set_key_value(mc, &array_proto, "constructor", &Value::Object(array_ctor))?;
    // Make constructor non-enumerable
    array_proto.borrow_mut(mc).set_non_enumerable("constructor");

    // Register static methods
    object_set_key_value(mc, &array_ctor, "isArray", &Value::Function("Array.isArray".to_string()))?;
    object_set_key_value(mc, &array_ctor, "from", &Value::Function("Array.from".to_string()))?;
    object_set_key_value(mc, &array_ctor, "of", &Value::Function("Array.of".to_string()))?;

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
        object_set_key_value(mc, &array_proto, method, &val)?;

        // Methods on prototypes should be non-enumerable so for..in doesn't list them
        array_proto.borrow_mut(mc).set_non_enumerable(method);
    }

    // Register Symbol.iterator on Array.prototype (alias to Array.prototype.values)
    if let Some(sym_val) = object_get_key_value(env, "Symbol") {
        if let Value::Object(sym_ctor) = &*sym_val.borrow() {
            if let Some(iter_sym_val) = object_get_key_value(sym_ctor, "iterator") {
                if let Value::Symbol(iter_sym) = &*iter_sym_val.borrow() {
                    let val = Value::Function("Array.prototype.values".to_string());
                    object_set_key_value(mc, &array_proto, iter_sym, &val)?;
                    array_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::Symbol(iter_sym.clone()));
                }
            }

            // Symbol.toStringTag default for Array.prototype
            if let Some(tag_sym_val) = object_get_key_value(sym_ctor, "toStringTag") {
                if let Value::Symbol(tag_sym) = &*tag_sym_val.borrow() {
                    object_set_key_value(mc, &array_proto, tag_sym, &Value::String(utf8_to_utf16("Array")))?;
                    array_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::Symbol(tag_sym.clone()));
                }
            }
        }
    }

    // Set Array.length = 1 (callable arity) so typeof Array.length === "number" per tests
    let arr_len_desc = crate::core::create_descriptor_object(mc, &Value::Number(1.0), false, false, false)?;
    crate::js_object::define_property_internal(mc, &array_ctor, "length", &arr_len_desc)?;

    env_set(mc, env, "Array", &Value::Object(array_ctor))?;
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
                return Err(raise_eval_error!("Array.isArray requires exactly one argument").into());
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
                return Err(raise_eval_error!("Array.from requires at least one argument").into());
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
                                Value::Closure(cl) => crate::core::call_closure(mc, &*cl, None, &call_args, env, None)?,
                                Value::Function(name) => crate::js_function::handle_global_function(mc, name, &call_args, env)?,
                                _ => return Err(raise_eval_error!("Array.from map function must be a function").into()),
                            };
                            result.push(mapped);
                        } else {
                            result.push(val.clone());
                        }
                    }
                }
                Value::Object(object) => {
                    // Support generic iterables via Symbol.iterator: call the iterator method and consume next() until done
                    if let Some(sym_ctor) = object_get_key_value(env, "Symbol") {
                        if let Value::Object(sym_obj) = &*sym_ctor.borrow() {
                            if let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator") {
                                if let Value::Symbol(iter_sym) = &*iter_sym_val.borrow() {
                                    // Support accessors: use accessor-aware property read which may call getter
                                    let iter_fn_val_res = crate::core::get_property_with_accessors(mc, env, &object, iter_sym)?;
                                    // If there is no iterator method (undefined), fall back to array-like handling
                                    if !matches!(iter_fn_val_res, Value::Undefined) {
                                        // Call iterator method on the object to get an iterator
                                        let iterator = match iter_fn_val_res {
                                            Value::Function(name) => {
                                                let call_env = crate::js_class::prepare_call_env_with_this(
                                                    mc,
                                                    Some(env),
                                                    Some(&Value::Object(object)),
                                                    None,
                                                    &[],
                                                    None,
                                                    Some(env),
                                                    None,
                                                )?;
                                                crate::core::evaluate_call_dispatch(
                                                    mc,
                                                    &call_env,
                                                    &Value::Function(name.clone()),
                                                    Some(&Value::Object(object)),
                                                    &[],
                                                )?
                                            }
                                            Value::Closure(cl) => {
                                                crate::core::call_closure(mc, &*cl, Some(&Value::Object(object)), &[], env, None)?
                                            }
                                            Value::Object(o) => {
                                                if let Some(cl_ptr) = o.borrow().get_closure() {
                                                    match &*cl_ptr.borrow() {
                                                        Value::Closure(cl) => {
                                                            crate::core::call_closure(mc, &*cl, Some(&Value::Object(o)), &[], env, None)?
                                                        }
                                                        Value::Function(name) => {
                                                            let call_env = crate::js_class::prepare_call_env_with_this(
                                                                mc,
                                                                Some(env),
                                                                Some(&Value::Object(o)),
                                                                None,
                                                                &[],
                                                                None,
                                                                Some(env),
                                                                None,
                                                            )?;
                                                            evaluate_call_dispatch(
                                                                mc,
                                                                &call_env,
                                                                &Value::Function(name.clone()),
                                                                Some(&Value::Object(o)),
                                                                &[],
                                                            )?
                                                        }
                                                        _ => {
                                                            return Err(raise_eval_error!("Array.from iterable is not iterable").into());
                                                        }
                                                    }
                                                } else {
                                                    return Err(raise_eval_error!("Array.from iterable is not iterable").into());
                                                }
                                            }
                                            _ => return Err(raise_eval_error!("Array.from iterable is not iterable").into()),
                                        };

                                        // Consume iterator by repeatedly calling its next() method
                                        match iterator {
                                            Value::Object(iter_obj) => {
                                                let mut idx = 0usize;
                                                loop {
                                                    if let Some(next_val) = object_get_key_value(&iter_obj, "next") {
                                                        let next_fn = next_val.borrow().clone();

                                                        let res = match &next_fn {
                                                            Value::Function(name) => {
                                                                let call_env = crate::js_class::prepare_call_env_with_this(
                                                                    mc,
                                                                    Some(env),
                                                                    Some(&Value::Object(iter_obj)),
                                                                    None,
                                                                    &[],
                                                                    None,
                                                                    Some(env),
                                                                    None,
                                                                )?;
                                                                evaluate_call_dispatch(
                                                                    mc,
                                                                    &call_env,
                                                                    &Value::Function(name.clone()),
                                                                    Some(&Value::Object(iter_obj)),
                                                                    &[],
                                                                )?
                                                            }
                                                            Value::Closure(cl) => crate::core::call_closure(
                                                                mc,
                                                                &*cl,
                                                                Some(&Value::Object(iter_obj)),
                                                                &[],
                                                                env,
                                                                None,
                                                            )?,
                                                            Value::Object(o) => {
                                                                if let Some(cl_ptr) = o.borrow().get_closure() {
                                                                    match &*cl_ptr.borrow() {
                                                                        Value::Closure(cl) => crate::core::call_closure(
                                                                            mc,
                                                                            &*cl,
                                                                            Some(&Value::Object(*o)),
                                                                            &[],
                                                                            env,
                                                                            None,
                                                                        )?,
                                                                        Value::Function(name) => {
                                                                            let call_env = crate::js_class::prepare_call_env_with_this(
                                                                                mc,
                                                                                Some(env),
                                                                                Some(&Value::Object(*o)),
                                                                                None,
                                                                                &[],
                                                                                None,
                                                                                Some(env),
                                                                                None,
                                                                            )?;
                                                                            evaluate_call_dispatch(
                                                                                mc,
                                                                                &call_env,
                                                                                &Value::Function(name.clone()),
                                                                                Some(&Value::Object(*o)),
                                                                                &[],
                                                                            )?
                                                                        }
                                                                        _ => {
                                                                            return Err(
                                                                                raise_eval_error!("Iterator.next is not callable").into()
                                                                            );
                                                                        }
                                                                    }
                                                                } else {
                                                                    return Err(raise_eval_error!("Iterator.next is not callable").into());
                                                                }
                                                            }
                                                            _ => {
                                                                return Err(raise_eval_error!("Iterator.next is not callable").into());
                                                            }
                                                        };

                                                        if let Value::Object(res_obj) = res {
                                                            // Use accessor-aware reads for 'done' and 'value' per spec
                                                            // so that getters on the iterator result may throw.
                                                            let done_val =
                                                                crate::core::get_property_with_accessors(mc, env, &res_obj, "done")?;
                                                            let done = matches!(done_val, Value::Boolean(b) if b);

                                                            if done {
                                                                break;
                                                            }

                                                            let value =
                                                                crate::core::get_property_with_accessors(mc, env, &res_obj, "value")?;

                                                            if let Some(ref fn_val) = map_fn {
                                                                // Support closures or function names for map function
                                                                let actual_fn = if let Value::Object(obj) = fn_val {
                                                                    if let Some(prop) = obj.borrow().get_closure() {
                                                                        prop.borrow().clone()
                                                                    } else {
                                                                        fn_val.clone()
                                                                    }
                                                                } else {
                                                                    fn_val.clone()
                                                                };

                                                                let call_args = vec![value, Value::Number(idx as f64)];
                                                                let mapped = match &actual_fn {
                                                                    Value::Closure(cl) => {
                                                                        crate::core::call_closure(mc, &*cl, None, &call_args, env, None)?
                                                                    }
                                                                    Value::Function(name) => crate::js_function::handle_global_function(
                                                                        mc, name, &call_args, env,
                                                                    )?,
                                                                    _ => {
                                                                        return Err(raise_eval_error!(
                                                                            "Array.from map function must be a function"
                                                                        )
                                                                        .into());
                                                                    }
                                                                };
                                                                result.push(mapped);
                                                            } else {
                                                                result.push(value);
                                                            }

                                                            idx += 1;
                                                            continue;
                                                        } else {
                                                            return Err(raise_eval_error!("Iterator.next did not return an object").into());
                                                        }
                                                    } else {
                                                        return Err(raise_eval_error!("Iterator has no next method").into());
                                                    }
                                                }
                                            }
                                            _ => return Err(raise_eval_error!("Iterator call did not return an object").into()),
                                        }

                                        let new_array = create_array(mc, env)?;
                                        set_array_length(mc, &new_array, result.len())?;
                                        for (i, val) in result.iter().enumerate() {
                                            object_set_key_value(mc, &new_array, i, val)?;
                                        }
                                        return Ok(Value::Object(new_array));
                                    }
                                }
                            }
                        }
                    }

                    if let Some(len) = get_array_length(mc, &object) {
                        for i in 0..len {
                            let element = crate::core::get_property_with_accessors(mc, env, &object, i)?;

                            if let Some(ref fn_val) = map_fn {
                                // Support closures or function names for map function
                                let actual_fn = if let Value::Object(obj) = fn_val {
                                    if let Some(prop) = obj.borrow().get_closure() {
                                        prop.borrow().clone()
                                    } else {
                                        fn_val.clone()
                                    }
                                } else {
                                    fn_val.clone()
                                };

                                let call_args = vec![element, Value::Number(i as f64)];
                                let mapped = match &actual_fn {
                                    Value::Closure(cl) => crate::core::call_closure(mc, &*cl, None, &call_args, env, None)?,
                                    Value::Function(name) => crate::js_function::handle_global_function(mc, name, &call_args, env)?,
                                    _ => return Err(raise_eval_error!("Array.from map function must be a function").into()),
                                };
                                result.push(mapped);
                            } else {
                                result.push(element);
                            }
                        }
                    } else {
                        return Err(raise_eval_error!("Array.from iterable must be array-like").into());
                    }
                }
                _ => {
                    return Err(raise_eval_error!("Array.from iterable must be array-like").into());
                }
            }

            let new_array = create_array(mc, env)?;
            set_array_length(mc, &new_array, result.len())?;
            for (i, val) in result.iter().enumerate() {
                object_set_key_value(mc, &new_array, i, val)?;
            }
            Ok(Value::Object(new_array))
        }
        "of" => {
            // Array.of(...elements)
            let new_array = create_array(mc, env)?;
            for (i, arg) in args.iter().enumerate() {
                // let val = evaluate_expr(mc, env, arg)?;
                object_set_key_value(mc, &new_array, i, arg)?;
            }
            set_array_length(mc, &new_array, args.len())?;
            Ok(Value::Object(new_array))
        }
        _ => Err(raise_eval_error!(format!("Array.{method} is not implemented")).into()),
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
                    return Err(raise_range_error!("Invalid array length").into());
                }
                if n.fract() != 0.0 {
                    return Err(raise_range_error!("Invalid array length").into());
                }
                if n < 0.0 {
                    return Err(raise_range_error!("Invalid array length").into());
                }
                if n > u32::MAX as f64 {
                    return Err(raise_range_error!("Invalid array length").into());
                }
                // Array(length) - create array with specified length
                let array_obj = create_array(mc, env)?;
                set_array_length(mc, &array_obj, n as usize)?;
                Ok(Value::Object(array_obj))
            }
            _ => {
                // Array(element) - create array with single element
                let array_obj = create_array(mc, env)?;
                object_set_key_value(mc, &array_obj, "0", &arg_val)?;
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
            object_set_key_value(mc, &array_obj, i, &arg_val)?;
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
                let val_opt = object_get_key_value(object, k.to_string());
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
                    val: &Value<'gc>,
                    current_len: &mut usize,
                ) -> Result<(), JSError> {
                    object_set_key_value(mc, map, *current_len, &val)?;
                    *current_len += 1;
                    Ok(())
                }

                // Fallback: mutate the local object copy
                for arg in args {
                    push_into_map(mc, object, arg, &mut current_len)?;
                }
                set_array_length(mc, object, current_len)?;
                // Return the array object (chainable)
                Ok(Value::Object(object.clone()))
            } else {
                Err(raise_eval_error!("Array.push expects at least one argument").into())
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
                if let Some(val) = object_get_key_value(object, i) {
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
                if let Some(val) = object_get_key_value(object, i) {
                    object_set_key_value(mc, &new_array, idx, &*val.borrow())?;
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
                    if let Some(val_rc) = object_get_key_value(object, i) {
                        let val = val_rc.borrow().clone();
                        let call_args = vec![val, Value::Number(i as f64), Value::Object(object.clone())];

                        let actual_func = if let Value::Object(obj) = &callback_val {
                            if let Some(prop) = obj.borrow().get_closure() {
                                prop.borrow().clone()
                            } else {
                                callback_val.clone()
                            }
                        } else {
                            callback_val.clone()
                        };

                        match &actual_func {
                            Value::Closure(cl) => {
                                crate::core::call_closure(mc, &*cl, None, &call_args, env, None)?;
                            }
                            Value::Function(name) => {
                                crate::js_function::handle_global_function(mc, name, &call_args, env)?;
                            }
                            _ => return Err(raise_eval_error!("Array.forEach callback must be a function").into()),
                        }
                    }
                }
                Ok(Value::Undefined)
            } else {
                Err(raise_eval_error!("Array.forEach expects at least one argument").into())
            }
        }
        "map" => {
            if !args.is_empty() {
                // let callback_val = evaluate_expr(mc, env, &args[0])?;
                let callback_val = args[0].clone();
                println!("DEBUG Array.map callback arg: {:?}", callback_val);
                let current_len = get_array_length(mc, object).unwrap_or(0);

                let new_array = create_array(mc, env)?;
                set_array_length(mc, &new_array, current_len)?;

                for i in 0..current_len {
                    if let Some(val_rc) = object_get_key_value(object, i) {
                        let val = val_rc.borrow().clone();
                        let call_args = vec![val, Value::Number(i as f64), Value::Object(object.clone())];
                        // Support inline closures wrapped as objects with internal closure like forEach does.
                        // Also, constructors are represented as objects; if a constructor object
                        // is passed (has a __native_ctor string), treat it as a global function
                        // with that name so it can be called.
                        let mut actual_callback_val = callback_val.clone();
                        if let Value::Object(obj) = &callback_val {
                            // Closure wrapper
                            if let Some(prop) = obj.borrow().get_closure() {
                                actual_callback_val = prop.borrow().clone();
                            } else if let Some(nc) = object_get_key_value(obj, "__native_ctor") {
                                if let Value::String(name_vec) = &*nc.borrow() {
                                    let name = crate::unicode::utf16_to_utf8(name_vec);
                                    actual_callback_val = Value::Function(name);
                                }
                            }
                        }

                        let res = match &actual_callback_val {
                            Value::Closure(cl) => crate::core::call_closure(mc, &*cl, None, &call_args, env, None)?,
                            Value::Function(name) => crate::js_function::handle_global_function(mc, name, &call_args, env)?,
                            _ => return Err(raise_eval_error!("Array.map callback must be a function").into()),
                        };
                        object_set_key_value(mc, &new_array, i, &res)?;
                    }
                }
                Ok(Value::Object(new_array))
            } else {
                Err(raise_eval_error!("Array.map expects at least one argument").into())
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
                    if let Some(val) = object_get_key_value(object, i) {
                        // Support inline closures wrapped as objects with internal closure like forEach does.
                        let actual_func = if let Value::Object(obj) = &callback_val {
                            if let Some(prop) = obj.borrow().get_closure() {
                                prop.borrow().clone()
                            } else {
                                callback_val.clone()
                            }
                        } else {
                            callback_val.clone()
                        };

                        let element_val = val.borrow().clone();
                        let call_args = vec![element_val.clone(), Value::Number(i as f64), Value::Object(object.clone())];

                        let res = match &actual_func {
                            Value::Closure(cl) => crate::core::call_closure(mc, &*cl, None, &call_args, env, None)?,
                            Value::Function(name) => crate::js_function::handle_global_function(mc, name, &call_args, env)?,
                            _ => return Err(raise_eval_error!("Array.filter expects a function").into()),
                        };

                        if res.to_truthy() {
                            object_set_key_value(mc, &new_array, idx, &element_val)?;
                            idx += 1;
                        }
                    }
                }
                set_array_length(mc, &new_array, idx)?;
                Ok(Value::Object(new_array))
            } else {
                Err(raise_eval_error!("Array.filter expects at least one argument").into())
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
                    return Err(raise_eval_error!("Array.reduce called on empty array with no initial value").into());
                }

                let mut accumulator: Value = if let Some(ref val) = initial_value {
                    val.clone()
                } else if let Some(val) = object_get_key_value(object, "0") {
                    val.borrow().clone()
                } else {
                    Value::Undefined
                };

                let start_idx = if initial_value.is_some() { 0 } else { 1 };
                for i in start_idx..current_len {
                    if let Some(val) = object_get_key_value(object, i) {
                        // Support inline closures wrapped as objects with internal closure.
                        let actual_func = if let Value::Object(obj) = &callback_val {
                            if let Some(prop) = obj.borrow().get_closure() {
                                prop.borrow().clone()
                            } else {
                                callback_val.clone()
                            }
                        } else {
                            callback_val.clone()
                        };

                        let args = vec![
                            accumulator.clone(),
                            val.borrow().clone(),
                            Value::Number(i as f64),
                            Value::Object(object.clone()),
                        ];

                        let res = match &actual_func {
                            Value::Closure(cl) => crate::core::call_closure(mc, &*cl, None, &args, env, None)?,
                            Value::Function(name) => crate::js_function::handle_global_function(mc, name, &args, env)?,
                            _ => return Err(raise_eval_error!("Array.reduce expects a function").into()),
                        };

                        accumulator = res;
                    }
                }
                Ok(accumulator)
            } else {
                Err(raise_eval_error!("Array.reduce expects at least one argument").into())
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
                    return Err(raise_eval_error!("Array.reduceRight called on empty array with no initial value").into());
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
                        if let Some(val) = object_get_key_value(object, i) {
                            accumulator = val.borrow().clone();
                            start_idx_rev = current_len - i;
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return Err(raise_eval_error!("Array.reduceRight called on empty array with no initial value").into());
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
                    if let Some(val) = object_get_key_value(object, i) {
                        // Support inline closures wrapped as objects with internal closure.
                        let actual_func = if let Value::Object(obj) = &callback_val {
                            if let Some(prop) = obj.borrow().get_closure() {
                                prop.borrow().clone()
                            } else {
                                callback_val.clone()
                            }
                        } else {
                            callback_val.clone()
                        };

                        let args = vec![
                            accumulator.clone(),
                            val.borrow().clone(),
                            Value::Number(i as f64),
                            Value::Object(object.clone()),
                        ];

                        let res = match &actual_func {
                            Value::Closure(cl) => crate::core::call_closure(mc, &*cl, None, &args, env, None)?,
                            Value::Function(name) => crate::js_function::handle_global_function(mc, name, &args, env)?,
                            _ => return Err(raise_eval_error!("Array.reduceRight expects a function").into()),
                        };

                        accumulator = res;
                    }
                }
                Ok(accumulator)
            } else {
                Err(raise_eval_error!("Array.reduceRight expects at least one argument").into())
            }
        }
        "find" => {
            if !args.is_empty() {
                let callback = args[0].clone();
                let current_len = get_array_length(mc, object).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(value) = object_get_key_value(object, i) {
                        // Support inline closures wrapped as objects with internal closure.
                        let actual_func = if let Value::Object(obj) = &callback {
                            if let Some(prop) = obj.borrow().get_closure() {
                                prop.borrow().clone()
                            } else {
                                callback.clone()
                            }
                        } else {
                            callback.clone()
                        };

                        let element = value.borrow().clone();
                        let args = vec![element.clone(), Value::Number(i as f64), Value::Object(object.clone())];

                        let res = match &actual_func {
                            Value::Closure(cl) => crate::core::call_closure(mc, &*cl, None, &args, env, None)?,
                            Value::Function(name) => crate::js_function::handle_global_function(mc, name, &args, env)?,
                            _ => return Err(raise_eval_error!("Array.find expects a function").into()),
                        };

                        if res.to_truthy() {
                            return Ok(element);
                        }
                    }
                }
                Ok(Value::Undefined)
            } else {
                Err(raise_eval_error!("Array.find expects at least one argument").into())
            }
        }
        "findIndex" => {
            if !args.is_empty() {
                let callback = args[0].clone();
                let current_len = get_array_length(mc, object).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(value) = object_get_key_value(object, i) {
                        let actual_func = if let Value::Object(obj) = &callback {
                            if let Some(prop) = obj.borrow().get_closure() {
                                prop.borrow().clone()
                            } else {
                                callback.clone()
                            }
                        } else {
                            callback.clone()
                        };

                        let element = value.borrow().clone();
                        let args = vec![element.clone(), Value::Number(i as f64), Value::Object(object.clone())];

                        let res = match &actual_func {
                            Value::Closure(cl) => crate::core::call_closure(mc, &*cl, None, &args, env, None)?,
                            Value::Function(name) => crate::js_function::handle_global_function(mc, name, &args, env)?,
                            _ => return Err(raise_eval_error!("Array.findIndex expects a function").into()),
                        };

                        if res.to_truthy() {
                            return Ok(Value::Number(i as f64));
                        }
                    }
                }
                Ok(Value::Number(-1.0))
            } else {
                Err(raise_eval_error!("Array.findIndex expects at least one argument").into())
            }
        }
        "some" => {
            if !args.is_empty() {
                let callback = args[0].clone();
                let current_len = get_array_length(mc, object).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(value) = object_get_key_value(object, i) {
                        let actual_func = if let Value::Object(obj) = &callback {
                            if let Some(prop) = obj.borrow().get_closure() {
                                prop.borrow().clone()
                            } else {
                                callback.clone()
                            }
                        } else {
                            callback.clone()
                        };

                        let element = value.borrow().clone();
                        let args = vec![element.clone(), Value::Number(i as f64), Value::Object(object.clone())];

                        let res = match &actual_func {
                            Value::Closure(cl) => crate::core::call_closure(mc, &*cl, None, &args, env, None)?,
                            Value::Function(name) => crate::js_function::handle_global_function(mc, name, &args, env)?,
                            _ => return Err(raise_eval_error!("Array.some expects a function").into()),
                        };

                        if res.to_truthy() {
                            return Ok(Value::Boolean(true));
                        }
                    }
                }
                Ok(Value::Boolean(false))
            } else {
                Err(raise_eval_error!("Array.some expects at least one argument").into())
            }
        }
        "every" => {
            if !args.is_empty() {
                let callback = args[0].clone();
                let current_len = get_array_length(mc, object).unwrap_or(0);

                for i in 0..current_len {
                    if let Some(value) = object_get_key_value(object, i) {
                        let actual_func = if let Value::Object(obj) = &callback {
                            if let Some(prop) = obj.borrow().get_closure() {
                                prop.borrow().clone()
                            } else {
                                callback.clone()
                            }
                        } else {
                            callback.clone()
                        };

                        let element = value.borrow().clone();
                        let args = vec![element.clone(), Value::Number(i as f64), Value::Object(object.clone())];

                        let res = match &actual_func {
                            Value::Closure(cl) => crate::core::call_closure(mc, &*cl, None, &args, env, None)?,
                            Value::Function(name) => crate::js_function::handle_global_function(mc, name, &args, env)?,
                            _ => return Err(raise_eval_error!("Array.every expects a function").into()),
                        };

                        if !res.to_truthy() {
                            return Ok(Value::Boolean(false));
                        }
                    }
                }
                Ok(Value::Boolean(true))
            } else {
                Err(raise_eval_error!("Array.every expects at least one argument").into())
            }
        }
        "concat" => {
            let result = create_array(mc, env)?;

            // First, copy all elements from current array
            let current_len = get_array_length(mc, object).unwrap_or(0);

            let mut new_index = 0;
            for i in 0..current_len {
                if let Some(val) = object_get_key_value(object, i) {
                    object_set_key_value(mc, &result, new_index, &*val.borrow())?;
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
                            if let Some(val) = object_get_key_value(&arg_obj, i) {
                                object_set_key_value(mc, &result, new_index, &*val.borrow())?;
                                new_index += 1;
                            }
                        }
                    }
                    _ => {
                        // If argument is not an array, append it directly
                        object_set_key_value(mc, &result, new_index, &arg_val)?;
                        new_index += 1;
                    }
                }
            }

            set_array_length(mc, &result, new_index)?;
            Ok(Value::Object(result))
        }
        "indexOf" => {
            if args.is_empty() {
                return Err(raise_eval_error!("Array.indexOf expects at least one argument").into());
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
                if let Some(val) = object_get_key_value(object, i)
                    && values_equal(mc, &val.borrow(), &search_element)
                {
                    return Ok(Value::Number(i as f64));
                }
            }

            Ok(Value::Number(-1.0))
        }
        "includes" => {
            if args.is_empty() {
                return Err(raise_eval_error!("Array.includes expects at least one argument").into());
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
                if let Some(val) = object_get_key_value(object, i)
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
                if let Some(val) = object_get_key_value(object, i) {
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
                // Support closures wrapped in objects or bare closures/functions
                let actual_fn = if let Value::Object(obj) = &compare_fn {
                    if let Some(prop) = obj.borrow().get_closure() {
                        prop.borrow().clone()
                    } else {
                        compare_fn.clone()
                    }
                } else {
                    compare_fn.clone()
                };

                elements.sort_by(|a, b| {
                    let args = vec![a.1.clone(), b.1.clone()];
                    match &actual_fn {
                        Value::Closure(cl) => match crate::core::call_closure(mc, &*cl, None, &args, env, None) {
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
                        },
                        Value::Function(name) => match crate::js_function::handle_global_function(mc, name, &args, env) {
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
                        },
                        _ => std::cmp::Ordering::Equal,
                    }
                });
            }

            // Update the array with sorted elements
            for (new_index, (_old_key, value)) in elements.iter().enumerate() {
                object_set_key_value(mc, object, new_index, &value)?;
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

                let left_val = object_get_key_value(object, &left_key).map(|v| v.borrow().clone());
                let right_val = object_get_key_value(object, &right_key).map(|v| v.borrow().clone());

                if let Some(val) = right_val {
                    object_set_key_value(mc, object, left_key, &val)?;
                } else {
                    object.borrow_mut(mc).properties.shift_remove(&PropertyKey::from(left_key));
                }

                if let Some(val) = left_val {
                    object_set_key_value(mc, object, right_key, &val)?;
                } else {
                    object.borrow_mut(mc).properties.shift_remove(&PropertyKey::from(right_key));
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
                if let Some(val) = object_get_key_value(object, i) {
                    deleted_elements.push(val.borrow().clone());
                }
            }

            // Create new array for deleted elements
            let deleted_array = create_array(mc, env)?;
            for (i, val) in deleted_elements.iter().enumerate() {
                object_set_key_value(mc, &deleted_array, i, &val)?;
            }
            set_array_length(mc, &deleted_array, deleted_elements.len())?;

            // Collect tail elements (elements that need to be shifted)
            // We must collect them before we start writing new items to avoid overwriting them
            let mut tail_elements = Vec::new();
            let shift_start = start + delete_count;
            for i in shift_start..current_len {
                let val_opt = object_get_key_value(object, i);
                tail_elements.push(val_opt.map(|v| v.borrow().clone()));
            }

            // Insert new items at start position
            let mut write_idx = start;
            for item in args.iter().skip(2) {
                object_set_key_value(mc, object, write_idx, &item)?;
                write_idx += 1;
            }

            // Write tail elements back
            for val_opt in tail_elements {
                if let Some(val) = val_opt {
                    object_set_key_value(mc, object, write_idx, &val)?;
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
                let first_element = object_get_key_value(object, "0").map(|v| v.borrow().clone());
                for i in 1..current_len {
                    let val_rc_opt = object_get_key_value(object, i);
                    if let Some(val_rc) = val_rc_opt {
                        object_set_key_value(mc, object, i - 1, &*val_rc.borrow())?;
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
                let val_rc_opt = object_get_key_value(object, i);
                if let Some(val_rc) = val_rc_opt {
                    object_set_key_value(mc, object, dest, &*val_rc.borrow())?;
                } else {
                    object.borrow_mut(mc).properties.shift_remove(&PropertyKey::from(dest));
                }
            }
            for (i, arg) in args.iter().enumerate() {
                object_set_key_value(mc, object, i, &arg)?;
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
                object_set_key_value(mc, object, i, &fill_value)?;
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
                if let Some(val) = object_get_key_value(object, i)
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
                if let Some(val) = object_get_key_value(object, i) {
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
            for (i, val) in result.iter().enumerate() {
                object_set_key_value(mc, &new_array, i, &val)?;
            }
            Ok(Value::Object(new_array))
        }
        "flatMap" => {
            if args.is_empty() {
                return Err(raise_eval_error!("Array.flatMap expects at least one argument").into());
            }

            let callback_val = args[0].clone();
            let current_len = get_array_length(mc, object).unwrap_or(0);

            let mut result = Vec::new();
            for i in 0..current_len {
                if let Some(val) = object_get_key_value(object, i) {
                    // Support inline closures wrapped as objects with internal closure.
                    let actual_func = if let Value::Object(obj) = &callback_val {
                        if let Some(prop) = obj.borrow().get_closure() {
                            prop.borrow().clone()
                        } else {
                            callback_val.clone()
                        }
                    } else {
                        callback_val.clone()
                    };

                    let args = vec![val.borrow().clone(), Value::Number(i as f64), Value::Object(object.clone())];

                    let mapped_val = match &actual_func {
                        Value::Closure(cl) => crate::core::call_closure(mc, &*cl, None, &args, env, None)?,
                        Value::Function(name) => crate::js_function::handle_global_function(mc, name, &args, env)?,
                        _ => return Err(raise_eval_error!("Array.flatMap expects a function").into()),
                    };

                    flatten_single_value(mc, &mapped_val, &mut result, 1)?;
                }
            }

            let new_array = create_array(mc, env)?;
            set_array_length(mc, &new_array, result.len())?;
            for (i, val) in result.iter().enumerate() {
                object_set_key_value(mc, &new_array, i, &val)?;
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
                if let Some(val) = object_get_key_value(object, i) {
                    temp_values.push(val.borrow().clone());
                }
            }

            for (i, val) in temp_values.iter().enumerate() {
                let dest_idx = target + i;
                if dest_idx < current_len {
                    object_set_key_value(mc, object, dest_idx, &val)?;
                }
            }

            Ok(Value::Object(object.clone()))
        }
        "keys" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Array.prototype.keys takes no arguments").into());
            }
            Ok(create_array_iterator(mc, env, object.clone(), "keys")?)
        }
        "values" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Array.prototype.values takes no arguments").into());
            }
            Ok(create_array_iterator(mc, env, object.clone(), "values")?)
        }
        "entries" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Array.prototype.entries takes no arguments").into());
            }
            Ok(create_array_iterator(mc, env, object.clone(), "entries")?)
        }
        "findLast" => {
            if !args.is_empty() {
                let callback = args[0].clone();
                let current_len = get_array_length(mc, object).unwrap_or(0);

                // Search from the end
                for i in (0..current_len).rev() {
                    if let Some(value) = object_get_key_value(object, i) {
                        let actual_func = if let Value::Object(obj) = &callback {
                            if let Some(prop) = obj.borrow().get_closure() {
                                prop.borrow().clone()
                            } else {
                                callback.clone()
                            }
                        } else {
                            callback.clone()
                        };

                        let element = value.borrow().clone();
                        let args = vec![element.clone(), Value::Number(i as f64), Value::Object(object.clone())];

                        let res = match &actual_func {
                            Value::Closure(cl) => crate::core::call_closure(mc, &*cl, None, &args, env, None)?,
                            Value::Function(name) => crate::js_function::handle_global_function(mc, name, &args, env)?,
                            _ => return Err(raise_eval_error!("Array.findLast expects a function").into()),
                        };

                        if res.to_truthy() {
                            return Ok(element);
                        }
                    }
                }
                Ok(Value::Undefined)
            } else {
                Err(raise_eval_error!("Array.findLast expects at least one argument").into())
            }
        }
        "findLastIndex" => {
            if !args.is_empty() {
                let callback = args[0].clone();
                let current_len = get_array_length(mc, object).unwrap_or(0);

                // Search from the end
                for i in (0..current_len).rev() {
                    if let Some(value) = object_get_key_value(object, i) {
                        let actual_func = if let Value::Object(obj) = &callback {
                            if let Some(prop) = obj.borrow().get_closure() {
                                prop.borrow().clone()
                            } else {
                                callback.clone()
                            }
                        } else {
                            callback.clone()
                        };

                        let element = value.borrow().clone();
                        let args = vec![element.clone(), Value::Number(i as f64), Value::Object(object.clone())];

                        let res = match &actual_func {
                            Value::Closure(cl) => crate::core::call_closure(mc, &*cl, None, &args, env, None)?,
                            Value::Function(name) => crate::js_function::handle_global_function(mc, name, &args, env)?,
                            _ => return Err(raise_eval_error!("Array.findLastIndex expects a function").into()),
                        };

                        if res.to_truthy() {
                            return Ok(Value::Number(i as f64));
                        }
                    }
                }
                Ok(Value::Number(-1.0))
            } else {
                Err(raise_eval_error!("Array.findLastIndex expects at least one argument").into())
            }
        }
        _ => Err(raise_eval_error!(format!("Array.{method} not found")).into()),
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
        if let Some(val) = object_get_key_value(object, i) {
            flatten_single_value(mc, &val.borrow(), result, depth)?;
        }
    }
    Ok(())
}

fn flatten_single_value<'gc>(
    mc: &MutationContext<'gc>,
    value: &Value<'gc>,
    result: &mut Vec<Value<'gc>>,
    depth: usize,
) -> Result<(), JSError> {
    if depth == 0 {
        result.push(value.clone());
        return Ok(());
    }

    match value {
        Value::Object(obj) => {
            // Check if it's an array-like object
            let is_arr = { is_array(mc, &obj) };
            if is_arr {
                flatten_array(mc, &obj, result, depth - 1)?;
            } else {
                result.push(Value::Object(*obj));
            }
        }
        _ => {
            result.push(value.clone());
        }
    }
    Ok(())
}

/// Check if an object is an Array
pub(crate) fn is_array<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>) -> bool {
    if let Some(val) = get_own_property(obj, "__is_array")
        && let Value::Boolean(b) = *val.borrow()
    {
        return b;
    }
    false
}

pub(crate) fn get_array_length<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>) -> Option<usize> {
    object_get_length(obj)
}

pub(crate) fn set_array_length<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>, new_length: usize) -> Result<(), JSError> {
    object_set_length(mc, obj, new_length)
}

pub(crate) fn create_array<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let arr = new_js_object_data(mc);
    set_array_length(mc, &arr, 0)?;

    object_set_key_value(mc, &arr, "__is_array", &Value::Boolean(true))?;
    arr.borrow_mut(mc).non_enumerable.insert("__is_array".into());
    // Mark 'length' as non-enumerable on arrays per spec
    arr.borrow_mut(mc).set_non_enumerable("length");

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

/// Create a new Array Iterator
pub(crate) fn create_array_iterator<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    object: JSObjectDataPtr<'gc>,
    kind: &str,
) -> Result<Value<'gc>, JSError> {
    let iterator = new_js_object_data(mc);

    // Store array
    object_set_key_value(mc, &iterator, "__iterator_array__", &Value::Object(object.clone()))?;
    // Store index
    object_set_key_value(mc, &iterator, "__iterator_index__", &Value::Number(0.0))?;
    // Store kind
    object_set_key_value(mc, &iterator, "__iterator_kind__", &Value::String(utf8_to_utf16(kind)))?;
    // next method
    object_set_key_value(mc, &iterator, "next", &Value::Function("ArrayIterator.prototype.next".to_string()))?;

    // Register Symbols
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        // Symbol.iterator
        if let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
            && let Value::Symbol(s) = &*iter_sym.borrow()
        {
            object_set_key_value(mc, &iterator, s, &Value::Function("IteratorSelf".to_string()))?;
        }

        // Symbol.toStringTag
        if let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
            && let Value::Symbol(s) = &*tag_sym.borrow()
        {
            object_set_key_value(mc, &iterator, s, &Value::String(utf8_to_utf16("Array Iterator")))?;
        }
    }

    Ok(Value::Object(iterator))
}

pub(crate) fn handle_array_iterator_next<'gc>(
    mc: &MutationContext<'gc>,
    iterator: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Get array
    let arr_val = object_get_key_value(iterator, "__iterator_array__").ok_or(EvalError::Js(raise_eval_error!("Iterator has no array")))?;
    let arr_ptr = if let Value::Object(o) = &*arr_val.borrow() {
        o.clone()
    } else {
        return Err(raise_eval_error!("Iterator array is invalid").into());
    };

    // Get index
    let index_val =
        object_get_key_value(iterator, "__iterator_index__").ok_or(EvalError::Js(raise_eval_error!("Iterator has no index")))?;
    let mut index = if let Value::Number(n) = &*index_val.borrow() {
        *n as usize
    } else {
        return Err(raise_eval_error!("Iterator index is invalid").into());
    };

    // Get kind
    let kind_val = object_get_key_value(iterator, "__iterator_kind__").ok_or(EvalError::Js(raise_eval_error!("Iterator has no kind")))?;
    let kind = if let Value::String(s) = &*kind_val.borrow() {
        crate::unicode::utf16_to_utf8(s)
    } else {
        return Err(raise_eval_error!("Iterator kind is invalid").into());
    };

    let length = get_array_length(mc, &arr_ptr).unwrap_or(0);

    if index >= length {
        let result_obj = new_js_object_data(mc);
        object_set_key_value(mc, &result_obj, "value", &Value::Undefined)?;
        object_set_key_value(mc, &result_obj, "done", &Value::Boolean(true))?;
        return Ok(Value::Object(result_obj));
    }

    let element_val = crate::core::get_property_with_accessors(mc, env, &arr_ptr, index)?;

    let result_value = match kind.as_str() {
        "keys" => Value::Number(index as f64),
        "values" => element_val,
        "entries" => {
            let entry = create_array(mc, env)?;
            object_set_key_value(mc, &entry, "0", &Value::Number(index as f64))?;
            object_set_key_value(mc, &entry, "1", &element_val)?;
            set_array_length(mc, &entry, 2)?;
            Value::Object(entry)
        }
        _ => return Err(raise_eval_error!("Unknown iterator kind").into()),
    };

    // Update index
    index += 1;
    object_set_key_value(mc, iterator, "__iterator_index__", &Value::Number(index as f64))?;

    let result_obj = new_js_object_data(mc);
    object_set_key_value(mc, &result_obj, "value", &result_value)?;
    object_set_key_value(mc, &result_obj, "done", &Value::Boolean(false))?;
    Ok(Value::Object(result_obj))
}

/// Serialize an array as "[a,b]" using the same element formatting used by Array.prototype.toString.
pub fn serialize_array_for_eval<'gc>(mc: &MutationContext<'gc>, object: &JSObjectDataPtr<'gc>) -> Result<String, JSError> {
    let current_len = get_array_length(mc, object).unwrap_or(0);
    let mut parts = Vec::new();
    for i in 0..current_len {
        if let Some(val_rc) = object_get_key_value(object, i) {
            match &*val_rc.borrow() {
                Value::Undefined | Value::Null => parts.push(String::new()),
                Value::String(s) => parts.push(format!("\"{}\"", utf16_to_utf8(s))),
                Value::Number(n) => parts.push(n.to_string()),
                Value::Boolean(b) => parts.push(b.to_string()),
                Value::BigInt(b) => parts.push(b.to_string()),
                Value::Object(o) => {
                    if is_array(mc, o) {
                        parts.push(serialize_array_for_eval(mc, o)?);
                    } else {
                        // Serialize nested object properties similarly to top-level object serialization
                        let mut seen_keys = std::collections::HashSet::new();
                        let mut props: Vec<(String, String)> = Vec::new();
                        let mut cur_obj_opt: Option<crate::core::JSObjectDataPtr<'_>> = Some(*o);
                        while let Some(cur_obj) = cur_obj_opt {
                            for key in cur_obj.borrow().properties.keys() {
                                // Skip non-enumerable and internal properties
                                if !cur_obj.borrow().is_enumerable(key)
                                    || matches!(key, crate::core::PropertyKey::String(s) if s == "__proto__")
                                {
                                    continue;
                                }
                                if seen_keys.contains(key) {
                                    continue;
                                }
                                seen_keys.insert(key.clone());
                                if let Some(val_rc) = object_get_key_value(&cur_obj, key) {
                                    let val = val_rc.borrow().clone();
                                    let val_str = match val {
                                        Value::String(s) => format!("\"{}\"", crate::unicode::utf16_to_utf8(&s)),
                                        Value::Number(n) => n.to_string(),
                                        Value::Boolean(b) => b.to_string(),
                                        Value::BigInt(b) => b.to_string(),
                                        Value::Undefined => "undefined".to_string(),
                                        Value::Null => "null".to_string(),
                                        Value::Object(o2) => {
                                            if is_array(mc, &o2) {
                                                serialize_array_for_eval(mc, &o2)?
                                            } else {
                                                "[object Object]".to_string()
                                            }
                                        }
                                        _ => "[object Object]".to_string(),
                                    };
                                    props.push((key.to_string(), val_str));
                                }
                            }
                            cur_obj_opt = cur_obj.borrow().prototype;
                        }
                        if props.is_empty() {
                            parts.push("{}".to_string());
                        } else {
                            let mut pairs: Vec<String> = Vec::new();
                            for (k, v) in props.iter() {
                                pairs.push(format!("\"{}\":{}", k, v));
                            }
                            parts.push(format!("{{{}}}", pairs.join(",")));
                        }
                    }
                }
                _ => parts.push("[object Object]".to_string()),
            }
        } else {
            parts.push(String::new());
        }
    }
    Ok(format!("[{}]", parts.join(",")))
}
