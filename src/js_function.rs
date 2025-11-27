use crate::error::JSError;
use crate::js_array::handle_array_constructor;
use crate::js_date::handle_date_constructor;
use crate::js_regexp::handle_regexp_constructor;
use crate::quickjs::{evaluate_expr, utf8_to_utf16, Expr, JSObjectDataPtr, Value};

pub fn handle_global_function(func_name: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match func_name {
        "std.sprintf" => {
            return crate::sprintf::handle_sprintf_call(env, args);
        }
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
                        if first_char == '-' || first_char == '+' || first_char.is_digit(10) {
                            end_pos = 1;
                            for ch in chars {
                                if ch.is_digit(10) {
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
            if args.len() >= 1 {
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
            if args.len() >= 1 {
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
        "Array" => {
            return handle_array_constructor(args, env);
        }
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
        "RegExp" => {
            // RegExp constructor - create a RegExp object
            handle_regexp_constructor(args, env)
        }
        "eval" => {
            // eval function - execute the code
            if args.len() >= 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                match arg_val {
                    Value::String(s) => {
                        let code = String::from_utf16_lossy(&s);
                        crate::quickjs::evaluate_script(&code)
                    }
                    _ => Ok(arg_val),
                }
            } else {
                Ok(Value::Undefined)
            }
        }
        "encodeURI" => {
            if args.len() >= 1 {
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
            if args.len() >= 1 {
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
        _ => Err(JSError::EvaluationError {
            message: format!("Unknown global function: {func_name}"),
        }),
    }
}
