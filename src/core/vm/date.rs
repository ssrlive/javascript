use super::*;

impl<'gc> VM<'gc> {
    /// Dispatch all `"date.*"` host function calls.
    pub(super) fn date_handle_host_fn(
        &mut self,
        ctx: &GcContext<'gc>,
        name: &str,
        receiver: Option<&Value<'gc>>,
        args: &[Value<'gc>],
    ) -> Value<'gc> {
        match name {
            "date.UTC" => {
                if args.is_empty() {
                    return Value::Number(f64::NAN);
                }
                let mut nums: Vec<f64> = Vec::new();
                for a in args {
                    match self.extract_number_with_coercion(ctx, a) {
                        Some(n) => nums.push(n),
                        None => return Value::Undefined,
                    }
                }
                let yr = nums[0];
                let month = nums.get(1).copied().unwrap_or(0.0);
                let day = nums.get(2).copied().unwrap_or(1.0);
                let hour = nums.get(3).copied().unwrap_or(0.0);
                let minute = nums.get(4).copied().unwrap_or(0.0);
                let second = nums.get(5).copied().unwrap_or(0.0);
                let millis = nums.get(6).copied().unwrap_or(0.0);
                if yr.is_nan()
                    || yr.is_infinite()
                    || month.is_nan()
                    || month.is_infinite()
                    || day.is_nan()
                    || day.is_infinite()
                    || hour.is_nan()
                    || hour.is_infinite()
                    || minute.is_nan()
                    || minute.is_infinite()
                    || second.is_nan()
                    || second.is_infinite()
                    || millis.is_nan()
                    || millis.is_infinite()
                {
                    Value::Number(f64::NAN)
                } else {
                    Value::Number(Self::make_date_from_components(yr, month, day, hour, minute, second, millis, false))
                }
            }
            "date.toPrimitive" => {
                let this = receiver.cloned().unwrap_or(Value::Undefined);
                if !matches!(&this, Value::VmObject(_)) {
                    self.throw_type_error(ctx, "Date.prototype[Symbol.toPrimitive] requires that 'this' be an Object");
                    return Value::Undefined;
                }
                let hint_str = match args.first() {
                    Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                    Some(v) => value_to_string(v),
                    None => String::new(),
                };
                match hint_str.as_str() {
                    "string" | "default" => {
                        let to_str = self.read_named_property(ctx, &this, "toString");
                        if self.is_value_callable(&to_str) {
                            match self.vm_call_function_value(ctx, &to_str, &this, &[]) {
                                Ok(v) if !matches!(v, Value::VmObject(_) | Value::VmArray(_)) => return v,
                                Ok(_) => {}
                                Err(e) => {
                                    self.set_pending_throw_from_error(&e);
                                    return Value::Undefined;
                                }
                            }
                        }
                        let val_of = self.read_named_property(ctx, &this, "valueOf");
                        if self.is_value_callable(&val_of) {
                            match self.vm_call_function_value(ctx, &val_of, &this, &[]) {
                                Ok(v) if !matches!(v, Value::VmObject(_) | Value::VmArray(_)) => return v,
                                Ok(_) => {}
                                Err(e) => {
                                    self.set_pending_throw_from_error(&e);
                                    return Value::Undefined;
                                }
                            }
                        }
                        self.throw_type_error(ctx, "Cannot convert object to primitive value");
                        Value::Undefined
                    }
                    "number" => {
                        let val_of = self.read_named_property(ctx, &this, "valueOf");
                        if self.is_value_callable(&val_of) {
                            match self.vm_call_function_value(ctx, &val_of, &this, &[]) {
                                Ok(v) if !matches!(v, Value::VmObject(_) | Value::VmArray(_)) => return v,
                                Ok(_) => {}
                                Err(e) => {
                                    self.set_pending_throw_from_error(&e);
                                    return Value::Undefined;
                                }
                            }
                        }
                        let to_str = self.read_named_property(ctx, &this, "toString");
                        if self.is_value_callable(&to_str) {
                            match self.vm_call_function_value(ctx, &to_str, &this, &[]) {
                                Ok(v) if !matches!(v, Value::VmObject(_) | Value::VmArray(_)) => return v,
                                Ok(_) => {}
                                Err(e) => {
                                    self.set_pending_throw_from_error(&e);
                                    return Value::Undefined;
                                }
                            }
                        }
                        self.throw_type_error(ctx, "Cannot convert object to primitive value");
                        Value::Undefined
                    }
                    _ => {
                        self.throw_type_error(ctx, "Invalid hint");
                        Value::Undefined
                    }
                }
            }
            "date.toJSON" => {
                let this = receiver.cloned().unwrap_or(Value::Undefined);
                if matches!(&this, Value::Undefined | Value::Null) {
                    self.throw_type_error(ctx, "Date.prototype.toJSON called on null or undefined");
                    return Value::Undefined;
                }
                let o = if matches!(&this, Value::VmObject(_) | Value::VmArray(_)) {
                    this.clone()
                } else {
                    self.call_builtin(ctx, BUILTIN_CTOR_OBJECT, std::slice::from_ref(&this))
                };
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let tv = self.try_to_primitive(ctx, &o, "number");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                if let Value::Number(n) = &tv
                    && (n.is_nan() || n.is_infinite())
                {
                    return Value::Null;
                }
                let to_iso = self.read_named_property(ctx, &o, "toISOString");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                if !self.is_value_callable(&to_iso) {
                    self.throw_type_error(ctx, "toISOString is not a function");
                    return Value::Undefined;
                }
                match self.vm_call_function_value(ctx, &to_iso, &o, &[]) {
                    Ok(v) => v,
                    Err(e) => {
                        self.set_pending_throw_from_error(&e);
                        Value::Undefined
                    }
                }
            }
            "date.toTimeString" => {
                let this = receiver.cloned().unwrap_or(Value::Undefined);
                if let Value::VmObject(obj) = &this
                    && let Some(Value::Number(ms)) = obj.borrow().get("__date_ms__").cloned()
                {
                    if ms.is_nan() || ms.is_infinite() {
                        return Value::from("Invalid Date");
                    }
                    use chrono::{Local, TimeZone};
                    if let Some(dt) = Local.timestamp_millis_opt(ms as i64).single() {
                        let s = dt.format("%H:%M:%S GMT%z").to_string();
                        return Value::from(&s);
                    }
                }
                self.throw_type_error(ctx, "this is not a Date object");
                Value::Undefined
            }
            n if n.starts_with("date.set") => self.date_setter_host_fn(ctx, n, receiver, args),
            _ => {
                log::warn!("Unhandled date host fn: {}", name);
                Value::Undefined
            }
        }
    }

    /// Initialize Date constructor and Date.prototype.
    pub(super) fn date_init_prototype(&mut self, ctx: &GcContext<'gc>) {
        let mut date_map = IndexMap::new();
        Self::init_native_ctor_header(&mut date_map, BUILTIN_CTOR_DATE, "Date", 7.0);
        date_map.insert("now".to_string(), Value::VmNativeFunction(BUILTIN_DATE_NOW));
        date_map.insert("parse".to_string(), Value::VmNativeFunction(BUILTIN_DATE_PARSE));
        date_map.insert(
            "UTC".to_string(),
            Self::make_host_fn_with_name_len(ctx, "date.UTC", "UTC", 7.0, false),
        );
        let mut date_proto = IndexMap::new();
        if let Some(Value::VmObject(obj_ctor)) = self.globals.get("Object")
            && let Some(obj_proto) = obj_ctor.borrow().get("prototype").cloned()
        {
            date_proto.insert("__proto__".to_string(), obj_proto);
        }
        for (key, value) in [
            ("getTime", Value::VmNativeFunction(BUILTIN_DATE_GETTIME)),
            ("valueOf", Value::VmNativeFunction(BUILTIN_DATE_VALUEOF)),
            ("toString", Value::VmNativeFunction(BUILTIN_DATE_TOSTRING)),
            (
                "toTimeString",
                Self::make_host_fn_with_name_len(ctx, "date.toTimeString", "toTimeString", 0.0, false),
            ),
            ("toUTCString", Value::VmNativeFunction(BUILTIN_DATE_TOUTCSTRING)),
            ("toDateString", Value::VmNativeFunction(BUILTIN_DATE_TODATESTRING)),
            ("setTime", Value::VmNativeFunction(BUILTIN_DATE_SETTIME)),
            ("toJSON", Self::make_host_fn_with_name_len(ctx, "date.toJSON", "toJSON", 1.0, false)),
            ("toLocaleDateString", Value::VmNativeFunction(BUILTIN_DATE_TOLOCALEDATESTRING)),
            ("toLocaleTimeString", Value::VmNativeFunction(BUILTIN_DATE_TOLOCALETIMESTRING)),
            ("toLocaleString", Value::VmNativeFunction(BUILTIN_DATE_TOLOCALESTRING)),
            ("toISOString", Value::VmNativeFunction(BUILTIN_DATE_TOISOSTRING)),
            ("getFullYear", Value::VmNativeFunction(BUILTIN_DATE_GETFULLYEAR)),
            ("getMonth", Value::VmNativeFunction(BUILTIN_DATE_GETMONTH)),
            ("getDate", Value::VmNativeFunction(BUILTIN_DATE_GETDATE)),
            ("getDay", Value::VmNativeFunction(BUILTIN_DATE_GETDAY)),
            ("getUTCDay", Value::VmNativeFunction(BUILTIN_DATE_GETUTCDAY)),
            ("getHours", Value::VmNativeFunction(BUILTIN_DATE_GETHOURS)),
            ("getMinutes", Value::VmNativeFunction(BUILTIN_DATE_GETMINUTES)),
            ("getSeconds", Value::VmNativeFunction(BUILTIN_DATE_GETSECONDS)),
            ("getMilliseconds", Value::VmNativeFunction(BUILTIN_DATE_GETMILLISECONDS)),
            ("setFullYear", Value::VmNativeFunction(BUILTIN_DATE_SETFULLYEAR)),
            (
                "setUTCFullYear",
                Self::make_host_fn_with_name_len(ctx, "date.setUTCFullYear", "setUTCFullYear", 3.0, false),
            ),
            ("setMonth", Value::VmNativeFunction(BUILTIN_DATE_SETMONTH)),
            (
                "setUTCMonth",
                Self::make_host_fn_with_name_len(ctx, "date.setUTCMonth", "setUTCMonth", 2.0, false),
            ),
            ("setDate", Value::VmNativeFunction(BUILTIN_DATE_SETDATE)),
            (
                "setUTCDate",
                Self::make_host_fn_with_name_len(ctx, "date.setUTCDate", "setUTCDate", 1.0, false),
            ),
            ("setHours", Value::VmNativeFunction(BUILTIN_DATE_SETHOURS)),
            (
                "setUTCHours",
                Self::make_host_fn_with_name_len(ctx, "date.setUTCHours", "setUTCHours", 4.0, false),
            ),
            ("setMinutes", Value::VmNativeFunction(BUILTIN_DATE_SETMINUTES)),
            (
                "setUTCMinutes",
                Self::make_host_fn_with_name_len(ctx, "date.setUTCMinutes", "setUTCMinutes", 3.0, false),
            ),
            (
                "setSeconds",
                Self::make_host_fn_with_name_len(ctx, "date.setSeconds", "setSeconds", 2.0, false),
            ),
            (
                "setUTCSeconds",
                Self::make_host_fn_with_name_len(ctx, "date.setUTCSeconds", "setUTCSeconds", 2.0, false),
            ),
            (
                "setMilliseconds",
                Self::make_host_fn_with_name_len(ctx, "date.setMilliseconds", "setMilliseconds", 1.0, false),
            ),
            (
                "setUTCMilliseconds",
                Self::make_host_fn_with_name_len(ctx, "date.setUTCMilliseconds", "setUTCMilliseconds", 1.0, false),
            ),
            ("getTimezoneOffset", Value::VmNativeFunction(BUILTIN_DATE_GETTIMEZONEOFFSET)),
            ("getUTCFullYear", Value::VmNativeFunction(BUILTIN_DATE_GETUTCFULLYEAR)),
            ("getUTCMonth", Value::VmNativeFunction(BUILTIN_DATE_GETUTCMONTH)),
            ("getUTCDate", Value::VmNativeFunction(BUILTIN_DATE_GETUTCDATE)),
            ("getUTCHours", Value::VmNativeFunction(BUILTIN_DATE_GETUTCHOURS)),
            ("getUTCMinutes", Value::VmNativeFunction(BUILTIN_DATE_GETUTCMINUTES)),
            ("getUTCSeconds", Value::VmNativeFunction(BUILTIN_DATE_GETUTCSECONDS)),
            ("getUTCMilliseconds", Value::VmNativeFunction(BUILTIN_DATE_GETUTCMILLISECONDS)),
        ] {
            date_proto.insert(key.to_string(), value);
            date_proto.insert(format!("__nonenumerable_{}__", key), Value::Boolean(true));
        }
        // Date.prototype[Symbol.toPrimitive]
        date_proto.insert(
            "@@sym:3".to_string(),
            Self::make_host_fn_with_name_len(ctx, "date.toPrimitive", "[Symbol.toPrimitive]", 1.0, false),
        );
        date_proto.insert("__nonenumerable_@@sym:3__".to_string(), Value::Boolean(true));
        date_proto.insert("__readonly_@@sym:3__".to_string(), Value::Boolean(true));
        let date_proto_obj = new_gc_cell_ptr(ctx, date_proto);
        date_map.insert("__nonenumerable_now__".to_string(), Value::Boolean(true));
        date_map.insert("__nonenumerable_parse__".to_string(), Value::Boolean(true));
        date_map.insert("__nonenumerable_UTC__".to_string(), Value::Boolean(true));
        let date_ctor_val = Self::finalize_ctor_with_prototype(ctx, date_map, date_proto_obj);
        self.globals.insert("Date".to_string(), date_ctor_val);
    }

    /// Handle Date-related IDs in `call_builtin`.
    pub(super) fn date_call_builtin(&mut self, _ctx: &GcContext<'gc>, id: FunctionID, args: &[Value<'gc>]) -> Value<'gc> {
        match id {
            BUILTIN_DATE_NOW => {
                use std::time::{SystemTime, UNIX_EPOCH};
                let ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as f64)
                    .unwrap_or(0.0);
                Value::Number(ms)
            }
            BUILTIN_DATE_PARSE => {
                let s_str = args.first().map(|v| value_to_string(v)).unwrap_or_default();
                let ms = self.date_parse_string(&s_str);
                Value::Number(ms)
            }
            BUILTIN_CTOR_DATE => {
                // Date() called as a function returns a date-time string.
                use chrono::{Local, TimeZone, Utc};
                let now = Utc::now().timestamp_millis();
                if let Some(dt) = Local.timestamp_millis_opt(now).single() {
                    let s = dt.format("%a %b %d %Y %H:%M:%S GMT%z").to_string();
                    Value::from(&s)
                } else {
                    Value::from("Invalid Date")
                }
            }
            _ => Value::Undefined,
        }
    }

    /// Handle Date-related IDs in `call_method_builtin`.
    /// Returns `Some(value)` if handled, `None` if not a Date method.
    pub(super) fn date_call_method_builtin(
        &mut self,
        ctx: &GcContext<'gc>,
        id: FunctionID,
        receiver: &Value<'gc>,
        args: &[Value<'gc>],
    ) -> Option<Value<'gc>> {
        match id {
            BUILTIN_DATE_PARSE => {
                let s_str = args.first().map(|v| value_to_string(v)).unwrap_or_default();
                let ms = self.date_parse_string(&s_str);
                return Some(Value::Number(ms));
            }
            BUILTIN_CTOR_DATE => {
                if let Value::VmObject(obj) = receiver {
                    use std::time::{SystemTime, UNIX_EPOCH};
                    let ms = if args.is_empty() {
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_millis() as f64)
                            .unwrap_or(0.0)
                    } else if args.len() == 1 {
                        let prim = match &args[0] {
                            Value::VmObject(_) | Value::VmArray(_) => self.try_to_primitive(ctx, &args[0], "default"),
                            other => other.clone(),
                        };
                        if self.pending_throw.is_some() {
                            f64::NAN
                        } else {
                            match &prim {
                                Value::Number(n) => Self::time_clip(*n),
                                Value::String(s) => {
                                    let s_str = crate::unicode::utf16_to_utf8(s);
                                    self.date_parse_string(&s_str)
                                }
                                Value::BigInt(_) => {
                                    self.throw_type_error(ctx, "Cannot convert a BigInt value to a number");
                                    f64::NAN
                                }
                                Value::Symbol(_) => {
                                    self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                                    f64::NAN
                                }
                                _ => to_number(&prim),
                            }
                        }
                    } else {
                        let yr = to_number(&args[0]);
                        let month = to_number(args.get(1).unwrap_or(&Value::Number(0.0)));
                        let day = to_number(args.get(2).unwrap_or(&Value::Number(1.0)));
                        let hour = to_number(args.get(3).unwrap_or(&Value::Number(0.0)));
                        let min_val = to_number(args.get(4).unwrap_or(&Value::Number(0.0)));
                        let sec = to_number(args.get(5).unwrap_or(&Value::Number(0.0)));
                        let ms_part = to_number(args.get(6).unwrap_or(&Value::Number(0.0)));
                        if yr.is_nan()
                            || yr.is_infinite()
                            || month.is_nan()
                            || month.is_infinite()
                            || day.is_nan()
                            || day.is_infinite()
                            || hour.is_nan()
                            || hour.is_infinite()
                            || min_val.is_nan()
                            || min_val.is_infinite()
                            || sec.is_nan()
                            || sec.is_infinite()
                            || ms_part.is_nan()
                            || ms_part.is_infinite()
                        {
                            f64::NAN
                        } else {
                            Self::make_date_from_components(yr, month, day, hour, min_val, sec, ms_part, true)
                        }
                    };

                    let mut borrow = obj.borrow_mut(ctx);
                    borrow.insert("__type__".to_string(), Value::from("Date"));
                    borrow.insert("__date_ms__".to_string(), Value::Number(ms));
                    return Some(receiver.clone());
                }
            }
            _ => {}
        }

        // Date instance methods — receiver must be a Date object with __date_ms__
        if let Value::VmObject(obj) = receiver {
            let date_ms = {
                let borrow = obj.borrow();
                match borrow.get("__date_ms__") {
                    Some(Value::Number(ms)) => Some(*ms),
                    _ => None,
                }
            };
            if let Some(ms) = date_ms {
                use chrono::{Datelike, Local, TimeZone, Timelike, Utc};
                let to_local = || {
                    if ms.is_nan() || ms.is_infinite() {
                        return None;
                    }
                    Local.timestamp_millis_opt(ms as i64).single()
                };
                let to_utc = || {
                    if ms.is_nan() || ms.is_infinite() {
                        return None;
                    }
                    Utc.timestamp_millis_opt(ms as i64).single()
                };
                match id {
                    BUILTIN_DATE_GETTIME | BUILTIN_DATE_VALUEOF => return Some(Value::Number(ms)),
                    BUILTIN_DATE_TOSTRING => {
                        if let Some(dt) = to_local() {
                            let y = dt.year();
                            let year_str = Self::format_year_for_display(y);
                            let s = format!("{} {}", dt.format("%a %b %d"), year_str) + &dt.format(" %H:%M:%S GMT%z").to_string();
                            return Some(Value::from(&s));
                        }
                        return Some(Value::from("Invalid Date"));
                    }
                    BUILTIN_DATE_TOLOCALEDATESTRING => {
                        if let Some(dt) = to_local() {
                            let s = format!("{}/{}/{}", dt.month(), dt.day(), dt.year());
                            return Some(Value::from(&s));
                        }
                        return Some(Value::from("Invalid Date"));
                    }
                    BUILTIN_DATE_TOLOCALETIMESTRING => {
                        if let Some(dt) = to_local() {
                            let s = dt.format("%H:%M:%S").to_string();
                            return Some(Value::from(&s));
                        }
                        return Some(Value::from("Invalid Date"));
                    }
                    BUILTIN_DATE_TOLOCALESTRING => {
                        if let Some(dt) = to_local() {
                            let s = format!("{}/{}/{} {}", dt.month(), dt.day(), dt.year(), dt.format("%H:%M:%S"));
                            return Some(Value::from(&s));
                        }
                        return Some(Value::from("Invalid Date"));
                    }
                    BUILTIN_DATE_TOISOSTRING => {
                        if ms.is_nan() || ms.is_infinite() || ms.abs() > 8.64e15 {
                            self.throw_range_error_object(ctx, "Invalid time value");
                            return Some(Value::Undefined);
                        }
                        let s = if let Some(dt) = to_utc() {
                            let y = dt.year();
                            if (0..=9999).contains(&y) {
                                dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
                            } else if y >= 0 {
                                format!("+{:06}-{}", y, dt.format("%m-%dT%H:%M:%S%.3fZ"))
                            } else {
                                format!("-{:06}-{}", -y, dt.format("%m-%dT%H:%M:%S%.3fZ"))
                            }
                        } else {
                            Self::format_iso_string_arithmetic(ms)
                        };
                        return Some(Value::from(&s));
                    }
                    BUILTIN_DATE_GETFULLYEAR => {
                        return Some(Value::Number(to_local().map(|dt| dt.year() as f64).unwrap_or(f64::NAN)));
                    }
                    BUILTIN_DATE_GETMONTH => {
                        return Some(Value::Number(to_local().map(|dt| (dt.month0()) as f64).unwrap_or(f64::NAN)));
                    }
                    BUILTIN_DATE_GETDATE => {
                        return Some(Value::Number(to_local().map(|dt| dt.day() as f64).unwrap_or(f64::NAN)));
                    }
                    BUILTIN_DATE_GETDAY => {
                        return Some(Value::Number(
                            to_local().map(|dt| dt.weekday().num_days_from_sunday() as f64).unwrap_or(f64::NAN),
                        ));
                    }
                    BUILTIN_DATE_GETHOURS => {
                        return Some(Value::Number(to_local().map(|dt| dt.hour() as f64).unwrap_or(f64::NAN)));
                    }
                    BUILTIN_DATE_GETMINUTES => {
                        return Some(Value::Number(to_local().map(|dt| dt.minute() as f64).unwrap_or(f64::NAN)));
                    }
                    BUILTIN_DATE_GETSECONDS => {
                        return Some(Value::Number(to_local().map(|dt| dt.second() as f64).unwrap_or(f64::NAN)));
                    }
                    BUILTIN_DATE_GETMILLISECONDS => {
                        return Some(Value::Number(
                            to_local().map(|dt| dt.timestamp_subsec_millis() as f64).unwrap_or(f64::NAN),
                        ));
                    }
                    BUILTIN_DATE_GETUTCFULLYEAR => {
                        return Some(Value::Number(to_utc().map(|dt| dt.year() as f64).unwrap_or(f64::NAN)));
                    }
                    BUILTIN_DATE_GETUTCMONTH => {
                        return Some(Value::Number(to_utc().map(|dt| dt.month0() as f64).unwrap_or(f64::NAN)));
                    }
                    BUILTIN_DATE_GETUTCDATE => {
                        return Some(Value::Number(to_utc().map(|dt| dt.day() as f64).unwrap_or(f64::NAN)));
                    }
                    BUILTIN_DATE_GETUTCHOURS => {
                        return Some(Value::Number(to_utc().map(|dt| dt.hour() as f64).unwrap_or(f64::NAN)));
                    }
                    BUILTIN_DATE_GETUTCMINUTES => {
                        return Some(Value::Number(to_utc().map(|dt| dt.minute() as f64).unwrap_or(f64::NAN)));
                    }
                    BUILTIN_DATE_GETUTCSECONDS => {
                        return Some(Value::Number(to_utc().map(|dt| dt.second() as f64).unwrap_or(f64::NAN)));
                    }
                    BUILTIN_DATE_GETUTCDAY => {
                        return Some(Value::Number(
                            to_utc().map(|dt| dt.weekday().num_days_from_sunday() as f64).unwrap_or(f64::NAN),
                        ));
                    }
                    BUILTIN_DATE_GETUTCMILLISECONDS => {
                        return Some(Value::Number(
                            to_utc().map(|dt| dt.timestamp_subsec_millis() as f64).unwrap_or(f64::NAN),
                        ));
                    }
                    BUILTIN_DATE_GETTIMEZONEOFFSET => {
                        if let Some(dt) = to_local() {
                            let mins = -(dt.offset().local_minus_utc() as f64 / 60.0);
                            return Some(Value::Number(mins));
                        }
                        return Some(Value::Number(f64::NAN));
                    }
                    BUILTIN_DATE_TODATESTRING => {
                        if let Some(dt) = to_local() {
                            let y = dt.year();
                            let year_str = Self::format_year_for_display(y);
                            let s = format!("{} {}", dt.format("%a %b %d"), year_str);
                            return Some(Value::from(&s));
                        }
                        return Some(Value::from("Invalid Date"));
                    }
                    BUILTIN_DATE_TOUTCSTRING => {
                        if let Some(dt) = to_utc() {
                            let y = dt.year();
                            let year_str = Self::format_year_for_display(y);
                            let s = format!("{} {} {}", dt.format("%a, %d %b"), year_str, dt.format("%H:%M:%S GMT"));
                            return Some(Value::from(&s));
                        }
                        return Some(Value::from("Invalid Date"));
                    }
                    BUILTIN_DATE_SETTIME => {
                        let new_ms = if args.is_empty() {
                            f64::NAN
                        } else {
                            match self.extract_number_with_coercion(ctx, &args[0]) {
                                Some(n) => Self::time_clip(n),
                                None => return Some(Value::Undefined),
                            }
                        };
                        obj.borrow_mut(ctx).insert("__date_ms__".to_string(), Value::Number(new_ms));
                        return Some(Value::Number(new_ms));
                    }
                    BUILTIN_DATE_SETFULLYEAR => {
                        return Some(self.date_setter_host_fn(ctx, "date.setFullYear", Some(receiver), args));
                    }
                    BUILTIN_DATE_SETDATE => {
                        return Some(self.date_setter_host_fn(ctx, "date.setDate", Some(receiver), args));
                    }
                    BUILTIN_DATE_SETMONTH => {
                        return Some(self.date_setter_host_fn(ctx, "date.setMonth", Some(receiver), args));
                    }
                    BUILTIN_DATE_SETHOURS => {
                        return Some(self.date_setter_host_fn(ctx, "date.setHours", Some(receiver), args));
                    }
                    BUILTIN_DATE_SETMINUTES => {
                        return Some(self.date_setter_host_fn(ctx, "date.setMinutes", Some(receiver), args));
                    }
                    _ => {}
                }
            }
            // Receiver is VmObject but doesn't have __date_ms__ — not a Date
            if Self::is_date_method(id) {
                self.throw_type_error(ctx, "this is not a Date object");
                return Some(Value::Undefined);
            }
        }
        // Non-object receiver for Date methods
        if Self::is_date_method(id) && !matches!(receiver, Value::VmObject(_)) {
            self.throw_type_error(ctx, "this is not a Date object");
            return Some(Value::Undefined);
        }

        None
    }

    /// Check if a function ID is a Date instance method.
    pub(super) fn is_date_method(id: FunctionID) -> bool {
        matches!(
            id,
            BUILTIN_DATE_GETTIME
                | BUILTIN_DATE_VALUEOF
                | BUILTIN_DATE_TOSTRING
                | BUILTIN_DATE_TODATESTRING
                | BUILTIN_DATE_TOUTCSTRING
                | BUILTIN_DATE_TOLOCALEDATESTRING
                | BUILTIN_DATE_TOLOCALETIMESTRING
                | BUILTIN_DATE_TOLOCALESTRING
                | BUILTIN_DATE_TOISOSTRING
                | BUILTIN_DATE_GETFULLYEAR
                | BUILTIN_DATE_GETMONTH
                | BUILTIN_DATE_GETDATE
                | BUILTIN_DATE_GETDAY
                | BUILTIN_DATE_GETHOURS
                | BUILTIN_DATE_GETMINUTES
                | BUILTIN_DATE_GETSECONDS
                | BUILTIN_DATE_GETMILLISECONDS
                | BUILTIN_DATE_GETUTCFULLYEAR
                | BUILTIN_DATE_GETUTCMONTH
                | BUILTIN_DATE_GETUTCDATE
                | BUILTIN_DATE_GETUTCHOURS
                | BUILTIN_DATE_GETUTCMINUTES
                | BUILTIN_DATE_GETUTCSECONDS
                | BUILTIN_DATE_GETUTCDAY
                | BUILTIN_DATE_GETUTCMILLISECONDS
                | BUILTIN_DATE_GETTIMEZONEOFFSET
                | BUILTIN_DATE_SETTIME
                | BUILTIN_DATE_SETFULLYEAR
                | BUILTIN_DATE_SETMONTH
                | BUILTIN_DATE_SETDATE
                | BUILTIN_DATE_SETHOURS
                | BUILTIN_DATE_SETMINUTES
        )
    }

    /// Construct a Date object for `new Date(...)` in opcode handler.
    /// Returns the ms value and whether it needs an abrupt-completion early return.
    pub(super) fn date_construct_ms(&mut self, ctx: &GcContext<'gc>, args: &[Value<'gc>]) -> f64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        if args.is_empty() {
            return SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0);
        }
        if args.len() == 1 {
            if let Value::VmObject(obj) = &args[0] {
                if let Some(Value::Number(ms)) = obj.borrow().get("__date_ms__").cloned() {
                    return Self::time_clip(ms);
                }
                let prim = self.try_to_primitive(ctx, &args[0], "default");
                if self.pending_throw.is_some() {
                    return f64::NAN;
                }
                return match &prim {
                    Value::Number(n) => Self::time_clip(*n),
                    Value::String(s) => {
                        let s_str = crate::unicode::utf16_to_utf8(s);
                        self.date_parse_string(&s_str)
                    }
                    Value::BigInt(_) => {
                        self.throw_type_error(ctx, "Cannot convert a BigInt value to a number");
                        f64::NAN
                    }
                    Value::Symbol(_) => {
                        self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                        f64::NAN
                    }
                    _ if Self::is_symbol_value(&prim) => {
                        self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                        f64::NAN
                    }
                    _ => to_number(&prim),
                };
            }
            let prim = match &args[0] {
                Value::VmArray(_) => self.try_to_primitive(ctx, &args[0], "default"),
                other => other.clone(),
            };
            if self.pending_throw.is_some() {
                return f64::NAN;
            }
            return match &prim {
                Value::Number(n) => Self::time_clip(*n),
                Value::String(s) => {
                    let s_str = crate::unicode::utf16_to_utf8(s);
                    self.date_parse_string(&s_str)
                }
                Value::BigInt(_) => {
                    self.throw_type_error(ctx, "Cannot convert a BigInt value to a number");
                    f64::NAN
                }
                Value::Symbol(_) => {
                    self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                    f64::NAN
                }
                _ if Self::is_symbol_value(&prim) => {
                    self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                    f64::NAN
                }
                _ => to_number(&prim),
            };
        }
        // Multiple args
        let yr = to_number(&args[0]);
        let month = to_number(args.get(1).unwrap_or(&Value::Number(0.0)));
        let day = to_number(args.get(2).unwrap_or(&Value::Number(1.0)));
        let hour = to_number(args.get(3).unwrap_or(&Value::Number(0.0)));
        let min_val = to_number(args.get(4).unwrap_or(&Value::Number(0.0)));
        let sec = to_number(args.get(5).unwrap_or(&Value::Number(0.0)));
        let ms_part = to_number(args.get(6).unwrap_or(&Value::Number(0.0)));
        if yr.is_nan()
            || yr.is_infinite()
            || month.is_nan()
            || month.is_infinite()
            || day.is_nan()
            || day.is_infinite()
            || hour.is_nan()
            || hour.is_infinite()
            || min_val.is_nan()
            || min_val.is_infinite()
            || sec.is_nan()
            || sec.is_infinite()
            || ms_part.is_nan()
            || ms_part.is_infinite()
        {
            f64::NAN
        } else {
            Self::make_date_from_components(yr, month, day, hour, min_val, sec, ms_part, true)
        }
    }

    /// Construct a Date with ToNumber coercion per arg (used in the 4th constructor path).
    /// Returns `Some(ms)` on success, `None` if an abrupt completion happened.
    pub(super) fn date_construct_ms_with_coercion(&mut self, ctx: &GcContext<'gc>, args: &[Value<'gc>]) -> Option<f64> {
        use std::time::{SystemTime, UNIX_EPOCH};
        if args.is_empty() {
            return Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as f64)
                    .unwrap_or(0.0),
            );
        }
        if args.len() == 1 {
            return Some(self.date_construct_ms(ctx, args));
        }
        // Multiple args — coerce via extract_number_with_coercion
        let defaults: [f64; 7] = [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let mut nums = Vec::with_capacity(args.len().min(7));
        for args_i in args.iter().take(args.len().min(7)) {
            match self.extract_number_with_coercion(ctx, args_i) {
                Some(n) => nums.push(n),
                None => return None, // abrupt completion
            }
        }
        let yr = nums[0];
        let month = nums.get(1).copied().unwrap_or(defaults[1]);
        let day = nums.get(2).copied().unwrap_or(defaults[2]);
        let hour = nums.get(3).copied().unwrap_or(defaults[3]);
        let min_val = nums.get(4).copied().unwrap_or(defaults[4]);
        let sec = nums.get(5).copied().unwrap_or(defaults[5]);
        let ms_part = nums.get(6).copied().unwrap_or(defaults[6]);
        if yr.is_nan()
            || yr.is_infinite()
            || month.is_nan()
            || month.is_infinite()
            || day.is_nan()
            || day.is_infinite()
            || hour.is_nan()
            || hour.is_infinite()
            || min_val.is_nan()
            || min_val.is_infinite()
            || sec.is_nan()
            || sec.is_infinite()
            || ms_part.is_nan()
            || ms_part.is_infinite()
        {
            Some(f64::NAN)
        } else {
            Some(Self::make_date_from_components(yr, month, day, hour, min_val, sec, ms_part, true))
        }
    }

    /// Handle date setter host functions (date.setUTCFullYear, date.setSeconds, etc.)
    fn date_setter_host_fn(&mut self, ctx: &GcContext<'gc>, name: &str, receiver: Option<&Value<'gc>>, args: &[Value<'gc>]) -> Value<'gc> {
        let this = receiver.cloned().unwrap_or(Value::Undefined);
        let Value::VmObject(obj) = &this else {
            self.throw_type_error(ctx, "this is not a Date object");
            return Value::Undefined;
        };
        let ms_val = obj.borrow().get("__date_ms__").cloned();
        let Some(Value::Number(ms)) = ms_val else {
            self.throw_type_error(ctx, "this is not a Date object");
            return Value::Undefined;
        };

        // Coerce all arguments to numbers first (per spec: all ToNumber happen before mutation)
        let mut nums: Vec<f64> = Vec::with_capacity(args.len());
        for a in args {
            match self.extract_number_with_coercion(ctx, a) {
                Some(n) => nums.push(n),
                None => return Value::Undefined,
            }
        }

        let any_nan = nums.iter().any(|n| n.is_nan() || n.is_infinite());
        let is_utc = name.contains("UTC");

        let get_components = |ms: f64, utc: bool| -> Option<(f64, f64, f64, f64, f64, f64, f64)> {
            if ms.is_nan() || ms.is_infinite() {
                return None;
            }
            use chrono::{Datelike, Local, TimeZone, Timelike, Utc};
            if utc {
                Utc.timestamp_millis_opt(ms as i64).single().map(|dt| {
                    (
                        dt.year() as f64,
                        (dt.month0()) as f64,
                        dt.day() as f64,
                        dt.hour() as f64,
                        dt.minute() as f64,
                        dt.second() as f64,
                        dt.timestamp_subsec_millis() as f64,
                    )
                })
            } else {
                Local.timestamp_millis_opt(ms as i64).single().map(|dt| {
                    (
                        dt.year() as f64,
                        (dt.month0()) as f64,
                        dt.day() as f64,
                        dt.hour() as f64,
                        dt.minute() as f64,
                        dt.second() as f64,
                        dt.timestamp_subsec_millis() as f64,
                    )
                })
            }
        };

        let new_ms = match name {
            "date.setUTCFullYear" | "date.setFullYear" => {
                if args.is_empty() || any_nan {
                    f64::NAN
                } else {
                    let comps = get_components(ms, is_utc).unwrap_or((0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0));
                    let year = nums[0];
                    let month = nums.get(1).copied().unwrap_or(comps.1);
                    let day = nums.get(2).copied().unwrap_or(comps.2);
                    Self::make_date_from_components_no_year_adjust(year, month, day, comps.3, comps.4, comps.5, comps.6, !is_utc)
                }
            }
            _ if ms.is_nan() => {
                return Value::Number(f64::NAN);
            }
            "date.setUTCMonth" | "date.setMonth" => {
                if args.is_empty() || any_nan {
                    f64::NAN
                } else {
                    let Some(comps) = get_components(ms, is_utc) else {
                        return self.date_store_nan(ctx, obj);
                    };
                    let month = nums[0];
                    let day = nums.get(1).copied().unwrap_or(comps.2);
                    Self::make_date_from_components_no_year_adjust(comps.0, month, day, comps.3, comps.4, comps.5, comps.6, !is_utc)
                }
            }
            "date.setUTCDate" | "date.setDate" => {
                if args.is_empty() || any_nan {
                    f64::NAN
                } else {
                    let Some(comps) = get_components(ms, is_utc) else {
                        return self.date_store_nan(ctx, obj);
                    };
                    let day = nums[0];
                    Self::make_date_from_components_no_year_adjust(comps.0, comps.1, day, comps.3, comps.4, comps.5, comps.6, !is_utc)
                }
            }
            "date.setUTCHours" | "date.setHours" => {
                if args.is_empty() || any_nan {
                    f64::NAN
                } else {
                    let Some(comps) = get_components(ms, is_utc) else {
                        return self.date_store_nan(ctx, obj);
                    };
                    let hour = nums[0];
                    let min = nums.get(1).copied().unwrap_or(comps.4);
                    let sec = nums.get(2).copied().unwrap_or(comps.5);
                    let ms_val = nums.get(3).copied().unwrap_or(comps.6);
                    Self::make_date_from_components_no_year_adjust(comps.0, comps.1, comps.2, hour, min, sec, ms_val, !is_utc)
                }
            }
            "date.setUTCMinutes" | "date.setMinutes" => {
                if args.is_empty() || any_nan {
                    f64::NAN
                } else {
                    let Some(comps) = get_components(ms, is_utc) else {
                        return self.date_store_nan(ctx, obj);
                    };
                    let min = nums[0];
                    let sec = nums.get(1).copied().unwrap_or(comps.5);
                    let ms_val = nums.get(2).copied().unwrap_or(comps.6);
                    Self::make_date_from_components_no_year_adjust(comps.0, comps.1, comps.2, comps.3, min, sec, ms_val, !is_utc)
                }
            }
            "date.setUTCSeconds" | "date.setSeconds" => {
                if args.is_empty() || any_nan {
                    f64::NAN
                } else {
                    let Some(comps) = get_components(ms, is_utc) else {
                        return self.date_store_nan(ctx, obj);
                    };
                    let sec = nums[0];
                    let ms_val = nums.get(1).copied().unwrap_or(comps.6);
                    Self::make_date_from_components_no_year_adjust(comps.0, comps.1, comps.2, comps.3, comps.4, sec, ms_val, !is_utc)
                }
            }
            "date.setUTCMilliseconds" | "date.setMilliseconds" => {
                if args.is_empty() || any_nan {
                    f64::NAN
                } else {
                    let Some(comps) = get_components(ms, is_utc) else {
                        return self.date_store_nan(ctx, obj);
                    };
                    let ms_val = nums[0];
                    Self::make_date_from_components_no_year_adjust(comps.0, comps.1, comps.2, comps.3, comps.4, comps.5, ms_val, !is_utc)
                }
            }
            _ => {
                if args.is_empty() {
                    f64::NAN
                } else {
                    nums[0]
                }
            }
        };
        obj.borrow_mut(ctx).insert("__date_ms__".to_string(), Value::Number(new_ms));
        Value::Number(new_ms)
    }

    fn date_store_nan(&mut self, ctx: &GcContext<'gc>, obj: &Gc<'gc, GcCell<IndexMap<String, Value<'gc>>>>) -> Value<'gc> {
        obj.borrow_mut(ctx).insert("__date_ms__".to_string(), Value::Number(f64::NAN));
        Value::Number(f64::NAN)
    }

    /// Compute a Date ms value from year/month/day/hour/min/sec/ms components.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn make_date_from_components(yr: f64, month: f64, day: f64, hour: f64, min: f64, sec: f64, ms: f64, local: bool) -> f64 {
        Self::make_date_from_components_inner(yr, month, day, hour, min, sec, ms, local, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn make_date_from_components_no_year_adjust(yr: f64, month: f64, day: f64, hour: f64, min: f64, sec: f64, ms: f64, local: bool) -> f64 {
        Self::make_date_from_components_inner(yr, month, day, hour, min, sec, ms, local, false)
    }

    #[allow(clippy::too_many_arguments)]
    fn make_date_from_components_inner(
        yr: f64,
        month: f64,
        day: f64,
        hour: f64,
        min: f64,
        sec: f64,
        ms: f64,
        local: bool,
        ctor_year_adjust: bool,
    ) -> f64 {
        let yr = yr.trunc();
        let month = month.trunc();
        let day = day.trunc();
        let hour = hour.trunc();
        let min = min.trunc();
        let sec = sec.trunc();
        let ms = ms.trunc();

        let yi = yr as i64;
        let full_year = if ctor_year_adjust && (0..100).contains(&yi) {
            yi + 1900
        } else {
            yi
        };
        let mi = month as i64;
        let adj_year = full_year + mi.div_euclid(12);
        let adj_month = mi.rem_euclid(12) as u32;

        let base_day = 1u32;
        use chrono::{Local, NaiveDate, TimeZone};
        let base_ms = if let Some(naive) = NaiveDate::from_ymd_opt(adj_year as i32, adj_month + 1, base_day) {
            let naive_dt = match naive.and_hms_opt(0, 0, 0) {
                Some(d) => d,
                None => return f64::NAN,
            };
            if local {
                match Local.from_local_datetime(&naive_dt).single() {
                    Some(dt) => dt.timestamp_millis() as f64,
                    None => return f64::NAN,
                }
            } else {
                naive_dt.and_utc().timestamp_millis() as f64
            }
        } else if !local {
            Self::make_day_arithmetic(adj_year, adj_month as i64) * 86_400_000.0
        } else {
            return f64::NAN;
        };
        let day_offset = day - 1.0;
        let total_ms = base_ms + day_offset * 86_400_000.0 + hour * 3_600_000.0 + min * 60_000.0 + sec * 1_000.0 + ms;
        Self::time_clip(total_ms)
    }

    pub(super) fn format_year_for_display(y: i32) -> String {
        if y >= 0 { format!("{:04}", y) } else { format!("-{:04}", -y) }
    }

    pub(super) fn time_clip(t: f64) -> f64 {
        if t.is_nan() || t.is_infinite() || t.abs() > 8.64e15 {
            f64::NAN
        } else {
            let v = t.trunc();
            if v == 0.0 { 0.0 } else { v }
        }
    }

    /// Pure arithmetic MakeDay for years outside chrono's range.
    fn make_day_arithmetic(year: i64, month0: i64) -> f64 {
        const MONTH_DAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        fn is_leap(y: i64) -> bool {
            y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
        }
        fn days_from_year(y: i64) -> i64 {
            365 * (y - 1970) + ((y - 1969) as f64 / 4.0).floor() as i64 - ((y - 1901) as f64 / 100.0).floor() as i64
                + ((y - 1601) as f64 / 400.0).floor() as i64
        }
        let mut d = days_from_year(year);
        for m in 0..month0 {
            d += MONTH_DAYS[m as usize];
            if m == 1 && is_leap(year) {
                d += 1;
            }
        }
        d as f64
    }

    /// Format a UTC date as ISO string using pure arithmetic (for dates outside chrono's range)
    pub(super) fn format_iso_string_arithmetic(ms: f64) -> String {
        const MONTH_DAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        fn is_leap(y: i64) -> bool {
            y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
        }
        fn days_in_year(y: i64) -> i64 {
            if is_leap(y) { 366 } else { 365 }
        }

        let total_ms = ms as i64;
        let mut remaining_days = total_ms.div_euclid(86_400_000);
        let time_ms = total_ms.rem_euclid(86_400_000);
        let hours = time_ms / 3_600_000;
        let minutes = (time_ms % 3_600_000) / 60_000;
        let seconds = (time_ms % 60_000) / 1_000;
        let millis = time_ms % 1_000;

        let mut year: i64 = 1970;
        if remaining_days >= 0 {
            loop {
                let dy = days_in_year(year);
                if remaining_days < dy {
                    break;
                }
                remaining_days -= dy;
                year += 1;
            }
        } else {
            loop {
                year -= 1;
                remaining_days += days_in_year(year);
                if remaining_days >= 0 {
                    break;
                }
            }
        }

        let mut month = 0u32;
        loop {
            let dm = MONTH_DAYS[month as usize] + if month == 1 && is_leap(year) { 1 } else { 0 };
            if remaining_days < dm {
                break;
            }
            remaining_days -= dm;
            month += 1;
        }
        let day = remaining_days + 1;

        if (0..=9999).contains(&year) {
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                year,
                month + 1,
                day,
                hours,
                minutes,
                seconds,
                millis
            )
        } else if year >= 0 {
            format!(
                "+{:06}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                year,
                month + 1,
                day,
                hours,
                minutes,
                seconds,
                millis
            )
        } else {
            format!(
                "-{:06}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                -year,
                month + 1,
                day,
                hours,
                minutes,
                seconds,
                millis
            )
        }
    }

    pub(super) fn date_parse_string(&self, s: &str) -> f64 {
        if s.starts_with("-000000") {
            return f64::NAN;
        }
        if let Some(ms) = Self::parse_extended_year_iso(s) {
            return Self::time_clip(ms);
        }
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            return Self::time_clip(dt.timestamp_millis() as f64);
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ") {
            return Self::time_clip(dt.and_utc().timestamp_millis() as f64);
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%MZ") {
            return Self::time_clip(dt.and_utc().timestamp_millis() as f64);
        }
        if let Ok(dt) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            && let Some(d) = dt.and_hms_opt(0, 0, 0)
        {
            return Self::time_clip(d.and_utc().timestamp_millis() as f64);
        }
        if s.len() >= 7
            && s.len() <= 7
            && s.chars().nth(4) == Some('-')
            && let Ok(dt) = chrono::NaiveDate::parse_from_str(&format!("{}-01", s), "%Y-%m-%d")
            && let Some(d) = dt.and_hms_opt(0, 0, 0)
        {
            return Self::time_clip(d.and_utc().timestamp_millis() as f64);
        }
        if s.len() == 4
            && s.chars().all(|c| c.is_ascii_digit())
            && let Ok(year) = s.parse::<i32>()
            && let Some(dt) = chrono::NaiveDate::from_ymd_opt(year, 1, 1)
            && let Some(d) = dt.and_hms_opt(0, 0, 0)
        {
            return Self::time_clip(d.and_utc().timestamp_millis() as f64);
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
            use chrono::{Local, TimeZone};
            if let Some(local_dt) = Local.from_local_datetime(&dt).single() {
                return Self::time_clip(local_dt.timestamp_millis() as f64);
            }
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
            use chrono::{Local, TimeZone};
            if let Some(local_dt) = Local.from_local_datetime(&dt).single() {
                return Self::time_clip(local_dt.timestamp_millis() as f64);
            }
        }
        if let Some(ms) = Self::parse_tostring_format(s) {
            return Self::time_clip(ms);
        }
        if let Some(ms) = Self::parse_utcstring_format(s) {
            return Self::time_clip(ms);
        }
        f64::NAN
    }

    fn month_name_to_num(name: &str) -> Option<u32> {
        match name {
            "Jan" => Some(1),
            "Feb" => Some(2),
            "Mar" => Some(3),
            "Apr" => Some(4),
            "May" => Some(5),
            "Jun" => Some(6),
            "Jul" => Some(7),
            "Aug" => Some(8),
            "Sep" => Some(9),
            "Oct" => Some(10),
            "Nov" => Some(11),
            "Dec" => Some(12),
            _ => None,
        }
    }

    fn parse_tostring_format(s: &str) -> Option<f64> {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() < 5 {
            return None;
        }
        let month = Self::month_name_to_num(parts[1])?;
        let day: u32 = parts[2].parse().ok()?;
        let year: i32 = parts[3].parse().ok()?;
        let time_parts: Vec<&str> = parts[4].split(':').collect();
        if time_parts.len() != 3 {
            return None;
        }
        let hour: u32 = time_parts[0].parse().ok()?;
        let minute: u32 = time_parts[1].parse().ok()?;
        let sec: u32 = time_parts[2].parse().ok()?;

        use chrono::NaiveDate;
        let naive = NaiveDate::from_ymd_opt(year, month, day)?;
        let naive_dt = naive.and_hms_opt(hour, minute, sec)?;

        if parts.len() >= 6 && parts[5].starts_with("GMT") {
            let tz_str = &parts[5][3..];
            if tz_str.len() >= 4 {
                let tz_sign: i64 = if tz_str.starts_with('-') { -1 } else { 1 };
                let tz_digits = &tz_str[1..];
                let tz_h: i64 = tz_digits[..2].parse().ok()?;
                let tz_m: i64 = tz_digits[2..4].parse().ok()?;
                let tz_offset_ms = tz_sign * (tz_h * 3_600_000 + tz_m * 60_000);
                let utc_ms = naive_dt.and_utc().timestamp_millis() as f64 - tz_offset_ms as f64;
                return Some(utc_ms);
            }
        }
        use chrono::{Local, TimeZone};
        let local_dt = Local.from_local_datetime(&naive_dt).single()?;
        Some(local_dt.timestamp_millis() as f64)
    }

    fn parse_utcstring_format(s: &str) -> Option<f64> {
        let s = s.trim_end_matches(" GMT");
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() < 5 {
            return None;
        }
        let day_str = parts[1];
        let month_str = parts[2];
        let year_str = parts[3];
        let time_str = parts[4];

        let day: u32 = day_str.parse().ok()?;
        let month = Self::month_name_to_num(month_str)?;
        let year: i32 = year_str.parse().ok()?;
        let time_parts: Vec<&str> = time_str.split(':').collect();
        if time_parts.len() != 3 {
            return None;
        }
        let hour: u32 = time_parts[0].parse().ok()?;
        let minute: u32 = time_parts[1].parse().ok()?;
        let sec: u32 = time_parts[2].parse().ok()?;

        use chrono::NaiveDate;
        let naive = NaiveDate::from_ymd_opt(year, month, day)?;
        let naive_dt = naive.and_hms_opt(hour, minute, sec)?;
        Some(naive_dt.and_utc().timestamp_millis() as f64)
    }

    fn parse_extended_year_iso(s: &str) -> Option<f64> {
        if s.len() < 7 {
            return None;
        }
        let (sign, rest) = if let Some(stripped) = s.strip_prefix('+') {
            (1i64, stripped)
        } else if let Some(stripped) = s.strip_prefix('-') {
            (-1_i64, stripped)
        } else {
            return None;
        };
        let year_end = rest.find('-')?;
        if year_end < 4 {
            return None;
        }
        let year_str = &rest[..year_end];
        let year: i64 = year_str.parse().ok()?;
        if sign == -1 && year == 0 {
            return None;
        }
        let year = sign * year;
        let remainder = &rest[year_end..];

        let parts: Vec<&str> = remainder.split('T').collect();
        let date_part = parts[0];
        let date_fields: Vec<&str> = date_part.split('-').filter(|x| !x.is_empty()).collect();
        let month: u32 = date_fields.first()?.parse().ok()?;
        let day: u32 = date_fields.get(1).and_then(|x| x.parse().ok()).unwrap_or(1);

        let (hour, minute, sec, ms_val, tz_offset_ms) = if parts.len() > 1 {
            let time_str = parts[1];
            let (time_part, tz_off) = if time_str.ends_with('Z') {
                (time_str.trim_end_matches('Z'), Some(0i64))
            } else if let Some(pos) = time_str.rfind('+') {
                if pos > 0 {
                    let tz = &time_str[pos + 1..];
                    let tz_parts: Vec<&str> = tz.split(':').collect();
                    let tz_h: i64 = tz_parts.first().and_then(|x| x.parse().ok()).unwrap_or(0);
                    let tz_m: i64 = tz_parts.get(1).and_then(|x| x.parse().ok()).unwrap_or(0);
                    (&time_str[..pos], Some(-(tz_h * 60 + tz_m) * 60000))
                } else {
                    (time_str, None)
                }
            } else if let Some(pos) = time_str.rfind('-') {
                if pos > 0 {
                    let tz = &time_str[pos + 1..];
                    let tz_parts: Vec<&str> = tz.split(':').collect();
                    let tz_h: i64 = tz_parts.first().and_then(|x| x.parse().ok()).unwrap_or(0);
                    let tz_m: i64 = tz_parts.get(1).and_then(|x| x.parse().ok()).unwrap_or(0);
                    (&time_str[..pos], Some((tz_h * 60 + tz_m) * 60000))
                } else {
                    (time_str, None)
                }
            } else {
                (time_str, None)
            };
            let time_fields: Vec<&str> = time_part.split(':').collect();
            let h: u32 = time_fields.first().and_then(|x| x.parse().ok()).unwrap_or(0);
            let m: u32 = time_fields.get(1).and_then(|x| x.parse().ok()).unwrap_or(0);
            let (s, ms) = if let Some(sec_str) = time_fields.get(2) {
                let sec_parts: Vec<&str> = sec_str.split('.').collect();
                let s: u32 = sec_parts[0].parse().ok()?;
                let ms: f64 = if sec_parts.len() > 1 {
                    let frac = sec_parts[1];
                    let frac_val: f64 = frac.parse().ok()?;
                    frac_val / 10f64.powi(frac.len() as i32) * 1000.0
                } else {
                    0.0
                };
                (s, ms)
            } else {
                (0, 0.0)
            };
            (h, m, s, ms, tz_off)
        } else {
            (0, 0, 0, 0.0, Some(0))
        };

        use chrono::NaiveDate;
        let chrono_result = if let Some(naive) = NaiveDate::from_ymd_opt(year as i32, month, day) {
            naive.and_hms_opt(hour, minute, sec).map(|naive_dt| {
                let base_ms = naive_dt.and_utc().timestamp_millis() as f64 + ms_val;
                if let Some(tz) = tz_offset_ms {
                    base_ms + tz as f64
                } else {
                    use chrono::{Local, TimeZone};
                    if let Some(local_dt) = Local.from_local_datetime(&naive_dt).single() {
                        local_dt.timestamp_millis() as f64 + ms_val
                    } else {
                        base_ms
                    }
                }
            })
        } else {
            None
        };
        if let Some(ms) = chrono_result {
            return Some(ms);
        }
        // Arithmetic fallback for years outside chrono's NaiveDate range
        let base_ms = (Self::make_day_arithmetic(year, (month as i64) - 1) + (day as f64 - 1.0)) * 86_400_000.0
            + (hour as f64) * 3_600_000.0
            + (minute as f64) * 60_000.0
            + (sec as f64) * 1000.0
            + ms_val;
        let adjusted = if let Some(tz) = tz_offset_ms {
            base_ms + tz as f64
        } else {
            use chrono::Local;
            let offset_ms = Local::now().offset().local_minus_utc() as f64 * 1000.0;
            base_ms - offset_ms
        };
        Some(adjusted)
    }
}
