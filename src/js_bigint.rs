use crate::core::MutationContext;
use crate::core::js_error::EvalError;
use crate::core::{
    InternalSlot, JSObjectDataPtr, PropertyKey, Value, env_set, new_js_object_data, object_get_key_value, object_set_key_value,
    slot_get_chained, slot_set, to_primitive,
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
) -> Result<Value<'gc>, EvalError<'gc>> {
    // BigInt(value) conversion per spec:
    if args.is_empty() {
        return Err(raise_type_error!("Cannot convert undefined to a BigInt").into());
    }
    let arg_val = &args[0];
    match arg_val {
        Value::BigInt(b) => Ok(Value::BigInt(b.clone())),
        Value::Number(n) => {
            if !n.is_finite() || n.fract() != 0.0 {
                return Err(raise_range_error!(format!(
                    "The number {} cannot be converted to a BigInt because it is not an integer",
                    n
                ))
                .into());
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
        Value::Undefined => Err(raise_type_error!("Cannot convert undefined to a BigInt").into()),
        Value::Null => Err(raise_type_error!("Cannot convert null to a BigInt").into()),
        Value::Symbol(_) => Err(raise_type_error!("Cannot convert a Symbol value to a BigInt").into()),
        Value::Object(obj) => {
            // Try ToPrimitive with number hint first
            let prim = to_primitive(mc, &Value::Object(*obj), "number", env)?;
            match prim {
                Value::Number(n) => {
                    if !n.is_finite() || n.fract() != 0.0 {
                        return Err(raise_range_error!(format!(
                            "The number {n} cannot be converted to a BigInt because it is not an integer",
                        ))
                        .into());
                    }
                    Ok(Value::BigInt(Box::new(BigInt::from(n as i64))))
                }
                Value::String(s) => {
                    let st = utf16_to_utf8(&s);
                    Ok(Value::BigInt(Box::new(parse_bigint_string(&st)?)))
                }
                Value::BigInt(b) => Ok(Value::BigInt(b)),
                Value::Boolean(b) => {
                    let bigint = if b { BigInt::from(1) } else { BigInt::from(0) };
                    Ok(Value::BigInt(Box::new(bigint)))
                }
                _ => Err(raise_type_error!("Cannot convert value to a BigInt").into()),
            }
        }
        _ => Err(raise_type_error!("Cannot convert value to a BigInt").into()),
    }
}

/// Handle BigInt object methods (toString, valueOf, toLocaleString)
pub fn handle_bigint_object_method<'gc>(this_val: &Value<'gc>, method: &str, args: &[Value<'gc>]) -> Result<Value<'gc>, JSError> {
    // thisBigIntValue: extract BigInt from this
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
        "toString" | "toLocaleString" => {
            let radix_val = args.first().cloned().unwrap_or(Value::Undefined);
            let radix = match &radix_val {
                Value::Undefined => 10,
                Value::Number(n) => {
                    let r = n.trunc() as i32;
                    if !(2..=36).contains(&r) {
                        return Err(raise_range_error!("toString() radix must be between 2 and 36"));
                    }
                    r
                }
                Value::BigInt(_) => {
                    return Err(raise_type_error!("Cannot convert a BigInt value to a number"));
                }
                Value::Symbol(_) => {
                    return Err(raise_type_error!("Cannot convert a Symbol value to a number"));
                }
                _ => {
                    // Try to convert to number via ToPrimitive-like handling
                    let n = crate::core::to_number(&radix_val).map_err(|_| raise_type_error!("Invalid radix"))?;
                    let r = n.trunc() as i32;
                    if !(2..=36).contains(&r) {
                        return Err(raise_range_error!("toString() radix must be between 2 and 36"));
                    }
                    r
                }
            };
            if radix == 10 {
                Ok(Value::String(utf8_to_utf16(&h.to_string())))
            } else {
                let s = h.to_str_radix(radix as u32);
                Ok(Value::String(utf8_to_utf16(&s)))
            }
        }
        "valueOf" => Ok(Value::BigInt(h)),
        _ => Err(raise_type_error!(format!("BigInt.prototype.{method} is not a function"))),
    }
}

/// Handle static methods on the BigInt constructor (asIntN, asUintN)
pub fn handle_bigint_static_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if method != "asIntN" && method != "asUintN" {
        return Err(raise_type_error!(format!("BigInt.{} is not a function", method)).into());
    }

    // Step 1: ToIndex(bits) — convert first arg
    let bits_raw = args.first().cloned().unwrap_or(Value::Undefined);
    let bits = to_index(mc, &bits_raw, env)?;

    // Step 2: ToBigInt(bigint) — convert second arg
    let bigint_raw = args.get(1).cloned().unwrap_or(Value::Undefined);
    let bi = to_bigint(mc, &bigint_raw, env)?;

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

/// ToIndex helper: convert a Value to a non-negative integer index
/// Per spec: ToIndex(value) → integer ∈ [0, 2^53-1] or throws RangeError/TypeError
fn to_index<'gc>(mc: &MutationContext<'gc>, val: &Value<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<usize, EvalError<'gc>> {
    if matches!(val, Value::Undefined) {
        return Ok(0);
    }
    let prim = if let Value::Object(_) = val {
        to_primitive(mc, val, "number", env)?
    } else {
        val.clone()
    };
    let n = match &prim {
        Value::Number(n) => *n,
        Value::BigInt(_) => return Err(raise_type_error!("Cannot convert a BigInt value to a number").into()),
        Value::Symbol(_) => return Err(raise_type_error!("Cannot convert a Symbol value to a number").into()),
        _ => crate::core::to_number(&prim)?,
    };
    let integer = if n.is_nan() || n == 0.0 { 0.0 } else { n.trunc() };
    // Spec: if integerIndex < 0 or integerIndex >= 2^53, throw RangeError
    const MAX_SAFE_PLUS_ONE: f64 = 9007199254740992.0; // 2^53
    if !(0.0..MAX_SAFE_PLUS_ONE).contains(&integer) {
        return Err(raise_range_error!("Invalid index").into());
    }
    Ok(integer as usize)
}

/// ToBigInt helper: convert a Value to a BigInt
fn to_bigint<'gc>(mc: &MutationContext<'gc>, val: &Value<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<BigInt, EvalError<'gc>> {
    let prim = if let Value::Object(_) = val {
        to_primitive(mc, val, "number", env)?
    } else {
        val.clone()
    };
    match &prim {
        Value::BigInt(b) => Ok((**b).clone()),
        Value::Boolean(b) => Ok(if *b { BigInt::from(1) } else { BigInt::from(0) }),
        Value::String(s) => {
            let st = utf16_to_utf8(s);
            Ok(parse_bigint_string(&st)?)
        }
        Value::Undefined => Err(raise_type_error!("Cannot convert undefined to a BigInt").into()),
        Value::Null => Err(raise_type_error!("Cannot convert null to a BigInt").into()),
        Value::Number(_) => Err(raise_type_error!("Cannot convert a Number value to a BigInt").into()),
        Value::Symbol(_) => Err(raise_type_error!("Cannot convert a Symbol value to a BigInt").into()),
        _ => Err(raise_type_error!("Cannot convert value to a BigInt").into()),
    }
}

pub fn parse_bigint_string(raw: &str) -> Result<BigInt, JSError> {
    // Trim whitespace (including Unicode whitespace chars)
    let trimmed = raw.trim();
    // Empty string → 0n  (StringToBigInt: "" → 0n)
    if trimmed.is_empty() {
        return Ok(BigInt::from(0));
    }
    // Handle optional leading sign (only for decimal)
    let (sign, after_sign) = if let Some(rest) = trimmed.strip_prefix('-') {
        (-1i8, rest)
    } else if let Some(rest) = trimmed.strip_prefix('+') {
        (1i8, rest)
    } else {
        (1i8, trimmed)
    };
    // Determine radix from prefix
    let (radix, digits) = if after_sign.starts_with("0x") || after_sign.starts_with("0X") {
        // Reject signs before non-decimal prefixes: "-0x1" is invalid
        if sign != 1 {
            return Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw)));
        }
        (16, &after_sign[2..])
    } else if after_sign.starts_with("0b") || after_sign.starts_with("0B") {
        if sign != 1 {
            return Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw)));
        }
        (2, &after_sign[2..])
    } else if after_sign.starts_with("0o") || after_sign.starts_with("0O") {
        if sign != 1 {
            return Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw)));
        }
        (8, &after_sign[2..])
    } else {
        (10, after_sign)
    };
    if digits.is_empty() {
        return Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw)));
    }
    match BigInt::parse_bytes(digits.as_bytes(), radix) {
        Some(mut val) => {
            if sign < 0 {
                val = -val;
            }
            Ok(val)
        }
        None => Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw))),
    }
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
    slot_set(mc, &bigint_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));

    // BigInt.length = 1, BigInt.name = "BigInt"
    object_set_key_value(mc, &bigint_ctor, "length", &Value::Number(1.0))?;
    bigint_ctor.borrow_mut(mc).set_non_enumerable("length");
    bigint_ctor.borrow_mut(mc).set_non_writable("length");
    object_set_key_value(mc, &bigint_ctor, "name", &Value::String(utf8_to_utf16("BigInt")))?;
    bigint_ctor.borrow_mut(mc).set_non_enumerable("name");
    bigint_ctor.borrow_mut(mc).set_non_writable("name");

    // Add static methods
    let as_int_n_fn = new_js_object_data(mc);
    slot_set(
        mc,
        &as_int_n_fn,
        InternalSlot::NativeCtor,
        &Value::String(utf8_to_utf16("BigInt.asIntN")),
    );
    slot_set(mc, &as_int_n_fn, InternalSlot::Callable, &Value::Boolean(true));
    object_set_key_value(mc, &as_int_n_fn, "length", &Value::Number(2.0))?;
    as_int_n_fn.borrow_mut(mc).set_non_enumerable("length");
    as_int_n_fn.borrow_mut(mc).set_non_writable("length");
    object_set_key_value(mc, &as_int_n_fn, "name", &Value::String(utf8_to_utf16("asIntN")))?;
    as_int_n_fn.borrow_mut(mc).set_non_enumerable("name");
    as_int_n_fn.borrow_mut(mc).set_non_writable("name");
    object_set_key_value(mc, &bigint_ctor, "asIntN", &Value::Object(as_int_n_fn))?;
    bigint_ctor.borrow_mut(mc).set_non_enumerable("asIntN");

    let as_uint_n_fn = new_js_object_data(mc);
    slot_set(
        mc,
        &as_uint_n_fn,
        InternalSlot::NativeCtor,
        &Value::String(utf8_to_utf16("BigInt.asUintN")),
    );
    slot_set(mc, &as_uint_n_fn, InternalSlot::Callable, &Value::Boolean(true));
    object_set_key_value(mc, &as_uint_n_fn, "length", &Value::Number(2.0))?;
    as_uint_n_fn.borrow_mut(mc).set_non_enumerable("length");
    as_uint_n_fn.borrow_mut(mc).set_non_writable("length");
    object_set_key_value(mc, &as_uint_n_fn, "name", &Value::String(utf8_to_utf16("asUintN")))?;
    as_uint_n_fn.borrow_mut(mc).set_non_enumerable("name");
    as_uint_n_fn.borrow_mut(mc).set_non_writable("name");
    object_set_key_value(mc, &bigint_ctor, "asUintN", &Value::Object(as_uint_n_fn))?;
    bigint_ctor.borrow_mut(mc).set_non_enumerable("asUintN");
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
    // BigInt.prototype is non-writable, non-enumerable, non-configurable
    bigint_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    bigint_ctor.borrow_mut(mc).set_non_configurable("prototype");
    bigint_ctor.borrow_mut(mc).set_non_writable("prototype");
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
        bigint_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::Symbol(*tag_sym));
        bigint_proto.borrow_mut(mc).set_non_writable(PropertyKey::Symbol(*tag_sym));
    }

    env_set(mc, env, "BigInt", &Value::Object(bigint_ctor))?;

    // Set BigInt.__proto__ and asIntN/asUintN.__proto__ to Function.prototype
    if let Some(func_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_val.borrow()
        && let Some(fp_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*fp_val.borrow()
    {
        bigint_ctor.borrow_mut(mc).prototype = Some(*func_proto);
        as_int_n_fn.borrow_mut(mc).prototype = Some(*func_proto);
        as_uint_n_fn.borrow_mut(mc).prototype = Some(*func_proto);
    }

    Ok(())
}
