#![allow(clippy::collapsible_if, clippy::collapsible_match)]

use crate::core::MutationContext;
use crate::core::{JSObjectDataPtr, Value, new_js_object_data, object_get_key_value, object_set_key_value, to_primitive};
use crate::env_set;
use crate::error::JSError;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};

pub fn initialize_number_module<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let number_obj = make_number_object(mc, env)?;
    env_set(mc, env, "Number", Value::Object(number_obj))?;
    Ok(())
}

/// Create the Number object with all number constants and functions
fn make_number_object<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let number_obj = new_js_object_data(mc);
    object_set_key_value(mc, &number_obj, "__is_constructor", Value::Boolean(true))?;
    object_set_key_value(mc, &number_obj, "__native_ctor", Value::String(utf8_to_utf16("Number")))?;

    object_set_key_value(mc, &number_obj, "MAX_VALUE", Value::Number(f64::MAX))?;
    object_set_key_value(mc, &number_obj, "MIN_VALUE", Value::Number(f64::from_bits(1)))?;
    object_set_key_value(mc, &number_obj, "NaN", Value::Number(f64::NAN))?;
    object_set_key_value(mc, &number_obj, "POSITIVE_INFINITY", Value::Number(f64::INFINITY))?;
    object_set_key_value(mc, &number_obj, "NEGATIVE_INFINITY", Value::Number(f64::NEG_INFINITY))?;
    object_set_key_value(mc, &number_obj, "EPSILON", Value::Number(f64::EPSILON))?;
    object_set_key_value(mc, &number_obj, "MAX_SAFE_INTEGER", Value::Number(9007199254740991.0))?;
    object_set_key_value(mc, &number_obj, "MIN_SAFE_INTEGER", Value::Number(-9007199254740991.0))?;
    object_set_key_value(mc, &number_obj, "isNaN", Value::Function("Number.isNaN".to_string()))?;
    object_set_key_value(mc, &number_obj, "isFinite", Value::Function("Number.isFinite".to_string()))?;
    object_set_key_value(mc, &number_obj, "isInteger", Value::Function("Number.isInteger".to_string()))?;
    object_set_key_value(
        mc,
        &number_obj,
        "isSafeInteger",
        Value::Function("Number.isSafeInteger".to_string()),
    )?;
    object_set_key_value(mc, &number_obj, "parseFloat", Value::Function("Number.parseFloat".to_string()))?;
    object_set_key_value(mc, &number_obj, "parseInt", Value::Function("Number.parseInt".to_string()))?;

    // Make static Number properties non-enumerable
    number_obj.borrow_mut(mc).set_non_enumerable("MAX_VALUE");
    number_obj.borrow_mut(mc).set_non_enumerable("MIN_VALUE");
    number_obj.borrow_mut(mc).set_non_enumerable("NaN");
    number_obj.borrow_mut(mc).set_non_enumerable("POSITIVE_INFINITY");
    number_obj.borrow_mut(mc).set_non_enumerable("NEGATIVE_INFINITY");
    number_obj.borrow_mut(mc).set_non_enumerable("EPSILON");
    number_obj.borrow_mut(mc).set_non_enumerable("MAX_SAFE_INTEGER");
    number_obj.borrow_mut(mc).set_non_enumerable("MIN_SAFE_INTEGER");
    number_obj.borrow_mut(mc).set_non_enumerable("isNaN");
    number_obj.borrow_mut(mc).set_non_enumerable("isFinite");
    number_obj.borrow_mut(mc).set_non_enumerable("isInteger");
    number_obj.borrow_mut(mc).set_non_enumerable("isSafeInteger");
    number_obj.borrow_mut(mc).set_non_enumerable("parseFloat");
    number_obj.borrow_mut(mc).set_non_enumerable("parseInt");

    // Per ECMAScript spec, the numeric constants on Number are non-writable and non-configurable
    number_obj.borrow_mut(mc).set_non_writable("MAX_VALUE");
    number_obj.borrow_mut(mc).set_non_configurable("MAX_VALUE");

    number_obj.borrow_mut(mc).set_non_writable("MIN_VALUE");
    number_obj.borrow_mut(mc).set_non_configurable("MIN_VALUE");
    number_obj.borrow_mut(mc).set_non_writable("NaN");
    number_obj.borrow_mut(mc).set_non_configurable("NaN");

    number_obj.borrow_mut(mc).set_non_writable("POSITIVE_INFINITY");
    number_obj.borrow_mut(mc).set_non_configurable("POSITIVE_INFINITY");

    number_obj.borrow_mut(mc).set_non_writable("NEGATIVE_INFINITY");
    number_obj.borrow_mut(mc).set_non_configurable("NEGATIVE_INFINITY");

    number_obj.borrow_mut(mc).set_non_writable("EPSILON");
    number_obj.borrow_mut(mc).set_non_configurable("EPSILON");

    number_obj.borrow_mut(mc).set_non_writable("MAX_SAFE_INTEGER");
    number_obj.borrow_mut(mc).set_non_configurable("MAX_SAFE_INTEGER");

    number_obj.borrow_mut(mc).set_non_writable("MIN_SAFE_INTEGER");
    number_obj.borrow_mut(mc).set_non_configurable("MIN_SAFE_INTEGER");

    // Internal markers and prototype should not be enumerable
    number_obj.borrow_mut(mc).set_non_enumerable("__is_constructor");
    number_obj.borrow_mut(mc).set_non_enumerable("__native_ctor");
    number_obj.borrow_mut(mc).set_non_enumerable("prototype");

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

    // Create Number.prototype
    let number_prototype = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        number_prototype.borrow_mut(mc).prototype = Some(proto);
    }

    object_set_key_value(
        mc,
        &number_prototype,
        "toString",
        Value::Function("Number.prototype.toString".to_string()),
    )?;
    object_set_key_value(
        mc,
        &number_prototype,
        "valueOf",
        Value::Function("Number.prototype.valueOf".to_string()),
    )?;
    object_set_key_value(
        mc,
        &number_prototype,
        "toLocaleString",
        Value::Function("Number.prototype.toLocaleString".to_string()),
    )?;
    object_set_key_value(
        mc,
        &number_prototype,
        "toExponential",
        Value::Function("Number.prototype.toExponential".to_string()),
    )?;
    object_set_key_value(
        mc,
        &number_prototype,
        "toFixed",
        Value::Function("Number.prototype.toFixed".to_string()),
    )?;
    object_set_key_value(
        mc,
        &number_prototype,
        "toPrecision",
        Value::Function("Number.prototype.toPrecision".to_string()),
    )?;

    // Make number prototype methods non-enumerable and mark constructor non-enumerable
    number_prototype.borrow_mut(mc).set_non_enumerable("toString");
    number_prototype.borrow_mut(mc).set_non_enumerable("valueOf");
    number_prototype.borrow_mut(mc).set_non_enumerable("toLocaleString");
    number_prototype.borrow_mut(mc).set_non_enumerable("toExponential");
    number_prototype.borrow_mut(mc).set_non_enumerable("toFixed");
    number_prototype.borrow_mut(mc).set_non_enumerable("toPrecision");
    number_prototype.borrow_mut(mc).set_non_enumerable("constructor");

    // Set prototype on Number constructor
    object_set_key_value(mc, &number_obj, "prototype", Value::Object(number_prototype))?;

    // Ensure Number.prototype.constructor points back to Number
    if let Some(proto_val) = object_get_key_value(&number_obj, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        object_set_key_value(mc, proto_obj, "constructor", Value::Object(number_obj))?;
        // Non-enumerable already set above, but ensure it's non-enumerable
        proto_obj.borrow_mut(mc).set_non_enumerable("constructor");
    }

    Ok(number_obj)
}

pub(crate) fn number_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Number constructor
    if let Some(arg_val) = args.first() {
        match arg_val {
            Value::Number(n) => Ok(Value::Number(*n)),
            Value::String(s) => {
                let str_val = utf16_to_utf8(s);
                Ok(Value::Number(string_to_f64(str_val.trim()).unwrap_or(f64::NAN)))
            }
            Value::Boolean(b) => Ok(Value::Number(if *b { 1.0 } else { 0.0 })),
            Value::Null => Ok(Value::Number(0.0)),
            Value::Undefined => Ok(Value::Number(f64::NAN)),
            Value::Object(obj) => {
                // Try ToPrimitive with 'number' hint
                let prim = to_primitive(mc, &Value::Object(*obj), "number", env)?;
                match prim {
                    Value::Number(n) => Ok(Value::Number(n)),
                    Value::String(s) => {
                        let str_val = utf16_to_utf8(&s);
                        Ok(Value::Number(string_to_f64(str_val.trim()).unwrap_or(f64::NAN)))
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

pub(crate) fn string_to_f64(s: &str) -> Result<f64, JSError> {
    let trimmed = s.trim();
    if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        Ok(i64::from_str_radix(hex, 16).map(|v| v as f64)?)
    } else if let Some(bin) = trimmed.strip_prefix("0b").or_else(|| trimmed.strip_prefix("0B")) {
        Ok(i64::from_str_radix(bin, 2).map(|v| v as f64)?)
    } else if let Some(oct) = trimmed.strip_prefix("0o").or_else(|| trimmed.strip_prefix("0O")) {
        Ok(i64::from_str_radix(oct, 8).map(|v| v as f64)?)
    } else {
        Ok(trimmed.parse::<f64>()?)
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
                        let str_val = utf16_to_utf8(s);
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
                        let str_val = utf16_to_utf8(s);
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
                // Use canonical JS string conversion for numbers
                Ok(Value::String(utf8_to_utf16(&crate::core::value_to_string(&Value::Number(*n)))))
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
                Ok(Value::String(utf8_to_utf16(&crate::core::value_to_string(&Value::Number(*n)))))
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
                None => Ok(Value::String(utf8_to_utf16(&crate::core::value_to_string(&Value::Number(*n))))),
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
        if let Some(val) = object_get_key_value(&obj, "__value__") {
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
