#![allow(clippy::println_empty_string)]

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

    if let Some(script) = cli.eval {
        script_content = Some(script);
    } else if let Some(file) = cli.file {
        match std::fs::read_to_string(&file) {
            Ok(content) => script_content = Some(content),
            Err(e) => {
                eprintln!("Error reading file {}: {}", file.display(), e);
                process::exit(1);
            }
        }
    } else {
        // No script argument -> start the interactive, persistent REPL
        run_persistent_repl();
        return;
    }

    // If we got here we have a script to execute. Prefer the safe evaluate_script
    if let Some(script) = script_content {
        match evaluate_script(script) {
            Ok(result) => print_eval_result(&result),
            Err(err) => {
                eprintln!("Evaluation failed: {err}");
                process::exit(1);
            }
        }
    }
}

fn print_eval_result(result: &Value) {
    match result {
        Value::Number(n) => println!("{n}"),
        Value::String(s) => println!("{}", String::from_utf16_lossy(s)),
        Value::Boolean(b) => println!("{b}"),
        Value::Undefined => {} // println!("undefined"),
        Value::Object(_) => println!("[object Object]"),
        Value::Function(name) => println!("[Function: {}]", name),
        Value::Closure(_, _, _) => println!("[Function]"),
        Value::AsyncClosure(_, _, _) => println!("[Function]"),
        Value::ClassDefinition(_) => println!("[Class]"),
        Value::Getter(_, _) => println!("[Getter]"),
        Value::Setter(_, _, _) => println!("[Setter]"),
        Value::Property { .. } => println!("[Property]"),
        Value::Promise(_) => println!("[object Promise]"),
        Value::Symbol(_) => println!("[object Symbol]"),
        Value::BigInt(s) => println!("{}", s.raw),
        Value::Map(_) => println!("[object Map]"),
        Value::Set(_) => println!("[object Set]"),
        Value::WeakMap(_) => println!("[object WeakMap]"),
        Value::WeakSet(_) => println!("[object WeakSet]"),
        Value::GeneratorFunction(_, _, _) => println!("[GeneratorFunction]"),
        Value::Generator(_) => println!("[object Generator]"),
        Value::Proxy(_) => println!("[object Proxy]"),
        Value::ArrayBuffer(_) => println!("[object ArrayBuffer]"),
        Value::DataView(_) => println!("[object DataView]"),
        Value::TypedArray(_) => println!("[object TypedArray]"),
    }
}

// Persistent rustyline-powered REPL loop extracted into a helper to keep `main()` small.
fn run_persistent_repl() {
    use rustyline::Editor;
    use rustyline::error::ReadlineError;
    use std::path::PathBuf;

    let ver = clap::crate_version!();
    println!("JavaScript Interpreter REPL (persistent environment) v{ver}. Type 'exit' or Ctrl-D to quit.");

    let mut rl = match Editor::<(), rustyline::history::FileHistory>::new() {
        Ok(e) => e,
        Err(err) => {
            eprintln!("Failed to initialize line editor: {}", err);
            process::exit(1);
        }
    };

    // Simple history file in the user's home directory
    let history_path: Option<PathBuf> = std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".js_repl_history"));
    if let Some(ref p) = history_path {
        let _ = rl.load_history(p);
    }

    let repl = Repl::new();

    let mut buffer = String::new();

    loop {
        let prompt = if buffer.is_empty() { "js> " } else { ".... " };

        match rl.readline(prompt) {
            Ok(line) => {
                // support quick exit from the REPL
                let trimmed = line.trim();
                if trimmed == "exit" || trimmed == ".exit" {
                    break;
                }

                // accumulate into buffer when multi-line is needed
                if buffer.is_empty() {
                    buffer = line.clone();
                } else {
                    buffer.push('\n');
                    buffer.push_str(&line);
                }

                // if the input looks incomplete (unclosed brackets/strings/templates/comments), keep reading
                if !Repl::is_complete_input(&buffer) {
                    continue;
                }

                // Avoid evaluating empty submissions
                if buffer.trim().is_empty() {
                    buffer.clear();
                    continue;
                }

                let _ = rl.add_history_entry(buffer.clone());

                match repl.eval(&buffer) {
                    Ok(val) => print_eval_result(&val),
                    Err(e) => eprintln!("Error: {:?}", e),
                }

                buffer.clear();
            }
            Err(ReadlineError::Interrupted) => {
                println!("");
                buffer.clear();
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("Goodbye");
                break;
            }
            Err(err) => {
                eprintln!("Readline error: {}", err);
                break;
            }
        }
    }

    if let Some(ref p) = history_path {
        let _ = rl.save_history(p);
    }
}
