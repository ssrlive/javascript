use std::env;
use std::process;

use rust_quickjs::quickjs::*;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 || args[1] != "-e" {
        eprintln!("Usage: {} -e script", args[0]);
        process::exit(1);
    }

    let script = &args[2];

    // Tokenize
    let tokens = match tokenize(script) {
        Ok(tokens) => tokens,
        Err(_) => {
            eprintln!("Tokenization failed");
            process::exit(1);
        }
    };

    // Parse
    let mut tokens = tokens;
    let statements = match parse_statements(&mut tokens) {
        Ok(statements) => statements,
        Err(_) => {
            eprintln!("Parsing failed");
            process::exit(1);
        }
    };

    // Evaluate
    let mut env = std::collections::HashMap::new();
    let result = match evaluate_statements(&mut env, &statements) {
        Ok(value) => value,
        Err(_) => {
            eprintln!("Evaluation failed");
            process::exit(1);
        }
    };

    // Print result
    match result {
        Value::Number(n) => println!("{}", n),
        Value::String(s) => println!("{}", String::from_utf16_lossy(&s)),
        Value::Undefined => println!("undefined"),
        Value::Object(name) => println!("[object {}]", name),
        Value::Function(name) => println!("[Function: {}]", name),
        Value::Closure(_, _, _) => println!("[Function]"),
    }
}
