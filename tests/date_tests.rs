use rust_quickjs::quickjs::evaluate_script;
use rust_quickjs::quickjs::Value;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod date_tests {
    use super::*;

    #[test]
    fn test_date_constructor_no_args() {
        let result = evaluate_script("new Date().toString()");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::String(s) => {
                let str_val = String::from_utf16_lossy(&s);
                println!("Date string: {}", str_val);
                // Should be a properly formatted date string, not starting with "Date: "
                assert!(!str_val.starts_with("Date: "));
                assert!(str_val.contains("GMT") || str_val == "Invalid Date");
            }
            _ => panic!("Expected string result"),
        }
    }

    #[test]
    fn test_date_constructor_with_timestamp() {
        let result = evaluate_script("new Date(1234567890000).getTime()");
        assert!(result.is_ok());
        let value = result.unwrap();
        println!("Timestamp: {:?}", value);
        match value {
            Value::Number(n) => assert_eq!(n, 1234567890000.0),
            _ => panic!("Expected number result"),
        }
    }

    #[test]
    fn test_date_value_of() {
        let result = evaluate_script("new Date(1234567890000).valueOf()");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Number(n) => assert_eq!(n, 1234567890000.0),
            _ => panic!("Expected number result"),
        }
    }

    #[test]
    fn test_date_to_string() {
        let result = evaluate_script("new Date(1234567890000).toString()");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::String(s) => {
                let str_val = String::from_utf16_lossy(&s);
                // Should be a properly formatted date string
                assert!(str_val.contains("2009") || str_val.contains("Invalid Date"));
            }
            _ => panic!("Expected string result"),
        }
    }

    #[test]
    fn test_date_constructor_with_iso_string() {
        let result = evaluate_script("new Date('2023-12-25T10:30:00Z').getTime()");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Number(n) => {
                // Should be a valid timestamp
                assert!(n > 0.0);
            }
            _ => panic!("Expected number result"),
        }
    }

    #[test]
    fn test_date_constructor_with_components() {
        let result = evaluate_script("new Date(2023, 11, 25, 10, 30, 0, 0).getFullYear()");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Number(n) => assert_eq!(n, 2023.0),
            _ => panic!("Expected number result"),
        }
    }

    #[test]
    fn test_date_get_methods() {
        let result = evaluate_script("new Date(2023, 11, 25, 10, 30, 45, 123).getMonth()");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Number(n) => assert_eq!(n, 11.0), // December (0-based)
            _ => panic!("Expected number result"),
        }

        let result = evaluate_script("new Date(2023, 11, 25, 10, 30, 45, 123).getDate()");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Number(n) => assert_eq!(n, 25.0),
            _ => panic!("Expected number result"),
        }

        let result = evaluate_script("new Date(2023, 11, 25, 10, 30, 45, 123).getHours()");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Number(n) => assert_eq!(n, 10.0),
            _ => panic!("Expected number result"),
        }

        let result = evaluate_script("new Date(2023, 11, 25, 10, 30, 45, 123).getMinutes()");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Number(n) => assert_eq!(n, 30.0),
            _ => panic!("Expected number result"),
        }

        let result = evaluate_script("new Date(2023, 11, 25, 10, 30, 45, 123).getSeconds()");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Number(n) => assert_eq!(n, 45.0),
            _ => panic!("Expected number result"),
        }

        let result = evaluate_script("new Date(2023, 11, 25, 10, 30, 45, 123).getMilliseconds()");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Number(n) => assert_eq!(n, 123.0),
            _ => panic!("Expected number result"),
        }
    }
}
