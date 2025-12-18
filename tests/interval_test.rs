use javascript::{Value, evaluate_script};

#[cfg(test)]
mod interval_tests {
    use super::*;

    #[test]
    fn test_set_interval() {
        let script = r#"
            let count = 0;
            let id = setInterval(() => {
                count++;
            }, 10);
            
            new Promise((resolve) => {
                setTimeout(() => {
                    clearInterval(id);
                    resolve(count);
                }, 50);
            })
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => {
                println!("Count after intervals: {n}");
                assert!(n >= 1.0, "Expected count >= 1, got {n}");
            }
            _ => panic!("Expected number, got {:?}", result),
        }
    }
}
