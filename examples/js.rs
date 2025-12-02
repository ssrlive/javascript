use javascript::*;
use std::process;

#[derive(clap::Parser)]
#[command(name = "js", version, about = "JavaScript Rust Interpreter")]
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

    let script_content: Option<String>;
    // script_content will hold the script to execute when a script is provided

    if let Some(script) = cli.eval {
        script_content = Some(script);
    } else if let Some(file) = cli.file {
        // filename previously used for low-level JS_Eval, no longer needed here
        match std::fs::read_to_string(&file) {
            Ok(content) => script_content = Some(content),
            Err(e) => {
                eprintln!("Error reading file {}: {}", file.display(), e);
                process::exit(1);
            }
        }
    } else {
        // No script argument -> start simple REPL (non-persistent environment per-line)
        // We intentionally use evaluate_script (safe API) which builds a fresh env per execution.
        use std::io::{self, Write};
        println!("JavaScript REPL (persistent environment). Type 'exit' or Ctrl-D to quit.");
        // persistent environment so definitions persist across inputs
        let repl = javascript::Repl::new();
        loop {
            print!("js> ");
            let _ = io::stdout().flush();
            let mut buf = String::new();
            match io::stdin().read_line(&mut buf) {
                Ok(0) => {
                    // EOF
                    println!();
                    break;
                }
                Ok(_) => {
                    let line = buf.trim_end();
                    if line == "exit" || line == "quit" {
                        break;
                    }
                    if line.is_empty() {
                        continue;
                    }
                    match repl.eval(line) {
                        Ok(result) => print_eval_result(&result),
                        Err(e) => eprintln!("Error: {:?}", e),
                    }
                }
                Err(e) => {
                    eprintln!("REPL read error: {}", e);
                    break;
                }
            }
        }
        return;
    }

    // If we got here we have a script to execute. Prefer the safe evaluate_script
    if let Some(script) = script_content {
        match evaluate_script(script) {
            Ok(result) => {
                print_eval_result(&result);
            }
            Err(err) => {
                eprintln!("Evaluation failed: {:?}", err);
                process::exit(1);
            }
        }
    }
}

fn print_eval_result(result: &Value) {
    match result {
        Value::Number(n) => println!("{}", n),
        Value::String(s) => println!("{}", String::from_utf16_lossy(s)),
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
