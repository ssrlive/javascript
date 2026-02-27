use crate::core::{
    ClosureData, EvalError, InternalSlot, JSObjectDataPtr, PropertyDescriptor, PropertyKey, Value, evaluate_call_dispatch,
    get_own_property, get_property_with_accessors, new_js_object_data, object_get_key_value, object_set_key_value,
    prepare_function_call_env, slot_get, slot_get_chained, slot_set,
};
use crate::core::{Gc, GcCell, GcPtr, MutationContext, new_gc_cell_ptr};
use crate::error::JSError;
use crate::js_array::{get_array_length, is_array, set_array_length};
use crate::js_date::is_date_object;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};

pub fn initialize_object_module<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    // 1. Create Object constructor
    let object_ctor = new_js_object_data(mc);
    slot_set(mc, &object_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &object_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("Object")));
    // Stamp OriginGlobal so get_function_realm can find the constructor's realm
    // (needed for cross-realm Object() calls to use the correct intrinsic prototypes).
    slot_set(mc, &object_ctor, InternalSlot::OriginGlobal, &Value::Object(*env));
    object_set_key_value(mc, &object_ctor, "length", &Value::Number(1.0))?;
    object_ctor.borrow_mut(mc).set_non_enumerable("length");
    object_ctor.borrow_mut(mc).set_non_writable("length");
    object_set_key_value(mc, &object_ctor, "name", &Value::String(utf8_to_utf16("Object")))?;
    object_ctor.borrow_mut(mc).set_non_enumerable("name");
    object_ctor.borrow_mut(mc).set_non_writable("name");

    // Register Object in the environment
    crate::core::env_set(mc, env, "Object", &Value::Object(object_ctor))?;

    // 2. Create Object.prototype
    let object_proto = new_js_object_data(mc);
    // Link prototype and constructor
    object_set_key_value(mc, &object_ctor, "prototype", &Value::Object(object_proto))?;
    object_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    object_ctor.borrow_mut(mc).set_non_writable("prototype");
    object_ctor.borrow_mut(mc).set_non_configurable("prototype");
    object_set_key_value(mc, &object_proto, "constructor", &Value::Object(object_ctor))?;
    // Make constructor non-enumerable
    object_proto.borrow_mut(mc).set_non_enumerable("constructor");

    // 3. Register static methods
    let static_methods = vec![
        "assign",
        "create",
        "defineProperties",
        "defineProperty",
        "entries",
        "freeze",
        "fromEntries",
        "getOwnPropertyDescriptor",
        "getOwnPropertyDescriptors",
        "getOwnPropertyNames",
        "getOwnPropertySymbols",
        "getPrototypeOf",
        "groupBy",
        "hasOwn",
        "is",
        "isExtensible",
        "isFrozen",
        "isSealed",
        "keys",
        "preventExtensions",
        "seal",
        "setPrototypeOf",
        "values",
    ];

    for method in static_methods {
        object_set_key_value(mc, &object_ctor, method, &Value::Function(format!("Object.{method}")))?;
        object_ctor.borrow_mut(mc).set_non_enumerable(method);
    }

    // 4. Register prototype methods
    let proto_methods = vec![
        "hasOwnProperty",
        "isPrototypeOf",
        "propertyIsEnumerable",
        "toLocaleString",
        "toString",
        "valueOf",
        "__defineGetter__",
        "__defineSetter__",
        "__lookupGetter__",
        "__lookupSetter__",
    ];

    for method in proto_methods {
        object_set_key_value(mc, &object_proto, method, &Value::Function(format!("Object.prototype.{method}")))?;
        // Methods on prototypes should be non-enumerable so for..in doesn't list them
        object_proto.borrow_mut(mc).set_non_enumerable(method);
    }

    object_set_key_value(
        mc,
        &object_proto,
        "__proto__",
        &Value::Property {
            value: None,
            getter: Some(Box::new(Value::Function("Object.prototype.get __proto__".to_string()))),
            setter: Some(Box::new(Value::Function("Object.prototype.set __proto__".to_string()))),
        },
    )?;
    // __proto__ accessor property must be non-enumerable
    object_proto.borrow_mut(mc).set_non_enumerable("__proto__");

    // Object.prototype is an immutable prototype exotic object (spec §19.1.3)
    slot_set(mc, &object_proto, InternalSlot::ImmutablePrototype, &Value::Boolean(true));

    Ok(())
}

fn to_object_for_object_static<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    value: &Value<'gc>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    match value {
        Value::Object(obj) => Ok(*obj),
        Value::Undefined | Value::Null => Err(raise_type_error!("Cannot convert undefined or null to object")),
        Value::String(s) => {
            let obj = new_js_object_data(mc);
            object_set_key_value(mc, &obj, "length", &Value::Number(s.len() as f64))?;
            slot_set(mc, &obj, InternalSlot::PrimitiveValue, &Value::String(s.clone()));
            obj.borrow_mut(mc).set_non_enumerable("length");
            obj.borrow_mut(mc).set_non_writable("length");
            obj.borrow_mut(mc).set_non_configurable("length");
            for (i, cu) in s.iter().enumerate() {
                object_set_key_value(mc, &obj, i, &Value::String(vec![*cu]))?;
                obj.borrow_mut(mc).set_non_writable(i);
                obj.borrow_mut(mc).set_non_configurable(i);
            }
            if let Err(e) = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "String") {
                log::warn!("Failed to set internal prototype for String object: {e:?}");
            }
            Ok(obj)
        }
        Value::Number(_n) => {
            let obj = new_js_object_data(mc);
            slot_set(mc, &obj, InternalSlot::PrimitiveValue, value);
            if let Err(e) = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Number") {
                log::warn!("Failed to set internal prototype for Number object: {e:?}");
            }
            Ok(obj)
        }
        Value::Boolean(_b) => {
            let obj = new_js_object_data(mc);
            slot_set(mc, &obj, InternalSlot::PrimitiveValue, value);
            if let Err(e) = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Boolean") {
                log::warn!("Failed to set internal prototype for Boolean object: {e:?}");
            }
            Ok(obj)
        }
        Value::BigInt(_h) => {
            let obj = new_js_object_data(mc);
            slot_set(mc, &obj, InternalSlot::PrimitiveValue, value);
            if let Err(e) = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "BigInt") {
                log::warn!("Failed to set internal prototype for BigInt object: {e:?}");
            }
            Ok(obj)
        }
        Value::Symbol(_sd) => {
            let obj = new_js_object_data(mc);
            slot_set(mc, &obj, InternalSlot::PrimitiveValue, value);
            if let Err(e) = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Symbol") {
                log::warn!("Failed to set internal prototype for Symbol object: {e:?}");
            }
            Ok(obj)
        }
        _other => Ok(new_js_object_data(mc)),
    }
}

#[allow(dead_code)]
pub fn define_properties<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>, props: &Value<'gc>) -> Result<(), EvalError<'gc>> {
    let props_obj = match props {
        Value::Object(o) => *o,
        _ => return Err(raise_type_error!("Property descriptors must be an object").into()),
    };

    let keys = crate::core::ordinary_own_property_keys_mc(mc, &props_obj)?;
    let mut descriptors = Vec::new();

    for key in keys {
        let is_enumerable = {
            let borrowed = props_obj.borrow();
            borrowed.properties.contains_key(&key) && borrowed.is_enumerable(&key)
        };

        if is_enumerable {
            let desc_val_rc = object_get_key_value(&props_obj, &key).unwrap();
            let desc_val = desc_val_rc.borrow().clone();
            if let Value::Object(desc_obj) = desc_val {
                descriptors.push((key, desc_obj));
            } else {
                return Err(raise_type_error!("Property description must be an object").into());
            }
        }
    }

    for (key, desc_obj) in descriptors {
        define_property_internal(mc, obj, key, &desc_obj)?;
    }

    Ok(())
}

pub(crate) fn define_property_internal<'gc>(
    mc: &MutationContext<'gc>,
    target_obj: &JSObjectDataPtr<'gc>,
    prop_key: impl Into<PropertyKey<'gc>>,
    desc_obj: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    let mut effective_prop_key = prop_key.into();
    let is_module_namespace = {
        let b = target_obj.borrow();
        b.deferred_module_path.is_some() || b.deferred_cache_env.is_some() || (b.prototype.is_none() && !b.is_extensible())
    };
    if object_get_key_value(target_obj, &effective_prop_key).is_none()
        && is_module_namespace
        && let PropertyKey::Symbol(sym_req) = &effective_prop_key
        && sym_req.description() == Some("Symbol.toStringTag")
    {
        let fallback_key = {
            let borrowed = target_obj.borrow();
            borrowed.properties.keys().find_map(|k| {
                if let PropertyKey::Symbol(sym_existing) = k
                    && sym_existing.description() == Some("Symbol.toStringTag")
                {
                    return Some(k.clone());
                }
                None
            })
        };
        if let Some(k) = fallback_key {
            effective_prop_key = k;
        }
    }
    let prop_key = &effective_prop_key;

    // Spec 10.1.6.3 ValidateAndApplyPropertyDescriptor step 2:
    // If current property is undefined and target is non-extensible, reject.
    if get_own_property(target_obj, prop_key).is_none() && !target_obj.borrow().is_extensible() {
        return Err(raise_type_error!("Cannot define property on a non-extensible object"));
    }

    // Parse descriptor into typed PropertyDescriptor
    let pd = PropertyDescriptor::from_object(desc_obj)?;
    // DEBUG: print parsed descriptor flags
    log::trace!(
        "define_property_internal: parsed descriptor writable={:?} enumerable={:?} configurable={:?} for key={:?} on obj_ptr={:p}",
        pd.writable,
        pd.enumerable,
        pd.configurable,
        prop_key,
        target_obj.as_ptr()
    );

    // If the property exists and is non-configurable on the target, apply ECMAScript-compatible checks
    if let Some(existing_rc) = get_own_property(target_obj, prop_key)
        && !target_obj.borrow().is_configurable(prop_key)
    {
        // If descriptor explicitly sets configurable true -> throw
        if pd.configurable == Some(true) {
            return Err(raise_type_error!("Cannot make non-configurable property configurable"));
        }

        // If descriptor explicitly sets enumerable and it's different -> throw
        if let Some(new_enum) = pd.enumerable {
            let existing_enum = target_obj.borrow().is_enumerable(prop_key);
            if new_enum != existing_enum {
                return Err(raise_type_error!("Cannot change enumerability of non-configurable property"));
            }
        }

        // Determine whether existing property is a data property or accessor
        let existing_is_accessor = match &*existing_rc.borrow() {
            Value::Property { value, getter, setter } => getter.is_some() || setter.is_some() || value.is_none(),
            Value::Getter(..) | Value::Setter(..) => true,
            _ => false,
        };

        // If existing is data property
        if !existing_is_accessor {
            // Disallow converting to accessor
            if pd.get.is_some() || pd.set.is_some() {
                return Err(raise_type_error!("Cannot convert non-configurable data property to an accessor"));
            }

            // If writable is being set from false -> true, disallow
            if pd.writable == Some(true) && !target_obj.borrow().is_writable(prop_key) {
                return Err(raise_type_error!("Cannot make non-writable property writable"));
            }

            // If attempting to change value while not writable and values differ -> throw
            if !target_obj.borrow().is_writable(prop_key)
                && let Some(pd_value) = &pd.value
            {
                // get existing value for comparison
                let existing_val = match &*existing_rc.borrow() {
                    Value::Property { value: Some(v), .. } => v.borrow().clone(),
                    Value::Property { value: None, .. } => Value::Undefined,
                    other => other.clone(),
                };
                let same_value = match (&existing_val, pd_value) {
                    (Value::Number(n1), Value::Number(n2)) => {
                        if n1.is_nan() && n2.is_nan() {
                            true
                        } else if *n1 == 0.0 && *n2 == 0.0 {
                            n1.to_bits() == n2.to_bits()
                        } else {
                            n1 == n2
                        }
                    }
                    _ => crate::core::values_equal(mc, &existing_val, pd_value),
                };
                if !same_value {
                    return Err(raise_type_error!("Cannot change value of non-writable, non-configurable property"));
                }
            }
        } else {
            // existing is accessor
            // Disallow converting to data property
            if pd.value.is_some() || pd.writable.is_some() {
                return Err(raise_type_error!("Cannot convert non-configurable accessor to a data property"));
            }

            let (current_get, current_set) = match &*existing_rc.borrow() {
                Value::Property { getter, setter, .. } => {
                    let g = getter.as_ref().map(|v| (**v).clone()).unwrap_or(Value::Undefined);
                    let s = setter.as_ref().map(|v| (**v).clone()).unwrap_or(Value::Undefined);
                    (g, s)
                }
                Value::Getter(..) => (existing_rc.borrow().clone(), Value::Undefined),
                Value::Setter(..) => (Value::Undefined, existing_rc.borrow().clone()),
                _ => (Value::Undefined, Value::Undefined),
            };

            if let Some(new_get) = &pd.get
                && !crate::core::values_equal(mc, new_get, &current_get)
            {
                return Err(raise_type_error!(
                    "Cannot change getter/setter of non-configurable accessor property"
                ));
            }

            if let Some(new_set) = &pd.set
                && !crate::core::values_equal(mc, new_set, &current_set)
            {
                return Err(raise_type_error!(
                    "Cannot change getter/setter of non-configurable accessor property"
                ));
            }
        }
    }

    let existing_own_value = get_own_property(target_obj, prop_key).map(|v| v.borrow().clone());
    let existing_was_accessor = existing_own_value.as_ref().is_some_and(|existing| match existing {
        Value::Property { getter, setter, value } => getter.is_some() || setter.is_some() || value.is_none(),
        Value::Getter(..) | Value::Setter(..) => true,
        _ => false,
    });

    let is_data_descriptor = pd.value.is_some() || pd.writable.is_some();
    let is_accessor_descriptor = pd.get.is_some() || pd.set.is_some();

    let mut getter_opt: Option<Box<Value>> = None;
    if !is_data_descriptor {
        if pd.get.is_some() {
            if let Some(get_val) = pd.get.clone()
                && !matches!(get_val, Value::Undefined)
            {
                getter_opt = Some(Box::new(get_val));
            }
        } else if let Some(existing) = &existing_own_value {
            match existing {
                Value::Property { getter: Some(g), .. } => getter_opt = Some(g.clone()),
                Value::Getter(..) => {}
                _ => {}
            }
        }
    }

    let mut setter_opt: Option<Box<Value>> = None;
    if !is_data_descriptor {
        if pd.set.is_some() {
            if let Some(set_val) = pd.set.clone()
                && !matches!(set_val, Value::Undefined)
            {
                setter_opt = Some(Box::new(set_val));
            }
        } else if let Some(existing) = &existing_own_value {
            match existing {
                Value::Property { setter: Some(s), .. } => setter_opt = Some(s.clone()),
                Value::Setter(..) => {}
                _ => {}
            }
        }
    }

    let value_cell = if is_accessor_descriptor {
        None
    } else if let Some(v) = pd.value.clone() {
        Some(new_gc_cell_ptr(mc, v))
    } else if let Some(existing) = &existing_own_value {
        match existing {
            Value::Property { value, .. } => *value,
            Value::Getter(..) | Value::Setter(..) => None,
            other => Some(new_gc_cell_ptr(mc, other.clone())),
        }
    } else {
        Some(new_gc_cell_ptr(mc, Value::Undefined))
    };

    // Create property descriptor value
    let prop_descriptor = Value::Property {
        value: value_cell,
        getter: getter_opt,
        setter: setter_opt,
    };

    // DEBUG: Log raw descriptor fields for troubleshooting
    log::debug!("define_property_internal: descriptor writable raw = {:?}", pd.writable);
    log::debug!("define_property_internal: descriptor enumerable raw = {:?}", pd.enumerable);

    // Compute existence and configurability BEFORE applying the new configurable flag.
    // This ensures that when a configurable property is being redefined as non-configurable,
    // we still allow writable/enumerable attributes to be updated in the same operation.
    let existed = get_own_property(target_obj, prop_key).is_some();
    let existing_is_configurable = !existed || target_obj.borrow().is_configurable(prop_key);

    if let Some(is_cfg) = pd.configurable {
        if is_cfg {
            log::trace!("define_property_internal: setting configurable=true for {:?}", prop_key);
            target_obj.borrow_mut(mc).set_configurable(prop_key.clone());
        } else {
            log::trace!("define_property_internal: setting configurable=false for {:?}", prop_key);
            target_obj.borrow_mut(mc).set_non_configurable(prop_key.clone());
        }
    }

    // For a new property, omitted attributes default to false.
    if !existed {
        // Writable applies to data/generic descriptors only.
        if pd.get.is_none() && pd.set.is_none() {
            if pd.writable == Some(true) {
                target_obj.borrow_mut(mc).set_writable(prop_key.clone());
            } else {
                log::trace!(
                    "define_property_internal: writable absent/false -> default false; setting non-writable for {:?} on obj_ptr={:p}",
                    prop_key,
                    target_obj.as_ptr()
                );
                target_obj.borrow_mut(mc).set_non_writable(prop_key.clone());
            }
        }

        if pd.enumerable == Some(true) {
            target_obj.borrow_mut(mc).set_enumerable(prop_key.clone());
        } else {
            log::trace!(
                "define_property_internal: enumerable absent/false -> default false; setting non-enumerable for {:?} on obj_ptr={:p}",
                prop_key,
                target_obj.as_ptr()
            );
            target_obj.borrow_mut(mc).set_non_enumerable(prop_key.clone());
        }

        if pd.configurable == Some(true) {
            target_obj.borrow_mut(mc).set_configurable(prop_key.clone());
        } else {
            log::trace!(
                "define_property_internal: configurable absent/false -> default false; setting non-configurable for {:?} on obj_ptr={:p}",
                prop_key,
                target_obj.as_ptr()
            );
            target_obj.borrow_mut(mc).set_non_configurable(prop_key.clone());
        }
    }

    // Only update writable/enumerable if property is configurable or does not exist
    let is_configurable = existing_is_configurable;
    log::debug!(
        "define_property_internal: existed={} is_configurable={} for {:?} on obj_ptr={:p}",
        existed,
        is_configurable,
        prop_key,
        target_obj.as_ptr()
    );
    if is_configurable || !existed {
        if let PropertyKey::String(s) = prop_key
            && s == "length"
        {
            log::debug!(
                "define_property_internal: defining 'length' desc fields: writable_present={} configurable_present={} enumerable_present={}",
                pd.writable.is_some(),
                pd.configurable.is_some(),
                pd.enumerable.is_some()
            );
        }
        // If writable flag explicitly set to false, mark property as non-writable
        if pd.writable == Some(false) {
            log::trace!(
                "define_property_internal: setting non-writable for {:?} on obj_ptr={:p}",
                prop_key,
                target_obj.as_ptr()
            );
            log::debug!(
                "define_property_internal: before set_non_writable contains={} list={:?} for obj_ptr={:p} key={:?}",
                target_obj.borrow().non_writable.contains(prop_key),
                target_obj.borrow().non_writable.iter().collect::<Vec<_>>(),
                target_obj.as_ptr(),
                prop_key
            );
            target_obj.borrow_mut(mc).set_non_writable(prop_key.clone());
            log::debug!(
                "define_property_internal: after set_non_writable contains={} list={:?} for obj_ptr={:p} key={:?}",
                target_obj.borrow().non_writable.contains(prop_key),
                target_obj.borrow().non_writable.iter().collect::<Vec<_>>(),
                target_obj.as_ptr(),
                prop_key
            );
        }
        // If writable flag explicitly set to true, clear non-writable
        if pd.writable == Some(true) {
            log::trace!(
                "define_property_internal: setting writable=true for {:?} on obj_ptr={:p}",
                prop_key,
                target_obj.as_ptr()
            );
            target_obj.borrow_mut(mc).set_writable(prop_key.clone());
        }

        if existed && existing_was_accessor && is_data_descriptor && pd.writable.is_none() {
            target_obj.borrow_mut(mc).set_non_writable(prop_key.clone());
        }

        // If enumerable flag explicitly set to false, mark property as non-enumerable
        if pd.enumerable == Some(false) {
            log::trace!(
                "define_property_internal: setting non-enumerable for {:?} on obj_ptr={:p}",
                prop_key,
                target_obj.as_ptr()
            );
            target_obj.borrow_mut(mc).set_non_enumerable(prop_key.clone());
        }
        // If enumerable flag explicitly set to true, clear non-enumerable
        if pd.enumerable == Some(true) {
            target_obj.borrow_mut(mc).set_enumerable(prop_key.clone());
        }
    }

    // On existing non-configurable data properties, writable may transition true -> false.
    if existed && !is_configurable && pd.writable == Some(false) {
        target_obj.borrow_mut(mc).set_non_writable(prop_key.clone());
    }

    // DEBUG: print non_writable/non_enumerable state before final set
    log::debug!(
        "define_property_internal: post-flag state for obj_ptr={:p} key={:?} non_writable_contains={} non_writable_list={:?} non_enumerable_contains={} non_enumerable_list={:?}",
        target_obj.as_ptr(),
        prop_key,
        target_obj.borrow().non_writable.contains(prop_key),
        target_obj.borrow().non_writable.iter().collect::<Vec<_>>(),
        target_obj.borrow().non_enumerable.contains(prop_key),
        target_obj.borrow().non_enumerable.iter().collect::<Vec<_>>()
    );

    // Always update value
    object_set_key_value(mc, target_obj, prop_key, &prop_descriptor)?;

    // Array exotic [[DefineOwnProperty]] (spec 10.4.2.1): when defining "length"
    // on an array with a smaller numeric value, delete indexed properties >= newLen.
    // object_set_key_value skips this for Value::Property wrappers, so handle here.
    if crate::js_array::is_array(mc, target_obj)
        && matches!(prop_key, PropertyKey::String(s) if s == "length")
        && let Some(ref new_val) = pd.value
    {
        let new_len = match new_val {
            Value::Number(n) if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 => Some(*n as usize),
            _ => None,
        };
        if let Some(new_len) = new_len {
            let mut indices_to_delete: Vec<usize> = target_obj
                .borrow()
                .properties
                .keys()
                .filter_map(|k| match k {
                    PropertyKey::String(s) => {
                        if let Ok(idx) = s.parse::<usize>()
                            && idx >= new_len
                            && idx.to_string() == *s
                        {
                            Some(idx)
                        } else {
                            None
                        }
                    }
                    _ => None,
                })
                .collect();
            indices_to_delete.sort_unstable_by(|a, b| b.cmp(a));
            for idx in indices_to_delete {
                let key = PropertyKey::String(idx.to_string());
                if !target_obj.borrow().is_configurable(key.clone()) {
                    break;
                }
                let _ = target_obj.borrow_mut(mc).properties.shift_remove(&key);
            }
        }
    }

    // Ensure explicitly-requested non-enumerable flag is applied even after
    // the property value has been stored. This guards against cases where
    // insertion order or existing property state prevented the marker from
    // being set earlier in the function.
    if pd.enumerable == Some(false) {
        target_obj.borrow_mut(mc).set_non_enumerable(prop_key.clone());
    }
    Ok(())
}

fn materialize_property_descriptor_object<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    src: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let out = new_js_object_data(mc);
    for key in ["value", "writable", "get", "set", "enumerable", "configurable"] {
        if object_get_key_value(src, key).is_some() {
            let v = get_property_with_accessors(mc, env, src, key)?;
            object_set_key_value(mc, &out, key, &v)?;
        }
    }
    Ok(out)
}

/// IsCallable check that includes Object-wrapped closures and bound functions
pub fn is_callable_for_from_entries<'gc>(val: &Value<'gc>) -> bool {
    match val {
        Value::Function(_)
        | Value::Closure(_)
        | Value::AsyncClosure(_)
        | Value::GeneratorFunction(..)
        | Value::AsyncGeneratorFunction(..) => true,
        Value::Object(obj) => {
            obj.borrow().get_closure().is_some()
                || obj.borrow().class_def.is_some()
                || slot_get(obj, &InternalSlot::NativeCtor).is_some()
                || slot_get(obj, &InternalSlot::BoundTarget).is_some()
                || slot_get(obj, &InternalSlot::Callable).is_some()
        }
        _ => false,
    }
}

pub fn handle_object_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "keys" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.keys requires at least one argument").into());
            }
            if args.len() > 1 {
                return Err(raise_type_error!("Object.keys accepts only one argument").into());
            }
            let obj = to_object_for_object_static(mc, env, &args[0])?;
            let mut keys = Vec::new();
            // For proxy wrappers, call proxy_own_keys directly to preserve error types
            let ordered = if let Some(proxy_cell) = crate::core::slot_get(&obj, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                crate::js_proxy::proxy_own_keys(mc, proxy)?
            } else {
                crate::core::ordinary_own_property_keys_mc(mc, &obj).map_err(|e| -> EvalError<'gc> { e.into() })?
            };
            for key in ordered {
                // Per spec, Object.keys only processes string keys (not symbols)
                if let PropertyKey::String(s) = &key {
                    if s == "__proto__" {
                        continue;
                    }
                    let is_enumerable = if let Some(proxy_cell) = crate::core::slot_get(&obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        crate::js_proxy::proxy_get_own_property_is_enumerable(mc, proxy, &key)?.unwrap_or(false)
                    } else {
                        obj.borrow().is_enumerable(&key)
                    };
                    if !is_enumerable {
                        continue;
                    }
                    let is_module_namespace = {
                        let b = obj.borrow();
                        b.deferred_module_path.is_some() || (b.prototype.is_none() && !b.is_extensible())
                    };
                    if is_module_namespace {
                        let _ = crate::core::get_property_with_accessors(mc, env, &obj, s.as_str())?;
                    }
                    keys.push(Value::String(utf8_to_utf16(s)));
                }
            }
            let result_obj = crate::js_array::create_array(mc, env)?;
            let len = keys.len();
            for (i, key) in keys.into_iter().enumerate() {
                object_set_key_value(mc, &result_obj, i, &key)?;
            }
            set_array_length(mc, &result_obj, len)?;
            Ok(Value::Object(result_obj))
        }
        "values" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.values requires at least one argument").into());
            }
            if args.len() > 1 {
                return Err(raise_type_error!("Object.values accepts only one argument").into());
            }
            let obj = to_object_for_object_static(mc, env, &args[0])?;
            let mut values = Vec::new();
            let ordered = crate::core::ordinary_own_property_keys_mc(mc, &obj)?;
            for key in ordered {
                let is_enumerable = if let Some(proxy_cell) = crate::core::slot_get(&obj, &InternalSlot::Proxy)
                    && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                {
                    crate::js_proxy::proxy_get_own_property_is_enumerable(mc, proxy, &key)?.unwrap_or(false)
                } else {
                    // Spec: Let desc be ? O.[[GetOwnProperty]](key). If desc is undefined, skip.
                    // A previous getter invocation may have deleted this key.
                    if !obj.borrow().properties.contains_key(&key) {
                        false
                    } else {
                        obj.borrow().is_enumerable(&key)
                    }
                };
                if !is_enumerable {
                    continue;
                }
                if let PropertyKey::String(s) = &key {
                    if s == "__proto__" {
                        continue;
                    }
                    let is_module_namespace = {
                        let b = obj.borrow();
                        b.deferred_module_path.is_some() || (b.prototype.is_none() && !b.is_extensible())
                    };
                    if is_module_namespace {
                        let _ = crate::core::get_property_with_accessors(mc, env, &obj, s.as_str())?;
                    }
                    // Use proxy get trap when object is a proxy
                    let proxy_opt = crate::core::slot_get(&obj, &InternalSlot::Proxy).and_then(|pc| match &*pc.borrow() {
                        Value::Proxy(p) => Some(*p),
                        _ => None,
                    });
                    let value = if let Some(proxy) = proxy_opt {
                        crate::js_proxy::proxy_get_property_with_receiver(mc, &proxy, &key, Some(Value::Object(obj)), None)?
                            .unwrap_or(Value::Undefined)
                    } else {
                        crate::core::get_property_with_accessors(mc, env, &obj, s.as_str())?
                    };
                    values.push(value);
                }
            }
            let result_obj = crate::js_array::create_array(mc, env)?;
            let len = values.len();
            for (i, value) in values.into_iter().enumerate() {
                object_set_key_value(mc, &result_obj, i, &value)?;
            }
            set_array_length(mc, &result_obj, len)?;
            Ok(Value::Object(result_obj))
        }
        "entries" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.entries requires at least one argument").into());
            }
            if args.len() > 1 {
                return Err(raise_type_error!("Object.entries accepts only one argument").into());
            }

            let obj = to_object_for_object_static(mc, env, &args[0])?;
            let ordered = crate::core::ordinary_own_property_keys_mc(mc, &obj)?;
            let result_obj = crate::js_array::create_array(mc, env)?;
            let mut out_index = 0usize;

            for key in ordered {
                let is_enumerable = if let Some(proxy_cell) = crate::core::slot_get(&obj, &InternalSlot::Proxy)
                    && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                {
                    crate::js_proxy::proxy_get_own_property_is_enumerable(mc, proxy, &key)?.unwrap_or(false)
                } else {
                    // Spec: Let desc be ? O.[[GetOwnProperty]](key). If desc is undefined, skip.
                    // A previous getter invocation may have deleted this key.
                    if !obj.borrow().properties.contains_key(&key) {
                        false
                    } else {
                        obj.borrow().is_enumerable(&key)
                    }
                };
                if !is_enumerable {
                    continue;
                }

                if let PropertyKey::String(s) = &key {
                    if s == "__proto__" {
                        continue;
                    }
                    // Use proxy get trap when object is a proxy
                    let proxy_opt = crate::core::slot_get(&obj, &InternalSlot::Proxy).and_then(|pc| match &*pc.borrow() {
                        Value::Proxy(p) => Some(*p),
                        _ => None,
                    });
                    let value = if let Some(proxy) = proxy_opt {
                        crate::js_proxy::proxy_get_property_with_receiver(mc, &proxy, &key, Some(Value::Object(obj)), None)?
                            .unwrap_or(Value::Undefined)
                    } else {
                        crate::core::get_property_with_accessors(mc, env, &obj, s.as_str())?
                    };
                    let pair = crate::js_array::create_array(mc, env)?;
                    object_set_key_value(mc, &pair, 0, &Value::String(utf8_to_utf16(s)))?;
                    object_set_key_value(mc, &pair, 1, &value)?;
                    set_array_length(mc, &pair, 2)?;

                    object_set_key_value(mc, &result_obj, out_index, &Value::Object(pair))?;
                    out_index += 1;
                }
            }

            set_array_length(mc, &result_obj, out_index)?;
            Ok(Value::Object(result_obj))
        }
        "hasOwn" => {
            if args.len() != 2 {
                return Err(raise_type_error!("Object.hasOwn requires exactly two arguments").into());
            }
            let obj_val = args[0].clone();
            let prop_val = args[1].clone();

            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot convert undefined or null to object").into());
            }

            let key = prop_val.to_property_key(mc, env)?;

            let has_own = match obj_val {
                Value::Object(obj) => {
                    if let PropertyKey::String(s) = &key {
                        crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, Some(s.as_str()))?;
                    }
                    obj.borrow().properties.contains_key(&key)
                }
                Value::String(s) => {
                    if let PropertyKey::String(k) = key {
                        if k == "length" {
                            true
                        } else if let Ok(idx) = k.parse::<usize>() {
                            idx < s.len()
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                _ => false,
            };

            Ok(Value::Boolean(has_own))
        }
        "getPrototypeOf" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.getPrototypeOf requires exactly one argument").into());
            }
            let obj_val = args[0].clone();
            match obj_val {
                Value::Proxy(proxy) => crate::js_proxy::proxy_get_prototype_of(mc, &proxy),
                Value::Object(obj) => {
                    // Check if this is a proxy wrapper object
                    if let Some(proxy_cell) = crate::core::slot_get(&obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        return crate::js_proxy::proxy_get_prototype_of(mc, proxy);
                    }
                    if let Some(proto_rc) = obj.borrow().prototype {
                        log::debug!(
                            "DBG Object.getPrototypeOf: obj ptr={:p} -> proto ptr={:p}",
                            Gc::as_ptr(obj),
                            Gc::as_ptr(proto_rc)
                        );
                        // DIAG: print whether object has an own '__proto__' property and its value
                        if let Some(pv) = slot_get_chained(&obj, &InternalSlot::Proto) {
                            log::debug!(
                                "DBG Object.getPrototypeOf: obj ptr={:p} has own __proto__ prop = {:?}",
                                Gc::as_ptr(obj),
                                pv.borrow()
                            );
                        } else {
                            log::debug!("DBG Object.getPrototypeOf: obj ptr={:p} has no own __proto__ prop", Gc::as_ptr(obj));
                        }
                        Ok(Value::Object(proto_rc))
                    } else {
                        log::debug!("DBG Object.getPrototypeOf: obj ptr={:p} -> proto NULL", Gc::as_ptr(obj));
                        if let Some(pv) = slot_get_chained(&obj, &InternalSlot::Proto) {
                            log::debug!(
                                "DBG Object.getPrototypeOf: obj ptr={:p} has own __proto__ prop = {:?}",
                                Gc::as_ptr(obj),
                                pv.borrow()
                            );
                        } else {
                            log::debug!("DBG Object.getPrototypeOf: obj ptr={:p} has no own __proto__ prop", Gc::as_ptr(obj));
                        }
                        Ok(Value::Null)
                    }
                }
                Value::Function(_)
                | Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::GeneratorFunction(..)
                | Value::AsyncGeneratorFunction(..) => {
                    if let Some(func_ctor) = crate::core::env_get(env, "Function")
                        && let Value::Object(func_obj) = &*func_ctor.borrow()
                        && let Some(func_proto) = object_get_key_value(func_obj, "prototype")
                        && let Value::Object(proto_obj) = &*func_proto.borrow()
                    {
                        Ok(Value::Object(*proto_obj))
                    } else {
                        Ok(Value::Null)
                    }
                }
                Value::Undefined | Value::Null => Err(raise_type_error!("Cannot convert undefined or null to object").into()),
                other => {
                    // For primitives, return the prototype of the wrapper type per spec
                    let wrapper_name = match other {
                        Value::Number(_) => Some("Number"),
                        Value::String(_) => Some("String"),
                        Value::Boolean(_) => Some("Boolean"),
                        Value::BigInt(_) => Some("BigInt"),
                        Value::Symbol(_) => Some("Symbol"),
                        _ => None,
                    };
                    if let Some(name) = wrapper_name
                        && let Some(ctor_val) = crate::core::env_get(env, name)
                        && let Value::Object(ctor_obj) = &*ctor_val.borrow()
                        && let Some(proto_val) = crate::core::object_get_key_value(ctor_obj, "prototype")
                        && let Value::Object(proto_obj) = &*proto_val.borrow()
                    {
                        Ok(Value::Object(*proto_obj))
                    } else {
                        Ok(Value::Null)
                    }
                }
            }
        }
        "isExtensible" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.isExtensible requires exactly one argument").into());
            }
            let obj_val = args[0].clone();
            match obj_val {
                Value::Object(obj) => {
                    // Check for proxy wrapper
                    if let Some(proxy_cell) = crate::core::slot_get(&obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        return Ok(Value::Boolean(crate::js_proxy::proxy_is_extensible(mc, proxy)?));
                    }
                    Ok(Value::Boolean(obj.borrow().is_extensible()))
                }
                // Built-in functions are extensible per spec
                Value::Function(_)
                | Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::GeneratorFunction(..)
                | Value::AsyncGeneratorFunction(..) => Ok(Value::Boolean(true)),
                _ => {
                    // ES6+: If Type(O) is not Object, return false
                    Ok(Value::Boolean(false))
                }
            }
        }
        "preventExtensions" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.preventExtensions requires exactly one argument").into());
            }
            match &args[0] {
                Value::Object(obj) => {
                    // Check if object is a proxy wrapper
                    if let Some(proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        let success = crate::js_proxy::proxy_prevent_extensions(mc, proxy)?;
                        if !success {
                            return Err(raise_type_error!("'preventExtensions' on proxy: trap returned falsish").into());
                        }
                        return Ok(args[0].clone());
                    }
                    obj.borrow_mut(mc).prevent_extensions();
                    Ok(Value::Object(*obj))
                }
                // ES6+: If Type(O) is not Object, return O
                other => Ok(other.clone()),
            }
        }
        "seal" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.seal requires exactly one argument").into());
            }
            match &args[0] {
                Value::Object(obj) => {
                    // Check if object is a proxy wrapper
                    if let Some(proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        // SetIntegrityLevel step 3: O.[[PreventExtensions]]()
                        let pe_result =
                            crate::js_proxy::apply_proxy_trap(mc, proxy, "preventExtensions", vec![(*proxy.target).clone()], || {
                                if let Value::Object(target_obj) = &*proxy.target {
                                    target_obj.borrow_mut(mc).prevent_extensions();
                                }
                                Ok(Value::Boolean(true))
                            })?;
                        let pe_success = match &pe_result {
                            Value::Boolean(b) => *b,
                            _ => pe_result.to_truthy(),
                        };
                        if !pe_success {
                            return Err(raise_type_error!("'preventExtensions' on proxy: trap returned falsish").into());
                        }
                        // SetIntegrityLevel step 5: get ownKeys, then defineProperty each with configurable:false
                        let keys = crate::js_proxy::proxy_own_keys(mc, proxy)?;
                        for k in keys {
                            let desc_obj = crate::core::new_js_object_data(mc);
                            crate::core::object_set_key_value(mc, &desc_obj, "configurable", &Value::Boolean(false))?;
                            crate::js_proxy::apply_proxy_trap(
                                mc,
                                proxy,
                                "defineProperty",
                                vec![
                                    (*proxy.target).clone(),
                                    crate::js_proxy::property_key_to_value_pub(&k),
                                    Value::Object(desc_obj),
                                ],
                                || {
                                    // Default: make non-configurable on target
                                    if let Value::Object(target_obj) = &*proxy.target {
                                        target_obj.borrow_mut(mc).set_non_configurable(k.clone());
                                    }
                                    Ok(Value::Boolean(true))
                                },
                            )?;
                        }
                        return Ok(args[0].clone());
                    }
                    // Make all own properties non-configurable
                    let ordered = crate::core::ordinary_own_property_keys_mc(mc, obj)?;
                    for k in ordered {
                        obj.borrow_mut(mc).set_non_configurable(k.clone());
                    }
                    // Make non-extensible
                    obj.borrow_mut(mc).prevent_extensions();
                    Ok(Value::Object(*obj))
                }
                // ES6+: If Type(O) is not Object, return O
                other => Ok(other.clone()),
            }
        }
        "isSealed" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.isSealed requires exactly one argument").into());
            }
            let arg = args[0].clone();
            match arg {
                Value::Object(obj) => {
                    let mut is_module_namespace = {
                        let b = obj.borrow();
                        b.deferred_module_path.is_some() || b.deferred_cache_env.is_some() || (b.prototype.is_none() && !b.is_extensible())
                    };
                    if !is_module_namespace
                        && let Some(sym_ctor_val) = crate::core::env_get(env, "Symbol")
                        && let Value::Object(sym_ctor) = sym_ctor_val.borrow().clone()
                        && let Some(tag_sym_val) = object_get_key_value(&sym_ctor, "toStringTag")
                        && let Value::Symbol(tag_sym) = tag_sym_val.borrow().clone()
                    {
                        let tag_key = PropertyKey::Symbol(tag_sym);
                        if let Some(tag_pd) = crate::core::build_property_descriptor(mc, &obj, &tag_key)
                            && tag_pd.configurable == Some(false)
                            && matches!(tag_pd.value, Some(Value::String(ref s)) if utf16_to_utf8(s) == "Module")
                        {
                            is_module_namespace = true;
                        }
                    }
                    if is_module_namespace {
                        return Ok(Value::Boolean(false));
                    }

                    // Check if this is a proxy
                    let proxy_opt = crate::core::slot_get(&obj, &InternalSlot::Proxy).and_then(|pc| match &*pc.borrow() {
                        Value::Proxy(p) => Some(*p),
                        _ => None,
                    });

                    if let Some(ref proxy) = proxy_opt {
                        // For proxy: check isExtensible trap
                        let ext_result =
                            crate::js_proxy::apply_proxy_trap(mc, proxy, "isExtensible", vec![(*proxy.target).clone()], || {
                                if let Value::Object(target_obj) = &*proxy.target {
                                    Ok(Value::Boolean(target_obj.borrow().is_extensible()))
                                } else {
                                    Ok(Value::Boolean(false))
                                }
                            })?;
                        if ext_result.to_truthy() {
                            return Ok(Value::Boolean(false));
                        }
                    } else if obj.borrow().is_extensible() {
                        return Ok(Value::Boolean(false));
                    }

                    let ordered = crate::core::ordinary_own_property_keys_mc(mc, &obj)?;
                    for k in ordered {
                        // Skip engine-internal __ properties
                        if let PropertyKey::String(ref s) = k
                            && s.len() > 2
                            && s.starts_with("__")
                        {
                            continue;
                        }
                        if let Some(ref proxy) = proxy_opt {
                            // Use getOwnPropertyDescriptor trap
                            let desc_result = crate::js_proxy::apply_proxy_trap(
                                mc,
                                proxy,
                                "getOwnPropertyDescriptor",
                                vec![(*proxy.target).clone(), crate::js_proxy::property_key_to_value_pub(&k)],
                                || {
                                    if let Value::Object(target_obj) = &*proxy.target {
                                        if let Some(pd) = crate::core::build_property_descriptor(mc, target_obj, &k) {
                                            Ok(Value::Object(pd.to_object(mc)?))
                                        } else {
                                            Ok(Value::Undefined)
                                        }
                                    } else {
                                        Ok(Value::Undefined)
                                    }
                                },
                            )?;
                            if let Value::Object(desc_obj) = &desc_result {
                                let configurable = crate::core::object_get_key_value(desc_obj, "configurable")
                                    .map(|v| v.borrow().to_truthy())
                                    .unwrap_or(false);
                                if configurable {
                                    return Ok(Value::Boolean(false));
                                }
                            }
                        } else if obj.borrow().is_configurable(&k) {
                            return Ok(Value::Boolean(false));
                        }
                    }
                    Ok(Value::Boolean(true))
                }
                // ES6+: If Type(O) is not Object, return true
                _ => Ok(Value::Boolean(true)),
            }
        }
        "freeze" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.freeze requires exactly one argument").into());
            }
            match &args[0] {
                Value::Object(obj) => {
                    // Check if object is a proxy wrapper
                    if let Some(proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        // SetIntegrityLevel step 3: O.[[PreventExtensions]]()
                        let pe_result =
                            crate::js_proxy::apply_proxy_trap(mc, proxy, "preventExtensions", vec![(*proxy.target).clone()], || {
                                if let Value::Object(target_obj) = &*proxy.target {
                                    target_obj.borrow_mut(mc).prevent_extensions();
                                }
                                Ok(Value::Boolean(true))
                            })?;
                        let pe_success = match &pe_result {
                            Value::Boolean(b) => *b,
                            _ => pe_result.to_truthy(),
                        };
                        if !pe_success {
                            return Err(raise_type_error!("'preventExtensions' on proxy: trap returned falsish").into());
                        }
                        // SetIntegrityLevel step 6 (frozen): get ownKeys, getOwnPropertyDescriptor, then defineProperty
                        let keys = crate::js_proxy::proxy_own_keys(mc, proxy)?;
                        for k in keys {
                            // Get current descriptor via proxy trap
                            let gopd_result = crate::js_proxy::apply_proxy_trap(
                                mc,
                                proxy,
                                "getOwnPropertyDescriptor",
                                vec![(*proxy.target).clone(), crate::js_proxy::property_key_to_value_pub(&k)],
                                || {
                                    // Default: build descriptor from target
                                    if let Value::Object(target_obj) = &*proxy.target
                                        && let Some(val_rc) = crate::core::object_get_key_value(target_obj, &k)
                                    {
                                        let desc_obj = crate::core::new_js_object_data(mc);
                                        crate::core::object_set_key_value(mc, &desc_obj, "value", &val_rc.borrow().clone())?;
                                        crate::core::object_set_key_value(mc, &desc_obj, "writable", &Value::Boolean(true))?;
                                        crate::core::object_set_key_value(mc, &desc_obj, "enumerable", &Value::Boolean(true))?;
                                        crate::core::object_set_key_value(mc, &desc_obj, "configurable", &Value::Boolean(true))?;
                                        return Ok(Value::Object(desc_obj));
                                    }
                                    Ok(Value::Undefined)
                                },
                            )?;

                            if let Value::Object(desc_obj) = &gopd_result {
                                let is_accessor = crate::core::object_get_key_value(desc_obj, "get").is_some()
                                    || crate::core::object_get_key_value(desc_obj, "set").is_some();
                                let new_desc = crate::core::new_js_object_data(mc);
                                crate::core::object_set_key_value(mc, &new_desc, "configurable", &Value::Boolean(false))?;
                                if !is_accessor {
                                    crate::core::object_set_key_value(mc, &new_desc, "writable", &Value::Boolean(false))?;
                                }
                                crate::js_proxy::apply_proxy_trap(
                                    mc,
                                    proxy,
                                    "defineProperty",
                                    vec![
                                        (*proxy.target).clone(),
                                        crate::js_proxy::property_key_to_value_pub(&k),
                                        Value::Object(new_desc),
                                    ],
                                    || {
                                        if let Value::Object(target_obj) = &*proxy.target {
                                            if !is_accessor {
                                                target_obj.borrow_mut(mc).set_non_writable(k.clone());
                                            }
                                            target_obj.borrow_mut(mc).set_non_configurable(k.clone());
                                        }
                                        Ok(Value::Boolean(true))
                                    },
                                )?;
                            }
                        }
                        return Ok(args[0].clone());
                    }

                    let is_module_namespace = {
                        let b = obj.borrow();
                        b.deferred_module_path.is_some() || b.deferred_cache_env.is_some() || (b.prototype.is_none() && !b.is_extensible())
                    };
                    if is_module_namespace {
                        return Err(raise_type_error!("Cannot freeze module namespace object").into());
                    }

                    // Per spec (10.4.5.3), TypedArray integer-indexed properties
                    // have fixed attributes {writable:true, configurable:false, enumerable:true}.
                    // Attempting to set writable:false on them must return false, which
                    // DefinePropertyOrThrow translates to a TypeError.
                    // Additionally, per spec 10.4.5.4, [[PreventExtensions]] returns false
                    // for TypedArrays backed by resizable ArrayBuffers, so Object.freeze
                    // always throws for such arrays (regardless of length).
                    if let Some(ta_val) = crate::core::slot_get(obj, &InternalSlot::TypedArray)
                        && let Value::TypedArray(ta) = &*ta_val.borrow()
                    {
                        // Check if backed by a resizable ArrayBuffer
                        let is_resizable = ta.buffer.borrow().max_byte_length.is_some();
                        if is_resizable {
                            return Err(raise_type_error!("Cannot freeze array buffer views backed by resizable buffers").into());
                        }
                        // Check effective length (may be length-tracking)
                        let eff_len = if ta.length_tracking {
                            let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                            (buf_len.saturating_sub(ta.byte_offset)) / ta.element_size()
                        } else {
                            ta.length
                        };
                        if eff_len > 0 {
                            return Err(raise_type_error!("Cannot freeze array buffer views with elements").into());
                        }
                    }

                    // For every own property: if data property -> make non-writable; in any case make non-configurable
                    let ordered = crate::core::ordinary_own_property_keys_mc(mc, obj)?;
                    for k in ordered.clone() {
                        if let Some(desc_rc) = crate::core::object_get_key_value(obj, &k) {
                            match &*desc_rc.borrow() {
                                // Accessor descriptor: do not set writable
                                Value::Property { getter: Some(_), .. } | Value::Property { setter: Some(_), .. } => {}
                                // Data descriptor
                                Value::Property { value: Some(_), .. } => {
                                    obj.borrow_mut(mc).set_non_writable(k.clone());
                                }
                                // Getter/setter stored directly
                                Value::Getter(..) | Value::Setter(..) => {}
                                // Any other stored value is a data property
                                _ => {
                                    obj.borrow_mut(mc).set_non_writable(k.clone());
                                }
                            }
                            obj.borrow_mut(mc).set_non_configurable(k.clone());
                        }
                    }
                    obj.borrow_mut(mc).prevent_extensions();
                    Ok(Value::Object(*obj))
                }
                // ES6+: If Type(O) is not Object, return O
                other => Ok(other.clone()),
            }
        }
        "isFrozen" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.isFrozen requires exactly one argument").into());
            }
            let arg = args[0].clone();
            match arg {
                Value::Object(obj) => {
                    let mut is_module_namespace = {
                        let b = obj.borrow();
                        b.deferred_module_path.is_some() || b.deferred_cache_env.is_some() || (b.prototype.is_none() && !b.is_extensible())
                    };
                    if !is_module_namespace
                        && let Some(sym_ctor_val) = crate::core::env_get(env, "Symbol")
                        && let Value::Object(sym_ctor) = sym_ctor_val.borrow().clone()
                        && let Some(tag_sym_val) = object_get_key_value(&sym_ctor, "toStringTag")
                        && let Value::Symbol(tag_sym) = tag_sym_val.borrow().clone()
                    {
                        let tag_key = PropertyKey::Symbol(tag_sym);
                        if let Some(tag_pd) = crate::core::build_property_descriptor(mc, &obj, &tag_key)
                            && tag_pd.configurable == Some(false)
                            && matches!(tag_pd.value, Some(Value::String(ref s)) if utf16_to_utf8(s) == "Module")
                        {
                            is_module_namespace = true;
                        }
                    }
                    if is_module_namespace {
                        return Ok(Value::Boolean(false));
                    }

                    // Check if this is a proxy
                    let proxy_opt = crate::core::slot_get(&obj, &InternalSlot::Proxy).and_then(|pc| match &*pc.borrow() {
                        Value::Proxy(p) => Some(*p),
                        _ => None,
                    });

                    if let Some(ref proxy) = proxy_opt {
                        let ext_result =
                            crate::js_proxy::apply_proxy_trap(mc, proxy, "isExtensible", vec![(*proxy.target).clone()], || {
                                if let Value::Object(target_obj) = &*proxy.target {
                                    Ok(Value::Boolean(target_obj.borrow().is_extensible()))
                                } else {
                                    Ok(Value::Boolean(false))
                                }
                            })?;
                        if ext_result.to_truthy() {
                            return Ok(Value::Boolean(false));
                        }
                    } else if obj.borrow().is_extensible() {
                        return Ok(Value::Boolean(false));
                    }

                    let ordered = crate::core::ordinary_own_property_keys_mc(mc, &obj)?;
                    for k in ordered {
                        // Skip engine-internal __ properties
                        if let PropertyKey::String(ref s) = k
                            && s.len() > 2
                            && s.starts_with("__")
                        {
                            continue;
                        }
                        if let Some(ref proxy) = proxy_opt {
                            // Use getOwnPropertyDescriptor trap
                            let desc_result = crate::js_proxy::apply_proxy_trap(
                                mc,
                                proxy,
                                "getOwnPropertyDescriptor",
                                vec![(*proxy.target).clone(), crate::js_proxy::property_key_to_value_pub(&k)],
                                || {
                                    if let Value::Object(target_obj) = &*proxy.target {
                                        if let Some(pd) = crate::core::build_property_descriptor(mc, target_obj, &k) {
                                            Ok(Value::Object(pd.to_object(mc)?))
                                        } else {
                                            Ok(Value::Undefined)
                                        }
                                    } else {
                                        Ok(Value::Undefined)
                                    }
                                },
                            )?;
                            if let Value::Object(desc_obj) = &desc_result {
                                let configurable = crate::core::object_get_key_value(desc_obj, "configurable")
                                    .map(|v| v.borrow().to_truthy())
                                    .unwrap_or(false);
                                if configurable {
                                    return Ok(Value::Boolean(false));
                                }
                                // isFrozen also needs non-writable for data properties
                                let is_accessor = crate::core::object_get_key_value(desc_obj, "get").is_some()
                                    || crate::core::object_get_key_value(desc_obj, "set").is_some();
                                if !is_accessor {
                                    let writable = crate::core::object_get_key_value(desc_obj, "writable")
                                        .map(|v| v.borrow().to_truthy())
                                        .unwrap_or(false);
                                    if writable {
                                        return Ok(Value::Boolean(false));
                                    }
                                }
                            }
                        } else {
                            if obj.borrow().is_configurable(&k) {
                                return Ok(Value::Boolean(false));
                            }
                            // If data property, it must be non-writable
                            if let Some(desc_rc) = crate::core::object_get_key_value(&obj, &k) {
                                match &*desc_rc.borrow() {
                                    // Accessor properties have no writable attribute
                                    Value::Property { getter: Some(_), .. } | Value::Property { setter: Some(_), .. } => {}
                                    Value::Getter(..) | Value::Setter(..) => {}
                                    // Data descriptor or direct value
                                    _ => {
                                        if obj.borrow().is_writable(&k) {
                                            return Ok(Value::Boolean(false));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Ok(Value::Boolean(true))
                }
                // ES6+: If Type(O) is not Object, return true
                _ => Ok(Value::Boolean(true)),
            }
        }
        "groupBy" => {
            // §22.1.2.5  Object.groupBy ( items, callbackfn )
            // GroupBy ( items, callbackfn, coercion )
            let items_val = args.first().cloned().unwrap_or(Value::Undefined);
            let callback_val = args.get(1).cloned().unwrap_or(Value::Undefined);

            // 2. If IsCallable(callbackfn) is false, throw a TypeError exception.
            if !is_callable_for_from_entries(&callback_val) {
                return Err(raise_type_error!("Object.groupBy: callbackfn is not a function").into());
            }

            // 3. Let groups be a new empty List.
            let result_obj = new_js_object_data(mc);
            result_obj.borrow_mut(mc).prototype = None;

            // 4. Let iteratorRecord be ? GetIterator(items, sync).
            // Support: Object (use @@iterator), String (iterate code points)
            let iter_sym = crate::js_iterator_helpers::get_well_known_symbol(env, "iterator")
                .ok_or_else(|| -> EvalError<'gc> { raise_type_error!("Symbol.iterator not found").into() })?;

            let iterable_obj = match &items_val {
                Value::Object(o) => *o,
                Value::String(_) => {
                    // Wrap string in a String object to access @@iterator
                    let str_obj = crate::js_class::handle_object_constructor(mc, std::slice::from_ref(&items_val), env)?;
                    match str_obj {
                        Value::Object(o) => o,
                        _ => return Err(raise_type_error!("Object.groupBy: cannot iterate over items").into()),
                    }
                }
                _ => return Err(raise_type_error!("Object.groupBy: items is not iterable").into()),
            };

            let method_val = get_property_with_accessors(mc, env, &iterable_obj, crate::core::PropertyKey::Symbol(iter_sym))?;
            if matches!(method_val, Value::Undefined | Value::Null) || !is_callable_for_from_entries(&method_val) {
                return Err(raise_type_error!("Object.groupBy: items is not iterable").into());
            }

            let iter_result = evaluate_call_dispatch(mc, env, &method_val, Some(&items_val), &[])?;
            let iter_obj = match &iter_result {
                Value::Object(o) => *o,
                _ => return Err(raise_type_error!("Iterator result is not an object").into()),
            };
            let next_method = get_property_with_accessors(mc, env, &iter_obj, "next")?;

            let mut k: usize = 0;
            loop {
                let step = evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                let step_obj = match &step {
                    Value::Object(o) => *o,
                    _ => return Err(raise_type_error!("Iterator result is not an object").into()),
                };
                let done_val = get_property_with_accessors(mc, env, &step_obj, "done")?;
                if done_val.to_truthy() {
                    break;
                }
                let val = get_property_with_accessors(mc, env, &step_obj, "value")?;

                // Call callback(value, k)
                let key_val = evaluate_call_dispatch(
                    mc,
                    env,
                    &callback_val,
                    Some(&Value::Undefined),
                    &[val.clone(), Value::Number(k as f64)],
                )?;

                // For Object.groupBy, coercion is "property" → ToPropertyKey
                let key = key_val.to_property_key(mc, env)?;

                let group_arr = if let Some(arr_rc) = object_get_key_value(&result_obj, &key) {
                    if let Value::Object(arr) = &*arr_rc.borrow() {
                        *arr
                    } else {
                        crate::js_array::create_array(mc, env)?
                    }
                } else {
                    let arr = crate::js_array::create_array(mc, env)?;
                    object_set_key_value(mc, &result_obj, &key, &Value::Object(arr))?;
                    arr
                };

                let current_len = get_array_length(mc, &group_arr).unwrap_or(0);
                object_set_key_value(mc, &group_arr, current_len, &val)?;
                crate::js_array::set_array_length(mc, &group_arr, current_len + 1)?;

                k += 1;
            }

            Ok(Value::Object(result_obj))
        }
        "create" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.create requires at least one argument").into());
            }
            let proto_val = args[0].clone();
            let proto_obj = match proto_val {
                Value::Object(obj) => Some(obj),
                Value::Null => None,
                _ => {
                    return Err(raise_type_error!("Object.create prototype must be an object or null").into());
                }
            };

            // Create new object
            let new_obj = new_js_object_data(mc);

            // Set prototype
            if let Some(proto) = proto_obj {
                new_obj.borrow_mut(mc).prototype = Some(proto);
            }

            // If properties descriptor is provided, apply ObjectDefineProperties semantics.
            if args.len() > 1 && !matches!(args[1], Value::Undefined) {
                let props_obj = to_object_for_object_static(mc, env, &args[1])?;

                let keys = crate::core::ordinary_own_property_keys_mc(mc, &props_obj)?;
                let mut descriptors: Vec<(PropertyKey<'gc>, JSObjectDataPtr<'gc>)> = Vec::new();

                for key in keys {
                    let is_enumerable = if let Some(proxy_cell) = crate::core::slot_get(&props_obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        match crate::js_proxy::proxy_get_own_property_is_enumerable(mc, proxy, &key)? {
                            Some(en) => en,
                            None => continue,
                        }
                    } else {
                        if crate::core::get_own_property(&props_obj, &key).is_none() {
                            continue;
                        }
                        props_obj.borrow().is_enumerable(&key)
                    };

                    if !is_enumerable {
                        continue;
                    }

                    let desc_val = get_property_with_accessors(mc, env, &props_obj, &key)?;
                    let desc_obj = match desc_val {
                        Value::Object(o) => o,
                        _ => return Err(raise_type_error!("Property description must be an object").into()),
                    };

                    let desc_obj_norm = materialize_property_descriptor_object(mc, env, &desc_obj)?;
                    let pd = PropertyDescriptor::from_object(&desc_obj_norm)?;
                    crate::core::validate_descriptor_for_define(mc, &pd)?;
                    descriptors.push((key, desc_obj_norm));
                }

                for (key, desc_obj) in descriptors {
                    define_property_internal(mc, &new_obj, &key, &desc_obj)?;
                }
            }

            Ok(Value::Object(new_obj))
        }
        "setPrototypeOf" => {
            if args.len() != 2 {
                return Err(raise_type_error!("Object.setPrototypeOf requires exactly two arguments").into());
            }
            let target = args[0].clone();
            let proto_obj = match &args[1] {
                Value::Object(o) => Some(*o),
                Value::Null => None,
                // Functions/closures are objects in JS – wrap them so we can store
                // as an internal prototype pointer while preserving the callable.
                Value::Function(_)
                | Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::GeneratorFunction(..)
                | Value::AsyncGeneratorFunction(..) => {
                    let wrapper = new_js_object_data(mc);
                    slot_set(mc, &wrapper, InternalSlot::Callable, &args[1]);
                    // Copy the function's own prototype chain so getPrototypeOf
                    // traversals keep working.
                    if let Some(fp) = crate::core::env_get(env, "Function")
                        && let Value::Object(func_ctor) = &*fp.borrow()
                        && let Some(fp_proto) = object_get_key_value(func_ctor, "prototype")
                        && let Value::Object(fp_obj) = &*fp_proto.borrow()
                    {
                        wrapper.borrow_mut(mc).prototype = Some(*fp_obj);
                    }
                    Some(wrapper)
                }
                _ => return Err(raise_type_error!("Object.setPrototypeOf prototype must be an object or null").into()),
            };

            match target {
                Value::Undefined | Value::Null => Err(raise_type_error!("Cannot convert undefined or null to object").into()),
                Value::Object(obj) => {
                    // Check if the object is a proxy – invoke setPrototypeOf trap
                    if let Some(proxy_cell) = crate::core::slot_get(&obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        let proto_val = match proto_obj {
                            Some(p) => Value::Object(p),
                            None => Value::Null,
                        };
                        let success = crate::js_proxy::proxy_set_prototype_of(mc, proxy, &proto_val)?;
                        if !success {
                            return Err(raise_type_error!("'setPrototypeOf' on proxy: trap returned falsish").into());
                        }
                        return Ok(Value::Object(obj));
                    }

                    // Immutable prototype exotic objects (e.g. Object.prototype):
                    // [[SetPrototypeOf]](V) returns true only if SameValue(V, current), else false → TypeError.
                    if crate::core::slot_has(&obj, &InternalSlot::ImmutablePrototype) {
                        let current_proto = obj.borrow().prototype;
                        let same = match (current_proto, proto_obj) {
                            (Some(cur), Some(next)) => crate::core::Gc::ptr_eq(cur, next),
                            (None, None) => true,
                            _ => false,
                        };
                        if !same {
                            return Err(raise_type_error!("Cannot set prototype of immutable prototype object").into());
                        }
                        return Ok(Value::Object(obj));
                    }

                    let current_proto = obj.borrow().prototype;
                    let is_extensible = obj.borrow().is_extensible();
                    let same_proto = match (current_proto, proto_obj) {
                        (Some(cur), Some(next)) => crate::core::Gc::ptr_eq(cur, next),
                        (None, None) => true,
                        _ => false,
                    };
                    if !same_proto && let Some(mut probe) = proto_obj {
                        loop {
                            if crate::core::Gc::ptr_eq(probe, obj) {
                                return Err(raise_type_error!("Cannot create prototype cycle").into());
                            }
                            if let Some(next) = probe.borrow().prototype {
                                probe = next;
                            } else {
                                break;
                            }
                        }
                    }
                    if !is_extensible && !same_proto {
                        return Err(raise_type_error!("Cannot set prototype of non-extensible object").into());
                    }
                    obj.borrow_mut(mc).prototype = proto_obj;
                    Ok(Value::Object(obj))
                }
                _ => Ok(target),
            }
        }
        "getOwnPropertySymbols" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.getOwnPropertySymbols requires exactly one argument").into());
            }
            let obj = to_object_for_object_static(mc, env, &args[0])?;
            crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, None)?;
            let result_obj = crate::js_array::create_array(mc, env)?;
            let mut idx = 0;
            let ordered = crate::core::ordinary_own_property_keys_mc(mc, &obj)?;
            for key in ordered {
                if let PropertyKey::Symbol(sym) = key {
                    object_set_key_value(mc, &result_obj, idx, &Value::Symbol(sym))?;
                    idx += 1;
                }
            }
            set_array_length(mc, &result_obj, idx)?;
            Ok(Value::Object(result_obj))
        }
        "getOwnPropertyNames" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.getOwnPropertyNames requires exactly one argument").into());
            }
            let obj = to_object_for_object_static(mc, env, &args[0])?;
            crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, None)?;
            let result_obj = crate::js_array::create_array(mc, env)?;
            let mut idx = 0;
            let ordered = crate::core::ordinary_own_property_keys_mc(mc, &obj)?;
            for key in ordered {
                if let PropertyKey::String(s) = key {
                    // Hide engine-internal properties (e.g. __proxy__, __is_array) but
                    // allow user-visible identifiers like "__" (exactly two underscores).
                    if s.len() > 2 && s.starts_with("__") {
                        continue;
                    }
                    object_set_key_value(mc, &result_obj, idx, &Value::String(utf8_to_utf16(&s)))?;
                    idx += 1;
                }
            }
            set_array_length(mc, &result_obj, idx)?;
            Ok(Value::Object(result_obj))
        }
        "getOwnPropertyDescriptor" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Object.getOwnPropertyDescriptor requires at least two arguments").into());
            }
            let obj_val = args[0].clone();
            let prop_val = args[1].clone();
            let key = prop_val.to_property_key(mc, env)?;

            let obj = match obj_val {
                Value::Object(o) => o,
                Value::Function(func_name) => {
                    let PropertyKey::String(prop_name) = key else {
                        return Ok(Value::Undefined);
                    };

                    let marker = format!("__fn_deleted::{}::{}", func_name, prop_name);
                    let deleted_in_env = crate::core::is_deleted_builtin_function_virtual_prop(func_name.as_str(), prop_name.as_str())
                        || crate::core::env_get(env, marker.as_str())
                            .map(|v| matches!(*v.borrow(), Value::Boolean(true)))
                            .unwrap_or(false);
                    let deleted_in_global = if let Some(global_this_rc) = crate::core::env_get(env, "globalThis")
                        && let Value::Object(global_obj) = &*global_this_rc.borrow()
                        && let Some(v) = slot_get(global_obj, &InternalSlot::FnDeleted(format!("{}::{}", func_name, prop_name)))
                    {
                        matches!(*v.borrow(), Value::Boolean(true))
                    } else {
                        false
                    };
                    if deleted_in_env || deleted_in_global {
                        return Ok(Value::Undefined);
                    }

                    let desc_obj = if prop_name == "length" {
                        let len = match func_name.as_str() {
                            "Array.prototype.push"
                            | "Array.prototype.at"
                            | "Array.prototype.flatMap"
                            | "Array.prototype.indexOf"
                            | "Array.prototype.lastIndexOf"
                            | "Array.prototype.concat"
                            | "Array.prototype.forEach"
                            | "Array.prototype.map"
                            | "Array.prototype.filter"
                            | "Array.prototype.some"
                            | "Array.prototype.every"
                            | "Array.prototype.find"
                            | "Array.prototype.findIndex"
                            | "Array.prototype.findLast"
                            | "Array.prototype.findLastIndex"
                            | "Array.prototype.reduce"
                            | "Array.prototype.reduceRight"
                            | "Array.prototype.fill"
                            | "Array.prototype.includes"
                            | "Array.prototype.join"
                            | "Array.prototype.unshift"
                            | "Object.entries"
                            | "Object.freeze"
                            | "Object.fromEntries"
                            | "Object.getOwnPropertyDescriptors"
                            | "Object.getOwnPropertyNames"
                            | "Object.getOwnPropertySymbols"
                            | "Object.getPrototypeOf"
                            | "Object.isExtensible"
                            | "Object.isFrozen"
                            | "Object.isSealed"
                            | "Object.keys"
                            | "Object.preventExtensions"
                            | "Object.seal"
                            | "Object.values"
                            | "Object.prototype.hasOwnProperty"
                            | "Object.prototype.isPrototypeOf"
                            | "Object.prototype.propertyIsEnumerable"
                            | "Object.prototype.set __proto__"
                            | "Object.prototype.__lookupGetter__"
                            | "Object.prototype.__lookupSetter__"
                            | "ArrayBuffer.isView"
                            | "ArrayBuffer.prototype.resize"
                            | "Array.prototype.sort"
                            | "Function.prototype.call"
                            | "Function.prototype.bind"
                            | "Function.prototype.[Symbol.hasInstance]"
                            | "Error.isError"
                            | "encodeURI"
                            | "encodeURIComponent"
                            | "decodeURI"
                            | "decodeURIComponent"
                            | "isNaN"
                            | "isFinite"
                            | "parseFloat"
                            | "Map.prototype.get"
                            | "Map.prototype.has"
                            | "Map.prototype.delete"
                            | "Map.prototype.forEach"
                            | "Set.prototype.add"
                            | "Set.prototype.has"
                            | "Set.prototype.delete"
                            | "Set.prototype.forEach"
                            | "WeakMap.prototype.get"
                            | "WeakMap.prototype.has"
                            | "WeakMap.prototype.delete"
                            | "WeakSet.prototype.add"
                            | "WeakSet.prototype.has"
                            | "WeakSet.prototype.delete"
                            | "Number.isNaN"
                            | "Number.isFinite"
                            | "Number.isInteger"
                            | "Number.isSafeInteger"
                            | "Number.prototype.toString"
                            | "Number.prototype.toFixed"
                            | "Number.prototype.toExponential"
                            | "Number.prototype.toPrecision"
                            | "Math.abs"
                            | "Math.acos"
                            | "Math.acosh"
                            | "Math.asin"
                            | "Math.asinh"
                            | "Math.atan"
                            | "Math.atanh"
                            | "Math.cbrt"
                            | "Math.ceil"
                            | "Math.clz32"
                            | "Math.cos"
                            | "Math.cosh"
                            | "Math.exp"
                            | "Math.expm1"
                            | "Math.floor"
                            | "Math.fround"
                            | "Math.log"
                            | "Math.log1p"
                            | "Math.log2"
                            | "Math.log10"
                            | "Math.round"
                            | "Math.sign"
                            | "Math.sin"
                            | "Math.sinh"
                            | "Math.sqrt"
                            | "Math.tan"
                            | "Math.tanh"
                            | "Math.trunc"
                            | "Reflect.getPrototypeOf"
                            | "Reflect.isExtensible"
                            | "Reflect.ownKeys"
                            | "Reflect.preventExtensions"
                            | "RegExp.prototype.exec"
                            | "RegExp.prototype.test"
                            | "RegExp.prototype.match"
                            | "RegExp.prototype.matchAll"
                            | "RegExp.prototype.search"
                            | "RegExp.escape"
                            | "String.prototype.charAt"
                            | "String.prototype.charCodeAt"
                            | "String.prototype.codePointAt"
                            | "String.prototype.concat"
                            | "String.prototype.endsWith"
                            | "String.prototype.indexOf"
                            | "String.prototype.lastIndexOf"
                            | "String.prototype.localeCompare"
                            | "String.prototype.match"
                            | "String.prototype.matchAll"
                            | "String.prototype.padEnd"
                            | "String.prototype.padStart"
                            | "String.prototype.repeat"
                            | "String.prototype.search"
                            | "String.prototype.startsWith"
                            | "String.prototype.at"
                            | "String.prototype.includes"
                            | "String.fromCharCode"
                            | "String.fromCodePoint"
                            | "String.raw" => 1.0,
                            "Array.prototype.slice"
                            | "Array.prototype.splice"
                            | "Array.prototype.copyWithin"
                            | "ArrayBuffer.prototype.slice"
                            | "Object.prototype.__defineGetter__"
                            | "Object.prototype.__defineSetter__"
                            | "Object.assign"
                            | "Object.create"
                            | "Object.defineProperties"
                            | "Object.getOwnPropertyDescriptor"
                            | "Map.groupBy"
                            | "Object.groupBy"
                            | "Object.hasOwn"
                            | "Object.is"
                            | "Object.setPrototypeOf"
                            | "Function.prototype.apply"
                            | "JSON.parse"
                            | "Map.prototype.set"
                            | "WeakMap.prototype.set"
                            | "Math.atan2"
                            | "Math.hypot"
                            | "Math.imul"
                            | "Math.max"
                            | "Math.min"
                            | "Math.pow"
                            | "parseInt"
                            | "Reflect.construct"
                            | "Reflect.deleteProperty"
                            | "Reflect.get"
                            | "Reflect.getOwnPropertyDescriptor"
                            | "Reflect.has"
                            | "Reflect.setPrototypeOf"
                            | "RegExp.prototype.replace"
                            | "RegExp.prototype.split"
                            | "String.prototype.replace"
                            | "String.prototype.replaceAll"
                            | "String.prototype.slice"
                            | "String.prototype.split"
                            | "String.prototype.substring"
                            | "String.prototype.substr" => 2.0,
                            "Object.defineProperty" | "JSON.stringify" | "Reflect.apply" | "Reflect.defineProperty" | "Reflect.set" => 3.0,
                            "Symbol.for" | "Symbol.keyFor" | "Symbol.prototype.[Symbol.toPrimitive]" => 1.0,
                            _ => 0.0,
                        };
                        crate::core::create_descriptor_object(mc, &Value::Number(len), false, false, true)?
                    } else if prop_name == "name" {
                        let short_name = if func_name.contains("[Symbol.hasInstance]") {
                            "[Symbol.hasInstance]"
                        } else if func_name == "Symbol.prototype.[Symbol.toPrimitive]" {
                            return {
                                let desc_obj = crate::core::create_descriptor_object(
                                    mc,
                                    &Value::String(utf8_to_utf16("[Symbol.toPrimitive]")),
                                    false,
                                    false,
                                    true,
                                )?;
                                crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                                Ok(Value::Object(desc_obj))
                            };
                        } else if func_name == "RegExp.prototype.match" {
                            return {
                                let desc_obj = crate::core::create_descriptor_object(
                                    mc,
                                    &Value::String(utf8_to_utf16("[Symbol.match]")),
                                    false,
                                    false,
                                    true,
                                )?;
                                crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                                Ok(Value::Object(desc_obj))
                            };
                        } else if func_name == "RegExp.prototype.replace" {
                            return {
                                let desc_obj = crate::core::create_descriptor_object(
                                    mc,
                                    &Value::String(utf8_to_utf16("[Symbol.replace]")),
                                    false,
                                    false,
                                    true,
                                )?;
                                crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                                Ok(Value::Object(desc_obj))
                            };
                        } else if func_name == "RegExp.prototype.search" {
                            return {
                                let desc_obj = crate::core::create_descriptor_object(
                                    mc,
                                    &Value::String(utf8_to_utf16("[Symbol.search]")),
                                    false,
                                    false,
                                    true,
                                )?;
                                crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                                Ok(Value::Object(desc_obj))
                            };
                        } else if func_name == "RegExp.prototype.split" {
                            return {
                                let desc_obj = crate::core::create_descriptor_object(
                                    mc,
                                    &Value::String(utf8_to_utf16("[Symbol.split]")),
                                    false,
                                    false,
                                    true,
                                )?;
                                crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                                Ok(Value::Object(desc_obj))
                            };
                        } else if func_name == "RegExp.prototype.matchAll" {
                            return {
                                let desc_obj = crate::core::create_descriptor_object(
                                    mc,
                                    &Value::String(utf8_to_utf16("[Symbol.matchAll]")),
                                    false,
                                    false,
                                    true,
                                )?;
                                crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                                Ok(Value::Object(desc_obj))
                            };
                        } else if func_name == "Map.prototype.size" || func_name == "Set.prototype.size" {
                            let base = func_name.rsplit('.').next().unwrap_or(func_name.as_str());
                            return {
                                let desc_obj = crate::core::create_descriptor_object(
                                    mc,
                                    &Value::String(utf8_to_utf16(&format!("get {}", base))),
                                    false,
                                    false,
                                    true,
                                )?;
                                crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                                Ok(Value::Object(desc_obj))
                            };
                        } else if func_name.ends_with("[Symbol.species]") {
                            return {
                                let desc_obj = crate::core::create_descriptor_object(
                                    mc,
                                    &Value::String(utf8_to_utf16("get [Symbol.species]")),
                                    false,
                                    false,
                                    true,
                                )?;
                                crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                                Ok(Value::Object(desc_obj))
                            };
                        } else if func_name.ends_with("[Symbol.iterator]") {
                            return {
                                let desc_obj = crate::core::create_descriptor_object(
                                    mc,
                                    &Value::String(utf8_to_utf16("[Symbol.iterator]")),
                                    false,
                                    false,
                                    true,
                                )?;
                                crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                                Ok(Value::Object(desc_obj))
                            };
                        } else {
                            func_name.rsplit('.').next().unwrap_or(func_name.as_str())
                        };
                        crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16(short_name)), false, false, true)?
                    } else {
                        return Ok(Value::Undefined);
                    };

                    crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                    return Ok(Value::Object(desc_obj));
                }
                _ => to_object_for_object_static(mc, env, &obj_val)?,
            };

            if let PropertyKey::String(s) = &key {
                crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, Some(s.as_str()))?;
            }

            // If the object is a proxy wrapper, delegate to proxy GOPD trap
            if let Some(proxy_cell) = crate::core::slot_get(&obj, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                match crate::js_proxy::proxy_get_own_property_descriptor(mc, proxy, &key)? {
                    Some(desc_obj) => {
                        crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                        return Ok(Value::Object(desc_obj));
                    }
                    None => return Ok(Value::Undefined),
                }
            }

            // TypedArray [[GetOwnProperty]]: synthesize descriptor for canonical numeric index strings
            if let PropertyKey::String(s) = &key
                && let Some(ta_cell) = slot_get(&obj, &InternalSlot::TypedArray)
                && let Value::TypedArray(ta) = &*ta_cell.borrow()
                && let Some(num_idx) = crate::js_typedarray::canonical_numeric_index_string(s)
            {
                if crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
                    let idx = num_idx as usize;
                    let value = match ta.kind {
                        crate::core::TypedArrayKind::BigInt64 | crate::core::TypedArrayKind::BigUint64 => {
                            let size = ta.element_size();
                            let byte_offset = ta.byte_offset + idx * size;
                            let buffer = ta.buffer.borrow();
                            let data = buffer.data.lock().unwrap();
                            if byte_offset + size <= data.len() {
                                let bytes = &data[byte_offset..byte_offset + size];
                                if matches!(ta.kind, crate::core::TypedArrayKind::BigInt64) {
                                    let mut b = [0u8; 8];
                                    b.copy_from_slice(bytes);
                                    Value::BigInt(Box::new(num_bigint::BigInt::from(i64::from_le_bytes(b))))
                                } else {
                                    let mut b = [0u8; 8];
                                    b.copy_from_slice(bytes);
                                    Value::BigInt(Box::new(num_bigint::BigInt::from(u64::from_le_bytes(b))))
                                }
                            } else {
                                Value::Undefined
                            }
                        }
                        _ => {
                            let n = ta.get(idx)?;
                            Value::Number(n)
                        }
                    };
                    // TypedArray indexed elements are always { writable: true, enumerable: true, configurable: true }
                    let pd = crate::core::PropertyDescriptor::new_data(&value, true, true, true);
                    let desc_obj = pd.to_object(mc)?;
                    crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                    return Ok(Value::Object(desc_obj));
                }
                // Not a valid integer index — property doesn't exist
                return Ok(Value::Undefined);

                // Not a canonical numeric index — fall through to ordinary GetOwnProperty
            }

            if let Some(_val_rc) = object_get_key_value(&obj, &key) {
                if let Some(mut pd) = crate::core::build_property_descriptor(mc, &obj, &key) {
                    let is_deferred_namespace = obj.borrow().deferred_module_path.is_some();
                    let is_module_namespace = {
                        let b = obj.borrow();
                        is_deferred_namespace || (b.prototype.is_none() && !b.is_extensible())
                    };
                    let is_accessor_descriptor = pd.get.is_some() || pd.set.is_some();
                    let needs_hydration = (is_module_namespace || !is_accessor_descriptor)
                        && (pd.value.is_none() || matches!(pd.value, Some(Value::Undefined)));
                    if needs_hydration && let PropertyKey::String(s) = &key {
                        let hydrated = crate::core::get_property_with_accessors(mc, env, &obj, s.as_str())?;
                        if is_module_namespace || !matches!(hydrated, Value::Undefined) {
                            pd.value = Some(hydrated);
                            pd.get = None;
                            pd.set = None;
                            if pd.writable.is_none() {
                                pd.writable = Some(true);
                            }
                        }
                    }
                    let desc_obj = pd.to_object(mc)?;
                    crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                    Ok(Value::Object(desc_obj))
                } else {
                    Ok(Value::Undefined)
                }
            } else {
                Ok(Value::Undefined)
            }
        }
        "getOwnPropertyDescriptors" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.getOwnPropertyDescriptors requires exactly one argument").into());
            }
            let obj = to_object_for_object_static(mc, env, &args[0])?;
            crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, None)?;
            let result_obj = new_js_object_data(mc);

            // Ensure result inherits from Object.prototype
            if let Some(obj_val) = crate::core::env_get(env, "Object")
                && let Value::Object(obj_ctor) = &*obj_val.borrow()
                && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
                && let Value::Object(proto) = &*proto_val.borrow()
            {
                result_obj.borrow_mut(mc).prototype = Some(*proto);
            }

            let ordered = crate::core::ordinary_own_property_keys_mc(mc, &obj)?;

            // Check if the object is a proxy
            let proxy_opt = crate::core::slot_get(&obj, &InternalSlot::Proxy).and_then(|pc| match &*pc.borrow() {
                Value::Proxy(p) => Some(*p),
                _ => None,
            });

            for key in &ordered {
                // Skip engine-internal __ properties
                if let PropertyKey::String(s) = key
                    && s.len() > 2
                    && s.starts_with("__")
                {
                    continue;
                }

                let desc_obj_opt = if let Some(ref proxy) = proxy_opt {
                    // For proxies, call the getOwnPropertyDescriptor trap with proper invariant checks
                    crate::js_proxy::proxy_get_own_property_descriptor(mc, proxy, key)?
                } else if let Some(pd) = crate::core::build_property_descriptor(mc, &obj, key) {
                    Some(pd.to_object(mc)?)
                } else {
                    None
                };

                if let Some(desc_obj) = desc_obj_opt {
                    match key {
                        PropertyKey::String(s) => {
                            object_set_key_value(mc, &result_obj, s, &Value::Object(desc_obj))?;
                        }
                        PropertyKey::Symbol(sym_rc) => {
                            let property_key = PropertyKey::Symbol(*sym_rc);
                            object_set_key_value(mc, &result_obj, &property_key, &Value::Object(desc_obj))?;
                        }
                        PropertyKey::Private(..) => {}
                        PropertyKey::Internal(..) => {}
                    }
                }
            }

            Ok(Value::Object(result_obj))
        }
        "assign" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.assign requires at least one argument").into());
            }
            let target_obj = to_object_for_object_static(mc, env, args.first().unwrap())?;

            // Iterate sources
            for src_expr in args.iter().skip(1) {
                let src_val = src_expr.clone();
                if matches!(src_val, Value::Undefined | Value::Null) {
                    continue;
                }

                let source_obj = to_object_for_object_static(mc, env, &src_val)?;
                let ordered = if let Some(proxy_cell) = crate::core::slot_get(&source_obj, &InternalSlot::Proxy)
                    && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                {
                    crate::js_proxy::proxy_own_keys(mc, proxy)?
                } else {
                    crate::core::ordinary_own_property_keys_mc(mc, &source_obj)?
                };

                for key in ordered {
                    if key == "__proto__".into() {
                        continue;
                    }

                    let is_enumerable = if let Some(proxy_cell) = crate::core::slot_get(&source_obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        match crate::js_proxy::proxy_get_own_property_is_enumerable(mc, proxy, &key)? {
                            Some(en) => en,
                            None => continue,
                        }
                    } else {
                        if crate::core::get_own_property(&source_obj, &key).is_none() {
                            continue;
                        }
                        source_obj.borrow().is_enumerable(&key)
                    };

                    if !is_enumerable {
                        continue;
                    }

                    let prop_value = crate::core::get_property_with_accessors(mc, env, &source_obj, &key)?;
                    crate::core::set_property_with_accessors(mc, env, &target_obj, key, &prop_value, Some(&Value::Object(target_obj)))?;
                }
            }

            Ok(Value::Object(target_obj))
        }
        "defineProperty" => {
            // Minimal implementation: Object.defineProperty(target, prop, descriptor)
            if args.len() < 3 {
                return Err(raise_type_error!("Object.defineProperty requires three arguments").into());
            }
            let target_val = args[0].clone();
            let target_obj = match target_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Object.defineProperty called on non-object").into()),
            };

            let prop_val = args[1].clone();
            let prop_key = prop_val.to_property_key(mc, env)?;

            if let PropertyKey::String(s) = &prop_key {
                crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &target_obj, Some(s.as_str()))?;
            }

            let desc_val = args[2].clone();
            let desc_obj = match desc_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Property descriptor must be an object").into()),
            };

            let desc_obj_norm = materialize_property_descriptor_object(mc, env, &desc_obj)?;
            let pd = PropertyDescriptor::from_object(&desc_obj_norm)?;
            crate::core::validate_descriptor_for_define(mc, &pd)?;

            if is_array(mc, &target_obj)
                && let PropertyKey::String(s) = &prop_key
                && s == "length"
            {
                if pd.get.is_some() || pd.set.is_some() {
                    return Err(raise_type_error!("Cannot redefine array length property as accessor").into());
                }

                let to_number_with_hint = |value: &Value<'gc>| -> Result<f64, JSError> {
                    let prim = crate::core::to_primitive(mc, value, "number", env)?;
                    crate::core::to_number(&prim).map_err(Into::into)
                };

                let old_len = get_array_length(mc, &target_obj).unwrap_or(0);

                let to_uint32 = |num: f64| -> u32 {
                    if !num.is_finite() || num == 0.0 || num.is_nan() {
                        return 0;
                    }
                    let int = num.signum() * num.abs().floor();
                    let two32 = 4294967296.0_f64;
                    let mut int_mod = int % two32;
                    if int_mod < 0.0 {
                        int_mod += two32;
                    }
                    int_mod as u32
                };

                let mut computed_new_len: Option<usize> = None;
                if let Some(v) = pd.value.clone() {
                    let uint32_len = to_uint32(to_number_with_hint(&v)?);
                    let number_len = to_number_with_hint(&v)?;

                    if (uint32_len as f64) != number_len {
                        return Err(raise_range_error!("Invalid array length").into());
                    }
                    computed_new_len = Some(uint32_len as usize);
                }

                // Overflow check must happen before descriptor validation side conditions.
                if let Some(new_len) = computed_new_len
                    && new_len > u32::MAX as usize
                {
                    return Err(raise_range_error!("Invalid array length").into());
                }

                if pd.configurable == Some(true) || pd.enumerable == Some(true) {
                    return Err(raise_type_error!("Cannot redefine array length property").into());
                }

                let length_writable = target_obj.borrow().is_writable("length");
                if pd.writable == Some(true) && !length_writable {
                    return Err(raise_type_error!("Cannot make non-writable property writable").into());
                }

                if let Some(new_len) = computed_new_len {
                    if !length_writable && new_len != old_len {
                        return Err(raise_type_error!("Cannot assign to read only property 'length'").into());
                    }
                    if let Err(e) = set_array_length(mc, &target_obj, new_len) {
                        if pd.writable == Some(false) {
                            target_obj.borrow_mut(mc).set_non_writable("length");
                        }
                        return Err(e.into());
                    }
                }

                if pd.writable == Some(false) {
                    target_obj.borrow_mut(mc).set_non_writable("length");
                } else if pd.writable == Some(true) {
                    target_obj.borrow_mut(mc).set_writable("length");
                }

                return Ok(Value::Object(target_obj));
            }

            if let Some(ta_cell) = slot_get(&target_obj, &InternalSlot::TypedArray)
                && let Value::TypedArray(ta) = &*ta_cell.borrow()
                && let PropertyKey::String(s) = &prop_key
                && let Some(num_idx) = crate::js_typedarray::canonical_numeric_index_string(s)
            {
                // TypedArray [[DefineOwnProperty]] for CanonicalNumericIndexString
                if !crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
                    return Err(raise_type_error!("Invalid typed array index").into());
                }
                if pd.get.is_some() || pd.set.is_some() {
                    return Err(raise_type_error!("Invalid typed array index descriptor").into());
                }
                if pd.configurable == Some(false) {
                    return Err(raise_type_error!("Invalid typed array index descriptor").into());
                }
                if pd.enumerable == Some(false) {
                    return Err(raise_type_error!("Invalid typed array index descriptor").into());
                }
                if pd.writable == Some(false) {
                    return Err(raise_type_error!("Invalid typed array index descriptor").into());
                }
                if let Some(val) = pd.value {
                    let idx = num_idx as usize;
                    if crate::js_typedarray::is_bigint_typed_array(&ta.kind) {
                        let n = crate::js_typedarray::to_bigint_i64(mc, env, &val)?;
                        ta.set_bigint(mc, idx, n)?;
                    } else {
                        let n = crate::core::to_number_with_env(mc, env, &val)?;
                        ta.set(mc, idx, n)?;
                    }
                }
                return Ok(Value::Object(target_obj));
            }

            let is_module_namespace = {
                let b = target_obj.borrow();
                b.deferred_module_path.is_some() || b.deferred_cache_env.is_some() || (b.prototype.is_none() && !b.is_extensible())
            };
            if is_module_namespace {
                if pd.get.is_some() || pd.set.is_some() {
                    return Err(raise_type_error!("Cannot redefine property on module namespace object").into());
                }

                match &prop_key {
                    PropertyKey::String(name) => {
                        if crate::core::build_property_descriptor(mc, &target_obj, &prop_key).is_none() {
                            return Err(raise_type_error!("Cannot redefine property on module namespace object").into());
                        }
                        if pd.configurable == Some(true) || pd.enumerable == Some(false) || pd.writable == Some(false) {
                            return Err(raise_type_error!("Cannot redefine property on module namespace object").into());
                        }
                        if let Some(v) = pd.value {
                            let cur = crate::core::get_property_with_accessors(mc, env, &target_obj, name.as_str())?;
                            if !crate::core::values_equal(mc, &cur, &v) {
                                return Err(raise_type_error!("Cannot redefine property on module namespace object").into());
                            }
                        }
                    }
                    PropertyKey::Symbol(sym) if sym.description() == Some("Symbol.toStringTag") => {
                        if pd.configurable == Some(true) || pd.enumerable == Some(true) || pd.writable == Some(true) {
                            return Err(raise_type_error!("Cannot redefine property on module namespace object").into());
                        }
                        if let Some(v) = pd.value
                            && !crate::core::values_equal(mc, &v, &Value::String(utf8_to_utf16("Module")))
                        {
                            return Err(raise_type_error!("Cannot redefine property on module namespace object").into());
                        }
                    }
                    _ => {
                        return Err(raise_type_error!("Cannot redefine property on module namespace object").into());
                    }
                }

                return Ok(Value::Object(target_obj));
            }

            // If the target is a proxy, invoke the defineProperty trap
            if let Some(proxy_cell) = crate::core::slot_get(&target_obj, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                let trap_result = crate::js_proxy::apply_proxy_trap(
                    mc,
                    proxy,
                    "defineProperty",
                    vec![
                        (*proxy.target).clone(),
                        crate::js_proxy::property_key_to_value_pub(&prop_key),
                        Value::Object(desc_obj_norm),
                    ],
                    || {
                        // Default: forward to target's [[DefineOwnProperty]]
                        if let Value::Object(target_inner) = &*proxy.target {
                            // If target is itself a proxy, recurse through its [[DefineOwnProperty]]
                            if let Some(inner_proxy_cell) = crate::core::slot_get(target_inner, &InternalSlot::Proxy)
                                && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                            {
                                let mat_desc = materialize_property_descriptor_object(mc, env, &desc_obj)?;
                                let ok = crate::js_proxy::proxy_define_own_property(mc, inner_proxy, &prop_key, &mat_desc)?;
                                return Ok(Value::Boolean(ok));
                            }
                            let mat_desc = materialize_property_descriptor_object(mc, env, &desc_obj)?;
                            define_property_internal(mc, target_inner, &prop_key, &mat_desc)?;
                        }
                        Ok(Value::Boolean(true))
                    },
                )?;
                let success = match trap_result {
                    Value::Boolean(b) => b,
                    _ => trap_result.to_truthy(),
                };
                if !success {
                    return Err(raise_type_error!("'defineProperty' on proxy: trap returned falsish").into());
                }

                // Invariant checks per spec 10.5.6 steps 14-20
                if let Value::Object(target_obj_inner) = &*proxy.target {
                    // Build the target descriptor object for IsCompatiblePropertyDescriptor
                    let target_desc_obj_opt = if let Some(inner_proxy_cell) = crate::core::slot_get(target_obj_inner, &InternalSlot::Proxy)
                        && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                    {
                        crate::js_proxy::proxy_get_own_property_descriptor(mc, inner_proxy, &prop_key)?
                    } else if crate::core::get_own_property(target_obj_inner, &prop_key).is_some() {
                        let td = crate::core::build_property_descriptor(mc, target_obj_inner, &prop_key);
                        match td {
                            Some(pd) => Some(pd.to_object(mc)?),
                            None => None,
                        }
                    } else {
                        None
                    };

                    let extensible_target = if let Some(inner_proxy_cell) = crate::core::slot_get(target_obj_inner, &InternalSlot::Proxy)
                        && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                    {
                        crate::js_proxy::proxy_is_extensible(mc, inner_proxy)?
                    } else {
                        target_obj_inner.borrow().is_extensible()
                    };

                    // Check if desc explicitly sets configurable: false
                    let setting_config_false = crate::core::object_get_key_value(&desc_obj_norm, "configurable")
                        .map(|v| !v.borrow().to_truthy())
                        .unwrap_or(false);

                    if let Some(ref target_desc_obj) = target_desc_obj_opt {
                        // Step 16a: IsCompatiblePropertyDescriptor
                        if !crate::js_proxy::is_compatible_property_descriptor_pub(mc, extensible_target, &desc_obj_norm, target_desc_obj) {
                            return Err(raise_type_error!("'defineProperty' on proxy: trap returned truish for property descriptor that is incompatible with the existing property in the proxy target").into());
                        }
                        // Step 16b: If settingConfigFalse and targetDesc.configurable is true, throw
                        let target_configurable = crate::core::object_get_key_value(target_desc_obj, "configurable")
                            .map(|v| v.borrow().to_truthy())
                            .unwrap_or(false);
                        if setting_config_false && target_configurable {
                            return Err(raise_type_error!("'defineProperty' on proxy: trap returned truish for defining non-configurable property which is configurable in the target").into());
                        }
                        // Step 16c
                        let target_writable = crate::core::object_get_key_value(target_desc_obj, "writable")
                            .map(|v| v.borrow().to_truthy())
                            .unwrap_or(false);
                        if !target_configurable
                            && target_writable
                            && let Some(desc_writ_rc) = crate::core::object_get_key_value(&desc_obj_norm, "writable")
                            && !desc_writ_rc.borrow().to_truthy()
                        {
                            return Err(raise_type_error!("'defineProperty' on proxy: trap returned truish for defining non-writable property which is writable in the non-configurable proxy target").into());
                        }
                    } else {
                        // Step 15: targetDesc is undefined
                        if !extensible_target {
                            return Err(raise_type_error!(
                                "'defineProperty' on proxy: trap returned truish for adding property to non-extensible target"
                            )
                            .into());
                        }
                        if setting_config_false {
                            return Err(raise_type_error!("'defineProperty' on proxy: trap returned truish for defining non-configurable property which does not exist on the target").into());
                        }
                    }
                }

                return Ok(Value::Object(target_obj));
            }

            define_property_internal(mc, &target_obj, &prop_key, &desc_obj_norm)?;
            Ok(Value::Object(target_obj))
        }
        "defineProperties" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Object.defineProperties requires two arguments").into());
            }
            let target_val = args[0].clone();
            let target_obj = match target_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Object.defineProperties called on non-object").into()),
            };

            let props_val = args[1].clone();
            let props_obj = to_object_for_object_static(mc, env, &props_val)?;

            // Collect all descriptors first (per spec ordering/atomicity), then define.
            let keys = if let Some(proxy_cell) = crate::core::slot_get(&props_obj, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                crate::js_proxy::proxy_own_keys(mc, proxy)?
            } else {
                crate::core::ordinary_own_property_keys_mc(mc, &props_obj)?
            };
            let mut descriptors: Vec<(PropertyKey<'gc>, JSObjectDataPtr<'gc>)> = Vec::new();

            for key in keys {
                let is_enumerable = if let Some(proxy_cell) = crate::core::slot_get(&props_obj, &InternalSlot::Proxy)
                    && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                {
                    match crate::js_proxy::proxy_get_own_property_is_enumerable(mc, proxy, &key)? {
                        Some(en) => en,
                        None => continue,
                    }
                } else {
                    if crate::core::get_own_property(&props_obj, &key).is_none() {
                        continue;
                    }
                    props_obj.borrow().is_enumerable(&key)
                };

                if !is_enumerable {
                    continue;
                }

                let desc_val = get_property_with_accessors(mc, env, &props_obj, &key)?;
                let desc_obj = match desc_val {
                    Value::Object(o) => o,
                    _ => return Err(raise_type_error!("Property descriptor must be an object").into()),
                };

                let desc_obj_norm = materialize_property_descriptor_object(mc, env, &desc_obj)?;
                let pd = PropertyDescriptor::from_object(&desc_obj_norm)?;
                crate::core::validate_descriptor_for_define(mc, &pd)?;
                descriptors.push((key, desc_obj_norm));
            }

            for (key, desc_obj) in descriptors {
                let key_val = match key {
                    PropertyKey::String(s) => Value::String(utf8_to_utf16(&s)),
                    PropertyKey::Symbol(sym) => Value::Symbol(sym),
                    PropertyKey::Private(..) => continue,
                    PropertyKey::Internal(..) => continue,
                };

                let define_args = [Value::Object(target_obj), key_val, Value::Object(desc_obj)];
                let _ = handle_object_method(mc, "defineProperty", &define_args, env)?;
            }

            Ok(Value::Object(target_obj))
        }
        "is" => {
            // Object.is(x,y) - SameValue comparison
            let a = args.first().cloned().unwrap_or(Value::Undefined);
            let b = args.get(1).cloned().unwrap_or(Value::Undefined);
            let eq = crate::core::values_equal(mc, &a, &b);
            Ok(Value::Boolean(eq))
        }
        "fromEntries" => {
            // Object.fromEntries(iterable) — §22.1.2.4
            let iterable = args.first().cloned().unwrap_or(Value::Undefined);
            if matches!(iterable, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Object.fromEntries requires an iterable argument").into());
            }

            let result = crate::core::new_js_object_data(mc);
            // Ensure result has Object.prototype
            if let Some(obj_ctor_rc) = crate::core::env_get(env, "Object")
                && let Value::Object(obj_ctor) = &*obj_ctor_rc.borrow()
                && let Some(proto_rc) = object_get_key_value(obj_ctor, "prototype")
                && let Value::Object(proto) = &*proto_rc.borrow()
            {
                result.borrow_mut(mc).prototype = Some(*proto);
            }

            // Step: Get @@iterator method from iterable
            let iter_sym = crate::js_iterator_helpers::get_well_known_symbol(env, "iterator")
                .ok_or_else(|| -> EvalError<'gc> { raise_type_error!("Symbol.iterator not found").into() })?;

            // Support both Object and non-object iterables. For non-objects (e.g. arrays stored as
            // Value::Object anyway), convert to object first.
            let iter_target_obj = match &iterable {
                Value::Object(o) => *o,
                _ => return Err(raise_type_error!("Object.fromEntries requires an iterable argument").into()),
            };
            let method_val =
                crate::core::get_property_with_accessors(mc, env, &iter_target_obj, crate::core::PropertyKey::Symbol(iter_sym))?;

            // IsCallable check that includes Object-wrapped closures
            let method_is_callable = is_callable_for_from_entries(&method_val);
            if matches!(method_val, Value::Undefined | Value::Null) || !method_is_callable {
                return Err(raise_type_error!("Object.fromEntries requires an iterable argument").into());
            }

            let iter_result = crate::core::evaluate_call_dispatch(mc, env, &method_val, Some(&iterable), &[])?;
            let iter_obj = match &iter_result {
                Value::Object(o) => *o,
                _ => return Err(raise_type_error!("Iterator result is not an object").into()),
            };
            let next_method = crate::core::get_property_with_accessors(mc, env, &iter_obj, "next")?;

            // Helper to close iterator
            let close_iterator = |mc2: &MutationContext<'gc>, env2: &JSObjectDataPtr<'gc>, iter_obj2: &JSObjectDataPtr<'gc>| {
                let return_method = crate::core::get_property_with_accessors(mc2, env2, iter_obj2, "return");
                if let Ok(ret_fn) = return_method
                    && is_callable_for_from_entries(&ret_fn)
                {
                    let _ = crate::core::evaluate_call_dispatch(mc2, env2, &ret_fn, Some(&Value::Object(*iter_obj2)), &[]);
                }
            };

            loop {
                let step = crate::core::evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                let step_obj = match &step {
                    Value::Object(o) => *o,
                    _ => return Err(raise_type_error!("Iterator result is not an object").into()),
                };
                let done_val = crate::core::get_property_with_accessors(mc, env, &step_obj, "done")?;
                if done_val.to_truthy() {
                    break;
                }
                let pair = crate::core::get_property_with_accessors(mc, env, &step_obj, "value")?;

                // Each entry must be an object (array-like with [0] and [1]).
                // If it's not an object, close the iterator and throw TypeError.
                let pair_obj = match &pair {
                    Value::Object(o) => *o,
                    _ => {
                        close_iterator(mc, env, &iter_obj);
                        return Err(raise_type_error!("Iterator value is not an object").into());
                    }
                };

                // Get key ("0") — if this throws, close iterator
                let key_result = crate::core::get_property_with_accessors(mc, env, &pair_obj, "0");
                let key_val = match key_result {
                    Ok(v) => v,
                    Err(e) => {
                        close_iterator(mc, env, &iter_obj);
                        return Err(e);
                    }
                };

                // Get value ("1") — if this throws, close iterator
                let val_result = crate::core::get_property_with_accessors(mc, env, &pair_obj, "1");
                let val = match val_result {
                    Ok(v) => v,
                    Err(e) => {
                        close_iterator(mc, env, &iter_obj);
                        return Err(e);
                    }
                };

                // Convert key to property key (supports symbols)
                let key_result2 = key_val.to_property_key(mc, env);
                let key = match key_result2 {
                    Ok(k) => k,
                    Err(e) => {
                        close_iterator(mc, env, &iter_obj);
                        return Err(e);
                    }
                };

                crate::core::object_set_key_value(mc, &result, &key, &val)?;
            }

            Ok(Value::Object(result))
        }
        _ => Err(raise_eval_error!(format!("Object.{method} is not implemented")).into()),
    }
}

pub(crate) fn handle_to_string_method<'gc>(
    mc: &MutationContext<'gc>,
    obj_val: &Value<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if !args.is_empty() {
        return Err(raise_type_error!(format!(
            "{}.toString() takes no arguments, but {} were provided",
            match obj_val {
                Value::Number(_) => "Number",
                Value::BigInt(_) => "BigInt",
                Value::String(_) => "String",
                Value::Boolean(_) => "Boolean",
                Value::Object(_) => "Object",
                Value::Function(_) => "Function",
                Value::Closure(..) | Value::AsyncClosure(..) => "Function",
                Value::Undefined => "undefined",
                Value::Null => "null",
                Value::ClassDefinition(_) => "Class",
                Value::Getter(..) => "Getter",
                Value::Setter(..) => "Setter",
                Value::Property { .. } => "Property",
                Value::Promise(_) => "Promise",
                Value::Symbol(_) => "Symbol",
                Value::Map(_) => "Map",
                Value::Set(_) => "Set",
                Value::WeakMap(_) => "WeakMap",
                Value::WeakSet(_) => "WeakSet",
                Value::GeneratorFunction(..) | Value::AsyncGeneratorFunction(..) => "GeneratorFunction",
                Value::Generator(_) | Value::AsyncGenerator(_) => "Generator",
                Value::Proxy(_) => "Proxy",
                Value::ArrayBuffer(_) => "ArrayBuffer",
                Value::DataView(_) => "DataView",
                Value::TypedArray(_) => "TypedArray",
                Value::Uninitialized => "undefined",
                Value::PrivateName(..) => "PrivateName",
            },
            args.len()
        ))
        .into());
    }

    if let Value::Object(object) = obj_val {
        // Check if this object defines its own toString method (Get semantics)
        if let Some(method_rc) = object_get_key_value(object, "toString") {
            let method_val = method_rc.borrow().clone();
            log::debug!("DBG handle_to_string_method: found toString property => {:?}", method_val);
            match method_val {
                Value::Property { getter, value, .. } => {
                    let actual = if let Some(g) = getter {
                        crate::core::call_accessor(mc, env, object, &g)?
                    } else if let Some(v) = value {
                        v.borrow().clone()
                    } else {
                        Value::Undefined
                    };
                    if matches!(
                        actual,
                        Value::Closure(_) | Value::AsyncClosure(_) | Value::Function(_) | Value::Object(_)
                    ) {
                        log::debug!("DBG handle_to_string_method: calling toString implementation");
                        let res = evaluate_call_dispatch(mc, env, &actual, Some(&obj_val.clone()), &Vec::new())?;
                        log::debug!("DBG handle_to_string_method: toString returned {:?}", res);
                        return Ok(res);
                    }
                    log::debug!("DBG handle_to_string_method: toString not callable -> returning Uninitialized sentinel");
                    return Ok(Value::Uninitialized);
                }
                // If the property is a callable, call it and return the result
                Value::Closure(_) | Value::AsyncClosure(_) | Value::Function(_) | Value::Object(_) => {
                    // If it's an object, it might be a function object (with an internal closure slot)
                    log::debug!("DBG handle_to_string_method: calling toString implementation");
                    let res = evaluate_call_dispatch(mc, env, &method_val, Some(&obj_val.clone()), &Vec::new())?;
                    log::debug!("DBG handle_to_string_method: toString returned {:?}", res);
                    return Ok(res);
                }
                // If the property exists but is not callable (e.g. `toString: null`),
                // per ECMAScript ToPrimitive semantics we must not call the prototype's
                // `toString` - instead indicate there's no callable toString by
                // returning `Uninitialized` so the caller can try `valueOf` next.
                _ => {
                    log::debug!("DBG handle_to_string_method: toString not callable -> returning Uninitialized sentinel");
                    // Indicate "no callable toString" by returning `Uninitialized` as
                    // a sentinel that is not a JS primitive and will cause the
                    // caller to proceed to `valueOf`.
                    return Ok(Value::Uninitialized);
                }
            }
        }
    }

    match obj_val {
        Value::Number(n) => Ok(Value::String(utf8_to_utf16(&crate::core::value_to_string(&Value::Number(*n))))),
        Value::BigInt(h) => Ok(Value::String(utf8_to_utf16(&h.to_string()))),
        Value::String(s) => Ok(Value::String(s.clone())),
        Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
        Value::Undefined => Ok(Value::String(utf8_to_utf16("[object Undefined]"))),
        Value::Null => Ok(Value::String(utf8_to_utf16("[object Null]"))),
        Value::Uninitialized => Ok(Value::String(utf8_to_utf16("[object Undefined]"))),
        Value::Object(object) => {
            // If this is an Error object, use Error.prototype.toString behavior
            if crate::core::js_error::is_error(&Value::Object(*object)) {
                return handle_error_to_string_method(mc, &Value::Object(*object), args, env);
            }

            // Check if this is a wrapped primitive object
            if let Some(wrapped_val) = slot_get_chained(object, &InternalSlot::PrimitiveValue) {
                match &*wrapped_val.borrow() {
                    Value::Number(n) => return Ok(Value::String(utf8_to_utf16(&crate::core::value_to_string(&Value::Number(*n))))),
                    Value::BigInt(h) => return Ok(Value::String(utf8_to_utf16(&h.to_string()))),
                    Value::Boolean(b) => return Ok(Value::String(utf8_to_utf16(&b.to_string()))),
                    Value::String(s) => return Ok(Value::String(s.clone())),
                    _ => {}
                }
            }

            // If this object looks like a Date (has __timestamp), call Date.toString()
            if is_date_object(object) {
                return crate::js_date::handle_date_method(mc, &Value::Object(*object), "toString", &[], env);
            }

            // Engine-internal function-like objects (constructors and callable wrappers)
            // should be branded as Function for Object.prototype.toString.
            if object.borrow().get_closure().is_some()
                || slot_get_chained(object, &InternalSlot::NativeCtor).is_some()
                || slot_get_chained(object, &InternalSlot::IsConstructor).is_some()
                || slot_get_chained(object, &InternalSlot::BoundTarget).is_some()
            {
                return Ok(Value::String(utf8_to_utf16("[object Function]")));
            }

            // If this object looks like an array, join elements with comma (Array.prototype.toString overrides Object.prototype)
            if is_array(mc, object) {
                let current_len = get_array_length(mc, object).unwrap_or(0);
                let mut parts = Vec::new();
                for i in 0..current_len {
                    if let Some(val_rc) = object_get_key_value(object, i) {
                        match &*val_rc.borrow() {
                            Value::Undefined | Value::Null => parts.push("".to_string()), // push empty string for null and undefined
                            Value::String(s) => parts.push(utf16_to_utf8(s)),
                            Value::Number(n) => parts.push(crate::core::value_to_string(&Value::Number(*n))),
                            Value::Boolean(b) => parts.push(b.to_string()),
                            Value::BigInt(b) => parts.push(format!("{}n", b)),
                            _ => parts.push("[object Object]".to_string()),
                        }
                    } else {
                        parts.push("".to_string())
                    }
                }
                return Ok(Value::String(utf8_to_utf16(&parts.join(","))));
            }

            // If this object contains a Symbol.toStringTag property, honor it
            if let Some(tag_sym_rc) = get_well_known_symbol(mc, env, "toStringTag")
                && let Value::Symbol(sd) = &*tag_sym_rc.borrow()
                && let Some(tag_val_rc) = object_get_key_value(object, sd)
                && let Value::String(s) = &*tag_val_rc.borrow()
            {
                return Ok(Value::String(utf8_to_utf16(&format!("[object {}]", utf16_to_utf8(s)))));
            }

            // Default object tag
            Ok(Value::String(utf8_to_utf16("[object Object]")))
        }
        Value::Function(name) => Ok(Value::String(utf8_to_utf16(&format!("[Function: {}]", name)))),
        Value::Closure(..) | Value::AsyncClosure(..) => Ok(Value::String(utf8_to_utf16("[Function]"))),
        Value::ClassDefinition(_) => Ok(Value::String(utf8_to_utf16("[Class]"))),
        Value::Getter(..) => Ok(Value::String(utf8_to_utf16("[Getter]"))),
        Value::Setter(..) => Ok(Value::String(utf8_to_utf16("[Setter]"))),
        Value::Property { .. } => Ok(Value::String(utf8_to_utf16("[Property]"))),
        Value::Promise(_) => Ok(Value::String(utf8_to_utf16("[object Promise]"))),
        Value::Symbol(symbol_data) => {
            let desc_str = symbol_data.description().unwrap_or("");
            Ok(Value::String(utf8_to_utf16(&format!("Symbol({})", desc_str))))
        }
        Value::Map(_) => Ok(Value::String(utf8_to_utf16("[object Map]"))),
        Value::Set(_) => Ok(Value::String(utf8_to_utf16("[object Set]"))),
        Value::WeakMap(_) => Ok(Value::String(utf8_to_utf16("[object WeakMap]"))),
        Value::WeakSet(_) => Ok(Value::String(utf8_to_utf16("[object WeakSet]"))),
        Value::GeneratorFunction(..) | Value::AsyncGeneratorFunction(..) => Ok(Value::String(utf8_to_utf16("[GeneratorFunction]"))),
        Value::Generator(_) | Value::AsyncGenerator(_) => Ok(Value::String(utf8_to_utf16("[object Generator]"))),
        Value::Proxy(_) => Ok(Value::String(utf8_to_utf16("[object Proxy]"))),
        Value::ArrayBuffer(_) => Ok(Value::String(utf8_to_utf16("[object ArrayBuffer]"))),
        Value::DataView(_) => Ok(Value::String(utf8_to_utf16("[object DataView]"))),
        Value::TypedArray(_) => Ok(Value::String(utf8_to_utf16("[object TypedArray]"))),
        Value::PrivateName(n, _) => Ok(Value::String(utf8_to_utf16(&format!("#{}", n)))),
    }
}

pub(crate) fn handle_error_to_string_method<'gc>(
    mc: &MutationContext<'gc>,
    obj_val: &Value<'gc>,
    _args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // 1. Let O be the this value.
    // 2. If Type(O) is not Object, throw a TypeError exception.
    let object = match obj_val {
        Value::Object(o) => *o,
        _ => return Err(raise_type_error!("Error.prototype.toString called on non-object").into()),
    };

    // 3. Let name be ? Get(O, "name").
    let name_val = crate::core::get_property_with_accessors(mc, env, &object, "name")?;
    // 4. If name is undefined, set name to "Error"; otherwise set name to ? ToString(name).
    let name = if matches!(name_val, Value::Undefined) {
        "Error".to_string()
    } else {
        if matches!(name_val, Value::Symbol(_)) {
            return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
        }
        let prim = crate::core::to_primitive(mc, &name_val, "string", env)?;
        if matches!(prim, Value::Symbol(_)) {
            return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
        }
        crate::core::value_to_string(&prim)
    };

    // 5. Let msg be ? Get(O, "message").
    let msg_val = crate::core::get_property_with_accessors(mc, env, &object, "message")?;
    // 6. If msg is undefined, set msg to the empty String; otherwise set msg to ? ToString(msg).
    let message = if matches!(msg_val, Value::Undefined) {
        String::new()
    } else {
        if matches!(msg_val, Value::Symbol(_)) {
            return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
        }
        let prim = crate::core::to_primitive(mc, &msg_val, "string", env)?;
        if matches!(prim, Value::Symbol(_)) {
            return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
        }
        crate::core::value_to_string(&prim)
    };

    // 7. If name is the empty String, return msg.
    if name.is_empty() {
        return Ok(Value::String(utf8_to_utf16(&message)));
    }
    // 8. If msg is the empty String, return name.
    if message.is_empty() {
        return Ok(Value::String(utf8_to_utf16(&name)));
    }
    // 9. Return the string-concatenation of name, the code unit 0x003A (COLON),
    //    the code unit 0x0020 (SPACE), and msg.
    Ok(Value::String(utf8_to_utf16(&format!("{}: {}", name, message))))
}

pub(crate) fn handle_value_of_method<'gc>(
    mc: &MutationContext<'gc>,
    obj_val: &Value<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if !args.is_empty() {
        return Err(raise_type_error!(format!(
            "{}.valueOf() takes no arguments, but {} were provided",
            match obj_val {
                Value::Number(_) => "Number",
                Value::String(_) => "String",
                Value::Boolean(_) => "Boolean",
                Value::Object(_) => "Object",
                Value::Function(_) => "Function",
                Value::Closure(..) | Value::AsyncClosure(..) => "Function",
                Value::Undefined => "undefined",
                Value::Null => "null",
                Value::ClassDefinition(_) => "Class",
                &Value::Getter(..) => "Getter",
                &Value::Setter(..) => "Setter",
                &Value::Property { .. } => "Property",
                &Value::Promise(_) => "Promise",
                Value::BigInt(_) => "BigInt",
                Value::Symbol(_) => "Symbol",
                Value::Map(_) => "Map",
                Value::Set(_) => "Set",
                Value::WeakMap(_) => "WeakMap",
                Value::WeakSet(_) => "WeakSet",
                &Value::GeneratorFunction(..) | &Value::AsyncGeneratorFunction(..) => "GeneratorFunction",
                &Value::Generator(_) | &Value::AsyncGenerator(_) => "Generator",
                &Value::Proxy(_) => "Proxy",
                &Value::ArrayBuffer(_) => "ArrayBuffer",
                &Value::DataView(_) => "DataView",
                &Value::TypedArray(_) => "TypedArray",
                Value::Uninitialized => "undefined",
                Value::PrivateName(..) => "PrivateName",
            },
            args.len()
        ))
        .into());
    }
    match obj_val {
        Value::Number(n) => Ok(Value::Number(*n)),
        Value::BigInt(s) => Ok(Value::BigInt(s.clone())),
        Value::String(s) => Ok(Value::String(s.clone())),
        Value::Boolean(b) => Ok(Value::Boolean(*b)),
        Value::Undefined => Err(raise_type_error!("Cannot convert undefined to object").into()),
        Value::Null => Err(raise_type_error!("Cannot convert null to object").into()),
        Value::Object(obj) => {
            // Check if this is a wrapped primitive object
            if let Some(wrapped_val) = slot_get_chained(obj, &InternalSlot::PrimitiveValue) {
                return Ok(wrapped_val.borrow().clone());
            }
            // If object defines a user valueOf function, call it and use its
            // primitive result if it returns a primitive.
            if let Some(method_rc) = object_get_key_value(obj, "valueOf") {
                let method_val = method_rc.borrow().clone();
                match method_val {
                    Value::Closure(data) | Value::AsyncClosure(data) => {
                        let _params = &data.params;
                        let body = data.body.clone();
                        let captured_env = &data.env;
                        let func_env = prepare_function_call_env(
                            mc,
                            Some(captured_env.as_ref().unwrap()),
                            Some(&Value::Object(*obj)),
                            None,
                            &[],
                            None,
                            Some(env),
                        )?;
                        let result = crate::core::evaluate_statements(mc, &func_env, &body)?;
                        if matches!(
                            result,
                            Value::Number(_)
                                | Value::String(_)
                                | Value::Boolean(_)
                                | Value::BigInt(_)
                                | Value::Symbol(_)
                                | Value::Null
                                | Value::Undefined
                        ) {
                            return Ok(result);
                        }
                    }
                    Value::Function(func_name) => {
                        log::debug!("DBG handle_value_of_method: found function name='{}'", func_name);
                        // If the function is one of the Object.prototype builtins,
                        // handle it directly here to avoid recursion back into
                        // `handle_global_function` which would call us again.
                        if func_name == "Object.prototype.valueOf" {
                            return Ok(Value::Object(*obj));
                        }
                        if func_name == "Object.prototype.toString" {
                            return crate::js_object::handle_to_string_method(mc, &Value::Object(*obj), args, env);
                        }
                        // Handle Date prototype functions specially when invoking as a method
                        if func_name.ends_with(".valueOf") && crate::js_date::is_date_object(obj) {
                            return crate::js_date::handle_date_method(mc, &Value::Object(*obj), "valueOf", args, env);
                        }
                        if func_name.ends_with(".toString") && crate::js_date::is_date_object(obj) {
                            return crate::js_date::handle_date_method(mc, &Value::Object(*obj), "toString", args, env);
                        }
                        if let Some(method) = func_name.strip_prefix("Date.prototype.") {
                            return crate::js_date::handle_date_method(mc, &Value::Object(*obj), method, args, env);
                        }

                        let func_env = prepare_function_call_env(mc, None, Some(&Value::Object(*obj)), None, &[], None, Some(env))?;
                        // Use the central `evaluate_call_dispatch` helper to perform the call so
                        // builtins like `Date.prototype.*` are routed correctly when represented
                        // as `Value::Function` names. This avoids falling back to `handle_global_function`
                        // which does not perform the same prefix-based dispatching.
                        let eval_args: Vec<Value<'gc>> = Vec::new();
                        let res = crate::core::evaluate_call_dispatch(
                            mc,
                            &func_env,
                            &Value::Function(func_name.clone()),
                            Some(&Value::Object(*obj)),
                            &eval_args,
                        )?;
                        if matches!(
                            res,
                            Value::Number(_)
                                | Value::String(_)
                                | Value::Boolean(_)
                                | Value::BigInt(_)
                                | Value::Symbol(_)
                                | Value::Null
                                | Value::Undefined
                        ) {
                            return Ok(res);
                        }
                    }
                    _ => {}
                }
                // Support method stored as a function-object (object wrapping a closure)
                if let Value::Object(func_obj_map) = &*method_rc.borrow()
                    && let Some(cl_rc) = func_obj_map.borrow().get_closure()
                {
                    match &*cl_rc.borrow() {
                        Value::Closure(data) | Value::AsyncClosure(data) => {
                            let _params = &data.params;
                            let body = data.body.clone();
                            let captured_env = &data.env;
                            let func_env = prepare_function_call_env(
                                mc,
                                Some(captured_env.as_ref().unwrap()),
                                Some(&Value::Object(*obj)),
                                None,
                                &[],
                                None,
                                Some(env),
                            )?;
                            let result = crate::core::evaluate_statements(mc, &func_env, &body)?;
                            if matches!(
                                result,
                                Value::Number(_)
                                    | Value::String(_)
                                    | Value::Boolean(_)
                                    | Value::BigInt(_)
                                    | Value::Symbol(_)
                                    | Value::Null
                                    | Value::Undefined
                            ) {
                                return Ok(result);
                            }
                        }
                        _ => {}
                    }
                }
            }
            // If this object looks like a Date (has __timestamp), call Date.valueOf()
            if is_date_object(obj) {
                return crate::js_date::handle_date_method(mc, obj_val, "valueOf", &[], env);
            }
            // For regular objects, return the object itself
            Ok(Value::Object(*obj))
        }
        Value::Function(name) => Ok(Value::Function(name.clone())),
        Value::Closure(data) | Value::AsyncClosure(data) => {
            let closure_data = ClosureData::new(&data.params, &data.body, data.env, None);
            Ok(Value::Closure(Gc::new(mc, closure_data)))
        }
        Value::ClassDefinition(class_def) => Ok(Value::ClassDefinition(*class_def)),
        Value::Getter(body, env, _) => Ok(Value::Getter(body.clone(), *env, None)),
        Value::Setter(param, body, env, _) => Ok(Value::Setter(param.clone(), body.clone(), *env, None)),
        Value::Property { value, getter, setter } => Ok(Value::Property {
            value: *value,
            getter: getter.clone(),
            setter: setter.clone(),
        }),
        Value::Promise(promise) => Ok(Value::Promise(*promise)),
        Value::Symbol(symbol_data) => Ok(Value::Symbol(*symbol_data)),
        Value::Map(map) => Ok(Value::Map(*map)),
        Value::Set(set) => Ok(Value::Set(*set)),
        Value::WeakMap(weakmap) => Ok(Value::WeakMap(*weakmap)),
        Value::WeakSet(weakset) => Ok(Value::WeakSet(*weakset)),
        Value::GeneratorFunction(_, data) => {
            let closure_data = ClosureData::new(&data.params, &data.body, data.env, None);
            Ok(Value::GeneratorFunction(None, Gc::new(mc, closure_data)))
        }
        Value::AsyncGeneratorFunction(_, data) => {
            let closure_data = ClosureData::new(&data.params, &data.body, data.env, None);
            Ok(Value::AsyncGeneratorFunction(None, Gc::new(mc, closure_data)))
        }
        Value::Generator(generator) => Ok(Value::Generator(*generator)),
        Value::AsyncGenerator(generator) => Ok(Value::AsyncGenerator(*generator)),
        Value::Proxy(proxy) => Ok(Value::Proxy(*proxy)),
        Value::ArrayBuffer(array_buffer) => Ok(Value::ArrayBuffer(*array_buffer)),
        Value::DataView(data_view) => Ok(Value::DataView(*data_view)),
        Value::TypedArray(typed_array) => Ok(Value::TypedArray(*typed_array)),
        Value::Uninitialized => Err(raise_type_error!("Cannot convert uninitialized to object").into()),
        Value::PrivateName(..) => Err(raise_type_error!("Cannot convert private name to object").into()),
    }
}

/// Handle built-in Object.prototype.* functions in one place so callers don't
/// have to duplicate matching logic. If `func_name` matches a supported
/// Object.prototype builtin, the function will execute it (evaluating args
/// where needed) and return `Ok(Some(Value))`. If it does not match, returns
/// `Ok(None)` so callers can fall back to other dispatch logic.
pub(crate) fn handle_object_prototype_builtin<'gc>(
    mc: &MutationContext<'gc>,
    func_name: &str,
    object: &JSObjectDataPtr<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    match func_name {
        "Object.prototype.hasOwnProperty" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("hasOwnProperty requires one argument").into());
            }
            let key_val = args[0].clone();
            let key_str_opt = match &key_val {
                Value::String(s) => Some(utf16_to_utf8(s)),
                Value::Number(n) => Some(crate::core::value_to_string(&Value::Number(*n))),
                Value::BigInt(b) => Some(b.to_string()),
                Value::Boolean(b) => Some(b.to_string()),
                Value::Undefined => Some("undefined".to_string()),
                _ => None,
            };
            if let Some(key_str) = &key_str_opt
                && (key_str == "length" || key_str == "name")
            {
                if crate::core::consume_pending_function_delete_hasown_check() {
                    return Ok(Some(Value::Boolean(false)));
                }
                let mut func_name_opt: Option<String> = None;
                if let Some(cl_ptr) = object.borrow().get_closure() {
                    func_name_opt = match &*cl_ptr.borrow() {
                        Value::Function(func_name) => Some(func_name.clone()),
                        Value::Closure(cl) | Value::AsyncClosure(cl) => cl.native_target.clone(),
                        _ => None,
                    };
                }
                if func_name_opt.is_none()
                    && let Some(native_ctor_rc) = crate::core::slot_get_chained(object, &InternalSlot::NativeCtor)
                {
                    match &*native_ctor_rc.borrow() {
                        Value::String(name) => {
                            func_name_opt = Some(utf16_to_utf8(name));
                        }
                        Value::Property { value: Some(v), .. } => {
                            if let Value::String(name) = &*v.borrow() {
                                func_name_opt = Some(utf16_to_utf8(name));
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(func_name) = func_name_opt {
                    let deleted = crate::core::is_deleted_builtin_function_virtual_prop(func_name.as_str(), key_str.as_str());
                    if deleted {
                        return Ok(Some(Value::Boolean(false)));
                    }
                    return Ok(Some(Value::Boolean(true)));
                }
            }
            let ns_export_meta =
                crate::core::get_own_property(object, PropertyKey::Private("__ns_export_names".to_string(), 1)).and_then(|v| {
                    match &*v.borrow() {
                        Value::Object(meta) => Some(*meta),
                        _ => None,
                    }
                });
            let is_module_namespace = {
                let b = object.borrow();
                b.deferred_module_path.is_some() || (b.prototype.is_none() && !b.is_extensible()) || ns_export_meta.is_some()
            };
            if is_module_namespace {
                let key_opt = match &key_val {
                    Value::String(s) => Some(utf16_to_utf8(s)),
                    Value::Number(n) => Some(crate::core::value_to_string(&Value::Number(*n))),
                    Value::BigInt(b) => Some(b.to_string()),
                    Value::Boolean(b) => Some(b.to_string()),
                    Value::Undefined => Some("undefined".to_string()),
                    _ => None,
                };
                if let Some(key) = key_opt {
                    let is_export_name = ns_export_meta
                        .map(|meta| crate::core::get_own_property(&meta, key.as_str()).is_some())
                        .unwrap_or(false);
                    if crate::core::get_own_property(object, key.as_str()).is_some() || is_export_name {
                        let val = crate::core::get_property_with_accessors(mc, env, object, key.as_str())?;
                        if matches!(val, Value::Undefined) {
                            return Err(raise_reference_error!("Cannot access binding before initialization").into());
                        }
                    }
                }
            }
            // For proxy-wrapped objects, delegate to [[GetOwnProperty]] trap
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                let prop_key = key_val.to_property_key(mc, env)?;
                let has = crate::js_proxy::proxy_get_own_property_descriptor(mc, proxy, &prop_key)?.is_some();
                return Ok(Some(Value::Boolean(has)));
            }
            let exists = crate::core::has_own_property_value(object, &key_val);
            Ok(Some(Value::Boolean(exists)))
        }
        "Object.prototype.isPrototypeOf" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("isPrototypeOf requires one argument").into());
            }
            let target_val = args[0].clone();
            match target_val {
                Value::Object(target_map) => {
                    let mut current_opt: Option<Gc<'gc, GcCell<crate::core::JSObjectData<'gc>>>> = target_map.borrow().prototype;
                    while let Some(parent) = current_opt {
                        if Gc::ptr_eq(parent, *object) {
                            return Ok(Some(Value::Boolean(true)));
                        }
                        current_opt = parent.borrow().prototype;
                    }
                    Ok(Some(Value::Boolean(false)))
                }
                _ => Ok(Some(Value::Boolean(false))),
            }
        }
        "Object.prototype.propertyIsEnumerable" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("propertyIsEnumerable requires one argument").into());
            }
            let key_val = args[0].clone();
            let ns_export_meta =
                get_own_property(object, PropertyKey::Private("__ns_export_names".to_string(), 1)).and_then(|v| match &*v.borrow() {
                    Value::Object(meta) => Some(*meta),
                    _ => None,
                });
            let is_module_namespace = {
                let b = object.borrow();
                b.deferred_module_path.is_some() || (b.prototype.is_none() && !b.is_extensible()) || ns_export_meta.is_some()
            };
            if is_module_namespace {
                let key_opt = match &key_val {
                    Value::String(s) => Some(utf16_to_utf8(s)),
                    Value::Number(n) => Some(crate::core::value_to_string(&Value::Number(*n))),
                    Value::BigInt(b) => Some(b.to_string()),
                    Value::Boolean(b) => Some(b.to_string()),
                    Value::Undefined => Some("undefined".to_string()),
                    _ => None,
                };
                if let Some(key) = key_opt {
                    let is_export_name = ns_export_meta
                        .map(|meta| get_own_property(&meta, key.as_str()).is_some())
                        .unwrap_or(false);
                    if get_own_property(object, key.as_str()).is_some() || is_export_name {
                        let val = get_property_with_accessors(mc, env, object, key.as_str())?;
                        if matches!(val, Value::Undefined) {
                            return Err(raise_reference_error!("Cannot access binding before initialization").into());
                        }
                    }
                }
            }
            // For proxy-wrapped objects, delegate to [[GetOwnProperty]] trap
            if let Some(proxy_cell) = crate::core::slot_get(object, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                let prop_key = key_val.to_property_key(mc, env)?;
                let is_enum = crate::js_proxy::proxy_get_own_property_is_enumerable(mc, proxy, &prop_key)?.unwrap_or(false);
                return Ok(Some(Value::Boolean(is_enum)));
            }
            let prop_key = key_val.to_property_key(mc, env)?;
            let exists = crate::core::get_own_property(object, &prop_key).is_some() && object.borrow().is_enumerable(&prop_key);
            Ok(Some(Value::Boolean(exists)))
        }
        "Object.prototype.__lookupGetter__" => {
            let key_val = args.first().cloned().unwrap_or(Value::Undefined);
            let key = match key_val {
                Value::Symbol(sym) => PropertyKey::Symbol(sym),
                other => PropertyKey::String(crate::core::value_to_string(&other)),
            };

            if let PropertyKey::String(s) = &key
                && s.starts_with('#')
            {
                return Ok(Some(Value::Undefined));
            }

            let mut cur = Some(*object);
            while let Some(cur_obj) = cur {
                if let Some(val_rc) = crate::core::get_own_property(&cur_obj, &key) {
                    let val = val_rc.borrow().clone();
                    return match val {
                        Value::Property { getter, .. } => {
                            if let Some(g) = getter {
                                Ok(Some((*g).clone()))
                            } else {
                                Ok(Some(Value::Undefined))
                            }
                        }
                        Value::Getter(..) => Ok(Some(val)),
                        _ => Ok(Some(Value::Undefined)),
                    };
                }
                cur = cur_obj.borrow().prototype;
            }
            Ok(Some(Value::Undefined))
        }
        "Object.prototype.__lookupSetter__" => {
            let key_val = args.first().cloned().unwrap_or(Value::Undefined);
            let key = match key_val {
                Value::Symbol(sym) => PropertyKey::Symbol(sym),
                other => PropertyKey::String(crate::core::value_to_string(&other)),
            };

            if let PropertyKey::String(s) = &key
                && s.starts_with('#')
            {
                return Ok(Some(Value::Undefined));
            }

            let mut cur = Some(*object);
            while let Some(cur_obj) = cur {
                if let Some(val_rc) = crate::core::get_own_property(&cur_obj, &key) {
                    let val = val_rc.borrow().clone();
                    return match val {
                        Value::Property { setter, .. } => {
                            if let Some(s) = setter {
                                Ok(Some((*s).clone()))
                            } else {
                                Ok(Some(Value::Undefined))
                            }
                        }
                        Value::Setter(..) => Ok(Some(val)),
                        _ => Ok(Some(Value::Undefined)),
                    };
                }
                cur = cur_obj.borrow().prototype;
            }
            Ok(Some(Value::Undefined))
        }
        "Object.prototype.toString" => Ok(Some(crate::js_object::handle_to_string_method(
            mc,
            &Value::Object(*object),
            args,
            env,
        )?)),
        "Object.prototype.valueOf" => Ok(Some(crate::js_object::handle_value_of_method(
            mc,
            &Value::Object(*object),
            args,
            env,
        )?)),
        "Object.prototype.toLocaleString" => Ok(Some(crate::js_object::handle_to_string_method(
            mc,
            &Value::Object(*object),
            args,
            env,
        )?)),
        _ => Ok(None),
    }
}

fn get_well_known_symbol<'gc>(_mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, name: &str) -> Option<GcPtr<'gc, Value<'gc>>> {
    if let Some(sym_ctor_val) = crate::core::env_get(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_ctor_val.borrow()
        && let Some(sym_val) = object_get_key_value(sym_ctor, name)
        && let Value::Symbol(_) = &*sym_val.borrow()
    {
        return Some(sym_val);
    }
    None
}
