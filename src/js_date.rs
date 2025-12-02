use crate::core::{Expr, JSObjectData, JSObjectDataPtr, Value, evaluate_expr, obj_set_value};
use crate::error::JSError;
use crate::utf16::utf8_to_utf16;
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc};
use std::cell::RefCell;
use std::rc::Rc;

/// Parse a date string into a timestamp (milliseconds since Unix epoch)
fn parse_date_string(date_str: &str) -> Option<f64> {
    // Try ISO 8601 format first (most common)
    if let Ok(dt) = DateTime::parse_from_rfc3339(date_str) {
        return Some(dt.timestamp_millis() as f64);
    }

    // Try parsing as RFC 2822 (email format)
    if let Ok(dt) = DateTime::parse_from_rfc2822(date_str) {
        return Some(dt.timestamp_millis() as f64);
    }

    // Try common formats
    let formats = [
        "%Y-%m-%dT%H:%M:%S%.fZ", // ISO with milliseconds
        "%Y-%m-%dT%H:%M:%SZ",    // ISO without milliseconds
        "%Y-%m-%d %H:%M:%S",     // MySQL format
        "%Y/%m/%d %H:%M:%S",     // Alternative format
        "%m/%d/%Y %H:%M:%S",     // US format
        "%d/%m/%Y %H:%M:%S",     // European format
        "%Y-%m-%d",              // Date only
        "%m/%d/%Y",              // US date only
        "%d/%m/%Y",              // European date only
    ];

    for format in &formats {
        if let Ok(dt) = NaiveDateTime::parse_from_str(date_str, format) {
            let utc_dt = Utc.from_utc_datetime(&dt);
            return Some(utc_dt.timestamp_millis() as f64);
        }
    }

    // Try parsing date-only formats and set time to 00:00:00
    let date_formats = ["%Y-%m-%d", "%m/%d/%Y", "%d/%m/%Y", "%Y/%m/%d"];

    for format in &date_formats {
        if let Ok(date) = NaiveDate::parse_from_str(date_str, format) {
            let datetime = date.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
            let utc_dt = Utc.from_utc_datetime(&datetime);
            return Some(utc_dt.timestamp_millis() as f64);
        }
    }

    None
}

/// Construct a date from year, month, day, hour, minute, second, millisecond components
fn construct_date_from_components(components: &[f64]) -> Option<f64> {
    if components.is_empty() || components.len() > 7 {
        return None;
    }

    let year = components[0] as i32;
    let month = if components.len() > 1 { components[1] as u32 } else { 0 };
    let day = if components.len() > 2 { components[2] as u32 } else { 1 };
    let hour = if components.len() > 3 { components[3] as u32 } else { 0 };
    let minute = if components.len() > 4 { components[4] as u32 } else { 0 };
    let second = if components.len() > 5 { components[5] as u32 } else { 0 };
    let millisecond = if components.len() > 6 { components[6] as u32 } else { 0 };

    // JavaScript Date months are 0-based, chrono months are 1-based
    let chrono_month = month + 1;

    // Handle year conversion (JavaScript allows 2-digit years)
    let full_year = if (0..100).contains(&year) {
        if year < 50 { 2000 + year } else { 1900 + year }
    } else {
        year
    };

    // Validate ranges
    if !(1..=12).contains(&chrono_month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 59 || millisecond > 999 {
        return None;
    }

    // Try to create the date
    if let Some(date) = NaiveDate::from_ymd_opt(full_year, chrono_month, day)
        && let Some(time) = NaiveTime::from_hms_milli_opt(hour, minute, second, millisecond)
    {
        let datetime = NaiveDateTime::new(date, time);
        let utc_dt = Utc.from_utc_datetime(&datetime);
        return Some(utc_dt.timestamp_millis() as f64);
    }

    None
}

/// Handle Date constructor calls
pub(crate) fn handle_date_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = if args.is_empty() {
        // new Date() - current time
        let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        duration.as_millis() as f64
    } else if args.len() == 1 {
        // new Date(dateString) or new Date(timestamp)
        let arg_val = evaluate_expr(env, &args[0])?;
        match arg_val {
            Value::String(s) => {
                let date_str = String::from_utf16_lossy(&s);
                if let Some(timestamp) = parse_date_string(&date_str) {
                    timestamp
                } else {
                    return Err(JSError::TypeError {
                        message: "Invalid date".to_string(),
                    });
                }
            }
            Value::Number(n) => {
                // new Date(timestamp)
                n
            }
            _ => {
                return Err(JSError::TypeError {
                    message: "Invalid date".to_string(),
                });
            }
        }
    } else {
        // new Date(year, month, day, hours, minutes, seconds, milliseconds)
        let mut components = Vec::new();
        for arg in args {
            let arg_val = evaluate_expr(env, arg)?;
            match arg_val {
                Value::Number(n) => components.push(n),
                _ => {
                    return Err(JSError::TypeError {
                        message: "Date constructor arguments must be numbers".to_string(),
                    });
                }
            }
        }

        if let Some(timestamp) = construct_date_from_components(&components) {
            timestamp
        } else {
            return Err(JSError::TypeError {
                message: "Invalid date".to_string(),
            });
        }
    };

    // Create a Date object with timestamp
    let date_obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(&date_obj, &"__timestamp".into(), Value::Number(timestamp))?;

    // Add toString method
    Ok(Value::Object(date_obj))
}

/// Handle Date instance method calls
pub(crate) fn handle_date_method(obj_map: &JSObjectDataPtr, method: &str, args: &[Expr]) -> Result<Value, JSError> {
    match method {
        "toString" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.toString() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    // Convert timestamp to DateTime
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        // Convert UTC to local time
                        let local_dt = Local.from_utc_datetime(&dt.naive_utc());
                        // Format similar to JavaScript's Date.toString()
                        let formatted = local_dt.format("%a %b %d %Y %H:%M:%S GMT%z (%Z)").to_string();
                        Ok(Value::String(utf8_to_utf16(&formatted)))
                    } else {
                        Ok(Value::String(utf8_to_utf16("Invalid Date")))
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "getTime" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.getTime() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    Ok(Value::Number(timestamp))
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "valueOf" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.valueOf() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    Ok(Value::Number(timestamp))
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "getFullYear" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.getFullYear() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        Ok(Value::Number(dt.year() as f64))
                    } else {
                        Ok(Value::Number(f64::NAN))
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "getMonth" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.getMonth() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        // JavaScript months are 0-based
                        Ok(Value::Number((dt.month() - 1) as f64))
                    } else {
                        Ok(Value::Number(f64::NAN))
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "getDate" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.getDate() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        Ok(Value::Number(dt.day() as f64))
                    } else {
                        Ok(Value::Number(f64::NAN))
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "getHours" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.getHours() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        Ok(Value::Number(dt.hour() as f64))
                    } else {
                        Ok(Value::Number(f64::NAN))
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "getMinutes" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.getMinutes() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        Ok(Value::Number(dt.minute() as f64))
                    } else {
                        Ok(Value::Number(f64::NAN))
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "getSeconds" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.getSeconds() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        Ok(Value::Number(dt.second() as f64))
                    } else {
                        Ok(Value::Number(f64::NAN))
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "getMilliseconds" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.getMilliseconds() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        Ok(Value::Number(dt.timestamp_subsec_millis() as f64))
                    } else {
                        Ok(Value::Number(f64::NAN))
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "toDateString" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.toDateString() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        let local_dt = Local.from_utc_datetime(&dt.naive_utc());
                        let formatted = local_dt.format("%a %b %d %Y").to_string();
                        Ok(Value::String(utf8_to_utf16(&formatted)))
                    } else {
                        Ok(Value::String(utf8_to_utf16("Invalid Date")))
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "toTimeString" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.toTimeString() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        let local_dt = Local.from_utc_datetime(&dt.naive_utc());
                        let formatted = local_dt.format("%H:%M:%S GMT%z").to_string();
                        Ok(Value::String(utf8_to_utf16(&formatted)))
                    } else {
                        Ok(Value::String(utf8_to_utf16("Invalid Date")))
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "toISOString" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.toISOString() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        let formatted = dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
                        Ok(Value::String(utf8_to_utf16(&formatted)))
                    } else {
                        Err(JSError::TypeError {
                            message: "Invalid time value".to_string(),
                        })
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "toUTCString" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.toUTCString() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        let formatted = dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
                        Ok(Value::String(utf8_to_utf16(&formatted)))
                    } else {
                        Ok(Value::String(utf8_to_utf16("Invalid Date")))
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "toJSON" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.toJSON() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if timestamp.is_nan() {
                        Ok(Value::Undefined)
                    } else if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        let formatted = dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
                        Ok(Value::String(utf8_to_utf16(&formatted)))
                    } else {
                        Ok(Value::Undefined)
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "toLocaleString" => {
            // For simplicity, we'll use the same format as toString()
            // In a real implementation, this would use locale-specific formatting
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.toLocaleString() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        let local_dt = Local.from_utc_datetime(&dt.naive_utc());
                        let formatted = local_dt.format("%a %b %d %Y %H:%M:%S GMT%z").to_string();
                        Ok(Value::String(utf8_to_utf16(&formatted)))
                    } else {
                        Ok(Value::String(utf8_to_utf16("Invalid Date")))
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "toLocaleDateString" => {
            // For simplicity, we'll use the same format as toDateString()
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.toLocaleDateString() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        let local_dt = Local.from_utc_datetime(&dt.naive_utc());
                        let formatted = local_dt.format("%a %b %d %Y").to_string();
                        Ok(Value::String(utf8_to_utf16(&formatted)))
                    } else {
                        Ok(Value::String(utf8_to_utf16("Invalid Date")))
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        "toLocaleTimeString" => {
            // For simplicity, we'll use the same format as toTimeString()
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.toLocaleTimeString() takes no arguments".to_string(),
                });
            }
            if let Some(timestamp_val) = obj_map.borrow().get(&"__timestamp".into()) {
                if let Value::Number(timestamp) = *timestamp_val.borrow() {
                    if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                        let local_dt = Local.from_utc_datetime(&dt.naive_utc());
                        let formatted = local_dt.format("%H:%M:%S GMT%z").to_string();
                        Ok(Value::String(utf8_to_utf16(&formatted)))
                    } else {
                        Ok(Value::String(utf8_to_utf16("Invalid Date")))
                    }
                } else {
                    Err(JSError::TypeError {
                        message: "Invalid Date object".to_string(),
                    })
                }
            } else {
                Err(JSError::TypeError {
                    message: "Invalid Date object".to_string(),
                })
            }
        }
        _ => Err(JSError::EvaluationError {
            message: format!("Date has no method '{method}'"),
        }),
    }
}

/// Handle Date static method calls
pub(crate) fn handle_date_static_method(method: &str, args: &[Expr], _env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "now" => {
            if !args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Date.now() takes no arguments".to_string(),
                });
            }
            use std::time::{SystemTime, UNIX_EPOCH};
            let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            Ok(Value::Number(duration.as_millis() as f64))
        }
        _ => Err(JSError::EvaluationError {
            message: format!("Date has no static method '{}'", method),
        }),
    }
}
