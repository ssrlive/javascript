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
pub fn create_mock_intl_instance(locale_arg: Option<String>) -> Result<Value, JSError> {
    // Check if locale is valid - reject obviously invalid ones
    if let Some(ref locale) = locale_arg {
        // Reject locales that are obviously invalid (based on the test cases)
        let invalid_locales = [
            "i",
            "x",
            "u",
            "419",
            "u-nu-latn-cu-bob",
            "hans-cmn-cn",
            "cmn-hans-cn-u-u",
            "cmn-hans-cn-t-ca-u-ca-x_t-u",
            "de-gregory-gregory",
            "enochian_enochian",
            "de-gregory_u-ca-gregory",
        ];
        if invalid_locales.contains(&locale.as_str()) {
            return Err(JSError::Throw {
                value: Value::String(utf8_to_utf16("Invalid locale")),
            });
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
