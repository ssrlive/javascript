use crate::core::{Expr, JSObjectData, JSObjectDataPtr, Value, evaluate_expr, obj_set_value};
use crate::error::JSError;
use std::cell::RefCell;
use std::rc::Rc;

/// Create the Math object with all mathematical constants and functions
pub fn make_math_object() -> Result<JSObjectDataPtr, JSError> {
    let math_obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(&math_obj, "PI", Value::Number(std::f64::consts::PI))?;
    obj_set_value(&math_obj, "E", Value::Number(std::f64::consts::E))?;
    obj_set_value(&math_obj, "floor", Value::Function("Math.floor".to_string()))?;
    obj_set_value(&math_obj, "ceil", Value::Function("Math.ceil".to_string()))?;
    obj_set_value(&math_obj, "round", Value::Function("Math.round".to_string()))?;
    obj_set_value(&math_obj, "abs", Value::Function("Math.abs".to_string()))?;
    obj_set_value(&math_obj, "sqrt", Value::Function("Math.sqrt".to_string()))?;
    obj_set_value(&math_obj, "pow", Value::Function("Math.pow".to_string()))?;
    obj_set_value(&math_obj, "sin", Value::Function("Math.sin".to_string()))?;
    obj_set_value(&math_obj, "cos", Value::Function("Math.cos".to_string()))?;
    obj_set_value(&math_obj, "tan", Value::Function("Math.tan".to_string()))?;
    obj_set_value(&math_obj, "random", Value::Function("Math.random".to_string()))?;
    obj_set_value(&math_obj, "clz32", Value::Function("Math.clz32".to_string()))?;
    obj_set_value(&math_obj, "imul", Value::Function("Math.imul".to_string()))?;
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
                    Err(JSError::EvaluationError {
                        message: "Math.floor expects a number".to_string(),
                    })
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Math.floor expects exactly one argument".to_string(),
                })
            }
        }
        "ceil" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.ceil()))
                } else {
                    Err(JSError::EvaluationError {
                        message: "Math.ceil expects a number".to_string(),
                    })
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Math.ceil expects exactly one argument".to_string(),
                })
            }
        }
        "round" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.round()))
                } else {
                    Err(JSError::EvaluationError {
                        message: "Math.round expects a number".to_string(),
                    })
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Math.round expects exactly one argument".to_string(),
                })
            }
        }
        "abs" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.abs()))
                } else {
                    Err(JSError::EvaluationError {
                        message: "Math.abs expects a number".to_string(),
                    })
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Math.abs expects exactly one argument".to_string(),
                })
            }
        }
        "sqrt" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.sqrt()))
                } else {
                    Err(JSError::EvaluationError {
                        message: "Math.sqrt expects a number".to_string(),
                    })
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Math.sqrt expects exactly one argument".to_string(),
                })
            }
        }
        "pow" => {
            if args.len() == 2 {
                let base_val = evaluate_expr(env, &args[0])?;
                let exp_val = evaluate_expr(env, &args[1])?;
                if let (Value::Number(base), Value::Number(exp)) = (base_val, exp_val) {
                    Ok(Value::Number(base.powf(exp)))
                } else {
                    Err(JSError::EvaluationError {
                        message: "Math.pow expects two numbers".to_string(),
                    })
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Math.pow expects exactly two arguments".to_string(),
                })
            }
        }
        "sin" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.sin()))
                } else {
                    Err(JSError::EvaluationError {
                        message: "Math.sin expects a number".to_string(),
                    })
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Math.sin expects exactly one argument".to_string(),
                })
            }
        }
        "cos" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.cos()))
                } else {
                    Err(JSError::EvaluationError {
                        message: "Math.cos expects a number".to_string(),
                    })
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Math.cos expects exactly one argument".to_string(),
                })
            }
        }
        "tan" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = arg_val {
                    Ok(Value::Number(n.tan()))
                } else {
                    Err(JSError::EvaluationError {
                        message: "Math.tan expects a number".to_string(),
                    })
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Math.tan expects exactly one argument".to_string(),
                })
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
                Err(JSError::EvaluationError {
                    message: "Math.random expects no arguments".to_string(),
                })
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
                    Err(JSError::EvaluationError {
                        message: "Math.clz32 expects a number".to_string(),
                    })
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Math.clz32 expects exactly one argument".to_string(),
                })
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
                    Err(JSError::EvaluationError {
                        message: "Math.imul expects two numbers".to_string(),
                    })
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Math.imul expects exactly two arguments".to_string(),
                })
            }
        }
        _ => Err(JSError::EvaluationError {
            message: format!("Math.{method} is not implemented"),
        }),
    }
}
