use crate::core::EvalError;
use crate::core::WeakKey;
use crate::core::{InternalSlot, MutationContext, slot_get_chained, slot_set};
use crate::{
    core::{JSObjectDataPtr, Value, env_set, new_js_object_data, object_get_key_value, object_set_key_value},
    error::JSError,
    unicode::utf8_to_utf16,
};

/// Handle `new FinalizationRegistry(cleanupCallback)` constructor calls (spec §26.2.1)
pub(crate) fn handle_fr_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    new_target: Option<&Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 2: If cleanupCallback is not callable, throw TypeError.
    let cleanup = args.first().cloned().unwrap_or(Value::Undefined);
    let is_callable = match &cleanup {
        Value::Object(obj) => {
            obj.borrow().get_closure().is_some()
                || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                || slot_get_chained(obj, &InternalSlot::Callable).is_some()
                || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
        }
        Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) => true,
        _ => false,
    };
    if !is_callable {
        return Err(raise_type_error!("FinalizationRegistry: cleanup must be a function").into());
    }

    // Step 3: Create the FR wrapper object.
    let fr_obj = new_js_object_data(mc);

    // Store the cleanup callback
    slot_set(mc, &fr_obj, InternalSlot::FRCleanup, &cleanup);

    // Mark this as a FinalizationRegistry
    slot_set(mc, &fr_obj, InternalSlot::FRMarker, &Value::Boolean(true));

    // Internal cells list — we store entries as an array object whose elements
    // are { target (WeakKey-like), heldValue, unregisterToken? }.
    // Since we don't have first-class GcWeak tracking in the eval loop, we
    // store the registrations and the cleanup callback so that `cleanupSome`
    // can process them.
    let cells_arr = new_js_object_data(mc);
    slot_set(mc, &fr_obj, InternalSlot::FRCells, &Value::Object(cells_arr));

    // OrdinaryCreateFromConstructor(NewTarget, "%FinalizationRegistry.prototype%")
    let mut proto_set = false;
    if let Some(Value::Object(nt_obj)) = new_target
        && let Some(proto) = crate::js_class::get_prototype_from_constructor(mc, nt_obj, env, "FinalizationRegistry")?
    {
        fr_obj.borrow_mut(mc).prototype = Some(proto);
        proto_set = true;
    }
    if !proto_set
        && let Some(fr_ctor) = object_get_key_value(env, "FinalizationRegistry")
        && let Value::Object(ctor) = &*fr_ctor.borrow()
        && let Some(proto) = object_get_key_value(ctor, "prototype")
        && let Value::Object(proto_obj) = &*proto.borrow()
    {
        fr_obj.borrow_mut(mc).prototype = Some(*proto_obj);
    }

    Ok(Value::Object(fr_obj))
}

/// Initialize FinalizationRegistry constructor and prototype (spec §26.2.2, §26.2.3)
pub fn initialize_finalization_registry<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let fr_ctor = new_js_object_data(mc);
    slot_set(mc, &fr_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(
        mc,
        &fr_ctor,
        InternalSlot::NativeCtor,
        &Value::String(utf8_to_utf16("FinalizationRegistry")),
    );

    // FinalizationRegistry.length = 1, .name = "FinalizationRegistry"
    object_set_key_value(mc, &fr_ctor, "length", &Value::Number(1.0))?;
    fr_ctor.borrow_mut(mc).set_non_enumerable("length");
    fr_ctor.borrow_mut(mc).set_non_writable("length");
    object_set_key_value(mc, &fr_ctor, "name", &Value::String(utf8_to_utf16("FinalizationRegistry")))?;
    fr_ctor.borrow_mut(mc).set_non_enumerable("name");
    fr_ctor.borrow_mut(mc).set_non_writable("name");

    // Set [[Prototype]] to Function.prototype
    if let Some(func_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_val.borrow()
        && let Some(func_proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_val.borrow()
    {
        fr_ctor.borrow_mut(mc).prototype = Some(*func_proto);
    }

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

    let fr_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        fr_proto.borrow_mut(mc).prototype = Some(proto);
    }

    object_set_key_value(mc, &fr_ctor, "prototype", &Value::Object(fr_proto))?;
    fr_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    fr_ctor.borrow_mut(mc).set_non_writable("prototype");
    fr_ctor.borrow_mut(mc).set_non_configurable("prototype");
    object_set_key_value(mc, &fr_proto, "constructor", &Value::Object(fr_ctor))?;

    // Register instance methods: register, unregister, toString
    let methods = vec!["register", "unregister", "toString"];
    for method in methods {
        let val = Value::Function(format!("FinalizationRegistry.prototype.{}", method));
        object_set_key_value(mc, &fr_proto, method, &val)?;
        fr_proto.borrow_mut(mc).set_non_enumerable(method);
    }
    // Mark constructor non-enumerable
    fr_proto.borrow_mut(mc).set_non_enumerable("constructor");

    // Symbol.toStringTag = "FinalizationRegistry"
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(s) = &*tag_sym.borrow()
    {
        let tag_desc =
            crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("FinalizationRegistry")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fr_proto, crate::core::PropertyKey::Symbol(*s), &tag_desc)?;
    }

    env_set(mc, env, "FinalizationRegistry", &Value::Object(fr_ctor))?;
    Ok(())
}

/// Handle FinalizationRegistry instance method calls.
pub(crate) fn handle_fr_instance_method<'gc>(
    mc: &MutationContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    method: &str,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Verify that this is indeed a FinalizationRegistry
    if slot_get_chained(obj, &InternalSlot::FRMarker).is_none() {
        return Err(raise_type_error!(format!(
            "Method FinalizationRegistry.prototype.{} called on incompatible receiver",
            method
        ))
        .into());
    }

    match method {
        "register" => {
            // FinalizationRegistry.prototype.register(target, heldValue [, unregisterToken])
            let target = args.first().cloned().unwrap_or(Value::Undefined);
            let held_value = args.get(1).cloned().unwrap_or(Value::Undefined);
            let unregister_token = args.get(2).cloned().unwrap_or(Value::Undefined);

            // Step 3: If target is not an object and not a non-registered symbol, throw TypeError.
            if WeakKey::from_value(&target).is_err() {
                return Err(raise_type_error!("FinalizationRegistry.register: target must be an object or a non-registered symbol").into());
            }

            // Step 4: If SameValue(target, heldValue), throw TypeError.
            if same_value(&target, &held_value) {
                return Err(raise_type_error!("FinalizationRegistry.register: target and heldValue must not be the same").into());
            }

            // Step 5: If unregisterToken is not undefined and not an object/non-registered symbol, throw TypeError.
            if !matches!(unregister_token, Value::Undefined) && WeakKey::from_value(&unregister_token).is_err() {
                return Err(raise_type_error!(
                    "FinalizationRegistry.register: unregisterToken must be an object, a non-registered symbol, or undefined"
                )
                .into());
            }

            // Step 6: Append { target, heldValue, unregisterToken } to [[Cells]].
            // We store each cell as a small object with properties "0"=target, "1"=heldValue, "2"=unregisterToken.
            if let Some(cells_val) = slot_get_chained(obj, &InternalSlot::FRCells)
                && let Value::Object(cells_obj) = &*cells_val.borrow()
            {
                let cell = new_js_object_data(mc);
                object_set_key_value(mc, &cell, "target", &target)?;
                object_set_key_value(mc, &cell, "heldValue", &held_value)?;
                object_set_key_value(mc, &cell, "unregisterToken", &unregister_token)?;

                // Push to the cells array (use a simple count-based scheme)
                let count_val = object_get_key_value(cells_obj, "length");
                let count = if let Some(rc) = count_val {
                    if let Value::Number(n) = &*rc.borrow() { *n as usize } else { 0 }
                } else {
                    0
                };
                object_set_key_value(mc, cells_obj, count, &Value::Object(cell))?;
                object_set_key_value(mc, cells_obj, "length", &Value::Number((count + 1) as f64))?;
            }

            Ok(Value::Undefined)
        }
        "unregister" => {
            // FinalizationRegistry.prototype.unregister(unregisterToken)
            let token = args.first().cloned().unwrap_or(Value::Undefined);

            // Step 3: If token is not an object and not a non-registered symbol, throw TypeError.
            if WeakKey::from_value(&token).is_err() {
                return Err(raise_type_error!(
                    "FinalizationRegistry.unregister: unregisterToken must be an object or a non-registered symbol"
                )
                .into());
            }

            // Step 4: Remove all cells whose unregisterToken matches.
            let mut removed = false;
            if let Some(cells_val) = slot_get_chained(obj, &InternalSlot::FRCells)
                && let Value::Object(cells_obj) = &*cells_val.borrow()
            {
                let count_val = object_get_key_value(cells_obj, "length");
                let count = if let Some(rc) = count_val {
                    if let Value::Number(n) = &*rc.borrow() { *n as usize } else { 0 }
                } else {
                    0
                };

                // Rebuild the array, keeping only cells whose unregisterToken doesn't match
                let mut new_count = 0usize;
                for i in 0..count {
                    let key = i.to_string();
                    if let Some(cell_rc) = object_get_key_value(cells_obj, &key) {
                        let cell_val = cell_rc.borrow().clone();
                        if let Value::Object(cell_obj) = &cell_val {
                            let ut = object_get_key_value(cell_obj, "unregisterToken");
                            let should_remove = if let Some(ut_rc) = ut {
                                let ut_val = ut_rc.borrow().clone();
                                strict_equal(&ut_val, &token)
                            } else {
                                false
                            };
                            if should_remove {
                                removed = true;
                                // Remove by setting to undefined (we'll compact later or leave holes)
                                object_set_key_value(mc, cells_obj, &key, &Value::Undefined)?;
                            } else {
                                if new_count != i {
                                    object_set_key_value(mc, cells_obj, new_count, &cell_val)?;
                                }
                                new_count += 1;
                            }
                        } else {
                            // Skip non-object slots (holes)
                        }
                    }
                }
                if removed {
                    // Compact: update length and remove trailing keys
                    object_set_key_value(mc, cells_obj, "length", &Value::Number(new_count as f64))?;
                    for i in new_count..count {
                        // Remove old trailing keys by setting to Undefined
                        let _ = object_set_key_value(mc, cells_obj, i, &Value::Undefined);
                    }
                }
            }

            Ok(Value::Boolean(removed))
        }
        "toString" => Ok(Value::String(utf8_to_utf16("[object FinalizationRegistry]"))),
        _ => Err(raise_type_error!(format!("FinalizationRegistry.prototype.{} is not a function", method)).into()),
    }
}

/// SameValue comparison (spec §7.2.11)
fn same_value<'gc>(x: &Value<'gc>, y: &Value<'gc>) -> bool {
    match (x, y) {
        (Value::Number(a), Value::Number(b)) => {
            if a.is_nan() && b.is_nan() {
                return true;
            }
            if *a == 0.0 && *b == 0.0 {
                // +0 and -0 are different
                return a.is_sign_positive() == b.is_sign_positive();
            }
            a == b
        }
        (Value::Object(a), Value::Object(b)) => std::ptr::eq(&*a.borrow() as *const _, &*b.borrow() as *const _),
        (Value::Symbol(a), Value::Symbol(b)) => std::ptr::eq(&**a as *const _, &**b as *const _),
        _ => strict_equal(x, y),
    }
}

/// Strict equality (===) for token comparison
fn strict_equal<'gc>(a: &Value<'gc>, b: &Value<'gc>) -> bool {
    match (a, b) {
        (Value::Undefined, Value::Undefined) => true,
        (Value::Null, Value::Null) => true,
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::Boolean(x), Value::Boolean(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Object(x), Value::Object(y)) => std::ptr::eq(&*x.borrow() as *const _, &*y.borrow() as *const _),
        (Value::Symbol(x), Value::Symbol(y)) => std::ptr::eq(&**x as *const _, &**y as *const _),
        _ => false,
    }
}
