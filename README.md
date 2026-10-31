# JavaScript Engine in Rust

[![Crates.io](https://img.shields.io/crates/v/javascript.svg)](https://crates.io/crates/javascript)
[![Documentation](https://docs.rs/javascript/badge.svg)](https://docs.rs/javascript)
[![License](https://img.shields.io/crates/l/javascript.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024%2B-blue.svg)](https://www.rust-lang.org/)
[![Build Status](https://img.shields.io/github/actions/workflow/status/ssrlive/javascript/rust.yml)](https://github.com/ssrlive/javascript/actions)
[![Test262](https://img.shields.io/github/actions/workflow/status/ssrlive/javascript/test262-all.yml?label=test262-all)](https://github.com/ssrlive/javascript/actions/workflows/test262-all.yml)
[![Downloads](https://img.shields.io/crates/d/javascript.svg)](https://crates.io/crates/javascript)

A JavaScript engine written in Rust. Source text is tokenized, parsed, compiled to bytecode, and executed by a bytecode VM:

Source -> Lexer -> Parser -> Compiler -> VM

The repository contains:

- A library crate: javascript
- A CLI example target: js-rs
- Rust integration tests and JavaScript self-tests
- A Node-based Test262 runner under ci/runner.js

## Current status

This project is actively developed. ECMAScript conformance is measured with the included Test262 runner; support is still incomplete.

Important: decorators are implemented for classes, methods, and fields, while support for advanced decorator semantics continues to expand.

Important: Proper tail calls (tail call optimization) are supported for strict-mode functions.

## Language mode policy

This engine is intentionally not a legacy-web-quirk-compatible sloppy-mode runtime.

- This engine does not explicitly target full, historical sloppy-mode compatibility.
- Legacy web quirks and edge-case loose-mode behaviors are not a primary compatibility goal.
- In practice, strict and modern ECMAScript behavior is prioritized over preserving legacy oddities.

## Highlights

- Bytecode VM architecture (not AST tree-walking execution)
- Optional Cranelift JIT for a restricted subset of numeric functions, with interpreter fallback
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
- src/core/vm.rs and src/core/vm/: VM execution and ECMAScript builtins
- src/core/value.rs: runtime value model
- src/runtime/evaluation.rs: script/module evaluation and module-graph setup
- src/runtime/mod.rs: embeddable runtime API and host integration
- src/host_api.rs: host-facing values, callbacks, module loader, and module registry
- src/lib.rs: crate public API re-exports
- js/main.rs: CLI entry point and REPL loop

## Build and run

### Build

```bash
cargo build --all-features --release
cargo build --example js-rs --release
cargo build --example js-rs --release --features jit

# Install the CLI into ~/.cargo/bin
cargo install --path . --example js-rs
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

# Fail if an ordinary JavaScript function call falls back from the JIT
cargo run -r --example js-rs -- --jit-required path/to/script.js

# Start REPL
cargo run -r --example js-rs
```

### CLI options

```text
Usage: js-rs [OPTIONS] [FILE]

Options:
  -e, --eval <EVAL>
      --timer-wait-ms <TIMER_WAIT_MS>
  -b, --gc-opcode-budget <GC_OPCODE_BUDGET>
      --module
      --jit-required
  -h, --help
  -V, --version
```

`--timer-wait-ms` is currently accepted for compatibility but is not wired to runtime behavior.

### Optional JIT

Enable the experimental Cranelift JIT with `--features jit`. It compiles eligible
ordinary function declarations that use numeric parameters and supported local
arithmetic, comparisons, and `for` loops. Straight-line numeric functions also
have a direct bytecode-to-Cranelift path for constants, parameter locals,
arithmetic, and numeric returns. Other eligible functions still use the
restricted AST backend. The generated function is used after its second call
when all arguments are numbers. Unsupported functions, methods, closures, and
calls with non-numeric arguments continue to use the interpreter. This is an
experimental MVP, not a general-purpose optimizing JavaScript JIT.

For JIT tests, call `JsRuntime::set_jit_required(true)` to require both top-level
script bytecode and JavaScript function bodies to enter compiled code. Eligible
numeric calls use the existing optimizing backend immediately. Other supported
operations use a threaded baseline backend: Cranelift compiles blocks with
opcode-specific VM semantic helper calls, not an interpreter dispatch loop.
Methods, constructors, callbacks, closures, tagged values, and coercion side
effects use the same VM semantics as the interpreter. This is helper-backed
coverage, not a claim that those operations have optimized native lowering.

The CLI accepts `--jit-required`, and the Test262 runner accepts the same mode
for selected subsets, for example:

```bash
node ci/runner.js --jit-required --focus "language/expressions/addition"
```

This is a strict diagnostic mode, not a claim that the full Test262 suite is
JIT-compatible. Unsupported baseline opcodes and compilation errors fail instead
of falling back. The baseline table includes class heritage, prototypes, private
brands, super access, generator suspension/delegation, and async/await. Suspension
ends the native block; the VM's traced generator and async frames retain live
values and exception handlers until resumption. The two reserved disposal opcodes
remain explicitly unsupported (the compiler currently emits disposal inline).
Built-ins retain their runtime implementations.

JIT entries use a tagged call frame and an explicit return/throw/deopt/suspend
status ABI. The optimizing backend supports scalar values and rooted identity
passthrough, with restricted exception and closure paths. Baseline blocks keep
all live JavaScript values in traced VM storage; they return at opcode-budget
boundaries so incremental GC can run between slices. Pending throws, catch
stack cleanup, and reentrant callbacks preserve VM behavior.
Baseline native templates are reused for matching opcode sequences and operand
widths across bytecode locations; actual operands remain in the current chunk.
Templates contain at most eight opcodes to limit cold compilation work and
increase shape reuse; longer paths continue in another native template. The
final helper returns directly to the VM without redundant continuation checks.
Opcode counters are accumulated per native invocation, including errors and
reentrant calls, instead of updating the VM counter after every opcode.
This reduces duplicate compilation without specializing away observable effects.
Per-iteration lexical bindings detach the previous iteration's captured cell
and preserve its latest value in the local slot. A new cell is materialized
only when captured or explicitly boxed, avoiding GC allocation on each
iteration of an uncaptured numeric loop in both ordinary and required-JIT modes.
Cold paths retain the compact helper template; after 32 visits to a bytecode
location, eligible straight-line numeric expressions are tiered up to Cranelift
arithmetic and comparisons. A region checks its Number
inputs and remaining opcode budget once, keeps intermediate values in native
SSA registers, and commits only live stack outputs and a final local assignment.
Captured numeric locals are read/written at region boundaries; no JavaScript
callbacks or GC safepoints occur inside a region. Type, TDZ, alias or budget
guard failures execute the original compiled semantic-helper path without
replaying side effects. These regions do not yet keep values in registers
across arbitrary templates, calls or allocation.
Hot, balanced numeric loops with one conditional exit and one backedge have a
separate native tier: Number locals stay in SSA registers across iterations,
with no helper calls or VM stack traffic in the loop. Captured scalar locals
are synchronized at entry/exit; captured per-iteration bindings, observable
operations and unsupported control flow retain the compiled baseline path.
Native loop invocations consume at most the remaining opcode budget and 1024
iterations, then commit locals and resume at the precise bytecode boundary.
`native_loop_opcodes` counts this additional subset of baseline execution.
Ordinary mode also activates this tier after 32 backward jumps, including
inside generators whose complete functions are not eligible for optimization.
Cold and unsupported ordinary paths remain interpreted; required-JIT guard
failures stay in the compiled baseline.
`JsRuntime::jit_stats()` exposes baseline opcode, interpreter opcode, and
optimized-call counters, plus `native_region_opcodes` (a subset of baseline
opcodes executed by numeric regions), for verifying execution paths. With required JIT enabled
from runtime creation, the interpreter opcode count must remain zero, including
eval, retained child realms, and completed Test262 agents. Agent threads inherit
the spawning VM's required-JIT setting; their counters are included after they
finish, without double-counting nested agents.

CI runs the JavaScript self-tests in both ordinary and required-JIT modes on
Linux, Windows, and macOS. Required-JIT Test262 gates cover arithmetic, class
statements/expressions, generators, yield, and await; an empty selection fails
rather than passing vacuously. These gates retain the runner's strict-mode and
feature-probe exclusions, so passing a selected suite is not full conformance.
The `test262-all` workflow additionally runs both execution modes over the full
`annexB,staging/decorators`, `intl402`, `built-ins`, and `language` selections.
Each mode has separate result artifacts. The acceptance target is zero failures
across all four selections, not just the smaller gates above.
Set `TEST262_RESULTS_FILE` to retain distinct logs when running disjoint
selections locally.

```bash
node ci/runner.js --jit-required --fail-on-failure --limit 100000 \
  --focus "annexB,staging/decorators" --focus "intl402" \
  --focus "built-ins" --focus "language"
```

```bash
cargo test --features jit --test jit_tests
cargo test --features jit --test jit_coverage -- --nocapture
cargo build --release --example js-rs --features jit
```

The coverage test prints a row for every bytecode opcode and built-in function.
It distinguishes restricted native lowering, infrastructure, uncovered
operations, and an explicit `baseline=helper-backed` marker. Generic builtin
calls through baseline helpers do not count as specialized native builtin
coverage.
The audit requires an explicit baseline coverage decision for each opcode and
review when the builtin inventory changes. Class and suspension bridges name
their semantic regressions in the report. Helper-backed execution preserves
existing VM behavior, including its limitations; for example, foreign class
constructor `instanceof` is not generally correct even in interpreter mode.

## REPL behavior

- Exit with .exit or Ctrl-D
- Multiline input is supported for incomplete code
- History is stored at ~/.js_rs_history (resolved via dirs::home_dir())

## Library usage

Add dependency:

```toml
[dependencies]
javascript = "0.3.5"
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

`console` is not an ECMAScript intrinsic. The default Cargo feature set enables `console`, so `JsRuntime` supplies it as a host global (writing to stdout); disable it with `--no-default-features` or enable it explicitly with `--features console`. The default `node` feature is an umbrella for the independent `process`, `os`, and `fs` features. It provides the Node-style global `process` and the `process`, `os`, `fs`, and `fs/promises` built-in modules, imported using either bare names or `node:` names. These are engine host APIs, not ECMAScript intrinsics. Filesystem operations currently run synchronously on the host thread, including operations exposed through callback and promise APIs; those APIs do not perform background I/O. Callback APIs queue their callback for VM execution, while `fs/promises` returns a promise. `process.exit()` stops JavaScript execution and records an exit request without terminating the embedding process; embedders can inspect `JsRuntime::process_exit_requested()` and `JsRuntime::process_exit_code()`. Build without the host APIs using `--no-default-features`, or enable just selected modules with `--no-default-features --features process` (likewise `os` or `fs`). Hosts can redirect console output, provide in-memory module sources, own timer scheduling, and settle promises from external work:

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
    #[cfg(feature = "console")]
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

`start_eval` / `run_slice` let the host decide when execution advances; after the script completes, call `drain_microtasks` and `step_timers(monotonic_ms)` from the host event loop as needed. `create_promise` returns a JS promise and a resolver that the host can use to resolve/reject after I/O. Each `JsRuntime` owns its `$262.agent` state; parser and RegExp thread-local state remains scoped to the executing thread. Build the core without host-provided console or Node-style APIs using `cargo check --no-default-features --lib`.

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

- --focus (optional; supports multiple and comma-separated values; defaults to the standard Test262 suite directories)
- --limit
- --jobs
- --timeout
- --cap-multiplier
- --fail-on-failure
- --keep-tmp

The runner builds the engine, composes harness files, runs tests, and writes logs to `test262-results.log`. If `--focus` is omitted, it selects from `annexB`, `built-ins`, `intl402`, and `language`.

## Known gaps

As of this version:

- Cranelift JIT compilation is experimental and limited to eligible functions and
  supported operations; all other JavaScript continues to run on the bytecode
  interpreter. See [Optional JIT](#optional-jit) for the current scope.
- decorators: class, method, and field decorators are supported; advanced semantics may still be incomplete
- full historical sloppy-mode quirks: intentionally not a compatibility target

These remain spec-visible differences or limitations.

## Performance comparison

The standalone comparison script runs the same JavaScript workloads under Node.js and `js-rs`. It requires Python 3 and both executables on `PATH`:

```bash
./benches/compare_engines.py
```

Use `--runs`, `--warmup`, and `--iterations` to adjust the workload. It reports in-script and end-to-end timings and verifies matching checksums. The in-script timer uses JavaScript `Date.now()` (millisecond resolution), so increase `--iterations` when Node.js measurements are only a few milliseconds. Results are workload-specific and do not represent general performance.

To compare ordinary `js-rs` with required-JIT execution on the same workloads:

```bash
cargo build --release --example js-rs --all-features
python benches/compare_engines.py --js-rs target/release/examples/js-rs --compare-jit
```

Ordinary mode already optimizes eligible hot numeric functions. Required-JIT
mode additionally compiles cold script and function paths that ordinary mode
interprets. The helper-backed baseline adds compilation and per-opcode helper
costs, so Test262 throughput can be lower even when all tests pass. Conformance
coverage is not a speedup claim. Compare identical release builds, test
selections, job counts, and exclusions; use the in-script and end-to-end results
to distinguish hot execution from startup/compilation costs.
For Test262 comparisons of saved engine binaries, set `TEST262_BIN` to the
executable path to skip rebuilding, and use distinct `TEST262_RESULTS_FILE`
paths. The runner prints the resolved engine path; an invalid override fails
instead of selecting a different binary.
The `numeric_regions` benchmark uses a generator to exercise these regions
independently of the pre-existing whole-function numeric optimizer.

## Contributing

1. Run format, clippy, and tests before opening PRs.
2. Keep parser/compiler/vm changes accompanied by tests (Rust and/or JS scripts).
3. For conformance work, include focused Test262 repro paths in commit or PR notes.

## License

MIT. See LICENSE.
