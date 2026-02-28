/// DisposableStack and AsyncDisposableStack built-in classes
/// Implements the TC39 Explicit Resource Management proposal.
use crate::core::{
    EvalError, InternalSlot, JSObjectDataPtr, MutationContext, Value, env_get, new_gc_cell_ptr, new_js_object_data, object_get_key_value,
    object_get_length, object_set_key_value, object_set_length, slot_get, slot_get_chained, slot_set,
};
use crate::unicode::utf8_to_utf16;
use crate::{JSError, PropertyKey};

/// Create a native built-in function object with the given dispatch name,
/// expected argument count (`length`) and display `name`.
fn create_builtin_method<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    dispatch_name: &str,
    length: usize,
    display_name: &str,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let fn_obj = new_js_object_data(mc);
    fn_obj
        .borrow_mut(mc)
        .set_closure(Some(new_gc_cell_ptr(mc, Value::Function(dispatch_name.to_string()))));
    slot_set(mc, &fn_obj, InternalSlot::Callable, &Value::Boolean(true));
    object_set_key_value(mc, &fn_obj, "length", &Value::Number(length as f64))?;
    fn_obj.borrow_mut(mc).set_non_enumerable("length");
    fn_obj.borrow_mut(mc).set_non_writable("length");
    object_set_key_value(mc, &fn_obj, "name", &Value::String(utf8_to_utf16(display_name)))?;
    fn_obj.borrow_mut(mc).set_non_enumerable("name");
    fn_obj.borrow_mut(mc).set_non_writable("name");

    // Set __proto__ to Function.prototype
    if let Some(func_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_val.borrow()
        && let Some(func_proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_val.borrow()
    {
        fn_obj.borrow_mut(mc).prototype = Some(*func_proto);
    }

    Ok(fn_obj)
}

// =========================================================================
// DisposableStack
// =========================================================================

pub fn initialize_disposable_stack<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let ctor = new_js_object_data(mc);
    slot_set(mc, &ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(
        mc,
        &ctor,
        InternalSlot::NativeCtor,
        &Value::String(utf8_to_utf16("DisposableStack")),
    );

    object_set_key_value(mc, &ctor, "length", &Value::Number(0.0))?;
    ctor.borrow_mut(mc).set_non_enumerable("length");
    ctor.borrow_mut(mc).set_non_writable("length");
    object_set_key_value(mc, &ctor, "name", &Value::String(utf8_to_utf16("DisposableStack")))?;
    ctor.borrow_mut(mc).set_non_enumerable("name");
    ctor.borrow_mut(mc).set_non_writable("name");

    if let Some(func_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_val.borrow()
        && let Some(func_proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_val.borrow()
    {
        ctor.borrow_mut(mc).prototype = Some(*func_proto);
    }

    let object_proto = if let Some(obj_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else {
        None
    };

    let proto = new_js_object_data(mc);
    if let Some(op) = object_proto {
        proto.borrow_mut(mc).prototype = Some(op);
    }

    object_set_key_value(mc, &ctor, "prototype", &Value::Object(proto))?;
    ctor.borrow_mut(mc).set_non_enumerable("prototype");
    ctor.borrow_mut(mc).set_non_writable("prototype");
    ctor.borrow_mut(mc).set_non_configurable("prototype");
    object_set_key_value(mc, &proto, "constructor", &Value::Object(ctor))?;
    proto.borrow_mut(mc).set_non_enumerable("constructor");

    let mut dispose_fn_obj = None;
    for &(method, arity) in &[("use", 1), ("adopt", 2), ("defer", 1), ("dispose", 0), ("move", 0)] {
        let fn_name = format!("DisposableStack.prototype.{}", method);
        let fn_obj = create_builtin_method(mc, env, &fn_name, arity, method)?;
        if method == "dispose" {
            dispose_fn_obj = Some(fn_obj);
        }
        object_set_key_value(mc, &proto, method, &Value::Object(fn_obj))?;
        proto.borrow_mut(mc).set_non_enumerable(method);
    }

    // `disposed` getter — needs to be a function object with name "get disposed"
    let disposed_getter = create_builtin_method(mc, env, "DisposableStack.prototype.disposed", 0, "get disposed")?;
    let disposed_prop = Value::Property {
        value: None,
        getter: Some(Box::new(Value::Object(disposed_getter))),
        setter: None,
    };
    object_set_key_value(mc, &proto, "disposed", &disposed_prop)?;
    proto.borrow_mut(mc).set_non_enumerable("disposed");

    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        if let Some(dispose_sym) = object_get_key_value(sym_obj, "dispose")
            && let Value::Symbol(s) = &*dispose_sym.borrow()
        {
            // Per spec, @@dispose must be the SAME function object as .dispose
            let dispose_val = if let Some(dfn) = dispose_fn_obj {
                Value::Object(dfn)
            } else {
                Value::Function("DisposableStack.prototype.dispose".into())
            };
            let desc = crate::core::create_descriptor_object(mc, &dispose_val, true, false, true)?;
            crate::js_object::define_property_internal(mc, &proto, PropertyKey::Symbol(*s), &desc)?;
        }
        if let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
            && let Value::Symbol(s) = &*tag_sym.borrow()
        {
            let desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("DisposableStack")), false, false, true)?;
            crate::js_object::define_property_internal(mc, &proto, PropertyKey::Symbol(*s), &desc)?;
        }
    }

    object_set_key_value(mc, env, "DisposableStack", &Value::Object(ctor))?;
    Ok(())
}

pub fn handle_disposable_stack_constructor<'gc>(
    mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    new_target: Option<&Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if new_target.is_none() {
        return Err(crate::raise_type_error!("Constructor DisposableStack requires 'new'").into());
    }
    let instance = new_js_object_data(mc);
    slot_set(mc, &instance, InternalSlot::Kind, &Value::String(utf8_to_utf16("pending")));
    slot_set(mc, &instance, InternalSlot::DisposableType, &Value::String(utf8_to_utf16("sync")));
    let resource_list = new_js_object_data(mc);
    object_set_length(mc, &resource_list, 0)?;
    slot_set(mc, &instance, InternalSlot::DisposableResources, &Value::Object(resource_list));

    if let Some(ctor_val) = env_get(env, "DisposableStack")
        && let Value::Object(ctor_obj) = &*ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(ctor_obj, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        instance.borrow_mut(mc).prototype = Some(*proto);
    }
    if let Some(Value::Object(nt_obj)) = new_target
        && let Some(proto) = crate::js_class::get_prototype_from_constructor(mc, nt_obj, env, "DisposableStack")?
    {
        instance.borrow_mut(mc).prototype = Some(proto);
    }
    Ok(Value::Object(instance))
}

pub fn handle_disposable_stack_method<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "use" => ds_use(mc, this_val, args, env),
        "adopt" => ds_adopt(mc, this_val, args, env),
        "defer" => ds_defer(mc, this_val, args, env),
        "dispose" => ds_dispose(mc, this_val, env),
        "move" => ds_move(mc, this_val, env),
        "disposed" => ds_disposed(this_val, "sync"),
        _ => Err(crate::raise_type_error!(format!("Unknown DisposableStack method: {}", method)).into()),
    }
}

// =========================================================================
// AsyncDisposableStack
// =========================================================================

pub fn initialize_async_disposable_stack<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let ctor = new_js_object_data(mc);
    slot_set(mc, &ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(
        mc,
        &ctor,
        InternalSlot::NativeCtor,
        &Value::String(utf8_to_utf16("AsyncDisposableStack")),
    );

    object_set_key_value(mc, &ctor, "length", &Value::Number(0.0))?;
    ctor.borrow_mut(mc).set_non_enumerable("length");
    ctor.borrow_mut(mc).set_non_writable("length");
    // length is configurable per spec
    object_set_key_value(mc, &ctor, "name", &Value::String(utf8_to_utf16("AsyncDisposableStack")))?;
    ctor.borrow_mut(mc).set_non_enumerable("name");
    ctor.borrow_mut(mc).set_non_writable("name");
    // name is configurable per spec

    if let Some(func_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_val.borrow()
        && let Some(func_proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_val.borrow()
    {
        ctor.borrow_mut(mc).prototype = Some(*func_proto);
    }

    let object_proto = if let Some(obj_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else {
        None
    };

    let proto = new_js_object_data(mc);
    if let Some(op) = object_proto {
        proto.borrow_mut(mc).prototype = Some(op);
    }

    object_set_key_value(mc, &ctor, "prototype", &Value::Object(proto))?;
    ctor.borrow_mut(mc).set_non_enumerable("prototype");
    ctor.borrow_mut(mc).set_non_writable("prototype");
    ctor.borrow_mut(mc).set_non_configurable("prototype");
    object_set_key_value(mc, &proto, "constructor", &Value::Object(ctor))?;
    proto.borrow_mut(mc).set_non_enumerable("constructor");

    let mut dispose_async_fn_obj = None;
    for &(method, arity) in &[("use", 1), ("adopt", 2), ("defer", 1), ("disposeAsync", 0), ("move", 0)] {
        let fn_name = format!("AsyncDisposableStack.prototype.{}", method);
        let fn_obj = create_builtin_method(mc, env, &fn_name, arity, method)?;
        if method == "disposeAsync" {
            dispose_async_fn_obj = Some(fn_obj);
        }
        object_set_key_value(mc, &proto, method, &Value::Object(fn_obj))?;
        proto.borrow_mut(mc).set_non_enumerable(method);
    }

    // `disposed` accessor — getter must be a proper function with name "get disposed"
    let disposed_getter = create_builtin_method(mc, env, "AsyncDisposableStack.prototype.disposed", 0, "get disposed")?;
    let disposed_prop = Value::Property {
        value: None,
        getter: Some(Box::new(Value::Object(disposed_getter))),
        setter: None,
    };
    object_set_key_value(mc, &proto, "disposed", &disposed_prop)?;
    proto.borrow_mut(mc).set_non_enumerable("disposed");

    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        if let Some(dispose_sym) = object_get_key_value(sym_obj, "asyncDispose")
            && let Value::Symbol(s) = &*dispose_sym.borrow()
            && let Some(da_fn) = dispose_async_fn_obj
        {
            let desc = crate::core::create_descriptor_object(mc, &Value::Object(da_fn), true, false, true)?;
            crate::js_object::define_property_internal(mc, &proto, PropertyKey::Symbol(*s), &desc)?;
        }
        if let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
            && let Value::Symbol(s) = &*tag_sym.borrow()
        {
            let desc =
                crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("AsyncDisposableStack")), false, false, true)?;
            crate::js_object::define_property_internal(mc, &proto, PropertyKey::Symbol(*s), &desc)?;
        }
    }

    object_set_key_value(mc, env, "AsyncDisposableStack", &Value::Object(ctor))?;
    Ok(())
}

pub fn handle_async_disposable_stack_constructor<'gc>(
    mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    new_target: Option<&Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if new_target.is_none() {
        return Err(crate::raise_type_error!("Constructor AsyncDisposableStack requires 'new'").into());
    }
    let instance = new_js_object_data(mc);
    slot_set(mc, &instance, InternalSlot::Kind, &Value::String(utf8_to_utf16("pending")));
    slot_set(mc, &instance, InternalSlot::DisposableType, &Value::String(utf8_to_utf16("async")));
    let resource_list = new_js_object_data(mc);
    object_set_length(mc, &resource_list, 0)?;
    slot_set(mc, &instance, InternalSlot::DisposableResources, &Value::Object(resource_list));

    if let Some(ctor_val) = env_get(env, "AsyncDisposableStack")
        && let Value::Object(ctor_obj) = &*ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(ctor_obj, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        instance.borrow_mut(mc).prototype = Some(*proto);
    }
    if let Some(Value::Object(nt_obj)) = new_target
        && let Some(proto) = crate::js_class::get_prototype_from_constructor(mc, nt_obj, env, "AsyncDisposableStack")?
    {
        instance.borrow_mut(mc).prototype = Some(proto);
    }
    Ok(Value::Object(instance))
}

pub fn handle_async_disposable_stack_method<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "use" => ads_use(mc, this_val, args, env),
        "adopt" => ads_adopt(mc, this_val, args, env),
        "defer" => ads_defer(mc, this_val, args, env),
        "disposeAsync" => ads_dispose_async(mc, this_val, env),
        "move" => ads_move(mc, this_val, env),
        "disposed" => ds_disposed(this_val, "async"),
        _ => Err(crate::raise_type_error!(format!("Unknown AsyncDisposableStack method: {}", method)).into()),
    }
}

// =========================================================================
// DisposableStack method implementations
// =========================================================================

fn ds_use<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = require_internal(this_val, "sync")?;
    require_not_disposed(&obj)?;
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    if matches!(value, Value::Null | Value::Undefined) {
        return Ok(value);
    }
    if !matches!(value, Value::Object(_)) {
        return Err(crate::raise_type_error!("DisposableStack.prototype.use: value must be an object, null, or undefined").into());
    }
    let method = get_symbol_dispose_method(mc, env, &value)?;
    let list = resource_list(&obj)?;
    let entry = new_js_object_data(mc);
    object_set_key_value(mc, &entry, "__kind", &Value::String(utf8_to_utf16("use")))?;
    slot_set(mc, &entry, InternalSlot::PrimitiveValue, &value);
    object_set_key_value(mc, &entry, "__dispose_method", &method)?;
    append_entry(mc, &list, &entry)?;
    Ok(value)
}

fn ds_adopt<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = require_internal(this_val, "sync")?;
    require_not_disposed(&obj)?;
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let on_dispose = args.get(1).cloned().unwrap_or(Value::Undefined);
    if !is_callable(&on_dispose) {
        return Err(crate::raise_type_error!("DisposableStack.prototype.adopt: onDispose is not callable").into());
    }
    let list = resource_list(&obj)?;
    let entry = new_js_object_data(mc);
    object_set_key_value(mc, &entry, "__kind", &Value::String(utf8_to_utf16("adopt")))?;
    slot_set(mc, &entry, InternalSlot::PrimitiveValue, &value);
    object_set_key_value(mc, &entry, "__callback", &on_dispose)?;
    append_entry(mc, &list, &entry)?;
    Ok(value)
}

fn ds_defer<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = require_internal(this_val, "sync")?;
    require_not_disposed(&obj)?;
    let on_dispose = args.first().cloned().unwrap_or(Value::Undefined);
    if !is_callable(&on_dispose) {
        return Err(crate::raise_type_error!("DisposableStack.prototype.defer: onDispose is not callable").into());
    }
    let list = resource_list(&obj)?;
    let entry = new_js_object_data(mc);
    object_set_key_value(mc, &entry, "__kind", &Value::String(utf8_to_utf16("defer")))?;
    object_set_key_value(mc, &entry, "__callback", &on_dispose)?;
    append_entry(mc, &list, &entry)?;
    Ok(Value::Undefined)
}

fn ds_dispose<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = require_internal(this_val, "sync")?;
    if is_disposed(&obj) {
        return Ok(Value::Undefined);
    }
    slot_set(mc, &obj, InternalSlot::Kind, &Value::String(utf8_to_utf16("disposed")));
    dispose_list(mc, &obj, env, false)?;
    Ok(Value::Undefined)
}

fn ds_move<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = require_internal(this_val, "sync")?;
    require_not_disposed(&obj)?;

    let new_inst = new_js_object_data(mc);
    slot_set(mc, &new_inst, InternalSlot::Kind, &Value::String(utf8_to_utf16("pending")));
    slot_set(mc, &new_inst, InternalSlot::DisposableType, &Value::String(utf8_to_utf16("sync")));

    // Steal the resource list
    if let Some(cell) = slot_get(&obj, &InternalSlot::DisposableResources) {
        slot_set(mc, &new_inst, InternalSlot::DisposableResources, &cell.borrow().clone());
    }
    let empty = new_js_object_data(mc);
    object_set_length(mc, &empty, 0)?;
    slot_set(mc, &obj, InternalSlot::DisposableResources, &Value::Object(empty));
    slot_set(mc, &obj, InternalSlot::Kind, &Value::String(utf8_to_utf16("disposed")));

    if let Some(ctor_val) = env_get(env, "DisposableStack")
        && let Value::Object(ctor_obj) = &*ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(ctor_obj, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        new_inst.borrow_mut(mc).prototype = Some(*proto);
    }
    Ok(Value::Object(new_inst))
}

fn ds_disposed<'gc>(this_val: Option<&Value<'gc>>, expected_type: &str) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = require_internal(this_val, expected_type)?;
    Ok(Value::Boolean(is_disposed(&obj)))
}

// =========================================================================
// AsyncDisposableStack method implementations
// =========================================================================

fn ads_use<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = require_internal(this_val, "async")?;
    require_not_disposed(&obj)?;
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    if matches!(value, Value::Null | Value::Undefined) {
        return Ok(value);
    }
    if !matches!(value, Value::Object(_)) {
        return Err(crate::raise_type_error!("AsyncDisposableStack.prototype.use: value must be an object, null, or undefined").into());
    }
    let method = get_symbol_async_dispose_or_dispose_method(mc, env, &value)?;
    let list = resource_list(&obj)?;
    let entry = new_js_object_data(mc);
    object_set_key_value(mc, &entry, "__kind", &Value::String(utf8_to_utf16("use")))?;
    object_set_key_value(mc, &entry, "__is_async", &Value::Boolean(true))?;
    slot_set(mc, &entry, InternalSlot::PrimitiveValue, &value);
    object_set_key_value(mc, &entry, "__dispose_method", &method)?;
    append_entry(mc, &list, &entry)?;
    Ok(value)
}

fn ads_adopt<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = require_internal(this_val, "async")?;
    require_not_disposed(&obj)?;
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let on_dispose = args.get(1).cloned().unwrap_or(Value::Undefined);
    if !is_callable(&on_dispose) {
        return Err(crate::raise_type_error!("AsyncDisposableStack.prototype.adopt: onDispose is not callable").into());
    }
    let list = resource_list(&obj)?;
    let entry = new_js_object_data(mc);
    object_set_key_value(mc, &entry, "__kind", &Value::String(utf8_to_utf16("adopt")))?;
    object_set_key_value(mc, &entry, "__is_async", &Value::Boolean(true))?;
    slot_set(mc, &entry, InternalSlot::PrimitiveValue, &value);
    object_set_key_value(mc, &entry, "__callback", &on_dispose)?;
    append_entry(mc, &list, &entry)?;
    Ok(value)
}

fn ads_defer<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = require_internal(this_val, "async")?;
    require_not_disposed(&obj)?;
    let on_dispose = args.first().cloned().unwrap_or(Value::Undefined);
    if !is_callable(&on_dispose) {
        return Err(crate::raise_type_error!("AsyncDisposableStack.prototype.defer: onDispose is not callable").into());
    }
    let list = resource_list(&obj)?;
    let entry = new_js_object_data(mc);
    object_set_key_value(mc, &entry, "__kind", &Value::String(utf8_to_utf16("defer")))?;
    object_set_key_value(mc, &entry, "__is_async", &Value::Boolean(true))?;
    object_set_key_value(mc, &entry, "__callback", &on_dispose)?;
    append_entry(mc, &list, &entry)?;
    Ok(Value::Undefined)
}

fn ads_dispose_async<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = require_internal(this_val, "async")?;
    if is_disposed(&obj) {
        return create_resolved_promise(mc, env, &Value::Undefined);
    }
    slot_set(mc, &obj, InternalSlot::Kind, &Value::String(utf8_to_utf16("disposed")));
    match dispose_list(mc, &obj, env, true) {
        Ok(()) => create_resolved_promise(mc, env, &Value::Undefined),
        Err(EvalError::Throw(val, _, _)) => create_rejected_promise(mc, env, &val),
        Err(other) => Err(other),
    }
}

fn ads_move<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<&Value<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let obj = require_internal(this_val, "async")?;
    require_not_disposed(&obj)?;

    let new_inst = new_js_object_data(mc);
    slot_set(mc, &new_inst, InternalSlot::Kind, &Value::String(utf8_to_utf16("pending")));
    slot_set(mc, &new_inst, InternalSlot::DisposableType, &Value::String(utf8_to_utf16("async")));
    if let Some(cell) = slot_get(&obj, &InternalSlot::DisposableResources) {
        slot_set(mc, &new_inst, InternalSlot::DisposableResources, &cell.borrow().clone());
    }
    let empty = new_js_object_data(mc);
    object_set_length(mc, &empty, 0)?;
    slot_set(mc, &obj, InternalSlot::DisposableResources, &Value::Object(empty));
    slot_set(mc, &obj, InternalSlot::Kind, &Value::String(utf8_to_utf16("disposed")));

    if let Some(ctor_val) = env_get(env, "AsyncDisposableStack")
        && let Value::Object(ctor_obj) = &*ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(ctor_obj, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        new_inst.borrow_mut(mc).prototype = Some(*proto);
    }
    Ok(Value::Object(new_inst))
}

// =========================================================================
// Internal helpers
// =========================================================================

fn require_internal<'gc>(this_val: Option<&Value<'gc>>, expected_type: &str) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    let obj = match this_val {
        Some(Value::Object(o)) => *o,
        _ => return Err(crate::raise_type_error!("Method called on incompatible receiver").into()),
    };
    if slot_get(&obj, &InternalSlot::Kind).is_none() {
        return Err(crate::raise_type_error!("Method called on an object that does not have [[DisposableState]] internal slot").into());
    }
    // Distinguish sync vs async
    if let Some(cell) = slot_get(&obj, &InternalSlot::DisposableType) {
        if let Value::String(s) = &*cell.borrow() {
            let t = crate::unicode::utf16_to_utf8(s);
            if t != expected_type {
                return Err(
                    crate::raise_type_error!("Method called on an object that does not have [[DisposableState]] internal slot").into(),
                );
            }
        }
    } else {
        return Err(crate::raise_type_error!("Method called on an object that does not have [[DisposableState]] internal slot").into());
    }
    Ok(obj)
}

fn require_not_disposed<'gc>(obj: &JSObjectDataPtr<'gc>) -> Result<(), EvalError<'gc>> {
    if is_disposed(obj) {
        return Err(crate::raise_reference_error!("DisposableStack has already been disposed").into());
    }
    Ok(())
}

fn is_disposed(obj: &JSObjectDataPtr) -> bool {
    if let Some(cell) = slot_get(obj, &InternalSlot::Kind)
        && let Value::String(s) = &*cell.borrow()
    {
        return crate::unicode::utf16_to_utf8(s) == "disposed";
    }
    false
}

fn resource_list<'gc>(obj: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    if let Some(cell) = slot_get(obj, &InternalSlot::DisposableResources)
        && let Value::Object(arr) = &*cell.borrow()
    {
        return Ok(*arr);
    }
    Err(crate::raise_type_error!("DisposableStack internal error: missing resource list").into())
}

fn append_entry<'gc>(mc: &MutationContext<'gc>, list: &JSObjectDataPtr<'gc>, entry: &JSObjectDataPtr<'gc>) -> Result<(), EvalError<'gc>> {
    let len = object_get_length(list).unwrap_or(0);
    object_set_key_value(mc, list, len, &Value::Object(*entry))?;
    object_set_length(mc, list, len + 1)?;
    Ok(())
}

/// Dispose all entries in the resource list in reverse order.
fn dispose_list<'gc>(
    mc: &MutationContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
    try_async: bool,
) -> Result<(), EvalError<'gc>> {
    let list = match slot_get(obj, &InternalSlot::DisposableResources) {
        Some(cell) => match &*cell.borrow() {
            Value::Object(a) => *a,
            _ => return Ok(()),
        },
        None => return Ok(()),
    };

    let len = object_get_length(&list).unwrap_or(0);
    let mut completion_error: Option<Value<'gc>> = None;

    for i in (0..len).rev() {
        let entry = match object_get_key_value(&list, i) {
            Some(cell) => match &*cell.borrow() {
                Value::Object(e) => *e,
                _ => continue,
            },
            None => continue,
        };

        let kind = entry_kind(&entry);
        let is_async_entry = object_get_key_value(&entry, "__is_async")
            .map(|c| matches!(&*c.borrow(), Value::Boolean(true)))
            .unwrap_or(false);

        let result = match kind.as_str() {
            "use" => {
                let resource = slot_get(&entry, &InternalSlot::PrimitiveValue)
                    .map(|c| c.borrow().clone())
                    .unwrap_or(Value::Undefined);
                // Use cached dispose method if available (stored at use() time)
                if let Some(method_cell) = object_get_key_value(&entry, "__dispose_method") {
                    let method = method_cell.borrow().clone();
                    crate::core::evaluate_call_dispatch(mc, env, &method, Some(&resource), &[])
                } else {
                    call_dispose_on_value(mc, env, &resource, try_async || is_async_entry)
                }
            }
            "adopt" => {
                let resource = slot_get(&entry, &InternalSlot::PrimitiveValue)
                    .map(|c| c.borrow().clone())
                    .unwrap_or(Value::Undefined);
                let cb = object_get_key_value(&entry, "__callback")
                    .map(|c| c.borrow().clone())
                    .unwrap_or(Value::Undefined);
                crate::core::evaluate_call_dispatch(mc, env, &cb, None, &[resource])
            }
            "defer" => {
                let cb = object_get_key_value(&entry, "__callback")
                    .map(|c| c.borrow().clone())
                    .unwrap_or(Value::Undefined);
                crate::core::evaluate_call_dispatch(mc, env, &cb, None, &[])
            }
            _ => continue,
        };

        if let Err(e) = result {
            let new_err = match e {
                EvalError::Throw(v, _, _) => v,
                other => Value::String(utf8_to_utf16(&format!("{:?}", other))),
            };
            completion_error = Some(match completion_error.take() {
                Some(prev) => crate::core::create_suppressed_error_value(mc, env, &new_err, &prev),
                None => new_err,
            });
        }
    }

    // Clear
    let empty = new_js_object_data(mc);
    object_set_length(mc, &empty, 0)?;
    slot_set(mc, obj, InternalSlot::DisposableResources, &Value::Object(empty));

    if let Some(err) = completion_error {
        return Err(EvalError::Throw(err, None, None));
    }
    Ok(())
}

fn entry_kind(entry: &JSObjectDataPtr) -> String {
    object_get_key_value(entry, "__kind")
        .map(|c| {
            if let Value::String(s) = &*c.borrow() {
                crate::unicode::utf16_to_utf8(s)
            } else {
                String::new()
            }
        })
        .unwrap_or_default()
}

fn is_callable(val: &Value) -> bool {
    match val {
        Value::Closure(_)
        | Value::AsyncClosure(_)
        | Value::GeneratorFunction(_, _)
        | Value::AsyncGeneratorFunction(_, _)
        | Value::Function(_) => true,
        Value::Object(obj) => {
            obj.borrow().get_closure().is_some()
                || slot_get_chained(obj, &InternalSlot::Callable).is_some()
                || slot_get_chained(obj, &InternalSlot::Function).is_some()
                || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
        }
        _ => false,
    }
}

fn get_symbol_dispose_method<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    value: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let sym_obj = get_symbol_obj(env)?;
    let dispose_sym = object_get_key_value(&sym_obj, "dispose")
        .ok_or_else(|| -> EvalError<'gc> { crate::raise_type_error!("Symbol.dispose not found").into() })?;
    if let Value::Symbol(sym_data) = &*dispose_sym.borrow() {
        let method = lookup_symbol_on_value(mc, env, value, sym_data)?;
        if matches!(method, Value::Undefined | Value::Null) {
            return Err(crate::raise_type_error!("The value does not have a callable [Symbol.dispose]() method").into());
        }
        if !is_callable(&method) {
            return Err(crate::raise_type_error!("The [Symbol.dispose] property is not a function").into());
        }
        Ok(method)
    } else {
        Err(crate::raise_type_error!("Symbol.dispose is not a symbol").into())
    }
}

fn get_symbol_async_dispose_or_dispose_method<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    value: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let sym_obj = get_symbol_obj(env)?;
    if let Some(async_cell) = object_get_key_value(&sym_obj, "asyncDispose")
        && let Value::Symbol(sym_data) = &*async_cell.borrow()
    {
        let method = lookup_symbol_on_value(mc, env, value, sym_data)?;
        if !matches!(method, Value::Undefined | Value::Null) && is_callable(&method) {
            return Ok(method);
        }
    }
    get_symbol_dispose_method(mc, env, value)
}

fn call_dispose_on_value<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    value: &Value<'gc>,
    try_async: bool,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if matches!(value, Value::Null | Value::Undefined) {
        return Ok(Value::Undefined);
    }
    let method = if try_async {
        get_symbol_async_dispose_or_dispose_method(mc, env, value)?
    } else {
        get_symbol_dispose_method(mc, env, value)?
    };
    crate::core::evaluate_call_dispatch(mc, env, &method, Some(value), &[])
}

fn get_symbol_obj<'gc>(env: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    let sym_ctor = env_get(env, "Symbol").ok_or_else(|| -> EvalError<'gc> { crate::raise_type_error!("Symbol not found").into() })?;
    match &*sym_ctor.borrow() {
        Value::Object(o) => Ok(*o),
        _ => Err(crate::raise_type_error!("Symbol is not an object").into()),
    }
}

fn lookup_symbol_on_value<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    value: &Value<'gc>,
    sym_data: &gc_arena::Gc<'gc, crate::core::SymbolData>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match value {
        Value::Object(obj) => crate::core::get_property_with_accessors(mc, env, obj, sym_data),
        _ => crate::core::get_primitive_prototype_property(mc, env, value, sym_data),
    }
}

fn create_resolved_promise<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    value: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let promise_ctor = env_get(env, "Promise").ok_or_else(|| -> EvalError<'gc> { crate::raise_type_error!("Promise not found").into() })?;
    let promise_val = promise_ctor.borrow().clone();
    if let Value::Object(ctor_obj) = &promise_val
        && let Some(resolve_cell) = object_get_key_value(ctor_obj, "resolve")
    {
        let resolve_fn = resolve_cell.borrow().clone();
        return crate::core::evaluate_call_dispatch(mc, env, &resolve_fn, Some(&promise_val), std::slice::from_ref(value));
    }
    Err(crate::raise_type_error!("Promise.resolve not found").into())
}

fn create_rejected_promise<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    reason: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let promise_ctor =
        env_get(env, "Promise").ok_or_else(|| -> EvalError<'gc> { crate::raise_type_error!("Promise.reject not found").into() })?;
    let promise_val = promise_ctor.borrow().clone();
    if let Value::Object(ctor_obj) = &promise_val
        && let Some(reject_cell) = object_get_key_value(ctor_obj, "reject")
    {
        let reject_fn = reject_cell.borrow().clone();
        return crate::core::evaluate_call_dispatch(mc, env, &reject_fn, Some(&promise_val), std::slice::from_ref(reason));
    }
    Err(crate::raise_type_error!("Promise.reject not found").into())
}
