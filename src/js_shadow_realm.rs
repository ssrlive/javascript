use crate::core::{
    InternalSlot, JSObjectDataPtr, MutationContext, StatementKind, Value, check_strict_mode_violations, env_set_strictness,
    evaluate_call_dispatch, evaluate_statements, get_property_with_accessors, initialize_global_constructors, new_gc_cell_ptr,
    new_js_object_data, object_get_key_value, object_set_key_value, parse_statements, slot_get, slot_remove, slot_set, tokenize,
};
use crate::error::JSError;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};

/// Internal slot key for the ShadowRealm's isolated global environment.
/// Stored on each ShadowRealm instance object.
pub(crate) const SHADOW_REALM_SLOT: InternalSlot = InternalSlot::ShadowRealm;

/// Throw a TypeError from a specific realm (so `instanceof TypeError` in the
/// caller realm holds true, even when the error originates from another realm).
fn throw_caller_realm_type_error<'gc>(
    mc: &MutationContext<'gc>,
    caller_env: &JSObjectDataPtr<'gc>,
    msg: &str,
) -> crate::core::EvalError<'gc> {
    use crate::unicode::utf8_to_utf16;
    let msg_val = Value::String(utf8_to_utf16(msg));
    if let Some(te_val) = object_get_key_value(caller_env, "TypeError")
        && let Value::Object(te_ctor) = &*te_val.borrow()
        && let Ok(err) = crate::js_class::evaluate_new(mc, caller_env, &Value::Object(*te_ctor), &[msg_val], None)
    {
        return crate::core::EvalError::Throw(err, None, None);
    }
    // Fallback if we can't construct a realm-specific TypeError
    crate::core::EvalError::Js(crate::raise_type_error!(msg))
}

/// Throw a SyntaxError from a specific realm (for parsing failures in evaluate).
fn throw_caller_realm_syntax_error<'gc>(
    mc: &MutationContext<'gc>,
    caller_env: &JSObjectDataPtr<'gc>,
    msg: &str,
) -> crate::core::EvalError<'gc> {
    use crate::unicode::utf8_to_utf16;
    let msg_val = Value::String(utf8_to_utf16(msg));
    if let Some(se_val) = object_get_key_value(caller_env, "SyntaxError")
        && let Value::Object(se_ctor) = &*se_val.borrow()
        && let Ok(err) = crate::js_class::evaluate_new(mc, caller_env, &Value::Object(*se_ctor), &[msg_val], None)
    {
        return crate::core::EvalError::Throw(err, None, None);
    }
    // Fallback
    crate::core::EvalError::Js(crate::raise_syntax_error!(msg))
}

// ---------------------------------------------------------------------------
//  Initialization: register global `ShadowRealm` constructor
// ---------------------------------------------------------------------------

pub fn initialize_shadow_realm<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let ctor = new_js_object_data(mc);
    slot_set(mc, &ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("ShadowRealm")));

    // ShadowRealm.prototype
    let proto = new_js_object_data(mc);

    // Link constructor ↔ prototype
    object_set_key_value(mc, &ctor, "prototype", &Value::Object(proto))?;
    object_set_key_value(mc, &proto, "constructor", &Value::Object(ctor))?;

    // ShadowRealm.prototype.evaluate — proper function object
    {
        let eval_obj = new_js_object_data(mc);
        eval_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(
            mc,
            Value::Function("ShadowRealm.prototype.evaluate".to_string()),
        )));
        // Store the creating realm so get_function_realm returns the correct
        // realm for cross-realm scenarios (tests 2-4).
        slot_set(mc, &eval_obj, InternalSlot::OriginGlobal, &Value::Object(*env));
        // [[Prototype]] = Function.prototype
        if let Some(func_ctor_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*proto_val.borrow()
        {
            eval_obj.borrow_mut(mc).prototype = Some(*func_proto);
        }
        let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("evaluate")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &eval_obj, "name", &name_desc)?;
        let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(1.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &eval_obj, "length", &len_desc)?;
        // writable, not enumerable, configurable
        let prop_desc = crate::core::create_descriptor_object(mc, &Value::Object(eval_obj), true, false, true)?;
        crate::js_object::define_property_internal(mc, &proto, "evaluate", &prop_desc)?;
    }

    // ShadowRealm.prototype.importValue — proper function object
    {
        let import_obj = new_js_object_data(mc);
        import_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(
            mc,
            Value::Function("ShadowRealm.prototype.importValue".to_string()),
        )));
        // Store the creating realm for cross-realm dispatch.
        slot_set(mc, &import_obj, InternalSlot::OriginGlobal, &Value::Object(*env));
        // [[Prototype]] = Function.prototype
        if let Some(func_ctor_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*proto_val.borrow()
        {
            import_obj.borrow_mut(mc).prototype = Some(*func_proto);
        }
        let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("importValue")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &import_obj, "name", &name_desc)?;
        let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(2.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &import_obj, "length", &len_desc)?;
        // writable, not enumerable, configurable
        let prop_desc = crate::core::create_descriptor_object(mc, &Value::Object(import_obj), true, false, true)?;
        crate::js_object::define_property_internal(mc, &proto, "importValue", &prop_desc)?;
    }

    // ShadowRealm.prototype[@@toStringTag] = "ShadowRealm"
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("ShadowRealm")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &proto, *tag_sym, &desc)?;
    }

    // Mark prototype non-enumerable on constructor
    ctor.borrow_mut(mc).set_non_enumerable("prototype");

    // ShadowRealm.length = 0
    let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(0.0), false, false, true)?;
    crate::js_object::define_property_internal(mc, &ctor, "length", &len_desc)?;

    // ShadowRealm.name = "ShadowRealm"
    let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("ShadowRealm")), false, false, true)?;
    crate::js_object::define_property_internal(mc, &ctor, "name", &name_desc)?;

    // Set ShadowRealm.__proto__ = Function.prototype
    if let Some(func_ctor_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(func_proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_val.borrow()
    {
        ctor.borrow_mut(mc).prototype = Some(*func_proto);
        // Also set the ShadowRealm.prototype's [[Prototype]] to Object.prototype
        if let Some(obj_ctor_val) = object_get_key_value(env, "Object")
            && let Value::Object(obj_ctor) = &*obj_ctor_val.borrow()
            && let Some(obj_proto_val) = object_get_key_value(obj_ctor, "prototype")
            && let Value::Object(obj_proto) = &*obj_proto_val.borrow()
        {
            proto.borrow_mut(mc).prototype = Some(*obj_proto);
        }
    }

    // Store the creating realm on the constructor so get_function_realm
    // returns the correct realm for Reflect.construct(OtherShadowRealm, []).
    slot_set(mc, &ctor, InternalSlot::OriginGlobal, &Value::Object(*env));

    // Register on global
    object_set_key_value(mc, env, "ShadowRealm", &Value::Object(ctor))?;
    // Make non-enumerable
    env.borrow_mut(mc).set_non_enumerable("ShadowRealm");

    Ok(())
}

// ---------------------------------------------------------------------------
//  Constructor: new ShadowRealm()
// ---------------------------------------------------------------------------

pub fn handle_shadow_realm_constructor<'gc>(
    mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    caller_env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, crate::core::EvalError<'gc>> {
    // Create a fresh global environment with all built-ins
    let realm_env = new_js_object_data(mc);

    // Initialize all global constructors in the new realm
    initialize_global_constructors(mc, &realm_env)?;

    // Per spec: ShadowRealm evaluates in non-strict mode by default.
    // initialize_global_constructors sets strict mode, so we reset it.
    env_set_strictness(mc, &realm_env, false)?;

    // Per spec: the GlobalSymbolRegistry is shared across all realms (per-agent).
    // Copy the caller's symbol registry into the shadow realm so `Symbol.for`
    // returns the same symbols.
    if let Some(registry_cell) = slot_get(caller_env, &InternalSlot::SymbolRegistry) {
        slot_set(mc, &realm_env, InternalSlot::SymbolRegistry, &registry_cell.borrow());
    }

    // Per spec: globalThis is an ordinary object whose [[Prototype]] is Object.prototype.
    if let Some(obj_ctor_val) = object_get_key_value(&realm_env, "Object")
        && let Value::Object(obj_ctor) = &*obj_ctor_val.borrow()
        && let Some(obj_proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(obj_proto) = &*obj_proto_val.borrow()
    {
        realm_env.borrow_mut(mc).prototype = Some(*obj_proto);
    }

    // Set globalThis to point to the realm environment itself
    object_set_key_value(mc, &realm_env, "globalThis", &Value::Object(realm_env))?;

    // Set up global constants
    object_set_key_value(mc, &realm_env, "undefined", &Value::Undefined)?;
    object_set_key_value(mc, &realm_env, "NaN", &Value::Number(f64::NAN))?;
    object_set_key_value(mc, &realm_env, "Infinity", &Value::Number(f64::INFINITY))?;

    // Set up global functions
    object_set_key_value(mc, &realm_env, "eval", &Value::Function("eval".to_string()))?;
    object_set_key_value(mc, &realm_env, "isFinite", &Value::Function("isFinite".to_string()))?;
    object_set_key_value(mc, &realm_env, "isNaN", &Value::Function("isNaN".to_string()))?;
    object_set_key_value(mc, &realm_env, "parseFloat", &Value::Function("parseFloat".to_string()))?;
    object_set_key_value(mc, &realm_env, "parseInt", &Value::Function("parseInt".to_string()))?;
    object_set_key_value(mc, &realm_env, "decodeURI", &Value::Function("decodeURI".to_string()))?;
    object_set_key_value(
        mc,
        &realm_env,
        "decodeURIComponent",
        &Value::Function("decodeURIComponent".to_string()),
    )?;
    object_set_key_value(mc, &realm_env, "encodeURI", &Value::Function("encodeURI".to_string()))?;
    object_set_key_value(
        mc,
        &realm_env,
        "encodeURIComponent",
        &Value::Function("encodeURIComponent".to_string()),
    )?;

    // Create the ShadowRealm instance object
    let instance = new_js_object_data(mc);

    // Store realm env as internal slot
    slot_set(mc, &instance, SHADOW_REALM_SLOT, &Value::Object(realm_env));

    // Set the prototype from the caller's ShadowRealm.prototype
    if let Some(sr_ctor_val) = object_get_key_value(caller_env, "ShadowRealm")
        && let Value::Object(sr_ctor) = &*sr_ctor_val.borrow()
        && let Some(sr_proto_val) = object_get_key_value(sr_ctor, "prototype")
        && let Value::Object(sr_proto) = &*sr_proto_val.borrow()
    {
        instance.borrow_mut(mc).prototype = Some(*sr_proto);
    }

    Ok(Value::Object(instance))
}

// ---------------------------------------------------------------------------
//  ShadowRealm.prototype.evaluate(sourceText)
// ---------------------------------------------------------------------------

pub fn handle_shadow_realm_evaluate<'gc>(
    mc: &MutationContext<'gc>,
    this_val: &Value<'gc>,
    args: &[Value<'gc>],
    caller_env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, crate::core::EvalError<'gc>> {
    // Step 1: Validate this — errors come from the *caller* realm (the
    // function's [[Realm]], resolved by get_function_realm → OriginGlobal).
    let instance = match this_val {
        Value::Object(obj) => *obj,
        _ => {
            return Err(throw_caller_realm_type_error(
                mc,
                caller_env,
                "ShadowRealm.prototype.evaluate called on non-object",
            ));
        }
    };

    // Step 2: Get [[ShadowRealm]] internal slot
    let realm_env = match slot_get(&instance, &SHADOW_REALM_SLOT) {
        Some(v) => match &*v.borrow() {
            Value::Object(o) => *o,
            _ => {
                return Err(throw_caller_realm_type_error(
                    mc,
                    caller_env,
                    "ShadowRealm.prototype.evaluate: invalid realm",
                ));
            }
        },
        None => {
            return Err(throw_caller_realm_type_error(
                mc,
                caller_env,
                "ShadowRealm.prototype.evaluate requires a ShadowRealm object",
            ));
        }
    };

    // Step 3: sourceText must be a string
    let source_text = match args.first() {
        Some(Value::String(s)) => utf16_to_utf8(s),
        Some(_) => {
            return Err(throw_caller_realm_type_error(
                mc,
                caller_env,
                "ShadowRealm.prototype.evaluate requires a string argument",
            ));
        }
        None => {
            return Err(throw_caller_realm_type_error(
                mc,
                caller_env,
                "ShadowRealm.prototype.evaluate requires a string argument",
            ));
        }
    };

    // Step 4: PerformRealmEval(sourceText, callerRealm, evalRealm)
    // Per spec §3.1.2.1 PerformRealmEval:
    //   Phase 1 – Parse: if parsing fails → SyntaxError from the *caller* realm
    //   Phase 2 – Evaluate + wrap: any exception → TypeError from the *caller* realm

    // Phase 1: Parse
    let statements = {
        let tokens_result = tokenize(&source_text);
        let mut tokens = match tokens_result {
            Ok(t) => t,
            Err(_e) => {
                return Err(throw_caller_realm_syntax_error(
                    mc,
                    caller_env,
                    "Invalid syntax in ShadowRealm evaluate",
                ));
            }
        };
        if tokens.last().map(|td| td.token == crate::core::Token::EOF).unwrap_or(false) {
            tokens.pop();
        }
        let mut index = 0;
        match parse_statements(&tokens, &mut index) {
            Ok(s) => s,
            Err(_e) => {
                return Err(throw_caller_realm_syntax_error(
                    mc,
                    caller_env,
                    "Invalid syntax in ShadowRealm evaluate",
                ));
            }
        }
    };

    // Phase 1b: If the code has a "use strict" directive prologue, check for
    // strict-mode-only SyntaxErrors (e.g. assignment to `arguments`).
    let is_strict = statements.first()
        .map(|s| matches!(&*s.kind, StatementKind::Expr(crate::core::Expr::StringLit(ss)) if crate::unicode::utf16_to_utf8(ss).as_str() == "use strict"))
        .unwrap_or(false);
    if is_strict && let Err(_e) = check_strict_mode_violations(&statements) {
        return Err(throw_caller_realm_syntax_error(
            mc,
            caller_env,
            "Strict mode only SyntaxError in ShadowRealm evaluate",
        ));
    }

    // Phase 2: Evaluate + wrap result
    // Per spec §3.1.2.1 PerformShadowRealmEval:
    //   - LexicalEnvironment = NewDeclarativeEnvironment(evalRealm.[[GlobalEnv]])
    //   - VariableEnvironment = evalRealm.[[GlobalEnv]]
    // This means const/let go to a fresh declarative env, while var/function
    // declarations go to the global env. We reuse the IsIndirectEval machinery
    // in evaluate_statements which already handles this distinction.
    let eval_result: Result<Value<'gc>, crate::core::EvalError<'gc>> = (|| {
        slot_set(mc, &realm_env, InternalSlot::IsIndirectEval, &Value::Boolean(true));
        let result = evaluate_statements(mc, &realm_env, &statements);
        // Clean up marker in case evaluate_statements didn't remove it (e.g. error path)
        let _ = slot_remove(mc, &realm_env, &InternalSlot::IsIndirectEval);
        let result = result?;
        // GetWrappedValue(callerRealm, result)
        get_wrapped_value(mc, caller_env, &realm_env, &result)
    })();

    match eval_result {
        Ok(val) => Ok(val),
        Err(_e) => {
            // Per spec: any runtime exception originating from the other realm is
            // wrapped into a TypeError thrown from the caller realm.
            Err(throw_caller_realm_type_error(
                mc,
                caller_env,
                "ShadowRealm evaluate threw an exception",
            ))
        }
    }
}

// ---------------------------------------------------------------------------
//  ShadowRealm.prototype.importValue(specifier, exportName)
// ---------------------------------------------------------------------------

pub fn handle_shadow_realm_import_value<'gc>(
    mc: &MutationContext<'gc>,
    this_val: &Value<'gc>,
    args: &[Value<'gc>],
    caller_env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, crate::core::EvalError<'gc>> {
    // Step 1: Validate this
    let instance = match this_val {
        Value::Object(obj) => *obj,
        _ => return Err(raise_type_error!("ShadowRealm.prototype.importValue called on non-object").into()),
    };

    // Step 2: Get [[ShadowRealm]] internal slot
    let realm_env = match slot_get(&instance, &SHADOW_REALM_SLOT) {
        Some(v) => match &*v.borrow() {
            Value::Object(o) => *o,
            _ => return Err(raise_type_error!("importValue: invalid realm").into()),
        },
        None => return Err(raise_type_error!("ShadowRealm.prototype.importValue requires a ShadowRealm object").into()),
    };

    // Step 3: specifier = ToString(specifier)
    let specifier = match args.first() {
        Some(v) => crate::js_string::spec_to_string(mc, v, caller_env)?,
        None => return Err(raise_type_error!("importValue requires a specifier").into()),
    };
    let specifier_str = utf16_to_utf8(&specifier);

    // Step 4: exportName must be a string (not coerced — spec says "If Type(exportNameString) is not String, throw a TypeError")
    let export_name_str = match args.get(1) {
        Some(Value::String(s)) => utf16_to_utf8(s),
        Some(_) => return Err(raise_type_error!("importValue requires exportName to be a string").into()),
        None => return Err(raise_type_error!("importValue requires an exportName").into()),
    };

    // Steps 5-6: Import the module in the realm and extract the export.
    // Per spec, module-related errors should result in a rejected promise
    // with a TypeError (from the caller realm), NOT a synchronous throw.
    let import_result: Result<Value<'gc>, crate::core::EvalError<'gc>> = (|| {
        let module_val = crate::js_module::load_module(mc, &specifier_str, None, Some(realm_env))?;

        let export_val = match module_val {
            Value::Object(exports_obj) => get_property_with_accessors(mc, &realm_env, &exports_obj, export_name_str.as_str())?,
            _ => Value::Undefined,
        };

        if matches!(export_val, Value::Undefined) {
            return Err(raise_type_error!(format!("importValue: export '{}' not found", export_name_str)).into());
        }

        // Wrap the exported value
        get_wrapped_value(mc, caller_env, &realm_env, &export_val)
    })();

    match import_result {
        Ok(wrapped) => {
            // Return a resolved promise
            crate::js_promise::handle_promise_static_method_val(mc, "resolve", &[wrapped], None, caller_env)
        }
        Err(_e) => {
            // Per spec: any exception from module loading/evaluation is wrapped
            // into a TypeError and returned as a rejected promise.
            let type_err = throw_caller_realm_type_error(mc, caller_env, "importValue failed");
            match type_err {
                crate::core::EvalError::Throw(err_val, _, _) => {
                    crate::js_promise::handle_promise_static_method_val(mc, "reject", &[err_val], None, caller_env)
                }
                crate::core::EvalError::Js(js_err) => {
                    let msg = Value::String(utf8_to_utf16(&format!("{}", js_err)));
                    crate::js_promise::handle_promise_static_method_val(mc, "reject", &[msg], None, caller_env)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
//  GetWrappedValue(callerRealm, value) — §3.1.3
// ---------------------------------------------------------------------------

fn get_wrapped_value<'gc>(
    mc: &MutationContext<'gc>,
    caller_env: &JSObjectDataPtr<'gc>,
    realm_env: &JSObjectDataPtr<'gc>,
    value: &Value<'gc>,
) -> Result<Value<'gc>, crate::core::EvalError<'gc>> {
    match value {
        // Primitives pass through directly
        Value::Undefined | Value::Null | Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::BigInt(_) | Value::Symbol(_) => {
            Ok(value.clone())
        }

        // Callable objects → create a WrappedFunction
        Value::Closure(_)
        | Value::AsyncClosure(_)
        | Value::Function(_)
        | Value::GeneratorFunction(..)
        | Value::AsyncGeneratorFunction(..) => create_wrapped_function(mc, caller_env, realm_env, value),

        Value::Object(obj) => {
            // Check if the object is callable (has a closure or is a function-object)
            let is_callable = obj.borrow().get_closure().is_some()
                || slot_get(obj, &InternalSlot::NativeCtor).is_some()
                || slot_get(obj, &InternalSlot::BoundTarget).is_some();

            // Check if it's a (callable) Proxy
            let is_callable_proxy = if let Some(proxy_cell) = slot_get(obj, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                crate::js_proxy::is_callable_proxy(proxy)
            } else {
                false
            };

            if is_callable || is_callable_proxy {
                create_wrapped_function(mc, caller_env, realm_env, value)
            } else {
                // Non-callable objects throw TypeError
                Err(raise_type_error!("ShadowRealm evaluate: non-callable object cannot cross realm boundary").into())
            }
        }

        Value::Proxy(proxy) => {
            if crate::js_proxy::is_callable_proxy(proxy) {
                create_wrapped_function(mc, caller_env, realm_env, value)
            } else {
                Err(raise_type_error!("ShadowRealm evaluate: non-callable object cannot cross realm boundary").into())
            }
        }

        // Everything else (Getter, Setter, Property, etc.) → error
        _ => Err(raise_type_error!("ShadowRealm evaluate: value cannot cross realm boundary").into()),
    }
}

// ---------------------------------------------------------------------------
//  WrappedFunctionCreate(callerRealm, targetFunction)
// ---------------------------------------------------------------------------

fn create_wrapped_function<'gc>(
    mc: &MutationContext<'gc>,
    caller_env: &JSObjectDataPtr<'gc>,
    realm_env: &JSObjectDataPtr<'gc>,
    target: &Value<'gc>,
) -> Result<Value<'gc>, crate::core::EvalError<'gc>> {
    // Check for revoked proxy — GetFunctionRealm throws on revoked proxy
    if let Value::Object(obj) = target
        && let Some(proxy_cell) = slot_get(obj, &InternalSlot::Proxy)
        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
        && proxy.revoked
    {
        return Err(raise_type_error!("Cannot wrap a revoked proxy").into());
    }
    if let Value::Proxy(proxy) = target
        && proxy.revoked
    {
        return Err(raise_type_error!("Cannot wrap a revoked proxy").into());
    }

    let wrapped = new_js_object_data(mc);

    // Store the target function and realm references
    slot_set(mc, &wrapped, InternalSlot::WrappedTarget, target);
    slot_set(mc, &wrapped, InternalSlot::WrappedCallerRealm, &Value::Object(*caller_env));
    slot_set(mc, &wrapped, InternalSlot::WrappedTargetRealm, &Value::Object(*realm_env));

    // Set [[Prototype]] to callerRealm's Function.prototype
    if let Some(func_ctor_val) = object_get_key_value(caller_env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(func_proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_val.borrow()
    {
        wrapped.borrow_mut(mc).prototype = Some(*func_proto);
    }

    // CopyNameAndLength: copy "name" and "length" from target
    copy_name_and_length(mc, &wrapped, target, caller_env, realm_env)?;

    // Store the wrapped function as a closure that dispatches to the target
    let trampoline = Value::Function("__shadow_realm_wrapped_fn__".to_string());
    wrapped.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, trampoline)));

    Ok(Value::Object(wrapped))
}

/// CopyNameAndLength(F, Target, prefix, argCount)
/// Per spec, HasOwnProperty and Get on the target must go through the
/// target's internal methods (e.g. proxy traps).  We use
/// `get_property_with_accessors` for Get (which honours proxies) and
/// `crate::js_object::has_own_property_with_proxy` for the has-check
/// (which honours the getOwnPropertyDescriptor proxy trap).
fn copy_name_and_length<'gc>(
    mc: &MutationContext<'gc>,
    wrapped: &JSObjectDataPtr<'gc>,
    target: &Value<'gc>,
    _caller_env: &JSObjectDataPtr<'gc>,
    realm_env: &JSObjectDataPtr<'gc>,
) -> Result<(), crate::core::EvalError<'gc>> {
    // Get target's length — must go through proxy traps if target is a Proxy
    let mut length: f64 = 0.0;
    if let Value::Object(target_obj) = target {
        // Per spec CopyNameAndLength step 3: Let targetHasLength be ? HasOwnProperty(Target, "length").
        // HasOwnProperty calls [[GetOwnProperty]] which for Proxy triggers getOwnPropertyDescriptor trap.
        let has_length = if let Some(proxy_cell) = slot_get(target_obj, &InternalSlot::Proxy)
            && let Value::Proxy(proxy) = &*proxy_cell.borrow()
        {
            // For proxy: call getOwnPropertyDescriptor trap
            crate::js_proxy::proxy_get_own_property_descriptor(mc, proxy, &crate::core::PropertyKey::String("length".to_string()))?
                .is_some()
        } else {
            object_get_key_value(target_obj, "length").is_some()
        };
        if has_length {
            let target_len_val = get_property_with_accessors(mc, realm_env, target_obj, "length")?;
            if let Value::Number(n) = target_len_val {
                if n.is_infinite() && n.is_sign_positive() {
                    length = f64::INFINITY;
                } else if n.is_infinite() && n.is_sign_negative() {
                    length = 0.0;
                } else if n.is_finite() {
                    let int_val = n.floor();
                    length = if int_val < 0.0 { 0.0 } else { int_val };
                }
            }
        }
    } else if let Value::Closure(cl) = target {
        length = cl.params.len() as f64;
    } else if let Value::Function(_name) = target {
        // Built-in functions: length not easily accessible, default 0
    }

    // Set length as non-writable, non-enumerable, configurable
    let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(length), false, false, true)?;
    crate::js_object::define_property_internal(mc, wrapped, "length", &len_desc)?;

    // Get target's name — per spec step 6: Let targetName be ? Get(Target, "name").
    // "name" does NOT have a HasOwnProperty check in the spec, but Get goes through proxy get trap.
    let mut name = String::new();
    if let Value::Object(target_obj) = target {
        // For proxy targets, getOwnPropertyDescriptor may also fire for "name" access.
        // But per spec, CopyNameAndLength step 6 is just ? Get(Target, "name"), no HasOwnProperty.
        let target_name_val = get_property_with_accessors(mc, realm_env, target_obj, "name")?;
        if let Value::String(s) = target_name_val {
            name = utf16_to_utf8(&s);
        }
    } else if let Value::Closure(_cl) = target {
        // Closures don't carry a name field; leave name as ""
    } else if let Value::Function(n) = target {
        name = n.clone();
    }

    // Set name as non-writable, non-enumerable, configurable
    let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16(&name)), false, false, true)?;
    crate::js_object::define_property_internal(mc, wrapped, "name", &name_desc)?;

    Ok(())
}

// ---------------------------------------------------------------------------
//  WrappedFunction [[Call]](thisArgument, argumentsList)
// ---------------------------------------------------------------------------

pub fn handle_wrapped_function_call<'gc>(
    mc: &MutationContext<'gc>,
    wrapped_obj: &JSObjectDataPtr<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, crate::core::EvalError<'gc>> {
    // Read internal slots
    let target = match slot_get(wrapped_obj, &InternalSlot::WrappedTarget) {
        Some(v) => v.borrow().clone(),
        None => return Err(raise_type_error!("Not a wrapped function").into()),
    };
    let caller_env = match slot_get(wrapped_obj, &InternalSlot::WrappedCallerRealm) {
        Some(v) => match &*v.borrow() {
            Value::Object(o) => *o,
            _ => return Err(raise_type_error!("Invalid caller realm").into()),
        },
        None => return Err(raise_type_error!("Invalid caller realm").into()),
    };
    let target_realm = match slot_get(wrapped_obj, &InternalSlot::WrappedTargetRealm) {
        Some(v) => match &*v.borrow() {
            Value::Object(o) => *o,
            _ => return Err(raise_type_error!("Invalid target realm").into()),
        },
        None => return Err(raise_type_error!("Invalid target realm").into()),
    };

    // Check for revoked proxy target
    if let Value::Object(target_obj) = &target
        && let Some(proxy_cell) = slot_get(target_obj, &InternalSlot::Proxy)
        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
        && proxy.revoked
    {
        return Err(raise_type_error!("Cannot call a wrapped function whose target is a revoked proxy").into());
    }
    if let Value::Proxy(proxy) = &target
        && proxy.revoked
    {
        return Err(raise_type_error!("Cannot call a wrapped function whose target is a revoked proxy").into());
    }

    // Wrap each argument: primitives pass through, callables get wrapped INTO the target realm,
    // non-callable objects throw TypeError from the *caller* realm
    // Then call the target and wrap result — all wrapped in error conversion.
    let call_result: Result<Value<'gc>, crate::core::EvalError<'gc>> = (|| {
        let mut wrapped_args = Vec::with_capacity(args.len());
        for arg in args {
            let wrapped_arg = wrap_argument_into_realm(mc, &target_realm, &caller_env, arg)?;
            wrapped_args.push(wrapped_arg);
        }

        // Call the target function in the target realm
        let result = evaluate_call_dispatch(mc, &target_realm, &target, None, &wrapped_args)?;

        // Wrap the result back to the caller realm
        get_wrapped_value(mc, &caller_env, &target_realm, &result)
    })();

    match call_result {
        Ok(val) => Ok(val),
        Err(_e) => {
            // Per spec: any exception from a wrapped function call is converted
            // to a TypeError from the caller realm.
            Err(throw_caller_realm_type_error(
                mc,
                &caller_env,
                "WrappedFunction call threw an exception",
            ))
        }
    }
}

/// Wrap an argument value when crossing from caller realm to target realm.
/// Primitives pass through. Callable objects get wrapped. Non-callable objects throw TypeError.
fn wrap_argument_into_realm<'gc>(
    mc: &MutationContext<'gc>,
    target_realm: &JSObjectDataPtr<'gc>,
    caller_realm: &JSObjectDataPtr<'gc>,
    value: &Value<'gc>,
) -> Result<Value<'gc>, crate::core::EvalError<'gc>> {
    match value {
        Value::Undefined | Value::Null | Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::BigInt(_) | Value::Symbol(_) => {
            Ok(value.clone())
        }

        Value::Closure(_)
        | Value::AsyncClosure(_)
        | Value::Function(_)
        | Value::GeneratorFunction(..)
        | Value::AsyncGeneratorFunction(..) => {
            // Wrap callable into the target realm (reverse direction)
            create_wrapped_function(mc, target_realm, caller_realm, value)
        }

        Value::Object(obj) => {
            let is_callable = obj.borrow().get_closure().is_some()
                || slot_get(obj, &InternalSlot::NativeCtor).is_some()
                || slot_get(obj, &InternalSlot::BoundTarget).is_some();

            let is_callable_proxy = if let Some(proxy_cell) = slot_get(obj, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                crate::js_proxy::is_callable_proxy(proxy)
            } else {
                false
            };

            // Also check if it's a wrapped function (has WrappedTarget slot)
            let is_wrapped = slot_get(obj, &InternalSlot::WrappedTarget).is_some();

            if is_callable || is_callable_proxy || is_wrapped {
                create_wrapped_function(mc, target_realm, caller_realm, value)
            } else {
                Err(raise_type_error!("Wrapped function arguments must be primitive or callable").into())
            }
        }

        Value::Proxy(proxy) => {
            if crate::js_proxy::is_callable_proxy(proxy) {
                create_wrapped_function(mc, target_realm, caller_realm, value)
            } else {
                Err(raise_type_error!("Wrapped function arguments must be primitive or callable").into())
            }
        }

        _ => Err(raise_type_error!("Wrapped function arguments must be primitive or callable").into()),
    }
}
