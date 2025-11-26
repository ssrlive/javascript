use std::env;
use std::process;

use rust_quickjs::quickjs::*;

fn main() {
    let args: Vec<String> = env::args().collect();

    // Initialize logger (controlled by RUST_LOG)
    #[cfg(feature = "env_logger")]
    env_logger::init();

    if args.len() < 3 || args[1] != "-e" {
        eprintln!("Usage: {} -e script", args[0]);
        process::exit(1);
    }

    let script = &args[2];

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
    }
}
