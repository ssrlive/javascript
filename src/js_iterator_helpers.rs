// js_iterator_helpers.rs — Iterator Helpers (iterator-helpers, iterator-sequencing, joint-iteration)
//
// Implements:
//   • `Iterator` constructor (abstract, subclassable)
//   • `Iterator.prototype` methods: map, filter, take, drop, flatMap,
//     reduce, toArray, forEach, some, every, find
//   • `Iterator.from` static method
//   • `%IteratorHelperPrototype%` for lazy adapters
//   • `%WrapForValidIteratorPrototype%` for Iterator.from wrapping
//   • `Iterator.concat` (iterator-sequencing)
//   • `Iterator.zip` / `Iterator.zipKeyed` (joint-iteration) [stubs]

use crate::core::{
    EvalError, Gc, InternalSlot, JSObjectDataPtr, MutationContext, PropertyKey, Value, create_descriptor_object, env_get, env_set,
    evaluate_call_dispatch, new_gc_cell_ptr, new_js_object_data, object_get_key_value, object_set_key_value, slot_get, slot_get_chained,
    slot_set,
};
use crate::js_object::define_property_internal;
use crate::unicode::utf8_to_utf16;

// ---------------------------------------------------------------------------
// InternalSlot names used by this module
// ---------------------------------------------------------------------------
// IteratorPrototype          — %IteratorPrototype%  (stored on env)
// IteratorHelperPrototype    — %IteratorHelperPrototype% (stored on env)
// WrapForValidIteratorProto  — %WrapForValidIteratorPrototype% (stored on env)
//
// Per-instance iterator-helper slots (stored on the helper object):
// IteratorHelperKind         — discriminant: "map", "filter", "take", "drop", "flatMap"
// IteratorHelperUnderlying   — the underlying iterator object
// IteratorHelperNextMethod   — the underlying .next method
// IteratorHelperCallback     — the mapper / predicate callback
// IteratorHelperCounter      — running counter (f64)
// IteratorHelperRemaining    — remaining count for take / drop (f64)
// IteratorHelperDone         — boolean, true when exhausted
// IteratorHelperInnerIter    — inner iterator for flatMap
// IteratorHelperInnerNext    — inner .next method for flatMap
//
// WrapForValid slots (stored on the wrapper object):
// WrapForValidUnderlying     — the underlying iterator object
// WrapForValidNextMethod     — the underlying .next method

// ===========================================================================
// Public initialisation
// ===========================================================================

/// Register `Iterator` constructor, `Iterator.prototype` methods, and helpers
/// on the global environment.  Must be called *after* `initialize_array` (which
/// creates %IteratorPrototype%) and *after* symbol + function constructors.
pub fn initialize_iterator_helpers<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), crate::error::JSError> {
    // --- Retrieve %IteratorPrototype% (stashed by js_array init) ---
    let iter_proto = match slot_get_chained(env, &InternalSlot::IteratorPrototype) {
        Some(rc) => match &*rc.borrow() {
            Value::Object(o) => *o,
            _ => return Ok(()), // silently skip if not set
        },
        None => return Ok(()),
    };

    // --- Retrieve Function.prototype ---
    let func_proto = get_func_proto(env);

    // --- Helper: create a builtin method object ---
    let mk_method = |native: &str, display: &str, len: f64| -> Result<JSObjectDataPtr<'gc>, crate::error::JSError> {
        let obj = new_js_object_data(mc);
        if let Some(fp) = func_proto {
            obj.borrow_mut(mc).prototype = Some(fp);
        }
        obj.borrow_mut(mc)
            .set_closure(Some(new_gc_cell_ptr(mc, Value::Function(native.to_string()))));
        let nd = create_descriptor_object(mc, &Value::String(utf8_to_utf16(display)), false, false, true)?;
        define_property_internal(mc, &obj, "name", &nd)?;
        let ld = create_descriptor_object(mc, &Value::Number(len), false, false, true)?;
        define_property_internal(mc, &obj, "length", &ld)?;
        Ok(obj)
    };

    // =======================================================================
    // 1) Iterator constructor
    // =======================================================================
    let iterator_ctor = new_js_object_data(mc);
    if let Some(fp) = func_proto {
        iterator_ctor.borrow_mut(mc).prototype = Some(fp);
    }
    // Mark it as a native constructor so `new SubIterator()` works
    slot_set(
        mc,
        &iterator_ctor,
        InternalSlot::NativeCtor,
        &Value::String(utf8_to_utf16("Iterator")),
    );
    slot_set(mc, &iterator_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &iterator_ctor, InternalSlot::Callable, &Value::Boolean(true));
    iterator_ctor
        .borrow_mut(mc)
        .set_closure(Some(new_gc_cell_ptr(mc, Value::Function("Iterator".to_string()))));

    // Iterator.name = "Iterator"
    let name_d = create_descriptor_object(mc, &Value::String(utf8_to_utf16("Iterator")), false, false, true)?;
    define_property_internal(mc, &iterator_ctor, "name", &name_d)?;
    // Iterator.length = 0
    let len_d = create_descriptor_object(mc, &Value::Number(0.0), false, false, true)?;
    define_property_internal(mc, &iterator_ctor, "length", &len_d)?;

    // Iterator.prototype = %IteratorPrototype%  { writable: false, enumerable: false, configurable: false }
    let proto_desc = create_descriptor_object(mc, &Value::Object(iter_proto), false, false, false)?;
    define_property_internal(mc, &iterator_ctor, "prototype", &proto_desc)?;

    // =======================================================================
    // 2) Iterator.prototype.constructor — accessor { get, set }
    // =======================================================================
    {
        let getter = mk_method("Iterator.prototype.get constructor", "get constructor", 0.0)?;
        let setter = mk_method("Iterator.prototype.set constructor", "set constructor", 1.0)?;
        let desc = accessor_descriptor(mc, &getter, &setter, false, true)?;
        define_property_internal(mc, &iter_proto, "constructor", &desc)?;
    }

    // =======================================================================
    // 3) Iterator.prototype[@@toStringTag] — accessor { get, set }
    // =======================================================================
    if let Some(tag_sym) = get_well_known_symbol(env, "toStringTag") {
        let getter = mk_method("Iterator.prototype.get @@toStringTag", "get [Symbol.toStringTag]", 0.0)?;
        let setter = mk_method("Iterator.prototype.set @@toStringTag", "set [Symbol.toStringTag]", 1.0)?;
        let desc = accessor_descriptor(mc, &getter, &setter, false, true)?;
        define_property_internal(mc, &iter_proto, PropertyKey::Symbol(tag_sym), &desc)?;
    }

    // =======================================================================
    // 4) Iterator.prototype methods (lazy — return IteratorHelper)
    // =======================================================================
    let lazy_methods: &[(&str, &str, f64)] = &[
        ("Iterator.prototype.map", "map", 1.0),
        ("Iterator.prototype.filter", "filter", 1.0),
        ("Iterator.prototype.take", "take", 1.0),
        ("Iterator.prototype.drop", "drop", 1.0),
        ("Iterator.prototype.flatMap", "flatMap", 1.0),
    ];
    for &(native, display, len) in lazy_methods {
        let m = mk_method(native, display, len)?;
        object_set_key_value(mc, &iter_proto, display, &Value::Object(m))?;
        iter_proto
            .borrow_mut(mc)
            .set_non_enumerable(PropertyKey::String(display.to_string()));
    }

    // =======================================================================
    // 5) Iterator.prototype methods (eager — return value immediately)
    // =======================================================================
    let eager_methods: &[(&str, &str, f64)] = &[
        ("Iterator.prototype.reduce", "reduce", 1.0),
        ("Iterator.prototype.toArray", "toArray", 0.0),
        ("Iterator.prototype.forEach", "forEach", 1.0),
        ("Iterator.prototype.some", "some", 1.0),
        ("Iterator.prototype.every", "every", 1.0),
        ("Iterator.prototype.find", "find", 1.0),
    ];
    for &(native, display, len) in eager_methods {
        let m = mk_method(native, display, len)?;
        object_set_key_value(mc, &iter_proto, display, &Value::Object(m))?;
        iter_proto
            .borrow_mut(mc)
            .set_non_enumerable(PropertyKey::String(display.to_string()));
    }

    // =======================================================================
    // 6) Iterator.from — static method
    // =======================================================================
    {
        let from_obj = mk_method("Iterator.from", "from", 1.0)?;
        object_set_key_value(mc, &iterator_ctor, "from", &Value::Object(from_obj))?;
        iterator_ctor
            .borrow_mut(mc)
            .set_non_enumerable(PropertyKey::String("from".to_string()));
    }

    // =======================================================================
    // 7) %IteratorHelperPrototype%
    // =======================================================================
    let helper_proto = new_js_object_data(mc);
    helper_proto.borrow_mut(mc).prototype = Some(iter_proto);

    // next method
    {
        let next_m = mk_method("IteratorHelper.prototype.next", "next", 0.0)?;
        object_set_key_value(mc, &helper_proto, "next", &Value::Object(next_m))?;
        helper_proto
            .borrow_mut(mc)
            .set_non_enumerable(PropertyKey::String("next".to_string()));
    }
    // return method
    {
        let ret_m = mk_method("IteratorHelper.prototype.return", "return", 0.0)?;
        object_set_key_value(mc, &helper_proto, "return", &Value::Object(ret_m))?;
        helper_proto
            .borrow_mut(mc)
            .set_non_enumerable(PropertyKey::String("return".to_string()));
    }
    // @@toStringTag = "Iterator Helper"
    if let Some(tag_sym) = get_well_known_symbol(env, "toStringTag") {
        let td = create_descriptor_object(mc, &Value::String(utf8_to_utf16("Iterator Helper")), false, false, true)?;
        define_property_internal(mc, &helper_proto, PropertyKey::Symbol(tag_sym), &td)?;
    }

    slot_set(mc, env, InternalSlot::IteratorHelperPrototype, &Value::Object(helper_proto));

    // =======================================================================
    // 8) %WrapForValidIteratorPrototype%
    // =======================================================================
    let wrap_proto = new_js_object_data(mc);
    wrap_proto.borrow_mut(mc).prototype = Some(iter_proto);

    {
        let next_m = mk_method("WrapForValid.prototype.next", "next", 0.0)?;
        object_set_key_value(mc, &wrap_proto, "next", &Value::Object(next_m))?;
        wrap_proto
            .borrow_mut(mc)
            .set_non_enumerable(PropertyKey::String("next".to_string()));
    }
    {
        let ret_m = mk_method("WrapForValid.prototype.return", "return", 0.0)?;
        object_set_key_value(mc, &wrap_proto, "return", &Value::Object(ret_m))?;
        wrap_proto
            .borrow_mut(mc)
            .set_non_enumerable(PropertyKey::String("return".to_string()));
    }

    slot_set(mc, env, InternalSlot::WrapForValidIteratorProto, &Value::Object(wrap_proto));

    // =======================================================================
    // 9) Iterator.concat — static (iterator-sequencing)
    // =======================================================================
    {
        let concat_obj = mk_method("Iterator.concat", "concat", 0.0)?;
        object_set_key_value(mc, &iterator_ctor, "concat", &Value::Object(concat_obj))?;
        iterator_ctor
            .borrow_mut(mc)
            .set_non_enumerable(PropertyKey::String("concat".to_string()));
    }

    // =======================================================================
    // 10) Iterator.zip / Iterator.zipKeyed — static (joint-iteration)
    // =======================================================================
    {
        let zip_obj = mk_method("Iterator.zip", "zip", 1.0)?;
        object_set_key_value(mc, &iterator_ctor, "zip", &Value::Object(zip_obj))?;
        iterator_ctor
            .borrow_mut(mc)
            .set_non_enumerable(PropertyKey::String("zip".to_string()));
    }
    {
        let zipk_obj = mk_method("Iterator.zipKeyed", "zipKeyed", 1.0)?;
        object_set_key_value(mc, &iterator_ctor, "zipKeyed", &Value::Object(zipk_obj))?;
        iterator_ctor
            .borrow_mut(mc)
            .set_non_enumerable(PropertyKey::String("zipKeyed".to_string()));
    }

    // =======================================================================
    // Store Iterator constructor globally
    // =======================================================================
    env_set(mc, env, "Iterator", &Value::Object(iterator_ctor))?;

    Ok(())
}

// ===========================================================================
// Native dispatch handler — called from `call_native_function`
// ===========================================================================

/// Handle all Iterator-helpers native dispatch names.  Returns `Some(value)` on
/// match or `None` when the name is not ours.
pub fn handle_iterator_helper_dispatch<'gc>(
    mc: &MutationContext<'gc>,
    name: &str,
    this_val: Option<&Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    match name {
        // --- Iterator constructor ---
        "Iterator" => Ok(Some(handle_iterator_constructor(mc, env)?)),

        // --- Iterator.prototype accessor getters/setters ---
        "Iterator.prototype.get constructor" => Ok(Some(handle_get_constructor(env)?)),
        "Iterator.prototype.set constructor" => {
            let val = args.first().cloned().unwrap_or(Value::Undefined);
            handle_set_constructor(mc, this_val, &val, env)?;
            Ok(Some(Value::Undefined))
        }
        "Iterator.prototype.get @@toStringTag" => Ok(Some(Value::String(utf8_to_utf16("Iterator")))),
        "Iterator.prototype.set @@toStringTag" => {
            let val = args.first().cloned().unwrap_or(Value::Undefined);
            handle_set_to_string_tag(mc, this_val, &val, env)?;
            Ok(Some(Value::Undefined))
        }

        // --- Lazy methods ---
        "Iterator.prototype.map" => {
            let cb = args.first().cloned().unwrap_or(Value::Undefined);
            Ok(Some(create_iterator_helper(mc, this_val, &cb, "map", 0.0, env)?))
        }
        "Iterator.prototype.filter" => {
            let cb = args.first().cloned().unwrap_or(Value::Undefined);
            Ok(Some(create_iterator_helper(mc, this_val, &cb, "filter", 0.0, env)?))
        }
        "Iterator.prototype.take" => {
            let limit = args.first().cloned().unwrap_or(Value::Undefined);
            let this_obj = require_object_this(this_val)?;
            // Step 2: Let numLimit = ? ToNumber(limit) — IfAbruptCloseIterator
            let num_limit = match crate::core::to_number_with_env(mc, env, &limit) {
                Ok(n) => n,
                Err(e) => {
                    close_underlying_iterator(mc, &this_obj, env);
                    return Err(e);
                }
            };
            // Step 3: If numLimit is NaN, throw a RangeError exception. Close iterator.
            if num_limit.is_nan() {
                close_underlying_iterator(mc, &this_obj, env);
                return Err(raise_range_error!("Iterator.prototype.take argument must be a non-negative number").into());
            }
            // Step 4: Let integerLimit = ! ToIntegerOrInfinity(numLimit).
            let n = if num_limit == 0.0 || num_limit.is_infinite() {
                num_limit
            } else {
                num_limit.trunc()
            };
            // Step 5: If integerLimit < 0, throw a RangeError exception. Close iterator.
            if n < 0.0 {
                close_underlying_iterator(mc, &this_obj, env);
                return Err(raise_range_error!("Iterator.prototype.take argument must be a non-negative number").into());
            }
            Ok(Some(create_iterator_helper(mc, this_val, &Value::Undefined, "take", n, env)?))
        }
        "Iterator.prototype.drop" => {
            let limit = args.first().cloned().unwrap_or(Value::Undefined);
            let this_obj = require_object_this(this_val)?;
            // Step 2: Let numLimit = ? ToNumber(limit) — IfAbruptCloseIterator
            let num_limit = match crate::core::to_number_with_env(mc, env, &limit) {
                Ok(n) => n,
                Err(e) => {
                    close_underlying_iterator(mc, &this_obj, env);
                    return Err(e);
                }
            };
            // Step 3: If numLimit is NaN, throw a RangeError exception. Close iterator.
            if num_limit.is_nan() {
                close_underlying_iterator(mc, &this_obj, env);
                return Err(raise_range_error!("Iterator.prototype.drop argument must be a non-negative number").into());
            }
            // Step 4: Let integerLimit = ! ToIntegerOrInfinity(numLimit).
            let n = if num_limit == 0.0 || num_limit.is_infinite() {
                num_limit
            } else {
                num_limit.trunc()
            };
            // Step 5: If integerLimit < 0, throw a RangeError exception. Close iterator.
            if n < 0.0 {
                close_underlying_iterator(mc, &this_obj, env);
                return Err(raise_range_error!("Iterator.prototype.drop argument must be a non-negative number").into());
            }
            Ok(Some(create_iterator_helper(mc, this_val, &Value::Undefined, "drop", n, env)?))
        }
        "Iterator.prototype.flatMap" => {
            let cb = args.first().cloned().unwrap_or(Value::Undefined);
            Ok(Some(create_iterator_helper(mc, this_val, &cb, "flatMap", 0.0, env)?))
        }

        // --- Eager methods ---
        "Iterator.prototype.reduce" => {
            let reducer = args.first().cloned().unwrap_or(Value::Undefined);
            let initial = args.get(1).cloned();
            Ok(Some(handle_reduce(mc, this_val, &reducer, initial.as_ref(), env)?))
        }
        "Iterator.prototype.toArray" => Ok(Some(handle_to_array(mc, this_val, env)?)),
        "Iterator.prototype.forEach" => {
            let cb = args.first().cloned().unwrap_or(Value::Undefined);
            Ok(Some(handle_for_each(mc, this_val, &cb, env)?))
        }
        "Iterator.prototype.some" => {
            let pred = args.first().cloned().unwrap_or(Value::Undefined);
            Ok(Some(handle_some(mc, this_val, &pred, env)?))
        }
        "Iterator.prototype.every" => {
            let pred = args.first().cloned().unwrap_or(Value::Undefined);
            Ok(Some(handle_every(mc, this_val, &pred, env)?))
        }
        "Iterator.prototype.find" => {
            let pred = args.first().cloned().unwrap_or(Value::Undefined);
            Ok(Some(handle_find(mc, this_val, &pred, env)?))
        }

        // --- Iterator.from ---
        "Iterator.from" => {
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            Ok(Some(handle_iterator_from(mc, &arg, env)?))
        }

        // --- IteratorHelper.prototype.next ---
        "IteratorHelper.prototype.next" => Ok(Some(handle_helper_next(mc, this_val, env)?)),
        "IteratorHelper.prototype.return" => Ok(Some(handle_helper_return(mc, this_val, env)?)),

        // --- WrapForValid.prototype.next ---
        "WrapForValid.prototype.next" => Ok(Some(handle_wrap_next(mc, this_val, env)?)),
        "WrapForValid.prototype.return" => Ok(Some(handle_wrap_return(mc, this_val, env)?)),

        // --- Iterator.concat ---
        "Iterator.concat" => Ok(Some(handle_iterator_concat(mc, args, env)?)),

        // --- Iterator.zip / Iterator.zipKeyed ---
        "Iterator.zip" => Ok(Some(handle_iterator_zip(mc, args, env)?)),
        "Iterator.zipKeyed" => Ok(Some(handle_iterator_zip_keyed(mc, args, env)?)),

        _ => Ok(None),
    }
}

// ===========================================================================
// Iterator constructor
// ===========================================================================

fn handle_iterator_constructor<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, EvalError<'gc>> {
    // Per spec: If NewTarget is undefined or the active function object, throw TypeError.
    // We check __new_target from the call environment.
    let new_target = slot_get_chained(env, &InternalSlot::NewTarget);
    match new_target {
        None => Err(raise_type_error!("Iterator is not callable without new").into()),
        Some(nt_rc) => {
            let nt = nt_rc.borrow().clone();
            // If new.target IS the Iterator constructor itself, throw.
            if let Some(iter_ctor_val) = env_get(env, "Iterator")
                && let Value::Object(iter_ctor) = &*iter_ctor_val.borrow()
                && let Value::Object(nt_obj) = &nt
                && Gc::as_ptr(*nt_obj) == Gc::as_ptr(*iter_ctor)
            {
                return Err(raise_type_error!("Abstract class Iterator not directly constructable").into());
            }
            // Subclass: return `this` (OrdinaryCreateFromConstructor result)
            if let Some(this_val) = slot_get_chained(env, &InternalSlot::Instance) {
                return Ok(this_val.borrow().clone());
            }
            // Create a new object with Iterator.prototype
            let obj = new_js_object_data(mc);
            if let Some(ip) = slot_get_chained(env, &InternalSlot::IteratorPrototype)
                && let Value::Object(ip_obj) = &*ip.borrow()
            {
                obj.borrow_mut(mc).prototype = Some(*ip_obj);
            }
            Ok(Value::Object(obj))
        }
    }
}

// ===========================================================================
// Accessor: Iterator.prototype.constructor
// ===========================================================================

fn handle_get_constructor<'gc>(env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, EvalError<'gc>> {
    // Getter always returns %Iterator%
    if let Some(ctor) = env_get(env, "Iterator") {
        Ok(ctor.borrow().clone())
    } else {
        Ok(Value::Undefined)
    }
}

fn handle_set_constructor<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    val: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), EvalError<'gc>> {
    setter_that_ignores_prototype_properties(mc, this_val, "constructor", val, env)
}

// ===========================================================================
// Accessor: Iterator.prototype[@@toStringTag]
// ===========================================================================

fn handle_set_to_string_tag<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    val: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), EvalError<'gc>> {
    setter_that_ignores_prototype_properties(mc, this_val, "@@toStringTag", val, env)
}

/// SetterThatIgnoresPrototypeProperties ( home, p, v )
fn setter_that_ignores_prototype_properties<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    prop: &str,
    val: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), EvalError<'gc>> {
    // 1. If this is not an Object, throw TypeError.
    let this_obj = match this_val {
        Some(Value::Object(o)) => *o,
        _ => return Err(raise_type_error!("setter called on non-object").into()),
    };

    // Get %IteratorPrototype% (home)
    let home = match slot_get_chained(env, &InternalSlot::IteratorPrototype) {
        Some(rc) => match &*rc.borrow() {
            Value::Object(o) => *o,
            _ => return Ok(()),
        },
        None => return Ok(()),
    };

    // 2. If this is home, throw TypeError
    if Gc::as_ptr(this_obj) == Gc::as_ptr(home) {
        return Err(raise_type_error!("Cannot set property on Iterator.prototype directly").into());
    }

    // 3. Let desc = ? this.[[GetOwnProperty]](p)
    let prop_key: PropertyKey = if prop == "@@toStringTag" {
        if let Some(sym) = get_well_known_symbol(env, "toStringTag") {
            PropertyKey::Symbol(sym)
        } else {
            return Ok(());
        }
    } else {
        PropertyKey::String(prop.to_string())
    };

    let has_own = {
        let borrow = this_obj.borrow();
        match &prop_key {
            PropertyKey::String(s) => borrow.get_property(s).is_some(),
            PropertyKey::Symbol(s) => borrow.get_property(PropertyKey::Symbol(*s)).is_some(),
            _ => false,
        }
    };

    if !has_own {
        // desc is undefined → CreateDataPropertyOrThrow(this, p, v)
        object_set_key_value(mc, &this_obj, prop_key, val)?;
    } else {
        // desc exists → Set(this, p, v, true)
        object_set_key_value(mc, &this_obj, prop_key, val)?;
    }
    Ok(())
}

// ===========================================================================
// create_iterator_helper — lazy adapters (map, filter, take, drop, flatMap)
// ===========================================================================

fn create_iterator_helper<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    callback: &Value<'gc>,
    kind: &str,
    numeric_arg: f64,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 1-2: Let O = RequireObjectCoercible(this)
    let iter_obj = require_object_this(this_val)?;

    // For map, filter, flatMap: callback must be callable BEFORE GetIteratorDirect
    // Per spec, if callable check fails, close the iterator and throw
    if matches!(kind, "map" | "filter" | "flatMap") && !is_callable(callback) {
        close_underlying_iterator(mc, &iter_obj, env);
        return Err(raise_type_error!("Expected a callable").into());
    }

    // GetIteratorDirect(this) — reads .next
    let next_method = get_next_method(mc, &iter_obj, env)?;

    // Create the helper object
    let helper = new_js_object_data(mc);
    if let Some(hp_val) = slot_get_chained(env, &InternalSlot::IteratorHelperPrototype)
        && let Value::Object(hp) = &*hp_val.borrow()
    {
        helper.borrow_mut(mc).prototype = Some(*hp);
    }

    slot_set(mc, &helper, InternalSlot::IteratorHelperKind, &Value::String(utf8_to_utf16(kind)));
    slot_set(mc, &helper, InternalSlot::IteratorHelperUnderlying, &Value::Object(iter_obj));
    slot_set(mc, &helper, InternalSlot::IteratorHelperNextMethod, &next_method);
    if !matches!(callback, Value::Undefined) {
        slot_set(mc, &helper, InternalSlot::IteratorHelperCallback, callback);
    }
    slot_set(mc, &helper, InternalSlot::IteratorHelperCounter, &Value::Number(0.0));
    slot_set(mc, &helper, InternalSlot::IteratorHelperRemaining, &Value::Number(numeric_arg));
    slot_set(mc, &helper, InternalSlot::IteratorHelperDone, &Value::Boolean(false));
    slot_set(mc, &helper, InternalSlot::IteratorHelperExecuting, &Value::Boolean(false));

    Ok(Value::Object(helper))
}

// ===========================================================================
// IteratorHelper.prototype.next
// ===========================================================================

fn handle_helper_next<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let helper = require_object_this(this_val)?;

    // Re-entrancy guard: if currently executing, throw TypeError
    if let Some(exec_rc) = slot_get(&helper, &InternalSlot::IteratorHelperExecuting)
        && let Value::Boolean(true) = &*exec_rc.borrow()
    {
        return Err(raise_type_error!("Cannot reenter a generator that is already running").into());
    }

    // Check if done
    if let Some(done_rc) = slot_get(&helper, &InternalSlot::IteratorHelperDone)
        && let Value::Boolean(true) = &*done_rc.borrow()
    {
        return make_iter_result(mc, env, &Value::Undefined, true);
    }

    let kind = get_slot_string(&helper, &InternalSlot::IteratorHelperKind);

    // Mark that .next() has been called (transition from suspended-start to suspended-yield)
    slot_set(mc, &helper, InternalSlot::IteratorHelperStarted, &Value::Boolean(true));

    // Set executing flag
    slot_set(mc, &helper, InternalSlot::IteratorHelperExecuting, &Value::Boolean(true));

    // concat kind doesn't use underlying/next_method the same way
    if kind == "concat" {
        let result = helper_next_concat(mc, &helper, env);
        slot_set(mc, &helper, InternalSlot::IteratorHelperExecuting, &Value::Boolean(false));
        return result;
    }

    // zip / zipKeyed also use their own slot layout
    if kind == "zip" || kind == "zipKeyed" {
        let result = helper_next_zip(mc, &helper, env, kind == "zipKeyed");
        slot_set(mc, &helper, InternalSlot::IteratorHelperExecuting, &Value::Boolean(false));
        return result;
    }

    let underlying = match get_slot_object(&helper, &InternalSlot::IteratorHelperUnderlying) {
        Ok(u) => u,
        Err(e) => {
            slot_set(mc, &helper, InternalSlot::IteratorHelperExecuting, &Value::Boolean(false));
            return Err(e);
        }
    };
    let next_method = match slot_get(&helper, &InternalSlot::IteratorHelperNextMethod) {
        Some(rc) => rc.borrow().clone(),
        None => {
            slot_set(mc, &helper, InternalSlot::IteratorHelperExecuting, &Value::Boolean(false));
            return Err(raise_type_error!("Invalid iterator helper state").into());
        }
    };

    // Per spec: the next method must be callable when actually invoked.
    match &next_method {
        Value::Function(_) | Value::Closure(_) | Value::Object(_) => {}
        _ => {
            slot_set(mc, &helper, InternalSlot::IteratorHelperExecuting, &Value::Boolean(false));
            return Err(raise_type_error!("Iterator does not have a callable next method").into());
        }
    }

    let result = match kind.as_str() {
        "map" => helper_next_map(mc, &helper, &underlying, &next_method, env),
        "filter" => helper_next_filter(mc, &helper, &underlying, &next_method, env),
        "take" => helper_next_take(mc, &helper, &underlying, &next_method, env),
        "drop" => helper_next_drop(mc, &helper, &underlying, &next_method, env),
        "flatMap" => helper_next_flat_map(mc, &helper, &underlying, &next_method, env),
        _ => Err(raise_type_error!("Unknown iterator helper kind").into()),
    };

    // Clear executing flag
    slot_set(mc, &helper, InternalSlot::IteratorHelperExecuting, &Value::Boolean(false));
    result
}

fn helper_next_map<'gc>(
    mc: &MutationContext<'gc>,
    helper: &JSObjectDataPtr<'gc>,
    underlying: &JSObjectDataPtr<'gc>,
    next_method: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let result = evaluate_call_dispatch(mc, env, next_method, Some(&Value::Object(*underlying)), &[])?;
    if iter_result_done(mc, &result, env)? {
        mark_done(mc, helper);
        return make_iter_result(mc, env, &Value::Undefined, true);
    }
    let value = iter_result_value(mc, &result, env)?;
    let counter = get_counter(helper);
    inc_counter(mc, helper);

    let callback = match slot_get(helper, &InternalSlot::IteratorHelperCallback) {
        Some(rc) => rc.borrow().clone(),
        None => return Err(raise_type_error!("map: missing callback").into()),
    };

    // IfAbruptCloseIterator: if callback throws, close the underlying iterator.
    let mapped = match evaluate_call_dispatch(mc, env, &callback, None, &[value, Value::Number(counter)]) {
        Ok(v) => v,
        Err(e) => {
            close_underlying_iterator(mc, underlying, env);
            return Err(e);
        }
    };
    make_iter_result(mc, env, &mapped, false)
}

fn helper_next_filter<'gc>(
    mc: &MutationContext<'gc>,
    helper: &JSObjectDataPtr<'gc>,
    underlying: &JSObjectDataPtr<'gc>,
    next_method: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let callback = match slot_get(helper, &InternalSlot::IteratorHelperCallback) {
        Some(rc) => rc.borrow().clone(),
        None => return Err(raise_type_error!("filter: missing callback").into()),
    };

    loop {
        let result = evaluate_call_dispatch(mc, env, next_method, Some(&Value::Object(*underlying)), &[])?;
        if iter_result_done(mc, &result, env)? {
            mark_done(mc, helper);
            return make_iter_result(mc, env, &Value::Undefined, true);
        }
        let value = iter_result_value(mc, &result, env)?;
        let counter = get_counter(helper);
        inc_counter(mc, helper);

        // IfAbruptCloseIterator: if callback throws, close the underlying iterator.
        let selected = match evaluate_call_dispatch(mc, env, &callback, None, &[value.clone(), Value::Number(counter)]) {
            Ok(v) => v,
            Err(e) => {
                close_underlying_iterator(mc, underlying, env);
                return Err(e);
            }
        };
        if to_boolean(&selected) {
            return make_iter_result(mc, env, &value, false);
        }
    }
}

fn helper_next_take<'gc>(
    mc: &MutationContext<'gc>,
    helper: &JSObjectDataPtr<'gc>,
    underlying: &JSObjectDataPtr<'gc>,
    next_method: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let remaining = get_remaining(helper);
    if remaining <= 0.0 {
        // Take limit reached: close underlying iterator, propagate errors from .return()
        mark_done(mc, helper);
        close_underlying_iterator_throwing(mc, underlying, env)?;
        return make_iter_result(mc, env, &Value::Undefined, true);
    }
    dec_remaining(mc, helper);

    let result = evaluate_call_dispatch(mc, env, next_method, Some(&Value::Object(*underlying)), &[])?;
    if iter_result_done(mc, &result, env)? {
        mark_done(mc, helper);
        return make_iter_result(mc, env, &Value::Undefined, true);
    }
    let value = iter_result_value(mc, &result, env)?;
    make_iter_result(mc, env, &value, false)
}

fn helper_next_drop<'gc>(
    mc: &MutationContext<'gc>,
    helper: &JSObjectDataPtr<'gc>,
    underlying: &JSObjectDataPtr<'gc>,
    next_method: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Drop phase: skip items until remaining == 0
    let mut remaining = get_remaining(helper);
    while remaining > 0.0 {
        let result = evaluate_call_dispatch(mc, env, next_method, Some(&Value::Object(*underlying)), &[])?;
        if iter_result_done(mc, &result, env)? {
            mark_done(mc, helper);
            return make_iter_result(mc, env, &Value::Undefined, true);
        }
        remaining -= 1.0;
    }
    // Set remaining to 0 so future calls skip the loop
    slot_set(mc, helper, InternalSlot::IteratorHelperRemaining, &Value::Number(0.0));

    // Now forward
    let result = evaluate_call_dispatch(mc, env, next_method, Some(&Value::Object(*underlying)), &[])?;
    if iter_result_done(mc, &result, env)? {
        mark_done(mc, helper);
        return make_iter_result(mc, env, &Value::Undefined, true);
    }
    let value = iter_result_value(mc, &result, env)?;
    make_iter_result(mc, env, &value, false)
}

fn helper_next_flat_map<'gc>(
    mc: &MutationContext<'gc>,
    helper: &JSObjectDataPtr<'gc>,
    underlying: &JSObjectDataPtr<'gc>,
    next_method: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let callback = match slot_get(helper, &InternalSlot::IteratorHelperCallback) {
        Some(rc) => rc.borrow().clone(),
        None => return Err(raise_type_error!("flatMap: missing callback").into()),
    };

    loop {
        // Check if we have an active inner iterator
        if let Some(inner_rc) = slot_get(helper, &InternalSlot::IteratorHelperInnerIter) {
            let inner_val = inner_rc.borrow().clone();
            if let Value::Object(inner) = inner_val {
                let inner_next = match slot_get(helper, &InternalSlot::IteratorHelperInnerNext) {
                    Some(rc) => rc.borrow().clone(),
                    None => {
                        slot_set(mc, helper, InternalSlot::IteratorHelperInnerIter, &Value::Null);
                        continue;
                    }
                };

                let inner_result = evaluate_call_dispatch(mc, env, &inner_next, Some(&Value::Object(inner)), &[])?;
                if iter_result_done(mc, &inner_result, env)? {
                    slot_set(mc, helper, InternalSlot::IteratorHelperInnerIter, &Value::Null);
                    slot_set(mc, helper, InternalSlot::IteratorHelperInnerNext, &Value::Null);
                    continue;
                }
                let value = iter_result_value(mc, &inner_result, env)?;
                return make_iter_result(mc, env, &value, false);
            }
            // Inner iterator is not an object (Null/Undefined) → fall through
        }

        // Get next from underlying
        let result = evaluate_call_dispatch(mc, env, next_method, Some(&Value::Object(*underlying)), &[])?;
        if iter_result_done(mc, &result, env)? {
            mark_done(mc, helper);
            return make_iter_result(mc, env, &Value::Undefined, true);
        }
        let value = iter_result_value(mc, &result, env)?;
        let counter = get_counter(helper);
        inc_counter(mc, helper);

        let mapped = match evaluate_call_dispatch(mc, env, &callback, None, &[value, Value::Number(counter)]) {
            Ok(v) => v,
            Err(e) => {
                close_underlying_iterator(mc, underlying, env);
                return Err(e);
            }
        };

        // GetIteratorFlattenable(mapped, reject-primitives)
        let (inner_obj, inner_next_method) = get_iterator_flattenable(mc, &mapped, env)?;
        slot_set(mc, helper, InternalSlot::IteratorHelperInnerIter, &Value::Object(inner_obj));
        slot_set(mc, helper, InternalSlot::IteratorHelperInnerNext, &inner_next_method);
    }
}

// ===========================================================================
// IteratorHelper.prototype.return
// ===========================================================================

fn handle_helper_return<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let helper = require_object_this(this_val)?;

    // Re-entrancy guard: if currently executing, throw TypeError
    if let Some(exec_rc) = slot_get(&helper, &InternalSlot::IteratorHelperExecuting)
        && let Value::Boolean(true) = &*exec_rc.borrow()
    {
        return Err(raise_type_error!("Cannot reenter a generator that is already running").into());
    }

    // If already done, return { value: undefined, done: true } without closing
    if let Some(done_rc) = slot_get(&helper, &InternalSlot::IteratorHelperDone)
        && let Value::Boolean(true) = &*done_rc.borrow()
    {
        return make_iter_result(mc, env, &Value::Undefined, true);
    }

    // Check if this helper has ever had .next() called (suspended-start vs suspended-yield)
    let is_started =
        slot_get(&helper, &InternalSlot::IteratorHelperStarted).is_some_and(|rc| matches!(&*rc.borrow(), Value::Boolean(true)));

    // Mark as done (state = completed) BEFORE closing
    mark_done(mc, &helper);

    // Only set executing flag for suspended-yield state.
    // In suspended-start, the state transitions directly to "completed",
    // so re-entrant .next()/.return() calls during close will see done=true
    // and return {value: undefined, done: true} instead of throwing.
    if is_started {
        slot_set(mc, &helper, InternalSlot::IteratorHelperExecuting, &Value::Boolean(true));
    }

    let kind = get_slot_string(&helper, &InternalSlot::IteratorHelperKind);

    let close_result = (|| -> Result<(), EvalError<'gc>> {
        if kind == "concat" {
            // For concat: close the current inner iterator if one is active
            if let Some(inner_rc) = slot_get(&helper, &InternalSlot::IteratorHelperInnerIter)
                && let Value::Object(inner) = &*inner_rc.borrow()
            {
                close_underlying_iterator_throwing(mc, inner, env)?;
            }
        } else if kind == "flatMap" {
            // For flatMap: close the active inner iterator (if any), then close underlying
            if let Some(inner_rc) = slot_get(&helper, &InternalSlot::IteratorHelperInnerIter)
                && let Value::Object(inner) = &*inner_rc.borrow()
            {
                close_underlying_iterator_throwing(mc, inner, env)?;
            }
            slot_set(mc, &helper, InternalSlot::IteratorHelperInnerIter, &Value::Null);
            let underlying = get_slot_object(&helper, &InternalSlot::IteratorHelperUnderlying)?;
            close_underlying_iterator_throwing(mc, &underlying, env)?;
        } else if kind == "zip" || kind == "zipKeyed" {
            // For zip/zipKeyed: close all open iterators in reverse order
            let iters = get_slot_object(&helper, &InternalSlot::ZipIterators)?;
            let opens = get_slot_object(&helper, &InternalSlot::ZipOpenFlags)?;
            let count = get_remaining(&helper) as usize;
            iterator_close_all(mc, &iters, &opens, count, None, env)?;
        } else {
            let underlying = get_slot_object(&helper, &InternalSlot::IteratorHelperUnderlying)?;
            close_underlying_iterator_throwing(mc, &underlying, env)?;
        }
        Ok(())
    })();

    // Clear executing flag
    if is_started {
        slot_set(mc, &helper, InternalSlot::IteratorHelperExecuting, &Value::Boolean(false));
    }

    close_result?;
    make_iter_result(mc, env, &Value::Undefined, true)
}

// ===========================================================================
// Eager methods: reduce, toArray, forEach, some, every, find
// ===========================================================================

fn handle_reduce<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    reducer: &Value<'gc>,
    initial: Option<&Value<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let iter_obj = require_object_this(this_val)?;
    if !is_callable(reducer) {
        close_underlying_iterator(mc, &iter_obj, env);
        return Err(raise_type_error!("Expected a callable").into());
    }
    let next_method = get_next_method(mc, &iter_obj, env)?;

    let mut accumulator: Option<Value<'gc>> = initial.cloned();
    let mut counter: f64 = 0.0;

    loop {
        let result = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
        if iter_result_done(mc, &result, env)? {
            break;
        }
        let value = iter_result_value(mc, &result, env)?;
        match &accumulator {
            None => {
                accumulator = Some(value);
            }
            Some(acc) => {
                // IfAbruptCloseIterator
                accumulator = Some(
                    match evaluate_call_dispatch(mc, env, reducer, None, &[acc.clone(), value, Value::Number(counter)]) {
                        Ok(v) => v,
                        Err(e) => {
                            close_underlying_iterator(mc, &iter_obj, env);
                            return Err(e);
                        }
                    },
                );
            }
        }
        counter += 1.0;
    }

    match accumulator {
        Some(v) => Ok(v),
        None => Err(raise_type_error!("Reduce of empty iterator with no initial value").into()),
    }
}

fn handle_to_array<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let iter_obj = require_object_this(this_val)?;
    let next_method = get_next_method(mc, &iter_obj, env)?;

    let arr = new_js_object_data(mc);
    // Set Array.prototype
    if let Some(arr_ctor_val) = env_get(env, "Array")
        && let Value::Object(arr_ctor) = &*arr_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(arr_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        arr.borrow_mut(mc).prototype = Some(*proto);
    }
    slot_set(mc, &arr, InternalSlot::IsArray, &Value::Boolean(true));

    let mut i: usize = 0;
    loop {
        let result = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
        if iter_result_done(mc, &result, env)? {
            break;
        }
        let value = iter_result_value(mc, &result, env)?;
        object_set_key_value(mc, &arr, i, &value)?;
        i += 1;
    }
    object_set_key_value(mc, &arr, "length", &Value::Number(i as f64))?;

    Ok(Value::Object(arr))
}

fn handle_for_each<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    callback: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let iter_obj = require_object_this(this_val)?;
    if !is_callable(callback) {
        close_underlying_iterator(mc, &iter_obj, env);
        return Err(raise_type_error!("Expected a callable").into());
    }
    let next_method = get_next_method(mc, &iter_obj, env)?;

    let mut counter: f64 = 0.0;
    loop {
        let result = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
        if iter_result_done(mc, &result, env)? {
            break;
        }
        let value = iter_result_value(mc, &result, env)?;
        // IfAbruptCloseIterator for callback
        let _ = match evaluate_call_dispatch(mc, env, callback, None, &[value, Value::Number(counter)]) {
            Ok(v) => v,
            Err(e) => {
                close_underlying_iterator(mc, &iter_obj, env);
                return Err(e);
            }
        };
        counter += 1.0;
    }
    Ok(Value::Undefined)
}

fn handle_some<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    predicate: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let iter_obj = require_object_this(this_val)?;
    if !is_callable(predicate) {
        close_underlying_iterator(mc, &iter_obj, env);
        return Err(raise_type_error!("Expected a callable").into());
    }
    let next_method = get_next_method(mc, &iter_obj, env)?;

    let mut counter: f64 = 0.0;
    loop {
        let result = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
        if iter_result_done(mc, &result, env)? {
            return Ok(Value::Boolean(false));
        }
        let value = iter_result_value(mc, &result, env)?;
        // IfAbruptCloseIterator
        let test = match evaluate_call_dispatch(mc, env, predicate, None, &[value, Value::Number(counter)]) {
            Ok(v) => v,
            Err(e) => {
                close_underlying_iterator(mc, &iter_obj, env);
                return Err(e);
            }
        };
        if to_boolean(&test) {
            close_underlying_iterator_throwing(mc, &iter_obj, env)?;
            return Ok(Value::Boolean(true));
        }
        counter += 1.0;
    }
}

fn handle_every<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    predicate: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let iter_obj = require_object_this(this_val)?;
    if !is_callable(predicate) {
        close_underlying_iterator(mc, &iter_obj, env);
        return Err(raise_type_error!("Expected a callable").into());
    }
    let next_method = get_next_method(mc, &iter_obj, env)?;

    let mut counter: f64 = 0.0;
    loop {
        let result = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
        if iter_result_done(mc, &result, env)? {
            return Ok(Value::Boolean(true));
        }
        let value = iter_result_value(mc, &result, env)?;
        // IfAbruptCloseIterator
        let test = match evaluate_call_dispatch(mc, env, predicate, None, &[value, Value::Number(counter)]) {
            Ok(v) => v,
            Err(e) => {
                close_underlying_iterator(mc, &iter_obj, env);
                return Err(e);
            }
        };
        if !to_boolean(&test) {
            close_underlying_iterator_throwing(mc, &iter_obj, env)?;
            return Ok(Value::Boolean(false));
        }
        counter += 1.0;
    }
}

fn handle_find<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    predicate: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let iter_obj = require_object_this(this_val)?;
    if !is_callable(predicate) {
        close_underlying_iterator(mc, &iter_obj, env);
        return Err(raise_type_error!("Expected a callable").into());
    }
    let next_method = get_next_method(mc, &iter_obj, env)?;

    let mut counter: f64 = 0.0;
    loop {
        let result = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
        if iter_result_done(mc, &result, env)? {
            return Ok(Value::Undefined);
        }
        let value = iter_result_value(mc, &result, env)?;
        // IfAbruptCloseIterator
        let test = match evaluate_call_dispatch(mc, env, predicate, None, &[value.clone(), Value::Number(counter)]) {
            Ok(v) => v,
            Err(e) => {
                close_underlying_iterator(mc, &iter_obj, env);
                return Err(e);
            }
        };
        if to_boolean(&test) {
            close_underlying_iterator_throwing(mc, &iter_obj, env)?;
            return Ok(value);
        }
        counter += 1.0;
    }
}

// ===========================================================================
// Iterator.from
// ===========================================================================

fn handle_iterator_from<'gc>(
    mc: &MutationContext<'gc>,
    arg: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // 1. If O is not an Object, throw TypeError (except strings handled via @@iterator)
    let obj = match arg {
        Value::String(_) => {
            // Strings are iterable: get their @@iterator
            let iter_obj = get_iterator_from_value(mc, arg, env)?;
            // If the result is already an instance of Iterator, return it directly
            if is_instance_of_iterator(&iter_obj, env) {
                return Ok(Value::Object(iter_obj));
            }
            // Otherwise wrap it
            return Ok(Value::Object(create_wrap_for_valid(mc, &iter_obj, env)?));
        }
        Value::Object(o) => *o,
        _ => return Err(raise_type_error!("Iterator.from requires an object argument").into()),
    };

    // 2. Try @@iterator method
    let iter_sym = get_well_known_symbol(env, "iterator");
    if let Some(sym) = iter_sym {
        let method_val = crate::core::get_property_with_accessors(mc, env, &obj, PropertyKey::Symbol(sym))?;
        match &method_val {
            Value::Undefined | Value::Null => {
                // Fall through to treating as a plain iterator (has .next)
            }
            _ => {
                // Call @@iterator to get the iterator
                let iter_result = evaluate_call_dispatch(mc, env, &method_val, Some(arg), &[])?;
                if let Value::Object(iter_obj) = &iter_result {
                    // If result is an instance of Iterator, return it directly
                    if is_instance_of_iterator(iter_obj, env) {
                        return Ok(iter_result);
                    }
                    // Otherwise, wrap it
                    return Ok(Value::Object(create_wrap_for_valid(mc, iter_obj, env)?));
                }
                return Err(raise_type_error!("Iterator.from: @@iterator did not return an object").into());
            }
        }
    }

    // 3. Fallback: treat arg as a plain iterator (must have .next)
    // The object must already have a .next method
    if is_instance_of_iterator(&obj, env) {
        return Ok(Value::Object(obj));
    }
    Ok(Value::Object(create_wrap_for_valid(mc, &obj, env)?))
}

// ===========================================================================
// WrapForValidIterator
// ===========================================================================

fn create_wrap_for_valid<'gc>(
    mc: &MutationContext<'gc>,
    iter_obj: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    let next_method = get_next_method(mc, iter_obj, env)?;

    let wrapper = new_js_object_data(mc);
    if let Some(wp_val) = slot_get_chained(env, &InternalSlot::WrapForValidIteratorProto)
        && let Value::Object(wp) = &*wp_val.borrow()
    {
        wrapper.borrow_mut(mc).prototype = Some(*wp);
    }

    slot_set(mc, &wrapper, InternalSlot::WrapForValidUnderlying, &Value::Object(*iter_obj));
    slot_set(mc, &wrapper, InternalSlot::WrapForValidNextMethod, &next_method);

    Ok(wrapper)
}

fn handle_wrap_next<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let wrapper = require_object_this(this_val)?;
    let underlying = get_slot_object(&wrapper, &InternalSlot::WrapForValidUnderlying)?;
    let next_method = match slot_get(&wrapper, &InternalSlot::WrapForValidNextMethod) {
        Some(rc) => rc.borrow().clone(),
        None => return Err(raise_type_error!("Invalid WrapForValid state").into()),
    };
    evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(underlying)), &[])
}

fn handle_wrap_return<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let wrapper = require_object_this(this_val)?;
    let underlying = get_slot_object(&wrapper, &InternalSlot::WrapForValidUnderlying)?;

    // Per spec: let returnMethod = GetMethod(iterator, "return")
    let ret_val = crate::core::get_property_with_accessors(mc, env, &underlying, PropertyKey::String("return".to_string()))?;

    if matches!(ret_val, Value::Undefined | Value::Null) || !is_callable(&ret_val) {
        return make_iter_result(mc, env, &Value::Undefined, true);
    }
    // Call returnMethod on iterator and return its result directly
    evaluate_call_dispatch(mc, env, &ret_val, Some(&Value::Object(underlying)), &[])
}

// ===========================================================================
// Iterator.concat (iterator-sequencing)
// ===========================================================================

fn handle_iterator_concat<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Validate all arguments are iterable first
    let iter_sym = get_well_known_symbol(env, "iterator")
        .ok_or_else(|| -> EvalError<'gc> { raise_type_error!("Symbol.iterator not found").into() })?;

    let mut iterables: Vec<Value<'gc>> = Vec::new();
    for arg in args {
        match arg {
            Value::Object(o) => {
                let method = crate::core::get_property_with_accessors(mc, env, o, PropertyKey::Symbol(iter_sym))?;
                if matches!(method, Value::Undefined | Value::Null) {
                    return Err(raise_type_error!("Iterator.concat: argument is not iterable").into());
                }
                if !is_callable(&method) {
                    return Err(raise_type_error!("Iterator.concat: @@iterator method is not callable").into());
                }
                // Store iterable and its iterator method (stride-2: iterable, method)
                iterables.push(arg.clone());
                iterables.push(method);
            }
            _ => return Err(raise_type_error!("Iterator.concat: argument is not an Object").into()),
        }
    }

    // Create a concat iterator helper
    let helper = new_js_object_data(mc);
    if let Some(hp_val) = slot_get_chained(env, &InternalSlot::IteratorHelperPrototype)
        && let Value::Object(hp) = &*hp_val.borrow()
    {
        helper.borrow_mut(mc).prototype = Some(*hp);
    }

    // Store iterables array as a JS array (stride-2: [iterable0, method0, iterable1, method1, ...])
    let iterables_arr = new_js_object_data(mc);
    for (i, it) in iterables.iter().enumerate() {
        object_set_key_value(mc, &iterables_arr, i, it)?;
    }
    object_set_key_value(mc, &iterables_arr, "length", &Value::Number(iterables.len() as f64))?;

    let num_iterables = iterables.len() / 2;

    slot_set(
        mc,
        &helper,
        InternalSlot::IteratorHelperKind,
        &Value::String(utf8_to_utf16("concat")),
    );
    slot_set(mc, &helper, InternalSlot::IteratorHelperUnderlying, &Value::Object(iterables_arr));
    slot_set(mc, &helper, InternalSlot::IteratorHelperCounter, &Value::Number(0.0)); // current iterable index
    slot_set(
        mc,
        &helper,
        InternalSlot::IteratorHelperRemaining,
        &Value::Number(num_iterables as f64),
    ); // count
    slot_set(mc, &helper, InternalSlot::IteratorHelperDone, &Value::Boolean(false));

    Ok(Value::Object(helper))
}

// ===========================================================================
// Iterator.zip (joint-iteration)
// ===========================================================================

/// GetOptionsObject(options): undefined → null-proto {}, Object → use it, else TypeError
fn get_options_object<'gc>(mc: &MutationContext<'gc>, val: &Value<'gc>) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    match val {
        Value::Undefined => {
            let obj = new_js_object_data(mc);
            // null-prototype object
            Ok(obj)
        }
        Value::Object(o) => Ok(*o),
        _ => Err(raise_type_error!("Invalid options argument").into()),
    }
}

/// Parse the `mode` option from options object. Must be a string primitive.
fn parse_zip_mode<'gc>(
    mc: &MutationContext<'gc>,
    options: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<String, EvalError<'gc>> {
    let mode_val = crate::core::get_property_with_accessors(mc, env, options, PropertyKey::String("mode".to_string()))?;
    match &mode_val {
        Value::Undefined => Ok("shortest".to_string()),
        Value::String(s) => {
            let s_utf8 = crate::unicode::utf16_to_utf8(s);
            match s_utf8.as_str() {
                "shortest" | "longest" | "strict" => Ok(s_utf8),
                _ => Err(raise_type_error!("Invalid mode option").into()),
            }
        }
        _ => Err(raise_type_error!("Invalid mode option").into()),
    }
}

/// IteratorCloseAll(iters, completion): close all open iterators in reverse order.
/// If `initial_error` is Some, we already have an error; inner errors are suppressed.
/// If `initial_error` is None, any close error is captured and propagated.
fn iterator_close_all<'gc>(
    mc: &MutationContext<'gc>,
    iterators: &JSObjectDataPtr<'gc>,
    open_flags: &JSObjectDataPtr<'gc>,
    count: usize,
    initial_error: Option<EvalError<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), EvalError<'gc>> {
    let mut error = initial_error;
    // Close in reverse order
    for i in (0..count).rev() {
        // Check if this iterator is still open
        if let Some(flag_ref) = object_get_key_value(open_flags, i)
            && let Value::Boolean(true) = &*flag_ref.borrow()
        {
            // Mark as closed
            let _ = object_set_key_value(mc, open_flags, i, &Value::Boolean(false));
            // Get iterator object
            if let Some(iter_ref) = object_get_key_value(iterators, i)
                && let Value::Object(iter_obj) = &*iter_ref.borrow()
            {
                let iter_obj = *iter_obj;
                // Try .return()
                match crate::core::get_property_with_accessors(mc, env, &iter_obj, PropertyKey::String("return".to_string())) {
                    Ok(ret_val) => {
                        if is_callable(&ret_val) {
                            match evaluate_call_dispatch(mc, env, &ret_val, Some(&Value::Object(iter_obj)), &[]) {
                                Ok(_) => {}
                                Err(e) => {
                                    if error.is_none() {
                                        error = Some(e);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if error.is_none() {
                            error = Some(e);
                        }
                    }
                }
            }
        }
    }
    match error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Close iterators in `iters` (reverse order) + close another "source" iterator
fn if_abrupt_close_iterators<'gc>(
    mc: &MutationContext<'gc>,
    iterators: &JSObjectDataPtr<'gc>,
    count: usize,
    source_iter: Option<&JSObjectDataPtr<'gc>>,
    err: EvalError<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> EvalError<'gc> {
    // Close collected iterators in reverse order, swallowing errors
    for i in (0..count).rev() {
        if let Some(iter_ref) = object_get_key_value(iterators, i)
            && let Value::Object(iter_obj) = &*iter_ref.borrow()
        {
            close_underlying_iterator(mc, iter_obj, env);
        }
    }
    // Close the source iterator if present
    if let Some(src) = source_iter {
        close_underlying_iterator(mc, src, env);
    }
    err
}

fn handle_iterator_zip<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 1: iterables must be an Object
    let iterables_arg = args.first().cloned().unwrap_or(Value::Undefined);
    let iterables_obj = match &iterables_arg {
        Value::Object(o) => *o,
        _ => return Err(raise_type_error!("Iterator.zip requires an iterable object as first argument").into()),
    };

    // Step 2: options = GetOptionsObject
    let options_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
    let options_obj = get_options_object(mc, &options_arg)?;

    // Step 3-5: mode
    let mode = parse_zip_mode(mc, &options_obj, env)?;

    // Step 6-9: padding (only for longest mode)
    let mut padding_arg: Option<Value<'gc>> = None;

    if mode == "longest" {
        let padding_val = crate::core::get_property_with_accessors(mc, env, &options_obj, PropertyKey::String("padding".to_string()))?;
        if !matches!(padding_val, Value::Undefined) {
            match &padding_val {
                Value::Object(_) => {
                    // We'll iterate this after we know iterCount
                    padding_arg = Some(padding_val);
                }
                _ => return Err(raise_type_error!("Iterator.zip: padding must be an object").into()),
            }
        }
    }

    // Step 10-13: Iterate iterables to collect input iterators via GetIteratorFlattenable
    // First get an iterator over the iterables
    let iter_sym = get_well_known_symbol(env, "iterator")
        .ok_or_else(|| -> EvalError<'gc> { raise_type_error!("Symbol.iterator not found").into() })?;

    let method_val = crate::core::get_property_with_accessors(mc, env, &iterables_obj, PropertyKey::Symbol(iter_sym))?;
    if matches!(method_val, Value::Undefined | Value::Null) || !is_callable(&method_val) {
        return Err(raise_type_error!("Iterator.zip: iterables argument is not iterable").into());
    }

    let source_result = evaluate_call_dispatch(mc, env, &method_val, Some(&Value::Object(iterables_obj)), &[])?;
    let source_iter = match &source_result {
        Value::Object(o) => *o,
        _ => return Err(raise_type_error!("Iterator.zip: iterables @@iterator did not return an object").into()),
    };
    let source_next = get_next_method(mc, &source_iter, env)?;

    // Collect iterators
    let iters_arr = new_js_object_data(mc);
    let nexts_arr = new_js_object_data(mc);
    let opens_arr = new_js_object_data(mc);
    let mut iter_count: usize = 0;

    loop {
        let step_result = match evaluate_call_dispatch(mc, env, &source_next, Some(&Value::Object(source_iter)), &[]) {
            Ok(v) => v,
            Err(e) => {
                return Err(if_abrupt_close_iterators(mc, &iters_arr, iter_count, None, e, env));
            }
        };
        if iter_result_done(mc, &step_result, env)? {
            break;
        }
        let element = match iter_result_value(mc, &step_result, env) {
            Ok(v) => v,
            Err(e) => {
                return Err(if_abrupt_close_iterators(mc, &iters_arr, iter_count, Some(&source_iter), e, env));
            }
        };

        // GetIteratorFlattenable(element, reject-strings) — rejects primitives
        let (inner_iter, inner_next) = match get_iterator_flattenable(mc, &element, env) {
            Ok(pair) => pair,
            Err(e) => {
                return Err(if_abrupt_close_iterators(mc, &iters_arr, iter_count, Some(&source_iter), e, env));
            }
        };

        object_set_key_value(mc, &iters_arr, iter_count, &Value::Object(inner_iter))?;
        object_set_key_value(mc, &nexts_arr, iter_count, &inner_next)?;
        object_set_key_value(mc, &opens_arr, iter_count, &Value::Boolean(true))?;
        iter_count += 1;
    }

    // Now handle padding for longest mode
    let padding_arr = new_js_object_data(mc);
    if mode == "longest" {
        if let Some(pad_val) = padding_arg {
            // Get iterator for padding
            let pad_iter_result = crate::js_generator::public_get_iterator(mc, &pad_val, env);
            match pad_iter_result {
                Ok(pad_iter) => {
                    let pad_next = match get_next_method(mc, &pad_iter, env) {
                        Ok(n) => n,
                        Err(e) => {
                            // Close all collected iterators
                            for i in (0..iter_count).rev() {
                                if let Some(ir) = object_get_key_value(&iters_arr, i)
                                    && let Value::Object(it) = &*ir.borrow()
                                {
                                    close_underlying_iterator(mc, it, env);
                                }
                            }
                            return Err(e);
                        }
                    };

                    // Iterate padding up to iter_count times
                    let mut pad_exhausted = false;
                    for i in 0..iter_count {
                        if pad_exhausted {
                            object_set_key_value(mc, &padding_arr, i, &Value::Undefined)?;
                            continue;
                        }
                        let pad_step = match evaluate_call_dispatch(mc, env, &pad_next, Some(&Value::Object(pad_iter)), &[]) {
                            Ok(v) => v,
                            Err(e) => {
                                // IfAbruptCloseIterators
                                for j in (0..iter_count).rev() {
                                    if let Some(ir) = object_get_key_value(&iters_arr, j)
                                        && let Value::Object(it) = &*ir.borrow()
                                    {
                                        close_underlying_iterator(mc, it, env);
                                    }
                                }
                                return Err(e);
                            }
                        };
                        let pad_done = match iter_result_done(mc, &pad_step, env) {
                            Ok(d) => d,
                            Err(e) => {
                                for j in (0..iter_count).rev() {
                                    if let Some(ir) = object_get_key_value(&iters_arr, j)
                                        && let Value::Object(it) = &*ir.borrow()
                                    {
                                        close_underlying_iterator(mc, it, env);
                                    }
                                }
                                return Err(e);
                            }
                        };
                        if pad_done {
                            pad_exhausted = true;
                            object_set_key_value(mc, &padding_arr, i, &Value::Undefined)?;
                        } else {
                            let pad_val_item = match iter_result_value(mc, &pad_step, env) {
                                Ok(v) => v,
                                Err(e) => {
                                    for j in (0..iter_count).rev() {
                                        if let Some(ir) = object_get_key_value(&iters_arr, j)
                                            && let Value::Object(it) = &*ir.borrow()
                                        {
                                            close_underlying_iterator(mc, it, env);
                                        }
                                    }
                                    return Err(e);
                                }
                            };
                            object_set_key_value(mc, &padding_arr, i, &pad_val_item)?;
                        }
                    }
                    // If padding not exhausted, close it
                    if !pad_exhausted {
                        match close_underlying_iterator_throwing(mc, &pad_iter, env) {
                            Ok(_) => {}
                            Err(e) => {
                                // IfAbruptCloseIterators
                                for j in (0..iter_count).rev() {
                                    if let Some(ir) = object_get_key_value(&iters_arr, j)
                                        && let Value::Object(it) = &*ir.borrow()
                                    {
                                        close_underlying_iterator(mc, it, env);
                                    }
                                }
                                return Err(e);
                            }
                        }
                    }
                }
                Err(e) => {
                    // IfAbruptCloseIterators
                    for i in (0..iter_count).rev() {
                        if let Some(ir) = object_get_key_value(&iters_arr, i)
                            && let Value::Object(it) = &*ir.borrow()
                        {
                            close_underlying_iterator(mc, it, env);
                        }
                    }
                    return Err(e);
                }
            }
        } else {
            // No padding specified → fill with undefined
            for i in 0..iter_count {
                object_set_key_value(mc, &padding_arr, i, &Value::Undefined)?;
            }
        }
    }

    // Create the zip iterator helper
    let helper = new_js_object_data(mc);
    if let Some(hp_val) = slot_get_chained(env, &InternalSlot::IteratorHelperPrototype)
        && let Value::Object(hp) = &*hp_val.borrow()
    {
        helper.borrow_mut(mc).prototype = Some(*hp);
    }

    slot_set(mc, &helper, InternalSlot::IteratorHelperKind, &Value::String(utf8_to_utf16("zip")));
    slot_set(mc, &helper, InternalSlot::ZipIterators, &Value::Object(iters_arr));
    slot_set(mc, &helper, InternalSlot::ZipNextMethods, &Value::Object(nexts_arr));
    slot_set(mc, &helper, InternalSlot::ZipOpenFlags, &Value::Object(opens_arr));
    slot_set(mc, &helper, InternalSlot::ZipMode, &Value::String(utf8_to_utf16(&mode)));
    slot_set(mc, &helper, InternalSlot::ZipPadding, &Value::Object(padding_arr));
    slot_set(
        mc,
        &helper,
        InternalSlot::IteratorHelperRemaining,
        &Value::Number(iter_count as f64),
    );
    slot_set(mc, &helper, InternalSlot::IteratorHelperDone, &Value::Boolean(false));
    slot_set(mc, &helper, InternalSlot::IteratorHelperExecuting, &Value::Boolean(false));
    slot_set(mc, &helper, InternalSlot::IteratorHelperStarted, &Value::Boolean(false));

    Ok(Value::Object(helper))
}

fn handle_iterator_zip_keyed<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 1: iterables must be an Object
    let iterables_arg = args.first().cloned().unwrap_or(Value::Undefined);
    let iterables_obj = match &iterables_arg {
        Value::Object(o) => *o,
        _ => return Err(raise_type_error!("Iterator.zipKeyed requires an object as first argument").into()),
    };

    // Step 2: options = GetOptionsObject
    let options_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
    let options_obj = get_options_object(mc, &options_arg)?;

    // Step 3-5: mode
    let mode = parse_zip_mode(mc, &options_obj, env)?;

    // Step 6-9: padding (only for longest mode) — for zipKeyed, padding is an object, not iterable
    let padding_obj: Option<JSObjectDataPtr<'gc>> = if mode == "longest" {
        let padding_val = crate::core::get_property_with_accessors(mc, env, &options_obj, PropertyKey::String("padding".to_string()))?;
        match &padding_val {
            Value::Undefined => None,
            Value::Object(o) => Some(*o),
            _ => return Err(raise_type_error!("Iterator.zipKeyed: padding must be an object").into()),
        }
    } else {
        None
    };

    // Step 10: allKeys = iterables.[[OwnPropertyKeys]]()
    let all_keys = crate::core::ordinary_own_property_keys_mc(mc, &iterables_obj)?;

    let iters_arr = new_js_object_data(mc);
    let nexts_arr = new_js_object_data(mc);
    let opens_arr = new_js_object_data(mc);
    let keys_arr = new_js_object_data(mc);
    let padding_arr = new_js_object_data(mc);
    let mut iter_count: usize = 0;

    // Check if iterables is a Proxy
    let is_proxy = slot_get(&iterables_obj, &InternalSlot::Proxy).is_some();

    // Step 12: For each key of allKeys...
    for key in &all_keys {
        // Step 12a: desc = iterables.[[GetOwnProperty]](key)
        // Step 12b: IfAbruptCloseIterators(desc, iters)
        let desc_result = if is_proxy {
            if let Some(proxy_cell) = slot_get(&iterables_obj, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                crate::js_proxy::proxy_get_own_property_is_enumerable(mc, proxy, key)
            } else {
                Ok(None) // shouldn't happen
            }
        } else {
            // For regular objects: check if own property exists and its enumerability
            if crate::core::get_own_property(&iterables_obj, key.clone()).is_some() {
                Ok(Some(iterables_obj.borrow().is_enumerable(key.clone())))
            } else {
                Ok(None)
            }
        };

        let desc_enum_opt = match desc_result {
            Ok(v) => v,
            Err(e) => {
                // IfAbruptCloseIterators: close all collected iterators in reverse
                let opens_tmp = new_js_object_data(mc);
                for j in 0..iter_count {
                    let _ = object_set_key_value(mc, &opens_tmp, j, &Value::Boolean(true));
                }
                return Err(iterator_close_all(mc, &iters_arr, &opens_tmp, iter_count, Some(e), env).unwrap_err());
            }
        };

        // Step 12c: If desc is not undefined and desc.[[Enumerable]] is true
        match desc_enum_opt {
            None => continue,        // desc is undefined — skip
            Some(false) => continue, // not enumerable — skip
            Some(true) => {}         // enumerable — proceed
        }

        // Step 12c.i: value = Get(iterables, key)
        // Step 12c.ii: IfAbruptCloseIterators(value, iters)
        let val = if is_proxy {
            if let Some(proxy_cell) = slot_get(&iterables_obj, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                match crate::js_proxy::proxy_get_property(mc, proxy, key) {
                    Ok(v) => v.unwrap_or(Value::Undefined),
                    Err(e) => {
                        let opens_tmp = new_js_object_data(mc);
                        for j in 0..iter_count {
                            let _ = object_set_key_value(mc, &opens_tmp, j, &Value::Boolean(true));
                        }
                        return Err(iterator_close_all(mc, &iters_arr, &opens_tmp, iter_count, Some(e), env).unwrap_err());
                    }
                }
            } else {
                Value::Undefined
            }
        } else {
            match crate::core::get_property_with_accessors(mc, env, &iterables_obj, key.clone()) {
                Ok(v) => v,
                Err(e) => {
                    let opens_tmp = new_js_object_data(mc);
                    for j in 0..iter_count {
                        let _ = object_set_key_value(mc, &opens_tmp, j, &Value::Boolean(true));
                    }
                    return Err(iterator_close_all(mc, &iters_arr, &opens_tmp, iter_count, Some(e), env).unwrap_err());
                }
            }
        };

        // Step 12c.iii: If value is undefined, skip this key
        if matches!(val, Value::Undefined) {
            continue;
        }

        // Step 12c.iv: GetIteratorFlattenable(value, reject-strings)
        // IfAbruptCloseIterators(innerIterator, iters)
        let (inner_iter, inner_next) = match get_iterator_flattenable(mc, &val, env) {
            Ok(pair) => pair,
            Err(e) => {
                let opens_tmp = new_js_object_data(mc);
                for j in 0..iter_count {
                    let _ = object_set_key_value(mc, &opens_tmp, j, &Value::Boolean(true));
                }
                return Err(iterator_close_all(mc, &iters_arr, &opens_tmp, iter_count, Some(e), env).unwrap_err());
            }
        };

        // Step 12c.v-vi: Append key and iterator
        // Store key as Value for later use in result building
        let key_val = match key {
            PropertyKey::String(s) => Value::String(utf8_to_utf16(s)),
            PropertyKey::Symbol(sym) => Value::Symbol(*sym),
            _ => continue, // Private/Internal keys — shouldn't appear
        };
        object_set_key_value(mc, &keys_arr, iter_count, &key_val)?;
        object_set_key_value(mc, &iters_arr, iter_count, &Value::Object(inner_iter))?;
        object_set_key_value(mc, &nexts_arr, iter_count, &inner_next)?;
        object_set_key_value(mc, &opens_arr, iter_count, &Value::Boolean(true))?;

        // Padding placeholder (filled later if mode is longest)
        object_set_key_value(mc, &padding_arr, iter_count, &Value::Undefined)?;

        iter_count += 1;
    }

    // Step 14: If mode is "longest", get padding values for each collected key
    if mode == "longest"
        && let Some(ref po) = padding_obj
    {
        for i in 0..iter_count {
            if let Some(key_ref) = object_get_key_value(&keys_arr, i) {
                let key_val = key_ref.borrow().clone();
                let pad_key = match &key_val {
                    Value::String(s) => PropertyKey::String(crate::unicode::utf16_to_utf8(s)),
                    Value::Symbol(sym) => PropertyKey::Symbol(*sym),
                    _ => continue,
                };
                let pad_val = match crate::core::get_property_with_accessors(mc, env, po, pad_key) {
                    Ok(v) => v,
                    Err(e) => {
                        // IfAbruptCloseIterators
                        let opens_tmp = new_js_object_data(mc);
                        for j in 0..iter_count {
                            let _ = object_set_key_value(mc, &opens_tmp, j, &Value::Boolean(true));
                        }
                        return Err(iterator_close_all(mc, &iters_arr, &opens_tmp, iter_count, Some(e), env).unwrap_err());
                    }
                };
                let _ = object_set_key_value(mc, &padding_arr, i, &pad_val);
            }
        }
    }

    // Create the zipKeyed iterator helper
    let helper = new_js_object_data(mc);
    if let Some(hp_val) = slot_get_chained(env, &InternalSlot::IteratorHelperPrototype)
        && let Value::Object(hp) = &*hp_val.borrow()
    {
        helper.borrow_mut(mc).prototype = Some(*hp);
    }

    slot_set(
        mc,
        &helper,
        InternalSlot::IteratorHelperKind,
        &Value::String(utf8_to_utf16("zipKeyed")),
    );
    slot_set(mc, &helper, InternalSlot::ZipIterators, &Value::Object(iters_arr));
    slot_set(mc, &helper, InternalSlot::ZipNextMethods, &Value::Object(nexts_arr));
    slot_set(mc, &helper, InternalSlot::ZipOpenFlags, &Value::Object(opens_arr));
    slot_set(mc, &helper, InternalSlot::ZipMode, &Value::String(utf8_to_utf16(&mode)));
    slot_set(mc, &helper, InternalSlot::ZipPadding, &Value::Object(padding_arr));
    slot_set(mc, &helper, InternalSlot::ZipKeys, &Value::Object(keys_arr));
    slot_set(
        mc,
        &helper,
        InternalSlot::IteratorHelperRemaining,
        &Value::Number(iter_count as f64),
    );
    slot_set(mc, &helper, InternalSlot::IteratorHelperDone, &Value::Boolean(false));
    slot_set(mc, &helper, InternalSlot::IteratorHelperExecuting, &Value::Boolean(false));
    slot_set(mc, &helper, InternalSlot::IteratorHelperStarted, &Value::Boolean(false));

    Ok(Value::Object(helper))
}

// ===========================================================================
// IteratorZip .next() for both zip and zipKeyed
// ===========================================================================

fn helper_next_zip<'gc>(
    mc: &MutationContext<'gc>,
    helper: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
    is_keyed: bool,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let iters = get_slot_object(helper, &InternalSlot::ZipIterators)?;
    let nexts = get_slot_object(helper, &InternalSlot::ZipNextMethods)?;
    let opens = get_slot_object(helper, &InternalSlot::ZipOpenFlags)?;
    let padding = get_slot_object(helper, &InternalSlot::ZipPadding)?;

    let mode_str = get_slot_string(helper, &InternalSlot::ZipMode);
    let iter_count = get_remaining(helper) as usize;

    let keys = if is_keyed {
        Some(get_slot_object(helper, &InternalSlot::ZipKeys)?)
    } else {
        None
    };

    // Edge case: if there are 0 iterators, immediately return done.
    if iter_count == 0 {
        mark_done(mc, helper);
        return make_iter_result(mc, env, &Value::Undefined, true);
    }

    // Collect results for this round
    let mut results: Vec<Value<'gc>> = Vec::with_capacity(iter_count);
    let mut any_done_this_round = false;
    let mut all_done_this_round = true;
    let mut first_done_this_round = false; // for strict mode: was iterator 0 done?

    for i in 0..iter_count {
        let is_open = object_get_key_value(&opens, i)
            .map(|r| matches!(&*r.borrow(), Value::Boolean(true)))
            .unwrap_or(false);

        if !is_open {
            // Already exhausted from a previous round — use padding value (longest mode)
            let pad = object_get_key_value(&padding, i)
                .map(|r| r.borrow().clone())
                .unwrap_or(Value::Undefined);
            results.push(pad);
            any_done_this_round = true;
            continue;
        }

        // Get iterator and next method for this index
        let iter_obj = match object_get_key_value(&iters, i) {
            Some(r) => match &*r.borrow() {
                Value::Object(o) => *o,
                _ => return Err(raise_type_error!("Invalid zip iterator state").into()),
            },
            None => return Err(raise_type_error!("Invalid zip iterator state").into()),
        };
        let next_method = match object_get_key_value(&nexts, i) {
            Some(r) => r.borrow().clone(),
            None => return Err(raise_type_error!("Invalid zip iterator state").into()),
        };

        // Call .next()
        let step_result = match evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[]) {
            Ok(v) => v,
            Err(e) => {
                // Remove this iterator from open (it faulted)
                let _ = object_set_key_value(mc, &opens, i, &Value::Boolean(false));
                // IteratorCloseAll remaining open iterators
                return Err(iterator_close_all(mc, &iters, &opens, iter_count, Some(e), env).unwrap_err());
            }
        };

        let is_done = match iter_result_done(mc, &step_result, env) {
            Ok(d) => d,
            Err(e) => {
                let _ = object_set_key_value(mc, &opens, i, &Value::Boolean(false));
                return Err(iterator_close_all(mc, &iters, &opens, iter_count, Some(e), env).unwrap_err());
            }
        };

        if is_done {
            // Mark as closed
            let _ = object_set_key_value(mc, &opens, i, &Value::Boolean(false));
            any_done_this_round = true;
            if i == 0 {
                first_done_this_round = true;
            }

            match mode_str.as_str() {
                "shortest" => {
                    // Close all other open iterators and return done
                    mark_done(mc, helper);
                    iterator_close_all(mc, &iters, &opens, iter_count, None, env)?;
                    return make_iter_result(mc, env, &Value::Undefined, true);
                }
                "longest" => {
                    // Use padding value, continue collecting
                    let pad = object_get_key_value(&padding, i)
                        .map(|r| r.borrow().clone())
                        .unwrap_or(Value::Undefined);
                    results.push(pad);
                    continue;
                }
                "strict" => {
                    if i == 0 {
                        // First iterator done → we need to check all others
                        results.push(Value::Undefined); // placeholder, won't be used
                        continue;
                    } else {
                        // i > 0 is done. Check if it is consistent with iterator 0.
                        if !first_done_this_round {
                            // Iterator 0 was NOT done, but this one IS → length mismatch
                            mark_done(mc, helper);
                            let err: EvalError<'gc> =
                                raise_type_error!("Iterator.zip: iterators have different lengths (strict mode)").into();
                            return Err(iterator_close_all(mc, &iters, &opens, iter_count, Some(err), env).unwrap_err());
                        }
                        // Both iterator 0 and this one are done → consistent, continue
                        results.push(Value::Undefined);
                        continue;
                    }
                }
                _ => unreachable!(),
            }
        } else {
            all_done_this_round = false;
            // Get value
            let value = match iter_result_value(mc, &step_result, env) {
                Ok(v) => v,
                Err(e) => {
                    let _ = object_set_key_value(mc, &opens, i, &Value::Boolean(false));
                    return Err(iterator_close_all(mc, &iters, &opens, iter_count, Some(e), env).unwrap_err());
                }
            };

            if mode_str == "strict" && first_done_this_round {
                // Iterator 0 was done but this one is NOT → length mismatch
                // This iterator is still open (not done), so it's in openIters. Close all.
                mark_done(mc, helper);
                let err: EvalError<'gc> = raise_type_error!("Iterator.zip: iterators have different lengths (strict mode)").into();
                return Err(iterator_close_all(mc, &iters, &opens, iter_count, Some(err), env).unwrap_err());
            }

            results.push(value);
        }
    }

    // Post-loop checks for strict mode: if first was done and all were done → OK
    // (This case has already been handled above since we'd have returned on mismatch)

    // Check if all done (for longest mode) — return done without yielding results
    if mode_str == "longest" && any_done_this_round {
        let mut all_closed = true;
        for i in 0..iter_count {
            if let Some(flag_ref) = object_get_key_value(&opens, i)
                && let Value::Boolean(true) = &*flag_ref.borrow()
            {
                all_closed = false;
                break;
            }
        }
        if all_closed {
            mark_done(mc, helper);
            return make_iter_result(mc, env, &Value::Undefined, true);
        }
    }

    // For strict mode when all done on the same round → mark done
    if mode_str == "strict" && first_done_this_round && all_done_this_round {
        mark_done(mc, helper);
        return make_iter_result(mc, env, &Value::Undefined, true);
    }

    // Build the result value
    let result_val = if is_keyed {
        // Create a null-prototype object with keys (string or symbol)
        let obj = new_js_object_data(mc);
        // null prototype: leave obj.prototype as None
        obj.borrow_mut(mc).prototype = None;
        let keys_obj = keys.unwrap();
        for (i, results_i) in results.iter().enumerate().take(iter_count) {
            if let Some(key_ref) = object_get_key_value(&keys_obj, i) {
                let key_val = key_ref.borrow().clone();
                match &key_val {
                    Value::String(key_s) => {
                        let key_utf8 = crate::unicode::utf16_to_utf8(key_s);
                        object_set_key_value(mc, &obj, &*key_utf8, results_i)?;
                    }
                    Value::Symbol(sym) => {
                        object_set_key_value(mc, &obj, *sym, results_i)?;
                    }
                    _ => {}
                }
            }
        }
        Value::Object(obj)
    } else {
        // Create an array
        let arr = crate::js_array::create_array(mc, env)?;
        for (i, val) in results.iter().enumerate() {
            object_set_key_value(mc, &arr, i, val)?;
        }
        object_set_key_value(mc, &arr, "length", &Value::Number(results.len() as f64))?;
        Value::Object(arr)
    };

    make_iter_result(mc, env, &result_val, false)
}

// ===========================================================================
// concat helper next (dispatched from handle_helper_next via "concat" kind)
// ===========================================================================

fn helper_next_concat<'gc>(
    mc: &MutationContext<'gc>,
    helper: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let iterables_obj = get_slot_object(helper, &InternalSlot::IteratorHelperUnderlying)?;
    let total = get_remaining(helper);
    let mut idx = get_counter(helper);

    loop {
        if idx >= total {
            mark_done(mc, helper);
            return make_iter_result(mc, env, &Value::Undefined, true);
        }

        // Check if we have a current inner iterator saved
        if let Some(inner_rc) = slot_get(helper, &InternalSlot::IteratorHelperInnerIter)
            && !matches!(&*inner_rc.borrow(), Value::Undefined | Value::Null)
        {
            let inner = match &*inner_rc.borrow() {
                Value::Object(o) => *o,
                _ => {
                    slot_set(mc, helper, InternalSlot::IteratorHelperInnerIter, &Value::Undefined);
                    continue;
                }
            };
            let inner_next = match slot_get(helper, &InternalSlot::IteratorHelperInnerNext) {
                Some(rc) => rc.borrow().clone(),
                None => {
                    slot_set(mc, helper, InternalSlot::IteratorHelperInnerIter, &Value::Undefined);
                    continue;
                }
            };

            let result = evaluate_call_dispatch(mc, env, &inner_next, Some(&Value::Object(inner)), &[])?;
            if !iter_result_done(mc, &result, env)? {
                let value = iter_result_value(mc, &result, env)?;
                return make_iter_result(mc, env, &value, false);
            }
            // Inner exhausted: move to next iterable
            slot_set(mc, helper, InternalSlot::IteratorHelperInnerIter, &Value::Undefined);
            slot_set(mc, helper, InternalSlot::IteratorHelperInnerNext, &Value::Undefined);
            idx += 1.0;
            slot_set(mc, helper, InternalSlot::IteratorHelperCounter, &Value::Number(idx));
            continue;
        }

        // No inner iterator; open the next iterable using stored method
        // The iterables array is stride-2: [iterable0, method0, iterable1, method1, ...]
        let arr_idx = (idx as usize) * 2;
        let iterable_val = match object_get_key_value(&iterables_obj, arr_idx) {
            Some(rc) => rc.borrow().clone(),
            None => {
                mark_done(mc, helper);
                return make_iter_result(mc, env, &Value::Undefined, true);
            }
        };
        let stored_method = match object_get_key_value(&iterables_obj, arr_idx + 1) {
            Some(rc) => rc.borrow().clone(),
            None => {
                mark_done(mc, helper);
                return make_iter_result(mc, env, &Value::Undefined, true);
            }
        };

        // Call the stored @@iterator method on the iterable
        let iter_result = evaluate_call_dispatch(mc, env, &stored_method, Some(&iterable_val), &[])?;
        let inner_iter = match iter_result {
            Value::Object(o) => o,
            _ => return Err(raise_type_error!("Iterator.concat: @@iterator did not return an object").into()),
        };
        let inner_next = get_next_method(mc, &inner_iter, env)?;
        slot_set(mc, helper, InternalSlot::IteratorHelperInnerIter, &Value::Object(inner_iter));
        slot_set(mc, helper, InternalSlot::IteratorHelperInnerNext, &inner_next);
    }
}

// ===========================================================================
// Utility helpers
// ===========================================================================

fn get_func_proto<'gc>(env: &JSObjectDataPtr<'gc>) -> Option<JSObjectDataPtr<'gc>> {
    if let Some(fc) = env_get(env, "Function")
        && let Value::Object(fc_obj) = &*fc.borrow()
        && let Some(pv) = object_get_key_value(fc_obj, "prototype")
        && let Value::Object(fp) = &*pv.borrow()
    {
        Some(*fp)
    } else {
        None
    }
}

pub fn get_well_known_symbol<'gc>(env: &JSObjectDataPtr<'gc>, name: &str) -> Option<gc_arena::Gc<'gc, crate::core::SymbolData>> {
    if let Some(sc) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sc.borrow()
        && let Some(sv) = object_get_key_value(sym_ctor, name)
        && let Value::Symbol(s) = &*sv.borrow()
    {
        Some(*s)
    } else {
        None
    }
}

fn accessor_descriptor<'gc>(
    mc: &MutationContext<'gc>,
    getter: &JSObjectDataPtr<'gc>,
    setter: &JSObjectDataPtr<'gc>,
    enumerable: bool,
    configurable: bool,
) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    let desc = new_js_object_data(mc);
    object_set_key_value(mc, &desc, "get", &Value::Object(*getter))?;
    object_set_key_value(mc, &desc, "set", &Value::Object(*setter))?;
    object_set_key_value(mc, &desc, "enumerable", &Value::Boolean(enumerable))?;
    object_set_key_value(mc, &desc, "configurable", &Value::Boolean(configurable))?;
    Ok(desc)
}

fn require_object_this<'gc>(this_val: Option<&Value<'gc>>) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    match this_val {
        Some(Value::Object(o)) => Ok(*o),
        _ => Err(raise_type_error!("Iterator method requires an object receiver").into()),
    }
}

fn is_callable<'gc>(val: &Value<'gc>) -> bool {
    match val {
        Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) => true,
        Value::Object(obj) => {
            // Check if object is actually callable
            if obj.borrow().class_def.is_some() {
                return true;
            }
            if obj.borrow().get_closure().is_some() {
                return true;
            }
            if let Some(is_ctor) = slot_get_chained(obj, &InternalSlot::IsConstructor)
                && matches!(*is_ctor.borrow(), Value::Boolean(true))
            {
                return true;
            }
            if let Some(callable) = slot_get_chained(obj, &InternalSlot::Callable)
                && matches!(*callable.borrow(), Value::Boolean(true))
            {
                return true;
            }
            if slot_get_chained(obj, &InternalSlot::NativeCtor).is_some() {
                return true;
            }
            false
        }
        _ => false,
    }
}

fn get_next_method<'gc>(
    mc: &MutationContext<'gc>,
    iter_obj: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Per spec GetIteratorDirect: Let nextMethod be ? Get(obj, "next").
    // No callability check here — the check happens when the next method is actually called.
    crate::core::get_property_with_accessors(mc, env, iter_obj, PropertyKey::String("next".to_string()))
}

fn get_iterator_from_value<'gc>(
    mc: &MutationContext<'gc>,
    val: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    // Use the engine's built-in get_iterator (via Symbol.iterator)
    crate::js_generator::public_get_iterator(mc, val, env)
}

/// GetIteratorFlattenable(obj, reject-primitives)
/// Per spec: rejects primitives, uses GetMethod for Symbol.iterator,
/// falls back to using the object itself as iterator if method is undefined.
fn get_iterator_flattenable<'gc>(
    mc: &MutationContext<'gc>,
    val: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(JSObjectDataPtr<'gc>, Value<'gc>), EvalError<'gc>> {
    // 1. If obj is not an Object, throw TypeError
    let obj = match val {
        Value::Object(o) => *o,
        _ => return Err(raise_type_error!("Iterator.prototype.flatMap mapper must return an Object").into()),
    };

    // 2. Let method = GetMethod(obj, @@iterator)
    //    GetMethod: get property, if null/undefined return undefined, else check callable
    let method_val = if let Some(sym) = get_well_known_symbol(env, "iterator") {
        crate::core::get_property_with_accessors(mc, env, &obj, PropertyKey::Symbol(sym))?
    } else {
        Value::Undefined
    };

    let method = match &method_val {
        Value::Undefined | Value::Null => None,
        other => {
            if !is_callable(other) {
                return Err(raise_type_error!("Symbol.iterator is not a function").into());
            }
            Some(method_val.clone())
        }
    };

    if let Some(m) = method {
        // 4. Call method on obj, result must be an Object
        let iter_result = evaluate_call_dispatch(mc, env, &m, Some(&Value::Object(obj)), &[])?;
        let iter_obj = match iter_result {
            Value::Object(o) => o,
            _ => return Err(raise_type_error!("Symbol.iterator must return an object").into()),
        };
        let next_method = get_next_method(mc, &iter_obj, env)?;
        Ok((iter_obj, next_method))
    } else {
        // 3. method is undefined → use obj itself as the iterator
        let next_method = get_next_method(mc, &obj, env)?;
        Ok((obj, next_method))
    }
}

fn iter_result_done<'gc>(mc: &MutationContext<'gc>, result: &Value<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<bool, EvalError<'gc>> {
    match result {
        Value::Object(o) => {
            let done_val = crate::core::get_property_with_accessors(mc, env, o, PropertyKey::String("done".to_string()))?;
            Ok(to_boolean(&done_val))
        }
        _ => Err(raise_type_error!("Iterator result is not an object").into()),
    }
}

fn iter_result_value<'gc>(
    mc: &MutationContext<'gc>,
    result: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match result {
        Value::Object(o) => {
            let val = crate::core::get_property_with_accessors(mc, env, o, PropertyKey::String("value".to_string()))?;
            Ok(val)
        }
        _ => Err(raise_type_error!("Iterator result is not an object").into()),
    }
}

fn get_global_env_helpers<'gc>(env: &JSObjectDataPtr<'gc>) -> JSObjectDataPtr<'gc> {
    let mut global_env = *env;
    loop {
        if global_env
            .borrow()
            .properties
            .contains_key(&crate::core::PropertyKey::String("globalThis".to_string()))
        {
            break;
        }
        let proto = global_env.borrow().prototype;
        if let Some(p) = proto {
            global_env = p;
        } else {
            break;
        }
    }
    global_env
}

fn make_iter_result<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    value: &Value<'gc>,
    done: bool,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = new_js_object_data(mc);
    let global_env = get_global_env_helpers(env);
    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, &global_env, "Object");
    object_set_key_value(mc, &obj, "value", value)?;
    object_set_key_value(mc, &obj, "done", &Value::Boolean(done))?;
    Ok(Value::Object(obj))
}

fn to_boolean(val: &Value<'_>) -> bool {
    match val {
        Value::Boolean(b) => *b,
        Value::Undefined | Value::Null => false,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::BigInt(b) => !num_traits::Zero::is_zero(&**b),
        _ => true,
    }
}

fn get_counter(obj: &JSObjectDataPtr<'_>) -> f64 {
    slot_get(obj, &InternalSlot::IteratorHelperCounter)
        .and_then(|rc| match &*rc.borrow() {
            Value::Number(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn inc_counter<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>) {
    let c = get_counter(obj) + 1.0;
    slot_set(mc, obj, InternalSlot::IteratorHelperCounter, &Value::Number(c));
}

fn get_remaining(obj: &JSObjectDataPtr<'_>) -> f64 {
    slot_get(obj, &InternalSlot::IteratorHelperRemaining)
        .and_then(|rc| match &*rc.borrow() {
            Value::Number(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn dec_remaining<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>) {
    let r = get_remaining(obj) - 1.0;
    slot_set(mc, obj, InternalSlot::IteratorHelperRemaining, &Value::Number(r));
}

fn mark_done<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>) {
    slot_set(mc, obj, InternalSlot::IteratorHelperDone, &Value::Boolean(true));
}

fn get_slot_string(obj: &JSObjectDataPtr<'_>, slot: &InternalSlot) -> String {
    slot_get(obj, slot)
        .and_then(|rc| match &*rc.borrow() {
            Value::String(s) => Some(crate::unicode::utf16_to_utf8(s)),
            _ => None,
        })
        .unwrap_or_default()
}

fn get_slot_object<'gc>(obj: &JSObjectDataPtr<'gc>, slot: &InternalSlot) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    match slot_get(obj, slot) {
        Some(rc) => match &*rc.borrow() {
            Value::Object(o) => Ok(*o),
            _ => Err(raise_type_error!("Expected object in iterator helper slot").into()),
        },
        None => Err(raise_type_error!("Iterator helper slot not found").into()),
    }
}

fn close_underlying_iterator<'gc>(mc: &MutationContext<'gc>, iter_obj: &JSObjectDataPtr<'gc>, env: &JSObjectDataPtr<'gc>) {
    // Try to call .return() if it exists (traverse prototype chain) — swallow errors
    if let Ok(ret) = crate::core::get_property_with_accessors(mc, env, iter_obj, PropertyKey::String("return".to_string()))
        && is_callable(&ret)
    {
        let _ = evaluate_call_dispatch(mc, env, &ret, Some(&Value::Object(*iter_obj)), &[]);
    }
}

/// Like close_underlying_iterator but propagates errors from getting or calling .return()
fn close_underlying_iterator_throwing<'gc>(
    mc: &MutationContext<'gc>,
    iter_obj: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), EvalError<'gc>> {
    // Per spec IteratorClose: Get the return method. If Get throws, propagate.
    let ret = crate::core::get_property_with_accessors(mc, env, iter_obj, PropertyKey::String("return".to_string()))?;
    // If return method is undefined or null, return normally
    if matches!(ret, Value::Undefined | Value::Null) {
        return Ok(());
    }
    // Call .return(). If it throws, propagate.
    evaluate_call_dispatch(mc, env, &ret, Some(&Value::Object(*iter_obj)), &[])?;
    Ok(())
}

fn is_instance_of_iterator<'gc>(obj: &JSObjectDataPtr<'gc>, env: &JSObjectDataPtr<'gc>) -> bool {
    // Walk prototype chain looking for %IteratorPrototype%
    let iter_proto_ptr = match slot_get_chained(env, &InternalSlot::IteratorPrototype) {
        Some(rc) => match &*rc.borrow() {
            Value::Object(o) => Gc::as_ptr(*o),
            _ => return false,
        },
        None => return false,
    };

    let mut current = Some(*obj);
    while let Some(c) = current {
        if Gc::as_ptr(c) == iter_proto_ptr {
            return true;
        }
        current = c.borrow().prototype;
    }
    false
}
