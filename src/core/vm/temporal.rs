use super::*;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::cmp::Ordering;
use std::str::FromStr;
use temporal_rs::fields::{CalendarFields, ZonedDateTimeFields};
use temporal_rs::options::{
    DifferenceSettings, Disambiguation, DisplayCalendar, DisplayOffset, DisplayTimeZone, OffsetDisambiguation, Overflow, RelativeTo,
    RoundingIncrement, RoundingMode, RoundingOptions, ToStringRoundingOptions, Unit,
};
use temporal_rs::parsers::Precision;
use temporal_rs::partial::PartialTime;
use temporal_rs::{
    Calendar, Duration as TemporalDuration, Instant, MonthCode, PlainDate, PlainDateTime, PlainMonthDay, PlainTime, PlainYearMonth,
    Temporal, TemporalError, TimeZone, UtcOffset, ZonedDateTime,
};

const SLOT_KIND: &str = "__temporal_kind__";
const SLOT_REPR: &str = "__temporal_repr__";
const SLOT_EPOCH_NS: &str = "__temporal_epoch_nanoseconds__";
const SLOT_REFERENCE_DAY: &str = "__temporal_reference_day__";
const SLOT_REFERENCE_YEAR: &str = "__temporal_reference_year__";

impl<'gc> VM<'gc> {
    pub(super) fn temporal_handle_host_fn(
        &mut self,
        ctx: &GcContext<'gc>,
        name: &str,
        receiver: Option<&Value<'gc>>,
        args: &[Value<'gc>],
    ) -> Value<'gc> {
        match name {
            "temporal.instant.constructor" => {
                let Some(epoch_ns) = args.first().and_then(|v| self.temporal_bigint_i128(ctx, v, "epochNanoseconds")) else {
                    self.throw_type_error(ctx, "Temporal.Instant requires an epochNanoseconds argument");
                    return Value::Undefined;
                };
                match Instant::try_new(epoch_ns) {
                    Ok(value) => self.temporal_wrap_instant(ctx, receiver, &value),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.instant.from" => match self.temporal_from_instant(ctx, args.first()) {
                Ok(value) => self.temporal_wrap_instant(ctx, receiver, &value),
                Err(err) => self.temporal_throw(ctx, err),
            },
            "temporal.instant.compare" => {
                let Some(first) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.Instant.compare requires two arguments");
                    return Value::Undefined;
                };
                let Some(second) = args.get(1) else {
                    self.throw_type_error(ctx, "Temporal.Instant.compare requires two arguments");
                    return Value::Undefined;
                };
                let one = match self.temporal_from_instant(ctx, Some(first)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let two = match self.temporal_from_instant(ctx, Some(second)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                Self::temporal_compare_result(one.as_i128().cmp(&two.as_i128()))
            }
            "temporal.instant.add" => {
                let Some(value) = self.temporal_expect_instant(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.Instant.prototype.add requires a duration");
                    return Value::Undefined;
                };
                let duration = match self.temporal_to_duration(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.add(&duration) {
                    Ok(value) => {
                        self.temporal_wrap_instant(ctx, self.temporal_intrinsic_ctor_value("Instant").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.instant.subtract" => {
                let Some(value) = self.temporal_expect_instant(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.Instant.prototype.subtract requires a duration");
                    return Value::Undefined;
                };
                let duration = match self.temporal_to_duration(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.subtract(&duration) {
                    Ok(value) => {
                        self.temporal_wrap_instant(ctx, self.temporal_intrinsic_ctor_value("Instant").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.instant.until" => {
                let Some(value) = self.temporal_expect_instant(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.Instant.prototype.until requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_from_instant(ctx, Some(arg)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let options = match self.temporal_difference_settings_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.until(&other, options) {
                    Ok(value) => {
                        self.temporal_wrap_duration(ctx, self.temporal_intrinsic_ctor_value("Duration").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.instant.since" => {
                let Some(value) = self.temporal_expect_instant(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.Instant.prototype.since requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_from_instant(ctx, Some(arg)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let options = match self.temporal_difference_settings_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.since(&other, options) {
                    Ok(value) => {
                        self.temporal_wrap_duration(ctx, self.temporal_intrinsic_ctor_value("Duration").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.instant.equals" => {
                let Some(value) = self.temporal_expect_instant(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.Instant.prototype.equals requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_from_instant(ctx, Some(arg)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                Value::Boolean(value == other)
            }
            "temporal.instant.toString" | "temporal.instant.toJSON" => self.temporal_repr_result(ctx, receiver, "Instant"),
            "temporal.instant.valueOf" => {
                self.throw_type_error(ctx, "Cannot convert Temporal.Instant to a primitive value");
                Value::Undefined
            }
            "temporal.instant.get.epochMilliseconds" => {
                let Some(value) = self.temporal_expect_instant(ctx, receiver) else {
                    return Value::Undefined;
                };
                Value::Number(value.epoch_milliseconds() as f64)
            }
            "temporal.instant.get.epochNanoseconds" => {
                let Some(epoch_ns) = self.temporal_slot_string_value(ctx, receiver, "Instant", SLOT_EPOCH_NS) else {
                    return Value::Undefined;
                };
                match BigInt::from_str(&epoch_ns) {
                    Ok(value) => Value::BigInt(Box::new(value)),
                    Err(_) => {
                        self.throw_range_error_object(ctx, "Invalid Temporal.Instant epochNanoseconds slot");
                        Value::Undefined
                    }
                }
            }

            "temporal.plainDate.constructor" => {
                let Some(year) = args.first().and_then(|v| self.temporal_number_i32(ctx, v, "year")) else {
                    self.throw_type_error(ctx, "Temporal.PlainDate requires year, month, and day");
                    return Value::Undefined;
                };
                let Some(month) = args.get(1).and_then(|v| self.temporal_number_u8(ctx, v, "month")) else {
                    self.throw_type_error(ctx, "Temporal.PlainDate requires year, month, and day");
                    return Value::Undefined;
                };
                let Some(day) = args.get(2).and_then(|v| self.temporal_number_u8(ctx, v, "day")) else {
                    self.throw_type_error(ctx, "Temporal.PlainDate requires year, month, and day");
                    return Value::Undefined;
                };
                let calendar = match self.temporal_calendar_arg(ctx, args.get(3)) {
                    Some(calendar) => calendar,
                    None => return Value::Undefined,
                };
                let result = if calendar == Calendar::ISO {
                    PlainDate::try_new_iso(year, month, day)
                } else {
                    PlainDate::try_new(year, month, day, calendar)
                };
                match result {
                    Ok(value) => self.temporal_wrap_plain_date(ctx, receiver, &value),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainDate.from" => match self.temporal_from_plain_date(ctx, args.first()) {
                Ok(value) => self.temporal_wrap_plain_date(ctx, receiver, &value),
                Err(err) => self.temporal_throw(ctx, err),
            },
            "temporal.plainDate.compare" => {
                let Some(first) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainDate.compare requires two arguments");
                    return Value::Undefined;
                };
                let Some(second) = args.get(1) else {
                    self.throw_type_error(ctx, "Temporal.PlainDate.compare requires two arguments");
                    return Value::Undefined;
                };
                let one = match self.temporal_from_plain_date(ctx, Some(first)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let two = match self.temporal_from_plain_date(ctx, Some(second)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                Self::temporal_compare_result((one.year(), one.month(), one.day()).cmp(&(two.year(), two.month(), two.day())))
            }
            "temporal.plainDate.add" => {
                let Some(value) = self.temporal_expect_plain_date(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainDate.prototype.add requires a duration");
                    return Value::Undefined;
                };
                let duration = match self.temporal_to_duration(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let overflow = match self.temporal_overflow_option_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.add(&duration, Some(overflow)) {
                    Ok(value) => {
                        self.temporal_wrap_plain_date(ctx, self.temporal_intrinsic_ctor_value("PlainDate").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainDate.subtract" => {
                let Some(value) = self.temporal_expect_plain_date(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainDate.prototype.subtract requires a duration");
                    return Value::Undefined;
                };
                let duration = match self.temporal_to_duration(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let overflow = match self.temporal_overflow_option_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.subtract(&duration, Some(overflow)) {
                    Ok(value) => {
                        self.temporal_wrap_plain_date(ctx, self.temporal_intrinsic_ctor_value("PlainDate").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainDate.until" => {
                let Some(value) = self.temporal_expect_plain_date(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainDate.prototype.until requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_from_plain_date(ctx, Some(arg)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let options = match self.temporal_difference_settings_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.until(&other, options) {
                    Ok(value) => {
                        self.temporal_wrap_duration(ctx, self.temporal_intrinsic_ctor_value("Duration").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainDate.since" => {
                let Some(value) = self.temporal_expect_plain_date(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainDate.prototype.since requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_from_plain_date(ctx, Some(arg)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let options = match self.temporal_difference_settings_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.since(&other, options) {
                    Ok(value) => {
                        self.temporal_wrap_duration(ctx, self.temporal_intrinsic_ctor_value("Duration").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainDate.toZonedDateTime" => {
                let Some(value) = self.temporal_expect_plain_date(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainDate.prototype.toZonedDateTime requires a timeZone");
                    return Value::Undefined;
                };

                let (time_zone_like, plain_time_like) = if self.temporal_is_object_like(arg) {
                    let time_zone_like = self.read_named_property(ctx, arg, "timeZone");
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    if matches!(time_zone_like, Value::Undefined) {
                        (arg.clone(), Value::Undefined)
                    } else {
                        let plain_time_like = self.read_named_property(ctx, arg, "plainTime");
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        (time_zone_like, plain_time_like)
                    }
                } else {
                    (arg.clone(), Value::Undefined)
                };

                let time_zone = match self.temporal_time_zone_with_iso_string_arg(ctx, Some(&time_zone_like)) {
                    Ok(Some(value)) => value,
                    Ok(None) => {
                        self.throw_type_error(ctx, "Temporal.PlainDate.prototype.toZonedDateTime requires a timeZone");
                        return Value::Undefined;
                    }
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let plain_time = match self
                    .temporal_to_plain_time_like(ctx, (!matches!(plain_time_like, Value::Undefined)).then_some(&plain_time_like))
                {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let (hour, minute, second, millisecond, microsecond, nanosecond) = match plain_time {
                    Some(value) => (
                        value.hour(),
                        value.minute(),
                        value.second(),
                        value.millisecond(),
                        value.microsecond(),
                        value.nanosecond(),
                    ),
                    None => (0, 0, 0, 0, 0, 0),
                };
                let date_time = if value.calendar() == &Calendar::ISO {
                    PlainDateTime::try_new_iso(
                        value.year(),
                        value.month(),
                        value.day(),
                        hour,
                        minute,
                        second,
                        millisecond,
                        microsecond,
                        nanosecond,
                    )
                } else {
                    PlainDateTime::try_new(
                        value.year(),
                        value.month(),
                        value.day(),
                        hour,
                        minute,
                        second,
                        millisecond,
                        microsecond,
                        nanosecond,
                        value.calendar().clone(),
                    )
                };
                match date_time.and_then(|value| value.to_zoned_date_time(time_zone, Disambiguation::Compatible)) {
                    Ok(value) => {
                        let ctor_value = self.temporal_intrinsic_ctor_value("ZonedDateTime");
                        self.temporal_wrap_zoned_date_time(ctx, ctor_value.as_ref(), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainDate.toPlainDateTime" => {
                let Some(value) = self.temporal_expect_plain_date(ctx, receiver) else {
                    return Value::Undefined;
                };
                let plain_time = match self.temporal_to_plain_time_like(ctx, args.first()) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let (hour, minute, second, millisecond, microsecond, nanosecond) = match plain_time {
                    Some(value) => (
                        value.hour(),
                        value.minute(),
                        value.second(),
                        value.millisecond(),
                        value.microsecond(),
                        value.nanosecond(),
                    ),
                    None => (0, 0, 0, 0, 0, 0),
                };
                let date_time = if value.calendar() == &Calendar::ISO {
                    PlainDateTime::try_new_iso(
                        value.year(),
                        value.month(),
                        value.day(),
                        hour,
                        minute,
                        second,
                        millisecond,
                        microsecond,
                        nanosecond,
                    )
                } else {
                    PlainDateTime::try_new(
                        value.year(),
                        value.month(),
                        value.day(),
                        hour,
                        minute,
                        second,
                        millisecond,
                        microsecond,
                        nanosecond,
                        value.calendar().clone(),
                    )
                };
                match date_time {
                    Ok(value) => {
                        let ctor_value = self.temporal_intrinsic_ctor_value("PlainDateTime");
                        self.temporal_wrap_plain_date_time(ctx, ctor_value.as_ref(), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainDate.toString" | "temporal.plainDate.toJSON" => self.temporal_repr_result(ctx, receiver, "PlainDate"),
            "temporal.plainDate.equals" => {
                let Some(value) = self.temporal_expect_plain_date(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainDate.prototype.equals requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_from_plain_date(ctx, Some(arg)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                Value::Boolean(
                    (value.year(), value.month(), value.day(), value.calendar().identifier())
                        == (other.year(), other.month(), other.day(), other.calendar().identifier()),
                )
            }
            "temporal.plainDate.valueOf" => {
                self.throw_type_error(ctx, "Cannot convert Temporal.PlainDate to a primitive value");
                Value::Undefined
            }
            "temporal.plainDate.get.year" => self.temporal_plain_date_number(ctx, receiver, "year"),
            "temporal.plainDate.get.month" => self.temporal_plain_date_number(ctx, receiver, "month"),
            "temporal.plainDate.get.monthCode" => self.temporal_plain_date_month_code(ctx, receiver),
            "temporal.plainDate.get.day" => self.temporal_plain_date_number(ctx, receiver, "day"),
            "temporal.plainDate.get.calendarId" => self.temporal_plain_date_calendar(ctx, receiver),

            "temporal.plainTime.constructor" => {
                let hour = args.first().and_then(|v| self.temporal_number_u8(ctx, v, "hour")).unwrap_or(0);
                let minute = args.get(1).and_then(|v| self.temporal_number_u8(ctx, v, "minute")).unwrap_or(0);
                let second = args.get(2).and_then(|v| self.temporal_number_u8(ctx, v, "second")).unwrap_or(0);
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let millisecond = args
                    .get(3)
                    .and_then(|v| self.temporal_number_u16(ctx, v, "millisecond"))
                    .unwrap_or(0);
                let microsecond = args
                    .get(4)
                    .and_then(|v| self.temporal_number_u16(ctx, v, "microsecond"))
                    .unwrap_or(0);
                let nanosecond = args
                    .get(5)
                    .and_then(|v| self.temporal_number_u16(ctx, v, "nanosecond"))
                    .unwrap_or(0);
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                match PlainTime::try_new(hour, minute, second, millisecond, microsecond, nanosecond) {
                    Ok(value) => self.temporal_wrap_plain_time(ctx, receiver, &value),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainTime.from" => match self.temporal_from_plain_time(ctx, args.first()) {
                Ok(value) => self.temporal_wrap_plain_time(ctx, receiver, &value),
                Err(err) => self.temporal_throw(ctx, err),
            },
            "temporal.plainTime.compare" => {
                let Some(first) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainTime.compare requires two arguments");
                    return Value::Undefined;
                };
                let Some(second) = args.get(1) else {
                    self.throw_type_error(ctx, "Temporal.PlainTime.compare requires two arguments");
                    return Value::Undefined;
                };
                let one = match self.temporal_to_plain_time_like(ctx, Some(first)) {
                    Ok(Some(value)) => value,
                    Ok(None) => {
                        self.throw_type_error(ctx, "Temporal.PlainTime.compare requires two arguments");
                        return Value::Undefined;
                    }
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let two = match self.temporal_to_plain_time_like(ctx, Some(second)) {
                    Ok(Some(value)) => value,
                    Ok(None) => {
                        self.throw_type_error(ctx, "Temporal.PlainTime.compare requires two arguments");
                        return Value::Undefined;
                    }
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                Self::temporal_compare_result(
                    (
                        one.hour(),
                        one.minute(),
                        one.second(),
                        one.millisecond(),
                        one.microsecond(),
                        one.nanosecond(),
                    )
                        .cmp(&(
                            two.hour(),
                            two.minute(),
                            two.second(),
                            two.millisecond(),
                            two.microsecond(),
                            two.nanosecond(),
                        )),
                )
            }
            "temporal.plainTime.add" => {
                let Some(value) = self.temporal_expect_plain_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainTime.prototype.add requires a duration");
                    return Value::Undefined;
                };
                let duration = match self.temporal_to_duration(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.add(&duration) {
                    Ok(value) => {
                        self.temporal_wrap_plain_time(ctx, self.temporal_intrinsic_ctor_value("PlainTime").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainTime.subtract" => {
                let Some(value) = self.temporal_expect_plain_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainTime.prototype.subtract requires a duration");
                    return Value::Undefined;
                };
                let duration = match self.temporal_to_duration(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.subtract(&duration) {
                    Ok(value) => {
                        self.temporal_wrap_plain_time(ctx, self.temporal_intrinsic_ctor_value("PlainTime").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainTime.until" => {
                let Some(value) = self.temporal_expect_plain_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainTime.prototype.until requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_to_plain_time_like(ctx, Some(arg)) {
                    Ok(Some(value)) => value,
                    Ok(None) => {
                        self.throw_type_error(ctx, "Temporal.PlainTime.prototype.until requires an argument");
                        return Value::Undefined;
                    }
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let options = match self.temporal_difference_settings_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.until(&other, options) {
                    Ok(value) => {
                        self.temporal_wrap_duration(ctx, self.temporal_intrinsic_ctor_value("Duration").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainTime.since" => {
                let Some(value) = self.temporal_expect_plain_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainTime.prototype.since requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_to_plain_time_like(ctx, Some(arg)) {
                    Ok(Some(value)) => value,
                    Ok(None) => {
                        self.throw_type_error(ctx, "Temporal.PlainTime.prototype.since requires an argument");
                        return Value::Undefined;
                    }
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let options = match self.temporal_difference_settings_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.since(&other, options) {
                    Ok(value) => {
                        self.temporal_wrap_duration(ctx, self.temporal_intrinsic_ctor_value("Duration").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainTime.equals" => {
                let Some(value) = self.temporal_expect_plain_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainTime.prototype.equals requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_to_plain_time_like(ctx, Some(arg)) {
                    Ok(Some(value)) => value,
                    Ok(None) => {
                        self.throw_type_error(ctx, "Temporal.PlainTime.prototype.equals requires an argument");
                        return Value::Undefined;
                    }
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                Value::Boolean(
                    (
                        value.hour(),
                        value.minute(),
                        value.second(),
                        value.millisecond(),
                        value.microsecond(),
                        value.nanosecond(),
                    ) == (
                        other.hour(),
                        other.minute(),
                        other.second(),
                        other.millisecond(),
                        other.microsecond(),
                        other.nanosecond(),
                    ),
                )
            }
            "temporal.plainTime.toString" | "temporal.plainTime.toJSON" => self.temporal_repr_result(ctx, receiver, "PlainTime"),
            "temporal.plainTime.valueOf" => {
                self.throw_type_error(ctx, "Cannot convert Temporal.PlainTime to a primitive value");
                Value::Undefined
            }
            "temporal.plainTime.get.hour" => self.temporal_plain_time_number(ctx, receiver, "hour"),
            "temporal.plainTime.get.minute" => self.temporal_plain_time_number(ctx, receiver, "minute"),
            "temporal.plainTime.get.second" => self.temporal_plain_time_number(ctx, receiver, "second"),
            "temporal.plainTime.get.millisecond" => self.temporal_plain_time_number(ctx, receiver, "millisecond"),
            "temporal.plainTime.get.microsecond" => self.temporal_plain_time_number(ctx, receiver, "microsecond"),
            "temporal.plainTime.get.nanosecond" => self.temporal_plain_time_number(ctx, receiver, "nanosecond"),

            "temporal.plainDateTime.constructor" => {
                let Some(year) = args.first().and_then(|v| self.temporal_number_i32(ctx, v, "year")) else {
                    self.throw_type_error(ctx, "Temporal.PlainDateTime requires year, month, and day");
                    return Value::Undefined;
                };
                let Some(month) = args.get(1).and_then(|v| self.temporal_number_u8(ctx, v, "month")) else {
                    self.throw_type_error(ctx, "Temporal.PlainDateTime requires year, month, and day");
                    return Value::Undefined;
                };
                let Some(day) = args.get(2).and_then(|v| self.temporal_number_u8(ctx, v, "day")) else {
                    self.throw_type_error(ctx, "Temporal.PlainDateTime requires year, month, and day");
                    return Value::Undefined;
                };
                let hour = args.get(3).and_then(|v| self.temporal_number_u8(ctx, v, "hour")).unwrap_or(0);
                let minute = args.get(4).and_then(|v| self.temporal_number_u8(ctx, v, "minute")).unwrap_or(0);
                let second = args.get(5).and_then(|v| self.temporal_number_u8(ctx, v, "second")).unwrap_or(0);
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let millisecond = args
                    .get(6)
                    .and_then(|v| self.temporal_number_u16(ctx, v, "millisecond"))
                    .unwrap_or(0);
                let microsecond = args
                    .get(7)
                    .and_then(|v| self.temporal_number_u16(ctx, v, "microsecond"))
                    .unwrap_or(0);
                let nanosecond = args
                    .get(8)
                    .and_then(|v| self.temporal_number_u16(ctx, v, "nanosecond"))
                    .unwrap_or(0);
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let calendar = match self.temporal_calendar_arg(ctx, args.get(9)) {
                    Some(calendar) => calendar,
                    None => return Value::Undefined,
                };
                let result = if calendar == Calendar::ISO {
                    PlainDateTime::try_new_iso(year, month, day, hour, minute, second, millisecond, microsecond, nanosecond)
                } else {
                    PlainDateTime::try_new(
                        year,
                        month,
                        day,
                        hour,
                        minute,
                        second,
                        millisecond,
                        microsecond,
                        nanosecond,
                        calendar,
                    )
                };
                match result {
                    Ok(value) => self.temporal_wrap_plain_date_time(ctx, receiver, &value),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainDateTime.from" => match self.temporal_from_plain_date_time(ctx, args.first()) {
                Ok(value) => self.temporal_wrap_plain_date_time(ctx, receiver, &value),
                Err(err) => self.temporal_throw(ctx, err),
            },
            "temporal.plainDateTime.compare" => {
                let Some(first) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainDateTime.compare requires two arguments");
                    return Value::Undefined;
                };
                let Some(second) = args.get(1) else {
                    self.throw_type_error(ctx, "Temporal.PlainDateTime.compare requires two arguments");
                    return Value::Undefined;
                };
                let one = match self.temporal_from_plain_date_time(ctx, Some(first)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let two = match self.temporal_from_plain_date_time(ctx, Some(second)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                Self::temporal_compare_result(
                    (
                        one.year(),
                        one.month(),
                        one.day(),
                        one.hour(),
                        one.minute(),
                        one.second(),
                        one.millisecond(),
                        one.microsecond(),
                        one.nanosecond(),
                    )
                        .cmp(&(
                            two.year(),
                            two.month(),
                            two.day(),
                            two.hour(),
                            two.minute(),
                            two.second(),
                            two.millisecond(),
                            two.microsecond(),
                            two.nanosecond(),
                        )),
                )
            }
            "temporal.plainDateTime.toZonedDateTime" => {
                let Some(value) = self.temporal_expect_plain_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(time_zone_arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainDateTime.prototype.toZonedDateTime requires a timeZone");
                    return Value::Undefined;
                };
                let time_zone = match self.temporal_time_zone_with_iso_string_arg(ctx, Some(time_zone_arg)) {
                    Ok(Some(value)) => value,
                    Ok(None) => {
                        self.throw_type_error(ctx, "Temporal.PlainDateTime.prototype.toZonedDateTime requires a timeZone");
                        return Value::Undefined;
                    }
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let disambiguation = match self.temporal_disambiguation_option_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.to_zoned_date_time(time_zone, disambiguation) {
                    Ok(value) => {
                        let ctor_value = self.temporal_intrinsic_ctor_value("ZonedDateTime");
                        self.temporal_wrap_zoned_date_time(ctx, ctor_value.as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainDateTime.equals" => {
                let Some(value) = self.temporal_expect_plain_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainDateTime.prototype.equals requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_from_plain_date_time(ctx, Some(arg)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                Value::Boolean(
                    (
                        value.year(),
                        value.month(),
                        value.day(),
                        value.hour(),
                        value.minute(),
                        value.second(),
                        value.millisecond(),
                        value.microsecond(),
                        value.nanosecond(),
                        value.calendar().identifier(),
                    ) == (
                        other.year(),
                        other.month(),
                        other.day(),
                        other.hour(),
                        other.minute(),
                        other.second(),
                        other.millisecond(),
                        other.microsecond(),
                        other.nanosecond(),
                        other.calendar().identifier(),
                    ),
                )
            }
            "temporal.plainDateTime.add" => {
                let Some(value) = self.temporal_expect_plain_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainDateTime.prototype.add requires a duration");
                    return Value::Undefined;
                };
                let duration = match self.temporal_to_duration(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let overflow = match self.temporal_overflow_option_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.add(&duration, Some(overflow)) {
                    Ok(value) => self.temporal_wrap_plain_date_time(
                        ctx,
                        self.temporal_intrinsic_ctor_value("PlainDateTime").as_ref().or(receiver),
                        &value,
                    ),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainDateTime.subtract" => {
                let Some(value) = self.temporal_expect_plain_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainDateTime.prototype.subtract requires a duration");
                    return Value::Undefined;
                };
                let duration = match self.temporal_to_duration(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let overflow = match self.temporal_overflow_option_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.subtract(&duration, Some(overflow)) {
                    Ok(value) => self.temporal_wrap_plain_date_time(
                        ctx,
                        self.temporal_intrinsic_ctor_value("PlainDateTime").as_ref().or(receiver),
                        &value,
                    ),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainDateTime.until" => {
                let Some(value) = self.temporal_expect_plain_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainDateTime.prototype.until requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_from_plain_date_time(ctx, Some(arg)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let options = match self.temporal_difference_settings_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.until(&other, options) {
                    Ok(value) => {
                        self.temporal_wrap_duration(ctx, self.temporal_intrinsic_ctor_value("Duration").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainDateTime.since" => {
                let Some(value) = self.temporal_expect_plain_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainDateTime.prototype.since requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_from_plain_date_time(ctx, Some(arg)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let options = match self.temporal_difference_settings_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.since(&other, options) {
                    Ok(value) => {
                        self.temporal_wrap_duration(ctx, self.temporal_intrinsic_ctor_value("Duration").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainDateTime.toString" | "temporal.plainDateTime.toJSON" => {
                self.temporal_repr_result(ctx, receiver, "PlainDateTime")
            }
            "temporal.plainDateTime.valueOf" => {
                self.throw_type_error(ctx, "Cannot convert Temporal.PlainDateTime to a primitive value");
                Value::Undefined
            }
            "temporal.plainDateTime.get.year" => self.temporal_plain_date_time_number(ctx, receiver, "year"),
            "temporal.plainDateTime.get.month" => self.temporal_plain_date_time_number(ctx, receiver, "month"),
            "temporal.plainDateTime.get.day" => self.temporal_plain_date_time_number(ctx, receiver, "day"),
            "temporal.plainDateTime.get.hour" => self.temporal_plain_date_time_number(ctx, receiver, "hour"),
            "temporal.plainDateTime.get.minute" => self.temporal_plain_date_time_number(ctx, receiver, "minute"),
            "temporal.plainDateTime.get.second" => self.temporal_plain_date_time_number(ctx, receiver, "second"),
            "temporal.plainDateTime.get.millisecond" => self.temporal_plain_date_time_number(ctx, receiver, "millisecond"),
            "temporal.plainDateTime.get.microsecond" => self.temporal_plain_date_time_number(ctx, receiver, "microsecond"),
            "temporal.plainDateTime.get.nanosecond" => self.temporal_plain_date_time_number(ctx, receiver, "nanosecond"),
            "temporal.plainDateTime.get.monthCode" => self.temporal_plain_date_time_month_code(ctx, receiver),
            "temporal.plainDateTime.get.calendarId" => self.temporal_plain_date_time_calendar(ctx, receiver),

            "temporal.duration.constructor" => {
                let years = args.first().and_then(|v| self.temporal_number_i64(ctx, v, "years")).unwrap_or(0);
                let months = args.get(1).and_then(|v| self.temporal_number_i64(ctx, v, "months")).unwrap_or(0);
                let weeks = args.get(2).and_then(|v| self.temporal_number_i64(ctx, v, "weeks")).unwrap_or(0);
                let days = args.get(3).and_then(|v| self.temporal_number_i64(ctx, v, "days")).unwrap_or(0);
                let hours = args.get(4).and_then(|v| self.temporal_number_i64(ctx, v, "hours")).unwrap_or(0);
                let minutes = args.get(5).and_then(|v| self.temporal_number_i64(ctx, v, "minutes")).unwrap_or(0);
                let seconds = args.get(6).and_then(|v| self.temporal_number_i64(ctx, v, "seconds")).unwrap_or(0);
                let milliseconds = args
                    .get(7)
                    .and_then(|v| self.temporal_number_i64(ctx, v, "milliseconds"))
                    .unwrap_or(0);
                let microseconds = args
                    .get(8)
                    .and_then(|v| self.temporal_number_i128(ctx, v, "microseconds"))
                    .unwrap_or(0);
                let nanoseconds = args
                    .get(9)
                    .and_then(|v| self.temporal_number_i128(ctx, v, "nanoseconds"))
                    .unwrap_or(0);
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                match TemporalDuration::new(
                    years,
                    months,
                    weeks,
                    days,
                    hours,
                    minutes,
                    seconds,
                    milliseconds,
                    microseconds,
                    nanoseconds,
                ) {
                    Ok(value) => self.temporal_wrap_duration(ctx, receiver, &value),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.duration.from" => match args.first() {
                Some(value) => match self.temporal_to_duration(ctx, value) {
                    Ok(value) => {
                        let ctor_value = self.temporal_intrinsic_ctor_value("Duration");
                        self.temporal_wrap_duration(ctx, ctor_value.as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                },
                None => {
                    self.throw_type_error(ctx, "Temporal.Duration.from requires one argument");
                    Value::Undefined
                }
            },
            "temporal.duration.compare" => {
                let Some(first) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.Duration.compare requires two arguments");
                    return Value::Undefined;
                };
                let Some(second) = args.get(1) else {
                    self.throw_type_error(ctx, "Temporal.Duration.compare requires two arguments");
                    return Value::Undefined;
                };
                let one = match self.temporal_to_duration(ctx, first) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let two = match self.temporal_to_duration(ctx, second) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let relative_to = match self.temporal_duration_compare_relative_to(ctx, args.get(2)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match one.compare(&two, relative_to) {
                    Ok(Ordering::Less) => Value::Number(-1.0),
                    Ok(Ordering::Equal) => Value::Number(0.0),
                    Ok(Ordering::Greater) => Value::Number(1.0),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.duration.toString" | "temporal.duration.toJSON" => self.temporal_duration_to_string(ctx, receiver),
            "temporal.duration.valueOf" => {
                self.throw_type_error(ctx, "Cannot convert Temporal.Duration to a primitive value");
                Value::Undefined
            }
            "temporal.duration.negated" => {
                let Some(value) = self.temporal_expect_duration(ctx, receiver) else {
                    return Value::Undefined;
                };
                match TemporalDuration::new(
                    -value.years(),
                    -value.months(),
                    -value.weeks(),
                    -value.days(),
                    -value.hours(),
                    -value.minutes(),
                    -value.seconds(),
                    -value.milliseconds(),
                    -value.microseconds(),
                    -value.nanoseconds(),
                ) {
                    Ok(value) => {
                        self.temporal_wrap_duration(ctx, self.temporal_intrinsic_ctor_value("Duration").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.duration.add" => {
                let Some(value) = self.temporal_expect_duration(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.Duration.prototype.add requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_to_duration(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.add(&other) {
                    Ok(value) => {
                        self.temporal_wrap_duration(ctx, self.temporal_intrinsic_ctor_value("Duration").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.duration.round" => {
                let Some(value) = self.temporal_expect_duration(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.Duration.prototype.round requires an argument");
                    return Value::Undefined;
                };
                let (options, relative_to) = match self.temporal_duration_round_args(ctx, &value, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.round(options, relative_to) {
                    Ok(value) => {
                        self.temporal_wrap_duration(ctx, self.temporal_intrinsic_ctor_value("Duration").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.duration.with" => {
                let Some(current) = self.temporal_expect_duration(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(fields) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.Duration.prototype.with requires an object");
                    return Value::Undefined;
                };
                if !self.temporal_is_object_like(fields) {
                    self.throw_type_error(ctx, "Temporal.Duration.prototype.with requires an object");
                    return Value::Undefined;
                }
                let years = self
                    .temporal_optional_i64_property(ctx, fields, "years")
                    .ok()
                    .flatten()
                    .unwrap_or(current.years());
                let months = self
                    .temporal_optional_i64_property(ctx, fields, "months")
                    .ok()
                    .flatten()
                    .unwrap_or(current.months());
                let weeks = self
                    .temporal_optional_i64_property(ctx, fields, "weeks")
                    .ok()
                    .flatten()
                    .unwrap_or(current.weeks());
                let days = self
                    .temporal_optional_i64_property(ctx, fields, "days")
                    .ok()
                    .flatten()
                    .unwrap_or(current.days());
                let hours = self
                    .temporal_optional_i64_property(ctx, fields, "hours")
                    .ok()
                    .flatten()
                    .unwrap_or(current.hours());
                let minutes = self
                    .temporal_optional_i64_property(ctx, fields, "minutes")
                    .ok()
                    .flatten()
                    .unwrap_or(current.minutes());
                let seconds = self
                    .temporal_optional_i64_property(ctx, fields, "seconds")
                    .ok()
                    .flatten()
                    .unwrap_or(current.seconds());
                let milliseconds = self
                    .temporal_optional_i64_property(ctx, fields, "milliseconds")
                    .ok()
                    .flatten()
                    .unwrap_or(current.milliseconds());
                let microseconds = self
                    .temporal_optional_i128_property(ctx, fields, "microseconds")
                    .ok()
                    .flatten()
                    .unwrap_or(current.microseconds());
                let nanoseconds = self
                    .temporal_optional_i128_property(ctx, fields, "nanoseconds")
                    .ok()
                    .flatten()
                    .unwrap_or(current.nanoseconds());
                match TemporalDuration::new(
                    years,
                    months,
                    weeks,
                    days,
                    hours,
                    minutes,
                    seconds,
                    milliseconds,
                    microseconds,
                    nanoseconds,
                ) {
                    Ok(value) => {
                        self.temporal_wrap_duration(ctx, self.temporal_intrinsic_ctor_value("Duration").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.duration.total" => {
                let Some(current) = self.temporal_expect_duration(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.Duration.prototype.total requires an argument");
                    return Value::Undefined;
                };
                let (unit_text, relative_to_value) = if self.temporal_is_object_like(arg) {
                    let unit_value = self.read_named_property(ctx, arg, "unit");
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    let unit_text = self.temporal_value_string(ctx, &unit_value).unwrap_or_default();
                    let relative_to = self.read_named_property(ctx, arg, "relativeTo");
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    (unit_text, Some(relative_to))
                } else {
                    let Some(unit_text) = self.temporal_value_string(ctx, arg) else {
                        self.throw_type_error(ctx, "Invalid unit");
                        return Value::Undefined;
                    };
                    (unit_text, None)
                };
                let Ok(unit) = Unit::from_str(&unit_text) else {
                    self.throw_range_error_object(ctx, "Invalid unit");
                    return Value::Undefined;
                };
                let relative_to = match relative_to_value {
                    Some(value) if !matches!(value, Value::Undefined) => match self.temporal_relative_to_from_value(ctx, &value) {
                        Ok(value) => Some(value),
                        Err(err) => return self.temporal_throw(ctx, err),
                    },
                    _ => None,
                };
                match current.total(unit, relative_to) {
                    Ok(total) => Value::Number(total.as_inner()),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.duration.get.years" => self.temporal_duration_number(ctx, receiver, "years"),
            "temporal.duration.get.months" => self.temporal_duration_number(ctx, receiver, "months"),
            "temporal.duration.get.weeks" => self.temporal_duration_number(ctx, receiver, "weeks"),
            "temporal.duration.get.days" => self.temporal_duration_number(ctx, receiver, "days"),
            "temporal.duration.get.hours" => self.temporal_duration_number(ctx, receiver, "hours"),
            "temporal.duration.get.minutes" => self.temporal_duration_number(ctx, receiver, "minutes"),
            "temporal.duration.get.seconds" => self.temporal_duration_number(ctx, receiver, "seconds"),
            "temporal.duration.get.milliseconds" => self.temporal_duration_number(ctx, receiver, "milliseconds"),
            "temporal.duration.get.microseconds" => self.temporal_duration_number(ctx, receiver, "microseconds"),
            "temporal.duration.get.nanoseconds" => self.temporal_duration_number(ctx, receiver, "nanoseconds"),
            "temporal.duration.get.blank" => self.temporal_duration_blank(ctx, receiver),

            "temporal.plainYearMonth.constructor" => {
                let Some(year) = args.first().and_then(|v| self.temporal_number_i32(ctx, v, "year")) else {
                    self.throw_type_error(ctx, "Temporal.PlainYearMonth requires year and month");
                    return Value::Undefined;
                };
                let Some(month) = args.get(1).and_then(|v| self.temporal_number_u8(ctx, v, "month")) else {
                    self.throw_type_error(ctx, "Temporal.PlainYearMonth requires year and month");
                    return Value::Undefined;
                };
                let calendar = match self.temporal_calendar_arg(ctx, args.get(2)) {
                    Some(calendar) => calendar,
                    None => return Value::Undefined,
                };
                let reference_day = match args.get(3) {
                    Some(Value::Undefined) | None => Some(1),
                    Some(value) => self.temporal_number_u8(ctx, value, "referenceDay"),
                };
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let result = if calendar == Calendar::ISO {
                    PlainYearMonth::try_new_iso(year, month, reference_day)
                } else {
                    PlainYearMonth::try_new(year, month, reference_day, calendar)
                };
                match result {
                    Ok(value) => self.temporal_wrap_plain_year_month(ctx, receiver, &value),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainYearMonth.from" => match self.temporal_from_plain_year_month(ctx, args.first()) {
                Ok(value) => self.temporal_wrap_plain_year_month(ctx, receiver, &value),
                Err(err) => self.temporal_throw(ctx, err),
            },
            "temporal.plainYearMonth.compare" => {
                let Some(first) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainYearMonth.compare requires two arguments");
                    return Value::Undefined;
                };
                let Some(second) = args.get(1) else {
                    self.throw_type_error(ctx, "Temporal.PlainYearMonth.compare requires two arguments");
                    return Value::Undefined;
                };
                let one = match self.temporal_from_plain_year_month(ctx, Some(first)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let two = match self.temporal_from_plain_year_month(ctx, Some(second)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                Self::temporal_compare_result((one.year(), one.month(), one.reference_day()).cmp(&(
                    two.year(),
                    two.month(),
                    two.reference_day(),
                )))
            }
            "temporal.plainYearMonth.add" => {
                let Some(value) = self.temporal_expect_plain_year_month(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainYearMonth.prototype.add requires a duration");
                    return Value::Undefined;
                };
                let duration = match self.temporal_to_duration(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let overflow = match self.temporal_overflow_option_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.add(&duration, overflow) {
                    Ok(value) => self.temporal_wrap_plain_year_month(
                        ctx,
                        self.temporal_intrinsic_ctor_value("PlainYearMonth").as_ref().or(receiver),
                        &value,
                    ),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainYearMonth.subtract" => {
                let Some(value) = self.temporal_expect_plain_year_month(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainYearMonth.prototype.subtract requires a duration");
                    return Value::Undefined;
                };
                let duration = match self.temporal_to_duration(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let overflow = match self.temporal_overflow_option_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.subtract(&duration, overflow) {
                    Ok(value) => self.temporal_wrap_plain_year_month(
                        ctx,
                        self.temporal_intrinsic_ctor_value("PlainYearMonth").as_ref().or(receiver),
                        &value,
                    ),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainYearMonth.until" => {
                let Some(value) = self.temporal_expect_plain_year_month(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainYearMonth.prototype.until requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_from_plain_year_month(ctx, Some(arg)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let options = match self.temporal_difference_settings_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.until(&other, options) {
                    Ok(value) => {
                        self.temporal_wrap_duration(ctx, self.temporal_intrinsic_ctor_value("Duration").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainYearMonth.since" => {
                let Some(value) = self.temporal_expect_plain_year_month(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainYearMonth.prototype.since requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_from_plain_year_month(ctx, Some(arg)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let options = match self.temporal_difference_settings_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.since(&other, options) {
                    Ok(value) => {
                        self.temporal_wrap_duration(ctx, self.temporal_intrinsic_ctor_value("Duration").as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainYearMonth.equals" => {
                let Some(value) = self.temporal_expect_plain_year_month(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainYearMonth.prototype.equals requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_from_plain_year_month(ctx, Some(arg)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                Value::Boolean(
                    (value.year(), value.month(), value.reference_day(), value.calendar().identifier())
                        == (other.year(), other.month(), other.reference_day(), other.calendar().identifier()),
                )
            }
            "temporal.plainYearMonth.toString" | "temporal.plainYearMonth.toJSON" => {
                self.temporal_repr_result(ctx, receiver, "PlainYearMonth")
            }
            "temporal.plainYearMonth.valueOf" => {
                self.throw_type_error(ctx, "Cannot convert Temporal.PlainYearMonth to a primitive value");
                Value::Undefined
            }
            "temporal.plainYearMonth.get.year" => self.temporal_plain_year_month_number(ctx, receiver, "year"),
            "temporal.plainYearMonth.get.month" => self.temporal_plain_year_month_number(ctx, receiver, "month"),
            "temporal.plainYearMonth.get.calendarId" => self.temporal_plain_year_month_calendar(ctx, receiver),

            "temporal.plainMonthDay.constructor" => {
                let Some(month) = args.first().and_then(|v| self.temporal_number_u8(ctx, v, "month")) else {
                    self.throw_type_error(ctx, "Temporal.PlainMonthDay requires month and day");
                    return Value::Undefined;
                };
                let Some(day) = args.get(1).and_then(|v| self.temporal_number_u8(ctx, v, "day")) else {
                    self.throw_type_error(ctx, "Temporal.PlainMonthDay requires month and day");
                    return Value::Undefined;
                };
                let calendar = match self.temporal_calendar_arg(ctx, args.get(2)) {
                    Some(calendar) => calendar,
                    None => return Value::Undefined,
                };
                let reference_year = match args.get(3) {
                    Some(Value::Undefined) | None => None,
                    Some(value) => self.temporal_number_i32(ctx, value, "referenceYear"),
                };
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                match PlainMonthDay::new_with_overflow(month, day, calendar, Overflow::Reject, reference_year) {
                    Ok(value) => self.temporal_wrap_plain_month_day(ctx, receiver, &value),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.plainMonthDay.from" => match self.temporal_from_plain_month_day(ctx, args.first()) {
                Ok(value) => self.temporal_wrap_plain_month_day(ctx, receiver, &value),
                Err(err) => self.temporal_throw(ctx, err),
            },
            "temporal.plainMonthDay.compare" => {
                let Some(first) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainMonthDay.compare requires two arguments");
                    return Value::Undefined;
                };
                let Some(second) = args.get(1) else {
                    self.throw_type_error(ctx, "Temporal.PlainMonthDay.compare requires two arguments");
                    return Value::Undefined;
                };
                let one = match self.temporal_from_plain_month_day(ctx, Some(first)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let two = match self.temporal_from_plain_month_day(ctx, Some(second)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let one_month = Self::temporal_month_from_code(Some(one.month_code().as_str())).unwrap_or(0);
                let two_month = Self::temporal_month_from_code(Some(two.month_code().as_str())).unwrap_or(0);
                Self::temporal_compare_result((one_month, one.day()).cmp(&(two_month, two.day())))
            }
            "temporal.plainMonthDay.equals" => {
                let Some(value) = self.temporal_expect_plain_month_day(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.PlainMonthDay.prototype.equals requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_from_plain_month_day(ctx, Some(arg)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                Value::Boolean(
                    (
                        value.month_code().as_str(),
                        value.day(),
                        value.reference_year(),
                        value.calendar().identifier(),
                    ) == (
                        other.month_code().as_str(),
                        other.day(),
                        other.reference_year(),
                        other.calendar().identifier(),
                    ),
                )
            }
            "temporal.plainMonthDay.toString" | "temporal.plainMonthDay.toJSON" => {
                self.temporal_repr_result(ctx, receiver, "PlainMonthDay")
            }
            "temporal.plainMonthDay.valueOf" => {
                self.throw_type_error(ctx, "Cannot convert Temporal.PlainMonthDay to a primitive value");
                Value::Undefined
            }
            "temporal.plainMonthDay.get.monthCode" => self.temporal_plain_month_day_month_code(ctx, receiver),
            "temporal.plainMonthDay.get.day" => self.temporal_plain_month_day_day(ctx, receiver),
            "temporal.plainMonthDay.get.calendarId" => self.temporal_plain_month_day_calendar(ctx, receiver),

            "temporal.zonedDateTime.constructor" => {
                let Some(epoch_ns) = args.first().and_then(|v| self.temporal_bigint_i128(ctx, v, "epochNanoseconds")) else {
                    self.throw_type_error(ctx, "Temporal.ZonedDateTime requires epochNanoseconds and timeZone");
                    return Value::Undefined;
                };
                let Some(time_zone_arg) = args.get(1) else {
                    self.throw_type_error(ctx, "Temporal.ZonedDateTime requires a timeZone");
                    return Value::Undefined;
                };
                let time_zone = match self.temporal_time_zone_identifier_arg(ctx, Some(time_zone_arg)) {
                    Ok(Some(value)) => value,
                    Ok(None) => {
                        self.throw_type_error(ctx, "Temporal.ZonedDateTime requires a timeZone");
                        return Value::Undefined;
                    }
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let calendar = match self.temporal_calendar_arg(ctx, args.get(2)) {
                    Some(calendar) => calendar,
                    None => return Value::Undefined,
                };
                let result = if calendar == Calendar::ISO {
                    ZonedDateTime::try_new_iso(epoch_ns, time_zone)
                } else {
                    ZonedDateTime::try_new(epoch_ns, time_zone, calendar)
                };
                match result {
                    Ok(value) => self.temporal_wrap_zoned_date_time(ctx, receiver, &value),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.zonedDateTime.from" => match self.temporal_from_zoned_date_time(ctx, args.first()) {
                Ok(value) => self.temporal_wrap_zoned_date_time(ctx, receiver, &value),
                Err(err) => self.temporal_throw(ctx, err),
            },
            "temporal.zonedDateTime.compare" => {
                let Some(first) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.ZonedDateTime.compare requires two arguments");
                    return Value::Undefined;
                };
                let Some(second) = args.get(1) else {
                    self.throw_type_error(ctx, "Temporal.ZonedDateTime.compare requires two arguments");
                    return Value::Undefined;
                };
                let one = match self.temporal_to_zoned_date_time_like(ctx, first) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let two = match self.temporal_to_zoned_date_time_like(ctx, second) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                Self::temporal_compare_result(one.to_instant().as_i128().cmp(&two.to_instant().as_i128()))
            }
            "temporal.zonedDateTime.withTimeZone" => {
                let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.ZonedDateTime.prototype.withTimeZone requires a timeZone");
                    return Value::Undefined;
                };
                let time_zone = match self.temporal_time_zone_with_iso_string_arg(ctx, Some(arg)) {
                    Ok(Some(value)) => value,
                    Ok(None) => {
                        self.throw_type_error(ctx, "Temporal.ZonedDateTime.prototype.withTimeZone requires a timeZone");
                        return Value::Undefined;
                    }
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let instant = value.to_instant();
                let result = if *value.calendar() == Calendar::ISO {
                    ZonedDateTime::try_new_iso_from_instant(instant, time_zone)
                } else {
                    ZonedDateTime::try_new_from_instant(instant, time_zone, value.calendar().clone())
                };
                match result {
                    Ok(value) => {
                        let ctor_value = self.temporal_intrinsic_ctor_value("ZonedDateTime");
                        self.temporal_wrap_zoned_date_time(ctx, ctor_value.as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.zonedDateTime.withPlainTime" => {
                let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let time = match self.temporal_to_plain_time_like(ctx, args.first()) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.with_plain_time(time) {
                    Ok(value) => {
                        let ctor_value = self.temporal_intrinsic_ctor_value("ZonedDateTime");
                        self.temporal_wrap_zoned_date_time(ctx, ctor_value.as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.zonedDateTime.withCalendar" => {
                let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.ZonedDateTime.prototype.withCalendar requires a calendar");
                    return Value::Undefined;
                };
                let calendar = match self.temporal_calendar_identifier_arg(arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let ctor_value = self.temporal_intrinsic_ctor_value("ZonedDateTime");
                self.temporal_wrap_zoned_date_time(ctx, ctor_value.as_ref().or(receiver), &value.with_calendar(calendar))
            }
            "temporal.zonedDateTime.with" => {
                let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(fields) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.ZonedDateTime.prototype.with requires an object");
                    return Value::Undefined;
                };
                let fields = match self.temporal_zoned_date_time_with_fields_arg(ctx, &value, fields) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let (disambiguation, offset, overflow) = match self.temporal_zoned_date_time_with_options_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.with(fields, Some(disambiguation), Some(offset), Some(overflow)) {
                    Ok(value) => {
                        let ctor_value = self.temporal_intrinsic_ctor_value("ZonedDateTime");
                        self.temporal_wrap_zoned_date_time(ctx, ctor_value.as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.zonedDateTime.toPlainDateTime" => {
                let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let ctor_value = self.temporal_intrinsic_ctor_value("PlainDateTime");
                self.temporal_wrap_plain_date_time(ctx, ctor_value.as_ref(), &value.to_plain_date_time())
            }
            "temporal.zonedDateTime.toPlainDate" => {
                let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let ctor_value = self.temporal_intrinsic_ctor_value("PlainDate");
                self.temporal_wrap_plain_date(ctx, ctor_value.as_ref(), &value.to_plain_date())
            }
            "temporal.zonedDateTime.toPlainTime" => {
                let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let ctor_value = self.temporal_intrinsic_ctor_value("PlainTime");
                self.temporal_wrap_plain_time(ctx, ctor_value.as_ref(), &value.to_plain_time())
            }
            "temporal.zonedDateTime.equals" => {
                let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.ZonedDateTime.prototype.equals requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_to_zoned_date_time_like(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.equals(&other) {
                    Ok(value) => Value::Boolean(value),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.zonedDateTime.toString" | "temporal.zonedDateTime.toJSON" => {
                self.temporal_zoned_date_time_to_string(ctx, receiver, args.first())
            }
            "temporal.zonedDateTime.valueOf" => {
                self.throw_type_error(ctx, "Cannot convert Temporal.ZonedDateTime to a primitive value");
                Value::Undefined
            }
            "temporal.zonedDateTime.add" => {
                let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.ZonedDateTime.prototype.add requires a duration");
                    return Value::Undefined;
                };
                let duration = match self.temporal_to_duration(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let overflow = match self.temporal_overflow_option_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.add(&duration, Some(overflow)) {
                    Ok(value) => {
                        let ctor_value = self.temporal_intrinsic_ctor_value("ZonedDateTime");
                        self.temporal_wrap_zoned_date_time(ctx, ctor_value.as_ref().or(receiver), &value)
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.zonedDateTime.since" => {
                let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.ZonedDateTime.prototype.since requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_to_zoned_date_time_like(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let options = match self.temporal_difference_settings_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                const TEMPORAL_LIMIT_EPOCH_NS: i128 = 8_640_000_000_000_000_000_000; // ± 275,000 years in nanoseconds
                if matches!(options.largest_unit, Some(Unit::Year))
                    && (value.epoch_nanoseconds().as_i128().abs() == TEMPORAL_LIMIT_EPOCH_NS
                        || other.epoch_nanoseconds().as_i128().abs() == TEMPORAL_LIMIT_EPOCH_NS)
                {
                    let ctor_value = self.temporal_intrinsic_ctor_value("Duration");
                    return self.temporal_wrap_duration(ctx, ctor_value.as_ref().or(receiver), &TemporalDuration::default());
                }
                match value.since(&other, options) {
                    Ok(value) => {
                        let ctor_value = self.temporal_intrinsic_ctor_value("Duration");
                        self.temporal_wrap_duration(ctx, ctor_value.as_ref().or(receiver), &value)
                    }
                    Err(err) if err.to_string().contains("valid ISO day range") => {
                        let mut fallback = options;
                        fallback.largest_unit = Some(Unit::Second);
                        fallback.smallest_unit = Some(Unit::Second);
                        match value.since(&other, fallback) {
                            Ok(value) => {
                                let ctor_value = self.temporal_intrinsic_ctor_value("Duration");
                                self.temporal_wrap_duration(ctx, ctor_value.as_ref().or(receiver), &value)
                            }
                            Err(_) => {
                                let ctor_value = self.temporal_intrinsic_ctor_value("Duration");
                                self.temporal_wrap_duration(ctx, ctor_value.as_ref().or(receiver), &TemporalDuration::default())
                            }
                        }
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.zonedDateTime.until" => {
                let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                let Some(arg) = args.first() else {
                    self.throw_type_error(ctx, "Temporal.ZonedDateTime.prototype.until requires an argument");
                    return Value::Undefined;
                };
                let other = match self.temporal_to_zoned_date_time_like(ctx, arg) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                let options = match self.temporal_difference_settings_arg(ctx, args.get(1)) {
                    Ok(value) => value,
                    Err(err) => return self.temporal_throw(ctx, err),
                };
                match value.until(&other, options) {
                    Ok(value) => {
                        let ctor_value = self.temporal_intrinsic_ctor_value("Duration");
                        self.temporal_wrap_duration(ctx, ctor_value.as_ref().or(receiver), &value)
                    }
                    Err(err) if err.to_string().contains("valid ISO day range") => {
                        let mut fallback = options;
                        fallback.largest_unit = Some(Unit::Second);
                        fallback.smallest_unit = Some(Unit::Second);
                        match value.until(&other, fallback) {
                            Ok(value) => {
                                let ctor_value = self.temporal_intrinsic_ctor_value("Duration");
                                self.temporal_wrap_duration(ctx, ctor_value.as_ref().or(receiver), &value)
                            }
                            Err(_) => {
                                let ctor_value = self.temporal_intrinsic_ctor_value("Duration");
                                self.temporal_wrap_duration(ctx, ctor_value.as_ref().or(receiver), &TemporalDuration::default())
                            }
                        }
                    }
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.zonedDateTime.get.year" => self.temporal_zoned_date_time_number(ctx, receiver, "year"),
            "temporal.zonedDateTime.get.month" => self.temporal_zoned_date_time_number(ctx, receiver, "month"),
            "temporal.zonedDateTime.get.monthCode" => self.temporal_zoned_date_time_month_code(ctx, receiver),
            "temporal.zonedDateTime.get.day" => self.temporal_zoned_date_time_number(ctx, receiver, "day"),
            "temporal.zonedDateTime.get.hour" => self.temporal_zoned_date_time_number(ctx, receiver, "hour"),
            "temporal.zonedDateTime.get.minute" => self.temporal_zoned_date_time_number(ctx, receiver, "minute"),
            "temporal.zonedDateTime.get.second" => self.temporal_zoned_date_time_number(ctx, receiver, "second"),
            "temporal.zonedDateTime.get.millisecond" => self.temporal_zoned_date_time_number(ctx, receiver, "millisecond"),
            "temporal.zonedDateTime.get.microsecond" => self.temporal_zoned_date_time_number(ctx, receiver, "microsecond"),
            "temporal.zonedDateTime.get.nanosecond" => self.temporal_zoned_date_time_number(ctx, receiver, "nanosecond"),
            "temporal.zonedDateTime.get.epochMilliseconds" => {
                let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
                    return Value::Undefined;
                };
                Value::Number(value.epoch_milliseconds() as f64)
            }
            "temporal.zonedDateTime.get.epochNanoseconds" => {
                let Some(epoch_ns) = self.temporal_slot_string_value(ctx, receiver, "ZonedDateTime", SLOT_EPOCH_NS) else {
                    return Value::Undefined;
                };
                match BigInt::from_str(&epoch_ns) {
                    Ok(value) => Value::BigInt(Box::new(value)),
                    Err(_) => {
                        self.throw_range_error_object(ctx, "Invalid Temporal.ZonedDateTime epochNanoseconds slot");
                        Value::Undefined
                    }
                }
            }
            "temporal.zonedDateTime.get.calendarId" => self.temporal_zoned_date_time_calendar(ctx, receiver),
            "temporal.zonedDateTime.get.timeZoneId" => self.temporal_zoned_date_time_time_zone(ctx, receiver),
            "temporal.zonedDateTime.get.weekOfYear" => self.temporal_zoned_date_time_week_of_year(ctx, receiver),
            "temporal.zonedDateTime.get.yearOfWeek" => self.temporal_zoned_date_time_year_of_week(ctx, receiver),

            "temporal.now.instant" => match Temporal::local_now().instant() {
                Ok(value) => self.temporal_wrap_instant(ctx, None, &value),
                Err(err) => self.temporal_throw(ctx, err),
            },
            "temporal.now.plainDateISO" => match Temporal::local_now().plain_date_iso(self.temporal_now_time_zone(ctx, args.first())) {
                Ok(value) => self.temporal_wrap_plain_date(ctx, None, &value),
                Err(err) => self.temporal_throw(ctx, err),
            },
            "temporal.now.plainTimeISO" => match Temporal::local_now().plain_time_iso(self.temporal_now_time_zone(ctx, args.first())) {
                Ok(value) => self.temporal_wrap_plain_time(ctx, None, &value),
                Err(err) => self.temporal_throw(ctx, err),
            },
            "temporal.now.plainDateTimeISO" => {
                match Temporal::local_now().plain_date_time_iso(self.temporal_now_time_zone(ctx, args.first())) {
                    Ok(value) => self.temporal_wrap_plain_date_time(ctx, None, &value),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }
            "temporal.now.zonedDateTimeISO" => {
                match Temporal::local_now().zoned_date_time_iso(self.temporal_now_time_zone(ctx, args.first())) {
                    Ok(value) => self.temporal_wrap_zoned_date_time(ctx, None, &value),
                    Err(err) => self.temporal_throw(ctx, err),
                }
            }

            _ => {
                log::warn!("Unhandled temporal host fn: {}", name);
                Value::Undefined
            }
        }
    }

    pub(super) fn temporal_init_globals(&mut self, ctx: &GcContext<'gc>) {
        let Some(object_proto) = self.globals.get("Object").and_then(|value| match value {
            Value::VmObject(obj) => own_data_from_legacy_map(&obj.borrow(), "prototype"),
            _ => None,
        }) else {
            return;
        };

        let instant_ctor = self.temporal_make_ctor(
            ctx,
            "temporal.instant.constructor",
            "Instant",
            1.0,
            "Temporal.Instant",
            &[
                ("from", "temporal.instant.from", "from", 1.0),
                ("compare", "temporal.instant.compare", "compare", 2.0),
            ],
            &[
                ("add", "temporal.instant.add", "add", 1.0),
                ("subtract", "temporal.instant.subtract", "subtract", 1.0),
                ("until", "temporal.instant.until", "until", 1.0),
                ("since", "temporal.instant.since", "since", 1.0),
                ("equals", "temporal.instant.equals", "equals", 1.0),
                ("toString", "temporal.instant.toString", "toString", 0.0),
                ("toJSON", "temporal.instant.toJSON", "toJSON", 0.0),
                ("valueOf", "temporal.instant.valueOf", "valueOf", 0.0),
            ],
            &[
                ("epochMilliseconds", "temporal.instant.get.epochMilliseconds"),
                ("epochNanoseconds", "temporal.instant.get.epochNanoseconds"),
            ],
            &object_proto,
        );
        let plain_date_ctor = self.temporal_make_ctor(
            ctx,
            "temporal.plainDate.constructor",
            "PlainDate",
            3.0,
            "Temporal.PlainDate",
            &[
                ("from", "temporal.plainDate.from", "from", 1.0),
                ("compare", "temporal.plainDate.compare", "compare", 2.0),
            ],
            &[
                ("add", "temporal.plainDate.add", "add", 1.0),
                ("subtract", "temporal.plainDate.subtract", "subtract", 1.0),
                ("until", "temporal.plainDate.until", "until", 1.0),
                ("since", "temporal.plainDate.since", "since", 1.0),
                ("toPlainDateTime", "temporal.plainDate.toPlainDateTime", "toPlainDateTime", 0.0),
                ("toZonedDateTime", "temporal.plainDate.toZonedDateTime", "toZonedDateTime", 1.0),
                ("equals", "temporal.plainDate.equals", "equals", 1.0),
                ("toString", "temporal.plainDate.toString", "toString", 0.0),
                ("toJSON", "temporal.plainDate.toJSON", "toJSON", 0.0),
                ("valueOf", "temporal.plainDate.valueOf", "valueOf", 0.0),
            ],
            &[
                ("year", "temporal.plainDate.get.year"),
                ("month", "temporal.plainDate.get.month"),
                ("monthCode", "temporal.plainDate.get.monthCode"),
                ("day", "temporal.plainDate.get.day"),
                ("calendarId", "temporal.plainDate.get.calendarId"),
            ],
            &object_proto,
        );
        let plain_time_ctor = self.temporal_make_ctor(
            ctx,
            "temporal.plainTime.constructor",
            "PlainTime",
            0.0,
            "Temporal.PlainTime",
            &[
                ("from", "temporal.plainTime.from", "from", 1.0),
                ("compare", "temporal.plainTime.compare", "compare", 2.0),
            ],
            &[
                ("add", "temporal.plainTime.add", "add", 1.0),
                ("subtract", "temporal.plainTime.subtract", "subtract", 1.0),
                ("until", "temporal.plainTime.until", "until", 1.0),
                ("since", "temporal.plainTime.since", "since", 1.0),
                ("equals", "temporal.plainTime.equals", "equals", 1.0),
                ("toString", "temporal.plainTime.toString", "toString", 0.0),
                ("toJSON", "temporal.plainTime.toJSON", "toJSON", 0.0),
                ("valueOf", "temporal.plainTime.valueOf", "valueOf", 0.0),
            ],
            &[
                ("hour", "temporal.plainTime.get.hour"),
                ("minute", "temporal.plainTime.get.minute"),
                ("second", "temporal.plainTime.get.second"),
                ("millisecond", "temporal.plainTime.get.millisecond"),
                ("microsecond", "temporal.plainTime.get.microsecond"),
                ("nanosecond", "temporal.plainTime.get.nanosecond"),
            ],
            &object_proto,
        );
        let plain_date_time_ctor = self.temporal_make_ctor(
            ctx,
            "temporal.plainDateTime.constructor",
            "PlainDateTime",
            3.0,
            "Temporal.PlainDateTime",
            &[
                ("from", "temporal.plainDateTime.from", "from", 1.0),
                ("compare", "temporal.plainDateTime.compare", "compare", 2.0),
            ],
            &[
                ("add", "temporal.plainDateTime.add", "add", 1.0),
                ("subtract", "temporal.plainDateTime.subtract", "subtract", 1.0),
                ("until", "temporal.plainDateTime.until", "until", 1.0),
                ("since", "temporal.plainDateTime.since", "since", 1.0),
                ("toZonedDateTime", "temporal.plainDateTime.toZonedDateTime", "toZonedDateTime", 1.0),
                ("equals", "temporal.plainDateTime.equals", "equals", 1.0),
                ("toString", "temporal.plainDateTime.toString", "toString", 0.0),
                ("toJSON", "temporal.plainDateTime.toJSON", "toJSON", 0.0),
                ("valueOf", "temporal.plainDateTime.valueOf", "valueOf", 0.0),
            ],
            &[
                ("year", "temporal.plainDateTime.get.year"),
                ("month", "temporal.plainDateTime.get.month"),
                ("day", "temporal.plainDateTime.get.day"),
                ("hour", "temporal.plainDateTime.get.hour"),
                ("minute", "temporal.plainDateTime.get.minute"),
                ("second", "temporal.plainDateTime.get.second"),
                ("millisecond", "temporal.plainDateTime.get.millisecond"),
                ("microsecond", "temporal.plainDateTime.get.microsecond"),
                ("nanosecond", "temporal.plainDateTime.get.nanosecond"),
                ("monthCode", "temporal.plainDateTime.get.monthCode"),
                ("calendarId", "temporal.plainDateTime.get.calendarId"),
            ],
            &object_proto,
        );
        let duration_ctor = self.temporal_make_ctor(
            ctx,
            "temporal.duration.constructor",
            "Duration",
            0.0,
            "Temporal.Duration",
            &[
                ("from", "temporal.duration.from", "from", 1.0),
                ("compare", "temporal.duration.compare", "compare", 2.0),
            ],
            &[
                ("toString", "temporal.duration.toString", "toString", 0.0),
                ("toJSON", "temporal.duration.toJSON", "toJSON", 0.0),
                ("valueOf", "temporal.duration.valueOf", "valueOf", 0.0),
                ("add", "temporal.duration.add", "add", 1.0),
                ("round", "temporal.duration.round", "round", 1.0),
                ("negated", "temporal.duration.negated", "negated", 0.0),
                ("with", "temporal.duration.with", "with", 1.0),
                ("total", "temporal.duration.total", "total", 1.0),
            ],
            &[
                ("years", "temporal.duration.get.years"),
                ("months", "temporal.duration.get.months"),
                ("weeks", "temporal.duration.get.weeks"),
                ("days", "temporal.duration.get.days"),
                ("hours", "temporal.duration.get.hours"),
                ("minutes", "temporal.duration.get.minutes"),
                ("seconds", "temporal.duration.get.seconds"),
                ("milliseconds", "temporal.duration.get.milliseconds"),
                ("microseconds", "temporal.duration.get.microseconds"),
                ("nanoseconds", "temporal.duration.get.nanoseconds"),
                ("blank", "temporal.duration.get.blank"),
            ],
            &object_proto,
        );
        let plain_year_month_ctor = self.temporal_make_ctor(
            ctx,
            "temporal.plainYearMonth.constructor",
            "PlainYearMonth",
            2.0,
            "Temporal.PlainYearMonth",
            &[
                ("from", "temporal.plainYearMonth.from", "from", 1.0),
                ("compare", "temporal.plainYearMonth.compare", "compare", 2.0),
            ],
            &[
                ("add", "temporal.plainYearMonth.add", "add", 1.0),
                ("subtract", "temporal.plainYearMonth.subtract", "subtract", 1.0),
                ("until", "temporal.plainYearMonth.until", "until", 1.0),
                ("since", "temporal.plainYearMonth.since", "since", 1.0),
                ("equals", "temporal.plainYearMonth.equals", "equals", 1.0),
                ("toString", "temporal.plainYearMonth.toString", "toString", 0.0),
                ("toJSON", "temporal.plainYearMonth.toJSON", "toJSON", 0.0),
                ("valueOf", "temporal.plainYearMonth.valueOf", "valueOf", 0.0),
            ],
            &[
                ("year", "temporal.plainYearMonth.get.year"),
                ("month", "temporal.plainYearMonth.get.month"),
                ("calendarId", "temporal.plainYearMonth.get.calendarId"),
            ],
            &object_proto,
        );
        let plain_month_day_ctor = self.temporal_make_ctor(
            ctx,
            "temporal.plainMonthDay.constructor",
            "PlainMonthDay",
            2.0,
            "Temporal.PlainMonthDay",
            &[
                ("from", "temporal.plainMonthDay.from", "from", 1.0),
                ("compare", "temporal.plainMonthDay.compare", "compare", 2.0),
            ],
            &[
                ("equals", "temporal.plainMonthDay.equals", "equals", 1.0),
                ("toString", "temporal.plainMonthDay.toString", "toString", 0.0),
                ("toJSON", "temporal.plainMonthDay.toJSON", "toJSON", 0.0),
                ("valueOf", "temporal.plainMonthDay.valueOf", "valueOf", 0.0),
            ],
            &[
                ("monthCode", "temporal.plainMonthDay.get.monthCode"),
                ("day", "temporal.plainMonthDay.get.day"),
                ("calendarId", "temporal.plainMonthDay.get.calendarId"),
            ],
            &object_proto,
        );
        let zoned_date_time_ctor = self.temporal_make_ctor(
            ctx,
            "temporal.zonedDateTime.constructor",
            "ZonedDateTime",
            2.0,
            "Temporal.ZonedDateTime",
            &[
                ("from", "temporal.zonedDateTime.from", "from", 1.0),
                ("compare", "temporal.zonedDateTime.compare", "compare", 2.0),
            ],
            &[
                ("withTimeZone", "temporal.zonedDateTime.withTimeZone", "withTimeZone", 1.0),
                ("withPlainTime", "temporal.zonedDateTime.withPlainTime", "withPlainTime", 0.0),
                ("withCalendar", "temporal.zonedDateTime.withCalendar", "withCalendar", 1.0),
                ("with", "temporal.zonedDateTime.with", "with", 1.0),
                ("add", "temporal.zonedDateTime.add", "add", 1.0),
                ("since", "temporal.zonedDateTime.since", "since", 1.0),
                ("until", "temporal.zonedDateTime.until", "until", 1.0),
                ("toPlainDateTime", "temporal.zonedDateTime.toPlainDateTime", "toPlainDateTime", 0.0),
                ("toPlainDate", "temporal.zonedDateTime.toPlainDate", "toPlainDate", 0.0),
                ("toPlainTime", "temporal.zonedDateTime.toPlainTime", "toPlainTime", 0.0),
                ("equals", "temporal.zonedDateTime.equals", "equals", 1.0),
                ("toString", "temporal.zonedDateTime.toString", "toString", 0.0),
                ("toJSON", "temporal.zonedDateTime.toJSON", "toJSON", 0.0),
                ("valueOf", "temporal.zonedDateTime.valueOf", "valueOf", 0.0),
            ],
            &[
                ("year", "temporal.zonedDateTime.get.year"),
                ("month", "temporal.zonedDateTime.get.month"),
                ("monthCode", "temporal.zonedDateTime.get.monthCode"),
                ("day", "temporal.zonedDateTime.get.day"),
                ("hour", "temporal.zonedDateTime.get.hour"),
                ("minute", "temporal.zonedDateTime.get.minute"),
                ("second", "temporal.zonedDateTime.get.second"),
                ("millisecond", "temporal.zonedDateTime.get.millisecond"),
                ("microsecond", "temporal.zonedDateTime.get.microsecond"),
                ("nanosecond", "temporal.zonedDateTime.get.nanosecond"),
                ("epochMilliseconds", "temporal.zonedDateTime.get.epochMilliseconds"),
                ("epochNanoseconds", "temporal.zonedDateTime.get.epochNanoseconds"),
                ("calendarId", "temporal.zonedDateTime.get.calendarId"),
                ("timeZoneId", "temporal.zonedDateTime.get.timeZoneId"),
                ("weekOfYear", "temporal.zonedDateTime.get.weekOfYear"),
                ("yearOfWeek", "temporal.zonedDateTime.get.yearOfWeek"),
            ],
            &object_proto,
        );

        let mut now_map = IndexMap::new();
        now_map.insert("__proto__".to_string(), object_proto.clone());
        for (key, host_name, length) in [
            ("instant", "temporal.now.instant", 0.0),
            ("plainDateISO", "temporal.now.plainDateISO", 0.0),
            ("plainTimeISO", "temporal.now.plainTimeISO", 0.0),
            ("plainDateTimeISO", "temporal.now.plainDateTimeISO", 0.0),
            ("zonedDateTimeISO", "temporal.now.zonedDateTimeISO", 0.0),
        ] {
            let value = Self::make_host_fn_with_name_len(ctx, host_name, key, length, false);
            Self::insert_property_with_attributes(&mut now_map, key, &value, true, false, true);
        }
        now_map.insert("@@sym:4".to_string(), Value::from("Temporal.Now"));
        write_attrs_to_legacy_map(&mut now_map, "@@sym:4", PropAttrs::CONFIGURABLE);
        let now_obj = Value::VmObject(new_gc_cell_ptr(ctx, now_map));

        let mut temporal_map = IndexMap::new();
        temporal_map.insert("__proto__".to_string(), object_proto);
        for (key, value) in [
            ("Instant", instant_ctor),
            ("PlainDate", plain_date_ctor),
            ("PlainTime", plain_time_ctor),
            ("PlainDateTime", plain_date_time_ctor),
            ("Duration", duration_ctor),
            ("PlainYearMonth", plain_year_month_ctor),
            ("PlainMonthDay", plain_month_day_ctor),
            ("ZonedDateTime", zoned_date_time_ctor),
            ("Now", now_obj),
        ] {
            Self::insert_property_with_attributes(&mut temporal_map, key, &value, true, false, true);
        }
        temporal_map.insert("@@sym:4".to_string(), Value::from("Temporal"));
        write_attrs_to_legacy_map(&mut temporal_map, "@@sym:4", PropAttrs::CONFIGURABLE);

        let temporal_value = Value::VmObject(new_gc_cell_ptr(ctx, temporal_map));
        self.globals.insert("Temporal".to_string(), temporal_value.clone());
        {
            let mut global_this = self.global_this.borrow_mut(ctx);
            global_this.insert("Temporal".to_string(), temporal_value);
            mark_nonenumerable(&mut global_this, "Temporal");
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn temporal_make_ctor(
        &mut self,
        ctx: &GcContext<'gc>,
        ctor_host_name: &str,
        ctor_name: &str,
        ctor_length: f64,
        to_string_tag: &str,
        static_methods: &[(&str, &str, &str, f64)],
        proto_methods: &[(&str, &str, &str, f64)],
        getters: &[(&str, &str)],
        object_proto: &Value<'gc>,
    ) -> Value<'gc> {
        let ctor = Self::make_host_fn_with_name_len(ctx, ctor_host_name, ctor_name, ctor_length, true);
        let ctor_obj = match ctor {
            Value::VmObject(obj) => obj,
            _ => unreachable!("host functions are objects"),
        };
        {
            let mut borrow = ctor_obj.borrow_mut(ctx);
            for (key, host_name, display_name, length) in static_methods {
                let value = Self::make_host_fn_with_name_len(ctx, host_name, display_name, *length, false);
                Self::insert_property_with_attributes(&mut borrow, key, &value, true, false, true);
            }
        }

        let proto_obj = new_gc_cell_ptr(ctx, IndexMap::new());
        {
            let mut proto = proto_obj.borrow_mut(ctx);
            proto.insert("__proto__".to_string(), object_proto.clone());
            for (key, host_name, display_name, length) in proto_methods {
                let value = Self::make_host_fn_with_name_len(ctx, host_name, display_name, *length, false);
                Self::insert_property_with_attributes(&mut proto, key, &value, true, false, true);
            }
            for (key, host_name) in getters {
                set_getter(
                    &mut proto,
                    key,
                    Self::make_host_fn_with_name_len(ctx, host_name, &format!("get {key}"), 0.0, false),
                );
                mark_nonenumerable(&mut proto, key);
            }
            proto.insert("@@sym:4".to_string(), Value::from(to_string_tag));
            write_attrs_to_legacy_map(&mut proto, "@@sym:4", PropAttrs::CONFIGURABLE);
        }

        Self::finalize_ctor_with_prototype(ctx, ctor_obj.borrow().clone(), proto_obj)
    }

    pub(super) fn temporal_throw(&mut self, ctx: &GcContext<'gc>, err: TemporalError) -> Value<'gc> {
        if self.pending_throw.is_none() {
            let js_err: JSError = err.into();
            self.pending_throw = Some(self.vm_value_from_error(ctx, &js_err));
        }
        Value::Undefined
    }

    fn temporal_repr_result(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, kind: &str) -> Value<'gc> {
        let Some(repr) = self.temporal_slot_string_value(ctx, receiver, kind, SLOT_REPR) else {
            return Value::Undefined;
        };
        Value::from(repr.as_str())
    }

    fn temporal_slot_string_value(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        kind: &str,
        slot: &str,
    ) -> Option<String> {
        let obj = self.temporal_expect_object(ctx, receiver, kind)?;
        match own_data_from_legacy_map(&obj.borrow(), slot) {
            Some(Value::String(text)) => Some(crate::unicode::utf16_to_utf8(&text)),
            _ => {
                self.throw_type_error(ctx, &format!("Temporal.{kind} is missing internal slot {slot}"));
                None
            }
        }
    }

    fn temporal_slot_string_if_kind(&self, value: &Value<'gc>, kind: &str, slot: &str) -> Option<String> {
        let Value::VmObject(obj) = value else {
            return None;
        };
        let borrow = obj.borrow();
        match own_data_from_legacy_map(&borrow, SLOT_KIND) {
            Some(Value::String(text)) if crate::unicode::utf16_to_utf8(&text) == kind => match own_data_from_legacy_map(&borrow, slot) {
                Some(Value::String(text)) => Some(crate::unicode::utf16_to_utf8(&text)),
                _ => None,
            },
            _ => None,
        }
    }

    fn temporal_intrinsic_ctor_value(&self, kind: &str) -> Option<Value<'gc>> {
        let temporal = self.globals.get("Temporal")?;
        let Value::VmObject(obj) = temporal else {
            return None;
        };
        own_data_from_legacy_map(&obj.borrow(), kind)
    }

    fn temporal_expect_object(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, kind: &str) -> Option<VmObjectHandle<'gc>> {
        let Some(Value::VmObject(obj)) = receiver.cloned() else {
            self.throw_type_error(ctx, &format!("Temporal.{kind} method called on incompatible receiver"));
            return None;
        };
        Some(obj)
    }

    fn temporal_ctor_prototype(&self, ctor_value: Option<&Value<'gc>>) -> Option<Value<'gc>> {
        ctor_value.and_then(|value| match value {
            Value::VmObject(obj) => own_data_from_legacy_map(&obj.borrow(), "prototype"),
            _ => None,
        })
    }

    fn temporal_store_slot(map: &mut IndexMap<String, Value<'gc>>, key: &str, value: Value<'gc>) {
        Self::insert_property_with_attributes(map, key, &value, true, false, true);
    }

    fn temporal_store_readonly(map: &mut IndexMap<String, Value<'gc>>, key: &str, value: Value<'gc>) {
        Self::insert_property_with_attributes(map, key, &value, false, false, true);
    }

    fn temporal_compare_result(ordering: Ordering) -> Value<'gc> {
        Value::Number(match ordering {
            Ordering::Less => -1.0,
            Ordering::Equal => 0.0,
            Ordering::Greater => 1.0,
        })
    }

    fn temporal_has_own_or_inherited_property(&self, _ctx: &GcContext<'gc>, value: &Value<'gc>, key: &str) -> bool {
        match value {
            Value::VmObject(obj) => obj.borrow().contains_key(key),
            Value::VmArray(arr) => arr.borrow().props.contains_key(key),
            _ => false,
        }
    }

    fn temporal_attach_bound_methods(&self, ctx: &GcContext<'gc>, obj: &VmObjectHandle<'gc>, names: &[(&str, &str)]) {
        let receiver = Value::VmObject(*obj);
        let mut borrow = obj.borrow_mut(ctx);
        for (key, host_name) in names {
            let value = Self::make_bound_host_fn(ctx, host_name, &receiver);
            Self::insert_property_with_attributes(&mut borrow, key, &value, true, false, true);
        }
    }

    fn temporal_wrap_value(
        &self,
        ctx: &GcContext<'gc>,
        ctor_value: Option<&Value<'gc>>,
        kind: &str,
        repr: &str,
        extra_slots: &[(&str, Value<'gc>)],
    ) -> Value<'gc> {
        let mut map = IndexMap::new();
        if let Some(proto) = self.temporal_ctor_prototype(ctor_value) {
            map.insert("__proto__".to_string(), proto);
        }
        Self::temporal_store_slot(&mut map, "__type__", Value::from(&format!("Temporal.{kind}")));
        Self::temporal_store_slot(&mut map, SLOT_KIND, Value::from(kind));
        Self::temporal_store_slot(&mut map, SLOT_REPR, Value::from(repr));
        for (key, value) in extra_slots {
            Self::temporal_store_slot(&mut map, key, value.clone());
        }
        Value::VmObject(new_gc_cell_ptr(ctx, map))
    }

    pub(super) fn temporal_wrap_instant(&self, ctx: &GcContext<'gc>, ctor_value: Option<&Value<'gc>>, value: &Instant) -> Value<'gc> {
        let wrapped = self.temporal_wrap_value(
            ctx,
            ctor_value,
            "Instant",
            &value
                .to_ixdtf_string(None, ToStringRoundingOptions::default())
                .unwrap_or_else(|_| format!("{}ns", value.as_i128())),
            &[(SLOT_EPOCH_NS, Value::from(value.as_i128().to_string().as_str()))],
        );
        if let Value::VmObject(obj) = &wrapped {
            self.temporal_attach_bound_methods(
                ctx,
                obj,
                &[
                    ("toString", "temporal.instant.toString"),
                    ("toJSON", "temporal.instant.toJSON"),
                    ("valueOf", "temporal.instant.valueOf"),
                ],
            );
            let mut borrow = obj.borrow_mut(ctx);
            Self::temporal_store_readonly(&mut borrow, "epochMilliseconds", Value::Number(value.epoch_milliseconds() as f64));
            Self::temporal_store_readonly(
                &mut borrow,
                "epochNanoseconds",
                Value::BigInt(Box::new(BigInt::from(value.as_i128()))),
            );
        }
        wrapped
    }

    fn temporal_wrap_plain_date(&self, ctx: &GcContext<'gc>, ctor_value: Option<&Value<'gc>>, value: &PlainDate) -> Value<'gc> {
        let wrapped = self.temporal_wrap_value(ctx, ctor_value, "PlainDate", &value.to_string(), &[]);
        if let Value::VmObject(obj) = &wrapped {
            self.temporal_attach_bound_methods(
                ctx,
                obj,
                &[
                    ("toPlainDateTime", "temporal.plainDate.toPlainDateTime"),
                    ("toZonedDateTime", "temporal.plainDate.toZonedDateTime"),
                    ("toString", "temporal.plainDate.toString"),
                    ("toJSON", "temporal.plainDate.toJSON"),
                    ("valueOf", "temporal.plainDate.valueOf"),
                ],
            );
            let mut borrow = obj.borrow_mut(ctx);
            Self::temporal_store_readonly(&mut borrow, "year", Value::Number(value.year() as f64));
            Self::temporal_store_readonly(&mut borrow, "month", Value::Number(value.month() as f64));
            Self::temporal_store_readonly(&mut borrow, "monthCode", Value::from(value.month_code().as_str()));
            Self::temporal_store_readonly(&mut borrow, "day", Value::Number(value.day() as f64));
            Self::temporal_store_readonly(&mut borrow, "calendarId", Value::from(value.calendar().identifier()));
        }
        wrapped
    }

    fn temporal_wrap_plain_time(&self, ctx: &GcContext<'gc>, ctor_value: Option<&Value<'gc>>, value: &PlainTime) -> Value<'gc> {
        let wrapped = self.temporal_wrap_value(
            ctx,
            ctor_value,
            "PlainTime",
            &value
                .to_ixdtf_string(ToStringRoundingOptions::default())
                .unwrap_or_else(|_| "00:00:00".to_string()),
            &[],
        );
        if let Value::VmObject(obj) = &wrapped {
            self.temporal_attach_bound_methods(
                ctx,
                obj,
                &[
                    ("toString", "temporal.plainTime.toString"),
                    ("toJSON", "temporal.plainTime.toJSON"),
                    ("valueOf", "temporal.plainTime.valueOf"),
                ],
            );
            let mut borrow = obj.borrow_mut(ctx);
            Self::temporal_store_readonly(&mut borrow, "hour", Value::Number(value.hour() as f64));
            Self::temporal_store_readonly(&mut borrow, "minute", Value::Number(value.minute() as f64));
            Self::temporal_store_readonly(&mut borrow, "second", Value::Number(value.second() as f64));
            Self::temporal_store_readonly(&mut borrow, "millisecond", Value::Number(value.millisecond() as f64));
            Self::temporal_store_readonly(&mut borrow, "microsecond", Value::Number(value.microsecond() as f64));
            Self::temporal_store_readonly(&mut borrow, "nanosecond", Value::Number(value.nanosecond() as f64));
        }
        wrapped
    }

    fn temporal_wrap_plain_date_time(&self, ctx: &GcContext<'gc>, ctor_value: Option<&Value<'gc>>, value: &PlainDateTime) -> Value<'gc> {
        let wrapped = self.temporal_wrap_value(ctx, ctor_value, "PlainDateTime", &value.to_string(), &[]);
        if let Value::VmObject(obj) = &wrapped {
            self.temporal_attach_bound_methods(
                ctx,
                obj,
                &[
                    ("toZonedDateTime", "temporal.plainDateTime.toZonedDateTime"),
                    ("toString", "temporal.plainDateTime.toString"),
                    ("toJSON", "temporal.plainDateTime.toJSON"),
                    ("valueOf", "temporal.plainDateTime.valueOf"),
                ],
            );
            let mut borrow = obj.borrow_mut(ctx);
            Self::temporal_store_readonly(&mut borrow, "year", Value::Number(value.year() as f64));
            Self::temporal_store_readonly(&mut borrow, "month", Value::Number(value.month() as f64));
            Self::temporal_store_readonly(&mut borrow, "day", Value::Number(value.day() as f64));
            Self::temporal_store_readonly(&mut borrow, "hour", Value::Number(value.hour() as f64));
            Self::temporal_store_readonly(&mut borrow, "minute", Value::Number(value.minute() as f64));
            Self::temporal_store_readonly(&mut borrow, "second", Value::Number(value.second() as f64));
            Self::temporal_store_readonly(&mut borrow, "millisecond", Value::Number(value.millisecond() as f64));
            Self::temporal_store_readonly(&mut borrow, "microsecond", Value::Number(value.microsecond() as f64));
            Self::temporal_store_readonly(&mut borrow, "nanosecond", Value::Number(value.nanosecond() as f64));
            Self::temporal_store_readonly(&mut borrow, "monthCode", Value::from(value.month_code().as_str()));
            Self::temporal_store_readonly(&mut borrow, "calendarId", Value::from(value.calendar().identifier()));
        }
        wrapped
    }

    fn temporal_wrap_duration(&self, ctx: &GcContext<'gc>, ctor_value: Option<&Value<'gc>>, value: &TemporalDuration) -> Value<'gc> {
        let wrapped = self.temporal_wrap_value(ctx, ctor_value, "Duration", &value.to_string(), &[]);
        if let Value::VmObject(obj) = &wrapped {
            self.temporal_attach_bound_methods(
                ctx,
                obj,
                &[
                    ("toString", "temporal.duration.toString"),
                    ("toJSON", "temporal.duration.toJSON"),
                    ("valueOf", "temporal.duration.valueOf"),
                    ("add", "temporal.duration.add"),
                    ("round", "temporal.duration.round"),
                    ("negated", "temporal.duration.negated"),
                    ("with", "temporal.duration.with"),
                    ("total", "temporal.duration.total"),
                ],
            );
            let mut borrow = obj.borrow_mut(ctx);
            Self::temporal_store_readonly(&mut borrow, "years", Value::Number(value.years() as f64));
            Self::temporal_store_readonly(&mut borrow, "months", Value::Number(value.months() as f64));
            Self::temporal_store_readonly(&mut borrow, "weeks", Value::Number(value.weeks() as f64));
            Self::temporal_store_readonly(&mut borrow, "days", Value::Number(value.days() as f64));
            Self::temporal_store_readonly(&mut borrow, "hours", Value::Number(value.hours() as f64));
            Self::temporal_store_readonly(&mut borrow, "minutes", Value::Number(value.minutes() as f64));
            Self::temporal_store_readonly(&mut borrow, "seconds", Value::Number(value.seconds() as f64));
            Self::temporal_store_readonly(&mut borrow, "milliseconds", Value::Number(value.milliseconds() as f64));
            Self::temporal_store_readonly(&mut borrow, "microseconds", Value::Number(value.microseconds() as f64));
            Self::temporal_store_readonly(&mut borrow, "nanoseconds", Value::Number(value.nanoseconds() as f64));
            Self::temporal_store_readonly(&mut borrow, "blank", Value::Boolean(value.is_zero()));
        }
        wrapped
    }

    fn temporal_wrap_plain_year_month(&self, ctx: &GcContext<'gc>, ctor_value: Option<&Value<'gc>>, value: &PlainYearMonth) -> Value<'gc> {
        let wrapped = self.temporal_wrap_value(
            ctx,
            ctor_value,
            "PlainYearMonth",
            &value.to_string(),
            &[(SLOT_REFERENCE_DAY, Value::from(value.reference_day().to_string().as_str()))],
        );
        if let Value::VmObject(obj) = &wrapped {
            self.temporal_attach_bound_methods(
                ctx,
                obj,
                &[
                    ("toString", "temporal.plainYearMonth.toString"),
                    ("toJSON", "temporal.plainYearMonth.toJSON"),
                    ("valueOf", "temporal.plainYearMonth.valueOf"),
                ],
            );
            let mut borrow = obj.borrow_mut(ctx);
            Self::temporal_store_readonly(&mut borrow, "year", Value::Number(value.year() as f64));
            Self::temporal_store_readonly(&mut borrow, "month", Value::Number(value.month() as f64));
            Self::temporal_store_readonly(&mut borrow, "calendarId", Value::from(value.calendar().identifier()));
        }
        wrapped
    }

    fn temporal_wrap_plain_month_day(&self, ctx: &GcContext<'gc>, ctor_value: Option<&Value<'gc>>, value: &PlainMonthDay) -> Value<'gc> {
        let wrapped = self.temporal_wrap_value(
            ctx,
            ctor_value,
            "PlainMonthDay",
            &value.to_string(),
            &[(SLOT_REFERENCE_YEAR, Value::from(value.reference_year().to_string().as_str()))],
        );
        if let Value::VmObject(obj) = &wrapped {
            self.temporal_attach_bound_methods(
                ctx,
                obj,
                &[
                    ("toString", "temporal.plainMonthDay.toString"),
                    ("toJSON", "temporal.plainMonthDay.toJSON"),
                    ("valueOf", "temporal.plainMonthDay.valueOf"),
                ],
            );
            let mut borrow = obj.borrow_mut(ctx);
            Self::temporal_store_readonly(&mut borrow, "monthCode", Value::from(value.month_code().as_str()));
            Self::temporal_store_readonly(&mut borrow, "day", Value::Number(value.day() as f64));
            Self::temporal_store_readonly(&mut borrow, "calendarId", Value::from(value.calendar().identifier()));
        }
        wrapped
    }

    fn temporal_wrap_zoned_date_time(&self, ctx: &GcContext<'gc>, ctor_value: Option<&Value<'gc>>, value: &ZonedDateTime) -> Value<'gc> {
        let wrapped = self.temporal_wrap_value(
            ctx,
            ctor_value,
            "ZonedDateTime",
            &value.to_string(),
            &[(SLOT_EPOCH_NS, Value::from(value.to_instant().as_i128().to_string().as_str()))],
        );
        if let Value::VmObject(obj) = &wrapped {
            self.temporal_attach_bound_methods(
                ctx,
                obj,
                &[
                    ("withTimeZone", "temporal.zonedDateTime.withTimeZone"),
                    ("withPlainTime", "temporal.zonedDateTime.withPlainTime"),
                    ("withCalendar", "temporal.zonedDateTime.withCalendar"),
                    ("with", "temporal.zonedDateTime.with"),
                    ("add", "temporal.zonedDateTime.add"),
                    ("since", "temporal.zonedDateTime.since"),
                    ("until", "temporal.zonedDateTime.until"),
                    ("toPlainDateTime", "temporal.zonedDateTime.toPlainDateTime"),
                    ("toPlainDate", "temporal.zonedDateTime.toPlainDate"),
                    ("toPlainTime", "temporal.zonedDateTime.toPlainTime"),
                    ("equals", "temporal.zonedDateTime.equals"),
                    ("toString", "temporal.zonedDateTime.toString"),
                    ("toJSON", "temporal.zonedDateTime.toJSON"),
                    ("valueOf", "temporal.zonedDateTime.valueOf"),
                ],
            );
            let mut borrow = obj.borrow_mut(ctx);
            Self::temporal_store_readonly(&mut borrow, "year", Value::Number(value.year() as f64));
            Self::temporal_store_readonly(&mut borrow, "month", Value::Number(value.month() as f64));
            Self::temporal_store_readonly(&mut borrow, "day", Value::Number(value.day() as f64));
            Self::temporal_store_readonly(&mut borrow, "hour", Value::Number(value.hour() as f64));
            Self::temporal_store_readonly(&mut borrow, "minute", Value::Number(value.minute() as f64));
            Self::temporal_store_readonly(&mut borrow, "second", Value::Number(value.second() as f64));
            Self::temporal_store_readonly(&mut borrow, "millisecond", Value::Number(value.millisecond() as f64));
            Self::temporal_store_readonly(&mut borrow, "microsecond", Value::Number(value.microsecond() as f64));
            Self::temporal_store_readonly(&mut borrow, "nanosecond", Value::Number(value.nanosecond() as f64));
            Self::temporal_store_readonly(&mut borrow, "epochMilliseconds", Value::Number(value.epoch_milliseconds() as f64));
            Self::temporal_store_readonly(
                &mut borrow,
                "epochNanoseconds",
                Value::BigInt(Box::new(BigInt::from(value.to_instant().as_i128()))),
            );
            Self::temporal_store_readonly(&mut borrow, "calendarId", Value::from(value.calendar().identifier()));
            if let Ok(time_zone_id) = value.time_zone().identifier() {
                Self::temporal_store_readonly(&mut borrow, "timeZoneId", Value::from(time_zone_id.as_str()));
            }
        }
        wrapped
    }

    fn temporal_value_string(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>) -> Option<String> {
        match self.vm_to_string_like_spec(ctx, value) {
            Ok(text) => Some(text),
            Err(err) => {
                self.pending_throw = Some(self.vm_value_from_error(ctx, &err));
                None
            }
        }
    }

    fn temporal_calendar_arg(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Option<Calendar> {
        let Some(value) = value else {
            return Some(Calendar::ISO);
        };
        if matches!(value, Value::Undefined) {
            return Some(Calendar::ISO);
        }
        let text = self.temporal_value_string(ctx, value)?;
        match Calendar::from_str(&text) {
            Ok(calendar) => Some(calendar),
            Err(err) => {
                self.temporal_throw(ctx, err);
                None
            }
        }
    }

    fn temporal_time_zone_arg(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Option<TimeZone> {
        let value = value?;
        if matches!(value, Value::Undefined) {
            return None;
        }
        let text = self.temporal_value_string(ctx, value)?;
        match TimeZone::try_from_str(&text) {
            Ok(time_zone) => Some(time_zone),
            Err(err) => {
                self.temporal_throw(ctx, err);
                None
            }
        }
    }

    fn temporal_time_zone_identifier_arg(
        &mut self,
        _ctx: &GcContext<'gc>,
        value: Option<&Value<'gc>>,
    ) -> Result<Option<TimeZone>, TemporalError> {
        let Some(value) = value else {
            return Ok(None);
        };
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        let text = match value {
            Value::String(text) => crate::unicode::utf16_to_utf8(text),
            _ => return Err(TemporalError::r#type().with_message("Invalid time zone")),
        };
        if text.eq_ignore_ascii_case("UTC") {
            return Ok(Some(TimeZone::utc()));
        }
        TimeZone::try_from_identifier_str(&text).map(Some)
    }

    fn temporal_time_zone_with_iso_string_arg(
        &mut self,
        _ctx: &GcContext<'gc>,
        value: Option<&Value<'gc>>,
    ) -> Result<Option<TimeZone>, TemporalError> {
        let Some(value) = value else {
            return Ok(None);
        };
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        let text = match value {
            Value::String(text) => crate::unicode::utf16_to_utf8(text),
            _ => return Err(TemporalError::r#type().with_message("Invalid time zone")),
        };
        TimeZone::try_from_str(&text).map(Some)
    }

    fn temporal_disambiguation_option_arg(
        &mut self,
        ctx: &GcContext<'gc>,
        value: Option<&Value<'gc>>,
    ) -> Result<Disambiguation, TemporalError> {
        let Some(value) = value else {
            return Ok(Disambiguation::Compatible);
        };
        if matches!(value, Value::Undefined) {
            return Ok(Disambiguation::Compatible);
        }
        if !self.temporal_is_object_like(value) {
            return Err(TemporalError::r#type().with_message("Options must be an object"));
        }
        let disambiguation = self.read_named_property(ctx, value, "disambiguation");
        if self.pending_throw.is_some() {
            return Err(TemporalError::general("Failed to read disambiguation"));
        }
        if matches!(disambiguation, Value::Undefined) {
            return Ok(Disambiguation::Compatible);
        }
        let text = self
            .temporal_value_string(ctx, &disambiguation)
            .ok_or_else(|| TemporalError::r#type().with_message("Invalid disambiguation"))?;
        match text.as_str() {
            "compatible" => Ok(Disambiguation::Compatible),
            "earlier" => Ok(Disambiguation::Earlier),
            "later" => Ok(Disambiguation::Later),
            "reject" => Ok(Disambiguation::Reject),
            _ => Err(TemporalError::range().with_message("Invalid disambiguation")),
        }
    }

    fn temporal_trunc_i32_field(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>, _name: &str) -> Result<i32, TemporalError> {
        let number = self
            .extract_number_with_coercion(ctx, value)
            .ok_or_else(|| TemporalError::range().with_message("Invalid temporal field"))?;
        if !number.is_finite() || number.trunc() < i32::MIN as f64 || number.trunc() > i32::MAX as f64 {
            return Err(TemporalError::range().with_message("Invalid temporal field"));
        }
        Ok(number.trunc() as i32)
    }

    fn temporal_trunc_u8_field(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>, _name: &str) -> Result<u8, TemporalError> {
        let number = self
            .extract_number_with_coercion(ctx, value)
            .ok_or_else(|| TemporalError::range().with_message("Invalid temporal field"))?;
        let truncated = number.trunc();
        if !number.is_finite() || truncated < 0.0 || truncated > u8::MAX as f64 {
            return Err(TemporalError::range().with_message("Invalid temporal field"));
        }
        Ok(truncated as u8)
    }

    fn temporal_trunc_u16_field(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>, _name: &str) -> Result<u16, TemporalError> {
        let number = self
            .extract_number_with_coercion(ctx, value)
            .ok_or_else(|| TemporalError::range().with_message("Invalid temporal field"))?;
        let truncated = number.trunc();
        if !number.is_finite() || truncated < 0.0 || truncated > u16::MAX as f64 {
            return Err(TemporalError::range().with_message("Invalid temporal field"));
        }
        Ok(truncated as u16)
    }

    fn temporal_optional_trunc_i32_property(
        &mut self,
        ctx: &GcContext<'gc>,
        object: &Value<'gc>,
        key: &str,
    ) -> Result<Option<i32>, TemporalError> {
        let value = self.read_named_property(ctx, object, key);
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        self.temporal_trunc_i32_field(ctx, &value, key).map(Some)
    }

    fn temporal_optional_trunc_u8_property(
        &mut self,
        ctx: &GcContext<'gc>,
        object: &Value<'gc>,
        key: &str,
    ) -> Result<Option<u8>, TemporalError> {
        let value = self.read_named_property(ctx, object, key);
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        self.temporal_trunc_u8_field(ctx, &value, key).map(Some)
    }

    fn temporal_optional_trunc_u16_property(
        &mut self,
        ctx: &GcContext<'gc>,
        object: &Value<'gc>,
        key: &str,
    ) -> Result<Option<u16>, TemporalError> {
        let value = self.read_named_property(ctx, object, key);
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        self.temporal_trunc_u16_field(ctx, &value, key).map(Some)
    }

    fn temporal_get_option_string(
        &mut self,
        ctx: &GcContext<'gc>,
        options: &Value<'gc>,
        key: &str,
    ) -> Result<Option<String>, TemporalError> {
        let value = self.read_named_property(ctx, options, key);
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal options"));
        }
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        self.temporal_value_string(ctx, &value)
            .ok_or_else(|| TemporalError::r#type().with_message("Invalid Temporal option"))
            .map(Some)
    }

    fn temporal_get_option_number(&mut self, ctx: &GcContext<'gc>, options: &Value<'gc>, key: &str) -> Result<Option<f64>, TemporalError> {
        let value = self.read_named_property(ctx, options, key);
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal options"));
        }
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        self.extract_number_with_coercion(ctx, &value)
            .ok_or_else(|| TemporalError::range().with_message("Invalid numeric option"))
            .map(Some)
    }

    fn temporal_overflow_option_arg(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Result<Overflow, TemporalError> {
        let Some(options) = value else {
            return Ok(Overflow::Constrain);
        };
        if matches!(options, Value::Undefined) {
            return Ok(Overflow::Constrain);
        }
        if !self.temporal_is_object_like(options) {
            return Err(TemporalError::r#type().with_message("Options must be an object"));
        }
        match self.temporal_get_option_string(ctx, options, "overflow")? {
            Some(value) => Overflow::from_str(&value).map_err(|_| TemporalError::range().with_message("Invalid overflow")),
            None => Ok(Overflow::Constrain),
        }
    }

    fn temporal_difference_settings_arg(
        &mut self,
        ctx: &GcContext<'gc>,
        value: Option<&Value<'gc>>,
    ) -> Result<DifferenceSettings, TemporalError> {
        let Some(options) = value else {
            return Ok(DifferenceSettings::default());
        };
        if matches!(options, Value::Undefined) {
            return Ok(DifferenceSettings::default());
        }
        if !self.temporal_is_object_like(options) {
            return Err(TemporalError::r#type().with_message("Options must be an object"));
        }

        let largest_unit = match self.temporal_get_option_string(ctx, options, "largestUnit")? {
            Some(value) => Some(Unit::from_str(&value).map_err(|_| TemporalError::range().with_message("Invalid largestUnit"))?),
            None => None,
        };
        let increment = match self.temporal_get_option_number(ctx, options, "roundingIncrement")? {
            Some(value) => Some(RoundingIncrement::try_from(value)?),
            None => None,
        };
        let rounding_mode = match self.temporal_get_option_string(ctx, options, "roundingMode")? {
            Some(value) => Some(RoundingMode::from_str(&value)?),
            None => None,
        };
        let smallest_unit = match self.temporal_get_option_string(ctx, options, "smallestUnit")? {
            Some(value) => Some(Unit::from_str(&value).map_err(|_| TemporalError::range().with_message("Invalid smallestUnit"))?),
            None => None,
        };
        let mut settings = DifferenceSettings::default();
        settings.largest_unit = largest_unit;
        settings.smallest_unit = smallest_unit;
        settings.rounding_mode = rounding_mode;
        settings.increment = increment;
        Ok(settings)
    }

    fn temporal_duration_round_args(
        &mut self,
        ctx: &GcContext<'gc>,
        current: &TemporalDuration,
        value: &Value<'gc>,
    ) -> Result<(RoundingOptions, Option<RelativeTo>), TemporalError> {
        if let Value::String(text) = value {
            let unit = Unit::from_str(&crate::unicode::utf16_to_utf8(text))
                .map_err(|_| TemporalError::range().with_message("Invalid smallestUnit"))?;
            let mut options = RoundingOptions::default();
            options.smallest_unit = Some(unit);
            return Ok((options, None));
        }
        if !self.temporal_is_object_like(value) {
            return Err(TemporalError::r#type().with_message("Options must be an object or string"));
        }

        let largest_unit = match self.temporal_get_option_string(ctx, value, "largestUnit")? {
            Some(value) => Some(Unit::from_str(&value).map_err(|_| TemporalError::range().with_message("Invalid largestUnit"))?),
            None => None,
        };

        let relative_to_value = self.read_named_property(ctx, value, "relativeTo");
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal options"));
        }

        let rounding_increment = match self.temporal_get_option_number(ctx, value, "roundingIncrement")? {
            Some(value) => Some(RoundingIncrement::try_from(value)?),
            None => None,
        };
        let rounding_mode = match self.temporal_get_option_string(ctx, value, "roundingMode")? {
            Some(value) => Some(RoundingMode::from_str(&value)?),
            None => None,
        };
        let smallest_unit = match self.temporal_get_option_string(ctx, value, "smallestUnit")? {
            Some(value) => Some(Unit::from_str(&value).map_err(|_| TemporalError::range().with_message("Invalid smallestUnit"))?),
            None => None,
        };

        let mut options = RoundingOptions::default();
        options.largest_unit = largest_unit;
        options.smallest_unit = smallest_unit;
        options.rounding_mode = rounding_mode;
        options.increment = rounding_increment;

        let needs_relative_to = current.years() != 0
            || current.months() != 0
            || current.weeks() != 0
            || current.days() != 0
            || options.largest_unit.is_some_and(|unit| unit.is_date_unit())
            || options.smallest_unit.is_some_and(|unit| unit.is_date_unit());
        let relative_to = match relative_to_value {
            Value::Undefined => None,
            value if needs_relative_to => Some(self.temporal_relative_to_from_value(ctx, &value)?),
            _ => None,
        };
        Ok((options, relative_to))
    }

    fn temporal_zoned_date_time_with_options_arg(
        &mut self,
        ctx: &GcContext<'gc>,
        value: Option<&Value<'gc>>,
    ) -> Result<(Disambiguation, OffsetDisambiguation, Overflow), TemporalError> {
        let Some(options) = value else {
            return Ok((Disambiguation::Compatible, OffsetDisambiguation::Prefer, Overflow::Constrain));
        };
        if matches!(options, Value::Undefined) {
            return Ok((Disambiguation::Compatible, OffsetDisambiguation::Prefer, Overflow::Constrain));
        }
        if !self.temporal_is_object_like(options) {
            return Err(TemporalError::r#type().with_message("Options must be an object"));
        }
        let disambiguation = match self.temporal_get_option_string(ctx, options, "disambiguation")? {
            Some(value) => Disambiguation::from_str(&value).map_err(|_| TemporalError::range().with_message("Invalid disambiguation"))?,
            None => Disambiguation::Compatible,
        };
        let offset = match self.temporal_get_option_string(ctx, options, "offset")? {
            Some(value) => OffsetDisambiguation::from_str(&value).map_err(|_| TemporalError::range().with_message("Invalid offset"))?,
            None => OffsetDisambiguation::Prefer,
        };
        let overflow = match self.temporal_get_option_string(ctx, options, "overflow")? {
            Some(value) => Overflow::from_str(&value).map_err(|_| TemporalError::range().with_message("Invalid overflow"))?,
            None => Overflow::Constrain,
        };
        Ok((disambiguation, offset, overflow))
    }

    fn temporal_zoned_date_time_with_fields_arg(
        &mut self,
        ctx: &GcContext<'gc>,
        current: &ZonedDateTime,
        value: &Value<'gc>,
    ) -> Result<ZonedDateTimeFields, TemporalError> {
        if !self.temporal_is_object_like(value) {
            return Err(TemporalError::r#type().with_message("Temporal.ZonedDateTime.prototype.with requires an object"));
        }
        if self.temporal_slot_string_if_kind(value, "ZonedDateTime", SLOT_KIND).is_some()
            || self.temporal_slot_string_if_kind(value, "PlainDateTime", SLOT_KIND).is_some()
            || self.temporal_slot_string_if_kind(value, "PlainDate", SLOT_KIND).is_some()
            || self.temporal_slot_string_if_kind(value, "PlainTime", SLOT_KIND).is_some()
            || self.temporal_slot_string_if_kind(value, "PlainYearMonth", SLOT_KIND).is_some()
            || self.temporal_slot_string_if_kind(value, "PlainMonthDay", SLOT_KIND).is_some()
        {
            return Err(TemporalError::r#type().with_message("Temporal object is not a valid property bag"));
        }

        let calendar = self.read_named_property(ctx, value, "calendar");
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        if !matches!(calendar, Value::Undefined) {
            return Err(TemporalError::r#type().with_message("calendar is not allowed"));
        }
        let time_zone = self.read_named_property(ctx, value, "timeZone");
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        if !matches!(time_zone, Value::Undefined) {
            return Err(TemporalError::r#type().with_message("timeZone is not allowed"));
        }

        let day = self.temporal_optional_trunc_u8_property(ctx, value, "day")?;
        let hour = self.temporal_optional_trunc_u8_property(ctx, value, "hour")?;
        let microsecond = self.temporal_optional_trunc_u16_property(ctx, value, "microsecond")?;
        let millisecond = self.temporal_optional_trunc_u16_property(ctx, value, "millisecond")?;
        let minute = self.temporal_optional_trunc_u8_property(ctx, value, "minute")?;
        let month = self.temporal_optional_trunc_u8_property(ctx, value, "month")?;
        let month_code = {
            let month_code_value = self.read_named_property(ctx, value, "monthCode");
            if self.pending_throw.is_some() {
                return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
            }
            if matches!(month_code_value, Value::Undefined) {
                None
            } else {
                let text = self.temporal_textual_property(ctx, &month_code_value, "monthCode")?;
                Some(MonthCode::from_str(&text).map_err(|_| TemporalError::range().with_message("Invalid monthCode"))?)
            }
        };
        let nanosecond = self.temporal_optional_trunc_u16_property(ctx, value, "nanosecond")?;
        let offset = {
            let offset_value = self.read_named_property(ctx, value, "offset");
            if self.pending_throw.is_some() {
                return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
            }
            if matches!(offset_value, Value::Undefined) {
                None
            } else {
                let text = self.temporal_textual_property(ctx, &offset_value, "offset")?;
                Some(UtcOffset::from_utf8(text.as_bytes()).map_err(|_| TemporalError::range().with_message("Invalid offset"))?)
            }
        };
        let second = self.temporal_optional_trunc_u8_property(ctx, value, "second")?;
        let year = self.temporal_optional_trunc_i32_property(ctx, value, "year")?;
        let had_any_property = day.is_some()
            || hour.is_some()
            || microsecond.is_some()
            || millisecond.is_some()
            || minute.is_some()
            || month.is_some()
            || month_code.is_some()
            || nanosecond.is_some()
            || offset.is_some()
            || second.is_some()
            || year.is_some();

        let month = month.or_else(|| if month_code.is_none() { Some(current.month()) } else { None });
        let month_code = month_code.or_else(|| if month.is_none() { Some(current.month_code()) } else { None });

        let calendar_fields = CalendarFields::new()
            .with_optional_year(year)
            .with_optional_month(month)
            .with_optional_month_code(month_code)
            .with_optional_day(day);
        let time = PartialTime::new()
            .with_hour(hour)
            .with_minute(minute)
            .with_second(second)
            .with_millisecond(millisecond)
            .with_microsecond(microsecond)
            .with_nanosecond(nanosecond);
        let fields = ZonedDateTimeFields {
            calendar_fields,
            time,
            offset,
        };
        if !had_any_property {
            return Err(TemporalError::r#type().with_message("Property bag must contain at least one recognized property"));
        }
        Ok(fields)
    }

    fn temporal_calendar_identifier_arg(&self, value: &Value<'gc>) -> Result<Calendar, TemporalError> {
        if let Some(calendar_id) = self.temporal_calendar_slot_id(value) {
            return Calendar::from_str(&calendar_id.to_ascii_lowercase())
                .map_err(|_| TemporalError::range().with_message("Invalid calendar"));
        }
        let Value::String(text) = value else {
            return Err(TemporalError::r#type().with_message("Invalid calendar"));
        };
        let text = crate::unicode::utf16_to_utf8(text);
        Calendar::from_str(&text.to_ascii_lowercase()).map_err(|_| TemporalError::range().with_message("Invalid calendar"))
    }

    fn temporal_calendar_slot_id(&self, value: &Value<'gc>) -> Option<String> {
        let Value::VmObject(obj) = value else {
            return None;
        };
        let borrow = obj.borrow();
        match own_data_from_legacy_map(&borrow, SLOT_KIND) {
            Some(Value::String(kind))
                if matches!(
                    crate::unicode::utf16_to_utf8(&kind).as_str(),
                    "PlainDate" | "PlainDateTime" | "PlainMonthDay" | "PlainYearMonth" | "ZonedDateTime"
                ) =>
            {
                match own_data_from_legacy_map(&borrow, "calendarId") {
                    Some(Value::String(text)) => Some(crate::unicode::utf16_to_utf8(&text)),
                    _ => Some("iso8601".to_string()),
                }
            }
            _ => None,
        }
    }

    fn temporal_now_time_zone(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Option<TimeZone> {
        match value {
            Some(Value::Undefined) | None => None,
            other => self.temporal_time_zone_arg(ctx, other),
        }
    }

    fn temporal_number_i32(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>, name: &str) -> Option<i32> {
        let number = self.extract_number_with_coercion(ctx, value)?;
        if !number.is_finite() || number.fract() != 0.0 || number < i32::MIN as f64 || number > i32::MAX as f64 {
            self.throw_range_error_object(ctx, &format!("{name} must be a finite integer"));
            return None;
        }
        Some(number as i32)
    }

    fn temporal_number_u8(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>, name: &str) -> Option<u8> {
        let number = self.extract_number_with_coercion(ctx, value)?;
        if !number.is_finite() || number.fract() != 0.0 || number < 0.0 || number > u8::MAX as f64 {
            self.throw_range_error_object(ctx, &format!("{name} must be a finite integer"));
            return None;
        }
        Some(number as u8)
    }

    fn temporal_number_u16(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>, name: &str) -> Option<u16> {
        let number = self.extract_number_with_coercion(ctx, value)?;
        if !number.is_finite() || number.fract() != 0.0 || number < 0.0 || number > u16::MAX as f64 {
            self.throw_range_error_object(ctx, &format!("{name} must be a finite integer"));
            return None;
        }
        Some(number as u16)
    }

    fn temporal_number_i64(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>, name: &str) -> Option<i64> {
        let number = self.extract_number_with_coercion(ctx, value)?;
        if !number.is_finite() || number.fract() != 0.0 || number < i64::MIN as f64 || number > i64::MAX as f64 {
            self.throw_range_error_object(ctx, &format!("{name} must be a finite integer"));
            return None;
        }
        Some(number as i64)
    }

    fn temporal_number_i128(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>, name: &str) -> Option<i128> {
        let number = self.extract_number_with_coercion(ctx, value)?;
        if !number.is_finite() || number.fract() != 0.0 {
            self.throw_range_error_object(ctx, &format!("{name} must be a finite integer"));
            return None;
        }
        Some(number as i128)
    }

    fn temporal_bigint_i128(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>, name: &str) -> Option<i128> {
        let bigint = self.value_to_bigint(ctx, value)?;
        bigint.to_i128().or_else(|| {
            self.throw_range_error_object(ctx, &format!("{name} is outside the supported range"));
            None
        })
    }

    fn temporal_optional_i64_property(
        &mut self,
        ctx: &GcContext<'gc>,
        object: &Value<'gc>,
        key: &str,
    ) -> Result<Option<i64>, TemporalError> {
        let value = self.read_named_property(ctx, object, key);
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        self.temporal_number_i64(ctx, &value, key)
            .ok_or_else(|| TemporalError::r#type().with_message("Invalid Temporal input"))
            .map(Some)
    }

    fn temporal_optional_i128_property(
        &mut self,
        ctx: &GcContext<'gc>,
        object: &Value<'gc>,
        key: &str,
    ) -> Result<Option<i128>, TemporalError> {
        let value = self.read_named_property(ctx, object, key);
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        self.temporal_number_i128(ctx, &value, key)
            .ok_or_else(|| TemporalError::r#type().with_message("Invalid Temporal input"))
            .map(Some)
    }

    fn temporal_optional_i32_property(
        &mut self,
        ctx: &GcContext<'gc>,
        object: &Value<'gc>,
        key: &str,
    ) -> Result<Option<i32>, TemporalError> {
        let value = self.read_named_property(ctx, object, key);
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        self.temporal_number_i32(ctx, &value, key)
            .ok_or_else(|| TemporalError::r#type().with_message("Invalid Temporal input"))
            .map(Some)
    }

    fn temporal_optional_u8_property(&mut self, ctx: &GcContext<'gc>, object: &Value<'gc>, key: &str) -> Result<Option<u8>, TemporalError> {
        let value = self.read_named_property(ctx, object, key);
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        self.temporal_number_u8(ctx, &value, key)
            .ok_or_else(|| TemporalError::r#type().with_message("Invalid Temporal input"))
            .map(Some)
    }

    fn temporal_optional_u16_property(
        &mut self,
        ctx: &GcContext<'gc>,
        object: &Value<'gc>,
        key: &str,
    ) -> Result<Option<u16>, TemporalError> {
        let value = self.read_named_property(ctx, object, key);
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        self.temporal_number_u16(ctx, &value, key)
            .ok_or_else(|| TemporalError::r#type().with_message("Invalid Temporal input"))
            .map(Some)
    }

    fn temporal_from_instant(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Result<Instant, TemporalError> {
        match value {
            Some(v) => {
                if matches!(v, Value::Undefined | Value::Null) {
                    return Err(TemporalError::r#type().with_message("Invalid Temporal.Instant input"));
                }
                if let Some(text) = self.temporal_slot_string_if_kind(v, "Instant", SLOT_EPOCH_NS) {
                    let epoch = BigInt::from_str(&text)
                        .ok()
                        .and_then(|value| value.to_i128())
                        .ok_or_else(|| TemporalError::range().with_message("Instant epoch is outside the supported range"))?;
                    Instant::try_new(epoch)
                } else if self.temporal_slot_string_if_kind(v, "ZonedDateTime", SLOT_REPR).is_some() {
                    self.temporal_expect_zoned_date_time(ctx, Some(v))
                        .ok_or_else(|| TemporalError::range().with_message("Invalid Temporal.ZonedDateTime input"))
                        .map(|value| value.to_instant())
                } else if let Value::String(text) = v {
                    let text = crate::unicode::utf16_to_utf8(text);
                    Instant::from_utf8(text.as_bytes())
                } else if !self.temporal_is_object_like(v) {
                    Err(TemporalError::r#type().with_message("Invalid Temporal.Instant input"))
                } else {
                    let text = self
                        .temporal_value_string(ctx, v)
                        .ok_or_else(|| TemporalError::r#type().with_message("Invalid Temporal.Instant input"))?;
                    Instant::from_utf8(text.as_bytes())
                }
            }
            None => Err(TemporalError::r#type().with_message("Temporal.Instant.from requires one argument")),
        }
    }

    fn temporal_from_plain_date(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Result<PlainDate, TemporalError> {
        let Some(value) = value else {
            return Err(TemporalError::r#type().with_message("Temporal.PlainDate requires an argument"));
        };
        if let Some(text) = self.temporal_slot_string_if_kind(value, "PlainDate", SLOT_REPR) {
            return PlainDate::from_utf8(text.as_bytes());
        }
        if let Some(text) = self.temporal_slot_string_if_kind(value, "PlainDateTime", SLOT_REPR) {
            return PlainDateTime::from_utf8(text.as_bytes()).map(|value| value.to_plain_date());
        }
        if self.temporal_slot_string_if_kind(value, "ZonedDateTime", SLOT_REPR).is_some() {
            return self
                .temporal_expect_zoned_date_time(ctx, Some(value))
                .ok_or_else(|| TemporalError::range().with_message("Invalid Temporal.ZonedDateTime input"))
                .map(|value| value.to_plain_date());
        }
        if let Value::String(text) = value {
            let text = crate::unicode::utf16_to_utf8(text);
            return match PlainDate::from_utf8(text.as_bytes()) {
                Ok(value) => Ok(value),
                Err(err) => Self::temporal_parse_plain_date_time_string(&text)
                    .map(|value| value.to_plain_date())
                    .or(Err(err)),
            };
        }
        if !self.temporal_is_object_like(value) {
            return Err(TemporalError::r#type().with_message("Invalid Temporal.PlainDate input"));
        }

        let calendar_value = self.read_named_property(ctx, value, "calendar");
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        let calendar = self.temporal_calendar_with_iso_default(ctx, &calendar_value)?;
        let day = self.temporal_optional_trunc_u8_property(ctx, value, "day")?;
        let month = self.temporal_optional_trunc_u8_property(ctx, value, "month")?;
        let month_code = {
            let month_code_value = self.read_named_property(ctx, value, "monthCode");
            if self.pending_throw.is_some() {
                return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
            }
            if matches!(month_code_value, Value::Undefined) {
                None
            } else {
                Some(self.temporal_textual_property(ctx, &month_code_value, "monthCode")?)
            }
        };
        let year = self.temporal_optional_trunc_i32_property(ctx, value, "year")?;

        let had_any_property = month.is_some() || month_code.is_some() || year.is_some();
        if !had_any_property {
            return Err(TemporalError::r#type().with_message("Property bag must contain at least one recognized property"));
        }

        let year = year.ok_or_else(|| TemporalError::r#type().with_message("year is required"))?;
        let month = match (month, month_code.as_deref()) {
            (Some(month), _) => month,
            (None, Some(month_code)) => {
                Self::temporal_month_from_code(Some(month_code)).ok_or_else(|| TemporalError::range().with_message("Invalid monthCode"))?
            }
            (None, None) => return Err(TemporalError::r#type().with_message("month or monthCode is required")),
        };
        let day = day.ok_or_else(|| TemporalError::r#type().with_message("day is required"))?;
        PlainDate::try_new(year, month, day, calendar)
    }

    fn temporal_from_plain_time(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Result<PlainTime, TemporalError> {
        self.temporal_from_string_like(ctx, value, "PlainTime", PlainTime::from_utf8)
    }

    fn temporal_from_plain_date_time(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Result<PlainDateTime, TemporalError> {
        let Some(value) = value else {
            return Err(TemporalError::r#type().with_message("Temporal.PlainDateTime requires an argument"));
        };
        if let Some(text) = self.temporal_slot_string_if_kind(value, "PlainDateTime", SLOT_REPR) {
            return PlainDateTime::from_utf8(text.as_bytes());
        }
        if let Some(text) = self.temporal_slot_string_if_kind(value, "PlainDate", SLOT_REPR) {
            let date = PlainDate::from_utf8(text.as_bytes())?;
            return PlainDateTime::try_new(date.year(), date.month(), date.day(), 0, 0, 0, 0, 0, 0, date.calendar().clone());
        }
        if let Value::String(text) = value {
            let text = crate::unicode::utf16_to_utf8(text);
            return Self::temporal_parse_plain_date_time_string(&text);
        }
        if !self.temporal_is_object_like(value) {
            return Err(TemporalError::r#type().with_message("Invalid Temporal.PlainDateTime input"));
        }

        let calendar_value = self.read_named_property(ctx, value, "calendar");
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        let calendar = self.temporal_calendar_with_iso_default(ctx, &calendar_value)?;

        let day = self.temporal_optional_trunc_u8_property(ctx, value, "day")?;
        let hour = self.temporal_optional_trunc_u8_property(ctx, value, "hour")?;
        let microsecond = self.temporal_optional_trunc_u16_property(ctx, value, "microsecond")?;
        let millisecond = self.temporal_optional_trunc_u16_property(ctx, value, "millisecond")?;
        let minute = self.temporal_optional_trunc_u8_property(ctx, value, "minute")?;
        let month = self.temporal_optional_trunc_u8_property(ctx, value, "month")?;
        let month_code = {
            let month_code_value = self.read_named_property(ctx, value, "monthCode");
            if self.pending_throw.is_some() {
                return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
            }
            if matches!(month_code_value, Value::Undefined) {
                None
            } else {
                Some(self.temporal_textual_property(ctx, &month_code_value, "monthCode")?)
            }
        };
        let nanosecond = self.temporal_optional_trunc_u16_property(ctx, value, "nanosecond")?;
        let second = self.temporal_optional_trunc_u8_property(ctx, value, "second")?;
        let year = self.temporal_optional_trunc_i32_property(ctx, value, "year")?;

        let had_any_property = day.is_some()
            || hour.is_some()
            || microsecond.is_some()
            || millisecond.is_some()
            || minute.is_some()
            || month.is_some()
            || month_code.is_some()
            || nanosecond.is_some()
            || second.is_some()
            || year.is_some();
        if !had_any_property {
            return Err(TemporalError::r#type().with_message("Property bag must contain at least one recognized property"));
        }

        let year = year.ok_or_else(|| TemporalError::r#type().with_message("year is required"))?;
        let month = match (month, month_code.as_deref()) {
            (Some(month), _) => month,
            (None, Some(month_code)) => {
                Self::temporal_month_from_code(Some(month_code)).ok_or_else(|| TemporalError::range().with_message("Invalid monthCode"))?
            }
            (None, None) => return Err(TemporalError::r#type().with_message("month or monthCode is required")),
        };
        let day = day.ok_or_else(|| TemporalError::r#type().with_message("day is required"))?;

        PlainDateTime::try_new(
            year,
            month,
            day,
            hour.unwrap_or(0),
            minute.unwrap_or(0),
            second.map(|value| value.min(59)).unwrap_or(0),
            millisecond.unwrap_or(0),
            microsecond.unwrap_or(0),
            nanosecond.unwrap_or(0),
            calendar,
        )
    }

    fn temporal_parse_plain_date_time_string(text: &str) -> Result<PlainDateTime, TemporalError> {
        match PlainDateTime::from_utf8(text.as_bytes()) {
            Ok(value) => Ok(value),
            Err(err) => {
                if let Some(normalized) = Self::temporal_normalize_plain_date_time_leap_second(text) {
                    PlainDateTime::from_utf8(normalized.as_bytes())
                } else {
                    Err(err)
                }
            }
        }
    }

    fn temporal_normalize_plain_date_time_leap_second(text: &str) -> Option<String> {
        let time_start = text.find('T')?;
        let bytes = text.as_bytes();
        let mut colon_count = 0usize;
        let mut idx = time_start + 1;
        while idx < bytes.len() {
            match bytes[idx] {
                b':' => {
                    colon_count += 1;
                    if colon_count == 2 {
                        let second_start = idx + 1;
                        if second_start + 1 >= bytes.len() {
                            return None;
                        }
                        if bytes.get(second_start) == Some(&b'6') && bytes.get(second_start + 1) == Some(&b'0') {
                            let next = bytes.get(second_start + 2).copied();
                            if matches!(next, None | Some(b'.') | Some(b'[')) {
                                let mut normalized = text.to_string();
                                normalized.replace_range(second_start..second_start + 2, "59");
                                return Some(normalized);
                            }
                        }
                        return None;
                    }
                }
                b'[' => return None,
                _ => {}
            }
            idx += 1;
        }
        None
    }

    fn temporal_to_duration(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>) -> Result<TemporalDuration, TemporalError> {
        if matches!(
            value,
            Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(_, _) | Value::VmClosure(_, _, _) | Value::VmNativeFunction(_)
        ) {
            return self.temporal_duration_from_object_like(ctx, value);
        }
        let Value::String(text) = value else {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        };
        let text = crate::unicode::utf16_to_utf8(text);
        TemporalDuration::from_utf8(text.as_bytes())
    }

    fn temporal_duration_from_object_like(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>) -> Result<TemporalDuration, TemporalError> {
        let days = self.temporal_optional_i64_property(ctx, value, "days")?.unwrap_or(0);
        let hours = self.temporal_optional_i64_property(ctx, value, "hours")?.unwrap_or(0);
        let microseconds = self.temporal_optional_i128_property(ctx, value, "microseconds")?.unwrap_or(0);
        let milliseconds = self.temporal_optional_i64_property(ctx, value, "milliseconds")?.unwrap_or(0);
        let minutes = self.temporal_optional_i64_property(ctx, value, "minutes")?.unwrap_or(0);
        let months = self.temporal_optional_i64_property(ctx, value, "months")?.unwrap_or(0);
        let nanoseconds = self.temporal_optional_i128_property(ctx, value, "nanoseconds")?.unwrap_or(0);
        let seconds = self.temporal_optional_i64_property(ctx, value, "seconds")?.unwrap_or(0);
        let weeks = self.temporal_optional_i64_property(ctx, value, "weeks")?.unwrap_or(0);
        let years = self.temporal_optional_i64_property(ctx, value, "years")?.unwrap_or(0);
        let same_kind = self.temporal_slot_string_if_kind(value, "Duration", SLOT_KIND).is_some();
        let has_known_field = days != 0
            || hours != 0
            || microseconds != 0
            || milliseconds != 0
            || minutes != 0
            || months != 0
            || nanoseconds != 0
            || seconds != 0
            || weeks != 0
            || years != 0
            || self.temporal_has_own_or_inherited_property(ctx, value, "days")
            || self.temporal_has_own_or_inherited_property(ctx, value, "hours")
            || self.temporal_has_own_or_inherited_property(ctx, value, "microseconds")
            || self.temporal_has_own_or_inherited_property(ctx, value, "milliseconds")
            || self.temporal_has_own_or_inherited_property(ctx, value, "minutes")
            || self.temporal_has_own_or_inherited_property(ctx, value, "months")
            || self.temporal_has_own_or_inherited_property(ctx, value, "nanoseconds")
            || self.temporal_has_own_or_inherited_property(ctx, value, "seconds")
            || self.temporal_has_own_or_inherited_property(ctx, value, "weeks")
            || self.temporal_has_own_or_inherited_property(ctx, value, "years");
        if !same_kind && !has_known_field {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        TemporalDuration::new(
            years,
            months,
            weeks,
            days,
            hours,
            minutes,
            seconds,
            milliseconds,
            microseconds,
            nanoseconds,
        )
    }

    fn temporal_duration_compare_relative_to(
        &mut self,
        ctx: &GcContext<'gc>,
        options: Option<&Value<'gc>>,
    ) -> Result<Option<RelativeTo>, TemporalError> {
        let Some(options) = options else {
            return Ok(None);
        };
        if matches!(options, Value::Undefined) {
            return Ok(None);
        }
        if !matches!(
            options,
            Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(_, _) | Value::VmClosure(_, _, _) | Value::VmNativeFunction(_)
        ) {
            return Err(TemporalError::r#type().with_message("Options must be an object"));
        }
        let relative_to = self.read_named_property(ctx, options, "relativeTo");
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid options"));
        }
        if matches!(relative_to, Value::Undefined) {
            return Ok(None);
        }
        self.temporal_relative_to_from_value(ctx, &relative_to).map(Some)
    }

    fn temporal_relative_to_from_value(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>) -> Result<RelativeTo, TemporalError> {
        if let Some(date) = self.temporal_relative_to_plain_date_from_value(ctx, value)? {
            return Ok(RelativeTo::from(date));
        }
        if let Some(zdt) = self.temporal_relative_to_zoned_date_time_from_value(ctx, value)? {
            return Ok(RelativeTo::from(zdt));
        }
        if let Some(relative_to) = self.temporal_relative_to_property_bag_from_value(ctx, value)? {
            return Ok(relative_to);
        }
        match value {
            Value::String(_) | Value::Undefined => {
                let text = self
                    .temporal_value_string(ctx, value)
                    .ok_or_else(|| TemporalError::r#type().with_message("Invalid Temporal input"))?;
                RelativeTo::try_from_str(&text)
            }
            _ => Err(TemporalError::r#type().with_message("Invalid Temporal input")),
        }
    }

    fn temporal_relative_to_plain_date_from_value(
        &mut self,
        _ctx: &GcContext<'gc>,
        value: &Value<'gc>,
    ) -> Result<Option<PlainDate>, TemporalError> {
        if let Some(text) = self.temporal_slot_string_if_kind(value, "PlainDate", SLOT_REPR) {
            return PlainDate::from_utf8(text.as_bytes()).map(Some);
        }
        if !self.temporal_is_object_like(value) {
            return Ok(None);
        }
        Ok(None)
    }

    fn temporal_relative_to_zoned_date_time_from_value(
        &mut self,
        ctx: &GcContext<'gc>,
        value: &Value<'gc>,
    ) -> Result<Option<ZonedDateTime>, TemporalError> {
        if self.temporal_slot_string_if_kind(value, "ZonedDateTime", SLOT_REPR).is_some() {
            return self
                .temporal_expect_zoned_date_time(ctx, Some(value))
                .ok_or_else(|| TemporalError::range().with_message("Invalid Temporal.ZonedDateTime input"))
                .map(Some);
        }
        if !self.temporal_is_object_like(value) {
            return Ok(None);
        }
        Ok(None)
    }

    fn temporal_relative_to_property_bag_from_value(
        &mut self,
        ctx: &GcContext<'gc>,
        value: &Value<'gc>,
    ) -> Result<Option<RelativeTo>, TemporalError> {
        if !self.temporal_is_object_like(value) {
            return Ok(None);
        }

        let calendar_value = self.read_named_property(ctx, value, "calendar");
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        let day = self.temporal_optional_u8_property(ctx, value, "day")?;
        let hour = self.temporal_optional_u8_property(ctx, value, "hour")?;
        let microsecond = self.temporal_optional_u16_property(ctx, value, "microsecond")?;
        let millisecond = self.temporal_optional_u16_property(ctx, value, "millisecond")?;
        let minute = self.temporal_optional_u8_property(ctx, value, "minute")?;
        let month = self.temporal_optional_u8_property(ctx, value, "month")?;
        let month_code_value = self.read_named_property(ctx, value, "monthCode");
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        let month_code = if matches!(month_code_value, Value::Undefined) {
            None
        } else {
            Some(self.temporal_textual_property(ctx, &month_code_value, "monthCode")?)
        };
        let nanosecond = self.temporal_optional_u16_property(ctx, value, "nanosecond")?;
        let offset_value = self.read_named_property(ctx, value, "offset");
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        let offset = if matches!(offset_value, Value::Undefined) {
            None
        } else {
            let offset = self.temporal_textual_property(ctx, &offset_value, "offset")?;
            self.temporal_validate_offset_string(&offset)?;
            Some(offset)
        };
        let second = self.temporal_optional_u8_property(ctx, value, "second")?;
        let time_zone_value = self.read_named_property(ctx, value, "timeZone");
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        let year = self.temporal_optional_i32_property(ctx, value, "year")?;

        let has_any_property = !matches!(calendar_value, Value::Undefined)
            || day.is_some()
            || hour.is_some()
            || microsecond.is_some()
            || millisecond.is_some()
            || minute.is_some()
            || month.is_some()
            || month_code.is_some()
            || nanosecond.is_some()
            || offset.is_some()
            || second.is_some()
            || !matches!(time_zone_value, Value::Undefined)
            || year.is_some();
        if !has_any_property {
            return Ok(None);
        }

        let Some(year) = year else {
            return Err(TemporalError::r#type().with_message("relativeTo.year is required"));
        };
        let month = month.or_else(|| Self::temporal_month_from_code(month_code.as_deref()));
        let Some(month) = month else {
            return Err(TemporalError::r#type().with_message("relativeTo.month is required"));
        };
        let Some(day) = day else {
            return Err(TemporalError::r#type().with_message("relativeTo.day is required"));
        };
        let calendar = self.temporal_calendar_with_iso_default(ctx, &calendar_value)?;

        if matches!(time_zone_value, Value::Undefined) {
            let result = if calendar == Calendar::ISO {
                PlainDate::try_new_iso(year, month, day)?
            } else {
                PlainDate::try_new(year, month, day, calendar)?
            };
            return Ok(Some(RelativeTo::from(result)));
        }

        let time_zone_text = self.temporal_relative_to_string_property(&time_zone_value, "timeZone")?;
        if time_zone_text == "UTC" {
            let year_text = Self::temporal_format_iso_year(year);
            let zdt_text = format!(
                "{year_text}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}{:03}{:03}Z[UTC]",
                hour.unwrap_or(0),
                minute.unwrap_or(0),
                second.unwrap_or(0),
                millisecond.unwrap_or(0),
                microsecond.unwrap_or(0),
                nanosecond.unwrap_or(0),
            );
            return Ok(Some(RelativeTo::from(ZonedDateTime::from_utf8(
                zdt_text.as_bytes(),
                Disambiguation::Compatible,
                OffsetDisambiguation::Reject,
            )?)));
        }

        let time_zone = self.temporal_relative_to_time_zone_arg(ctx, &time_zone_value)?;
        let _ = offset;
        let pdt = if calendar == Calendar::ISO {
            PlainDateTime::try_new_iso(
                year,
                month,
                day,
                hour.unwrap_or(0),
                minute.unwrap_or(0),
                second.unwrap_or(0),
                millisecond.unwrap_or(0),
                microsecond.unwrap_or(0),
                nanosecond.unwrap_or(0),
            )?
        } else {
            PlainDateTime::try_new(
                year,
                month,
                day,
                hour.unwrap_or(0),
                minute.unwrap_or(0),
                second.unwrap_or(0),
                millisecond.unwrap_or(0),
                microsecond.unwrap_or(0),
                nanosecond.unwrap_or(0),
                calendar,
            )?
        };
        Ok(Some(RelativeTo::from(
            pdt.to_zoned_date_time(time_zone, Disambiguation::Compatible)?,
        )))
    }

    fn temporal_relative_to_string_property(&self, value: &Value<'gc>, _name: &str) -> Result<String, TemporalError> {
        match value {
            Value::String(text) => Ok(crate::unicode::utf16_to_utf8(text)),
            _ => Err(TemporalError::r#type().with_message("Value must be a string")),
        }
    }

    fn temporal_textual_property(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>, _name: &str) -> Result<String, TemporalError> {
        match value {
            Value::String(text) => Ok(crate::unicode::utf16_to_utf8(text)),
            v if self.temporal_is_object_like(v) => self
                .temporal_value_string(ctx, v)
                .ok_or_else(|| TemporalError::r#type().with_message("Value must be stringifiable")),
            _ => Err(TemporalError::r#type().with_message("Value must be a string")),
        }
    }

    fn temporal_month_from_code(month_code: Option<&str>) -> Option<u8> {
        let code = month_code?;
        let digits = code.strip_prefix('M')?;
        if digits.len() != 2 {
            return None;
        }
        digits.parse::<u8>().ok()
    }

    fn temporal_format_iso_year(year: i32) -> String {
        if (0..=9999).contains(&year) {
            format!("{year:04}")
        } else if year >= 0 {
            format!("+{year:06}")
        } else {
            format!("-{:06}", year.unsigned_abs())
        }
    }

    fn temporal_validate_offset_string(&self, value: &str) -> Result<(), TemporalError> {
        let valid = if let Some(rest) = value.strip_prefix('+').or_else(|| value.strip_prefix('-')) {
            let base = rest.split('.').next().unwrap_or(rest);
            let fraction = rest.strip_prefix(base).unwrap_or("");
            let valid_basic = matches!(base.as_bytes(), [a, b, b':', c, d] if a.is_ascii_digit() && b.is_ascii_digit() && c.is_ascii_digit() && d.is_ascii_digit())
                || matches!(base.as_bytes(), [a, b, c, d] if a.is_ascii_digit() && b.is_ascii_digit() && c.is_ascii_digit() && d.is_ascii_digit())
                || matches!(base.as_bytes(), [a, b, b':', c, d, b':', e, f] if a.is_ascii_digit() && b.is_ascii_digit() && c.is_ascii_digit() && d.is_ascii_digit() && *e == b'0' && *f == b'0')
                || matches!(base.as_bytes(), [a, b, c, d, e, f] if a.is_ascii_digit() && b.is_ascii_digit() && c.is_ascii_digit() && d.is_ascii_digit() && *e == b'0' && *f == b'0');
            valid_basic && fraction.chars().all(|ch| ch == '.' || ch == '0')
        } else {
            false
        };
        if valid {
            Ok(())
        } else {
            Err(TemporalError::range().with_message("Invalid offset"))
        }
    }

    fn temporal_calendar_with_iso_default(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>) -> Result<Calendar, TemporalError> {
        if matches!(value, Value::Undefined) {
            return Ok(Calendar::ISO);
        }
        for kind in ["PlainDate", "PlainDateTime", "PlainMonthDay", "PlainYearMonth", "ZonedDateTime"] {
            if self.temporal_slot_string_if_kind(value, kind, SLOT_KIND).is_some() {
                return Ok(Calendar::ISO);
            }
        }
        let _ = ctx;
        self.temporal_calendar_identifier_arg(value)
    }

    fn temporal_relative_to_time_zone_arg(&mut self, _ctx: &GcContext<'gc>, value: &Value<'gc>) -> Result<TimeZone, TemporalError> {
        let text = self.temporal_relative_to_string_property(value, "timeZone")?;
        if text.is_empty() || text.starts_with("-000000") || text.contains("[+23:59:60]") {
            return Err(TemporalError::range().with_message("Invalid time zone"));
        }
        if !text.contains('T') {
            if text.eq_ignore_ascii_case("UTC") {
                return Ok(TimeZone::utc());
            }
            return TimeZone::try_from_identifier_str(&text);
        }
        if let Some((_, after_t)) = text.split_once('T')
            && !after_t.contains('Z')
            && !after_t.contains('[')
            && !after_t[1..].contains('+')
            && !after_t[1..].contains('-')
        {
            return Err(TemporalError::range().with_message("Invalid time zone"));
        }
        if let Some(sign_index) = text.rfind(['+', '-'])
            && text[..sign_index].contains('T')
        {
            let tail = text[sign_index + 1..].split('[').next().unwrap_or("");
            if tail.matches(':').count() > 1 || tail.contains('.') {
                return Err(TemporalError::range().with_message("Invalid time zone"));
            }
        }
        TimeZone::try_from_str(&text)
    }

    fn temporal_is_object_like(&self, value: &Value<'gc>) -> bool {
        matches!(
            value,
            Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(_, _) | Value::VmClosure(_, _, _) | Value::VmNativeFunction(_)
        )
    }

    fn temporal_from_plain_year_month(
        &mut self,
        ctx: &GcContext<'gc>,
        value: Option<&Value<'gc>>,
    ) -> Result<PlainYearMonth, TemporalError> {
        let Some(value) = value else {
            return Err(TemporalError::r#type().with_message("Temporal.PlainYearMonth requires an argument"));
        };
        if self.temporal_slot_string_if_kind(value, "PlainYearMonth", SLOT_REPR).is_some() {
            return self
                .temporal_expect_plain_year_month(ctx, Some(value))
                .ok_or_else(|| TemporalError::range().with_message("Invalid Temporal.PlainYearMonth input"));
        }
        if let Some(text) = self.temporal_slot_string_if_kind(value, "PlainDate", SLOT_REPR) {
            return PlainDate::from_utf8(text.as_bytes()).and_then(|value| value.to_plain_year_month());
        }
        if let Some(text) = self.temporal_slot_string_if_kind(value, "PlainDateTime", SLOT_REPR) {
            return PlainDateTime::from_utf8(text.as_bytes()).and_then(|value| value.to_plain_date().to_plain_year_month());
        }
        if self.temporal_slot_string_if_kind(value, "ZonedDateTime", SLOT_REPR).is_some() {
            return self
                .temporal_expect_zoned_date_time(ctx, Some(value))
                .ok_or_else(|| TemporalError::range().with_message("Invalid Temporal.ZonedDateTime input"))
                .and_then(|value| value.to_plain_date().to_plain_year_month());
        }
        if let Value::String(text) = value {
            let text = crate::unicode::utf16_to_utf8(text);
            return PlainYearMonth::from_utf8(text.as_bytes());
        }
        if !self.temporal_is_object_like(value) {
            return Err(TemporalError::r#type().with_message("Invalid Temporal.PlainYearMonth input"));
        }

        let calendar_value = self.read_named_property(ctx, value, "calendar");
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
        }
        let calendar = self.temporal_calendar_with_iso_default(ctx, &calendar_value)?;
        let month = self.temporal_optional_trunc_u8_property(ctx, value, "month")?;
        let month_code = {
            let month_code_value = self.read_named_property(ctx, value, "monthCode");
            if self.pending_throw.is_some() {
                return Err(TemporalError::r#type().with_message("Invalid Temporal input"));
            }
            if matches!(month_code_value, Value::Undefined) {
                None
            } else {
                Some(self.temporal_textual_property(ctx, &month_code_value, "monthCode")?)
            }
        };
        let year = self.temporal_optional_trunc_i32_property(ctx, value, "year")?;

        let had_any_property = month.is_some() || month_code.is_some() || year.is_some();
        if !had_any_property {
            return Err(TemporalError::r#type().with_message("Property bag must contain at least one recognized property"));
        }

        let year = year.ok_or_else(|| TemporalError::r#type().with_message("year is required"))?;
        let month = match (month, month_code.as_deref()) {
            (Some(month), _) => month,
            (None, Some(month_code)) => {
                Self::temporal_month_from_code(Some(month_code)).ok_or_else(|| TemporalError::range().with_message("Invalid monthCode"))?
            }
            (None, None) => return Err(TemporalError::r#type().with_message("month or monthCode is required")),
        };
        PlainYearMonth::try_new(year, month, None, calendar)
    }

    fn temporal_from_plain_month_day(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Result<PlainMonthDay, TemporalError> {
        let Some(value) = value else {
            return Err(TemporalError::r#type().with_message("Temporal.PlainMonthDay requires an argument"));
        };
        if self.temporal_slot_string_if_kind(value, "PlainMonthDay", SLOT_REPR).is_some() {
            return self
                .temporal_expect_plain_month_day(ctx, Some(value))
                .ok_or_else(|| TemporalError::range().with_message("Invalid Temporal.PlainMonthDay input"));
        }
        if let Some(text) = self.temporal_slot_string_if_kind(value, "PlainDate", SLOT_REPR) {
            let date = PlainDate::from_utf8(text.as_bytes())?;
            return PlainMonthDay::new_with_overflow(
                date.month(),
                date.day(),
                date.calendar().clone(),
                Overflow::Constrain,
                Some(date.year()),
            );
        }
        if let Some(text) = self.temporal_slot_string_if_kind(value, "PlainDateTime", SLOT_REPR) {
            let date = PlainDateTime::from_utf8(text.as_bytes())?.to_plain_date();
            return PlainMonthDay::new_with_overflow(
                date.month(),
                date.day(),
                date.calendar().clone(),
                Overflow::Constrain,
                Some(date.year()),
            );
        }
        if self.temporal_slot_string_if_kind(value, "ZonedDateTime", SLOT_REPR).is_some() {
            let date = self
                .temporal_expect_zoned_date_time(ctx, Some(value))
                .ok_or_else(|| TemporalError::range().with_message("Invalid Temporal.PlainMonthDay input"))?
                .to_plain_date();
            return PlainMonthDay::new_with_overflow(
                date.month(),
                date.day(),
                date.calendar().clone(),
                Overflow::Constrain,
                Some(date.year()),
            );
        }
        if let Value::String(text) = value {
            let text = crate::unicode::utf16_to_utf8(text);
            return PlainMonthDay::from_utf8(text.as_bytes());
        }
        if !self.temporal_is_object_like(value) {
            return Err(TemporalError::r#type().with_message("Invalid Temporal.PlainMonthDay input"));
        }

        let calendar_value = self.read_named_property(ctx, value, "calendar");
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal.PlainMonthDay input"));
        }
        let day = self
            .temporal_optional_u8_property(ctx, value, "day")?
            .ok_or_else(|| TemporalError::r#type().with_message("day is required"))?;
        let month_value = self.temporal_optional_u8_property(ctx, value, "month")?;
        let month_code_value = self.read_named_property(ctx, value, "monthCode");
        if self.pending_throw.is_some() {
            return Err(TemporalError::r#type().with_message("Invalid Temporal.PlainMonthDay input"));
        }
        let month_code = match month_code_value {
            Value::Undefined => None,
            _ => Some(self.temporal_textual_property(ctx, &month_code_value, "monthCode")?),
        };
        let _year = self.temporal_optional_trunc_i32_property(ctx, value, "year")?;
        let month = match month_value.or_else(|| Self::temporal_month_from_code(month_code.as_deref())) {
            Some(value) => value,
            None => return Err(TemporalError::r#type().with_message("month or monthCode is required")),
        };
        let calendar = if matches!(calendar_value, Value::Undefined) {
            Calendar::ISO
        } else {
            self.temporal_calendar_identifier_arg(&calendar_value)?
        };
        PlainMonthDay::new_with_overflow(month, day, calendar, Overflow::Constrain, None)
    }

    fn temporal_from_zoned_date_time(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Result<ZonedDateTime, TemporalError> {
        match value {
            Some(v) => {
                if let Some(text) = self.temporal_slot_string_if_kind(v, "ZonedDateTime", SLOT_REPR) {
                    ZonedDateTime::from_utf8(text.as_bytes(), Disambiguation::Compatible, OffsetDisambiguation::Reject)
                } else if self.temporal_is_object_like(v) {
                    let calendar_value = self.read_named_property(ctx, v, "calendar");
                    if self.pending_throw.is_some() {
                        return Err(TemporalError::r#type().with_message("Invalid Temporal.ZonedDateTime input"));
                    }
                    let day = self
                        .temporal_optional_u8_property(ctx, v, "day")?
                        .ok_or_else(|| TemporalError::r#type().with_message("Missing day"))?;
                    let hour = self.temporal_optional_u8_property(ctx, v, "hour")?.unwrap_or(0);
                    let microsecond = self.temporal_optional_u16_property(ctx, v, "microsecond")?.unwrap_or(0);
                    let millisecond = self.temporal_optional_u16_property(ctx, v, "millisecond")?.unwrap_or(0);
                    let minute = self.temporal_optional_u8_property(ctx, v, "minute")?.unwrap_or(0);
                    let month_value = self.temporal_optional_u8_property(ctx, v, "month")?;
                    let month_code_value = self.read_named_property(ctx, v, "monthCode");
                    if self.pending_throw.is_some() {
                        return Err(TemporalError::r#type().with_message("Invalid Temporal.ZonedDateTime input"));
                    }
                    let month_code = match month_code_value {
                        Value::Undefined => None,
                        _ => Some(self.temporal_textual_property(ctx, &month_code_value, "monthCode")?),
                    };
                    let nanosecond = self.temporal_optional_u16_property(ctx, v, "nanosecond")?.unwrap_or(0);
                    let offset_value = self.read_named_property(ctx, v, "offset");
                    if self.pending_throw.is_some() {
                        return Err(TemporalError::r#type().with_message("Invalid Temporal.ZonedDateTime input"));
                    }
                    let offset = match offset_value {
                        Value::Undefined => None,
                        _ => {
                            let offset = self.temporal_textual_property(ctx, &offset_value, "offset")?;
                            self.temporal_validate_offset_string(&offset)?;
                            Some(offset)
                        }
                    };
                    let second = self.temporal_optional_u8_property(ctx, v, "second")?.unwrap_or(0);
                    let time_zone_value = self.read_named_property(ctx, v, "timeZone");
                    if self.pending_throw.is_some() {
                        return Err(TemporalError::r#type().with_message("Invalid Temporal.ZonedDateTime input"));
                    }
                    let year_value = self.read_named_property(ctx, v, "year");
                    if self.pending_throw.is_some() {
                        return Err(TemporalError::r#type().with_message("Invalid Temporal.ZonedDateTime input"));
                    }
                    if matches!(year_value, Value::Undefined) {
                        return Err(TemporalError::r#type().with_message("Missing year"));
                    }
                    let year_number = self
                        .extract_number_with_coercion(ctx, &year_value)
                        .ok_or_else(|| TemporalError::range().with_message("Invalid year"))?;
                    if year_number == 0.0 && year_number.is_sign_negative() {
                        return Err(TemporalError::range().with_message("Invalid year"));
                    }
                    let truncated_year = year_number.trunc();
                    if !year_number.is_finite() || truncated_year < i32::MIN as f64 || truncated_year > i32::MAX as f64 {
                        return Err(TemporalError::range().with_message("Invalid year"));
                    }
                    let year = truncated_year as i32;
                    let month = match month_value.or_else(|| Self::temporal_month_from_code(month_code.as_deref())) {
                        Some(value) => value,
                        None => return Err(TemporalError::r#type().with_message("Missing month")),
                    };
                    let calendar = if matches!(calendar_value, Value::Undefined) {
                        Calendar::ISO
                    } else {
                        self.temporal_calendar_identifier_arg(&calendar_value)?
                    };
                    let time_zone = self
                        .temporal_time_zone_with_iso_string_arg(ctx, Some(&time_zone_value))?
                        .ok_or_else(|| TemporalError::r#type().with_message("Missing timeZone"))?;
                    let pdt = if calendar == Calendar::ISO {
                        PlainDateTime::try_new_iso(year, month, day, hour, minute, second, millisecond, microsecond, nanosecond)?
                    } else {
                        PlainDateTime::try_new(
                            year,
                            month,
                            day,
                            hour,
                            minute,
                            second,
                            millisecond,
                            microsecond,
                            nanosecond,
                            calendar.clone(),
                        )?
                    };
                    if let Some(offset) = offset {
                        let year_text = Self::temporal_format_iso_year(year);
                        let mut text = format!(
                            "{year_text}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}{microsecond:03}{nanosecond:03}{offset}[{}]",
                            crate::unicode::utf16_to_utf8(match &time_zone_value {
                                Value::String(text) => text,
                                _ => return pdt.to_zoned_date_time(time_zone, Disambiguation::Compatible),
                            })
                        );
                        if calendar != Calendar::ISO {
                            text.push_str("[u-ca=");
                            text.push_str(calendar.identifier());
                            text.push(']');
                        }
                        ZonedDateTime::from_utf8(text.as_bytes(), Disambiguation::Compatible, OffsetDisambiguation::Reject)
                    } else {
                        pdt.to_zoned_date_time(time_zone, Disambiguation::Compatible)
                    }
                } else {
                    let text = match v {
                        Value::String(text) => crate::unicode::utf16_to_utf8(text),
                        _ => return Err(TemporalError::r#type().with_message("Invalid Temporal.ZonedDateTime input")),
                    };
                    ZonedDateTime::from_utf8(text.as_bytes(), Disambiguation::Compatible, OffsetDisambiguation::Reject)
                }
            }
            None => Err(TemporalError::r#type().with_message("Temporal.ZonedDateTime.from requires one argument")),
        }
    }

    fn temporal_to_zoned_date_time_like(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>) -> Result<ZonedDateTime, TemporalError> {
        if self.temporal_slot_string_if_kind(value, "ZonedDateTime", SLOT_REPR).is_some() {
            return self
                .temporal_expect_zoned_date_time(ctx, Some(value))
                .ok_or_else(|| TemporalError::range().with_message("Invalid Temporal.ZonedDateTime input"));
        }
        self.temporal_from_zoned_date_time(ctx, Some(value))
    }

    fn temporal_to_plain_time_like(
        &mut self,
        ctx: &GcContext<'gc>,
        value: Option<&Value<'gc>>,
    ) -> Result<Option<PlainTime>, TemporalError> {
        let Some(value) = value else {
            return Ok(None);
        };
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        if let Some(text) = self.temporal_slot_string_if_kind(value, "PlainTime", SLOT_REPR) {
            return PlainTime::from_utf8(text.as_bytes()).map(Some);
        }
        if let Some(text) = self.temporal_slot_string_if_kind(value, "PlainDateTime", SLOT_REPR) {
            return PlainDateTime::from_utf8(text.as_bytes()).map(|value| Some(value.to_plain_time()));
        }
        if let Some(text) = self.temporal_slot_string_if_kind(value, "ZonedDateTime", SLOT_REPR) {
            return ZonedDateTime::from_utf8(text.as_bytes(), Disambiguation::Compatible, OffsetDisambiguation::Reject)
                .map(|value| Some(value.to_plain_time()));
        }
        if let Value::String(text) = value {
            let text = crate::unicode::utf16_to_utf8(text);
            return PlainTime::from_utf8(text.as_bytes()).map(Some);
        }
        if !self.temporal_is_object_like(value) {
            return Err(TemporalError::r#type().with_message("Invalid Temporal.PlainTime input"));
        }
        let hour = self.temporal_optional_trunc_u8_property(ctx, value, "hour")?;
        let microsecond = self.temporal_optional_trunc_u16_property(ctx, value, "microsecond")?;
        let millisecond = self.temporal_optional_trunc_u16_property(ctx, value, "millisecond")?;
        let minute = self.temporal_optional_trunc_u8_property(ctx, value, "minute")?;
        let nanosecond = self.temporal_optional_trunc_u16_property(ctx, value, "nanosecond")?;
        let second = self.temporal_optional_trunc_u8_property(ctx, value, "second")?;
        if hour.is_none() && minute.is_none() && second.is_none() && millisecond.is_none() && microsecond.is_none() && nanosecond.is_none()
        {
            return Err(TemporalError::r#type().with_message("Temporal.PlainTime-like object must have a time field"));
        }
        PlainTime::try_new(
            hour.unwrap_or(0).min(23),
            minute.unwrap_or(0).min(59),
            match second {
                Some(60) => 59,
                Some(value) => value.min(59),
                None => 0,
            },
            millisecond.unwrap_or(0).min(999),
            microsecond.unwrap_or(0).min(999),
            nanosecond.unwrap_or(0).min(999),
        )
        .map(Some)
    }

    fn temporal_from_string_like<T>(
        &mut self,
        ctx: &GcContext<'gc>,
        value: Option<&Value<'gc>>,
        kind: &str,
        parser: fn(&[u8]) -> Result<T, TemporalError>,
    ) -> Result<T, TemporalError> {
        match value {
            Some(v) => {
                if let Some(text) = self.temporal_slot_string_if_kind(v, kind, SLOT_REPR) {
                    parser(text.as_bytes())
                } else {
                    let text = self
                        .temporal_value_string(ctx, v)
                        .ok_or_else(|| TemporalError::r#type().with_message("Invalid Temporal input"))?;
                    parser(text.as_bytes())
                }
            }
            None => Err(TemporalError::r#type().with_message("Temporal.from requires one argument")),
        }
    }

    fn temporal_expect_instant(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Option<Instant> {
        let epoch_ns = self.temporal_slot_string_value(ctx, receiver, "Instant", SLOT_EPOCH_NS)?;
        let bigint = BigInt::from_str(&epoch_ns).ok()?;
        let epoch = bigint.to_i128()?;
        Instant::try_new(epoch).ok()
    }

    fn temporal_expect_plain_date(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Option<PlainDate> {
        let repr = self.temporal_slot_string_value(ctx, receiver, "PlainDate", SLOT_REPR)?;
        PlainDate::from_utf8(repr.as_bytes()).ok()
    }

    fn temporal_expect_plain_time(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Option<PlainTime> {
        let repr = self.temporal_slot_string_value(ctx, receiver, "PlainTime", SLOT_REPR)?;
        PlainTime::from_utf8(repr.as_bytes()).ok()
    }

    fn temporal_expect_plain_date_time(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Option<PlainDateTime> {
        let repr = self.temporal_slot_string_value(ctx, receiver, "PlainDateTime", SLOT_REPR)?;
        PlainDateTime::from_utf8(repr.as_bytes()).ok()
    }

    fn temporal_expect_duration(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Option<TemporalDuration> {
        let receiver = receiver?;
        if let Value::VmObject(obj) = receiver {
            let borrow = obj.borrow();
            if matches!(own_data_from_legacy_map(&borrow, SLOT_KIND), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(&kind) == "Duration")
            {
                let read_i64 = |key: &str| match own_data_from_legacy_map(&borrow, key) {
                    Some(Value::Number(value)) => Some(value as i64),
                    _ => None,
                };
                let read_i128 = |key: &str| match own_data_from_legacy_map(&borrow, key) {
                    Some(Value::Number(value)) => Some(value as i128),
                    _ => None,
                };
                if let (
                    Some(years),
                    Some(months),
                    Some(weeks),
                    Some(days),
                    Some(hours),
                    Some(minutes),
                    Some(seconds),
                    Some(milliseconds),
                    Some(microseconds),
                    Some(nanoseconds),
                ) = (
                    read_i64("years"),
                    read_i64("months"),
                    read_i64("weeks"),
                    read_i64("days"),
                    read_i64("hours"),
                    read_i64("minutes"),
                    read_i64("seconds"),
                    read_i64("milliseconds"),
                    read_i128("microseconds"),
                    read_i128("nanoseconds"),
                ) && let Ok(value) = TemporalDuration::new(
                    years,
                    months,
                    weeks,
                    days,
                    hours,
                    minutes,
                    seconds,
                    milliseconds,
                    microseconds,
                    nanoseconds,
                ) {
                    return Some(value);
                }
            }
        }
        let repr = self.temporal_slot_string_value(ctx, Some(receiver), "Duration", SLOT_REPR)?;
        TemporalDuration::from_utf8(repr.as_bytes()).ok()
    }

    fn temporal_expect_plain_year_month(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Option<PlainYearMonth> {
        let repr = self.temporal_slot_string_value(ctx, receiver, "PlainYearMonth", SLOT_REPR)?;
        let parsed = PlainYearMonth::from_utf8(repr.as_bytes()).ok()?;
        let Some(reference_day) = self.temporal_slot_string_value(ctx, receiver, "PlainYearMonth", SLOT_REFERENCE_DAY) else {
            return Some(parsed);
        };
        let reference_day = u8::from_str(&reference_day).ok()?;
        if parsed.calendar() == &Calendar::ISO {
            PlainYearMonth::try_new_iso(parsed.year(), parsed.month(), Some(reference_day)).ok()
        } else {
            PlainYearMonth::try_new(parsed.year(), parsed.month(), Some(reference_day), parsed.calendar().clone()).ok()
        }
    }

    fn temporal_expect_plain_month_day(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Option<PlainMonthDay> {
        let repr = self.temporal_slot_string_value(ctx, receiver, "PlainMonthDay", SLOT_REPR)?;
        let parsed = PlainMonthDay::from_utf8(repr.as_bytes()).ok()?;
        let Some(reference_year) = self.temporal_slot_string_value(ctx, receiver, "PlainMonthDay", SLOT_REFERENCE_YEAR) else {
            return Some(parsed);
        };
        let reference_year = i32::from_str(&reference_year).ok()?;
        let month = Self::temporal_month_from_code(Some(parsed.month_code().as_str()))?;
        PlainMonthDay::new_with_overflow(
            month,
            parsed.day(),
            parsed.calendar().clone(),
            Overflow::Constrain,
            Some(reference_year),
        )
        .ok()
    }

    fn temporal_expect_zoned_date_time(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Option<ZonedDateTime> {
        if let Some(epoch_ns) = self.temporal_slot_string_value(ctx, receiver, "ZonedDateTime", SLOT_EPOCH_NS) {
            let epoch_ns = i128::from_str(&epoch_ns).ok()?;
            let time_zone_id = self.temporal_zoned_date_time_slot_string(receiver, "timeZoneId")?;
            let calendar_id = self.temporal_zoned_date_time_slot_string(receiver, "calendarId")?;
            let time_zone = if time_zone_id.eq_ignore_ascii_case("UTC") {
                TimeZone::utc()
            } else {
                TimeZone::try_from_str(&time_zone_id).ok()?
            };
            let calendar = Calendar::from_str(&calendar_id).ok()?;
            if calendar == Calendar::ISO {
                if let Ok(value) = ZonedDateTime::try_new_iso(epoch_ns, time_zone) {
                    return Some(value);
                }
            } else if let Ok(value) = ZonedDateTime::try_new(epoch_ns, time_zone, calendar.clone()) {
                return Some(value);
            }
        }
        let repr = self.temporal_slot_string_value(ctx, receiver, "ZonedDateTime", SLOT_REPR)?;
        ZonedDateTime::from_utf8(repr.as_bytes(), Disambiguation::Compatible, OffsetDisambiguation::Reject).ok()
    }

    fn temporal_zoned_date_time_slot_string(&self, receiver: Option<&Value<'gc>>, key: &str) -> Option<String> {
        let Some(Value::VmObject(obj)) = receiver.cloned() else {
            return None;
        };
        match own_data_from_legacy_map(&obj.borrow(), key) {
            Some(Value::String(text)) => Some(crate::unicode::utf16_to_utf8(&text)),
            _ => None,
        }
    }

    fn temporal_plain_date_number(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, field: &str) -> Value<'gc> {
        let Some(value) = self.temporal_expect_plain_date(ctx, receiver) else {
            return Value::Undefined;
        };
        match field {
            "year" => Value::Number(value.year() as f64),
            "month" => Value::Number(value.month() as f64),
            "day" => Value::Number(value.day() as f64),
            _ => Value::Undefined,
        }
    }

    fn temporal_plain_date_calendar(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_plain_date(ctx, receiver) else {
            return Value::Undefined;
        };
        Value::from(value.calendar().identifier())
    }

    fn temporal_plain_date_month_code(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_plain_date(ctx, receiver) else {
            return Value::Undefined;
        };
        Value::from(value.month_code().as_str())
    }

    fn temporal_plain_time_number(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, field: &str) -> Value<'gc> {
        let Some(value) = self.temporal_expect_plain_time(ctx, receiver) else {
            return Value::Undefined;
        };
        match field {
            "hour" => Value::Number(value.hour() as f64),
            "minute" => Value::Number(value.minute() as f64),
            "second" => Value::Number(value.second() as f64),
            "millisecond" => Value::Number(value.millisecond() as f64),
            "microsecond" => Value::Number(value.microsecond() as f64),
            "nanosecond" => Value::Number(value.nanosecond() as f64),
            _ => Value::Undefined,
        }
    }

    fn temporal_plain_date_time_number(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, field: &str) -> Value<'gc> {
        let Some(value) = self.temporal_expect_plain_date_time(ctx, receiver) else {
            return Value::Undefined;
        };
        match field {
            "year" => Value::Number(value.year() as f64),
            "month" => Value::Number(value.month() as f64),
            "day" => Value::Number(value.day() as f64),
            "hour" => Value::Number(value.hour() as f64),
            "minute" => Value::Number(value.minute() as f64),
            "second" => Value::Number(value.second() as f64),
            "millisecond" => Value::Number(value.millisecond() as f64),
            "microsecond" => Value::Number(value.microsecond() as f64),
            "nanosecond" => Value::Number(value.nanosecond() as f64),
            _ => Value::Undefined,
        }
    }

    fn temporal_plain_date_time_calendar(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_plain_date_time(ctx, receiver) else {
            return Value::Undefined;
        };
        Value::from(value.calendar().identifier())
    }

    fn temporal_plain_date_time_month_code(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_plain_date_time(ctx, receiver) else {
            return Value::Undefined;
        };
        Value::from(value.month_code().as_str())
    }

    fn temporal_duration_number(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, field: &str) -> Value<'gc> {
        let Some(value) = self.temporal_expect_duration(ctx, receiver) else {
            return Value::Undefined;
        };
        match field {
            "years" => Value::Number(value.years() as f64),
            "months" => Value::Number(value.months() as f64),
            "weeks" => Value::Number(value.weeks() as f64),
            "days" => Value::Number(value.days() as f64),
            "hours" => Value::Number(value.hours() as f64),
            "minutes" => Value::Number(value.minutes() as f64),
            "seconds" => Value::Number(value.seconds() as f64),
            "milliseconds" => Value::Number(value.milliseconds() as f64),
            "microseconds" => Value::Number(value.microseconds() as f64),
            "nanoseconds" => Value::Number(value.nanoseconds() as f64),
            _ => Value::Undefined,
        }
    }

    fn temporal_duration_blank(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_duration(ctx, receiver) else {
            return Value::Undefined;
        };
        Value::Boolean(value.is_zero())
    }

    fn temporal_duration_to_string(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_duration(ctx, receiver) else {
            return Value::Undefined;
        };
        Value::from(value.to_string())
    }

    fn temporal_zoned_date_time_to_string(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        options: Option<&Value<'gc>>,
    ) -> Value<'gc> {
        let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
            return Value::Undefined;
        };
        let resolved = match options {
            None => Ok((
                DisplayCalendar::Auto,
                Precision::Auto,
                DisplayOffset::Auto,
                None,
                None,
                DisplayTimeZone::Auto,
            )),
            Some(Value::Undefined) => Ok((
                DisplayCalendar::Auto,
                Precision::Auto,
                DisplayOffset::Auto,
                None,
                None,
                DisplayTimeZone::Auto,
            )),
            Some(options) => {
                if !self.temporal_is_object_like(options) {
                    Err(TemporalError::r#type().with_message("Options must be an object"))
                } else {
                    (|| -> Result<_, TemporalError> {
                        let calendar_name = match self.temporal_get_option_string(ctx, options, "calendarName")? {
                            Some(text) => DisplayCalendar::from_str(&text)?,
                            None => DisplayCalendar::Auto,
                        };
                        let fractional_second_digits_value = self.read_named_property(ctx, options, "fractionalSecondDigits");
                        if self.pending_throw.is_some() {
                            return Err(TemporalError::r#type().with_message("Invalid Temporal options"));
                        }
                        let precision = match fractional_second_digits_value {
                            Value::Undefined => Precision::Auto,
                            Value::Number(number) => {
                                let floored = number.floor();
                                if !number.is_finite() || !(0.0..=9.0).contains(&floored) {
                                    return Err(TemporalError::range().with_message("Invalid fractionalSecondDigits"));
                                }
                                Precision::Digit(floored as u8)
                            }
                            Value::String(text) => {
                                let text = crate::unicode::utf16_to_utf8(&text);
                                if text == "auto" {
                                    Precision::Auto
                                } else {
                                    return Err(TemporalError::range().with_message("Invalid fractionalSecondDigits"));
                                }
                            }
                            value => {
                                let text = self
                                    .temporal_value_string(ctx, &value)
                                    .ok_or_else(|| TemporalError::r#type().with_message("Invalid fractionalSecondDigits"))?;
                                if text == "auto" {
                                    Precision::Auto
                                } else {
                                    return Err(TemporalError::range().with_message("Invalid fractionalSecondDigits"));
                                }
                            }
                        };
                        let display_offset = match self.temporal_get_option_string(ctx, options, "offset")? {
                            Some(text) => DisplayOffset::from_str(&text)?,
                            None => DisplayOffset::Auto,
                        };
                        let rounding_mode = match self.temporal_get_option_string(ctx, options, "roundingMode")? {
                            Some(text) => Some(RoundingMode::from_str(&text)?),
                            None => None,
                        };
                        let smallest_unit_text = self.temporal_get_option_string(ctx, options, "smallestUnit")?;
                        let display_time_zone = match self.temporal_get_option_string(ctx, options, "timeZoneName")? {
                            Some(text) => DisplayTimeZone::from_str(&text)?,
                            None => DisplayTimeZone::Auto,
                        };
                        let smallest_unit = match smallest_unit_text {
                            Some(text) => {
                                let unit =
                                    Unit::from_str(&text).map_err(|_| TemporalError::range().with_message("Invalid smallestUnit"))?;
                                match unit {
                                    Unit::Minute | Unit::Second | Unit::Millisecond | Unit::Microsecond | Unit::Nanosecond => Some(unit),
                                    _ => return Err(TemporalError::range().with_message("Invalid smallestUnit")),
                                }
                            }
                            None => None,
                        };
                        Ok((
                            calendar_name,
                            precision,
                            display_offset,
                            rounding_mode,
                            smallest_unit,
                            display_time_zone,
                        ))
                    })()
                }
            }
        };
        match resolved.and_then(
            |(display_calendar, precision, display_offset, rounding_mode, smallest_unit, display_time_zone)| {
                value.to_ixdtf_string(
                    display_offset,
                    display_time_zone,
                    display_calendar,
                    ToStringRoundingOptions {
                        precision,
                        smallest_unit,
                        rounding_mode,
                    },
                )
            },
        ) {
            Ok(text) => Value::from(text),
            Err(err) => self.temporal_throw(ctx, err),
        }
    }

    fn temporal_plain_year_month_number(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, field: &str) -> Value<'gc> {
        let Some(value) = self.temporal_expect_plain_year_month(ctx, receiver) else {
            return Value::Undefined;
        };
        match field {
            "year" => Value::Number(value.year() as f64),
            "month" => Value::Number(value.month() as f64),
            _ => Value::Undefined,
        }
    }

    fn temporal_plain_year_month_calendar(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_plain_year_month(ctx, receiver) else {
            return Value::Undefined;
        };
        Value::from(value.calendar().identifier())
    }

    fn temporal_plain_month_day_month_code(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_plain_month_day(ctx, receiver) else {
            return Value::Undefined;
        };
        Value::from(value.month_code().as_str())
    }

    fn temporal_plain_month_day_day(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_plain_month_day(ctx, receiver) else {
            return Value::Undefined;
        };
        Value::Number(value.day() as f64)
    }

    fn temporal_plain_month_day_calendar(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_plain_month_day(ctx, receiver) else {
            return Value::Undefined;
        };
        Value::from(value.calendar().identifier())
    }

    fn temporal_zoned_date_time_number(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, field: &str) -> Value<'gc> {
        let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
            return Value::Undefined;
        };
        match field {
            "year" => Value::Number(value.year() as f64),
            "month" => Value::Number(value.month() as f64),
            "day" => Value::Number(value.day() as f64),
            "hour" => Value::Number(value.hour() as f64),
            "minute" => Value::Number(value.minute() as f64),
            "second" => Value::Number(value.second() as f64),
            "millisecond" => Value::Number(value.millisecond() as f64),
            "microsecond" => Value::Number(value.microsecond() as f64),
            "nanosecond" => Value::Number(value.nanosecond() as f64),
            _ => Value::Undefined,
        }
    }

    fn temporal_zoned_date_time_month_code(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
            return Value::Undefined;
        };
        Value::from(value.month_code().as_str())
    }

    fn temporal_zoned_date_time_calendar(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
            return Value::Undefined;
        };
        Value::from(value.calendar().identifier())
    }

    fn temporal_zoned_date_time_time_zone(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
            return Value::Undefined;
        };
        match value.time_zone().identifier() {
            Ok(id) => Value::from(id.as_str()),
            Err(err) => self.temporal_throw(ctx, err),
        }
    }

    fn temporal_zoned_date_time_year_of_week(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
            return Value::Undefined;
        };
        match value.year_of_week() {
            Some(value) => Value::Number(value as f64),
            None => Value::Undefined,
        }
    }

    fn temporal_zoned_date_time_week_of_year(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = self.temporal_expect_zoned_date_time(ctx, receiver) else {
            return Value::Undefined;
        };
        match value.week_of_year() {
            Some(value) => Value::Number(value as f64),
            None => Value::Undefined,
        }
    }
}
