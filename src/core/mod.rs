use crate::error::JSError;
use crate::js_array::initialize_array;
use crate::js_bigint::initialize_bigint;
use crate::js_console::initialize_console_object;
use crate::js_date::initialize_date;
use crate::js_json::initialize_json;
use crate::js_map::initialize_map;
use crate::js_math::initialize_math;
use crate::js_number::initialize_number_module;
use crate::js_regexp::initialize_regexp;
use crate::js_set::initialize_set;
use crate::js_string::initialize_string;
use crate::js_symbol::initialize_symbol;
use crate::js_weakmap::initialize_weakmap;
use crate::raise_eval_error;
use crate::unicode::utf8_to_utf16;
pub(crate) use gc_arena::GcWeak;
pub(crate) use gc_arena::Mutation as MutationContext;
pub(crate) use gc_arena::collect::Trace as GcTrace;
pub(crate) use gc_arena::lock::RefLock as GcCell;
pub(crate) use gc_arena::{Collect, Gc};
pub(crate) type GcPtr<'gc, T> = Gc<'gc, GcCell<T>>;
use std::collections::HashMap;

mod gc;

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

pub mod js_error;
pub use js_error::*;

#[derive(Collect)]
#[collect(no_drop)]
pub struct JsRoot<'gc> {
    pub global_env: JSObjectDataPtr<'gc>,
    pub well_known_symbols: Gc<'gc, GcCell<HashMap<String, GcPtr<'gc, Value<'gc>>>>>,
}

pub type JsArena = gc_arena::Arena<gc_arena::Rootable!['gc => JsRoot<'gc>]>;

pub fn initialize_global_constructors<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let object_ctor = new_js_object_data(mc);
    obj_set_key_value(mc, &object_ctor, &"__is_constructor".into(), Value::Boolean(true))?;

    let object_proto = new_js_object_data(mc);
    obj_set_key_value(mc, &object_ctor, &"prototype".into(), Value::Object(object_proto))?;
    obj_set_key_value(mc, &object_proto, &"constructor".into(), Value::Object(object_ctor))?;
    // Make prototype builtin properties non-enumerable
    object_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from("constructor"));

    let val = Value::Function("Object.prototype.toString".to_string());
    obj_set_key_value(mc, &object_proto, &"toString".into(), val)?;

    object_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from("toString"));

    env_set(mc, env, "Object", Value::Object(object_ctor))?;

    initialize_error_constructor(mc, env)?;

    let console_obj = initialize_console_object(mc)?;
    env_set(mc, env, "console", Value::Object(console_obj))?;

    initialize_number_module(mc, env)?;

    initialize_math(mc, env)?;
    initialize_string(mc, env)?;
    initialize_array(mc, env)?;
    // crate::js_function::initialize_function(mc, env)?;
    initialize_regexp(mc, env)?;
    // Initialize Date constructor and prototype
    initialize_date(mc, env)?;
    initialize_bigint(mc, env)?;
    initialize_json(mc, env)?;
    initialize_map(mc, env)?;
    initialize_weakmap(mc, env)?;
    initialize_symbol(mc, env)?;
    initialize_set(mc, env)?;

    env_set(mc, env, "undefined", Value::Undefined)?;
    env_set(mc, env, "NaN", Value::Number(f64::NAN))?;
    env_set(mc, env, "Infinity", Value::Number(f64::INFINITY))?;
    env_set(mc, env, "eval", Value::Function("eval".to_string()))?;

    #[cfg(feature = "os")]
    crate::js_os::initialize_os_module(mc, env)?;

    #[cfg(feature = "std")]
    crate::js_std::initialize_std_module(mc, env)?;

    Ok(())
}

pub fn evaluate_script<T, P>(script: T, script_path: Option<P>) -> Result<String, JSError>
where
    T: AsRef<str>,
    P: AsRef<std::path::Path>,
{
    let script = script.as_ref();
    let mut tokens = tokenize(script)?;
    if tokens.last().map(|td| td.token == Token::EOF).unwrap_or(false) {
        tokens.pop();
    }
    let mut index = 0;
    let mut statements = parse_statements(&tokens, &mut index)?;
    // DEBUG: show parsed statements for troubleshooting
    log::trace!("PARSED STATEMENTS: {:#?}", statements);

    let arena = JsArena::new(|mc| {
        let global_env = new_js_object_data(mc);
        global_env.borrow_mut(mc).is_function_scope = true;

        JsRoot {
            global_env,
            well_known_symbols: Gc::new(mc, GcCell::new(HashMap::new())),
        }
    });

    arena.mutate(|mc, root| {
        initialize_global_constructors(mc, &root.global_env)?;
        env_set(mc, &root.global_env, "globalThis", Value::Object(root.global_env))?;
        if let Some(p) = script_path.as_ref() {
            let p_str = p.as_ref().to_string_lossy().to_string();
            // Store __filepath
            obj_set_key_value(mc, &root.global_env, &"__filepath".into(), Value::String(utf8_to_utf16(&p_str)))?;
        }
        match evaluate_statements(mc, &root.global_env, &mut statements) {
            Ok(result) => Ok(value_to_string(&result)),
            Err(e) => match e {
                EvalError::Js(js_err) => Err(js_err),
                EvalError::Throw(val, line, column) => {
                    let mut err = crate::raise_throw_error!(val);
                    if let Some((l, c)) = line.zip(column) {
                        err.set_js_location(l, c);
                    }
                    if let Value::Object(obj) = &val {
                        if let Some(stack_str) = obj.borrow().get_property("stack") {
                            let lines: Vec<String> = stack_str
                                .lines()
                                .map(|s| s.trim().to_string())
                                .filter(|s| s.starts_with("at "))
                                .collect();
                            err.inner.stack = lines;
                        }
                    }
                    Err(err)
                }
            },
        }
    })
}

// Helper to resolve a constructor's prototype object if present in `env`.
pub fn get_constructor_prototype<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    name: &str,
) -> Result<Option<JSObjectDataPtr<'gc>>, JSError> {
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
    match evaluate_expr(mc, env, &Expr::Var(name.to_string(), None, None)) {
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
// If the constructor.prototype is available, sets `obj.borrow_mut(mc).prototype`
// to that object. This consolidates the common pattern used when boxing
// primitives and creating instances.
pub fn set_internal_prototype_from_constructor<'gc>(
    mc: &MutationContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
    ctor_name: &str,
) -> Result<(), JSError> {
    if let Some(proto_obj) = get_constructor_prototype(mc, env, ctor_name)? {
        // set internal prototype pointer (store Weak to avoid cycles)
        obj.borrow_mut(mc).prototype = Some(proto_obj.clone());
    }
    Ok(())
}

// Helper to initialize a collection from an iterable argument.
// Used by Map, Set, WeakMap, WeakSet constructors.
pub fn initialize_collection_from_iterable<'gc, F>(args: &[Value<'gc>], constructor_name: &str, mut process_item: F) -> Result<(), JSError>
where
    F: FnMut(Value<'gc>) -> Result<(), JSError>,
{
    if args.is_empty() {
        return Ok(());
    }
    if args.len() > 1 {
        let msg = format!("{constructor_name} constructor takes at most one argument",);
        return Err(raise_eval_error!(msg));
    }
    let iterable = args[0].clone();
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

/// Read a script file from disk and decode it into a UTF-8 Rust `String`.
/// Supports UTF-8 (with optional BOM) and UTF-16 (LE/BE) with BOM.
pub fn read_script_file<P: AsRef<std::path::Path>>(path: P) -> Result<String, JSError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|e| raise_eval_error!(format!("Failed to read script file '{}': {e}", path.display())))?;
    if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        // UTF-8 with BOM
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
