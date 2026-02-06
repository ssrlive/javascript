use javascript::*;

#[derive(clap::Parser)]
#[command(name = "js", version, about = "JavaScript Rust Interpreter")]
struct Cli {
    /// Execute script
    #[arg(short, long)]
    eval: Option<String>,

    /// JavaScript file to execute
    file: Option<std::path::PathBuf>,

    /// Milliseconds threshold for short timers which `evaluate_script` will wait for
    #[arg(long, default_value_t = 20)]
    timer_wait_ms: u64,

    /// Execute as an ES module (enables import/export handling)
    #[arg(long, default_value_t = false)]
    module: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    // Initialize logger (controlled by RUST_LOG)
    env_logger::init();

    #[cfg(windows)]
    {
        // Spawn a thread with larger stack size (8MB) to avoid stack overflow on Windows
        // where the default stack size is 1MB.
        let builder = std::thread::Builder::new().stack_size(8 * 1024 * 1024);
        let handler = builder.spawn(run_main)?;
        handler.join().unwrap()
    }

    #[cfg(unix)]
    run_main()
}

fn run_main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let cli = <Cli as clap::Parser>::parse();

    // Apply configured short-timer threshold so evaluate_script can decide which
    // timers to wait for before returning (default 20 ms).
    set_short_timer_threshold_ms(cli.timer_wait_ms);

    // If executing a file, mirror Node semantics by keeping the event loop alive
    // while there are active handles (timers, intervals). This allows scripts
    // that set intervals or long timeouts to keep the process running like Node.
    set_wait_for_active_handles(cli.file.is_some());

    let script_content = if let Some(ref script) = cli.eval {
        script.clone()
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
    let script_path = cli.file.as_ref().map(|p| std::fs::canonicalize(p).unwrap_or(p.clone()));

    let result = if cli.module {
        evaluate_module(&script_content, script_path.as_ref())
    } else {
        evaluate_script(&script_content, script_path.as_ref())
    };

    match result {
        Ok(result) => {
            if cli.eval.is_some() {
                println!("{result}");
            }
        }
        Err(err) => {
            if let Some(file_path) = script_path.as_ref()
                && let Some(line) = err.js_line()
            {
                eprintln!("{}:{}", file_path.display(), line);
                let lines: Vec<&str> = script_content.lines().collect();
                if line > 0 && line <= lines.len() {
                    eprintln!("{}", lines[line - 1]);
                    if let Some(col) = err.js_column()
                        && col > 0
                    {
                        eprintln!("{}^", " ".repeat(col - 1));
                    }
                }
                eprintln!();
            }

            eprintln!("{}", err.message());

            let stack = err.stack();
            if !stack.is_empty() {
                for frame in stack {
                    let formatted_frame = if let Some(file_path) = script_path.as_ref() {
                        frame.replace("(:", &format!("({}:", file_path.display()))
                    } else {
                        frame.clone()
                    };
                    eprintln!("    {}", formatted_frame);
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
        // Use ANSI escape codes for color: \x1b[1;32m is bold green, \x1b[1;33m is bold yellow, \x1b[0m is reset
        let prompt = if buffer.is_empty() {
            "\x1b[1;32mjs> \x1b[0m"
        } else {
            "\x1b[1;33m... \x1b[0m"
        };

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
                        let stack = e.stack();
                        if !stack.is_empty() {
                            eprintln!("Stack trace:");
                            for frame in stack {
                                eprintln!("    at {}", frame);
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
