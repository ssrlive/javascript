use javascript::*;

#[test]
fn test_plain_date_to_zoned_date_time_basic() {
    let script = r#"
        Temporal.PlainDate.from("2020-01-01")
            .toZonedDateTime({ timeZone: "UTC", plainTime: "12:00" })
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"2020-01-01T12:00:00+00:00[UTC]\"");
}

#[test]
fn test_zoned_date_time_to_plain_time_basic() {
    let script = r#"
        Temporal.ZonedDateTime.from("2019-10-29T09:46:38.271986102Z[-07:00]")
            .toPlainTime()
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"02:46:38.271986102\"");
}

#[test]
fn test_plain_date_to_plain_date_time_defaults_to_midnight() {
    let script = r#"
        Temporal.PlainDate.from("2020-01-01")
            .toPlainDateTime()
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"2020-01-01T00:00:00\"");
}

#[test]
fn test_temporal_compare_basics() {
    let script = r#"
        [
          Temporal.PlainDate.compare("1976-11-18", "2019-06-30"),
          Temporal.ZonedDateTime.compare(
            "1976-11-18T15:23:30.123456789+01:00[+01:00]",
            "2019-10-29T10:46:38.271986102+01:00[+01:00]"
          )
        ].join(",")
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"-1,-1\"");
}

#[test]
fn test_temporal_instant_round_basic() {
    let script = r#"
        Temporal.Instant.from("1970-01-01T00:00:00.0005005Z")
            .round("microsecond")
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"1970-01-01T00:00:00.000501Z\"");
}

#[test]
fn test_temporal_plain_time_round_basic() {
    let script = r#"
        Temporal.PlainTime.from("12:34:56.7895")
            .round({ smallestUnit: "millisecond" })
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"12:34:56.79\"");
}

#[test]
fn test_temporal_duration_subtract_basic() {
    let script = r#"
        Temporal.Duration.from("PT5H30M")
            .subtract("PT45M")
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"PT4H45M\"");
}

#[test]
fn test_temporal_zoned_date_time_subtract_basic() {
    let script = r#"
        Temporal.ZonedDateTime.from("2020-03-08T03:30:00-04:00[America/New_York]")
            .subtract({ hours: 1 })
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"2020-03-08T01:30:00-05:00[America/New_York]\"");
}

#[test]
fn test_temporal_plain_time_to_string_fractional_second_digits() {
    let script = r#"
        Temporal.PlainTime.from("12:34:56.7895")
            .toString({ fractionalSecondDigits: 3 })
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"12:34:56.789\"");
}

#[test]
fn test_temporal_instant_to_string_with_time_zone() {
    let script = r#"
        Temporal.Instant.from("1970-01-01T00:00:00Z")
            .toString({ timeZone: "UTC" })
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"1970-01-01T00:00:00+00:00\"");
}

#[test]
fn test_temporal_plain_date_time_with_plain_time_defaults_to_midnight() {
    let script = r#"
        Temporal.PlainDateTime.from("2015-12-07T03:24:30.0000035")
            .withPlainTime()
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"2015-12-07T00:00:00\"");
}

#[test]
fn test_temporal_zoned_date_time_from_accepts_zoned_date_time_time_zone_object() {
    let script = r#"
        Temporal.ZonedDateTime.from({
            year: 2000,
            month: 5,
            day: 2,
            timeZone: new Temporal.ZonedDateTime(0n, "UTC")
        }).timeZoneId
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"UTC\"");
}

#[test]
fn test_temporal_zoned_date_time_from_rejects_mismatched_offset_by_default() {
    let script = r#"
        try {
            Temporal.ZonedDateTime.from("1970-01-01T00:00-04:15[+01:00]");
            "nope";
        } catch (e) {
            e instanceof RangeError;
        }
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_temporal_plain_year_month_from_constrains_month() {
    let script = r#"
        Temporal.PlainYearMonth.from({ year: 2021, month: 13 }, { overflow: "constrain" })
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"2021-12\"");
}

#[test]
fn test_temporal_plain_month_day_from_uses_year_for_overflow_only() {
    let script = r#"
        Temporal.PlainMonthDay.from({ year: 2021, monthCode: "M02", day: 29 })
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"02-28\"");
}

#[test]
fn test_temporal_plain_year_month_from_rejects_utc_designator() {
    let script = r#"
        try {
            Temporal.PlainYearMonth.from("2021-07-16T00:00Z");
            "nope";
        } catch (e) {
            e instanceof RangeError;
        }
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_temporal_plain_month_day_from_out_of_range_year_only_affects_overflow() {
    let script = r#"
        Temporal.PlainMonthDay.from({ year: -999999, monthCode: "M02", day: 29 })
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"02-28\"");
}

#[test]
fn test_temporal_plain_month_day_from_invalid_month_code_still_throws_with_numeric_month() {
    let script = r#"
        try {
            Temporal.PlainMonthDay.from({ month: 1, monthCode: "M00", day: 17 });
            "nope";
        } catch (e) {
            e instanceof RangeError;
        }
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_temporal_plain_date_with_constrains_month() {
    let script = r#"
        new Temporal.PlainDate(1976, 11, 18)
            .with({ month: 13 })
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"1976-12-18\"");
}

#[test]
fn test_temporal_plain_date_time_with_updates_date_fields() {
    let script = r#"
        new Temporal.PlainDateTime(2000, 5, 2, 12, 34, 56, 987, 654, 321)
            .with({ year: 2001, month: 6, day: 3 })
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"2001-06-03T12:34:56.987654321\"");
}

#[test]
fn test_temporal_plain_time_with_updates_fields() {
    let script = r#"
        new Temporal.PlainTime(12, 34, 56, 987, 654, 321)
            .with({ hour: 1, minute: 2, second: 3 })
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"01:02:03.987654321\"");
}

#[test]
fn test_temporal_plain_year_month_with_updates_fields() {
    let script = r#"
        new Temporal.PlainYearMonth(2000, 5)
            .with({ year: 2001, month: 6 })
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"2001-06\"");
}

#[test]
fn test_temporal_plain_month_day_with_updates_fields() {
    let script = r#"
        new Temporal.PlainMonthDay(5, 2)
            .with({ monthCode: "M06", day: 3 })
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"06-03\"");
}

#[test]
fn test_temporal_plain_date_with_calendar_accepts_iso_string() {
    let script = r#"
        new Temporal.PlainDate(1976, 11, 18)
            .withCalendar("2020-01-01T00:00:00.000000000")
            .calendarId
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"iso8601\"");
}

#[test]
fn test_temporal_plain_date_time_with_calendar_requires_argument() {
    let script = r#"
        try {
            new Temporal.PlainDateTime(1976, 11, 18, 15, 23, 30).withCalendar(undefined);
            "nope";
        } catch (e) {
            e instanceof TypeError;
        }
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_temporal_instant_to_zoned_date_time_iso_basic() {
    let script = r#"
        Temporal.Instant.from("1970-01-01T00:00:00Z")
            .toZonedDateTimeISO("UTC")
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"1970-01-01T00:00:00+00:00[UTC]\"");
}

#[test]
fn test_temporal_plain_year_month_to_plain_date_basic() {
    let script = r#"
        Temporal.PlainYearMonth.from("2002-01")
            .toPlainDate({ day: 22 })
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"2002-01-22\"");
}

#[test]
fn test_temporal_plain_month_day_to_plain_date_basic() {
    let script = r#"
        Temporal.PlainMonthDay.from("01-22")
            .toPlainDate({ year: 2002 })
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"2002-01-22\"");
}

#[test]
fn test_temporal_plain_date_to_plain_year_month_basic() {
    let script = r#"
        new Temporal.PlainDate(1970, 12, 24)
            .toPlainYearMonth()
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"1970-12\"");
}

#[test]
fn test_temporal_plain_date_time_to_plain_date_basic() {
    let script = r#"
        new Temporal.PlainDateTime(2021, 12, 11, 1, 2, 3, 4, 5, 6)
            .toPlainDate()
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"2021-12-11\"");
}

#[test]
fn test_temporal_plain_date_time_to_plain_time_basic() {
    let script = r#"
        Temporal.PlainDateTime.from("2020-02-12T11:42:56.987654321+01:00[Europe/Amsterdam]")
            .toPlainTime()
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"11:42:56.987654321\"");
}

#[test]
fn test_temporal_zoned_date_time_to_instant_basic() {
    let script = r#"
        Temporal.ZonedDateTime.from("2019-10-29T10:46:38.271986102+01:00[+01:00]")
            .toInstant()
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"2019-10-29T09:46:38.271986102Z\"");
}

#[test]
fn test_temporal_zoned_date_time_get_time_zone_transition_utc_returns_null() {
    let script = r#"
        new Temporal.ZonedDateTime(0n, "UTC").getTimeZoneTransition("next")
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "null");
}

#[test]
fn test_temporal_duration_abs_basic() {
    let script = r#"
        new Temporal.Duration(-1, -2, -3, -4, -5, -6, -7, -8, -9, -10)
            .abs()
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"P1Y2M3W4DT5H6M7.00800901S\"");
}

#[test]
fn test_temporal_zoned_date_time_start_of_day_basic() {
    let script = r#"
        const ns = 10000n * 86400_000_000_000n + 7272_123_456_789n;
        new Temporal.ZonedDateTime(ns, "UTC").startOfDay().epochNanoseconds
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "864000000000000000");
}

#[test]
fn test_temporal_plain_date_to_plain_month_day_basic() {
    let script = r#"
        new Temporal.PlainDate(1976, 11, 18)
            .toPlainMonthDay()
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"11-18\"");
}

#[test]
fn test_temporal_plain_date_calendar_getters_basic() {
    let script = r#"
        const date = new Temporal.PlainDate(1976, 11, 18);
        [
          date.dayOfWeek,
          date.dayOfYear,
          date.daysInMonth,
          date.daysInWeek,
          date.daysInYear,
          date.monthsInYear,
          date.inLeapYear,
          date.weekOfYear,
          date.yearOfWeek,
        ].join(",")
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"4,323,30,7,366,12,true,47,1976\"");
}

#[test]
fn test_temporal_plain_date_time_calendar_getters_basic() {
    let script = r#"
        const dateTime = new Temporal.PlainDateTime(1976, 11, 18, 15, 23, 30, 123, 456, 789);
        [
          dateTime.dayOfWeek,
          dateTime.dayOfYear,
          dateTime.daysInMonth,
          dateTime.daysInWeek,
          dateTime.daysInYear,
          dateTime.monthsInYear,
          dateTime.inLeapYear,
          dateTime.weekOfYear,
          dateTime.yearOfWeek,
        ].join(",")
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"4,323,30,7,366,12,true,47,1976\"");
}

#[test]
fn test_temporal_duration_sign_with_and_total_basic() {
    let script = r#"
        const duration = Temporal.Duration.from({ years: 5, days: 1 });
        [
          duration.sign,
          duration.with({ years: -1, days: 0, minutes: -1 }).toString(),
          new Temporal.Duration(0, 0, 0, 0, 1).total("minutes"),
        ].join(",")
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"1,-P1YT1M,60\"");
}

#[test]
fn test_temporal_duration_constructor_treats_undefined_as_zero() {
    let script = r#"
        new Temporal.Duration(1, 1, 1, undefined).toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"P1Y1M1W\"");
}

#[test]
fn test_temporal_plain_year_month_calendar_getters_basic() {
    let script = r#"
        const value = new Temporal.PlainYearMonth(1976, 11);
        [value.daysInMonth, value.daysInYear, value.monthsInYear, value.inLeapYear].join(",")
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"30,366,12,true\"");
}

#[test]
fn test_temporal_zoned_date_time_additional_getters_basic() {
    let script = r#"
        const value = new Temporal.ZonedDateTime(217178610123456789n, "+01:00");
        [
          value.offset,
          value.offsetNanoseconds,
          value.dayOfWeek,
          value.dayOfYear,
          value.daysInWeek,
        ].join(",")
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"+01:00,3600000000000,4,323,7\"");
}

#[test]
fn test_temporal_to_locale_string_and_era_getters_basic() {
    let script = r#"
        const date = new Temporal.PlainDate(2000, 3, 6);
        const dateTime = new Temporal.PlainDateTime(2000, 3, 6, 1, 2, 3);
        const yearMonth = new Temporal.PlainDate(2000, 3, 6, "gregory").toPlainYearMonth();
        const monthDay = new Temporal.PlainDate(2000, 3, 6, "gregory").toPlainMonthDay();
        const zoned = new Temporal.ZonedDateTime(952300923000000000n, "UTC");
        [
          date.toLocaleString("en-US"),
          dateTime.toLocaleString("en-US"),
          new Temporal.PlainTime(1, 2, 3).toLocaleString("en-US"),
          Temporal.Duration.from("PT1H").toLocaleString("en-US"),
          yearMonth.toLocaleString("en-US"),
          monthDay.toLocaleString("en-US"),
          zoned.toLocaleString("en-US"),
          String(date.era),
          String(date.eraYear),
          String(dateTime.era),
          String(dateTime.eraYear),
          String(yearMonth.era),
          String(yearMonth.eraYear),
          String(zoned.era),
          String(zoned.eraYear),
        ].join("|")
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(
        result,
        "\"3/6/2000|3/6/2000, 1:02:03 AM|1:02:03 AM|1 hour|3/2000|3/6|3/6/2000, 12:02:03 AM UTC|undefined|undefined|undefined|undefined|ce|2000|undefined|undefined\""
    );
}

#[test]
fn test_temporal_instant_from_epoch_factories_basic() {
    let script = r#"
        [
          Temporal.Instant.fromEpochMilliseconds(217175010123).epochNanoseconds,
          Temporal.Instant.fromEpochNanoseconds(217175010123456789n).epochNanoseconds,
        ].join(",")
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"217175010123000000,217175010123456789\"");
}

#[test]
fn test_temporal_plain_time_from_property_bag_basic() {
    let script = r#"
        Temporal.PlainTime.from({ hour: 12, minute: 34, second: 56, millisecond: 987, microsecond: 654, nanosecond: 321 })
            .toString()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"12:34:56.987654321\"");
}

#[test]
fn test_temporal_plain_date_to_string_calendar_name() {
    let script = r#"
        new Temporal.PlainDate(2000, 5, 2).toString({ calendarName: "always" })
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"2000-05-02[u-ca=iso8601]\"");
}

#[test]
fn test_temporal_now_returns_intrinsic_instances() {
    let script = r#"
        [
          Temporal.Now.instant() instanceof Temporal.Instant,
          Temporal.Now.plainDateISO() instanceof Temporal.PlainDate,
          Temporal.Now.plainTimeISO("UTC") instanceof Temporal.PlainTime,
          Temporal.Now.plainDateTimeISO() instanceof Temporal.PlainDateTime,
          typeof Temporal.Now.timeZoneId()
        ].join(",")
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"true,true,true,true,string\"");
}

#[test]
fn test_temporal_constructor_requires_new() {
    let script = r#"
        let ok = false;
        try {
          Temporal.Instant(0n);
        } catch (err) {
          ok = err instanceof TypeError;
        }
        ok
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}
