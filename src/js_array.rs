use crate::core::{
    ClosureData, Gc, InternalSlot, Value, new_gc_cell_ptr, object_get_key_value, object_set_key_value, slot_get, slot_get_chained,
    slot_set, value_to_sort_string, values_equal,
};
use crate::core::{MutationContext, object_get_length, object_set_length};
use crate::js_proxy::proxy_set_property_with_receiver;
use crate::{
    core::{EvalError, JSObjectDataPtr, PropertyKey, env_get, env_set, evaluate_call_dispatch, new_js_object_data},
    error::JSError,
    unicode::{utf8_to_utf16, utf16_to_utf8},
};

/// CreateDataPropertyOrThrow(O, P, V) — defines P on O with
/// {value: V, writable: true, enumerable: true, configurable: true}.
/// Throws TypeError if the define fails (non-extensible or non-configurable property).
fn create_data_property_or_throw<'gc>(
    mc: &MutationContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: impl Into<PropertyKey<'gc>>,
    val: &Value<'gc>,
) -> Result<(), EvalError<'gc>> {
    let key: PropertyKey<'gc> = key.into();
    // If obj is a Proxy, invoke the [[DefineOwnProperty]] trap (CreateDataPropertyOrThrow spec step)
    if let Some(proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
    {
        let ok = crate::js_proxy::proxy_define_data_property(mc, proxy, &key, val)?;
        if !ok {
            return Err(raise_type_error!("Cannot define property on proxy").into());
        }
        return Ok(());
    }
    let desc = crate::core::create_descriptor_object(mc, val, true, true, true).map_err(EvalError::from)?;
    crate::js_object::define_property_internal(mc, obj, key, &desc).map_err(EvalError::from)
}

/// Checks whether a value is a constructor (can be called with `new`).
fn is_constructor_val<'gc>(v: &Value<'gc>) -> bool {
    match v {
        Value::Object(obj) => {
            obj.borrow().class_def.is_some()
                || crate::core::slot_get_chained(obj, &crate::core::InternalSlot::IsConstructor).is_some()
                || crate::core::slot_get_chained(obj, &crate::core::InternalSlot::NativeCtor).is_some()
                || obj.borrow().get_closure().is_some()
        }
        Value::Closure(cl) | Value::AsyncClosure(cl) => !cl.is_arrow,
        _ => false,
    }
}

/// IsArray(argument) — spec 7.2.2, with recursive Proxy support.
/// Returns true if argument is an Array (directly or through proxy chain).
fn is_array_spec<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>) -> Result<bool, EvalError<'gc>> {
    // Direct array?
    if is_array(mc, obj) {
        return Ok(true);
    }
    // Proxy exotic object — recurse into target
    if let Some(proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
    {
        if proxy.revoked {
            return Err(raise_type_error!("Cannot perform 'IsArray' on a revoked proxy").into());
        }
        if let Value::Object(target) = &*proxy.target {
            return is_array_spec(mc, target);
        }
    }
    Ok(false)
}

/// ArraySpeciesCreate(originalArray, length) — ECMAScript spec 9.4.2.3.
/// Returns a new array-like object created via the array's @@species constructor,
/// falling back to a plain Array if no species is found.
pub(crate) fn array_species_create_impl<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    receiver: &JSObjectDataPtr<'gc>,
    length: f64,
) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    // Step 1-2: If IsArray(receiver) is false, just create a plain array.
    if !is_array_spec(mc, receiver)? {
        let arr = create_array(mc, env)?;
        set_array_length(mc, &arr, length as usize)?;
        return Ok(arr);
    }

    // Step 3: Let C = receiver.constructor
    let ctor_val = crate::core::get_property_with_accessors(mc, env, receiver, "constructor")?;

    // Step 6: If C is undefined, return ArrayCreate(length)
    if matches!(ctor_val, Value::Undefined) {
        let arr = create_array(mc, env)?;
        set_array_length(mc, &arr, length as usize)?;
        return Ok(arr);
    }

    let mut c = ctor_val.clone();

    // Step 4 (cross-realm): if C is a constructor with NativeCtor="Array" but is NOT the
    // current realm's Array constructor, treat it as undefined (fall back to ArrayCreate).
    // This implements: "If C is a constructor and GetFunctionRealm(C) ≠ currentRealm, and
    // SameValue(C, realmC.Array) is true, set C to undefined."
    if let Value::Object(ctor_obj) = &c {
        let is_foreign_array = if let Some(nc_rc) = crate::core::slot_get_chained(ctor_obj, &InternalSlot::NativeCtor)
            && let Value::String(nc_name) = &*nc_rc.borrow()
            && crate::unicode::utf16_to_utf8(nc_name) == "Array"
        {
            // Check if it's the same Array as in the current env
            if let Some(cur_array_rc) = object_get_key_value(env, "Array")
                && let Value::Object(cur_array) = &*cur_array_rc.borrow()
            {
                cur_array.as_ptr() != ctor_obj.as_ptr()
            } else {
                false
            }
        } else {
            false
        };
        if is_foreign_array {
            let arr = create_array(mc, env)?;
            set_array_length(mc, &arr, length as usize)?;
            return Ok(arr);
        }
    }

    // Step 5: If Type(C) is Object, get C[@@species]
    if let Value::Object(ctor_obj) = &ctor_val
        && let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_val.borrow()
        && let Some(species_sym_val) = object_get_key_value(sym_obj, "species")
        && let Value::Symbol(species_sym) = &*species_sym_val.borrow()
    {
        let species = crate::core::get_property_with_accessors(mc, env, ctor_obj, *species_sym)?;
        match species {
            Value::Null | Value::Undefined => {
                let arr = create_array(mc, env)?;
                set_array_length(mc, &arr, length as usize)?;
                return Ok(arr);
            }
            other => c = other,
        }
    }

    // Step 6: If C is still undefined after species lookup, return ArrayCreate(length)
    if matches!(c, Value::Undefined) {
        let arr = create_array(mc, env)?;
        set_array_length(mc, &arr, length as usize)?;
        return Ok(arr);
    }

    // Step 7: If IsConstructor(C) is false, throw TypeError
    if !is_constructor_val(&c) {
        return Err(raise_type_error!("Array species constructor is not a constructor").into());
    }

    // Step 8: Return Construct(C, «length»)
    let constructed = crate::js_class::evaluate_new(mc, env, &c, &[Value::Number(length)], None)?;
    match constructed {
        Value::Object(obj) => Ok(obj),
        _ => Err(raise_type_error!("Array species constructor must return an object").into()),
    }
}

pub fn initialize_array<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let array_ctor = new_js_object_data(mc);
    slot_set(mc, &array_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &array_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("Array")));
    let array_ctor = new_js_object_data(mc);
    slot_set(mc, &array_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &array_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("Array")));

    let object_proto = if let Some(obj_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else {
        None
    };

    let array_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        array_proto.borrow_mut(mc).prototype = Some(proto);
    }

    slot_set(mc, &array_proto, InternalSlot::IsArray, &Value::Boolean(true));

    object_set_key_value(mc, &array_ctor, "prototype", &Value::Object(array_proto))?;
    array_ctor.borrow_mut(mc).set_non_writable("prototype");
    array_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    array_ctor.borrow_mut(mc).set_non_configurable("prototype");
    object_set_key_value(mc, &array_proto, "constructor", &Value::Object(array_ctor))?;
    array_proto.borrow_mut(mc).set_non_enumerable("constructor");

    for (method, method_length) in [("isArray", 1.0_f64), ("from", 1.0_f64), ("of", 0.0_f64)] {
        let func_obj = new_js_object_data(mc);

        if let Some(func_ctor_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*proto_val.borrow()
        {
            func_obj.borrow_mut(mc).prototype = Some(*func_proto);
        }

        let closure = ClosureData {
            env: Some(*env),
            native_target: Some(format!("Array.{method}")),
            enforce_strictness_inheritance: true,
            ..ClosureData::default()
        };
        func_obj
            .borrow_mut(mc)
            .set_closure(Some(new_gc_cell_ptr(mc, Value::Closure(Gc::new(mc, closure)))));

        let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16(method)), false, false, true)?;
        crate::js_object::define_property_internal(mc, &func_obj, "name", &name_desc)?;

        let length_desc = crate::core::create_descriptor_object(mc, &Value::Number(method_length), false, false, true)?;
        crate::js_object::define_property_internal(mc, &func_obj, "length", &length_desc)?;

        object_set_key_value(mc, &array_ctor, method, &Value::Object(func_obj))?;
        array_ctor.borrow_mut(mc).set_non_enumerable(method);
    }

    let methods = vec![
        "at",
        "push",
        "pop",
        "join",
        "slice",
        "splice",
        "shift",
        "unshift",
        "concat",
        "reverse",
        "sort",
        "flat",
        "flatMap",
        "includes",
        "indexOf",
        "lastIndexOf",
        "forEach",
        "map",
        "every",
        "some",
        "filter",
        "find",
        "findIndex",
        "findLast",
        "findLastIndex",
        "reduce",
        "reduceRight",
        "fill",
        "copyWithin",
        "keys",
        "values",
        "entries",
        "toString",
        "toLocaleString",
    ];

    for method in methods {
        let val = Value::Function(format!("Array.prototype.{method}"));
        object_set_key_value(mc, &array_proto, method, &val)?;
        array_proto.borrow_mut(mc).set_non_enumerable(method);
    }

    object_set_key_value(mc, &array_proto, "length", &Value::Number(0.0))?;
    array_proto.borrow_mut(mc).set_non_enumerable("length");
    array_proto.borrow_mut(mc).set_non_configurable("length");

    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_val.borrow()
    {
        if let Some(iter_sym_val) = object_get_key_value(sym_ctor, "iterator")
            && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
        {
            let val = Value::Function("Array.prototype.values".to_string());
            object_set_key_value(mc, &array_proto, iter_sym, &val)?;
            array_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::Symbol(*iter_sym));
        }

        if let Some(tag_sym_val) = object_get_key_value(sym_ctor, "toStringTag")
            && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
        {
            object_set_key_value(mc, &array_proto, tag_sym, &Value::String(utf8_to_utf16("Array")))?;
            array_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::Symbol(*tag_sym));
        }

        if let Some(unscopables_sym_val) = object_get_key_value(sym_ctor, "unscopables")
            && let Value::Symbol(unscopables_sym) = &*unscopables_sym_val.borrow()
        {
            let unscopables_obj = new_js_object_data(mc);
            unscopables_obj.borrow_mut(mc).prototype = None;

            for name in [
                "copyWithin",
                "entries",
                "fill",
                "find",
                "findIndex",
                "flat",
                "flatMap",
                "includes",
                "keys",
                "values",
            ] {
                object_set_key_value(mc, &unscopables_obj, name, &Value::Boolean(true))?;
            }

            let unscopables_desc = crate::core::create_descriptor_object(mc, &Value::Object(unscopables_obj), false, false, true)?;
            crate::js_object::define_property_internal(mc, &array_proto, PropertyKey::Symbol(*unscopables_sym), &unscopables_desc)?;
        }

        // Array[Symbol.species] — accessor getter returning `this`, non-enumerable, configurable
        if let Some(species_sym_val) = object_get_key_value(sym_ctor, "species")
            && let Value::Symbol(species_sym) = &*species_sym_val.borrow()
        {
            let getter_fn = new_js_object_data(mc);
            if let Some(func_ctor_val) = object_get_key_value(env, "Function")
                && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
                && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
                && let Value::Object(func_proto) = &*proto_val.borrow()
            {
                getter_fn.borrow_mut(mc).prototype = Some(*func_proto);
            }
            let getter_closure = ClosureData {
                env: Some(*env),
                native_target: Some("Array.species".to_string()),
                enforce_strictness_inheritance: true,
                ..ClosureData::default()
            };
            getter_fn
                .borrow_mut(mc)
                .set_closure(Some(new_gc_cell_ptr(mc, Value::Closure(Gc::new(mc, getter_closure)))));
            let gname_desc =
                crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("get [Symbol.species]")), false, false, true)?;
            crate::js_object::define_property_internal(mc, &getter_fn, "name", &gname_desc)?;
            let glen_desc = crate::core::create_descriptor_object(mc, &Value::Number(0.0), false, false, true)?;
            crate::js_object::define_property_internal(mc, &getter_fn, "length", &glen_desc)?;

            let species_desc_obj = new_js_object_data(mc);
            object_set_key_value(mc, &species_desc_obj, "get", &Value::Object(getter_fn))?;
            object_set_key_value(mc, &species_desc_obj, "enumerable", &Value::Boolean(false))?;
            object_set_key_value(mc, &species_desc_obj, "configurable", &Value::Boolean(true))?;
            crate::js_object::define_property_internal(mc, &array_ctor, PropertyKey::Symbol(*species_sym), &species_desc_obj)?;
        }
    }

    let arr_name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("Array")), false, false, true)?;
    crate::js_object::define_property_internal(mc, &array_ctor, "name", &arr_name_desc)?;

    let arr_len_desc = crate::core::create_descriptor_object(mc, &Value::Number(1.0), false, false, true)?;
    crate::js_object::define_property_internal(mc, &array_ctor, "length", &arr_len_desc)?;

    // --- Create %IteratorPrototype% and %ArrayIteratorPrototype% ---
    // %IteratorPrototype% has [[Prototype]] = Object.prototype and a
    // Symbol.iterator method that returns `this`.
    let iterator_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        iterator_proto.borrow_mut(mc).prototype = Some(proto);
    }
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_val.borrow()
        && let Some(iter_sym_val) = object_get_key_value(sym_ctor, "iterator")
        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
    {
        // Create a proper function object with name="[Symbol.iterator]" and length=0
        let iter_fn_obj = new_js_object_data(mc);
        if let Some(func_ctor_val) = crate::core::env_get(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*proto_val.borrow()
        {
            iter_fn_obj.borrow_mut(mc).prototype = Some(*func_proto);
        }
        iter_fn_obj
            .borrow_mut(mc)
            .set_closure(Some(crate::core::new_gc_cell_ptr(mc, Value::Function("IteratorSelf".to_string()))));
        let name_desc = crate::core::create_descriptor_object(
            mc,
            &Value::String(crate::unicode::utf8_to_utf16("[Symbol.iterator]")),
            false,
            false,
            true,
        )?;
        crate::js_object::define_property_internal(mc, &iter_fn_obj, "name", &name_desc)?;
        let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(0.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &iter_fn_obj, "length", &len_desc)?;

        object_set_key_value(mc, &iterator_proto, iter_sym, &Value::Object(iter_fn_obj))?;
        iterator_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::Symbol(*iter_sym));
    }

    // %ArrayIteratorPrototype% has [[Prototype]] = %IteratorPrototype%,
    // a `next` method, and Symbol.toStringTag = "Array Iterator".
    let array_iter_proto = new_js_object_data(mc);
    array_iter_proto.borrow_mut(mc).prototype = Some(iterator_proto);

    // next method (writable, non-enumerable, configurable)
    object_set_key_value(
        mc,
        &array_iter_proto,
        "next",
        &Value::Function("ArrayIterator.prototype.next".to_string()),
    )?;
    array_iter_proto.borrow_mut(mc).set_non_enumerable("next");

    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_val.borrow()
    {
        // Symbol.toStringTag = "Array Iterator" (non-writable, non-enumerable, configurable)
        if let Some(tag_sym_val) = object_get_key_value(sym_ctor, "toStringTag")
            && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
        {
            let tag_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("Array Iterator")), false, false, true)?;
            crate::js_object::define_property_internal(mc, &array_iter_proto, PropertyKey::Symbol(*tag_sym), &tag_desc)?;
        }
    }

    // Store %ArrayIteratorPrototype% in env (hidden via internal slot)
    slot_set(mc, env, InternalSlot::ArrayIteratorPrototype, &Value::Object(array_iter_proto));

    env_set(mc, env, "Array", &Value::Object(array_ctor))?;
    Ok(())
}

/// Handle Array static method calls (Array.isArray, Array.from, Array.of)
pub(crate) fn handle_array_static_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    this_val: Option<&Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let mut effective_this = if let Some(tv) = this_val {
        Some(tv.clone())
    } else {
        crate::core::env_get(env, "this").map(|tv_rc| tv_rc.borrow().clone())
    };

    if matches!(method, "from" | "of")
        && let Some(Value::Object(this_obj)) = effective_this.as_ref()
        && let Some(cl_ptr) = this_obj.borrow().get_closure()
    {
        let native_name = match &*cl_ptr.borrow() {
            Value::Closure(cl) | Value::AsyncClosure(cl) => cl.native_target.clone(),
            _ => None,
        };

        if matches!(native_name.as_deref(), Some("Array.from") | Some("Array.of"))
            && let Some(array_ctor_rc) = crate::core::env_get(env, "Array")
        {
            effective_this = Some(array_ctor_rc.borrow().clone());
        }
    }

    let create_with_ctor_or_array = |len_arg: Option<usize>| -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
        if let Some(tv) = effective_this.as_ref()
            && !matches!(tv, Value::Undefined | Value::Null)
        {
            let maybe_constructor_object = || -> Option<JSObjectDataPtr<'gc>> {
                match tv {
                    Value::Object(obj) => Some(*obj),
                    Value::Function(name) => {
                        if let Some(resolved_rc) = crate::core::env_get(env, name)
                            && let Value::Object(resolved_obj) = &*resolved_rc.borrow()
                        {
                            Some(*resolved_obj)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            };

            let ensure_constructor_prototype = |out_obj: JSObjectDataPtr<'gc>| -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
                if let Some(ctor_obj) = maybe_constructor_object()
                    && let Some(proto_val) = object_get_key_value(&ctor_obj, "prototype")
                {
                    let proto_candidate = match &*proto_val.borrow() {
                        Value::Object(p) => Some(*p),
                        Value::Property { value: Some(v), .. } => match &*v.borrow() {
                            Value::Object(p) => Some(*p),
                            _ => None,
                        },
                        _ => None,
                    };

                    if let Some(proto_obj) = proto_candidate {
                        let should_update = out_obj
                            .borrow()
                            .prototype
                            .map(|p| !crate::core::Gc::ptr_eq(p, proto_obj))
                            .unwrap_or(true);
                        if should_update {
                            out_obj.borrow_mut(mc).prototype = Some(proto_obj);
                            slot_set(mc, &out_obj, InternalSlot::Proto, &Value::Object(proto_obj));
                        }
                    }
                }

                Ok(out_obj)
            };

            let ctor_args = if let Some(len) = len_arg {
                vec![Value::Number(len as f64)]
            } else {
                vec![]
            };

            let attempt = match tv {
                Value::Function(name) if name == "Array" => crate::js_array::handle_array_constructor(mc, &ctor_args, env, None),
                Value::Function(name) if name == "Object" => crate::js_class::handle_object_constructor(mc, &ctor_args, env),
                Value::Function(name) => {
                    if let Some(resolved) = crate::core::env_get(env, name) {
                        let resolved_val = resolved.borrow().clone();
                        crate::js_class::evaluate_new(mc, env, &resolved_val, &ctor_args, None)
                    } else {
                        crate::js_class::evaluate_new(mc, env, tv, &ctor_args, None)
                    }
                }
                Value::Object(obj) if slot_get_chained(obj, &InternalSlot::NativeCtor).is_some() => {
                    if let Some(native_ctor) = slot_get_chained(obj, &InternalSlot::NativeCtor)
                        && let Value::String(name_u16) = &*native_ctor.borrow()
                    {
                        let native_name = utf16_to_utf8(name_u16);
                        if native_name == "Array" {
                            crate::js_array::handle_array_constructor(mc, &ctor_args, env, None)
                        } else if native_name == "Object" {
                            crate::js_class::handle_object_constructor(mc, &ctor_args, env)
                        } else {
                            crate::js_class::evaluate_new(mc, env, tv, &ctor_args, None)
                        }
                    } else {
                        crate::js_class::evaluate_new(mc, env, tv, &ctor_args, None)
                    }
                }
                _ => crate::js_class::evaluate_new(mc, env, tv, &ctor_args, None),
            };

            match attempt {
                Ok(Value::Object(out_obj)) => {
                    let out_obj = ensure_constructor_prototype(out_obj)?;
                    return Ok(out_obj);
                }
                Ok(_) => return Err(raise_type_error!("Array static constructor must return an object").into()),
                Err(err) => {
                    let is_not_ctor = match &err {
                        EvalError::Js(js_err) => {
                            let msg = js_err.message();
                            msg.contains("Not a constructor") || msg.contains("Constructor is not callable")
                        }
                        _ => false,
                    };
                    if !is_not_ctor {
                        return Err(err);
                    }
                }
            }
        }

        let fallback_len = len_arg.unwrap_or(0) as f64;
        if let Some(array_ctor_rc) = crate::core::env_get(env, "Array")
            && let Value::Object(array_ctor_obj) = &*array_ctor_rc.borrow()
            && let Ok(Value::Object(out_obj)) =
                crate::js_class::evaluate_new(mc, env, &Value::Object(*array_ctor_obj), &[Value::Number(fallback_len)], None)
        {
            return Ok(out_obj);
        }

        let out = create_array(mc, env).map_err(EvalError::from)?;
        if let Some(array_ctor_rc) = crate::core::env_get(env, "Array")
            && let Value::Object(array_ctor_obj) = &*array_ctor_rc.borrow()
            && let Some(array_proto_rc) = object_get_key_value(array_ctor_obj, "prototype")
        {
            let array_proto_candidate = match &*array_proto_rc.borrow() {
                Value::Object(p) => Some(*p),
                Value::Property { value: Some(v), .. } => match &*v.borrow() {
                    Value::Object(p) => Some(*p),
                    _ => None,
                },
                _ => None,
            };

            if let Some(array_proto) = array_proto_candidate {
                let should_update = out
                    .borrow()
                    .prototype
                    .map(|p| !crate::core::Gc::ptr_eq(p, array_proto))
                    .unwrap_or(true);
                if should_update {
                    out.borrow_mut(mc).prototype = Some(array_proto);
                    slot_set(mc, &out, InternalSlot::Proto, &Value::Object(array_proto));
                }
            }
        }
        Ok(out)
    };

    let create_data_property_or_throw = |target: &JSObjectDataPtr<'gc>, key: usize, value: &Value<'gc>| -> Result<(), EvalError<'gc>> {
        if let Some(proxy_cell) = crate::core::slot_get(target, &InternalSlot::Proxy)
            && let Value::Proxy(proxy) = &*proxy_cell.borrow()
        {
            let prop_key = PropertyKey::from(key.to_string());
            let ok = crate::js_proxy::proxy_define_data_property(mc, proxy, &prop_key, value)?;
            if !ok {
                return Err(raise_type_error!("Cannot define property").into());
            }
            return Ok(());
        }

        let desc = crate::core::create_descriptor_object(mc, value, true, true, true)?;
        crate::js_object::define_property_internal(mc, target, key.to_string(), &desc)?;
        Ok(())
    };

    let set_length_with_set_semantics = |target: &JSObjectDataPtr<'gc>, len: usize| -> Result<(), EvalError<'gc>> {
        if let Some(proxy_cell) = crate::core::slot_get(target, &InternalSlot::Proxy)
            && let Value::Proxy(proxy) = &*proxy_cell.borrow()
        {
            let prop_key = PropertyKey::from("length");
            let ok = crate::js_proxy::proxy_set_property(mc, proxy, &prop_key, &Value::Number(len as f64))?;
            if !ok {
                return Err(raise_type_error!("Cannot set property 'length'").into());
            }
            return Ok(());
        }

        let key = PropertyKey::from("length");
        let mut owner = Some(*target);

        while let Some(obj) = owner {
            if let Some(prop) = crate::core::get_own_property(&obj, &key) {
                match &*prop.borrow() {
                    Value::Property {
                        value: _,
                        getter: _,
                        setter: Some(setter_fn),
                    } => {
                        let setter_args = vec![Value::Number(len as f64)];
                        let _ = evaluate_call_dispatch(mc, env, setter_fn, Some(&Value::Object(*target)), &setter_args)?;
                        return Ok(());
                    }
                    Value::Property {
                        value: _,
                        getter: _,
                        setter: None,
                    } => {
                        if !obj.borrow().is_writable(&key) {
                            return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
                        }
                        object_set_key_value(mc, target, "length", &Value::Number(len as f64))?;
                        return Ok(());
                    }
                    _ => {
                        if !obj.borrow().is_writable(&key) {
                            return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
                        }
                        object_set_key_value(mc, target, "length", &Value::Number(len as f64))?;
                        return Ok(());
                    }
                }
            }
            owner = obj.borrow().prototype;
        }

        object_set_key_value(mc, target, "length", &Value::Number(len as f64))?;
        Ok(())
    };

    match method {
        "isArray" => {
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            let is_array_value = match arg {
                Value::Object(object) => {
                    let mut current = object;
                    loop {
                        if let Some(proxy_cell) = crate::core::slot_get(&current, &InternalSlot::Proxy)
                            && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                        {
                            if proxy.revoked {
                                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
                            }
                            if let Value::Object(target_obj) = &*proxy.target {
                                current = *target_obj;
                                continue;
                            }
                            break false;
                        }
                        break is_array(mc, &current);
                    }
                }
                _ => false,
            };
            Ok(Value::Boolean(is_array_value))
        }
        "from" => {
            let items = args.first().cloned().unwrap_or(Value::Undefined);
            if matches!(items, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Array.from requires an array-like or iterable object").into());
            }

            let map_fn = args.get(1).cloned();
            let this_arg = args.get(2).cloned().unwrap_or(Value::Undefined);

            let mapper = if let Some(fn_val) = map_fn {
                if matches!(fn_val, Value::Undefined) {
                    None
                } else {
                    let callable = match &fn_val {
                        Value::Closure(_)
                        | Value::AsyncClosure(_)
                        | Value::Function(_)
                        | Value::GeneratorFunction(_, _)
                        | Value::AsyncGeneratorFunction(_, _) => true,
                        Value::Object(obj) => {
                            obj.borrow().get_closure().is_some()
                                || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                                || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                        }
                        _ => false,
                    };
                    if !callable {
                        return Err(raise_type_error!("Array.from map function must be callable").into());
                    }
                    Some(fn_val)
                }
            } else {
                None
            };

            let mut result: Vec<Value<'gc>> = Vec::new();
            let mut used_iterator_or_string = false;

            fn map_value<'gc>(
                mc: &MutationContext<'gc>,
                env: &JSObjectDataPtr<'gc>,
                mapper: &Option<Value<'gc>>,
                this_arg: &Value<'gc>,
                idx: usize,
                value: Value<'gc>,
            ) -> Result<Value<'gc>, EvalError<'gc>> {
                if let Some(fn_val) = &mapper {
                    let mut actual_fn = fn_val.clone();
                    if let Value::Object(obj) = fn_val {
                        if let Some(prop) = obj.borrow().get_closure() {
                            actual_fn = prop.borrow().clone();
                        } else if let Some(nc) = slot_get_chained(obj, &InternalSlot::NativeCtor)
                            && let Value::String(name_vec) = &*nc.borrow()
                        {
                            actual_fn = Value::Function(utf16_to_utf8(name_vec));
                        }
                    }
                    let call_args = vec![value, Value::Number(idx as f64)];
                    evaluate_call_dispatch(mc, env, &actual_fn, Some(this_arg), &call_args)
                } else {
                    Ok(value)
                }
            }

            match items.clone() {
                Value::String(s) => {
                    used_iterator_or_string = true;
                    for (idx, ch) in s.into_iter().enumerate() {
                        result.push(map_value(mc, env, &mapper, &this_arg, idx, Value::String(vec![ch]))?);
                    }
                }
                Value::Object(object) => {
                    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                        && let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator")
                        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
                    {
                        let iter_fn = crate::core::get_property_with_accessors(mc, env, &object, iter_sym)?;
                        if !matches!(iter_fn, Value::Undefined) {
                            let out = create_with_ctor_or_array(None)?;
                            let close_iterator = |iter_obj: &JSObjectDataPtr<'gc>| {
                                if let Ok(return_fn) = crate::core::get_property_with_accessors(mc, env, iter_obj, "return")
                                    && !matches!(return_fn, Value::Undefined | Value::Null)
                                {
                                    let _ = evaluate_call_dispatch(mc, env, &return_fn, Some(&Value::Object(*iter_obj)), &[]);
                                }
                            };

                            let iterator = evaluate_call_dispatch(mc, env, &iter_fn, Some(&Value::Object(object)), &[])?;
                            let iter_obj = match iterator {
                                Value::Object(o) => o,
                                _ => return Err(raise_type_error!("Array.from iterator must return an object").into()),
                            };

                            let mut idx = 0usize;
                            loop {
                                let next_fn = match crate::core::get_property_with_accessors(mc, env, &iter_obj, "next") {
                                    Ok(v) => v,
                                    Err(err) => {
                                        close_iterator(&iter_obj);
                                        return Err(err);
                                    }
                                };
                                let next_res = match evaluate_call_dispatch(mc, env, &next_fn, Some(&Value::Object(iter_obj)), &[]) {
                                    Ok(v) => v,
                                    Err(err) => {
                                        close_iterator(&iter_obj);
                                        return Err(err);
                                    }
                                };
                                let next_obj = match next_res {
                                    Value::Object(o) => o,
                                    _ => {
                                        close_iterator(&iter_obj);
                                        return Err(raise_type_error!("Iterator.next must return an object").into());
                                    }
                                };
                                let done_val = match crate::core::get_property_with_accessors(mc, env, &next_obj, "done") {
                                    Ok(v) => v,
                                    Err(err) => {
                                        close_iterator(&iter_obj);
                                        return Err(err);
                                    }
                                };
                                if done_val.to_truthy() {
                                    break;
                                }
                                let value = match crate::core::get_property_with_accessors(mc, env, &next_obj, "value") {
                                    Ok(v) => v,
                                    Err(err) => {
                                        close_iterator(&iter_obj);
                                        return Err(err);
                                    }
                                };
                                let mapped = match map_value(mc, env, &mapper, &this_arg, idx, value) {
                                    Ok(v) => v,
                                    Err(err) => {
                                        close_iterator(&iter_obj);
                                        return Err(err);
                                    }
                                };
                                if let Err(err) = create_data_property_or_throw(&out, idx, &mapped) {
                                    close_iterator(&iter_obj);
                                    return Err(err);
                                }
                                idx += 1;
                            }

                            set_length_with_set_semantics(&out, idx)?;
                            return Ok(Value::Object(out));
                        }
                    }

                    let len_val = crate::core::get_property_with_accessors(mc, env, &object, "length")?;
                    let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
                    let len_num = crate::core::to_number(&len_prim)?;
                    let max_len = 9007199254740991.0_f64;
                    let len = if len_num.is_nan() || len_num <= 0.0 {
                        0usize
                    } else if !len_num.is_finite() {
                        max_len as usize
                    } else {
                        len_num.floor().min(max_len) as usize
                    };

                    for i in 0..len {
                        let element = crate::core::get_property_with_accessors(mc, env, &object, i.to_string())?;
                        result.push(map_value(mc, env, &mapper, &this_arg, i, element)?);
                    }
                }
                _ => return Err(raise_type_error!("Array.from requires an array-like or iterable object").into()),
            }

            let out = if used_iterator_or_string {
                create_with_ctor_or_array(None)?
            } else {
                create_with_ctor_or_array(Some(result.len()))?
            };
            for (i, val) in result.iter().enumerate() {
                create_data_property_or_throw(&out, i, val)?;
            }
            set_length_with_set_semantics(&out, result.len())?;
            Ok(Value::Object(out))
        }
        "of" => {
            let out = create_with_ctor_or_array(Some(args.len()))?;
            for (i, arg) in args.iter().enumerate() {
                create_data_property_or_throw(&out, i, arg)?;
            }
            set_length_with_set_semantics(&out, args.len())?;
            Ok(Value::Object(out))
        }
        _ => Err(raise_eval_error!(format!("Array.{method} is not implemented")).into()),
    }
}

pub(crate) fn handle_array_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    new_target: Option<&Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let result = if args.is_empty() {
        // Array() - create empty array
        let array_obj = create_array(mc, env)?;
        set_array_length(mc, &array_obj, 0)?;
        Value::Object(array_obj)
    } else if args.len() == 1 {
        // Array(length) or Array(element)
        let arg_val = args[0].clone();
        match arg_val {
            Value::Number(n) => {
                if n.is_nan() {
                    return Err(raise_range_error!("Invalid array length").into());
                }
                if n.fract() != 0.0 {
                    return Err(raise_range_error!("Invalid array length").into());
                }
                if n < 0.0 {
                    return Err(raise_range_error!("Invalid array length").into());
                }
                if n > u32::MAX as f64 {
                    return Err(raise_range_error!("Invalid array length").into());
                }
                // Array(length) - create array with specified length
                let array_obj = create_array(mc, env)?;
                set_array_length(mc, &array_obj, n as usize)?;
                Value::Object(array_obj)
            }
            _ => {
                // Array(element) - create array with single element
                let array_obj = create_array(mc, env)?;
                object_set_key_value(mc, &array_obj, "0", &arg_val)?;
                set_array_length(mc, &array_obj, 1)?;
                Value::Object(array_obj)
            }
        }
    } else {
        // Array(element1, element2, ...) - create array with multiple elements
        let array_obj = create_array(mc, env)?;
        for (i, arg) in args.iter().enumerate() {
            let arg_val = arg.clone();
            object_set_key_value(mc, &array_obj, i, &arg_val)?;
        }
        set_array_length(mc, &array_obj, args.len())?;
        Value::Object(array_obj)
    };

    // GetPrototypeFromConstructor when new_target is provided.
    if let Some(_nt) = new_target
        && let Value::Object(array_obj) = &result
        && let Value::Object(nt_obj) = _nt
        && let Some(proto) = crate::js_class::get_prototype_from_constructor(mc, nt_obj, env, "Array")?
    {
        array_obj.borrow_mut(mc).prototype = Some(proto);
        slot_set(mc, array_obj, InternalSlot::Proto, &Value::Object(proto));
    }

    Ok(result)
}

/// Handle Array instance method calls
pub(crate) fn handle_array_instance_method<'gc>(
    mc: &MutationContext<'gc>,
    object: &JSObjectDataPtr<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "at" => {
            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let mut len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                usize::MAX
            } else {
                len_num.floor() as usize
            };
            if len > isize::MAX as usize {
                len = isize::MAX as usize;
            }

            let index = if !args.is_empty() {
                let idx_prim = crate::core::to_primitive(mc, &args[0], "number", env)?;
                let idx_num = crate::core::to_number(&idx_prim)?;
                if idx_num.is_nan() || idx_num == 0.0 {
                    0isize
                } else if idx_num.is_infinite() {
                    if idx_num.is_sign_negative() { isize::MIN } else { isize::MAX }
                } else {
                    idx_num.trunc() as isize
                }
            } else {
                0isize
            };

            let k = if index >= 0 { index } else { (len as isize).saturating_add(index) };

            if k < 0 || (k as usize) >= len {
                Ok(Value::Undefined)
            } else {
                Ok(crate::core::get_property_with_accessors(mc, env, object, k as usize)?)
            }
        }
        "push" => {
            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let max_len = 9007199254740991.0_f64;
            let is_array_receiver = is_array(mc, object);
            let length_is_non_writable = || -> Result<bool, EvalError<'gc>> {
                let desc = crate::js_object::handle_object_method(
                    mc,
                    "getOwnPropertyDescriptor",
                    &[Value::Object(*object), Value::String(utf8_to_utf16("length"))],
                    env,
                )?;
                if let Value::Object(desc_obj) = desc {
                    let writable = crate::core::get_property_with_accessors(mc, env, &desc_obj, "writable")?;
                    if matches!(writable, Value::Boolean(false)) {
                        return Ok(true);
                    }
                }
                Ok(false)
            };

            if !object.borrow().is_writable("length") || length_is_non_writable()? {
                return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
            }

            let mut current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                max_len as usize
            } else {
                len_num.floor().min(max_len) as usize
            };

            if (current_len as f64) + (args.len() as f64) > max_len {
                return Err(raise_type_error!("Invalid array length").into());
            }

            for arg in args {
                let key = current_len.to_string();
                if crate::core::get_own_property(object, key.as_str()).is_some() && !object.borrow().is_writable(key.as_str()) {
                    return Err(raise_type_error!("Cannot assign to read only property").into());
                }
                let mut handled_by_setter = false;
                let mut cur_proto = object.borrow().prototype;
                while let Some(proto) = cur_proto {
                    if let Some(prop) = crate::core::get_own_property(&proto, key.as_str()) {
                        match &*prop.borrow() {
                            Value::Property {
                                setter: Some(setter_fn), ..
                            } => {
                                let setter_args = vec![arg.clone()];
                                let _ = evaluate_call_dispatch(mc, env, setter_fn, Some(&Value::Object(*object)), &setter_args)?;
                                handled_by_setter = true;
                            }
                            Value::Property {
                                setter: None,
                                getter: Some(_),
                                value: None,
                            } => {
                                return Err(raise_type_error!("Cannot set property without setter").into());
                            }
                            _ => {}
                        }
                        break;
                    }
                    cur_proto = proto.borrow().prototype;
                }

                if !handled_by_setter && object_set_key_value(mc, object, current_len, arg).is_err() {
                    return Err(raise_type_error!("Cannot set array element").into());
                }
                current_len += 1;
            }

            if !object.borrow().is_writable("length") || length_is_non_writable()? {
                return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
            }

            if is_array_receiver {
                if current_len > u32::MAX as usize {
                    return Err(raise_range_error!("Invalid array length").into());
                }
                if set_array_length(mc, object, current_len).is_err() {
                    return Err(raise_type_error!("Cannot set length").into());
                }
            } else if object_set_key_value(mc, object, "length", &Value::Number(current_len as f64)).is_err() {
                return Err(raise_type_error!("Cannot set length").into());
            }

            Ok(Value::Number(current_len as f64))
        }
        "pop" => {
            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let max_len = 9007199254740991.0_f64;
            let is_array_receiver = is_array(mc, object);
            let length_is_non_writable = || -> Result<bool, EvalError<'gc>> {
                let desc = crate::js_object::handle_object_method(
                    mc,
                    "getOwnPropertyDescriptor",
                    &[Value::Object(*object), Value::String(utf8_to_utf16("length"))],
                    env,
                )?;
                if let Value::Object(desc_obj) = desc {
                    let writable = crate::core::get_property_with_accessors(mc, env, &desc_obj, "writable")?;
                    if matches!(writable, Value::Boolean(false)) {
                        return Ok(true);
                    }
                }
                Ok(false)
            };

            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                max_len as usize
            } else {
                len_num.floor().min(max_len) as usize
            };

            if current_len == 0 {
                if !object.borrow().is_writable("length") || length_is_non_writable()? {
                    return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
                }
                if is_array_receiver {
                    if set_array_length(mc, object, 0).is_err() {
                        return Err(raise_type_error!("Cannot set length").into());
                    }
                } else if object_set_key_value(mc, object, "length", &Value::Number(0.0)).is_err() {
                    return Err(raise_type_error!("Cannot set length").into());
                }
                return Ok(Value::Undefined);
            }

            let new_len = current_len - 1;
            let index_key = new_len.to_string();
            let element = crate::core::get_property_with_accessors(mc, env, object, new_len)?;

            if crate::core::get_own_property(object, index_key.as_str()).is_some() {
                if !object.borrow().is_configurable(index_key.as_str()) {
                    return Err(raise_type_error!("Cannot delete non-configurable property").into());
                }
                object.borrow_mut(mc).properties.shift_remove(&PropertyKey::from(index_key.clone()));
            }

            if !object.borrow().is_writable("length") || length_is_non_writable()? {
                return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
            }

            if is_array_receiver {
                if set_array_length(mc, object, new_len).is_err() {
                    return Err(raise_type_error!("Cannot set length").into());
                }
            } else if object_set_key_value(mc, object, "length", &Value::Number(new_len as f64)).is_err() {
                return Err(raise_type_error!("Cannot set length").into());
            }

            Ok(element)
        }
        "length" => {
            let length = Value::Number(get_array_length(mc, object).unwrap_or(0) as f64);
            Ok(length)
        }
        "join" => {
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            let current_len = if let Some(ta) = typed_array_ptr {
                if ta.length_tracking {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    if buf_len <= ta.byte_offset {
                        0
                    } else {
                        (buf_len - ta.byte_offset) / ta.element_size()
                    }
                } else {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    let needed = ta.byte_offset + ta.length * ta.element_size();
                    if buf_len < needed { 0 } else { ta.length }
                }
            } else {
                let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
                let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
                let len_num = crate::core::to_number(&len_prim)?;
                let max_len = 9007199254740991.0_f64;
                if len_num.is_nan() || len_num <= 0.0 {
                    0usize
                } else if !len_num.is_finite() {
                    max_len as usize
                } else {
                    len_num.floor().min(max_len) as usize
                }
            };

            let separator = if let Some(sep_val) = args.first() {
                if matches!(sep_val, Value::Undefined) {
                    ",".to_string()
                } else {
                    let prim = crate::core::to_primitive(mc, sep_val, "string", env)?;
                    if matches!(prim, Value::Symbol(_)) {
                        return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
                    }
                    crate::core::value_to_string(&prim)
                }
            } else {
                ",".to_string()
            };

            if current_len == 0 {
                return Ok(Value::String(utf8_to_utf16("")));
            }

            let mut result = String::new();
            for i in 0..current_len {
                if i > 0 {
                    result.push_str(&separator);
                }
                let element = crate::core::get_property_with_accessors(mc, env, object, i)?;
                match element {
                    Value::Undefined | Value::Null => {}
                    _ => {
                        let prim = crate::core::to_primitive(mc, &element, "string", env)?;
                        if matches!(prim, Value::Symbol(_)) {
                            return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
                        }
                        result.push_str(&crate::core::value_to_string(&prim));
                    }
                }
            }
            Ok(Value::String(utf8_to_utf16(&result)))
        }
        "slice" => {
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            let current_len = if let Some(ta) = typed_array_ptr {
                if ta.length_tracking {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    if buf_len <= ta.byte_offset {
                        0
                    } else {
                        (buf_len - ta.byte_offset) / ta.element_size()
                    }
                } else {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    let needed = ta.byte_offset + ta.length * ta.element_size();
                    if buf_len < needed { 0 } else { ta.length }
                }
            } else {
                let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
                let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
                let len_num = crate::core::to_number(&len_prim)?;
                let max_len = 9007199254740991.0_f64;
                if len_num.is_nan() || len_num <= 0.0 {
                    0usize
                } else if !len_num.is_finite() {
                    max_len as usize
                } else {
                    len_num.floor().min(max_len) as usize
                }
            };

            let to_integer_or_infinity = |value: &Value<'gc>| -> Result<f64, EvalError<'gc>> {
                let prim = crate::core::to_primitive(mc, value, "number", env)?;
                if matches!(prim, Value::Symbol(_)) {
                    return Err(raise_type_error!("Cannot convert a Symbol value to a number").into());
                }
                let num = crate::core::to_number(&prim)?;
                if num.is_nan() || num == 0.0 {
                    Ok(0.0)
                } else if num.is_infinite() {
                    Ok(num)
                } else {
                    Ok(num.trunc())
                }
            };

            let relative_start = if let Some(start_arg) = args.first() {
                to_integer_or_infinity(start_arg)?
            } else {
                0.0
            };
            let len_i = current_len as i128;
            let start_i = if relative_start == f64::NEG_INFINITY {
                0_i128
            } else if relative_start == f64::INFINITY {
                len_i
            } else {
                let rel = relative_start as i128;
                if rel < 0 { (len_i + rel).max(0) } else { rel.min(len_i) }
            };

            let relative_end = if let Some(end_arg) = args.get(1) {
                if matches!(end_arg, Value::Undefined) {
                    len_i as f64
                } else {
                    to_integer_or_infinity(end_arg)?
                }
            } else {
                len_i as f64
            };
            let end_i = if relative_end == f64::NEG_INFINITY {
                0_i128
            } else if relative_end == f64::INFINITY {
                len_i
            } else {
                let rel = relative_end as i128;
                if rel < 0 { (len_i + rel).max(0) } else { rel.min(len_i) }
            };

            let start = start_i as usize;
            let end = end_i as usize;

            let count = end.saturating_sub(start);
            if count > u32::MAX as usize {
                return Err(raise_range_error!("Invalid array length").into());
            }

            let new_array = array_species_create_impl(mc, env, object, count as f64)?;
            let mut n = 0usize;
            let mut k = start;
            while k < end {
                let has_property = if let Some(ta) = typed_array_ptr {
                    let effective_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0 } else { ta.length }
                    };
                    k < effective_len
                } else if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy) {
                    if let Value::Proxy(proxy) = &*proxy_cell.borrow() {
                        crate::js_proxy::proxy_has_property(mc, proxy, k.to_string())?
                    } else {
                        object_get_key_value(object, k.to_string()).is_some()
                    }
                } else {
                    object_get_key_value(object, k.to_string()).is_some()
                };

                if has_property {
                    let val = crate::core::get_property_with_accessors(mc, env, object, k.to_string())?;
                    create_data_property_or_throw(mc, &new_array, n, &val)?;
                }

                n += 1;
                k += 1;
            }

            set_array_length(mc, &new_array, count)?;
            Ok(Value::Object(new_array))
        }
        "forEach" => {
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            let callback_val = args.first().cloned().unwrap_or(Value::Undefined);
            let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                usize::MAX
            } else {
                len_num.floor() as usize
            };

            let callback_callable = match &callback_val {
                Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::Function(_)
                | Value::GeneratorFunction(_, _)
                | Value::AsyncGeneratorFunction(_, _) => true,
                Value::Object(obj) => {
                    obj.borrow().get_closure().is_some()
                        || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                        || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                }
                _ => false,
            };
            if !callback_callable {
                return Err(raise_type_error!("Array.forEach callback must be a function").into());
            }

            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            let mut actual_callback_val = callback_val.clone();
            if let Value::Object(obj) = &callback_val {
                if let Some(prop) = obj.borrow().get_closure() {
                    actual_callback_val = prop.borrow().clone();
                } else if let Some(nc) = slot_get_chained(obj, &InternalSlot::NativeCtor)
                    && let Value::String(name_vec) = &*nc.borrow()
                {
                    let name = crate::unicode::utf16_to_utf8(name_vec);
                    actual_callback_val = Value::Function(name);
                }
            }

            for i in 0..current_len {
                let has_property = if let Some(ta) = typed_array_ptr {
                    let effective_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0 } else { ta.length }
                    };
                    i < effective_len
                } else {
                    object_get_key_value(object, i).is_some()
                };

                if has_property {
                    let val = crate::core::get_property_with_accessors(mc, env, object, i)?;
                    let call_args = vec![val, Value::Number(i as f64), Value::Object(*object)];
                    evaluate_call_dispatch(mc, env, &actual_callback_val, Some(&this_arg), &call_args)?;
                }
            }

            Ok(Value::Undefined)
        }
        "map" => {
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            let callback_val = args.first().cloned().unwrap_or(Value::Undefined);
            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                usize::MAX
            } else {
                len_num.floor() as usize
            };

            let callback_callable = match &callback_val {
                Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::Function(_)
                | Value::GeneratorFunction(_, _)
                | Value::AsyncGeneratorFunction(_, _) => true,
                Value::Object(obj) => {
                    obj.borrow().get_closure().is_some()
                        || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                        || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                }
                _ => false,
            };
            if !callback_callable {
                return Err(raise_type_error!("Array.map callback must be a function").into());
            }

            if current_len > u32::MAX as usize {
                return Err(raise_range_error!("Invalid array length").into());
            }

            let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
            let new_array = array_species_create_impl(mc, env, object, current_len as f64)?;

            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            for i in 0..current_len {
                let has_property = if let Some(ta) = typed_array_ptr {
                    let effective_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0 } else { ta.length }
                    };
                    i < effective_len
                } else {
                    object_get_key_value(object, i).is_some()
                };

                if has_property {
                    let val = crate::core::get_property_with_accessors(mc, env, object, i)?;
                    let call_args = vec![val, Value::Number(i as f64), Value::Object(*object)];

                    let mut actual_callback_val = callback_val.clone();
                    if let Value::Object(obj) = &callback_val {
                        if let Some(prop) = obj.borrow().get_closure() {
                            actual_callback_val = prop.borrow().clone();
                        } else if let Some(nc) = slot_get_chained(obj, &InternalSlot::NativeCtor)
                            && let Value::String(name_vec) = &*nc.borrow()
                        {
                            let name = crate::unicode::utf16_to_utf8(name_vec);
                            actual_callback_val = Value::Function(name);
                        }
                    }

                    let res = evaluate_call_dispatch(mc, env, &actual_callback_val, Some(&this_arg), &call_args)?;
                    create_data_property_or_throw(mc, &new_array, i, &res)?;
                }
            }

            Ok(Value::Object(new_array))
        }
        "filter" => {
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            let callback_val = args.first().cloned().unwrap_or(Value::Undefined);

            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                usize::MAX
            } else {
                len_num.floor() as usize
            };

            let callback_callable = match &callback_val {
                Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::Function(_)
                | Value::GeneratorFunction(_, _)
                | Value::AsyncGeneratorFunction(_, _) => true,
                Value::Object(obj) => {
                    obj.borrow().get_closure().is_some()
                        || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                        || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                }
                _ => false,
            };
            if !callback_callable {
                return Err(raise_type_error!("Array.filter callback must be a function").into());
            }

            if current_len > u32::MAX as usize {
                return Err(raise_range_error!("Invalid array length").into());
            }

            let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
            let new_array = array_species_create_impl(mc, env, object, 0.0)?;
            let mut idx = 0usize;

            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            for i in 0..current_len {
                let has_property = if let Some(ta) = typed_array_ptr {
                    let effective_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0 } else { ta.length }
                    };
                    i < effective_len
                } else {
                    object_get_key_value(object, i).is_some()
                };

                if has_property {
                    let element_val = crate::core::get_property_with_accessors(mc, env, object, i)?;
                    let call_args = vec![element_val.clone(), Value::Number(i as f64), Value::Object(*object)];

                    let mut actual_callback_val = callback_val.clone();
                    if let Value::Object(obj) = &callback_val {
                        if let Some(prop) = obj.borrow().get_closure() {
                            actual_callback_val = prop.borrow().clone();
                        } else if let Some(nc) = slot_get_chained(obj, &InternalSlot::NativeCtor)
                            && let Value::String(name_vec) = &*nc.borrow()
                        {
                            let name = crate::unicode::utf16_to_utf8(name_vec);
                            actual_callback_val = Value::Function(name);
                        }
                    }

                    let res = evaluate_call_dispatch(mc, env, &actual_callback_val, Some(&this_arg), &call_args)?;
                    if res.to_truthy() {
                        create_data_property_or_throw(mc, &new_array, idx, &element_val)?;
                        idx += 1;
                    }
                }
            }

            set_array_length(mc, &new_array, idx)?;
            Ok(Value::Object(new_array))
        }
        "reduce" => {
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            let callback_val = args.first().cloned().unwrap_or(Value::Undefined);
            let initial_value = if args.len() >= 2 { Some(args[1].clone()) } else { None };

            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                usize::MAX
            } else {
                len_num.floor() as usize
            };

            let callback_callable = match &callback_val {
                Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::Function(_)
                | Value::GeneratorFunction(_, _)
                | Value::AsyncGeneratorFunction(_, _) => true,
                Value::Object(obj) => {
                    obj.borrow().get_closure().is_some()
                        || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                        || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                }
                _ => false,
            };
            if !callback_callable {
                return Err(raise_type_error!("Array.reduce callback must be a function").into());
            }

            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            let has_property_at = |index: usize| -> bool {
                if let Some(ta) = typed_array_ptr {
                    let effective_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0 } else { ta.length }
                    };
                    index < effective_len
                } else {
                    let key = index.to_string();
                    let mut cur = Some(*object);
                    while let Some(cur_obj) = cur {
                        if crate::core::get_own_property(&cur_obj, key.as_str()).is_some() {
                            return true;
                        }
                        cur = cur_obj.borrow().prototype;
                    }
                    false
                }
            };

            if current_len == 0 && initial_value.is_none() {
                return Err(raise_type_error!("Array.reduce called on empty array with no initial value").into());
            }

            let mut k = 0usize;
            let mut accumulator: Value = if let Some(val) = initial_value {
                val
            } else {
                let mut found = false;
                let mut acc = Value::Undefined;
                while k < current_len {
                    if has_property_at(k) {
                        acc = crate::core::get_property_with_accessors(mc, env, object, k)?;
                        found = true;
                        k += 1;
                        break;
                    }
                    k += 1;
                }
                if !found {
                    return Err(raise_type_error!("Array.reduce called on empty array with no initial value").into());
                }
                acc
            };

            let mut actual_callback_val = callback_val.clone();
            if let Value::Object(obj) = &callback_val {
                if let Some(prop) = obj.borrow().get_closure() {
                    actual_callback_val = prop.borrow().clone();
                } else if let Some(nc) = slot_get_chained(obj, &InternalSlot::NativeCtor)
                    && let Value::String(name_vec) = &*nc.borrow()
                {
                    let name = crate::unicode::utf16_to_utf8(name_vec);
                    actual_callback_val = Value::Function(name);
                }
            }

            while k < current_len {
                if has_property_at(k) {
                    let k_value = crate::core::get_property_with_accessors(mc, env, object, k)?;
                    let call_args = vec![accumulator.clone(), k_value, Value::Number(k as f64), Value::Object(*object)];
                    accumulator = evaluate_call_dispatch(mc, env, &actual_callback_val, Some(&Value::Undefined), &call_args)?;
                }
                k += 1;
            }

            Ok(accumulator)
        }
        "reduceRight" => {
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            let callback_val = args.first().cloned().unwrap_or(Value::Undefined);
            let initial_value = if args.len() >= 2 { Some(args[1].clone()) } else { None };

            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                usize::MAX
            } else {
                len_num.floor() as usize
            };

            let callback_callable = match &callback_val {
                Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::Function(_)
                | Value::GeneratorFunction(_, _)
                | Value::AsyncGeneratorFunction(_, _) => true,
                Value::Object(obj) => {
                    obj.borrow().get_closure().is_some()
                        || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                        || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                }
                _ => false,
            };
            if !callback_callable {
                return Err(raise_type_error!("Array.reduceRight callback must be a function").into());
            }

            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            let has_property_at = |index: usize| -> bool {
                if let Some(ta) = typed_array_ptr {
                    let effective_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0 } else { ta.length }
                    };
                    index < effective_len
                } else {
                    object_get_key_value(object, index).is_some()
                }
            };

            if current_len == 0 && initial_value.is_none() {
                return Err(raise_type_error!("Array.reduceRight called on empty array with no initial value").into());
            }
            if current_len == 0 {
                return Ok(initial_value.unwrap());
            }

            let mut k = current_len.saturating_sub(1);
            let mut accumulator: Value = if let Some(val) = initial_value {
                val
            } else {
                let mut found = false;
                let mut acc = Value::Undefined;
                loop {
                    if has_property_at(k) {
                        acc = crate::core::get_property_with_accessors(mc, env, object, k)?;
                        found = true;
                        break;
                    }
                    if k == 0 {
                        break;
                    }
                    k -= 1;
                }
                if !found {
                    return Err(raise_type_error!("Array.reduceRight called on empty array with no initial value").into());
                }
                if k == 0 {
                    return Ok(acc);
                }
                k -= 1;
                acc
            };

            let mut actual_callback_val = callback_val.clone();
            if let Value::Object(obj) = &callback_val {
                if let Some(prop) = obj.borrow().get_closure() {
                    actual_callback_val = prop.borrow().clone();
                } else if let Some(nc) = slot_get_chained(obj, &InternalSlot::NativeCtor)
                    && let Value::String(name_vec) = &*nc.borrow()
                {
                    let name = crate::unicode::utf16_to_utf8(name_vec);
                    actual_callback_val = Value::Function(name);
                }
            }

            loop {
                if has_property_at(k) {
                    let k_value = crate::core::get_property_with_accessors(mc, env, object, k)?;
                    let call_args = vec![accumulator.clone(), k_value, Value::Number(k as f64), Value::Object(*object)];
                    accumulator = evaluate_call_dispatch(mc, env, &actual_callback_val, Some(&Value::Undefined), &call_args)?;
                }

                if k == 0 {
                    break;
                }
                k -= 1;
            }

            Ok(accumulator)
        }
        "find" => {
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            let callback_val = args.first().cloned().unwrap_or(Value::Undefined);
            let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                usize::MAX
            } else {
                len_num.floor() as usize
            };

            let callback_callable = match &callback_val {
                Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::Function(_)
                | Value::GeneratorFunction(_, _)
                | Value::AsyncGeneratorFunction(_, _) => true,
                Value::Object(obj) => {
                    obj.borrow().get_closure().is_some()
                        || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                        || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                }
                _ => false,
            };
            if !callback_callable {
                return Err(raise_type_error!("Array.find callback must be a function").into());
            }

            let mut actual_callback_val = callback_val.clone();
            if let Value::Object(obj) = &callback_val {
                if let Some(prop) = obj.borrow().get_closure() {
                    actual_callback_val = prop.borrow().clone();
                } else if let Some(nc) = slot_get_chained(obj, &InternalSlot::NativeCtor)
                    && let Value::String(name_vec) = &*nc.borrow()
                {
                    let name = crate::unicode::utf16_to_utf8(name_vec);
                    actual_callback_val = Value::Function(name);
                }
            }

            for i in 0..current_len {
                let element = crate::core::get_property_with_accessors(mc, env, object, i)?;
                let call_args = vec![element.clone(), Value::Number(i as f64), Value::Object(*object)];
                let res = evaluate_call_dispatch(mc, env, &actual_callback_val, Some(&this_arg), &call_args)?;
                if res.to_truthy() {
                    return Ok(element);
                }
            }

            Ok(Value::Undefined)
        }
        "findIndex" => {
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            let callback_val = args.first().cloned().unwrap_or(Value::Undefined);
            let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                usize::MAX
            } else {
                len_num.floor() as usize
            };

            let callback_callable = match &callback_val {
                Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::Function(_)
                | Value::GeneratorFunction(_, _)
                | Value::AsyncGeneratorFunction(_, _) => true,
                Value::Object(obj) => {
                    obj.borrow().get_closure().is_some()
                        || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                        || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                }
                _ => false,
            };
            if !callback_callable {
                return Err(raise_type_error!("Array.findIndex callback must be a function").into());
            }

            let mut actual_callback_val = callback_val.clone();
            if let Value::Object(obj) = &callback_val {
                if let Some(prop) = obj.borrow().get_closure() {
                    actual_callback_val = prop.borrow().clone();
                } else if let Some(nc) = slot_get_chained(obj, &InternalSlot::NativeCtor)
                    && let Value::String(name_vec) = &*nc.borrow()
                {
                    let name = crate::unicode::utf16_to_utf8(name_vec);
                    actual_callback_val = Value::Function(name);
                }
            }

            for i in 0..current_len {
                let element = crate::core::get_property_with_accessors(mc, env, object, i)?;
                let call_args = vec![element, Value::Number(i as f64), Value::Object(*object)];
                let res = evaluate_call_dispatch(mc, env, &actual_callback_val, Some(&this_arg), &call_args)?;
                if res.to_truthy() {
                    return Ok(Value::Number(i as f64));
                }
            }

            Ok(Value::Number(-1.0))
        }
        "some" => {
            let callback = args.first().cloned().unwrap_or(Value::Undefined);

            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                usize::MAX
            } else {
                len_num.floor() as usize
            };

            let callback_callable = match &callback {
                Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::Function(_)
                | Value::GeneratorFunction(_, _)
                | Value::AsyncGeneratorFunction(_, _) => true,
                Value::Object(obj) => {
                    obj.borrow().get_closure().is_some()
                        || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                        || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                }
                _ => false,
            };
            if !callback_callable {
                return Err(raise_type_error!("Array.some callback must be a function").into());
            }

            let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            for i in 0..current_len {
                let has_property = if let Some(ta) = typed_array_ptr {
                    let effective_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0 } else { ta.length }
                    };
                    i < effective_len
                } else {
                    object_get_key_value(object, i).is_some()
                };

                if has_property {
                    let element = crate::core::get_property_with_accessors(mc, env, object, i)?;
                    let call_args = vec![element, Value::Number(i as f64), Value::Object(*object)];

                    let mut actual_callback_val = callback.clone();
                    if let Value::Object(obj) = &callback {
                        if let Some(prop) = obj.borrow().get_closure() {
                            actual_callback_val = prop.borrow().clone();
                        } else if let Some(nc) = slot_get_chained(obj, &InternalSlot::NativeCtor)
                            && let Value::String(name_vec) = &*nc.borrow()
                        {
                            let name = crate::unicode::utf16_to_utf8(name_vec);
                            actual_callback_val = Value::Function(name);
                        }
                    }

                    let res = evaluate_call_dispatch(mc, env, &actual_callback_val, Some(&this_arg), &call_args)?;
                    if res.to_truthy() {
                        return Ok(Value::Boolean(true));
                    }
                }
            }

            Ok(Value::Boolean(false))
        }
        "every" => {
            let callback = args.first().cloned().unwrap_or(Value::Undefined);

            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                usize::MAX
            } else {
                len_num.floor() as usize
            };

            let callback_callable = match &callback {
                Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::Function(_)
                | Value::GeneratorFunction(_, _)
                | Value::AsyncGeneratorFunction(_, _) => true,
                Value::Object(obj) => {
                    obj.borrow().get_closure().is_some()
                        || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                        || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                }
                _ => false,
            };
            if !callback_callable {
                return Err(raise_type_error!("Array.every callback must be a function").into());
            }

            let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            for i in 0..current_len {
                let has_property = if let Some(ta) = typed_array_ptr {
                    let effective_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0 } else { ta.length }
                    };
                    i < effective_len
                } else {
                    object_get_key_value(object, i).is_some()
                };

                if has_property {
                    let element = crate::core::get_property_with_accessors(mc, env, object, i)?;
                    let call_args = vec![element, Value::Number(i as f64), Value::Object(*object)];

                    let mut actual_callback_val = callback.clone();
                    if let Value::Object(obj) = &callback {
                        if let Some(prop) = obj.borrow().get_closure() {
                            actual_callback_val = prop.borrow().clone();
                        } else if let Some(nc) = slot_get_chained(obj, &InternalSlot::NativeCtor)
                            && let Value::String(name_vec) = &*nc.borrow()
                        {
                            let name = crate::unicode::utf16_to_utf8(name_vec);
                            actual_callback_val = Value::Function(name);
                        }
                    }

                    let res = evaluate_call_dispatch(mc, env, &actual_callback_val, Some(&this_arg), &call_args)?;
                    if !res.to_truthy() {
                        return Ok(Value::Boolean(false));
                    }
                }
            }

            Ok(Value::Boolean(true))
        }
        "concat" => {
            let is_array_or_throw = |obj: &JSObjectDataPtr<'gc>| -> Result<bool, EvalError<'gc>> {
                if let Some(proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                    && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                {
                    if proxy.revoked {
                        return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
                    }
                    if let Value::Object(target_obj) = &*proxy.target {
                        return Ok(is_array(mc, target_obj));
                    }
                }
                Ok(is_array(mc, obj))
            };

            let length_of_array_like = |obj: &JSObjectDataPtr<'gc>| -> Result<usize, EvalError<'gc>> {
                let len_val = crate::core::get_property_with_accessors(mc, env, obj, "length")?;
                let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
                let len_num = crate::core::to_number(&len_prim)?;
                let max_len = 9007199254740991.0_f64;
                let len = if len_num.is_nan() || len_num <= 0.0 {
                    0usize
                } else if !len_num.is_finite() {
                    max_len as usize
                } else {
                    len_num.floor().min(max_len) as usize
                };
                Ok(len)
            };

            let is_concat_spreadable = |value: &Value<'gc>| -> Result<bool, EvalError<'gc>> {
                let Value::Object(obj) = value else {
                    return Ok(false);
                };

                if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                    && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                    && let Some(sym_val) = object_get_key_value(sym_obj, "isConcatSpreadable")
                    && let Value::Symbol(spread_sym) = &*sym_val.borrow()
                {
                    let spreadable_val = crate::core::get_property_with_accessors(mc, env, obj, *spread_sym)?;
                    if !matches!(spreadable_val, Value::Undefined) {
                        return Ok(spreadable_val.to_truthy());
                    }
                }

                is_array_or_throw(obj)
            };

            let out = array_species_create_impl(mc, env, object, 0.0)?;
            let mut n = 0usize;
            let mut items: Vec<Value<'gc>> = Vec::with_capacity(args.len() + 1);
            items.push(Value::Object(*object));
            items.extend(args.iter().cloned());

            for item in items {
                if is_concat_spreadable(&item)? {
                    let Value::Object(src_obj) = item else {
                        continue;
                    };
                    let src_len = length_of_array_like(&src_obj)?;
                    for k in 0..src_len {
                        let has_property = object_get_key_value(&src_obj, k.to_string()).is_some();
                        if has_property {
                            let sub = crate::core::get_property_with_accessors(mc, env, &src_obj, k.to_string())?;
                            create_data_property_or_throw(mc, &out, n, &sub)?;
                        }
                        n += 1;
                    }
                } else {
                    create_data_property_or_throw(mc, &out, n, &item)?;
                    n += 1;
                }
            }

            set_array_length(mc, &out, n)?;
            Ok(Value::Object(out))
        }
        "indexOf" => {
            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            let current_len = if let Some(ta) = typed_array_ptr {
                if ta.length_tracking {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    if buf_len <= ta.byte_offset {
                        0
                    } else {
                        (buf_len - ta.byte_offset) / ta.element_size()
                    }
                } else {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    let needed = ta.byte_offset + ta.length * ta.element_size();
                    if buf_len < needed { 0 } else { ta.length }
                }
            } else {
                let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
                let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
                let len_num = crate::core::to_number(&len_prim)?;
                if len_num.is_nan() || len_num <= 0.0 {
                    0usize
                } else if !len_num.is_finite() {
                    if len_num.is_sign_negative() { 0usize } else { usize::MAX }
                } else {
                    len_num.floor() as usize
                }
            };

            if current_len == 0 {
                return Ok(Value::Number(-1.0));
            }

            let search_element = args.first().cloned().unwrap_or(Value::Undefined);
            let from_index = if args.len() > 1 {
                let prim = crate::core::to_primitive(mc, &args[1], "number", env)?;
                let n = crate::core::to_number(&prim)?;
                if n.is_nan() || n == 0.0 {
                    0isize
                } else if n.is_infinite() {
                    if n.is_sign_negative() { isize::MIN } else { isize::MAX }
                } else {
                    n.trunc() as isize
                }
            } else {
                0isize
            };

            let start = if from_index < 0 {
                (current_len as isize + from_index).max(0) as usize
            } else {
                from_index as usize
            };

            let is_typed_array_receiver = typed_array_ptr.is_some();

            if let Some(ta) = typed_array_ptr {
                let number_search = if let Value::Number(n) = &search_element { Some(*n) } else { None };
                let bigint_search = if let Value::BigInt(b) = &search_element {
                    Some((**b).clone())
                } else {
                    None
                };

                let effective_len = if ta.length_tracking {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    if buf_len <= ta.byte_offset {
                        0
                    } else {
                        (buf_len - ta.byte_offset) / ta.element_size()
                    }
                } else {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    let needed = ta.byte_offset + ta.length * ta.element_size();
                    if buf_len < needed { 0 } else { ta.length }
                };

                let end = current_len.min(effective_len);
                if start >= end {
                    return Ok(Value::Number(-1.0));
                }

                match ta.kind {
                    crate::core::TypedArrayKind::BigInt64 | crate::core::TypedArrayKind::BigUint64 => {
                        let Some(search_bi) = bigint_search else {
                            return Ok(Value::Number(-1.0));
                        };
                        let size = 8usize;
                        let base = ta.byte_offset;
                        let buffer = ta.buffer.borrow();
                        let data = buffer.data.lock().unwrap();
                        for i in start..end {
                            let byte_offset = base + i * size;
                            if byte_offset + size > data.len() {
                                break;
                            }
                            let bytes = &data[byte_offset..byte_offset + size];
                            let cur_bi = if matches!(ta.kind, crate::core::TypedArrayKind::BigInt64) {
                                let mut b = [0u8; 8];
                                b.copy_from_slice(bytes);
                                num_bigint::BigInt::from(i64::from_le_bytes(b))
                            } else {
                                let mut b = [0u8; 8];
                                b.copy_from_slice(bytes);
                                num_bigint::BigInt::from(u64::from_le_bytes(b))
                            };
                            if cur_bi == search_bi {
                                return Ok(Value::Number(i as f64));
                            }
                        }
                    }
                    _ => {
                        let Some(search_n) = number_search else {
                            return Ok(Value::Number(-1.0));
                        };
                        if search_n.is_nan() {
                            return Ok(Value::Number(-1.0));
                        }

                        let base = ta.byte_offset;
                        let buffer = ta.buffer.borrow();
                        let data = buffer.data.lock().unwrap();
                        match ta.kind {
                            crate::core::TypedArrayKind::Int8 => {
                                for i in start..end {
                                    let byte_offset = base + i;
                                    if byte_offset >= data.len() {
                                        break;
                                    }
                                    if (data[byte_offset] as i8 as f64) == search_n {
                                        return Ok(Value::Number(i as f64));
                                    }
                                }
                            }
                            crate::core::TypedArrayKind::Uint8 | crate::core::TypedArrayKind::Uint8Clamped => {
                                for i in start..end {
                                    let byte_offset = base + i;
                                    if byte_offset >= data.len() {
                                        break;
                                    }
                                    if (data[byte_offset] as f64) == search_n {
                                        return Ok(Value::Number(i as f64));
                                    }
                                }
                            }
                            crate::core::TypedArrayKind::Int16 => {
                                for i in start..end {
                                    let byte_offset = base + i * 2;
                                    if byte_offset + 2 > data.len() {
                                        break;
                                    }
                                    let mut b = [0u8; 2];
                                    b.copy_from_slice(&data[byte_offset..byte_offset + 2]);
                                    if (i16::from_le_bytes(b) as f64) == search_n {
                                        return Ok(Value::Number(i as f64));
                                    }
                                }
                            }
                            crate::core::TypedArrayKind::Uint16 => {
                                for i in start..end {
                                    let byte_offset = base + i * 2;
                                    if byte_offset + 2 > data.len() {
                                        break;
                                    }
                                    let mut b = [0u8; 2];
                                    b.copy_from_slice(&data[byte_offset..byte_offset + 2]);
                                    if (u16::from_le_bytes(b) as f64) == search_n {
                                        return Ok(Value::Number(i as f64));
                                    }
                                }
                            }
                            crate::core::TypedArrayKind::Int32 => {
                                for i in start..end {
                                    let byte_offset = base + i * 4;
                                    if byte_offset + 4 > data.len() {
                                        break;
                                    }
                                    let mut b = [0u8; 4];
                                    b.copy_from_slice(&data[byte_offset..byte_offset + 4]);
                                    if (i32::from_le_bytes(b) as f64) == search_n {
                                        return Ok(Value::Number(i as f64));
                                    }
                                }
                            }
                            crate::core::TypedArrayKind::Uint32 => {
                                for i in start..end {
                                    let byte_offset = base + i * 4;
                                    if byte_offset + 4 > data.len() {
                                        break;
                                    }
                                    let mut b = [0u8; 4];
                                    b.copy_from_slice(&data[byte_offset..byte_offset + 4]);
                                    if (u32::from_le_bytes(b) as f64) == search_n {
                                        return Ok(Value::Number(i as f64));
                                    }
                                }
                            }
                            crate::core::TypedArrayKind::Float32 => {
                                for i in start..end {
                                    let byte_offset = base + i * 4;
                                    if byte_offset + 4 > data.len() {
                                        break;
                                    }
                                    let mut b = [0u8; 4];
                                    b.copy_from_slice(&data[byte_offset..byte_offset + 4]);
                                    if (f32::from_le_bytes(b) as f64) == search_n {
                                        return Ok(Value::Number(i as f64));
                                    }
                                }
                            }
                            crate::core::TypedArrayKind::Float64 => {
                                for i in start..end {
                                    let byte_offset = base + i * 8;
                                    if byte_offset + 8 > data.len() {
                                        break;
                                    }
                                    let mut b = [0u8; 8];
                                    b.copy_from_slice(&data[byte_offset..byte_offset + 8]);
                                    if f64::from_le_bytes(b) == search_n {
                                        return Ok(Value::Number(i as f64));
                                    }
                                }
                            }
                            crate::core::TypedArrayKind::BigInt64 | crate::core::TypedArrayKind::BigUint64 => unreachable!(),
                        }
                    }
                }

                return Ok(Value::Number(-1.0));
            }

            let can_use_sparse_fast_path = if !is_typed_array_receiver && is_array(mc, object) {
                let mut proto_has_numeric_key = false;
                let mut cur_proto = object.borrow().prototype;
                while let Some(proto) = cur_proto {
                    let has_numeric = {
                        let p = proto.borrow();
                        p.properties.keys().any(|k| match k {
                            PropertyKey::String(s) => s.parse::<usize>().ok().map(|idx| idx.to_string() == *s).unwrap_or(false),
                            _ => false,
                        })
                    };
                    if has_numeric {
                        proto_has_numeric_key = true;
                        break;
                    }
                    cur_proto = proto.borrow().prototype;
                }
                !proto_has_numeric_key
            } else {
                false
            };

            if can_use_sparse_fast_path {
                let (mut indices, only_plain_data_elements): (Vec<usize>, bool) = {
                    let o = object.borrow();
                    let mut idxs = Vec::new();
                    let mut plain_only = true;
                    for (k, v) in &o.properties {
                        if let PropertyKey::String(s) = k
                            && let Ok(idx) = s.parse::<usize>()
                            && idx.to_string() == *s
                            && idx >= start
                            && idx < current_len
                        {
                            if matches!(&*v.borrow(), Value::Property { .. }) {
                                plain_only = false;
                                break;
                            }
                            idxs.push(idx);
                        }
                    }
                    (idxs, plain_only)
                };

                if !only_plain_data_elements {
                    // Fallback to full spec-like scan when accessor/descriptor-backed elements are present.
                } else {
                    indices.sort_unstable();

                    for i in indices {
                        let element = crate::core::get_property_with_accessors(mc, env, object, i)?;
                        let is_match = match (&element, &search_element) {
                            (Value::Number(a), Value::Number(b)) => !a.is_nan() && !b.is_nan() && a == b,
                            _ => values_equal(mc, &element, &search_element),
                        };
                        if is_match {
                            return Ok(Value::Number(i as f64));
                        }
                    }

                    return Ok(Value::Number(-1.0));
                }
            }

            for i in start..current_len {
                // indexOf skips holes, so first check property presence (including prototype chain).
                let has_property = if let Some(ta) = typed_array_ptr {
                    let effective_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0 } else { ta.length }
                    };
                    i < effective_len
                } else {
                    object_get_key_value(object, i).is_some()
                };

                if has_property {
                    let element = crate::core::get_property_with_accessors(mc, env, object, i)?;
                    let is_match = match (&element, &search_element) {
                        (Value::Number(a), Value::Number(b)) => !a.is_nan() && !b.is_nan() && a == b,
                        _ => values_equal(mc, &element, &search_element),
                    };
                    if is_match {
                        return Ok(Value::Number(i as f64));
                    }
                }
            }

            Ok(Value::Number(-1.0))
        }
        "includes" => {
            // ToLength: cap at 2^53-1
            const MAX_SAFE_LEN: usize = (1u64 << 53) as usize - 1; // 9007199254740991
            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                if len_num.is_sign_negative() { 0usize } else { MAX_SAFE_LEN }
            } else {
                (len_num.floor() as usize).min(MAX_SAFE_LEN)
            };

            if current_len == 0 {
                return Ok(Value::Boolean(false));
            }

            let search_element = args.first().cloned().unwrap_or(Value::Undefined);
            let from_index = if args.len() > 1 {
                let prim = crate::core::to_primitive(mc, &args[1], "number", env)?;
                let n = crate::core::to_number(&prim)?;
                if n.is_nan() || n == 0.0 {
                    0isize
                } else if n.is_infinite() {
                    if n.is_sign_negative() { isize::MIN } else { isize::MAX }
                } else {
                    n.trunc() as isize
                }
            } else {
                0isize
            };

            let start = if from_index < 0 {
                (current_len as isize + from_index).max(0) as usize
            } else {
                from_index as usize
            };

            for i in start..current_len {
                let element = crate::core::get_property_with_accessors(mc, env, object, i)?;
                // Array.prototype.includes uses SameValueZero (not SameValue)
                if crate::core::same_value_zero(&element, &search_element) {
                    return Ok(Value::Boolean(true));
                }
            }

            Ok(Value::Boolean(false))
        }
        "sort" => {
            let compare_fn = args.first().cloned().unwrap_or(Value::Undefined);
            let has_compare_fn = !matches!(compare_fn, Value::Undefined);
            if has_compare_fn {
                let callable = match &compare_fn {
                    Value::Closure(_)
                    | Value::AsyncClosure(_)
                    | Value::Function(_)
                    | Value::GeneratorFunction(_, _)
                    | Value::AsyncGeneratorFunction(_, _) => true,
                    Value::Object(obj) => {
                        obj.borrow().get_closure().is_some()
                            || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                            || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                    }
                    _ => false,
                };
                if !callable {
                    return Err(raise_type_error!("The comparison function must be either a function or undefined").into());
                }
            }

            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let max_len = 9007199254740991.0_f64;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                max_len as usize
            } else {
                len_num.floor().min(max_len) as usize
            };

            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            // SortIndexedProperties with holes skipped: collect existing elements first
            // (Get is observable and must run before writing back sorted results).
            let mut items: Vec<Value<'gc>> = Vec::new();
            for i in 0..current_len {
                let has_property = if let Some(ta) = typed_array_ptr {
                    let effective_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0 } else { ta.length }
                    };
                    i < effective_len
                } else if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy) {
                    if let Value::Proxy(proxy) = &*proxy_cell.borrow() {
                        crate::js_proxy::proxy_has_property(mc, proxy, i.to_string())?
                    } else {
                        object_get_key_value(object, i.to_string()).is_some()
                    }
                } else {
                    object_get_key_value(object, i.to_string()).is_some()
                };

                if has_property {
                    let v = crate::core::get_property_with_accessors(mc, env, object, i.to_string())?;
                    items.push(v);
                }
            }

            let compare_items = |a: &Value<'gc>, b: &Value<'gc>| -> Result<std::cmp::Ordering, EvalError<'gc>> {
                // Undefined always sorts to the end for default/normal compare behavior.
                match (a, b) {
                    (Value::Undefined, Value::Undefined) => return Ok(std::cmp::Ordering::Equal),
                    (Value::Undefined, _) => return Ok(std::cmp::Ordering::Greater),
                    (_, Value::Undefined) => return Ok(std::cmp::Ordering::Less),
                    _ => {}
                }

                if has_compare_fn {
                    let compare_args = vec![a.clone(), b.clone()];
                    let this_arg = Value::Undefined;
                    let result = evaluate_call_dispatch(mc, env, &compare_fn, Some(&this_arg), &compare_args)?;
                    let num_prim = crate::core::to_primitive(mc, &result, "number", env)?;
                    let num = crate::core::to_number(&num_prim)?;
                    if num.is_nan() || num == 0.0 {
                        Ok(std::cmp::Ordering::Equal)
                    } else if num < 0.0 {
                        Ok(std::cmp::Ordering::Less)
                    } else {
                        Ok(std::cmp::Ordering::Greater)
                    }
                } else {
                    let a_prim = crate::core::to_primitive(mc, a, "string", env)?;
                    let b_prim = crate::core::to_primitive(mc, b, "string", env)?;
                    if matches!(a_prim, Value::Symbol(_)) || matches!(b_prim, Value::Symbol(_)) {
                        return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
                    }
                    let a_str = crate::core::value_to_string(&a_prim);
                    let b_str = crate::core::value_to_string(&b_prim);
                    Ok(a_str.cmp(&b_str))
                }
            };

            // Stable insertion sort with fallible comparator.
            let mut i = 1usize;
            while i < items.len() {
                let mut j = i;
                while j > 0 {
                    let ord = compare_items(&items[j - 1], &items[j])?;
                    if ord == std::cmp::Ordering::Greater {
                        items.swap(j - 1, j);
                        j -= 1;
                    } else {
                        break;
                    }
                }
                i += 1;
            }

            let set_index_or_throw = |index: usize, value: &Value<'gc>| -> Result<(), EvalError<'gc>> {
                let key = index.to_string();

                if let Some(own) = crate::core::get_own_property(object, key.as_str()) {
                    match &*own.borrow() {
                        Value::Getter(..) => {
                            return Err(raise_type_error!("Cannot set property without setter").into());
                        }
                        Value::Property {
                            setter: Some(setter_fn), ..
                        } => {
                            let setter_args = vec![value.clone()];
                            let _ = evaluate_call_dispatch(mc, env, setter_fn, Some(&Value::Object(*object)), &setter_args)?;
                            return Ok(());
                        }
                        Value::Property {
                            setter: None,
                            getter: Some(_),
                            ..
                        } => {
                            return Err(raise_type_error!("Cannot set property without setter").into());
                        }
                        _ => {}
                    }
                }

                let mut handled_by_setter = false;
                let mut cur_proto = object.borrow().prototype;
                while let Some(proto) = cur_proto {
                    if let Some(prop) = crate::core::get_own_property(&proto, key.as_str()) {
                        match &*prop.borrow() {
                            Value::Getter(..) => {
                                return Err(raise_type_error!("Cannot set property without setter").into());
                            }
                            Value::Property {
                                setter: Some(setter_fn), ..
                            } => {
                                let setter_args = vec![value.clone()];
                                let _ = evaluate_call_dispatch(mc, env, setter_fn, Some(&Value::Object(*object)), &setter_args)?;
                                handled_by_setter = true;
                            }
                            Value::Property {
                                setter: None,
                                getter: Some(_),
                                ..
                            } => {
                                return Err(raise_type_error!("Cannot set property without setter").into());
                            }
                            _ => {}
                        }
                        break;
                    }
                    cur_proto = proto.borrow().prototype;
                }

                if !handled_by_setter && object_set_key_value(mc, object, index, value).is_err() {
                    return Err(raise_type_error!("Cannot set array element").into());
                }

                Ok(())
            };

            for (idx, value) in items.iter().enumerate() {
                set_index_or_throw(idx, value)?;
            }

            // Remove remaining own indexed properties in [itemCount, len).
            let mut idx = items.len();
            while idx < current_len {
                if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy) {
                    if let Value::Proxy(proxy) = &*proxy_cell.borrow() {
                        let key_prop = PropertyKey::from(idx.to_string());
                        let deleted = crate::js_proxy::proxy_delete_property(mc, proxy, &key_prop)?;
                        if !deleted {
                            return Err(raise_type_error!("Cannot delete target property").into());
                        }
                    } else if crate::core::get_own_property(object, idx).is_some() {
                        let key_prop = PropertyKey::from(idx.to_string());
                        if !object.borrow().is_configurable(key_prop.clone()) {
                            return Err(raise_type_error!("Cannot delete target property").into());
                        }
                        object.borrow_mut(mc).properties.shift_remove(&key_prop);
                    }
                } else if crate::core::get_own_property(object, idx).is_some() {
                    let key_prop = PropertyKey::from(idx.to_string());
                    if !object.borrow().is_configurable(key_prop.clone()) {
                        return Err(raise_type_error!("Cannot delete target property").into());
                    }
                    object.borrow_mut(mc).properties.shift_remove(&key_prop);
                }
                idx += 1;
            }

            Ok(Value::Object(*object))
        }
        "reverse" => {
            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let max_len = 9007199254740991.0_f64;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                max_len as usize
            } else {
                len_num.floor().min(max_len) as usize
            };

            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            let has_property_at = |index: usize| -> Result<bool, EvalError<'gc>> {
                if let Some(ta) = typed_array_ptr {
                    let effective_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0 } else { ta.length }
                    };
                    Ok(index < effective_len)
                } else if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy) {
                    if let Value::Proxy(proxy) = &*proxy_cell.borrow() {
                        Ok(crate::js_proxy::proxy_has_property(mc, proxy, index.to_string())?)
                    } else {
                        Ok(object_get_key_value(object, index.to_string()).is_some())
                    }
                } else {
                    Ok(object_get_key_value(object, index.to_string()).is_some())
                }
            };

            let delete_property_or_throw = |index: usize| -> Result<(), EvalError<'gc>> {
                if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                    && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                {
                    let key_prop = PropertyKey::from(index.to_string());
                    let deleted = crate::js_proxy::proxy_delete_property(mc, proxy, &key_prop)?;
                    if !deleted {
                        return Err(raise_type_error!("Cannot delete target property").into());
                    }
                    return Ok(());
                }

                if crate::core::get_own_property(object, index).is_some() {
                    let key_prop = PropertyKey::from(index.to_string());
                    if !object.borrow().is_configurable(key_prop.clone()) {
                        return Err(raise_type_error!("Cannot delete target property").into());
                    }
                    object.borrow_mut(mc).properties.shift_remove(&key_prop);
                }
                Ok(())
            };

            let set_property_or_throw = |index: usize, value: &Value<'gc>| -> Result<(), EvalError<'gc>> {
                if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                    && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                {
                    let key_prop = PropertyKey::from(index.to_string());
                    let ok = proxy_set_property_with_receiver(mc, proxy, &key_prop, value, Some(&Value::Object(*object)))?;
                    if !ok {
                        return Err(raise_type_error!("Cannot set target property").into());
                    }
                    return Ok(());
                }

                object_set_key_value(mc, object, index, value)?;
                Ok(())
            };

            let middle = current_len / 2;
            for lower in 0..middle {
                let upper = current_len - lower - 1;

                let lower_exists = has_property_at(lower)?;
                let lower_value = if lower_exists {
                    Some(crate::core::get_property_with_accessors(mc, env, object, lower.to_string())?)
                } else {
                    None
                };

                let upper_exists = has_property_at(upper)?;
                let upper_value = if upper_exists {
                    Some(crate::core::get_property_with_accessors(mc, env, object, upper.to_string())?)
                } else {
                    None
                };

                if lower_exists && upper_exists {
                    set_property_or_throw(lower, &upper_value.unwrap_or(Value::Undefined))?;
                    set_property_or_throw(upper, &lower_value.unwrap_or(Value::Undefined))?;
                } else if !lower_exists && upper_exists {
                    set_property_or_throw(lower, &upper_value.unwrap_or(Value::Undefined))?;
                    delete_property_or_throw(upper)?;
                } else if lower_exists && !upper_exists {
                    delete_property_or_throw(lower)?;
                    set_property_or_throw(upper, &lower_value.unwrap_or(Value::Undefined))?;
                }
            }

            Ok(Value::Object(*object))
        }
        "splice" => {
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let max_len = 9007199254740991.0_f64;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                max_len as usize
            } else {
                len_num.floor().min(max_len) as usize
            };

            let to_integer_or_infinity = |value: &Value<'gc>| -> Result<f64, EvalError<'gc>> {
                let prim = crate::core::to_primitive(mc, value, "number", env)?;
                let num = crate::core::to_number(&prim)?;
                if num.is_nan() || num == 0.0 {
                    Ok(0.0)
                } else if num.is_infinite() {
                    Ok(num)
                } else {
                    Ok(num.trunc())
                }
            };

            let len_i = current_len as i128;
            let relative_start = if let Some(start_arg) = args.first() {
                to_integer_or_infinity(start_arg)?
            } else {
                0.0
            };
            let actual_start_i = if relative_start == f64::NEG_INFINITY {
                0_i128
            } else if relative_start < 0.0 {
                (len_i + relative_start as i128).max(0)
            } else {
                (relative_start as i128).min(len_i)
            };
            let actual_start = actual_start_i as usize;

            let insert_count = args.len().saturating_sub(2);
            let actual_delete_count = if args.is_empty() {
                0
            } else if args.len() == 1 {
                current_len.saturating_sub(actual_start)
            } else {
                let dc = to_integer_or_infinity(&args[1])?;
                let dc_i = if dc == f64::NEG_INFINITY {
                    0_i128
                } else if dc == f64::INFINITY {
                    (current_len - actual_start) as i128
                } else {
                    (dc as i128).max(0)
                };
                dc_i.min((current_len - actual_start) as i128) as usize
            };

            let set_length_or_throw = |new_len: usize| -> Result<(), EvalError<'gc>> {
                let invoke_length_setter = |setter_val: &Value<'gc>| -> Result<(), EvalError<'gc>> {
                    let arg = Value::Number(new_len as f64);
                    match setter_val {
                        Value::Setter(params, body, captured_env, home_opt) => {
                            let call_env = crate::core::prepare_function_call_env_with_home(
                                mc,
                                Some(captured_env),
                                Some(&Value::Object(*object)),
                                Some(params),
                                std::slice::from_ref(&arg),
                                None,
                                Some(env),
                                home_opt.clone(),
                            )?;
                            let _ = crate::core::evaluate_statements(mc, &call_env, body)?;
                            Ok(())
                        }
                        Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) | Value::Object(_) => {
                            let _ = evaluate_call_dispatch(mc, env, setter_val, Some(&Value::Object(*object)), std::slice::from_ref(&arg))?;
                            Ok(())
                        }
                        _ => Err(raise_type_error!("Cannot assign to read only property 'length'").into()),
                    }
                };

                if let Some(own) = crate::core::get_own_property(object, "length") {
                    let own_val = own.borrow().clone();
                    match &own_val {
                        Value::Setter(..) => {
                            invoke_length_setter(&own_val)?;
                            return Ok(());
                        }
                        Value::Getter(..) => {
                            return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
                        }
                        Value::Property {
                            setter: Some(setter_fn),
                            getter: _,
                            value,
                        } if value.is_none() => {
                            invoke_length_setter(setter_fn)?;
                            return Ok(());
                        }
                        Value::Property {
                            setter: None,
                            getter: Some(_),
                            value,
                        } if value.is_none() => {
                            return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
                        }
                        _ => {}
                    }
                } else {
                    let mut cur_proto = object.borrow().prototype;
                    while let Some(proto) = cur_proto {
                        if let Some(prop) = crate::core::get_own_property(&proto, "length") {
                            let prop_val = prop.borrow().clone();
                            match &prop_val {
                                Value::Setter(..) => {
                                    invoke_length_setter(&prop_val)?;
                                    return Ok(());
                                }
                                Value::Getter(..) => {
                                    return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
                                }
                                Value::Property {
                                    setter: Some(setter_fn),
                                    getter: _,
                                    value,
                                } if value.is_none() => {
                                    invoke_length_setter(setter_fn)?;
                                    return Ok(());
                                }
                                Value::Property {
                                    setter: None,
                                    getter: Some(_),
                                    value,
                                } if value.is_none() => {
                                    return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
                                }
                                _ => {}
                            }
                            break;
                        }
                        cur_proto = proto.borrow().prototype;
                    }
                }

                if !object.borrow().is_writable("length") {
                    return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
                }

                if is_array(mc, object) {
                    if new_len > u32::MAX as usize {
                        return Err(raise_range_error!("Invalid array length").into());
                    }
                    set_array_length(mc, object, new_len).map_err(EvalError::from)?;
                } else if object_set_key_value(mc, object, "length", &Value::Number(new_len as f64)).is_err() {
                    return Err(raise_type_error!("Cannot set length").into());
                }

                Ok(())
            };

            let new_len = current_len - actual_delete_count + insert_count;
            if (new_len as f64) > max_len {
                return Err(raise_type_error!("Invalid array length").into());
            }

            let has_property_at = |index: usize| -> Result<bool, EvalError<'gc>> {
                if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy) {
                    if let Value::Proxy(proxy) = &*proxy_cell.borrow() {
                        Ok(crate::js_proxy::proxy_has_property(mc, proxy, index.to_string())?)
                    } else {
                        Ok(object_get_key_value(object, index.to_string()).is_some())
                    }
                } else {
                    Ok(object_get_key_value(object, index.to_string()).is_some())
                }
            };

            let delete_property_or_throw = |index: usize| -> Result<(), EvalError<'gc>> {
                if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                    && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                {
                    let key_prop = PropertyKey::from(index.to_string());
                    let deleted = crate::js_proxy::proxy_delete_property(mc, proxy, &key_prop)?;
                    if !deleted {
                        return Err(raise_type_error!("Cannot delete target property").into());
                    }
                    return Ok(());
                }

                if crate::core::get_own_property(object, index).is_some() {
                    let key_prop = PropertyKey::from(index.to_string());
                    if !object.borrow().is_configurable(key_prop.clone()) {
                        return Err(raise_type_error!("Cannot delete target property").into());
                    }
                    object.borrow_mut(mc).properties.shift_remove(&key_prop);
                }
                Ok(())
            };

            let deleted_array = array_species_create_impl(mc, env, object, actual_delete_count as f64)?;
            for k in 0..actual_delete_count {
                let from = actual_start + k;
                if has_property_at(from)? {
                    let from_val = crate::core::get_property_with_accessors(mc, env, object, from.to_string())?;
                    create_data_property_or_throw(mc, &deleted_array, k, &from_val)?;
                }
            }
            // Step 12: Perform ? Set(A, "length", actualDeleteCount, true) — must go through [[Set]] for proxy support
            if let Some(proxy_cell) = crate::core::slot_get(&deleted_array, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                let key = PropertyKey::from("length");
                let ok = crate::js_proxy::proxy_set_property(mc, proxy, &key, &Value::Number(actual_delete_count as f64))?;
                if !ok {
                    return Err(raise_type_error!("Cannot set property 'length' on proxy").into());
                }
            } else if is_array(mc, &deleted_array) {
                set_array_length(mc, &deleted_array, actual_delete_count)?;
            } else {
                object_set_key_value(mc, &deleted_array, "length", &Value::Number(actual_delete_count as f64))?;
            }

            if insert_count < actual_delete_count {
                let mut k = actual_start;
                while k < (current_len - actual_delete_count) {
                    let from = k + actual_delete_count;
                    let to = k + insert_count;
                    if has_property_at(from)? {
                        let from_val = crate::core::get_property_with_accessors(mc, env, object, from.to_string())?;
                        object_set_key_value(mc, object, to, &from_val)?;
                    } else {
                        delete_property_or_throw(to)?;
                    }
                    k += 1;
                }

                let mut k = current_len;
                while k > (current_len - actual_delete_count + insert_count) {
                    delete_property_or_throw(k - 1)?;
                    k -= 1;
                }
            } else if insert_count > actual_delete_count {
                let mut k = current_len - actual_delete_count;
                while k > actual_start {
                    let from = k + actual_delete_count - 1;
                    let to = k + insert_count - 1;
                    if has_property_at(from)? {
                        let from_val = crate::core::get_property_with_accessors(mc, env, object, from.to_string())?;
                        object_set_key_value(mc, object, to, &from_val)?;
                    } else {
                        delete_property_or_throw(to)?;
                    }
                    k -= 1;
                }
            }

            let mut item_index = actual_start;
            for item in args.iter().skip(2) {
                object_set_key_value(mc, object, item_index, item)?;
                item_index += 1;
            }

            set_length_or_throw(new_len)?;

            Ok(Value::Object(deleted_array))
        }
        "shift" => {
            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let max_len = 9007199254740991.0_f64;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                max_len as usize
            } else {
                len_num.floor().min(max_len) as usize
            };

            let length_is_non_writable = || -> Result<bool, EvalError<'gc>> {
                let desc = crate::js_object::handle_object_method(
                    mc,
                    "getOwnPropertyDescriptor",
                    &[Value::Object(*object), Value::String(utf8_to_utf16("length"))],
                    env,
                )?;
                if let Value::Object(desc_obj) = desc {
                    let writable = crate::core::get_property_with_accessors(mc, env, &desc_obj, "writable")?;
                    if matches!(writable, Value::Boolean(false)) {
                        return Ok(true);
                    }
                }
                Ok(false)
            };

            let has_property_at = |index: usize| -> Result<bool, EvalError<'gc>> {
                if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy) {
                    if let Value::Proxy(proxy) = &*proxy_cell.borrow() {
                        Ok(crate::js_proxy::proxy_has_property(mc, proxy, index.to_string())?)
                    } else {
                        Ok(object_get_key_value(object, index.to_string()).is_some())
                    }
                } else {
                    Ok(object_get_key_value(object, index.to_string()).is_some())
                }
            };

            let delete_property_or_throw = |index: usize| -> Result<(), EvalError<'gc>> {
                if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                    && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                {
                    let key_prop = PropertyKey::from(index.to_string());
                    let deleted = crate::js_proxy::proxy_delete_property(mc, proxy, &key_prop)?;
                    if !deleted {
                        return Err(raise_type_error!("Cannot delete target property").into());
                    }
                    return Ok(());
                }

                if crate::core::get_own_property(object, index).is_some() {
                    let key_prop = PropertyKey::from(index.to_string());
                    if !object.borrow().is_configurable(key_prop.clone()) {
                        return Err(raise_type_error!("Cannot delete target property").into());
                    }
                    object.borrow_mut(mc).properties.shift_remove(&key_prop);
                }
                Ok(())
            };

            let first = if current_len > 0 {
                crate::core::get_property_with_accessors(mc, env, object, 0)?
            } else {
                Value::Undefined
            };

            if current_len > 0 {
                for k in 1..current_len {
                    let from = k;
                    let to = k - 1;
                    if has_property_at(from)? {
                        let from_val = crate::core::get_property_with_accessors(mc, env, object, from)?;
                        object_set_key_value(mc, object, to, &from_val)?;
                    } else {
                        delete_property_or_throw(to)?;
                    }
                }
                delete_property_or_throw(current_len - 1)?;
            }

            if !object.borrow().is_writable("length") || length_is_non_writable()? {
                return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
            }

            let new_len = current_len.saturating_sub(1);
            if is_array(mc, object) {
                if set_array_length(mc, object, new_len).is_err() {
                    return Err(raise_type_error!("Cannot set length").into());
                }
            } else if object_set_key_value(mc, object, "length", &Value::Number(new_len as f64)).is_err() {
                return Err(raise_type_error!("Cannot set length").into());
            }

            Ok(first)
        }
        "unshift" => {
            if !is_array(mc, object)
                && let Some(wrapped) = slot_get_chained(object, &InternalSlot::PrimitiveValue)
                && matches!(*wrapped.borrow(), Value::String(_))
            {
                return Err(raise_type_error!("Cannot assign to read only property").into());
            }

            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let max_len = 9007199254740991.0_f64;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                max_len as usize
            } else {
                len_num.floor().min(max_len) as usize
            };

            let length_is_non_writable = || -> Result<bool, EvalError<'gc>> {
                let desc = crate::js_object::handle_object_method(
                    mc,
                    "getOwnPropertyDescriptor",
                    &[Value::Object(*object), Value::String(utf8_to_utf16("length"))],
                    env,
                )?;
                if let Value::Object(desc_obj) = desc {
                    let writable = crate::core::get_property_with_accessors(mc, env, &desc_obj, "writable")?;
                    if matches!(writable, Value::Boolean(false)) {
                        return Ok(true);
                    }
                }
                Ok(false)
            };

            let arg_count = args.len();
            if arg_count == 0 {
                if !object.borrow().is_writable("length") || length_is_non_writable()? {
                    return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
                }
                if is_array(mc, object) {
                    if current_len > u32::MAX as usize {
                        return Err(raise_range_error!("Invalid array length").into());
                    }
                    if set_array_length(mc, object, current_len).is_err() {
                        return Err(raise_type_error!("Cannot set length").into());
                    }
                } else if object_set_key_value(mc, object, "length", &Value::Number(current_len as f64)).is_err() {
                    return Err(raise_type_error!("Cannot set length").into());
                }
                return Ok(Value::Number(current_len as f64));
            }

            if (current_len as f64) + (arg_count as f64) > max_len {
                return Err(raise_type_error!("Invalid array length").into());
            }

            if !object.borrow().is_writable("length") || length_is_non_writable()? {
                return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
            }

            let set_index_or_throw = |index: usize, value: &Value<'gc>| -> Result<(), EvalError<'gc>> {
                let key = index.to_string();

                if let Some(own) = crate::core::get_own_property(object, key.as_str()) {
                    match &*own.borrow() {
                        Value::Getter(..) => {
                            return Err(raise_type_error!("Cannot set property without setter").into());
                        }
                        Value::Property {
                            setter: Some(setter_fn), ..
                        } => {
                            let setter_args = vec![value.clone()];
                            let _ = evaluate_call_dispatch(mc, env, setter_fn, Some(&Value::Object(*object)), &setter_args)?;
                            return Ok(());
                        }
                        Value::Property {
                            setter: None,
                            getter: Some(_),
                            ..
                        } => {
                            return Err(raise_type_error!("Cannot set property without setter").into());
                        }
                        _ => {
                            if !object.borrow().is_writable(key.as_str()) {
                                return Err(raise_type_error!("Cannot assign to read only property").into());
                            }
                        }
                    }
                }

                let mut handled_by_setter = false;
                let mut cur_proto = object.borrow().prototype;
                while let Some(proto) = cur_proto {
                    if let Some(prop) = crate::core::get_own_property(&proto, key.as_str()) {
                        match &*prop.borrow() {
                            Value::Getter(..) => {
                                return Err(raise_type_error!("Cannot set property without setter").into());
                            }
                            Value::Property {
                                setter: Some(setter_fn), ..
                            } => {
                                let setter_args = vec![value.clone()];
                                let _ = evaluate_call_dispatch(mc, env, setter_fn, Some(&Value::Object(*object)), &setter_args)?;
                                handled_by_setter = true;
                            }
                            Value::Property {
                                setter: None,
                                getter: Some(_),
                                ..
                            } => {
                                return Err(raise_type_error!("Cannot set property without setter").into());
                            }
                            _ => {}
                        }
                        break;
                    }
                    cur_proto = proto.borrow().prototype;
                }

                if !handled_by_setter && object_set_key_value(mc, object, index, value).is_err() {
                    return Err(raise_type_error!("Cannot set array element").into());
                }

                Ok(())
            };

            let has_property_at = |index: usize| -> Result<bool, EvalError<'gc>> {
                if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy) {
                    if let Value::Proxy(proxy) = &*proxy_cell.borrow() {
                        Ok(crate::js_proxy::proxy_has_property(mc, proxy, index.to_string())?)
                    } else {
                        Ok(object_get_key_value(object, index.to_string()).is_some())
                    }
                } else {
                    Ok(object_get_key_value(object, index.to_string()).is_some())
                }
            };

            let delete_property_or_throw = |index: usize| -> Result<(), EvalError<'gc>> {
                if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                    && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                {
                    let key_prop = PropertyKey::from(index.to_string());
                    let deleted = crate::js_proxy::proxy_delete_property(mc, proxy, &key_prop)?;
                    if !deleted {
                        return Err(raise_type_error!("Cannot delete target property").into());
                    }
                    return Ok(());
                }

                if crate::core::get_own_property(object, index).is_some() {
                    let key_prop = PropertyKey::from(index.to_string());
                    if !object.borrow().is_configurable(key_prop.clone()) {
                        return Err(raise_type_error!("Cannot delete target property").into());
                    }
                    object.borrow_mut(mc).properties.shift_remove(&key_prop);
                }
                Ok(())
            };

            if current_len > 0 {
                let mut k = current_len;
                while k > 0 {
                    let from = k - 1;
                    let to = from + arg_count;
                    if has_property_at(from)? {
                        let from_val = crate::core::get_property_with_accessors(mc, env, object, from)?;
                        set_index_or_throw(to, &from_val)?;
                    } else {
                        delete_property_or_throw(to)?;
                    }
                    k -= 1;
                }
            }

            for (j, arg) in args.iter().enumerate() {
                set_index_or_throw(j, arg)?;
            }

            if !object.borrow().is_writable("length") || length_is_non_writable()? {
                return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
            }

            let new_len = current_len + arg_count;
            if is_array(mc, object) {
                if new_len > u32::MAX as usize {
                    return Err(raise_range_error!("Invalid array length").into());
                }
                if set_array_length(mc, object, new_len).is_err() {
                    return Err(raise_type_error!("Cannot set length").into());
                }
            } else if object_set_key_value(mc, object, "length", &Value::Number(new_len as f64)).is_err() {
                return Err(raise_type_error!("Cannot set length").into());
            }

            Ok(Value::Number(new_len as f64))
        }
        "fill" => {
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            let fill_value = args.first().cloned().unwrap_or(Value::Undefined);

            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            let current_len = if let Some(ta) = typed_array_ptr {
                if ta.length_tracking {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    if buf_len <= ta.byte_offset {
                        0
                    } else {
                        (buf_len - ta.byte_offset) / ta.element_size()
                    }
                } else {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    let needed = ta.byte_offset + ta.length * ta.element_size();
                    if buf_len < needed { 0 } else { ta.length }
                }
            } else {
                let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
                let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
                let len_num = crate::core::to_number(&len_prim)?;
                if len_num.is_nan() || len_num <= 0.0 {
                    0usize
                } else if !len_num.is_finite() {
                    usize::MAX
                } else {
                    len_num.floor() as usize
                }
            };

            let to_integer_or_infinity = |value: &Value<'gc>| -> Result<f64, EvalError<'gc>> {
                let prim = crate::core::to_primitive(mc, value, "number", env)?;
                let num = crate::core::to_number(&prim)?;
                if num.is_nan() || num == 0.0 {
                    Ok(0.0)
                } else if num.is_infinite() {
                    Ok(num)
                } else {
                    Ok(num.trunc())
                }
            };

            let relative_start = if let Some(start_arg) = args.get(1) {
                to_integer_or_infinity(start_arg)?
            } else {
                0.0
            };

            let len_f = current_len as f64;
            let start = if relative_start == f64::NEG_INFINITY {
                0usize
            } else if relative_start < 0.0 {
                (len_f + relative_start).max(0.0).min(len_f) as usize
            } else {
                relative_start.min(len_f) as usize
            };

            let relative_end = if let Some(end_arg) = args.get(2) {
                if matches!(end_arg, Value::Undefined) {
                    len_f
                } else {
                    to_integer_or_infinity(end_arg)?
                }
            } else {
                len_f
            };

            let end = if relative_end == f64::NEG_INFINITY {
                0usize
            } else if relative_end < 0.0 {
                (len_f + relative_end).max(0.0).min(len_f) as usize
            } else {
                relative_end.min(len_f) as usize
            };

            let mut k = start;
            while k < end {
                if let Some(ta) = typed_array_ptr {
                    let _ = crate::core::to_primitive(mc, &fill_value, "number", env)?;

                    let effective_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0 } else { ta.length }
                    };

                    if k >= effective_len {
                        break;
                    }
                }

                if let Some(existing_prop) = crate::core::get_own_property(object, k)
                    && let Value::Property {
                        value: _,
                        getter: _,
                        setter: Some(setter_fn),
                    } = &*existing_prop.borrow()
                {
                    let setter_args = vec![fill_value.clone()];
                    let _ = evaluate_call_dispatch(mc, env, setter_fn, Some(&Value::Object(*object)), &setter_args)?;
                    k += 1;
                    continue;
                }

                object_set_key_value(mc, object, k, &fill_value)?;
                k += 1;
            }

            Ok(Value::Object(*object))
        }
        "lastIndexOf" => {
            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            let search_element = args.first().cloned().unwrap_or(Value::Undefined);

            let current_len = if let Some(ta) = typed_array_ptr {
                if ta.length_tracking {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    if buf_len <= ta.byte_offset {
                        0
                    } else {
                        (buf_len - ta.byte_offset) / ta.element_size()
                    }
                } else {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    let needed = ta.byte_offset + ta.length * ta.element_size();
                    if buf_len < needed { 0 } else { ta.length }
                }
            } else {
                let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
                let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
                let len_num = crate::core::to_number(&len_prim)?;
                if len_num.is_nan() || len_num <= 0.0 {
                    0usize
                } else if !len_num.is_finite() {
                    if len_num.is_sign_negative() { 0usize } else { usize::MAX }
                } else {
                    len_num.floor() as usize
                }
            };

            if current_len == 0 {
                return Ok(Value::Number(-1.0));
            }

            let from_index = if args.len() > 1 {
                let prim = crate::core::to_primitive(mc, &args[1], "number", env)?;
                let n = crate::core::to_number(&prim)?;
                if n.is_nan() || n == 0.0 {
                    0isize
                } else if n.is_infinite() {
                    if n.is_sign_negative() { isize::MIN } else { isize::MAX }
                } else {
                    n.trunc() as isize
                }
            } else {
                current_len.saturating_sub(1) as isize
            };

            let mut k = if from_index == isize::MAX {
                current_len.saturating_sub(1) as isize
            } else if from_index >= 0 {
                std::cmp::min(from_index as usize, current_len.saturating_sub(1)) as isize
            } else {
                current_len as isize + from_index
            };

            if k < 0 {
                return Ok(Value::Number(-1.0));
            }

            if let Some(ta) = typed_array_ptr {
                let number_search = if let Value::Number(n) = &search_element { Some(*n) } else { None };
                let bigint_search = if let Value::BigInt(b) = &search_element {
                    Some((**b).clone())
                } else {
                    None
                };

                let effective_len = if ta.length_tracking {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    if buf_len <= ta.byte_offset {
                        0
                    } else {
                        (buf_len - ta.byte_offset) / ta.element_size()
                    }
                } else {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    let needed = ta.byte_offset + ta.length * ta.element_size();
                    if buf_len < needed { 0 } else { ta.length }
                };

                if effective_len == 0 {
                    return Ok(Value::Number(-1.0));
                }

                let start = std::cmp::min(k as usize, current_len.saturating_sub(1));
                if start >= effective_len && effective_len == 0 {
                    return Ok(Value::Number(-1.0));
                }
                let mut idx = std::cmp::min(start, effective_len.saturating_sub(1));

                match ta.kind {
                    crate::core::TypedArrayKind::BigInt64 | crate::core::TypedArrayKind::BigUint64 => {
                        let Some(search_bi) = bigint_search else {
                            return Ok(Value::Number(-1.0));
                        };
                        let size = 8usize;
                        let base = ta.byte_offset;
                        let buffer = ta.buffer.borrow();
                        let data = buffer.data.lock().unwrap();
                        loop {
                            let byte_offset = base + idx * size;
                            if byte_offset + size > data.len() {
                                if idx == 0 {
                                    break;
                                }
                                idx -= 1;
                                continue;
                            }
                            let bytes = &data[byte_offset..byte_offset + size];
                            let cur_bi = if matches!(ta.kind, crate::core::TypedArrayKind::BigInt64) {
                                let mut b = [0u8; 8];
                                b.copy_from_slice(bytes);
                                num_bigint::BigInt::from(i64::from_le_bytes(b))
                            } else {
                                let mut b = [0u8; 8];
                                b.copy_from_slice(bytes);
                                num_bigint::BigInt::from(u64::from_le_bytes(b))
                            };
                            if cur_bi == search_bi {
                                return Ok(Value::Number(idx as f64));
                            }
                            if idx == 0 {
                                break;
                            }
                            idx -= 1;
                        }
                    }
                    _ => {
                        let Some(search_n) = number_search else {
                            return Ok(Value::Number(-1.0));
                        };
                        if search_n.is_nan() {
                            return Ok(Value::Number(-1.0));
                        }

                        let base = ta.byte_offset;
                        let buffer = ta.buffer.borrow();
                        let data = buffer.data.lock().unwrap();
                        loop {
                            let matches = match ta.kind {
                                crate::core::TypedArrayKind::Int8 => {
                                    let byte_offset = base + idx;
                                    byte_offset < data.len() && (data[byte_offset] as i8 as f64) == search_n
                                }
                                crate::core::TypedArrayKind::Uint8 | crate::core::TypedArrayKind::Uint8Clamped => {
                                    let byte_offset = base + idx;
                                    byte_offset < data.len() && (data[byte_offset] as f64) == search_n
                                }
                                crate::core::TypedArrayKind::Int16 => {
                                    let byte_offset = base + idx * 2;
                                    if byte_offset + 2 > data.len() {
                                        false
                                    } else {
                                        let mut b = [0u8; 2];
                                        b.copy_from_slice(&data[byte_offset..byte_offset + 2]);
                                        (i16::from_le_bytes(b) as f64) == search_n
                                    }
                                }
                                crate::core::TypedArrayKind::Uint16 => {
                                    let byte_offset = base + idx * 2;
                                    if byte_offset + 2 > data.len() {
                                        false
                                    } else {
                                        let mut b = [0u8; 2];
                                        b.copy_from_slice(&data[byte_offset..byte_offset + 2]);
                                        (u16::from_le_bytes(b) as f64) == search_n
                                    }
                                }
                                crate::core::TypedArrayKind::Int32 => {
                                    let byte_offset = base + idx * 4;
                                    if byte_offset + 4 > data.len() {
                                        false
                                    } else {
                                        let mut b = [0u8; 4];
                                        b.copy_from_slice(&data[byte_offset..byte_offset + 4]);
                                        (i32::from_le_bytes(b) as f64) == search_n
                                    }
                                }
                                crate::core::TypedArrayKind::Uint32 => {
                                    let byte_offset = base + idx * 4;
                                    if byte_offset + 4 > data.len() {
                                        false
                                    } else {
                                        let mut b = [0u8; 4];
                                        b.copy_from_slice(&data[byte_offset..byte_offset + 4]);
                                        (u32::from_le_bytes(b) as f64) == search_n
                                    }
                                }
                                crate::core::TypedArrayKind::Float32 => {
                                    let byte_offset = base + idx * 4;
                                    if byte_offset + 4 > data.len() {
                                        false
                                    } else {
                                        let mut b = [0u8; 4];
                                        b.copy_from_slice(&data[byte_offset..byte_offset + 4]);
                                        (f32::from_le_bytes(b) as f64) == search_n
                                    }
                                }
                                crate::core::TypedArrayKind::Float64 => {
                                    let byte_offset = base + idx * 8;
                                    if byte_offset + 8 > data.len() {
                                        false
                                    } else {
                                        let mut b = [0u8; 8];
                                        b.copy_from_slice(&data[byte_offset..byte_offset + 8]);
                                        f64::from_le_bytes(b) == search_n
                                    }
                                }
                                crate::core::TypedArrayKind::BigInt64 | crate::core::TypedArrayKind::BigUint64 => unreachable!(),
                            };

                            if matches {
                                return Ok(Value::Number(idx as f64));
                            }
                            if idx == 0 {
                                break;
                            }
                            idx -= 1;
                        }
                    }
                }

                return Ok(Value::Number(-1.0));
            }

            let can_use_sparse_fast_path = if is_array(mc, object) {
                let mut proto_has_numeric_key = false;
                let mut cur_proto = object.borrow().prototype;
                while let Some(proto) = cur_proto {
                    let has_numeric = {
                        let p = proto.borrow();
                        p.properties.keys().any(|key| match key {
                            PropertyKey::String(s) => s.parse::<usize>().ok().map(|idx| idx.to_string() == *s).unwrap_or(false),
                            _ => false,
                        })
                    };
                    if has_numeric {
                        proto_has_numeric_key = true;
                        break;
                    }
                    cur_proto = proto.borrow().prototype;
                }
                !proto_has_numeric_key
            } else {
                false
            };

            if can_use_sparse_fast_path {
                let start = k as usize;
                let (mut indices, only_plain_data_elements): (Vec<usize>, bool) = {
                    let o = object.borrow();
                    let mut idxs = Vec::new();
                    let mut plain_only = true;
                    for (prop_key, value) in &o.properties {
                        if let PropertyKey::String(s) = prop_key
                            && let Ok(idx) = s.parse::<usize>()
                            && idx.to_string() == *s
                            && idx <= start
                            && idx < current_len
                        {
                            if matches!(&*value.borrow(), Value::Property { .. }) {
                                plain_only = false;
                                break;
                            }
                            idxs.push(idx);
                        }
                    }
                    (idxs, plain_only)
                };

                if only_plain_data_elements {
                    indices.sort_unstable();
                    for idx in indices.into_iter().rev() {
                        let element = crate::core::get_property_with_accessors(mc, env, object, idx)?;
                        let is_match = match (&element, &search_element) {
                            (Value::Number(a), Value::Number(b)) => !a.is_nan() && !b.is_nan() && a == b,
                            _ => values_equal(mc, &element, &search_element),
                        };
                        if is_match {
                            return Ok(Value::Number(idx as f64));
                        }
                    }
                    return Ok(Value::Number(-1.0));
                }
            }

            while k >= 0 {
                let idx = k as usize;
                let has_property = object_get_key_value(object, idx).is_some();
                if has_property {
                    let element = crate::core::get_property_with_accessors(mc, env, object, idx)?;
                    let is_match = match (&element, &search_element) {
                        (Value::Number(a), Value::Number(b)) => !a.is_nan() && !b.is_nan() && a == b,
                        _ => values_equal(mc, &element, &search_element),
                    };
                    if is_match {
                        return Ok(Value::Number(idx as f64));
                    }
                }
                k -= 1;
            }

            Ok(Value::Number(-1.0))
        }
        "toString" => {
            let join_method = crate::core::get_property_with_accessors(mc, env, object, "join")?;
            let join_callable = match &join_method {
                Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::Function(_)
                | Value::GeneratorFunction(_, _)
                | Value::AsyncGeneratorFunction(_, _) => true,
                Value::Object(obj) => {
                    obj.borrow().get_closure().is_some()
                        || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                        || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                }
                _ => false,
            };

            if join_callable {
                let this_arg = Value::Object(*object);
                return evaluate_call_dispatch(mc, env, &join_method, Some(&this_arg), &[]);
            }

            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                && let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
                && let Value::Symbol(s) = &*tag_sym.borrow()
            {
                let _ = crate::core::get_property_with_accessors(mc, env, object, *s)?;
            }

            Ok(crate::core::handle_object_prototype_to_string(mc, &Value::Object(*object), env)?)
        }
        "toLocaleString" => {
            let length_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let current_len = match length_val {
                Value::Number(n) if n.is_finite() && n > 0.0 => n.floor() as usize,
                _ => 0,
            };

            let get_primitive_locale_method = |value: &Value<'gc>| -> Option<Value<'gc>> {
                let ctor_name = match value {
                    Value::Boolean(_) => "Boolean",
                    Value::Number(_) => "Number",
                    Value::String(_) => "String",
                    Value::BigInt(_) => "BigInt",
                    Value::Symbol(_) => "Symbol",
                    _ => return None,
                };
                if let Some(ctor_rc) = env_get(env, ctor_name)
                    && let Value::Object(ctor) = &*ctor_rc.borrow()
                    && let Some(proto_rc) = object_get_key_value(ctor, "prototype")
                    && let Value::Object(proto) = &*proto_rc.borrow()
                    && let Ok(method) = crate::core::get_property_with_accessors(mc, env, proto, "toLocaleString")
                {
                    return Some(method);
                }
                None
            };

            let mut result = String::new();
            for i in 0..current_len {
                if i > 0 {
                    result.push(',');
                }
                let key = i.to_string();
                let element = crate::core::get_property_with_accessors(mc, env, object, key.as_str())?;
                match element {
                    Value::Undefined | Value::Null => {}
                    other => {
                        let method = if let Value::Object(o) = &other {
                            crate::core::get_property_with_accessors(mc, env, o, "toLocaleString")?
                        } else if let Some(m) = get_primitive_locale_method(&other) {
                            m
                        } else {
                            Value::Undefined
                        };

                        if !matches!(method, Value::Function(_) | Value::Closure(_) | Value::Object(_)) {
                            return Err(raise_type_error!("Array.prototype.toLocaleString element method is not callable").into());
                        }

                        let locale_str = evaluate_call_dispatch(mc, env, &method, Some(&other), &[])?;
                        result.push_str(&value_to_sort_string(&locale_str));
                    }
                }
            }
            Ok(Value::String(utf8_to_utf16(&result)))
        }
        "flat" => {
            let depth = if !args.is_empty() {
                match args[0].clone() {
                    Value::Number(n) => n as usize,
                    _ => 1,
                }
            } else {
                1
            };

            let mut result = Vec::new();
            flatten_array(mc, object, &mut result, depth)?;

            let new_array = array_species_create_impl(mc, env, object, 0.0)?;
            for (i, val) in result.iter().enumerate() {
                create_data_property_or_throw(mc, &new_array, i, val)?;
            }
            set_array_length(mc, &new_array, result.len())?;
            Ok(Value::Object(new_array))
        }
        "flatMap" => {
            if args.is_empty() {
                return Err(raise_eval_error!("Array.flatMap expects at least one argument").into());
            }

            let callback_val = args[0].clone();
            let current_len = get_array_length(mc, object).unwrap_or(0);

            let mut result = Vec::new();
            for i in 0..current_len {
                if let Some(val) = object_get_key_value(object, i) {
                    // Support inline closures wrapped as objects with internal closure.
                    let actual_func = if let Value::Object(obj) = &callback_val {
                        if let Some(prop) = obj.borrow().get_closure() {
                            prop.borrow().clone()
                        } else {
                            callback_val.clone()
                        }
                    } else {
                        callback_val.clone()
                    };

                    let args = vec![val.borrow().clone(), Value::Number(i as f64), Value::Object(*object)];

                    let mapped_val = match &actual_func {
                        Value::Closure(cl) => crate::core::call_closure(mc, cl, None, &args, env, None)?,
                        Value::Function(name) => crate::js_function::handle_global_function(mc, name, &args, env)?,
                        _ => return Err(raise_eval_error!("Array.flatMap expects a function").into()),
                    };

                    flatten_single_value(mc, &mapped_val, &mut result, 1)?;
                }
            }

            let new_array = array_species_create_impl(mc, env, object, 0.0)?;
            for (i, val) in result.iter().enumerate() {
                create_data_property_or_throw(mc, &new_array, i, val)?;
            }
            set_array_length(mc, &new_array, result.len())?;
            Ok(Value::Object(new_array))
        }
        "copyWithin" => {
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            let typed_array_ptr = if let Some(ta_cell) = slot_get_chained(object, &InternalSlot::TypedArray) {
                if let Value::TypedArray(ta) = &*ta_cell.borrow() {
                    Some(*ta)
                } else {
                    None
                }
            } else {
                None
            };

            let current_len = if let Some(ta) = typed_array_ptr {
                if ta.length_tracking {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    if buf_len <= ta.byte_offset {
                        0usize
                    } else {
                        (buf_len - ta.byte_offset) / ta.element_size()
                    }
                } else {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    let needed = ta.byte_offset + ta.length * ta.element_size();
                    if buf_len < needed { 0usize } else { ta.length }
                }
            } else {
                let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
                let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
                let len_num = crate::core::to_number(&len_prim)?;
                let max_len = 9007199254740991.0_f64;
                if len_num.is_nan() || len_num <= 0.0 {
                    0usize
                } else if !len_num.is_finite() {
                    max_len as usize
                } else {
                    len_num.floor().min(max_len) as usize
                }
            };

            if args.is_empty() {
                return Ok(Value::Object(*object));
            }

            let to_integer_or_infinity = |value: &Value<'gc>| -> Result<f64, EvalError<'gc>> {
                let prim = crate::core::to_primitive(mc, value, "number", env)?;
                let num = crate::core::to_number(&prim)?;
                if num.is_nan() || num == 0.0 {
                    Ok(0.0)
                } else if num.is_infinite() {
                    Ok(num)
                } else {
                    Ok(num.trunc())
                }
            };

            let len_i = current_len as i128;
            let relative_target = to_integer_or_infinity(&args[0])?;
            let to = if relative_target == f64::NEG_INFINITY {
                0_i128
            } else if relative_target < 0.0 {
                (len_i + relative_target as i128).max(0)
            } else {
                (relative_target as i128).min(len_i)
            };

            let relative_start = if let Some(start_arg) = args.get(1) {
                to_integer_or_infinity(start_arg)?
            } else {
                0.0
            };
            let from = if relative_start == f64::NEG_INFINITY {
                0_i128
            } else if relative_start < 0.0 {
                (len_i + relative_start as i128).max(0)
            } else {
                (relative_start as i128).min(len_i)
            };

            let relative_end = if let Some(end_arg) = args.get(2) {
                if matches!(end_arg, Value::Undefined) {
                    len_i as f64
                } else {
                    to_integer_or_infinity(end_arg)?
                }
            } else {
                len_i as f64
            };
            let final_i = if relative_end == f64::NEG_INFINITY {
                0_i128
            } else if relative_end < 0.0 {
                (len_i + relative_end as i128).max(0)
            } else {
                (relative_end as i128).min(len_i)
            };

            let mut count = (final_i - from).max(0).min((len_i - to).max(0));
            if count <= 0 {
                return Ok(Value::Object(*object));
            }

            let mut from_idx = from;
            let mut to_idx = to;
            let direction: i128 = if from_idx < to_idx && to_idx < from_idx + count {
                from_idx += count - 1;
                to_idx += count - 1;
                -1
            } else {
                1
            };

            while count > 0 {
                let from_key = from_idx as usize;
                let to_key = to_idx as usize;

                let has_from = if let Some(ta) = typed_array_ptr {
                    let effective_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0usize
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0usize } else { ta.length }
                    };
                    from_key < effective_len
                } else if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy) {
                    if let Value::Proxy(proxy) = &*proxy_cell.borrow() {
                        crate::js_proxy::proxy_has_property(mc, proxy, from_key.to_string())?
                    } else {
                        object_get_key_value(object, from_key.to_string()).is_some()
                    }
                } else {
                    object_get_key_value(object, from_key.to_string()).is_some()
                };

                if has_from {
                    let from_val = crate::core::get_property_with_accessors(mc, env, object, from_key.to_string())?;

                    if let Some(existing_prop) = crate::core::get_own_property(object, to_key)
                        && let Value::Property {
                            value: _,
                            getter: _,
                            setter: Some(setter_fn),
                        } = &*existing_prop.borrow()
                    {
                        let setter_args = vec![from_val];
                        let _ = evaluate_call_dispatch(mc, env, setter_fn, Some(&Value::Object(*object)), &setter_args)?;
                    } else {
                        object_set_key_value(mc, object, to_key, &from_val)?;
                    }
                } else if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy) {
                    if let Value::Proxy(proxy) = &*proxy_cell.borrow() {
                        let to_key_prop = PropertyKey::from(to_key.to_string());
                        let deleted = crate::js_proxy::proxy_delete_property(mc, proxy, &to_key_prop)?;
                        if !deleted {
                            return Err(raise_type_error!("Cannot delete target property").into());
                        }
                    } else if object.borrow().non_configurable.contains(&PropertyKey::from(to_key.to_string())) {
                        return Err(raise_type_error!("Cannot delete target property").into());
                    } else {
                        let _ = object
                            .borrow_mut(mc)
                            .properties
                            .shift_remove(&PropertyKey::from(to_key.to_string()));
                    }
                } else if object.borrow().non_configurable.contains(&PropertyKey::from(to_key.to_string())) {
                    return Err(raise_type_error!("Cannot delete target property").into());
                } else {
                    let _ = object
                        .borrow_mut(mc)
                        .properties
                        .shift_remove(&PropertyKey::from(to_key.to_string()));
                }

                from_idx += direction;
                to_idx += direction;
                count -= 1;
            }

            Ok(Value::Object(*object))
        }
        "keys" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Array.prototype.keys takes no arguments").into());
            }
            Ok(create_array_iterator(mc, env, *object, "keys")?)
        }
        "values" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Array.prototype.values takes no arguments").into());
            }
            Ok(create_array_iterator(mc, env, *object, "values")?)
        }
        "entries" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Array.prototype.entries takes no arguments").into());
            }
            Ok(create_array_iterator(mc, env, *object, "entries")?)
        }
        "findLast" => {
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            let callback_val = args.first().cloned().unwrap_or(Value::Undefined);
            let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                usize::MAX
            } else {
                len_num.floor() as usize
            };

            let callback_callable = match &callback_val {
                Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::Function(_)
                | Value::GeneratorFunction(_, _)
                | Value::AsyncGeneratorFunction(_, _) => true,
                Value::Object(obj) => {
                    obj.borrow().get_closure().is_some()
                        || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                        || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                }
                _ => false,
            };
            if !callback_callable {
                return Err(raise_type_error!("Array.findLast callback must be a function").into());
            }

            let mut actual_callback_val = callback_val.clone();
            if let Value::Object(obj) = &callback_val {
                if let Some(prop) = obj.borrow().get_closure() {
                    actual_callback_val = prop.borrow().clone();
                } else if let Some(nc) = slot_get_chained(obj, &InternalSlot::NativeCtor)
                    && let Value::String(name_vec) = &*nc.borrow()
                {
                    let name = crate::unicode::utf16_to_utf8(name_vec);
                    actual_callback_val = Value::Function(name);
                }
            }

            for i in (0..current_len).rev() {
                let element = crate::core::get_property_with_accessors(mc, env, object, i)?;
                let call_args = vec![element.clone(), Value::Number(i as f64), Value::Object(*object)];
                let res = evaluate_call_dispatch(mc, env, &actual_callback_val, Some(&this_arg), &call_args)?;
                if res.to_truthy() {
                    return Ok(element);
                }
            }

            Ok(Value::Undefined)
        }
        "findLastIndex" => {
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                && proxy.revoked
            {
                return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
            }

            let callback_val = args.first().cloned().unwrap_or(Value::Undefined);
            let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

            let len_val = crate::core::get_property_with_accessors(mc, env, object, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            let current_len = if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                usize::MAX
            } else {
                len_num.floor() as usize
            };

            let callback_callable = match &callback_val {
                Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::Function(_)
                | Value::GeneratorFunction(_, _)
                | Value::AsyncGeneratorFunction(_, _) => true,
                Value::Object(obj) => {
                    obj.borrow().get_closure().is_some()
                        || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                        || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                }
                _ => false,
            };
            if !callback_callable {
                return Err(raise_type_error!("Array.findLastIndex callback must be a function").into());
            }

            let mut actual_callback_val = callback_val.clone();
            if let Value::Object(obj) = &callback_val {
                if let Some(prop) = obj.borrow().get_closure() {
                    actual_callback_val = prop.borrow().clone();
                } else if let Some(nc) = slot_get_chained(obj, &InternalSlot::NativeCtor)
                    && let Value::String(name_vec) = &*nc.borrow()
                {
                    let name = crate::unicode::utf16_to_utf8(name_vec);
                    actual_callback_val = Value::Function(name);
                }
            }

            for i in (0..current_len).rev() {
                let element = crate::core::get_property_with_accessors(mc, env, object, i)?;
                let call_args = vec![element, Value::Number(i as f64), Value::Object(*object)];
                let res = evaluate_call_dispatch(mc, env, &actual_callback_val, Some(&this_arg), &call_args)?;
                if res.to_truthy() {
                    return Ok(Value::Number(i as f64));
                }
            }

            Ok(Value::Number(-1.0))
        }
        _ => Err(raise_eval_error!(format!("Array.{method} not found")).into()),
    }
}

// Helper functions for array flattening
fn flatten_array<'gc>(
    mc: &MutationContext<'gc>,
    object: &JSObjectDataPtr<'gc>,
    result: &mut Vec<Value<'gc>>,
    depth: usize,
) -> Result<(), JSError> {
    let current_len = get_array_length(mc, object).unwrap_or(0);

    for i in 0..current_len {
        if let Some(val) = object_get_key_value(object, i) {
            flatten_single_value(mc, &val.borrow(), result, depth)?;
        }
    }
    Ok(())
}

fn flatten_single_value<'gc>(
    mc: &MutationContext<'gc>,
    value: &Value<'gc>,
    result: &mut Vec<Value<'gc>>,
    depth: usize,
) -> Result<(), JSError> {
    if depth == 0 {
        result.push(value.clone());
        return Ok(());
    }

    match value {
        Value::Object(obj) => {
            // Check if it's an array-like object
            let is_arr = { is_array(mc, obj) };
            if is_arr {
                flatten_array(mc, obj, result, depth - 1)?;
            } else {
                result.push(Value::Object(*obj));
            }
        }
        _ => {
            result.push(value.clone());
        }
    }
    Ok(())
}

/// Check if an object is an Array
pub(crate) fn is_array<'gc>(_mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>) -> bool {
    if let Some(val) = slot_get(obj, &InternalSlot::IsArray)
        && let Value::Boolean(b) = *val.borrow()
    {
        return b;
    }
    false
}

pub(crate) fn get_array_length<'gc>(_mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>) -> Option<usize> {
    object_get_length(obj)
}

pub(crate) fn set_array_length<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>, new_length: usize) -> Result<(), JSError> {
    object_set_length(mc, obj, new_length)
}

pub(crate) fn create_array<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let arr = new_js_object_data(mc);
    set_array_length(mc, &arr, 0)?;

    slot_set(mc, &arr, InternalSlot::IsArray, &Value::Boolean(true));
    // Mark 'length' as non-enumerable on arrays per spec
    arr.borrow_mut(mc).set_non_enumerable("length");
    arr.borrow_mut(mc).set_non_configurable("length");

    if let Some(array_ctor_rc) = crate::core::env_get(env, "Array")
        && let Value::Object(array_ctor_obj) = &*array_ctor_rc.borrow()
        && let Some(array_proto_rc) = object_get_key_value(array_ctor_obj, "prototype")
    {
        let array_proto_candidate = match &*array_proto_rc.borrow() {
            Value::Object(p) => Some(*p),
            Value::Property { value: Some(v), .. } => match &*v.borrow() {
                Value::Object(p) => Some(*p),
                _ => None,
            },
            _ => None,
        };

        if let Some(array_proto) = array_proto_candidate {
            arr.borrow_mut(mc).prototype = Some(array_proto);
            slot_set(mc, &arr, InternalSlot::Proto, &Value::Object(array_proto));
            return Ok(arr);
        }
    }

    // Set prototype
    let mut root_env_opt = Some(*env);
    while let Some(r) = root_env_opt {
        if let Some(proto_rc) = r.borrow().prototype {
            root_env_opt = Some(proto_rc);
        } else {
            break;
        }
    }
    if let Some(root_env) = root_env_opt {
        // Try to set prototype to Array.prototype
        if crate::core::set_internal_prototype_from_constructor(mc, &arr, &root_env, "Array").is_err() {
            // Fallback to Object.prototype
            let _ = crate::core::set_internal_prototype_from_constructor(mc, &arr, &root_env, "Object");
        }
    }

    Ok(arr)
}

/// Create a new Array Iterator
pub(crate) fn create_array_iterator<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    object: JSObjectDataPtr<'gc>,
    kind: &str,
) -> Result<Value<'gc>, JSError> {
    let iterator = new_js_object_data(mc);

    // Set [[Prototype]] to %ArrayIteratorPrototype%
    if let Some(proto_val) = slot_get_chained(env, &InternalSlot::ArrayIteratorPrototype)
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        iterator.borrow_mut(mc).prototype = Some(*proto);
    }

    // Store array
    slot_set(mc, &iterator, InternalSlot::IteratorArray, &Value::Object(object));
    // Store index
    slot_set(mc, &iterator, InternalSlot::IteratorIndex, &Value::Number(0.0));
    // Store kind
    slot_set(mc, &iterator, InternalSlot::IteratorKind, &Value::String(utf8_to_utf16(kind)));

    Ok(Value::Object(iterator))
}

pub(crate) fn handle_array_iterator_next<'gc>(
    mc: &MutationContext<'gc>,
    iterator: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Check own internal slots — if missing, `this` is not a real Array Iterator
    let arr_val = slot_get(iterator, &InternalSlot::IteratorArray).ok_or_else(|| {
        EvalError::Js(raise_type_error!(
            "ArrayIterator.prototype.next requires that 'this' be an Array Iterator"
        ))
    })?;
    let arr_ptr = if let Value::Object(o) = &*arr_val.borrow() {
        *o
    } else if matches!(&*arr_val.borrow(), Value::Undefined) {
        let result_obj = new_js_object_data(mc);
        object_set_key_value(mc, &result_obj, "value", &Value::Undefined)?;
        object_set_key_value(mc, &result_obj, "done", &Value::Boolean(true))?;
        return Ok(Value::Object(result_obj));
    } else {
        return Err(raise_eval_error!("Iterator array is invalid").into());
    };

    // Get index
    let index_val = slot_get(iterator, &InternalSlot::IteratorIndex).ok_or_else(|| {
        EvalError::Js(raise_type_error!(
            "ArrayIterator.prototype.next requires that 'this' be an Array Iterator"
        ))
    })?;
    let mut index = if let Value::Number(n) = &*index_val.borrow() {
        *n as usize
    } else {
        return Err(raise_eval_error!("Iterator index is invalid").into());
    };

    // Get kind
    let kind_val = slot_get(iterator, &InternalSlot::IteratorKind).ok_or_else(|| {
        EvalError::Js(raise_type_error!(
            "ArrayIterator.prototype.next requires that 'this' be an Array Iterator"
        ))
    })?;
    let kind = if let Value::String(s) = &*kind_val.borrow() {
        crate::unicode::utf16_to_utf8(s)
    } else {
        return Err(raise_eval_error!("Iterator kind is invalid").into());
    };

    let length = if let Some(ta_cell) = slot_get_chained(&arr_ptr, &InternalSlot::TypedArray) {
        if let Value::TypedArray(ta) = &*ta_cell.borrow() {
            // Spec step 8: If a has a [[TypedArrayName]], check for detached buffer.
            if ta.buffer.borrow().detached {
                return Err(raise_type_error!("Cannot perform operation on a detached ArrayBuffer").into());
            }
            if ta.length_tracking {
                let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                if ta.byte_offset > 0 && buf_len < ta.byte_offset {
                    return Err(raise_type_error!("Cannot perform operation on an out-of-bounds TypedArray").into());
                }
                if buf_len <= ta.byte_offset {
                    0
                } else {
                    (buf_len - ta.byte_offset) / ta.element_size()
                }
            } else {
                let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                let needed = ta.byte_offset + ta.length * ta.element_size();
                if buf_len < needed {
                    return Err(raise_type_error!("Cannot perform operation on an out-of-bounds TypedArray").into());
                }
                ta.length
            }
        } else {
            let len_val = crate::core::get_property_with_accessors(mc, env, &arr_ptr, "length")?;
            let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
            let len_num = crate::core::to_number(&len_prim)?;
            if len_num.is_nan() || len_num <= 0.0 {
                0usize
            } else if !len_num.is_finite() {
                usize::MAX
            } else {
                len_num.floor() as usize
            }
        }
    } else {
        let len_val = crate::core::get_property_with_accessors(mc, env, &arr_ptr, "length")?;
        let len_prim = crate::core::to_primitive(mc, &len_val, "number", env)?;
        let len_num = crate::core::to_number(&len_prim)?;
        if len_num.is_nan() || len_num <= 0.0 {
            0usize
        } else if !len_num.is_finite() {
            usize::MAX
        } else {
            len_num.floor() as usize
        }
    };

    if index >= length {
        slot_set(mc, iterator, InternalSlot::IteratorArray, &Value::Undefined);
        let result_obj = new_js_object_data(mc);
        object_set_key_value(mc, &result_obj, "value", &Value::Undefined)?;
        object_set_key_value(mc, &result_obj, "done", &Value::Boolean(true))?;
        return Ok(Value::Object(result_obj));
    }

    let element_val = crate::core::get_property_with_accessors(mc, env, &arr_ptr, index)?;

    let result_value = match kind.as_str() {
        "keys" => Value::Number(index as f64),
        "values" => element_val,
        "entries" => {
            let entry = create_array(mc, env)?;
            object_set_key_value(mc, &entry, "0", &Value::Number(index as f64))?;
            object_set_key_value(mc, &entry, "1", &element_val)?;
            set_array_length(mc, &entry, 2)?;
            Value::Object(entry)
        }
        _ => return Err(raise_eval_error!("Unknown iterator kind").into()),
    };

    // Update index
    index += 1;
    slot_set(mc, iterator, InternalSlot::IteratorIndex, &Value::Number(index as f64));

    let result_obj = new_js_object_data(mc);
    object_set_key_value(mc, &result_obj, "value", &result_value)?;
    object_set_key_value(mc, &result_obj, "done", &Value::Boolean(false))?;
    Ok(Value::Object(result_obj))
}

/// Serialize an array as "[a,b]" using the same element formatting used by Array.prototype.toString.
pub fn serialize_array_for_eval<'gc>(mc: &MutationContext<'gc>, object: &JSObjectDataPtr<'gc>) -> Result<String, JSError> {
    let current_len = get_array_length(mc, object).unwrap_or(0);
    let mut parts = Vec::new();
    for i in 0..current_len {
        if let Some(val_rc) = object_get_key_value(object, i) {
            match &*val_rc.borrow() {
                Value::Undefined | Value::Null => parts.push(String::new()),
                Value::String(s) => parts.push(format!("\"{}\"", utf16_to_utf8(s))),
                Value::Number(n) => parts.push(n.to_string()),
                Value::Boolean(b) => parts.push(b.to_string()),
                Value::BigInt(b) => parts.push(b.to_string()),
                Value::Object(o) => {
                    if is_array(mc, o) {
                        parts.push(serialize_array_for_eval(mc, o)?);
                    } else {
                        // Serialize nested object properties similarly to top-level object serialization
                        let mut seen_keys = std::collections::HashSet::new();
                        let mut props: Vec<(String, String)> = Vec::new();
                        let mut cur_obj_opt: Option<crate::core::JSObjectDataPtr<'_>> = Some(*o);
                        while let Some(cur_obj) = cur_obj_opt {
                            for key in cur_obj.borrow().properties.keys() {
                                // Skip non-enumerable and internal properties
                                if !cur_obj.borrow().is_enumerable(key)
                                    || matches!(key, crate::core::PropertyKey::String(s) if s == "__proto__")
                                {
                                    continue;
                                }
                                if seen_keys.contains(key) {
                                    continue;
                                }
                                seen_keys.insert(key.clone());
                                if let Some(val_rc) = object_get_key_value(&cur_obj, key) {
                                    let val = val_rc.borrow().clone();
                                    let val_str = match val {
                                        Value::String(s) => format!("\"{}\"", crate::unicode::utf16_to_utf8(&s)),
                                        Value::Number(n) => n.to_string(),
                                        Value::Boolean(b) => b.to_string(),
                                        Value::BigInt(b) => b.to_string(),
                                        Value::Undefined => "undefined".to_string(),
                                        Value::Null => "null".to_string(),
                                        Value::Object(o2) => {
                                            if is_array(mc, &o2) {
                                                serialize_array_for_eval(mc, &o2)?
                                            } else {
                                                "[object Object]".to_string()
                                            }
                                        }
                                        _ => "[object Object]".to_string(),
                                    };
                                    props.push((key.to_string(), val_str));
                                }
                            }
                            cur_obj_opt = cur_obj.borrow().prototype;
                        }
                        if props.is_empty() {
                            parts.push("{}".to_string());
                        } else {
                            let mut pairs: Vec<String> = Vec::new();
                            for (k, v) in props.iter() {
                                pairs.push(format!("\"{}\":{}", k, v));
                            }
                            parts.push(format!("{{{}}}", pairs.join(",")));
                        }
                    }
                }
                _ => parts.push("[object Object]".to_string()),
            }
        } else {
            parts.push(String::new());
        }
    }
    Ok(format!("[{}]", parts.join(",")))
}
