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

    let script_content = if let Some(script) = cli.eval {
        script
    } else if let Some(ref file) = cli.file {
        match read_script_file(file) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Error reading file {}: {}", file.display(), e.user_message());
                process::exit(1);
            }
        }
    } else {
        // No script argument -> start the interactive, persistent REPL
        run_persistent_repl();
        return;
    };

    // If we got here we have a script to execute. Prefer the safe evaluate_script
    match evaluate_script(script_content, cli.file.as_ref()) {
        Ok(result) => println!("{result}"),
        Err(err) => {
            eprintln!("{}", err.user_message());
            if let Some(file_path) = cli.file.as_ref() {
                eprintln!("  in file: {}", file_path.display());
            }
            process::exit(1);
        }
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
                    Ok(val) => println!("{val}"),
                    Err(e) => {
                        eprintln!("{}", e.user_message());
                        // Show the code that caused the error for better debugging context
                        if buffer.lines().count() == 1 {
                            eprintln!("  at: {}", buffer.trim());
                        } else {
                            eprintln!("  in:");
                            for line in buffer.lines() {
                                eprintln!("    {}", line);
                            }
                        }
                    }
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
