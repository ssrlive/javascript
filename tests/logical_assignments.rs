use javascript::Value;
use javascript::evaluate_script;
use javascript::tokenize;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_logical_assignments() {
    // Test logical AND assignment (&&=)
    let code1 = "let a = 5; a &&= 10; a";
    let result1 = evaluate_script(code1);
    assert!(result1.is_ok(), "evaluate_script(code1) failed: {:?}", result1);
    match result1 {
        Ok(Value::Number(n)) => assert_eq!(n, 10.0),
        _ => panic!("Expected number 10.0, got {:?}", result1),
    }

    let code2 = "let b = 0; b &&= 10; b";
    let result2 = evaluate_script(code2);
    assert!(result2.is_ok(), "evaluate_script(code2) failed: {:?}", result2);
    match result2 {
        Ok(Value::Number(n)) => assert_eq!(n, 0.0),
        _ => panic!("Expected number 0.0, got {:?}", result2),
    }

    // Test logical OR assignment (||=)
    let code3 = "let c = 5; c ||= 10; c";
    let result3 = evaluate_script(code3);
    assert!(result3.is_ok(), "evaluate_script(code3) failed: {:?}", result3);
    match result3 {
        Ok(Value::Number(n)) => assert_eq!(n, 5.0),
        _ => panic!("Expected number 5.0, got {:?}", result3),
    }

    let code4 = "let d = 0; d ||= 10; d";
    let result4 = evaluate_script(code4);
    assert!(result4.is_ok(), "evaluate_script(code4) failed: {:?}", result4);
    match result4 {
        Ok(Value::Number(n)) => assert_eq!(n, 10.0),
        _ => panic!("Expected number 10.0, got {:?}", result4),
    }

    // Test nullish coalescing assignment (??=)
    let code5 = "let e = 5; e ??= 10; e";
    let result5 = evaluate_script(code5);
    assert!(result5.is_ok(), "evaluate_script(code5) failed: {:?}", result5);
    match result5 {
        Ok(Value::Number(n)) => assert_eq!(n, 5.0),
        _ => panic!("Expected number 5.0, got {:?}", result5),
    }

    let code6 = "let f; f ??= 10; f";
    let result6 = evaluate_script(code6);
    assert!(result6.is_ok(), "evaluate_script(code6) failed: {:?}", result6);
    match result6 {
        Ok(Value::Number(n)) => assert_eq!(n, 10.0),
        _ => panic!("Expected number 10.0, got {:?}", result6),
    }
}

#[test]
fn eval_debug_logical_or_assign() {
    let code = "let c = 5; c ||= 10; c";
    let res = evaluate_script(code);
    println!("evaluate_script result: {:?}", res);
}

#[test]
fn token_debug_logical_or_assign() {
    let code = "let c = 5; c ||= 10; c";
    match tokenize(code) {
        Ok(tokens) => println!("Tokens for '{}': {:?}", code, tokens),
        Err(e) => println!("Tokenize error: {:?}", e),
    }
}
