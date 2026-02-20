# JavaScript Engine in Rust

[![Crates.io](https://img.shields.io/crates/v/javascript.svg)](https://crates.io/crates/javascript)
[![Documentation](https://docs.rs/javascript/badge.svg)](https://docs.rs/javascript)
[![License](https://img.shields.io/crates/l/javascript.svg)](https://github.com/ssrlive/javascript/blob/master/LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024+-blue.svg)](https://www.rust-lang.org/)
[![Build Status](https://img.shields.io/github/actions/workflow/status/ssrlive/javascript/rust.yml)](https://github.com/ssrlive/javascript/actions)
[![Tests](https://img.shields.io/badge/tests-passing-brightgreen.svg)](https://github.com/ssrlive/javascript/actions)
[![Downloads](https://img.shields.io/crates/d/javascript.svg)](https://crates.io/crates/javascript)

A JavaScript engine implementation written in Rust, providing a complete JavaScript runtime
with support for modern language features including ES6+ modules, async/await, BigInt, TypedArray, and more.

## Features

> Note: This engine runs in strict mode only — all scripts and eval'd code are executed using ECMAScript strict semantics.


### Core JavaScript Features (ES5-ES2020)
- **Variables and Scoping**: `let`, `const`, `var` declarations with proper scope rules
- **Data Types**: Numbers, strings, booleans, BigInt, symbols, objects, arrays, functions, classes
- **Control Flow**: `if/else`, loops (`for`, `while`, `do-while`), `switch`, `try/catch/finally`
- **Functions**: Regular functions, arrow functions, async/await, generators, parameters with defaults and rest/spread
- **Classes**: Class definitions, inheritance, constructors, static methods/properties, getters/setters
- **Promises**: Full Promise implementation with async task scheduling
- **Destructuring**: Array and object destructuring assignments
- **Template Literals**: String interpolation with embedded expressions
- **Optional Chaining**: Safe property access (`?.`)
- **Nullish Coalescing**: `??` operator and assignments (`??=`)
- **Logical Assignments**: `&&=`, `||=` operators
- **Modules**: ES6 `import`/`export` syntax and dynamic `import()`
- **Iterators**: Full `Symbol.iterator` support and `for...of` loops
- **Generators**: Generator functions (`function*`) and generator objects

### Built-in Objects and APIs
- **Array**: Complete array methods (`push`, `pop`, `map`, `filter`, `reduce`, etc.)
- **Object**: Property manipulation, prototype chains, static methods (`keys`, `values`, `assign`, etc.)
- **String**: String methods with UTF-16 support
- **Number**: Number parsing and formatting
- **BigInt**: Large integer arithmetic with Number interop
- **Math**: Mathematical functions and constants
- **Date**: Date/time handling (powered by chrono)
- **RegExp**: Regular expressions (powered by fancy-regex)
- **JSON**: JSON parsing and stringification
- **Promise**: Full Promise API with event loop
- **Symbol**: Symbol primitives including well-known symbols
- **Map/Set**: Map, Set, WeakMap, WeakSet collections
- **Proxy**: Complete proxy objects with revocable proxies
- **Reflect**: Full Reflect API
- **TypedArray**: All typed arrays (Int8Array, Uint8Array, Float32Array, etc.)
- **ArrayBuffer**: Binary data buffers
- **DataView**: Binary data views with endianness support
- **setTimeout/clearTimeout/setInterval/clearInterval**: Asynchronous timer functions with cancellation support (IDs are numeric). Long-running timers are scheduled on a background timer thread; short timers may be handled inline for tests. The short-timer wait threshold is configurable via `set_short_timer_threshold_ms` and the `--timer-wait-ms` CLI flag.
- **Error**: Error types and stack traces
- **OS**: File system operations and path manipulation

### Advanced Features
- **Event Loop**: Asynchronous task scheduling and execution
- **Memory Management**: Reference counting with garbage collection
- **FFI Integration**: C-compatible API similar to QuickJS
- **REPL**: Interactive persistent environment
- **Binary Data**: Complete TypedArray and DataView support

## Installation

Add this to your `Cargo.toml` (latest published version shown here):

```toml
[dependencies]
javascript = "0.1.14"
```

## Usage

### Basic Evaluation

```rust
use javascript::evaluate_script;

let result = evaluate_script(r#"
    let x = 42n;
    let y = x * 2n;
    y + 10n
"#, None::<&std::path::Path>).unwrap();

assert_eq!(result, "94");
```

### Using Built-in Modules

```rust
// The "os" module is only available when the "os" feature is enabled
#[cfg(feature = "os")]
{
    use javascript::evaluate_module;
    let result = evaluate_module(r#"
        import * as console from "console";
        import * as os from "os";

        console.log("Hello from JavaScript!");
        let cwd = os.getcwd();
        cwd
    "#, None::<&std::path::Path>).unwrap();
}
```

### Working with Promises

```rust,no_run
use javascript::evaluate_script;

let result = evaluate_script(r#"
    async function example() {
        let promise = new Promise((resolve) => {
            setTimeout(() => resolve("Done!"), 100);
        });
        return await promise;
    }
    await example()
"#, None::<&std::path::Path>).unwrap();
// The engine automatically runs the event loop to resolve promises
```

### Using setTimeout / setInterval

```rust
use javascript::evaluate_script;

let result = evaluate_script(r#"
    let timeoutId = setTimeout(() => {
        console.log("Timeout executed!");
    }, 1000);

    // Cancel the timeout
    clearTimeout(timeoutId);

    // setInterval returns an id as well
    let intervalId = setInterval(() => { console.log('tick'); }, 1000);
    clearInterval(intervalId);

    // The script's final expression is returned by evaluate_script; if the
    // last statement returns a timer id you will see that numeric value. Use
    // `void` or an explicit `undefined` expression to avoid returning the id.
    undefined
"#, None::<&std::path::Path>).unwrap();
```

### Command Line Interface

The crate provides a CLI binary with REPL support:

#### `js` - Command-line interface with REPL
```bash
# Execute a script string
cargo run --package js -- -e "console.log('Hello World!')"

# Execute a JavaScript file
cargo run --package js -- script.js

# Start interactive REPL (persistent environment)
cargo run --package js
```

The REPL maintains state between evaluations, allowing you to define variables and functions that persist across multiple inputs.

## API Reference

### Core Functions

- `evaluate_script<T: AsRef<str>, P: AsRef<Path>>(code: T, script_path: Option<P>) -> Result<Value, JSError>`:
  Evaluate JavaScript code with optional script path for error reporting
- `tokenize(code: &str) -> Result<Vec<Token>, JSError>`: Perform lexical analysis
- `parse_statements(tokens: &mut Vec<Token>) -> Result<Vec<Statement>, JSError>`: Parse tokens into AST
- `Repl::new() -> Repl`: Create a new persistent REPL environment
- `Repl::eval(&self, code: &str) -> Result<Value, JSError>`: Evaluate code in REPL context

### Value Types

The engine uses a comprehensive `Value` enum to represent JavaScript values, including primitives
(numbers, strings, booleans), objects, functions, promises, symbols, BigInts, collections
(Map, Set, WeakMap, WeakSet), generators, proxies, and typed arrays.

## Architecture

The engine consists of several key components:

- **Lexer**: Converts source code to tokens (`tokenize`)
- **Parser**: Builds AST from tokens (`parse_statements`)
- **Evaluator**: Executes AST in managed environment (`evaluate_statements`)
- **Object System**: Reference-counted objects with prototype inheritance
- **Event Loop**: Handles async operations and promise resolution
- **Built-in Modules**: Standard library implementations (Array, Object, Math, etc.)
- **FFI Layer**: C-compatible interface for embedding

## Testing

### Unit Tests
Run the comprehensive test suite:

```bash
cargo test
```

Run with detailed logging:

```bash
RUST_LOG=debug cargo test
```

### Benchmarks
Run performance benchmarks:

```bash
cargo bench
```

## Performance

The engine is optimized for:
- Fast lexical analysis and parsing
- Efficient AST evaluation
- Minimal memory allocations during execution
- Reference-counted memory management
- Event loop for async operations

Benchmark results show competitive performance for interpretation workloads.

## Limitations

While the engine supports most modern JavaScript features, some areas are still developing:

- **Web APIs**: No DOM, Fetch, WebSocket, or browser-specific APIs
- **WebAssembly**: No WASM support
- **Workers**: No multi-threaded worker/agent support (SharedArrayBuffer and Atomics work on the main thread)
- **JIT Compilation**: Interpreted only (no just-in-time compilation)
- **TypeScript**: No type checking or compilation
- **Source Maps**: No source map support for debugging

## Contributing

Contributions are welcome! Areas for potential improvement:

- **Performance**: JIT compilation, optimization passes
- **Compatibility**: Additional Web APIs, Worker threads
- **Tooling**: TypeScript support, source maps, debugger
- **Testing**: More comprehensive test coverage, fuzzing
- **Documentation**: API docs, tutorials, examples

### Development Setup

```bash
# Clone the repository
git clone https://github.com/ssrlive/javascript.git
cd javascript

# Run tests
cargo test

# Run benchmarks
cargo bench

# Build documentation
cargo doc --open
```

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- Inspired by the QuickJS JavaScript engine
- Built with Rust's powerful type system and memory safety
- Uses excellent Rust crates: `chrono`, `fancy-regex`, `num-bigint`, `serde_json`, `thiserror`, etc.
- Thanks to the Rust community for outstanding tooling and libraries
