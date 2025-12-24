use crate::core::{Expr, JSObjectDataPtr, Value, evaluate_expr, obj_get_key_value, parse_bigint_string, to_primitive};
use crate::error::JSError;
use crate::unicode::utf8_to_utf16;
use num_bigint::BigInt;
use num_bigint::Sign;

pub(crate) fn bigint_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // BigInt(value) conversion per simplified rules:
    if args.len() != 1 {
        return Err(raise_type_error!("BigInt requires exactly one argument"));
    }
    let arg_val = evaluate_expr(env, &args[0])?;
    match arg_val {
        Value::BigInt(b) => Ok(Value::BigInt(b)),
        Value::Number(n) => {
            if n.is_nan() || !n.is_finite() || n.fract() != 0.0 {
                return Err(raise_type_error!("Cannot convert number to BigInt"));
            }
            Ok(Value::BigInt(BigInt::from(n as i64)))
        }
        Value::String(s) => {
            let st = String::from_utf16_lossy(&s);
            Ok(Value::BigInt(parse_bigint_string(&st)?))
        }
        Value::Boolean(b) => {
            let bigint = if b { BigInt::from(1) } else { BigInt::from(0) };
            Ok(Value::BigInt(bigint))
        }
        Value::Object(obj) => {
            // Try ToPrimitive with number hint first
            let prim = to_primitive(&Value::Object(obj.clone()), "number", env)?;
            match prim {
                Value::Number(n) => {
                    if n.is_nan() || !n.is_finite() || n.fract() != 0.0 {
                        return Err(raise_type_error!("Cannot convert number to BigInt"));
                    }
                    Ok(Value::BigInt(BigInt::from(n as i64)))
                }
                Value::String(s) => {
                    let st = String::from_utf16_lossy(&s);
                    Ok(Value::BigInt(parse_bigint_string(&st)?))
                }
                Value::BigInt(b) => Ok(Value::BigInt(b)),
                _ => Err(raise_type_error!("Cannot convert object to BigInt")),
            }
        }
        _ => Err(raise_type_error!("Cannot convert value to BigInt")),
    }
}

/// Handle boxed BigInt object methods (toString, valueOf)
pub fn handle_bigint_object_method(object: &JSObjectDataPtr, method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if let Some(value_val) = obj_get_key_value(object, &"__value__".into())? {
        if let Value::BigInt(h) = &*value_val.borrow() {
            match method {
                "toString" => {
                    if args.is_empty() {
                        return Ok(Value::String(utf8_to_utf16(&h.to_string())));
                    } else {
                        // radix support: expect a number argument
                        let arg0 = crate::core::evaluate_expr(env, &args[0])?;
                        if let Value::Number(rad) = arg0 {
                            let r = rad as i32;
                            if !(2..=36).contains(&r) {
                                return Err(raise_eval_error!("toString() radix out of range"));
                            }
                            let h_clone = h.clone();
                            let bi = h_clone;
                            let s = bi.to_str_radix(r as u32);
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
        } else {
            return Err(raise_eval_error!("Invalid __value__ for BigInt instance"));
        }
    }
    Err(raise_eval_error!("__value__ not found on BigInt instance"))
}

/// Handle static methods on the BigInt constructor (asIntN, asUintN)
pub fn handle_bigint_static_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Evaluate arguments
    if method != "asIntN" && method != "asUintN" {
        return Err(raise_eval_error!(format!("BigInt has no static method '{}'", method)));
    }
    if args.len() != 2 {
        return Err(raise_eval_error!(format!("BigInt.{} requires 2 arguments", method)));
    }

    // bits must be a non-negative integer (ToIndex)
    let bits_val = crate::core::evaluate_expr(env, &args[0])?;
    let bits = match bits_val {
        Value::Number(n) => {
            if n.is_nan() || n < 0.0 || n.fract() != 0.0 {
                return Err(raise_eval_error!("bits must be a non-negative integer"));
            }
            // limit to usize
            if n < 0.0 {
                return Err(raise_eval_error!("bits must be non-negative"));
            }
            n as usize
        }
        _ => return Err(raise_eval_error!("bits must be a number")),
    };

    // bigint argument: accept BigInt, Number (integer), String, Boolean, or Object (ToPrimitive)
    let bigint_val = crate::core::evaluate_expr(env, &args[1])?;
    let bi = match bigint_val {
        Value::BigInt(b) => b,
        Value::Number(n) => {
            if n.is_nan() || !n.is_finite() || n.fract() != 0.0 {
                return Err(raise_eval_error!("Cannot convert number to BigInt"));
            }
            BigInt::from(n as i64)
        }
        Value::String(s) => {
            let st = String::from_utf16_lossy(&s);
            parse_bigint_string(&st)?
        }
        Value::Boolean(b) => {
            if b {
                BigInt::from(1)
            } else {
                BigInt::from(0)
            }
        }
        Value::Object(obj) => {
            // Try ToPrimitive with number hint first
            let prim = crate::core::to_primitive(&Value::Object(obj.clone()), "number", env)?;
            match prim {
                Value::Number(n) => {
                    if n.is_nan() || !n.is_finite() || n.fract() != 0.0 {
                        return Err(raise_eval_error!("Cannot convert number to BigInt"));
                    }
                    BigInt::from(n as i64)
                }
                Value::String(s) => {
                    let st = String::from_utf16_lossy(&s);
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
