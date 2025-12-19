use crate::core::{Expr, JSObjectDataPtr, Value, evaluate_expr, new_js_object_data, obj_set_key_value};
use crate::error::JSError;

/// Create the Math object with all mathematical constants and functions
pub fn make_math_object() -> Result<JSObjectDataPtr, JSError> {
    let math_obj = new_js_object_data();
    obj_set_key_value(&math_obj, &"PI".into(), Value::Number(std::f64::consts::PI))?;
    math_obj.borrow_mut().set_non_configurable("PI".into());
    math_obj.borrow_mut().set_non_writable("PI".into());

    obj_set_key_value(&math_obj, &"E".into(), Value::Number(std::f64::consts::E))?;
    math_obj.borrow_mut().set_non_configurable("E".into());
    math_obj.borrow_mut().set_non_writable("E".into());

    obj_set_key_value(&math_obj, &"LN2".into(), Value::Number(std::f64::consts::LN_2))?;
    math_obj.borrow_mut().set_non_configurable("LN2".into());
    math_obj.borrow_mut().set_non_writable("LN2".into());

    obj_set_key_value(&math_obj, &"LN10".into(), Value::Number(std::f64::consts::LN_10))?;
    math_obj.borrow_mut().set_non_configurable("LN10".into());
    math_obj.borrow_mut().set_non_writable("LN10".into());

    obj_set_key_value(&math_obj, &"LOG2E".into(), Value::Number(std::f64::consts::LOG2_E))?;
    math_obj.borrow_mut().set_non_configurable("LOG2E".into());
    math_obj.borrow_mut().set_non_writable("LOG2E".into());

    obj_set_key_value(&math_obj, &"LOG10E".into(), Value::Number(std::f64::consts::LOG10_E))?;
    math_obj.borrow_mut().set_non_configurable("LOG10E".into());
    math_obj.borrow_mut().set_non_writable("LOG10E".into());

    obj_set_key_value(&math_obj, &"SQRT1_2".into(), Value::Number(std::f64::consts::FRAC_1_SQRT_2))?;
    math_obj.borrow_mut().set_non_configurable("SQRT1_2".into());
    math_obj.borrow_mut().set_non_writable("SQRT1_2".into());

    obj_set_key_value(&math_obj, &"SQRT2".into(), Value::Number(std::f64::consts::SQRT_2))?;
    math_obj.borrow_mut().set_non_configurable("SQRT2".into());
    math_obj.borrow_mut().set_non_writable("SQRT2".into());

    obj_set_key_value(&math_obj, &"floor".into(), Value::Function("Math.floor".to_string()))?;
    obj_set_key_value(&math_obj, &"ceil".into(), Value::Function("Math.ceil".to_string()))?;
    obj_set_key_value(&math_obj, &"round".into(), Value::Function("Math.round".to_string()))?;
    obj_set_key_value(&math_obj, &"abs".into(), Value::Function("Math.abs".to_string()))?;
    obj_set_key_value(&math_obj, &"sqrt".into(), Value::Function("Math.sqrt".to_string()))?;
    obj_set_key_value(&math_obj, &"pow".into(), Value::Function("Math.pow".to_string()))?;
    obj_set_key_value(&math_obj, &"sin".into(), Value::Function("Math.sin".to_string()))?;
    obj_set_key_value(&math_obj, &"cos".into(), Value::Function("Math.cos".to_string()))?;
    obj_set_key_value(&math_obj, &"tan".into(), Value::Function("Math.tan".to_string()))?;
    obj_set_key_value(&math_obj, &"random".into(), Value::Function("Math.random".to_string()))?;
    obj_set_key_value(&math_obj, &"clz32".into(), Value::Function("Math.clz32".to_string()))?;
    obj_set_key_value(&math_obj, &"imul".into(), Value::Function("Math.imul".to_string()))?;
    obj_set_key_value(&math_obj, &"max".into(), Value::Function("Math.max".to_string()))?;
    obj_set_key_value(&math_obj, &"min".into(), Value::Function("Math.min".to_string()))?;
    Ok(math_obj)
}

/// Handle Math object method calls
pub fn handle_math_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "floor" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
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
                let arg_val = evaluate_expr(env, &args[0])?;
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
                let arg_val = evaluate_expr(env, &args[0])?;
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
                let arg_val = evaluate_expr(env, &args[0])?;
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
                let arg_val = evaluate_expr(env, &args[0])?;
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
                let base_val = evaluate_expr(env, &args[0])?;
                let exp_val = evaluate_expr(env, &args[1])?;
                if let (Value::Number(base), Value::Number(exp)) = (base_val, exp_val) {
                    Ok(Value::Number(base.powf(exp)))
                } else {
                    Err(raise_eval_error!("Math.pow expects two numbers"))
                }
            } else {
                Err(raise_eval_error!("Math.pow expects exactly two arguments"))
            }
        }
        "sin" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
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
                let arg_val = evaluate_expr(env, &args[0])?;
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
                let arg_val = evaluate_expr(env, &args[0])?;
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
                let arg_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = arg_val {
                    // Convert to u32, handling NaN and Infinity
                    let u32_val = if n.is_nan() || n.is_infinite() { 0u32 } else { (n as i32) as u32 };
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
                let a_val = evaluate_expr(env, &args[0])?;
                let b_val = evaluate_expr(env, &args[1])?;
                if let (Value::Number(a), Value::Number(b)) = (a_val, b_val) {
                    // Convert to i32 and multiply, then convert back to f64
                    let a_i32 = a as i32;
                    let b_i32 = b as i32;
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
                    let arg_val = evaluate_expr(env, arg)?;
                    if let Value::Number(n) = arg_val {
                        if n.is_nan() {
                            return Ok(Value::Number(f64::NAN));
                        }
                        if n > max_val {
                            max_val = n;
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
                    let arg_val = evaluate_expr(env, arg)?;
                    if let Value::Number(n) = arg_val {
                        if n.is_nan() {
                            return Ok(Value::Number(f64::NAN));
                        }
                        if n < min_val {
                            min_val = n;
                        }
                    } else {
                        // If any argument is not a number, return NaN
                        return Ok(Value::Number(f64::NAN));
                    }
                }
                Ok(Value::Number(min_val))
            }
        }
        _ => Err(raise_eval_error!(format!("Math.{method} is not implemented"))),
    }
}
