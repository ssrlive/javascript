use javascript::Value;
use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod os_tests {
    use super::*;

    #[test]
    fn test_os_open_close() {
        let script = r#"
            import * as os from "os";
            let fd = os.open("test.txt", 578);
            if (fd >= 0) {
                let result = os.close(fd);
                result;
            } else {
                -1;
            }
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        if let Err(e) = &result {
            println!("Error: {:?}", e);
        }
        assert!(result.is_ok());
        // Clean up
        std::fs::remove_file("test.txt").ok();
    }

    #[test]
    fn test_os_write_read() {
        let script = r#"
            import * as os from "os";
            let fd = os.open("test_write.txt", 578);
            if (fd >= 0) {
                let written = os.write(fd, "Hello World");
                os.seek(fd, 0, 0);
                let data = os.read(fd, 11);
                os.close(fd);
                data;
            } else {
                "";
            }
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        assert!(result.is_ok());
        assert_eq!(
            match result.unwrap() {
                Value::String(vec) => String::from_utf16_lossy(&vec),
                _ => panic!("Expected string result"),
            },
            "Hello World"
        );
        // Clean up
        std::fs::remove_file("test_write.txt").ok();
    }

    #[test]
    fn test_os_getcwd() {
        let script = r#"
            import * as os from "os";
            os.getcwd();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::String(s) => {
                let cwd = String::from_utf16_lossy(&s);
                let expected_cwd = std::env::current_dir().unwrap().to_str().unwrap().to_string();
                assert_eq!(cwd, expected_cwd);
            }
            _ => panic!("Expected string result"),
        }
    }

    #[test]
    fn test_os_getpid() {
        let script = r#"
            import * as os from "os";
            os.getpid();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::Number(pid) => {
                assert!(pid > 0.0);
            }
            _ => panic!("Expected number result"),
        }
    }

    #[test]
    fn test_os_path_join() {
        let script = r#"
            import * as os from "os";
            os.path.join("a", "b", "c");
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::String(s) => {
                let joined = String::from_utf16_lossy(&s);
                let expected = format!("a{}b{}c", std::path::MAIN_SEPARATOR, std::path::MAIN_SEPARATOR);
                assert_eq!(joined, expected);
            }
            _ => panic!("Expected string result"),
        }
    }

    #[test]
    fn test_os_path_basename() {
        let script = r#"
            import * as os from "os";
            os.path.basename("path/to/file.txt");
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::String(s) => {
                let basename = String::from_utf16_lossy(&s);
                assert_eq!(basename, "file.txt");
            }
            _ => panic!("Expected string result"),
        }
    }

    #[test]
    fn test_os_path_extname() {
        let script = r#"
            import * as os from "os";
            os.path.extname("file.txt");
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::String(s) => {
                let ext = String::from_utf16_lossy(&s);
                assert_eq!(ext, ".txt");
            }
            _ => panic!("Expected string result"),
        }
    }

    #[test]
    fn test_os_getppid() {
        let script = r#"
            import * as os from "os";
            os.getppid();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        // Just check that it doesn't crash and returns some number
        assert!(result.is_ok());
        match result.unwrap() {
            Value::Number(ppid) => assert!(ppid > 0.0),
            _ => panic!("Expected number result"),
        }
    }
}
