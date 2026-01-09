#![allow(clippy::collapsible_if, clippy::collapsible_match)]

use crate::core::MutationContext;
use crate::core::{JSObjectDataPtr, Value, new_js_object_data, obj_get_key_value, obj_set_key_value, to_primitive};
use crate::error::JSError;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use crate::{PropertyKey, env_set};

pub fn initialize_number_module<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let number_obj = make_number_object(mc, env)?;
    env_set(mc, env, "Number", Value::Object(number_obj))?;
    Ok(())
}

/// Create the Number object with all number constants and functions
fn make_number_object<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let number_obj = new_js_object_data(mc);
    obj_set_key_value(mc, &number_obj, &"__is_constructor".into(), Value::Boolean(true))?;
    obj_set_key_value(mc, &number_obj, &"__native_ctor".into(), Value::String(utf8_to_utf16("Number")))?;

    obj_set_key_value(mc, &number_obj, &"MAX_VALUE".into(), Value::Number(f64::MAX))?;
    obj_set_key_value(mc, &number_obj, &"MIN_VALUE".into(), Value::Number(f64::MIN_POSITIVE))?;
    obj_set_key_value(mc, &number_obj, &"NaN".into(), Value::Number(f64::NAN))?;
    obj_set_key_value(mc, &number_obj, &"POSITIVE_INFINITY".into(), Value::Number(f64::INFINITY))?;
    obj_set_key_value(mc, &number_obj, &"NEGATIVE_INFINITY".into(), Value::Number(f64::NEG_INFINITY))?;
    obj_set_key_value(mc, &number_obj, &"EPSILON".into(), Value::Number(f64::EPSILON))?;
    obj_set_key_value(mc, &number_obj, &"MAX_SAFE_INTEGER".into(), Value::Number(9007199254740991.0))?;
    obj_set_key_value(mc, &number_obj, &"MIN_SAFE_INTEGER".into(), Value::Number(-9007199254740991.0))?;
    obj_set_key_value(mc, &number_obj, &"isNaN".into(), Value::Function("Number.isNaN".to_string()))?;
    obj_set_key_value(mc, &number_obj, &"isFinite".into(), Value::Function("Number.isFinite".to_string()))?;
    obj_set_key_value(
        mc,
        &number_obj,
        &"isInteger".into(),
        Value::Function("Number.isInteger".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &number_obj,
        &"isSafeInteger".into(),
        Value::Function("Number.isSafeInteger".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &number_obj,
        &"parseFloat".into(),
        Value::Function("Number.parseFloat".to_string()),
    )?;
    obj_set_key_value(mc, &number_obj, &"parseInt".into(), Value::Function("Number.parseInt".to_string()))?;

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

    // Create Number.prototype
    let number_prototype = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        number_prototype.borrow_mut(mc).prototype = Some(proto);
    }

    obj_set_key_value(
        mc,
        &number_prototype,
        &"toString".into(),
        Value::Function("Number.prototype.toString".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &number_prototype,
        &"valueOf".into(),
        Value::Function("Number.prototype.valueOf".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &number_prototype,
        &"toLocaleString".into(),
        Value::Function("Number.prototype.toLocaleString".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &number_prototype,
        &"toExponential".into(),
        Value::Function("Number.prototype.toExponential".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &number_prototype,
        &"toFixed".into(),
        Value::Function("Number.prototype.toFixed".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &number_prototype,
        &"toPrecision".into(),
        Value::Function("Number.prototype.toPrecision".to_string()),
    )?;

    // Make number prototype methods non-enumerable and mark constructor non-enumerable
    number_prototype.borrow_mut(mc).set_non_enumerable(PropertyKey::from("toString"));
    number_prototype.borrow_mut(mc).set_non_enumerable(PropertyKey::from("valueOf"));
    number_prototype
        .borrow_mut(mc)
        .set_non_enumerable(PropertyKey::from("toLocaleString"));
    number_prototype
        .borrow_mut(mc)
        .set_non_enumerable(PropertyKey::from("toExponential"));
    number_prototype.borrow_mut(mc).set_non_enumerable(PropertyKey::from("toFixed"));
    number_prototype.borrow_mut(mc).set_non_enumerable(PropertyKey::from("toPrecision"));
    number_prototype.borrow_mut(mc).set_non_enumerable(PropertyKey::from("constructor"));

    // Set prototype on Number constructor
    obj_set_key_value(mc, &number_obj, &"prototype".into(), Value::Object(number_prototype))?;

    Ok(number_obj)
}

pub(crate) fn number_constructor<'gc>(args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    // Number constructor
    if let Some(arg_val) = args.first() {
        match arg_val {
            Value::Number(n) => Ok(Value::Number(*n)),
            Value::String(s) => {
                let str_val = utf16_to_utf8(&s);
                match str_val.trim().parse::<f64>() {
                    Ok(n) => Ok(Value::Number(n)),
                    Err(_) => Ok(Value::Number(f64::NAN)),
                }
            }
            Value::Boolean(b) => Ok(Value::Number(if *b { 1.0 } else { 0.0 })),
            Value::Null => Ok(Value::Number(0.0)),
            Value::Undefined => Ok(Value::Number(f64::NAN)),
            Value::Object(obj) => {
                // Try ToPrimitive with 'number' hint
                let prim = to_primitive(&Value::Object(obj.clone()), "number", env)?;
                match prim {
                    Value::Number(n) => Ok(Value::Number(n)),
                    Value::String(s) => {
                        let str_val = utf16_to_utf8(&s);
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

/// Handle Number object method calls
pub fn handle_number_static_method<'gc>(method: &str, args: &[Value<'gc>]) -> Result<Value<'gc>, JSError> {
    match method {
        "isNaN" => {
            if let Some(arg_val) = args.first() {
                if let Value::Number(n) = arg_val {
                    Ok(Value::Boolean(n.is_nan()))
                } else {
                    Ok(Value::Boolean(false))
                }
            } else {
                Err(raise_eval_error!("Number.isNaN expects exactly one argument"))
            }
        }
        "isFinite" => {
            if let Some(arg_val) = args.first() {
                if let Value::Number(n) = arg_val {
                    Ok(Value::Boolean(n.is_finite()))
                } else {
                    Ok(Value::Boolean(false))
                }
            } else {
                Err(raise_eval_error!("Number.isFinite expects exactly one argument"))
            }
        }
        "isInteger" => {
            if let Some(arg_val) = args.first() {
                if let Value::Number(n) = arg_val {
                    Ok(Value::Boolean(n.fract() == 0.0 && n.is_finite()))
                } else {
                    Ok(Value::Boolean(false))
                }
            } else {
                Err(raise_eval_error!("Number.isInteger expects exactly one argument"))
            }
        }
        "isSafeInteger" => {
            if let Some(arg_val) = args.first() {
                if let Value::Number(n) = arg_val {
                    let is_int = n.fract() == 0.0 && n.is_finite();
                    let is_safe = (-9007199254740991.0..=9007199254740991.0).contains(n);
                    Ok(Value::Boolean(is_int && is_safe))
                } else {
                    Ok(Value::Boolean(false))
                }
            } else {
                Err(raise_eval_error!("Number.isSafeInteger expects exactly one argument"))
            }
        }
        "parseFloat" => {
            if let Some(arg_val) = args.first() {
                match arg_val {
                    Value::String(s) => {
                        let str_val = utf16_to_utf8(&s);
                        match str_val.trim().parse::<f64>() {
                            Ok(n) => Ok(Value::Number(n)),
                            Err(_) => Ok(Value::Number(f64::NAN)),
                        }
                    }
                    Value::Number(n) => Ok(Value::Number(*n)),
                    _ => Ok(Value::Number(f64::NAN)),
                }
            } else {
                Err(raise_eval_error!("Number.parseFloat expects exactly one argument"))
            }
        }
        "parseInt" => {
            if !args.is_empty() {
                let arg_val = &args[0];
                let radix = if args.len() >= 2 {
                    if let Value::Number(r) = &args[1] { *r as u32 } else { 10 }
                } else {
                    10
                };

                match arg_val {
                    Value::String(s) => {
                        let str_val = utf16_to_utf8(&s);
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
                Err(raise_eval_error!("Number.parseInt expects at least one argument"))
            }
        }
        _ => Err(raise_eval_error!(format!("Number.{method} is not implemented"))),
    }
}

/// Handle Number instance method calls
pub fn handle_number_instance_method<'gc>(n: &f64, method: &str, args: &[Value<'gc>]) -> Result<Value<'gc>, JSError> {
    match method {
        "toString" => {
            if args.is_empty() {
                Ok(Value::String(utf8_to_utf16(&n.to_string())))
            } else {
                let msg = format!("toString method expects no arguments, got {}", args.len());
                Err(raise_eval_error!(msg))
            }
        }
        "valueOf" => {
            if args.is_empty() {
                Ok(Value::Number(*n))
            } else {
                Err(raise_eval_error!(format!(
                    "valueOf method expects no arguments, got {}",
                    args.len()
                )))
            }
        }
        "toLocaleString" => {
            if args.is_empty() {
                // For now, same as toString
                Ok(Value::String(utf8_to_utf16(&n.to_string())))
            } else {
                let msg = format!("toLocaleString method expects no arguments, got {}", args.len());
                Err(raise_eval_error!(msg))
            }
        }
        "toExponential" => {
            let fraction_digits = if !args.is_empty() {
                match &args[0] {
                    Value::Number(d) => Some(*d as usize),
                    _ => None,
                }
            } else {
                None
            };

            match fraction_digits {
                Some(d) => Ok(Value::String(utf8_to_utf16(&format!("{:.1$e}", n, d)))),
                None => Ok(Value::String(utf8_to_utf16(&format!("{:e}", n)))),
            }
        }
        "toFixed" => {
            let digits = if !args.is_empty() {
                match &args[0] {
                    Value::Number(d) => *d as usize,
                    _ => 0,
                }
            } else {
                0
            };

            if digits > 100 {
                return Err(raise_eval_error!("toFixed() digits argument must be between 0 and 100"));
            }

            Ok(Value::String(utf8_to_utf16(&format!("{:.1$}", n, digits))))
        }
        "toPrecision" => {
            let precision = if !args.is_empty() {
                match &args[0] {
                    Value::Number(p) => Some(*p as usize),
                    Value::Undefined => None,
                    _ => None,
                }
            } else {
                None
            };

            match precision {
                Some(p) => {
                    if !(1..=100).contains(&p) {
                        return Err(raise_eval_error!("toPrecision() argument must be between 1 and 100"));
                    }

                    if n.is_nan() || n.is_infinite() {
                        return Ok(Value::String(utf8_to_utf16(&n.to_string())));
                    }

                    // Format in exponential to get the exponent
                    let s_exp = format!("{:.1$e}", n, p - 1);
                    let parts: Vec<&str> = s_exp.split('e').collect();
                    let exponent: i32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);

                    if exponent < -6 || exponent >= p as i32 {
                        Ok(Value::String(utf8_to_utf16(&s_exp)))
                    } else {
                        // Use fixed notation
                        // digits after decimal = p - 1 - exponent
                        let digits_after_decimal = p as i32 - 1 - exponent;
                        let width = if digits_after_decimal < 0 {
                            0
                        } else {
                            digits_after_decimal as usize
                        };
                        Ok(Value::String(utf8_to_utf16(&format!("{:.1$}", n, width))))
                    }
                }
                None => Ok(Value::String(utf8_to_utf16(&n.to_string()))),
            }
        }
        _ => Err(raise_eval_error!(format!("Number.prototype.{method} is not implemented"))),
    }
}

/// Handle Number prototype method calls
pub fn handle_number_prototype_method<'gc>(this_val: Option<Value<'gc>>, method: &str, args: &[Value<'gc>]) -> Result<Value<'gc>, JSError> {
    if let Some(Value::Number(n)) = this_val {
        handle_number_instance_method(&n, method, args)
    } else if let Some(Value::Object(obj)) = this_val {
        if let Some(val) = obj_get_key_value(&obj, &"__value__".into())? {
            if let Value::Number(n) = &*val.borrow() {
                handle_number_instance_method(n, method, args)
            } else {
                Err(raise_eval_error!("TypeError: Number.prototype method called on non-number object"))
            }
        } else {
            Err(raise_eval_error!(
                "TypeError: Number.prototype method called on incompatible receiver"
            ))
        }
    } else {
        Err(raise_eval_error!("TypeError: Number.prototype method called on non-number"))
    }
}
