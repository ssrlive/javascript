use crate::core::{
    GcContext, InternalSlot, JSObjectDataPtr, PropertyKey, Value, create_descriptor_object, new_js_object_data, object_get_key_value,
    object_set_key_value, slot_set,
};
use crate::error::JSError;
use crate::unicode::utf8_to_utf16;

const ABSTRACT_MODULE_SOURCE_CTOR_SLOT: &str = "__abstract_module_source_ctor";

fn lookup_abstract_module_source_ctor<'gc>(env: &JSObjectDataPtr<'gc>) -> Option<JSObjectDataPtr<'gc>> {
    let mut cur = Some(*env);
    while let Some(scope) = cur {
        if let Some(v) = object_get_key_value(&scope, ABSTRACT_MODULE_SOURCE_CTOR_SLOT)
            && let Value::Object(ctor) = &*v.borrow()
        {
            return Some(*ctor);
        }
        cur = scope.borrow().prototype;
    }
    None
}

pub fn initialize_abstract_module_source<'gc>(ctx: &GcContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let ctor = new_js_object_data(ctx);
    slot_set(ctx, &ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(
        ctx,
        &ctor,
        InternalSlot::NativeCtor,
        &Value::String(utf8_to_utf16("AbstractModuleSource")),
    );

    let _ = crate::core::set_internal_prototype_from_constructor(ctx, &ctor, env, "Function");

    let proto = new_js_object_data(ctx);
    let _ = crate::core::set_internal_prototype_from_constructor(ctx, &proto, env, "Object");

    let ctor_on_proto_desc = create_descriptor_object(ctx, &Value::Object(ctor), true, true, true)?;
    crate::js_object::define_property_internal(ctx, &proto, "constructor", &ctor_on_proto_desc)?;
    proto.borrow_mut(ctx).set_non_enumerable("constructor");

    if let Some(sym_ctor_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_ctor_val.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_ctor, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let to_string_tag_accessor = Value::Property {
            value: None,
            getter: Some(Box::new(Value::Function(
                "AbstractModuleSource.prototype.@@toStringTag".to_string(),
            ))),
            setter: None,
        };
        let key = PropertyKey::Symbol(*tag_sym);
        object_set_key_value(ctx, &proto, &key, &to_string_tag_accessor)?;
        proto.borrow_mut(ctx).set_non_enumerable(key);
    }

    let len_desc = create_descriptor_object(ctx, &Value::Number(0.0), false, false, true)?;
    crate::js_object::define_property_internal(ctx, &ctor, "length", &len_desc)?;

    let name_desc = create_descriptor_object(ctx, &Value::String(utf8_to_utf16("AbstractModuleSource")), false, false, true)?;
    crate::js_object::define_property_internal(ctx, &ctor, "name", &name_desc)?;

    let proto_desc = create_descriptor_object(ctx, &Value::Object(proto), false, false, false)?;
    crate::js_object::define_property_internal(ctx, &ctor, "prototype", &proto_desc)?;

    object_set_key_value(ctx, env, ABSTRACT_MODULE_SOURCE_CTOR_SLOT, &Value::Object(ctor))?;
    env.borrow_mut(ctx).set_non_enumerable(ABSTRACT_MODULE_SOURCE_CTOR_SLOT);

    Ok(())
}

pub fn create_module_source_placeholder<'gc>(ctx: &GcContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    let obj = new_js_object_data(ctx);

    if let Some(ctor_obj) = lookup_abstract_module_source_ctor(env)
        && let Some(proto_val) = object_get_key_value(&ctor_obj, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        obj.borrow_mut(ctx).prototype = Some(*proto_obj);
    } else {
        let _ = crate::core::set_internal_prototype_from_constructor(ctx, &obj, env, "Object");
    }

    object_set_key_value(
        ctx,
        &obj,
        "__module_source_class_name",
        &Value::String(utf8_to_utf16("ModuleSource")),
    )?;

    Ok(Value::Object(obj))
}
