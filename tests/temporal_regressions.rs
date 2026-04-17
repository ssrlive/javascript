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
