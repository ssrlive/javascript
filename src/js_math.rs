use crate::core::MutationContext;
use crate::core::{JSObjectDataPtr, Value, env_set, new_js_object_data, object_set_key_value};
use crate::error::JSError;

/// Create the Math object with all mathematical constants and functions
pub fn initialize_math<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let math_obj = new_js_object_data(mc);
    object_set_key_value(mc, &math_obj, "PI", Value::Number(std::f64::consts::PI))?;
    math_obj.borrow_mut(mc).set_non_configurable("PI");
    math_obj.borrow_mut(mc).set_non_writable("PI");

    object_set_key_value(mc, &math_obj, "E", Value::Number(std::f64::consts::E))?;
    math_obj.borrow_mut(mc).set_non_configurable("E");
    math_obj.borrow_mut(mc).set_non_writable("E");

    object_set_key_value(mc, &math_obj, "LN2", Value::Number(std::f64::consts::LN_2))?;
    math_obj.borrow_mut(mc).set_non_configurable("LN2");
    math_obj.borrow_mut(mc).set_non_writable("LN2");

    object_set_key_value(mc, &math_obj, "LN10", Value::Number(std::f64::consts::LN_10))?;
    math_obj.borrow_mut(mc).set_non_configurable("LN10");
    math_obj.borrow_mut(mc).set_non_writable("LN10");

    object_set_key_value(mc, &math_obj, "LOG2E", Value::Number(std::f64::consts::LOG2_E))?;
    math_obj.borrow_mut(mc).set_non_configurable("LOG2E");
    math_obj.borrow_mut(mc).set_non_writable("LOG2E");

    object_set_key_value(mc, &math_obj, "LOG10E", Value::Number(std::f64::consts::LOG10_E))?;
    math_obj.borrow_mut(mc).set_non_configurable("LOG10E");
    math_obj.borrow_mut(mc).set_non_writable("LOG10E");

    object_set_key_value(mc, &math_obj, "SQRT1_2", Value::Number(std::f64::consts::FRAC_1_SQRT_2))?;
    math_obj.borrow_mut(mc).set_non_configurable("SQRT1_2");
    math_obj.borrow_mut(mc).set_non_writable("SQRT1_2");
    object_set_key_value(mc, &math_obj, "SQRT2", Value::Number(std::f64::consts::SQRT_2))?;
    math_obj.borrow_mut(mc).set_non_configurable("SQRT2");
    math_obj.borrow_mut(mc).set_non_writable("SQRT2");

    object_set_key_value(mc, &math_obj, "floor", Value::Function("Math.floor".to_string()))?;
    object_set_key_value(mc, &math_obj, "ceil", Value::Function("Math.ceil".to_string()))?;
    object_set_key_value(mc, &math_obj, "round", Value::Function("Math.round".to_string()))?;
    object_set_key_value(mc, &math_obj, "abs", Value::Function("Math.abs".to_string()))?;
    object_set_key_value(mc, &math_obj, "sqrt", Value::Function("Math.sqrt".to_string()))?;
    object_set_key_value(mc, &math_obj, "pow", Value::Function("Math.pow".to_string()))?;
    object_set_key_value(mc, &math_obj, "sin", Value::Function("Math.sin".to_string()))?;
    object_set_key_value(mc, &math_obj, "cos", Value::Function("Math.cos".to_string()))?;
    object_set_key_value(mc, &math_obj, "tan", Value::Function("Math.tan".to_string()))?;
    object_set_key_value(mc, &math_obj, "random", Value::Function("Math.random".to_string()))?;
    object_set_key_value(mc, &math_obj, "clz32", Value::Function("Math.clz32".to_string()))?;
    object_set_key_value(mc, &math_obj, "imul", Value::Function("Math.imul".to_string()))?;
    object_set_key_value(mc, &math_obj, "max", Value::Function("Math.max".to_string()))?;
    object_set_key_value(mc, &math_obj, "min", Value::Function("Math.min".to_string()))?;
    object_set_key_value(mc, &math_obj, "asin", Value::Function("Math.asin".to_string()))?;
    object_set_key_value(mc, &math_obj, "acos", Value::Function("Math.acos".to_string()))?;
    object_set_key_value(mc, &math_obj, "atan", Value::Function("Math.atan".to_string()))?;
    object_set_key_value(mc, &math_obj, "atan2", Value::Function("Math.atan2".to_string()))?;
    object_set_key_value(mc, &math_obj, "sinh", Value::Function("Math.sinh".to_string()))?;
    object_set_key_value(mc, &math_obj, "cosh", Value::Function("Math.cosh".to_string()))?;
    object_set_key_value(mc, &math_obj, "tanh", Value::Function("Math.tanh".to_string()))?;
    object_set_key_value(mc, &math_obj, "asinh", Value::Function("Math.asinh".to_string()))?;
    object_set_key_value(mc, &math_obj, "acosh", Value::Function("Math.acosh".to_string()))?;
    object_set_key_value(mc, &math_obj, "atanh", Value::Function("Math.atanh".to_string()))?;
    object_set_key_value(mc, &math_obj, "exp", Value::Function("Math.exp".to_string()))?;
    object_set_key_value(mc, &math_obj, "expm1", Value::Function("Math.expm1".to_string()))?;
    object_set_key_value(mc, &math_obj, "log", Value::Function("Math.log".to_string()))?;
    object_set_key_value(mc, &math_obj, "log10", Value::Function("Math.log10".to_string()))?;
    object_set_key_value(mc, &math_obj, "log1p", Value::Function("Math.log1p".to_string()))?;
    object_set_key_value(mc, &math_obj, "log2", Value::Function("Math.log2".to_string()))?;
    object_set_key_value(mc, &math_obj, "fround", Value::Function("Math.fround".to_string()))?;
    object_set_key_value(mc, &math_obj, "trunc", Value::Function("Math.trunc".to_string()))?;
    object_set_key_value(mc, &math_obj, "cbrt", Value::Function("Math.cbrt".to_string()))?;
    object_set_key_value(mc, &math_obj, "hypot", Value::Function("Math.hypot".to_string()))?;
    object_set_key_value(mc, &math_obj, "sign", Value::Function("Math.sign".to_string()))?;

    env_set(mc, env, "Math", Value::Object(math_obj))?;
    Ok(())
}

/// Handle Math object method calls
pub fn handle_math_call<'gc>(
    _mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    match method {
        "floor" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.floor()))
                } else {
                    Err(raise_eval_error!("Math.floor expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.floor expects exactly one argument"))
            }
        }
        "ceil" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.ceil()))
                } else {
                    Err(raise_eval_error!("Math.ceil expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.ceil expects exactly one argument"))
            }
        }
        "round" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.round()))
                } else {
                    Err(raise_eval_error!("Math.round expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.round expects exactly one argument"))
            }
        }
        "abs" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.abs()))
                } else {
                    Err(raise_eval_error!("Math.abs expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.abs expects exactly one argument"))
            }
        }
        "sqrt" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.sqrt()))
                } else {
                    Err(raise_eval_error!("Math.sqrt expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.sqrt expects exactly one argument"))
            }
        }
        "pow" => {
            if args.len() == 2 {
                let base_val = &args[0];
                let exp_val = &args[1];
                if let (Value::Number(base), Value::Number(exp)) = (base_val, exp_val) {
                    Ok(Value::Number(base.powf(*exp)))
                } else {
                    Err(raise_eval_error!("Math.pow expects two numbers"))
                }
            } else {
                Err(raise_eval_error!("Math.pow expects exactly two arguments"))
            }
        }
        "sin" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.sin()))
                } else {
                    Err(raise_eval_error!("Math.sin expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.sin expects exactly one argument"))
            }
        }
        "cos" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.cos()))
                } else {
                    Err(raise_eval_error!("Math.cos expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.cos expects exactly one argument"))
            }
        }
        "tan" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.tan()))
                } else {
                    Err(raise_eval_error!("Math.tan expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.tan expects exactly one argument"))
            }
        }
        "random" => {
            if args.is_empty() {
                use std::time::{SystemTime, UNIX_EPOCH};
                let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                let seed = duration.as_nanos() as u64;
                // Simple linear congruential generator for random number
                let a = 1664525u64;
                let c = 1013904223u64;
                let m = 2u64.pow(32);
                let random_u32 = ((seed.wrapping_mul(a).wrapping_add(c)) % m) as u32;
                let random_f64 = random_u32 as f64 / m as f64;
                Ok(Value::Number(random_f64))
            } else {
                Err(raise_eval_error!("Math.random expects no arguments"))
            }
        }
        "clz32" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    // Convert to u32, handling NaN and Infinity
                    let u32_val = if n.is_nan() || n.is_infinite() { 0u32 } else { (*n as i32) as u32 };
                    let leading_zeros = u32_val.leading_zeros();
                    Ok(Value::Number(leading_zeros as f64))
                } else {
                    Err(raise_eval_error!("Math.clz32 expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.clz32 expects exactly one argument"))
            }
        }
        "imul" => {
            if args.len() == 2 {
                let a_val = &args[0];
                let b_val = &args[1];
                if let (Value::Number(a), Value::Number(b)) = (a_val, b_val) {
                    // Convert to i32 and multiply, then convert back to f64
                    let a_i32 = *a as i32;
                    let b_i32 = *b as i32;
                    let result_i32 = a_i32.wrapping_mul(b_i32);
                    Ok(Value::Number(result_i32 as f64))
                } else {
                    Err(raise_eval_error!("Math.imul expects two numbers"))
                }
            } else {
                Err(raise_eval_error!("Math.imul expects exactly two arguments"))
            }
        }
        "max" => {
            if args.is_empty() {
                Ok(Value::Number(f64::NEG_INFINITY))
            } else {
                let mut max_val = f64::NEG_INFINITY;
                for arg in args {
                    let arg_val = arg;
                    if let Value::Number(n) = arg_val {
                        if n.is_nan() {
                            return Ok(Value::Number(f64::NAN));
                        }
                        if *n > max_val {
                            max_val = *n;
                        }
                    } else {
                        // If any argument is not a number, return NaN
                        return Ok(Value::Number(f64::NAN));
                    }
                }
                Ok(Value::Number(max_val))
            }
        }
        "min" => {
            if args.is_empty() {
                Ok(Value::Number(f64::INFINITY))
            } else {
                let mut min_val = f64::INFINITY;
                for arg in args {
                    let arg_val = arg;
                    if let Value::Number(n) = arg_val {
                        if n.is_nan() {
                            return Ok(Value::Number(f64::NAN));
                        }
                        if *n < min_val {
                            min_val = *n;
                        }
                    } else {
                        // If any argument is not a number, return NaN
                        return Ok(Value::Number(f64::NAN));
                    }
                }
                Ok(Value::Number(min_val))
            }
        }
        "asin" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.asin()))
                } else {
                    Err(raise_eval_error!("Math.asin expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.asin expects exactly one argument"))
            }
        }
        "acos" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.acos()))
                } else {
                    Err(raise_eval_error!("Math.acos expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.acos expects exactly one argument"))
            }
        }
        "atan" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.atan()))
                } else {
                    Err(raise_eval_error!("Math.atan expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.atan expects exactly one argument"))
            }
        }
        "atan2" => {
            if args.len() == 2 {
                let y_val = &args[0];
                let x_val = &args[1];
                if let (Value::Number(y), Value::Number(x)) = (y_val, x_val) {
                    Ok(Value::Number(y.atan2(*x)))
                } else {
                    Err(raise_eval_error!("Math.atan2 expects two numbers"))
                }
            } else {
                Err(raise_eval_error!("Math.atan2 expects exactly two arguments"))
            }
        }
        "sinh" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.sinh()))
                } else {
                    Err(raise_eval_error!("Math.sinh expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.sinh expects exactly one argument"))
            }
        }
        "cosh" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.cosh()))
                } else {
                    Err(raise_eval_error!("Math.cosh expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.cosh expects exactly one argument"))
            }
        }
        "tanh" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.tanh()))
                } else {
                    Err(raise_eval_error!("Math.tanh expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.tanh expects exactly one argument"))
            }
        }
        "asinh" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.asinh()))
                } else {
                    Err(raise_eval_error!("Math.asinh expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.asinh expects exactly one argument"))
            }
        }
        "acosh" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.acosh()))
                } else {
                    Err(raise_eval_error!("Math.acosh expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.acosh expects exactly one argument"))
            }
        }
        "atanh" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.atanh()))
                } else {
                    Err(raise_eval_error!("Math.atanh expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.atanh expects exactly one argument"))
            }
        }
        "exp" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.exp()))
                } else {
                    Err(raise_eval_error!("Math.exp expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.exp expects exactly one argument"))
            }
        }
        "expm1" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.exp_m1()))
                } else {
                    Err(raise_eval_error!("Math.expm1 expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.expm1 expects exactly one argument"))
            }
        }
        "log" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.ln()))
                } else {
                    Err(raise_eval_error!("Math.log expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.log expects exactly one argument"))
            }
        }
        "log10" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.log10()))
                } else {
                    Err(raise_eval_error!("Math.log10 expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.log10 expects exactly one argument"))
            }
        }
        "log1p" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.ln_1p()))
                } else {
                    Err(raise_eval_error!("Math.log1p expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.log1p expects exactly one argument"))
            }
        }
        "log2" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.log2()))
                } else {
                    Err(raise_eval_error!("Math.log2 expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.log2 expects exactly one argument"))
            }
        }
        "fround" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number((*n as f32) as f64))
                } else {
                    Err(raise_eval_error!("Math.fround expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.fround expects exactly one argument"))
            }
        }
        "trunc" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.trunc()))
                } else {
                    Err(raise_eval_error!("Math.trunc expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.trunc expects exactly one argument"))
            }
        }
        "cbrt" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.cbrt()))
                } else {
                    Err(raise_eval_error!("Math.cbrt expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.cbrt expects exactly one argument"))
            }
        }
        "hypot" => {
            let mut sum_sq = 0.0;
            for arg in args {
                let arg_val = arg;
                if let Value::Number(n) = arg_val {
                    sum_sq += n * n;
                } else {
                    return Ok(Value::Number(f64::NAN));
                }
            }
            Ok(Value::Number(sum_sq.sqrt()))
        }
        "sign" => {
            if args.len() == 1 {
                let arg_val = &args[0];
                if let Value::Number(n) = arg_val {
                    if n.is_nan() {
                        Ok(Value::Number(f64::NAN))
                    } else if *n == 0.0 {
                        Ok(Value::Number(*n)) // Preserves signed zero
                    } else if *n > 0.0 {
                        Ok(Value::Number(1.0))
                    } else {
                        Ok(Value::Number(-1.0))
                    }
                } else {
                    Err(raise_eval_error!("Math.sign expects a number"))
                }
            } else {
                Err(raise_eval_error!("Math.sign expects exactly one argument"))
            }
        }
        _ => Err(raise_eval_error!(format!("Math.{method} is not implemented"))),
    }
}
