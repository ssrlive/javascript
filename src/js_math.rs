use crate::core::MutationContext;
use crate::core::js_error::EvalError;
use crate::core::{
    JSObjectDataPtr, PropertyKey, Value, env_set, new_js_object_data, object_get_key_value, object_set_key_value, to_number_with_env,
};
use crate::error::JSError;
use crate::unicode::utf8_to_utf16;

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// ToNumber coercion for a single Math argument (missing → NaN like `undefined`).
#[inline]
fn arg_to_number<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    args: &[Value<'gc>],
    idx: usize,
) -> Result<f64, EvalError<'gc>> {
    to_number_with_env(mc, env, args.get(idx).unwrap_or(&Value::Undefined))
}

/// ToUint32 per spec (7.1.7).
#[inline]
fn to_uint32(n: f64) -> u32 {
    if n.is_nan() || n == 0.0 || !n.is_finite() {
        return 0;
    }
    let two32: f64 = 4_294_967_296.0;
    let mut int = n.trunc() % two32;
    if int < 0.0 {
        int += two32;
    }
    int as u32
}

/// JS `Math.round` semantics: round ties towards +∞, preserving -0.
#[inline]
fn js_round(n: f64) -> f64 {
    if n.is_nan() || n.is_infinite() || n == 0.0 {
        return n; // NaN, ±∞, ±0 → identity
    }
    let f = n.floor();
    if (n - f) >= 0.5 { n.ceil() } else { f }
}

/// JS `Number::exponentiate` — spec edge-cases that differ from IEEE powf.
#[inline]
fn js_pow(base: f64, exp: f64) -> f64 {
    if exp.is_nan() {
        return f64::NAN;
    }
    if exp == 0.0 {
        return 1.0;
    }
    if base.is_nan() {
        return f64::NAN;
    }
    if base.abs() == 1.0 && exp.is_infinite() {
        return f64::NAN;
    }
    base.powf(exp)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Initialization
// ═══════════════════════════════════════════════════════════════════════════════

/// Create the Math object with all mathematical constants and functions
pub fn initialize_math<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let math_obj = new_js_object_data(mc);
    let _ = crate::core::set_internal_prototype_from_constructor(mc, &math_obj, env, "Object");

    // --- Constants (writable:false, enumerable:false, configurable:false) ---
    let constants: &[(&str, f64)] = &[
        ("PI", std::f64::consts::PI),
        ("E", std::f64::consts::E),
        ("LN2", std::f64::consts::LN_2),
        ("LN10", std::f64::consts::LN_10),
        ("LOG2E", std::f64::consts::LOG2_E),
        ("LOG10E", std::f64::consts::LOG10_E),
        ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
        ("SQRT2", std::f64::consts::SQRT_2),
    ];
    for &(name, val) in constants {
        object_set_key_value(mc, &math_obj, name, &Value::Number(val))?;
        math_obj.borrow_mut(mc).set_non_enumerable(name);
        math_obj.borrow_mut(mc).set_non_configurable(name);
        math_obj.borrow_mut(mc).set_non_writable(name);
    }

    // --- Methods (non-enumerable) ---
    let methods = [
        "floor", "ceil", "round", "abs", "sqrt", "pow", "sin", "cos", "tan", "random", "clz32", "imul", "max", "min", "asin", "acos",
        "atan", "atan2", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh", "exp", "expm1", "log", "log10", "log1p", "log2", "fround",
        "trunc", "cbrt", "hypot", "sign",
    ];
    for name in methods {
        object_set_key_value(mc, &math_obj, name, &Value::Function(format!("Math.{name}")))?;
        math_obj.borrow_mut(mc).set_non_enumerable(name);
    }

    // --- Symbol.toStringTag = "Math" { writable:false, enumerable:false, configurable:true } ---
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_val.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let tag_desc = crate::core::create_descriptor_object(
            mc,
            &Value::String(utf8_to_utf16("Math")),
            false, // writable
            false, // enumerable
            true,  // configurable
        )?;
        crate::js_object::define_property_internal(mc, &math_obj, PropertyKey::Symbol(*tag_sym), &tag_desc)?;
    }

    env_set(mc, env, "Math", &Value::Object(math_obj))?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Dispatch
// ═══════════════════════════════════════════════════════════════════════════════

/// Handle Math object method calls — all arguments undergo ToNumber coercion.
pub fn handle_math_call<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        // --- 1-arg functions: f(ToNumber(arg0)) ---
        "floor" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.floor()))
        }
        "ceil" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.ceil()))
        }
        "round" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(js_round(n)))
        }
        "abs" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.abs()))
        }
        "sqrt" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.sqrt()))
        }
        "sin" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.sin()))
        }
        "cos" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.cos()))
        }
        "tan" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.tan()))
        }
        "asin" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.asin()))
        }
        "acos" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.acos()))
        }
        "atan" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.atan()))
        }
        "sinh" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.sinh()))
        }
        "cosh" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.cosh()))
        }
        "tanh" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.tanh()))
        }
        "asinh" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.asinh()))
        }
        "acosh" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.acosh()))
        }
        "atanh" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.atanh()))
        }
        "exp" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.exp()))
        }
        "expm1" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.exp_m1()))
        }
        "log" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.ln()))
        }
        "log10" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.log10()))
        }
        "log1p" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.ln_1p()))
        }
        "log2" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.log2()))
        }
        "fround" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number((n as f32) as f64))
        }
        "trunc" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.trunc()))
        }
        "cbrt" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(n.cbrt()))
        }
        "sign" => {
            let n = arg_to_number(mc, env, args, 0)?;
            Ok(Value::Number(if n.is_nan() {
                f64::NAN
            } else if n == 0.0 {
                n // preserves ±0
            } else if n > 0.0 {
                1.0
            } else {
                -1.0
            }))
        }

        // --- 2-arg functions ---
        "pow" => {
            let b = arg_to_number(mc, env, args, 0)?;
            let e = arg_to_number(mc, env, args, 1)?;
            Ok(Value::Number(js_pow(b, e)))
        }
        "atan2" => {
            let y = arg_to_number(mc, env, args, 0)?;
            let x = arg_to_number(mc, env, args, 1)?;
            Ok(Value::Number(y.atan2(x)))
        }

        // --- clz32: ToUint32 then leading zeros ---
        "clz32" => {
            let n = arg_to_number(mc, env, args, 0)?;
            let u = to_uint32(n);
            Ok(Value::Number(u.leading_zeros() as f64))
        }

        // --- imul: ToUint32 both, wrapping i32 multiply ---
        "imul" => {
            let a = arg_to_number(mc, env, args, 0)?;
            let b = arg_to_number(mc, env, args, 1)?;
            let result = (to_uint32(a) as i32).wrapping_mul(to_uint32(b) as i32);
            Ok(Value::Number(result as f64))
        }

        // --- max: coerce all, then find highest; ±0 aware ---
        "max" => {
            if args.is_empty() {
                return Ok(Value::Number(f64::NEG_INFINITY));
            }
            let mut coerced: Vec<f64> = Vec::with_capacity(args.len());
            for arg in args {
                coerced.push(to_number_with_env(mc, env, arg)?);
            }
            let mut highest = f64::NEG_INFINITY;
            for n in &coerced {
                if n.is_nan() {
                    return Ok(Value::Number(f64::NAN));
                }
                if *n == 0.0 && highest == 0.0 {
                    if n.is_sign_positive() {
                        highest = 0.0; // +0 > -0
                    }
                } else if *n > highest {
                    highest = *n;
                }
            }
            Ok(Value::Number(highest))
        }

        // --- min: coerce all, then find lowest; ±0 aware ---
        "min" => {
            if args.is_empty() {
                return Ok(Value::Number(f64::INFINITY));
            }
            let mut coerced: Vec<f64> = Vec::with_capacity(args.len());
            for arg in args {
                coerced.push(to_number_with_env(mc, env, arg)?);
            }
            let mut lowest = f64::INFINITY;
            for n in &coerced {
                if n.is_nan() {
                    return Ok(Value::Number(f64::NAN));
                }
                if *n == 0.0 && lowest == 0.0 {
                    if n.is_sign_negative() {
                        lowest = -0.0; // -0 < +0
                    }
                } else if *n < lowest {
                    lowest = *n;
                }
            }
            Ok(Value::Number(lowest))
        }

        // --- hypot: Infinity beats NaN ---
        "hypot" => {
            let mut coerced: Vec<f64> = Vec::with_capacity(args.len());
            for arg in args {
                coerced.push(to_number_with_env(mc, env, arg)?);
            }
            // Infinity takes priority over NaN
            for n in &coerced {
                if n.is_infinite() {
                    return Ok(Value::Number(f64::INFINITY));
                }
            }
            for n in &coerced {
                if n.is_nan() {
                    return Ok(Value::Number(f64::NAN));
                }
            }
            let mut sum_sq = 0.0_f64;
            for n in &coerced {
                sum_sq += n * n;
            }
            Ok(Value::Number(sum_sq.sqrt()))
        }

        // --- random ---
        "random" => {
            use std::time::{SystemTime, UNIX_EPOCH};
            let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            let seed = duration.as_nanos() as u64;
            let a = 1664525u64;
            let c = 1013904223u64;
            let m = 2u64.pow(32);
            let random_u32 = ((seed.wrapping_mul(a).wrapping_add(c)) % m) as u32;
            let random_f64 = random_u32 as f64 / m as f64;
            Ok(Value::Number(random_f64))
        }

        _ => Err(raise_eval_error!(format!("Math.{method} is not implemented")).into()),
    }
}
