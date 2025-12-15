#![allow(clippy::collapsible_if, clippy::collapsible_match)]

use crate::error::JSError;
use crate::js_promise::{PromiseState, run_event_loop};
use crate::raise_eval_error;
use crate::unicode::utf8_to_utf16;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

mod value;
pub use value::*;

mod property_key;
pub use property_key::*;

mod statement;
pub use statement::*;

mod token;
pub use token::*;

mod number;

mod eval;
pub use eval::*;

mod parser;
pub use parser::*;

thread_local! {
    // Well-known symbols storage (iterator, toStringTag, etc.)
    static WELL_KNOWN_SYMBOLS: RefCell<HashMap<String, Rc<RefCell<Value>>>> = RefCell::new(HashMap::new());
}

pub fn evaluate_script<T, P>(script: T, script_path: Option<P>) -> Result<Value, JSError>
where
    T: AsRef<str>,
    P: AsRef<std::path::Path>,
{
    let script = script.as_ref();
    log::debug!("evaluate_script async called with script len {}", script.len());
    log::trace!("evaluate_script: entry");
    let filtered = filter_input_script(script);
    log::trace!("filtered script:\n{}", filtered);
    let mut tokens = match tokenize(&filtered) {
        Ok(t) => t,
        Err(e) => {
            log::debug!("tokenize error: {e:?}");
            return Err(e);
        }
    };
    let statements = match parse_statements(&mut tokens) {
        Ok(s) => s,
        Err(e) => {
            log::debug!("parse_statements error: {e:?}");
            return Err(e);
        }
    };
    log::debug!("parsed {} statements", statements.len());
    for (i, stmt) in statements.iter().enumerate() {
        log::trace!("stmt[{i}] = {stmt:?}");
    }
    let env: JSObjectDataPtr = new_js_object_data();
    env.borrow_mut().is_function_scope = true;
    // Record a script name on the root environment so stack frames can include it.
    let path = script_path.map_or("<script>".to_string(), |p| p.as_ref().to_string_lossy().to_string());
    let _ = obj_set_key_value(&env, &"__script_name".into(), Value::String(utf8_to_utf16(&path)));

    // Inject simple host `std` / `os` shims when importing with the pattern:
    //   import * as NAME from "std";
    for line in script.lines() {
        let l = line.trim();
        if l.starts_with("import * as")
            && l.contains("from")
            && let (Some(as_idx), Some(from_idx)) = (l.find("as"), l.find("from"))
        {
            let name_part = &l[as_idx + 2..from_idx].trim();
            let name = PropertyKey::String(name_part.trim().to_string());
            if let Some(start_quote) = l[from_idx..].find(|c: char| ['"', '\''].contains(&c)) {
                let quote_char = l[from_idx + start_quote..].chars().next().unwrap();
                let rest = &l[from_idx + start_quote + 1..];
                if let Some(end_quote) = rest.find(quote_char) {
                    let module = &rest[..end_quote];
                    if module == "std" {
                        obj_set_key_value(&env, &name, Value::Object(crate::js_std::make_std_object()?))?;
                    } else if module == "os" {
                        obj_set_key_value(&env, &name, Value::Object(crate::js_os::make_os_object()?))?;
                    }
                }
            }
        }
    }

    // Initialize global built-in constructors
    initialize_global_constructors(&env)?;

    // Expose `globalThis` binding to the global environment (points to the global object)
    obj_set_key_value(&env, &"globalThis".into(), Value::Object(env.clone()))?;

    let v = evaluate_statements(&env, &statements)?;
    // If the result is a Promise object (wrapped in Object with __promise property), wait for it to resolve
    if let Value::Object(obj) = &v
        && let Some(promise_val_rc) = obj_get_key_value(obj, &"__promise".into())?
        && let Value::Promise(promise) = &*promise_val_rc.borrow()
    {
        // Run the event loop until the promise is resolved
        loop {
            run_event_loop()?;
            let promise_borrow = promise.borrow();
            match &promise_borrow.state {
                PromiseState::Fulfilled(val) => return Ok(val.clone()),
                PromiseState::Rejected(_reason) => {
                    log::trace!("evaluate_script: top-level promise is Rejected, running EXTRA_ITERATIONS");
                    // Give a few extra event-loop iterations a chance to run so any
                    // late-attached handlers (microtasks) can register and be
                    // scheduled. This reduces spurious uncaught rejections where
                    // a rejection is handled shortly after it occurs.
                    // Try up to a small number of extra iterations, breaking out
                    // early if handlers appear and get a chance to run.
                    const EXTRA_ITERATIONS: usize = 5;
                    for _ in 0..EXTRA_ITERATIONS {
                        // If there are already attached handlers, run the loop once
                        // to give them a chance to execute and settle the promise.
                        run_event_loop()?;
                        // If the promise is no longer rejected, we can continue
                        if let PromiseState::Pending | PromiseState::Fulfilled(_) = &promise.borrow().state {
                            break;
                        }
                        // If the promise has attached rejection handlers, run again
                        // to let queued rejection tasks execute.
                        if !promise.borrow().on_rejected.is_empty() {
                            run_event_loop()?;
                            break;
                        }
                    }
                    // Re-check the promise state after a chance to run tasks
                    let promise_borrow = promise.borrow();
                    if let PromiseState::Rejected(_reason) = &promise_borrow.state {
                        // Give some extra iterations to allow pending unhandled checks to settle
                        // (same logic used for non-promise top-level scripts). This gives
                        // late-attached handlers a chance to register before we surface
                        // the top-level rejection.
                        const EXTRA_UNHANDLED_ITER: usize = 5;
                        for _ in 0..EXTRA_UNHANDLED_ITER {
                            if crate::js_promise::pending_unhandled_count() == 0 {
                                break;
                            }
                            run_event_loop()?;
                        }
                        // If a recorded unhandled rejection exists, run a small
                        // deterministic final drain (multiple ticks) to let
                        // harness/late handlers register before we attempt to
                        // consume the recorded unhandled. Use `peek` so we do
                        // not consume the recorded slot prematurely.
                        const FINAL_DRAIN_ITER: usize = 5;
                        if crate::js_promise::peek_unhandled_rejection().is_some() {
                            log::trace!("evaluate_script: peek_unhandled_rejection -> Some; running final drain");
                            for _ in 0..FINAL_DRAIN_ITER {
                                run_event_loop()?;
                                // Wait until there are no pending unhandled checks and
                                // no queued tasks to give the harness a final chance
                                // to register handlers and flush logs.
                                if crate::js_promise::pending_unhandled_count() == 0 && crate::js_promise::task_queue_len() == 0 {
                                    break;
                                }
                            }
                        } else {
                            log::trace!("evaluate_script: peek_unhandled_rejection -> None");
                        }
                        // Run one extra event loop turn to advance the tick once more,
                        // giving late handlers a final chance to attach before consuming.
                        run_event_loop()?;
                        // Only surface the top-level rejected promise as an error if the
                        // promise machinery recorded it as an unhandled rejection. This
                        // prevents prematurely converting a rejected Promise into a
                        // thrown error when test harnesses attach late handlers.
                        if let Some(unhandled_reason) = crate::js_promise::take_unhandled_rejection() {
                            log::trace!("evaluate_script: consuming recorded unhandled rejection (deferring surfacing)");
                            // Log helpful info about the value recorded as unhandled
                            match &unhandled_reason {
                                Value::Object(obj) => {
                                    if let Ok(Some(ctor_rc)) = obj_get_key_value(obj, &"constructor".into()) {
                                        log::debug!("Top-level promise rejected with object whose constructor = {:?}", ctor_rc.borrow());
                                    } else {
                                        log::debug!("Top-level promise rejected with object ptr={:p}", Rc::as_ptr(obj));
                                    }
                                    if let Ok(Some(stack_val)) = obj_get_key_value(obj, &"stack".into()) {
                                        log::debug!("Top-level rejected object stack = {}", value_to_string(&stack_val.borrow()));
                                    }
                                }
                                _ => {
                                    log::debug!("Top-level promise rejected with value={}", value_to_string(&unhandled_reason));
                                }
                            }
                            // Defer surfacing the unhandled rejection; return normally
                            // so the script (and any final synchronous work) can complete
                            // like Node does, allowing harnesses to print summaries.
                            return Ok(Value::Undefined);
                        }

                        // No recorded unhandled rejection — assume the rejection was
                        // handled (or will be) and finish the script without surfacing
                        // a top-level thrown error.
                        log::debug!("Not surfacing top-level rejection: no recorded unhandled rejection");
                        return Ok(Value::Undefined);
                    }
                }
                PromiseState::Pending => {
                    // Continue running the event loop
                }
            }
        }
    }
    // Run the event loop to process any queued asynchronous tasks
    run_event_loop()?;
    // Give some extra iterations to allow pending unhandled checks to settle
    const EXTRA_UNHANDLED_ITER: usize = 3;
    for _ in 0..EXTRA_UNHANDLED_ITER {
        if crate::js_promise::pending_unhandled_count() == 0 {
            break;
        }
        run_event_loop()?;
    }
    // If an unhandled rejection was recorded by the promise machinery, give
    // a deterministic final drain (multiple ticks) to allow late-attached
    // handlers a final chance to register. Use `peek_unhandled_rejection()`
    // to avoid consuming the recorded value prematurely.
    const FINAL_DRAIN_ITER: usize = 5;
    if crate::js_promise::peek_unhandled_rejection().is_some() {
        for _ in 0..FINAL_DRAIN_ITER {
            run_event_loop()?;
            if crate::js_promise::pending_unhandled_count() == 0 && crate::js_promise::task_queue_len() == 0 {
                break;
            }
        }
        if crate::js_promise::take_unhandled_rejection().is_some() {
            log::debug!("Recorded unhandled rejection present after final chance; deferring surfacing");
            return Ok(Value::Undefined);
        }
    }
    Ok(v)
}

/// Read a script file from disk and decode it into a UTF-8 Rust `String`.
/// Supports UTF-8 (with optional BOM) and UTF-16 (LE/BE) with BOM.
pub fn read_script_file<P: AsRef<std::path::Path>>(path: P) -> Result<String, JSError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|e| raise_eval_error!(format!("Failed to read script file '{}': {e}", path.display())))?;
    if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        let s = std::str::from_utf8(&bytes[3..]).map_err(|e| raise_eval_error!(format!("Script file contains invalid UTF-8: {e}")))?;
        return Ok(s.to_string());
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        // UTF-16LE
        if (bytes.len() - 2) % 2 != 0 {
            return Err(raise_eval_error!("Invalid UTF-16LE script file length"));
        }
        let mut u16s = Vec::with_capacity((bytes.len() - 2) / 2);
        for chunk in bytes[2..].chunks(2) {
            let lo = chunk[0] as u16;
            let hi = chunk[1] as u16;
            u16s.push((hi << 8) | lo);
        }
        return String::from_utf16(&u16s).map_err(|e| raise_eval_error!(format!("Invalid UTF-16LE script file contents: {e}")));
    }
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        // UTF-16BE
        if (bytes.len() - 2) % 2 != 0 {
            return Err(raise_eval_error!("Invalid UTF-16BE script file length"));
        }
        let mut u16s = Vec::with_capacity((bytes.len() - 2) / 2);
        for chunk in bytes[2..].chunks(2) {
            let hi = chunk[0] as u16;
            let lo = chunk[1] as u16;
            u16s.push((hi << 8) | lo);
        }
        return String::from_utf16(&u16s).map_err(|e| raise_eval_error!(format!("Invalid UTF-16BE script file contents: {e}")));
    }
    // Otherwise assume UTF-8 without BOM
    std::str::from_utf8(&bytes)
        .map(|s| s.to_string())
        .map_err(|e| raise_eval_error!(format!("Script file contains invalid UTF-8: {e}")))
}

// Helper to ensure a constructor-like object exists in the root env.
// Creates an object, marks it with `marker_key` (e.g. "__is_string_constructor")
// creates an empty `prototype` object whose internal prototype points to
// `Object.prototype` when available, stores the constructor in `env` under
// `name`, and returns the constructor object pointer.
pub fn ensure_constructor_object(env: &JSObjectDataPtr, name: &str, marker_key: &str) -> Result<JSObjectDataPtr, JSError> {
    // If already present and is an object, return it
    if let Some(val_rc) = obj_get_key_value(env, &name.into())? {
        if let Value::Object(obj) = &*val_rc.borrow() {
            return Ok(obj.clone());
        }
    }

    let ctor = new_js_object_data();
    // mark constructor
    obj_set_key_value(&ctor, &marker_key.into(), Value::Boolean(true))?;
    // Generic constructor marker for typeof checks
    obj_set_key_value(&ctor, &"__is_constructor".into(), Value::Boolean(true))?;

    // create prototype object
    let proto = new_js_object_data();
    // link prototype.__proto__ to Object.prototype if available
    if let Some(object_ctor_val) = obj_get_key_value(env, &"Object".into())?
        && let Value::Object(object_ctor) = &*object_ctor_val.borrow()
        && let Some(obj_proto_val) = obj_get_key_value(object_ctor, &"prototype".into())?
        && let Value::Object(obj_proto_obj) = &*obj_proto_val.borrow()
    {
        proto.borrow_mut().prototype = Some(obj_proto_obj.clone());
    }

    obj_set_key_value(&ctor, &"prototype".into(), Value::Object(proto.clone()))?;
    // Ensure prototype.constructor points back to the constructor object
    obj_set_key_value(&proto, &"constructor".into(), Value::Object(ctor.clone()))?;

    obj_set_key_value(env, &name.into(), Value::Object(ctor.clone()))?;
    Ok(ctor)
}

// Helper to resolve a constructor's prototype object if present in `env`.
pub fn get_constructor_prototype(env: &JSObjectDataPtr, name: &str) -> Result<Option<JSObjectDataPtr>, JSError> {
    // First try to find a constructor object already stored in the environment
    if let Some(val_rc) = obj_get_key_value(env, &name.into())? {
        if let Value::Object(ctor_obj) = &*val_rc.borrow() {
            if let Some(proto_val_rc) = obj_get_key_value(ctor_obj, &"prototype".into())? {
                if let Value::Object(proto_obj) = &*proto_val_rc.borrow() {
                    return Ok(Some(proto_obj.clone()));
                }
            }
        }
    }

    // If not found, attempt to evaluate the variable to force lazy creation
    match evaluate_expr(env, &Expr::Var(name.to_string())) {
        Ok(Value::Object(ctor_obj)) => {
            if let Some(proto_val_rc) = obj_get_key_value(&ctor_obj, &"prototype".into())? {
                if let Value::Object(proto_obj) = &*proto_val_rc.borrow() {
                    return Ok(Some(proto_obj.clone()));
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

// Helper to set an object's internal prototype from a constructor name.
// If the constructor.prototype is available, sets `obj.borrow_mut().prototype`
// to that object. This consolidates the common pattern used when boxing
// primitives and creating instances.
pub fn set_internal_prototype_from_constructor(obj: &JSObjectDataPtr, env: &JSObjectDataPtr, ctor_name: &str) -> Result<(), JSError> {
    if let Some(proto_obj) = get_constructor_prototype(env, ctor_name)? {
        // set internal prototype pointer
        obj.borrow_mut().prototype = Some(proto_obj.clone());
    }
    Ok(())
}

// Helper to initialize a collection from an iterable argument.
// Used by Map, Set, WeakMap, WeakSet constructors.
pub fn initialize_collection_from_iterable<F>(
    args: &[Expr],
    env: &JSObjectDataPtr,
    constructor_name: &str,
    mut process_item: F,
) -> Result<(), JSError>
where
    F: FnMut(Value) -> Result<(), JSError>,
{
    if args.is_empty() {
        return Ok(());
    }
    if args.len() > 1 {
        let msg = format!("{constructor_name} constructor takes at most one argument",);
        return Err(raise_eval_error!(msg));
    }
    let iterable = evaluate_expr(env, &args[0])?;
    match iterable {
        Value::Object(obj) => {
            let mut i = 0;
            loop {
                let key = format!("{i}");
                if let Some(item_val) = obj_get_key_value(&obj, &key.into())? {
                    let item = item_val.borrow().clone();
                    process_item(item)?;
                } else {
                    break;
                }
                i += 1;
            }
            Ok(())
        }
        _ => Err(raise_eval_error!(format!("{constructor_name} constructor requires an iterable"))),
    }
}

#[derive(Debug, Clone)]
pub enum Expr {
    Number(f64),
    /// BigInt literal (string form)
    BigInt(String),
    StringLit(Vec<u16>),
    Boolean(bool),
    Var(String),
    Binary(Box<Expr>, BinaryOp, Box<Expr>),
    UnaryNeg(Box<Expr>),
    UnaryPlus(Box<Expr>),
    BitNot(Box<Expr>),
    LogicalNot(Box<Expr>),
    TypeOf(Box<Expr>),
    Delete(Box<Expr>),
    Void(Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),                   // target, value
    LogicalAndAssign(Box<Expr>, Box<Expr>),         // target, value
    LogicalOrAssign(Box<Expr>, Box<Expr>),          // target, value
    NullishAssign(Box<Expr>, Box<Expr>),            // target, value
    AddAssign(Box<Expr>, Box<Expr>),                // target, value
    SubAssign(Box<Expr>, Box<Expr>),                // target, value
    PowAssign(Box<Expr>, Box<Expr>),                // target, value
    MulAssign(Box<Expr>, Box<Expr>),                // target, value
    DivAssign(Box<Expr>, Box<Expr>),                // target, value
    ModAssign(Box<Expr>, Box<Expr>),                // target, value
    BitXorAssign(Box<Expr>, Box<Expr>),             // target, value
    BitAndAssign(Box<Expr>, Box<Expr>),             // target, value
    BitOrAssign(Box<Expr>, Box<Expr>),              // target, value
    LeftShiftAssign(Box<Expr>, Box<Expr>),          // target, value
    RightShiftAssign(Box<Expr>, Box<Expr>),         // target, value
    UnsignedRightShiftAssign(Box<Expr>, Box<Expr>), // target, value
    Increment(Box<Expr>),
    Decrement(Box<Expr>),
    PostIncrement(Box<Expr>),
    PostDecrement(Box<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Property(Box<Expr>, String),
    Call(Box<Expr>, Vec<Expr>),
    Function(Option<String>, Vec<(String, Option<Box<Expr>>)>, Vec<Statement>), // optional name, parameters, body
    AsyncFunction(Option<String>, Vec<(String, Option<Box<Expr>>)>, Vec<Statement>), // optional name, parameters, body for async functions
    GeneratorFunction(Option<String>, Vec<(String, Option<Box<Expr>>)>, Vec<Statement>), // optional name, parameters, body for generator functions
    ArrowFunction(Vec<(String, Option<Box<Expr>>)>, Vec<Statement>),                     // parameters, body
    AsyncArrowFunction(Vec<(String, Option<Box<Expr>>)>, Vec<Statement>),                // parameters, body for async arrow functions
    Object(Vec<(String, Expr)>),                                                         // object literal: key-value pairs
    Array(Vec<Expr>),                                                                    // array literal: [elem1, elem2, ...]
    Getter(Box<Expr>),                                                                   // getter function
    Setter(Box<Expr>),                                                                   // setter function
    Spread(Box<Expr>),                                                                   // spread operator: ...expr
    OptionalProperty(Box<Expr>, String),                                                 // optional property access: obj?.prop
    OptionalCall(Box<Expr>, Vec<Expr>),                                                  // optional call: obj?.method(args)
    OptionalIndex(Box<Expr>, Box<Expr>),                                                 // optional bracket access: obj?.[expr]
    Await(Box<Expr>),                                                                    // await expression
    Yield(Option<Box<Expr>>),                                                            // yield expression (optional value)
    YieldStar(Box<Expr>),                                                                // yield* expression (delegation)
    This,                                                                                // this keyword
    New(Box<Expr>, Vec<Expr>),                                                           // new expression: new Constructor(args)
    Super,                                                                               // super keyword
    SuperCall(Vec<Expr>),                                                                // super() call in constructor
    SuperProperty(String),                                                               // super.property access
    SuperMethod(String, Vec<Expr>),                                                      // super.method() call
    ArrayDestructuring(Vec<DestructuringElement>),                                       // array destructuring: [a, b, ...rest]
    ObjectDestructuring(Vec<ObjectDestructuringElement>),                                // object destructuring: {a, b: c, ...rest}
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>), // conditional expression: condition ? trueExpr : falseExpr
    /// Regular expression literal: pattern, flags
    Regex(String, String),
    /// Logical operators with short-circuit semantics
    LogicalAnd(Box<Expr>, Box<Expr>),
    LogicalOr(Box<Expr>, Box<Expr>),
    Comma(Box<Expr>, Box<Expr>), // comma operator: expr1, expr2
    Value(Value),                // literal value
}

#[derive(Debug, Clone)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Equal,
    StrictEqual,
    NotEqual,
    StrictNotEqual,
    LessThan,
    GreaterThan,
    LessEqual,
    GreaterEqual,
    InstanceOf,
    In,
    NullishCoalescing,
    Pow,
    BitXor,
    BitAnd,
    BitOr,
    LeftShift,
    RightShift,
    UnsignedRightShift,
}

#[derive(Debug, Clone)]
pub enum DestructuringElement {
    Variable(String, Option<Box<Expr>>),           // a or a = default
    NestedArray(Vec<DestructuringElement>),        // [a, b]
    NestedObject(Vec<ObjectDestructuringElement>), // {a, b}
    Rest(String),                                  // ...rest
    Empty,                                         // for skipped elements: [, b] = [1, 2]
}

#[derive(Debug, Clone)]
pub enum ObjectDestructuringElement {
    Property { key: String, value: DestructuringElement }, // a: b or a
    Rest(String),                                          // ...rest
}

pub(crate) fn filter_input_script(script: &str) -> String {
    // Remove comments and simple import lines that we've already handled via shim injection
    let mut filtered = String::new();
    let chars: Vec<char> = script.trim().chars().collect();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    let mut escape = false;

    while i < chars.len() {
        let ch = chars[i];

        // Handle escape sequences
        if escape {
            filtered.push(ch);
            escape = false;
            i += 1;
            continue;
        }
        if ch == '\\' {
            escape = true;
            filtered.push(ch);
            i += 1;
            continue;
        }

        // Handle quote states
        match ch {
            '\'' if !in_double && !in_backtick => {
                in_single = !in_single;
                filtered.push(ch);
                i += 1;
                continue;
            }
            '"' if !in_single && !in_backtick => {
                in_double = !in_double;
                filtered.push(ch);
                i += 1;
                continue;
            }
            '`' if !in_single && !in_double => {
                in_backtick = !in_backtick;
                filtered.push(ch);
                i += 1;
                continue;
            }
            _ => {}
        }

        // Only process comments when not inside quotes
        if !in_single && !in_double && !in_backtick {
            // Handle single-line comments: //
            if i + 1 < chars.len() && ch == '/' && chars[i + 1] == '/' {
                // Skip to end of line
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                // Don't add the newline yet, continue to next iteration
                continue;
            }

            // Handle multi-line comments: /* */
            if i + 1 < chars.len() && ch == '/' && chars[i + 1] == '*' {
                i += 2; // Skip /*
                while i + 1 < chars.len() {
                    if chars[i] == '*' && chars[i + 1] == '/' {
                        i += 2; // Skip */
                        break;
                    }
                    i += 1;
                }
                continue;
            }
        }

        // Handle regular characters and newlines
        filtered.push(ch);
        i += 1;
    }

    // Now process the filtered script line by line for import statements
    let mut final_filtered = String::new();
    for (i, line) in filtered.lines().enumerate() {
        // Split line on semicolons only when not inside quotes/backticks
        let mut current = String::new();
        let mut in_single = false;
        let mut in_double = false;
        let mut in_backtick = false;
        let mut escape = false;
        // track parts along with whether they were followed by a semicolon
        let mut parts: Vec<(String, bool)> = Vec::new();
        for ch in line.chars() {
            if escape {
                current.push(ch);
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                current.push(ch);
                continue;
            }
            match ch {
                '\'' if !in_double && !in_backtick => {
                    in_single = !in_single;
                    current.push(ch);
                    continue;
                }
                '"' if !in_single && !in_backtick => {
                    in_double = !in_double;
                    current.push(ch);
                    continue;
                }
                '`' if !in_single && !in_double => {
                    in_backtick = !in_backtick;
                    current.push(ch);
                    continue;
                }
                _ => {}
            }
            if ch == ';' && !in_single && !in_double && !in_backtick {
                parts.push((current.clone(), true));
                current.clear();
                continue;
            }
            current.push(ch);
        }
        // If there is a trailing part (possibly no trailing semicolon), add it
        if !current.is_empty() {
            parts.push((current, false));
        }

        for (part, had_semicolon) in parts.iter() {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            log::trace!("script part[{i}]='{p}'");
            if p.starts_with("import * as") && p.contains("from") {
                log::debug!("skipping import part[{i}]: \"{p}\"");
                continue;
            }
            final_filtered.push_str(p);
            // Re-add semicolon if the original part was followed by a semicolon
            if *had_semicolon {
                final_filtered.push(';');
            }
        }
        final_filtered.push('\n');
    }

    // Remove any trailing newline(s) added during filtering to avoid an extra
    // empty statement at the end when tokenizing/parsing.
    final_filtered.trim().to_string()
}

/// Initialize global built-in constructors in the environment
pub fn initialize_global_constructors(env: &JSObjectDataPtr) -> Result<(), JSError> {
    // Create Function constructor early
    let _function_ctor = ensure_constructor_object(env, "Function", "__is_function_constructor")?;

    // Create Error constructor object early so its prototype exists.
    let error_ctor = ensure_constructor_object(env, "Error", "__is_error_constructor")?;

    // Ensure Error.prototype.toString uses our handler
    if let Some(proto_val) = obj_get_key_value(&error_ctor, &"prototype".into())? {
        if let Value::Object(proto_obj) = &*proto_val.borrow() {
            obj_set_key_value(
                proto_obj,
                &"toString".into(),
                Value::Function("Error.prototype.toString".to_string()),
            )?;
        }
    }

    // Create common Error sub-constructors and point their prototype.toString to Error.prototype.toString
    let error_types = ["TypeError", "SyntaxError", "ReferenceError", "RangeError", "EvalError", "URIError"];
    for t in error_types.iter() {
        let ctor = ensure_constructor_object(env, t, &format!("__is_{}_constructor", t.to_lowercase()))?;
        // Mark as error constructor so evaluate_new handles it generically
        obj_set_key_value(&ctor, &"__is_error_constructor".into(), Value::Boolean(true))?;
        if let Some(proto_val) = obj_get_key_value(&ctor, &"prototype".into())? {
            if let Value::Object(proto_obj) = &*proto_val.borrow() {
                obj_set_key_value(
                    proto_obj,
                    &"toString".into(),
                    Value::Function("Error.prototype.toString".to_string()),
                )?;
            }
        }
    }

    let mut env_borrow = env.borrow_mut();

    // Object constructor (object with static methods) and Object.prototype
    let object_obj = new_js_object_data();
    obj_set_key_value(&object_obj, &"__is_constructor".into(), Value::Boolean(true))?;

    // Add static Object.* methods (handlers routed by presence of keys)
    obj_set_key_value(&object_obj, &"keys".into(), Value::Function("Object.keys".to_string()))?;
    obj_set_key_value(&object_obj, &"values".into(), Value::Function("Object.values".to_string()))?;
    obj_set_key_value(&object_obj, &"assign".into(), Value::Function("Object.assign".to_string()))?;
    obj_set_key_value(&object_obj, &"create".into(), Value::Function("Object.create".to_string()))?;
    obj_set_key_value(
        &object_obj,
        &"getOwnPropertySymbols".into(),
        Value::Function("Object.getOwnPropertySymbols".to_string()),
    )?;
    obj_set_key_value(
        &object_obj,
        &"getOwnPropertyNames".into(),
        Value::Function("Object.getOwnPropertyNames".to_string()),
    )?;
    obj_set_key_value(
        &object_obj,
        &"getOwnPropertyDescriptors".into(),
        Value::Function("Object.getOwnPropertyDescriptors".to_string()),
    )?;

    // Create Object.prototype and add prototype-level helpers
    let object_prototype = new_js_object_data();
    obj_set_key_value(
        &object_prototype,
        &"hasOwnProperty".into(),
        Value::Function("Object.prototype.hasOwnProperty".to_string()),
    )?;
    obj_set_key_value(
        &object_prototype,
        &"isPrototypeOf".into(),
        Value::Function("Object.prototype.isPrototypeOf".to_string()),
    )?;
    obj_set_key_value(
        &object_prototype,
        &"propertyIsEnumerable".into(),
        Value::Function("Object.prototype.propertyIsEnumerable".to_string()),
    )?;
    obj_set_key_value(
        &object_prototype,
        &"toString".into(),
        Value::Function("Object.prototype.toString".to_string()),
    )?;
    obj_set_key_value(
        &object_prototype,
        &"valueOf".into(),
        Value::Function("Object.prototype.valueOf".to_string()),
    )?;
    // Add toLocaleString to Object.prototype that delegates to toString/locale handling
    obj_set_key_value(
        &object_prototype,
        &"toLocaleString".into(),
        Value::Function("Object.prototype.toLocaleString".to_string()),
    )?;

    // wire prototype reference onto constructor
    obj_set_key_value(&object_obj, &"prototype".into(), Value::Object(object_prototype.clone()))?;

    // expose Object constructor as an object with static methods
    env_borrow.insert(
        PropertyKey::String("Object".to_string()),
        Rc::new(RefCell::new(Value::Object(object_obj))),
    );

    // Number constructor - handled by evaluate_var
    // env_borrow.insert(PropertyKey::String("Number".to_string()), Rc::new(RefCell::new(Value::Function("Number".to_string()))));

    // Boolean and String constructors are created lazily by `evaluate_var`
    // to allow creation of singleton constructor objects with prototypes.

    // Array constructor (already handled by js_array module)
    env_borrow.insert(
        PropertyKey::String("Array".to_string()),
        Rc::new(RefCell::new(Value::Function("Array".to_string()))),
    );

    // Date constructor (already handled by js_date module)
    env_borrow.insert(
        PropertyKey::String("Date".to_string()),
        Rc::new(RefCell::new(Value::Function("Date".to_string()))),
    );

    // RegExp constructor (already handled by js_regexp module)
    env_borrow.insert(
        PropertyKey::String("RegExp".to_string()),
        Rc::new(RefCell::new(Value::Function("RegExp".to_string()))),
    );

    // Symbol constructor
    env_borrow.insert(
        PropertyKey::String("Symbol".to_string()),
        Rc::new(RefCell::new(Value::Function("Symbol".to_string()))),
    );

    // Map constructor
    env_borrow.insert(
        PropertyKey::String("Map".to_string()),
        Rc::new(RefCell::new(Value::Function("Map".to_string()))),
    );

    // Set constructor
    env_borrow.insert(
        PropertyKey::String("Set".to_string()),
        Rc::new(RefCell::new(Value::Function("Set".to_string()))),
    );

    // Proxy constructor
    env_borrow.insert(
        PropertyKey::String("Proxy".to_string()),
        Rc::new(RefCell::new(Value::Function("Proxy".to_string()))),
    );

    // WeakMap constructor
    env_borrow.insert(
        PropertyKey::String("WeakMap".to_string()),
        Rc::new(RefCell::new(Value::Function("WeakMap".to_string()))),
    );

    // WeakSet constructor
    env_borrow.insert(
        PropertyKey::String("WeakSet".to_string()),
        Rc::new(RefCell::new(Value::Function("WeakSet".to_string()))),
    );

    // Create a few well-known symbols and store them in the well-known symbol registry
    WELL_KNOWN_SYMBOLS.with(|wk| {
        let mut map = wk.borrow_mut();
        // Symbol.iterator
        let iter_sym_data = Rc::new(SymbolData {
            description: Some("Symbol.iterator".to_string()),
        });
        map.insert("iterator".to_string(), Rc::new(RefCell::new(Value::Symbol(iter_sym_data.clone()))));

        // Symbol.toStringTag
        let tt_sym_data = Rc::new(SymbolData {
            description: Some("Symbol.toStringTag".to_string()),
        });
        map.insert("toStringTag".to_string(), Rc::new(RefCell::new(Value::Symbol(tt_sym_data.clone()))));
        // Symbol.toPrimitive
        let tp_sym_data = Rc::new(SymbolData {
            description: Some("Symbol.toPrimitive".to_string()),
        });
        map.insert("toPrimitive".to_string(), Rc::new(RefCell::new(Value::Symbol(tp_sym_data.clone()))));
    });

    // Internal promise resolution functions
    env_borrow.insert(
        PropertyKey::String("__internal_resolve_promise".to_string()),
        Rc::new(RefCell::new(Value::Function("__internal_resolve_promise".to_string()))),
    );
    env_borrow.insert(
        PropertyKey::String("__internal_reject_promise".to_string()),
        Rc::new(RefCell::new(Value::Function("__internal_reject_promise".to_string()))),
    );
    env_borrow.insert(
        PropertyKey::String("__internal_allsettled_state_record_fulfilled".to_string()),
        Rc::new(RefCell::new(Value::Function(
            "__internal_allsettled_state_record_fulfilled".to_string(),
        ))),
    );
    env_borrow.insert(
        PropertyKey::String("__internal_allsettled_state_record_rejected".to_string()),
        Rc::new(RefCell::new(Value::Function(
            "__internal_allsettled_state_record_rejected".to_string(),
        ))),
    );

    // Initialize TypedArray constructors
    let arraybuffer_constructor = crate::js_typedarray::make_arraybuffer_constructor()?;
    env_borrow.insert(
        PropertyKey::String("ArrayBuffer".to_string()),
        Rc::new(RefCell::new(Value::Object(arraybuffer_constructor))),
    );

    // SharedArrayBuffer constructor
    let shared_arraybuffer_constructor = crate::js_typedarray::make_sharedarraybuffer_constructor()?;
    env_borrow.insert(
        PropertyKey::String("SharedArrayBuffer".to_string()),
        Rc::new(RefCell::new(Value::Object(shared_arraybuffer_constructor))),
    );

    let dataview_constructor = crate::js_typedarray::make_dataview_constructor()?;
    env_borrow.insert(
        PropertyKey::String("DataView".to_string()),
        Rc::new(RefCell::new(Value::Object(dataview_constructor))),
    );

    let typedarray_constructors = crate::js_typedarray::make_typedarray_constructors()?;
    for (name, constructor) in typedarray_constructors {
        env_borrow.insert(PropertyKey::String(name), Rc::new(RefCell::new(Value::Object(constructor))));
    }

    // Atomics object
    let atomics_obj = crate::js_typedarray::make_atomics_object()?;
    env_borrow.insert(
        PropertyKey::String("Atomics".to_string()),
        Rc::new(RefCell::new(Value::Object(atomics_obj))),
    );

    // setTimeout function
    env_borrow.insert(
        PropertyKey::String("setTimeout".to_string()),
        Rc::new(RefCell::new(Value::Function("setTimeout".to_string()))),
    );

    // clearTimeout function
    env_borrow.insert(
        PropertyKey::String("clearTimeout".to_string()),
        Rc::new(RefCell::new(Value::Function("clearTimeout".to_string()))),
    );

    // Global NaN and Infinity properties
    env_borrow.insert(
        PropertyKey::String("NaN".to_string()),
        Rc::new(RefCell::new(Value::Number(f64::NAN))),
    );
    env_borrow.insert(
        PropertyKey::String("Infinity".to_string()),
        Rc::new(RefCell::new(Value::Number(f64::INFINITY))),
    );

    drop(env_borrow);

    // Fix up prototype chains
    // 1. Function.prototype should be the prototype of all constructors (including Function itself)
    if let Some(func_ctor_val) = obj_get_key_value(env, &"Function".into())? {
        if let Value::Object(func_ctor) = &*func_ctor_val.borrow() {
            if let Some(func_proto_val) = obj_get_key_value(func_ctor, &"prototype".into())? {
                if let Value::Object(func_proto) = &*func_proto_val.borrow() {
                    // Helper to set __proto__
                    let set_proto = |target: &JSObjectDataPtr| {
                        target.borrow_mut().prototype = Some(func_proto.clone());
                        let _ = obj_set_key_value(target, &"__proto__".into(), Value::Object(func_proto.clone()));
                    };

                    set_proto(func_ctor); // Function.__proto__ = Function.prototype

                    if let Some(obj_ctor_val) = obj_get_key_value(env, &"Object".into())? {
                        if let Value::Object(obj_ctor) = &*obj_ctor_val.borrow() {
                            set_proto(obj_ctor); // Object.__proto__ = Function.prototype

                            if let Some(obj_proto_val) = obj_get_key_value(obj_ctor, &"prototype".into())? {
                                if let Value::Object(obj_proto) = &*obj_proto_val.borrow() {
                                    // Fix Function.prototype.__proto__ -> Object.prototype
                                    func_proto.borrow_mut().prototype = Some(obj_proto.clone());
                                    let _ = obj_set_key_value(func_proto, &"__proto__".into(), Value::Object(obj_proto.clone()));

                                    // Fix Error.prototype.__proto__ -> Object.prototype
                                    if let Some(err_ctor_val) = obj_get_key_value(env, &"Error".into())? {
                                        if let Value::Object(err_ctor) = &*err_ctor_val.borrow() {
                                            set_proto(err_ctor); // Error.__proto__ = Function.prototype

                                            if let Some(err_proto_val) = obj_get_key_value(err_ctor, &"prototype".into())? {
                                                if let Value::Object(err_proto) = &*err_proto_val.borrow() {
                                                    err_proto.borrow_mut().prototype = Some(obj_proto.clone());
                                                    let _ =
                                                        obj_set_key_value(err_proto, &"__proto__".into(), Value::Object(obj_proto.clone()));
                                                }
                                            }
                                        }
                                    }

                                    // Fix sub-error constructors
                                    let error_types = ["TypeError", "SyntaxError", "ReferenceError", "RangeError", "EvalError", "URIError"];
                                    for t in error_types.iter() {
                                        if let Some(ctor_val) = obj_get_key_value(env, &t.to_string().into())? {
                                            if let Value::Object(ctor) = &*ctor_val.borrow() {
                                                set_proto(ctor);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
