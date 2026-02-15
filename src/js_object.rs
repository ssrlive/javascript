use crate::core::{
    ClosureData, EvalError, JSObjectDataPtr, PropertyDescriptor, PropertyKey, Value, evaluate_call_dispatch, get_own_property,
    get_property_with_accessors, new_js_object_data, object_get_key_value, object_set_key_value, prepare_closure_call_env,
    prepare_function_call_env, value_to_string,
};
use crate::core::{Gc, GcCell, GcPtr, MutationContext, new_gc_cell_ptr};
use crate::error::JSError;
use crate::js_array::{get_array_length, is_array, set_array_length};
use crate::js_date::is_date_object;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};

pub fn initialize_object_module<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    // 1. Create Object constructor
    let object_ctor = new_js_object_data(mc);
    object_set_key_value(mc, &object_ctor, "__is_constructor", &Value::Boolean(true))?;
    object_set_key_value(mc, &object_ctor, "__native_ctor", &Value::String(utf8_to_utf16("Object")))?;

    // Register Object in the environment
    crate::core::env_set(mc, env, "Object", &Value::Object(object_ctor))?;

    // 2. Create Object.prototype
    let object_proto = new_js_object_data(mc);
    // Link prototype and constructor
    object_set_key_value(mc, &object_ctor, "prototype", &Value::Object(object_proto))?;
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
    }

    // 4. Register prototype methods
    let proto_methods = vec![
        "hasOwnProperty",
        "isPrototypeOf",
        "propertyIsEnumerable",
        "toLocaleString",
        "toString",
        "valueOf",
        "__lookupGetter__",
        "__lookupSetter__",
    ];

    for method in proto_methods {
        object_set_key_value(mc, &object_proto, method, &Value::Function(format!("Object.prototype.{method}")))?;
        // Methods on prototypes should be non-enumerable so for..in doesn't list them
        object_proto.borrow_mut(mc).set_non_enumerable(method);
    }

    Ok(())
}

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
    if let Some(existing_rc) = object_get_key_value(target_obj, prop_key)
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
            Value::Property { value: _, getter, setter } => getter.is_some() || setter.is_some(),
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
                    other => other.clone(),
                };
                if !crate::core::values_equal(mc, &existing_val, pd_value) {
                    return Err(raise_type_error!("Cannot change value of non-writable, non-configurable property"));
                }
            }
        } else {
            // existing is accessor
            // Disallow converting to data property
            if pd.value.is_some() || pd.writable.is_some() {
                return Err(raise_type_error!("Cannot convert non-configurable accessor to a data property"));
            }

            // Disallow changing getter/setter functions on non-configurable accessor
            if pd.get.is_some() || pd.set.is_some() {
                return Err(raise_type_error!(
                    "Cannot change getter/setter of non-configurable accessor property"
                ));
            }
        }
    }

    let mut getter_opt: Option<Box<Value>> = None;
    if pd.get.is_some()
        && let Some(get_val) = pd.get.clone()
        && !matches!(get_val, Value::Undefined)
    {
        getter_opt = Some(Box::new(get_val));
    }

    let mut setter_opt: Option<Box<Value>> = None;
    if pd.set.is_some()
        && let Some(set_val) = pd.set.clone()
        && !matches!(set_val, Value::Undefined)
    {
        setter_opt = Some(Box::new(set_val));
    }

    // Create property descriptor value
    let prop_descriptor = Value::Property {
        value: pd.value.clone().map(|v| new_gc_cell_ptr(mc, v)),
        getter: getter_opt,
        setter: setter_opt,
    };

    // DEBUG: Log raw descriptor fields for troubleshooting
    log::debug!("define_property_internal: descriptor writable raw = {:?}", pd.writable);
    log::debug!("define_property_internal: descriptor enumerable raw = {:?}", pd.enumerable);

    // Compute existence and configurability BEFORE applying the new configurable flag.
    // This ensures that when a configurable property is being redefined as non-configurable,
    // we still allow writable/enumerable attributes to be updated in the same operation.
    let is_property_desc = pd.value.is_some() || pd.get.is_some() || pd.set.is_some();
    let existed = object_get_key_value(target_obj, prop_key).is_some();
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

    // If descriptor is a property descriptor (has value/get/set), unspecified attributes
    // default to false only when creating a new property. Redefining an existing property
    // must preserve omitted attributes.
    if is_property_desc && !existed {
        // Only default missing 'writable' to false for data descriptors (value or writable present).
        // For accessor descriptors (get/set) the 'writable' attribute does not apply and
        // must not be defaulted to false here.
        if (pd.value.is_some() || pd.writable.is_some()) && pd.writable.is_none() {
            log::trace!(
                "define_property_internal: writable absent -> default false; setting non-writable for {:?} on obj_ptr={:p}",
                prop_key,
                target_obj.as_ptr()
            );
            target_obj.borrow_mut(mc).set_non_writable(prop_key.clone());
        }
        // Default missing 'enumerable' to false
        if pd.enumerable.is_none() {
            log::trace!(
                "define_property_internal: enumerable absent -> default false; setting non-enumerable for {:?} on obj_ptr={:p}",
                prop_key,
                target_obj.as_ptr()
            );
            target_obj.borrow_mut(mc).set_non_enumerable(prop_key.clone());
        }
        // Default missing 'configurable' to false
        if pd.configurable.is_none() {
            log::trace!(
                "define_property_internal: configurable absent -> default false; setting non-configurable for {:?} on obj_ptr={:p}",
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
    // Ensure explicitly-requested non-enumerable flag is applied even after
    // the property value has been stored. This guards against cases where
    // insertion order or existing property state prevented the marker from
    // being set earlier in the function.
    if pd.enumerable == Some(false) {
        target_obj.borrow_mut(mc).set_non_enumerable(prop_key.clone());
    }
    Ok(())
}

pub fn handle_object_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    match method {
        "keys" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.keys requires at least one argument"));
            }
            if args.len() > 1 {
                return Err(raise_type_error!("Object.keys accepts only one argument"));
            }
            match args[0] {
                Value::Object(obj) => {
                    let mut keys = Vec::new();
                    let ordered = crate::core::ordinary_own_property_keys_mc(mc, &obj)?;
                    for key in ordered {
                        if !obj.borrow().is_enumerable(&key) {
                            continue;
                        }
                        if let PropertyKey::String(s) = key {
                            let is_module_namespace = {
                                let b = obj.borrow();
                                b.deferred_module_path.is_some() || (b.prototype.is_none() && !b.is_extensible())
                            };
                            if is_module_namespace {
                                let _ = crate::core::get_property_with_accessors(mc, env, &obj, s.as_str())?;
                            }
                            // Only include string keys (array indices and others). Skip 'length' because it's non-enumerable
                            keys.push(Value::String(utf8_to_utf16(&s)));
                        }
                    }
                    // Create a proper Array for keys
                    let result_obj = crate::js_array::create_array(mc, env)?;
                    let len = keys.len();
                    for (i, key) in keys.into_iter().enumerate() {
                        object_set_key_value(mc, &result_obj, i, &key)?;
                    }
                    set_array_length(mc, &result_obj, len)?;
                    Ok(Value::Object(result_obj))
                }
                Value::Undefined => Err(raise_type_error!("Object.keys called on undefined")),
                _ => {
                    // For primitive values, return empty array (like in JS)
                    let result_obj = crate::js_array::create_array(mc, env)?;
                    set_array_length(mc, &result_obj, 0)?;
                    Ok(Value::Object(result_obj))
                }
            }
        }
        "values" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.values requires at least one argument"));
            }
            if args.len() > 1 {
                return Err(raise_type_error!("Object.values accepts only one argument"));
            }
            match args[0] {
                Value::Object(obj) => {
                    let mut values = Vec::new();
                    let ordered = crate::core::ordinary_own_property_keys_mc(mc, &obj)?;
                    for key in ordered {
                        if !obj.borrow().is_enumerable(&key) {
                            continue;
                        }
                        if let PropertyKey::String(_s) = &key {
                            if let PropertyKey::String(s) = &key {
                                let is_module_namespace = {
                                    let b = obj.borrow();
                                    b.deferred_module_path.is_some() || (b.prototype.is_none() && !b.is_extensible())
                                };
                                if is_module_namespace {
                                    let _ = crate::core::get_property_with_accessors(mc, env, &obj, s.as_str())?;
                                }
                            }
                            // Only include string keys (array indices and others); 'length' is non-enumerable so won't appear
                            if let Some(v_rc) = object_get_key_value(&obj, &key) {
                                values.push(v_rc.borrow().clone());
                            }
                        }
                    }
                    // Create a proper Array for values
                    let result_obj = crate::js_array::create_array(mc, env)?;
                    let len = values.len();
                    for (i, value) in values.into_iter().enumerate() {
                        object_set_key_value(mc, &result_obj, i, &value)?;
                    }
                    set_array_length(mc, &result_obj, len)?;
                    Ok(Value::Object(result_obj))
                }
                Value::Undefined => Err(raise_type_error!("Object.values called on undefined")),
                _ => {
                    // For primitive values, return empty array (like in JS)
                    let result_obj = crate::js_array::create_array(mc, env)?;
                    set_array_length(mc, &result_obj, 0)?;
                    Ok(Value::Object(result_obj))
                }
            }
        }
        "hasOwn" => {
            if args.len() != 2 {
                return Err(raise_type_error!("Object.hasOwn requires exactly two arguments"));
            }
            let obj_val = args[0].clone();
            let prop_val = args[1].clone();

            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot convert undefined or null to object"));
            }

            let key = match prop_val {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::BigInt(b) => PropertyKey::String(b.to_string()),
                Value::Symbol(sd) => PropertyKey::Symbol(sd),
                Value::Object(_) => {
                    // ToPropertyKey semantics: ToPrimitive with hint 'string'
                    let prim = crate::core::to_primitive(mc, &prop_val, "string", env)?;
                    match prim {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                        Value::Symbol(s) => PropertyKey::Symbol(s),
                        other => PropertyKey::String(value_to_string(&other)),
                    }
                }
                val => PropertyKey::String(value_to_string(&val)),
            };

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
                return Err(raise_type_error!("Object.getPrototypeOf requires exactly one argument"));
            }
            let obj_val = args[0].clone();
            match obj_val {
                Value::Object(obj) => {
                    if let Some(proto_rc) = obj.borrow().prototype {
                        log::debug!(
                            "DBG Object.getPrototypeOf: obj ptr={:p} -> proto ptr={:p}",
                            Gc::as_ptr(obj),
                            Gc::as_ptr(proto_rc)
                        );
                        // DIAG: print whether object has an own '__proto__' property and its value
                        if let Some(pv) = object_get_key_value(&obj, "__proto__") {
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
                        if let Some(pv) = object_get_key_value(&obj, "__proto__") {
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
                Value::Undefined | Value::Null => Err(raise_type_error!("Cannot convert undefined or null to object")),
                _ => {
                    // For primitives, we should ideally return the prototype of the wrapper.
                    // For now, returning Null is a safe fallback if wrappers aren't fully supported.
                    Ok(Value::Null)
                }
            }
        }
        "isExtensible" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.isExtensible requires exactly one argument"));
            }
            let obj_val = args[0].clone();
            match obj_val {
                Value::Object(obj) => Ok(Value::Boolean(obj.borrow().is_extensible())),
                Value::Undefined | Value::Null => Err(raise_type_error!("Cannot convert undefined or null to object")),
                _ => {
                    // Primitives are considered wrapped objects; treat as extensible for now
                    Ok(Value::Boolean(true))
                }
            }
        }
        "preventExtensions" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.preventExtensions requires exactly one argument"));
            }
            match &args[0] {
                Value::Object(obj) => {
                    obj.borrow_mut(mc).prevent_extensions();
                    Ok(Value::Object(*obj))
                }
                _ => Err(raise_type_error!("Object.preventExtensions called on non-object")),
            }
        }
        "seal" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.seal requires exactly one argument"));
            }
            match &args[0] {
                Value::Object(obj) => {
                    // Make all own properties non-configurable
                    let ordered = crate::core::ordinary_own_property_keys_mc(mc, obj)?;
                    for k in ordered {
                        obj.borrow_mut(mc).set_non_configurable(k.clone());
                    }
                    // Make non-extensible
                    obj.borrow_mut(mc).prevent_extensions();
                    Ok(Value::Object(*obj))
                }
                _ => Err(raise_type_error!("Object.seal called on non-object")),
            }
        }
        "isSealed" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.isSealed requires exactly one argument"));
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

                    if obj.borrow().is_extensible() {
                        return Ok(Value::Boolean(false));
                    }
                    let ordered = crate::core::ordinary_own_property_keys_mc(mc, &obj)?;
                    for k in ordered {
                        if obj.borrow().is_configurable(&k) {
                            return Ok(Value::Boolean(false));
                        }
                    }
                    Ok(Value::Boolean(true))
                }
                Value::Undefined | Value::Null => Err(raise_type_error!("Cannot convert undefined or null to object")),
                _ => Ok(Value::Boolean(true)),
            }
        }
        "freeze" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.freeze requires exactly one argument"));
            }
            match &args[0] {
                Value::Object(obj) => {
                    let is_module_namespace = {
                        let b = obj.borrow();
                        b.deferred_module_path.is_some() || b.deferred_cache_env.is_some() || (b.prototype.is_none() && !b.is_extensible())
                    };
                    if is_module_namespace {
                        return Err(raise_type_error!("Cannot freeze module namespace object"));
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
                _ => Err(raise_type_error!("Object.freeze called on non-object")),
            }
        }
        "isFrozen" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.isFrozen requires exactly one argument"));
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

                    if obj.borrow().is_extensible() {
                        return Ok(Value::Boolean(false));
                    }
                    let ordered = crate::core::ordinary_own_property_keys_mc(mc, &obj)?;
                    for k in ordered {
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
                    Ok(Value::Boolean(true))
                }
                Value::Undefined | Value::Null => Err(raise_type_error!("Cannot convert undefined or null to object")),
                _ => Ok(Value::Boolean(true)),
            }
        }
        "groupBy" => {
            if args.len() != 2 {
                return Err(raise_type_error!("Object.groupBy requires exactly two arguments"));
            }
            let items_val = args[0].clone();
            let callback_val = args[1].clone();

            let items_obj = match items_val {
                Value::Object(obj) => obj,
                _ => return Err(raise_type_error!("Object.groupBy expects an object as first argument")),
            };

            let result_obj = new_js_object_data(mc);
            // Object.groupBy returns a null-prototype object
            result_obj.borrow_mut(mc).prototype = None;

            let len = get_array_length(mc, &items_obj).unwrap_or(0);

            for i in 0..len {
                if let Some(val_rc) = object_get_key_value(&items_obj, i) {
                    let val = val_rc.borrow().clone();

                    let key_val = if let Some((params, body, captured_env)) = crate::core::extract_closure_from_value(&callback_val) {
                        let args = vec![val.clone(), Value::Number(i as f64)];
                        let func_env = prepare_closure_call_env(mc, Some(&captured_env), Some(&params), &args, Some(env))?;
                        crate::core::evaluate_statements(mc, &func_env, &body)?
                    } else {
                        return Err(raise_type_error!("Object.groupBy expects a function as second argument"));
                    };

                    let key = match key_val {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::BigInt(b) => PropertyKey::String(b.to_string()),
                        Value::Symbol(sd) => PropertyKey::Symbol(sd),
                        _ => PropertyKey::String(value_to_string(&key_val)),
                    };

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
                }
            }

            Ok(Value::Object(result_obj))
        }
        "create" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.create requires at least one argument"));
            }
            let proto_val = args[0].clone();
            let proto_obj = match proto_val {
                Value::Object(obj) => Some(obj),
                Value::Undefined | Value::Null => None,
                _ => {
                    return Err(raise_type_error!("Object.create prototype must be an object, null, or undefined"));
                }
            };

            // Create new object
            let new_obj = new_js_object_data(mc);

            // Set prototype
            if let Some(proto) = proto_obj {
                new_obj.borrow_mut(mc).prototype = Some(proto);
            }

            // If properties descriptor is provided, add properties
            if args.len() > 1 {
                let props_val = args[1].clone();
                if let Value::Object(props_obj) = props_val {
                    for (key, desc_val) in props_obj.borrow().properties.iter() {
                        if let Value::Object(desc_obj) = &*desc_val.borrow() {
                            // Handle property descriptor
                            let _value = if let Some(val) = object_get_key_value(desc_obj, "value") {
                                val.borrow().clone()
                            } else {
                                Value::Undefined
                            };
                            let mut desc_obj_copy = desc_obj.borrow().clone();
                            let desc_obj_ptr = new_js_object_data(mc);
                            for (k, v) in desc_obj_copy.properties.drain(..) {
                                object_set_key_value(mc, &desc_obj_ptr, k, &v.borrow().clone())?;
                            }
                            define_property_internal(mc, &new_obj, key, &desc_obj_ptr)?;
                        }
                    }
                }
            }

            Ok(Value::Object(new_obj))
        }
        "setPrototypeOf" => {
            if args.len() != 2 {
                return Err(raise_type_error!("Object.setPrototypeOf requires exactly two arguments"));
            }
            match &args[0] {
                Value::Object(obj) => match &args[1] {
                    Value::Object(proto_obj) => {
                        let current_proto = obj.borrow().prototype;
                        let is_extensible = obj.borrow().is_extensible();
                        let same_proto = current_proto.is_some_and(|p| crate::core::Gc::ptr_eq(p, *proto_obj));
                        if !is_extensible && !same_proto {
                            return Err(raise_type_error!("Cannot set prototype of non-extensible object"));
                        }
                        obj.borrow_mut(mc).prototype = Some(*proto_obj);
                        Ok(Value::Object(*obj))
                    }
                    Value::Undefined | Value::Null => {
                        let current_proto = obj.borrow().prototype;
                        let is_extensible = obj.borrow().is_extensible();
                        if !is_extensible && current_proto.is_some() {
                            return Err(raise_type_error!("Cannot set prototype of non-extensible object"));
                        }
                        obj.borrow_mut(mc).prototype = None;
                        Ok(Value::Object(*obj))
                    }
                    Value::Function(func_name) => {
                        if !obj.borrow().is_extensible() {
                            return Err(raise_type_error!("Cannot set prototype of non-extensible object"));
                        }
                        // Functions are objects in JS. Our engine represents some built-ins as Value::Function,
                        // so wrap it in an object shell that behaves like a function object for prototype chains.
                        let fn_obj = new_js_object_data(mc);
                        if let Some(func_ctor_val) = object_get_key_value(env, "Function")
                            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
                            && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
                            && let Value::Object(func_proto) = &*proto_val.borrow()
                        {
                            fn_obj.borrow_mut(mc).prototype = Some(*func_proto);
                        }
                        fn_obj
                            .borrow_mut(mc)
                            .set_closure(Some(new_gc_cell_ptr(mc, Value::Function(func_name.clone()))));
                        obj.borrow_mut(mc).prototype = Some(fn_obj);
                        Ok(Value::Object(*obj))
                    }
                    _ => Err(raise_type_error!("Object.setPrototypeOf prototype must be an object or null")),
                },
                _ => Err(raise_type_error!("Object.setPrototypeOf target must be an object")),
            }
        }
        "getOwnPropertySymbols" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.getOwnPropertySymbols requires exactly one argument"));
            }
            match args[0] {
                Value::Object(obj) => {
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
                _ => Err(raise_type_error!("Object.getOwnPropertySymbols called on non-object")),
            }
        }
        "getOwnPropertyNames" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.getOwnPropertyNames requires exactly one argument"));
            }
            match args[0] {
                Value::Object(obj) => {
                    crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, None)?;
                    let result_obj = crate::js_array::create_array(mc, env)?;
                    let mut idx = 0;
                    let ordered = crate::core::ordinary_own_property_keys_mc(mc, &obj)?;
                    for key in ordered {
                        if let PropertyKey::String(s) = key {
                            if s == "__is_array" {
                                continue;
                            }
                            object_set_key_value(mc, &result_obj, idx, &Value::String(utf8_to_utf16(&s)))?;
                            idx += 1;
                        }
                    }
                    set_array_length(mc, &result_obj, idx)?;
                    Ok(Value::Object(result_obj))
                }
                _ => Err(raise_type_error!("Object.getOwnPropertyNames called on non-object")),
            }
        }
        "getOwnPropertyDescriptor" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Object.getOwnPropertyDescriptor requires at least two arguments"));
            }
            let obj_val = args[0].clone();
            let obj = match obj_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Object.getOwnPropertyDescriptor called on non-object")),
            };

            let prop_val = args[1].clone();
            let key = match prop_val {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::BigInt(b) => PropertyKey::String(b.to_string()),
                Value::Symbol(sd) => PropertyKey::Symbol(sd),
                Value::Object(_) => {
                    let prim = crate::core::to_primitive(mc, &prop_val, "string", env)?;
                    match prim {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                        Value::Symbol(s) => PropertyKey::Symbol(s),
                        other => PropertyKey::String(value_to_string(&other)),
                    }
                }
                val => PropertyKey::String(value_to_string(&val)),
            };

            if let PropertyKey::String(s) = &key {
                crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, Some(s.as_str()))?;
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
                return Err(raise_type_error!("Object.getOwnPropertyDescriptors requires exactly one argument"));
            }
            let obj_val = args[0].clone();
            match obj_val {
                Value::Object(ref obj) => {
                    crate::js_module::ensure_deferred_namespace_evaluated(mc, env, obj, None)?;
                    let result_obj = new_js_object_data(mc);

                    let ordered = crate::core::ordinary_own_property_keys_mc(mc, obj)?;
                    for key in &ordered {
                        if let Some(pd) = crate::core::build_property_descriptor(mc, obj, key) {
                            let desc_obj = pd.to_object(mc)?; // Put descriptor onto result using the original key (string or symbol)
                            match key {
                                PropertyKey::String(s) => {
                                    object_set_key_value(mc, &result_obj, s, &Value::Object(desc_obj))?;
                                }
                                PropertyKey::Symbol(sym_rc) => {
                                    // Push symbol-keyed property on returned object with the same symbol key
                                    let property_key = PropertyKey::Symbol(*sym_rc);
                                    object_set_key_value(mc, &result_obj, &property_key, &Value::Object(desc_obj))?;
                                }
                                PropertyKey::Private(..) => {}
                            }
                        }
                    }

                    Ok(Value::Object(result_obj))
                }
                _ => Err(raise_type_error!("Object.getOwnPropertyDescriptors called on non-object")),
            }
        }
        "assign" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.assign requires at least one argument"));
            }
            // Evaluate target and apply ToObject semantics: throw on undefined,
            // box primitives into corresponding object wrappers, or use the
            // object directly.
            let target_val = args[0].clone();
            let target_obj = match target_val {
                Value::Object(o) => o,
                Value::Undefined => return Err(raise_type_error!("Object.assign target cannot be undefined or null")),
                Value::Number(n) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "valueOf", &Value::Function("Number_valueOf".to_string()))?;
                    object_set_key_value(mc, &obj, "toString", &Value::Function("Number_toString".to_string()))?;
                    object_set_key_value(mc, &obj, "__value__", &Value::Number(n))?;
                    // Set prototype to Number.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Number");
                    obj
                }
                Value::Boolean(b) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "valueOf", &Value::Function("Boolean_valueOf".to_string()))?;
                    object_set_key_value(mc, &obj, "toString", &Value::Function("Boolean_toString".to_string()))?;
                    object_set_key_value(mc, &obj, "__value__", &Value::Boolean(b))?;
                    // Set prototype to Boolean.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Boolean");
                    obj
                }
                Value::String(s) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "valueOf", &Value::Function("String_valueOf".to_string()))?;
                    object_set_key_value(mc, &obj, "toString", &Value::Function("String_toString".to_string()))?;
                    object_set_key_value(mc, &obj, "length", &Value::Number(s.len() as f64))?;
                    object_set_key_value(mc, &obj, "__value__", &Value::String(s.clone()))?;
                    // Set prototype to String.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "String");
                    obj
                }
                Value::BigInt(h) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "__value__", &Value::BigInt(h.clone()))?;
                    // Set prototype to BigInt.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "BigInt");
                    obj
                }
                Value::Symbol(sd) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "__value__", &Value::Symbol(sd))?;

                    // Set prototype to Symbol.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Symbol");
                    obj
                }
                // For other types (functions, closures, etc.), create a plain object
                _ => new_js_object_data(mc),
            };

            // Iterate sources
            for src_expr in args.iter().skip(1) {
                let src_val = src_expr.clone();
                if let Value::Object(source_obj) = src_val {
                    let ordered = crate::core::ordinary_own_property_keys_mc(mc, &source_obj)?;
                    for key in ordered {
                        if key == "__proto__".into() {
                            continue;
                        }
                        // Copy both string and symbol keyed enumerable own properties
                        if (matches!(key, PropertyKey::String(_)) || matches!(key, PropertyKey::Symbol(_)))
                            && source_obj.borrow().is_enumerable(&key)
                            && let Some(v_rc) = object_get_key_value(&source_obj, &key)
                        {
                            object_set_key_value(mc, &target_obj, &key, &v_rc.borrow().clone())?;
                        }
                    }
                }
                // non-objects are skipped, like in JS
            }

            Ok(Value::Object(target_obj))
        }
        "defineProperty" => {
            // Minimal implementation: Object.defineProperty(target, prop, descriptor)
            if args.len() < 3 {
                return Err(raise_type_error!("Object.defineProperty requires three arguments"));
            }
            let target_val = args[0].clone();
            let target_obj = match target_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Object.defineProperty called on non-object")),
            };

            let prop_val = args[1].clone();
            // Determine property key (support strings & numbers for now)
            let prop_key = match prop_val {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                Value::Symbol(s) => PropertyKey::Symbol(s),
                _ => return Err(raise_type_error!("Unsupported property key type in Object.defineProperty")),
            };

            if let PropertyKey::String(s) = &prop_key {
                crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &target_obj, Some(s.as_str()))?;
            }

            let desc_val = args[2].clone();
            let desc_obj = match desc_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Property descriptor must be an object")),
            };

            let pd = PropertyDescriptor::from_object(&desc_obj)?;
            crate::core::validate_descriptor_for_define(mc, &pd)?;

            let is_module_namespace = {
                let b = target_obj.borrow();
                b.deferred_module_path.is_some() || b.deferred_cache_env.is_some() || (b.prototype.is_none() && !b.is_extensible())
            };
            if is_module_namespace {
                if pd.get.is_some() || pd.set.is_some() {
                    return Err(raise_type_error!("Cannot redefine property on module namespace object"));
                }

                match &prop_key {
                    PropertyKey::String(name) => {
                        if crate::core::build_property_descriptor(mc, &target_obj, &prop_key).is_none() {
                            return Err(raise_type_error!("Cannot redefine property on module namespace object"));
                        }
                        if pd.configurable == Some(true) || pd.enumerable == Some(false) || pd.writable == Some(false) {
                            return Err(raise_type_error!("Cannot redefine property on module namespace object"));
                        }
                        if let Some(v) = pd.value {
                            let cur = crate::core::get_property_with_accessors(mc, env, &target_obj, name.as_str())?;
                            if !crate::core::values_equal(mc, &cur, &v) {
                                return Err(raise_type_error!("Cannot redefine property on module namespace object"));
                            }
                        }
                    }
                    PropertyKey::Symbol(sym) if sym.description() == Some("Symbol.toStringTag") => {
                        if pd.configurable == Some(true) || pd.enumerable == Some(true) || pd.writable == Some(true) {
                            return Err(raise_type_error!("Cannot redefine property on module namespace object"));
                        }
                        if let Some(v) = pd.value
                            && !crate::core::values_equal(mc, &v, &Value::String(utf8_to_utf16("Module")))
                        {
                            return Err(raise_type_error!("Cannot redefine property on module namespace object"));
                        }
                    }
                    _ => {
                        return Err(raise_type_error!("Cannot redefine property on module namespace object"));
                    }
                }

                return Ok(Value::Object(target_obj));
            }

            define_property_internal(mc, &target_obj, &prop_key, &desc_obj)?;
            Ok(Value::Object(target_obj))
        }
        "defineProperties" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Object.defineProperties requires two arguments"));
            }
            let target_val = args[0].clone();
            let target_obj = match target_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Object.defineProperties called on non-object")),
            };

            let props_val = args[1].clone();
            let props_obj = match props_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Object.defineProperties requires an object as second argument")),
            };

            // Iterate over own properties of props_obj
            for (key, val_rc) in props_obj.borrow().properties.iter() {
                // Only process own properties (already handled by properties map)
                // In JS, it also checks enumerability, but for now we iterate all.
                // Actually, Object.defineProperties only uses own enumerable properties.
                // Let's check enumerability.
                if !props_obj.borrow().is_enumerable(key) {
                    continue;
                }

                let desc_val = val_rc.borrow().clone();
                let desc_obj = match desc_val {
                    Value::Object(o) => o,
                    _ => return Err(raise_type_error!("Property descriptor must be an object")),
                };

                let pd = PropertyDescriptor::from_object(&desc_obj)?;
                crate::core::validate_descriptor_for_define(mc, &pd)?;

                define_property_internal(mc, &target_obj, key, &desc_obj)?;
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
        _ => Err(raise_eval_error!(format!("Object.{method} is not implemented"))),
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
                return handle_error_to_string_method(mc, &Value::Object(*object), args);
            }

            // Check if this is a wrapped primitive object
            if let Some(wrapped_val) = object_get_key_value(object, "__value__") {
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
    _mc: &MutationContext<'gc>,
    obj_val: &Value<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError<'gc>> {
    if !args.is_empty() {
        return Err(raise_type_error!("Error.prototype.toString takes no arguments").into());
    }

    // Expect an object receiver
    if let Value::Object(object) = obj_val {
        // name default to "Error"
        let name = if let Some(n_rc) = object_get_key_value(object, "name") {
            if let Value::String(s) = &*n_rc.borrow() {
                utf16_to_utf8(s)
            } else {
                "Error".to_string()
            }
        } else {
            "Error".to_string()
        };

        // message default to empty
        let message = if let Some(m_rc) = object_get_key_value(object, "message") {
            if let Value::String(s) = &*m_rc.borrow() {
                utf16_to_utf8(s)
            } else {
                "".to_string()
            }
        } else {
            "".to_string()
        };

        if message.is_empty() {
            Ok(Value::String(utf8_to_utf16(&name)))
        } else {
            Ok(Value::String(utf8_to_utf16(&format!("{}: {}", name, message))))
        }
    } else {
        Err(raise_type_error!("Error.prototype.toString called on non-object").into())
    }
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
            if let Some(wrapped_val) = object_get_key_value(obj, "__value__") {
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
                            Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::BigInt(_) | Value::Symbol(_)
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
                            Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::BigInt(_) | Value::Symbol(_)
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
                                Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::BigInt(_) | Value::Symbol(_)
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
            let exists = crate::core::has_own_property_value(object, &key_val);
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
