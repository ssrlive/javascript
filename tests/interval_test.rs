use javascript::evaluate_script;

#[cfg(test)]
mod interval_tests {
    use super::*;

    #[test]
    #[ignore]
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
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert!(result.parse::<f64>().unwrap() >= 1.0);
    }
}
