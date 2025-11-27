use crate::{
    error::JSError,
    quickjs::{
        evaluate_expr, evaluate_statements, obj_get, obj_set_val, utf8_to_utf16, Expr, JSObjectData, JSObjectDataPtr, Statement, Value,
    },
};
use std::{cell::RefCell, rc::Rc};

#[derive(Debug, Clone)]
pub enum ClassMember {
    Constructor(Vec<String>, Vec<Statement>),          // parameters, body
    Method(String, Vec<String>, Vec<Statement>),       // name, parameters, body
    StaticMethod(String, Vec<String>, Vec<Statement>), // name, parameters, body
    Property(String, Expr),                            // name, value
    StaticProperty(String, Expr),                      // name, value
}

#[derive(Debug, Clone)]
pub struct ClassDefinition {
    pub name: String,
    pub extends: Option<String>,
    pub members: Vec<ClassMember>,
}

pub(crate) fn is_class_instance(obj: &JSObjectDataPtr) -> bool {
    // Check if the object's prototype has a __class_def__ property
    // This means the object was created with 'new ClassName()'
    if let Some(proto_val) = obj_get(obj, "__proto__") {
        if let Value::Object(proto_obj) = &*proto_val.borrow() {
            // Check if the prototype object has __class_def__
            if let Some(class_def_val) = obj_get(proto_obj, "__class_def__") {
                if let Value::ClassDefinition(_) = *class_def_val.borrow() {
                    return true;
                }
            }
        }
    }
    false
}

pub(crate) fn get_class_proto_obj(class_obj: &JSObjectDataPtr) -> Result<JSObjectDataPtr, JSError> {
    if let Some(proto_val) = obj_get(class_obj, "__proto__") {
        if let Value::Object(proto_obj) = &*proto_val.borrow() {
            return Ok(proto_obj.clone());
        }
    }
    Err(JSError::TypeError {
        message: "Prototype object not found".to_string(),
    })
}

pub(crate) fn evaluate_this(env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Check if 'this' is bound in the current environment
    if let Some(this_val) = obj_get(env, "this") {
        Ok(this_val.borrow().clone())
    } else {
        // Default to global object if no 'this' binding exists
        Ok(Value::Object(env.clone()))
    }
}

pub(crate) fn evaluate_new(env: &JSObjectDataPtr, constructor: &Expr, args: &[Expr]) -> Result<Value, JSError> {
    // Evaluate the constructor
    let constructor_val = evaluate_expr(env, constructor)?;

    match constructor_val {
        Value::Object(class_obj) => {
            // Check if this is a class object
            if let Some(class_def_val) = obj_get(&class_obj, "__class_def__") {
                if let Value::ClassDefinition(ref class_def) = *class_def_val.borrow() {
                    // Create instance
                    let instance = Rc::new(RefCell::new(JSObjectData::new()));

                    // Set prototype
                    if let Some(prototype_val) = obj_get(&class_obj, "prototype") {
                        obj_set_val(&instance, "__proto__", prototype_val.borrow().clone());
                    }

                    // Call constructor if it exists
                    for member in &class_def.members {
                        if let ClassMember::Constructor(params, body) = member {
                            // Create function environment with 'this' bound to instance
                            let func_env = Rc::new(RefCell::new(JSObjectData::new()));

                            // Bind 'this' to the instance
                            obj_set_val(&func_env, "this", Value::Object(instance.clone()));

                            // Bind parameters
                            for (i, param) in params.iter().enumerate() {
                                if i < args.len() {
                                    let arg_val = evaluate_expr(env, &args[i])?;
                                    obj_set_val(&func_env, param, arg_val);
                                }
                            }

                            // Execute constructor body
                            evaluate_statements(&func_env, &body)?;
                            break;
                        }
                    }

                    return Ok(Value::Object(instance));
                }
            }
        }
        Value::Closure(params, body, captured_env) => {
            // Handle function constructors
            let instance = Rc::new(RefCell::new(JSObjectData::new()));
            let func_env = captured_env.clone();

            // Bind 'this' to the instance
            obj_set_val(&func_env, "this", Value::Object(instance.clone()));

            // Bind parameters
            for (i, param) in params.iter().enumerate() {
                if i < args.len() {
                    let arg_val = evaluate_expr(env, &args[i])?;
                    obj_set_val(&func_env, param, arg_val);
                }
            }

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
                _ => {
                    log::warn!("evaluate_new - constructor is not an object or closure: Function({func_name})",);
                }
            }
        }
        _ => {
            log::warn!("evaluate_new - constructor is not an object or closure: {constructor_val:?}");
        }
    }

    Err(JSError::TypeError {
        message: "Constructor is not callable".to_string(),
    })
}

pub(crate) fn create_class_object(
    name: &str,
    extends: &Option<String>,
    members: &[ClassMember],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    // Create a class object (function) that can be instantiated with 'new'
    let class_obj = Rc::new(RefCell::new(JSObjectData::new()));

    // Set class name
    obj_set_val(&class_obj, "name", Value::String(utf8_to_utf16(name)));

    // Create the prototype object first
    let prototype_obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_val(&class_obj, "prototype", Value::Object(prototype_obj.clone()));

    // Store class definition for later use
    let class_def = ClassDefinition {
        name: name.to_string(),
        extends: extends.clone(),
        members: members.to_vec(),
    };

    // Store class definition in a special property
    let class_def_val = Value::ClassDefinition(Rc::new(class_def));
    obj_set_val(&class_obj, "__class_def__", class_def_val.clone());

    // Store class definition in prototype as well for instanceof checks
    obj_set_val(&prototype_obj, "__class_def__", class_def_val);

    // Add methods to prototype
    for member in members {
        match member {
            ClassMember::Method(method_name, params, body) => {
                // Create a closure for the method
                let method_closure = Value::Closure(params.clone(), body.clone(), env.clone());
                obj_set_val(&prototype_obj, method_name, method_closure);
            }
            ClassMember::Constructor(_, _) => {
                // Constructor is handled separately during instantiation
            }
            ClassMember::Property(_, _) => {
                // Instance properties not implemented yet
            }
            ClassMember::StaticMethod(method_name, params, body) => {
                // Add static method to class object
                let method_closure = Value::Closure(params.clone(), body.clone(), env.clone());
                obj_set_val(&class_obj, method_name, method_closure);
            }
            ClassMember::StaticProperty(_, _) => {
                // Static properties not implemented yet
            }
        }
    }

    Ok(Value::Object(class_obj))
}

pub(crate) fn call_class_method(obj_map: &JSObjectDataPtr, method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let proto_obj = get_class_proto_obj(&obj_map)?;
    // Look for method in prototype
    if let Some(method_val) = obj_get(&proto_obj, method) {
        log::trace!("Found method {method} in prototype");
        match &*method_val.borrow() {
            Value::Closure(params, body, _captured_env) => {
                log::trace!("Method is a closure with {} params", params.len());
                // Create function environment with 'this' bound to the instance
                let func_env = Rc::new(RefCell::new(JSObjectData::new()));

                // Bind 'this' to the instance
                obj_set_val(&func_env, "this", Value::Object(obj_map.clone()));
                log::trace!("Bound 'this' to instance");

                // Bind parameters
                for (i, param) in params.iter().enumerate() {
                    if i < args.len() {
                        let arg_val = evaluate_expr(env, &args[i])?;
                        obj_set_val(&func_env, param, arg_val);
                    }
                }

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
    Err(JSError::EvaluationError {
        message: format!("Method {method} not implemented for this object type"),
    })
}

pub(crate) fn is_instance_of(obj: &JSObjectDataPtr, constructor: &JSObjectDataPtr) -> bool {
    // Get the prototype of the constructor
    if let Some(constructor_proto) = obj_get(&constructor, "prototype") {
        if let Value::Object(constructor_proto_obj) = &*constructor_proto.borrow() {
            // Check if obj's prototype chain contains constructor's prototype
            let mut current_proto = obj_get(&obj, "__proto__");
            while let Some(proto_val) = current_proto {
                if let Value::Object(proto_obj) = &*proto_val.borrow() {
                    if Rc::ptr_eq(proto_obj, constructor_proto_obj) {
                        return true;
                    }
                    current_proto = obj_get(proto_obj, "__proto__");
                } else {
                    break;
                }
            }
        }
    }
    false
}
