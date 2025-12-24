use crate::core::{
    ClosureData, Expr, JSObjectDataPtr, Statement, StatementKind, Value, evaluate_expr, has_own_property_value, prepare_function_call_env,
};

// Centralize dispatching small builtins that are represented as Value::Function
// names and need to be applied to an object receiver (e.g., "BigInt_toString",
// "BigInt_valueOf", "Date.prototype.*"). Returns Ok(Some(Value)) if handled,
// Ok(None) if not recognized, Err on error.
pub(crate) fn handle_receiver_builtin(
    func_name: &str,
    object: &JSObjectDataPtr,
    args: &[Expr],
    env: &JSObjectDataPtr,
) -> Result<Option<Value>, crate::error::JSError> {
    // BigInt builtins
    if func_name == "BigInt_toString" {
        return Ok(Some(crate::js_bigint::handle_bigint_object_method(object, "toString", args, env)?));
    }
    if func_name == "BigInt_valueOf" {
        return Ok(Some(crate::js_bigint::handle_bigint_object_method(object, "valueOf", args, env)?));
    }
    // Date prototype methods
    if func_name.starts_with("Date.prototype.") {
        let method_name = func_name.strip_prefix("Date.prototype.").unwrap();
        return Ok(Some(crate::js_date::handle_date_method(object, method_name, args, env)?));
    }
    Ok(None)
}
use crate::core::{obj_get_key_value, obj_set_key_value};
use crate::error::JSError;
use crate::js_array::handle_array_constructor;
use crate::js_date::handle_date_constructor;
use crate::unicode::utf8_to_utf16;
use std::cell::RefCell;
use std::rc::Rc;

/// Helper function to extract and validate arguments for internal functions
/// Returns a vector of evaluated arguments or an error
fn extract_internal_args(args: &[Expr], env: &JSObjectDataPtr, expected_count: usize) -> Result<Vec<Value>, JSError> {
    if args.len() != expected_count {
        let msg = format!("Internal function requires exactly {expected_count} arguments, got {}", args.len());
        return Err(raise_type_error!(msg));
    }

    let mut evaluated_args = Vec::with_capacity(expected_count);
    for arg in args {
        evaluated_args.push(evaluate_expr(env, arg)?);
    }
    Ok(evaluated_args)
}

/// Helper function to validate that first N arguments are numbers
fn validate_number_args(args: &[Value], count: usize) -> Result<Vec<f64>, JSError> {
    if args.len() < count {
        return Err(raise_type_error!(format!("Expected at least {count} arguments")));
    }

    let mut numbers = Vec::with_capacity(count);
    for i in 0..count {
        match args[i] {
            Value::Number(n) => numbers.push(n),
            _ => {
                return Err(raise_type_error!(format!("Argument {i} must be a number")));
            }
        }
    }
    Ok(numbers)
}

pub fn handle_global_function(func_name: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match func_name {
        "console.log" => crate::js_console::handle_console_method("log", args, env),
        "import" => dynamic_import_function(args, env),
        "std.sprintf" => crate::sprintf::handle_sprintf_call(env, args),
        "Object.prototype.valueOf" => object_prototype_value_of(args, env),
        "Object.prototype.toString" => object_prototype_to_string(args, env),
        "Object.prototype.hasOwnProperty" => handle_object_has_own_property(args, env),
        "String" => crate::js_string::string_constructor(args, env),
        "Function" => function_constructor(args, env),
        "parseInt" => parse_int_function(args, env),
        "parseFloat" => parse_float_function(args, env),
        "isNaN" => is_nan_function(args, env),
        "isFinite" => is_finite_function(args, env),
        "encodeURIComponent" => encode_uri_component(args, env),
        "decodeURIComponent" => decode_uri_component(args, env),
        "Array" => handle_array_constructor(args, env),
        "Object" => crate::js_class::handle_object_constructor(args, env),
        "BigInt" => crate::js_bigint::bigint_constructor(args, env),
        "Number" => crate::js_number::number_constructor(args, env),
        "Boolean" => boolean_constructor(args, env),
        "Boolean_toString" => crate::js_class::boolean_prototype_to_string(args, env),
        "Boolean_valueOf" => crate::js_class::boolean_prototype_value_of(args, env),
        "Date" => handle_date_constructor(args, env),
        "Symbol" => symbol_constructor(args, env),
        "Symbol_valueOf" => symbol_prototype_value_of(args, env),
        "Symbol_toString" => symbol_prototype_to_string(args, env),
        "new" => evaluate_new_expression(args, env),
        "eval" => evalute_eval_function(args, env),
        "encodeURI" => encode_uri(args, env),
        "decodeURI" => decode_uri(args, env),
        "__internal_resolve_promise" => internal_resolve_promise(args, env),
        "__internal_reject_promise" => internal_reject_promise(args, env),
        "__internal_promise_allsettled_resolve" => internal_promise_allsettled_resolve(args, env),
        "__internal_promise_allsettled_reject" => internal_promise_allsettled_reject(args, env),
        "__internal_allsettled_state_record_fulfilled" => internal_allsettled_state_record_fulfilled(args, env),
        "__internal_allsettled_state_record_rejected" => internal_allsettled_state_record_rejected(args, env),
        "__internal_promise_any_resolve" => internal_promise_any_resolve(args, env),
        "__internal_promise_any_reject" => internal_promise_any_reject(args, env),
        "__internal_promise_race_resolve" => internal_promise_race_resolve(args, env),
        "__internal_promise_all_resolve" => internal_promise_all_resolve(args, env),
        "__internal_promise_all_reject" => internal_promise_all_reject(args, env),
        "Promise.prototype.then" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    return crate::js_promise::handle_promise_then(&obj, args, env);
                }
            }
            Err(raise_eval_error!("Promise.prototype.then called without a promise receiver"))
        }
        "Promise.prototype.catch" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    return crate::js_promise::handle_promise_catch(&obj, args, env);
                }
            }
            Err(raise_eval_error!("Promise.prototype.catch called without a promise receiver"))
        }
        name if name.starts_with("Array.prototype.") => {
            let method = name.trim_start_matches("Array.prototype.");
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                match this_val {
                    Value::Object(obj) => {
                        return crate::js_array::handle_array_instance_method(&obj, method, args, env);
                    }
                    Value::String(s) => {
                        // Create temporary String object for array methods
                        let str_obj = crate::core::new_js_object_data();
                        crate::core::obj_set_key_value(&str_obj, &"__value__".into(), Value::String(s.clone()))?;
                        crate::core::obj_set_key_value(&str_obj, &"length".into(), Value::Number(crate::unicode::utf16_len(&s) as f64))?;
                        // Populate indices
                        let mut i = 0;
                        while let Some(c) = crate::unicode::utf16_char_at(&s, i) {
                            let char_str = crate::unicode::utf16_to_utf8(&[c]);
                            crate::core::obj_set_key_value(
                                &str_obj,
                                &i.to_string().into(),
                                Value::String(crate::unicode::utf8_to_utf16(&char_str)),
                            )?;
                            i += 1;
                        }
                        return crate::js_array::handle_array_instance_method(&str_obj, method, args, env);
                    }
                    _ => return Err(raise_type_error!("Array.prototype method called on incompatible receiver")),
                }
            }
            Err(raise_type_error!("Array.prototype method called without this"))
        }
        "Promise.prototype.finally" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    return crate::js_promise::handle_promise_finally(&obj, args, env);
                }
            }
            Err(raise_eval_error!("Promise.prototype.finally called without a promise receiver"))
        }
        "testWithIntlConstructors" => test_with_intl_constructors(args, env),
        "setTimeout" => crate::js_promise::handle_set_timeout(args, env),
        "clearTimeout" => crate::js_promise::handle_clear_timeout(args, env),
        "setInterval" => crate::js_promise::handle_set_interval(args, env),
        "clearInterval" => crate::js_promise::handle_clear_interval(args, env),

        // Basic Function.prototype.call support so builtin methods can be invoked
        // via `.call` (e.g., Object.prototype.hasOwnProperty.call(obj, 'key'))
        "Function.prototype.call" => {
            // The function to be invoked is bound as the `this` value on env
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                match this_val {
                    Value::Function(func_name) => {
                        // Implement forwarding for Object.prototype.* and Array.prototype.* builtins
                        if func_name.starts_with("Object.prototype.") || func_name.starts_with("Array.prototype.") {
                            // Need at least receiver arg
                            if args.is_empty() {
                                return Err(raise_eval_error!("call requires a receiver"));
                            }

                            // For Array.prototype methods, we can't just build a property call expression
                            // because the receiver might not have the method (e.g. string doesn't have forEach).
                            // Instead, we should call the global function handler directly with 'this' set to receiver.

                            let receiver_val = evaluate_expr(env, &args[0])?;
                            let forwarded_args = args[1..].to_vec();

                            // Create a new call environment with 'this' bound to receiver
                            let call_env = prepare_function_call_env(Some(env), Some(receiver_val), None, &[], None, Some(env))?;

                            return handle_global_function(&func_name, &forwarded_args, &call_env);
                        }
                        Err(raise_eval_error!(format!(
                            "Function.prototype.call target not supported: {}",
                            func_name
                        )))
                    }
                    Value::Closure(data) => {
                        // Call the closure with `this` set to receiver (first arg)
                        if args.is_empty() {
                            return Err(raise_eval_error!("call requires a receiver"));
                        }
                        let receiver_val = evaluate_expr(env, &args[0])?;
                        let forwarded = args[1..].to_vec();
                        let mut evaluated_args: Vec<Value> = Vec::new();
                        for ae in &forwarded {
                            evaluated_args.push(evaluate_expr(env, ae)?);
                        }
                        let params = &data.params;
                        let body = &data.body;
                        let captured_env = &data.env;
                        let func_env = prepare_function_call_env(
                            Some(captured_env),
                            Some(receiver_val),
                            Some(params),
                            &evaluated_args,
                            None,
                            Some(env),
                        )?;

                        // Create arguments object
                        let arguments_obj = crate::js_array::create_array(&func_env)?;
                        crate::js_array::set_array_length(&arguments_obj, evaluated_args.len())?;
                        for (i, arg) in evaluated_args.iter().enumerate() {
                            crate::core::obj_set_key_value(&arguments_obj, &i.to_string().into(), arg.clone())?;
                        }
                        crate::core::obj_set_key_value(&func_env, &"arguments".into(), Value::Object(arguments_obj))?;

                        crate::core::evaluate_statements(&func_env, body)
                    }
                    Value::Object(object) => {
                        // If this is an object wrapping a closure, extract and call it
                        if let Some(cl_rc) = crate::core::obj_get_key_value(&object, &"__closure__".into())?
                            && let Value::Closure(data) = &*cl_rc.borrow()
                        {
                            if args.is_empty() {
                                return Err(raise_eval_error!("call requires a receiver"));
                            }
                            let receiver_val = evaluate_expr(env, &args[0])?;
                            let forwarded = args[1..].to_vec();
                            let mut evaluated_args: Vec<Value> = Vec::new();
                            for ae in &forwarded {
                                evaluated_args.push(evaluate_expr(env, ae)?);
                            }
                            let params = &data.params;
                            let body = &data.body;
                            let captured_env = &data.env;
                            let func_env = prepare_function_call_env(
                                Some(captured_env),
                                Some(receiver_val),
                                Some(params),
                                &evaluated_args,
                                None,
                                Some(env),
                            )?;

                            // Create arguments object
                            let arguments_obj = crate::js_array::create_array(&func_env)?;
                            crate::js_array::set_array_length(&arguments_obj, evaluated_args.len())?;
                            for (i, arg) in evaluated_args.iter().enumerate() {
                                crate::core::obj_set_key_value(&arguments_obj, &i.to_string().into(), arg.clone())?;
                            }
                            crate::core::obj_set_key_value(&func_env, &"arguments".into(), Value::Object(arguments_obj))?;

                            return crate::core::evaluate_statements(&func_env, body);
                        }

                        Err(raise_eval_error!("Function.prototype.call called on non-callable"))
                    }
                    _ => Err(raise_eval_error!("Function.prototype.call called on non-callable")),
                }
            } else {
                Err(raise_eval_error!("Function.prototype.call called without this"))
            }
        }

        "Function.prototype.apply" => {
            // Minimal apply implementation for Object.prototype.* and Array.prototype.* builtins
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                match this_val {
                    Value::Function(func_name) => {
                        if func_name.starts_with("Object.prototype.") || func_name.starts_with("Array.prototype.") {
                            if args.is_empty() {
                                return Err(raise_eval_error!("apply requires a receiver"));
                            }
                            // Evaluate receiver
                            let receiver_val = evaluate_expr(env, &args[0])?;
                            // Evaluate arg array (if provided)
                            let mut forwarded_exprs: Vec<Expr> = Vec::new();
                            if args.len() >= 2 {
                                match evaluate_expr(env, &args[1])? {
                                    Value::Object(arr_obj) if crate::js_array::is_array(&arr_obj) => {
                                        // Collect numeric indices 0..n
                                        let mut i = 0usize;
                                        loop {
                                            let key = i.to_string();
                                            if let Some(val_rc) = crate::core::get_own_property(&arr_obj, &key.into()) {
                                                forwarded_exprs.push(Expr::Value(val_rc.borrow().clone()));
                                            } else {
                                                break;
                                            }
                                            i += 1;
                                        }
                                    }
                                    _ => {
                                        // Non-array apply - ignore for now
                                    }
                                }
                            }

                            // Create a new call environment with 'this' bound to receiver
                            let call_env = prepare_function_call_env(Some(env), Some(receiver_val), None, &[], None, Some(env))?;

                            return handle_global_function(&func_name, &forwarded_exprs, &call_env);
                        }
                        Err(raise_eval_error!(format!(
                            "Function.prototype.apply target not supported: {}",
                            func_name
                        )))
                    }
                    Value::Closure(data) => {
                        // apply to closure target
                        if args.is_empty() {
                            return Err(raise_eval_error!("apply requires a receiver"));
                        }
                        let receiver_val = evaluate_expr(env, &args[0])?;
                        // Evaluate argument array (if provided)
                        let mut evaluated_args: Vec<Value> = Vec::new();
                        if args.len() >= 2 {
                            match evaluate_expr(env, &args[1])? {
                                Value::Object(arr_obj) if crate::js_array::is_array(&arr_obj) => {
                                    let mut i = 0usize;
                                    loop {
                                        let key = i.to_string();
                                        if let Some(val_rc) = crate::core::get_own_property(&arr_obj, &key.into()) {
                                            evaluated_args.push(val_rc.borrow().clone());
                                        } else {
                                            break;
                                        }
                                        i += 1;
                                    }
                                }
                                _ => {}
                            }
                        }
                        let params = &data.params;
                        let body = &data.body;
                        let captured_env = &data.env;
                        let func_env = prepare_function_call_env(
                            Some(captured_env),
                            Some(receiver_val),
                            Some(params),
                            &evaluated_args,
                            None,
                            Some(env),
                        )?;

                        // Create arguments object
                        let arguments_obj = crate::js_array::create_array(&func_env)?;
                        crate::js_array::set_array_length(&arguments_obj, evaluated_args.len())?;
                        for (i, arg) in evaluated_args.iter().enumerate() {
                            crate::core::obj_set_key_value(&arguments_obj, &i.to_string().into(), arg.clone())?;
                        }
                        crate::core::obj_set_key_value(&func_env, &"arguments".into(), Value::Object(arguments_obj))?;

                        crate::core::evaluate_statements(&func_env, body)
                    }
                    Value::Object(object) => {
                        // object wrapping closure
                        if args.is_empty() {
                            return Err(raise_eval_error!("apply requires a receiver"));
                        }
                        if let Some(cl_rc) = crate::core::obj_get_key_value(&object, &"__closure__".into())?
                            && let Value::Closure(data) = &*cl_rc.borrow()
                        {
                            let receiver_val = evaluate_expr(env, &args[0])?;
                            let mut evaluated_args: Vec<Value> = Vec::new();
                            if args.len() >= 2 {
                                match evaluate_expr(env, &args[1])? {
                                    Value::Object(arr_obj) if crate::js_array::is_array(&arr_obj) => {
                                        let mut i = 0usize;
                                        loop {
                                            let key = i.to_string();
                                            if let Some(val_rc) = crate::core::get_own_property(&arr_obj, &key.into()) {
                                                evaluated_args.push(val_rc.borrow().clone());
                                            } else {
                                                break;
                                            }
                                            i += 1;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            let params = &data.params;
                            let body = &data.body;
                            let captured_env = &data.env;
                            let func_env = prepare_function_call_env(
                                Some(captured_env),
                                Some(receiver_val),
                                Some(params),
                                &evaluated_args,
                                None,
                                Some(env),
                            )?;

                            // Create arguments object
                            let arguments_obj = crate::js_array::create_array(&func_env)?;
                            crate::js_array::set_array_length(&arguments_obj, evaluated_args.len())?;
                            for (i, arg) in evaluated_args.iter().enumerate() {
                                crate::core::obj_set_key_value(&arguments_obj, &i.to_string().into(), arg.clone())?;
                            }
                            crate::core::obj_set_key_value(&func_env, &"arguments".into(), Value::Object(arguments_obj))?;

                            return crate::core::evaluate_statements(&func_env, body);
                        }

                        Err(raise_eval_error!("Function.prototype.apply called on non-callable"))
                    }
                    _ => Err(raise_eval_error!("Function.prototype.apply called on non-callable")),
                }
            } else {
                Err(raise_eval_error!("Function.prototype.apply called without this"))
            }
        }

        _ => Err(raise_eval_error!(format!("Global function {func_name} is not implemented"))),
    }
}

fn dynamic_import_function(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Dynamic import() function
    if args.len() != 1 {
        return Err(raise_type_error!("import() requires exactly one argument"));
    }
    let module_specifier = evaluate_expr(env, &args[0])?;
    let module_name = match module_specifier {
        Value::String(s) => String::from_utf16_lossy(&s),
        _ => return Err(raise_type_error!("import() argument must be a string")),
    };

    // Load the module dynamically
    let module_value = crate::js_module::load_module(&module_name, None)?;

    // Return a Promise that resolves to the module
    let promise = Rc::new(RefCell::new(crate::js_promise::JSPromise {
        state: crate::js_promise::PromiseState::Fulfilled(module_value.clone()),
        value: Some(module_value),
        on_fulfilled: Vec::new(),
        on_rejected: Vec::new(),
    }));

    let promise_obj = Value::Object(Rc::new(RefCell::new(crate::JSObjectData::new())));
    if let Value::Object(obj) = &promise_obj {
        obj.borrow_mut()
            .insert("__promise".into(), Rc::new(RefCell::new(Value::Promise(promise))));
    }
    Ok(promise_obj)
}

fn object_prototype_value_of(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // When the prototype valueOf function is invoked as a global
    // function, `this` is provided in the `env`. Delegate to the
    // same helper used for method calls so boxed primitives and
    // object behavior are consistent.
    if let Some(this_rc) = crate::core::env_get(env, "this") {
        let this_val = this_rc.borrow().clone();
        return crate::js_object::handle_value_of_method(&this_val, args, env);
    }
    Err(raise_eval_error!("Object.prototype.valueOf called without this"))
}

fn object_prototype_to_string(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if let Some(this_rc) = crate::core::env_get(env, "this") {
        let this_val = this_rc.borrow().clone();
        match this_val {
            Value::Object(_) => return crate::js_object::handle_to_string_method(&this_val, args, env),
            Value::String(_s) => {
                return Ok(Value::String(utf8_to_utf16("[object String]")));
            }
            Value::Number(_n) => {
                return Ok(Value::String(utf8_to_utf16("[object Number]")));
            }
            Value::Boolean(_b) => {
                return Ok(Value::String(utf8_to_utf16("[object Boolean]")));
            }
            Value::BigInt(_b) => {
                return Ok(Value::String(utf8_to_utf16("[object BigInt]")));
            }
            // For null/undefined/symbol/others, delegate to handler directly
            _ => return crate::js_object::handle_to_string_method(&this_val, args, env),
        }
    }
    Err(raise_eval_error!("Object.prototype.toString called without this"))
}

fn parse_int_function(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Evaluate all arguments for side effects
    let mut evaluated_args = Vec::new();
    for arg in args {
        evaluated_args.push(evaluate_expr(env, arg)?);
    }

    if evaluated_args.is_empty() {
        return Ok(Value::Number(f64::NAN));
    }

    let input_val = &evaluated_args[0];
    let input_str = match input_val {
        Value::String(s) => crate::unicode::utf16_to_utf8(s),
        _ => crate::core::value_to_string(input_val),
    };

    // 1. Trim leading whitespace
    let trimmed = input_str.trim_start();

    // 2. Handle sign
    let mut sign = 1.0;
    let mut current_str = trimmed;

    if let Some(stripped) = trimmed.strip_prefix('-') {
        sign = -1.0;
        current_str = stripped;
    } else if let Some(stripped) = trimmed.strip_prefix('+') {
        current_str = stripped;
    }

    // 3. Determine radix
    let mut radix = 10;
    let mut strip_prefix = true;

    if evaluated_args.len() > 1 {
        let radix_val = &evaluated_args[1];
        let r_num = match radix_val {
            Value::Number(n) => *n,
            Value::Boolean(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::String(s) => {
                let s_utf8 = crate::unicode::utf16_to_utf8(s);
                if s_utf8.trim().is_empty() {
                    0.0
                } else {
                    s_utf8.trim().parse::<f64>().unwrap_or(f64::NAN)
                }
            }
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            _ => f64::NAN,
        };

        // ToInt32 logic inline
        let r_int = if !r_num.is_finite() || r_num == 0.0 {
            0
        } else {
            let int = r_num.trunc();
            let two_32 = 4294967296.0;
            let int32bit = ((int % two_32) + two_32) % two_32;
            if int32bit >= two_32 / 2.0 {
                (int32bit - two_32) as i32
            } else {
                int32bit as i32
            }
        };

        if r_int != 0 {
            if !(2..=36).contains(&r_int) {
                return Ok(Value::Number(f64::NAN));
            }
            radix = r_int;
            if radix != 16 {
                strip_prefix = false;
            }
        }
    }

    if strip_prefix && current_str.starts_with("0x") || current_str.starts_with("0X") {
        radix = 16;
        current_str = &current_str[2..];
    }

    // 4. Parse digits
    let mut end_index = 0;
    for (i, ch) in current_str.char_indices() {
        if ch.is_digit(radix as u32) {
            end_index = i + ch.len_utf8();
        } else {
            break;
        }
    }

    if end_index == 0 {
        return Ok(Value::Number(f64::NAN));
    }

    let num_part = &current_str[..end_index];

    let mut result: f64 = 0.0;
    let radix_f64 = radix as f64;

    for ch in num_part.chars() {
        let digit = ch.to_digit(radix as u32).unwrap() as f64;
        result = result * radix_f64 + digit;
    }

    Ok(Value::Number(sign * result))
}

fn parse_float_function(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Evaluate all arguments for side effects
    let mut evaluated_args = Vec::new();
    for arg in args {
        evaluated_args.push(evaluate_expr(env, arg)?);
    }

    if evaluated_args.is_empty() {
        return Ok(Value::Number(f64::NAN));
    }

    let arg_val = &evaluated_args[0];
    let str_val = match arg_val {
        Value::String(s) => crate::unicode::utf16_to_utf8(s),
        _ => crate::core::value_to_string(arg_val),
    };

    let trimmed = str_val.trim_start();
    if trimmed.is_empty() {
        return Ok(Value::Number(f64::NAN));
    }

    // Find the longest prefix that is a valid float number
    // This is a simplified implementation. A full implementation would need a proper lexer.
    // We can try to parse substrings of increasing length, or better, find the end of the number.
    // Valid characters: 0-9, +, -, ., e, E

    // Simple heuristic: scan for valid float characters.
    // Note: This is not perfect (e.g. "1.2.3" -> "1.2", "1-2" -> "1")

    let mut end_index = 0;
    let mut seen_dot = false;
    let mut seen_e = false;
    let mut seen_sign_after_e = false;

    let chars: Vec<char> = trimmed.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_ascii_digit() {
            end_index = i + 1;
        } else if ch == '.' {
            if seen_dot || seen_e {
                break;
            }
            seen_dot = true;
            end_index = i + 1; // . can be part of number if followed by digits or if it is "1."
        } else if ch == 'e' || ch == 'E' {
            if seen_e {
                break;
            }
            seen_e = true;
            seen_dot = true; // cannot have dot after e
            end_index = i + 1; // e can be part if followed by digits/sign
        } else if ch == '+' || ch == '-' {
            if i == 0 {
                end_index = i + 1;
            } else if seen_e && !seen_sign_after_e && (chars[i - 1] == 'e' || chars[i - 1] == 'E') {
                seen_sign_after_e = true;
                end_index = i + 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Refine end_index: "1." is valid (1), "1e" is invalid (NaN? No, 1), "1e+" is invalid (1)
    // We need to backtrack if parsing fails.

    // Collect char indices to safely slice
    let indices: Vec<usize> = trimmed.char_indices().map(|(i, _)| i).collect();
    let len = trimmed.len();

    // We try candidates from longest to shortest
    // end_index is the count of characters
    let mut current_char_count = end_index;

    while current_char_count > 0 {
        let byte_len = if current_char_count >= indices.len() {
            len
        } else {
            indices[current_char_count]
        };

        let slice = &trimmed[..byte_len];
        if let Ok(n) = slice.parse::<f64>() {
            return Ok(Value::Number(n));
        }
        current_char_count -= 1;
    }

    // If we fall through, maybe it's "Infinity"?
    if trimmed.starts_with("Infinity") || trimmed.starts_with("+Infinity") {
        return Ok(Value::Number(f64::INFINITY));
    }
    if trimmed.starts_with("-Infinity") {
        return Ok(Value::Number(f64::NEG_INFINITY));
    }

    Ok(Value::Number(f64::NAN))
}

fn is_nan_function(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Evaluate all arguments for side effects
    let mut evaluated_args = Vec::new();
    for arg in args {
        evaluated_args.push(evaluate_expr(env, arg)?);
    }

    let arg_val = if evaluated_args.is_empty() {
        Value::Undefined
    } else {
        evaluated_args[0].clone()
    };

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
        _ => Ok(Value::Boolean(true)),                  // Objects are usually NaN (simplified)
    }
}

fn is_finite_function(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Evaluate all arguments for side effects
    let mut evaluated_args = Vec::new();
    for arg in args {
        evaluated_args.push(evaluate_expr(env, arg)?);
    }

    let arg_val = if evaluated_args.is_empty() {
        Value::Undefined
    } else {
        evaluated_args[0].clone()
    };

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

fn function_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Evaluate arguments
    let mut evaluated_args = Vec::new();
    for arg in args {
        evaluated_args.push(evaluate_expr(env, arg)?);
    }

    let body_str = if !evaluated_args.is_empty() {
        let val = evaluated_args.last().unwrap();
        match val {
            Value::String(s) => String::from_utf16_lossy(s),
            _ => crate::core::value_to_string(val),
        }
    } else {
        "".to_string()
    };

    let mut params_str = String::new();
    if evaluated_args.len() > 1 {
        for (i, arg) in evaluated_args.iter().take(evaluated_args.len() - 1).enumerate() {
            if i > 0 {
                params_str.push(',');
            }
            let arg_str = match arg {
                Value::String(s) => String::from_utf16_lossy(s),
                _ => crate::core::value_to_string(arg),
            };
            params_str.push_str(&arg_str);
        }
    }

    let func_source = format!("function anonymous({}) {{ {} }}", params_str, body_str);
    let mut tokens = crate::core::tokenize(&func_source)?;

    let stmts = crate::core::parse_statements(&mut tokens)?;

    if let Some(Statement {
        kind: StatementKind::FunctionDeclaration(_n, params, body, _i),
        ..
    }) = stmts.first()
    {
        // Create a closure with the current environment (should be global ideally, but current is acceptable for now)
        Ok(Value::Closure(Rc::new(ClosureData::new(params, body, env, None))))
    } else {
        Err(raise_type_error!("Failed to parse function body"))
    }
}

fn encode_uri_component(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Evaluate all arguments for side effects
    let mut evaluated_args = Vec::new();
    for arg in args {
        evaluated_args.push(evaluate_expr(env, arg)?);
    }

    let arg_val = if evaluated_args.is_empty() {
        Value::Undefined
    } else {
        evaluated_args[0].clone()
    };

    let str_val = match arg_val {
        Value::String(s) => String::from_utf16_lossy(&s),
        _ => crate::core::value_to_string(&arg_val),
    };

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

fn decode_uri_component(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Evaluate all arguments for side effects
    let mut evaluated_args = Vec::new();
    for arg in args {
        evaluated_args.push(evaluate_expr(env, arg)?);
    }

    let arg_val = if evaluated_args.is_empty() {
        Value::Undefined
    } else {
        evaluated_args[0].clone()
    };

    let str_val = match arg_val {
        Value::String(s) => String::from_utf16_lossy(&s),
        _ => crate::core::value_to_string(&arg_val),
    };

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

fn boolean_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Evaluate all arguments for side effects
    let mut evaluated_args = Vec::new();
    for arg in args {
        evaluated_args.push(evaluate_expr(env, arg)?);
    }

    if evaluated_args.is_empty() {
        return Ok(Value::Boolean(false));
    }

    let arg_val = &evaluated_args[0];
    let bool_val = match arg_val {
        Value::Boolean(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Object(_) => true,
        Value::Undefined => false,
        Value::Null => false,
        _ => false,
    };
    Ok(Value::Boolean(bool_val))
}

fn symbol_prototype_value_of(_args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let this_val = crate::js_class::evaluate_this(env)?;
    match this_val {
        Value::Symbol(s) => Ok(Value::Symbol(s)),
        Value::Object(obj) => {
            if let Some(val) = obj_get_key_value(&obj, &"__value__".into())?
                && let Value::Symbol(s) = &*val.borrow()
            {
                return Ok(Value::Symbol(s.clone()));
            }
            Err(raise_type_error!("Symbol.prototype.valueOf requires that 'this' be a Symbol"))
        }
        _ => Err(raise_type_error!("Symbol.prototype.valueOf requires that 'this' be a Symbol")),
    }
}

fn symbol_prototype_to_string(_args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let this_val = crate::js_class::evaluate_this(env)?;
    let sym = match this_val {
        Value::Symbol(s) => s,
        Value::Object(obj) => {
            if let Some(val) = obj_get_key_value(&obj, &"__value__".into())? {
                if let Value::Symbol(s) = &*val.borrow() {
                    s.clone()
                } else {
                    return Err(raise_type_error!("Symbol.prototype.toString requires that 'this' be a Symbol"));
                }
            } else {
                return Err(raise_type_error!("Symbol.prototype.toString requires that 'this' be a Symbol"));
            }
        }
        _ => return Err(raise_type_error!("Symbol.prototype.toString requires that 'this' be a Symbol")),
    };

    let desc = sym.description.as_deref().unwrap_or("");
    Ok(Value::String(utf8_to_utf16(&format!("Symbol({})", desc))))
}

fn symbol_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Evaluate all arguments for side effects
    let mut evaluated_args = Vec::new();
    for arg in args {
        evaluated_args.push(evaluate_expr(env, arg)?);
    }

    let description = if evaluated_args.is_empty() {
        None
    } else {
        let arg_val = &evaluated_args[0];
        match arg_val {
            Value::String(s) => Some(String::from_utf16_lossy(s)),
            Value::Undefined => None,
            _ => Some(crate::core::value_to_string(arg_val)),
        }
    };

    let symbol_data = Rc::new(crate::core::SymbolData { description });
    Ok(Value::Symbol(symbol_data))
}

fn evaluate_new_expression(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Handle new expressions: new Constructor(args)
    if args.len() == 1
        && let Expr::Call(constructor_expr, constructor_args) = &args[0]
        && let Expr::Var(constructor_name, _, _) = &**constructor_expr
    {
        match constructor_name.as_str() {
            "RegExp" => return crate::js_regexp::handle_regexp_constructor(constructor_args, env),
            "Array" => return crate::js_array::handle_array_constructor(constructor_args, env),
            "Date" => return crate::js_date::handle_date_constructor(constructor_args, env),
            "Promise" => return crate::js_promise::handle_promise_constructor(constructor_args, env),
            _ => {
                return Err(raise_eval_error!(format!("Constructor {constructor_name} not implemented")));
            }
        }
    }
    Err(raise_eval_error!("Invalid new expression"))
}

fn evalute_eval_function(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // eval function - execute the code
    if !args.is_empty() {
        let arg_val = evaluate_expr(env, &args[0])?;
        match arg_val {
            Value::String(s) => {
                let code = String::from_utf16_lossy(&s);
                match crate::core::evaluate_script(&code, None::<&std::path::Path>) {
                    Ok(v) => Ok(v),
                    Err(err) => {
                        // Convert parse/eval errors into a thrown JS Error object so that
                        // `try { eval(...) } catch (e) { e instanceof SyntaxError }` works
                        let msg = err.message();
                        let msg_expr = Expr::StringLit(crate::unicode::utf8_to_utf16(&msg));
                        let constructor = Expr::Var("SyntaxError".to_string(), None, None);
                        match crate::js_class::evaluate_new(env, &constructor, &[msg_expr]) {
                            Ok(Value::Object(obj)) => Err(raise_throw_error!(Value::Object(obj))),
                            Ok(other) => Err(raise_throw_error!(other)),
                            Err(_) => Err(err),
                        }
                    }
                }
            }
            _ => Ok(arg_val),
        }
    } else {
        Ok(Value::Undefined)
    }
}

fn encode_uri(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn decode_uri(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn internal_resolve_promise(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Internal function to resolve a promise - requires 2 args: (promise, value)
    let args = extract_internal_args(args, env, 2)?;
    log::trace!("__internal_resolve_promise called with value: {:?}", args[1]);

    match &args[0] {
        Value::Promise(promise) => {
            crate::js_promise::resolve_promise(promise, args[1].clone());
            Ok(Value::Undefined)
        }
        _ => Err(raise_type_error!("First argument must be a promise")),
    }
}

fn internal_reject_promise(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Internal function to reject a promise - requires 2 args: (promise, reason)
    let args = extract_internal_args(args, env, 2)?;
    log::trace!("__internal_reject_promise called with reason: {:?}", args[1]);

    match &args[0] {
        Value::Promise(promise) => {
            crate::js_promise::reject_promise(promise, args[1].clone());
            Ok(Value::Undefined)
        }
        _ => Err(raise_type_error!("First argument must be a promise")),
    }
}

fn internal_promise_allsettled_resolve(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Internal function for legacy allSettled - requires 3 args: (idx, value, shared_state)
    let args = extract_internal_args(args, env, 3)?;
    let numbers = validate_number_args(&args, 1)?;
    crate::js_promise::__internal_promise_allsettled_resolve(numbers[0], args[1].clone(), args[2].clone())?;
    Ok(Value::Undefined)
}

fn internal_promise_allsettled_reject(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Internal function for legacy allSettled - requires 3 args: (idx, reason, shared_state)
    let args = extract_internal_args(args, env, 3)?;
    let numbers = validate_number_args(&args, 1)?;
    crate::js_promise::__internal_promise_allsettled_reject(numbers[0], args[1].clone(), args[2].clone())?;
    Ok(Value::Undefined)
}

fn internal_allsettled_state_record_fulfilled(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Internal function for new allSettled - requires 3 args: (state_index, index, value)
    let args = extract_internal_args(args, env, 3)?;
    let numbers = validate_number_args(&args, 2)?;
    log::trace!(
        "__internal_allsettled_state_record_fulfilled called: state_id={}, index={}, value={:?}",
        numbers[0],
        numbers[1],
        args[2]
    );
    crate::js_promise::__internal_allsettled_state_record_fulfilled(numbers[0], numbers[1], args[2].clone())?;
    Ok(Value::Undefined)
}

fn internal_allsettled_state_record_rejected(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Internal function for new allSettled - requires 3 args: (state_index, index, reason)
    let args = extract_internal_args(args, env, 3)?;
    let numbers = validate_number_args(&args, 2)?;
    log::trace!(
        "__internal_allsettled_state_record_rejected called: state_id={}, index={}, reason={:?}",
        numbers[0],
        numbers[1],
        args[2]
    );
    crate::js_promise::__internal_allsettled_state_record_rejected(numbers[0], numbers[1], args[2].clone())?;
    Ok(Value::Undefined)
}

fn internal_promise_any_resolve(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Internal function for Promise.any resolve - requires 2 args: (value, result_promise)
    let args = extract_internal_args(args, env, 2)?;
    match &args[1] {
        Value::Promise(result_promise) => {
            crate::js_promise::__internal_promise_any_resolve(args[0].clone(), result_promise.clone());
            Ok(Value::Undefined)
        }
        _ => Err(raise_type_error!("Second argument must be a promise")),
    }
}

fn internal_promise_any_reject(_args: &[Expr], _env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Internal function for Promise.any reject - requires 6 args: (idx, reason, rejections, rejected_count, total, result_promise)
    // Note: This function has complex Rc<RefCell<>> parameters that cannot be easily reconstructed from JS values
    // It should only be called from within closures, not directly
    Err(raise_type_error!(
        "__internal_promise_any_reject cannot be called directly - use Promise.any instead"
    ))
}

fn internal_promise_race_resolve(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Internal function for Promise.race resolve - requires 2 args: (value, result_promise)
    let args = extract_internal_args(args, env, 2)?;
    match &args[1] {
        Value::Promise(result_promise) => {
            crate::js_promise::__internal_promise_race_resolve(args[0].clone(), result_promise.clone());
            Ok(Value::Undefined)
        }
        _ => Err(raise_type_error!("Second argument must be a promise")),
    }
}

fn internal_promise_all_resolve(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Internal function for Promise.all resolve - requires 3 args: (idx, value, state)
    let args = extract_internal_args(args, env, 3)?;
    let numbers = validate_number_args(&args, 1)?;
    let idx = numbers[0] as usize;
    let value = args[1].clone();
    if let Value::Object(state_obj) = args[2].clone() {
        // Store value in results[idx]
        if let Some(results_val_rc) = obj_get_key_value(&state_obj, &"results".into())?
            && let Value::Object(results_obj) = &*results_val_rc.borrow()
        {
            obj_set_key_value(results_obj, &idx.to_string().into(), value)?;
        }
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
                    && let Value::Promise(result_promise) = &*promise_val_rc.borrow()
                    && let Some(results_val_rc) = obj_get_key_value(&state_obj, &"results".into())?
                    && let Value::Object(results_obj) = &*results_val_rc.borrow()
                {
                    crate::js_promise::resolve_promise(result_promise, Value::Object(results_obj.clone()));
                }
            }
        }
    }
    Ok(Value::Undefined)
}

fn internal_promise_all_reject(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Internal function for Promise.all reject - requires 2 args: (reason, state)
    let args = extract_internal_args(args, env, 2)?;
    let reason = args[0].clone();
    if let Value::Object(state_obj) = args[1].clone() {
        // Reject result_promise
        if let Some(promise_val_rc) = obj_get_key_value(&state_obj, &"result_promise".into())?
            && let Value::Promise(result_promise) = &*promise_val_rc.borrow()
        {
            crate::js_promise::reject_promise(result_promise, reason);
        }
    }
    Ok(Value::Undefined)
}

fn test_with_intl_constructors(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // testWithIntlConstructors function - used for testing Intl constructors
    if args.len() != 1 {
        return Err(raise_type_error!("testWithIntlConstructors requires exactly 1 argument"));
    }
    let callback = evaluate_expr(env, &args[0])?;
    let callback_func = match callback {
        Value::Closure(data) | Value::AsyncClosure(data) => (data.params.clone(), data.body.clone(), data.env.clone()),
        _ => {
            return Err(raise_type_error!("testWithIntlConstructors requires a function as argument"));
        }
    };

    // Create a mock constructor
    let mock_constructor = crate::js_testintl::create_mock_intl_constructor()?;

    // Call the callback function with the mock constructor as argument
    // Create a fresh function environment and bind parameters
    let args_vals = vec![mock_constructor];
    let func_env = crate::core::prepare_function_call_env(Some(&callback_func.2), None, Some(&callback_func.0), &args_vals, None, None)?;
    // Execute function body
    crate::core::evaluate_statements(&func_env, &callback_func.1)?;

    Ok(Value::Undefined)
}

fn handle_object_has_own_property(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // hasOwnProperty should inspect the bound `this` and take one argument
    if args.len() != 1 {
        return Err(raise_eval_error!("hasOwnProperty requires one argument"));
    }
    let key_val = evaluate_expr(env, &args[0])?;
    if let Some(this_rc) = crate::core::env_get(env, "this") {
        let this_val = this_rc.borrow().clone();
        match this_val {
            Value::Object(obj) => {
                let exists = has_own_property_value(&obj, &key_val);
                Ok(Value::Boolean(exists))
            }
            Value::String(s) => {
                // boxed string has 'length' and indexed properties
                let key_str = match key_val {
                    Value::String(ss) => String::from_utf16_lossy(&ss),
                    Value::Number(n) => n.to_string(),
                    Value::Boolean(b) => b.to_string(),
                    Value::Undefined => "undefined".to_string(),
                    Value::Symbol(_) => return Ok(Value::Boolean(false)),
                    other => crate::core::value_to_string(&other),
                };
                if key_str == "length" {
                    return Ok(Value::Boolean(true));
                }
                if let Ok(idx) = key_str.parse::<usize>() {
                    return Ok(Value::Boolean(idx < crate::unicode::utf16_len(&s)));
                }
                Ok(Value::Boolean(false))
            }
            _ => Ok(Value::Boolean(false)),
        }
    } else {
        Err(raise_eval_error!("hasOwnProperty called without this"))
    }
}
