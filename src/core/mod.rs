use crate::error::JSError;
use crate::raise_eval_error;
pub(crate) use gc_arena::GcWeak;
pub(crate) use gc_arena::Mutation as GcContext;
pub(crate) use gc_arena::collect::Trace as GcTrace;
pub(crate) use gc_arena::lock::RefLock as GcCell;
pub(crate) use gc_arena::{Collect, Gc};
pub(crate) type GcPtr<'gc, T> = Gc<'gc, GcCell<T>>;

#[inline]
pub fn new_gc_cell_ptr<'gc, T: 'gc + Collect<'gc>>(ctx: &GcContext<'gc>, value: T) -> GcPtr<'gc, T> {
    Gc::new(ctx, GcCell::new(value))
}

mod gc;

mod value;
pub use value::*;

mod statement;
pub use statement::*;

mod token;
pub use token::*;

/// Prefix for internal private field/method keys to separate them from public
/// properties that happen to start with `#`.
pub const PRIVATE_KEY_PREFIX: &str = "\x00#";

/// Create an internal property key for a private class member.
/// The `name` argument should NOT include the `#` prefix.
pub fn make_private_key(name: &str) -> String {
    format!("{}{}", PRIVATE_KEY_PREFIX, name)
}

mod parser;
pub use parser::*;

pub mod js_error;

pub mod opcode;
pub use opcode::*;

pub mod vm;
pub use vm::*;

pub mod compiler;
pub use compiler::*;

pub type JsArenaVm = gc_arena::Arena<gc_arena::Rootable!['gc => VM<'gc>]>;

fn extract_injected_module_filepath(script: &str) -> Option<String> {
    let marker = "globalThis.__filepath = \"";
    let start = script.find(marker)? + marker.len();
    let rest = &script[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn script_declares_await_identifier(s: &str) -> bool {
    s.contains("function await")
        || s.contains("function await(")
        || s.contains("var await")
        || s.contains("let await")
        || s.contains("const await")
        || s.contains("class await")
}

fn parse_program_statements(script: &str, run_as_module: bool) -> Result<Vec<Statement>, JSError> {
    let mut tokens = tokenize(script)?;
    if tokens.last().map(|td| td.token == Token::EOF).unwrap_or(false) {
        tokens.pop();
    }

    let mut index = 0;
    if !run_as_module {
        let enable_top_level_await = !script_declares_await_identifier(script);
        if enable_top_level_await {
            crate::core::parser::push_await_context();
            let res = parse_statements(&tokens, &mut index);
            crate::core::parser::pop_await_context();
            res
        } else {
            parse_statements(&tokens, &mut index)
        }
    } else {
        crate::core::parser::push_await_context();
        let res = parse_statements(&tokens, &mut index);
        crate::core::parser::pop_await_context();
        res
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

pub fn evaluate_script_with_vm<T: AsRef<str>, P: AsRef<std::path::Path>>(
    script: T,
    run_as_module: bool,
    script_path: Option<P>,
) -> Result<String, JSError> {
    let script_str = script.as_ref();
    let statements = parse_program_statements(script_str, run_as_module)?;
    let script_path_buf = script_path.as_ref().map(|p| p.as_ref().to_path_buf());

    let mut arena = JsArenaVm::new(|ctx| VM::new(Chunk::new(), ctx));

    let result = arena.mutate_root(|ctx, vm| {
        if !crate::js_agent::is_agent_thread() {
            crate::js_agent::reset_agent_state();
        }

        let script_path_buf = if let Some(p) = script_path_buf.as_ref() {
            let mut p_str = p.to_string_lossy().to_string();
            if run_as_module && let Some(injected_path) = extract_injected_module_filepath(script_str) {
                p_str = injected_path;
            }

            Some(std::path::PathBuf::from(p_str))
        } else {
            None
        };

        let compiler = Compiler::new();
        let chunk = compiler.compile(&statements)?;
        vm.chunk = chunk;

        // In module mode, top-level `this` is undefined (not globalThis)
        if run_as_module {
            vm.set_module_this();
        }

        // let mut vm = VM::new(chunk, ctx);
        vm.set_source_context(script_str, script_path_buf.as_deref());
        let mut v = vm.run(ctx)?;

        // VM helper behavior: if top-level result is a settled Promise, expose its
        // resolved/rejected payload so tests can assert final values directly.
        for _ in 0..8 {
            let step = if let Value::VmObject(obj) = &v {
                let b = obj.borrow();
                let is_promise = matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "Promise");
                if is_promise {
                    let rejected = matches!(b.get("__promise_rejected__"), Some(Value::Boolean(true)));
                    let next = b.get("__promise_value__").cloned();
                    Some((rejected, next))
                } else {
                    None
                }
            } else {
                None
            };

            let Some((rejected, next)) = step else {
                break;
            };
            let Some(next) = next else {
                break;
            };

            if rejected && let Value::VmObject(obj) = &next {
                let b = obj.borrow();
                if let Some(Value::String(t)) = b.get("__type__") {
                    let tn = crate::unicode::utf16_to_utf8(t);
                    if tn == "Error" || tn.ends_with("Error") {
                        drop(b);
                        return Err(vm.vm_error_to_js_error(ctx, &next));
                    }
                }
            }
            v = next;
        }

        match v {
            Value::String(s) => {
                let s_utf8 = crate::unicode::utf16_to_utf8(&s);
                match serde_json::to_string(&s_utf8) {
                    Ok(quoted) => Ok(quoted),
                    Err(_) => Ok(format!("\"{}\"", s_utf8)),
                }
            }
            Value::VmArray(_) | Value::VmObject(_) => Ok(value_to_compact_result_string(&v)),
            _ => Ok(value_to_string(&v)),
        }
    });

    // Run incremental GC to reclaim unreachable objects before returning.
    arena.collect_debt();

    result
}
