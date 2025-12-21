use crate::core::{Expr, JSObjectDataPtr, Value, ValuePtr, evaluate_expr, get_own_property, new_js_object_data, obj_set_key_value};
use crate::error::JSError;
use crate::unicode::utf8_to_utf16;
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc};

/// Check if an object is a Date object
pub fn is_date_object(obj: &JSObjectDataPtr) -> bool {
    internal_get_time_stamp_value(obj).is_some()
}

fn internal_get_time_stamp_value(date_obj: &JSObjectDataPtr) -> Option<ValuePtr> {
    get_own_property(date_obj, &"__timestamp".into())
}

fn get_time_stamp_value(date_obj: &JSObjectDataPtr) -> Result<f64, JSError> {
    if let Some(timestamp_val) = internal_get_time_stamp_value(date_obj) {
        if let Value::Number(timestamp) = *timestamp_val.borrow() {
            return Ok(timestamp);
        } else {
            return Err(raise_type_error!("Timestamp value is not a number"));
        }
    }
    Err(raise_type_error!("Invalid Date object"))
}

fn set_time_stamp_value(date_obj: &JSObjectDataPtr, timestamp: f64) -> Result<(), JSError> {
    obj_set_key_value(date_obj, &"__timestamp".into(), Value::Number(timestamp))
}

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

    // Try parsing "Aug 9, 1995" format manually
    if let Some(timestamp) = parse_month_day_year(date_str) {
        return Some(timestamp);
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

/// Parse dates in "Aug 9, 1995" format
fn parse_month_day_year(date_str: &str) -> Option<f64> {
    let trimmed = date_str.trim();
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() == 3 {
        let month_str = parts[0];
        let day_str = parts[1].trim_end_matches(',');
        let year_str = parts[2];

        let month = match month_str {
            "Jan" => 1,
            "Feb" => 2,
            "Mar" => 3,
            "Apr" => 4,
            "May" => 5,
            "Jun" => 6,
            "Jul" => 7,
            "Aug" => 8,
            "Sep" => 9,
            "Oct" => 10,
            "Nov" => 11,
            "Dec" => 12,
            _ => return None,
        };

        if let (Ok(day), Ok(year)) = (day_str.parse::<u32>(), year_str.parse::<i32>())
            && let Some(date) = Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).single()
        {
            return Some(date.timestamp_millis() as f64);
        }
    }
    None
}

/// Construct a date from year, month, day, hour, minute, second, millisecond components
fn construct_date_from_components(components: &[f64]) -> Option<f64> {
    if components.is_empty() || components.len() > 7 {
        return None;
    }

    let year_val = components[0];
    let month_val = if components.len() > 1 { components[1] } else { 0.0 };
    let day_val = if components.len() > 2 { components[2] } else { 1.0 };
    let hour_val = if components.len() > 3 { components[3] } else { 0.0 };
    let minute_val = if components.len() > 4 { components[4] } else { 0.0 };
    let second_val = if components.len() > 5 { components[5] } else { 0.0 };
    let millisecond_val = if components.len() > 6 { components[6] } else { 0.0 };

    // Handle 2-digit years (0-99) -> 1900-1999
    let mut year = year_val as i32;
    if (0..=99).contains(&year) {
        year += 1900;
    }

    // Normalize month/year
    let mut year_int = year as i64;

    // Adjust year based on month overflow
    year_int += (month_val / 12.0).floor() as i64;

    let mut month_rem = (month_val % 12.0) as i64;
    if month_rem < 0 {
        month_rem += 12;
    }

    let chrono_month = (month_rem + 1) as u32;
    let chrono_year = year_int as i32;

    // Create base date at 1st of the month
    if let Some(base_date) = NaiveDate::from_ymd_opt(chrono_year, chrono_month, 1) {
        // Calculate total offset in milliseconds
        // Add (day - 1) days
        let day_offset = (day_val - 1.0) * 86_400_000.0;

        let time_ms = hour_val * 3_600_000.0 + minute_val * 60_000.0 + second_val * 1_000.0 + millisecond_val;

        let total_offset_ms = day_offset + time_ms;

        // Convert base_date to DateTime<Utc> at midnight
        if let Some(base_dt) = base_date.and_hms_opt(0, 0, 0) {
            let base_dt_utc = Utc.from_utc_datetime(&base_dt);

            // Add milliseconds
            let duration = chrono::Duration::milliseconds(total_offset_ms as i64);

            if let Some(final_dt) = base_dt_utc.checked_add_signed(duration) {
                return Some(final_dt.timestamp_millis() as f64);
            }
        }
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
                    return Err(raise_type_error!("Invalid date"));
                }
            }
            Value::Number(n) => {
                // new Date(timestamp)
                n
            }
            _ => {
                return Err(raise_type_error!("Invalid date"));
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
                    return Err(raise_type_error!("Date constructor arguments must be numbers"));
                }
            }
        }

        if let Some(timestamp) = construct_date_from_components(&components) {
            timestamp
        } else {
            return Err(raise_type_error!("Invalid date"));
        }
    };

    // Create a Date object with timestamp
    let date_obj = new_js_object_data();
    set_time_stamp_value(&date_obj, timestamp)?;

    // Set prototype
    crate::core::set_internal_prototype_from_constructor(&date_obj, env, "Date")?;

    Ok(Value::Object(date_obj))
}

/// Handle Date instance method calls
pub(crate) fn handle_date_method(obj: &JSObjectDataPtr, method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "toString" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.toString() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
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
        }
        "getTime" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.getTime() takes no arguments"));
            }
            Ok(Value::Number(get_time_stamp_value(obj)?))
        }
        "valueOf" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.valueOf() takes no arguments"));
            }
            Ok(Value::Number(get_time_stamp_value(obj)?))
        }
        "getFullYear" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.getFullYear() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                Ok(Value::Number(dt.year() as f64))
            } else {
                Ok(Value::Number(f64::NAN))
            }
        }
        "getMonth" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.getMonth() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                // JavaScript months are 0-based
                Ok(Value::Number((dt.month() - 1) as f64))
            } else {
                Ok(Value::Number(f64::NAN))
            }
        }
        "getDate" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.getDate() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                Ok(Value::Number(dt.day() as f64))
            } else {
                Ok(Value::Number(f64::NAN))
            }
        }
        "getHours" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.getHours() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                Ok(Value::Number(dt.hour() as f64))
            } else {
                Ok(Value::Number(f64::NAN))
            }
        }
        "getMinutes" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.getMinutes() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                Ok(Value::Number(dt.minute() as f64))
            } else {
                Ok(Value::Number(f64::NAN))
            }
        }
        "getSeconds" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.getSeconds() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                Ok(Value::Number(dt.second() as f64))
            } else {
                Ok(Value::Number(f64::NAN))
            }
        }
        "getMilliseconds" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.getMilliseconds() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                Ok(Value::Number(dt.timestamp_subsec_millis() as f64))
            } else {
                Ok(Value::Number(f64::NAN))
            }
        }
        "getTimezoneOffset" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.getTimezoneOffset() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                // Convert UTC instant to local datetime
                let local_dt = Local.from_utc_datetime(&dt.naive_utc());
                // Compute offset in seconds (local - utc)
                // local_dt.offset().local_minus_utc() returns seconds difference
                let offset_seconds = local_dt.offset().local_minus_utc();
                // JS getTimezoneOffset returns minutes between UTC and local time (UTC - local)
                let minutes = -((offset_seconds as f64) / 60.0);
                Ok(Value::Number(minutes))
            } else {
                Ok(Value::Number(f64::NAN))
            }
        }
        "getDay" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.getDay() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                // JavaScript getDay(): 0 = Sunday, 1 = Monday, ..., 6 = Saturday
                let weekday_num = match dt.weekday() {
                    chrono::Weekday::Sun => 0,
                    chrono::Weekday::Mon => 1,
                    chrono::Weekday::Tue => 2,
                    chrono::Weekday::Wed => 3,
                    chrono::Weekday::Thu => 4,
                    chrono::Weekday::Fri => 5,
                    chrono::Weekday::Sat => 6,
                };
                Ok(Value::Number(weekday_num as f64))
            } else {
                Ok(Value::Number(f64::NAN))
            }
        }
        "setFullYear" => {
            if args.is_empty() || args.len() > 3 {
                return Err(raise_type_error!("Date.setFullYear() takes 1 to 3 arguments"));
            }

            // Get current timestamp
            let current_timestamp = get_time_stamp_value(obj)?;
            let current_dt = Utc
                .timestamp_millis_opt(current_timestamp as i64)
                .single()
                .ok_or_else(|| raise_type_error!("Utc::timestamp_millis_opt::single failed"))?;
            // Evaluate arguments
            let year_val = evaluate_expr(env, &args[0])?;
            let year = if let Value::Number(y) = year_val {
                y as i32
            } else {
                return Err(raise_type_error!("Date.setFullYear() year must be a number"));
            };

            let month = if args.len() >= 2 {
                let month_val = evaluate_expr(env, &args[1])?;
                match month_val {
                    Value::Number(m) => m as u32,
                    _ => return Err(raise_type_error!("Date.setFullYear() month must be a number")),
                }
            } else {
                current_dt.month() - 1 // JavaScript months are 0-based
            };

            let day = if args.len() >= 3 {
                let day_val = evaluate_expr(env, &args[2])?;
                if let Value::Number(d) = day_val {
                    d as u32
                } else {
                    return Err(raise_type_error!("Date.setFullYear() day must be a number"));
                }
            } else {
                current_dt.day()
            };

            // Create new date with updated year, month, day
            // JavaScript months are 0-based, chrono months are 1-based
            if let Some(new_dt) = Utc
                .with_ymd_and_hms(year, month + 1, day, current_dt.hour(), current_dt.minute(), current_dt.second())
                .single()
            {
                let new_timestamp = new_dt.timestamp_millis() as f64;
                // Update the timestamp property
                set_time_stamp_value(obj, new_timestamp)?;
                Ok(Value::Number(new_timestamp))
            } else {
                // Invalid date
                set_time_stamp_value(obj, f64::NAN)?;
                Ok(Value::Number(f64::NAN))
            }
        }
        "setTime" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Date.setTime() takes exactly 1 argument"));
            }

            // Evaluate the time argument
            let time_val = evaluate_expr(env, &args[0])?;
            let Value::Number(time) = time_val else {
                return Err(raise_type_error!("Date.setTime() argument must be a number"));
            };

            // Set the timestamp
            set_time_stamp_value(obj, time)?;
            Ok(Value::Number(time))
        }
        "toDateString" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.toDateString() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                let local_dt = Local.from_utc_datetime(&dt.naive_utc());
                let formatted = local_dt.format("%a %b %d %Y").to_string();
                Ok(Value::String(utf8_to_utf16(&formatted)))
            } else {
                Ok(Value::String(utf8_to_utf16("Invalid Date")))
            }
        }
        "toTimeString" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.toTimeString() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                let local_dt = Local.from_utc_datetime(&dt.naive_utc());
                let formatted = local_dt.format("%H:%M:%S GMT%z").to_string();
                Ok(Value::String(utf8_to_utf16(&formatted)))
            } else {
                Ok(Value::String(utf8_to_utf16("Invalid Date")))
            }
        }
        "toISOString" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.toISOString() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                let formatted = dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
                Ok(Value::String(utf8_to_utf16(&formatted)))
            } else {
                Err(raise_type_error!("Invalid time value"))
            }
        }
        "toUTCString" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.toUTCString() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                let formatted = dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
                Ok(Value::String(utf8_to_utf16(&formatted)))
            } else {
                Ok(Value::String(utf8_to_utf16("Invalid Date")))
            }
        }
        "toJSON" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.toJSON() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if timestamp.is_nan() {
                Ok(Value::Undefined)
            } else if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                let formatted = dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
                Ok(Value::String(utf8_to_utf16(&formatted)))
            } else {
                Ok(Value::Undefined)
            }
        }
        "toLocaleString" => {
            // For simplicity, we'll use the same format as toString()
            // In a real implementation, this would use locale-specific formatting
            if !args.is_empty() {
                return Err(raise_type_error!("Date.toLocaleString() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                let local_dt = Local.from_utc_datetime(&dt.naive_utc());
                let formatted = local_dt.format("%a %b %d %Y %H:%M:%S GMT%z").to_string();
                Ok(Value::String(utf8_to_utf16(&formatted)))
            } else {
                Ok(Value::String(utf8_to_utf16("Invalid Date")))
            }
        }
        "toLocaleDateString" => {
            // For simplicity, we'll use the same format as toDateString()
            if !args.is_empty() {
                return Err(raise_type_error!("Date.toLocaleDateString() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                let local_dt = Local.from_utc_datetime(&dt.naive_utc());
                let formatted = local_dt.format("%a %b %d %Y").to_string();
                Ok(Value::String(utf8_to_utf16(&formatted)))
            } else {
                Ok(Value::String(utf8_to_utf16("Invalid Date")))
            }
        }
        "toLocaleTimeString" => {
            // For simplicity, we'll use the same format as toTimeString()
            if !args.is_empty() {
                return Err(raise_type_error!("Date.toLocaleTimeString() takes no arguments"));
            }
            let timestamp = get_time_stamp_value(obj)?;
            if let Some(dt) = Utc.timestamp_millis_opt(timestamp as i64).single() {
                let local_dt = Local.from_utc_datetime(&dt.naive_utc());
                let formatted = local_dt.format("%H:%M:%S GMT%z").to_string();
                Ok(Value::String(utf8_to_utf16(&formatted)))
            } else {
                Ok(Value::String(utf8_to_utf16("Invalid Date")))
            }
        }
        _ => Err(raise_eval_error!(format!("Date has no method '{method}'"))),
    }
}

/// Handle Date static method calls
pub(crate) fn handle_date_static_method(method: &str, args: &[Expr], _env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "now" => {
            if !args.is_empty() {
                return Err(raise_type_error!("Date.now() takes no arguments"));
            }
            use std::time::{SystemTime, UNIX_EPOCH};
            let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            Ok(Value::Number(duration.as_millis() as f64))
        }
        "parse" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Date.parse() takes exactly 1 argument"));
            }
            // Evaluate the argument
            let arg_val = evaluate_expr(_env, &args[0])?;
            if let Value::String(s) = arg_val {
                let date_str = String::from_utf16_lossy(&s);
                if let Some(timestamp) = parse_date_string(&date_str) {
                    Ok(Value::Number(timestamp))
                } else {
                    Ok(Value::Number(f64::NAN))
                }
            } else {
                Ok(Value::Number(f64::NAN))
            }
        }
        "UTC" => {
            // Date.UTC(year, month[, day[, hour[, minute[, second[, millisecond]]]]])
            if args.len() < 2 {
                return Err(raise_type_error!("Date.UTC() requires at least year and month"));
            }
            // Evaluate and coerce args to numbers
            let eval_num = |i: usize, default: f64| -> Result<f64, JSError> {
                if i < args.len() {
                    match evaluate_expr(_env, &args[i])? {
                        Value::Number(n) => Ok(n),
                        _ => Ok(f64::NAN),
                    }
                } else {
                    Ok(default)
                }
            };

            let year_n = eval_num(0, 0.0)?;
            let month_n = eval_num(1, 0.0)?;
            let day_n = eval_num(2, 1.0)?;
            let hour_n = eval_num(3, 0.0)?;
            let minute_n = eval_num(4, 0.0)?;
            let second_n = eval_num(5, 0.0)?;
            let ms_n = eval_num(6, 0.0)?;

            if year_n.is_nan()
                || month_n.is_nan()
                || day_n.is_nan()
                || hour_n.is_nan()
                || minute_n.is_nan()
                || second_n.is_nan()
                || ms_n.is_nan()
            {
                return Ok(Value::Number(f64::NAN));
            }

            // ToInteger semantics
            let mut year = year_n as i32;
            if (0..=99).contains(&year) {
                year += 1900;
            }
            // month is 0-based in JS
            let month = month_n as i64;
            let day = day_n as i64;
            let hour = hour_n as i64;
            let minute = minute_n as i64;
            let second = second_n as i64;
            let millisecond = ms_n as i64;

            // Normalize months (allow overflow/underflow)
            let total_months = year as i64 * 12 + month;
            let norm_year = (total_months.div_euclid(12)) as i32;
            let norm_month = (total_months.rem_euclid(12) + 1) as u32; // chrono months 1-12

            // Build NaiveDate and NaiveTime, allowing chrono to reject invalid dates
            if let Some(naive_date) = chrono::NaiveDate::from_ymd_opt(norm_year, norm_month, day as u32)
                && let Some(naive_dt) = naive_date.and_hms_milli_opt(hour as u32, minute as u32, second as u32, millisecond as u32)
            {
                let dt = chrono::DateTime::<Utc>::from_naive_utc_and_offset(naive_dt, Utc);
                return Ok(Value::Number(dt.timestamp_millis() as f64));
            }
            Ok(Value::Number(f64::NAN))
        }
        _ => Err(raise_eval_error!(format!("Date has no static method '{method}'"))),
    }
}
