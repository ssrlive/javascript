use crate::core::{evaluate_expr, obj_set_value, Expr, JSObjectData, JSObjectDataPtr, Value};
use crate::error::JSError;
use std::cell::RefCell;
use std::rc::Rc;

/// Create the Number object with all number constants and functions
pub fn make_number_object() -> Result<JSObjectDataPtr, JSError> {
    let number_obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(&number_obj, "MAX_VALUE", Value::Number(f64::MAX))?;
    obj_set_value(&number_obj, "MIN_VALUE", Value::Number(f64::MIN_POSITIVE))?;
    obj_set_value(&number_obj, "NaN", Value::Number(f64::NAN))?;
    obj_set_value(&number_obj, "POSITIVE_INFINITY", Value::Number(f64::INFINITY))?;
    obj_set_value(&number_obj, "NEGATIVE_INFINITY", Value::Number(f64::NEG_INFINITY))?;
    obj_set_value(&number_obj, "EPSILON", Value::Number(f64::EPSILON))?;
    obj_set_value(&number_obj, "MAX_SAFE_INTEGER", Value::Number(9007199254740991.0))?;
    obj_set_value(&number_obj, "MIN_SAFE_INTEGER", Value::Number(-9007199254740991.0))?;
    obj_set_value(&number_obj, "isNaN", Value::Function("Number.isNaN".to_string()))?;
    obj_set_value(&number_obj, "isFinite", Value::Function("Number.isFinite".to_string()))?;
    obj_set_value(&number_obj, "isInteger", Value::Function("Number.isInteger".to_string()))?;
    obj_set_value(&number_obj, "isSafeInteger", Value::Function("Number.isSafeInteger".to_string()))?;
    obj_set_value(&number_obj, "parseFloat", Value::Function("Number.parseFloat".to_string()))?;
    obj_set_value(&number_obj, "parseInt", Value::Function("Number.parseInt".to_string()))?;
    Ok(number_obj)
}

/// Handle Number object method calls
pub fn handle_number_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "isNaN" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = arg_val {
                    Ok(Value::Boolean(n.is_nan()))
                } else {
                    Ok(Value::Boolean(false))
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Number.isNaN expects exactly one argument".to_string(),
                })
            }
        }
        "isFinite" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = arg_val {
                    Ok(Value::Boolean(n.is_finite()))
                } else {
                    Ok(Value::Boolean(false))
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Number.isFinite expects exactly one argument".to_string(),
                })
            }
        }
        "isInteger" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = arg_val {
                    Ok(Value::Boolean(n.fract() == 0.0 && n.is_finite()))
                } else {
                    Ok(Value::Boolean(false))
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Number.isInteger expects exactly one argument".to_string(),
                })
            }
        }
        "isSafeInteger" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = arg_val {
                    let is_int = n.fract() == 0.0 && n.is_finite();
                    let is_safe = (-9007199254740991.0..=9007199254740991.0).contains(&n);
                    Ok(Value::Boolean(is_int && is_safe))
                } else {
                    Ok(Value::Boolean(false))
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Number.isSafeInteger expects exactly one argument".to_string(),
                })
            }
        }
        "parseFloat" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                match arg_val {
                    Value::String(s) => {
                        let str_val = String::from_utf16_lossy(&s);
                        match str_val.trim().parse::<f64>() {
                            Ok(n) => Ok(Value::Number(n)),
                            Err(_) => Ok(Value::Number(f64::NAN)),
                        }
                    }
                    Value::Number(n) => Ok(Value::Number(n)),
                    _ => Ok(Value::Number(f64::NAN)),
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Number.parseFloat expects exactly one argument".to_string(),
                })
            }
        }
        "parseInt" => {
            if !args.is_empty() {
                let arg_val = evaluate_expr(env, &args[0])?;
                let radix = if args.len() >= 2 {
                    let radix_val = evaluate_expr(env, &args[1])?;
                    if let Value::Number(r) = radix_val {
                        r as u32
                    } else {
                        10
                    }
                } else {
                    10
                };

                match arg_val {
                    Value::String(s) => {
                        let str_val = String::from_utf16_lossy(&s);
                        let trimmed = str_val.trim();
                        if trimmed.is_empty() {
                            Ok(Value::Number(f64::NAN))
                        } else {
                            // For parseInt, we need to parse only the integer part
                            // Find the first non-digit character (considering radix)
                            let mut end_pos = 0;
                            let chars: Vec<char> = trimmed.chars().collect();

                            // Handle sign
                            let mut start_pos = 0;
                            if !chars.is_empty() && (chars[0] == '+' || chars[0] == '-') {
                                start_pos = 1;
                            }

                            // Parse digits based on radix
                            for (i, &c) in chars.iter().enumerate().skip(start_pos) {
                                let digit_val = match c {
                                    '0'..='9' => (c as u32) - ('0' as u32),
                                    'a'..='z' => (c as u32) - ('a' as u32) + 10,
                                    'A'..='Z' => (c as u32) - ('A' as u32) + 10,
                                    _ => break,
                                };

                                if digit_val >= radix {
                                    break;
                                }
                                end_pos = i + 1;
                            }

                            if end_pos > start_pos || (start_pos == 1 && end_pos >= 1) {
                                let int_part = &trimmed[..end_pos];
                                if radix == 10 {
                                    match int_part.parse::<i64>() {
                                        Ok(n) => Ok(Value::Number(n as f64)),
                                        Err(_) => Ok(Value::Number(f64::NAN)),
                                    }
                                } else {
                                    match i64::from_str_radix(int_part, radix) {
                                        Ok(n) => Ok(Value::Number(n as f64)),
                                        Err(_) => Ok(Value::Number(f64::NAN)),
                                    }
                                }
                            } else {
                                Ok(Value::Number(f64::NAN))
                            }
                        }
                    }
                    Value::Number(n) => Ok(Value::Number(n.trunc())),
                    _ => Ok(Value::Number(f64::NAN)),
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Number.parseInt expects at least one argument".to_string(),
                })
            }
        }
        _ => Err(JSError::EvaluationError {
            message: format!("Number.{method} is not implemented"),
        }),
    }
}
