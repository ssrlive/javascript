// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod tests {
    use javascript::{Value, evaluate_script};

    #[test]
    fn test_import_statement() {
        let script = r#"
            import { PI, E } from "math";
            import identity from "math";
            PI + E
        "#;
        let result = evaluate_script(script);
        assert!(result.is_ok(), "Import statement should work");
        // The result should be PI + E
        if let Ok(Value::Number(val)) = result {
            assert!((val - (std::f64::consts::PI + std::f64::consts::E)).abs() < 0.0001);
        } else {
            panic!("Expected number result");
        }
    }

    #[test]
    fn test_dynamic_import() {
        let script = r#"
            import("math").then(module => {
                return module.PI + module.E;
            })
        "#;
        let result = evaluate_script(script);
        // Dynamic import should return a Promise
        assert!(result.is_ok(), "Dynamic import should work");
    }

    #[test]
    fn test_export_statement() {
        let script = r#"
            export const x = 42;
            export function add(a, b) { return a + b; }
            1
        "#;
        let result = evaluate_script(script);
        assert!(result.is_ok(), "Export statement should work");
    }

    #[test]
    fn test_import_star_from_os() {
        let script = r#"
            import * as os from "os";
            import {log} from "console";

            // Call some OS functions
            let cwd = os.getcwd();
            let pid = os.getpid();
            let ppid = os.getppid();

            // Call some path functions
            let basename = os.path.basename("/home/user/test.js");
            let dirname = os.path.dirname("/home/user/test.js");
            let joined = os.path.join("home", "user", "test.js");

            // Log the results
            log("Current working directory:", cwd);
            log("Process ID:", pid);
            log("Parent Process ID:", ppid);
            console.log("Basename of /home/user/test.js:", basename);
            console.log("Dirname of /home/user/test.js:", dirname);
            console.log("Joined path:", joined);

            // Return a success value
            42
        "#;
        let result = evaluate_script(script);
        assert!(result.is_ok(), "Import star from os should work");
        // The result should be 42
        if let Ok(Value::Number(val)) = result {
            assert_eq!(val, 42.0);
        } else {
            panic!("Expected number result 42");
        }
    }

    #[test]
    fn test_import_from_js_file() {
        let script = r#"
            import { PI, E, add } from "./tests/test_module.js";
            import multiply from "./tests/test_module.js";

            // Test imported values
            console.log("PI:", PI);
            console.log("E:", E);
            console.log("add(3, 4):", add(3, 4));
            console.log("multiply(3, 4):", multiply(3, 4));

            // Verify values
            let pi_ok = Math.abs(PI - 3.14159) < 0.0001;
            let e_ok = Math.abs(E - 2.71828) < 0.0001;
            let add_ok = add(3, 4) === 7;
            let multiply_ok = multiply(3, 4) === 12;

            pi_ok && e_ok && add_ok && multiply_ok
        "#;
        let result = evaluate_script(script);
        assert!(result.is_ok(), "Import from JS file should work");
        // The result should be true
        if let Ok(Value::Boolean(val)) = result {
            assert!(val, "All imported values should be correct");
        } else {
            panic!("Expected boolean result true");
        }
    }
}
