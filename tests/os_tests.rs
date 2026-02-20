#![allow(unused)]

use javascript::*;

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
    #[cfg(feature = "os")]
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
        let result = evaluate_module(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "0");
        // Clean up
        std::fs::remove_file("test.txt").ok();
    }

    #[test]
    #[cfg(feature = "os")]
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
        let result = evaluate_module(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"Hello World\"");
        // Clean up
        std::fs::remove_file("test_write.txt").ok();
    }

    #[test]
    #[cfg(feature = "os")]
    fn test_os_getcwd() {
        let script = r#"
            import * as os from "os";
            os.getcwd();
        "#;
        let result = evaluate_module(script, None::<&std::path::Path>).unwrap();
        let expected_cwd = std::env::current_dir().unwrap().to_str().unwrap().to_string();
        // Use JSON stringification for the expected value so platform-specific escaping (e.g. backslashes on Windows) matches
        assert_eq!(result, serde_json::to_string(&expected_cwd).unwrap());
    }

    #[test]
    #[cfg(feature = "os")]
    fn test_os_getpid() {
        let script = r#"
            import * as os from "os";
            os.getpid();
        "#;
        let result = evaluate_module(script, None::<&std::path::Path>).unwrap();
        assert!(result.parse::<i32>().unwrap() > 0);
    }

    #[test]
    #[cfg(feature = "os")]
    fn test_os_path_join() {
        let script = r#"
            import * as os from "os";
            os.path.join("a", "b", "c");
        "#;
        let result = evaluate_module(script, None::<&std::path::Path>).unwrap();
        let expected = format!("a{}b{}c", std::path::MAIN_SEPARATOR, std::path::MAIN_SEPARATOR);
        assert_eq!(result, serde_json::to_string(&expected).unwrap());
    }

    #[test]
    #[cfg(feature = "os")]
    fn test_os_path_basename() {
        let script = r#"
            import * as os from "os";
            os.path.basename("path/to/file.txt");
        "#;
        let result = evaluate_module(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"file.txt\"");
    }

    #[test]
    #[cfg(feature = "os")]
    fn test_os_path_extname() {
        let script = r#"
            import * as os from "os";
            os.path.extname("file.txt");
        "#;
        let result = evaluate_module(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\".txt\"");
    }

    #[test]
    #[cfg(feature = "os")]
    fn test_os_getppid() {
        let script = r#"
            import * as os from "os";
            os.getppid();
        "#;
        let result = evaluate_module(script, None::<&std::path::Path>).unwrap();
        // Just check that it doesn't crash and returns some number
        assert!(result.parse::<i32>().unwrap() > 0);
    }
}
