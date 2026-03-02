use crate::core::{
    EvalError, InternalSlot, JSObjectDataPtr, MutationContext, Value, env_set, new_gc_cell_ptr, new_js_object_data, object_get_key_value,
    object_set_key_value, slot_get, slot_set,
};
use crate::error::JSError;
use crate::js_array::{create_array, set_array_length};
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use regress::Regex;
use std::cell::RefCell;
use std::collections::HashMap;

/// SameValue(x, y) — ES2024 §6.1.6.1.14
/// Like strict equality but NaN === NaN is true and +0 !== -0.
fn same_value<'a>(x: &Value<'a>, y: &Value<'a>) -> bool {
    match (x, y) {
        (Value::Number(a), Value::Number(b)) => {
            if a.is_nan() && b.is_nan() {
                return true;
            }
            // Distinguish +0 from -0
            if *a == 0.0 && *b == 0.0 {
                return a.is_sign_positive() == b.is_sign_positive();
            }
            a == b
        }
        _ => crate::core::same_value_zero(x, y),
    }
}

// ---------------------------------------------------------------------------
// Thread-local cache for compiled regress::Regex objects.
// Key: (pattern_utf16, regress_flags).  Avoids recompiling the same regex
// every time exec/test/split/replace is called.
// ---------------------------------------------------------------------------
thread_local! {
    static REGEX_CACHE: RefCell<HashMap<(Vec<u16>, String), Regex>> =
        RefCell::new(HashMap::new());
}

// ---------------------------------------------------------------------------
// AnnexB B.2.4: Legacy RegExp static properties (RegExp.$1-$9, etc.)
// Thread-local state updated after each successful RegExp built-in exec.
// ---------------------------------------------------------------------------
#[derive(Clone, Default)]
struct LegacyRegExpState {
    input: Vec<u16>,         // [[RegExpInput]]
    last_match: Vec<u16>,    // [[RegExpLastMatch]]
    left_context: Vec<u16>,  // [[RegExpLeftContext]]
    right_context: Vec<u16>, // [[RegExpRightContext]]
    last_paren: Vec<u16>,    // [[RegExpLastParen]]
    parens: [Vec<u16>; 9],   // [[RegExpParen1]]-[[RegExpParen9]]
}

thread_local! {
    static LEGACY_REGEXP_STATE: RefCell<LegacyRegExpState> =
        RefCell::new(LegacyRegExpState::default());
    /// Manually set input via `RegExp.input = val` (the [[RegExpInput]] slot
    /// that can be written by user code).
    static LEGACY_REGEXP_INPUT_OVERRIDE: RefCell<Option<Vec<u16>>> =
        const { RefCell::new(None) };
}

/// Update legacy regexp state after a successful match.
fn update_legacy_regexp_state(input: &[u16], match_start: usize, match_end: usize, captures: &[Option<std::ops::Range<usize>>]) {
    LEGACY_REGEXP_STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.input = input.to_vec();
        s.last_match = input[match_start..match_end].to_vec();
        s.left_context = input[..match_start].to_vec();
        s.right_context = input[match_end..].to_vec();
        // Parens: captures[0] is group 1, captures[1] is group 2, etc.
        for i in 0..9 {
            if i < captures.len() {
                if let Some(ref range) = captures[i] {
                    s.parens[i] = input[range.start..range.end].to_vec();
                } else {
                    s.parens[i] = Vec::new();
                }
            } else {
                s.parens[i] = Vec::new();
            }
        }
        // lastParen = last actually-matched capturing group
        s.last_paren = Vec::new();
        for i in (0..captures.len().min(9)).rev() {
            if captures[i].is_some() {
                s.last_paren = s.parens[i].clone();
                break;
            }
        }
    });
    // Clear input override on successful match
    LEGACY_REGEXP_INPUT_OVERRIDE.with(|o| *o.borrow_mut() = None);
}

/// Read legacy state for getter dispatch.
pub(crate) fn get_legacy_regexp_property(property: &str) -> Vec<u16> {
    LEGACY_REGEXP_STATE.with(|state| {
        let s = state.borrow();
        match property {
            "input" | "$_" => LEGACY_REGEXP_INPUT_OVERRIDE.with(|o| o.borrow().clone().unwrap_or_else(|| s.input.clone())),
            "lastMatch" | "$&" => s.last_match.clone(),
            "leftContext" | "$`" => s.left_context.clone(),
            "rightContext" | "$'" => s.right_context.clone(),
            "lastParen" | "$+" => s.last_paren.clone(),
            "$1" => s.parens[0].clone(),
            "$2" => s.parens[1].clone(),
            "$3" => s.parens[2].clone(),
            "$4" => s.parens[3].clone(),
            "$5" => s.parens[4].clone(),
            "$6" => s.parens[5].clone(),
            "$7" => s.parens[6].clone(),
            "$8" => s.parens[7].clone(),
            "$9" => s.parens[8].clone(),
            _ => Vec::new(),
        }
    })
}

/// Set input override (for `RegExp.input = val`).
pub(crate) fn set_legacy_regexp_input(val: Vec<u16>) {
    LEGACY_REGEXP_INPUT_OVERRIDE.with(|o| *o.borrow_mut() = Some(val));
}

/// Compile a regex, returning a cached copy when the same pattern+flags
/// have been compiled before.
pub(crate) fn get_or_compile_regex(pattern: &[u16], flags: &str) -> Result<Regex, String> {
    REGEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let key = (pattern.to_vec(), flags.to_string());
        if let Some(re) = cache.get(&key) {
            return Ok(re.clone());
        }
        let re = create_regex_from_utf16(pattern, flags)?;
        cache.insert(key, re.clone());
        Ok(re)
    })
}

pub fn initialize_regexp<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let regexp_ctor = new_js_object_data(mc);
    slot_set(mc, &regexp_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &regexp_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("RegExp")));

    // RegExp.length = 2 (per spec §22.2.4: writable:false, enumerable:false, configurable:true)
    object_set_key_value(mc, &regexp_ctor, "length", &Value::Number(2.0))?;
    regexp_ctor.borrow_mut(mc).set_non_enumerable("length");
    regexp_ctor.borrow_mut(mc).set_non_writable("length");

    // Stamp with OriginGlobal so cross-realm new Ctor() picks up the correct realm
    slot_set(mc, &regexp_ctor, InternalSlot::OriginGlobal, &Value::Object(*env));

    // Get Object.prototype
    let object_proto = if let Some(obj_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else {
        None
    };

    let regexp_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        regexp_proto.borrow_mut(mc).prototype = Some(proto);
    }
    // Mark as RegExp.prototype for getter spec checks
    slot_set(mc, &regexp_proto, InternalSlot::IsRegExpPrototype, &Value::Boolean(true));

    object_set_key_value(mc, &regexp_ctor, "prototype", &Value::Object(regexp_proto))?;
    regexp_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    regexp_ctor.borrow_mut(mc).set_non_writable("prototype");
    regexp_ctor.borrow_mut(mc).set_non_configurable("prototype");
    object_set_key_value(mc, &regexp_proto, "constructor", &Value::Object(regexp_ctor))?;

    // Register RegExp.escape static method (§22.2.4.3)
    let escape_fn = Value::Function("RegExp.escape".to_string());
    object_set_key_value(mc, &regexp_ctor, "escape", &escape_fn)?;
    regexp_ctor.borrow_mut(mc).set_non_enumerable("escape");
    // RegExp.escape.length = 1, .name = "escape"
    // (handled by native function dispatch)

    // Register instance methods
    let methods = vec!["exec", "test", "toString"];

    for method in methods {
        let val = Value::Function(format!("RegExp.prototype.{method}"));
        object_set_key_value(mc, &regexp_proto, method, &val)?;
        regexp_proto.borrow_mut(mc).set_non_enumerable(method);
    }

    // `compile` needs to track its realm's RegExp constructor for cross-realm
    // checks, so wrap it in an Object with OriginGlobal pointing to the realm.
    {
        let compile_fn_obj = new_js_object_data(mc);
        let compile_fn_val = Value::Function("RegExp.prototype.compile".to_string());
        compile_fn_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, compile_fn_val)));
        slot_set(mc, &compile_fn_obj, InternalSlot::OriginGlobal, &Value::Object(*env));
        // Set length = 2 and name = "compile" so property descriptor tests pass
        // Per spec: { writable: false, enumerable: false, configurable: true }
        object_set_key_value(mc, &compile_fn_obj, "length", &Value::Number(2.0))?;
        compile_fn_obj.borrow_mut(mc).set_non_enumerable("length");
        compile_fn_obj.borrow_mut(mc).set_non_writable("length");
        object_set_key_value(mc, &compile_fn_obj, "name", &Value::String(utf8_to_utf16("compile")))?;
        compile_fn_obj.borrow_mut(mc).set_non_enumerable("name");
        compile_fn_obj.borrow_mut(mc).set_non_writable("name");
        object_set_key_value(mc, &regexp_proto, "compile", &Value::Object(compile_fn_obj))?;
        regexp_proto.borrow_mut(mc).set_non_enumerable("compile");
    }

    if let Some(sym_ctor_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_ctor_val.borrow()
    {
        if let Some(match_sym_val) = object_get_key_value(sym_ctor, "match")
            && let Value::Symbol(match_sym) = &*match_sym_val.borrow()
        {
            let match_fn = Value::Function("RegExp.prototype.match".to_string());
            object_set_key_value(mc, &regexp_proto, *match_sym, &match_fn)?;
            regexp_proto.borrow_mut(mc).set_non_enumerable(*match_sym);
        }

        // Register Symbol.replace, Symbol.search, Symbol.split, Symbol.matchAll on RegExp.prototype
        for sym_name in ["replace", "search", "split", "matchAll"] {
            if let Some(sym_val) = object_get_key_value(sym_ctor, sym_name)
                && let Value::Symbol(sym) = &*sym_val.borrow()
            {
                let fn_val = Value::Function(format!("RegExp.prototype.{sym_name}"));
                object_set_key_value(mc, &regexp_proto, *sym, &fn_val)?;
                regexp_proto.borrow_mut(mc).set_non_enumerable(*sym);
            }
        }

        if let Some(species_sym_val) = object_get_key_value(sym_ctor, "species")
            && let Value::Symbol(species_sym) = &*species_sym_val.borrow()
        {
            let species_getter = Value::Function("RegExp[Symbol.species]".to_string());
            let species_accessor = Value::Property {
                value: None,
                getter: Some(Box::new(species_getter)),
                setter: None,
            };
            object_set_key_value(mc, &regexp_ctor, *species_sym, &species_accessor)?;
            regexp_ctor.borrow_mut(mc).set_non_enumerable(*species_sym);
        }
    }

    // Register accessor properties on RegExp.prototype per spec
    let accessor_props = vec![
        "source",
        "global",
        "ignoreCase",
        "multiline",
        "dotAll",
        "unicode",
        "sticky",
        "hasIndices",
        "unicodeSets",
        "flags",
    ];
    for prop_name in accessor_props {
        let getter_fn = Value::Function(format!("RegExp.prototype.get {prop_name}"));
        // Wrap getter in an Object with OriginGlobal so cross-realm .call() uses the
        // getter's defining realm (not the caller's realm) for the %RegExp.prototype%
        // identity check per spec §22.2.5 step 3a.
        let getter_obj = new_js_object_data(mc);
        getter_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, getter_fn)));
        slot_set(mc, &getter_obj, InternalSlot::OriginGlobal, &Value::Object(*env));
        // Spec-required function properties: length = 0, name = "get <prop>"
        object_set_key_value(mc, &getter_obj, "length", &Value::Number(0.0))?;
        getter_obj.borrow_mut(mc).set_non_enumerable("length");
        getter_obj.borrow_mut(mc).set_non_writable("length");
        object_set_key_value(mc, &getter_obj, "name", &Value::String(utf8_to_utf16(&format!("get {prop_name}"))))?;
        getter_obj.borrow_mut(mc).set_non_enumerable("name");
        getter_obj.borrow_mut(mc).set_non_writable("name");
        // Set prototype to Function.prototype
        if let Some(func_ctor_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(func_proto_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*func_proto_val.borrow()
        {
            getter_obj.borrow_mut(mc).prototype = Some(*func_proto);
        }
        let getter = Value::Object(getter_obj);
        let accessor = Value::Property {
            value: None,
            getter: Some(Box::new(getter)),
            setter: None,
        };
        object_set_key_value(mc, &regexp_proto, prop_name, &accessor)?;
        regexp_proto.borrow_mut(mc).set_non_enumerable(prop_name);
    }

    regexp_proto.borrow_mut(mc).set_non_enumerable("constructor");

    env_set(mc, env, "RegExp", &Value::Object(regexp_ctor))?;

    // Ensure RegExp constructor [[Prototype]] = Function.prototype
    if let Err(e) = crate::core::set_internal_prototype_from_constructor(mc, &regexp_ctor, env, "Function") {
        log::warn!("Failed to set RegExp constructor's internal prototype from Function: {e:?}");
    }

    // AnnexB B.2.4: Legacy RegExp static accessor properties on the constructor
    // Properties with both getter and setter: input/$_
    // Properties with getter only (setter = undefined): $1-$9, lastMatch/$&,
    //   lastParen/$+, leftContext/$`, rightContext/$'
    {
        let getter_only_props = [
            "$1",
            "$2",
            "$3",
            "$4",
            "$5",
            "$6",
            "$7",
            "$8",
            "$9",
            "lastMatch",
            "$&",
            "lastParen",
            "$+",
            "leftContext",
            "$`",
            "rightContext",
            "$'",
        ];
        let getter_setter_props = ["input", "$_"];

        // Get Function.prototype for getter function objects
        let func_proto = if let Some(func_ctor_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(func_proto_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*func_proto_val.borrow()
        {
            Some(*func_proto)
        } else {
            None
        };

        for prop in getter_only_props {
            let getter_fn = Value::Function(format!("RegExp.legacy.get {prop}"));
            let getter_obj = new_js_object_data(mc);
            getter_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, getter_fn)));
            slot_set(mc, &getter_obj, InternalSlot::OriginGlobal, &Value::Object(*env));
            object_set_key_value(mc, &getter_obj, "length", &Value::Number(0.0))?;
            getter_obj.borrow_mut(mc).set_non_enumerable("length");
            getter_obj.borrow_mut(mc).set_non_writable("length");
            object_set_key_value(mc, &getter_obj, "name", &Value::String(utf8_to_utf16(&format!("get {prop}"))))?;
            getter_obj.borrow_mut(mc).set_non_enumerable("name");
            getter_obj.borrow_mut(mc).set_non_writable("name");
            if let Some(fp) = func_proto {
                getter_obj.borrow_mut(mc).prototype = Some(fp);
            }
            let accessor = Value::Property {
                value: None,
                getter: Some(Box::new(Value::Object(getter_obj))),
                setter: None, // spec says [[Set]]: undefined for read-only legacy props
            };
            object_set_key_value(mc, &regexp_ctor, prop, &accessor)?;
            regexp_ctor.borrow_mut(mc).set_non_enumerable(prop);
        }

        for prop in getter_setter_props {
            let getter_fn = Value::Function(format!("RegExp.legacy.get {prop}"));
            let getter_obj = new_js_object_data(mc);
            getter_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, getter_fn)));
            slot_set(mc, &getter_obj, InternalSlot::OriginGlobal, &Value::Object(*env));
            object_set_key_value(mc, &getter_obj, "length", &Value::Number(0.0))?;
            getter_obj.borrow_mut(mc).set_non_enumerable("length");
            getter_obj.borrow_mut(mc).set_non_writable("length");
            object_set_key_value(mc, &getter_obj, "name", &Value::String(utf8_to_utf16(&format!("get {prop}"))))?;
            getter_obj.borrow_mut(mc).set_non_enumerable("name");
            getter_obj.borrow_mut(mc).set_non_writable("name");
            if let Some(fp) = func_proto {
                getter_obj.borrow_mut(mc).prototype = Some(fp);
            }

            let setter_fn = Value::Function(format!("RegExp.legacy.set {prop}"));
            let setter_obj = new_js_object_data(mc);
            setter_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, setter_fn)));
            slot_set(mc, &setter_obj, InternalSlot::OriginGlobal, &Value::Object(*env));
            object_set_key_value(mc, &setter_obj, "length", &Value::Number(1.0))?;
            setter_obj.borrow_mut(mc).set_non_enumerable("length");
            setter_obj.borrow_mut(mc).set_non_writable("length");
            object_set_key_value(mc, &setter_obj, "name", &Value::String(utf8_to_utf16(&format!("set {prop}"))))?;
            setter_obj.borrow_mut(mc).set_non_enumerable("name");
            setter_obj.borrow_mut(mc).set_non_writable("name");
            if let Some(fp) = func_proto {
                setter_obj.borrow_mut(mc).prototype = Some(fp);
            }

            let accessor = Value::Property {
                value: None,
                getter: Some(Box::new(Value::Object(getter_obj))),
                setter: Some(Box::new(Value::Object(setter_obj))),
            };
            object_set_key_value(mc, &regexp_ctor, prop, &accessor)?;
            regexp_ctor.borrow_mut(mc).set_non_enumerable(prop);
        }
    }

    Ok(())
}

pub fn internal_get_regex_pattern(obj: &JSObjectDataPtr) -> Result<Vec<u16>, JSError> {
    match slot_get(obj, &InternalSlot::Regex) {
        Some(val) => match &*val.borrow() {
            Value::String(s) => Ok(s.clone()),
            _ => Err(raise_type_error!("Invalid regex pattern")),
        },
        None => Err(raise_type_error!("Invalid regex object")),
    }
}

/// Handle RegExp.prototype getter accessors (source, global, ignoreCase, etc.)
/// These read internal slots from the RegExp instance (`this`).
/// For `flags`, the spec requires reading observable properties from any object.
/// `mc` and `env` are needed for `flags` getter to invoke accessors.
pub(crate) fn handle_regexp_getter_with_this<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    this_val: &Value<'gc>,
    prop: &str,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    // Helper: throw a TypeError using the current env's TypeError constructor
    // so cross-realm getters produce errors from their own realm.
    let throw_realm_type_error = |msg: String| -> EvalError<'gc> {
        let js_err = raise_type_error!(msg);
        let val = crate::core::js_error_to_value(mc, env, &js_err);
        EvalError::Throw(val, None, None)
    };

    // Step 1: If Type(this) is not Object, throw TypeError
    let obj = match this_val {
        Value::Object(o) => o,
        _ => {
            return Err(throw_realm_type_error(format!(
                "RegExp.prototype.{prop} getter called on incompatible receiver"
            )));
        }
    };

    // Helper: check if obj is the *current realm's* %RegExp.prototype% (identity check)
    let is_this_realm_regexp_proto = if let Some(regexp_ctor_val) = object_get_key_value(env, "RegExp")
        && let Value::Object(regexp_ctor) = &*regexp_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(regexp_ctor, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        std::ptr::eq(&**obj as *const _, &**proto_obj as *const _)
    } else {
        false
    };

    // Special case: `flags` getter should work on any object per spec §22.2.5.3
    // MUST always read properties observably (never use internal flags slot)
    if prop == "flags" {
        // For RegExp.prototype with no actual regex, return ""
        if is_this_realm_regexp_proto && slot_get(obj, &InternalSlot::Regex).is_none() {
            return Ok(Some(Value::String(Vec::new())));
        }
        // Build flags from observable property reads (spec §22.2.5.3)
        let mut result = String::new();
        let flag_props: &[(&str, char)] = &[
            ("hasIndices", 'd'),
            ("global", 'g'),
            ("ignoreCase", 'i'),
            ("multiline", 'm'),
            ("dotAll", 's'),
            ("unicode", 'u'),
            ("unicodeSets", 'v'),
            ("sticky", 'y'),
        ];
        for (prop_name, flag_char) in flag_props {
            let val = crate::core::get_property_with_accessors(mc, env, obj, *prop_name)?;
            if val.to_truthy() {
                result.push(*flag_char);
            }
        }
        return Ok(Some(Value::String(utf8_to_utf16(&result))));
    }

    // Check if obj is actually a RegExp (has [[OriginalFlags]] / __regex internal slot)
    let is_regexp = slot_get(obj, &InternalSlot::Regex).is_some();
    if !is_regexp {
        // Step 3: If R does not have [[OriginalFlags]], then
        //   a. If SameValue(R, %RegExpPrototype%) is true, return undefined.
        //   b. Otherwise, throw TypeError.
        // SameValue means identity check against the *current realm's* %RegExpPrototype%.
        if is_this_realm_regexp_proto {
            return match prop {
                "source" => Ok(Some(Value::String(utf8_to_utf16("(?:)")))),
                _ => Ok(Some(Value::Undefined)), // global, ignoreCase, etc.
            };
        }
        // Otherwise, throw TypeError (includes cross-realm RegExp.prototype)
        return Err(throw_realm_type_error(format!(
            "RegExp.prototype.{prop} getter requires a RegExp object"
        )));
    }
    match prop {
        "source" => {
            if let Some(val) = slot_get(obj, &InternalSlot::Regex) {
                if let Value::String(s) = &*val.borrow() {
                    if s.is_empty() {
                        return Ok(Some(Value::String(utf8_to_utf16("(?:)"))));
                    }
                    // EscapeRegExpPattern: escape /, line terminators
                    let escaped = escape_regexp_pattern(s);
                    return Ok(Some(Value::String(escaped)));
                }
                Ok(Some(val.borrow().clone()))
            } else {
                Ok(Some(Value::String(utf8_to_utf16("(?:)"))))
            }
        }
        "global" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexGlobal))),
        "ignoreCase" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexIgnoreCase))),
        "multiline" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexMultiline))),
        "dotAll" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexDotAll))),
        "unicode" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexUnicode))),
        "sticky" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexSticky))),
        "hasIndices" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexHasIndices))),
        "unicodeSets" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexUnicodeSets))),
        _ => Ok(None),
    }
}

fn get_bool_slot<'gc>(obj: &JSObjectDataPtr<'gc>, slot: &InternalSlot) -> Value<'gc> {
    if let Some(val) = slot_get(obj, slot) {
        val.borrow().clone()
    } else {
        Value::Boolean(false)
    }
}

pub fn create_regex_from_utf16(pattern: &[u16], flags: &str) -> Result<Regex, String> {
    if flags.contains('u') || flags.contains('v') {
        // Unicode / unicodeSets mode: decode surrogate pairs into code points
        let it = std::char::decode_utf16(pattern.iter().cloned()).map(|r| match r {
            Ok(c) => c as u32,
            Err(e) => e.unpaired_surrogate() as u32,
        });
        Regex::from_unicode(it, flags).map_err(|e| e.to_string())
    } else {
        // Non-unicode mode: each UTF-16 code unit is a separate element,
        // EXCEPT inside named group identifiers (?<name> and \k<name>
        // where surrogate pairs must be decoded so regress accepts them.
        let processed = preprocess_pattern_non_unicode(pattern);
        Regex::from_unicode(processed.into_iter(), flags).map_err(|e| e.to_string())
    }
}

/// For non-unicode regex patterns, pass raw UTF-16 code units to regress so
/// that supplementary characters are matched as two separate code units (via
/// `find_from_ucs2`).  However, named capture group identifiers (`(?<name>`)
/// and named backreferences (`\k<name>`) require valid Unicode identifier
/// characters, so surrogate pairs inside those contexts are decoded into full
/// code points.
fn preprocess_pattern_non_unicode(pattern: &[u16]) -> Vec<u32> {
    let mut result = Vec::with_capacity(pattern.len());
    let mut i = 0;
    let len = pattern.len();

    while i < len {
        // ---- named capture group  (?<name>  ---------------------
        if i + 3 <= len && pattern[i] == b'(' as u16 && pattern[i + 1] == b'?' as u16 && pattern[i + 2] == b'<' as u16 {
            // Distinguish from look-behind (?<= / (?<!
            if i + 3 < len && (pattern[i + 3] == b'=' as u16 || pattern[i + 3] == b'!' as u16) {
                result.push(pattern[i] as u32);
                i += 1;
                continue;
            }
            // Push (?<
            result.push(b'(' as u32);
            result.push(b'?' as u32);
            result.push(b'<' as u32);
            i += 3;
            // Decode surrogates inside the group name (until >)
            while i < len && pattern[i] != b'>' as u16 {
                if i + 1 < len && (0xD800..=0xDBFF).contains(&pattern[i]) && (0xDC00..=0xDFFF).contains(&pattern[i + 1]) {
                    let hi = pattern[i] as u32;
                    let lo = pattern[i + 1] as u32;
                    result.push(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00));
                    i += 2;
                } else {
                    result.push(pattern[i] as u32);
                    i += 1;
                }
            }
            // Push the closing >
            if i < len {
                result.push(pattern[i] as u32);
                i += 1;
            }
            continue;
        }

        // ---- named back-reference  \k<name>  --------------------
        if i + 3 <= len && pattern[i] == b'\\' as u16 && pattern[i + 1] == b'k' as u16 && pattern[i + 2] == b'<' as u16 {
            result.push(b'\\' as u32);
            result.push(b'k' as u32);
            result.push(b'<' as u32);
            i += 3;
            while i < len && pattern[i] != b'>' as u16 {
                if i + 1 < len && (0xD800..=0xDBFF).contains(&pattern[i]) && (0xDC00..=0xDFFF).contains(&pattern[i + 1]) {
                    let hi = pattern[i] as u32;
                    let lo = pattern[i + 1] as u32;
                    result.push(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00));
                    i += 2;
                } else {
                    result.push(pattern[i] as u32);
                    i += 1;
                }
            }
            if i < len {
                result.push(pattern[i] as u32);
                i += 1;
            }
            continue;
        }

        // ---- escaped character  \X  — skip so \ before ( etc. isn't mis-parsed
        if pattern[i] == b'\\' as u16 && i + 1 < len {
            result.push(pattern[i] as u32);
            result.push(pattern[i + 1] as u32);
            i += 2;
            continue;
        }

        // ---- default: raw code unit
        result.push(pattern[i] as u32);
        i += 1;
    }
    result
}

/// EscapeRegExpPattern — escape forward slashes and line terminators so the
/// source property re-renders as a valid regex literal.
fn escape_regexp_pattern(s: &[u16]) -> Vec<u16> {
    let mut result = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let ch = s[i];
        match ch {
            // Forward slash: escape only if not already preceded by backslash
            0x002F => {
                // '/'
                // Check if previous char is a backslash (already escaped)
                let already_escaped = !result.is_empty() && *result.last().unwrap() == 0x005C;
                if !already_escaped {
                    result.push(0x005C); // '\'
                }
                result.push(ch);
            }
            // Line terminators: escape
            0x000A => {
                result.push(0x005C);
                result.push(0x006E);
            } // \n
            0x000D => {
                result.push(0x005C);
                result.push(0x0072);
            } // \r
            0x2028 => {
                result.push(0x005C);
                result.push(0x0075); // \u2028
                result.push(0x0032);
                result.push(0x0030);
                result.push(0x0032);
                result.push(0x0038);
            }
            0x2029 => {
                result.push(0x005C);
                result.push(0x0075); // \u2029
                result.push(0x0032);
                result.push(0x0030);
                result.push(0x0032);
                result.push(0x0039);
            }
            _ => result.push(ch),
        }
        i += 1;
    }
    result
}

fn contains_lone_surrogate(units: &[u16]) -> bool {
    let mut i = 0;
    while i < units.len() {
        let u = units[i];
        if (0xD800..=0xDBFF).contains(&u) {
            if i + 1 >= units.len() {
                return true;
            }
            let next = units[i + 1];
            if !(0xDC00..=0xDFFF).contains(&next) {
                return true;
            }
            i += 2;
            continue;
        }
        if (0xDC00..=0xDFFF).contains(&u) {
            return true;
        }
        i += 1;
    }
    false
}

fn validate_named_group_identifiers(pattern_u16: &[u16]) -> Result<(), JSError> {
    let mut i = 0usize;
    while i + 2 < pattern_u16.len() {
        if pattern_u16[i] == b'(' as u16 && pattern_u16[i + 1] == b'?' as u16 && pattern_u16[i + 2] == b'<' as u16 {
            if i + 3 < pattern_u16.len() {
                let next = pattern_u16[i + 3];
                if next == b'=' as u16 || next == b'!' as u16 {
                    i += 1;
                    continue;
                }
            }

            let mut j = i + 3;
            while j < pattern_u16.len() && pattern_u16[j] != b'>' as u16 {
                j += 1;
            }

            if j < pattern_u16.len() {
                if contains_lone_surrogate(&pattern_u16[i + 3..j]) {
                    return Err(raise_syntax_error!("Invalid token at named capture group identifier"));
                }
                i = j;
            }
        }
        i += 1;
    }
    Ok(())
}

pub fn is_regex_object(obj: &JSObjectDataPtr) -> bool {
    internal_get_regex_pattern(obj).is_ok()
}

pub fn get_regex_pattern(obj: &JSObjectDataPtr) -> Result<String, JSError> {
    let pattern_utf16 = internal_get_regex_pattern(obj)?;
    Ok(utf16_to_utf8(&pattern_utf16))
}

pub fn get_regex_literal_pattern(obj: &JSObjectDataPtr) -> Result<String, JSError> {
    let pat = get_regex_pattern(obj)?;
    let flags = match slot_get(obj, &InternalSlot::Flags) {
        Some(val) => match &*val.borrow() {
            Value::String(s) => utf16_to_utf8(s),
            _ => String::new(),
        },
        None => String::new(),
    };
    if flags.is_empty() {
        Ok(format!("/{pat}/"))
    } else {
        Ok(format!("/{pat}/{flags}"))
    }
}

fn create_regexp_object_from_parts<'gc>(
    mc: &MutationContext<'gc>,
    env: Option<&JSObjectDataPtr<'gc>>,
    pattern_u16: Vec<u16>,
    flags: String,
    validate_pattern: bool,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let mut global = false;
    let mut ignore_case = false;
    let mut multiline = false;
    let mut dot_matches_new_line = false;
    let mut swap_greed = false;
    let mut unicode = false;
    let mut sticky = false;
    let mut crlf = false;
    let mut has_indices = false;
    let mut unicode_sets = false;

    // Check for duplicate flags
    let mut seen_flags = std::collections::HashSet::new();
    for flag in flags.chars() {
        if !seen_flags.insert(flag) {
            return Err(raise_syntax_error!(format!("Duplicate RegExp flag: {flag}")).into());
        }
    }

    for flag in flags.chars() {
        match flag {
            'g' => global = true,
            'i' => ignore_case = true,
            'm' => multiline = true,
            's' => dot_matches_new_line = true,
            'U' => swap_greed = true,
            'u' => unicode = true,
            'y' => sticky = true,
            'R' => crlf = true,
            'd' => has_indices = true,
            'v' => unicode_sets = true,
            _ => {
                return Err(raise_syntax_error!(format!("Invalid RegExp flag: {flag}")).into());
            }
        }
    }

    if unicode && unicode_sets {
        return Err(raise_syntax_error!("Invalid RegExp flags: cannot use both 'u' and 'v'").into());
    }

    // Validate named capture identifiers to reject malformed UTF-16 names
    // (e.g. lone surrogates), matching Test262 expectations.
    validate_named_group_identifiers(&pattern_u16).map_err(EvalError::from)?;

    let mut regress_flags = String::new();
    for c in flags.chars() {
        if "gimsuvy".contains(c) {
            regress_flags.push(c);
        }
    }

    if validate_pattern && let Err(e) = get_or_compile_regex(&pattern_u16, &regress_flags) {
        return Err(raise_syntax_error!(format!("Invalid RegExp: {}", e)).into());
    }

    let regexp_obj = new_js_object_data(mc);
    if let Some(env) = env
        && let Some(regexp_ctor_val) = object_get_key_value(env, "RegExp")
        && let Value::Object(regexp_ctor_obj) = &*regexp_ctor_val.borrow()
        && let Some(regexp_proto_val) = object_get_key_value(regexp_ctor_obj, "prototype")
        && let Value::Object(regexp_proto_obj) = &*regexp_proto_val.borrow()
    {
        regexp_obj.borrow_mut(mc).prototype = Some(*regexp_proto_obj);
    }
    slot_set(mc, &regexp_obj, InternalSlot::Regex, &Value::String(pattern_u16.clone()));
    slot_set(mc, &regexp_obj, InternalSlot::Flags, &Value::String(utf8_to_utf16(&flags)));
    slot_set(mc, &regexp_obj, InternalSlot::RegexGlobal, &Value::Boolean(global));
    slot_set(mc, &regexp_obj, InternalSlot::RegexIgnoreCase, &Value::Boolean(ignore_case));
    slot_set(mc, &regexp_obj, InternalSlot::RegexMultiline, &Value::Boolean(multiline));
    slot_set(mc, &regexp_obj, InternalSlot::RegexDotAll, &Value::Boolean(dot_matches_new_line));
    slot_set(mc, &regexp_obj, InternalSlot::RegexUnicode, &Value::Boolean(unicode));
    slot_set(mc, &regexp_obj, InternalSlot::RegexSticky, &Value::Boolean(sticky));
    slot_set(mc, &regexp_obj, InternalSlot::SwapGreed, &Value::Boolean(swap_greed));
    slot_set(mc, &regexp_obj, InternalSlot::Crlf, &Value::Boolean(crlf));
    slot_set(mc, &regexp_obj, InternalSlot::RegexHasIndices, &Value::Boolean(has_indices));
    slot_set(mc, &regexp_obj, InternalSlot::RegexUnicodeSets, &Value::Boolean(unicode_sets));

    object_set_key_value(mc, &regexp_obj, "lastIndex", &Value::Number(0.0))?;
    regexp_obj.borrow_mut(mc).set_non_enumerable("lastIndex");
    regexp_obj.borrow_mut(mc).set_non_configurable("lastIndex");

    // Per spec, source/global/flags/etc. are accessor properties on RegExp.prototype,
    // not own data properties on instances. The prototype getters read internal slots
    // (__regex, __global, __flags, etc.) from the instance via `this`.
    // Do NOT set per-instance data properties for these.

    Ok(Value::Object(regexp_obj))
}

pub(crate) fn create_regexp_object_fast_for_eval<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    pattern_u16: Vec<u16>,
    flags: String,
) -> Result<Value<'gc>, EvalError<'gc>> {
    create_regexp_object_from_parts(mc, Some(env), pattern_u16, flags, false)
}

/// Handle RegExp constructor calls
pub(crate) fn handle_regexp_constructor<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    handle_regexp_constructor_with_env(mc, None, args)
}

/// §7.2.8 IsRegExp(argument)
/// Returns true if the argument has a truthy @@match property, or has [[RegExpMatcher]] internal slot.
/// Uses proper Get (invokes getters) for @@match check per spec.
fn is_regexp_with_env<'gc>(
    mc: &MutationContext<'gc>,
    val: &Value<'gc>,
    env: Option<&JSObjectDataPtr<'gc>>,
) -> Result<bool, EvalError<'gc>> {
    if let Value::Object(obj) = val {
        // Step 1-2: Check @@match property via proper Get
        if let Some(env) = env
            && let Some(sym_ctor_val) = object_get_key_value(env, "Symbol")
            && let Value::Object(sym_ctor) = &*sym_ctor_val.borrow()
            && let Some(match_sym_val) = object_get_key_value(sym_ctor, "match")
            && let Value::Symbol(match_sym) = &*match_sym_val.borrow()
        {
            let matcher = crate::core::get_property_with_accessors(mc, env, obj, crate::core::PropertyKey::Symbol(*match_sym))?;
            if !matches!(matcher, Value::Undefined) {
                return Ok(matcher.to_truthy());
            }
        }
        // Step 3: Check [[RegExpMatcher]] internal slot
        return Ok(slot_get(obj, &InternalSlot::Regex).is_some());
    }
    Ok(false)
}

/// Extract pattern and flags from a value that might be a RegExp object.
/// Returns Some((pattern_u16, flags_str)) if the value is a RegExp, None otherwise.
fn extract_regexp_parts<'gc>(val: &Value<'gc>) -> Option<(Vec<u16>, String)> {
    if let Value::Object(obj) = val
        && let Some(pat_rc) = slot_get(obj, &InternalSlot::Regex)
    {
        let pattern = match &*pat_rc.borrow() {
            Value::String(s) => s.clone(),
            _ => Vec::new(),
        };
        let flags = match slot_get(obj, &InternalSlot::Flags) {
            Some(f_rc) => match &*f_rc.borrow() {
                Value::String(s) => utf16_to_utf8(s),
                _ => String::new(),
            },
            None => String::new(),
        };
        return Some((pattern, flags));
    }
    None
}

/// Coerce a value to a pattern string for the RegExp constructor.
fn value_to_pattern_u16<'gc>(val: &Value<'gc>) -> Vec<u16> {
    match val {
        Value::String(s) => s.clone(),
        Value::Number(n) => utf8_to_utf16(&n.to_string()),
        Value::Boolean(b) => utf8_to_utf16(&b.to_string()),
        Value::Null => utf8_to_utf16("null"),
        Value::Undefined => Vec::new(),
        _ => utf8_to_utf16(&crate::core::value_to_string(val)),
    }
}

pub(crate) fn handle_regexp_constructor_with_env<'gc>(
    mc: &MutationContext<'gc>,
    env: Option<&JSObjectDataPtr<'gc>>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError<'gc>> {
    let arg0 = args.first().cloned().unwrap_or(Value::Undefined);
    let arg1 = args.get(1).cloned();

    // Per spec 22.2.3.1 step 2: always call IsRegExp first so side effects
    // on Symbol.match (e.g. recompiling via .compile()) are observable
    // before we read internal slots in steps 4-5.
    let pattern_is_regexp = is_regexp_with_env(mc, &arg0, env)?;

    // Step 4: If pattern has [[RegExpMatcher]] internal slot, use internal source/flags
    if let Some((re_pattern, re_flags)) = extract_regexp_parts(&arg0) {
        let flags = if let Some(f) = &arg1 {
            match f {
                Value::Undefined => re_flags,
                Value::String(s) => utf16_to_utf8(s),
                Value::Object(_) => {
                    if let Some(env) = env {
                        let prim = crate::core::to_primitive(mc, f, "string", env)?;
                        crate::core::value_to_string(&prim)
                    } else {
                        crate::core::value_to_string(f)
                    }
                }
                _ => crate::core::value_to_string(f),
            }
        } else {
            re_flags
        };
        return create_regexp_object_from_parts(mc, env, re_pattern, flags, true);
    }

    // Step 5: Else if patternIsRegExp, read source/flags from regexp-like object
    if pattern_is_regexp && let Value::Object(obj) = &arg0 {
        // Step 5a: P = ? Get(pattern, "source")
        let source_val = if let Some(env) = env {
            crate::core::get_property_with_accessors(mc, env, obj, "source")?
        } else if let Some(v) = object_get_key_value(obj, "source") {
            v.borrow().clone()
        } else {
            Value::Undefined
        };
        let pattern_u16 = match &source_val {
            Value::String(s) => s.clone(),
            Value::Undefined => Vec::new(),
            other => utf8_to_utf16(&crate::core::value_to_string(other)),
        };

        // Step 5b-c: If flags undefined, F = ? Get(pattern, "flags"); else F = flags
        let flags = if let Some(f) = &arg1 {
            match f {
                Value::Undefined => {
                    // Read flags from object
                    let flags_val = if let Some(env) = env {
                        crate::core::get_property_with_accessors(mc, env, obj, "flags")?
                    } else if let Some(v) = object_get_key_value(obj, "flags") {
                        v.borrow().clone()
                    } else {
                        Value::Undefined
                    };
                    match &flags_val {
                        Value::String(s) => utf16_to_utf8(s),
                        Value::Undefined => String::new(),
                        other => crate::core::value_to_string(other),
                    }
                }
                Value::String(s) => utf16_to_utf8(s),
                Value::Object(_) => {
                    if let Some(env) = env {
                        let prim = crate::core::to_primitive(mc, f, "string", env)?;
                        crate::core::value_to_string(&prim)
                    } else {
                        crate::core::value_to_string(f)
                    }
                }
                _ => crate::core::value_to_string(f),
            }
        } else {
            // flags argument absent → read from object
            let flags_val = if let Some(env) = env {
                crate::core::get_property_with_accessors(mc, env, obj, "flags")?
            } else if let Some(v) = object_get_key_value(obj, "flags") {
                v.borrow().clone()
            } else {
                Value::Undefined
            };
            match &flags_val {
                Value::String(s) => utf16_to_utf8(s),
                Value::Undefined => String::new(),
                other => crate::core::value_to_string(other),
            }
        };

        return create_regexp_object_from_parts(mc, env, pattern_u16, flags, true);
    }

    // ToString coercion on pattern (undefined → empty)
    let pattern_u16 = match &arg0 {
        Value::Undefined => Vec::new(),
        Value::String(s) => s.clone(),
        Value::Object(_) => {
            if let Some(env) = env {
                let prim = crate::core::to_primitive(mc, &arg0, "string", env)?;
                match prim {
                    Value::String(s) => s,
                    other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                }
            } else {
                utf8_to_utf16(&crate::core::value_to_string(&arg0))
            }
        }
        _ => value_to_pattern_u16(&arg0),
    };

    // ToString coercion on flags
    let flags = match &arg1 {
        Some(Value::String(s)) => utf16_to_utf8(s),
        Some(Value::Undefined) | None => String::new(),
        Some(Value::Object(_)) => {
            if let Some(env) = env {
                let prim = crate::core::to_primitive(mc, arg1.as_ref().unwrap(), "string", env)?;
                crate::core::value_to_string(&prim)
            } else {
                crate::core::value_to_string(arg1.as_ref().unwrap())
            }
        }
        Some(f) => crate::core::value_to_string(f),
    };

    create_regexp_object_from_parts(mc, env, pattern_u16, flags, true)
}

/// Handle RegExp() called as a function (without new).
/// Per spec §21.2.3.1: if pattern is RegExp, flags is undefined, and
/// pattern.constructor === RegExp, return the same object.
pub(crate) fn handle_regexp_call_with_env<'gc>(
    mc: &MutationContext<'gc>,
    env: Option<&JSObjectDataPtr<'gc>>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError<'gc>> {
    let arg0 = args.first().cloned().unwrap_or(Value::Undefined);
    let arg1 = args.get(1).cloned();

    // Step 1: Let patternIsRegExp = IsRegExp(pattern)
    let pattern_is_regexp = is_regexp_with_env(mc, &arg0, env)?;

    // Step 4b: If patternIsRegExp is true and flags is undefined
    if pattern_is_regexp {
        let flags_undefined = matches!(&arg1, None | Some(Value::Undefined));
        if flags_undefined && let Value::Object(obj) = &arg0 {
            // Step 4b.i: Let patternConstructor = ? Get(pattern, "constructor")
            let ctor_val = if let Some(env) = env {
                crate::core::get_property_with_accessors(mc, env, obj, "constructor")?
            } else if let Some(v) = object_get_key_value(obj, "constructor") {
                v.borrow().clone()
            } else {
                Value::Undefined
            };

            // Step 4b.iii: If SameValue(newTarget, patternConstructor) is true, return pattern
            let ctor_is_regexp = if let Some(env) = env {
                if let Some(regexp_val) = object_get_key_value(env, "RegExp") {
                    match (&ctor_val, &*regexp_val.borrow()) {
                        (Value::Object(a), Value::Object(b)) => std::ptr::eq(a.as_ptr(), b.as_ptr()),
                        _ => false,
                    }
                } else {
                    false
                }
            } else {
                false
            };
            if ctor_is_regexp {
                return Ok(arg0);
            }
        }
    }

    // Otherwise, construct a new RegExp
    handle_regexp_constructor_with_env(mc, env, args)
}

/// Read the flags string from a RegExp object's internal Flags slot.
/// ToLength: clamp to integer in [0, 2^53-1] (for primitive values only)
#[allow(dead_code)]
fn to_length_primitive(val: &Value) -> usize {
    let n = match val {
        Value::Number(n) => {
            if n.is_nan() || *n <= 0.0 {
                return 0;
            }
            if n.is_infinite() {
                return (1usize << 53) - 1;
            }
            n.trunc() as usize
        }
        Value::String(s) => {
            let s_utf8 = utf16_to_utf8(s);
            match s_utf8.parse::<f64>() {
                Ok(n) if !n.is_nan() && n > 0.0 => n.trunc() as usize,
                _ => 0,
            }
        }
        Value::Boolean(b) => {
            if *b {
                1
            } else {
                0
            }
        }
        Value::Undefined | Value::Null => 0,
        _ => 0,
    };
    n.min((1usize << 53) - 1)
}

/// ToLength with ToPrimitive for objects: calls valueOf/toString on objects.
/// Throws TypeError for Symbol values (per ToNumber spec).
fn to_length_with_coercion<'gc>(mc: &MutationContext<'gc>, val: &Value<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<usize, EvalError<'gc>> {
    // Use to_number_with_env which properly handles ToPrimitive for objects
    // and throws TypeError for Symbols
    let n = crate::core::to_number_with_env(mc, env, val)?;
    if n.is_nan() || n <= 0.0 {
        return Ok(0);
    }
    if n.is_infinite() {
        return Ok((1usize << 53) - 1);
    }
    Ok((n.trunc() as usize).min((1usize << 53) - 1))
}

/// Try to set lastIndex; throw TypeError if non-writable (strict mode)
fn set_last_index_checked<'gc>(mc: &MutationContext<'gc>, object: &JSObjectDataPtr<'gc>, value: f64) -> Result<(), EvalError<'gc>> {
    // Check if lastIndex is non-writable
    if object
        .borrow()
        .non_writable
        .contains(&crate::core::PropertyKey::String("lastIndex".to_string()))
    {
        return Err(raise_type_error!("Cannot set property lastIndex of [object Object] which has only a getter").into());
    }
    object_set_key_value(mc, object, "lastIndex", &Value::Number(value))?;
    Ok(())
}

/// §22.2.4.2 SpeciesConstructor(O, defaultConstructor)
/// Returns the species constructor for a RegExp-like object.
fn species_constructor<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    // Step 1: Let C = ? Get(O, "constructor")
    let ctor = crate::core::get_property_with_accessors(mc, env, obj, "constructor")?;

    // Step 2: If C is undefined, return defaultConstructor
    if matches!(ctor, Value::Undefined) {
        return Ok(None);
    }

    // Step 3: If Type(C) is not Object, throw TypeError
    // (Functions count as objects in JS)
    let ctor_obj = match &ctor {
        Value::Object(o) => *o,
        _ => return Err(raise_type_error!("Species constructor: constructor is not an object").into()),
    };

    // Step 4: Let S = ? Get(C, @@species)
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(species_sym_val) = object_get_key_value(sym_obj, "species")
        && let Value::Symbol(species_sym) = &*species_sym_val.borrow()
    {
        let species = crate::core::get_property_with_accessors(mc, env, &ctor_obj, crate::core::PropertyKey::Symbol(*species_sym))?;

        // Step 5: If S is undefined or null, return defaultConstructor
        if matches!(species, Value::Undefined | Value::Null) {
            return Ok(None);
        }

        // Step 6: If IsConstructor(S), return S
        let is_ctor = match &species {
            Value::Object(o) => {
                o.borrow().class_def.is_some()
                    || slot_get(o, &InternalSlot::IsConstructor).is_some()
                    || slot_get(o, &InternalSlot::NativeCtor).is_some()
                    || o.borrow().get_closure().is_some()
            }
            Value::Closure(cl) | Value::AsyncClosure(cl) => !cl.is_arrow,
            Value::Function(_) => true,
            _ => false,
        };
        if is_ctor {
            return Ok(Some(species));
        }

        // Step 7: Throw TypeError
        return Err(raise_type_error!("Species constructor is not a constructor").into());
    }

    // Symbol.species not available — use default
    Ok(None)
}

#[allow(dead_code)]
fn internal_get_flags_string(object: &JSObjectDataPtr) -> String {
    match slot_get(object, &InternalSlot::Flags) {
        Some(val) => match &*val.borrow() {
            Value::String(s) => utf16_to_utf8(s),
            _ => String::new(),
        },
        None => String::new(),
    }
}

/// §22.2.5.2.1 RegExpExec abstract operation.
/// Checks for a user-defined `exec` property on the regexp object;
/// if callable, delegates to it. Otherwise falls back to built-in exec.
fn regexp_exec_abstract<'gc>(
    mc: &MutationContext<'gc>,
    rx: &JSObjectDataPtr<'gc>,
    string: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 1: Let exec be ? Get(R, "exec")
    let exec_val = crate::core::get_property_with_accessors(mc, env, rx, "exec")?;
    // Step 2: If IsCallable(exec), then
    let is_callable = match &exec_val {
        Value::Function(_) | Value::Closure(_) => true,
        Value::Object(o) => o.borrow().get_closure().is_some(),
        _ => false,
    };
    if is_callable {
        // Call exec with rx as this, [string] as args
        let result =
            crate::js_promise::call_function_with_this(mc, &exec_val, Some(&Value::Object(*rx)), std::slice::from_ref(string), env)?;
        // Step 2.a: Result must be null or object
        match &result {
            Value::Null | Value::Object(_) => return Ok(result),
            _ => return Err(raise_type_error!("RegExpExec: exec result must be null or an object").into()),
        }
    }
    // Step 3: Fall back to built-in exec
    handle_regexp_method(mc, rx, "exec", std::slice::from_ref(string), env)
}

/// Handle RegExp instance method calls
pub(crate) fn handle_regexp_method<'gc>(
    mc: &MutationContext<'gc>,
    object: &JSObjectDataPtr<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "exec" => {
            // Step 1: If this does not have [[RegExpMatcher]], throw TypeError
            if slot_get(object, &InternalSlot::Regex).is_none() {
                return Err(raise_type_error!("RegExp.prototype.exec called on incompatible receiver").into());
            }
            // Per spec: if no argument, use "undefined" string
            let input_val = if args.is_empty() {
                Value::String(utf8_to_utf16("undefined"))
            } else {
                args[0].clone()
            };
            // ToString coercion — calls toString()/valueOf() on objects
            let input_u16 = match input_val {
                Value::String(s) => s,
                Value::Number(n) => utf8_to_utf16(&crate::core::value_to_string(&Value::Number(n))),
                Value::Boolean(b) => utf8_to_utf16(&b.to_string()),
                Value::Undefined => utf8_to_utf16("undefined"),
                Value::Null => utf8_to_utf16("null"),
                Value::Object(_) => {
                    let prim = crate::core::to_primitive(mc, &input_val, "string", env)?;
                    match prim {
                        Value::String(s) => s,
                        other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                    }
                }
                _ => utf8_to_utf16(&crate::core::value_to_string(&input_val)),
            };

            // Get regex pattern and flags
            let pattern_u16 = internal_get_regex_pattern(object)?;
            let flags = match slot_get(object, &InternalSlot::Flags) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => "".to_string(),
                },
                None => "".to_string(),
            };

            let crlf = flags.contains('R');
            let global = flags.contains('g');
            let sticky = flags.contains('y');
            let has_indices = flags.contains('d');
            let use_last = global || sticky;

            // Handle CRLF normalization
            let (working_input, mapping) = if crlf {
                let mut res = Vec::with_capacity(input_u16.len());
                let mut i = 0;
                while i < input_u16.len() {
                    if input_u16[i] == '\r' as u16 && i + 1 < input_u16.len() && input_u16[i + 1] == '\n' as u16 {
                        res.push('\n' as u16);
                        i += 2;
                    } else {
                        res.push(input_u16[i]);
                        i += 1;
                    }
                }
                (res, true)
            } else {
                (input_u16.clone(), false)
            };

            // Filter flags for regress
            let mut r_flags = String::new();
            for c in flags.chars() {
                if "gimsuvy".contains(c) {
                    r_flags.push(c);
                }
            }

            let re = get_or_compile_regex(&pattern_u16, &r_flags).map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {e}")))?;
            let full_unicode = flags.contains('u') || flags.contains('v');

            // Per spec (22.2.5.2.2): Always read lastIndex via Get (observable)
            let last_index_val = crate::core::get_property_with_accessors(mc, env, object, "lastIndex").unwrap_or(Value::Number(0.0));
            // ToLength coercion (calls valueOf on objects)
            let raw_last_index = to_length_with_coercion(mc, &last_index_val, env)?;

            let last_index = if use_last {
                raw_last_index
            } else {
                // Non-global/non-sticky: always start from 0, but we still read lastIndex above (observable)
                0
            };

            // Per spec: without 'u'/'v' flag, operate on UTF-16 code units (ucs2);
            // with 'u'/'v' flag, operate on codepoints (utf16 decodes surrogate pairs).
            let match_result = if full_unicode {
                re.find_from_utf16(&working_input, last_index).next()
            } else {
                re.find_from_ucs2(&working_input, last_index).next()
            };
            // For sticky: match must start at exactly lastIndex
            let match_result = if sticky {
                match match_result {
                    Some(ref m) if m.range.start != last_index => None,
                    other => other,
                }
            } else {
                match_result
            };

            match match_result {
                Some(m) => {
                    let start = m.range.start;
                    let end = m.range.end;

                    let (orig_start, orig_end) = if mapping {
                        (map_index_back(&input_u16, start), map_index_back(&input_u16, end))
                    } else {
                        (start, end)
                    };

                    // AnnexB: Update legacy regexp static properties
                    {
                        let caps: Vec<Option<std::ops::Range<usize>>> = m
                            .captures
                            .iter()
                            .map(|c| {
                                c.as_ref().map(|r| {
                                    if mapping {
                                        map_index_back(&input_u16, r.start)..map_index_back(&input_u16, r.end)
                                    } else {
                                        r.clone()
                                    }
                                })
                            })
                            .collect();
                        update_legacy_regexp_state(&input_u16, orig_start, orig_end, &caps);
                    }

                    // Construct result array
                    let result_array = create_array(mc, env)?;

                    let full_match_u16 = input_u16[orig_start..orig_end].to_vec();
                    object_set_key_value(mc, &result_array, "0", &Value::String(full_match_u16))?;

                    let indices_array = if has_indices { Some(create_array(mc, env)?) } else { None };
                    let mut groups_obj: Option<JSObjectDataPtr<'gc>> = None;
                    let mut indices_groups_obj: Option<JSObjectDataPtr<'gc>> = None;

                    if let Some(indices) = &indices_array {
                        let match_indices = create_array(mc, env)?;
                        object_set_key_value(mc, &match_indices, "0", &Value::Number(orig_start as f64))?;
                        object_set_key_value(mc, &match_indices, "1", &Value::Number(orig_end as f64))?;
                        set_array_length(mc, &match_indices, 2)?;
                        object_set_key_value(mc, indices, "0", &Value::Object(match_indices))?;
                    }

                    let mut group_index = 1;
                    for cap in m.captures.iter() {
                        if let Some(range) = cap {
                            let (cs, ce) = if mapping {
                                (map_index_back(&input_u16, range.start), map_index_back(&input_u16, range.end))
                            } else {
                                (range.start, range.end)
                            };
                            let cap_str = input_u16[cs..ce].to_vec();
                            object_set_key_value(mc, &result_array, group_index, &Value::String(cap_str))?;

                            if let Some(indices) = &indices_array {
                                let group_indices = create_array(mc, env)?;
                                object_set_key_value(mc, &group_indices, "0", &Value::Number(cs as f64))?;
                                object_set_key_value(mc, &group_indices, "1", &Value::Number(ce as f64))?;
                                set_array_length(mc, &group_indices, 2)?;
                                object_set_key_value(mc, indices, group_index, &Value::Object(group_indices))?;
                            }
                        } else {
                            object_set_key_value(mc, &result_array, group_index, &Value::Undefined)?;
                            if let Some(indices) = &indices_array {
                                object_set_key_value(mc, indices, group_index, &Value::Undefined)?;
                            }
                        }
                        group_index += 1;
                    }

                    for (name, range_opt) in m.named_groups() {
                        let groups = groups_obj.get_or_insert_with(|| new_js_object_data(mc));
                        match range_opt {
                            Some(range) => {
                                let (cs, ce) = if mapping {
                                    (map_index_back(&input_u16, range.start), map_index_back(&input_u16, range.end))
                                } else {
                                    (range.start, range.end)
                                };
                                let cap_str = input_u16[cs..ce].to_vec();
                                object_set_key_value(mc, groups, name, &Value::String(cap_str))?;

                                if let Some(indices) = &indices_array {
                                    let indices_groups = indices_groups_obj.get_or_insert_with(|| new_js_object_data(mc));
                                    let group_indices = create_array(mc, env)?;
                                    object_set_key_value(mc, &group_indices, "0", &Value::Number(cs as f64))?;
                                    object_set_key_value(mc, &group_indices, "1", &Value::Number(ce as f64))?;
                                    set_array_length(mc, &group_indices, 2)?;
                                    object_set_key_value(mc, indices_groups, name, &Value::Object(group_indices))?;
                                    let _ = indices;
                                }
                            }
                            None => {
                                object_set_key_value(mc, groups, name, &Value::Undefined)?;
                                if let Some(indices) = &indices_array {
                                    let indices_groups = indices_groups_obj.get_or_insert_with(|| new_js_object_data(mc));
                                    object_set_key_value(mc, indices_groups, name, &Value::Undefined)?;
                                    let _ = indices;
                                }
                            }
                        }
                    }
                    set_array_length(mc, &result_array, group_index)?;

                    object_set_key_value(mc, &result_array, "index", &Value::Number(orig_start as f64))?;
                    object_set_key_value(mc, &result_array, "input", &Value::String(input_u16.clone()))?;
                    if let Some(groups) = groups_obj {
                        object_set_key_value(mc, &result_array, "groups", &Value::Object(groups))?;
                    } else {
                        object_set_key_value(mc, &result_array, "groups", &Value::Undefined)?;
                    }

                    if let Some(indices) = indices_array {
                        if let Some(indices_groups) = indices_groups_obj {
                            object_set_key_value(mc, &indices, "groups", &Value::Object(indices_groups))?;
                        } else {
                            object_set_key_value(mc, &indices, "groups", &Value::Undefined)?;
                        }
                        object_set_key_value(mc, &result_array, "indices", &Value::Object(indices))?;
                    }

                    if use_last {
                        set_last_index_checked(mc, object, orig_end as f64)?;
                    }

                    Ok(Value::Object(result_array))
                }
                None => {
                    if use_last {
                        set_last_index_checked(mc, object, 0.0)?;
                    }
                    Ok(Value::Null)
                }
            }
        }
        "test" => {
            // Step 1: If this does not have [[RegExpMatcher]], throw TypeError
            if slot_get(object, &InternalSlot::Regex).is_none() {
                return Err(raise_type_error!("RegExp.prototype.test called on incompatible receiver").into());
            }
            // Per spec: if no argument, use "undefined" string
            let input_val = if args.is_empty() {
                Value::String(utf8_to_utf16("undefined"))
            } else {
                args[0].clone()
            };
            // ToString coercion — calls toString()/valueOf() on objects
            let input_u16 = match input_val {
                Value::String(s) => s,
                Value::Number(n) => utf8_to_utf16(&crate::core::value_to_string(&Value::Number(n))),
                Value::Boolean(b) => utf8_to_utf16(&b.to_string()),
                Value::Undefined => utf8_to_utf16("undefined"),
                Value::Null => utf8_to_utf16("null"),
                Value::Object(_) => {
                    let prim = crate::core::to_primitive(mc, &input_val, "string", env)?;
                    match prim {
                        Value::String(s) => s,
                        other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                    }
                }
                _ => utf8_to_utf16(&crate::core::value_to_string(&input_val)),
            };

            let pattern_u16 = internal_get_regex_pattern(object)?;
            let flags = match slot_get(object, &InternalSlot::Flags) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => "".to_string(),
                },
                None => "".to_string(),
            };

            let crlf = flags.contains('R');
            let global = flags.contains('g');
            let sticky = flags.contains('y');
            let use_last = global || sticky;

            let (working_input, mapping) = if crlf {
                let mut res = Vec::with_capacity(input_u16.len());
                let mut i = 0;
                while i < input_u16.len() {
                    if input_u16[i] == '\r' as u16 && i + 1 < input_u16.len() && input_u16[i + 1] == '\n' as u16 {
                        res.push('\n' as u16);
                        i += 2;
                    } else {
                        res.push(input_u16[i]);
                        i += 1;
                    }
                }
                (res, true)
            } else {
                (input_u16.clone(), false)
            };

            // Filter flags for regress (same as exec)
            let mut r_flags = String::new();
            for c in flags.chars() {
                if "gimsuvy".contains(c) {
                    r_flags.push(c);
                }
            }

            let re = get_or_compile_regex(&pattern_u16, &r_flags).map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {}", e)))?;
            let full_unicode = flags.contains('u') || flags.contains('v');

            // Per spec: Always read lastIndex via Get (observable), then ToLength
            let last_index_val = crate::core::get_property_with_accessors(mc, env, object, "lastIndex").unwrap_or(Value::Number(0.0));
            let raw_last_index = to_length_with_coercion(mc, &last_index_val, env)?;
            let last_index = if use_last { raw_last_index } else { 0 };

            // Per spec: without 'u'/'v' flag, operate on UTF-16 code units (ucs2);
            // with 'u'/'v' flag, operate on codepoints (utf16 decodes surrogate pairs).
            let match_result = if full_unicode {
                re.find_from_utf16(&working_input, last_index).next()
            } else {
                re.find_from_ucs2(&working_input, last_index).next()
            };
            // For sticky: match must start at exactly lastIndex
            let match_result = if sticky {
                match match_result {
                    Some(ref m) if m.range.start != last_index => None,
                    other => other,
                }
            } else {
                match_result
            };

            match match_result {
                Some(m) => {
                    // AnnexB: Update legacy regexp static properties
                    {
                        let start = m.range.start;
                        let end = m.range.end;
                        let (orig_start, orig_end) = if mapping {
                            (map_index_back(&input_u16, start), map_index_back(&input_u16, end))
                        } else {
                            (start, end)
                        };
                        let caps: Vec<Option<std::ops::Range<usize>>> = m
                            .captures
                            .iter()
                            .map(|c| {
                                c.as_ref().map(|r| {
                                    if mapping {
                                        map_index_back(&input_u16, r.start)..map_index_back(&input_u16, r.end)
                                    } else {
                                        r.clone()
                                    }
                                })
                            })
                            .collect();
                        update_legacy_regexp_state(&input_u16, orig_start, orig_end, &caps);
                    }
                    if use_last {
                        let end = m.range.end;
                        let orig_end = if mapping { map_index_back(&input_u16, end) } else { end };
                        set_last_index_checked(mc, object, orig_end as f64)?;
                    }
                    Ok(Value::Boolean(true))
                }
                None => {
                    if use_last {
                        set_last_index_checked(mc, object, 0.0)?;
                    }
                    Ok(Value::Boolean(false))
                }
            }
        }
        "compile" => {
            // AnnexB B.2.5.1 RegExp.prototype.compile(pattern, flags)
            // Helper: create a TypeError from the current realm's TypeError constructor
            // so that cross-realm instanceof checks work correctly.
            let realm_type_error = |mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, message: &str| -> EvalError<'gc> {
                if let Some(tc_val) = crate::core::env_get(env, "TypeError")
                    && let Some(tc_obj) = match &*tc_val.borrow() {
                        Value::Object(tc) => Some(*tc),
                        Value::Property { value: Some(v), .. } => match &*v.borrow() {
                            Value::Object(tc) => Some(*tc),
                            _ => None,
                        },
                        _ => None,
                    }
                {
                    let msg_val = Value::String(utf8_to_utf16(message));
                    if let Ok(err_val) = crate::js_class::evaluate_new(mc, env, &Value::Object(tc_obj), &[msg_val], None)
                        && let Value::Object(err_obj) = err_val
                    {
                        return EvalError::Throw(Value::Object(err_obj), None, None);
                    }
                }
                raise_type_error!(message).into()
            };

            // Step 1: Validate `this` is a RegExp object
            if slot_get(object, &InternalSlot::Regex).is_none() && slot_get(object, &InternalSlot::IsRegExpPrototype).is_none() {
                return Err(realm_type_error(
                    mc,
                    env,
                    "RegExp.prototype.compile called on incompatible receiver",
                ));
            }

            // legacy-regexp spec: The receiver's [[Prototype]] chain must lead to
            // exactly the current realm's %RegExp.prototype% directly (i.e. not a
            // subclass and not a cross-realm RegExp).
            // Check: Object.getPrototypeOf(this) === %RegExp.prototype%
            {
                let this_proto = object.borrow().prototype;
                let regexp_proto_match = if let Some(regexp_ctor_val) = object_get_key_value(env, "RegExp")
                    && let Value::Object(regexp_ctor) = &*regexp_ctor_val.borrow()
                    && let Some(proto_val) = object_get_key_value(regexp_ctor, "prototype")
                    && let Value::Object(proto_obj) = &*proto_val.borrow()
                {
                    if let Some(tp) = this_proto {
                        std::ptr::eq(&*tp as *const _, &**proto_obj as *const _)
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !regexp_proto_match {
                    return Err(realm_type_error(
                        mc,
                        env,
                        "RegExp.prototype.compile called on incompatible receiver",
                    ));
                }
            }

            let pattern_arg = args.first().cloned().unwrap_or(Value::Undefined);
            let flags_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

            let (new_pattern, new_flags) = if let Value::Object(p_obj) = &pattern_arg
                && slot_get(p_obj, &InternalSlot::Regex).is_some()
            {
                // If pattern is a RegExp and flags is not undefined, throw TypeError
                if !matches!(flags_arg, Value::Undefined) {
                    return Err(raise_type_error!("Cannot supply flags when constructing one RegExp from another").into());
                }
                // Read directly from [[OriginalSource]] and [[OriginalFlags]] internal slots
                let src = match slot_get(p_obj, &InternalSlot::Regex) {
                    Some(v) => match &*v.borrow() {
                        Value::String(s) => utf16_to_utf8(s),
                        _ => String::new(),
                    },
                    None => String::new(),
                };
                let flg = internal_get_flags_string(p_obj);
                (src, flg)
            } else {
                let p = if matches!(pattern_arg, Value::Undefined) {
                    String::new()
                } else {
                    let p_s = crate::js_string::spec_to_string(mc, &pattern_arg, env)?;
                    utf16_to_utf8(&p_s)
                };
                let f = if matches!(flags_arg, Value::Undefined) {
                    String::new()
                } else {
                    let f_s = crate::js_string::spec_to_string(mc, &flags_arg, env)?;
                    utf16_to_utf8(&f_s)
                };
                (p, f)
            };

            // Validate flags
            let mut seen = std::collections::HashSet::new();
            for c in new_flags.chars() {
                if !"dgimsuy".contains(c) {
                    return Err(raise_syntax_error!(format!("invalid flags: {}", new_flags)).into());
                }
                if !seen.insert(c) {
                    return Err(raise_syntax_error!(format!("invalid flags: {}", new_flags)).into());
                }
            }

            // Try to compile the new pattern
            let mut r_flags = String::new();
            for c in new_flags.chars() {
                if "gimsuvy".contains(c) {
                    r_flags.push(c);
                }
            }
            let pattern_u16 = utf8_to_utf16(&new_pattern);
            let _ = get_or_compile_regex(&pattern_u16, &r_flags).map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {}", e)))?;

            // Update the RegExp internal slots
            slot_set(mc, object, InternalSlot::Regex, &Value::String(pattern_u16));
            slot_set(mc, object, InternalSlot::Flags, &Value::String(utf8_to_utf16(&new_flags)));
            // Update individual flag boolean slots
            slot_set(mc, object, InternalSlot::RegexGlobal, &Value::Boolean(new_flags.contains('g')));
            slot_set(mc, object, InternalSlot::RegexIgnoreCase, &Value::Boolean(new_flags.contains('i')));
            slot_set(mc, object, InternalSlot::RegexMultiline, &Value::Boolean(new_flags.contains('m')));
            slot_set(mc, object, InternalSlot::RegexDotAll, &Value::Boolean(new_flags.contains('s')));
            slot_set(mc, object, InternalSlot::RegexUnicode, &Value::Boolean(new_flags.contains('u')));
            slot_set(mc, object, InternalSlot::RegexSticky, &Value::Boolean(new_flags.contains('y')));
            slot_set(mc, object, InternalSlot::RegexHasIndices, &Value::Boolean(new_flags.contains('d')));
            slot_set(mc, object, InternalSlot::RegexUnicodeSets, &Value::Boolean(new_flags.contains('v')));
            // Reset lastIndex to 0 (throws TypeError if non-writable per spec §21.2.3.2.2 step 12)
            set_last_index_checked(mc, object, 0.0)?;

            Ok(Value::Object(*object))
        }
        "toString" => {
            // §22.2.5.14 RegExp.prototype.toString()
            // Step 1: Let R = this value (object).
            // Step 2: Let pattern = ? ToString(? Get(R, "source"))
            let source_val = crate::core::get_property_with_accessors(mc, env, object, "source")?;
            let pattern = utf16_to_utf8(&crate::js_string::spec_to_string(mc, &source_val, env)?);

            // Step 3: Let flags = ? ToString(? Get(R, "flags"))
            let flags_val = crate::core::get_property_with_accessors(mc, env, object, "flags")?;
            let flags = utf16_to_utf8(&crate::js_string::spec_to_string(mc, &flags_val, env)?);

            let result = format!("/{}/{}", pattern, flags);
            Ok(Value::String(utf8_to_utf16(&result)))
        }
        "match" => {
            // §22.2.5.6 RegExp.prototype[@@match](string)
            // Step 3: Let S = ? ToString(string)
            let input_str = if args.is_empty() {
                utf8_to_utf16("undefined")
            } else {
                match &args[0] {
                    Value::String(s) => s.clone(),
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &args[0], "string", env)?;
                        match prim {
                            Value::String(s) => s,
                            other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                        }
                    }
                    other => utf8_to_utf16(&crate::core::value_to_string(other)),
                }
            };

            // Step 4: Let flags = ? ToString(? Get(rx, "flags"))
            let flags_val = crate::core::get_property_with_accessors(mc, env, object, "flags")?;
            let flags_str = match &flags_val {
                Value::String(s) => utf16_to_utf8(s),
                Value::Object(_) => {
                    let prim = crate::core::to_primitive(mc, &flags_val, "string", env)?;
                    match prim {
                        Value::String(s) => utf16_to_utf8(&s),
                        other => crate::core::value_to_string(&other),
                    }
                }
                other => crate::core::value_to_string(other),
            };
            let global = flags_str.contains('g');

            if !global {
                // Step 5: Non-global: just call exec and return its result
                return regexp_exec_abstract(mc, object, &Value::String(input_str), env);
            }

            // Step 6: Global match
            let full_unicode = flags_str.contains('u') || flags_str.contains('v');

            // Step 6b: Set lastIndex = 0
            set_last_index_checked(mc, object, 0.0)?;

            let result_array = create_array(mc, env)?;
            let mut n = 0usize;

            loop {
                // Step 6e.i: Let result = ? RegExpExec(rx, S)
                let exec_result = regexp_exec_abstract(mc, object, &Value::String(input_str.clone()), env)?;
                if matches!(exec_result, Value::Null) {
                    if n == 0 {
                        return Ok(Value::Null);
                    }
                    set_array_length(mc, &result_array, n)?;
                    return Ok(Value::Object(result_array));
                }

                // Step 6e.iii: Let matchStr = ? ToString(? Get(result, "0"))
                let match_str_val = if let Value::Object(res_obj) = &exec_result {
                    crate::core::get_property_with_accessors(mc, env, res_obj, "0")?
                } else {
                    Value::Undefined
                };
                let match_str = match &match_str_val {
                    Value::String(s) => s.clone(),
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &match_str_val, "string", env)?;
                        match prim {
                            Value::String(s) => s,
                            other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                        }
                    }
                    other => utf8_to_utf16(&crate::core::value_to_string(other)),
                };

                // Step 6e.iv
                object_set_key_value(mc, &result_array, n, &Value::String(match_str.clone()))?;

                // Step 6e.v: If matchStr is empty, advance lastIndex
                if match_str.is_empty() {
                    let this_index_val = crate::core::get_property_with_accessors(mc, env, object, "lastIndex")?;
                    let this_index = to_length_with_coercion(mc, &this_index_val, env)?;
                    let next_index = if full_unicode {
                        advance_string_index_unicode(&input_str, this_index)
                    } else {
                        this_index + 1
                    };
                    set_last_index_checked(mc, object, next_index as f64)?;
                }

                n += 1;
                if n > 1_000_000 {
                    break;
                }
            }
            set_array_length(mc, &result_array, n)?;
            Ok(Value::Object(result_array))
        }
        "replace" => {
            // §22.2.5.8 RegExp.prototype[@@replace](string, replaceValue)
            // Step 3: Let string = ? ToString(string)
            let input_str = if args.is_empty() {
                utf8_to_utf16("undefined")
            } else {
                match &args[0] {
                    Value::String(s) => s.clone(),
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &args[0], "string", env)?;
                        match prim {
                            Value::String(s) => s,
                            other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                        }
                    }
                    other => utf8_to_utf16(&crate::core::value_to_string(other)),
                }
            };

            let replace_value = args.get(1).cloned().unwrap_or(Value::Undefined);

            // Step 5: Let flags = ? ToString(? Get(rx, "flags"))
            let flags_val = crate::core::get_property_with_accessors(mc, env, object, "flags")?;
            let flags_str = match &flags_val {
                Value::String(s) => utf16_to_utf8(s),
                Value::Object(_) => {
                    let prim = crate::core::to_primitive(mc, &flags_val, "string", env)?;
                    match prim {
                        Value::String(s) => utf16_to_utf8(&s),
                        other => crate::core::value_to_string(&other),
                    }
                }
                other => crate::core::value_to_string(other),
            };
            let global = flags_str.contains('g');
            let full_unicode = flags_str.contains('u') || flags_str.contains('v');

            if global {
                set_last_index_checked(mc, object, 0.0)?;
            }

            // Collect all match results
            let mut results: Vec<Value<'gc>> = Vec::new();
            loop {
                let exec_result = regexp_exec_abstract(mc, object, &Value::String(input_str.clone()), env)?;
                if matches!(exec_result, Value::Null) {
                    break;
                }
                results.push(exec_result.clone());

                if !global {
                    break;
                }

                // Advance lastIndex if empty match
                if let Value::Object(res_obj) = &exec_result {
                    let match_val = crate::core::get_property_with_accessors(mc, env, res_obj, "0").unwrap_or(Value::Undefined);
                    let match_str = match &match_val {
                        Value::String(s) => s.clone(),
                        Value::Object(_) => {
                            let prim = crate::core::to_primitive(mc, &match_val, "string", env)?;
                            match prim {
                                Value::String(s) => s,
                                other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                            }
                        }
                        other => utf8_to_utf16(&crate::core::value_to_string(other)),
                    };

                    if match_str.is_empty() {
                        let this_index_val =
                            crate::core::get_property_with_accessors(mc, env, object, "lastIndex").unwrap_or(Value::Number(0.0));
                        let this_index = to_length_with_coercion(mc, &this_index_val, env)?;
                        let next_index = if full_unicode {
                            advance_string_index_unicode(&input_str, this_index)
                        } else {
                            this_index + 1
                        };
                        set_last_index_checked(mc, object, next_index as f64)?;
                    }
                }

                if results.len() > 1_000_000 {
                    break;
                }
            }

            // Check if replaceValue is callable
            let replace_is_fn = matches!(&replace_value, Value::Object(o) if {
                let b = o.borrow();
                b.get_closure().is_some() ||
                crate::core::slot_get(o, &InternalSlot::Callable).is_some() ||
                crate::core::slot_get(o, &InternalSlot::Function).is_some()
            }) || matches!(&replace_value, Value::Function(_) | Value::Closure(_));

            // If not callable, coerce replaceValue to string
            let replace_str_val = if !replace_is_fn {
                match &replace_value {
                    Value::String(s) => s.clone(),
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &replace_value, "string", env)?;
                        match prim {
                            Value::String(s) => s,
                            other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                        }
                    }
                    other => utf8_to_utf16(&crate::core::value_to_string(other)),
                }
            } else {
                Vec::new()
            };

            // Build result string
            let mut acc_next_source_position = 0usize;
            let mut accumulated = Vec::<u16>::new();

            for result in &results {
                let result_obj = match result {
                    Value::Object(o) => o,
                    _ => continue,
                };

                // Step 14a: Let nCaptures = ? ToLength(? Get(result, "length"))
                let n_captures_val = crate::core::get_property_with_accessors(mc, env, result_obj, "length")?;
                let n_captures_raw = to_length_with_coercion(mc, &n_captures_val, env)?;
                let n_captures = if n_captures_raw > 0 { n_captures_raw - 1 } else { 0 };

                // Step 14c: Let matched = ? ToString(? Get(result, "0"))
                let matched_val = crate::core::get_property_with_accessors(mc, env, result_obj, "0")?;
                let matched = match &matched_val {
                    Value::String(s) => s.clone(),
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &matched_val, "string", env)?;
                        match prim {
                            Value::String(s) => s,
                            other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                        }
                    }
                    other => utf8_to_utf16(&crate::core::value_to_string(other)),
                };

                // Step 14d: Let position = ? ToIntegerOrInfinity(? Get(result, "index"))
                let position_val = crate::core::get_property_with_accessors(mc, env, result_obj, "index")?;
                let position = match &position_val {
                    Value::Number(n) => (*n as isize).max(0) as usize,
                    Value::Undefined => 0,
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &position_val, "number", env)?;
                        match prim {
                            Value::Number(n) => (n as isize).max(0) as usize,
                            _ => 0,
                        }
                    }
                    _ => {
                        let s = crate::core::value_to_string(&position_val);
                        s.parse::<f64>().unwrap_or(0.0).max(0.0) as usize
                    }
                };
                let position = position.min(input_str.len());

                // Step 14e-g: Collect captures
                let mut captures: Vec<Value<'gc>> = Vec::new();
                for i in 1..=n_captures {
                    let cap_val = crate::core::get_property_with_accessors(mc, env, result_obj, i)?;
                    if matches!(cap_val, Value::Undefined) {
                        captures.push(Value::Undefined);
                    } else {
                        let cap_str = match &cap_val {
                            Value::String(s) => Value::String(s.clone()),
                            Value::Object(_) => {
                                let prim = crate::core::to_primitive(mc, &cap_val, "string", env)?;
                                match prim {
                                    Value::String(s) => Value::String(s),
                                    other => Value::String(utf8_to_utf16(&crate::core::value_to_string(&other))),
                                }
                            }
                            other => Value::String(utf8_to_utf16(&crate::core::value_to_string(other))),
                        };
                        captures.push(cap_str);
                    }
                }

                // Step 14h: Get named captures
                let named_captures_val = crate::core::get_property_with_accessors(mc, env, result_obj, "groups")?;
                let named_captures = if matches!(named_captures_val, Value::Undefined) {
                    None
                } else {
                    // Step 14l.i: If namedCaptures is not undefined, Set namedCaptures to ? ToObject(namedCaptures).
                    // ToObject(null) throws TypeError
                    if !replace_is_fn {
                        match &named_captures_val {
                            Value::Null => {
                                return Err(raise_type_error!("Cannot convert null to object").into());
                            }
                            Value::Object(_) => Some(named_captures_val),
                            Value::String(s) => {
                                // ToObject wraps string in a String object
                                let str_obj = new_js_object_data(mc);
                                let s_clone = s.clone();
                                object_set_key_value(mc, &str_obj, "length", &Value::Number(s_clone.len() as f64))?;
                                for (idx, &ch) in s_clone.iter().enumerate() {
                                    object_set_key_value(mc, &str_obj, idx, &Value::String(vec![ch]))?;
                                }
                                Some(Value::Object(str_obj))
                            }
                            Value::Number(_) | Value::Boolean(_) => {
                                // ToObject wraps in Number/Boolean object — no useful properties for GetSubstitution
                                let wrap_obj = new_js_object_data(mc);
                                Some(Value::Object(wrap_obj))
                            }
                            _ => Some(named_captures_val),
                        }
                    } else {
                        Some(named_captures_val)
                    }
                };

                let replacement: Vec<u16> = if replace_is_fn {
                    // Call replaceValue function
                    let mut call_args = vec![Value::String(matched.clone())];
                    for cap in &captures {
                        call_args.push(cap.clone());
                    }
                    call_args.push(Value::Number(position as f64));
                    call_args.push(Value::String(input_str.clone()));
                    if let Some(nc) = &named_captures {
                        call_args.push(nc.clone());
                    }
                    let rep_val = crate::js_promise::call_function(mc, &replace_value, &call_args, env)?;
                    match rep_val {
                        Value::String(s) => s,
                        Value::Object(_) => {
                            let prim = crate::core::to_primitive(mc, &rep_val, "string", env)?;
                            match prim {
                                Value::String(s) => s,
                                other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                            }
                        }
                        other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                    }
                } else {
                    // GetSubstitution
                    get_substitution(
                        &matched,
                        &input_str,
                        position,
                        &captures,
                        &named_captures,
                        &replace_str_val,
                        mc,
                        env,
                    )?
                };

                // Append non-matched portion + replacement
                if position >= acc_next_source_position {
                    accumulated.extend_from_slice(&input_str[acc_next_source_position..position]);
                    accumulated.extend_from_slice(&replacement);
                    acc_next_source_position = position + matched.len();
                }
            }

            // Append remaining portion of input
            if acc_next_source_position < input_str.len() {
                accumulated.extend_from_slice(&input_str[acc_next_source_position..]);
            }

            Ok(Value::String(accumulated))
        }
        "search" => {
            // §22.2.5.11 RegExp.prototype[@@search](string)
            let input_str = if args.is_empty() {
                utf8_to_utf16("undefined")
            } else {
                crate::js_string::spec_to_string(mc, &args[0], env)?
            };

            // Step 4: let previousLastIndex = ? Get(rx, "lastIndex")
            let previous_last_index = crate::core::get_property_with_accessors(mc, env, object, "lastIndex")?;

            // Step 5: if SameValue(previousLastIndex, +0) is false, Set(rx, "lastIndex", +0, true)
            if !same_value(&previous_last_index, &Value::Number(0.0)) {
                crate::core::set_property_with_accessors(mc, env, object, "lastIndex", &Value::Number(0.0), None)?;
            }

            // Step 6: let result = ? RegExpExec(rx, S)
            let exec_result = regexp_exec_abstract(mc, object, &Value::String(input_str), env)?;

            // Step 7: let currentLastIndex = ? Get(rx, "lastIndex")
            let current_last_index = crate::core::get_property_with_accessors(mc, env, object, "lastIndex")?;

            // Step 8: if SameValue(currentLastIndex, previousLastIndex) is false, Set(rx, "lastIndex", previousLastIndex, true)
            if !same_value(&current_last_index, &previous_last_index) {
                crate::core::set_property_with_accessors(mc, env, object, "lastIndex", &previous_last_index, None)?;
            }

            if matches!(exec_result, Value::Null) {
                Ok(Value::Number(-1.0))
            } else if let Value::Object(res_obj) = &exec_result {
                // Step 10: Return ? Get(result, "index")
                Ok(crate::core::get_property_with_accessors(mc, env, res_obj, "index")?)
            } else {
                Ok(Value::Number(-1.0))
            }
        }
        "split" => {
            // §22.2.5.13 RegExp.prototype[@@split](string, limit)
            // Step 3: Let S = ? ToString(string)
            let input_str = if args.is_empty() {
                utf8_to_utf16("undefined")
            } else {
                crate::js_string::spec_to_string(mc, &args[0], env)?
            };

            // Step 5: Let C = ? SpeciesConstructor(rx, %RegExp%)
            let species_ctor = species_constructor(mc, env, object)?;

            // Step 6: Let flags = ? ToString(? Get(rx, "flags"))
            let flags_val = crate::core::get_property_with_accessors(mc, env, object, "flags")?;
            let flags_str = crate::js_string::spec_to_string(mc, &flags_val, env)?;
            let flags_str_utf8 = utf16_to_utf8(&flags_str);

            // Step 7: unicodeMatching
            let full_unicode = flags_str_utf8.contains('u') || flags_str_utf8.contains('v');

            // Step 8: newFlags = flags with "y" added
            let new_flags = if flags_str_utf8.contains('y') {
                flags_str_utf8.clone()
            } else {
                format!("{}y", flags_str_utf8)
            };

            // Step 9: Let splitter = ? Construct(C, [rx, newFlags])
            let splitter = if let Some(ctor) = species_ctor {
                let ctor_args = vec![Value::Object(*object), Value::String(utf8_to_utf16(&new_flags))];
                let v = crate::js_class::evaluate_new(mc, env, &ctor, &ctor_args, None)?;
                match v {
                    Value::Object(o) => o,
                    _ => return Err(raise_type_error!("[Symbol.split]: species constructor did not return an object").into()),
                }
            } else {
                // Default: construct a new RegExp
                let ctor_args = vec![Value::Object(*object), Value::String(utf8_to_utf16(&new_flags))];
                let v = handle_regexp_constructor_with_env(mc, Some(env), &ctor_args)?;
                match v {
                    Value::Object(o) => o,
                    _ => return Err(raise_type_error!("[Symbol.split]: failed to construct splitter RegExp").into()),
                }
            };

            // Step 10: Let A = ! ArrayCreate(0)
            let result_array = create_array(mc, env)?;

            // Step 11-12: lengthA = 0
            let mut arr_len = 0usize;

            // Step 13: Let lim = limit === undefined ? 2^32-1 : ToUint32(limit)
            let limit = if let Some(lim) = args.get(1) {
                match lim {
                    Value::Undefined => u32::MAX,
                    _ => crate::core::to_uint32_value_with_env(mc, env, lim)?,
                }
            } else {
                u32::MAX
            };

            if limit == 0 {
                set_array_length(mc, &result_array, 0)?;
                return Ok(Value::Object(result_array));
            }

            let size = input_str.len();

            // Step 16: If size = 0
            if size == 0 {
                let z = regexp_exec_abstract(mc, &splitter, &Value::String(input_str.clone()), env)?;
                if matches!(z, Value::Null) {
                    object_set_key_value(mc, &result_array, 0usize, &Value::String(input_str))?;
                    set_array_length(mc, &result_array, 1)?;
                }
                return Ok(Value::Object(result_array));
            }

            // Step 17: Let p = 0
            let mut p = 0usize;
            // Step 18: Let q = p
            let mut q = p;

            // Step 19: Repeat, while q < size
            while q < size {
                // Step 19.a: Perform ? Set(splitter, "lastIndex", q, true)
                crate::core::set_property_with_accessors(mc, env, &splitter, "lastIndex", &Value::Number(q as f64), None)?;

                // Step 19.b-c: Let z = ? RegExpExec(splitter, S)
                let z = regexp_exec_abstract(mc, &splitter, &Value::String(input_str.clone()), env)?;

                // Step 19.d: If z is null
                if matches!(z, Value::Null) {
                    q = if full_unicode {
                        advance_string_index_unicode(&input_str, q)
                    } else {
                        q + 1
                    };
                    continue;
                }

                // Step 19.e: z is not null
                let z_obj = match &z {
                    Value::Object(o) => *o,
                    _ => return Err(raise_type_error!("[Symbol.split]: exec result is not an object").into()),
                };

                // Step 19.e.i: Let e = ? ToLength(? Get(splitter, "lastIndex"))
                let e_val = crate::core::get_property_with_accessors(mc, env, &splitter, "lastIndex")?;
                let e_raw = to_length_with_coercion(mc, &e_val, env)?;
                let e = e_raw.min(size);

                // Step 19.e.ii: If e = p, advance q
                if e == p {
                    q = if full_unicode {
                        advance_string_index_unicode(&input_str, q)
                    } else {
                        q + 1
                    };
                    continue;
                }

                // Step 19.e.iii: e ≠ p
                // Add T = S[p..q]
                let sub = input_str[p..q].to_vec();
                object_set_key_value(mc, &result_array, arr_len, &Value::String(sub))?;
                arr_len += 1;
                if arr_len as u32 >= limit {
                    set_array_length(mc, &result_array, arr_len)?;
                    return Ok(Value::Object(result_array));
                }

                // Step 19.e.iii.7: Let p = e
                p = e;

                // Step 19.e.iii.8: Let numberOfCaptures = ? ToLength(? Get(z, "length"))
                let n_cap_val = crate::core::get_property_with_accessors(mc, env, &z_obj, "length")?;
                let number_of_captures = to_length_with_coercion(mc, &n_cap_val, env)?;
                let number_of_captures = if number_of_captures > 0 { number_of_captures - 1 } else { 0 };

                // Step 19.e.iii.9-12: Add captures
                for i in 1..=number_of_captures {
                    let cap_val = crate::core::get_property_with_accessors(mc, env, &z_obj, i)?;
                    object_set_key_value(mc, &result_array, arr_len, &cap_val)?;
                    arr_len += 1;
                    if arr_len as u32 >= limit {
                        set_array_length(mc, &result_array, arr_len)?;
                        return Ok(Value::Object(result_array));
                    }
                }

                // Step 19.e.iii.13: Let q = p
                q = p;
            }

            // Step 20: Add tail T = S[p..size]
            let sub = input_str[p..size].to_vec();
            object_set_key_value(mc, &result_array, arr_len, &Value::String(sub))?;
            arr_len += 1;
            set_array_length(mc, &result_array, arr_len)?;
            Ok(Value::Object(result_array))
        }
        "matchAll" => {
            // §22.2.5.9 RegExp.prototype[@@matchAll](string)

            // Step 3: Let S = ? ToString(string)
            let input_str = if args.is_empty() {
                utf8_to_utf16("undefined")
            } else {
                crate::js_string::spec_to_string(mc, &args[0], env)?
            };

            // Step 4: Let C = ? SpeciesConstructor(R, %RegExp%)
            let species_ctor = species_constructor(mc, env, object)?;

            // Step 5: Let flags = ? ToString(? Get(R, "flags"))
            let flags_val = crate::core::get_property_with_accessors(mc, env, object, "flags")?;
            let flags_str = utf16_to_utf8(&crate::js_string::spec_to_string(mc, &flags_val, env)?);

            // Step 6: Let matcher = ? Construct(C, [R, flags])
            let matcher_obj = if let Some(ctor) = species_ctor {
                let ctor_args = vec![Value::Object(*object), Value::String(utf8_to_utf16(&flags_str))];
                let v = crate::js_class::evaluate_new(mc, env, &ctor, &ctor_args, None)?;
                match v {
                    Value::Object(o) => o,
                    _ => return Err(raise_type_error!("[Symbol.matchAll]: species constructor did not return an object").into()),
                }
            } else {
                // Default: construct a new RegExp
                let ctor_args = vec![Value::Object(*object), Value::String(utf8_to_utf16(&flags_str))];
                let v = handle_regexp_constructor_with_env(mc, Some(env), &ctor_args)?;
                match v {
                    Value::Object(o) => o,
                    _ => return Err(raise_type_error!("[Symbol.matchAll]: failed to construct matcher RegExp").into()),
                }
            };

            // Step 7-8: Let lastIndex = ? ToLength(? Get(R, "lastIndex"))
            let last_index_val = crate::core::get_property_with_accessors(mc, env, object, "lastIndex")?;
            let last_index = to_length_with_coercion(mc, &last_index_val, env)?;
            set_last_index_checked(mc, &matcher_obj, last_index as f64)?;

            // Step 9-10: Determine global and fullUnicode
            let global = flags_str.contains('g');
            let full_unicode = flags_str.contains('u') || flags_str.contains('v');

            // Step 11: Return CreateRegExpStringIterator(matcher, S, global, fullUnicode)
            create_regexp_string_iterator(mc, env, matcher_obj, input_str, global, full_unicode)
        }
        _ => Err(raise_eval_error!(format!("RegExp.prototype.{method} is not implemented")).into()),
    }
}

/// Advance string index by one code point (surrogate-aware for Unicode mode).
fn advance_string_index_unicode(s: &[u16], index: usize) -> usize {
    if index + 1 >= s.len() {
        return index + 1;
    }
    let first = s[index];
    if (0xD800..=0xDBFF).contains(&first) {
        let second = s[index + 1];
        if (0xDC00..=0xDFFF).contains(&second) {
            return index + 2;
        }
    }
    index + 1
}

// ────────────────────────────────────────────────────────────────────────────
// §22.2.7  RegExp String Iterator Objects
// ────────────────────────────────────────────────────────────────────────────

/// Create %RegExpStringIteratorPrototype%. Must be called after %IteratorPrototype% exists.
pub fn initialize_regexp_string_iterator_prototype<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), crate::error::JSError> {
    use crate::core::{PropertyKey, slot_get_chained};

    let proto = new_js_object_data(mc);

    // [[Prototype]] = %IteratorPrototype%
    if let Some(iter_proto_val) = slot_get_chained(env, &InternalSlot::IteratorPrototype)
        && let Value::Object(iter_proto) = &*iter_proto_val.borrow()
    {
        proto.borrow_mut(mc).prototype = Some(*iter_proto);
    }

    // next method – non-enumerable
    object_set_key_value(
        mc,
        &proto,
        "next",
        &Value::Function("RegExpStringIterator.prototype.next".to_string()),
    )?;
    proto.borrow_mut(mc).set_non_enumerable("next");

    // @@toStringTag = "RegExp String Iterator" (non-writable, non-enumerable, configurable)
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let tag_desc =
            crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("RegExp String Iterator")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &proto, PropertyKey::Symbol(*tag_sym), &tag_desc)?;
    }

    slot_set(mc, env, InternalSlot::RegExpStringIteratorPrototype, &Value::Object(proto));

    Ok(())
}

/// §22.2.7.1 CreateRegExpStringIterator(R, S, global, fullUnicode)
fn create_regexp_string_iterator<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    matcher: JSObjectDataPtr<'gc>,
    string: Vec<u16>,
    global: bool,
    full_unicode: bool,
) -> Result<Value<'gc>, EvalError<'gc>> {
    use crate::core::slot_get_chained;

    let iterator = new_js_object_data(mc);

    // [[Prototype]] = %RegExpStringIteratorPrototype%
    if let Some(proto_val) = slot_get_chained(env, &InternalSlot::RegExpStringIteratorPrototype)
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        iterator.borrow_mut(mc).prototype = Some(*proto);
    }

    // Internal slots
    slot_set(mc, &iterator, InternalSlot::RegExpIteratorMatcher, &Value::Object(matcher));
    slot_set(mc, &iterator, InternalSlot::RegExpIteratorString, &Value::String(string));
    slot_set(mc, &iterator, InternalSlot::RegExpIteratorGlobal, &Value::Boolean(global));
    slot_set(mc, &iterator, InternalSlot::RegExpIteratorUnicode, &Value::Boolean(full_unicode));
    slot_set(mc, &iterator, InternalSlot::RegExpIteratorDone, &Value::Boolean(false));

    Ok(Value::Object(iterator))
}

/// §22.2.7.2.1 %RegExpStringIterator%.prototype.next()
pub(crate) fn handle_regexp_string_iterator_next<'gc>(
    mc: &MutationContext<'gc>,
    iterator: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // 1. If O.[[Done]] is true, return CreateIterResultObject(undefined, true)
    let done = match slot_get(iterator, &InternalSlot::RegExpIteratorDone) {
        Some(v) => matches!(&*v.borrow(), Value::Boolean(true)),
        None => false,
    };
    if done {
        return create_iter_result(mc, Value::Undefined, true);
    }

    // 2. Read internal slots
    let matcher = match slot_get(iterator, &InternalSlot::RegExpIteratorMatcher) {
        Some(v) => match &*v.borrow() {
            Value::Object(o) => *o,
            _ => return Err(raise_type_error!("RegExpStringIterator: matcher is not an object").into()),
        },
        None => return Err(raise_type_error!("RegExpStringIterator: missing matcher slot").into()),
    };

    let s = match slot_get(iterator, &InternalSlot::RegExpIteratorString) {
        Some(v) => match &*v.borrow() {
            Value::String(s) => s.clone(),
            _ => return Err(raise_type_error!("RegExpStringIterator: string is not a string").into()),
        },
        None => return Err(raise_type_error!("RegExpStringIterator: missing string slot").into()),
    };

    let global = match slot_get(iterator, &InternalSlot::RegExpIteratorGlobal) {
        Some(v) => matches!(&*v.borrow(), Value::Boolean(true)),
        None => false,
    };

    let full_unicode = match slot_get(iterator, &InternalSlot::RegExpIteratorUnicode) {
        Some(v) => matches!(&*v.borrow(), Value::Boolean(true)),
        None => false,
    };

    // 3. Let match = ? RegExpExec(R, S)
    let match_result = regexp_exec_abstract(mc, &matcher, &Value::String(s.clone()), env)?;

    // 4. If match is null:
    if matches!(match_result, Value::Null) {
        // 4.a. Set O.[[Done]] to true
        slot_set(mc, iterator, InternalSlot::RegExpIteratorDone, &Value::Boolean(true));
        // 4.b. Return CreateIterResultObject(undefined, true)
        return create_iter_result(mc, Value::Undefined, true);
    }

    // 5. match is not null
    if global {
        // 5.a. global is true
        // 5.a.i. Let matchStr = ? ToString(? Get(match, "0"))
        let match_val = if let Value::Object(match_obj) = &match_result {
            crate::core::get_property_with_accessors(mc, env, match_obj, "0")?
        } else {
            Value::Undefined
        };
        let match_str = crate::js_string::spec_to_string(mc, &match_val, env)?;

        // 5.a.ii. If matchStr is the empty String, advance lastIndex
        if match_str.is_empty()
            && let Value::Object(match_obj) = &match_result
        {
            let this_index_val = crate::core::get_property_with_accessors(mc, env, &matcher, "lastIndex")?;
            let this_index = to_length_with_coercion(mc, &this_index_val, env)?;
            let next_index = if full_unicode {
                advance_string_index_unicode(&s, this_index)
            } else {
                this_index + 1
            };
            set_last_index_checked(mc, &matcher, next_index as f64)?;
            // Still yield this match
            let _ = match_obj; // suppress unused warning
        }

        // 5.a.iii. Return CreateIterResultObject(match, false)
        create_iter_result(mc, match_result, false)
    } else {
        // 5.b. global is false
        // 5.b.i. Set O.[[Done]] to true
        slot_set(mc, iterator, InternalSlot::RegExpIteratorDone, &Value::Boolean(true));
        // 5.b.ii. Return CreateIterResultObject(match, false)
        create_iter_result(mc, match_result, false)
    }
}

/// Create a {value, done} iterator result object.
fn create_iter_result<'gc>(mc: &MutationContext<'gc>, value: Value<'gc>, done: bool) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = new_js_object_data(mc);
    object_set_key_value(mc, &obj, "value", &value)?;
    object_set_key_value(mc, &obj, "done", &Value::Boolean(done))?;
    Ok(Value::Object(obj))
}

/// §21.1.3.17.1 GetSubstitution(matched, str, position, captures, namedCaptures, replacementTemplate)
#[allow(clippy::too_many_arguments)]
fn get_substitution<'gc>(
    matched: &[u16],
    string: &[u16],
    position: usize,
    captures: &[Value<'gc>],
    named_captures: &Option<Value<'gc>>,
    replacement: &[u16],
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Vec<u16>, EvalError<'gc>> {
    let mut result = Vec::new();
    let tail_pos = (position + matched.len()).min(string.len());
    let m = captures.len();
    let mut i = 0;

    while i < replacement.len() {
        let ch = replacement[i];
        if ch == '$' as u16 && i + 1 < replacement.len() {
            let next = replacement[i + 1];
            match next {
                // $$ → literal $
                c if c == '$' as u16 => {
                    result.push('$' as u16);
                    i += 2;
                }
                // $& → matched substring
                c if c == '&' as u16 => {
                    result.extend_from_slice(matched);
                    i += 2;
                }
                // $` → portion before match
                c if c == '`' as u16 => {
                    if position > 0 {
                        result.extend_from_slice(&string[..position]);
                    }
                    i += 2;
                }
                // $' → portion after match
                c if c == '\'' as u16 => {
                    if tail_pos < string.len() {
                        result.extend_from_slice(&string[tail_pos..]);
                    }
                    i += 2;
                }
                // $<name> → named capture
                c if c == '<' as u16 => {
                    if named_captures.is_none() {
                        // namedCaptures is undefined → literal $<
                        result.push('$' as u16);
                        result.push('<' as u16);
                        i += 2;
                    } else if let Some(gt_pos) = replacement[i + 2..].iter().position(|&u| u == '>' as u16) {
                        let name_u16 = &replacement[i + 2..i + 2 + gt_pos];
                        let name = utf16_to_utf8(name_u16);
                        if let Some(Value::Object(groups)) = named_captures {
                            let capture_val = crate::core::get_property_with_accessors(mc, env, groups, &*name)?;
                            match &capture_val {
                                Value::Undefined => {} // empty replacement
                                Value::String(s) => result.extend_from_slice(s),
                                Value::Object(_) => {
                                    let prim = crate::core::to_primitive(mc, &capture_val, "string", env)?;
                                    match prim {
                                        Value::String(s) => result.extend_from_slice(&s),
                                        other => {
                                            let s = crate::core::value_to_string(&other);
                                            result.extend_from_slice(&utf8_to_utf16(&s));
                                        }
                                    }
                                }
                                other => {
                                    let s = crate::core::value_to_string(other);
                                    result.extend_from_slice(&utf8_to_utf16(&s));
                                }
                            }
                        }
                        i += 2 + gt_pos + 1;
                    } else {
                        // No matching > found
                        result.push(ch);
                        i += 1;
                    }
                }
                // $n or $nn → numbered capture
                c if (c as u8).is_ascii_digit() => {
                    let d1 = (c as u8 - b'0') as usize;
                    // Check for two-digit reference $nn
                    if i + 2 < replacement.len() {
                        let next2 = replacement[i + 2];
                        if (next2 as u8).is_ascii_digit() {
                            let d2 = (next2 as u8 - b'0') as usize;
                            let nn = d1 * 10 + d2;
                            if nn >= 1 && nn <= m {
                                match &captures[nn - 1] {
                                    Value::String(s) => result.extend_from_slice(s),
                                    Value::Undefined => {}
                                    other => {
                                        let s = crate::core::value_to_string(other);
                                        result.extend_from_slice(&utf8_to_utf16(&s));
                                    }
                                }
                                i += 3;
                                continue;
                            }
                        }
                    }
                    // Single digit
                    if d1 >= 1 && d1 <= m {
                        match &captures[d1 - 1] {
                            Value::String(s) => result.extend_from_slice(s),
                            Value::Undefined => {}
                            other => {
                                let s = crate::core::value_to_string(other);
                                result.extend_from_slice(&utf8_to_utf16(&s));
                            }
                        }
                        i += 2;
                    } else {
                        result.push('$' as u16);
                        result.push(c);
                        i += 2;
                    }
                }
                _ => {
                    result.push(ch);
                    i += 1;
                }
            }
        } else {
            result.push(ch);
            i += 1;
        }
    }

    Ok(result)
}

fn map_index_back(original: &[u16], working_index: usize) -> usize {
    let mut w = 0;
    let mut o = 0;
    while w < working_index {
        if o < original.len() && original[o] == '\r' as u16 && o + 1 < original.len() && original[o + 1] == '\n' as u16 {
            o += 2;
            w += 1;
        } else {
            o += 1;
            w += 1;
        }
    }
    o
}

/// RegExp.escape ( string ) — §22.2.4.3
/// Returns a new string with regex-special characters escaped.
pub(crate) fn regexp_escape<'gc>(
    _mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 1: If S is not a String, throw a TypeError exception.
    let s = if args.is_empty() {
        return Err(raise_type_error!("RegExp.escape requires a string argument").into());
    } else {
        match &args[0] {
            Value::String(s) => s.clone(),
            _ => return Err(raise_type_error!("RegExp.escape requires a string argument").into()),
        }
    };

    // Syntax characters: ^$\.*+?()[]{}|  plus U+002F (SOLIDUS)
    const SYNTAX_CHARS: &[u16] = &[
        b'^' as u16,
        b'$' as u16,
        b'\\' as u16,
        b'.' as u16,
        b'*' as u16,
        b'+' as u16,
        b'?' as u16,
        b'(' as u16,
        b')' as u16,
        b'[' as u16,
        b']' as u16,
        b'{' as u16,
        b'}' as u16,
        b'|' as u16,
        b'/' as u16,
    ];

    // Other punctuators: ,-=<>#&!%:;@~'`"
    const OTHER_PUNCTUATORS: &[u16] = &[
        b',' as u16,
        b'-' as u16,
        b'=' as u16,
        b'<' as u16,
        b'>' as u16,
        b'#' as u16,
        b'&' as u16,
        b'!' as u16,
        b'%' as u16,
        b':' as u16,
        b';' as u16,
        b'@' as u16,
        b'~' as u16,
        b'\'' as u16,
        b'`' as u16,
        b'"' as u16,
    ];

    fn is_whitespace(c: u32) -> bool {
        matches!(
            c,
            0x0009 | 0x000B | 0x000C | 0x0020 | 0x00A0 | 0xFEFF | 0x1680 | 0x2000..=0x200A | 0x202F | 0x205F | 0x3000
        )
    }

    fn is_line_terminator(c: u32) -> bool {
        matches!(c, 0x000A | 0x000D | 0x2028 | 0x2029)
    }

    fn is_surrogate(c: u32) -> bool {
        (0xD800..=0xDFFF).contains(&c)
    }

    fn is_decimal_digit(c: u32) -> bool {
        (0x30..=0x39).contains(&c) // '0'..'9'
    }

    fn is_ascii_letter(c: u32) -> bool {
        (0x41..=0x5A).contains(&c) || (0x61..=0x7A).contains(&c) // A-Z, a-z
    }

    // Iterate over code points (decode surrogate pairs)
    let mut result: Vec<u16> = Vec::with_capacity(s.len() * 2);
    let mut first = true;
    let mut i = 0;
    while i < s.len() {
        let cu = s[i];
        // Decode code point from UTF-16
        let (cp, advance) = if (0xD800..=0xDBFF).contains(&cu) && i + 1 < s.len() && (0xDC00..=0xDFFF).contains(&s[i + 1]) {
            let hi = cu as u32;
            let lo = s[i + 1] as u32;
            ((hi - 0xD800) * 0x400 + (lo - 0xDC00) + 0x10000, 2)
        } else {
            (cu as u32, 1)
        };

        if first {
            first = false;
            if is_decimal_digit(cp) || is_ascii_letter(cp) {
                // Step 4a: Escape initial digit/letter as \xHH
                result.extend_from_slice(&encode_hex_escape(cp));
                i += advance;
                continue;
            }
        }

        // Step 4b.i: EncodeForRegExpEscape(c)

        // 1. If c is a SyntaxCharacter, return \c
        if advance == 1 && SYNTAX_CHARS.contains(&cu) {
            result.push(b'\\' as u16);
            result.push(cu);
        }
        // 2. ControlEscape: \t, \n, \v, \f, \r
        else if cp == 0x0009 {
            result.push(b'\\' as u16);
            result.push(b't' as u16);
        } else if cp == 0x000A {
            result.push(b'\\' as u16);
            result.push(b'n' as u16);
        } else if cp == 0x000B {
            result.push(b'\\' as u16);
            result.push(b'v' as u16);
        } else if cp == 0x000C {
            result.push(b'\\' as u16);
            result.push(b'f' as u16);
        } else if cp == 0x000D {
            result.push(b'\\' as u16);
            result.push(b'r' as u16);
        }
        // 5. otherPunctuators, WhiteSpace, LineTerminator, or surrogate
        else if (advance == 1 && OTHER_PUNCTUATORS.contains(&cu)) || is_whitespace(cp) || is_line_terminator(cp) || is_surrogate(cp) {
            if cp <= 0xFF {
                result.extend_from_slice(&encode_hex_escape(cp));
            } else {
                // UTF16EncodeCodePoint then UnicodeEscape each code unit
                if cp <= 0xFFFF {
                    result.extend_from_slice(&encode_unicode_escape(cp as u16));
                } else {
                    // Surrogate pair
                    let hi = ((cp - 0x10000) >> 10) as u16 + 0xD800;
                    let lo = ((cp - 0x10000) & 0x3FF) as u16 + 0xDC00;
                    result.extend_from_slice(&encode_unicode_escape(hi));
                    result.extend_from_slice(&encode_unicode_escape(lo));
                }
            }
        }
        // 6. Pass through: UTF16EncodeCodePoint(c)
        else {
            for j in 0..advance {
                result.push(s[i + j]);
            }
        }

        i += advance;
    }

    Ok(Value::String(result))
}

/// Encode a code point as \xHH (2-digit hex)
fn encode_hex_escape(cp: u32) -> [u16; 4] {
    let hex = format!("{cp:02x}");
    let bytes = hex.as_bytes();
    [b'\\' as u16, b'x' as u16, bytes[0] as u16, bytes[1] as u16]
}

/// Encode a code unit as \uHHHH (4-digit hex)
fn encode_unicode_escape(cu: u16) -> [u16; 6] {
    let hex = format!("{cu:04x}");
    let bytes = hex.as_bytes();
    [
        b'\\' as u16,
        b'u' as u16,
        bytes[0] as u16,
        bytes[1] as u16,
        bytes[2] as u16,
        bytes[3] as u16,
    ]
}
