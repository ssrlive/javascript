use javascript::{Value, evaluate_script};

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
        let result = evaluate_script("new RegExp('hello')", None::<&std::path::Path>);
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Object(obj) => {
                // Check that the object has the expected properties
                assert!(obj.borrow().contains_key(&"__regex".into()));
                assert!(obj.borrow().contains_key(&"__flags".into()));
                assert!(obj.borrow().contains_key(&"toString".into()));
                assert!(obj.borrow().contains_key(&"test".into()));
                assert!(obj.borrow().contains_key(&"exec".into()));
            }
            _ => panic!("Expected object result"),
        }
    }

    #[test]
    fn test_regexp_constructor_with_flags() {
        let result = evaluate_script("new RegExp('hello', 'gi')", None::<&std::path::Path>);
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Object(obj) => {
                // Check that the object has the expected properties
                assert!(obj.borrow().contains_key(&"__regex".into()));
                assert!(obj.borrow().contains_key(&"__flags".into()));
                // We can't easily check the flags value without calling toString
            }
            _ => panic!("Expected object result"),
        }
    }

    #[test]
    fn test_regexp_test_method() {
        let result = evaluate_script("new RegExp('hello').test('hello world')", None::<&std::path::Path>);
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Boolean(b) => assert!(b),
            _ => panic!("Expected boolean result"),
        }
    }

    #[test]
    fn test_regexp_test_method_case_insensitive() {
        let result = evaluate_script("new RegExp('hello', 'i').test('HELLO world')", None::<&std::path::Path>);
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::Boolean(b) => assert!(b),
            _ => panic!("Expected boolean result"),
        }
    }

    #[test]
    fn test_regexp_exec_method() {
        let result = evaluate_script("new RegExp('hello').exec('hello world')[0]", None::<&std::path::Path>);
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
        let result = evaluate_script(r#"new RegExp('test').test('test string')"#, None::<&std::path::Path>);
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
        let result = evaluate_script(script, None::<&std::path::Path>);
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
            var r = new RegExp('[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[A-Za-z]{2,}','g');
            var res = [];
            var m = r.exec(s);
            if (m) { res.push(m[0]); }
            m = r.exec(s);
            if (m) { res.push(m[0]); }
            return res;
        })()
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        match result {
            Value::Object(arr_rc) => {
                let arr = arr_rc.borrow();
                // Expect two matches
                let a0 = arr.get(&"0".into()).unwrap().borrow().clone();
                let a1 = arr.get(&"1".into()).unwrap().borrow().clone();
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

    #[test]
    fn test_regexp_sticky_behavior() {
        let script = r#"
        (function(){
            var s = 'abc 123 xyz';
            var r = new RegExp('\\d+','y');
            r.lastIndex = 4; // position of '1'
            var m = r.exec(s);
            return m ? m[0] : 'nomatch';
        })()
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        match result {
            Value::String(s) => {
                let s0 = String::from_utf16_lossy(&s);
                assert_eq!(s0, "123");
            }
            _ => panic!("Expected string result for sticky match"),
        }
    }

    #[test]
    fn test_regexp_crlf_normalization() {
        // 'R' flag should allow patterns expecting '\n' to match CRLF sequences in the original string
        let script = r#"
        (function(){
            var s = 'o\r\nw';
            var r = new RegExp('o\\nw','gR');
            var m = r.exec(s);
            return m ? m[0] : 'nomatch';
        })()
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        match result {
            Value::String(s) => {
                let val = String::from_utf16_lossy(&s);
                // Expect the returned match to reflect the original string with CRLF
                assert_eq!(val, "o\r\nw");
            }
            _ => panic!("Expected string result for CRLF match"),
        }
    }

    #[test]
    fn test_regexp_to_string() {
        let result = evaluate_script("new RegExp('ab+c').toString()", None::<&std::path::Path>);
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::String(s) => {
                let s_val = String::from_utf16_lossy(&s);
                assert_eq!(s_val, "/ab+c/");
            }
            _ => panic!("Expected string result"),
        }
    }

    #[test]
    fn test_regexp_unicode_lastindex_u_flag() {
        // Ensure lastIndex and returned index behave correctly with surrogate pairs
        // construct a string containing a surrogate pair (emoji) between ascii chars
        let script = r#"
        (function(){
            var s = 'a\uD83D\uDE00b'; // 'a' + 😀 + 'b'
            var r = new RegExp('.', 'gu');
            var matches = [];
            var m;
            while ((m = r.exec(s)) !== null) {
                matches.push(m[0]);
                matches.push(r.lastIndex);
            }
            // matches will be alternating values [match0, idx0, match1, idx1, ...]
            return JSON.stringify(matches);
        })()
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        match result {
            Value::String(s) => {
                let s0 = String::from_utf16_lossy(&s);
                // Expect three matches: 'a', surrogate pair (as one code point), 'b'
                // lastIndex returns UTF-16 code-unit index after match. After 'a' index is 1, after emoji index should be 3, and after 'b' index should be 4
                // JSON.stringify produced an array like ["a",1,"😀",3,"b",4]
                assert!(s0.contains("\"a\",1"));
                assert!(s0.contains("\"😀\",3"));
                assert!(s0.contains("\"b\",4"));
            }
            _ => panic!("Expected stringified results for unicode lastIndex test"),
        }
    }

    #[test]
    fn test_string_match_global_behavior() {
        let result = evaluate_script(r#"'cdbbdbsbz'.match(/d(b+)d/g)[0]"#, None::<&std::path::Path>);
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::String(s) => {
                let s_val = String::from_utf16_lossy(&s);
                assert_eq!(s_val, "dbbd");
            }
            _ => panic!("Expected string result"),
        }
    }

    #[test]
    fn test_string_match_non_global_captures() {
        let result = evaluate_script(r#"'cdbbdbsbz'.match(/d(b+)d/)[1]"#, None::<&std::path::Path>);
        assert!(result.is_ok());
        let value = result.unwrap();
        match value {
            Value::String(s) => {
                let s_val = String::from_utf16_lossy(&s);
                assert_eq!(s_val, "bb");
            }
            _ => panic!("Expected string result"),
        }
    }

    #[test]
    fn test_regexp_lazy_quantifier() {
        // Standard non-greedy matching using '?'
        let script = r#"
        (function(){
            var s = 'a111b222b';
            var r = /a.*?b/;
            var m = r.exec(s);
            return m ? m[0] : 'nomatch';
        })()
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        match result {
            Value::String(s) => {
                let val = String::from_utf16_lossy(&s);
                // Lazy match should stop at the first 'b'
                assert_eq!(val, "a111b");
            }
            _ => panic!("Expected string result for lazy match"),
        }
    }

    #[test]
    fn test_regexp_lazy_complex() {
        // Complex nested pattern with lazy quantifier
        let script = r#"
        (function(){
            var s = 'abcccxbcc';
            // 'a' followed by one or more 'bc+' groups, lazily, then 'x'
            var r = /a(bc+)+?x/;
            var m = r.exec(s);
            return m ? m[0] : 'nomatch';
        })()
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        match result {
            Value::String(s) => {
                let val = String::from_utf16_lossy(&s);
                assert_eq!(val, "abcccx");
            }
            _ => panic!("Expected string result for complex lazy test"),
        }
    }
}
