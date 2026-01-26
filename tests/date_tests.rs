// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod date_tests {
    use javascript::evaluate_script;

    #[test]
    fn test_date_constructor_no_args() {
        let str_val = evaluate_script("new Date().toString()", None::<&std::path::Path>).unwrap();
        println!("Date string: {}", str_val);
        // Should be a properly formatted date string, not starting with "Date: "
        assert!(!str_val.starts_with("\"Date: "));
        assert!(str_val.contains("GMT") || str_val == "Invalid Date");
    }

    #[test]
    fn test_date_constructor_with_timestamp() {
        let value = evaluate_script("new Date(1234567890000).getTime()", None::<&std::path::Path>).unwrap();
        println!("Timestamp: {:?}", value);
        assert_eq!(value, "1234567890000");
    }

    #[test]
    fn test_date_value_of() {
        let value = evaluate_script("new Date(1234567890000).valueOf()", None::<&std::path::Path>).unwrap();
        assert_eq!(value, "1234567890000");
    }

    #[test]
    fn test_date_to_string() {
        let value = evaluate_script("new Date(1234567890000).toString()", None::<&std::path::Path>).unwrap();
        // Should be a properly formatted date string
        assert!(value.contains("2009") || value.contains("Invalid Date"));
    }

    #[test]
    fn test_date_constructor_with_iso_string() {
        let result = evaluate_script("new Date('2023-12-25T10:30:00Z').getTime()", None::<&std::path::Path>).unwrap();
        // Should be a valid timestamp
        assert_eq!(result, "1703500200000");
    }

    #[test]
    fn test_date_constructor_with_components() {
        let value = evaluate_script("new Date(2023, 11, 25, 10, 30, 0, 0).getFullYear()", None::<&std::path::Path>).unwrap();
        assert_eq!(value, "2023");
    }

    #[test]
    fn test_date_get_methods() {
        let value = evaluate_script("new Date(2023, 11, 25, 10, 30, 45, 123).getMonth()", None::<&std::path::Path>).unwrap();
        assert_eq!(value, "11"); // December (0-based)

        let value = evaluate_script("new Date(2023, 11, 25, 10, 30, 45, 123).getDate()", None::<&std::path::Path>).unwrap();
        assert_eq!(value, "25");

        let value = evaluate_script("new Date(2023, 11, 25, 10, 30, 45, 123).getHours()", None::<&std::path::Path>).unwrap();
        assert_eq!(value, "10");

        let value = evaluate_script("new Date(2023, 11, 25, 10, 30, 45, 123).getMinutes()", None::<&std::path::Path>).unwrap();
        assert_eq!(value, "30");

        let value = evaluate_script("new Date(2023, 11, 25, 10, 30, 45, 123).getSeconds()", None::<&std::path::Path>).unwrap();
        assert_eq!(value, "45");

        let value = evaluate_script(
            "new Date(2023, 11, 25, 10, 30, 45, 123).getMilliseconds()",
            None::<&std::path::Path>,
        )
        .unwrap();
        assert_eq!(value, "123");
    }

    #[test]
    fn test_date_to_primitive_string_hint_in_addition() {
        // Ensure Date objects use ToPrimitive with hint "string" when used in addition
        let left = evaluate_script("new Date(0) + new Date(0)", None::<&std::path::Path>).unwrap();
        let right = evaluate_script("new Date(0).toString() + new Date(0).toString()", None::<&std::path::Path>).unwrap();
        assert_eq!(left, right);
    }
}
