use javascript::*;
use std::process;

#[derive(clap::Parser)]
#[command(name = "rust_js", version, about = "JavaScript Rust Interpreter with imports")]
struct Cli {
    /// Execute script
    #[arg(short, long)]
    eval: Option<String>,

    /// JavaScript file to execute
    file: Option<std::path::PathBuf>,
}

fn main() {
    let cli = <Cli as clap::Parser>::parse();

    // Initialize logger (controlled by RUST_LOG)
    env_logger::init();

    let script: String;
    if let Some(s) = cli.eval {
        script = s;
    } else if let Some(file) = cli.file {
        match std::fs::read_to_string(&file) {
            Ok(content) => script = content,
            Err(e) => {
                eprintln!("Error reading file {}: {}", file.display(), e);
                process::exit(1);
            }
        }
    } else {
        eprintln!("Error: Must provide either --eval or a file");
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
        Value::Promise(_) => println!("[object Promise]"),
        Value::Symbol(_) => println!("[object Symbol]"),
    }
}
