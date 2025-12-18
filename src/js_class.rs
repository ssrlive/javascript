#![allow(clippy::collapsible_if, clippy::collapsible_match)]

use crate::core::{Expr, JSObjectDataPtr, Statement, Value, evaluate_expr, evaluate_statements, get_own_property, new_js_object_data};
use crate::core::{obj_get_key_value, obj_set_key_value, value_to_string};
use crate::js_array::is_array;
use crate::{error::JSError, unicode::utf8_to_utf16};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum ClassMember {
    Constructor(Vec<(String, Option<Box<Expr>>)>, Vec<Statement>), // parameters, body
    Method(String, Vec<(String, Option<Box<Expr>>)>, Vec<Statement>), // name, parameters, body
    StaticMethod(String, Vec<(String, Option<Box<Expr>>)>, Vec<Statement>), // name, parameters, body
    Property(String, Expr),                                        // name, value
    StaticProperty(String, Expr),                                  // name, value
    Getter(String, Vec<Statement>),                                // name, body
    Setter(String, Vec<(String, Option<Box<Expr>>)>, Vec<Statement>), // name, parameter, body
    StaticGetter(String, Vec<Statement>),                          // name, body
    StaticSetter(String, Vec<(String, Option<Box<Expr>>)>, Vec<Statement>), // name, parameter, body
}

#[derive(Debug, Clone)]
pub struct ClassDefinition {
    pub name: String,
    pub extends: Option<Expr>,
    pub members: Vec<ClassMember>,
}

pub(crate) fn is_class_instance(obj: &JSObjectDataPtr) -> Result<bool, JSError> {
    // Check if the object's prototype has a __class_def__ property
    // This means the object was created with 'new ClassName()'
    if let Some(proto_val) = obj_get_key_value(obj, &"__proto__".into())?
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        // Check if the prototype object has __class_def__
        if let Some(class_def_val) = obj_get_key_value(proto_obj, &"__class_def__".into())?
            && let Value::ClassDefinition(_) = *class_def_val.borrow()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn get_class_proto_obj(class_obj: &JSObjectDataPtr) -> Result<JSObjectDataPtr, JSError> {
    if let Some(proto_val) = obj_get_key_value(class_obj, &"__proto__".into())?
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        return Ok(proto_obj.clone());
    }
    Err(raise_type_error!("Prototype object not found"))
}

pub(crate) fn evaluate_this(env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Walk the environment/prototype (scope) chain looking for a bound
    // `this` value. Some nested/temporarily created environments (e.g.
    // catch-block envs) do not bind `this` themselves but inherit the
    // effective global `this` from an outer environment. Return the
    // first `this` value found; if none is present, return the topmost
    // environment object as the default global object.
    let mut env_opt: Option<JSObjectDataPtr> = Some(env.clone());
    let mut last_seen: JSObjectDataPtr = env.clone();
    while let Some(env_ptr) = env_opt {
        last_seen = env_ptr.clone();
        if let Some(this_val_rc) = obj_get_key_value(&env_ptr, &"this".into())? {
            return Ok(this_val_rc.borrow().clone());
        }
        env_opt = env_ptr.borrow().prototype.clone();
    }
    Ok(Value::Object(last_seen))
}

pub(crate) fn evaluate_new(env: &JSObjectDataPtr, constructor: &Expr, args: &[Expr]) -> Result<Value, JSError> {
    // Evaluate the constructor
    let constructor_val = evaluate_expr(env, constructor)?;
    // Log pointer/type of the evaluated constructor value for diagnostics
    match &constructor_val {
        Value::Object(o) => {
            log::debug!("DBG evaluate_new - constructor evaluated -> Object ptr={:p}", Rc::as_ptr(o));
        }
        Value::Function(name) => {
            log::debug!("DBG evaluate_new - constructor evaluated -> Builtin Function {}", name);
        }
        Value::Closure(..) | Value::AsyncClosure(..) => {
            log::debug!("DBG evaluate_new - constructor evaluated -> Closure")
        }
        other => {
            log::debug!("DBG evaluate_new - constructor evaluated -> {:?}", other);
        }
    }
    log::trace!("evaluate_new - invoking constructor (evaluated)");

    match constructor_val {
        Value::Object(class_obj) => {
            // If this object wraps a closure (created from a function
            // expression/declaration), treat it as a constructor by
            // extracting the internal closure and invoking it as a
            // constructor. This allows script-defined functions stored
            // as objects to be used with `new` while still exposing
            // assignable `prototype` properties.
            if let Some(cl_val_rc) = obj_get_key_value(&class_obj, &"__closure__".into())? {
                if let Value::Closure(params, body, captured_env, _) | Value::AsyncClosure(params, body, captured_env, _) =
                    &*cl_val_rc.borrow()
                {
                    // Create the instance object
                    let instance = new_js_object_data();

                    // Attach a debug identifier to help correlate runtime instances
                    // with logs (printed as a pointer string).
                    let dbg_ptr_str = format!("{:p}", Rc::as_ptr(&instance));
                    obj_set_key_value(&instance, &"__dbg_ptr__".into(), Value::String(utf8_to_utf16(&dbg_ptr_str)))?;
                    log::debug!(
                        "DBG evaluate_new - created instance ptr={:p} __dbg_ptr__={}",
                        Rc::as_ptr(&instance),
                        dbg_ptr_str
                    );

                    // Set prototype from the constructor object's `.prototype` if available
                    if let Some(prototype_val) = obj_get_key_value(&class_obj, &"prototype".into())? {
                        if let Value::Object(proto_obj) = &*prototype_val.borrow() {
                            instance.borrow_mut().prototype = Some(proto_obj.clone());
                            obj_set_key_value(&instance, &"__proto__".into(), Value::Object(proto_obj.clone()))?;
                        } else {
                            obj_set_key_value(&instance, &"__proto__".into(), prototype_val.borrow().clone())?;
                        }
                    }

                    // Prepare function environment with 'this' bound to the instance
                    let func_env = new_js_object_data();
                    func_env.borrow_mut().prototype = Some(captured_env.clone());
                    obj_set_key_value(&func_env, &"this".into(), Value::Object(instance.clone()))?;

                    // Collect all arguments, expanding spreads
                    let mut evaluated_args = Vec::new();
                    crate::core::expand_spread_in_call_args(env, args, &mut evaluated_args)?;

                    // Bind parameters
                    crate::core::bind_function_parameters(&func_env, params, &evaluated_args)?;

                    // Execute constructor body
                    evaluate_statements(&func_env, body)?;

                    // Ensure instance.constructor points back to the constructor object
                    obj_set_key_value(&instance, &"constructor".into(), Value::Object(class_obj.clone()))?;

                    return Ok(Value::Object(instance));
                }
            }
            // Check if this is a TypedArray constructor
            if get_own_property(&class_obj, &"__kind".into()).is_some() {
                return crate::js_typedarray::handle_typedarray_constructor(&class_obj, args, env);
            }

            // Check if this is ArrayBuffer constructor
            if get_own_property(&class_obj, &"__arraybuffer".into()).is_some() {
                return crate::js_typedarray::handle_arraybuffer_constructor(args, env);
            }

            // Check if this is DataView constructor
            if get_own_property(&class_obj, &"__dataview".into()).is_some() {
                return crate::js_typedarray::handle_dataview_constructor(args, env);
            }

            // Check if this is a class object
            if let Some(class_def_val) = obj_get_key_value(&class_obj, &"__class_def__".into())?
                && let Value::ClassDefinition(ref class_def) = *class_def_val.borrow()
            {
                // Create instance
                let instance = new_js_object_data();

                // Set prototype (both internal pointer and __proto__ property)
                if let Some(prototype_val) = obj_get_key_value(&class_obj, &"prototype".into())? {
                    if let Value::Object(proto_obj) = &*prototype_val.borrow() {
                        instance.borrow_mut().prototype = Some(proto_obj.clone());
                        obj_set_key_value(&instance, &"__proto__".into(), Value::Object(proto_obj.clone()))?;
                    } else {
                        // Fallback: store whatever prototype value was provided
                        obj_set_key_value(&instance, &"__proto__".into(), prototype_val.borrow().clone())?;
                    }
                }

                // Set instance properties
                for member in &class_def.members {
                    if let ClassMember::Property(prop_name, value_expr) = member {
                        let value = evaluate_expr(env, value_expr)?;
                        obj_set_key_value(&instance, &prop_name.into(), value)?;
                    }
                }

                // Call constructor if it exists
                for member in &class_def.members {
                    if let ClassMember::Constructor(params, body) = member {
                        // Create function environment with 'this' bound to instance
                        let func_env = new_js_object_data();

                        // Bind 'this' to the instance
                        obj_set_key_value(&func_env, &"this".into(), Value::Object(instance.clone()))?;

                        // Collect all arguments, expanding spreads
                        let mut evaluated_args = Vec::new();
                        crate::core::expand_spread_in_call_args(env, args, &mut evaluated_args)?;

                        // Bind parameters
                        crate::core::bind_function_parameters(&func_env, params, &evaluated_args)?;

                        // Execute constructor body
                        evaluate_statements(&func_env, body)?;
                        break;
                    }
                }

                // Also set an own `constructor` property on the instance so `err.constructor`
                // resolves directly to the canonical constructor object.
                obj_set_key_value(&instance, &"constructor".into(), Value::Object(class_obj.clone()))?;

                return Ok(Value::Object(instance));
            }
            // Check if this is the Number constructor object
            if obj_get_key_value(&class_obj, &"MAX_VALUE".into())?.is_some() {
                return handle_number_constructor(args, env);
            }
            // Check for constructor-like singleton objects created by the evaluator
            if get_own_property(&class_obj, &"__is_string_constructor".into()).is_some() {
                return handle_string_constructor(args, env);
            }
            if get_own_property(&class_obj, &"__is_boolean_constructor".into()).is_some() {
                return handle_boolean_constructor(args, env);
            }
            // Error-like constructors (Error) created via ensure_constructor_object
            if get_own_property(&class_obj, &"__is_error_constructor".into()).is_some() {
                log::debug!(
                    "DBG evaluate_new - entered error-like constructor branch, args.len={} class_obj ptr={:p}",
                    args.len(),
                    Rc::as_ptr(&class_obj)
                );
                if !args.is_empty() {
                    log::debug!("DBG evaluate_new - args[0] expr = {:?}", args[0]);
                }
                // Use the class_obj as the canonical constructor
                let canonical_ctor = class_obj.clone();

                // Create instance object
                let instance = new_js_object_data();

                // Attach a debug identifier (pointer string) so we can correlate
                // runtime-created instances with later logs (e.g. thrown object ptrs).
                let dbg_ptr_str = format!("{:p}", Rc::as_ptr(&instance));
                obj_set_key_value(&instance, &"__dbg_ptr__".into(), Value::String(utf8_to_utf16(&dbg_ptr_str)))?;
                log::debug!(
                    "DBG evaluate_new - created instance ptr={:p} __dbg_ptr__={}",
                    Rc::as_ptr(&instance),
                    dbg_ptr_str
                );

                // Set prototype from the canonical constructor's `.prototype` if available
                if let Some(prototype_val) = obj_get_key_value(&canonical_ctor, &"prototype".into())? {
                    if let Value::Object(proto_obj) = &*prototype_val.borrow() {
                        instance.borrow_mut().prototype = Some(proto_obj.clone());
                        obj_set_key_value(&instance, &"__proto__".into(), Value::Object(proto_obj.clone()))?;
                    } else {
                        obj_set_key_value(&instance, &"__proto__".into(), prototype_val.borrow().clone())?;
                    }
                }

                // If a message argument was supplied, set the message property
                if !args.is_empty() {
                    log::debug!("DBG evaluate_new - about to evaluate args[0]");
                    match evaluate_expr(env, &args[0]) {
                        Ok(val) => {
                            log::debug!("DBG evaluate_new - eval args[0] result = {:?}", val);
                            match val {
                                Value::String(s) => {
                                    log::debug!("DBG evaluate_new - setting message (string) = {:?}", String::from_utf16_lossy(&s));
                                    obj_set_key_value(&instance, &"message".into(), Value::String(s))?;
                                }
                                Value::Number(n) => {
                                    log::debug!("DBG evaluate_new - setting message (number) = {}", n);
                                    obj_set_key_value(&instance, &"message".into(), Value::String(utf8_to_utf16(&n.to_string())))?;
                                }
                                _ => {
                                    // convert other types to string via value_to_string
                                    let s = utf8_to_utf16(&value_to_string(&val));
                                    log::debug!("DBG evaluate_new - setting message (other) = {:?}", String::from_utf16_lossy(&s));
                                    obj_set_key_value(&instance, &"message".into(), Value::String(s))?;
                                }
                            }
                        }
                        Err(err) => {
                            log::debug!("DBG evaluate_new - failed to evaluate args[0]: {:?}", err);
                        }
                    }
                }

                // Ensure prototype.constructor points back to the canonical constructor
                if let Some(prototype_val) = obj_get_key_value(&canonical_ctor, &"prototype".into())? {
                    if let Value::Object(proto_obj) = &*prototype_val.borrow() {
                        match crate::core::get_own_property(proto_obj, &"constructor".into()) {
                            Some(existing_rc) => {
                                if let Value::Object(existing_ctor_obj) = &*existing_rc.borrow() {
                                    if !Rc::ptr_eq(existing_ctor_obj, &canonical_ctor) {
                                        obj_set_key_value(proto_obj, &"constructor".into(), Value::Object(canonical_ctor.clone()))?;
                                    }
                                } else {
                                    obj_set_key_value(proto_obj, &"constructor".into(), Value::Object(canonical_ctor.clone()))?;
                                }
                            }
                            None => {
                                obj_set_key_value(proto_obj, &"constructor".into(), Value::Object(canonical_ctor.clone()))?;
                            }
                        }
                    }
                }

                // Ensure constructor.name exists
                let ctor_name = "Error";
                match crate::core::get_own_property(&canonical_ctor, &"name".into()) {
                    Some(name_rc) => {
                        if let Value::Undefined = &*name_rc.borrow() {
                            obj_set_key_value(&canonical_ctor, &"name".into(), Value::String(utf8_to_utf16(ctor_name)))?;
                        }
                    }
                    None => {
                        obj_set_key_value(&canonical_ctor, &"name".into(), Value::String(utf8_to_utf16(ctor_name)))?;
                    }
                }

                // Also set an own `constructor` property on the instance so `err.constructor`
                // resolves directly to the canonical constructor object used by the bootstrap.
                obj_set_key_value(&instance, &"constructor".into(), Value::Object(canonical_ctor.clone()))?;

                // Build a minimal stack string from any linked __frame/__caller
                // frames available on the current environment. This provides a
                // reasonable default for Error instances created via `new Error()`.
                let mut stack_lines: Vec<String> = Vec::new();
                // First line: Error: <message>
                let message_text = match crate::core::get_own_property(&instance, &"message".into()) {
                    Some(mrc) => match &*mrc.borrow() {
                        Value::String(s) => String::from_utf16_lossy(s),
                        other => crate::core::value_to_string(other),
                    },
                    None => String::new(),
                };
                stack_lines.push(format!("Error: {}", message_text));

                // Walk caller chain starting from current env
                let mut env_opt: Option<crate::core::JSObjectDataPtr> = Some(env.clone());
                while let Some(env_ptr) = env_opt {
                    if let Ok(Some(frame_val_rc)) = obj_get_key_value(&env_ptr, &"__frame".into()) {
                        if let Value::String(s_utf16) = &*frame_val_rc.borrow() {
                            stack_lines.push(format!("    at {}", String::from_utf16_lossy(s_utf16)));
                        }
                    }
                    // follow caller link if present
                    if let Ok(Some(caller_rc)) = obj_get_key_value(&env_ptr, &"__caller".into()) {
                        if let Value::Object(caller_env) = &*caller_rc.borrow() {
                            env_opt = Some(caller_env.clone());
                            continue;
                        }
                    }
                    break;
                }

                let stack_combined = stack_lines.join("\n");
                obj_set_key_value(&instance, &"stack".into(), Value::String(utf8_to_utf16(&stack_combined)))?;

                return Ok(Value::Object(instance));
            }
        }
        Value::Closure(params, body, captured_env, _) | Value::AsyncClosure(params, body, captured_env, _) => {
            // Handle function constructors
            let instance = new_js_object_data();
            let func_env = new_js_object_data();
            func_env.borrow_mut().prototype = Some(captured_env.clone());

            // Bind 'this' to the instance
            obj_set_key_value(&func_env, &"this".into(), Value::Object(instance.clone()))?;

            // Collect all arguments, expanding spreads
            let mut evaluated_args = Vec::new();
            crate::core::expand_spread_in_call_args(env, args, &mut evaluated_args)?;

            // Bind parameters
            crate::core::bind_function_parameters(&func_env, &params, &evaluated_args)?;

            // Execute function body
            evaluate_statements(&func_env, &body)?;

            return Ok(Value::Object(instance));
        }
        Value::Function(func_name) => {
            // Handle built-in constructors
            match func_name.as_str() {
                "Date" => {
                    return crate::js_date::handle_date_constructor(args, env);
                }
                "Array" => {
                    return crate::js_array::handle_array_constructor(args, env);
                }
                "RegExp" => {
                    return crate::js_regexp::handle_regexp_constructor(args, env);
                }
                "Object" => {
                    return handle_object_constructor(args, env);
                }
                "Number" => {
                    return handle_number_constructor(args, env);
                }
                "Boolean" => {
                    return handle_boolean_constructor(args, env);
                }
                "String" => {
                    return handle_string_constructor(args, env);
                }
                "Promise" => {
                    return crate::js_promise::handle_promise_constructor(args, env);
                }
                "Map" => return crate::js_map::handle_map_constructor(args, env),
                "Set" => return crate::js_set::handle_set_constructor(args, env),
                "Proxy" => return crate::js_proxy::handle_proxy_constructor(args, env),
                "WeakMap" => return crate::js_weakmap::handle_weakmap_constructor(args, env),
                "WeakSet" => return crate::js_weakset::handle_weakset_constructor(args, env),
                "MockIntlConstructor" => {
                    // Handle mock Intl constructor for testing
                    let locale_arg = if !args.is_empty() {
                        match evaluate_expr(env, &args[0])? {
                            // Accept either a single string or an array containing a string
                            Value::String(s) => Some(crate::unicode::utf16_to_utf8(&s)),
                            Value::Object(arr_obj) if is_array(&arr_obj) => {
                                // Try to read index 0 from the array
                                if let Some(first_rc) = obj_get_key_value(&arr_obj, &"0".into())? {
                                    match &*first_rc.borrow() {
                                        Value::String(s) => Some(crate::unicode::utf16_to_utf8(s)),
                                        _ => None,
                                    }
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    return crate::js_testintl::create_mock_intl_instance(locale_arg, env);
                }
                _ => {
                    log::warn!("evaluate_new - constructor is not an object or closure: Function({func_name})",);
                }
            }
        }
        _ => {
            log::warn!("evaluate_new - constructor is not an object or closure: {constructor_val:?}");
        }
    }

    Err(raise_type_error!("Constructor is not callable"))
}

pub(crate) fn create_class_object(
    name: &str,
    extends: &Option<Expr>,
    members: &[ClassMember],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    // Create a class object (function) that can be instantiated with 'new'
    let class_obj = new_js_object_data();

    // Set class name
    obj_set_key_value(&class_obj, &"name".into(), Value::String(utf8_to_utf16(name)))?;

    // Create the prototype object first
    let prototype_obj = new_js_object_data();

    // Handle inheritance if extends is specified
    if let Some(parent_expr) = extends {
        // Evaluate the extends expression to get the parent class object
        let parent_val = evaluate_expr(env, parent_expr)?;
        if let Value::Object(parent_class_obj) = parent_val {
            // Get the parent class's prototype
            if let Some(parent_proto_val) = obj_get_key_value(&parent_class_obj, &"prototype".into())?
                && let Value::Object(parent_proto_obj) = &*parent_proto_val.borrow()
            {
                // Set the child class prototype's internal prototype pointer and __proto__ property
                prototype_obj.borrow_mut().prototype = Some(parent_proto_obj.clone());
                obj_set_key_value(&prototype_obj, &"__proto__".into(), Value::Object(parent_proto_obj.clone()))?;
            }
        } else {
            return Err(raise_eval_error!("Parent class expression did not evaluate to a class constructor"));
        }
    }

    obj_set_key_value(&class_obj, &"prototype".into(), Value::Object(prototype_obj.clone()))?;

    // Store class definition for later use
    let class_def = ClassDefinition {
        name: name.to_string(),
        extends: extends.clone(),
        members: members.to_vec(),
    };

    // Store class definition in a special property
    let class_def_val = Value::ClassDefinition(Rc::new(class_def));
    obj_set_key_value(&class_obj, &"__class_def__".into(), class_def_val.clone())?;

    // Store class definition in prototype as well for instanceof checks
    obj_set_key_value(&prototype_obj, &"__class_def__".into(), class_def_val)?;

    // Add methods to prototype
    for member in members {
        match member {
            ClassMember::Method(method_name, params, body) => {
                // Create a closure for the method
                let method_closure = Value::Closure(params.clone(), body.clone(), env.clone(), Some(prototype_obj.clone()));
                obj_set_key_value(&prototype_obj, &method_name.into(), method_closure)?;
            }
            ClassMember::Constructor(_, _) => {
                // Constructor is handled separately during instantiation
            }
            ClassMember::Property(_, _) => {
                // Instance properties not implemented yet
            }
            ClassMember::Getter(getter_name, body) => {
                // Create a getter for the prototype
                let getter = Value::Getter(body.clone(), env.clone(), Some(prototype_obj.clone()));
                obj_set_key_value(&prototype_obj, &getter_name.into(), getter)?;
            }
            ClassMember::Setter(setter_name, param, body) => {
                // Create a setter for the prototype
                let setter = Value::Setter(param.clone(), body.clone(), env.clone(), Some(prototype_obj.clone()));
                obj_set_key_value(&prototype_obj, &setter_name.into(), setter)?;
            }
            ClassMember::StaticMethod(method_name, params, body) => {
                // Add static method to class object
                let method_closure = Value::Closure(params.clone(), body.clone(), env.clone(), Some(class_obj.clone()));
                obj_set_key_value(&class_obj, &method_name.into(), method_closure)?;
            }
            ClassMember::StaticProperty(prop_name, value_expr) => {
                // Add static property to class object
                let value = evaluate_expr(env, value_expr)?;
                obj_set_key_value(&class_obj, &prop_name.into(), value)?;
            }
            ClassMember::StaticGetter(getter_name, body) => {
                // Create a static getter for the class object
                let getter = Value::Getter(body.clone(), env.clone(), Some(class_obj.clone()));
                obj_set_key_value(&class_obj, &getter_name.into(), getter)?;
            }
            ClassMember::StaticSetter(setter_name, param, body) => {
                // Create a static setter for the class object
                let setter = Value::Setter(param.clone(), body.clone(), env.clone(), Some(class_obj.clone()));
                obj_set_key_value(&class_obj, &setter_name.into(), setter)?;
            }
        }
    }

    Ok(Value::Object(class_obj))
}

pub(crate) fn call_static_method(
    class_obj: &JSObjectDataPtr,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    // Look for static method directly on the class object
    if let Some(method_val) = obj_get_key_value(class_obj, &method.into())? {
        match &*method_val.borrow() {
            Value::Closure(params, body, _captured_env, _) | Value::AsyncClosure(params, body, _captured_env, _) => {
                // Create function environment
                let func_env = new_js_object_data();

                // Static methods don't have 'this' bound to an instance
                // 'this' in static methods refers to the class itself
                obj_set_key_value(&func_env, &"this".into(), Value::Object(class_obj.clone()))?;

                // Collect all arguments, expanding spreads
                let mut evaluated_args = Vec::new();
                crate::core::expand_spread_in_call_args(env, args, &mut evaluated_args)?;

                // Bind parameters
                crate::core::bind_function_parameters(&func_env, params, &evaluated_args)?;

                // Execute method body
                return evaluate_statements(&func_env, body);
            }
            _ => {
                return Err(raise_eval_error!(format!("'{method}' is not a static method")));
            }
        }
    }
    Err(raise_eval_error!(format!("Static method '{method}' not found on class")))
}

pub(crate) fn call_class_method(obj_map: &JSObjectDataPtr, method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let proto_obj = get_class_proto_obj(obj_map)?;
    // Look for method in prototype
    if let Some(method_val) = obj_get_key_value(&proto_obj, &method.into())? {
        log::trace!("Found method {method} in prototype");
        match &*method_val.borrow() {
            Value::Closure(params, body, captured_env, home_obj) | Value::AsyncClosure(params, body, captured_env, home_obj) => {
                log::trace!("Method is a closure with {} params", params.len());
                // Use the closure's captured environment as the base for the
                // function environment so outer-scope lookups work as expected.
                let func_env = new_js_object_data();
                func_env.borrow_mut().prototype = Some(captured_env.clone());

                if let Some(home) = home_obj {
                    crate::core::obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home.clone()))?;
                }

                // Bind 'this' to the instance (use env_set to avoid invoking setters)
                crate::core::env_set(&func_env, "this", Value::Object(obj_map.clone()))?;
                log::trace!("Bound 'this' to instance");

                // Collect all arguments, expanding spreads
                let mut evaluated_args = Vec::new();
                crate::core::expand_spread_in_call_args(env, args, &mut evaluated_args)?;

                // Bind parameters
                crate::core::bind_function_parameters(&func_env, params, &evaluated_args)?;

                // Execute method body
                log::trace!("Executing method body");
                return evaluate_statements(&func_env, body);
            }
            _ => {
                log::warn!("Method is not a closure: {:?}", method_val.borrow());
            }
        }
    }
    // Other object methods not implemented
    Err(raise_eval_error!(format!("Method '{method}' not found on class instance")))
}

pub(crate) fn is_instance_of(obj: &JSObjectDataPtr, constructor: &JSObjectDataPtr) -> Result<bool, JSError> {
    // Get the prototype of the constructor
    if let Some(constructor_proto) = obj_get_key_value(constructor, &"prototype".into())? {
        log::trace!("is_instance_of: constructor.prototype raw = {:?}", constructor_proto);
        if let Value::Object(constructor_proto_obj) = &*constructor_proto.borrow() {
            // Walk the internal prototype chain directly (don't use obj_get_key_value for __proto__)
            let mut current_proto_opt: Option<JSObjectDataPtr> = obj.borrow().prototype.clone();
            log::trace!(
                "is_instance_of: starting internal current_proto = {:?}",
                current_proto_opt.as_ref().map(Rc::as_ptr)
            );
            while let Some(proto_obj) = current_proto_opt {
                log::trace!(
                    "is_instance_of: proto_obj={:p}, constructor_proto_obj={:p}",
                    Rc::as_ptr(&proto_obj),
                    Rc::as_ptr(constructor_proto_obj)
                );
                if Rc::ptr_eq(&proto_obj, constructor_proto_obj) {
                    return Ok(true);
                }
                current_proto_opt = proto_obj.borrow().prototype.clone();
            }
        }
    }
    Ok(false)
}

pub(crate) fn evaluate_super(env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // super refers to the parent class prototype
    // We need to find it from the current class context
    if let Some(this_val) = obj_get_key_value(env, &"this".into())?
        && let Value::Object(instance) = &*this_val.borrow()
        && let Some(proto_val) = obj_get_key_value(instance, &"__proto__".into())?
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        // Get the parent prototype from the current prototype's __proto__
        if let Some(parent_proto_val) = obj_get_key_value(proto_obj, &"__proto__".into())? {
            return Ok(parent_proto_val.borrow().clone());
        }
    }
    Err(raise_eval_error!("super can only be used in class methods or constructors"))
}

pub(crate) fn evaluate_super_call(env: &JSObjectDataPtr, args: &[Expr]) -> Result<Value, JSError> {
    // super() calls the parent constructor
    if let Some(this_val) = obj_get_key_value(env, &"this".into())?
        && let Value::Object(instance) = &*this_val.borrow()
        && let Some(proto_val) = obj_get_key_value(instance, &"__proto__".into())?
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        // Get the parent prototype
        if let Some(parent_proto_val) = obj_get_key_value(proto_obj, &"__proto__".into())?
            && let Value::Object(parent_proto_obj) = &*parent_proto_val.borrow()
        {
            // Find the parent class constructor
            if let Some(parent_class_def_val) = obj_get_key_value(parent_proto_obj, &"__class_def__".into())?
                && let Value::ClassDefinition(ref parent_class_def) = *parent_class_def_val.borrow()
            {
                // Call parent constructor
                for member in &parent_class_def.members {
                    if let ClassMember::Constructor(params, body) = member {
                        // Create function environment with 'this' bound to instance
                        let func_env = new_js_object_data();

                        // Bind 'this' to the instance
                        obj_set_key_value(&func_env, &"this".into(), Value::Object(instance.clone()))?;

                        // Collect all arguments, expanding spreads
                        let mut evaluated_args = Vec::new();
                        crate::core::expand_spread_in_call_args(env, args, &mut evaluated_args)?;

                        // Bind parameters
                        crate::core::bind_function_parameters(&func_env, params, &evaluated_args)?;

                        // Execute parent constructor body
                        return evaluate_statements(&func_env, body);
                    }
                }
            }
        }
    }
    Err(raise_eval_error!("super() can only be called in class constructors"))
}

pub(crate) fn evaluate_super_property(env: &JSObjectDataPtr, prop: &str) -> Result<Value, JSError> {
    // super.property accesses parent class properties
    // Use [[HomeObject]] if available
    if let Some(home_obj_val) = obj_get_key_value(env, &"__home_object__".into())? {
        if let Value::Object(home_obj) = &*home_obj_val.borrow() {
            // Super is the prototype of HomeObject
            if let Some(super_obj) = &home_obj.borrow().prototype {
                // Look up property on super object
                if let Some(prop_val) = obj_get_key_value(super_obj, &prop.into())? {
                    return Ok(prop_val.borrow().clone());
                }
                return Ok(Value::Undefined);
            }
        }
    }

    // Fallback for legacy class implementation (if any)
    if let Some(this_val) = obj_get_key_value(env, &"this".into())?
        && let Value::Object(instance) = &*this_val.borrow()
        && let Some(proto_val) = obj_get_key_value(instance, &"__proto__".into())?
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        // Get the parent prototype
        if let Some(parent_proto_val) = obj_get_key_value(proto_obj, &"__proto__".into())?
            && let Value::Object(parent_proto_obj) = &*parent_proto_val.borrow()
        {
            // Look for property in parent prototype
            if let Some(prop_val) = obj_get_key_value(parent_proto_obj, &prop.into())? {
                return Ok(prop_val.borrow().clone());
            }
        }
    }
    Err(raise_eval_error!(format!("Property '{prop}' not found in parent class")))
}

pub(crate) fn evaluate_super_method(env: &JSObjectDataPtr, method: &str, args: &[Expr]) -> Result<Value, JSError> {
    // super.method() calls parent class methods

    // Use [[HomeObject]] if available
    if let Some(home_obj_val) = obj_get_key_value(env, &"__home_object__".into())? {
        if let Value::Object(home_obj) = &*home_obj_val.borrow() {
            // Super is the prototype of HomeObject
            if let Some(super_obj) = &home_obj.borrow().prototype {
                // Look up method on super object
                if let Some(method_val) = obj_get_key_value(super_obj, &method.into())? {
                    // We need to call this method with the current 'this'
                    if let Some(this_val) = obj_get_key_value(env, &"this".into())? {
                        match &*method_val.borrow() {
                            Value::Closure(params, body, captured_env, home_obj)
                            | Value::AsyncClosure(params, body, captured_env, home_obj) => {
                                // Create function environment with 'this' bound to instance
                                let func_env = new_js_object_data();
                                func_env.borrow_mut().prototype = Some(captured_env.clone());

                                if let Some(home) = home_obj {
                                    obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home.clone()))?;
                                }

                                // Bind 'this' to the instance
                                obj_set_key_value(&func_env, &"this".into(), this_val.borrow().clone())?;

                                // Collect all arguments, expanding spreads
                                let mut evaluated_args = Vec::new();
                                crate::core::expand_spread_in_call_args(env, args, &mut evaluated_args)?;

                                // Bind parameters
                                crate::core::bind_function_parameters(&func_env, params, &evaluated_args)?;

                                // Execute method body
                                return evaluate_statements(&func_env, body);
                            }
                            Value::Function(func_name) => {
                                if func_name == "Object.prototype.toString" {
                                    return crate::js_object::handle_to_string_method(&this_val.borrow().clone(), args, env);
                                }
                                if func_name == "Object.prototype.valueOf" {
                                    return crate::js_object::handle_value_of_method(&this_val.borrow().clone(), args, env);
                                }
                            }
                            Value::Object(func_obj) => {
                                if let Some(cl_rc) = obj_get_key_value(func_obj, &"__closure__".into())? {
                                    match &*cl_rc.borrow() {
                                        Value::Closure(params, body, captured_env, home_obj)
                                        | Value::AsyncClosure(params, body, captured_env, home_obj) => {
                                            // Create function environment with 'this' bound to instance
                                            let func_env = new_js_object_data();
                                            func_env.borrow_mut().prototype = Some(captured_env.clone());

                                            if let Some(home) = home_obj {
                                                obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home.clone()))?;
                                            }

                                            // Bind 'this' to the instance
                                            obj_set_key_value(&func_env, &"this".into(), this_val.borrow().clone())?;

                                            // Collect all arguments, expanding spreads
                                            let mut evaluated_args = Vec::new();
                                            crate::core::expand_spread_in_call_args(env, args, &mut evaluated_args)?;

                                            // Bind parameters
                                            crate::core::bind_function_parameters(&func_env, params, &evaluated_args)?;

                                            // Execute method body
                                            return evaluate_statements(&func_env, body);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // Fallback for legacy class implementation (if any)
    if let Some(this_val) = obj_get_key_value(env, &"this".into())?
        && let Value::Object(instance) = &*this_val.borrow()
        && let Some(proto_val) = obj_get_key_value(instance, &"__proto__".into())?
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        // Get the parent prototype
        if let Some(parent_proto_val) = obj_get_key_value(proto_obj, &"__proto__".into())?
            && let Value::Object(parent_proto_obj) = &*parent_proto_val.borrow()
        {
            // Look for method in parent prototype
            if let Some(method_val) = obj_get_key_value(parent_proto_obj, &method.into())? {
                match &*method_val.borrow() {
                    Value::Closure(params, body, _captured_env, _) | Value::AsyncClosure(params, body, _captured_env, _) => {
                        // Create function environment with 'this' bound to instance
                        let func_env = new_js_object_data();

                        // Bind 'this' to the instance
                        obj_set_key_value(&func_env, &"this".into(), Value::Object(instance.clone()))?;

                        // Collect all arguments, expanding spreads
                        let mut evaluated_args = Vec::new();
                        crate::core::expand_spread_in_call_args(env, args, &mut evaluated_args)?;

                        // Bind parameters
                        crate::core::bind_function_parameters(&func_env, params, &evaluated_args)?;

                        // Execute method body
                        return evaluate_statements(&func_env, body);
                    }
                    _ => {
                        return Err(raise_eval_error!(format!("'{method}' is not a method in parent class")));
                    }
                }
            }
        }
    }
    Err(raise_eval_error!(format!("Method '{method}' not found in parent class")))
}

/// Handle Object constructor calls
pub(crate) fn handle_object_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        // Object() - create empty object
        let obj = new_js_object_data();
        return Ok(Value::Object(obj));
    }
    // Object(value) - convert value to object
    let arg_val = evaluate_expr(env, &args[0])?;
    match arg_val {
        Value::Undefined => {
            // Object(undefined) creates empty object
            let obj = new_js_object_data();
            Ok(Value::Object(obj))
        }
        Value::Object(obj) => {
            // Object(object) returns the object itself
            Ok(Value::Object(obj))
        }
        Value::Number(n) => {
            // Object(number) creates Number object
            let obj = new_js_object_data();
            obj_set_key_value(&obj, &"valueOf".into(), Value::Function("Number_valueOf".to_string()))?;
            obj_set_key_value(&obj, &"toString".into(), Value::Function("Number_toString".to_string()))?;
            obj_set_key_value(&obj, &"__value__".into(), Value::Number(n))?;
            // Set internal prototype to Number.prototype if available
            crate::core::set_internal_prototype_from_constructor(&obj, env, "Number")?;
            Ok(Value::Object(obj))
        }
        Value::Boolean(b) => {
            // Object(boolean) creates Boolean object
            let obj = new_js_object_data();
            obj_set_key_value(&obj, &"valueOf".into(), Value::Function("Boolean_valueOf".to_string()))?;
            obj_set_key_value(&obj, &"toString".into(), Value::Function("Boolean_toString".to_string()))?;
            obj_set_key_value(&obj, &"__value__".into(), Value::Boolean(b))?;
            // Set internal prototype to Boolean.prototype if available
            crate::core::set_internal_prototype_from_constructor(&obj, env, "Boolean")?;
            Ok(Value::Object(obj))
        }
        Value::String(s) => {
            // Object(string) creates String object
            let obj = new_js_object_data();
            obj_set_key_value(&obj, &"valueOf".into(), Value::Function("String_valueOf".to_string()))?;
            obj_set_key_value(&obj, &"toString".into(), Value::Function("String_toString".to_string()))?;
            obj_set_key_value(&obj, &"length".into(), Value::Number(s.len() as f64))?;
            obj_set_key_value(&obj, &"__value__".into(), Value::String(s))?;
            // Set internal prototype to String.prototype if available
            crate::core::set_internal_prototype_from_constructor(&obj, env, "String")?;
            Ok(Value::Object(obj))
        }
        Value::BigInt(h) => {
            // Object(bigint) creates a boxed BigInt-like object
            let obj = new_js_object_data();
            obj_set_key_value(&obj, &"valueOf".into(), Value::Function("BigInt_valueOf".to_string()))?;
            obj_set_key_value(&obj, &"toString".into(), Value::Function("BigInt_toString".to_string()))?;
            obj_set_key_value(&obj, &"__value__".into(), Value::BigInt(h.clone()))?;
            // Set internal prototype to BigInt.prototype if available
            crate::core::set_internal_prototype_from_constructor(&obj, env, "BigInt")?;
            Ok(Value::Object(obj))
        }
        Value::Symbol(sd) => {
            // Object(symbol) creates Symbol object
            let obj = new_js_object_data();
            obj_set_key_value(&obj, &"valueOf".into(), Value::Function("Symbol_valueOf".to_string()))?;
            obj_set_key_value(&obj, &"toString".into(), Value::Function("Symbol_toString".to_string()))?;
            obj_set_key_value(&obj, &"__value__".into(), Value::Symbol(sd.clone()))?;
            // Set internal prototype to Symbol.prototype if available
            crate::core::set_internal_prototype_from_constructor(&obj, env, "Symbol")?;
            Ok(Value::Object(obj))
        }
        _ => {
            // For other types, return empty object
            let obj = new_js_object_data();
            Ok(Value::Object(obj))
        }
    }
}

/// Handle Number constructor calls
pub(crate) fn handle_number_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let num_val = if args.is_empty() {
        // Number() - returns 0
        0.0
    } else {
        // Number(value) - convert value to number
        let arg_val = evaluate_expr(env, &args[0])?;
        match arg_val {
            Value::Number(n) => n,
            Value::String(s) => {
                let str_val = String::from_utf16_lossy(&s);
                str_val.trim().parse::<f64>().unwrap_or(f64::NAN)
            }
            Value::Boolean(b) => {
                if b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Undefined => f64::NAN,
            Value::Object(_) => f64::NAN,
            _ => f64::NAN,
        }
    };

    // Create Number object
    let obj = new_js_object_data();
    obj_set_key_value(&obj, &"valueOf".into(), Value::Function("Number_valueOf".to_string()))?;
    obj_set_key_value(&obj, &"toString".into(), Value::Function("Number_toString".to_string()))?;
    obj_set_key_value(&obj, &"__value__".into(), Value::Number(num_val))?;
    // Set internal prototype to Number.prototype if available
    crate::core::set_internal_prototype_from_constructor(&obj, env, "Number")?;
    Ok(Value::Object(obj))
}

/// Handle Boolean constructor calls
pub(crate) fn handle_boolean_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let bool_val = if args.is_empty() {
        // Boolean() - returns false
        false
    } else {
        // Boolean(value) - convert value to boolean
        let arg_val = evaluate_expr(env, &args[0])?;
        match arg_val {
            Value::Boolean(b) => b,
            Value::Number(n) => n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Undefined => false,
            Value::Object(_) => true,
            _ => false,
        }
    };

    // Create Boolean object
    let obj = new_js_object_data();
    obj_set_key_value(&obj, &"valueOf".into(), Value::Function("Boolean_valueOf".to_string()))?;
    obj_set_key_value(&obj, &"toString".into(), Value::Function("Boolean_toString".to_string()))?;
    obj_set_key_value(&obj, &"__value__".into(), Value::Boolean(bool_val))?;
    // Set internal prototype to Boolean.prototype if available
    crate::core::set_internal_prototype_from_constructor(&obj, env, "Boolean")?;
    Ok(Value::Object(obj))
}

pub(crate) fn boolean_prototype_to_string(_args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let this_val = evaluate_this(env)?;
    match this_val {
        Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
        Value::Object(obj) => {
            if let Some(val) = obj_get_key_value(&obj, &"__value__".into())? {
                if let Value::Boolean(b) = *val.borrow() {
                    return Ok(Value::String(utf8_to_utf16(&b.to_string())));
                }
            }
            Err(raise_type_error!("Boolean.prototype.toString requires that 'this' be a Boolean"))
        }
        _ => Err(raise_type_error!("Boolean.prototype.toString requires that 'this' be a Boolean")),
    }
}

pub(crate) fn boolean_prototype_value_of(_args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let this_val = evaluate_this(env)?;
    match this_val {
        Value::Boolean(b) => Ok(Value::Boolean(b)),
        Value::Object(obj) => {
            if let Some(val) = obj_get_key_value(&obj, &"__value__".into())? {
                if let Value::Boolean(b) = *val.borrow() {
                    return Ok(Value::Boolean(b));
                }
            }
            Err(raise_type_error!("Boolean.prototype.valueOf requires that 'this' be a Boolean"))
        }
        _ => Err(raise_type_error!("Boolean.prototype.valueOf requires that 'this' be a Boolean")),
    }
}

/// Handle String constructor calls
pub(crate) fn handle_string_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let str_val = if args.is_empty() {
        // String() - returns empty string
        Vec::new()
    } else {
        // String(value) - convert value to string
        let arg_val = evaluate_expr(env, &args[0])?;
        match arg_val {
            Value::String(s) => s.clone(),
            Value::Number(n) => utf8_to_utf16(&n.to_string()),
            Value::Boolean(b) => utf8_to_utf16(&b.to_string()),
            Value::Undefined => utf8_to_utf16("undefined"),
            Value::Null => utf8_to_utf16("null"),
            Value::Object(_) => utf8_to_utf16("[object Object]"),
            Value::Function(name) => utf8_to_utf16(&format!("[Function: {}]", name)),
            Value::Closure(..) | Value::AsyncClosure(..) => utf8_to_utf16("[Function]"),
            Value::ClassDefinition(_) => utf8_to_utf16("[Class]"),
            Value::Getter(..) => utf8_to_utf16("[Getter]"),
            Value::Setter(..) => utf8_to_utf16("[Setter]"),
            Value::Property { .. } => utf8_to_utf16("[Property]"),
            Value::Promise(_) => utf8_to_utf16("[object Promise]"),
            Value::Symbol(_) => utf8_to_utf16("[object Symbol]"),
            Value::BigInt(s) => utf8_to_utf16(&s.to_string()),
            Value::Map(_) => utf8_to_utf16("[object Map]"),
            Value::Set(_) => utf8_to_utf16("[object Set]"),
            Value::WeakMap(_) => utf8_to_utf16("[object WeakMap]"),
            Value::WeakSet(_) => utf8_to_utf16("[object WeakSet]"),
            Value::GeneratorFunction(..) => utf8_to_utf16("[GeneratorFunction]"),
            Value::Generator(_) => utf8_to_utf16("[object Generator]"),
            Value::Proxy(_) => utf8_to_utf16("[object Proxy]"),
            Value::ArrayBuffer(_) => utf8_to_utf16("[object ArrayBuffer]"),
            Value::DataView(_) => utf8_to_utf16("[object DataView]"),
            Value::TypedArray(_) => utf8_to_utf16("[object TypedArray]"),
        }
    };

    // Create String object
    let obj = new_js_object_data();
    obj_set_key_value(&obj, &"valueOf".into(), Value::Function("String_valueOf".to_string()))?;
    obj_set_key_value(&obj, &"toString".into(), Value::Function("String_toString".to_string()))?;
    obj_set_key_value(&obj, &"length".into(), Value::Number(str_val.len() as f64))?;
    obj_set_key_value(&obj, &"__value__".into(), Value::String(str_val))?;
    // Set internal prototype to String.prototype if available
    crate::core::set_internal_prototype_from_constructor(&obj, env, "String")?;
    Ok(Value::Object(obj))
}
