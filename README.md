# JavaScript Engine in Rust

[![Crates.io](https://img.shields.io/crates/v/javascript.svg)](https://crates.io/crates/javascript)
[![Documentation](https://docs.rs/javascript/badge.svg)](https://docs.rs/javascript)
[![License](https://img.shields.io/crates/l/javascript.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024%2B-blue.svg)](https://www.rust-lang.org/)
[![Build Status](https://img.shields.io/github/actions/workflow/status/ssrlive/javascript/rust.yml)](https://github.com/ssrlive/javascript/actions)
[![Test262](https://img.shields.io/github/actions/workflow/status/ssrlive/javascript/test262-all.yml?label=test262-all)](https://github.com/ssrlive/javascript/actions/workflows/test262-all.yml)
[![Downloads](https://img.shields.io/crates/d/javascript.svg)](https://crates.io/crates/javascript)

A JavaScript engine written in Rust, with a full pipeline from source text to bytecode execution:

Source -> Lexer -> Parser -> Compiler -> VM

The repository contains:

- A library crate: javascript
- A CLI example target: js-rs
- Rust integration tests and JavaScript self-tests
- A Node-based Test262 runner under ci/runner.js

## Current status

This project is actively developed and already passes a large portion of Test262 in practice.

Important: decorators are implemented for classes, methods, and fields, while support for advanced decorator semantics continues to expand.

Important: Proper tail calls (tail call optimization) are supported for strict-mode functions.

## Language mode policy

This engine is intentionally not a legacy-web-quirk-compatible sloppy-mode runtime.

- This engine does not explicitly target full, historical sloppy-mode compatibility.
- Legacy web quirks and edge-case loose-mode behaviors are not a primary compatibility goal.
- In practice, strict and modern ECMAScript behavior is prioritized over preserving legacy oddities.

## Highlights

- Bytecode VM architecture (not AST tree-walking execution)
- ECMAScript parser and compiler implemented in Rust
- Supports script and module execution
- Includes async-related features (Promises, async/await, top-level await in module mode)
- REPL with persistent command history
- Cross-platform CLI (Linux, macOS, Windows)

## Architecture

Core source layout:

- src/core/tokenizer.rs: lexer/tokenizer
- src/core/parser.rs: parser (AST construction)
- src/core/statement.rs: AST node definitions
- src/core/compiler.rs: AST to bytecode compilation
- src/core/opcode.rs: opcode definitions
- src/core/vm.rs and src/core/vm/: VM runtime and builtins
- src/core/value.rs: runtime value model
- src/core/mod.rs: top-level compile/run pipeline and public core entry points
- src/lib.rs: crate public API re-exports
- js/main.rs: CLI entry point and REPL loop

## Build and run

### Build

```bash
cargo build --all-features --release
cargo build --example js-rs --release
```

### Run CLI

```bash
# Run a file
cargo run -r --example js-rs -- path/to/script.js

# Run a module
cargo run -r --example js-rs -- --module path/to/module.js
cargo run -r --example js-rs -- path/to/module.mjs

# Evaluate inline code
cargo run -r --example js-rs -- -e "1 + 2"

# Tune GC safe-point frequency (positive opcode count; default: 4096)
cargo run -r --example js-rs -- --gc-opcode-budget 1024 path/to/script.js

# Start REPL
cargo run -r --example js-rs
```

### CLI options

```text
Usage: js [OPTIONS] [FILE]

Options:
  -e, --eval <EVAL>
      --timer-wait-ms <TIMER_WAIT_MS>
      --gc-opcode-budget <GC_OPCODE_BUDGET>
      --module
  -h, --help
  -V, --version
```

## REPL behavior

- Exit with .exit or Ctrl-D
- Multiline input is supported for incomplete code
- History is stored at ~/.js_rs_history (resolved via dirs::home_dir())

## Library usage

Add dependency:

```toml
[dependencies]
javascript = "0.1.14"
```

Basic script evaluation:

```rust
use javascript::{DEFAULT_GC_OPCODE_BUDGET, evaluate_script};

fn main() {
    let out = evaluate_script("1 + 2", false, Option::<&std::path::Path>::None, DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(out, "3");
}
```

Module evaluation:

```rust
use javascript::{DEFAULT_GC_OPCODE_BUDGET, evaluate_script};

fn main() {
    let src = "export const x = 1;\n x";
    let out = evaluate_script(src, true, Option::<&std::path::Path>::None, DEFAULT_GC_OPCODE_BUDGET).unwrap();
    println!("{out}");
}
```

Persistent REPL API:

```rust
use javascript::Repl;

fn main() {
    let mut repl = Repl::new();
    repl.eval("let a = 10;").unwrap();
    let out = repl.eval("a + 5").unwrap();
    assert_eq!(out, "15");
}
```

Embeddable `JsRuntime` API (persistent state, host callbacks, handles, and JSON conversion):

```rust
use javascript::{JsValue, JsRuntime};

fn main() {
    let mut rt = JsRuntime::new();
    rt.register_fn("hostAdd", |args| {
        let a = args.first().and_then(JsValue::as_number).unwrap_or(0.0);
        let b = args.get(1).and_then(JsValue::as_number).unwrap_or(0.0);
        Ok(JsValue::from(a + b))
    });

    rt.eval("function compute(x) { return hostAdd(x, 10); }").unwrap();
    let out = rt.call("compute", &[JsValue::from(32)]).unwrap();
    assert_eq!(out, JsValue::Number(42.0));
}
```

Hosts can provide in-memory module sources, own timer scheduling, redirect console output, and settle promises from external work:

```rust
use javascript::{HostHooks, JsRunStatus, JsRuntime, JsValue, ModuleLoader, ModuleSource};

struct Modules;

impl ModuleLoader for Modules {
    fn resolve(&mut self, specifier: &str, _referrer: Option<&str>) -> Result<String, javascript::JSError> {
        match specifier {
            "./math.js" => Ok("app/math.js".to_string()),
            _ => Err(javascript::JSError::new(
                javascript::JSErrorKind::EvaluationError {
                    message: format!("Unknown module: {specifier}"),
                },
                file!().to_string(),
                "Modules::resolve".to_string(),
                None,
            )),
        }
    }

    fn load(&mut self, key: &str) -> Result<ModuleSource, javascript::JSError> {
        match key {
            "app/math.js" => Ok(ModuleSource::Text("export const answer = 42;".to_string())),
            _ => Err(javascript::JSError::new(
                javascript::JSErrorKind::EvaluationError {
                    message: format!("Missing module: {key}"),
                },
                file!().to_string(),
                "Modules::load".to_string(),
                None,
            )),
        }
    }
}

fn run_hosted() -> Result<(), javascript::JSError> {
    let mut runtime = JsRuntime::new();
    runtime.set_module_loader(Modules);

    let mut hooks = HostHooks::default();
    hooks.set_console_handler(|message| eprintln!("{:?}: {}", message.level, message.text));
    runtime.set_host_hooks(hooks);

    runtime.eval_module(
        "import { answer } from './math.js'; console.log(answer);",
        "app/main.js",
    )?;

    runtime.start_eval("setTimeout(() => console.log('timer'), 100);")?;
    while matches!(runtime.run_slice(1024)?, JsRunStatus::Yielded) {}
    runtime.step_timers(100.0)?;
    Ok(())
}
```

`start_eval` / `run_slice` let the host decide when execution advances; after the script completes, call `drain_microtasks` and `step_timers(monotonic_ms)` from the host event loop as needed. `create_promise` returns a JS promise and a resolver that the host can use to resolve/reject after I/O. Each `JsRuntime` owns its `$262.agent` state; parser and RegExp thread-local state remains scoped to the executing thread. The `os` and `std` Cargo features remain optional; build the core without them using `cargo check --no-default-features --lib`.

Other exported APIs include tokenize, parse_statement, parse_statements, read_script_file, and value/string helpers.

## Testing and quality checks

### Rust tests

```bash
cargo test -r --all-features --tests
```

### Lint and format

```bash
cargo clippy -r --all-features --all-targets -- -D warnings
cargo fmt --all
```

### JS self-test sweep

```bash
# Uses bash + timeout logic and categorizes PASS/FAIL/SKIP/TIMEOUT
bash ./test-vm.sh
```

Windows note: run through bash (for example Git Bash), not directly as a PowerShell script.

## Test262 runner

Node-based runner:

```bash
# Requires Node.js
node ci/runner.js --limit 10000 --focus "language/expressions/addition"

# Run one focused case
node ci/runner.js --limit 1 --focus "language/literals/string/S7.8.4_A4.1_T1.js"
```

Key runner options:

- --focus (required, supports multiple and comma-separated values)
- --limit
- --jobs
- --timeout
- --fail-on-failure
- --keep-tmp

The runner builds the engine, composes harness files, runs tests, and writes logs to test262-results.log.

## Known gaps

As of current implementation:

- decorators: class, method, and field decorators are supported; advanced semantics may still be incomplete
- full historical sloppy-mode quirks: intentionally not a compatibility target

These are common gaps across many real-world engines/runtimes as well, but they remain spec-visible differences.

## Contributing

1. Run format, clippy, and tests before opening PRs.
2. Keep parser/compiler/vm changes accompanied by tests (Rust and/or JS scripts).
3. For conformance work, include focused Test262 repro paths in commit or PR notes.

## License

MIT. See LICENSE.
