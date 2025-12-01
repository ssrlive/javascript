use crate::core::{Expr, JSObjectData, JSObjectDataPtr, Value, evaluate_expr, obj_set_value};
use crate::error::JSError;
use std::cell::RefCell;
use std::rc::Rc;

/// Create the console object with logging functions
pub fn make_console_object() -> Result<JSObjectDataPtr, JSError> {
    let console_obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(&console_obj, &"log".into(), Value::Function("console.log".to_string()))?;
    Ok(console_obj)
}

/// Handle console object method calls
pub fn handle_console_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "log" => {
            // console.log call
            for arg in args {
                let arg_val = evaluate_expr(env, arg)?;
                match arg_val {
                    Value::Number(n) => print!("{}", n),
                    Value::String(s) => {
                        print!("{}", String::from_utf16_lossy(&s))
                    }
                    Value::Boolean(b) => print!("{}", b),
                    Value::Undefined => print!("undefined"),
                    Value::Object(obj) => {
                        if crate::js_array::is_array(&obj) {
                            // Print array contents
                            let len = crate::js_array::get_array_length(&obj).unwrap_or(0);
                            print!("[");
                            for i in 0..len {
                                if i > 0 {
                                    print!(",");
                                }
                                if let Some(val_rc) = crate::core::obj_get_value(&obj, &i.to_string().into())? {
                                    match &*val_rc.borrow() {
                                        Value::Number(n) => print!("{}", n),
                                        Value::String(s) => print!("\"{}\"", String::from_utf16_lossy(s)),
                                        Value::Boolean(b) => print!("{}", b),
                                        Value::Undefined => print!("undefined"),
                                        _ => print!("[object Object]"),
                                    }
                                }
                            }
                            print!("]");
                        } else {
                            // Print object properties
                            print!("{{");
                            let mut first = true;
                            for (key, val_rc) in obj.borrow().properties.iter() {
                                if !first {
                                    print!(", ");
                                }
                                first = false;
                                print!("{}: ", key);
                                match &*val_rc.borrow() {
                                    Value::Number(n) => print!("{}", n),
                                    Value::String(s) => print!("\"{}\"", String::from_utf16_lossy(s)),
                                    Value::Boolean(b) => print!("{}", b),
                                    Value::Undefined => print!("undefined"),
                                    Value::Object(inner_obj) => {
                                        if crate::js_array::is_array(inner_obj) {
                                            print!("[Array]");
                                        } else {
                                            print!("[object Object]");
                                        }
                                    }
                                    _ => print!("[object Object]"),
                                }
                            }
                            print!("}}");
                        }
                    }
                    Value::Function(name) => print!("function {}() {{ [native code] }}", name),
                    Value::Closure(params, _, _) => {
                        print!("function(");
                        for (i, param) in params.iter().enumerate() {
                            if i > 0 {
                                print!(", ");
                            }
                            print!("{}", param);
                        }
                        print!(") {{ [closure code] }}");
                    }
                    Value::ClassDefinition(ref class_def) => print!("class {}", class_def.name),
                    Value::Getter(_, _) => print!("[Getter]"),
                    Value::Setter(_, _, _) => print!("[Setter]"),
                    Value::Property { value, getter, setter } => {
                        print!("[Property");
                        if value.is_some() {
                            print!(" value");
                        }
                        if getter.is_some() {
                            print!(" getter");
                        }
                        if setter.is_some() {
                            print!(" setter");
                        }
                        print!("]");
                    }
                    Value::Promise(_) => print!("[object Promise]"),
                    Value::Symbol(_) => print!("[object Symbol]"),
                }
            }
            println!();
            Ok(Value::Undefined)
        }
        _ => Err(JSError::EvaluationError {
            message: format!("Console method {method} not implemented"),
        }),
    }
}
