use crate::error::JSError;
use crate::quickjs::JSObjectData;
use crate::quickjs::{evaluate_expr, utf16_to_utf8, utf8_to_utf16, Expr, Value};

pub(crate) fn handle_sprintf_call(env: &JSObjectData, args: &[Expr]) -> Result<Value, JSError> {
    if args.is_empty() {
        return Ok(Value::String(utf8_to_utf16("")));
    }
    let format_val = evaluate_expr(env, &args[0])?;
    let format_str = match format_val {
        Value::String(s) => utf16_to_utf8(&s),
        _ => {
            return Err(JSError::EvaluationError {
                message: "sprintf format must be a string".to_string(),
            })
        }
    };
    let result = sprintf_impl(env, &format_str, &args[1..])?;
    Ok(Value::String(utf8_to_utf16(&result)))
}

pub fn sprintf_impl(env: &JSObjectData, format: &str, args: &[Expr]) -> Result<String, JSError> {
    let mut result = String::new();
    let mut arg_index = 0;
    let mut chars = format.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            if let Some(next_ch) = chars.peek() {
                if *next_ch == '%' {
                    result.push('%');
                    chars.next();
                    continue;
                }
            }

            // Parse format specifier
            let mut width = None;
            let mut precision = None;
            let mut flags = String::new();
            let mut length_modifier = String::new();

            // Parse flags
            while let Some(&ch) = chars.peek() {
                if ch == '-' || ch == '+' || ch == ' ' || ch == '#' || ch == '0' {
                    flags.push(ch);
                    chars.next();
                } else {
                    break;
                }
            }

            // Parse width
            if let Some(&ch) = chars.peek() {
                if ch == '*' {
                    chars.next();
                    if arg_index < args.len() {
                        let width_val = evaluate_expr(env, &args[arg_index])?;
                        if let Value::Number(n) = width_val {
                            width = Some(n as usize);
                        }
                        arg_index += 1;
                    }
                } else if ch.is_ascii_digit() {
                    let mut w = 0;
                    while let Some(&ch) = chars.peek() {
                        if ch.is_ascii_digit() {
                            w = w * 10 + (ch as u32 - '0' as u32) as usize;
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    width = Some(w);
                }
            }

            // Parse precision
            if let Some(&ch) = chars.peek() {
                if ch == '.' {
                    chars.next();
                    if let Some(&ch) = chars.peek() {
                        if ch == '*' {
                            chars.next();
                            if arg_index < args.len() {
                                let prec_val = evaluate_expr(env, &args[arg_index])?;
                                if let Value::Number(n) = prec_val {
                                    precision = Some(n as usize);
                                }
                                arg_index += 1;
                            }
                        } else if ch.is_ascii_digit() {
                            let mut p = 0;
                            while let Some(&ch) = chars.peek() {
                                if ch.is_ascii_digit() {
                                    p = p * 10 + (ch as u32 - '0' as u32) as usize;
                                    chars.next();
                                } else {
                                    break;
                                }
                            }
                            precision = Some(p);
                        }
                    }
                }
            }

            // Parse length modifier
            while let Some(&ch) = chars.peek() {
                if ch == 'l' || ch == 'L' || ch == 'h' || ch == 'j' || ch == 'z' || ch == 't' {
                    length_modifier.push(ch);
                    chars.next();
                } else {
                    break;
                }
            }

            // Parse conversion specifier
            if let Some(specifier) = chars.next() {
                if arg_index >= args.len() {
                    return Err(JSError::EvaluationError {
                        message: "Not enough arguments for sprintf".to_string(),
                    });
                }

                // Get the argument value
                let arg_val = evaluate_expr(env, &args[arg_index])?;

                match specifier {
                    'd' | 'i' => {
                        // Integer
                        let val = match arg_val {
                            Value::Number(n) => n as i64,
                            Value::Boolean(b) => {
                                if b {
                                    1
                                } else {
                                    0
                                }
                            }
                            _ => 0,
                        };
                        let formatted = if let Some(w) = width {
                            if flags.contains('0') {
                                format!("{:0>width$}", val, width = w)
                            } else {
                                format!("{:>width$}", val, width = w)
                            }
                        } else {
                            val.to_string()
                        };
                        result.push_str(&formatted);
                    }
                    'x' | 'X' => {
                        // Hexadecimal
                        let val = match arg_val {
                            Value::Number(n) => n as i64,
                            _ => 0,
                        };
                        let formatted = if length_modifier == "l" {
                            if flags.contains('#') {
                                format!("0x{:x}", val as u64)
                            } else {
                                format!("{:x}", val as u64)
                            }
                        } else {
                            format!("{:x}", val as u32)
                        };
                        result.push_str(&formatted);
                    }
                    'f' | 'F' | 'e' | 'E' | 'g' | 'G' => {
                        // Float
                        let val = match arg_val {
                            Value::Number(n) => n,
                            _ => 0.0,
                        };
                        let formatted = if let Some(w) = width {
                            if let Some(p) = precision {
                                format!("{:>width$.precision$}", val, width = w, precision = p)
                            } else {
                                format!("{:>width$}", val, width = w)
                            }
                        } else if let Some(p) = precision {
                            format!("{:.precision$}", val, precision = p)
                        } else {
                            val.to_string()
                        };
                        result.push_str(&formatted);
                    }
                    's' => {
                        // String
                        let val = match arg_val {
                            Value::String(s) => utf16_to_utf8(&s),
                            Value::Number(n) => n.to_string(),
                            Value::Boolean(b) => b.to_string(),
                            _ => "".to_string(),
                        };
                        let formatted = if let Some(w) = width {
                            format!("{:>width$}", val, width = w)
                        } else {
                            val
                        };
                        result.push_str(&formatted);
                    }
                    'c' => {
                        // Character
                        let val = match arg_val {
                            Value::Number(n) => n as u8 as char,
                            _ => '?',
                        };
                        result.push(val);
                    }
                    _ => {
                        // Unknown specifier, just output as is
                        result.push('%');
                        result.push(specifier);
                    }
                }
                arg_index += 1;
            }
        } else {
            result.push(ch);
        }
    }

    Ok(result)
}
