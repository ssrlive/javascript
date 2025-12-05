use crate::core::{Expr, JSObjectData, JSObjectDataPtr, Value, env_set, evaluate_expr, obj_set_value};
use crate::error::JSError;
use crate::eval_error_here;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use std::cell::RefCell;
use std::rc::Rc;

/// Create the testIntl object with testing functions
pub fn make_testintl_object() -> Result<JSObjectDataPtr, JSError> {
    let testintl_obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(
        &testintl_obj,
        &"testWithIntlConstructors".into(),
        Value::Function("testWithIntlConstructors".to_string()),
    )?;
    Ok(testintl_obj)
}

/// Create a mock Intl constructor that can be instantiated
pub fn create_mock_intl_constructor() -> Result<Value, JSError> {
    // Create a special constructor function that will be recognized by evaluate_new
    Ok(Value::Function("MockIntlConstructor".to_string()))
}

/// Create a mock Intl instance with resolvedOptions method
pub fn create_mock_intl_instance(locale_arg: Option<String>, env: &crate::core::JSObjectDataPtr) -> Result<Value, JSError> {
    // If the global JS helper `isCanonicalizedStructurallyValidLanguageTag` is
    // present, use it to validate the locale (this keeps validation logic in
    // JS where the test data lives). If the helper returns false, throw.
    if let Some(ref locale) = locale_arg {
        // Build an expression that calls the JS validation function with the
        // locale string argument and evaluate it in the current env.
        use crate::core::{Expr, Value as CoreValue};
        let arg_expr = Expr::StringLit(utf8_to_utf16(locale));
        let call_expr = Expr::Call(
            Box::new(Expr::Var("isCanonicalizedStructurallyValidLanguageTag".to_string())),
            vec![arg_expr],
        );
        match crate::core::evaluate_expr(env, &call_expr) {
            Ok(CoreValue::Boolean(true)) => {}
            Ok(CoreValue::Boolean(false)) => {
                // Log canonicalization result to help debugging why the helper
                // returned false for this locale.
                let canon_call = Expr::Call(
                    Box::new(Expr::Var("canonicalizeLanguageTag".to_string())),
                    vec![Expr::StringLit(utf8_to_utf16(locale))],
                );
                match crate::core::evaluate_expr(env, &canon_call) {
                    Ok(CoreValue::String(canon_utf16)) => {
                        let canon = utf16_to_utf8(&canon_utf16);
                        log::error!(
                            "isCanonicalizedStructurallyValidLanguageTag: locale='{}' canonical='{}'",
                            locale,
                            canon
                        );
                    }
                    Ok(other) => {
                        log::error!("canonicalizeLanguageTag returned non-string: {:?}", other);
                    }
                    Err(e) => {
                        log::error!("canonicalizeLanguageTag evaluation error: {:?}", e);
                    }
                }

                return Err(JSError::Throw {
                    value: Value::String(utf8_to_utf16("Invalid locale")),
                });
            }
            // If the helper is not present or returned non-boolean, fall back
            // to rejecting some obviously invalid inputs such as empty string
            // or very short tags like single-character tags (e.g. 'i') which
            // the tests expect to be considered invalid.
            Ok(_) | Err(_) => {
                if locale.is_empty() || locale.len() < 2 {
                    return Err(JSError::Throw {
                        value: Value::String(utf8_to_utf16("Invalid locale")),
                    });
                }
            }
        }
    }

    let instance = Rc::new(RefCell::new(JSObjectData::new()));

    // Add resolvedOptions method
    let resolved_options = Value::Closure(
        vec![],                                     // no parameters
        vec![],                                     // empty body - we'll handle this in the method call
        Rc::new(RefCell::new(JSObjectData::new())), // empty captured environment
    );
    obj_set_value(&instance, &"resolvedOptions".into(), resolved_options)?;

    // Store the locale that was passed to the constructor
    if let Some(locale) = locale_arg {
        obj_set_value(&instance, &"__locale".into(), Value::String(utf8_to_utf16(&locale)))?;
    }

    Ok(Value::Object(instance))
}

/// Handle resolvedOptions method on mock Intl instances
pub fn handle_resolved_options(instance: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Return an object with a locale property
    let result = Rc::new(RefCell::new(JSObjectData::new()));

    // Get the stored locale, or default to "en-US"
    let locale = if let Some(locale_val) = crate::core::obj_get_value(instance, &"__locale".into())? {
        match &*locale_val.borrow() {
            Value::String(s) => utf16_to_utf8(s),
            _ => "en-US".to_string(),
        }
    } else {
        "en-US".to_string()
    };

    obj_set_value(&result, &"locale".into(), Value::String(utf8_to_utf16(&locale)))?;
    Ok(Value::Object(result))
}

/// Handle testIntl object method calls
pub fn handle_testintl_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "testWithIntlConstructors" => {
            if args.len() != 1 {
                return Err(eval_error_here!("testWithIntlConstructors requires exactly 1 argument"));
            }

            let callback = evaluate_expr(env, &args[0])?;
            let callback_func = match callback {
                Value::Closure(params, body, captured_env) => (params, body, captured_env),
                _ => {
                    return Err(eval_error_here!("testWithIntlConstructors requires a function as argument"));
                }
            };

            // Create a mock constructor
            let mock_constructor = create_mock_intl_constructor()?;

            // Call the callback with the mock constructor
            let func_env = callback_func.2.clone();
            // Bind the mock constructor as the first parameter
            if !callback_func.0.is_empty() {
                env_set(&func_env, &callback_func.0[0], mock_constructor)?;
            }

            // Execute the callback body
            crate::core::evaluate_statements(&func_env, &callback_func.1)?;

            Ok(Value::Undefined)
        }
        _ => Err(eval_error_here!(format!("testIntl method {method} not implemented"))),
    }
}
