use javascript::Value;
use javascript::evaluate_script;

// Init logger for tests
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_for_single_statement_body() {
    let script = r#"
        function f() {
            var a = [];
            for (var i = 0; i < 3; i++) a.push(i);
            return a.join(',');
        }
        f();
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(Value::String(s)) => assert_eq!(s, "0,1,2".encode_utf16().collect::<Vec<u16>>()),
        Ok(v) => panic!("Unexpected ok value: {:?}", v),
        Err(e) => panic!("Parse/eval error: {:?}", e),
    }
}

#[test]
fn test_while_single_statement_body() {
    let script = r#"
        function f() {
            var sum = 0;
            var i = 0;
            while (i < 4) sum += i++;
            return sum;
        }
        f();
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 6.0), // 0+1+2+3
        Ok(v) => panic!("Unexpected ok value: {:?}", v),
        Err(e) => panic!("Parse/eval error: {:?}", e),
    }
}

#[test]
fn test_do_while_single_statement_body() {
    let script = r#"
        function f() {
            var i = 0;
            var x = [];
            do x.push(i++); while (i < 2);
            return x.join('-');
        }
        f();
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(Value::String(s)) => assert_eq!(s, "0-1".encode_utf16().collect::<Vec<u16>>()),
        Ok(v) => panic!("Unexpected ok value: {:?}", v),
        Err(e) => panic!("Parse/eval error: {:?}", e),
    }
}

#[test]
fn test_if_single_statement_then_and_else() {
    let script = r#"
        function f(v) {
            var out = 0;
            if (v) out = 1; else out = 2;
            return out;
        }
        [f(true), f(false)].join(',');
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(Value::String(s)) => assert_eq!(s, "1,2".encode_utf16().collect::<Vec<u16>>()),
        Ok(v) => panic!("Unexpected ok value: {:?}", v),
        Err(e) => panic!("Parse/eval error: {:?}", e),
    }
}
