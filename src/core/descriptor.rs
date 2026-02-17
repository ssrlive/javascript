use crate::core::{
    ClosureData, DestructuringElement, Gc, JSObjectDataPtr, MutationContext, PropertyKey, Value, new_gc_cell_ptr, new_js_object_data,
    object_get_key_value, object_set_key_value,
};
use crate::{JSError, raise_type_error};

/// A Rust representation of a property descriptor used by the engine.
/// Supports both data descriptors (`value` + `writable`) and accessor descriptors (`get`/`set`).
/// Fields are optional to support "partial" descriptors (as accepted by DefineProperty).
#[derive(Clone, Debug, Default)]
pub struct PropertyDescriptor<'gc> {
    // Data fields
    pub value: Option<Value<'gc>>,
    pub writable: Option<bool>,
    // Accessor fields
    pub get: Option<Value<'gc>>,
    pub set: Option<Value<'gc>>,
    // Common flags
    pub enumerable: Option<bool>,
    pub configurable: Option<bool>,
}

fn compute_function_length(params: &[DestructuringElement]) -> usize {
    let mut fn_length = 0_usize;
    for p in params.iter() {
        match p {
            DestructuringElement::Variable(_, default_opt) => {
                if default_opt.is_some() {
                    break;
                }
                fn_length += 1;
            }
            DestructuringElement::Rest(_) => break,
            DestructuringElement::NestedArray(..) | DestructuringElement::NestedObject(..) => {
                fn_length += 1;
            }
            DestructuringElement::Empty => {}
            _ => {}
        }
    }
    fn_length
}

fn define_function_length<'gc>(mc: &MutationContext<'gc>, func_obj: &JSObjectDataPtr<'gc>, length: usize) -> Result<(), JSError> {
    let desc = create_descriptor_object(mc, &Value::Number(length as f64), false, false, true)?;
    crate::js_object::define_property_internal(mc, func_obj, "length", &desc)?;
    Ok(())
}

impl<'gc> PropertyDescriptor<'gc> {
    /// Construct a full data descriptor from explicit values
    pub fn new_data(value: &Value<'gc>, writable: bool, enumerable: bool, configurable: bool) -> Self {
        PropertyDescriptor {
            value: Some(value.clone()),
            writable: Some(writable),
            get: None,
            set: None,
            enumerable: Some(enumerable),
            configurable: Some(configurable),
        }
    }

    /// Construct an accessor descriptor
    pub fn new_accessor(get: Option<Value<'gc>>, set: Option<Value<'gc>>, enumerable: bool, configurable: bool) -> Self {
        PropertyDescriptor {
            value: None,
            writable: None,
            get,
            set,
            enumerable: Some(enumerable),
            configurable: Some(configurable),
        }
    }

    /// Convert a JS object (descriptor object) into a `PropertyDescriptor`.
    /// If a field is missing or of an unexpected type, the corresponding
    /// `Option` will be `None`.
    pub fn from_object(obj: &JSObjectDataPtr<'gc>) -> Result<Self, JSError> {
        // `object_get_key_value` traverses own+prototype chain; descriptor parsing
        // should read own properties, but inherited values are accepted here.
        let value = object_get_key_value(obj, "value").map(|vptr| vptr.borrow().normalize_slot());

        let writable = object_get_key_value(obj, "writable").map(|wptr| wptr.borrow().normalize_slot().to_truthy());

        let get = object_get_key_value(obj, "get").map(|gptr| gptr.borrow().normalize_slot());

        let set = object_get_key_value(obj, "set").map(|sptr| sptr.borrow().normalize_slot());

        let enumerable = object_get_key_value(obj, "enumerable").map(|eptr| eptr.borrow().normalize_slot().to_truthy());

        let configurable = object_get_key_value(obj, "configurable").map(|cptr| cptr.borrow().normalize_slot().to_truthy());

        Ok(PropertyDescriptor {
            value,
            writable,
            get,
            set,
            enumerable,
            configurable,
        })
    }

    /// Produce a JS object representing this descriptor. Missing fields are
    /// materialized using sensible defaults so the returned object is a complete
    /// descriptor suitable for APIs like `Reflect.getOwnPropertyDescriptor`.
    pub fn to_object(&self, mc: &MutationContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
        let desc = new_js_object_data(mc);
        // If this is an accessor descriptor (get/set present), expose get/set and flags
        if self.get.is_some() || self.set.is_some() {
            if let Some(g) = &self.get {
                object_set_key_value(mc, &desc, "get", g)?;
            }
            if let Some(s) = &self.set {
                object_set_key_value(mc, &desc, "set", s)?;
            }
            object_set_key_value(mc, &desc, "enumerable", &Value::Boolean(self.enumerable.unwrap_or(false)))?;
            object_set_key_value(mc, &desc, "configurable", &Value::Boolean(self.configurable.unwrap_or(false)))?;
            return Ok(desc);
        }

        // Otherwise, data descriptor behavior
        let val = self.value.clone().unwrap_or(Value::Undefined);
        object_set_key_value(mc, &desc, "value", &val)?;
        object_set_key_value(mc, &desc, "writable", &Value::Boolean(self.writable.unwrap_or(false)))?;
        object_set_key_value(mc, &desc, "enumerable", &Value::Boolean(self.enumerable.unwrap_or(false)))?;
        object_set_key_value(mc, &desc, "configurable", &Value::Boolean(self.configurable.unwrap_or(false)))?;
        Ok(desc)
    }
}

/// Create a descriptor object populated with `value`, `writable`, `enumerable`, `configurable` fields.
/// Returned object can be used as a property descriptor (e.g., for reflecting APIs).
pub fn create_descriptor_object<'gc>(
    mc: &MutationContext<'gc>,
    value: &Value<'gc>,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    PropertyDescriptor::new_data(value, writable, enumerable, configurable).to_object(mc)
}

/// Build a `PropertyDescriptor` for an own property on `obj` if present.
/// Converts internal `Value::Property` / Getter / Setter variants into a
/// `PropertyDescriptor` suitable for `to_object`.
pub(crate) fn build_property_descriptor<'gc>(
    mc: &MutationContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: &PropertyKey<'gc>,
) -> Option<PropertyDescriptor<'gc>> {
    if crate::core::get_own_property(obj, key.clone()).is_none()
        && let PropertyKey::String(s) = key
        && !s.starts_with("__")
        && s != "then"
    {
        let (module_path, cache_env) = {
            let b = obj.borrow();
            (b.deferred_module_path.clone(), b.deferred_cache_env)
        };
        if let (Some(module_path), Some(cache_env)) = (module_path, cache_env)
            && let Ok(Value::Object(exports_obj)) = crate::js_module::load_module(mc, module_path.as_str(), None, Some(cache_env))
            && let Some(v) = object_get_key_value(&exports_obj, s)
        {
            let resolved = v.borrow().clone();
            let _ = crate::core::object_set_key_value(mc, obj, s.as_str(), &resolved);
        }
    }

    if let Some(val_rc) = crate::core::get_own_property(obj, key.clone()) {
        let pd = match &*val_rc.borrow() {
            Value::Property { value, getter, setter } => {
                let mut pd = PropertyDescriptor::default();
                let is_accessor_like = getter.is_some() || setter.is_some() || value.is_none();
                if !is_accessor_like && let Some(v) = value {
                    pd.value = Some(v.borrow().clone());
                    pd.writable = Some(obj.borrow().is_writable(key));
                }
                if is_accessor_like {
                    pd.get = Some(Value::Undefined);
                    pd.set = Some(Value::Undefined);
                }
                if let Some(g) = getter {
                    match &*g.clone() {
                        Value::Getter(body, captured_env, _home) => {
                            let func_obj = new_js_object_data(mc);
                            let _ = crate::core::set_internal_prototype_from_constructor(mc, &func_obj, captured_env, "Function");
                            let closure_data = ClosureData {
                                body: body.clone(),
                                env: Some(*captured_env),
                                enforce_strictness_inheritance: true,
                                ..ClosureData::default()
                            };
                            let closure_val = Value::Closure(Gc::new(mc, closure_data));
                            func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, closure_val)));
                            let _ = define_function_length(mc, &func_obj, 0);
                            // Per spec, accessor functions created from object literal should have a name
                            // of the form "get <prop>". Compute property name string for Symbol/String keys.
                            let prop_name = match key {
                                PropertyKey::String(s) => s.clone(),
                                PropertyKey::Symbol(sd) => {
                                    if let Some(desc) = sd.description() {
                                        format!("[{}]", desc)
                                    } else {
                                        String::new()
                                    }
                                }
                                PropertyKey::Private(s, _) => s.clone(),
                            };
                            let full_name = format!("get {}", prop_name);
                            let desc = crate::core::create_descriptor_object(
                                mc,
                                &Value::String(crate::unicode::utf8_to_utf16(&full_name)),
                                false,
                                false,
                                true,
                            )
                            .unwrap();
                            let _ = crate::js_object::define_property_internal(mc, &func_obj, "name", &desc);
                            pd.get = Some(Value::Object(func_obj));
                        }
                        other => {
                            pd.get = Some(other.clone());
                        }
                    }
                }
                if let Some(s) = setter {
                    match &*s.clone() {
                        Value::Setter(params, body, captured_env, _home) => {
                            let func_obj = new_js_object_data(mc);
                            let _ = crate::core::set_internal_prototype_from_constructor(mc, &func_obj, captured_env, "Function");
                            let closure_data = ClosureData {
                                params: params.clone(),
                                body: body.clone(),
                                env: Some(*captured_env),
                                enforce_strictness_inheritance: true,
                                ..ClosureData::default()
                            };
                            let closure_val = Value::Closure(Gc::new(mc, closure_data));
                            func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, closure_val)));
                            let _ = define_function_length(mc, &func_obj, compute_function_length(params));
                            let prop_name = match key {
                                PropertyKey::String(s) => s.clone(),
                                PropertyKey::Symbol(sd) => {
                                    if let Some(desc) = sd.description() {
                                        format!("[{}]", desc)
                                    } else {
                                        String::new()
                                    }
                                }
                                PropertyKey::Private(s, _) => s.clone(),
                            };
                            let full_name = format!("set {}", prop_name);
                            let desc = crate::core::create_descriptor_object(
                                mc,
                                &Value::String(crate::unicode::utf8_to_utf16(&full_name)),
                                false,
                                false,
                                true,
                            )
                            .unwrap();
                            let _ = crate::js_object::define_property_internal(mc, &func_obj, "name", &desc);
                            pd.set = Some(Value::Object(func_obj));
                        }
                        other => {
                            pd.set = Some(other.clone());
                        }
                    }
                }
                pd.enumerable = Some(obj.borrow().is_enumerable(key));
                pd.configurable = Some(obj.borrow().is_configurable(key));
                pd
            }
            Value::Getter(body, captured_env, _home_opt) => {
                let func_obj = new_js_object_data(mc);
                let _ = crate::core::set_internal_prototype_from_constructor(mc, &func_obj, captured_env, "Function");
                let closure_data = ClosureData {
                    body: body.clone(),
                    env: Some(*captured_env),
                    enforce_strictness_inheritance: true,
                    ..ClosureData::default()
                };
                let closure_val = Value::Closure(Gc::new(mc, closure_data));
                func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, closure_val)));
                let _ = define_function_length(mc, &func_obj, 0);

                // Set accessor function name: "get <prop>"
                let prop_name = match key {
                    PropertyKey::String(s) => s.clone(),
                    PropertyKey::Symbol(sd) => {
                        if let Some(desc) = sd.description() {
                            format!("[{}]", desc)
                        } else {
                            String::new()
                        }
                    }
                    PropertyKey::Private(s, _) => s.clone(),
                };
                let full_name = format!("get {}", prop_name);
                let desc = crate::core::create_descriptor_object(
                    mc,
                    &Value::String(crate::unicode::utf8_to_utf16(&full_name)),
                    false,
                    false,
                    true,
                )
                .unwrap();
                let _ = crate::js_object::define_property_internal(mc, &func_obj, "name", &desc);

                PropertyDescriptor::new_accessor(
                    Some(Value::Object(func_obj)),
                    Some(Value::Undefined),
                    obj.borrow().is_enumerable(key),
                    obj.borrow().is_configurable(key),
                )
            }
            Value::Setter(params, body, captured_env, _home_opt) => {
                let func_obj = new_js_object_data(mc);
                let _ = crate::core::set_internal_prototype_from_constructor(mc, &func_obj, captured_env, "Function");
                let closure_data = ClosureData {
                    params: params.clone(),
                    body: body.clone(),
                    env: Some(*captured_env),
                    enforce_strictness_inheritance: true,
                    ..ClosureData::default()
                };
                let closure_val = Value::Closure(Gc::new(mc, closure_data));
                func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, closure_val)));
                let _ = define_function_length(mc, &func_obj, compute_function_length(params));

                // Set accessor function name: "set <prop>"
                let prop_name = match key {
                    PropertyKey::String(s) => s.clone(),
                    PropertyKey::Symbol(sd) => {
                        if let Some(desc) = sd.description() {
                            format!("[{}]", desc)
                        } else {
                            String::new()
                        }
                    }
                    PropertyKey::Private(s, _) => s.clone(),
                };
                let full_name = format!("set {}", prop_name);
                let desc = crate::core::create_descriptor_object(
                    mc,
                    &Value::String(crate::unicode::utf8_to_utf16(&full_name)),
                    false,
                    false,
                    true,
                )
                .unwrap();
                let _ = crate::js_object::define_property_internal(mc, &func_obj, "name", &desc);

                PropertyDescriptor::new_accessor(
                    Some(Value::Undefined),
                    Some(Value::Object(func_obj)),
                    obj.borrow().is_enumerable(key),
                    obj.borrow().is_configurable(key),
                )
            }
            other => PropertyDescriptor::new_data(
                other,
                obj.borrow().is_writable(key),
                obj.borrow().is_enumerable(key),
                obj.borrow().is_configurable(key),
            ),
        };
        Some(pd)
    } else {
        None
    }
}

/// Validate a descriptor for use in DefineProperty/DefineProperties.
/// Ensures it is NOT both a data and an accessor descriptor and that
/// getter/setter values are functions or `undefined`.
pub fn validate_descriptor_for_define<'gc>(_mc: &MutationContext<'gc>, pd: &PropertyDescriptor<'gc>) -> Result<(), JSError> {
    if (pd.get.is_some() || pd.set.is_some()) && (pd.value.is_some() || pd.writable.is_some()) {
        return Err(raise_type_error!(
            "Invalid property descriptor: cannot be both a data and an accessor descriptor"
        ));
    }
    if let Some(get_val) = &pd.get {
        match get_val {
            Value::Undefined => {}
            Value::Closure(_) | Value::AsyncClosure(_) | Value::Function(_) => {}
            Value::Object(obj_ptr) => {
                if obj_ptr.borrow().get_closure().is_none() {
                    return Err(raise_type_error!("Property descriptor getter must be a function or undefined"));
                }
            }
            _ => return Err(raise_type_error!("Property descriptor getter must be a function or undefined")),
        }
    }
    if let Some(set_val) = &pd.set {
        match set_val {
            Value::Undefined => {}
            Value::Closure(_) | Value::AsyncClosure(_) | Value::Function(_) => {}
            Value::Object(obj_ptr) => {
                if obj_ptr.borrow().get_closure().is_none() {
                    return Err(raise_type_error!("Property descriptor setter must be a function or undefined"));
                }
            }
            _ => return Err(raise_type_error!("Property descriptor setter must be a function or undefined")),
        }
    }
    Ok(())
}
