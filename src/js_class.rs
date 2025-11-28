use crate::{
    core::{
        evaluate_expr, evaluate_statements, obj_get_value, obj_set_value, utf8_to_utf16, Expr, JSObjectData, JSObjectDataPtr, Statement,
        Value,
    },
    error::JSError,
};
use std::{cell::RefCell, rc::Rc};

#[derive(Debug, Clone)]
pub enum ClassMember {
    Constructor(Vec<String>, Vec<Statement>),          // parameters, body
    Method(String, Vec<String>, Vec<Statement>),       // name, parameters, body
    StaticMethod(String, Vec<String>, Vec<Statement>), // name, parameters, body
    Property(String, Expr),                            // name, value
    StaticProperty(String, Expr),                      // name, value
    Getter(String, Vec<Statement>),                    // name, body
    Setter(String, Vec<String>, Vec<Statement>),       // name, parameter, body
    StaticGetter(String, Vec<Statement>),              // name, body
    StaticSetter(String, Vec<String>, Vec<Statement>), // name, parameter, body
}

#[derive(Debug, Clone)]
pub struct ClassDefinition {
    pub name: String,
    pub extends: Option<String>,
    pub members: Vec<ClassMember>,
}

pub(crate) fn is_class_instance(obj: &JSObjectDataPtr) -> Result<bool, JSError> {
    // Check if the object's prototype has a __class_def__ property
    // This means the object was created with 'new ClassName()'
    if let Some(proto_val) = obj_get_value(obj, "__proto__")? {
        if let Value::Object(proto_obj) = &*proto_val.borrow() {
            // Check if the prototype object has __class_def__
            if let Some(class_def_val) = obj_get_value(proto_obj, "__class_def__")? {
                if let Value::ClassDefinition(_) = *class_def_val.borrow() {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

pub(crate) fn get_class_proto_obj(class_obj: &JSObjectDataPtr) -> Result<JSObjectDataPtr, JSError> {
    if let Some(proto_val) = obj_get_value(class_obj, "__proto__")? {
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
    if let Some(this_val) = obj_get_value(env, "this")? {
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
            if let Some(class_def_val) = obj_get_value(&class_obj, "__class_def__")? {
                if let Value::ClassDefinition(ref class_def) = *class_def_val.borrow() {
                    // Create instance
                    let instance = Rc::new(RefCell::new(JSObjectData::new()));

                    // Set prototype
                    if let Some(prototype_val) = obj_get_value(&class_obj, "prototype")? {
                        obj_set_value(&instance, "__proto__", prototype_val.borrow().clone())?;
                    }

                    // Set instance properties
                    for member in &class_def.members {
                        if let ClassMember::Property(prop_name, value_expr) = member {
                            let value = evaluate_expr(env, value_expr)?;
                            obj_set_value(&instance, prop_name, value)?;
                        }
                    }

                    // Call constructor if it exists
                    for member in &class_def.members {
                        if let ClassMember::Constructor(params, body) = member {
                            // Create function environment with 'this' bound to instance
                            let func_env = Rc::new(RefCell::new(JSObjectData::new()));

                            // Bind 'this' to the instance
                            obj_set_value(&func_env, "this", Value::Object(instance.clone()))?;

                            // Bind parameters
                            for (i, param) in params.iter().enumerate() {
                                if i < args.len() {
                                    let arg_val = evaluate_expr(env, &args[i])?;
                                    obj_set_value(&func_env, param, arg_val)?;
                                }
                            }

                            // Execute constructor body
                            evaluate_statements(&func_env, body)?;
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
            obj_set_value(&func_env, "this", Value::Object(instance.clone()))?;

            // Bind parameters
            for (i, param) in params.iter().enumerate() {
                if i < args.len() {
                    let arg_val = evaluate_expr(env, &args[i])?;
                    obj_set_value(&func_env, param, arg_val)?;
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
    obj_set_value(&class_obj, "name", Value::String(utf8_to_utf16(name)))?;

    // Create the prototype object first
    let prototype_obj = Rc::new(RefCell::new(JSObjectData::new()));

    // Handle inheritance if extends is specified
    if let Some(parent_class_name) = extends {
        // Look up the parent class in the environment
        if let Some(parent_class_val) = obj_get_value(env, parent_class_name)? {
            if let Value::Object(parent_class_obj) = &*parent_class_val.borrow() {
                // Get the parent class's prototype
                if let Some(parent_proto_val) = obj_get_value(parent_class_obj, "prototype")? {
                    if let Value::Object(parent_proto_obj) = &*parent_proto_val.borrow() {
                        // Set the child class prototype's __proto__ to parent prototype
                        obj_set_value(&prototype_obj, "__proto__", Value::Object(parent_proto_obj.clone()))?;
                    }
                }
            }
        } else {
            return Err(JSError::EvaluationError {
                message: format!("Parent class '{}' not found", parent_class_name),
            });
        }
    }

    obj_set_value(&class_obj, "prototype", Value::Object(prototype_obj.clone()))?;

    // Store class definition for later use
    let class_def = ClassDefinition {
        name: name.to_string(),
        extends: extends.clone(),
        members: members.to_vec(),
    };

    // Store class definition in a special property
    let class_def_val = Value::ClassDefinition(Rc::new(class_def));
    obj_set_value(&class_obj, "__class_def__", class_def_val.clone())?;

    // Store class definition in prototype as well for instanceof checks
    obj_set_value(&prototype_obj, "__class_def__", class_def_val)?;

    // Add methods to prototype
    for member in members {
        match member {
            ClassMember::Method(method_name, params, body) => {
                // Create a closure for the method
                let method_closure = Value::Closure(params.clone(), body.clone(), env.clone());
                obj_set_value(&prototype_obj, method_name, method_closure)?;
            }
            ClassMember::Constructor(_, _) => {
                // Constructor is handled separately during instantiation
            }
            ClassMember::Property(_, _) => {
                // Instance properties not implemented yet
            }
            ClassMember::Getter(getter_name, body) => {
                // Create a getter for the prototype
                let getter = Value::Getter(body.clone(), env.clone());
                obj_set_value(&prototype_obj, getter_name, getter)?;
            }
            ClassMember::Setter(setter_name, param, body) => {
                // Create a setter for the prototype
                let setter = Value::Setter(param.clone(), body.clone(), env.clone());
                obj_set_value(&prototype_obj, setter_name, setter)?;
            }
            ClassMember::StaticMethod(method_name, params, body) => {
                // Add static method to class object
                let method_closure = Value::Closure(params.clone(), body.clone(), env.clone());
                obj_set_value(&class_obj, method_name, method_closure)?;
            }
            ClassMember::StaticProperty(prop_name, value_expr) => {
                // Add static property to class object
                let value = evaluate_expr(env, value_expr)?;
                obj_set_value(&class_obj, prop_name, value)?;
            }
            ClassMember::StaticGetter(getter_name, body) => {
                // Create a static getter for the class object
                let getter = Value::Getter(body.clone(), env.clone());
                obj_set_value(&class_obj, getter_name, getter)?;
            }
            ClassMember::StaticSetter(setter_name, param, body) => {
                // Create a static setter for the class object
                let setter = Value::Setter(param.clone(), body.clone(), env.clone());
                obj_set_value(&class_obj, setter_name, setter)?;
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
    if let Some(method_val) = obj_get_value(class_obj, method)? {
        match &*method_val.borrow() {
            Value::Closure(params, body, _captured_env) => {
                // Create function environment
                let func_env = Rc::new(RefCell::new(JSObjectData::new()));

                // Static methods don't have 'this' bound to an instance
                // 'this' in static methods refers to the class itself
                obj_set_value(&func_env, "this", Value::Object(class_obj.clone()))?;

                // Bind parameters
                for (i, param) in params.iter().enumerate() {
                    if i < args.len() {
                        let arg_val = evaluate_expr(env, &args[i])?;
                        obj_set_value(&func_env, param, arg_val)?;
                    }
                }

                // Execute method body
                return evaluate_statements(&func_env, body);
            }
            _ => {
                return Err(JSError::EvaluationError {
                    message: format!("'{}' is not a static method", method),
                });
            }
        }
    }
    Err(JSError::EvaluationError {
        message: format!("Static method '{}' not found on class", method),
    })
}

pub(crate) fn call_class_method(obj_map: &JSObjectDataPtr, method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let proto_obj = get_class_proto_obj(obj_map)?;
    // Look for method in prototype
    if let Some(method_val) = obj_get_value(&proto_obj, method)? {
        log::trace!("Found method {method} in prototype");
        match &*method_val.borrow() {
            Value::Closure(params, body, _captured_env) => {
                log::trace!("Method is a closure with {} params", params.len());
                // Create function environment with 'this' bound to the instance
                let func_env = Rc::new(RefCell::new(JSObjectData::new()));

                // Bind 'this' to the instance
                obj_set_value(&func_env, "this", Value::Object(obj_map.clone()))?;
                log::trace!("Bound 'this' to instance");

                // Bind parameters
                for (i, param) in params.iter().enumerate() {
                    if i < args.len() {
                        let arg_val = evaluate_expr(env, &args[i])?;
                        obj_set_value(&func_env, param, arg_val)?;
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

pub(crate) fn is_instance_of(obj: &JSObjectDataPtr, constructor: &JSObjectDataPtr) -> Result<bool, JSError> {
    // Get the prototype of the constructor
    if let Some(constructor_proto) = obj_get_value(constructor, "prototype")? {
        if let Value::Object(constructor_proto_obj) = &*constructor_proto.borrow() {
            // Check if obj's prototype chain contains constructor's prototype
            let mut current_proto = obj_get_value(obj, "__proto__")?;
            while let Some(proto_val) = current_proto {
                if let Value::Object(proto_obj) = &*proto_val.borrow() {
                    if Rc::ptr_eq(proto_obj, constructor_proto_obj) {
                        return Ok(true);
                    }
                    current_proto = obj_get_value(proto_obj, "__proto__")?;
                } else {
                    break;
                }
            }
        }
    }
    Ok(false)
}

pub(crate) fn evaluate_super(env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // super refers to the parent class prototype
    // We need to find it from the current class context
    if let Some(this_val) = obj_get_value(env, "this")? {
        if let Value::Object(instance) = &*this_val.borrow() {
            if let Some(proto_val) = obj_get_value(instance, "__proto__")? {
                if let Value::Object(proto_obj) = &*proto_val.borrow() {
                    // Get the parent prototype from the current prototype's __proto__
                    if let Some(parent_proto_val) = obj_get_value(proto_obj, "__proto__")? {
                        return Ok(parent_proto_val.borrow().clone());
                    }
                }
            }
        }
    }
    Err(JSError::EvaluationError {
        message: "super can only be used in class methods or constructors".to_string(),
    })
}

pub(crate) fn evaluate_super_call(env: &JSObjectDataPtr, args: &[Expr]) -> Result<Value, JSError> {
    // super() calls the parent constructor
    if let Some(this_val) = obj_get_value(env, "this")? {
        if let Value::Object(instance) = &*this_val.borrow() {
            if let Some(proto_val) = obj_get_value(instance, "__proto__")? {
                if let Value::Object(proto_obj) = &*proto_val.borrow() {
                    // Get the parent prototype
                    if let Some(parent_proto_val) = obj_get_value(proto_obj, "__proto__")? {
                        if let Value::Object(parent_proto_obj) = &*parent_proto_val.borrow() {
                            // Find the parent class constructor
                            if let Some(parent_class_def_val) = obj_get_value(parent_proto_obj, "__class_def__")? {
                                if let Value::ClassDefinition(ref parent_class_def) = *parent_class_def_val.borrow() {
                                    // Call parent constructor
                                    for member in &parent_class_def.members {
                                        if let ClassMember::Constructor(params, body) = member {
                                            // Create function environment with 'this' bound to instance
                                            let func_env = Rc::new(RefCell::new(JSObjectData::new()));

                                            // Bind 'this' to the instance
                                            obj_set_value(&func_env, "this", Value::Object(instance.clone()))?;

                                            // Bind parameters
                                            for (i, param) in params.iter().enumerate() {
                                                if i < args.len() {
                                                    let arg_val = evaluate_expr(env, &args[i])?;
                                                    obj_set_value(&func_env, param, arg_val)?;
                                                }
                                            }

                                            // Execute parent constructor body
                                            return evaluate_statements(&func_env, body);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Err(JSError::EvaluationError {
        message: "super() can only be called in class constructors".to_string(),
    })
}

pub(crate) fn evaluate_super_property(env: &JSObjectDataPtr, prop: &str) -> Result<Value, JSError> {
    // super.property accesses parent class properties
    if let Some(this_val) = obj_get_value(env, "this")? {
        if let Value::Object(instance) = &*this_val.borrow() {
            if let Some(proto_val) = obj_get_value(instance, "__proto__")? {
                if let Value::Object(proto_obj) = &*proto_val.borrow() {
                    // Get the parent prototype
                    if let Some(parent_proto_val) = obj_get_value(proto_obj, "__proto__")? {
                        if let Value::Object(parent_proto_obj) = &*parent_proto_val.borrow() {
                            // Look for property in parent prototype
                            if let Some(prop_val) = obj_get_value(parent_proto_obj, prop)? {
                                return Ok(prop_val.borrow().clone());
                            }
                        }
                    }
                }
            }
        }
    }
    Err(JSError::EvaluationError {
        message: format!("Property '{}' not found in parent class", prop),
    })
}

pub(crate) fn evaluate_super_method(env: &JSObjectDataPtr, method: &str, args: &[Expr]) -> Result<Value, JSError> {
    // super.method() calls parent class methods
    if let Some(this_val) = obj_get_value(env, "this")? {
        if let Value::Object(instance) = &*this_val.borrow() {
            if let Some(proto_val) = obj_get_value(instance, "__proto__")? {
                if let Value::Object(proto_obj) = &*proto_val.borrow() {
                    // Get the parent prototype
                    if let Some(parent_proto_val) = obj_get_value(proto_obj, "__proto__")? {
                        if let Value::Object(parent_proto_obj) = &*parent_proto_val.borrow() {
                            // Look for method in parent prototype
                            if let Some(method_val) = obj_get_value(parent_proto_obj, method)? {
                                match &*method_val.borrow() {
                                    Value::Closure(params, body, _captured_env) => {
                                        // Create function environment with 'this' bound to instance
                                        let func_env = Rc::new(RefCell::new(JSObjectData::new()));

                                        // Bind 'this' to the instance
                                        obj_set_value(&func_env, "this", Value::Object(instance.clone()))?;

                                        // Bind parameters
                                        for (i, param) in params.iter().enumerate() {
                                            if i < args.len() {
                                                let arg_val = evaluate_expr(env, &args[i])?;
                                                obj_set_value(&func_env, param, arg_val)?;
                                            }
                                        }

                                        // Execute method body
                                        return evaluate_statements(&func_env, body);
                                    }
                                    _ => {
                                        return Err(JSError::EvaluationError {
                                            message: format!("'{}' is not a method in parent class", method),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Err(JSError::EvaluationError {
        message: format!("Method '{}' not found in parent class", method),
    })
}

/// Handle Object constructor calls
pub(crate) fn handle_object_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        // Object() - create empty object
        let obj = Rc::new(RefCell::new(JSObjectData::new()));
        return Ok(Value::Object(obj));
    }
    // Object(value) - convert value to object
    let arg_val = evaluate_expr(env, &args[0])?;
    match arg_val {
        Value::Undefined => {
            // Object(undefined) creates empty object
            let obj = Rc::new(RefCell::new(JSObjectData::new()));
            Ok(Value::Object(obj))
        }
        Value::Object(obj) => {
            // Object(object) returns the object itself
            Ok(Value::Object(obj))
        }
        Value::Number(n) => {
            // Object(number) creates Number object
            let obj = Rc::new(RefCell::new(JSObjectData::new()));
            obj_set_value(&obj, "valueOf", Value::Function("Number_valueOf".to_string()))?;
            obj_set_value(&obj, "toString", Value::Function("Number_toString".to_string()))?;
            obj_set_value(&obj, "__value__", Value::Number(n))?;
            Ok(Value::Object(obj))
        }
        Value::Boolean(b) => {
            // Object(boolean) creates Boolean object
            let obj = Rc::new(RefCell::new(JSObjectData::new()));
            obj_set_value(&obj, "valueOf", Value::Function("Boolean_valueOf".to_string()))?;
            obj_set_value(&obj, "toString", Value::Function("Boolean_toString".to_string()))?;
            obj_set_value(&obj, "__value__", Value::Boolean(b))?;
            Ok(Value::Object(obj))
        }
        Value::String(s) => {
            // Object(string) creates String object
            let obj = Rc::new(RefCell::new(JSObjectData::new()));
            obj_set_value(&obj, "valueOf", Value::Function("String_valueOf".to_string()))?;
            obj_set_value(&obj, "toString", Value::Function("String_toString".to_string()))?;
            obj_set_value(&obj, "length", Value::Number(s.len() as f64))?;
            obj_set_value(&obj, "__value__", Value::String(s))?;
            Ok(Value::Object(obj))
        }
        _ => {
            // For other types, return empty object
            let obj = Rc::new(RefCell::new(JSObjectData::new()));
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
    let obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(&obj, "valueOf", Value::Function("Number_valueOf".to_string()))?;
    obj_set_value(&obj, "toString", Value::Function("Number_toString".to_string()))?;
    obj_set_value(&obj, "__value__", Value::Number(num_val))?;
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
    let obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(&obj, "valueOf", Value::Function("Boolean_valueOf".to_string()))?;
    obj_set_value(&obj, "toString", Value::Function("Boolean_toString".to_string()))?;
    obj_set_value(&obj, "__value__", Value::Boolean(bool_val))?;
    Ok(Value::Object(obj))
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
            Value::Object(_) => utf8_to_utf16("[object Object]"),
            Value::Function(name) => utf8_to_utf16(&format!("[Function: {}]", name)),
            Value::Closure(_, _, _) => utf8_to_utf16("[Function]"),
            Value::ClassDefinition(_) => utf8_to_utf16("[Class]"),
            Value::Getter(_, _) => utf8_to_utf16("[Getter]"),
            Value::Setter(_, _, _) => utf8_to_utf16("[Setter]"),
            Value::Property { .. } => utf8_to_utf16("[Property]"),
        }
    };

    // Create String object
    let obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(&obj, "valueOf", Value::Function("String_valueOf".to_string()))?;
    obj_set_value(&obj, "toString", Value::Function("String_toString".to_string()))?;
    obj_set_value(&obj, "length", Value::Number(str_val.len() as f64))?;
    obj_set_value(&obj, "__value__", Value::String(str_val))?;
    Ok(Value::Object(obj))
}
