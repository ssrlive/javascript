use crate::core::{Expr, JSObjectDataPtr, Value, env_set, evaluate_expr, to_primitive, value_to_string};
use crate::error::JSError;
use crate::js_array::handle_array_constructor;
use crate::js_date::handle_date_constructor;
use crate::utf16::utf8_to_utf16;
use std::rc::Rc;

/// Helper function to extract and validate arguments for internal functions
/// Returns a vector of evaluated arguments or an error
fn extract_internal_args(args: &[Expr], env: &JSObjectDataPtr, expected_count: usize) -> Result<Vec<Value>, JSError> {
    if args.len() != expected_count {
        return Err(JSError::TypeError {
            message: format!(
                "Internal function requires exactly {} arguments, got {}",
                expected_count,
                args.len()
            ),
        });
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
        return Err(JSError::TypeError {
            message: format!("Expected at least {} arguments", count),
        });
    }

    let mut numbers = Vec::with_capacity(count);
    for i in 0..count {
        match args[i] {
            Value::Number(n) => numbers.push(n),
            _ => {
                return Err(JSError::TypeError {
                    message: format!("Argument {} must be a number", i),
                });
            }
        }
    }
    Ok(numbers)
}

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
                    Value::Object(obj) => {
                        // Attempt ToPrimitive with 'string' hint first (honor [Symbol.toPrimitive] or fallback)
                        let prim = to_primitive(&Value::Object(obj.clone()), "string")?;
                        match prim {
                            Value::String(s) => Ok(Value::String(s)),
                            Value::Number(n) => Ok(Value::String(utf8_to_utf16(&n.to_string()))),
                            Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
                            Value::Symbol(sd) => match sd.description {
                                Some(ref d) => Ok(Value::String(utf8_to_utf16(&format!("Symbol({})", d)))),
                                None => Ok(Value::String(utf8_to_utf16("Symbol()"))),
                            },
                            _ => Ok(Value::String(utf8_to_utf16("[object Object]"))),
                        }
                    }
                    Value::Function(name) => Ok(Value::String(utf8_to_utf16(&format!("[Function: {name}]")))),
                    Value::Closure(_, _, _) => Ok(Value::String(utf8_to_utf16("[Function]"))),
                    Value::ClassDefinition(_) => Ok(Value::String(utf8_to_utf16("[Class]"))),
                    Value::Getter(_, _) => Ok(Value::String(utf8_to_utf16("[Getter]"))),
                    Value::Setter(_, _, _) => Ok(Value::String(utf8_to_utf16("[Setter]"))),
                    Value::Property { .. } => Ok(Value::String(utf8_to_utf16("[property]"))),
                    Value::Promise(_) => Ok(Value::String(utf8_to_utf16("[object Promise]"))),
                    Value::Symbol(symbol_data) => match &symbol_data.description {
                        Some(d) => Ok(Value::String(utf8_to_utf16(&format!("Symbol({d})")))),
                        None => Ok(Value::String(utf8_to_utf16("Symbol()"))),
                    },
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
                    Value::Object(obj) => {
                        // Try ToPrimitive with 'number' hint
                        let prim = to_primitive(&Value::Object(obj.clone()), "number")?;
                        match prim {
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
                    }
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
        "Symbol" => {
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
        "new" => {
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
                        return Err(JSError::EvaluationError {
                            message: format!("Constructor {constructor_name} not implemented"),
                        });
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
        "__internal_resolve_promise" => {
            // Internal function to resolve a promise - requires 2 args: (promise, value)
            let args = extract_internal_args(args, env, 2)?;
            log::trace!("__internal_resolve_promise called with value: {:?}", args[1]);

            match &args[0] {
                Value::Promise(promise) => {
                    crate::js_promise::resolve_promise(promise, args[1].clone());
                    Ok(Value::Undefined)
                }
                _ => Err(JSError::TypeError {
                    message: "First argument must be a promise".to_string(),
                }),
            }
        }
        "__internal_reject_promise" => {
            // Internal function to reject a promise - requires 2 args: (promise, reason)
            let args = extract_internal_args(args, env, 2)?;
            log::trace!("__internal_reject_promise called with reason: {:?}", args[1]);

            match &args[0] {
                Value::Promise(promise) => {
                    crate::js_promise::reject_promise(promise, args[1].clone());
                    Ok(Value::Undefined)
                }
                _ => Err(JSError::TypeError {
                    message: "First argument must be a promise".to_string(),
                }),
            }
        }
        "__internal_promise_allsettled_resolve" => {
            // Internal function for legacy allSettled - requires 3 args: (idx, value, shared_state)
            let args = extract_internal_args(args, env, 3)?;
            let numbers = validate_number_args(&args, 1)?;
            crate::js_promise::__internal_promise_allsettled_resolve(numbers[0], args[1].clone(), args[2].clone())?;
            Ok(Value::Undefined)
        }
        "__internal_promise_allsettled_reject" => {
            // Internal function for legacy allSettled - requires 3 args: (idx, reason, shared_state)
            let args = extract_internal_args(args, env, 3)?;
            let numbers = validate_number_args(&args, 1)?;
            crate::js_promise::__internal_promise_allsettled_reject(numbers[0], args[1].clone(), args[2].clone())?;
            Ok(Value::Undefined)
        }
        "__internal_allsettled_state_record_fulfilled" => {
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
        "__internal_allsettled_state_record_rejected" => {
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
        "__internal_promise_any_resolve" => {
            // Internal function for Promise.any resolve - requires 2 args: (value, result_promise)
            let args = extract_internal_args(args, env, 2)?;
            match &args[1] {
                Value::Promise(result_promise) => {
                    crate::js_promise::__internal_promise_any_resolve(args[0].clone(), result_promise.clone());
                    Ok(Value::Undefined)
                }
                _ => Err(JSError::TypeError {
                    message: "Second argument must be a promise".to_string(),
                }),
            }
        }
        "__internal_promise_any_reject" => {
            // Internal function for Promise.any reject - requires 6 args: (idx, reason, rejections, rejected_count, total, result_promise)
            // Note: This function has complex Rc<RefCell<>> parameters that cannot be easily reconstructed from JS values
            // It should only be called from within closures, not directly
            Err(JSError::TypeError {
                message: "__internal_promise_any_reject cannot be called directly - use Promise.any instead".to_string(),
            })
        }
        "__internal_promise_race_resolve" => {
            // Internal function for Promise.race resolve - requires 2 args: (value, result_promise)
            let args = extract_internal_args(args, env, 2)?;
            match &args[1] {
                Value::Promise(result_promise) => {
                    crate::js_promise::__internal_promise_race_resolve(args[0].clone(), result_promise.clone());
                    Ok(Value::Undefined)
                }
                _ => Err(JSError::TypeError {
                    message: "Second argument must be a promise".to_string(),
                }),
            }
        }
        "__internal_promise_all_resolve" => {
            // Internal function for Promise.all resolve - requires 3 args: (idx, value, state)
            let args = extract_internal_args(args, env, 3)?;
            let numbers = validate_number_args(&args, 1)?;
            let idx = numbers[0] as usize;
            let value = args[1].clone();
            if let Value::Object(state_obj) = args[2].clone() {
                // Store value in results[idx]
                if let Some(results_val_rc) = crate::core::obj_get_value(&state_obj, &"results".into())?
                    && let Value::Object(results_obj) = &*results_val_rc.borrow()
                {
                    crate::core::obj_set_value(results_obj, &idx.to_string().into(), value)?;
                }
                // Increment completed
                if let Some(completed_val_rc) = crate::core::obj_get_value(&state_obj, &"completed".into())?
                    && let Value::Number(completed) = &*completed_val_rc.borrow()
                {
                    let new_completed = completed + 1.0;
                    crate::core::obj_set_value(&state_obj, &"completed".into(), Value::Number(new_completed))?;
                    // Check if all completed
                    if let Some(total_val_rc) = crate::core::obj_get_value(&state_obj, &"total".into())?
                        && let Value::Number(total) = &*total_val_rc.borrow()
                        && new_completed == *total
                    {
                        // Resolve result_promise with results array
                        if let Some(promise_val_rc) = crate::core::obj_get_value(&state_obj, &"result_promise".into())?
                            && let Value::Promise(result_promise) = &*promise_val_rc.borrow()
                            && let Some(results_val_rc) = crate::core::obj_get_value(&state_obj, &"results".into())?
                            && let Value::Object(results_obj) = &*results_val_rc.borrow()
                        {
                            crate::js_promise::resolve_promise(result_promise, Value::Object(results_obj.clone()));
                        }
                    }
                }
            }
            Ok(Value::Undefined)
        }
        "__internal_promise_all_reject" => {
            // Internal function for Promise.all reject - requires 2 args: (reason, state)
            let args = extract_internal_args(args, env, 2)?;
            let reason = args[0].clone();
            if let Value::Object(state_obj) = args[1].clone() {
                // Reject result_promise
                if let Some(promise_val_rc) = crate::core::obj_get_value(&state_obj, &"result_promise".into())?
                    && let Value::Promise(result_promise) = &*promise_val_rc.borrow()
                {
                    crate::js_promise::reject_promise(result_promise, reason);
                }
            }
            Ok(Value::Undefined)
        }
        "testWithIntlConstructors" => {
            // testWithIntlConstructors function - used for testing Intl constructors
            if args.len() != 1 {
                return Err(JSError::TypeError {
                    message: "testWithIntlConstructors requires exactly 1 argument".to_string(),
                });
            }
            let callback = evaluate_expr(env, &args[0])?;
            let callback_func = match callback {
                Value::Closure(params, body, captured_env) => (params, body, captured_env),
                _ => {
                    return Err(JSError::TypeError {
                        message: "testWithIntlConstructors requires a function as argument".to_string(),
                    });
                }
            };

            // Create a mock constructor
            let mock_constructor = crate::js_testintl::create_mock_intl_constructor()?;

            // Call the callback function with the mock constructor as argument
            // Create new environment starting with captured environment
            let func_env = callback_func.2.clone();
            // Bind the mock constructor to the first parameter
            if !callback_func.0.is_empty() {
                env_set(&func_env, &callback_func.0[0], mock_constructor)?;
            }
            // Execute function body
            crate::core::evaluate_statements(&func_env, &callback_func.1)?;

            Ok(Value::Undefined)
        }

        _ => Err(JSError::EvaluationError {
            message: format!("Global function {} is not implemented", func_name),
        }),
    }
}
