#![allow(clippy::collapsible_if, clippy::collapsible_match)]

use crate::core::MutationContext;
use crate::core::js_error::EvalError;
use crate::core::{
    InternalSlot, JSObjectDataPtr, Value, new_js_object_data, object_get_key_value, object_set_key_value, slot_get_chained, slot_set,
    to_number_with_env,
};
use crate::env_set;
use crate::error::JSError;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use num_traits::ToPrimitive;

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// ECMAScript whitespace characters (broader than Rust's `.trim()`).
fn is_es_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}' | '\u{000A}' | '\u{000B}' | '\u{000C}' | '\u{000D}' | '\u{0020}' | '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// Trim ECMAScript whitespace from both ends of a string.
pub(crate) fn es_trim(s: &str) -> &str {
    let start = s.find(|c: char| !is_es_whitespace(c)).unwrap_or(s.len());
    let end = s
        .rfind(|c: char| !is_es_whitespace(c))
        .map_or(start, |i| i + s[i..].chars().next().unwrap().len_utf8());
    &s[start..end]
}

/// ToIntegerOrInfinity on a JS value, propagating errors for BigInt/Symbol/objects.
fn to_integer_or_infinity<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, val: &Value<'gc>) -> Result<f64, EvalError<'gc>> {
    let n = to_number_with_env(mc, env, val)?;
    if n.is_nan() || n == 0.0 {
        Ok(0.0)
    } else if !n.is_finite() {
        Ok(n) // ±∞
    } else {
        Ok(n.trunc())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Initialization
// ═══════════════════════════════════════════════════════════════════════════════

pub fn initialize_number_module<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let number_obj = make_number_object(mc, env)?;
    env_set(mc, env, "Number", &Value::Object(number_obj))?;
    Ok(())
}

/// Create the Number object with all number constants and functions
fn make_number_object<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let number_obj = new_js_object_data(mc);
    slot_set(mc, &number_obj, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &number_obj, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("Number")));

    // --- Constants (non-writable, non-enumerable, non-configurable) ---
    let constants: &[(&str, f64)] = &[
        ("MAX_VALUE", f64::MAX),
        ("MIN_VALUE", f64::from_bits(1)),
        ("NaN", f64::NAN),
        ("POSITIVE_INFINITY", f64::INFINITY),
        ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
        ("EPSILON", f64::EPSILON),
        ("MAX_SAFE_INTEGER", 9007199254740991.0),
        ("MIN_SAFE_INTEGER", -9007199254740991.0),
    ];
    for &(name, val) in constants {
        object_set_key_value(mc, &number_obj, name, &Value::Number(val))?;
        number_obj.borrow_mut(mc).set_non_enumerable(name);
        number_obj.borrow_mut(mc).set_non_writable(name);
        number_obj.borrow_mut(mc).set_non_configurable(name);
    }

    // --- Static methods (non-enumerable) ---
    let statics: &[(&str, &str)] = &[
        ("isNaN", "Number.isNaN"),
        ("isFinite", "Number.isFinite"),
        ("isInteger", "Number.isInteger"),
        ("isSafeInteger", "Number.isSafeInteger"),
        // Number.parseFloat/parseInt must be the SAME function object as global parseFloat/parseInt
        ("parseFloat", "parseFloat"),
        ("parseInt", "parseInt"),
    ];
    for &(name, tag) in statics {
        object_set_key_value(mc, &number_obj, name, &Value::Function(tag.to_string()))?;
        number_obj.borrow_mut(mc).set_non_enumerable(name);
    }

    // prototype descriptor: non-enumerable, non-writable, non-configurable
    number_obj.borrow_mut(mc).set_non_enumerable("prototype");
    number_obj.borrow_mut(mc).set_non_writable("prototype");
    number_obj.borrow_mut(mc).set_non_configurable("prototype");

    // Number.length = 1
    object_set_key_value(mc, &number_obj, "length", &Value::Number(1.0))?;
    number_obj.borrow_mut(mc).set_non_enumerable("length");
    number_obj.borrow_mut(mc).set_non_writable("length");

    // --- Object.prototype ---
    let object_proto = if let Some(obj_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else {
        None
    };

    // --- Number.prototype ---
    let number_prototype = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        number_prototype.borrow_mut(mc).prototype = Some(proto);
    }
    // Number.prototype is itself a Number object with [[NumberData]] = +0
    slot_set(mc, &number_prototype, InternalSlot::PrimitiveValue, &Value::Number(0.0));

    let proto_methods = ["toString", "valueOf", "toLocaleString", "toExponential", "toFixed", "toPrecision"];
    for name in proto_methods {
        object_set_key_value(mc, &number_prototype, name, &Value::Function(format!("Number.prototype.{name}")))?;
        number_prototype.borrow_mut(mc).set_non_enumerable(name);
    }
    number_prototype.borrow_mut(mc).set_non_enumerable("constructor");

    // Set prototype on Number constructor
    object_set_key_value(mc, &number_obj, "prototype", &Value::Object(number_prototype))?;

    // Ensure Number.prototype.constructor → Number
    if let Some(proto_val) = object_get_key_value(&number_obj, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        object_set_key_value(mc, proto_obj, "constructor", &Value::Object(number_obj))?;
        proto_obj.borrow_mut(mc).set_non_enumerable("constructor");
    }

    Ok(number_obj)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Constructor
// ═══════════════════════════════════════════════════════════════════════════════

pub(crate) fn number_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.is_empty() {
        return Ok(Value::Number(0.0));
    }
    let arg_val = &args[0];
    match arg_val {
        Value::Number(n) => Ok(Value::Number(*n)),
        Value::Boolean(b) => Ok(Value::Number(if *b { 1.0 } else { 0.0 })),
        Value::Null => Ok(Value::Number(0.0)),
        Value::Undefined => Ok(Value::Number(f64::NAN)),
        Value::BigInt(b) => {
            // Number(BigInt) does NOT throw — converts lossy via ℝ(n)
            Ok(Value::Number(b.to_f64().unwrap_or(f64::NAN)))
        }
        Value::Symbol(_) => Err(EvalError::from(raise_type_error!("Cannot convert a Symbol value to a number"))),
        Value::String(s) => {
            let str_val = utf16_to_utf8(s);
            Ok(Value::Number(es_string_to_number(&str_val)))
        }
        Value::Object(obj) => {
            // ToNumeric: ToPrimitive(number hint), then if BigInt → ℝ, else ToNumber
            let prim = crate::core::to_primitive(mc, &Value::Object(*obj), "number", env)?;
            match prim {
                Value::Number(n) => Ok(Value::Number(n)),
                Value::Boolean(b) => Ok(Value::Number(if b { 1.0 } else { 0.0 })),
                Value::Null => Ok(Value::Number(0.0)),
                Value::Undefined => Ok(Value::Number(f64::NAN)),
                Value::BigInt(b) => Ok(Value::Number(b.to_f64().unwrap_or(f64::NAN))),
                Value::Symbol(_) => Err(EvalError::from(raise_type_error!("Cannot convert a Symbol value to a number"))),
                Value::String(s) => {
                    let str_val = utf16_to_utf8(&s);
                    Ok(Value::Number(es_string_to_number(&str_val)))
                }
                _ => Ok(Value::Number(f64::NAN)),
            }
        }
        _ => Ok(Value::Number(f64::NAN)),
    }
}

/// ES-compliant string→number (used by Number(), NOT by parseFloat/parseInt).
/// Empty/whitespace → 0, only exact "Infinity"/"+Infinity"/"-Infinity", hex/bin/oct.
pub(crate) fn es_string_to_number(s: &str) -> f64 {
    let trimmed = es_trim(s);
    if trimmed.is_empty() {
        return 0.0;
    }
    if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).map(|v| v as f64).unwrap_or(f64::NAN);
    }
    if let Some(bin) = trimmed.strip_prefix("0b").or_else(|| trimmed.strip_prefix("0B")) {
        return i64::from_str_radix(bin, 2).map(|v| v as f64).unwrap_or(f64::NAN);
    }
    if let Some(oct) = trimmed.strip_prefix("0o").or_else(|| trimmed.strip_prefix("0O")) {
        return i64::from_str_radix(oct, 8).map(|v| v as f64).unwrap_or(f64::NAN);
    }
    // Only accept exact "Infinity"/"+Infinity"/"-Infinity" (case-sensitive)
    match trimmed {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    // Guard: reject word-form "infinity"/"INFINITY" etc. (Rust's parse accepts them),
    // but allow legitimate numeric overflow like "10e10000" → Infinity.
    let result = trimmed.parse::<f64>().unwrap_or(f64::NAN);
    if result.is_infinite() {
        let stripped = trimmed.strip_prefix('+').or_else(|| trimmed.strip_prefix('-')).unwrap_or(trimmed);
        if stripped.starts_with(|c: char| c.is_alphabetic()) {
            return f64::NAN; // case-insensitive word "infinity" → NaN
        }
    }
    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// Static methods
// ═══════════════════════════════════════════════════════════════════════════════

pub fn handle_number_static_method<'gc>(method: &str, args: &[Value<'gc>]) -> Result<Value<'gc>, JSError> {
    match method {
        "isNaN" => {
            let arg = args.first().unwrap_or(&Value::Undefined);
            if let Value::Number(n) = arg {
                Ok(Value::Boolean(n.is_nan()))
            } else {
                Ok(Value::Boolean(false)) // not Number type → false
            }
        }
        "isFinite" => {
            let arg = args.first().unwrap_or(&Value::Undefined);
            if let Value::Number(n) = arg {
                Ok(Value::Boolean(n.is_finite()))
            } else {
                Ok(Value::Boolean(false))
            }
        }
        "isInteger" => {
            let arg = args.first().unwrap_or(&Value::Undefined);
            if let Value::Number(n) = arg {
                Ok(Value::Boolean(n.is_finite() && n.fract() == 0.0))
            } else {
                Ok(Value::Boolean(false))
            }
        }
        "isSafeInteger" => {
            let arg = args.first().unwrap_or(&Value::Undefined);
            if let Value::Number(n) = arg {
                let is_int = n.is_finite() && n.fract() == 0.0;
                let is_safe = (-9007199254740991.0..=9007199254740991.0).contains(n);
                Ok(Value::Boolean(is_int && is_safe))
            } else {
                Ok(Value::Boolean(false))
            }
        }
        _ => Err(raise_eval_error!(format!("Number.{method} is not implemented"))),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Prototype / instance methods
// ═══════════════════════════════════════════════════════════════════════════════

/// Core instance-method dispatch. `n` is the resolved thisNumberValue.
fn handle_number_instance_method<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    n: f64,
    method: &str,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        // ─── toString ───────────────────────────────────────────────
        "toString" => {
            let radix_arg = args.first().unwrap_or(&Value::Undefined);
            if matches!(radix_arg, Value::Undefined) {
                return Ok(Value::String(utf8_to_utf16(&crate::core::value_to_string(&Value::Number(n)))));
            }
            let radix_f = to_number_with_env(mc, env, radix_arg)?;
            let radix = radix_f as i32;
            if !(2..=36).contains(&radix) {
                return Err(raise_range_error!("toString() radix must be between 2 and 36").into());
            }
            if radix == 10 {
                return Ok(Value::String(utf8_to_utf16(&crate::core::value_to_string(&Value::Number(n)))));
            }
            Ok(Value::String(utf8_to_utf16(&number_to_radix_string(n, radix as u32))))
        }

        // ─── valueOf ────────────────────────────────────────────────
        "valueOf" => Ok(Value::Number(n)),

        // ─── toLocaleString ─────────────────────────────────────────
        "toLocaleString" => Ok(Value::String(utf8_to_utf16(&crate::core::value_to_string(&Value::Number(n))))),

        // ─── toExponential ──────────────────────────────────────────
        "toExponential" => {
            let fd_arg = args.first().unwrap_or(&Value::Undefined);
            let fd_undefined = matches!(fd_arg, Value::Undefined);

            // Step 2: let f = ToIntegerOrInfinity(fractionDigits) — BEFORE checking NaN/Infinity
            let f = if fd_undefined {
                0.0
            } else {
                to_integer_or_infinity(mc, env, fd_arg)?
            };

            // Step 4-6: NaN / Infinity
            if n.is_nan() {
                return Ok(Value::String(utf8_to_utf16("NaN")));
            }
            if n.is_infinite() {
                return Ok(Value::String(utf8_to_utf16(if n > 0.0 { "Infinity" } else { "-Infinity" })));
            }

            // Range check AFTER NaN/Infinity handling
            if !fd_undefined && !(0.0..=100.0).contains(&f) {
                return Err(raise_range_error!("toExponential() argument must be between 0 and 100").into());
            }
            let f = f as usize;

            Ok(Value::String(utf8_to_utf16(&es_to_exponential(
                n,
                if fd_undefined { None } else { Some(f) },
            ))))
        }

        // ─── toFixed ────────────────────────────────────────────────
        "toFixed" => {
            let fd_arg = args.first().unwrap_or(&Value::Undefined);
            let f = to_integer_or_infinity(mc, env, fd_arg)?;
            if !(0.0..=100.0).contains(&f) {
                return Err(raise_range_error!("toFixed() digits argument must be between 0 and 100").into());
            }
            let f = f as usize;
            if n.is_nan() {
                return Ok(Value::String(utf8_to_utf16("NaN")));
            }
            if n.is_infinite() {
                return Ok(Value::String(utf8_to_utf16(if n > 0.0 { "Infinity" } else { "-Infinity" })));
            }
            // Step 9: If x >= 10^21, let m = ToString(x)
            if n.abs() >= 1e21 {
                return Ok(Value::String(utf8_to_utf16(&crate::core::value_to_string(&Value::Number(n)))));
            }
            Ok(Value::String(utf8_to_utf16(&format!("{:.prec$}", n, prec = f))))
        }

        // ─── toPrecision ────────────────────────────────────────────
        "toPrecision" => {
            let p_arg = args.first().unwrap_or(&Value::Undefined);
            if matches!(p_arg, Value::Undefined) {
                return Ok(Value::String(utf8_to_utf16(&crate::core::value_to_string(&Value::Number(n)))));
            }
            let p = to_integer_or_infinity(mc, env, p_arg)?;
            if n.is_nan() {
                return Ok(Value::String(utf8_to_utf16("NaN")));
            }
            if n.is_infinite() {
                return Ok(Value::String(utf8_to_utf16(if n > 0.0 { "Infinity" } else { "-Infinity" })));
            }
            let p = p as usize;
            if !(1..=100).contains(&p) {
                return Err(raise_range_error!("toPrecision() argument must be between 1 and 100").into());
            }
            // Handle ±0 specially
            let x = if n == 0.0 { 0.0_f64.copysign(1.0) } else { n };
            let negative = n < 0.0; // -0 is NOT negative for toPrecision
            let abs_x = x.abs();
            // Use exponential format to get the exponent
            let s_exp = format!("{:.prec$e}", abs_x, prec = p - 1);
            let parts: Vec<&str> = s_exp.split('e').collect();
            let exponent: i32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);

            let result = if exponent < -6 || exponent >= p as i32 {
                // Exponential notation
                normalize_exponent(&s_exp)
            } else {
                // Fixed notation: digits after decimal = p - 1 - exponent
                let digits_after = (p as i32 - 1 - exponent).max(0) as usize;
                format!("{:.prec$}", abs_x, prec = digits_after)
            };
            if negative {
                Ok(Value::String(utf8_to_utf16(&format!("-{result}"))))
            } else {
                Ok(Value::String(utf8_to_utf16(&result)))
            }
        }

        _ => Err(raise_eval_error!(format!("Number.prototype.{method} is not implemented")).into()),
    }
}

/// ES-spec compliant toExponential with correct rounding (round half away from zero).
fn es_to_exponential(n: f64, fraction_digits: Option<usize>) -> String {
    let negative = n < 0.0;
    let x = n.abs();

    if x == 0.0 {
        // Special case for ±0
        return match fraction_digits {
            Some(0) | None => "0e+0".to_string(),
            Some(f) => format!("0.{}e+0", "0".repeat(f)),
        };
    }

    let p = match fraction_digits {
        Some(f) => f + 1, // total significant digits
        None => {
            // Shortest representation: use Rust's default but fix sign
            let s = format!("{:e}", x);
            let result = normalize_exponent(&s);
            return if negative { format!("-{result}") } else { result };
        }
    };

    // Generate high-precision digits to avoid double-rounding.
    // Use many extra digits so we can do a single correct round to p sig digits.
    let extra = 40.max(p + 20);
    let hp = format!("{:.prec$e}", x, prec = extra);
    let (mant_str, exp_str) = hp.split_once('e').unwrap();
    let mut exp: i32 = exp_str.parse().unwrap();

    // Remove the dot to get all digits
    let all_digits: Vec<u8> = mant_str.bytes().filter(|b| *b != b'.').collect();

    // We have extra+1 significant digits; take p and decide rounding from the rest.
    let mut result_digits: Vec<u8> = all_digits[..p.min(all_digits.len())].to_vec();
    let remaining = &all_digits[p.min(all_digits.len())..];

    // Round half away from zero: if remaining >= 0.5 (i.e. first remaining digit >= 5), round up
    let round_up = !remaining.is_empty() && remaining[0] >= b'5';
    if round_up {
        let mut carry = true;
        for d in result_digits.iter_mut().rev() {
            if carry {
                if *d == b'9' {
                    *d = b'0';
                } else {
                    *d += 1;
                    carry = false;
                }
            }
        }
        if carry {
            result_digits.insert(0, b'1');
            result_digits.pop();
            exp += 1;
        }
    }

    // Pad to exactly p digits
    while result_digits.len() < p {
        result_digits.push(b'0');
    }

    let digits_str: String = result_digits.iter().map(|&b| b as char).collect();
    let (first, rest) = digits_str.split_at(1);
    let sign = if exp >= 0 { "+" } else { "" };
    let mantissa = if rest.is_empty() {
        first.to_string()
    } else {
        format!("{first}.{rest}")
    };
    let result = format!("{mantissa}e{sign}{exp}");
    if negative { format!("-{result}") } else { result }
}

/// Normalize Rust exponent format `1.23e2` → `1.23e+2`, keep `e-4` as-is.
fn normalize_exponent(s: &str) -> String {
    if let Some((mantissa, exp)) = s.split_once('e') {
        if !exp.starts_with('+') && !exp.starts_with('-') {
            format!("{mantissa}e+{exp}")
        } else {
            s.to_string()
        }
    } else {
        s.to_string()
    }
}

/// Number→string in non-decimal radix (2–36), with fractional part.
fn number_to_radix_string(n: f64, radix: u32) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n == f64::INFINITY {
        return "Infinity".to_string();
    }
    if n == f64::NEG_INFINITY {
        return "-Infinity".to_string();
    }
    if n == 0.0 {
        return "0".to_string();
    } // handles ±0

    let negative = n < 0.0;
    let abs_n = n.abs();
    let integer_part = abs_n.trunc() as u64;
    let fractional_part = abs_n - (integer_part as f64);

    // Integer part
    let mut int_digits = Vec::new();
    if integer_part == 0 {
        int_digits.push('0');
    } else {
        let mut rem = integer_part;
        while rem > 0 {
            let d = (rem % radix as u64) as u8;
            int_digits.push(if d < 10 { (b'0' + d) as char } else { (b'a' + d - 10) as char });
            rem /= radix as u64;
        }
        int_digits.reverse();
    }

    // Fractional part
    let mut out: String = int_digits.into_iter().collect();
    if fractional_part > 0.0 {
        out.push('.');
        let mut frac = fractional_part;
        for _ in 0..52 {
            // precision limit
            frac *= radix as f64;
            let digit = frac.trunc() as u8;
            out.push(if digit < 10 {
                (b'0' + digit) as char
            } else {
                (b'a' + digit - 10) as char
            });
            frac -= digit as f64;
            if frac < f64::EPSILON * radix as f64 {
                break;
            }
        }
    }

    if negative { format!("-{out}") } else { out }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Prototype dispatch — resolves `this` then delegates
// ═══════════════════════════════════════════════════════════════════════════════

pub fn handle_number_prototype_method<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    this: Option<&Value<'gc>>,
    method: &str,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError<'gc>> {
    // thisNumberValue(this)
    let n = match this {
        Some(Value::Number(n)) => *n,
        Some(Value::Object(obj)) => {
            if let Some(val) = slot_get_chained(obj, &InternalSlot::PrimitiveValue) {
                if let Value::Number(n) = &*val.borrow() {
                    *n
                } else {
                    return Err(raise_type_error!("Number.prototype method called on non-number object").into());
                }
            } else {
                return Err(raise_type_error!("Number.prototype method called on incompatible receiver").into());
            }
        }
        _ => {
            return Err(raise_type_error!("Number.prototype method requires that 'this' be a Number").into());
        }
    };
    handle_number_instance_method(mc, env, n, method, args)
}
