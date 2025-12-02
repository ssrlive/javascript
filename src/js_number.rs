use crate::core::{Expr, JSObjectData, JSObjectDataPtr, Value, evaluate_expr, obj_get_value, obj_set_value};
use crate::error::JSError;
use crate::unicode::utf8_to_utf16;
use std::cell::RefCell;
use std::rc::Rc;

/// Create the Number object with all number constants and functions
pub fn make_number_object() -> Result<JSObjectDataPtr, JSError> {
    let number_obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(&number_obj, &"MAX_VALUE".into(), Value::Number(f64::MAX))?;
    obj_set_value(&number_obj, &"MIN_VALUE".into(), Value::Number(f64::MIN_POSITIVE))?;
    obj_set_value(&number_obj, &"NaN".into(), Value::Number(f64::NAN))?;
    obj_set_value(&number_obj, &"POSITIVE_INFINITY".into(), Value::Number(f64::INFINITY))?;
    obj_set_value(&number_obj, &"NEGATIVE_INFINITY".into(), Value::Number(f64::NEG_INFINITY))?;
    obj_set_value(&number_obj, &"EPSILON".into(), Value::Number(f64::EPSILON))?;
    obj_set_value(&number_obj, &"MAX_SAFE_INTEGER".into(), Value::Number(9007199254740991.0))?;
    obj_set_value(&number_obj, &"MIN_SAFE_INTEGER".into(), Value::Number(-9007199254740991.0))?;
    obj_set_value(&number_obj, &"isNaN".into(), Value::Function("Number.isNaN".to_string()))?;
    obj_set_value(&number_obj, &"isFinite".into(), Value::Function("Number.isFinite".to_string()))?;
    obj_set_value(&number_obj, &"isInteger".into(), Value::Function("Number.isInteger".to_string()))?;
    obj_set_value(
        &number_obj,
        &"isSafeInteger".into(),
        Value::Function("Number.isSafeInteger".to_string()),
    )?;
    obj_set_value(&number_obj, &"parseFloat".into(), Value::Function("Number.parseFloat".to_string()))?;
    obj_set_value(&number_obj, &"parseInt".into(), Value::Function("Number.parseInt".to_string()))?;
    // Create Number.prototype
    let number_prototype = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(
        &number_prototype,
        &"toString".into(),
        Value::Function("Number.prototype.toString".to_string()),
    )?;
    obj_set_value(
        &number_prototype,
        &"valueOf".into(),
        Value::Function("Number.prototype.valueOf".to_string()),
    )?;
    obj_set_value(
        &number_prototype,
        &"toLocaleString".into(),
        Value::Function("Number.prototype.toLocaleString".to_string()),
    )?;

    // Set prototype on Number constructor
    obj_set_value(&number_obj, &"prototype".into(), Value::Object(number_prototype))?;

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
                    if let Value::Number(r) = radix_val { r as u32 } else { 10 }
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

/// Handle Number instance method calls
pub fn handle_number_instance_method(n: &f64, method: &str, args: &[Expr], _env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "toString" => {
            if args.is_empty() {
                Ok(Value::String(utf8_to_utf16(&n.to_string())))
            } else {
                Err(JSError::EvaluationError {
                    message: format!("toString method expects no arguments, got {}", args.len()),
                })
            }
        }
        "valueOf" => {
            if args.is_empty() {
                Ok(Value::Number(*n))
            } else {
                Err(JSError::EvaluationError {
                    message: format!("valueOf method expects no arguments, got {}", args.len()),
                })
            }
        }
        "toLocaleString" => {
            if args.is_empty() {
                // For now, same as toString
                Ok(Value::String(utf8_to_utf16(&n.to_string())))
            } else {
                Err(JSError::EvaluationError {
                    message: format!("toLocaleString method expects no arguments, got {}", args.len()),
                })
            }
        }
        _ => Err(JSError::EvaluationError {
            message: format!("Number.prototype.{method} is not implemented"),
        }),
    }
}

/// Handle Number object method calls (for boxed Number objects)
pub fn handle_number_object_method(
    obj_map: &JSObjectDataPtr,
    method: &str,
    _args: &[Expr],
    _env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    // Handle Number instance methods
    if let Some(value_val) = crate::core::obj_get_value(obj_map, &"__value__".into())? {
        if let Value::Number(n) = *value_val.borrow() {
            match method {
                "toString" => Ok(Value::String(utf8_to_utf16(&n.to_string()))),
                "valueOf" => Ok(Value::Number(n)),
                "toLocaleString" => Ok(Value::String(utf8_to_utf16(&n.to_string()))), // For now, same as toString
                _ => Err(JSError::EvaluationError {
                    message: format!("Number.prototype.{method} is not implemented"),
                }),
            }
        } else {
            Err(JSError::EvaluationError {
                message: "Invalid __value__ for Number instance".to_string(),
            })
        }
    } else {
        Err(JSError::EvaluationError {
            message: "__value__ not found on Number instance".to_string(),
        })
    }
}

/// Box a number into a Number object and get a property
pub fn box_number_and_get_property(n: f64, prop: &str, env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Box the number into a Number object
    let number_obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(&number_obj, &"__value__".into(), Value::Number(n))?;
    // Set prototype to Number.prototype
    if let Some(number_constructor) = obj_get_value(env, &"Number".into())?
        && let Value::Object(num_ctor) = &*number_constructor.borrow()
        && let Some(proto_val) = obj_get_value(num_ctor, &"prototype".into())?
    {
        obj_set_value(&number_obj, &"__proto__".into(), proto_val.borrow().clone())?;
    }
    // Now look up the property on the boxed object
    if let Some(val) = obj_get_value(&number_obj, &prop.into())? {
        Ok(val.borrow().clone())
    } else {
        Ok(Value::Undefined)
    }
}
