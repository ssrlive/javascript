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
        // using rustyline for interactive features (history, nicer prompt)
        println!("JavaScript REPL (persistent environment). Type 'exit' or Ctrl-D to quit.");
        // persistent environment so definitions persist across inputs
        let repl = javascript::Repl::new();

        // configure history path — prefer $HOME/.js_repl_history
        let history_path = std::env::var_os("HOME")
            .map(|h| {
                let mut p = std::path::PathBuf::from(h);
                p.push(".js_repl_history");
                p
            })
            .unwrap_or_else(|| std::path::PathBuf::from(".js_repl_history"));

        let mut rl = rustyline::DefaultEditor::new().expect("failed to create editor");
        if rl.load_history(&history_path).is_err() {
            // no history yet — ignore
        }

        // small bracket-balance check for crude multi-line input support
        fn needs_more_input(s: &str) -> bool {
            let mut stack = Vec::new();
            for ch in s.chars() {
                match ch {
                    '(' => stack.push(')'),
                    '[' => stack.push(']'),
                    '{' => stack.push('}'),
                    ')' | ']' | '}' => {
                        if stack.pop() != Some(ch) {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
            !stack.is_empty()
        }

        loop {
            match rl.readline("js> ") {
                Ok(mut line) => {
                    // support multi-line while brackets are unbalanced
                    while needs_more_input(&line) {
                        match rl.readline("...> ") {
                            Ok(cont) => {
                                line.push('\n');
                                line.push_str(&cont);
                            }
                            Err(rustyline::error::ReadlineError::Interrupted) => {
                                println!("");
                                break;
                            }
                            Err(_) => {
                                println!("");
                                break;
                            }
                        }
                    }

                    let trimmed = line.trim();
                    if trimmed == "exit" || trimmed == "quit" {
                        break;
                    }
                    if trimmed.is_empty() {
                        continue;
                    }

                    let _ = rl.add_history_entry(trimmed);

                    match repl.eval(line) {
                        Ok(result) => print_eval_result(&result),
                        Err(e) => eprintln!("Error: {:?}", e),
                    }
                }
                Err(rustyline::error::ReadlineError::Interrupted) => {
                    // Ctrl-C
                    println!("");
                    continue;
                }
                Err(rustyline::error::ReadlineError::Eof) => {
                    // Ctrl-D
                    println!("");
                    break;
                }
                Err(e) => {
                    eprintln!("REPL error: {e}");
                    break;
                }
            }
        }

        let _ = rl.save_history(&history_path);
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
