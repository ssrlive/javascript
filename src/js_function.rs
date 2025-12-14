use crate::core::{Expr, JSObjectDataPtr, Value, env_set, evaluate_expr, value_to_string};
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
        "String" => crate::js_string::string_constructor(args, env),
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
        "Date" => handle_date_constructor(args, env),
        "Symbol" => symbol_constructor(args, env),
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
        "testWithIntlConstructors" => test_with_intl_constructors(args, env),
        "setTimeout" => crate::js_promise::handle_set_timeout(args, env),
        "clearTimeout" => crate::js_promise::handle_clear_timeout(args, env),

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

fn parse_int_function(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        return Err(raise_type_error!("parseInt requires at least one argument"));
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
            if let Some(first_char) = chars.next()
                && (first_char == '-' || first_char == '+' || first_char.is_ascii_digit())
            {
                end_pos = 1;
                for ch in chars {
                    if ch.is_ascii_digit() {
                        end_pos += 1;
                    } else {
                        break;
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
                Value::AsyncClosure(_, _, _) => "[Function]".to_string(),
                _ => unreachable!(), // All cases covered above
            };
            match str_val.parse::<i32>() {
                Ok(n) => Ok(Value::Number(n as f64)),
                Err(_) => Ok(Value::Number(f64::NAN)),
            }
        }
    }
}

fn parse_float_function(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        return Err(raise_type_error!("parseFloat requires at least one argument"));
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
                Value::AsyncClosure(_, _, _) => "[Function]".to_string(),
                _ => unreachable!(), // All cases covered above
            };
            match str_val.parse::<f64>() {
                Ok(n) => Ok(Value::Number(n)),
                Err(_) => Ok(Value::Number(f64::NAN)),
            }
        }
    }
}

fn is_nan_function(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        return Err(raise_type_error!("isNaN requires at least one argument"));
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

fn is_finite_function(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        return Err(raise_type_error!("isFinite requires at least one argument"));
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

fn encode_uri_component(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn decode_uri_component(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn boolean_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn symbol_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Symbol constructor - creates a unique symbol
    if args.len() == 1 {
        let arg_val = evaluate_expr(env, &args[0])?;
        let description = match arg_val {
            Value::String(s) => Some(String::from_utf16_lossy(&s)),
            Value::Undefined => None,
            _ => Some(value_to_string(&arg_val)),
        };
        let symbol_data = Rc::new(crate::core::SymbolData { description });
        Ok(Value::Symbol(symbol_data))
    } else {
        let symbol_data = Rc::new(crate::core::SymbolData { description: None });
        Ok(Value::Symbol(symbol_data)) // Symbol() with no args creates symbol with no description
    }
}

fn evaluate_new_expression(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Handle new expressions: new Constructor(args)
    if args.len() == 1
        && let Expr::Call(constructor_expr, constructor_args) = &args[0]
        && let Expr::Var(constructor_name) = &**constructor_expr
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
                crate::core::evaluate_script(&code, None::<&std::path::Path>) // Evaluate in global context
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
        Value::Closure(params, body, captured_env) | Value::AsyncClosure(params, body, captured_env) => (params, body, captured_env),
        _ => {
            return Err(raise_type_error!("testWithIntlConstructors requires a function as argument"));
        }
    };

    // Create a mock constructor
    let mock_constructor = crate::js_testintl::create_mock_intl_constructor()?;

    // Call the callback function with the mock constructor as argument
    // Create new environment starting with captured environment
    let func_env = callback_func.2.clone();
    // Bind the mock constructor to the first parameter
    if !callback_func.0.is_empty() {
        let name = &callback_func.0[0].0;
        env_set(&func_env, name.as_str(), mock_constructor)?;
    }
    // Execute function body
    crate::core::evaluate_statements(&func_env, &callback_func.1)?;

    Ok(Value::Undefined)
}
