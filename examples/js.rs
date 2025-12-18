use javascript::*;

#[derive(clap::Parser)]
#[command(name = "js", version, about = "JavaScript Rust Interpreter")]
struct Cli {
    /// Execute script
    #[arg(short, long)]
    eval: Option<String>,

    /// JavaScript file to execute
    file: Option<std::path::PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    // Initialize logger (controlled by RUST_LOG)
    env_logger::init();

    #[cfg(windows)]
    {
        // Spawn a thread with larger stack size (8MB) to avoid stack overflow on Windows
        // where the default stack size is 1MB.
        let builder = std::thread::Builder::new().stack_size(8 * 1024 * 1024);
        let handler = builder.spawn(|| run_main())?;
        handler.join().unwrap()
    }

    #[cfg(unix)]
    run_main()
}

fn run_main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let cli = <Cli as clap::Parser>::parse();

    let script_content = if let Some(script) = cli.eval {
        script
    } else if let Some(ref file) = cli.file {
        match read_script_file(file) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Error reading file {}: {}", file.display(), e.user_message());
                std::process::exit(1);
            }
        }
    } else {
        // No script argument -> start the interactive, persistent REPL
        run_persistent_repl()?;
        return Ok(());
    };

    // If we got here we have a script to execute. Prefer the safe evaluate_script
    match evaluate_script(script_content, cli.file.as_ref()) {
        Ok(result) => println!("{result}"),
        Err(err) => {
            eprintln!("{}", err.user_message());
            if let Some(file_path) = cli.file.as_ref() {
                if let (Some(line), Some(col)) = (err.js_line(), err.js_column()) {
                    eprintln!("  in file: {}:{}:{}", file_path.display(), line, col);
                } else {
                    eprintln!("  in file: {}", file_path.display());
                }
            }
            std::process::exit(1);
        }
    }
    Ok(())
}

// Persistent rustyline-powered REPL loop extracted into a helper to keep `main()` small.
#[allow(clippy::println_empty_string)]
fn run_persistent_repl() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    use rustyline::Editor;
    use rustyline::error::ReadlineError;
    use std::path::PathBuf;

    let ver = clap::crate_version!();
    println!("JavaScript Interpreter REPL (persistent environment) v{ver}. Type 'exit' or Ctrl-D to quit.");

    let mut rl = match Editor::<(), rustyline::history::FileHistory>::new() {
        Ok(e) => e,
        Err(err) => {
            eprintln!("Failed to initialize line editor: {err}");
            std::process::exit(1);
        }
    };

    // Simple history file in the user's home directory
    let history_path: Option<PathBuf> = std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".js_repl_history"));
    if let Some(ref p) = history_path {
        rl.load_history(p)?;
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

                rl.add_history_entry(buffer.clone())?;

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
                                eprintln!("    {line}");
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
                eprintln!("Readline error: {err}");
                break;
            }
        }
    }

    if let Some(ref p) = history_path {
        rl.save_history(p)?;
    }
    Ok(())
}
