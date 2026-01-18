use crate::core::{JSObjectDataPtr, MutationContext, Value, new_js_object_data, object_get_key_value, object_set_key_value};
use crate::env_set;
use crate::error::JSError;
use crate::unicode::utf8_to_utf16;
use num_bigint::BigInt;

pub fn initialize_boolean<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let boolean_ctor = new_js_object_data(mc);
    object_set_key_value(mc, &boolean_ctor, "__is_constructor", Value::Boolean(true))?;
    object_set_key_value(mc, &boolean_ctor, "__native_ctor", Value::String(utf8_to_utf16("Boolean")))?;

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

    object_set_key_value(mc, &boolean_ctor, "prototype", Value::Object(boolean_proto))?;
    object_set_key_value(mc, &boolean_proto, "constructor", Value::Object(boolean_ctor))?;

    let val = Value::Function("Boolean.prototype.toString".to_string());
    object_set_key_value(mc, &boolean_proto, "toString", val)?;

    let val = Value::Function("Boolean.prototype.valueOf".to_string());
    object_set_key_value(mc, &boolean_proto, "valueOf", val)?;

    env_set(mc, env, "Boolean", Value::Object(boolean_ctor))?;
    Ok(())
}

pub fn to_boolean(val: &Value<'_>) -> bool {
    match val {
        Value::Undefined | Value::Null => false,
        Value::Boolean(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::BigInt(b) => *b != BigInt::from(0),
        Value::Object(_)
        | Value::Function(_)
        | Value::Closure(_)
        | Value::AsyncClosure(_)
        | Value::GeneratorFunction(_, _)
        | Value::ClassDefinition(_)
        | Value::Promise(_)
        | Value::Map(_)
        | Value::Set(_)
        | Value::WeakMap(_)
        | Value::WeakSet(_)
        | Value::Generator(_)
        | Value::Proxy(_)
        | Value::ArrayBuffer(_)
        | Value::DataView(_)
        | Value::TypedArray(_)
        | Value::Symbol(_) => true,
        _ => true,
    }
}

pub fn boolean_constructor<'gc>(args: &[Value<'gc>]) -> Result<Value<'gc>, JSError> {
    if let Some(arg) = args.first() {
        Ok(Value::Boolean(to_boolean(arg)))
    } else {
        Ok(Value::Boolean(false))
    }
}

pub fn handle_boolean_prototype_method<'gc>(this_val: Value<'gc>, method: &str) -> Result<Value<'gc>, JSError> {
    match method {
        "toString" => {
            let b = this_boolean_value(&this_val)?;
            Ok(Value::String(utf8_to_utf16(&b.to_string())))
        }
        "valueOf" => {
            let b = this_boolean_value(&this_val)?;
            Ok(Value::Boolean(b))
        }
        _ => Err(crate::raise_type_error!(format!("Boolean.prototype.{} is not a function", method))),
    }
}

fn this_boolean_value<'gc>(value: &Value<'gc>) -> Result<bool, JSError> {
    match value {
        Value::Boolean(b) => Ok(*b),
        Value::Object(obj) => {
            if let Some(val) = object_get_key_value(obj, "__value__")
                && let Value::Boolean(b) = *val.borrow()
            {
                return Ok(b);
            }
            Err(crate::raise_type_error!("Boolean.prototype method called on incompatible receiver"))
        }
        _ => Err(crate::raise_type_error!("Boolean.prototype method called on incompatible receiver")),
    }
}
