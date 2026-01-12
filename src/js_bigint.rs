use crate::PropertyKey;
use crate::core::MutationContext;
use crate::core::{JSObjectDataPtr, Value, env_set, new_js_object_data, obj_get_key_value, obj_set_key_value, to_primitive};
use crate::error::JSError;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use num_bigint::BigInt;
use num_bigint::Sign;

pub(crate) fn bigint_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // BigInt(value) conversion per simplified rules:
    if args.len() != 1 {
        return Err(raise_type_error!("BigInt requires exactly one argument"));
    }
    let arg_val = &args[0];
    match arg_val {
        Value::BigInt(b) => Ok(Value::BigInt(b.clone())),
        Value::Number(n) => {
            if n.is_nan() || !n.is_finite() || n.fract() != 0.0 {
                return Err(raise_type_error!("Cannot convert number to BigInt"));
            }
            Ok(Value::BigInt(BigInt::from(*n as i64)))
        }
        Value::String(s) => {
            let st = utf16_to_utf8(&s);
            Ok(Value::BigInt(parse_bigint_string(&st)?))
        }
        Value::Boolean(b) => {
            let bigint = if *b { BigInt::from(1) } else { BigInt::from(0) };
            Ok(Value::BigInt(bigint))
        }
        Value::Object(obj) => {
            // Try ToPrimitive with number hint first
            let prim = to_primitive(mc, &Value::Object(obj.clone()), "number", env)?;
            match prim {
                Value::Number(n) => {
                    if n.is_nan() || !n.is_finite() || n.fract() != 0.0 {
                        return Err(raise_type_error!("Cannot convert number to BigInt"));
                    }
                    Ok(Value::BigInt(BigInt::from(n as i64)))
                }
                Value::String(s) => {
                    let st = utf16_to_utf8(&s);
                    Ok(Value::BigInt(parse_bigint_string(&st)?))
                }
                Value::BigInt(b) => Ok(Value::BigInt(b)),
                _ => Err(raise_type_error!("Cannot convert object to BigInt")),
            }
        }
        _ => Err(raise_type_error!("Cannot convert value to BigInt")),
    }
}

/// Handle BigInt object methods (toString, valueOf)
pub fn handle_bigint_object_method<'gc>(this_val: Value<'gc>, method: &str, args: &[Value<'gc>]) -> Result<Value<'gc>, JSError> {
    let h = match this_val {
        Value::BigInt(b) => b,
        Value::Object(obj) => {
            if let Some(value_val) = obj_get_key_value(&obj, &"__value__".into())? {
                if let Value::BigInt(h) = &*value_val.borrow() {
                    h.clone()
                } else {
                    return Err(raise_type_error!("BigInt.prototype method called on incompatible object"));
                }
            } else {
                return Err(raise_type_error!("BigInt.prototype method called on incompatible object"));
            }
        }
        _ => return Err(raise_type_error!("BigInt.prototype method called on incompatible receiver")),
    };

    match method {
        "toString" => {
            if args.is_empty() {
                return Ok(Value::String(utf8_to_utf16(&h.to_string())));
            } else {
                // radix support: expect a number argument
                let arg0 = &args[0];
                if let Value::Number(rad) = arg0 {
                    let r = *rad as i32;
                    if !(2..=36).contains(&r) {
                        return Err(raise_eval_error!("toString() radix out of range"));
                    }
                    let s = h.to_str_radix(r as u32);
                    return Ok(Value::String(utf8_to_utf16(&s)));
                } else {
                    return Err(raise_eval_error!("toString radix must be a number"));
                }
            }
        }
        "valueOf" => {
            if args.is_empty() {
                return Ok(Value::BigInt(h.clone()));
            } else {
                return Err(raise_eval_error!("valueOf expects no arguments"));
            }
        }
        _ => return Err(raise_eval_error!(format!("BigInt.prototype.{} is not implemented", method))),
    }
}

/// Handle static methods on the BigInt constructor (asIntN, asUintN)
pub fn handle_bigint_static_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Evaluate arguments
    if method != "asIntN" && method != "asUintN" {
        return Err(raise_eval_error!(format!("BigInt has no static method '{}'", method)));
    }
    if args.len() != 2 {
        return Err(raise_eval_error!(format!("BigInt.{} requires 2 arguments", method)));
    }

    // bits must be a non-negative integer (ToIndex)
    let bits_val = &args[0];
    let bits = match bits_val {
        Value::Number(n) => {
            if n.is_nan() || *n < 0.0 || n.fract() != 0.0 {
                return Err(raise_eval_error!("bits must be a non-negative integer"));
            }
            // limit to usize
            if *n < 0.0 {
                return Err(raise_eval_error!("bits must be non-negative"));
            }
            *n as usize
        }
        _ => return Err(raise_eval_error!("bits must be a number")),
    };

    // bigint argument: accept BigInt, Number (integer), String, Boolean, or Object (ToPrimitive)
    let bigint_val = &args[1];
    let bi = match bigint_val {
        Value::BigInt(b) => b.clone(),
        Value::Number(n) => {
            if n.is_nan() || !n.is_finite() || n.fract() != 0.0 {
                return Err(raise_eval_error!("Cannot convert number to BigInt"));
            }
            BigInt::from(*n as i64)
        }
        Value::String(s) => {
            let st = utf16_to_utf8(&s);
            parse_bigint_string(&st)?
        }
        Value::Boolean(b) => {
            if *b {
                BigInt::from(1)
            } else {
                BigInt::from(0)
            }
        }
        Value::Object(obj) => {
            // Try ToPrimitive with number hint first
            let prim = to_primitive(mc, &Value::Object(obj.clone()), "number", env)?;
            match prim {
                Value::Number(n) => {
                    if n.is_nan() || !n.is_finite() || n.fract() != 0.0 {
                        return Err(raise_eval_error!("Cannot convert number to BigInt"));
                    }
                    BigInt::from(n as i64)
                }
                Value::String(s) => {
                    let st = utf16_to_utf8(&s);
                    parse_bigint_string(&st)?
                }
                Value::BigInt(b) => b,
                _ => return Err(raise_eval_error!("Cannot convert object to BigInt")),
            }
        }
        _ => return Err(raise_eval_error!("bigint argument must be a BigInt or convertible value")),
    };

    // modulus = 2 ** bits
    let modulus = if bits == 0 { BigInt::from(1u8) } else { BigInt::from(1u8) << bits };

    // r = bi mod modulus (non-negative)
    let mut r = (&bi) % &modulus;
    if r.sign() == Sign::Minus {
        r += &modulus;
    }

    if method == "asUintN" {
        return Ok(Value::BigInt(r));
    }

    // asIntN: if r >= 2^(bits-1) then r -= 2^bits
    if bits == 0 {
        return Ok(Value::BigInt(BigInt::from(0)));
    }
    let half = &modulus >> 1;
    if r >= half {
        r -= &modulus;
    }
    Ok(Value::BigInt(r))
}

pub fn parse_bigint_string(raw: &str) -> Result<BigInt, JSError> {
    let s = if let Some(st) = raw.strip_suffix('n') { st } else { raw };
    let (radix, num_str) = if s.starts_with("0x") || s.starts_with("0X") {
        (16, &s[2..])
    } else if s.starts_with("0b") || s.starts_with("0B") {
        (2, &s[2..])
    } else if s.starts_with("0o") || s.starts_with("0O") {
        (8, &s[2..])
    } else {
        (10, s)
    };
    BigInt::parse_bytes(num_str.as_bytes(), radix).ok_or(raise_eval_error!("invalid bigint"))
}

pub fn initialize_bigint<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let bigint_ctor = new_js_object_data(mc);
    obj_set_key_value(mc, &bigint_ctor, &"__native_ctor".into(), Value::String(utf8_to_utf16("BigInt")))?;

    // Add static methods
    obj_set_key_value(mc, &bigint_ctor, &"asIntN".into(), Value::Function("BigInt.asIntN".to_string()))?;
    obj_set_key_value(mc, &bigint_ctor, &"asUintN".into(), Value::Function("BigInt.asUintN".to_string()))?;

    // Create prototype
    let bigint_proto = new_js_object_data(mc);
    // Set BigInt.prototype's prototype to Object.prototype if available
    if let Some(obj_val) = obj_get_key_value(env, &"Object".into())?
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = obj_get_key_value(obj_ctor, &"prototype".into())?
        && let Value::Object(obj_proto) = &*proto_val.borrow()
    {
        bigint_proto.borrow_mut(mc).prototype = Some(*obj_proto);
    }

    let to_string = Value::Function("BigInt.prototype.toString".to_string());
    obj_set_key_value(mc, &bigint_proto, &"toString".into(), to_string)?;
    let value_of = Value::Function("BigInt.prototype.valueOf".to_string());
    obj_set_key_value(mc, &bigint_proto, &"valueOf".into(), value_of)?;

    // Wire up
    obj_set_key_value(mc, &bigint_ctor, &"prototype".into(), Value::Object(bigint_proto.clone()))?;
    obj_set_key_value(mc, &bigint_proto, &"constructor".into(), Value::Object(bigint_ctor.clone()))?;

    // Mark prototype methods and constructor non-enumerable
    bigint_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from("toString"));
    bigint_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from("valueOf"));
    bigint_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from("constructor"));

    env_set(mc, env, "BigInt", Value::Object(bigint_ctor))?;

    Ok(())
}
