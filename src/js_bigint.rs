use crate::core::MutationContext;
use crate::core::{
    InternalSlot, JSObjectDataPtr, Value, env_set, new_js_object_data, object_get_key_value, object_set_key_value, slot_get_chained,
    slot_set, to_primitive,
};
use crate::error::JSError;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use num_bigint::BigInt;
use num_bigint::Sign;
use num_traits::{FromPrimitive, ToPrimitive};

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
            Ok(Value::BigInt(Box::new(BigInt::from(*n as i64))))
        }
        Value::String(s) => {
            let st = utf16_to_utf8(s);
            Ok(Value::BigInt(Box::new(parse_bigint_string(&st)?)))
        }
        Value::Boolean(b) => {
            let bigint = if *b { BigInt::from(1) } else { BigInt::from(0) };
            Ok(Value::BigInt(Box::new(bigint)))
        }
        Value::Object(obj) => {
            // Try ToPrimitive with number hint first
            let prim = to_primitive(mc, &Value::Object(*obj), "number", env)?;
            match prim {
                Value::Number(n) => {
                    if n.is_nan() || !n.is_finite() || n.fract() != 0.0 {
                        return Err(raise_type_error!("Cannot convert number to BigInt"));
                    }
                    Ok(Value::BigInt(Box::new(BigInt::from(n as i64))))
                }
                Value::String(s) => {
                    let st = utf16_to_utf8(&s);
                    Ok(Value::BigInt(Box::new(parse_bigint_string(&st)?)))
                }
                Value::BigInt(b) => Ok(Value::BigInt(b)),
                _ => Err(raise_type_error!("Cannot convert object to BigInt")),
            }
        }
        _ => Err(raise_type_error!("Cannot convert value to BigInt")),
    }
}

/// Handle BigInt object methods (toString, valueOf)
pub fn handle_bigint_object_method<'gc>(this_val: &Value<'gc>, method: &str, args: &[Value<'gc>]) -> Result<Value<'gc>, JSError> {
    let h = match this_val {
        Value::BigInt(b) => b.clone(),
        Value::Object(obj) => {
            if let Some(value_val) = slot_get_chained(obj, &InternalSlot::PrimitiveValue) {
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
                Ok(Value::String(utf8_to_utf16(&h.to_string())))
            } else {
                // radix support: expect a number argument
                let arg0 = &args[0];
                if let Value::Number(rad) = arg0 {
                    let r = *rad as i32;
                    if !(2..=36).contains(&r) {
                        return Err(raise_eval_error!("toString() radix out of range"));
                    }
                    let s = h.to_str_radix(r as u32);
                    Ok(Value::String(utf8_to_utf16(&s)))
                } else {
                    Err(raise_eval_error!("toString radix must be a number"))
                }
            }
        }
        "valueOf" => {
            if args.is_empty() {
                Ok(Value::BigInt(h.clone()))
            } else {
                Err(raise_eval_error!("valueOf expects no arguments"))
            }
        }
        _ => Err(raise_eval_error!(format!("BigInt.prototype.{} is not implemented", method))),
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
        Value::BigInt(b) => (**b).clone(),
        Value::Number(n) => {
            if n.is_nan() || !n.is_finite() || n.fract() != 0.0 {
                return Err(raise_eval_error!("Cannot convert number to BigInt"));
            }
            BigInt::from(*n as i64)
        }
        Value::String(s) => {
            let st = utf16_to_utf8(s);
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
            let prim = to_primitive(mc, &Value::Object(*obj), "number", env)?;
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
                Value::BigInt(b) => (*b).clone(),
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
        return Ok(Value::BigInt(Box::new(r)));
    }

    // asIntN: if r >= 2^(bits-1) then r -= 2^bits
    if bits == 0 {
        return Ok(Value::BigInt(Box::new(BigInt::from(0))));
    }
    let half = &modulus >> 1;
    if r >= half {
        r -= &modulus;
    }
    Ok(Value::BigInt(Box::new(r)))
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

/// Parse a string (trimmed) into a BigInt for equality/relational comparisons
/// following the heuristic used by Test262 (accepts optional leading +/- and
/// radix prefixes, rejects malformed digits like "++0" or "0."). Returns
/// `None` when the string is not a valid integer literal for BigInt comparison.
pub fn string_to_bigint_for_eq(s: &str) -> Option<BigInt> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Some(BigInt::from(0));
    }

    let (sign, rest) = if let Some(stripped) = trimmed.strip_prefix('-') {
        (-1, stripped)
    } else if let Some(stripped) = trimmed.strip_prefix('+') {
        (1, stripped)
    } else {
        (1, trimmed)
    };

    if rest.is_empty() {
        return None;
    }

    let (radix, digits) = if rest.starts_with("0x") || rest.starts_with("0X") {
        (16, &rest[2..])
    } else if rest.starts_with("0b") || rest.starts_with("0B") {
        (2, &rest[2..])
    } else if rest.starts_with("0o") || rest.starts_with("0O") {
        (8, &rest[2..])
    } else {
        (10, rest)
    };

    if digits.is_empty() {
        return None;
    }

    // Reject digits that begin with an extra sign or contain invalid chars
    if digits.starts_with('+') || digits.starts_with('-') {
        return None;
    }

    let valid = match radix {
        10 => digits.chars().all(|c| c.is_ascii_digit()),
        16 => digits.chars().all(|c| c.is_ascii_hexdigit()),
        8 => digits.chars().all(|c| ('0'..='7').contains(&c)),
        2 => digits.chars().all(|c| c == '0' || c == '1'),
        _ => false,
    };
    if !valid {
        return None;
    }

    let mut value = BigInt::parse_bytes(digits.as_bytes(), radix)?;
    if sign < 0 {
        value = -value;
    }
    Some(value)
}

/// Compare a BigInt and a JS Number for relational operators.
/// Returns `Some(Ordering)` when a deterministic comparison can be made, or
/// `None` when the comparison is undefined (e.g., due to NaN).
pub fn compare_bigint_and_number(b: &BigInt, n: f64) -> Option<std::cmp::Ordering> {
    if n.is_nan() {
        return None;
    }
    // If the number is an integer and can be converted to BigInt, compare as BigInt
    if n.is_finite()
        && n.fract() == 0.0
        && let Some(rb) = BigInt::from_f64(n)
    {
        return Some(b.cmp(&rb));
    }
    // Otherwise fall back to floating-point comparison using BigInt -> f64
    let bf = b.to_f64().unwrap_or(f64::NAN);
    if bf.is_nan() {
        return None;
    }
    if bf < n {
        Some(std::cmp::Ordering::Less)
    } else if bf > n {
        Some(std::cmp::Ordering::Greater)
    } else {
        Some(std::cmp::Ordering::Equal)
    }
}

pub fn initialize_bigint<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let bigint_ctor = new_js_object_data(mc);
    slot_set(mc, &bigint_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("BigInt")));

    // Add static methods
    object_set_key_value(mc, &bigint_ctor, "asIntN", &Value::Function("BigInt.asIntN".to_string()))?;
    object_set_key_value(mc, &bigint_ctor, "asUintN", &Value::Function("BigInt.asUintN".to_string()))?;
    // Create prototype
    let bigint_proto = new_js_object_data(mc);
    // Set BigInt.prototype's prototype to Object.prototype if available
    if let Some(obj_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(obj_proto) = &*proto_val.borrow()
    {
        bigint_proto.borrow_mut(mc).prototype = Some(*obj_proto);
    }

    let to_string = Value::Function("BigInt.prototype.toString".to_string());
    object_set_key_value(mc, &bigint_proto, "toString", &to_string)?;
    let value_of = Value::Function("BigInt.prototype.valueOf".to_string());
    object_set_key_value(mc, &bigint_proto, "valueOf", &value_of)?;

    // Wire up
    object_set_key_value(mc, &bigint_ctor, "prototype", &Value::Object(bigint_proto))?;
    object_set_key_value(mc, &bigint_proto, "constructor", &Value::Object(bigint_ctor))?;

    // Mark prototype methods and constructor non-enumerable
    bigint_proto.borrow_mut(mc).set_non_enumerable("toString");
    bigint_proto.borrow_mut(mc).set_non_enumerable("valueOf");
    bigint_proto.borrow_mut(mc).set_non_enumerable("constructor");

    // Set BigInt.prototype[@@toStringTag] = "BigInt"
    if let Some(sym_ctor_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor_val.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        object_set_key_value(mc, &bigint_proto, *tag_sym, &Value::String(utf8_to_utf16("BigInt")))?;
        bigint_proto
            .borrow_mut(mc)
            .set_non_enumerable(crate::core::PropertyKey::Symbol(*tag_sym));
    }

    env_set(mc, env, "BigInt", &Value::Object(bigint_ctor))?;

    Ok(())
}
