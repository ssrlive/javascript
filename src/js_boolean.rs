use crate::core::{
    EvalError, InternalSlot, JSObjectDataPtr, MutationContext, Value, new_js_object_data, object_get_key_value, object_set_key_value,
    slot_get_chained, slot_set,
};
use crate::env_set;
use crate::error::JSError;
use crate::unicode::utf8_to_utf16;

pub fn initialize_boolean<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let boolean_ctor = new_js_object_data(mc);
    slot_set(mc, &boolean_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(
        mc,
        &boolean_ctor,
        InternalSlot::NativeCtor,
        &Value::String(utf8_to_utf16("Boolean")),
    );

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

    let boolean_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        boolean_proto.borrow_mut(mc).prototype = Some(proto);
    }

    object_set_key_value(mc, &boolean_ctor, "prototype", &Value::Object(boolean_proto))?;
    object_set_key_value(mc, &boolean_proto, "constructor", &Value::Object(boolean_ctor))?;
    boolean_proto.borrow_mut(mc).set_non_enumerable("constructor");

    let val = Value::Function("Boolean.prototype.toString".to_string());
    object_set_key_value(mc, &boolean_proto, "toString", &val)?;
    boolean_proto.borrow_mut(mc).set_non_enumerable("toString");

    let val = Value::Function("Boolean.prototype.valueOf".to_string());
    object_set_key_value(mc, &boolean_proto, "valueOf", &val)?;
    boolean_proto.borrow_mut(mc).set_non_enumerable("valueOf");

    boolean_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    boolean_ctor.borrow_mut(mc).set_non_writable("prototype");
    boolean_ctor.borrow_mut(mc).set_non_configurable("prototype");

    // Boolean.length = 1 (non-writable, non-enumerable, non-configurable)
    object_set_key_value(mc, &boolean_ctor, "length", &Value::Number(1.0))?;
    boolean_ctor.borrow_mut(mc).set_non_enumerable("length");
    boolean_ctor.borrow_mut(mc).set_non_writable("length");

    // Ensure the Boolean constructor object uses Function.prototype as its internal prototype
    // (Function may have already been initialized).
    if let Err(e) = crate::core::set_internal_prototype_from_constructor(mc, &boolean_ctor, env, "Function") {
        log::warn!("Failed to set Boolean constructor's internal prototype from Function: {e:?}");
    }

    env_set(mc, env, "Boolean", &Value::Object(boolean_ctor))?;
    Ok(())
}

pub(crate) fn handle_boolean_constructor<'gc>(
    mc: &MutationContext<'gc>,
    evaluated_args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let bool_val = if evaluated_args.is_empty() {
        false
    } else {
        evaluated_args[0].to_truthy()
    };
    let obj = new_js_object_data(mc);
    slot_set(mc, &obj, InternalSlot::PrimitiveValue, &Value::Boolean(bool_val));
    crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Boolean")?;
    Ok(Value::Object(obj))
}

pub fn boolean_prototype_to_string<'gc>(
    _mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if let Some(this_rc) = crate::core::env_get(env, "this") {
        return handle_boolean_prototype_method(&this_rc.borrow(), "toString");
    }
    Err(crate::raise_eval_error!("Boolean.prototype.toString called without this"))
}

pub fn boolean_prototype_value_of<'gc>(
    _mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if let Some(this_rc) = crate::core::env_get(env, "this") {
        return handle_boolean_prototype_method(&this_rc.borrow(), "valueOf");
    }
    Err(crate::raise_eval_error!("Boolean.prototype.valueOf called without this"))
}

pub fn boolean_constructor<'gc>(args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    if let Some(arg) = args.first() {
        Ok(Value::Boolean(arg.to_truthy()))
    } else {
        Ok(Value::Boolean(false))
    }
}

pub fn handle_boolean_prototype_method<'gc>(this_val: &Value<'gc>, method: &str) -> Result<Value<'gc>, JSError> {
    match method {
        "toString" => {
            let b = this_boolean_value(this_val)?;
            Ok(Value::String(utf8_to_utf16(&b.to_string())))
        }
        "valueOf" => {
            let b = this_boolean_value(this_val)?;
            Ok(Value::Boolean(b))
        }
        _ => Err(crate::raise_type_error!(format!("Boolean.prototype.{} is not a function", method))),
    }
}

fn this_boolean_value<'gc>(value: &Value<'gc>) -> Result<bool, JSError> {
    match value {
        Value::Boolean(b) => Ok(*b),
        Value::Object(obj) => {
            if let Some(val) = slot_get_chained(obj, &InternalSlot::PrimitiveValue)
                && let Value::Boolean(b) = *val.borrow()
            {
                return Ok(b);
            }
            Err(crate::raise_type_error!("Boolean.prototype method called on incompatible receiver"))
        }
        _ => Err(crate::raise_type_error!("Boolean.prototype method called on incompatible receiver")),
    }
}
