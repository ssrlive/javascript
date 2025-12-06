use std::{cell::RefCell, rc::Rc};

use crate::{
    JSError,
    core::{BinaryOp, Expr, PropertyKey, Statement, evaluate_statements, get_well_known_symbol_rc, utf8_to_utf16},
    js_array::is_array,
    js_class::ClassDefinition,
    js_promise::JSPromise,
    raise_type_error,
};

pub type JSObjectDataPtr = Rc<RefCell<JSObjectData>>;

#[derive(Clone, Default)]
pub struct JSObjectData {
    pub properties: std::collections::HashMap<PropertyKey, Rc<RefCell<Value>>>,
    pub constants: std::collections::HashSet<String>,
    pub prototype: Option<Rc<RefCell<JSObjectData>>>,
    pub is_function_scope: bool,
}

impl std::fmt::Debug for JSObjectData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "JSObjectData {{ properties: {}, constants: {}, prototype: {}, is_function_scope: {} }}",
            self.properties.len(),
            self.constants.len(),
            self.prototype.is_some(),
            self.is_function_scope
        )
    }
}

impl JSObjectData {
    pub fn new() -> Self {
        JSObjectData::default()
    }

    pub fn insert(&mut self, key: PropertyKey, val: Rc<RefCell<Value>>) {
        self.properties.insert(key, val);
    }

    pub fn get(&self, key: &PropertyKey) -> Option<Rc<RefCell<Value>>> {
        self.properties.get(key).cloned()
    }

    pub fn contains_key(&self, key: &PropertyKey) -> bool {
        self.properties.contains_key(key)
    }

    pub fn remove(&mut self, key: &PropertyKey) -> Option<Rc<RefCell<Value>>> {
        self.properties.remove(key)
    }

    pub fn keys(&self) -> std::collections::hash_map::Keys<'_, PropertyKey, Rc<RefCell<Value>>> {
        self.properties.keys()
    }

    pub fn is_const(&self, key: &str) -> bool {
        self.constants.contains(key)
    }

    pub fn set_const(&mut self, key: String) {
        self.constants.insert(key);
    }
}

#[derive(Clone, Debug)]
pub struct SymbolData {
    pub description: Option<String>,
}

#[derive(Clone)]
pub enum Value {
    Number(f64),
    /// BigInt literal stored as string form (e.g. "123n" or "0x123n")
    BigInt(String),
    String(Vec<u16>), // UTF-16 code units
    Boolean(bool),
    Undefined,
    Object(JSObjectDataPtr),                                    // Object with properties
    Function(String),                                           // Function name
    Closure(Vec<String>, Vec<Statement>, JSObjectDataPtr),      // parameters, body, captured environment
    AsyncClosure(Vec<String>, Vec<Statement>, JSObjectDataPtr), // parameters, body, captured environment
    ClassDefinition(Rc<ClassDefinition>),                       // Class definition
    Getter(Vec<Statement>, JSObjectDataPtr),                    // getter body, captured environment
    Setter(Vec<String>, Vec<Statement>, JSObjectDataPtr),       // setter parameter, body, captured environment
    Property {
        // Property descriptor with getter/setter/value
        value: Option<Rc<RefCell<Value>>>,
        getter: Option<(Vec<Statement>, JSObjectDataPtr)>,
        setter: Option<(Vec<String>, Vec<Statement>, JSObjectDataPtr)>,
    },
    Promise(Rc<RefCell<JSPromise>>), // Promise object
    Symbol(Rc<SymbolData>),          // Symbol primitive with description
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Number(n) => write!(f, "Number({})", n),
            Value::BigInt(s) => write!(f, "BigInt({})", s),
            Value::String(s) => write!(f, "String({})", String::from_utf16_lossy(s)),
            Value::Boolean(b) => write!(f, "Boolean({})", b),
            Value::Undefined => write!(f, "Undefined"),
            Value::Object(obj) => write!(f, "Object({:p})", Rc::as_ptr(obj)),
            Value::Function(name) => write!(f, "Function({})", name),
            Value::Closure(_, _, _) => write!(f, "Closure"),
            Value::AsyncClosure(_, _, _) => write!(f, "AsyncClosure"),
            Value::ClassDefinition(_) => write!(f, "ClassDefinition"),
            Value::Getter(_, _) => write!(f, "Getter"),
            Value::Setter(_, _, _) => write!(f, "Setter"),
            Value::Property { .. } => write!(f, "Property"),
            Value::Promise(p) => write!(f, "Promise({:p})", Rc::as_ptr(p)),
            Value::Symbol(_) => write!(f, "Symbol"),
        }
    }
}

pub fn is_truthy(val: &Value) -> bool {
    match val {
        Value::BigInt(s) => {
            // Simple check: treat bigint as falsy only when it's zero (0n, 0x0n, 0b0n, 0o0n).
            let s_no_n = if s.ends_with('n') { &s[..s.len() - 1] } else { s.as_str() };
            #[allow(clippy::if_same_then_else)]
            let s_no_prefix = if s_no_n.starts_with("0x") || s_no_n.starts_with("0X") {
                &s_no_n[2..]
            } else if s_no_n.starts_with("0b") || s_no_n.starts_with("0B") {
                &s_no_n[2..]
            } else if s_no_n.starts_with("0o") || s_no_n.starts_with("0O") {
                &s_no_n[2..]
            } else {
                s_no_n
            };
            !s_no_prefix.chars().all(|c| c == '0')
        }
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Boolean(b) => *b,
        Value::Undefined => false,
        Value::Object(_) => true,
        Value::Function(_) => true,
        Value::Closure(_, _, _) => true,
        Value::AsyncClosure(_, _, _) => true,
        Value::ClassDefinition(_) => true,
        Value::Getter(_, _) => true,
        Value::Setter(_, _, _) => true,
        Value::Property { .. } => true,
        Value::Promise(_) => true,
        Value::Symbol(_) => true,
    }
}

// Helper function to compare two values for equality
pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::BigInt(sa), Value::BigInt(sb)) => sa == sb,
        (Value::Number(na), Value::Number(nb)) => na == nb,
        (Value::String(sa), Value::String(sb)) => sa == sb,
        (Value::Boolean(ba), Value::Boolean(bb)) => ba == bb,
        (Value::Undefined, Value::Undefined) => true,
        (Value::Object(_), Value::Object(_)) => false, // Objects are not equal unless same reference
        (Value::Symbol(sa), Value::Symbol(sb)) => Rc::ptr_eq(sa, sb), // Symbols are equal if same reference
        _ => false,                                    // Different types are not equal
    }
}

// Helper function to convert value to string for display
pub fn value_to_string(val: &Value) -> String {
    match val {
        Value::Number(n) => n.to_string(),
        Value::BigInt(s) => s.clone(),
        Value::String(s) => String::from_utf16_lossy(s),
        Value::Boolean(b) => b.to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Object(_) => "[object Object]".to_string(),
        Value::Function(name) => format!("function {}", name),
        Value::Closure(_, _, _) => "function".to_string(),
        Value::AsyncClosure(_, _, _) => "function".to_string(),
        Value::ClassDefinition(_) => "class".to_string(),
        Value::Getter(_, _) => "getter".to_string(),
        Value::Setter(_, _, _) => "setter".to_string(),
        Value::Property { .. } => "[property]".to_string(),
        Value::Promise(_) => "[object Promise]".to_string(),
        Value::Symbol(desc) => match desc.description.as_ref() {
            Some(d) => format!("Symbol({})", d),
            None => "Symbol()".to_string(),
        },
    }
}

// Helper: perform ToPrimitive coercion with a given hint ('string', 'number', 'default')
pub fn to_primitive(val: &Value, hint: &str) -> Result<Value, JSError> {
    match val {
        Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::Undefined | Value::Symbol(_) => Ok(val.clone()),
        Value::Object(obj_map) => {
            // Prefer explicit [Symbol.toPrimitive] if present and callable
            if let Some(tp_sym) = get_well_known_symbol_rc("toPrimitive") {
                let key = PropertyKey::Symbol(tp_sym.clone());
                if let Some(method_rc) = obj_get_value(obj_map, &key)? {
                    let method_val = method_rc.borrow().clone();
                    match method_val {
                        Value::Closure(params, body, captured_env) | Value::AsyncClosure(params, body, captured_env) => {
                            // Create a new execution env and bind this
                            let func_env = Rc::new(RefCell::new(JSObjectData::new()));
                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                            env_set(&func_env, "this", Value::Object(obj_map.clone()))?;
                            // Pass hint as first param if the function declares params
                            if !params.is_empty() {
                                env_set(&func_env, &params[0], Value::String(utf8_to_utf16(hint)))?;
                            }
                            let result = evaluate_statements(&func_env, &body)?;
                            match result {
                                Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::Symbol(_) => return Ok(result),
                                _ => {
                                    return Err(raise_type_error!("[Symbol.toPrimitive] must return a primitive"));
                                }
                            }
                        }
                        _ => {
                            // Not a closure/minimally supported callable - fall through to default algorithm
                        }
                    }
                }
            }

            // Default algorithm: order depends on hint
            if hint == "string" {
                // toString -> valueOf
                let to_s = crate::js_object::handle_to_string_method(&Value::Object(obj_map.clone()), &[])?;
                if matches!(to_s, Value::String(_) | Value::Number(_) | Value::Boolean(_)) {
                    return Ok(to_s);
                }
                let val_of = crate::js_object::handle_value_of_method(&Value::Object(obj_map.clone()), &[])?;
                if matches!(val_of, Value::String(_) | Value::Number(_) | Value::Boolean(_)) {
                    return Ok(val_of);
                }
            } else {
                // number or default: valueOf -> toString
                let val_of = crate::js_object::handle_value_of_method(&Value::Object(obj_map.clone()), &[])?;
                if matches!(val_of, Value::Number(_) | Value::String(_) | Value::Boolean(_)) {
                    return Ok(val_of);
                }
                let to_s = crate::js_object::handle_to_string_method(&Value::Object(obj_map.clone()), &[])?;
                if matches!(to_s, Value::String(_) | Value::Number(_) | Value::Boolean(_)) {
                    return Ok(to_s);
                }
            }

            Err(raise_type_error!("Cannot convert object to primitive"))
        }
        _ => Ok(val.clone()),
    }
}

// Helper function to convert value to string for sorting
pub fn value_to_sort_string(val: &Value) -> String {
    match val {
        Value::Number(n) => {
            if n.is_nan() {
                "NaN".to_string()
            } else if *n == f64::INFINITY {
                "Infinity".to_string()
            } else if *n == f64::NEG_INFINITY {
                "-Infinity".to_string()
            } else {
                n.to_string()
            }
        }
        Value::BigInt(s) => s.clone(),
        Value::String(s) => String::from_utf16_lossy(s),
        Value::Boolean(b) => b.to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Object(_) => "[object Object]".to_string(),
        Value::Function(name) => format!("[function {}]", name),
        Value::Closure(_, _, _) | Value::AsyncClosure(_, _, _) => "[function]".to_string(),
        Value::ClassDefinition(_) => "[class]".to_string(),
        Value::Getter(_, _) => "[getter]".to_string(),
        Value::Setter(_, _, _) => "[setter]".to_string(),
        Value::Property { .. } => "[property]".to_string(),
        Value::Promise(_) => "[object Promise]".to_string(),
        Value::Symbol(_) => "[object Symbol]".to_string(),
    }
}

// Helper accessors for objects and environments
pub fn obj_get_value(js_obj: &JSObjectDataPtr, key: &PropertyKey) -> Result<Option<Rc<RefCell<Value>>>, JSError> {
    // Search own properties and then walk the prototype chain until we find
    // a matching property or run out of prototypes.
    let mut current: Option<JSObjectDataPtr> = Some(js_obj.clone());
    while let Some(cur) = current {
        if let Some(val) = cur.borrow().get(key) {
            // Found an own/inherited value on `cur`. For getters we bind `this` to
            // the original object (`js_obj`) as per JS semantics.
            let val_clone = val.borrow().clone();
            match val_clone {
                Value::Property { value, getter, .. } => {
                    log::trace!("obj_get_value - property descriptor found for key {}", key);
                    if let Some((body, env)) = getter {
                        // Create a new environment with this bound to the original object
                        let getter_env = Rc::new(RefCell::new(JSObjectData::new()));
                        getter_env.borrow_mut().prototype = Some(env);
                        env_set(&getter_env, "this", Value::Object(js_obj.clone()))?;
                        let result = evaluate_statements(&getter_env, &body)?;
                        return Ok(Some(Rc::new(RefCell::new(result))));
                    } else if let Some(val_rc) = value {
                        return Ok(Some(val_rc));
                    } else {
                        return Ok(Some(Rc::new(RefCell::new(Value::Undefined))));
                    }
                }
                Value::Getter(body, env) => {
                    log::trace!("obj_get_value - getter found for key {}", key);
                    let getter_env = Rc::new(RefCell::new(JSObjectData::new()));
                    getter_env.borrow_mut().prototype = Some(env);
                    env_set(&getter_env, "this", Value::Object(js_obj.clone()))?;
                    let result = evaluate_statements(&getter_env, &body)?;
                    return Ok(Some(Rc::new(RefCell::new(result))));
                }
                _ => {
                    log::trace!("obj_get_value - raw value found for key {}", key);
                    return Ok(Some(val.clone()));
                }
            }
        }
        // Not found on this object; continue with prototype.
        current = cur.borrow().prototype.clone();
    }

    // No own or inherited property found, fall back to special-case handling
    // (well-known symbol fallbacks, array/string iterator helpers, etc.).

    // Provide default well-known symbol fallbacks (non-own) for some built-ins.
    if let PropertyKey::Symbol(sym_rc) = key {
        // Support default Symbol.iterator for built-ins like Array and wrapped String objects.
        if let Some(iter_sym_rc) = get_well_known_symbol_rc("iterator")
            && let (Value::Symbol(iter_sd), Value::Symbol(req_sd)) = (&*iter_sym_rc.borrow(), &*sym_rc.borrow())
            && Rc::ptr_eq(iter_sd, req_sd)
        {
            // Array default iterator
            if is_array(js_obj) {
                // next function body
                let next_body = vec![
                    Statement::Let("idx".to_string(), Some(Expr::Var("__i".to_string()))),
                    Statement::If(
                        Expr::Binary(
                            Box::new(Expr::Var("idx".to_string())),
                            BinaryOp::LessThan,
                            Box::new(Expr::Property(Box::new(Expr::Var("__array".to_string())), "length".to_string())),
                        ),
                        vec![
                            Statement::Let(
                                "v".to_string(),
                                Some(Expr::Index(
                                    Box::new(Expr::Var("__array".to_string())),
                                    Box::new(Expr::Var("idx".to_string())),
                                )),
                            ),
                            Statement::Expr(Expr::Assign(
                                Box::new(Expr::Var("__i".to_string())),
                                Box::new(Expr::Binary(
                                    Box::new(Expr::Var("idx".to_string())),
                                    BinaryOp::Add,
                                    Box::new(Expr::Value(Value::Number(1.0))),
                                )),
                            )),
                            Statement::Return(Some(Expr::Object(vec![
                                ("value".to_string(), Expr::Var("v".to_string())),
                                ("done".to_string(), Expr::Value(Value::Boolean(false))),
                            ]))),
                        ],
                        Some(vec![Statement::Return(Some(Expr::Object(vec![(
                            "done".to_string(),
                            Expr::Value(Value::Boolean(true)),
                        )])))]),
                    ),
                ];

                let arr_iter_body = vec![
                    Statement::Let("__i".to_string(), Some(Expr::Value(Value::Number(0.0)))),
                    Statement::Return(Some(Expr::Object(vec![(
                        "next".to_string(),
                        Expr::Function(Vec::new(), next_body),
                    )]))),
                ];

                let captured_env = Rc::new(RefCell::new(JSObjectData::new()));
                captured_env.borrow_mut().insert(
                    PropertyKey::String("__array".to_string()),
                    Rc::new(RefCell::new(Value::Object(js_obj.clone()))),
                );
                let closure = Value::Closure(Vec::new(), arr_iter_body, captured_env.clone());
                return Ok(Some(Rc::new(RefCell::new(closure))));
            }

            // Wrapped String iterator (for String objects)
            if let Some(wrapped) = js_obj.borrow().get(&"__value__".into())
                && let Value::String(_) = &*wrapped.borrow()
            {
                let next_body = vec![
                    Statement::Let("idx".to_string(), Some(Expr::Var("__i".to_string()))),
                    Statement::If(
                        Expr::Binary(
                            Box::new(Expr::Var("idx".to_string())),
                            BinaryOp::LessThan,
                            Box::new(Expr::Property(Box::new(Expr::Var("__s".to_string())), "length".to_string())),
                        ),
                        vec![
                            Statement::Let(
                                "v".to_string(),
                                Some(Expr::Index(
                                    Box::new(Expr::Var("__s".to_string())),
                                    Box::new(Expr::Var("idx".to_string())),
                                )),
                            ),
                            Statement::Expr(Expr::Assign(
                                Box::new(Expr::Var("__i".to_string())),
                                Box::new(Expr::Binary(
                                    Box::new(Expr::Var("idx".to_string())),
                                    BinaryOp::Add,
                                    Box::new(Expr::Value(Value::Number(1.0))),
                                )),
                            )),
                            Statement::Return(Some(Expr::Object(vec![
                                ("value".to_string(), Expr::Var("v".to_string())),
                                ("done".to_string(), Expr::Value(Value::Boolean(false))),
                            ]))),
                        ],
                        Some(vec![Statement::Return(Some(Expr::Object(vec![(
                            "done".to_string(),
                            Expr::Value(Value::Boolean(true)),
                        )])))]),
                    ),
                ];

                let str_iter_body = vec![
                    Statement::Let("__i".to_string(), Some(Expr::Value(Value::Number(0.0)))),
                    Statement::Return(Some(Expr::Object(vec![(
                        "next".to_string(),
                        Expr::Function(Vec::new(), next_body),
                    )]))),
                ];

                let captured_env = Rc::new(RefCell::new(JSObjectData::new()));
                captured_env.borrow_mut().insert(
                    PropertyKey::String("__s".to_string()),
                    Rc::new(RefCell::new(wrapped.borrow().clone())),
                );
                let closure = Value::Closure(Vec::new(), str_iter_body, captured_env.clone());
                return Ok(Some(Rc::new(RefCell::new(closure))));
            }
        }
        if let Some(tag_sym_rc) = get_well_known_symbol_rc("toStringTag")
            && let (Value::Symbol(tag_sd), Value::Symbol(req_sd)) = (&*tag_sym_rc.borrow(), &*sym_rc.borrow())
            && Rc::ptr_eq(tag_sd, req_sd)
        {
            if is_array(js_obj) {
                return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("Array"))))));
            }
            if let Some(wrapped) = js_obj.borrow().get(&"__value__".into()) {
                match &*wrapped.borrow() {
                    Value::String(_) => return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("String")))))),
                    Value::Number(_) => return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("Number")))))),
                    Value::Boolean(_) => return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("Boolean")))))),
                    _ => {}
                }
            }
            if js_obj.borrow().contains_key(&"__timestamp".into()) {
                return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("Date"))))));
            }
        }
    }

    Ok(None)
}

pub fn obj_set_value(js_obj: &JSObjectDataPtr, key: &PropertyKey, val: Value) -> Result<(), JSError> {
    // Check if there's a setter for this property
    let existing_opt = js_obj.borrow().get(key);
    if let Some(existing) = existing_opt {
        match existing.borrow().clone() {
            Value::Property { value: _, getter, setter } => {
                if let Some((param, body, env)) = setter {
                    // Create a new environment with this bound to the object and the parameter
                    let setter_env = Rc::new(RefCell::new(JSObjectData::new()));
                    setter_env.borrow_mut().prototype = Some(env);
                    env_set(&setter_env, "this", Value::Object(js_obj.clone()))?;
                    env_set(&setter_env, &param[0], val)?;
                    let _v = evaluate_statements(&setter_env, &body)?;
                } else {
                    // No setter, update value
                    let value = Some(Rc::new(RefCell::new(val)));
                    let new_prop = Value::Property { value, getter, setter };
                    js_obj.borrow_mut().insert(key.clone(), Rc::new(RefCell::new(new_prop)));
                }
                return Ok(());
            }
            Value::Setter(param, body, env) => {
                // Create a new environment with this bound to the object and the parameter
                let setter_env = Rc::new(RefCell::new(JSObjectData::new()));
                setter_env.borrow_mut().prototype = Some(env);
                env_set(&setter_env, "this", Value::Object(js_obj.clone()))?;
                env_set(&setter_env, &param[0], val)?;
                evaluate_statements(&setter_env, &body)?;
                return Ok(());
            }
            _ => {}
        }
    }
    // No setter, just set the value normally
    js_obj.borrow_mut().insert(key.clone(), Rc::new(RefCell::new(val)));
    Ok(())
}

pub fn obj_set_rc(map: &JSObjectDataPtr, key: &PropertyKey, val_rc: Rc<RefCell<Value>>) {
    map.borrow_mut().insert(key.clone(), val_rc);
}

pub fn obj_delete(map: &JSObjectDataPtr, key: &PropertyKey) -> bool {
    map.borrow_mut().remove(key);
    true // In JavaScript, delete always returns true
}

pub fn env_get<T: AsRef<str>>(env: &JSObjectDataPtr, key: T) -> Option<Rc<RefCell<Value>>> {
    env.borrow().get(&key.as_ref().into())
}

pub fn env_set<T: AsRef<str>>(env: &JSObjectDataPtr, key: T, val: Value) -> Result<(), JSError> {
    let key = key.as_ref();
    if env.borrow().is_const(key) {
        return Err(raise_type_error!(format!("Assignment to constant variable '{key}'")));
    }
    env.borrow_mut()
        .insert(PropertyKey::String(key.to_string()), Rc::new(RefCell::new(val)));
    Ok(())
}

pub fn env_set_recursive<T: AsRef<str>>(env: &JSObjectDataPtr, key: T, val: Value) -> Result<(), JSError> {
    let key_str = key.as_ref();
    let mut current = env.clone();
    loop {
        if current.borrow().contains_key(&key_str.into()) {
            return env_set(&current, key_str, val);
        }
        let parent_opt = current.borrow().prototype.clone();
        if let Some(parent) = parent_opt {
            current = parent;
        } else {
            // if not found, set in current env
            return env_set(env, key_str, val);
        }
    }
}

pub fn env_set_var(env: &JSObjectDataPtr, key: &str, val: Value) -> Result<(), JSError> {
    let mut current = env.clone();
    loop {
        if current.borrow().is_function_scope {
            return env_set(&current, key, val);
        }
        let parent_opt = current.borrow().prototype.clone();
        if let Some(parent) = parent_opt {
            current = parent;
        } else {
            // If no function scope found, set in current env (global)
            return env_set(env, key, val);
        }
    }
}

pub fn env_set_const(env: &JSObjectDataPtr, key: &str, val: Value) {
    let mut env_mut = env.borrow_mut();
    env_mut.insert(PropertyKey::String(key.to_string()), Rc::new(RefCell::new(val)));
    env_mut.set_const(key.to_string());
}
