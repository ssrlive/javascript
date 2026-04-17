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
