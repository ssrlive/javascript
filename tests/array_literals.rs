use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_empty_array_literal() {
    let script = r#"
        let arr = [];
        arr.length
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(n) => assert_eq!(n, "0"),
        _ => panic!("Expected number 0, got {:?}", result),
    }
}

#[test]
fn test_array_literal_with_elements() {
    let script = r#"
        let arr = [1, 2, 3];
        arr.length
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(n) => assert_eq!(n, "3"),
        _ => panic!("Expected number 3, got {:?}", result),
    }
}

#[test]
fn test_array_literal_indexing() {
    let script = r#"
        let arr = [10, 20, 30];
        arr[0] + arr[1] + arr[2]
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(n) => assert_eq!(n, "60"),
        _ => panic!("Expected number 60, got {:?}", result),
    }
}

#[test]
fn test_array_literal_mixed_types() {
    let script = r#"
        let arr = [1, "hello", true];
        arr.length
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(n) => assert_eq!(n, "3"),
        _ => panic!("Expected number 3, got {:?}", result),
    }
}

#[test]
fn test_array_literal_nested() {
    let script = r#"
        console.log((6 ^ 3) ^ 3 === 6);  // 5
        console.log(((6 ^ 3) ^ 3) === 6); // true
        console.log(9 + "20"); // 920
        console.log(9 - "20"); // -11
        console.log(9 * "20"); // 180
        console.log(9 / "3"); // 3
        console.log(9 % "4"); // 1
        console.log(2 ** 3); // 8
        console.log(2 ** "4"); // 16
        console.log(5 & 3); // 1
        console.log(5 | 2); // 7
        console.log(5 ^ 2); // 7
        console.log(5 << 1); // 10
        console.log(20 >> 2); // 5
        console.log(20 >>> 2); // 5
        console.log(5 & "3"); // 1
        console.log(5 | "2"); // 7
        console.log(!!""); // false
        console.log(~1); // -2
        console.log(+1); // 1
        console.log(-100); // -100
        console.log(+"123"); // 123

        var a;
        var b;
        a = b = 1;
        console.log(a);
        console.log(b);

        let arr = [ [1, 2], [3, 4] ];
        arr[0][0] + arr[0][1] + arr[1][0] + arr[1][1]
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(n) => assert_eq!(n, "10"),
        _ => panic!("Expected number 10, got {result:?}"),
    }
}
