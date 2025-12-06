#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use crate::error::JSError;
use crate::js_promise::{PromiseState, run_event_loop};
use crate::raise_eval_error;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

mod ffi;
pub use ffi::*;

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

pub fn evaluate_script<T: AsRef<str>>(script: T) -> Result<Value, JSError> {
    let script = script.as_ref();
    log::debug!("evaluate_script async called with script len {}", script.len());
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
    let env: JSObjectDataPtr = Rc::new(RefCell::new(JSObjectData::new()));
    env.borrow_mut().is_function_scope = true;

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
                        obj_set_value(&env, &name, Value::Object(crate::js_std::make_std_object()?))?;
                    } else if module == "os" {
                        obj_set_value(&env, &name, Value::Object(crate::js_os::make_os_object()?))?;
                    }
                }
            }
        }
    }

    // Initialize global built-in constructors
    initialize_global_constructors(&env)?;

    let v = evaluate_statements(&env, &statements)?;
    // If the result is a Promise object (wrapped in Object with __promise property), wait for it to resolve
    if let Value::Object(obj) = &v
        && let Some(promise_val_rc) = obj_get_value(obj, &"__promise".into())?
        && let Value::Promise(promise) = &*promise_val_rc.borrow()
    {
        // Run the event loop until the promise is resolved
        loop {
            run_event_loop()?;
            let promise_borrow = promise.borrow();
            match &promise_borrow.state {
                PromiseState::Fulfilled(val) => return Ok(val.clone()),
                PromiseState::Rejected(reason) => {
                    return Err(raise_eval_error!(format!("Promise rejected: {}", value_to_string(reason))));
                }
                PromiseState::Pending => {
                    // Continue running the event loop
                }
            }
        }
    }
    // Run the event loop to process any queued asynchronous tasks
    run_event_loop()?;
    Ok(v)
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
    Function(Vec<String>, Vec<Statement>),                // parameters, body
    AsyncFunction(Vec<String>, Vec<Statement>),           // parameters, body for async functions
    ArrowFunction(Vec<String>, Vec<Statement>),           // parameters, body
    AsyncArrowFunction(Vec<String>, Vec<Statement>),      // parameters, body for async arrow functions
    Object(Vec<(String, Expr)>),                          // object literal: key-value pairs
    Array(Vec<Expr>),                                     // array literal: [elem1, elem2, ...]
    Getter(Box<Expr>),                                    // getter function
    Setter(Box<Expr>),                                    // setter function
    Spread(Box<Expr>),                                    // spread operator: ...expr
    OptionalProperty(Box<Expr>, String),                  // optional property access: obj?.prop
    OptionalCall(Box<Expr>, Vec<Expr>),                   // optional call: obj?.method(args)
    OptionalIndex(Box<Expr>, Box<Expr>),                  // optional bracket access: obj?.[expr]
    Await(Box<Expr>),                                     // await expression
    This,                                                 // this keyword
    New(Box<Expr>, Vec<Expr>),                            // new expression: new Constructor(args)
    Super,                                                // super keyword
    SuperCall(Vec<Expr>),                                 // super() call in constructor
    SuperProperty(String),                                // super.property access
    SuperMethod(String, Vec<Expr>),                       // super.method() call
    ArrayDestructuring(Vec<DestructuringElement>),        // array destructuring: [a, b, ...rest]
    ObjectDestructuring(Vec<ObjectDestructuringElement>), // object destructuring: {a, b: c, ...rest}
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),         // conditional expression: condition ? trueExpr : falseExpr
    /// Regular expression literal: pattern, flags
    Regex(String, String),
    /// Logical operators with short-circuit semantics
    LogicalAnd(Box<Expr>, Box<Expr>),
    LogicalOr(Box<Expr>, Box<Expr>),
    BitXor(Box<Expr>, Box<Expr>),
    Value(Value), // literal value
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
    let chars: Vec<char> = script.chars().collect();
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
pub(crate) fn initialize_global_constructors(env: &JSObjectDataPtr) -> Result<(), JSError> {
    let mut env_borrow = env.borrow_mut();

    // Object constructor (object with static methods) and Object.prototype
    let object_obj = Rc::new(RefCell::new(JSObjectData::new()));

    // Add static Object.* methods (handlers routed by presence of keys)
    obj_set_value(&object_obj, &"keys".into(), Value::Function("Object.keys".to_string()))?;
    obj_set_value(&object_obj, &"values".into(), Value::Function("Object.values".to_string()))?;
    obj_set_value(&object_obj, &"assign".into(), Value::Function("Object.assign".to_string()))?;
    obj_set_value(&object_obj, &"create".into(), Value::Function("Object.create".to_string()))?;
    obj_set_value(
        &object_obj,
        &"getOwnPropertySymbols".into(),
        Value::Function("Object.getOwnPropertySymbols".to_string()),
    )?;
    obj_set_value(
        &object_obj,
        &"getOwnPropertyNames".into(),
        Value::Function("Object.getOwnPropertyNames".to_string()),
    )?;
    obj_set_value(
        &object_obj,
        &"getOwnPropertyDescriptors".into(),
        Value::Function("Object.getOwnPropertyDescriptors".to_string()),
    )?;

    // Create Object.prototype and add prototype-level helpers
    let object_prototype = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(
        &object_prototype,
        &"hasOwnProperty".into(),
        Value::Function("Object.prototype.hasOwnProperty".to_string()),
    )?;
    obj_set_value(
        &object_prototype,
        &"isPrototypeOf".into(),
        Value::Function("Object.prototype.isPrototypeOf".to_string()),
    )?;
    obj_set_value(
        &object_prototype,
        &"propertyIsEnumerable".into(),
        Value::Function("Object.prototype.propertyIsEnumerable".to_string()),
    )?;
    obj_set_value(
        &object_prototype,
        &"toString".into(),
        Value::Function("Object.prototype.toString".to_string()),
    )?;
    obj_set_value(
        &object_prototype,
        &"valueOf".into(),
        Value::Function("Object.prototype.valueOf".to_string()),
    )?;
    // Add toLocaleString to Object.prototype that delegates to toString/locale handling
    obj_set_value(
        &object_prototype,
        &"toLocaleString".into(),
        Value::Function("Object.prototype.toLocaleString".to_string()),
    )?;

    // wire prototype reference onto constructor
    obj_set_value(&object_obj, &"prototype".into(), Value::Object(object_prototype.clone()))?;

    // expose Object constructor as an object with static methods
    env_borrow.insert(
        PropertyKey::String("Object".to_string()),
        Rc::new(RefCell::new(Value::Object(object_obj))),
    );

    // Number constructor - handled by evaluate_var
    // env_borrow.insert(PropertyKey::String("Number".to_string()), Rc::new(RefCell::new(Value::Function("Number".to_string()))));

    // Boolean constructor
    env_borrow.insert(
        PropertyKey::String("Boolean".to_string()),
        Rc::new(RefCell::new(Value::Function("Boolean".to_string()))),
    );

    // String constructor
    env_borrow.insert(
        PropertyKey::String("String".to_string()),
        Rc::new(RefCell::new(Value::Function("String".to_string()))),
    );

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
    Ok(())
}
