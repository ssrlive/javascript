# JavaScript Engine in Rust

[![Crates.io](https://img.shields.io/crates/v/javascript.svg)](https://crates.io/crates/javascript)
[![Documentation](https://docs.rs/javascript/badge.svg)](https://docs.rs/javascript)
[![License](https://img.shields.io/crates/l/javascript.svg)](https://github.com/ssrlive/javascript/blob/master/LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024+-blue.svg)](https://www.rust-lang.org/)
[![Build Status](https://img.shields.io/github/actions/workflow/status/ssrlive/javascript/rust.yml)](https://github.com/ssrlive/javascript/actions)
[![Test262](https://img.shields.io/github/actions/workflow/status/ssrlive/javascript/test262.yml?label=test262)](https://github.com/ssrlive/javascript/actions/workflows/test262.yml)
[![Downloads](https://img.shields.io/crates/d/javascript.svg)](https://crates.io/crates/javascript)

A JavaScript engine written from scratch in Rust — lexer, parser, and **tree-walking interpreter** in
~105 000 lines of Rust — supporting ECMAScript 2024+ features including ES modules, async/await,
generators, Proxy/Reflect, TypedArray, SharedArrayBuffer, Atomics, WeakRef, FinalizationRegistry,
ShadowRealm, Iterator Helpers, Explicit Resource Management (`using`/`await using`), and more.

> **Strict mode only** — all scripts and `eval`'d code execute under ECMAScript strict semantics.

---

## Table of Contents

- [Highlights](#highlights)
- [Language Features](#language-features)
- [Built-in Objects & APIs](#built-in-objects--apis)
- [Architecture](#architecture)
- [Project Layout](#project-layout)
- [Installation](#installation)
- [Usage](#usage)
- [Test262 Conformance](#test262-conformance)
- [Testing](#testing)
- [Performance Notes](#performance-notes)
- [Limitations](#limitations)
- [Contributing](#contributing)
- [License](#license)
- [Acknowledgments](#acknowledgments)

---

## Highlights

| Aspect | Details |
|---|---|
| **Language** | Rust 2024 edition, safe & `Send`+`Sync` where needed |
| **Architecture** | Tree-walking AST interpreter (no bytecode / JIT) |
| **Memory** | GC-managed heap via [`gc-arena`](https://github.com/kyren/gc-arena) with reference-counted roots |
| **Regex** | [`regress`](https://github.com/ridiculousfish/regress) (Rust-native, full ES2024 regex including `v` flag, lookbehind, named groups, Unicode properties) |
| **Codebase** | ~105 K lines Rust source · ~11 K lines integration tests · 97 JS self-test scripts |
| **Test262** | 41-shard parallel CI against the official ECMAScript conformance suite (74 000+ test files) with 178 feature probes |
| **Platforms** | Linux, macOS, Windows (CI tested on all three) |
| **License** | MIT |

---

## Language Features

### Core (ES5 – ES2020)

- **Variables & Scoping** — `let`, `const`, `var` with block/function/module scoping, TDZ enforcement
- **Data Types** — Number (IEEE-754), String (UTF-16 internally), Boolean, BigInt, Symbol, `null`, `undefined`
- **Operators** — arithmetic, bitwise, logical, optional chaining (`?.`), nullish coalescing (`??`), logical assignments (`&&=`, `||=`, `??=`), exponentiation (`**`)
- **Control Flow** — `if`/`else`, `for`, `for-in`, `for-of`, `for-await-of`, `while`, `do-while`, `switch`, `try`/`catch`/`finally`, labeled statements, `break`/`continue`
- **Functions** — declarations, expressions, arrow (`=>`), default & rest parameters, spread, `new.target`
- **Async** — `async`/`await`, `async function*`, `for-await-of`, top-level `await` in modules
- **Generators** — `function*`, `yield`, `yield*`, full iterator protocol
- **Classes** — `class` / `extends`, `constructor`, static members, private fields/methods (`#name`), computed property names, class static blocks
- **Destructuring** — array and object patterns in declarations, assignments, function parameters; nested patterns, defaults, rest elements
- **Template Literals** — string interpolation and tagged templates
- **Modules** — `import`/`export`, `export default`, `import()` dynamic import, `import.meta`, `export * as ns`
- **Iterators** — `Symbol.iterator`, `for...of`, spread in arrays/function calls

### Modern (ES2021 – ES2024+)

- **Explicit Resource Management** — `using` / `await using` declarations, `DisposableStack`, `AsyncDisposableStack`, `Symbol.dispose` / `Symbol.asyncDispose`, `SuppressedError`
- **Iterator Helpers** — `Iterator.prototype.{map, filter, take, drop, flatMap, reduce, toArray, forEach, some, every, find}`, `Iterator.from`, `Iterator.concat`
- **ShadowRealm** — `new ShadowRealm()`, `realm.evaluate()`, `realm.importValue()`
- **WeakRef & FinalizationRegistry** — weak references and post-mortem cleanup callbacks
- **Array** — `findLast`/`findLastIndex`, `toReversed`/`toSorted`/`toSpliced`/`with` (change-by-copy), `Array.fromAsync`, grouping (`Object.groupBy`, `Map.groupBy`)
- **Hashbang** — `#!` scripts
- **RegExp** — `v` flag (set notation), `d` flag (match indices), duplicate named groups, modifiers, `dotAll`, lookbehind
- **`Error.cause`**, **`Error.isError`**
- **Promise** — `Promise.any`, `Promise.allSettled`, `Promise.withResolvers`, `Promise.try`, `Promise.prototype.finally`
- **Set methods** — `intersection`, `union`, `difference`, `symmetricDifference`, `isSubsetOf`, `isSupersetOf`, `isDisjointOf`
- **Uint8Array Base64** — `Uint8Array.fromBase64`, `Uint8Array.prototype.toBase64`, `Uint8Array.fromHex`, `Uint8Array.prototype.toHex`
- **JSON** — `JSON.parse` with source, well-formed `JSON.stringify`
- **String** — `isWellFormed`, `toWellFormed`, `matchAll`, `replaceAll`, `at`, `trimStart`/`trimEnd`
- **Symbols as WeakMap keys**
- **`ArrayBuffer.prototype.transfer`**, resizable ArrayBuffers
- **`Object.hasOwn`**, `Object.fromEntries`
- **Numeric separator** (`1_000_000`), `globalThis`

---

## Built-in Objects & APIs

| Category | Objects / Constructors |
|---|---|
| **Primitives** | `Number`, `String`, `Boolean`, `BigInt`, `Symbol` |
| **Collections** | `Array`, `Object`, `Map`, `Set`, `WeakMap`, `WeakSet` |
| **Typed Data** | `ArrayBuffer`, `SharedArrayBuffer`, `DataView`, `Int8Array`, `Uint8Array`, `Uint8ClampedArray`, `Int16Array`, `Uint16Array`, `Int32Array`, `Uint32Array`, `Float32Array`, `Float64Array` |
| **Async** | `Promise`, `AsyncFunction`, `AsyncGeneratorFunction` |
| **Iterators** | `Generator`, `AsyncGenerator`, `Iterator` (with helpers) |
| **Meta** | `Proxy`, `Reflect`, `ShadowRealm` |
| **Lifecycle** | `WeakRef`, `FinalizationRegistry`, `DisposableStack`, `AsyncDisposableStack` |
| **Concurrency** | `Atomics` (`load`, `store`, `add`, `sub`, `and`, `or`, `xor`, `exchange`, `compareExchange`, `wait`, `notify`, `waitAsync`, `isLockFree`) |
| **Errors** | `Error`, `TypeError`, `RangeError`, `ReferenceError`, `SyntaxError`, `URIError`, `EvalError`, `AggregateError`, `SuppressedError` |
| **Utilities** | `Math`, `Date`, `JSON`, `RegExp`, `console`, `eval` (direct & indirect) |
| **Timers** | `setTimeout`, `clearTimeout`, `setInterval`, `clearInterval` — backed by a dedicated timer thread; short-timer threshold configurable via `--timer-wait-ms` |
| **Globals** | `globalThis`, `parseInt`, `parseFloat`, `isNaN`, `isFinite`, `encodeURI`, `decodeURI`, `encodeURIComponent`, `decodeURIComponent` |
| **I/O** (feature-gated) | `os` module: `open`, `close`, `read`, `write`, `seek`, `remove`, `mkdir`, `readdir`, `stat`, `lstat`, `symlink`, `readlink`, `getcwd`, `realpath`, `exec`, `pipe`, `waitpid`, `kill`, `isatty`, `getpid`, `getppid`, `utimes` |
| | `os.path`: `join`, `dirname`, `basename`, `extname`, `resolve`, `normalize`, `relative` |
| | `std` module: `sprintf`, `tmpfile`, `loadFile`, `open`, `popen`, `fdopen`, `gc` |

---

## Architecture

```plain
Source Code
   │
   ▼
┌──────────┐    ┌──────────┐    ┌──────────────────┐    ┌────────────┐
│  Lexer   │───▸│  Parser  │───▸│  AST Evaluator   │───▸│   Value    │
│ (token.rs│    │(parser.rs│    │   (eval.rs)      │    │  (value.rs)│
│  2116 L) │    │  6598 L) │    │   (24 938 L)     │    │  (2592 L)  │
└──────────┘    └──────────┘    └──────────────────┘    └────────────┘
                                        │
                              ┌─────────┼──────────┐
                              ▼         ▼          ▼
                         ┌────────┐ ┌────────┐ ┌────────────┐
                         │ Event  │ │  GC    │ │  Built-in  │
                         │  Loop  │ │ Arena  │ │  Modules   │
                         │Promises│ │gc-arena│ │(40+ files) │
                         │Timers  │ │        │ │            │
                         └────────┘ └────────┘ └────────────┘
```

### Key Components

| Component | File(s) | Lines | Description |
|---|---|---|---|
| **Lexer** | `src/core/token.rs` | 2 116 | Tokenizes source into `TokenData` stream; handles template literals, regex literals, hashbang, Unicode escapes |
| **Parser** | `src/core/parser.rs` | 6 598 | Recursive-descent parser producing `Statement`/`Expr` AST nodes; supports full ES2024 grammar including classes, destructuring, async generators, `import`/`export` |
| **Evaluator** | `src/core/eval.rs` | 24 938 | Tree-walking interpreter: expression evaluation, statement execution, hoisting, scope chains, prototype chain resolution, `eval()` handling, module linking |
| **Value System** | `src/core/value.rs` | 2 592 | `Value<'gc>` enum with 30+ variants (Number, String, Object, Promise, Proxy, TypedArray, Generator, …); `JSObjectData` property map with descriptors |
| **Property Descriptors** | `src/core/descriptor.rs` | 411 | `[[Configurable]]`, `[[Enumerable]]`, `[[Writable]]`, `[[Get]]`, `[[Set]]` per ES spec |
| **GC Integration** | `src/core/gc.rs` | 187 | Manual `Trace` impls for AST nodes so the GC can walk live references |
| **Scope / Environment** | `src/core/mod.rs` | 1 136 | `JsArenaVm` creation, `initialize_global_constructors`, entry points |
| **Error System** | `src/error.rs` + `src/core/js_error.rs` | 854 | `JSError` / `EvalError` types with stack traces, line/column info |
| **Timer Thread** | `src/timer_thread.rs` | 102 | Dedicated background thread with a min-heap scheduler for `setTimeout`/`setInterval` |
| **REPL** | `src/repl.rs` | 342 | Persistent environment across evaluations; used by the CLI REPL |
| **Agent ($262.agent)** | `src/js_agent.rs` | 205 | Multi-threaded agent support for `SharedArrayBuffer`/`Atomics` testing; each agent gets its own GC arena on a dedicated OS thread |

### Memory Management

The engine uses [`gc-arena`](https://github.com/kyren/gc-arena) for garbage collection:

- All JS heap objects are `Gc<'gc, ...>` pointers rooted in a `JsArenaVm`
- A `GcContext` scope provides safe mutability
- Internal prototype chains use `Gc` pointers (no reference cycles thanks to tracing GC)
- `SharedArrayBuffer` backing stores use `Arc<Mutex<Vec<u8>>>` for cross-thread sharing

---

## Installation

### As a library

```toml
[dependencies]
javascript = "0.1.14"
```

Available Cargo features:

| Feature | Default | Description |
|---|---|---|
| `std` | ✅ | `std` module (`sprintf`, `tmpfile`, `loadFile`, …) |
| `os` | ✅ | `os` module (file I/O, process control, path utilities) |

### CLI binary

```bash
# From Git
cargo install js --git https://github.com/ssrlive/javascript.git

# From local checkout
cargo install js --path ./js
```

---

## Usage

### Evaluate a script (library)

```rust
use javascript::evaluate_script_with_vm;

let result = evaluate_script_with_vm(r#"
    let x = 42n;
    let y = x * 2n;
    y + 10n
"#, false, None::<&std::path::Path>).unwrap();

assert_eq!(result, "94");
```

### Evaluate an ES module

```rust,no_run
use javascript::evaluate_script_with_vm;

let result = evaluate_script_with_vm(r#"
    const greet = (name) => `Hello, ${name}!`;
    export default greet("world");
"#, true, None::<&std::path::Path>).unwrap();
```

### Use the `os` module

```rust
#[cfg(feature = "os")]
{
    use javascript::evaluate_script_with_vm;
    let result = evaluate_script_with_vm(r#"
        import * as os from "os";
        let cwd = os.getcwd();
        cwd
    "#, true, None::<&std::path::Path>).unwrap();
}
```

### Promises & async/await

```rust
use javascript::evaluate_script_with_vm;

let result = evaluate_script_with_vm(r#"
    async function fetchData() {
        return await Promise.resolve(42);
    }
    await fetchData()
"#, false, None::<&std::path::Path>).unwrap();

assert_eq!(result, "42");
```

### Timers

```rust
use javascript::evaluate_script_with_vm;

let result = evaluate_script_with_vm(r#"
    let id = setTimeout(() => console.log("fired"), 100);
    clearTimeout(id);
    undefined
"#, false, None::<&std::path::Path>).unwrap();
```

### Persistent REPL (library)

```rust
use javascript::Repl;

let mut repl = Repl::new();
repl.eval("let x = 10;").unwrap();
let result = repl.eval("x + 5").unwrap();
assert_eq!(result, "15");
```

### Tokenizer / Parser (library)

```rust
use javascript::{tokenize, parse_statements};

let tokens = tokenize("let x = 1 + 2;").unwrap();
let mut tokens_vec = tokens;
let mut index = 0;
let ast = parse_statements(&mut tokens_vec, &mut index).unwrap();
```

### CLI

```bash
# Run a script file
js script.js

# Run as ES module (auto-detected for .mjs files)
js module.mjs
js --module script.js

# Evaluate an expression
js -e "console.log(2 ** 10)"

# Start interactive REPL
js

# Configure short-timer threshold (ms)
js --timer-wait-ms 50 script.js
```

---

## Test262 Conformance

The engine is continuously validated against
[Test262](https://github.com/tc39/test262), the official ECMAScript conformance test suite
(74 000+ test files).

### CI Setup

- **41 parallel shards** in GitHub Actions, each targeting a different slice of the test suite
- **178 feature probe scripts** (`ci/feature_probes/`) — each tests whether the engine supports
  a given ES feature; tests requiring unsupported features are automatically skipped
- Results are aggregated into a summary with pass / fail / skip counts and a ranked
  list of unsupported features

### Running locally

```bash
# Prerequisite: clone test262 alongside this repo
# git clone https://github.com/tc39/test262.git ../test262

# Build the engine in release mode
cargo build -p js --release

# Run a custom focus
node ci/runner.js --limit 10000 --focus built-ins/Promise

# Run with extended timeout
node ci/runner.js --limit 10000 --focus language/literals --timeout 200
```

### Feature probes

Feature detection scripts live in `ci/feature_probes/`. They are tiny JS programs that
print `OK` on success, allowing `runner.js` to skip tests requiring features the engine
does not yet implement (e.g. `Temporal`, `decorators`, `tail-call-optimization`).

---

## Testing

### Rust unit & integration tests

```bash
# Full test suite
cargo test --all-features

# With logging
RUST_LOG=debug cargo test

# Run a specific test file
cargo test --test async_await_tests
```

### JS self-test scripts

97 standalone JavaScript files in `js-scripts/` exercise specific features end-to-end:

```bash
# Run one script
cargo run -p js -- js-scripts/promise_tests.js

# Run an ES module
cargo run -p js -- --module js-scripts/es6_module.mjs
```

These are also run in CI on all three platforms (Linux, macOS, Windows).

### Benchmarks

```bash
cargo bench                          # All benchmarks
cargo bench --bench promise_benchmarks
cargo bench --bench bigint_bench
```

---

## Performance Notes

This is a **tree-walking interpreter** — each AST node is visited and evaluated at runtime
with no bytecode compilation or JIT.

Typical overhead on a modern machine:

| Operation | Approximate cost |
|---|---|
| Loop iteration | ~5 µs |
| Method call (e.g. `Array.push`) | ~160 µs |
| Property access | ~10 µs |

This is perfectly adequate for test conformance, scripts, tooling, and embedding scenarios.
Code-intensive loops with millions of iterations (e.g. RegExp Unicode property-escape
generated tests that iterate over the full 0–0x10FFFF range) are intentionally skipped in CI.

---

## Limitations

- **No JIT / Bytecode** — interpretation only; hot loops are orders of magnitude slower than V8/SpiderMonkey
- **Strict mode only** — sloppy-mode–specific semantics (`with`, non-strict `arguments`, etc.) are not supported
- **No Web APIs** — no DOM, Fetch, WebSocket, `XMLHttpRequest`, or browser globals
- **No WebAssembly** — no WASM support
- **No Workers** — `SharedArrayBuffer` and `Atomics` work, but there is no `Worker` API; multi-agent support is limited to test262's `$262.agent` harness
- **No TypeScript** — no type checking or transpilation
- **No Source Maps** — no source-map-based debugging
- **Temporal** — the TC39 Temporal proposal (stage 3, 4 493 test262 tests) is not implemented
- **Decorators** — the decorators proposal is not implemented
- **Tail Call Optimization** — proper tail calls (PTC) are not implemented

---

## Contributing

Contributions are welcome! Areas where help is especially useful:

1. **Conformance** — fix failing test262 tests or implement missing built-in methods
2. **Performance** — profiling, optimizing hot paths, or exploring bytecode compilation
3. **Documentation** — API docs, tutorials, more examples
4. **Tooling** — source maps, debugger, TypeScript transpilation
5. **Fuzzing** — property-based / coverage-guided fuzz testing of the parser and evaluator

### Development Setup

```bash
git clone https://github.com/ssrlive/javascript.git
cd javascript

# Build & test
cargo test --all-features

# Lint
cargo clippy --all-features --all-targets -- -D warnings

# Format
cargo fmt --all

# Build release CLI
cargo build -p js --release
```

---

## License

MIT — see [LICENSE](LICENSE) for details.

## Acknowledgments

- Inspired by [QuickJS](https://bellard.org/quickjs/) — the `os` / `std` module API mirrors its design
- Built on Rust's type system and memory safety guarantees
- [`gc-arena`](https://github.com/kyren/gc-arena) by kyren for safe, performant tracing GC
- [`regress`](https://github.com/ridiculousfish/regress) for full ES-specification regex support
- The [Test262](https://github.com/tc39/test262) project for the definitive conformance test suite
- Thanks to the Rust ecosystem: `chrono`, `num-bigint`, `serde_json`, `crossbeam`, `indexmap`, and many more
