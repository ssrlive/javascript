use rust_quickjs::quickjs::evaluate_script;
use rust_quickjs::quickjs::Value;

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
        let result = evaluate_script(script);
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
        let result = evaluate_script(script);
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
        let result = evaluate_script(script);
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
}
