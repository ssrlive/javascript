use javascript::core::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod promise_tests {
    use super::*;

    #[test]
    fn test_promise_constructor_basic() {
        let code = r#"
            let resolved = false;
            let value = null;
            let p = new Promise(function(resolve, reject) {
                resolve(42);
            });
            resolved = true;
            value = 42;
            resolved
        "#;
        let result = evaluate_script(code);
        assert!(result.is_ok());
        // For now, just check that it doesn't crash
    }

    #[test]
    fn test_promise_then_basic() {
        let code = r#"
            let result = null;
            let p = new Promise(function(resolve, reject) {
                resolve(100);
            });
            p.then(function(val) {
                result = val * 2;
            });
            result
        "#;
        let result = evaluate_script(code);
        assert!(result.is_ok());
        // For now, just check that it doesn't crash
    }

    #[test]
    fn test_promise_chaining() {
        let code = r#"
            let finalResult = null;
            let p = new Promise(function(resolve, reject) {
                resolve(10);
            });
            p.then(function(val) {
                return val + 5;
            }).then(function(val) {
                return val * 2;
            }).then(function(val) {
                finalResult = val;
            });
            finalResult
        "#;
        let result = evaluate_script(code);
        assert!(result.is_ok());
        // For now, just check that it doesn't crash - full chaining requires async execution
    }

    #[test]
    fn test_promise_all_resolved() {
        let code = r#"
            let result = null;
            let p1 = new Promise(function(resolve, reject) {
                resolve(1);
            });
            let p2 = new Promise(function(resolve, reject) {
                resolve(2);
            });
            let p3 = new Promise(function(resolve, reject) {
                resolve(3);
            });
            Promise.all([p1, p2, p3]).then(function(values) {
                result = values[0] + values[1] + values[2];
            });
            result
        "#;
        let result = evaluate_script(code);
        assert!(result.is_ok());
        // For now, just check that it doesn't crash - full functionality requires async execution
    }

    #[test]
    fn test_promise_race_resolved() {
        let code = r#"
            let result = null;
            let p1 = new Promise(function(resolve, reject) {
                resolve(1);
            });
            let p2 = new Promise(function(resolve, reject) {
                resolve(2);
            });
            Promise.race([p1, p2]).then(function(value) {
                result = value;
            });
            result
        "#;
        let result = evaluate_script(code);
        assert!(result.is_ok());
        // For now, just check that it doesn't crash - full functionality requires async execution
    }
}
