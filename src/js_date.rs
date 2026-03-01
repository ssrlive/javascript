use crate::core::{EvalError, GcPtr, InternalSlot, JSObjectDataPtr, slot_get, slot_set};
use crate::core::{MutationContext, Value, env_set, new_js_object_data, object_get_key_value, object_set_key_value};
use crate::error::JSError;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};

// =========================================================================
// TimeClip (ECMAScript spec §21.4.1.15)
// =========================================================================

/// TimeClip — if the absolute value exceeds 8.64e15, or is NaN/Infinity, return NaN.
/// Also converts -0 to +0.
fn time_clip(time: f64) -> f64 {
    if time.is_nan() || time.is_infinite() || time.abs() > 8.64e15 {
        return f64::NAN;
    }
    let t = time.trunc();
    // Convert -0 to +0
    if t == 0.0 {
        return 0.0;
    }
    t
}

// =========================================================================
// MakeTime / MakeDay / MakeDate  (ECMAScript spec §21.4.1)
// =========================================================================

/// MakeTime(hour, min, sec, ms) — spec 21.4.1.12
fn make_time(hour: f64, min: f64, sec: f64, ms: f64) -> f64 {
    if !hour.is_finite() || !min.is_finite() || !sec.is_finite() || !ms.is_finite() {
        return f64::NAN;
    }
    let h = hour.trunc();
    let m = min.trunc();
    let s = sec.trunc();
    let milli = ms.trunc();
    h * 3_600_000.0 + m * 60_000.0 + s * 1_000.0 + milli
}

// =========================================================================
// Pure-math ES spec date functions (§21.4.1)
// =========================================================================

const MS_PER_DAY: f64 = 86_400_000.0;

fn spec_day(t: f64) -> f64 {
    (t / MS_PER_DAY).floor()
}

#[allow(dead_code)]
fn time_within_day(t: f64) -> f64 {
    t.rem_euclid(MS_PER_DAY)
}

fn is_leap_year(y: i64) -> bool {
    if y % 400 == 0 {
        true
    } else if y % 100 == 0 {
        false
    } else {
        y % 4 == 0
    }
}

#[allow(dead_code)]
fn days_in_year_i(y: i64) -> f64 {
    if is_leap_year(y) { 366.0 } else { 365.0 }
}

/// DayFromYear(y) — number of days from epoch (Jan 1 1970) to Jan 1 of year y
fn day_from_year(y: f64) -> f64 {
    365.0 * (y - 1970.0) + ((y - 1969.0) / 4.0).floor() - ((y - 1901.0) / 100.0).floor() + ((y - 1601.0) / 400.0).floor()
}

#[allow(dead_code)]
fn time_from_year(y: f64) -> f64 {
    MS_PER_DAY * day_from_year(y)
}

/// YearFromTime(t) — find largest y such that TimeFromYear(y) <= t
fn year_from_time(t: f64) -> f64 {
    let d = spec_day(t);
    let mut y = (1970.0 + d / 365.2425).floor();
    // Adjust forward/backward
    while day_from_year(y + 1.0) <= d {
        y += 1.0;
    }
    while day_from_year(y) > d {
        y -= 1.0;
    }
    y
}

fn day_within_year(t: f64) -> f64 {
    spec_day(t) - day_from_year(year_from_time(t))
}

fn in_leap_year(t: f64) -> bool {
    is_leap_year(year_from_time(t) as i64)
}

fn month_from_time(t: f64) -> f64 {
    let d = day_within_year(t);
    let leap = if in_leap_year(t) { 1.0 } else { 0.0 };
    if d < 31.0 {
        0.0
    } else if d < 59.0 + leap {
        1.0
    } else if d < 90.0 + leap {
        2.0
    } else if d < 120.0 + leap {
        3.0
    } else if d < 151.0 + leap {
        4.0
    } else if d < 181.0 + leap {
        5.0
    } else if d < 212.0 + leap {
        6.0
    } else if d < 243.0 + leap {
        7.0
    } else if d < 273.0 + leap {
        8.0
    } else if d < 304.0 + leap {
        9.0
    } else if d < 334.0 + leap {
        10.0
    } else {
        11.0
    }
}

fn date_from_time(t: f64) -> f64 {
    let d = day_within_year(t);
    let leap = if in_leap_year(t) { 1.0 } else { 0.0 };
    match month_from_time(t) as u32 {
        0 => d + 1.0,
        1 => d - 30.0,
        2 => d - 58.0 - leap,
        3 => d - 89.0 - leap,
        4 => d - 119.0 - leap,
        5 => d - 150.0 - leap,
        6 => d - 180.0 - leap,
        7 => d - 211.0 - leap,
        8 => d - 242.0 - leap,
        9 => d - 272.0 - leap,
        10 => d - 303.0 - leap,
        11 => d - 333.0 - leap,
        _ => unreachable!(),
    }
}

fn hour_from_time(t: f64) -> f64 {
    ((t / 3_600_000.0).floor()).rem_euclid(24.0)
}
fn min_from_time(t: f64) -> f64 {
    ((t / 60_000.0).floor()).rem_euclid(60.0)
}
fn sec_from_time(t: f64) -> f64 {
    ((t / 1_000.0).floor()).rem_euclid(60.0)
}
fn ms_from_time(t: f64) -> f64 {
    t.rem_euclid(1000.0)
}
fn weekday_from_time(t: f64) -> f64 {
    (spec_day(t) + 4.0).rem_euclid(7.0) // Jan 1, 1970 was Thursday (4)
}

/// Cumulative days from Jan 1 to start of month m (0-based) in year y
fn cumulative_month_days(year: f64, month: i64) -> f64 {
    let leap = if is_leap_year(year as i64) { 1.0 } else { 0.0 };
    match month {
        0 => 0.0,
        1 => 31.0,
        2 => 59.0 + leap,
        3 => 90.0 + leap,
        4 => 120.0 + leap,
        5 => 151.0 + leap,
        6 => 181.0 + leap,
        7 => 212.0 + leap,
        8 => 243.0 + leap,
        9 => 273.0 + leap,
        10 => 304.0 + leap,
        11 => 334.0 + leap,
        _ => 0.0,
    }
}

// Formatting helpers
fn weekday_name(wd: f64) -> &'static str {
    match wd as u32 {
        0 => "Sun",
        1 => "Mon",
        2 => "Tue",
        3 => "Wed",
        4 => "Thu",
        5 => "Fri",
        6 => "Sat",
        _ => "???",
    }
}
fn month_name(m: f64) -> &'static str {
    match m as u32 {
        0 => "Jan",
        1 => "Feb",
        2 => "Mar",
        3 => "Apr",
        4 => "May",
        5 => "Jun",
        6 => "Jul",
        7 => "Aug",
        8 => "Sep",
        9 => "Oct",
        10 => "Nov",
        11 => "Dec",
        _ => "???",
    }
}

/// Format year for toString/toDateString: "-0001" for negative, "0042" for small positive, "1970" etc.
fn format_year_display(year: f64) -> String {
    let yi = year as i64;
    if yi < 0 { format!("-{:04}", -yi) } else { format!("{:04}", yi) }
}

/// Format year for toISOString: extended year ±YYYYYY for years outside 0..9999
fn format_year_iso(year: f64) -> String {
    let yi = year as i64;
    if !(0..=9999).contains(&yi) {
        format!("{:+07}", yi)
    } else {
        format!("{:04}", yi)
    }
}

/// MakeDay(year, month, date) — spec 21.4.1.13 (pure math, no chrono)
fn make_day(year: f64, month: f64, date: f64) -> f64 {
    if !year.is_finite() || !month.is_finite() || !date.is_finite() {
        return f64::NAN;
    }
    let y = year.trunc();
    let m = month.trunc();
    let dt = date.trunc();

    // Adjust year and month so month is in 0..11
    let ym = y + (m / 12.0).floor();
    let mn = m.rem_euclid(12.0) as i64;

    // Reject absurd years (prevent precision loss)
    if ym.abs() > 1e8 {
        return f64::NAN;
    }

    // Day of Jan 1 of year ym + days to start of month mn + (date - 1)
    day_from_year(ym) + cumulative_month_days(ym, mn) + dt - 1.0
}

/// MakeDate(day, time) — spec 21.4.1.14
fn make_date(day: f64, time: f64) -> f64 {
    if !day.is_finite() || !time.is_finite() {
        return f64::NAN;
    }
    day * 86_400_000.0 + time
}

// =========================================================================
// Decompose timestamp to UTC components
// =========================================================================

struct DateComponents {
    year: f64,
    month: f64, // 0-based
    date: f64,  // 1-based
    hour: f64,
    min: f64,
    sec: f64,
    ms: f64,
    weekday: f64,
}

fn decompose_utc(t: f64) -> Option<DateComponents> {
    if t.is_nan() || t.is_infinite() {
        return None;
    }
    Some(DateComponents {
        year: year_from_time(t),
        month: month_from_time(t),
        date: date_from_time(t),
        hour: hour_from_time(t),
        min: min_from_time(t),
        sec: sec_from_time(t),
        ms: ms_from_time(t),
        weekday: weekday_from_time(t),
    })
}

fn decompose_local(t: f64) -> Option<DateComponents> {
    if t.is_nan() || t.is_infinite() {
        return None;
    }
    let offset = local_tz_offset_ms(t);
    let local_t = t + offset;
    decompose_utc(local_t)
}

/// Get timezone offset in milliseconds for a given UTC timestamp.
/// Uses chrono when possible, falls back to current standard offset.
fn local_tz_offset_ms(utc_t: f64) -> f64 {
    let ms = utc_t as i64;
    if let Some(dt) = Utc.timestamp_millis_opt(ms).single() {
        let local = Local.from_utc_datetime(&dt.naive_utc());
        return local.offset().local_minus_utc() as f64 * 1000.0;
    }
    // Fallback for extreme dates: use current timezone standard offset
    let now = Local::now();
    now.offset().local_minus_utc() as f64 * 1000.0
}

/// Convert local-time components back to UTC timestamp
fn local_to_utc(year: f64, month: f64, date: f64, hour: f64, min: f64, sec: f64, ms: f64) -> f64 {
    // Build as UTC first, then adjust with timezone offset
    let day = make_day(year, month, date);
    let time = make_time(hour, min, sec, ms);
    let utc_guess = make_date(day, time);
    if utc_guess.is_nan() {
        return f64::NAN;
    }
    // Get the local timezone offset at approx. this time
    let guess_i64 = utc_guess as i64;
    if let Some(dt) = Utc.timestamp_millis_opt(guess_i64).single() {
        let local_dt = Local.from_utc_datetime(&dt.naive_utc());
        let offset_ms = local_dt.offset().local_minus_utc() as f64 * 1000.0;
        utc_guess - offset_ms
    } else {
        // Fallback: use current timezone standard offset for extreme dates
        let now = Local::now();
        let offset_ms = now.offset().local_minus_utc() as f64 * 1000.0;
        utc_guess - offset_ms
    }
}

/// Get timezone offset in milliseconds at timestamp t
fn timezone_offset_ms(t: f64) -> f64 {
    if t.is_nan() {
        return f64::NAN;
    }
    local_tz_offset_ms(t)
}

// =========================================================================
// ToNumber helper for Date method args
// =========================================================================
fn to_number_val<'gc>(mc: &MutationContext<'gc>, v: &Value<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<f64, EvalError<'gc>> {
    let prim = crate::core::to_primitive(mc, v, "number", env)?;
    crate::core::to_number(&prim)
}

// =========================================================================
// Initialize Date constructor
// =========================================================================

pub(crate) fn initialize_date<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let date_ctor = new_js_object_data(mc);
    slot_set(mc, &date_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &date_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("Date")));

    // Set [[Prototype]] = Function.prototype
    if let Err(e) = crate::core::set_internal_prototype_from_constructor(mc, &date_ctor, env, "Function") {
        log::warn!("Failed to set Date constructor's internal prototype from Function: {e:?}");
    }

    // Date.length = 7 (non-writable, non-enumerable, configurable)
    let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(7.0), false, false, true)?;
    crate::js_object::define_property_internal(mc, &date_ctor, "length", &len_desc)?;

    // Date.name = "Date"
    let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("Date")), false, false, true)?;
    crate::js_object::define_property_internal(mc, &date_ctor, "name", &name_desc)?;

    // Get Object.prototype
    let object_proto = if let Some(obj_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else {
        None
    };

    let date_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        date_proto.borrow_mut(mc).prototype = Some(proto);
    }

    // Date.prototype — non-writable, non-enumerable, non-configurable
    object_set_key_value(mc, &date_ctor, "prototype", &Value::Object(date_proto))?;
    date_ctor.borrow_mut(mc).set_non_writable("prototype");
    date_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    date_ctor.borrow_mut(mc).set_non_configurable("prototype");

    // Date.prototype.constructor — writable, non-enumerable, configurable
    object_set_key_value(mc, &date_proto, "constructor", &Value::Object(date_ctor))?;
    date_proto.borrow_mut(mc).set_non_enumerable("constructor");

    // Instance methods with their spec'd lengths
    let methods: &[(&str, f64)] = &[
        ("toString", 0.0),
        ("getTime", 0.0),
        ("valueOf", 0.0),
        ("getFullYear", 0.0),
        ("getUTCFullYear", 0.0),
        ("getMonth", 0.0),
        ("getUTCMonth", 0.0),
        ("getDate", 0.0),
        ("getUTCDate", 0.0),
        ("getDay", 0.0),
        ("getUTCDay", 0.0),
        ("getHours", 0.0),
        ("getUTCHours", 0.0),
        ("getMinutes", 0.0),
        ("getUTCMinutes", 0.0),
        ("getSeconds", 0.0),
        ("getUTCSeconds", 0.0),
        ("getMilliseconds", 0.0),
        ("getUTCMilliseconds", 0.0),
        ("getTimezoneOffset", 0.0),
        ("setFullYear", 3.0),
        ("setUTCFullYear", 3.0),
        ("setMonth", 2.0),
        ("setUTCMonth", 2.0),
        ("setDate", 1.0),
        ("setUTCDate", 1.0),
        ("setHours", 4.0),
        ("setUTCHours", 4.0),
        ("setMinutes", 3.0),
        ("setUTCMinutes", 3.0),
        ("setSeconds", 2.0),
        ("setUTCSeconds", 2.0),
        ("setMilliseconds", 1.0),
        ("setUTCMilliseconds", 1.0),
        ("setTime", 1.0),
        ("toDateString", 0.0),
        ("toTimeString", 0.0),
        ("toISOString", 0.0),
        ("toUTCString", 0.0),
        ("toJSON", 1.0),
        ("toLocaleString", 0.0),
        ("toLocaleDateString", 0.0),
        ("toLocaleTimeString", 0.0),
        // Annex B legacy methods
        ("getYear", 0.0),
        ("setYear", 1.0),
    ];

    for (method, arity) in methods {
        let fn_obj = new_js_object_data(mc);
        fn_obj.borrow_mut(mc).set_closure(Some(crate::core::new_gc_cell_ptr(
            mc,
            Value::Function(format!("Date.prototype.{method}")),
        )));
        // Set Function.prototype as [[Prototype]]
        if let Some(func_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_val.borrow()
            && let Some(fp_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(fp) = &*fp_val.borrow()
        {
            fn_obj.borrow_mut(mc).prototype = Some(*fp);
        }
        // name: non-writable, non-enumerable, configurable
        let nm_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16(method)), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fn_obj, "name", &nm_desc)?;
        // length: non-writable, non-enumerable, configurable
        let ln_desc = crate::core::create_descriptor_object(mc, &Value::Number(*arity), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fn_obj, "length", &ln_desc)?;
        // Store on prototype: writable, non-enumerable, configurable
        object_set_key_value(mc, &date_proto, method.to_string(), &Value::Object(fn_obj))?;
        date_proto.borrow_mut(mc).set_non_enumerable(*method);
    }

    // Annex B: Date.prototype.toGMTString === Date.prototype.toUTCString (same object)
    if let Some(utc_fn) = object_get_key_value(&date_proto, "toUTCString") {
        object_set_key_value(mc, &date_proto, "toGMTString", &utc_fn.borrow())?;
        date_proto.borrow_mut(mc).set_non_enumerable("toGMTString");
    }

    // Symbol.toPrimitive — Date.prototype[@@toPrimitive](hint)
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_val.borrow()
        && let Some(tp_sym_val) = object_get_key_value(sym_obj, "toPrimitive")
        && let Value::Symbol(tp_sym) = &*tp_sym_val.borrow()
    {
        let fn_obj = new_js_object_data(mc);
        fn_obj.borrow_mut(mc).set_closure(Some(crate::core::new_gc_cell_ptr(
            mc,
            Value::Function("Date.prototype.@@toPrimitive".to_string()),
        )));
        if let Some(func_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_val.borrow()
            && let Some(fp_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(fp) = &*fp_val.borrow()
        {
            fn_obj.borrow_mut(mc).prototype = Some(*fp);
        }
        let nm_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("[Symbol.toPrimitive]")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fn_obj, "name", &nm_desc)?;
        let ln_desc = crate::core::create_descriptor_object(mc, &Value::Number(1.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fn_obj, "length", &ln_desc)?;
        // non-writable, non-enumerable, configurable
        object_set_key_value(mc, &date_proto, crate::core::PropertyKey::Symbol(*tp_sym), &Value::Object(fn_obj))?;
        date_proto
            .borrow_mut(mc)
            .set_non_writable(crate::core::PropertyKey::Symbol(*tp_sym));
        date_proto
            .borrow_mut(mc)
            .set_non_enumerable(crate::core::PropertyKey::Symbol(*tp_sym));
    }

    // Static methods
    let static_methods: &[(&str, f64)] = &[("now", 0.0), ("parse", 1.0), ("UTC", 7.0)];
    for (method, arity) in static_methods {
        let fn_obj = new_js_object_data(mc);
        fn_obj
            .borrow_mut(mc)
            .set_closure(Some(crate::core::new_gc_cell_ptr(mc, Value::Function(format!("Date.{method}")))));
        if let Some(func_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_val.borrow()
            && let Some(fp_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(fp) = &*fp_val.borrow()
        {
            fn_obj.borrow_mut(mc).prototype = Some(*fp);
        }
        let nm_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16(method)), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fn_obj, "name", &nm_desc)?;
        let ln_desc = crate::core::create_descriptor_object(mc, &Value::Number(*arity), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fn_obj, "length", &ln_desc)?;
        object_set_key_value(mc, &date_ctor, method.to_string(), &Value::Object(fn_obj))?;
        date_ctor.borrow_mut(mc).set_non_enumerable(*method);
    }

    env_set(mc, env, "Date", &Value::Object(date_ctor))?;
    Ok(())
}

// =========================================================================
// Check if an object is a Date object
// =========================================================================

#[allow(dead_code)]
pub fn is_date_object(obj: &JSObjectDataPtr) -> bool {
    internal_get_time_stamp_value(obj).is_some()
}

pub(crate) fn internal_get_time_stamp_value<'gc>(date_obj: &JSObjectDataPtr<'gc>) -> Option<GcPtr<'gc, Value<'gc>>> {
    slot_get(date_obj, &InternalSlot::Timestamp)
}

/// thisTimeValue(value) — spec step for all Date prototype methods.
/// Returns the [[DateValue]] or throws TypeError.
fn this_time_value<'gc>(this: &Value<'gc>) -> Result<f64, EvalError<'gc>> {
    if let Value::Object(obj) = this
        && let Some(ts_ptr) = internal_get_time_stamp_value(obj)
        && let Value::Number(n) = *ts_ptr.borrow()
    {
        return Ok(n);
    }
    Err(raise_type_error!("this is not a Date object").into())
}

fn set_time_stamp_value<'gc>(mc: &MutationContext<'gc>, date_obj: &JSObjectDataPtr<'gc>, timestamp: f64) -> Result<(), JSError> {
    slot_set(mc, date_obj, InternalSlot::Timestamp, &Value::Number(timestamp));
    Ok(())
}

// =========================================================================
// Date string parsing
// =========================================================================

pub(crate) fn parse_date_string(date_str: &str) -> Option<f64> {
    let s = date_str.trim();

    // Reject extended year -000000 (negative zero is invalid per spec)
    if s.starts_with("-000000") && (s.len() == 7 || !s.as_bytes()[7].is_ascii_digit()) {
        return None;
    }

    // Try custom ISO parser first (handles extended years, year-only, etc.)
    if let Some(v) = parse_iso_date(s) {
        return Some(v);
    }

    // Try RFC 3339
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp_millis() as f64);
    }

    // Try simplified ISO 8601 (YYYY-MM-DDTHH:mm:ss.sssZ)
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ") {
        let utc_dt = DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc);
        return Some(utc_dt.timestamp_millis() as f64);
    }

    // Try ISO date without time: YYYY-MM-DD (treated as UTC per spec)
    if let Ok(nd) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = nd.and_hms_opt(0, 0, 0)?;
        let utc_dt = DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc);
        return Some(utc_dt.timestamp_millis() as f64);
    }

    // Try ISO date-time without Z (treated as local per spec)
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        && let Some(local_dt) = Local.from_local_datetime(&dt).single()
    {
        return Some(local_dt.with_timezone(&Utc).timestamp_millis() as f64);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        && let Some(local_dt) = Local.from_local_datetime(&dt).single()
    {
        return Some(local_dt.with_timezone(&Utc).timestamp_millis() as f64);
    }

    // Try ISO with timezone offset (e.g., 2024-01-01T00:00:00+05:30)
    if let Ok(dt) = DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%:z") {
        return Some(dt.timestamp_millis() as f64);
    }
    if let Ok(dt) = DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f%:z") {
        return Some(dt.timestamp_millis() as f64);
    }

    // Try RFC 2822 format
    if let Ok(dt) = DateTime::parse_from_rfc2822(s) {
        return Some(dt.timestamp_millis() as f64);
    }

    // Strip parenthetical timezone name from toString output:
    // "Thu Jan 01 1970 00:00:00 GMT+0000 (UTC)" → "Thu Jan 01 1970 00:00:00 GMT+0000"
    let stripped = if let Some(idx) = s.rfind('(') { s[..idx].trim() } else { s };

    // Common JS Date.parse formats
    let formats = [
        "%b %d %Y %H:%M:%S GMT%z",
        "%a %b %d %Y %H:%M:%S GMT%z",
        "%b %d, %Y",
        "%B %d, %Y",
        "%d %B %Y",
        "%d %b %Y",
    ];
    for fmt in &formats {
        if let Ok(dt) = DateTime::parse_from_str(stripped, fmt) {
            return Some(dt.timestamp_millis() as f64);
        }
    }

    // Try month/day/year format
    parse_month_day_year(s)
}

/// Parse ISO 8601 date strings including extended year format (±YYYYYY)
/// and year-only format ("1970").
fn parse_iso_date(s: &str) -> Option<f64> {
    // Year-only: "1970", "2024", etc. (4 digits, treated as UTC)
    if s.len() == 4 && s.bytes().all(|b| b.is_ascii_digit()) {
        let year: f64 = s.parse().ok()?;
        let day = day_from_year(year);
        return Some(time_clip(day * MS_PER_DAY));
    }

    // Extended year format: +YYYYYY-... or -YYYYYY-...
    // Also handle negative 4-digit years: -YYYY-MM-DD...
    let (year_str, rest) = if s.starts_with('+') || s.starts_with('-') {
        // Find the first '-' after the sign that separates year from month
        let after_sign = &s[1..];
        // Extended year: exactly 6 digits after sign
        if after_sign.len() >= 6 && after_sign[..6].bytes().all(|b| b.is_ascii_digit()) {
            let sign = if s.starts_with('-') { -1i64 } else { 1i64 };
            let year_abs: i64 = after_sign[..6].parse().ok()?;
            let year = sign * year_abs;

            // Reject -000000 (negative zero year is invalid per spec)
            if sign == -1 && year_abs == 0 {
                return None;
            }

            let remainder = &after_sign[6..];
            (year, remainder)
        } else {
            return None;
        }
    } else {
        return None; // Not an extended year format; let other parsers handle it
    };

    // Parse optional -MM-DDTHH:mm:ss.sssZ parts
    let (month, day_of_month, hour, min, sec, ms, is_utc, tz_offset_min) = parse_iso_tail(rest)?;

    // Build timestamp using pure math
    let day = day_from_year(year_str as f64) + cumulative_month_days(year_str as f64, (month - 1) as i64) + (day_of_month as f64) - 1.0;
    let time = (hour as f64) * 3_600_000.0 + (min as f64) * 60_000.0 + (sec as f64) * 1_000.0 + (ms as f64);
    let mut result = day * MS_PER_DAY + time;

    // Apply timezone offset
    if is_utc || rest.is_empty() || rest == "Z" {
        // Already UTC
    } else if tz_offset_min != 0 {
        result -= (tz_offset_min as f64) * 60_000.0;
    } else if !is_utc {
        // No timezone specified and not "Z" — for date-only extended years, treat as UTC
        // (per spec, date-only forms are UTC)
    }

    Some(time_clip(result))
}

/// Parse the tail of an ISO date string: -MM-DDTHH:mm:ss.sssZ or subsets
/// Returns (month 1-12, day 1-31, hour, min, sec, ms, is_utc, tz_offset_minutes)
#[allow(clippy::type_complexity)]
fn parse_iso_tail(s: &str) -> Option<(u32, u32, u32, u32, u32, u32, bool, i32)> {
    if s.is_empty() {
        // Year-only extended year
        return Some((1, 1, 0, 0, 0, 0, true, 0));
    }

    let bytes = s.as_bytes();
    let mut pos = 0;

    // Expect '-'
    if pos >= bytes.len() || bytes[pos] != b'-' {
        return None;
    }
    pos += 1;

    // Month: 2 digits
    if pos + 2 > bytes.len() {
        return None;
    }
    let month: u32 = s[pos..pos + 2].parse().ok()?;
    pos += 2;

    // Optional -DD
    let mut day_of_month = 1u32;
    if pos < bytes.len() && bytes[pos] == b'-' {
        pos += 1;
        if pos + 2 > bytes.len() {
            return None;
        }
        day_of_month = s[pos..pos + 2].parse().ok()?;
        pos += 2;
    }

    // Optional THH:mm:ss.sss
    let mut hour = 0u32;
    let mut min = 0u32;
    let mut sec = 0u32;
    let mut ms = 0u32;
    let mut is_utc = true;
    let mut tz_offset = 0i32;

    if pos < bytes.len() && bytes[pos] == b'T' {
        pos += 1;
        // HH
        if pos + 2 > bytes.len() {
            return None;
        }
        hour = s[pos..pos + 2].parse().ok()?;
        pos += 2;
        // :mm
        if pos < bytes.len() && bytes[pos] == b':' {
            pos += 1;
            if pos + 2 > bytes.len() {
                return None;
            }
            min = s[pos..pos + 2].parse().ok()?;
            pos += 2;
        }
        // :ss
        if pos < bytes.len() && bytes[pos] == b':' {
            pos += 1;
            if pos + 2 > bytes.len() {
                return None;
            }
            sec = s[pos..pos + 2].parse().ok()?;
            pos += 2;
        }
        // .sss
        if pos < bytes.len() && bytes[pos] == b'.' {
            pos += 1;
            let start = pos;
            while pos < bytes.len() && bytes[pos].is_ascii_digit() {
                pos += 1;
            }
            let frac_str = &s[start..pos];
            // Pad or truncate to 3 digits
            let ms_val: f64 = if frac_str.len() <= 3 {
                let padded = format!("{:0<3}", frac_str);
                padded.parse().unwrap_or(0.0)
            } else {
                frac_str[..3].parse().unwrap_or(0.0)
            };
            ms = ms_val as u32;
        }
        // Timezone: Z or ±HH:mm
        if pos < bytes.len() {
            if bytes[pos] == b'Z' {
                is_utc = true;
                // pos += 1;
            } else if bytes[pos] == b'+' || bytes[pos] == b'-' {
                let tz_sign = if bytes[pos] == b'+' { 1i32 } else { -1i32 };
                pos += 1;
                if pos + 2 > bytes.len() {
                    return None;
                }
                let tz_h: i32 = s[pos..pos + 2].parse().ok()?;
                pos += 2;
                let mut tz_m = 0i32;
                if pos < bytes.len() && bytes[pos] == b':' {
                    pos += 1;
                    if pos + 2 > bytes.len() {
                        return None;
                    }
                    tz_m = s[pos..pos + 2].parse().ok()?;
                    // pos += 2;
                }
                tz_offset = tz_sign * (tz_h * 60 + tz_m);
                is_utc = false;
            }
        }
    }

    Some((month, day_of_month, hour, min, sec, ms, is_utc, tz_offset))
}

fn parse_month_day_year(date_str: &str) -> Option<f64> {
    let parts: Vec<&str> = date_str.split('/').collect();
    if parts.len() == 3 {
        let month: u32 = parts[0].parse().ok()?;
        let day: u32 = parts[1].parse().ok()?;
        let year: i32 = parts[2].parse().ok()?;
        if (1..=12).contains(&month) && (1..=31).contains(&day) {
            let nd = NaiveDate::from_ymd_opt(year, month, day)?;
            let dt = nd.and_hms_opt(0, 0, 0)?;
            let utc_dt = DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc);
            return Some(utc_dt.timestamp_millis() as f64);
        }
    }
    None
}

// =========================================================================
// construct_date_from_components — used by multi-arg Date constructor
// Returns local-time based timestamp (like V8/SpiderMonkey).
// =========================================================================

pub(crate) fn construct_date_from_components(components: &[f64]) -> f64 {
    if components.is_empty() || components.len() > 7 {
        return f64::NAN;
    }
    // Any NaN component → NaN
    for c in components {
        if c.is_nan() || c.is_infinite() {
            return f64::NAN;
        }
    }

    let year_val = components[0].trunc();
    let month_val = if components.len() > 1 { components[1].trunc() } else { 0.0 };
    let day_val = if components.len() > 2 { components[2].trunc() } else { 1.0 };
    let hour_val = if components.len() > 3 { components[3].trunc() } else { 0.0 };
    let minute_val = if components.len() > 4 { components[4].trunc() } else { 0.0 };
    let second_val = if components.len() > 5 { components[5].trunc() } else { 0.0 };
    let ms_val = if components.len() > 6 { components[6].trunc() } else { 0.0 };

    // Handle 2-digit years (0-99) -> 1900-1999
    let yr = year_val as i64;
    let year = if (0..=99).contains(&yr) { 1900.0 + year_val } else { year_val };

    // Build date in local time then convert to UTC
    let t = local_to_utc(year, month_val, day_val, hour_val, minute_val, second_val, ms_val);
    time_clip(t)
}

// =========================================================================
// Handle Date constructor calls
// =========================================================================

pub(crate) fn handle_date_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    new_target: Option<&Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = if args.is_empty() {
        // new Date() - current time
        let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        duration.as_millis() as f64
    } else if args.len() == 1 {
        // new Date(value) — ToNumber or string parse
        let arg_val = &args[0];
        match arg_val {
            Value::String(s) => {
                let date_str = utf16_to_utf8(s);
                parse_date_string(&date_str).map(time_clip).unwrap_or(f64::NAN)
            }
            Value::Object(obj) => {
                // If the argument is a Date object, get the timestamp directly
                if is_date_object(obj) {
                    if let Some(ts_ptr) = internal_get_time_stamp_value(obj) {
                        if let Value::Number(n) = &*ts_ptr.borrow() { *n } else { f64::NAN }
                    } else {
                        f64::NAN
                    }
                } else {
                    // ToPrimitive with no preferred type (uses "default" hint)
                    let prim = crate::core::to_primitive(mc, arg_val, "default", env)?;
                    match &prim {
                        Value::String(s) => {
                            let date_str = utf16_to_utf8(s);
                            parse_date_string(&date_str).map(time_clip).unwrap_or(f64::NAN)
                        }
                        _ => time_clip(crate::core::to_number(&prim)?),
                    }
                }
            }
            _ => time_clip(crate::core::to_number(arg_val)?),
        }
    } else {
        // new Date(year, month, day, hours, minutes, seconds, milliseconds)
        // Each argument is coerced with ToNumber (which may throw)
        let mut components = Vec::new();
        for arg in args {
            components.push(to_number_val(mc, arg, env)?);
        }
        construct_date_from_components(&components)
    };

    // Create a Date object with clipped timestamp
    let date_obj = new_js_object_data(mc);
    set_time_stamp_value(mc, &date_obj, timestamp)?;

    // OrdinaryCreateFromConstructor: use new_target's prototype if provided
    let mut proto_set = false;
    if let Some(nt) = new_target
        && let Value::Object(nt_obj) = nt
        && let Some(proto) = crate::js_class::get_prototype_from_constructor(mc, nt_obj, env, "Date")?
    {
        date_obj.borrow_mut(mc).prototype = Some(proto);
        proto_set = true;
    }
    if !proto_set {
        crate::core::set_internal_prototype_from_constructor(mc, &date_obj, env, "Date")?;
    }

    Ok(Value::Object(date_obj))
}

// =========================================================================
// Handle Date instance methods
// =========================================================================

pub(crate) fn handle_date_method<'gc>(
    mc: &MutationContext<'gc>,
    obj: &Value<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // @@toPrimitive
    if method == "@@toPrimitive" {
        // Step 1: If Type(O) is not Object, throw TypeError
        if !matches!(obj, Value::Object(_)) {
            return Err(raise_type_error!("Date.prototype[Symbol.toPrimitive] requires that 'this' be an Object").into());
        }
        let hint_str = if let Some(hint_val) = args.first()
            && let Value::String(s) = hint_val
        {
            utf16_to_utf8(s)
        } else {
            String::new()
        };

        let try_first = match hint_str.as_str() {
            "string" | "default" => "string",
            "number" => "number",
            _ => return Err(raise_type_error!("Invalid hint").into()),
        };

        // OrdinaryToPrimitive(O, tryFirst)
        let is_primitive = |v: &Value<'gc>| {
            matches!(
                v,
                Value::Number(_)
                    | Value::BigInt(_)
                    | Value::String(_)
                    | Value::Boolean(_)
                    | Value::Null
                    | Value::Undefined
                    | Value::Symbol(_)
            )
        };
        let obj_ptr = match obj {
            Value::Object(o) => o,
            _ => unreachable!(),
        };
        let methods: &[&str] = if try_first == "string" {
            &["toString", "valueOf"]
        } else {
            &["valueOf", "toString"]
        };
        for method_name in methods {
            let func_val = crate::core::get_property_with_accessors(mc, env, obj_ptr, *method_name)?;
            if matches!(func_val, Value::Undefined | Value::Null) {
                continue;
            }
            let result = crate::js_promise::call_function_with_this(mc, &func_val, Some(obj), &[], env)?;
            if is_primitive(&result) {
                return Ok(result);
            }
        }
        return Err(raise_type_error!("Cannot convert object to primitive value").into());
    }

    // toJSON is generic — it uses ToObject/ToPrimitive, NOT thisTimeValue
    if method == "toJSON" {
        // 21.4.4.37 Date.prototype.toJSON ( key )
        // 1. Let O be ? ToObject(this value).
        let o = match obj {
            Value::Object(_) => obj.clone(),
            Value::Null | Value::Undefined => {
                return Err(raise_type_error!("Date.prototype.toJSON called on null or undefined").into());
            }
            // Box primitives to their object wrappers
            Value::Number(n) => {
                let boxed = crate::core::new_js_object_data(mc);
                slot_set(mc, &boxed, InternalSlot::PrimitiveValue, &Value::Number(*n));
                let _ = crate::core::set_internal_prototype_from_constructor(mc, &boxed, env, "Number");
                Value::Object(boxed)
            }
            Value::String(s) => {
                let boxed = crate::core::new_js_object_data(mc);
                slot_set(mc, &boxed, InternalSlot::PrimitiveValue, &Value::String(s.clone()));
                let _ = crate::core::set_internal_prototype_from_constructor(mc, &boxed, env, "String");
                Value::Object(boxed)
            }
            Value::Boolean(b) => {
                let boxed = crate::core::new_js_object_data(mc);
                slot_set(mc, &boxed, InternalSlot::PrimitiveValue, &Value::Boolean(*b));
                let _ = crate::core::set_internal_prototype_from_constructor(mc, &boxed, env, "Boolean");
                Value::Object(boxed)
            }
            Value::Symbol(_) => {
                let boxed = crate::core::new_js_object_data(mc);
                slot_set(mc, &boxed, InternalSlot::PrimitiveValue, obj);
                let _ = crate::core::set_internal_prototype_from_constructor(mc, &boxed, env, "Symbol");
                Value::Object(boxed)
            }
            _ => obj.clone(),
        };
        // 2. Let tv be ? ToPrimitive(O, number).
        let tv = crate::core::to_primitive(mc, &o, "number", env)?;
        // 3. If Type(tv) is Number and tv is not finite, return null.
        if let Value::Number(n) = &tv
            && !n.is_finite()
        {
            return Ok(Value::Null);
        }
        // 4. Return ? Invoke(O, "toISOString").
        if let Value::Object(obj_ptr) = &o {
            let to_iso = crate::core::get_property_with_accessors(mc, env, obj_ptr, "toISOString")?;
            if matches!(to_iso, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("toISOString is not a function").into());
            }
            return crate::js_promise::call_function_with_this(mc, &to_iso, Some(&o), &[], env);
        }
        return Err(raise_type_error!("Date.prototype.toJSON called on non-object").into());
    }

    // All other Date prototype methods: thisTimeValue(this) first
    let t = this_time_value(obj)?;

    // Getter for an object pointer (needed for setters)
    let obj_ptr = match obj {
        Value::Object(o) => o,
        _ => return Err(raise_type_error!("this is not a Date object").into()),
    };

    match method {
        // ===================== Getters =====================
        "getTime" => Ok(Value::Number(t)),
        "valueOf" => Ok(Value::Number(t)),

        "getFullYear" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            Ok(Value::Number(c.year))
        }
        "getUTCFullYear" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_utc(t).unwrap();
            Ok(Value::Number(c.year))
        }
        "getMonth" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            Ok(Value::Number(c.month))
        }
        "getUTCMonth" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_utc(t).unwrap();
            Ok(Value::Number(c.month))
        }
        "getDate" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            Ok(Value::Number(c.date))
        }
        "getUTCDate" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_utc(t).unwrap();
            Ok(Value::Number(c.date))
        }
        "getDay" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            Ok(Value::Number(c.weekday))
        }
        "getUTCDay" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_utc(t).unwrap();
            Ok(Value::Number(c.weekday))
        }
        "getHours" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            Ok(Value::Number(c.hour))
        }
        "getUTCHours" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_utc(t).unwrap();
            Ok(Value::Number(c.hour))
        }
        "getMinutes" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            Ok(Value::Number(c.min))
        }
        "getUTCMinutes" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_utc(t).unwrap();
            Ok(Value::Number(c.min))
        }
        "getSeconds" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            Ok(Value::Number(c.sec))
        }
        "getUTCSeconds" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_utc(t).unwrap();
            Ok(Value::Number(c.sec))
        }
        "getMilliseconds" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            Ok(Value::Number(c.ms))
        }
        "getUTCMilliseconds" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_utc(t).unwrap();
            Ok(Value::Number(c.ms))
        }
        "getTimezoneOffset" => {
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            // Spec: (t − LocalTime(t)) / msPerMinute
            // Use subtraction form (0.0 - x) instead of negation (-x)
            // to avoid producing −0 when offset is 0 (UTC).
            let offset_ms = timezone_offset_ms(t);
            Ok(Value::Number((0.0 - offset_ms) / 60_000.0))
        }

        // ===================== toString variants =====================
        "toString" => {
            if t.is_nan() {
                return Ok(Value::String(utf8_to_utf16("Invalid Date")));
            }
            let c = decompose_local(t).unwrap();
            let offset_ms = timezone_offset_ms(t);
            let total_min = (offset_ms / 60_000.0) as i32;
            let sign = if total_min >= 0 { '+' } else { '-' };
            let abs_min = total_min.abs();
            let tz_h = abs_min / 60;
            let tz_m = abs_min % 60;
            // Try to get timezone abbreviation from chrono
            let tz_name = if let Some(dt) = Utc.timestamp_millis_opt(t as i64).single() {
                let local = Local.from_utc_datetime(&dt.naive_utc());
                local.format("%Z").to_string()
            } else {
                format!("GMT{}{:02}{:02}", sign, tz_h, tz_m)
            };
            let formatted = format!(
                "{} {} {:02} {} {:02}:{:02}:{:02} GMT{}{:02}{:02} ({})",
                weekday_name(c.weekday),
                month_name(c.month),
                c.date as u32,
                format_year_display(c.year),
                c.hour as u32,
                c.min as u32,
                c.sec as u32,
                sign,
                tz_h,
                tz_m,
                tz_name
            );
            Ok(Value::String(utf8_to_utf16(&formatted)))
        }
        "toDateString" => {
            if t.is_nan() {
                return Ok(Value::String(utf8_to_utf16("Invalid Date")));
            }
            let c = decompose_local(t).unwrap();
            let formatted = format!(
                "{} {} {:02} {}",
                weekday_name(c.weekday),
                month_name(c.month),
                c.date as u32,
                format_year_display(c.year)
            );
            Ok(Value::String(utf8_to_utf16(&formatted)))
        }
        "toTimeString" => {
            if t.is_nan() {
                return Ok(Value::String(utf8_to_utf16("Invalid Date")));
            }
            let c = decompose_local(t).unwrap();
            let offset_ms = timezone_offset_ms(t);
            let total_min = (offset_ms / 60_000.0) as i32;
            let sign = if total_min >= 0 { '+' } else { '-' };
            let abs_min = total_min.abs();
            let formatted = format!(
                "{:02}:{:02}:{:02} GMT{}{:02}{:02}",
                c.hour as u32,
                c.min as u32,
                c.sec as u32,
                sign,
                abs_min / 60,
                abs_min % 60
            );
            Ok(Value::String(utf8_to_utf16(&formatted)))
        }
        "toISOString" => {
            if t.is_nan() {
                return Err(raise_range_error!("Invalid time value").into());
            }
            let c = decompose_utc(t).unwrap();
            let formatted = format!(
                "{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                format_year_iso(c.year),
                c.month as u32 + 1,
                c.date as u32,
                c.hour as u32,
                c.min as u32,
                c.sec as u32,
                c.ms as u32
            );
            Ok(Value::String(utf8_to_utf16(&formatted)))
        }
        "toUTCString" => {
            if t.is_nan() {
                return Ok(Value::String(utf8_to_utf16("Invalid Date")));
            }
            let c = decompose_utc(t).unwrap();
            let formatted = format!(
                "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
                weekday_name(c.weekday),
                c.date as u32,
                month_name(c.month),
                format_year_display(c.year),
                c.hour as u32,
                c.min as u32,
                c.sec as u32,
            );
            Ok(Value::String(utf8_to_utf16(&formatted)))
        }
        "toLocaleString" | "toLocaleDateString" | "toLocaleTimeString" => {
            if t.is_nan() {
                return Ok(Value::String(utf8_to_utf16("Invalid Date")));
            }
            let c = decompose_local(t).unwrap();
            let offset_ms = timezone_offset_ms(t);
            let total_min = (offset_ms / 60_000.0) as i32;
            let sign = if total_min >= 0 { '+' } else { '-' };
            let abs_min = total_min.abs();
            let formatted = match method {
                "toLocaleTimeString" => format!(
                    "{:02}:{:02}:{:02} GMT{}{:02}{:02}",
                    c.hour as u32,
                    c.min as u32,
                    c.sec as u32,
                    sign,
                    abs_min / 60,
                    abs_min % 60
                ),
                "toLocaleDateString" => format!(
                    "{} {} {:02} {}",
                    weekday_name(c.weekday),
                    month_name(c.month),
                    c.date as u32,
                    format_year_display(c.year)
                ),
                _ => format!(
                    "{} {} {:02} {} {:02}:{:02}:{:02} GMT{}{:02}{:02}",
                    weekday_name(c.weekday),
                    month_name(c.month),
                    c.date as u32,
                    format_year_display(c.year),
                    c.hour as u32,
                    c.min as u32,
                    c.sec as u32,
                    sign,
                    abs_min / 60,
                    abs_min % 60
                ),
            };
            Ok(Value::String(utf8_to_utf16(&formatted)))
        }

        // ===================== setTime =====================
        "setTime" => {
            let v = if args.is_empty() {
                f64::NAN
            } else {
                to_number_val(mc, &args[0], env)?
            };
            let clipped = time_clip(v);
            set_time_stamp_value(mc, obj_ptr, clipped)?;
            Ok(Value::Number(clipped))
        }

        // ===================== setMilliseconds / setUTCMilliseconds =====================
        "setMilliseconds" => {
            let ms = if args.is_empty() {
                f64::NAN
            } else {
                to_number_val(mc, &args[0], env)?
            };
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            let new_t = time_clip(local_to_utc(c.year, c.month, c.date, c.hour, c.min, c.sec, ms));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }
        "setUTCMilliseconds" => {
            let ms = if args.is_empty() {
                f64::NAN
            } else {
                to_number_val(mc, &args[0], env)?
            };
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_utc(t).unwrap();
            let day = make_day(c.year, c.month, c.date);
            let time = make_time(c.hour, c.min, c.sec, ms);
            let new_t = time_clip(make_date(day, time));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }

        // ===================== setSeconds / setUTCSeconds =====================
        "setSeconds" => {
            let s = to_number_val(mc, args.first().unwrap_or(&Value::Undefined), env)?;
            let ms = if args.len() >= 2 {
                to_number_val(mc, &args[1], env)?
            } else if t.is_nan() {
                f64::NAN
            } else {
                decompose_local(t).unwrap().ms
            };
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            let new_t = time_clip(local_to_utc(c.year, c.month, c.date, c.hour, c.min, s, ms));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }
        "setUTCSeconds" => {
            let s = to_number_val(mc, args.first().unwrap_or(&Value::Undefined), env)?;
            let ms = if args.len() >= 2 {
                to_number_val(mc, &args[1], env)?
            } else if t.is_nan() {
                f64::NAN
            } else {
                decompose_utc(t).unwrap().ms
            };
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_utc(t).unwrap();
            let day = make_day(c.year, c.month, c.date);
            let time = make_time(c.hour, c.min, s, ms);
            let new_t = time_clip(make_date(day, time));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }

        // ===================== setMinutes / setUTCMinutes =====================
        "setMinutes" => {
            let m = to_number_val(mc, args.first().unwrap_or(&Value::Undefined), env)?;
            let s = if args.len() >= 2 {
                to_number_val(mc, &args[1], env)?
            } else if t.is_nan() {
                f64::NAN
            } else {
                decompose_local(t).unwrap().sec
            };
            let ms = if args.len() >= 3 {
                to_number_val(mc, &args[2], env)?
            } else if t.is_nan() {
                f64::NAN
            } else {
                decompose_local(t).unwrap().ms
            };
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            let new_t = time_clip(local_to_utc(c.year, c.month, c.date, c.hour, m, s, ms));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }
        "setUTCMinutes" => {
            let m = to_number_val(mc, args.first().unwrap_or(&Value::Undefined), env)?;
            let s = if args.len() >= 2 {
                to_number_val(mc, &args[1], env)?
            } else if t.is_nan() {
                f64::NAN
            } else {
                decompose_utc(t).unwrap().sec
            };
            let ms = if args.len() >= 3 {
                to_number_val(mc, &args[2], env)?
            } else if t.is_nan() {
                f64::NAN
            } else {
                decompose_utc(t).unwrap().ms
            };
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_utc(t).unwrap();
            let day = make_day(c.year, c.month, c.date);
            let time = make_time(c.hour, m, s, ms);
            let new_t = time_clip(make_date(day, time));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }

        // ===================== setHours / setUTCHours =====================
        "setHours" => {
            let h = to_number_val(mc, args.first().unwrap_or(&Value::Undefined), env)?;
            let m = if args.len() >= 2 {
                to_number_val(mc, &args[1], env)?
            } else if t.is_nan() {
                f64::NAN
            } else {
                decompose_local(t).unwrap().min
            };
            let s = if args.len() >= 3 {
                to_number_val(mc, &args[2], env)?
            } else if t.is_nan() {
                f64::NAN
            } else {
                decompose_local(t).unwrap().sec
            };
            let ms = if args.len() >= 4 {
                to_number_val(mc, &args[3], env)?
            } else if t.is_nan() {
                f64::NAN
            } else {
                decompose_local(t).unwrap().ms
            };
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            let new_t = time_clip(local_to_utc(c.year, c.month, c.date, h, m, s, ms));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }
        "setUTCHours" => {
            let h = to_number_val(mc, args.first().unwrap_or(&Value::Undefined), env)?;
            let m = if args.len() >= 2 {
                to_number_val(mc, &args[1], env)?
            } else if t.is_nan() {
                f64::NAN
            } else {
                decompose_utc(t).unwrap().min
            };
            let s = if args.len() >= 3 {
                to_number_val(mc, &args[2], env)?
            } else if t.is_nan() {
                f64::NAN
            } else {
                decompose_utc(t).unwrap().sec
            };
            let ms = if args.len() >= 4 {
                to_number_val(mc, &args[3], env)?
            } else if t.is_nan() {
                f64::NAN
            } else {
                decompose_utc(t).unwrap().ms
            };
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_utc(t).unwrap();
            let day = make_day(c.year, c.month, c.date);
            let time = make_time(h, m, s, ms);
            let new_t = time_clip(make_date(day, time));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }

        // ===================== setDate / setUTCDate =====================
        "setDate" => {
            let dt = to_number_val(mc, args.first().unwrap_or(&Value::Undefined), env)?;
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            let new_t = time_clip(local_to_utc(c.year, c.month, dt, c.hour, c.min, c.sec, c.ms));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }
        "setUTCDate" => {
            let dt = to_number_val(mc, args.first().unwrap_or(&Value::Undefined), env)?;
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_utc(t).unwrap();
            let day = make_day(c.year, c.month, dt);
            let time = make_time(c.hour, c.min, c.sec, c.ms);
            let new_t = time_clip(make_date(day, time));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }

        // ===================== setMonth / setUTCMonth =====================
        "setMonth" => {
            let m = to_number_val(mc, args.first().unwrap_or(&Value::Undefined), env)?;
            let dt = if args.len() >= 2 {
                to_number_val(mc, &args[1], env)?
            } else if t.is_nan() {
                f64::NAN
            } else {
                decompose_local(t).unwrap().date
            };
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            let new_t = time_clip(local_to_utc(c.year, m, dt, c.hour, c.min, c.sec, c.ms));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }
        "setUTCMonth" => {
            let m = to_number_val(mc, args.first().unwrap_or(&Value::Undefined), env)?;
            let dt = if args.len() >= 2 {
                to_number_val(mc, &args[1], env)?
            } else if t.is_nan() {
                f64::NAN
            } else {
                decompose_utc(t).unwrap().date
            };
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_utc(t).unwrap();
            let day = make_day(c.year, m, dt);
            let time = make_time(c.hour, c.min, c.sec, c.ms);
            let new_t = time_clip(make_date(day, time));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }

        // ===================== setFullYear / setUTCFullYear =====================
        "setFullYear" => {
            let y = to_number_val(mc, args.first().unwrap_or(&Value::Undefined), env)?;
            // Per spec §21.4.4.27: if t is NaN, set t to +0 (do NOT apply LocalTime)
            let c = if t.is_nan() {
                decompose_utc(0.0).unwrap()
            } else {
                decompose_local(t).unwrap()
            };
            let m = if args.len() >= 2 {
                to_number_val(mc, &args[1], env)?
            } else {
                c.month
            };
            let dt = if args.len() >= 3 {
                to_number_val(mc, &args[2], env)?
            } else {
                c.date
            };
            let new_t = time_clip(local_to_utc(y, m, dt, c.hour, c.min, c.sec, c.ms));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }
        "setUTCFullYear" => {
            let y = to_number_val(mc, args.first().unwrap_or(&Value::Undefined), env)?;
            // Per spec: if t is NaN, let t be +0
            let base_t = if t.is_nan() { 0.0 } else { t };
            let c = decompose_utc(base_t).unwrap();
            let m = if args.len() >= 2 {
                to_number_val(mc, &args[1], env)?
            } else {
                c.month
            };
            let dt = if args.len() >= 3 {
                to_number_val(mc, &args[2], env)?
            } else {
                c.date
            };
            let day = make_day(y, m, dt);
            let time = make_time(c.hour, c.min, c.sec, c.ms);
            let new_t = time_clip(make_date(day, time));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }

        // ===================== Annex B (legacy) =====================
        "getYear" => {
            // B.2.4.1: getYear() → getFullYear() - 1900
            if t.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            let c = decompose_local(t).unwrap();
            Ok(Value::Number(c.year - 1900.0))
        }
        "setYear" => {
            // B.2.4.2: setYear(year)
            let y = to_number_val(mc, args.first().unwrap_or(&Value::Undefined), env)?;
            if y.is_nan() {
                let new_t = f64::NAN;
                set_time_stamp_value(mc, obj_ptr, new_t)?;
                return Ok(Value::Number(new_t));
            }
            let yi = y.trunc();
            let yr = if (0.0..=99.0).contains(&yi) { yi + 1900.0 } else { yi };
            // Per spec: if t is NaN, let t be +0𝔽 (so we start from epoch)
            let c = if t.is_nan() {
                decompose_utc(0.0).unwrap()
            } else {
                decompose_local(t).unwrap()
            };
            let new_t = time_clip(local_to_utc(yr, c.month, c.date, c.hour, c.min, c.sec, c.ms));
            set_time_stamp_value(mc, obj_ptr, new_t)?;
            Ok(Value::Number(new_t))
        }
        "toGMTString" => {
            // B.2.4.3: toGMTString is the same Function object as toUTCString
            if t.is_nan() {
                return Ok(Value::String(utf8_to_utf16("Invalid Date")));
            }
            let c = decompose_utc(t).unwrap();
            let formatted = format!(
                "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
                weekday_name(c.weekday),
                c.date as u32,
                month_name(c.month),
                format_year_display(c.year),
                c.hour as u32,
                c.min as u32,
                c.sec as u32,
            );
            Ok(Value::String(utf8_to_utf16(&formatted)))
        }

        _ => Err(raise_type_error!(format!("Date.prototype.{method} is not a function")).into()),
    }
}

// =========================================================================
// Handle Date static methods
// =========================================================================

pub(crate) fn handle_date_static_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "now" => {
            use std::time::{SystemTime, UNIX_EPOCH};
            let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            Ok(Value::Number(duration.as_millis() as f64))
        }
        "parse" => {
            let arg_val = args.first().unwrap_or(&Value::Undefined);
            // Convert argument to string first
            let s = match arg_val {
                Value::String(s) => utf16_to_utf8(s),
                Value::Undefined => "undefined".to_string(),
                other => {
                    let prim = crate::core::to_primitive(mc, other, "string", env)?;
                    crate::core::value_to_string(&prim)
                }
            };
            Ok(Value::Number(parse_date_string(&s).map(time_clip).unwrap_or(f64::NAN)))
        }
        "UTC" => {
            // Date.UTC(year [, month [, date [, hours [, minutes [, seconds [, ms]]]]]])
            // Per spec: year is required but month defaults to 0 if not provided.
            // With 0 args → NaN (from ToNumber(undefined) = NaN)
            // With 1 arg → year only, month = 0
            // Step 1: year
            let y = if args.is_empty() {
                f64::NAN
            } else {
                to_number_val(mc, &args[0], env)?
            };
            // Step 2: month (default 0 if 1 arg)
            let m = if args.len() >= 2 { to_number_val(mc, &args[1], env)? } else { 0.0 };
            // Remaining optional args
            let dt = if args.len() >= 3 { to_number_val(mc, &args[2], env)? } else { 1.0 };
            let h = if args.len() >= 4 { to_number_val(mc, &args[3], env)? } else { 0.0 };
            let min = if args.len() >= 5 { to_number_val(mc, &args[4], env)? } else { 0.0 };
            let s = if args.len() >= 6 { to_number_val(mc, &args[5], env)? } else { 0.0 };
            let ms = if args.len() >= 7 { to_number_val(mc, &args[6], env)? } else { 0.0 };

            if y.is_nan() || m.is_nan() || dt.is_nan() || h.is_nan() || min.is_nan() || s.is_nan() || ms.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            if !y.is_finite()
                || !m.is_finite()
                || !dt.is_finite()
                || !h.is_finite()
                || !min.is_finite()
                || !s.is_finite()
                || !ms.is_finite()
            {
                return Ok(Value::Number(f64::NAN));
            }

            // Step 8: 0-99 year adjustment
            let yr = y.trunc();
            let adjusted_yr = if (0.0..=99.0).contains(&yr) { 1900.0 + yr } else { yr };

            let day = make_day(adjusted_yr, m, dt);
            let time = make_time(h, min, s, ms);
            let result = time_clip(make_date(day, time));
            Ok(Value::Number(result))
        }
        _ => Err(raise_type_error!(format!("Date.{method} is not a function")).into()),
    }
}
