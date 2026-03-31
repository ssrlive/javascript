# Copilot Instructions

## What This Project Is

A complete JavaScript engine written in Rust — a tree-walking bytecode interpreter targeting ECMAScript 2024+. It supports async/await, generators, classes, modules, closures, BigInt, TypedArrays, Proxy/Reflect, WeakRef, and more. It runs in strict mode only. Test262 conformance is validated against 74,000+ test files.

## Build, Test, Lint

```bash
# Build
cargo build --all-features
cargo build -p js --release

# Full test suite
cargo test --all-features --tests

# Single Rust integration test
cargo test --test builtin_functions -- test_name
cargo test --test promise_tests -- promise_resolve

# Lint / format
cargo clippy --all-features --all-targets -- -D warnings
cargo fmt --all

# Benchmarks
cargo bench
cargo bench --bench promise_benchmarks
cargo bench --bench bigint_bench

# Run a single JS script
cargo run -p js -- js-scripts/promise_tests.js
cargo run -p js -- --module js-scripts/es6_module.mjs

# Run all JS self-tests, categorized PASS/FAIL/SKIP/TIMEOUT
bash ./test-vm.sh

# Test262 CI runner (Node.js)
node ci/runner.js --limit 10000 --focus "language/expressions/addition"
node ci/runner.js --limit 1 --focus "language/literals/string"
```

## Architecture

Source code flows through four stages:

```
Source → Lexer (token.rs) → Parser (parser.rs) → Compiler (compiler.rs) → Chunk (bytecode) → VM (vm.rs)
```

| File | Role |
|---|---|
| `src/core/token.rs` | Tokenizer; produces `Vec<TokenData>` |
| `src/core/parser.rs` | Recursive-descent parser; produces `Vec<Statement>` AST |
| `src/core/statement.rs` | AST node definitions (`StatementKind`, `Expr` enums) |
| `src/core/compiler.rs` | Compiles AST to stack-based bytecode (`Chunk`) |
| `src/core/opcode.rs` | 90 opcode definitions |
| `src/core/vm.rs` | Executes bytecode; ~35k lines; contains all builtins |
| `src/core/value.rs` | `Value<'gc>` enum — all JS value types |
| `src/core/mod.rs` | Module wiring + public entry points |
| `src/lib.rs` | Crate public API (`evaluate_script_with_vm`, `Repl`, etc.) |
| `src/error.rs` | `JSError` / `EvalError` types |
| `src/repl.rs` | Persistent REPL environment |
| `src/js_bigint.rs` | BigInt implementation |
| `src/js_regexp.rs` | RegExp engine (uses `regress` crate) |
| `src/unicode.rs` | UTF-8 ↔ UTF-16 conversion |
| `js/src/main.rs` | CLI binary: file execution, eval, REPL |

### Entry Point

```rust
// Public API
evaluate_script_with_vm(source, run_as_module, script_path) -> Result<String, JSError>
```

Internally: parse → create GC arena → compile → `VM::run()` → format result.

### Memory / GC

All JS objects live behind `Gc<'gc, GcCell<...>>` pointers managed by `gc-arena`. Every type that can be inside a `Gc` must implement `gc_arena::Collect`. The lifetime `'gc` threads through all types that hold GC'd values.

## Key Conventions

### Value Representation

```rust
pub enum Value<'gc> {
    Number(f64),           // IEEE-754; no integer type
    String(Vec<u16>),      // UTF-16 internally
    BigInt(Box<BigInt>),
    Boolean(bool),
    Undefined,
    Null,
    VmObject(...),         // IndexMap<String, Value> for properties
    VmArray(...),          // Vec<Value> + named props
    VmFunction(...),       // bytecode function
    VmClosure(...),        // function + captured upvalues
    VmNativeFunction(FunctionID),  // builtin function by numeric ID
    Symbol(...),
    Property { value, getter, setter },  // internal property descriptor
}
```

Strings are `Vec<u16>` (UTF-16). Use `utf8_to_utf16` / `utf16_to_utf8` from `src/unicode.rs` to convert.

### Property Descriptors

Property descriptors are not a separate struct. They are encoded as `Value::Property { value, getter, setter }` and stored in the object's `IndexMap` alongside normal values. Getter/setter keys use the naming convention `__get_<key>` / `__set_<key>`.

### Opcode Handlers

Each opcode has a dedicated method on `VM`: `run_opcode_<name>`. The return type is `Result<OpcodeAction<'gc>, JSError>` where:

```rust
enum OpcodeAction<'gc> {
    Continue,          // VM loop advances ip and continues
    Exit(Value<'gc>),  // VM exits the current run loop with this value
}
```

Most handlers pop operands from `self.stack`, compute a result, push it back, and return `Ok(OpcodeAction::Continue)`. Handlers that terminate execution (e.g., `Return`, `Yield`) return `Ok(OpcodeAction::Exit(value))`.

### Builtin Functions

Builtins are identified by `FunctionID` (a `usize` constant, e.g., `BUILTIN_CONSOLE_LOG = 0`). All builtins are dispatched in `call_native_function` inside `vm.rs`. New builtins require:
1. A new `const BUILTIN_*: FunctionID` constant
2. A `Value::VmNativeFunction(BUILTIN_*)` entry set in `initialize_global_constructors`
3. A match arm in `call_native_function`

### Closures / Upvalues

The compiler tracks captured variables in `UpvalueInfo` entries and emits `MakeClosure` opcodes encoding the function index plus the list of captured variables. At runtime, captured variables live in `VmUpvalueCells` — shared mutable cells so mutations are reflected across all closures.

### Bytecode Encoding

`Chunk` stores a flat `Vec<u8>` code stream. Opcodes are emitted as single bytes; multi-byte operands follow immediately (e.g., `emit_u16` for constant table indices). The constant table (`chunk.constants: Vec<Value>`) holds literals referenced by index from the bytecode.

### Error Handling

All fallible operations return `Result<_, JSError>`. `JSError` carries kind (`JSErrorKind`), message, and optional source location. The `?` operator propagates errors up through the pipeline.

### Adding a New Opcode

1. Add variant to `Opcode` enum in `src/core/opcode.rs`
2. Add `compile_*` logic in `src/core/compiler.rs` to emit it
3. Add `run_opcode_*` handler in `src/core/vm.rs`
4. Add match arm in the main dispatch in `vm.rs`'s `run_inner` (or equivalent)

### Test Conventions

- **Rust integration tests** live in `tests/*.rs`; use the `assert_eval_eq!(js_source, expected)` macro which calls `evaluate_script_with_vm` and compares the string result.
- **JS self-tests** live in `js-scripts/*.js`; they `throw` on failure or rely on exit code. Run one with `cargo run -p js -- js-scripts/<file>.js`.
- **Test262** is run via `node ci/runner.js`; feature probes in `ci/feature_probes/` auto-skip tests for unsupported features.
