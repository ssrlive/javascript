# JavaScript Engine in Rust

[![Crates.io](https://img.shields.io/crates/v/javascript.svg)](https://crates.io/crates/javascript)
[![Documentation](https://docs.rs/javascript/badge.svg)](https://docs.rs/javascript)
[![License](https://img.shields.io/crates/l/javascript.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024%2B-blue.svg)](https://www.rust-lang.org/)
[![Build Status](https://img.shields.io/github/actions/workflow/status/ssrlive/javascript/rust.yml)](https://github.com/ssrlive/javascript/actions)
[![Test262](https://img.shields.io/github/actions/workflow/status/ssrlive/javascript/test262.yml?label=test262)](https://github.com/ssrlive/javascript/actions/workflows/test262-all.yml)
[![Downloads](https://img.shields.io/crates/d/javascript.svg)](https://crates.io/crates/javascript)

A JavaScript engine written in Rust, with a full pipeline from source text to bytecode execution:

Source -> Lexer -> Parser -> Compiler -> VM

The repository contains:

- A library crate: javascript
- A CLI binary crate: js
- Rust integration tests and JavaScript self-tests
- A Node-based Test262 runner under ci/runner.js

## Current status

This project is actively developed and already passes a large portion of Test262 in practice.

Important: two well-known missing areas are still:

- decorators
- proper tail calls (tail call optimization)

If you need either today, use transpilation (for decorators) or avoid relying on spec-level PTC behavior.

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

- src/core/token.rs: lexer/tokenizer
- src/core/parser.rs: parser (AST construction)
- src/core/statement.rs: AST node definitions
- src/core/compiler.rs: AST to bytecode compilation
- src/core/opcode.rs: opcode definitions
- src/core/vm.rs and src/core/vm/: VM runtime and builtins
- src/core/value.rs: runtime value model
- src/core/mod.rs: top-level compile/run pipeline and public core entry points
- src/lib.rs: crate public API re-exports
- js/src/main.rs: CLI entry point and REPL loop

## Build and run

### Build

```bash
cargo build --all-features --release
cargo build -p js --release
```

### Run CLI

```bash
# Run a file
cargo run -r -p js -- path/to/script.js

# Run a module
cargo run -r -p js -- --module path/to/module.js
cargo run -r -p js -- path/to/module.mjs

# Evaluate inline code
cargo run -r -p js -- -e "1 + 2"

# Start REPL
cargo run -r -p js
```

### CLI options

```text
Usage: js [OPTIONS] [FILE]

Options:
  -e, --eval <EVAL>
      --timer-wait-ms <TIMER_WAIT_MS>
      --module
  -h, --help
  -V, --version
```

## REPL behavior

- Exit with .exit or Ctrl-D
- Multiline input is supported for incomplete code
- History is stored at ~/.js_repl_history (resolved via dirs::home_dir())

## Library usage

Add dependency:

```toml
[dependencies]
javascript = "0.1.14"
```

Basic script evaluation:

```rust
use javascript::evaluate_script;

fn main() {
    let out = evaluate_script("1 + 2", false, Option::<&std::path::Path>::None).unwrap();
    assert_eq!(out, "3");
}
```

Module evaluation:

```rust
use javascript::evaluate_script;

fn main() {
    let src = "export const x = 1;\n x";
    let out = evaluate_script(src, true, Option::<&std::path::Path>::None).unwrap();
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

- decorators: not implemented
- tail-call-optimization: not implemented
- full historical sloppy-mode quirks: intentionally not a compatibility target

These are common gaps across many real-world engines/runtimes as well, but they remain spec-visible differences.

## Contributing

1. Run format, clippy, and tests before opening PRs.
2. Keep parser/compiler/vm changes accompanied by tests (Rust and/or JS scripts).
3. For conformance work, include focused Test262 repro paths in commit or PR notes.

## License

MIT. See LICENSE.
