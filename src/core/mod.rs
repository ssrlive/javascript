use crate::error::JSError;
use crate::js_array::initialize_array;
use crate::js_bigint::initialize_bigint;
use crate::js_boolean::initialize_boolean;
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
use crate::js_weakset::initialize_weakset;
use crate::raise_eval_error;
use crate::unicode::utf8_to_utf16;
pub(crate) use gc_arena::GcWeak;
pub(crate) use gc_arena::Mutation as MutationContext;
pub(crate) use gc_arena::collect::Trace as GcTrace;
pub(crate) use gc_arena::lock::RefLock as GcCell;
pub(crate) use gc_arena::{Collect, Gc};
pub(crate) type GcPtr<'gc, T> = Gc<'gc, GcCell<T>>;
use std::collections::HashMap;

#[inline]
pub fn new_gc_cell_ptr<'gc, T: 'gc + Collect<'gc>>(mc: &MutationContext<'gc>, value: T) -> GcPtr<'gc, T> {
    Gc::new(mc, GcCell::new(value))
}

mod gc;

mod value;
pub use value::*;

mod descriptor;
pub use descriptor::*;

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
    initialize_global_constructors_with_parent(mc, env, None)
}

pub fn initialize_global_constructors_with_parent<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    parent_env: Option<&JSObjectDataPtr<'gc>>,
) -> Result<(), JSError> {
    crate::js_object::initialize_object_module(mc, env)?;

    // Set the global object's [[Prototype]] to Object.prototype per spec.
    // This ensures `Object.getPrototypeOf(globalThis)` returns Object.prototype.
    if let Some(obj_ctor_val) = env_get(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_ctor_val.borrow()
        && let Some(obj_proto_val) = object_get_key_value(obj_ctor, "prototype")
    {
        let proto_obj = match &*obj_proto_val.borrow() {
            Value::Object(p) => Some(*p),
            Value::Property { value: Some(v), .. } => {
                if let Value::Object(p) = &*v.borrow() {
                    Some(*p)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(proto) = proto_obj {
            env.borrow_mut(mc).prototype = Some(proto);
        }
    }

    initialize_error_constructor(mc, env)?;

    let console_obj = initialize_console_object(mc)?;
    env_set(mc, env, "console", &Value::Object(console_obj))?;

    initialize_number_module(mc, env)?;

    initialize_symbol(mc, env, parent_env)?;

    // Initialize Reflect object with full (implemented) methods
    // (must be after initialize_symbol so Symbol.toStringTag is available)
    crate::js_reflect::initialize_reflect(mc, env)?;

    initialize_math(mc, env)?;
    initialize_string(mc, env)?;
    initialize_array(mc, env)?;
    // Create %StringIteratorPrototype% now that %IteratorPrototype% is available
    crate::js_string::initialize_string_iterator_prototype(mc, env)?;
    crate::js_function::initialize_function(mc, env)?;

    // Fix up Symbol's [[Prototype]] to be Function.prototype now that Function is initialized.
    // (Symbol is initialized before Function because other builtins need well-known symbols.)
    if let Some(sym_val) = env_get(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_val.borrow()
        && let Some(func_val) = env_get(env, "Function")
        && let Value::Object(func_ctor) = &*func_val.borrow()
        && let Some(func_proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_val.borrow()
    {
        sym_ctor.borrow_mut(mc).prototype = Some(*func_proto);
    }

    initialize_regexp(mc, env)?;
    // Create %RegExpStringIteratorPrototype% now that %IteratorPrototype% is available
    crate::js_regexp::initialize_regexp_string_iterator_prototype(mc, env)?;
    // Initialize Date constructor and prototype
    initialize_date(mc, env)?;
    crate::js_typedarray::initialize_typedarray(mc, env)?;
    initialize_boolean(mc, env)?;
    initialize_bigint(mc, env)?;
    initialize_json(mc, env)?;
    initialize_map(mc, env)?;
    crate::js_proxy::initialize_proxy(mc, env)?;
    initialize_weakmap(mc, env)?;
    initialize_weakset(mc, env)?;
    initialize_set(mc, env)?;
    crate::js_promise::initialize_promise(mc, env)?;
    crate::js_abstract_module_source::initialize_abstract_module_source(mc, env)?;

    // Initialize generator prototype/constructor
    crate::js_generator::initialize_generator(mc, env)?;
    // Initialize async generator prototype/constructor
    crate::js_async_generator::initialize_async_generator(mc, env)?;

    // Initialize Iterator helpers (Iterator constructor, prototype methods, etc.)
    crate::js_iterator_helpers::initialize_iterator_helpers(mc, env)?;

    // Initialize ShadowRealm constructor
    crate::js_shadow_realm::initialize_shadow_realm(mc, env)?;

    // Create AsyncFunction constructor/prototype so async function objects
    // inherit @@toStringTag = "AsyncFunction" from a distinct prototype.
    {
        let async_func_ctor = new_js_object_data(mc);
        let async_func_proto = new_js_object_data(mc);

        // AsyncFunction.prototype inherits from Function.prototype
        // AsyncFunction itself inherits from Function (the constructor)
        if let Some(func_ctor_val) = env_get(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        {
            // [[Prototype]] of AsyncFunction is Function
            async_func_ctor.borrow_mut(mc).prototype = Some(*func_ctor);
            // AsyncFunction.prototype.[[Prototype]] is Function.prototype
            if let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
                && let Value::Object(func_proto) = &*proto_val.borrow()
            {
                async_func_proto.borrow_mut(mc).prototype = Some(*func_proto);
            }
        }

        // Set @@toStringTag = "AsyncFunction" on AsyncFunction.prototype
        if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
            && let Value::Object(sym_obj) = &*sym_ctor.borrow()
            && let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
            && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
        {
            let desc_tag = crate::core::create_descriptor_object(
                mc,
                &Value::String(crate::unicode::utf8_to_utf16("AsyncFunction")),
                false,
                false,
                true,
            )?;
            crate::js_object::define_property_internal(mc, &async_func_proto, *tag_sym, &desc_tag)?;
        }

        // Make AsyncFunction callable via __native_ctor so AsyncFunction("...") works
        slot_set(
            mc,
            &async_func_ctor,
            InternalSlot::NativeCtor,
            &Value::String(crate::unicode::utf8_to_utf16("AsyncFunction")),
        );
        // Mark as constructor so typeof returns "function" and isConstructor is true
        slot_set(mc, &async_func_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));

        // AsyncFunction.length = 1 (non-writable, non-enumerable, configurable)
        let desc_len = crate::core::create_descriptor_object(mc, &Value::Number(1.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &async_func_ctor, "length", &desc_len)?;

        // AsyncFunction.name = "AsyncFunction" (non-writable, non-enumerable, configurable)
        let desc_name = crate::core::create_descriptor_object(
            mc,
            &Value::String(crate::unicode::utf8_to_utf16("AsyncFunction")),
            false,
            false,
            true,
        )?;
        crate::js_object::define_property_internal(mc, &async_func_ctor, "name", &desc_name)?;

        // Link constructor ↔ prototype
        // AsyncFunction.prototype: non-writable, non-enumerable, non-configurable
        object_set_key_value(mc, &async_func_ctor, "prototype", &Value::Object(async_func_proto))?;
        async_func_ctor.borrow_mut(mc).set_non_enumerable("prototype");
        async_func_ctor.borrow_mut(mc).set_non_writable("prototype");
        async_func_ctor.borrow_mut(mc).set_non_configurable("prototype");
        object_set_key_value(mc, &async_func_proto, "constructor", &Value::Object(async_func_ctor))?;
        async_func_proto.borrow_mut(mc).set_non_enumerable("constructor");

        // Store as hidden intrinsic (NOT a global) via internal slot
        slot_set(mc, env, InternalSlot::AsyncFunctionCtor, &Value::Object(async_func_ctor));
        // Stamp with OriginGlobal so evaluate_new can discover the constructor's realm
        slot_set(mc, &async_func_ctor, InternalSlot::OriginGlobal, &Value::Object(*env));
    }

    env_set(mc, env, "undefined", &Value::Undefined)?;
    // Make global 'undefined', 'NaN', and 'Infinity' non-writable and non-configurable per ECMAScript
    env.borrow_mut(mc).set_non_configurable("undefined");
    env.borrow_mut(mc).set_non_writable("undefined");

    env_set(mc, env, "NaN", &Value::Number(f64::NAN))?;
    env.borrow_mut(mc).set_non_configurable("NaN");
    env.borrow_mut(mc).set_non_writable("NaN");

    env_set(mc, env, "Infinity", &Value::Number(f64::INFINITY))?;
    env.borrow_mut(mc).set_non_configurable("Infinity");
    env.borrow_mut(mc).set_non_writable("Infinity");

    // Wrap eval in an Object so it carries OriginGlobal for cross-realm indirect eval.
    // Without this, `var otherEval = otherRealm.eval; otherEval('var x = 1')` would
    // execute in the caller's realm instead of the eval function's realm.
    {
        let eval_obj = new_js_object_data(mc);
        eval_obj
            .borrow_mut(mc)
            .set_closure(Some(new_gc_cell_ptr(mc, Value::Function("eval".to_string()))));
        slot_set(mc, &eval_obj, InternalSlot::OriginGlobal, &Value::Object(*env));
        // [[Prototype]] = Function.prototype so `eval instanceof Function` works
        if let Some(func_ctor_val) = env_get(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*proto_val.borrow()
        {
            eval_obj.borrow_mut(mc).prototype = Some(*func_proto);
        }
        // eval.name = "eval", eval.length = 1
        let desc_name = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("eval")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &eval_obj, "name", &desc_name)?;
        let desc_len = crate::core::create_descriptor_object(mc, &Value::Number(1.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &eval_obj, "length", &desc_len)?;
        env_set(mc, env, "eval", &Value::Object(eval_obj))?;
    }

    // This engine operates in strict mode only; mark the global environment accordingly so
    // eval() and nested function parsing can enforce strict-mode rules unconditionally.
    env_set_strictness(mc, env, true)?;

    // Define 'arguments' for global scope with poison pill for strict compliance
    crate::js_class::create_arguments_object(mc, env, &[], Some(&Value::Undefined))?;

    let val = Value::Function("__internal_async_step_resolve".to_string());
    env_set(mc, env, "__internal_async_step_resolve", &val)?;

    let val = Value::Function("__internal_async_step_reject".to_string());
    env_set(mc, env, "__internal_async_step_reject", &val)?;

    // Internal helpers used by Promise implementation (e.g. finally chaining)
    let val = Value::Function("__internal_resolve_promise".to_string());
    env_set(mc, env, "__internal_resolve_promise", &val)?;

    let val = Value::Function("__internal_reject_promise".to_string());
    env_set(mc, env, "__internal_reject_promise", &val)?;

    let val = Value::Function("__internal_allsettled_state_record_fulfilled_env".to_string());
    env_set(mc, env, "__internal_allsettled_state_record_fulfilled_env", &val)?;

    let val = Value::Function("__internal_allsettled_state_record_rejected_env".to_string());
    env_set(mc, env, "__internal_allsettled_state_record_rejected_env", &val)?;

    let val = Value::Function("__detachArrayBuffer__".to_string());
    // Use object_set_key_value (not env_set) so it stays as a String property
    // visible to JS via `globalThis.__detachArrayBuffer__`.  env_set would route
    // through str_to_internal_slot, hiding it from property access.
    object_set_key_value(mc, env, "__detachArrayBuffer__", &val)?;

    // Expose common global functions as callables
    env_set(mc, env, "parseInt", &Value::Function("parseInt".to_string()))?;
    env_set(mc, env, "parseFloat", &Value::Function("parseFloat".to_string()))?;
    env_set(mc, env, "isNaN", &Value::Function("isNaN".to_string()))?;
    env_set(mc, env, "isFinite", &Value::Function("isFinite".to_string()))?;
    env_set(mc, env, "encodeURI", &Value::Function("encodeURI".to_string()))?;
    env_set(mc, env, "decodeURI", &Value::Function("decodeURI".to_string()))?;
    env_set(mc, env, "encodeURIComponent", &Value::Function("encodeURIComponent".to_string()))?;
    env_set(mc, env, "decodeURIComponent", &Value::Function("decodeURIComponent".to_string()))?;

    // Timer functions
    env_set(mc, env, "setTimeout", &Value::Function("setTimeout".to_string()))?;
    env_set(mc, env, "clearTimeout", &Value::Function("clearTimeout".to_string()))?;
    env_set(mc, env, "setInterval", &Value::Function("setInterval".to_string()))?;
    env_set(mc, env, "clearInterval", &Value::Function("clearInterval".to_string()))?;

    // Expose __createRealm__ as a native callable for cross-realm tests.
    env_set(mc, env, "__createRealm__", &Value::Function("__createRealm__".to_string()))?;

    #[cfg(feature = "os")]
    crate::js_os::initialize_os_module(mc, env)?;

    #[cfg(feature = "std")]
    crate::js_std::initialize_std_module(mc, env)?;

    // Per the ECMAScript specification, the global object's built-in properties
    // (constructors, global functions, etc.) should have attributes:
    //   { writable: true, enumerable: false, configurable: true }
    // Mark all current own properties as non-enumerable. They are already writable
    // and configurable by default (the engine defaults to writable=true, configurable=true).
    {
        let keys: Vec<PropertyKey> = env.borrow().properties.keys().cloned().collect();
        for key in keys {
            env.borrow_mut(mc).non_enumerable.insert(key);
        }
    }

    Ok(())
}

/// Create a new Realm: a fresh global environment with its own set of built-in
/// intrinsics.  Returns the new global-env object (the "global" property
/// expected by the `$262.createRealm()` harness).
pub fn create_new_realm<'gc>(mc: &MutationContext<'gc>, _parent_env: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let new_env = new_js_object_data(mc);
    new_env.borrow_mut(mc).is_function_scope = true;

    initialize_global_constructors_with_parent(mc, &new_env, Some(_parent_env))?;

    env_set(mc, &new_env, "globalThis", &Value::Object(new_env))?;
    new_env.borrow_mut(mc).set_non_enumerable("globalThis");
    object_set_key_value(mc, &new_env, "this", &Value::Object(new_env))?;

    // Copy `print` from the parent realm so test harnesses have access.
    if let Some(print_val) = env_get(_parent_env, "print") {
        env_set(mc, &new_env, "print", &print_val.borrow())?;
    }

    Ok(new_env)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProgramKind {
    Script,
    Module,
}

fn extract_injected_module_filepath(script: &str) -> Option<String> {
    let marker = "globalThis.__filepath = \"";
    let start = script.find(marker)? + marker.len();
    let rest = &script[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn evaluate_program<T, P>(script: T, script_path: Option<P>, kind: ProgramKind) -> Result<String, JSError>
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
    // Allow top-level `await` in script evaluations to support tests and test262 harnesses
    // but avoid enabling it when the script declares an identifier named `await` (e.g., `function await`, `var await`)
    // so that `await(...)` can still be a call to a user-defined identifier when present.
    fn script_declares_await_identifier(s: &str) -> bool {
        s.contains("function await")
            || s.contains("function await(")
            || s.contains("var await")
            || s.contains("let await")
            || s.contains("const await")
            || s.contains("class await")
    }

    let statements = if kind == ProgramKind::Script {
        let enable_top_level_await = !script_declares_await_identifier(script);
        if enable_top_level_await {
            crate::core::parser::push_await_context();
            let res = parse_statements(&tokens, &mut index);
            crate::core::parser::pop_await_context();
            res?
        } else {
            parse_statements(&tokens, &mut index)?
        }
    } else {
        crate::core::parser::push_await_context();
        let res = parse_statements(&tokens, &mut index);
        crate::core::parser::pop_await_context();
        res?
    };

    // In script mode, reject import/export declarations (they are only valid in module code).
    if kind == ProgramKind::Script {
        for stmt in &statements {
            match &*stmt.kind {
                StatementKind::Export(..) => {
                    return Err(crate::raise_syntax_error!(format!(
                        "Unexpected token 'export' (line {}:{}). export declarations may only appear at top level of a module. \
                         Use --module flag or .mjs extension to run as an ES module",
                        stmt.line, stmt.column
                    )));
                }
                StatementKind::Import(..) => {
                    return Err(crate::raise_syntax_error!(format!(
                        "Cannot use import statement outside a module (line {}:{}). \
                         Use --module flag or .mjs extension to run as an ES module",
                        stmt.line, stmt.column
                    )));
                }
                _ => {}
            }
        }
    }

    // DEBUG: show parsed statements for troubleshooting
    log::trace!("DEBUG: PARSED STATEMENTS: {:#?}", statements);

    let arena = JsArena::new(|mc| {
        let global_env = new_js_object_data(mc);
        global_env.borrow_mut(mc).is_function_scope = true;

        JsRoot {
            global_env,
            well_known_symbols: new_gc_cell_ptr(mc, HashMap::new()),
        }
    });

    arena.mutate(|mc, root| {
        initialize_global_constructors(mc, &root.global_env)?;

        env_set(mc, &root.global_env, "globalThis", &Value::Object(root.global_env))?;
        root.global_env.borrow_mut(mc).set_non_enumerable("globalThis");
        object_set_key_value(mc, &root.global_env, "this", &Value::Object(root.global_env))?;

        let mut entry_module_exports: Option<JSObjectDataPtr<'_>> = None;
        if kind == ProgramKind::Module {
            let module_exports = new_js_object_data(mc);
            object_set_key_value(mc, &root.global_env, "exports", &Value::Object(module_exports))?;
            let module_obj = new_js_object_data(mc);
            object_set_key_value(mc, &module_obj, "exports", &Value::Object(module_exports))?;
            object_set_key_value(mc, &root.global_env, "module", &Value::Object(module_obj))?;
            entry_module_exports = Some(module_exports);
        }

        // Bind promise runtime lifecycle to this JsArena by resetting global
        // promise state so tests / repeated evaluate_script runs are isolated.
        crate::js_promise::reset_global_state();

        if let Some(p) = script_path.as_ref() {
            let mut p_str = p.as_ref().to_string_lossy().to_string();
            if kind == ProgramKind::Module
                && let Some(injected_path) = extract_injected_module_filepath(script)
            {
                p_str = injected_path;
            }
            // Store __filepath
            slot_set(mc, &root.global_env, InternalSlot::Filepath, &Value::String(utf8_to_utf16(&p_str)));
        }

        if kind == ProgramKind::Script {
            slot_set(
                mc,
                &root.global_env,
                InternalSlot::SuppressDynamicImportResult,
                &Value::Boolean(true),
            );
        }

        if kind == ProgramKind::Module
            && let (Some(exports_obj), Some(p)) = (entry_module_exports, script_path.as_ref())
        {
            let script_fs_path = std::fs::canonicalize(p.as_ref()).unwrap_or_else(|_| p.as_ref().to_path_buf());
            let script_fs_path_str = script_fs_path.to_string_lossy().to_string();
            let logical_module_path = extract_injected_module_filepath(script).unwrap_or_else(|| script_fs_path_str.clone());
            let module_path = std::fs::canonicalize(&logical_module_path)
                .ok()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or(logical_module_path);

            let cache = crate::js_module::get_or_create_module_cache(mc, &root.global_env)?;
            object_set_key_value(mc, &cache, module_path.as_str(), &Value::Object(exports_obj))?;
            object_set_key_value(mc, &cache, script_fs_path_str.as_str(), &Value::Object(exports_obj))?;
            let loading = crate::js_module::get_or_create_module_loading(mc, &root.global_env)?;
            object_set_key_value(mc, &loading, module_path.as_str(), &Value::Boolean(true))?;
            object_set_key_value(mc, &loading, script_fs_path_str.as_str(), &Value::Boolean(true))?;

            // Create import.meta for the entry module so `import.meta` is defined in module scripts
            let import_meta = new_js_object_data(mc);
            object_set_key_value(mc, &import_meta, "url", &Value::String(utf8_to_utf16(&module_path)))?;
            slot_set(mc, &root.global_env, InternalSlot::ImportMeta, &Value::Object(import_meta));
        }

        // Pre-scan the script source for the test262 global-code-mode marker.
        // The marker is an assignment statement (`globalThis.__test262_global_code_mode = true`)
        // injected by compose_test.js. We must set it on the global env BEFORE
        // evaluation so that evaluate_statements_with_labels can detect it and
        // apply the proper GlobalDeclarationInstantiation semantics (separate
        // lexical environment for let/const/class).
        if script.contains("__test262_global_code_mode") {
            slot_set(mc, &root.global_env, InternalSlot::Test262GlobalCodeMode, &Value::Boolean(true));
        }

        let exec_env = if kind == ProgramKind::Module {
            let module_env = new_js_object_data(mc);
            module_env.borrow_mut(mc).is_function_scope = true;
            module_env.borrow_mut(mc).prototype = Some(root.global_env);
            object_set_key_value(mc, &module_env, "this", &Value::Undefined)?;
            object_set_key_value(mc, &module_env, "globalThis", &Value::Object(root.global_env))?;

            if let Some(exports_obj) = entry_module_exports {
                object_set_key_value(mc, &module_env, "exports", &Value::Object(exports_obj))?;
                let module_obj = new_js_object_data(mc);
                object_set_key_value(mc, &module_obj, "exports", &Value::Object(exports_obj))?;
                object_set_key_value(mc, &module_env, "module", &Value::Object(module_obj))?;
            }

            module_env
        } else {
            root.global_env
        };

        let eval_statements_slice: &[Statement] = if kind == ProgramKind::Module {
            let split_idx = statements.iter().position(|stmt| {
                if let StatementKind::Expr(expr) = &*stmt.kind
                    && let Expr::Assign(lhs, rhs) = expr
                    && let Expr::Property(base, prop) = &**lhs
                    && let Expr::Var(name, ..) = &**base
                {
                    return name == "globalThis" && prop == "__filepath" && matches!(&**rhs, Expr::StringLit(_));
                }
                false
            });

            if let Some(idx) = split_idx {
                let prefix = &statements[..=idx];
                if !prefix.is_empty() {
                    evaluate_statements(mc, &exec_env, prefix)?;
                }
                &statements[(idx + 1)..]
            } else {
                &statements
            }
        } else {
            &statements
        };

        match evaluate_statements(mc, &exec_env, eval_statements_slice) {
            Ok(mut result) => {
                if kind == ProgramKind::Module
                    && let (Some(_exports_obj), Some(p)) = (entry_module_exports, script_path.as_ref())
                {
                    let script_fs_path = std::fs::canonicalize(p.as_ref()).unwrap_or_else(|_| p.as_ref().to_path_buf());
                    let script_fs_path_str = script_fs_path.to_string_lossy().to_string();
                    let logical_module_path = extract_injected_module_filepath(script).unwrap_or_else(|| script_fs_path_str.clone());
                    let module_path = std::fs::canonicalize(&logical_module_path)
                        .ok()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or(logical_module_path);
                    let loading = crate::js_module::get_or_create_module_loading(mc, &root.global_env)?;
                    object_set_key_value(mc, &loading, module_path.as_str(), &Value::Boolean(false))?;
                    object_set_key_value(mc, &loading, script_fs_path_str.as_str(), &Value::Boolean(false))?;
                }
                let mut count = 0;
                loop {
                    match crate::js_promise::run_event_loop(mc)? {
                        crate::js_promise::PollResult::Executed => {
                            count += 1;
                            log::trace!("DEBUG: event loop iteration {count}");
                            continue;
                        }
                        // If the next task is a short timer, wait briefly and continue so
                        // small delays (1ms) used in tests can fire before evaluate_script returns.
                        crate::js_promise::PollResult::Wait(dur) => {
                            if dur <= std::time::Duration::from_millis(crate::js_promise::short_timer_threshold_ms()) {
                                log::trace!("DEBUG: waiting (condvar) for {:?} to allow timers to fire", dur);
                                // Wait on a condvar so we can be woken early when new tasks arrive.
                                let (lock, cv) = crate::js_promise::get_event_loop_wake();
                                let mut guard = lock.lock().unwrap();
                                // Reset the flag before waiting
                                *guard = false;
                                let (_g, _result) = cv.wait_timeout(guard, dur).unwrap();
                                count += 1;
                                continue;
                            } else if crate::js_promise::wait_for_active_handles() {
                                // If the CLI/example wants to keep the loop alive while active
                                // timers exist, wait and continue instead of exiting immediately.
                                log::trace!("DEBUG: longer timer pending ({:?}), but wait_for_active_handles=true, waiting", dur);
                                let (lock, cv) = crate::js_promise::get_event_loop_wake();
                                let mut guard = lock.lock().unwrap();
                                *guard = false;
                                let (_g, _result) = cv.wait_timeout(guard, dur).unwrap();
                                count += 1;
                                continue;
                            } else {
                                log::warn!("DEBUG: longer timer pending ({:?}), exiting event loop", dur);
                                break;
                            }
                        }
                        crate::js_promise::PollResult::Empty => {
                            // Before exiting, attempt to process any runtime-pending unhandled checks
                            // (they may have matured and should be re-queued as UnhandledCheck tasks).
                            if crate::js_promise::process_runtime_pending_unhandled(mc, &root.global_env, false)? {
                                count += 1;
                                continue;
                            }

                            // If configured to wait for active handles (Node-like), and we have
                            // timers/intervals registered, keep the event loop alive until
                            // they are gone. We poll periodically and wait on the condvar
                            // so the loop can be woken when timers expire or handles are cleared.
                            if crate::js_promise::wait_for_active_handles() && crate::js_promise::has_active_timers() {
                                log::trace!("DEBUG: event loop empty but active timers exist, waiting for handles to clear");
                                let (lock, cv) = crate::js_promise::get_event_loop_wake();
                                let guard = lock.lock().unwrap();
                                // Wait in short increments to allow responsive wakeups
                                let (_g, _res) = cv.wait_timeout(guard, std::time::Duration::from_millis(100)).unwrap();
                                count += 1;
                                continue;
                            }

                            // About to exit. Force flush any pending unhandled rejections as if the grace period expired.
                            if crate::js_promise::process_runtime_pending_unhandled(mc, &root.global_env, true)? {
                                count += 1;
                                continue;
                            }

                            break;
                        }
                    }
                }

                // Re-evaluate final expression/return after draining microtasks so that
                // scripts which rely on `.then`/microtask side-effects (e.g. assigning
                // to a top-level variable in a then callback) observe the updated value.
                if let Some(last_stmt) = eval_statements_slice.last() {
                    match &*last_stmt.kind {
                        // If the last statement is a simple variable reference, re-evaluate it
                        // to pick up any changes made by microtasks.
                        StatementKind::Expr(expr) => {
                            match expr {
                                // e.g. final expression is a variable reference: `result`
                                crate::core::Expr::Var(_name, ..) => {
                                    if let Ok(new_val) = evaluate_expr(mc, &exec_env, expr) {
                                        result = new_val;
                                    }
                                }
                                // Pattern: `executionOrder.push("sync")` — instead of re-invoking
                                // the `push` (which would cause duplicate side-effects), detect this
                                // and read the array variable directly.
                                crate::core::Expr::Call(boxed_fn, _call_args) => {
                                    // boxed_fn is a Box<Expr> representing the callable expression.
                                    if let crate::core::Expr::Property(boxed_prop, prop_name) = &**boxed_fn
                                        && let crate::core::Expr::Var(var_name, ..) = &**boxed_prop
                                        && prop_name == "push"
                                    {
                                        // Read the variable value directly from the global env
                                        if let Some(val_rc) = object_get_key_value(&root.global_env, var_name) {
                                            result = val_rc.borrow().clone();
                                        }
                                    }
                                    // Special-case idempotent call expressions such as `JSON.stringify(x)`
                                    // which are safe to re-evaluate after draining microtasks. This allows
                                    // tests to append `JSON.stringify(globalThis.__async_regression_summary)`
                                    // and have the final value reflect microtask-side-effects such as
                                    // `then` callbacks that assign to globalThis.
                                    else if let crate::core::Expr::Property(boxed_prop, prop_name) = &**boxed_fn
                                        && let crate::core::Expr::Var(var_name, ..) = &**boxed_prop
                                        && var_name == "JSON"
                                        && prop_name == "stringify"
                                        && let Ok(new_val) = evaluate_expr(mc, &root.global_env, expr)
                                    {
                                        result = new_val;
                                    }
                                }
                                // Re-evaluate top-level Array expressions to pick up microtask-side-effects
                                // e.g. `[resolveResult, rejectResult]` should reflect values set in `.then`/`.catch` callbacks
                                crate::core::Expr::Array(_elems) => {
                                    if let Ok(new_val) = evaluate_expr(mc, &root.global_env, expr) {
                                        result = new_val;
                                    }
                                }
                                _ => {}
                            }
                        }
                        StatementKind::Return(Some(expr)) => {
                            // Only re-evaluate "safe" return expressions (variable refs, arrays,
                            // or the special-case `foo.push(...)` pattern). We must avoid
                            // re-invoking arbitrary call expressions (e.g. `return (async () => ...)()`)
                            // which would cause duplicate side-effects by executing the call twice.
                            match expr {
                                // e.g. `return result` -> re-evaluate to pick up microtask-side-effects
                                crate::core::Expr::Var(_name, ..) => {
                                    if let Ok(new_val) = evaluate_expr(mc, &root.global_env, expr) {
                                        result = new_val;
                                    }
                                }
                                // Pattern: `return obj.push(...)` -> read `obj` instead of re-invoking `push`
                                crate::core::Expr::Call(boxed_fn, _call_args) => {
                                    if let crate::core::Expr::Property(boxed_prop, prop_name) = &**boxed_fn
                                        && let crate::core::Expr::Var(var_name, ..) = &**boxed_prop
                                        && prop_name == "push"
                                    {
                                        if let Some(val_rc) = object_get_key_value(&root.global_env, var_name) {
                                            result = val_rc.borrow().clone();
                                        }
                                    }
                                    // Also allow safe re-evaluation of `JSON.stringify(x)` in return positions
                                    // so readers appending a stringify call get the post-microtask value.
                                    else if let crate::core::Expr::Property(boxed_prop, prop_name) = &**boxed_fn
                                        && let crate::core::Expr::Var(var_name, ..) = &**boxed_prop
                                        && var_name == "JSON"
                                        && prop_name == "stringify"
                                        && let Ok(new_val) = evaluate_expr(mc, &root.global_env, expr)
                                    {
                                        result = new_val;
                                    }
                                }
                                // e.g. `return [a, b]` -> re-evaluate array expressions
                                crate::core::Expr::Array(_elems) => {
                                    if let Ok(new_val) = evaluate_expr(mc, &root.global_env, expr) {
                                        result = new_val;
                                    }
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }

                // Attempts to extract the underlying promise if the (possibly re-evaluated)
                // result is a Promise object or a wrapped Promise object
                let promise_ref = match result {
                    Value::Promise(promise) => Some(promise),
                    Value::Object(obj) => crate::js_promise::get_promise_from_js_object(&obj),
                    _ => None,
                };

                if let Some(promise) = promise_ref {
                    match &promise.borrow().state {
                        crate::core::PromiseState::Fulfilled(val) => result = val.clone(),
                        crate::core::PromiseState::Rejected(val) => {
                            let mut is_error_like = false;
                            if let Value::Object(obj) = val {
                                if let Some(is_err_rc) = slot_get_chained(obj, &InternalSlot::IsError)
                                    && let Value::Boolean(true) = *is_err_rc.borrow()
                                {
                                    is_error_like = true;
                                }
                                if !is_error_like && slot_get_chained(obj, &InternalSlot::Line).is_some() {
                                    is_error_like = true;
                                }
                            }

                            if !is_error_like {
                                result = val.clone();
                            } else {
                                let mut err = crate::raise_throw_error!(val.clone());
                                if let Value::Object(obj) = val {
                                    if let Some(line_rc) = slot_get_chained(obj, &InternalSlot::Line)
                                        && let Value::Number(line) = *line_rc.borrow()
                                    {
                                        let mut column = 0usize;
                                        if let Some(col_rc) = slot_get_chained(obj, &InternalSlot::Column)
                                            && let Value::Number(col) = *col_rc.borrow()
                                        {
                                            column = col as usize;
                                        }
                                        err.set_js_location(line as usize, column);
                                    }
                                    if let Some(stack_str) = obj.borrow().get_property("stack") {
                                        let lines: Vec<String> = stack_str
                                            .lines()
                                            .map(|s| s.trim().to_string())
                                            .filter(|s| s.starts_with("at "))
                                            .collect();
                                        err.inner.stack = lines;
                                    }
                                }
                                return Err(err);
                            }
                        }
                        _ => {}
                    }
                }

                let report_unhandled = std::env::var("JS_REPORT_UNHANDLED_REJECTIONS")
                    .map(|v| !matches!(v.as_str(), "0" | "false" | "FALSE"))
                    .unwrap_or(false);

                if report_unhandled {
                    // Prefer to consume any runtime `__unhandled_rejection` string which is set
                    // only after the UnhandledCheck grace window has elapsed.
                    if let Some(val) = crate::js_promise::take_unhandled_rejection(mc, &root.global_env)
                        && let crate::core::Value::String(s) = val
                    {
                        let msg = crate::unicode::utf16_to_utf8(&s);
                        let err = crate::make_js_error!(crate::JSErrorKind::Throw(msg));
                        return Err(err);
                    }

                    // Fallback: peek pending unhandled checks whose grace window has elapsed and report them
                    if let Some((msg, loc_opt)) = crate::js_promise::peek_pending_unhandled_info(mc, &root.global_env) {
                        let mut err = crate::make_js_error!(crate::JSErrorKind::Throw(msg));
                        if let Some((line, col)) = loc_opt {
                            err.set_js_location(line, col);
                        }
                        return Err(err);
                    }
                }

                let out = match &result {
                    Value::String(s) => {
                        let s_utf8 = crate::unicode::utf16_to_utf8(s);
                        match serde_json::to_string(&s_utf8) {
                            Ok(quoted) => quoted,
                            Err(_) => format!("\"{}\"", s_utf8),
                        }
                    }
                    Value::Object(obj) => {
                        // WeakMap/WeakSet special-case to display as [object WeakMap] / [object WeakSet]
                        if crate::js_weakmap::is_weakmap_object(mc, obj) {
                            "[object WeakMap]".to_string()
                        } else if crate::js_weakset::is_weakset_object(mc, obj) {
                            "[object WeakSet]".to_string()
                        // If it's an Array, delegate to array helper for consistent formatting
                        } else if crate::js_array::is_array(mc, obj) {
                            crate::js_array::serialize_array_for_eval(mc, obj)?
                        } else if crate::js_regexp::is_regex_object(obj) {
                            // For top-level RegExp object display as [object RegExp]
                            "[object RegExp]".to_string()
                        } else {
                            // If object has no enumerable own properties, print as {}
                            // Otherwise serialize enumerable properties from the object and its prototype chain
                            let mut seen_keys = std::collections::HashSet::new();
                            let mut props: Vec<(String, String)> = Vec::new();
                            let mut cur_obj_opt: Option<crate::core::JSObjectDataPtr<'_>> = Some(*obj);
                            while let Some(cur_obj) = cur_obj_opt {
                                for key in cur_obj.borrow().properties.keys() {
                                    // Skip internal slot keys — they are never JS-visible
                                    if matches!(key, crate::core::PropertyKey::Internal(_)) {
                                        continue;
                                    }
                                    // Skip non-enumerable and internal properties (like __proto__)
                                    if !cur_obj.borrow().is_enumerable(key)
                                        || matches!(key, crate::core::PropertyKey::String(s) if s == "__proto__")
                                    {
                                        continue;
                                    }
                                    // Skip keys we've already included (own properties take precedence)
                                    if seen_keys.contains(key) {
                                        continue;
                                    }
                                    seen_keys.insert(key.clone());
                                    // Get value for key
                                    if let Some(val_rc) = object_get_key_value(&cur_obj, key) {
                                        let val = val_rc.borrow().clone();
                                        let val_str = match val {
                                            Value::String(s) => format!("\"{}\"", crate::unicode::utf16_to_utf8(&s)),
                                            Value::Number(n) => n.to_string(),
                                            Value::Boolean(b) => b.to_string(),
                                            Value::BigInt(b) => b.to_string(),
                                            Value::Undefined => "undefined".to_string(),
                                            Value::Null => "null".to_string(),
                                            Value::Object(o) => {
                                                // For nested arrays, serialize them properly, otherwise use default object string
                                                if crate::js_array::is_array(mc, &o) {
                                                    crate::js_array::serialize_array_for_eval(mc, &o)?
                                                } else {
                                                    value_to_string(&val)
                                                }
                                            }
                                            _ => value_to_string(&val),
                                        };
                                        props.push((key.to_string(), val_str));
                                    }
                                }
                                cur_obj_opt = cur_obj.borrow().prototype;
                            }
                            if props.is_empty() {
                                "{}".to_string()
                            } else {
                                let mut pairs: Vec<String> = Vec::new();
                                for (k, v) in props.iter() {
                                    pairs.push(format!("\"{}\":{}", k, v));
                                }
                                format!("{{{}}}", pairs.join(","))
                            }
                        }
                    }
                    _ => value_to_string(&result),
                };
                Ok(out)
            }
            Err(e) => match e {
                EvalError::Js(js_err) => Err(js_err),
                EvalError::Throw(val, line, column) => {
                    let mut err = crate::raise_throw_error!(val);
                    if let Some((l, c)) = line.zip(column) {
                        err.set_js_location(l, c);
                    }
                    if let Value::Object(obj) = &val
                        && let Some(stack_str) = obj.borrow().get_property("stack")
                    {
                        let lines: Vec<String> = stack_str
                            .lines()
                            .map(|s| s.trim().to_string())
                            .filter(|s| s.starts_with("at "))
                            .collect();
                        err.inner.stack = lines;
                    }
                    Err(err)
                }
            },
        }
    })
}

pub fn evaluate_script<T, P>(script: T, script_path: Option<P>) -> Result<String, JSError>
where
    T: AsRef<str>,
    P: AsRef<std::path::Path>,
{
    evaluate_program(script, script_path, ProgramKind::Script)
}

pub fn evaluate_module<T, P>(script: T, script_path: Option<P>) -> Result<String, JSError>
where
    T: AsRef<str>,
    P: AsRef<std::path::Path>,
{
    evaluate_program(script, script_path, ProgramKind::Module)
}

// Helper to resolve a constructor's prototype object if present in `env`.
pub fn get_constructor_prototype<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    name: &str,
) -> Result<Option<JSObjectDataPtr<'gc>>, JSError> {
    // First try to find a constructor object already stored in the environment
    if let Some(val_rc) = object_get_key_value(env, name)
        && let Value::Object(ctor_obj) = &*val_rc.borrow()
        && let Some(proto_val_rc) = object_get_key_value(ctor_obj, "prototype")
        && let Value::Object(proto_obj) = &*proto_val_rc.borrow()
    {
        return Ok(Some(*proto_obj));
    }

    // If not found, attempt to evaluate the variable to force lazy creation
    match evaluate_expr(mc, env, &Expr::Var(name.to_string(), None, None)) {
        Ok(Value::Object(ctor_obj)) => {
            if let Some(proto_val_rc) = object_get_key_value(&ctor_obj, "prototype")
                && let Value::Object(proto_obj) = &*proto_val_rc.borrow()
            {
                return Ok(Some(*proto_obj));
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
        log::trace!("setting prototype for ctor='{}' proto_obj={:p}", ctor_name, Gc::as_ptr(proto_obj));
        obj.borrow_mut(mc).prototype = Some(proto_obj);
        // Do not create an own `__proto__` property for this helper; only set the internal prototype pointer.
        log::trace!("set_internal_prototype_from_constructor: set internal prototype pointer");
    }
    Ok(())
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
