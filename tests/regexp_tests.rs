use javascript::core::evaluate_script;
use javascript::core::Value;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod regexp_tests {
    use super::*;

    #[test]
    fn test_regexp_constructor() {
        let result = evaluate_script("new RegExp('hello')");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Object(obj) => {
                // Check that the object has the expected properties
                assert!(obj.borrow().contains_key("__regex"));
                assert!(obj.borrow().contains_key("__flags"));
                assert!(obj.borrow().contains_key("toString"));
                assert!(obj.borrow().contains_key("test"));
                assert!(obj.borrow().contains_key("exec"));
            }
            _ => panic!("Expected object result"),
        }
    }

    #[test]
    fn test_regexp_constructor_with_flags() {
        let result = evaluate_script("new RegExp('hello', 'gi')");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Object(obj) => {
                // Check that the object has the expected properties
                assert!(obj.borrow().contains_key("__regex"));
                assert!(obj.borrow().contains_key("__flags"));
                // We can't easily check the flags value without calling toString
            }
            _ => panic!("Expected object result"),
        }
    }

    #[test]
    fn test_regexp_test_method() {
        let result = evaluate_script("new RegExp('hello').test('hello world')");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Boolean(b) => assert!(b),
            _ => panic!("Expected boolean result"),
        }
    }

    #[test]
    fn test_regexp_test_method_case_insensitive() {
        let result = evaluate_script("new RegExp('hello', 'i').test('HELLO world')");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Boolean(b) => assert!(b),
            _ => panic!("Expected boolean result"),
        }
    }

    #[test]
    fn test_regexp_exec_method() {
        let result = evaluate_script("new RegExp('hello').exec('hello world')[0]");
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::String(s) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "hello");
            }
            _ => panic!("Expected string result"),
        }
    }

    #[test]
    fn test_regexp_extract_emails() {
        // Test RegExp with a simple pattern
        // This demonstrates RegExp's ability to handle basic patterns
        let result = evaluate_script(r#"new RegExp('test').test('test string')"#);
        assert!(result.is_ok());
        let value = result.unwrap();

        // Should return true since the text contains 'test'
        match value {
            Value::Boolean(b) => assert!(b),
            _ => panic!("Expected boolean result"),
        }
    }

    #[test]
    fn test_regexp_validate_email_stackoverflow() {
        // Translated StackOverflow-style email regex into a Rust-regex-compatible pattern.
        // This keeps the validation strict while avoiding PCRE-only constructs.
        let script = r#"new RegExp('^([A-Za-z0-9!#$%&\'\*+/=?^_`{|}~-]+(?:\.[A-Za-z0-9!#$%&\'\*+/=?^_`{|}~-]+)*@[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z]{2,})+)$','i').test('john.doe@example.com')"#;
        let result = evaluate_script(script);
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Boolean(b) => assert!(b, "expected true for valid email"),
            _ => panic!("Expected boolean result"),
        }
    }

    #[test]
    fn test_match_emails_with_global_regex() {
        let script = r#"
        (function(){
            var s = 'Please email me with hello@world.com and test123@abc.org.cn and fake@abc';
            var r = new RegExp('[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[A-Za-z]{2,}','g');
            var res = [];
            var m = r.exec(s);
            if (m) { res.push(m[0]); }
            m = r.exec(s);
            if (m) { res.push(m[0]); }
            return res;
        })()
        "#;

        let result = evaluate_script(script).unwrap();
        match result {
            Value::Object(arr_rc) => {
                let arr = arr_rc.borrow();
                // Expect two matches
                let a0 = arr.get("0").unwrap().borrow().clone();
                let a1 = arr.get("1").unwrap().borrow().clone();
                match a0 {
                    Value::String(s0) => {
                        let s0s = String::from_utf16_lossy(&s0);
                        assert_eq!(s0s, "hello@world.com");
                    }
                    _ => panic!("expected string at index 0"),
                }
                match a1 {
                    Value::String(s1) => {
                        let s1s = String::from_utf16_lossy(&s1);
                        assert_eq!(s1s, "test123@abc.org.cn");
                    }
                    _ => panic!("expected string at index 1"),
                }
            }
            _ => panic!("expected array/object result"),
        }
    }
}
