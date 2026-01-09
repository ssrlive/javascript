#![allow(warnings)]

use crate::core::{
    ClosureData, Expr, Gc, JSObjectDataPtr, MutationContext, PromiseState, PropertyKey, Statement, StatementKind, Value, evaluate_expr,
    get_own_property, has_own_property_value, new_js_object_data, prepare_call_env_with_this, prepare_function_call_env,
};
use crate::core::{obj_get_key_value, obj_set_key_value};
use crate::error::{EvalError, JSError};
use crate::js_array::handle_array_constructor;
use crate::js_date::handle_date_constructor;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use std::cell::RefCell;
use std::rc::Rc;

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

/// Helper function to extract and validate arguments for internal functions
/// Returns a vector of evaluated arguments or an error
fn extract_internal_args<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Expr],
    env: &JSObjectDataPtr<'gc>,
    expected_count: usize,
) -> Result<Vec<Value<'gc>>, JSError> {
    if args.len() != expected_count {
        let msg = format!("Internal function requires exactly {expected_count} arguments, got {}", args.len());
        return Err(raise_type_error!(msg));
    }

    let mut evaluated_args = Vec::with_capacity(expected_count);
    for arg in args {
        evaluated_args.push(evaluate_expr(mc, env, arg)?);
    }
    Ok(evaluated_args)
}

/// Helper function to validate that first N arguments are numbers

fn validate_internal_args(args: &[Value], count: usize) -> Result<(), JSError> {
    if args.len() != count {
        let msg = format!("Internal function requires exactly {} arguments, got {}", count, args.len());
        return Err(raise_type_error!(msg));
    }
    Ok(())
}

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

pub fn handle_global_function<'gc>(
    mc: &MutationContext<'gc>,
    func_name: &str,
    args: &[Expr],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Handle functions that expect unevaluated expressions
    match func_name {
        "import" => return dynamic_import_function(mc, args, env),
        "Function" => return function_constructor(mc, args, env),
        "new" => return evaluate_new_expression(mc, args, env),
        "eval" => return evalute_eval_function(mc, args, env),
        "Date" => return handle_date_constructor(mc, args, env),
        "testWithIntlConstructors" => return test_with_intl_constructors(mc, args, env),

        "Object.prototype.valueOf" => return object_prototype_value_of(mc, args, env),
        "Object.prototype.toString" => return object_prototype_to_string(mc, args, env),
        "Object.prototype.hasOwnProperty" => return handle_object_has_own_property(mc, args, env),
        "parseInt" => return parse_int_function(mc, args, env),
        "parseFloat" => return parse_float_function(mc, args, env),
        "isNaN" => return is_nan_function(mc, args, env),
        "isFinite" => return is_finite_function(mc, args, env),
        "encodeURIComponent" => return encode_uri_component(mc, args, env),
        "decodeURIComponent" => return decode_uri_component(mc, args, env),
        "Object" => return crate::js_class::handle_object_constructor(mc, args, env),
        "BigInt" => return crate::js_bigint::bigint_constructor(mc, args, env),
        "Number" => return crate::js_number::number_constructor(mc, args, env),
        "Boolean" => return boolean_constructor(mc, args, env),
        "Boolean_toString" => return crate::js_class::boolean_prototype_to_string(mc, args, env),
        "Boolean_valueOf" => return crate::js_class::boolean_prototype_value_of(mc, args, env),
        "Symbol" => return symbol_constructor(mc, args, env),
        "Symbol_valueOf" => return symbol_prototype_value_of(mc, args, env),
        "Symbol_toString" => return symbol_prototype_to_string(mc, args, env),
        "encodeURI" => return encode_uri(mc, args, env),
        "decodeURI" => return decode_uri(mc, args, env),
        "__internal_resolve_promise" => return internal_resolve_promise(mc, args, env),
        "__internal_reject_promise" => return internal_reject_promise(mc, args, env),
        "__internal_promise_allsettled_resolve" => return internal_promise_allsettled_resolve(mc, args, env),
        "__internal_promise_allsettled_reject" => return internal_promise_allsettled_reject(mc, args, env),
        "__internal_allsettled_state_record_fulfilled" => return internal_allsettled_state_record_fulfilled(mc, args, env),
        "__internal_allsettled_state_record_rejected" => return internal_allsettled_state_record_rejected(mc, args, env),
        "__internal_promise_any_resolve" => return internal_promise_any_resolve(mc, args, env),
        "__internal_promise_any_reject" => return internal_promise_any_reject(mc, args, env),
        "__internal_promise_race_resolve" => return internal_promise_race_resolve(mc, args, env),
        "__internal_promise_all_resolve" => return internal_promise_all_resolve(mc, args, env),
        "__internal_promise_all_reject" => return internal_promise_all_reject(mc, args, env),

        "Promise.prototype.then" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    return crate::js_promise::handle_promise_then(mc, &obj, args, env);
                }
            }
            return Err(raise_eval_error!("Promise.prototype.then called without a promise receiver"));
        }
        "Promise.prototype.catch" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    return crate::js_promise::handle_promise_catch(mc, &obj, args, env);
                }
            }
            return Err(raise_eval_error!("Promise.prototype.catch called without a promise receiver"));
        }
        "Promise.prototype.finally" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    return crate::js_promise::handle_promise_finally(mc, &obj, args, env);
                }
            }
            return Err(raise_eval_error!("Promise.prototype.finally called without a promise receiver"));
        }
        "setTimeout" => return crate::js_promise::handle_set_timeout(mc, args, env),
        "clearTimeout" => return crate::js_promise::handle_clear_timeout(mc, args, env),
        "setInterval" => return crate::js_promise::handle_set_interval(mc, args, env),
        "clearInterval" => return crate::js_promise::handle_clear_interval(mc, args, env),

        "Function.prototype.call" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                match this_val {
                    Value::Function(func_name) => {
                        if func_name.starts_with("Object.prototype.") || func_name.starts_with("Array.prototype.") {
                            if args.is_empty() {
                                return Err(raise_eval_error!("call requires a receiver"));
                            }
                            let receiver_val = evaluate_expr(mc, env, &args[0])?;
                            let forwarded_args = args[1..].to_vec();
                            let call_env = prepare_call_env_with_this(mc, Some(env), receiver_val, None, &[], Some(env))?;
                            return handle_global_function(mc, &func_name, &forwarded_args, &call_env);
                        }
                        return Err(raise_eval_error!(format!(
                            "Function.prototype.call target not supported: {}",
                            func_name
                        )));
                    }
                    Value::Closure(data) => {
                        if args.is_empty() {
                            return Err(raise_eval_error!("call requires a receiver"));
                        }
                        let receiver_val = evaluate_expr(mc, env, &args[0])?;
                        let forwarded = args[1..].to_vec();
                        let mut evaluated_args: Vec<Value> = Vec::new();
                        for ae in &forwarded {
                            evaluated_args.push(evaluate_expr(mc, env, ae)?);
                        }
                        let params = &data.params;
                        let body = &data.body;
                        let captured_env = &data.env;
                        let func_env = prepare_function_call_env(
                            mc,
                            Some(captured_env),
                            Some(receiver_val),
                            Some(params),
                            &evaluated_args,
                            None,
                            Some(env),
                        )?;

                        let arguments_obj = crate::js_array::create_array(mc, &func_env)?;
                        crate::js_array::set_array_length(mc, &arguments_obj, evaluated_args.len())?;
                        for (i, arg) in evaluated_args.iter().enumerate() {
                            crate::core::obj_set_key_value(mc, &arguments_obj, &i.to_string().into(), arg.clone())?;
                        }
                        crate::core::obj_set_key_value(mc, &func_env, &"arguments".into(), Value::Object(arguments_obj))?;

                        return crate::core::evaluate_statements(mc, &func_env, body);
                    }
                    Value::Object(object) => {
                        if let Some(cl_rc) = crate::core::obj_get_key_value(&object, &"__closure__".into())?
                            && let Value::Closure(data) = &*cl_rc.borrow()
                        {
                            if args.is_empty() {
                                return Err(raise_eval_error!("call requires a receiver"));
                            }
                            let receiver_val = evaluate_expr(mc, env, &args[0])?;
                            let forwarded = args[1..].to_vec();
                            let mut evaluated_args: Vec<Value> = Vec::new();
                            for ae in &forwarded {
                                evaluated_args.push(evaluate_expr(mc, env, ae)?);
                            }
                            let params = &data.params;
                            let body = &data.body;
                            let captured_env = &data.env;
                            let func_env = prepare_function_call_env(
                                mc,
                                Some(captured_env),
                                Some(receiver_val),
                                Some(params),
                                &evaluated_args,
                                None,
                                Some(env),
                            )?;

                            let arguments_obj = crate::js_array::create_array(mc, &func_env)?;
                            crate::js_array::set_array_length(mc, &arguments_obj, evaluated_args.len())?;
                            for (i, arg) in evaluated_args.iter().enumerate() {
                                crate::core::obj_set_key_value(mc, &arguments_obj, &i.to_string().into(), arg.clone())?;
                            }
                            crate::core::obj_set_key_value(mc, &func_env, &"arguments".into(), Value::Object(arguments_obj))?;

                            return crate::core::evaluate_statements(mc, &func_env, body);
                        }
                        return Err(raise_eval_error!("Function.prototype.call called on non-callable"));
                    }
                    _ => return Err(raise_eval_error!("Function.prototype.call called on non-callable")),
                }
            } else {
                return Err(raise_eval_error!("Function.prototype.call called without this"));
            }
        }

        "Function.prototype.apply" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                match this_val {
                    Value::Function(func_name) => {
                        if func_name.starts_with("Object.prototype.") || func_name.starts_with("Array.prototype.") {
                            if args.is_empty() {
                                return Err(raise_eval_error!("apply requires a receiver"));
                            }
                            let receiver_val = evaluate_expr(mc, env, &args[0])?;
                            let mut forwarded_exprs: Vec<Expr> = Vec::new();
                            if args.len() >= 2 {
                                match evaluate_expr(mc, env, &args[1])? {
                                    Value::Object(arr_obj) if crate::js_array::is_array(mc, &arr_obj) => {
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
                                    _ => {}
                                }
                            }
                            let call_env = prepare_call_env_with_this(mc, Some(env), receiver_val, None, &[], Some(env))?;
                            return handle_global_function(mc, &func_name, &forwarded_exprs, &call_env);
                        }
                        return Err(raise_eval_error!(format!(
                            "Function.prototype.apply target not supported: {}",
                            func_name
                        )));
                    }
                    Value::Closure(data) => {
                        if args.is_empty() {
                            return Err(raise_eval_error!("apply requires a receiver"));
                        }
                        let receiver_val = evaluate_expr(mc, env, &args[0])?;
                        let mut evaluated_args: Vec<Value> = Vec::new();
                        if args.len() >= 2 {
                            match evaluate_expr(mc, env, &args[1])? {
                                Value::Object(arr_obj) if crate::js_array::is_array(mc, &arr_obj) => {
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
                            mc,
                            Some(captured_env),
                            Some(receiver_val),
                            Some(params),
                            &evaluated_args,
                            None,
                            Some(env),
                        )?;

                        let arguments_obj = crate::js_array::create_array(mc, &func_env)?;
                        crate::js_array::set_array_length(mc, &arguments_obj, evaluated_args.len())?;
                        for (i, arg) in evaluated_args.iter().enumerate() {
                            crate::core::obj_set_key_value(mc, &arguments_obj, &i.to_string().into(), arg.clone())?;
                        }
                        crate::core::obj_set_key_value(mc, &func_env, &"arguments".into(), Value::Object(arguments_obj))?;

                        return crate::core::evaluate_statements(mc, &func_env, body);
                    }
                    Value::Object(object) => {
                        if let Some(cl_rc) = crate::core::obj_get_key_value(&object, &"__closure__".into())?
                            && let Value::Closure(data) = &*cl_rc.borrow()
                        {
                            if args.is_empty() {
                                return Err(raise_eval_error!("apply requires a receiver"));
                            }
                            let receiver_val = evaluate_expr(mc, env, &args[0])?;
                            let mut evaluated_args: Vec<Value> = Vec::new();
                            if args.len() >= 2 {
                                match evaluate_expr(mc, env, &args[1])? {
                                    Value::Object(arr_obj) if crate::js_array::is_array(mc, &arr_obj) => {
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
                                mc,
                                Some(captured_env),
                                Some(receiver_val),
                                Some(params),
                                &evaluated_args,
                                None,
                                Some(env),
                            )?;

                            let arguments_obj = crate::js_array::create_array(mc, &func_env)?;
                            crate::js_array::set_array_length(mc, &arguments_obj, evaluated_args.len())?;
                            for (i, arg) in evaluated_args.iter().enumerate() {
                                crate::core::obj_set_key_value(mc, &arguments_obj, &i.to_string().into(), arg.clone())?;
                            }
                            crate::core::obj_set_key_value(mc, &func_env, &"arguments".into(), Value::Object(arguments_obj))?;

                            return crate::core::evaluate_statements(mc, &func_env, body);
                        }
                        return Err(raise_eval_error!("Function.prototype.apply called on non-callable"));
                    }
                    _ => return Err(raise_eval_error!("Function.prototype.apply called on non-callable")),
                }
            } else {
                return Err(raise_eval_error!("Function.prototype.apply called without this"));
            }
        }
        _ => {}
    }

    // Evaluate arguments for others
    let mut evaluated_args = Vec::with_capacity(args.len());
    for arg in args {
        evaluated_args.push(evaluate_expr(mc, env, arg)?);
    }
    let args = &evaluated_args;

    match func_name {
        "console.error" => Ok(crate::js_console::handle_console_method(mc, "error", args, env)?),
        "console.log" => Ok(crate::js_console::handle_console_method(mc, "log", args, env)?),
        "String" => crate::js_string::string_constructor(mc, args, env),
        "Array" => handle_array_constructor(mc, args, env),

        name if name.starts_with("Array.prototype.") => {
            let method = name.trim_start_matches("Array.prototype.");
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                match this_val {
                    Value::Object(obj) => {
                        return crate::js_array::handle_array_instance_method(mc, &obj, method, args, env);
                    }
                    Value::String(s) => {
                        let str_obj = crate::core::new_js_object_data(mc);
                        crate::core::obj_set_key_value(mc, &str_obj, &"__value__".into(), Value::String(s.clone()))?;
                        crate::core::obj_set_key_value(
                            mc,
                            &str_obj,
                            &"length".into(),
                            Value::Number(crate::unicode::utf16_len(&s) as f64),
                        )?;
                        let mut i = 0;
                        while let Some(c) = crate::unicode::utf16_char_at(&s, i) {
                            let char_str = crate::unicode::utf16_to_utf8(&[c]);
                            crate::core::obj_set_key_value(
                                mc,
                                &str_obj,
                                &i.to_string().into(),
                                Value::String(crate::unicode::utf8_to_utf16(&char_str)),
                            )?;
                            i += 1;
                        }
                        return crate::js_array::handle_array_instance_method(mc, &str_obj, method, args, env);
                    }
                    _ => return Err(raise_type_error!("Array.prototype method called on incompatible receiver")),
                }
            }
            Err(raise_type_error!("Array.prototype method called without this"))
        }

        _ => Err(raise_eval_error!(format!("Global function {} not found", func_name))),
    }
}

fn dynamic_import_function<'gc>(mc: &MutationContext<'gc>, args: &[Expr], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    // Dynamic import() function
    if args.len() != 1 {
        return Err(raise_type_error!("import() requires exactly one argument"));
    }
    let module_specifier = evaluate_expr(mc, env, &args[0])?;
    let module_name = match module_specifier {
        Value::String(s) => utf16_to_utf8(&s),
        _ => return Err(raise_type_error!("import() argument must be a string")),
    };

    // Load the module dynamically
    let module_value = crate::js_module::load_module(mc, &module_name, None)?;

    // Return a Promise that resolves to the module
    let promise = Gc::new(
        mc,
        GcCell::new(crate::js_promise::JSPromise {
            state: crate::js_promise::PromiseState::Fulfilled(module_value.clone()),
            value: Some(module_value),
            on_fulfilled: Vec::new(),
            on_rejected: Vec::new(),
        }),
    );

    let promise_obj = Value::Object(new_js_object_data(mc));
    if let Value::Object(obj) = &promise_obj {
        obj_set_key_value(mc, obj, &"__promise".into(), Value::Promise(promise))?;
    }
    Ok(promise_obj)
}

fn object_prototype_value_of<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // When the prototype valueOf function is invoked as a global
    // function, `this` is provided in the `env`. Delegate to the
    // same helper used for method calls so boxed primitives and
    // object behavior are consistent.
    if let Some(this_rc) = crate::core::env_get(env, "this") {
        let this_val = this_rc.borrow().clone();
        return crate::js_object::handle_value_of_method(mc, &this_val, args, env);
    }
    Err(raise_eval_error!("Object.prototype.valueOf called without this"))
}

fn object_prototype_to_string<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if let Some(this_rc) = crate::core::env_get(env, "this") {
        let this_val = this_rc.borrow().clone();
        match this_val {
            Value::Object(_) => return crate::js_object::handle_to_string_method(mc, &this_val, args, env),
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
            _ => return crate::js_object::handle_to_string_method(mc, &this_val, args, env),
        }
    }
    Err(raise_eval_error!("Object.prototype.toString called without this"))
}

fn parse_int_function<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    // Evaluate all arguments for side effects

    if args.is_empty() {
        return Ok(Value::Number(f64::NAN));
    }

    let input_val = args[0];
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

    if args.len() > 1 {
        let radix_val = args[1];
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

fn parse_float_function<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    // Evaluate all arguments for side effects

    if args.is_empty() {
        return Ok(Value::Number(f64::NAN));
    }

    let arg_val = args[0];
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

fn is_nan_function<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    // Evaluate all arguments for side effects

    let arg_val = if args.is_empty() {
        Value::Undefined
    } else {
        evaluated_args[0].clone()
    };

    match arg_val {
        Value::Number(n) => Ok(Value::Boolean(n.is_nan())),
        Value::String(s) => {
            let str_val = utf16_to_utf8(&s);
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

fn is_finite_function<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    // Evaluate all arguments for side effects

    let arg_val = if args.is_empty() {
        Value::Undefined
    } else {
        evaluated_args[0].clone()
    };

    match arg_val {
        Value::Number(n) => Ok(Value::Boolean(n.is_finite())),
        Value::String(s) => {
            let str_val = utf16_to_utf8(&s);
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

fn function_constructor<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    // Evaluate arguments

    let body_str = if !args.is_empty() {
        let val = args.last().unwrap();
        match val {
            Value::String(s) => utf16_to_utf8(s),
            _ => crate::core::value_to_string(val),
        }
    } else {
        "".to_string()
    };

    let mut params_str = String::new();
    if args.len() > 1 {
        for (i, arg) in args.iter().take(args.len() - 1).enumerate() {
            if i > 0 {
                params_str.push(',');
            }
            let arg_str = match arg {
                Value::String(s) => utf16_to_utf8(s),
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
        Ok(Value::Closure(Gc::allocate(mc, ClosureData::new(params, body, env, None))))
    } else {
        Err(raise_type_error!("Failed to parse function body"))
    }
}

fn encode_uri_component<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    // Evaluate all arguments for side effects

    let arg_val = if args.is_empty() {
        Value::Undefined
    } else {
        evaluated_args[0].clone()
    };

    let str_val = match arg_val {
        Value::String(s) => utf16_to_utf8(&s),
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

fn decode_uri_component<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    // Evaluate all arguments for side effects

    let arg_val = if args.is_empty() {
        Value::Undefined
    } else {
        evaluated_args[0].clone()
    };

    let str_val = match arg_val {
        Value::String(s) => utf16_to_utf8(&s),
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

fn boolean_constructor<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    // Evaluate all arguments for side effects

    if args.is_empty() {
        return Ok(Value::Boolean(false));
    }

    let arg_val = args[0];
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

fn symbol_prototype_value_of<'gc>(
    mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let this_val = crate::js_class::evaluate_this(mc, env)?;
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

fn symbol_prototype_to_string<'gc>(
    mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let this_val = crate::js_class::evaluate_this(mc, env)?;
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

fn symbol_constructor<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    // Evaluate all arguments for side effects

    let description = if args.is_empty() {
        None
    } else {
        let arg_val = args[0];
        match arg_val {
            Value::String(s) => Some(utf16_to_utf8(s)),
            Value::Undefined => None,
            _ => Some(crate::core::value_to_string(arg_val)),
        }
    };

    let symbol_data = Gc::allocate(mc, crate::core::SymbolData { description });
    Ok(Value::Symbol(symbol_data))
}

fn evaluate_new_expression<'gc>(mc: &MutationContext<'gc>, args: &[Expr], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    // Handle new expressions: new Constructor(args)
    if args.len() == 1
        && let Expr::Call(constructor_expr, constructor_args) = &args[0]
        && let Expr::Var(constructor_name, _, _) = &**constructor_expr
    {
        match constructor_name.as_str() {
            "RegExp" => return crate::js_regexp::handle_regexp_constructor(mc, constructor_args, env),
            "Array" => return crate::js_array::handle_array_constructor(mc, constructor_args, env),
            "Date" => return crate::js_date::handle_date_constructor(mc, constructor_args, env),
            "Promise" => return crate::js_promise::handle_promise_constructor(mc, constructor_args, env),
            _ => {
                return Err(raise_eval_error!(format!("Constructor {constructor_name} not implemented")));
            }
        }
    }
    Err(raise_eval_error!("Invalid new expression"))
}

fn evalute_eval_function<'gc>(mc: &MutationContext<'gc>, args: &[Expr], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    // eval function - execute the code
    if !args.is_empty() {
        let arg_val = evaluate_expr(mc, env, &args[0])?;
        match arg_val {
            Value::String(s) => {
                let code = utf16_to_utf8(&s);
                match crate::core::evaluate_script(mc, &code, None::<&std::path::Path>) {
                    Ok(v) => Ok(v),
                    Err(err) => {
                        // Convert parse/eval errors into a thrown JS Error object so that
                        // `try { eval(...) } catch (e) { e instanceof SyntaxError }` works
                        let msg = err.message();
                        let msg_expr = Expr::StringLit(crate::unicode::utf8_to_utf16(&msg));
                        let constructor = Expr::Var("SyntaxError".to_string(), None, None);
                        match crate::js_class::evaluate_new(mc, env, &constructor, &[msg_expr]) {
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

fn encode_uri<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    if !args.is_empty() {
        let arg_val = args[0].clone();
        match arg_val {
            Value::String(s) => {
                let str_val = utf16_to_utf8(&s);
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

fn decode_uri<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    if !args.is_empty() {
        let arg_val = args[0].clone();
        match arg_val {
            Value::String(s) => {
                let str_val = utf16_to_utf8(&s);
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

fn internal_resolve_promise<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Internal function to resolve a promise - requires 2 args: (promise, value)
    validate_internal_args(args, 2)?;
    log::trace!("__internal_resolve_promise called with value: {:?}", args[1]);

    match &args[0] {
        Value::Promise(promise) => {
            crate::js_promise::resolve_promise(mc, promise, args[1].clone());
            Ok(Value::Undefined)
        }
        _ => Err(raise_type_error!("First argument must be a promise")),
    }
}

fn internal_reject_promise<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    // Internal function to reject a promise - requires 2 args: (promise, reason)
    validate_internal_args(args, 2)?;
    log::trace!("__internal_reject_promise called with reason: {:?}", args[1]);

    match &args[0] {
        Value::Promise(promise) => {
            crate::js_promise::reject_promise(mc, promise, args[1].clone());
            Ok(Value::Undefined)
        }
        _ => Err(raise_type_error!("First argument must be a promise")),
    }
}

fn internal_promise_allsettled_resolve<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Internal function for legacy allSettled - requires 3 args: (idx, value, shared_state)
    validate_internal_args(args, 3)?;
    let numbers = validate_number_args(&args, 1)?;
    crate::js_promise::__internal_promise_allsettled_resolve(mc, numbers[0], args[1].clone(), args[2].clone())?;
    Ok(Value::Undefined)
}

fn internal_promise_allsettled_reject<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Internal function for legacy allSettled - requires 3 args: (idx, reason, shared_state)
    validate_internal_args(args, 3)?;
    let numbers = validate_number_args(&args, 1)?;
    crate::js_promise::__internal_promise_allsettled_reject(mc, numbers[0], args[1].clone(), args[2].clone())?;
    Ok(Value::Undefined)
}

fn internal_allsettled_state_record_fulfilled<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Internal function for new allSettled - requires 3 args: (state_index, index, value)
    validate_internal_args(args, 3)?;
    let numbers = validate_number_args(&args, 2)?;
    log::trace!(
        "__internal_allsettled_state_record_fulfilled called: state_id={}, index={}, value={:?}",
        numbers[0],
        numbers[1],
        args[2]
    );
    crate::js_promise::__internal_allsettled_state_record_fulfilled(mc, numbers[0], numbers[1], args[2].clone())?;
    Ok(Value::Undefined)
}

fn internal_allsettled_state_record_rejected<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Internal function for new allSettled - requires 3 args: (state_index, index, reason)
    validate_internal_args(args, 3)?;
    let numbers = validate_number_args(&args, 2)?;
    log::trace!(
        "__internal_allsettled_state_record_rejected called: state_id={}, index={}, reason={:?}",
        numbers[0],
        numbers[1],
        args[2]
    );
    crate::js_promise::__internal_allsettled_state_record_rejected(mc, numbers[0], numbers[1], args[2].clone())?;
    Ok(Value::Undefined)
}

fn internal_promise_any_resolve<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Internal function for Promise.any resolve - requires 2 args: (value, result_promise)
    validate_internal_args(args, 2)?;
    match &args[1] {
        Value::Promise(result_promise) => {
            crate::js_promise::__internal_promise_any_resolve(mc, args[0].clone(), result_promise.clone());
            Ok(Value::Undefined)
        }
        _ => Err(raise_type_error!("Second argument must be a promise")),
    }
}

fn internal_promise_any_reject<'gc>(
    _mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Internal function for Promise.any reject - requires 6 args: (idx, reason, rejections, rejected_count, total, result_promise)
    // Note: This function has complex Rc<RefCell<>> parameters that cannot be easily reconstructed from JS values
    // It should only be called from within closures, not directly
    Err(raise_type_error!(
        "__internal_promise_any_reject cannot be called directly - use Promise.any instead"
    ))
}

fn internal_promise_race_resolve<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Internal function for Promise.race resolve - requires 2 args: (value, result_promise)
    validate_internal_args(args, 2)?;
    match &args[1] {
        Value::Promise(result_promise) => {
            crate::js_promise::__internal_promise_race_resolve(mc, args[0].clone(), result_promise.clone());
            Ok(Value::Undefined)
        }
        _ => Err(raise_type_error!("Second argument must be a promise")),
    }
}

fn internal_promise_all_resolve<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Internal function for Promise.all resolve - requires 3 args: (idx, value, state)
    validate_internal_args(args, 3)?;
    let numbers = validate_number_args(&args, 1)?;
    let idx = numbers[0] as usize;
    let value = args[1].clone();
    if let Value::Object(state_obj) = args[2].clone() {
        // Store value in results[idx]
        if let Some(results_val_rc) = obj_get_key_value(&state_obj, &"results".into())?
            && let Value::Object(results_obj) = &*results_val_rc.borrow()
        {
            obj_set_key_value(mc, results_obj, &idx.to_string().into(), value)?;
        }
        // Increment completed
        if let Some(completed_val_rc) = obj_get_key_value(&state_obj, &"completed".into())?
            && let Value::Number(completed) = &*completed_val_rc.borrow()
        {
            let new_completed = completed + 1.0;
            obj_set_key_value(mc, &state_obj, &"completed".into(), Value::Number(new_completed))?;
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
                    crate::js_promise::resolve_promise(mc, result_promise, Value::Object(results_obj.clone()));
                }
            }
        }
    }
    Ok(Value::Undefined)
}

fn internal_promise_all_reject<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Internal function for Promise.all reject - requires 2 args: (reason, state)
    validate_internal_args(args, 2)?;
    let reason = args[0].clone();
    if let Value::Object(state_obj) = args[1].clone() {
        // Reject result_promise
        if let Some(promise_val_rc) = obj_get_key_value(&state_obj, &"result_promise".into())?
            && let Value::Promise(result_promise) = &*promise_val_rc.borrow()
        {
            crate::js_promise::reject_promise(mc, result_promise, reason);
        }
    }
    Ok(Value::Undefined)
}

fn test_with_intl_constructors<'gc>(mc: &MutationContext<'gc>, args: &[Expr], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    /*
    // testWithIntlConstructors function - used for testing Intl constructors
    if args.len() != 1 {
        return Err(raise_type_error!("testWithIntlConstructors requires exactly 1 argument"));
    }
    let callback = evaluate_expr(mc, env, &args[0])?;
    let callback_func = match callback {
        Value::Closure(data) | Value::AsyncClosure(data) => (data.params.clone(), data.body.clone(), data.env.clone()),
        _ => {
            return Err(raise_type_error!("testWithIntlConstructors requires a function as argument"));
        }
    };

    // Create a mock constructor
    let mock_constructor = crate::js_testintl::create_mock_intl_constructor(mc)?;

    // Call the callback function with the mock constructor as argument
    // Create a fresh function environment and bind parameters
    let args_vals = vec![mock_constructor];
    let func_env =
        crate::core::prepare_function_call_env(mc, Some(&callback_func.2), None, Some(&callback_func.0), &args_vals, None, None)?;
    // Execute function body
    crate::core::evaluate_statements(mc, &func_env, &callback_func.1)?;

    Ok(Value::Undefined)
    // */
    todo!("testWithIntlConstructors is not yet implemented");
}

fn handle_object_has_own_property<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // hasOwnProperty should inspect the bound `this` and take one argument
    if args.len() != 1 {
        return Err(raise_eval_error!("hasOwnProperty requires one argument"));
    }
    let key_val = args[0].clone();
    if let Some(this_rc) = crate::core::env_get(env, "this") {
        let this_val = this_rc.borrow().clone();
        match this_val {
            Value::Object(obj) => {
                let exists = has_own_property_value(mc, &obj, &key_val);
                Ok(Value::Boolean(exists))
            }
            Value::String(s) => {
                // boxed string has 'length' and indexed properties
                let key_str = match key_val {
                    Value::String(ss) => utf16_to_utf8(&ss),
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

pub fn initialize_function<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let func_ctor = new_js_object_data(mc);
    obj_set_key_value(mc, &func_ctor, &"name".into(), Value::String(utf8_to_utf16("Function")))?;
    
    let func_proto = new_js_object_data(mc);
    
    obj_set_key_value(mc, &func_ctor, &"prototype".into(), Value::Object(func_proto.clone()))?;
    obj_set_key_value(mc, &func_proto, &"constructor".into(), Value::Object(func_ctor.clone()))?;

    // Function.prototype.bind
    obj_set_key_value(mc, &func_proto, &"bind".into(), Value::Function("Function.prototype.bind".to_string()))?;

    crate::core::env_set(mc, env, "Function", Value::Object(func_ctor))?;
    Ok(())
}
