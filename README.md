# JavaScript Engine in Rust

[![Crates.io](https://img.shields.io/crates/v/javascript.svg)](https://crates.io/crates/javascript)
[![Documentation](https://docs.rs/javascript/badge.svg)](https://docs.rs/javascript)
[![License](https://img.shields.io/crates/l/javascript.svg)](https://github.com/ssrlive/javascript/blob/master/LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024+-blue.svg)](https://www.rust-lang.org/)
[![Build Status](https://img.shields.io/github/actions/workflow/status/ssrlive/javascript/rust.yml)](https://github.com/ssrlive/javascript/actions)
[![Tests](https://img.shields.io/badge/tests-passing-brightgreen.svg)](https://github.com/ssrlive/javascript/actions)
[![Downloads](https://img.shields.io/crates/d/javascript.svg)](https://crates.io/crates/javascript)

A JavaScript engine implementation written in Rust, providing a complete JavaScript runtime
with support for modern language features.

## Features

### Core JavaScript Features
- **Variables and Scoping**: `let`, `const`, `var` declarations
- **Data Types**: Numbers, strings, booleans, objects, arrays, functions, classes
- **Control Flow**: `if/else`, loops (`for`, `while`, `do-while`), `switch`, `try/catch/finally`
- **Functions**: Regular functions, arrow functions, async/await
- **Classes**: Class definitions, inheritance, static methods/properties, getters/setters
- **Promises**: Promise creation, resolution, async/await syntax
- **Destructuring**: Array and object destructuring
- **Template Literals**: String interpolation
- **Optional Chaining**: Safe property access (`?.`)
- **Nullish Coalescing**: `??` operator and assignments (`??=`)
- **Logical Assignments**: `&&=`, `||=` operators

### Built-in Objects and APIs
- **Array**: Full array methods and static constructors
- **Object**: Property manipulation, prototype chain
- **String**: String methods and UTF-16 support
- **Number**: Number parsing and formatting
- **Math**: Mathematical functions and constants
- **Date**: Date/time handling with chrono integration
- **RegExp**: Regular expressions with regex crate
- **JSON**: JSON parsing and stringification
- **Console**: Logging and debugging utilities
- **OS**: File system operations, path manipulation
- **File**: File I/O operations

### Advanced Features
- **Modules**: Import/export system with `import * as name from "module"`
- **Event Loop**: Asynchronous task scheduling and execution
- **Memory Management**: Reference counting and garbage collection
- **FFI Integration**: C-compatible API similar to QuickJS

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
javascript = "0.1.0"
```

## Usage

### Basic Evaluation

```rust
use javascript::evaluate_script;

let result = evaluate_script(r#"
    let x = 42;
    let y = x * 2;
    y + 10
"#, None::<&std::path::Path>).unwrap();

match result {
    javascript::Value::Number(n) => println!("Result: {}", n), // Output: Result: 94
    _ => println!("Unexpected result"),
}
```

### Using Built-in Modules

```rust
use javascript::evaluate_script;

let result = evaluate_script(r#"
    import * as console from "console";
    import * as os from "os";

    console.log("Hello from JavaScript!");
    let cwd = os.getcwd();
    cwd
"#, None::<&std::path::Path>).unwrap();
```

### Command Line Interface

The crate provides an example CLI binary with REPL support:

#### `js` - Command-line interface with REPL
```bash
cargo run --example js -- -e "console.log('Hello World!')"
cargo run --example js script.js
cargo run --example js  # no args -> enter persistent REPL (state is retained across inputs)
```

## API Reference

### Core Functions

- `evaluate_script(code: &str) -> Result<Value, JSError>`: Evaluate JavaScript code
- `evaluate_script_async(code: &str) -> Result<Value, JSError>`: Evaluate with async support
- `tokenize(code: &str) -> Result<Vec<Token>, JSError>`: Lexical analysis

### FFI Interface (QuickJS-compatible)

- `JS_NewRuntime() -> *mut JSRuntime`: Create a new runtime
- `JS_NewContext(rt: *mut JSRuntime) -> *mut JSContext`: Create a context
- `JS_Eval(ctx, code, len, filename, flags) -> JSValue`: Evaluate code
- `JS_NewString(ctx, str) -> JSValue`: Create a string value
- `JS_DefinePropertyValue(ctx, obj, atom, val, flags) -> i32`: Define object property

### Value Types

The engine uses a `Value` enum to represent JavaScript values. See the source code for the complete definition, which includes variants for numbers, strings, objects, functions, promises, and more.

## Architecture

The engine consists of several key components:

- **Parser**: Converts JavaScript source code into an AST
- **Evaluator**: Executes the AST in a managed environment
- **Object System**: Reference-counted objects with prototype chains
- **Memory Management**: Custom allocators and garbage collection
- **FFI Layer**: C-compatible interface for embedding
- **Built-in Modules**: Standard library implementations

## Testing

Run the test suite:

```bash
cargo test
```

Run with logging:

```bash
RUST_LOG=debug cargo test
```

## Performance

The engine is optimized for:
- Fast parsing and evaluation
- Efficient memory usage with Rc<RefCell<>>
- Minimal allocations during execution
- QuickJS-compatible FFI for high-performance embedding

## Limitations

- No JIT compilation (interpreted only)
- Limited browser API compatibility
- Some ES6+ features may be incomplete
- Error handling could be more robust

## Contributing

Contributions are welcome! Areas for improvement:

- JIT compilation support
- More comprehensive test coverage
- Browser API compatibility
- Performance optimizations
- Additional language features

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- Inspired by the QuickJS JavaScript engine
- Built with Rust's powerful type system and memory safety guarantees
- Uses several excellent Rust crates: `regex`, `chrono`, `serde_json`, etc.