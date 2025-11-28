use std::env;
use std::process;

use javascript::core::*;

fn main() {
    let args: Vec<String> = env::args().collect();

    // Initialize logger (controlled by RUST_LOG)
    #[cfg(feature = "env_logger")]
    env_logger::init();

    let script: String;
    if args.len() >= 3 && args[1] == "-e" {
        script = args[2].clone();
    } else if args.len() >= 2 && args[1] != "-h" {
        // Read from file
        match std::fs::read_to_string(&args[1]) {
            Ok(content) => script = content,
            Err(e) => {
                eprintln!("Error reading file {}: {}", args[1], e);
                process::exit(1);
            }
        }
    } else {
        eprintln!("Usage: {} [file.js | -e script]", args[0]);
        process::exit(1);
    }

    // Evaluate using the script evaluator that handles imports
    let result = match evaluate_script(script) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("Evaluation failed: {:?}", err);
            process::exit(1);
        }
    };

    // Print result
    match result {
        Value::Number(n) => println!("{}", n),
        Value::String(s) => println!("{}", String::from_utf16_lossy(&s)),
        Value::Boolean(b) => println!("{}", b),
        Value::Undefined => println!("undefined"),
        Value::Object(_) => println!("[object Object]"),
        Value::Function(name) => println!("[Function: {}]", name),
        Value::Closure(_, _, _) => println!("[Function]"),
        Value::ClassDefinition(_) => println!("[Class]"),
        Value::Getter(_, _) => println!("[Getter]"),
        Value::Setter(_, _, _) => println!("[Setter]"),
        Value::Property { .. } => println!("[Property]"),
    }
}
