// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod builtin_functions_tests {
    use javascript::evaluate_script;

    #[test]
    fn test_array_methods_exist() {
        let script = "let arr = Array(); typeof arr.every";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"function\"");
    }

    #[test]
    fn test_math_constants() {
        let script = "Math.PI";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3.141592653589793");

        let script = "Math.E";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "2.718281828459045");
    }

    #[test]
    fn test_math_floor() {
        let script = "Math.floor(3.7)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");
    }

    #[test]
    fn test_math_ceil() {
        let script = "Math.ceil(3.1)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "4");
    }

    #[test]
    fn test_math_sqrt() {
        let script = "Math.sqrt(9)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");
    }

    #[test]
    fn test_math_pow() {
        let script = "Math.pow(2, 3)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "8");
    }

    #[test]
    fn test_math_sin() {
        let script = "Math.sin(0)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "0");
    }

    #[test]
    fn test_math_random() {
        let script = "Math.random()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert!(
            (0.0..1.0).contains(&result.parse::<f64>().unwrap()),
            "Expected Math.random() to be in [0, 1), got {result}",
        );
    }

    #[test]
    fn test_math_clz32() {
        // Test clz32 with zero
        let script = "Math.clz32(0)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "32");

        // Test clz32 with 1
        let script2 = "Math.clz32(1)";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2, "31");

        // Test clz32 with larger number (268435456 = 2^28)
        let script3 = "Math.clz32(268435456)";
        let result3 = evaluate_script(script3, None::<&std::path::Path>).unwrap();
        assert_eq!(result3, "3");

        // Test clz32 with NaN (should return 32)
        let script4 = "Math.clz32(NaN)";
        let result4 = evaluate_script(script4, None::<&std::path::Path>).unwrap();
        assert_eq!(result4, "32");
    }

    #[test]
    fn test_math_imul() {
        // Test basic imul
        let script = "Math.imul(2, 3)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 6.0);

        // Test imul with negative numbers
        let script2 = "Math.imul(-2, 3)";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2.parse::<f64>().unwrap(), -6.0);

        // Test imul with overflow (should wrap around)
        // 2147483647 is 2^31 - 1 (max 32-bit signed int)
        let script3 = "Math.imul(2147483647, 2)";
        let result3 = evaluate_script(script3, None::<&std::path::Path>).unwrap();
        assert_eq!(result3.parse::<f64>().unwrap(), -2.0); // Should wrap to -2
    }

    #[test]
    fn test_math_max() {
        // Test max with multiple arguments
        let script = "Math.max(1, 5, 3, 9, 2)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 9.0);

        // Test max with two arguments
        let script2 = "Math.max(10, 20)";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2.parse::<f64>().unwrap(), 20.0);

        // Test max with single argument
        let script3 = "Math.max(42)";
        let result3 = evaluate_script(script3, None::<&std::path::Path>).unwrap();
        assert_eq!(result3.parse::<f64>().unwrap(), 42.0);

        // Test max with no arguments (should return -Infinity)
        let script4 = "Math.max()";
        let result4 = evaluate_script(script4, None::<&std::path::Path>).unwrap();
        let n4 = result4.parse::<f64>().unwrap();
        assert!(n4.is_infinite() && n4.is_sign_negative());

        // Test max with NaN argument (should return NaN)
        let script5 = "Math.max(1, NaN, 3)";
        let result5 = evaluate_script(script5, None::<&std::path::Path>).unwrap();
        assert!(result5.parse::<f64>().unwrap().is_nan());

        // Test max with negative numbers
        let script6 = "Math.max(-5, -1, -10)";
        let result6 = evaluate_script(script6, None::<&std::path::Path>).unwrap();
        assert_eq!(result6.parse::<f64>().unwrap(), -1.0);
    }

    #[test]
    fn test_math_min() {
        // Test min with multiple arguments
        let script = "Math.min(1, 5, 3, 9, 2)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 1.0);

        // Test min with two arguments
        let script2 = "Math.min(10, 20)";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2.parse::<f64>().unwrap(), 10.0);

        // Test min with single argument
        let script3 = "Math.min(42)";
        let result3 = evaluate_script(script3, None::<&std::path::Path>).unwrap();
        assert_eq!(result3.parse::<f64>().unwrap(), 42.0);

        // Test min with no arguments (should return +Infinity)
        let script4 = "Math.min()";
        let result4 = evaluate_script(script4, None::<&std::path::Path>).unwrap();
        let n4 = result4.parse::<f64>().unwrap();
        assert!(n4.is_infinite() && n4.is_sign_positive());

        // Test min with NaN argument (should return NaN)
        let script5 = "Math.min(1, NaN, 3)";
        let result5 = evaluate_script(script5, None::<&std::path::Path>).unwrap();
        assert!(result5.parse::<f64>().unwrap().is_nan());

        // Test min with negative numbers
        let script6 = "Math.min(-5, -1, -10)";
        let result6 = evaluate_script(script6, None::<&std::path::Path>).unwrap();
        assert_eq!(result6.parse::<f64>().unwrap(), -10.0);
    }

    #[test]
    fn test_parse_int() {
        let script = "parseInt('42')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 42.0);

        let script = "parseInt('3.14')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 3.0);
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_parse_float() {
        let script = "parseFloat('3.14')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 3.14_f64);
    }

    #[test]
    fn test_is_nan() {
        let script = "isNaN(NaN)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");

        let script = "isNaN(42)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");
    }

    #[test]
    fn test_is_finite() {
        let script = "isFinite(42)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");

        let script = "isFinite(Infinity)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");
    }

    #[test]
    fn test_json_stringify() {
        let script = "JSON.stringify(42)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        // result is a JSON string literal like "42"; extract inner string
        let inner: String = serde_json::from_str(&result).unwrap();
        assert_eq!(inner, "42");
    }

    #[test]
    fn test_json_parse_stringify_roundtrip() {
        let script = r#"let obj = JSON.parse('{"name":"John","age":30,"city":"New York"}'); JSON.stringify(obj)"#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner: String = serde_json::from_str(&result).unwrap();
        // Should be the same as input (order may differ, but for this simple case it should match)
        assert_eq!(inner, r#"{"age":30,"city":"New York","name":"John"}"#);
    }

    #[test]
    fn test_json_parse_array() {
        let script = r#"let arr = JSON.parse('[1, "hello", true, null]'); arr.length"#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 4.0);

        // Check elements
        let script2 = r#"let arr = JSON.parse('[1, "hello", true, null]'); arr[0]"#;
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2.parse::<f64>().unwrap(), 1.0);

        let script3 = r#"let arr = JSON.parse('[1, "hello", true, null]'); arr[1]"#;
        let result3 = evaluate_script(script3, None::<&std::path::Path>).unwrap();
        assert_eq!(result3, "\"hello\"");

        let script4 = r#"let arr = JSON.parse('[1, "hello", true, null]'); arr[2]"#;
        let result4 = evaluate_script(script4, None::<&std::path::Path>).unwrap();
        assert_eq!(result4, "true");

        let script5 = r#"let arr = JSON.parse('[1, "hello", true, null]'); arr[3]"#;
        let result5 = evaluate_script(script5, None::<&std::path::Path>).unwrap();
        assert_eq!(result5, "undefined");
    }

    #[test]
    fn test_array_push() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.length";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 2.0);
    }

    #[test]
    fn test_array_pop() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.pop()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 2.0);
    }

    #[test]
    fn test_array_join() {
        let script = "let arr = Array(); let arr2 = arr.push('a'); let arr3 = arr2.push('b'); arr3.join('-')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner: String = serde_json::from_str(&result).unwrap();
        assert_eq!(inner, "a-b");
    }

    #[test]
    fn test_object_keys() {
        let script = "let obj = {a: 1, b: 2}; Object.keys(obj).length";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "2");
    }

    #[test]
    fn test_object_assign() {
        // Test basic assign
        let script = "let target = {a: 1}; let source = {b: 2, c: 3}; Object.assign(target, source); target.c";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");

        // Test assign with multiple sources
        let script2 =
            "let target = {a: 1}; let source1 = {b: 2}; let source2 = {c: 3}; Object.assign(target, source1, source2); target.b + target.c";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2, "5");

        // Test assign returns target
        let script3 = "let target = {a: 1}; let result = Object.assign(target, {b: 2}); result.a + result.b";
        let result3 = evaluate_script(script3, None::<&std::path::Path>).unwrap();
        assert_eq!(result3, "3");
    }

    #[test]
    fn test_object_create() {
        // Test basic create with null prototype
        let script = "let obj = Object.create(null); obj.a = 1; obj.a";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "1");

        // Test create with prototype
        let script2 = "let proto = {inherited: 'yes'}; let obj = Object.create(proto); obj.own = 'mine'; obj.inherited + obj.own";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2, "\"yesmine\"");

        // Test create with property descriptors (basic)
        let script3 = "let obj = Object.create({}, {prop: {value: 42}}); obj.prop";
        let result3 = evaluate_script(script3, None::<&std::path::Path>).unwrap();
        assert_eq!(result3, "42");
    }

    #[test]
    fn test_encode_uri_component() {
        let script = "encodeURIComponent('hello world')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"hello%20world\"");
    }

    #[test]
    fn test_decode_uri_component() {
        let script = "decodeURIComponent('hello%20world')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"hello world\"");
    }

    #[test]
    fn test_number_constructor() {
        let script = "Number('42.5')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42.5");
    }

    #[test]
    fn test_boolean_constructor() {
        let script = "Boolean(1)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_eval_function() {
        let script = "eval('\"hello\"')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"hello\"");
    }

    #[test]
    fn test_eval_expression() {
        let script = "eval('1 + 2')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");
    }

    #[test]
    fn test_encode_uri() {
        let script = "encodeURI('hello world')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "hello%20world");
    }

    #[test]
    fn test_decode_uri() {
        let script = "decodeURI('hello%20world')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "hello world");
    }

    #[test]
    fn test_array_for_each() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.forEach(function(x) { return x; })";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "undefined");
    }

    #[test]
    fn test_array_map() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let mapped = arr3.map(function(x) { return x * 2; }); mapped.length";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 2.0);
    }

    #[test]
    fn test_array_filter() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let filtered = arr3.filter(function(x) { return x > 1; }); filtered.length";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 1.0);
    }

    #[test]
    fn test_array_reduce() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.reduce(function(acc, x) { return acc + x; }, 0)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 6.0);
    }

    #[test]
    fn test_string_split_simple() {
        let script = "let parts = 'a,b,c'.split(','); parts.length";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 3.0);
    }

    #[test]
    fn test_string_split_empty_sep() {
        let script = "let parts = 'abc'.split(''); parts.length";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 3.0);
    }

    #[test]
    fn test_string_split_no_args() {
        let script = "let parts = 'hello world'.split(); parts.length";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 1.0);

        let script = "let parts = 'hello world'.split(); parts[0]";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "hello world");
    }

    #[test]
    fn test_string_split_with_limit() {
        let script = "let parts = 'a,b,c,d'.split(',', 2); parts.length";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 2.0);

        let script = "let parts = 'a,b,c,d'.split(',', 2); parts[0]";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "a");

        let script = "let parts = 'a,b,c,d'.split(',', 2); parts[1]";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "b");
    }

    #[test]
    fn test_string_char_at() {
        let script = "'hello'.charAt(1)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"e\"");
    }

    #[test]
    fn test_string_char_at_negative() {
        let script = "'hello'.charAt(-1)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner.len(), 0);
    }

    #[test]
    fn test_string_replace_functional() {
        let script = "'hello world'.replace('world', 'there')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "hello there");
    }

    #[test]
    fn test_string_substr() {
        let script = "'hello world'.substr(2, 7)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "llo wor");

        let script = "'hello world'.substr(-3)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "rld");

        let script = "'my name is hello'.substr(3)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "name is hello");

        let script = "'my name is hello'.substr(4, -7)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "");

        let script = "'my name is hello'.substr(-7, 5)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "s hel");
    }

    #[test]
    fn test_string_substring_swap() {
        let script = "'hello world'.substring(5, 2)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "llo");

        let script = "'hello world'.substring(-3)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "hello world");

        let script = "'hello world'.substring(7, -2)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "hello w");
    }

    #[test]
    fn test_array_map_values() {
        let script = "let arr = Array(); let a2 = arr.push(1); let a3 = a2.push(2); let mapped = a3.map(function(x) { return x * 2; }); mapped.join(',')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "2,4");
    }

    #[test]
    fn test_string_trim() {
        let script = "'  hello world  '.trim()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "hello world");
    }

    #[test]
    fn test_string_trim_end() {
        let script = "'  hello world  '.trimEnd()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "  hello world");
    }

    #[test]
    fn test_string_trim_start() {
        let script = "'  hello world  '.trimStart()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "hello world  ");
    }

    #[test]
    fn test_string_starts_with() {
        let script = "'hello world'.startsWith('hello')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_string_ends_with() {
        let script = "'hello world'.endsWith('world')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_string_includes() {
        let script = "'hello world'.includes('lo wo')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_string_repeat() {
        let script = "'ha'.repeat(3)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "hahaha");
    }

    #[test]
    fn test_string_concat() {
        let script = "'hello'.concat(' ', 'world', '!')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "hello world!");
    }

    #[test]
    fn test_string_pad_start() {
        let script = "'5'.padStart(3, '0')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "005");
    }

    #[test]
    fn test_string_pad_end() {
        let script = "'5'.padEnd(3, '0')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "500");
    }

    #[test]
    fn test_string_index_of() {
        let script = "'hello world'.indexOf('world')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "6");

        let script = "'hello world'.indexOf('m')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "-1");

        let script = "'hello world'.indexOf('l', 3)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");

        let script = "'hello world'.indexOf('l', 4)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "9");
    }

    #[test]
    fn test_string_last_index_of() {
        let script = "'hello world'.lastIndexOf('l')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "9");

        let script = "'hello world'.lastIndexOf('m')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "-1");

        let script = "'hello world'.lastIndexOf('l', 8)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");
    }

    #[test]
    fn test_array_find() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.find(function(x) { return x > 2; })";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");

        // Test find with no match
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.find(function(x) { return x > 5; })";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "undefined");
    }

    #[test]
    fn test_array_find_index() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.findIndex(function(x) { return x > 2; })";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "2");

        // Test findIndex with no match
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.findIndex(function(x) { return x > 5; })";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "-1");
    }

    #[test]
    fn test_array_some() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.some(function(x) { return x > 2; })";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");

        // Test some with no match
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.some(function(x) { return x > 5; })";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");
    }

    #[test]
    fn test_array_every() {
        let script = "let arr = Array(); let arr2 = arr.push(2); let arr3 = arr2.push(4); let arr4 = arr3.push(6); arr4.every(function(x) { return x > 1; })";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");

        // Test every with some elements not matching
        let script = "let arr = Array(); let arr2 = arr.push(2); let arr3 = arr2.push(1); let arr4 = arr3.push(6); arr4.every(function(x) { return x > 1; })";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");
    }

    #[test]
    fn test_array_concat() {
        let script = "let arr1 = Array(); let arr2 = arr1.push(1); let arr3 = arr2.push(2); let arr4 = Array(); let arr5 = arr4.push(3); let arr6 = arr5.push(4); let result = arr3.concat(arr6); result.length";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 4.0);

        // Test concat with non-array values
        let script = "let arr1 = Array(); let arr2 = arr1.push(1); let result = arr2.concat(2, 3); result.length";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 3.0);
    }

    #[test]
    fn test_array_index_of() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.indexOf(2)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 1.0);

        // Test indexOf with element not found
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.indexOf(5)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), -1.0);

        // Test indexOf with fromIndex
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(2); arr4.indexOf(2, 2)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 2.0);
    }

    #[test]
    fn test_array_includes() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.includes(2)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");

        // Test includes with element not found
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.includes(5)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");

        // Test includes with fromIndex
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(2); arr4.includes(2, 2)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_array_sort() {
        let script = "let arr = Array(); let arr2 = arr.push(3); let arr3 = arr2.push(1); let arr4 = arr3.push(2); let sorted = arr4.sort(); sorted.join(',')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "1,2,3");

        // Test sort with custom compare function
        let script = "let arr = Array(); let arr2 = arr.push(3); let arr3 = arr2.push(1); let arr4 = arr3.push(2); let sorted = arr4.sort(function(a, b) { return b - a; }); sorted.join(',')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "3,2,1");
    }

    #[test]
    fn test_array_reverse() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); let reversed = arr4.reverse(); reversed.join(',')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "3,2,1");
    }

    #[test]
    fn test_array_splice() {
        // Test basic splice - remove elements
        let script =
            "let arr = Array(); arr.push(1); arr.push(2); arr.push(3); arr.push(4); let removed = arr.splice(1, 2); removed.join(',')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "2,3");

        // Test splice with insertion (no elements removed)
        let script2 = "let arr = Array(); arr.push(1); arr.push(4); let removed = arr.splice(1, 0, 2, 3); removed.length";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2.parse::<f64>().unwrap(), 0.0); // No elements were removed
    }

    #[test]
    fn test_array_shift() {
        let script = "let arr = Array(); arr.push(1); arr.push(2); arr.push(3); let first = arr.shift(); arr.join(',')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"2,3\"");

        // Test shift on empty array
        let script2 = "let arr = Array(); arr.shift()";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2, "undefined");
    }

    #[test]
    fn test_array_unshift() {
        let script = "let arr = Array(); arr.push(3); arr.push(4); let len = arr.unshift(1, 2); arr.join(',') + ',' + len";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"1,2,3,4,4\"");

        // Test unshift on empty array
        let script2 = "let arr = Array(); let len = arr.unshift(1, 2, 3); len";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2, "3");
    }

    #[test]
    fn test_array_fill() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); let arr5 = arr4.push(4); let filled = arr5.fill(9, 1, 3); filled.join(',')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"1,9,9,4\"");

        // Test fill entire array
        let script2 = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); let filled = arr4.fill(0); filled.join(',')";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2, "\"0,0,0\"");
    }

    #[test]
    fn test_array_last_index_of() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); let arr5 = arr4.push(2); let arr6 = arr5.push(1); arr6.lastIndexOf(2)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");

        // Test element not found
        let script2 = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.lastIndexOf(4)";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2, "-1");

        // Test with fromIndex
        let script3 = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); let arr5 = arr4.push(2); arr5.lastIndexOf(2, 2)";
        let result3 = evaluate_script(script3, None::<&std::path::Path>).unwrap();
        assert_eq!(result3, "1");
    }

    #[test]
    fn test_array_to_string() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.toString()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "1,2,3");

        // Test empty array
        let script2 = "let arr = Array(); arr.toString()";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        let inner2 = serde_json::from_str::<String>(&result2).unwrap_or(result2.clone());
        assert_eq!(inner2, "");

        // Test array with different types
        let script3 = "let arr = Array(); arr.push(1); arr.push('hello'); arr.push(true); arr.toString()";
        let result3 = evaluate_script(script3, None::<&std::path::Path>).unwrap();
        let inner3 = serde_json::from_str::<String>(&result3).unwrap_or(result3.clone());
        assert_eq!(inner3, "1,hello,true");
    }

    #[test]
    fn test_array_flat() {
        // Test basic flat - create nested array manually
        let script = "let arr = Array(); let subarr = Array(); subarr.push(2); subarr.push(3); arr.push(1); arr.push(subarr); arr.push(4); let flat = arr.flat(); flat.length === 4 && flat[0] === 1 && flat[1] === 2 && flat[2] === 3 && flat[3] === 4";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_array_flat_map() {
        // Test basic flatMap - create arrays manually
        let script = "let arr = Array(); arr.push(1); arr.push(2); let res = arr.flatMap(function(x) { let result = Array(); result.push(x); result.push(x*2); return result; }); res.length === 4 && res[0] === 1 && res[1] === 2 && res[2] === 2 && res[3] === 4";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_array_copy_within() {
        // Test copyWithin
        let script = "let arr = Array(); arr.push(1); arr.push(2); arr.push(3); arr.push(4); let res = arr.copyWithin(0, 2, 4); res.length === 4 && res[0] === 3 && res[1] === 4 && res[2] === 3 && res[3] === 4";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_array_entries() {
        // Test entries — convert iterator to array and return JSON
        let script = r#"
            let arr = Array();
            arr.push(1);
            arr.push(2);
            let entries = Array.from(arr.entries());
            JSON.stringify(entries)
            "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"[[0,1],[1,2]]\"");
    }

    #[test]
    fn test_array_from() {
        // Test Array.from with array-like object
        let script = r#"
            let arr = Array();
            let arr2 = arr.push(1);
            let arr3 = arr.push(2);
            let res = Array.from(arr);
            res.length === 2 && res[0] === 1 && res[1] === 2
            "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_array_is_array() {
        // Test Array.isArray with array
        let script = "let arr = Array(); arr.push(1); Array.isArray(arr)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");

        // Test Array.isArray with non-array
        let script2 = "Array.isArray(42)";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2, "false");
    }

    #[test]
    fn test_array_of() {
        // Test Array.of
        let script = "let res = Array.of(1, 2, 3); res.length === 3 && res[0] === 1 && res[1] === 2 && res[2] === 3";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_typeof_operator() {
        // Test typeof with different types
        let script = "typeof 42";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "number");

        let script = "typeof 'hello'";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "string");

        let script = "typeof true";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "boolean");

        let script = "typeof undefined";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "undefined");

        let script = "typeof {}";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "object");

        let script = "typeof function(){}";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result).unwrap_or(result.clone());
        assert_eq!(inner, "function");
    }

    #[test]
    fn test_delete_operator() {
        // Test deleting a property from an object
        let script = "let obj = {}; obj.x = 42; delete obj.x; obj.x";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "undefined"); // obj.x should be undefined after deletion

        // Test deleting a non-existent property
        let script = "let obj = {}; delete obj.nonexistent";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true"); // Should return true even for non-existent properties

        // Test deleting a variable (should throw SyntaxError in strict mode)
        let script = "let x = 42; delete x";
        let result = evaluate_script(script, None::<&std::path::Path>);
        assert!(result.is_err());
    }

    #[test]
    fn test_void_operator() {
        // Test void with a number
        let script = "void 42";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "undefined");

        // Test void with an expression
        let script = "void (1 + 2 + 3)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "undefined");

        // Test void with a string
        let script = "void 'hello'";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "undefined");
    }

    #[test]
    fn test_in_operator() {
        // Test property exists
        let script = "let obj = {}; obj.x = 42; 'x' in obj";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");

        // Test property doesn't exist
        let script = "let obj = {}; 'nonexistent' in obj";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");

        // Test inherited property
        let script = "let proto = {}; proto.inherited = 'yes'; let obj = {}; obj.__proto__ = proto; 'inherited' in obj";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_array_find_last() {
        // Test findLast with even numbers
        let script = "let arr = [1, 2, 3, 4, 5]; arr.findLast(x => x % 2 === 0)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 4.0);

        // Test findLast with no match
        let script2 = "let arr = [1, 3, 5]; arr.findLast(x => x % 2 === 0)";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2, "undefined");

        // Test findLast with strings
        let script3 = "let arr = ['a', 'b', 'c']; arr.findLast(x => x === 'b')";
        let result3 = evaluate_script(script3, None::<&std::path::Path>).unwrap();
        let inner = serde_json::from_str::<String>(&result3).unwrap_or(result3.clone());
        assert_eq!(inner, "b");
    }

    #[test]
    fn test_array_find_last_index() {
        // Test findLastIndex with even numbers
        let script = "let arr = [1, 2, 3, 4, 5]; arr.findLastIndex(x => x % 2 === 0)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result.parse::<f64>().unwrap(), 3.0);

        // Test findLastIndex with no match
        let script2 = "let arr = [1, 3, 5]; arr.findLastIndex(x => x % 2 === 0)";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2.parse::<f64>().unwrap(), -1.0);

        // Test findLastIndex with strings
        let script3 = "let arr = ['a', 'b', 'c', 'b']; arr.findLastIndex(x => x === 'b')";
        let result3 = evaluate_script(script3, None::<&std::path::Path>).unwrap();
        assert_eq!(result3.parse::<f64>().unwrap(), 3.0);
    }
}
