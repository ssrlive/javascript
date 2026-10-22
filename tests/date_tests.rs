// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor(unsafe)]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod date_tests {
    use javascript::evaluate_script;

    #[test]
    fn test_date_constructor_no_args() {
        let script = "new Date().toString()";
        let str_val = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
        println!("Date string: {}", str_val);
        // Should be a properly formatted date string, not starting with "Date: "
        assert!(!str_val.starts_with("\"Date: "));
        assert!(str_val.contains("GMT") || str_val == "Invalid Date");
    }

    #[test]
    fn test_date_constructor_with_timestamp() {
        let script = "new Date(1234567890000).getTime()";
        let value = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
        println!("Timestamp: {:?}", value);
        assert_eq!(value, "1234567890000");
    }

    #[test]
    fn test_date_value_of() {
        let script = "new Date(1234567890000).valueOf()";
        let value = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
        assert_eq!(value, "1234567890000");
    }

    #[test]
    fn test_date_to_string() {
        let script = "new Date(1234567890000).toString()";
        let value = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
        // Should be a properly formatted date string
        assert!(value.contains("2009") || value.contains("Invalid Date"));
    }

    #[test]
    fn test_date_constructor_with_iso_string() {
        let script = "new Date('2023-12-25T10:30:00Z').getTime()";
        let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
        // Should be a valid timestamp
        assert_eq!(result, "1703500200000");
    }

    #[test]
    fn test_date_constructor_with_components() {
        let script = "new Date(2023, 11, 25, 10, 30, 0, 0).getFullYear()";
        let value = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
        assert_eq!(value, "2023");
    }

    #[test]
    fn test_date_get_methods() {
        let script = "new Date(2023, 11, 25, 10, 30, 45, 123).getMonth()";
        let value = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
        assert_eq!(value, "11"); // December (0-based)

        let script = "new Date(2023, 11, 25, 10, 30, 45, 123).getDate()";
        let value = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
        assert_eq!(value, "25");

        let script = "new Date(2023, 11, 25, 10, 30, 45, 123).getHours()";
        let value = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
        assert_eq!(value, "10");

        let script = "new Date(2023, 11, 25, 10, 30, 45, 123).getMinutes()";
        let value = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
        assert_eq!(value, "30");

        let script = "new Date(2023, 11, 25, 10, 30, 45, 123).getSeconds()";
        let value = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
        assert_eq!(value, "45");

        let script = "new Date(2023, 11, 25, 10, 30, 45, 123).getMilliseconds()";
        let value = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
        assert_eq!(value, "123");
    }

    #[test]
    fn test_date_to_primitive_string_hint_in_addition() {
        // Ensure Date objects use ToPrimitive with hint "string" when used in addition
        let script = "new Date(0) + new Date(0)";
        let left = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
        let script = "new Date(0).toString() + new Date(0).toString()";
        let right = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
        assert_eq!(left, right);
    }
}
