use crate::core::{Expr, JSObjectData, JSObjectDataPtr, Value, evaluate_expr, obj_set_value};
use crate::error::JSError;
use crate::utf16::{utf8_to_utf16, utf16_to_utf8};
use regex::RegexBuilder;
use std::cell::RefCell;
use std::rc::Rc;

/// Handle RegExp constructor calls
pub(crate) fn handle_regexp_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let (pattern, flags) = if args.is_empty() {
        // new RegExp() - empty regex
        ("".to_string(), "".to_string())
    } else if args.len() == 1 {
        // new RegExp(pattern)
        let pattern_val = evaluate_expr(env, &args[0])?;
        let pattern = match pattern_val {
            Value::String(s) => utf16_to_utf8(&s),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            _ => {
                return Err(JSError::TypeError {
                    message: "Invalid RegExp pattern".to_string(),
                });
            }
        };
        (pattern, "".to_string())
    } else {
        // new RegExp(pattern, flags)
        let pattern_val = evaluate_expr(env, &args[0])?;
        let flags_val = evaluate_expr(env, &args[1])?;

        let pattern = match pattern_val {
            Value::String(s) => utf16_to_utf8(&s),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            _ => {
                return Err(JSError::TypeError {
                    message: "Invalid RegExp pattern".to_string(),
                });
            }
        };

        let flags = match flags_val {
            Value::String(s) => utf16_to_utf8(&s),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            _ => {
                return Err(JSError::TypeError {
                    message: "Invalid RegExp flags".to_string(),
                });
            }
        };

        (pattern, flags)
    };

    // Build regex with flags
    let regex_pattern = pattern.clone();
    let _regex_flags = regex::RegexBuilder::new("");

    // Parse flags
    let mut global = false;
    let mut ignore_case = false;
    let mut multiline = false;
    let mut dot_matches_new_line = false;
    let mut swap_greed = false;
    let mut unicode = false;
    let mut crlf = false;
    let mut case_insensitive = false;

    for flag in flags.chars() {
        match flag {
            'g' => global = true,
            'i' => {
                ignore_case = true;
                case_insensitive = true;
            }
            'm' => multiline = true,
            's' => dot_matches_new_line = true,
            'U' => swap_greed = true,
            'u' => unicode = true,
            'R' => crlf = true,
            _ => {
                return Err(JSError::SyntaxError {
                    message: format!("Invalid RegExp flag: {flag}"),
                });
            }
        }
    }

    // Validate the regex pattern by trying to compile it
    if let Err(e) = RegexBuilder::new(&regex_pattern)
        .case_insensitive(case_insensitive)
        .multi_line(multiline)
        .dot_matches_new_line(dot_matches_new_line)
        .swap_greed(swap_greed)
        .crlf(crlf)
        .unicode(unicode)
        .build()
    {
        return Err(JSError::SyntaxError {
            message: format!("Invalid RegExp: {}", e),
        });
    }

    // Create RegExp object
    let regexp_obj = Rc::new(RefCell::new(JSObjectData::new()));

    // Store regex and flags as properties
    obj_set_value(&regexp_obj, &"__regex".into(), Value::String(utf8_to_utf16(&pattern)))?;
    obj_set_value(&regexp_obj, &"__flags".into(), Value::String(utf8_to_utf16(&flags)))?;
    obj_set_value(&regexp_obj, &"__global".into(), Value::Boolean(global))?;
    obj_set_value(&regexp_obj, &"__ignoreCase".into(), Value::Boolean(ignore_case))?;
    obj_set_value(&regexp_obj, &"__multiline".into(), Value::Boolean(multiline))?;
    obj_set_value(&regexp_obj, &"__dotAll".into(), Value::Boolean(dot_matches_new_line))?;
    obj_set_value(&regexp_obj, &"__unicode".into(), Value::Boolean(unicode))?;
    obj_set_value(&regexp_obj, &"__sticky".into(), Value::Boolean(false))?; // Not implemented
    obj_set_value(&regexp_obj, &"__lastIndex".into(), Value::Number(0.0))?;

    // Add methods
    obj_set_value(&regexp_obj, &"exec".into(), Value::Function("RegExp.prototype.exec".to_string()))?;
    obj_set_value(&regexp_obj, &"test".into(), Value::Function("RegExp.prototype.test".to_string()))?;
    obj_set_value(
        &regexp_obj,
        &"toString".into(),
        Value::Function("RegExp.prototype.toString".to_string()),
    )?;

    Ok(Value::Object(regexp_obj))
}

/// Handle RegExp instance method calls
pub(crate) fn handle_regexp_method(
    obj_map: &JSObjectDataPtr,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    match method {
        "exec" => {
            if args.is_empty() {
                return Err(JSError::TypeError {
                    message: "RegExp.prototype.exec requires a string argument".to_string(),
                });
            }

            let input_val = evaluate_expr(env, &args[0])?;
            let input = match input_val {
                Value::String(s) => utf16_to_utf8(&s),
                _ => {
                    return Err(JSError::TypeError {
                        message: "RegExp.prototype.exec requires a string argument".to_string(),
                    });
                }
            };

            // Get regex pattern and flags
            let pattern = match obj_map.borrow().get(&"__regex".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => {
                        return Err(JSError::TypeError {
                            message: "Invalid regex pattern".to_string(),
                        });
                    }
                },
                None => {
                    return Err(JSError::TypeError {
                        message: "Invalid regex object".to_string(),
                    });
                }
            };

            let flags = match obj_map.borrow().get(&"__flags".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => "".to_string(),
                },
                None => "".to_string(),
            };

            // Build regex
            let mut regex_builder = regex::RegexBuilder::new(&pattern);
            if flags.contains('i') {
                regex_builder.case_insensitive(true);
            }
            if flags.contains('m') {
                regex_builder.multi_line(true);
            }
            if flags.contains('s') {
                regex_builder.dot_matches_new_line(true);
            }

            let regex = regex_builder.build().map_err(|e| JSError::SyntaxError {
                message: format!("Invalid RegExp: {e}"),
            })?;

            // Get lastIndex for global regex
            let mut last_index = 0;
            let global = flags.contains('g');
            if global
                && let Some(last_index_val) = obj_map.borrow().get(&"__lastIndex".into())
                && let Value::Number(n) = &*last_index_val.borrow()
            {
                last_index = *n as usize;
            }

            // Execute regex
            if let Some(captures) = regex.captures(&input[last_index..]) {
                // Create result array
                let result_array = Rc::new(RefCell::new(JSObjectData::new()));

                // Add matched string
                if let Some(matched) = captures.get(0) {
                    obj_set_value(&result_array, &"0".into(), Value::String(utf8_to_utf16(matched.as_str())))?;
                    obj_set_value(&result_array, &"index".into(), Value::Number((last_index + matched.start()) as f64))?;
                    obj_set_value(&result_array, &"input".into(), Value::String(utf8_to_utf16(&input)))?;
                }

                // Add capture groups
                let mut group_index = 1;
                for capture in captures.iter().skip(1) {
                    if let Some(capture_match) = capture {
                        obj_set_value(
                            &result_array,
                            &group_index.to_string().into(),
                            Value::String(utf8_to_utf16(capture_match.as_str())),
                        )?;
                    } else {
                        obj_set_value(&result_array, &group_index.to_string().into(), Value::Undefined)?;
                    }
                    group_index += 1;
                }

                // Set length
                obj_set_value(&result_array, &"length".into(), Value::Number(group_index as f64))?;

                // Update lastIndex for global regex
                if global && let Some(matched) = captures.get(0) {
                    let new_last_index = last_index + matched.end();
                    obj_set_value(obj_map, &"__lastIndex".into(), Value::Number(new_last_index as f64))?;
                }

                Ok(Value::Object(result_array))
            } else {
                // Reset lastIndex for global regex on no match
                if global {
                    obj_set_value(obj_map, &"__lastIndex".into(), Value::Number(0.0))?;
                }
                Ok(Value::Undefined) // RegExp.exec returns null on no match, but we use Undefined
            }
        }
        "test" => {
            if args.is_empty() {
                return Err(JSError::TypeError {
                    message: "RegExp.prototype.test requires a string argument".to_string(),
                });
            }

            let input_val = evaluate_expr(env, &args[0])?;
            let input = match input_val {
                Value::String(s) => utf16_to_utf8(&s),
                _ => {
                    return Err(JSError::TypeError {
                        message: "RegExp.prototype.test requires a string argument".to_string(),
                    });
                }
            };

            // Get regex pattern and flags
            let pattern = match obj_map.borrow().get(&"__regex".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => {
                        return Err(JSError::TypeError {
                            message: "Invalid regex pattern".to_string(),
                        });
                    }
                },
                None => {
                    return Err(JSError::TypeError {
                        message: "Invalid regex object".to_string(),
                    });
                }
            };

            let flags = match obj_map.borrow().get(&"__flags".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => "".to_string(),
                },
                None => "".to_string(),
            };

            // Build regex
            let mut regex_builder = regex::RegexBuilder::new(&pattern);
            if flags.contains('i') {
                regex_builder.case_insensitive(true);
            }
            if flags.contains('m') {
                regex_builder.multi_line(true);
            }
            if flags.contains('s') {
                regex_builder.dot_matches_new_line(true);
            }

            let regex = regex_builder.build().map_err(|e| JSError::SyntaxError {
                message: format!("Invalid RegExp: {}", e),
            })?;

            // Get lastIndex for global regex
            let mut last_index = 0;
            let global = flags.contains('g');
            if global
                && let Some(last_index_val) = obj_map.borrow().get(&"__lastIndex".into())
                && let Value::Number(n) = &*last_index_val.borrow()
            {
                last_index = *n as usize;
            }

            // Test regex
            let is_match = regex.is_match(&input[last_index..]);

            // Update lastIndex for global regex
            if global && is_match {
                if let Some(mat) = regex.find(&input[last_index..]) {
                    let new_last_index = last_index + mat.end();
                    obj_set_value(obj_map, &"__lastIndex".into(), Value::Number(new_last_index as f64))?;
                }
            } else if global && !is_match {
                obj_set_value(obj_map, &"__lastIndex".into(), Value::Number(0.0))?;
            }

            Ok(Value::Boolean(is_match))
        }
        "toString" => {
            // Get pattern and flags
            let pattern = match obj_map.borrow().get(&"__regex".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => "".to_string(),
                },
                None => "".to_string(),
            };

            let flags = match obj_map.borrow().get(&"__flags".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => "".to_string(),
                },
                None => "".to_string(),
            };

            let result = format!("/{}/{}", pattern, flags);
            Ok(Value::String(utf8_to_utf16(&result)))
        }
        _ => Err(JSError::EvaluationError {
            message: format!("RegExp.prototype.{method} is not implemented"),
        }),
    }
}
