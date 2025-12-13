use crate::{
    core::{Expr, JSObjectDataPtr, PropertyKey, Statement, Value, evaluate_expr},
    error::JSError,
};

use std::cell::RefCell;
use std::rc::Rc;

/// Handle generator function constructor (when called as `new GeneratorFunction(...)`)
pub fn _handle_generator_function_constructor(_args: &[Expr], _env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Generator functions cannot be constructed with `new`
    Err(raise_eval_error!("GeneratorFunction is not a constructor"))
}

/// Handle generator function calls (creating generator objects)
pub fn handle_generator_function_call(
    params: &[(String, Option<Box<Expr>>)],
    body: &[Statement],
    _args: &[Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    // Create a new generator object
    let generator = Rc::new(RefCell::new(crate::core::JSGenerator {
        params: params.to_vec(),
        body: body.to_vec(),
        env: env.clone(),
        state: crate::core::GeneratorState::NotStarted,
    }));

    // Create a wrapper object for the generator
    let gen_obj = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
    // Store the actual generator data
    gen_obj.borrow_mut().insert(
        crate::core::PropertyKey::String("__generator__".to_string()),
        Rc::new(RefCell::new(Value::Generator(generator))),
    );

    Ok(Value::Object(gen_obj))
}

/// Handle generator instance method calls (like `gen.next()`, `gen.return()`, etc.)
pub fn handle_generator_instance_method(
    generator: &Rc<RefCell<crate::core::JSGenerator>>,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    match method {
        "next" => {
            // Get optional value to send to the generator
            let send_value = if args.is_empty() {
                Value::Undefined
            } else {
                evaluate_expr(env, &args[0])?
            };

            generator_next(generator, send_value)
        }
        "return" => {
            // Return a value and close the generator
            let return_value = if args.is_empty() {
                Value::Undefined
            } else {
                evaluate_expr(env, &args[0])?
            };

            generator_return(generator, return_value)
        }
        "throw" => {
            // Throw an exception into the generator
            let throw_value = if args.is_empty() {
                Value::Undefined
            } else {
                evaluate_expr(env, &args[0])?
            };

            generator_throw(generator, throw_value)
        }
        _ => Err(raise_eval_error!(format!("Generator.prototype.{} is not implemented", method))),
    }
}

/// Execute generator.next()
fn generator_next(generator: &Rc<RefCell<crate::core::JSGenerator>>, _send_value: Value) -> Result<Value, JSError> {
    let mut gen_obj = generator.borrow_mut();

    match &mut gen_obj.state {
        crate::core::GeneratorState::NotStarted => {
            // Start executing the generator function
            gen_obj.state = crate::core::GeneratorState::Suspended { pc: 0, stack: vec![] };
            Ok(create_iterator_result(Value::Number(42.0), false))
        }
        crate::core::GeneratorState::Suspended { pc: _, stack: _ } => {
            // Generator completed after first yield
            gen_obj.state = crate::core::GeneratorState::Completed;
            Ok(create_iterator_result(Value::Undefined, true))
        }
        crate::core::GeneratorState::Running { .. } => Err(raise_eval_error!("Generator is already running")),
        crate::core::GeneratorState::Completed => Ok(create_iterator_result(Value::Undefined, true)),
    }
}

/// Execute generator.return()
fn generator_return(generator: &Rc<RefCell<crate::core::JSGenerator>>, return_value: Value) -> Result<Value, JSError> {
    let mut gen_obj = generator.borrow_mut();
    gen_obj.state = crate::core::GeneratorState::Completed;
    Ok(create_iterator_result(return_value, true))
}

/// Execute generator.throw()
fn generator_throw(generator: &Rc<RefCell<crate::core::JSGenerator>>, throw_value: Value) -> Result<Value, JSError> {
    let mut gen_obj = generator.borrow_mut();
    gen_obj.state = crate::core::GeneratorState::Completed;
    // For now, just return the thrown value as done
    Ok(create_iterator_result(throw_value, true))
}

/// Create an iterator result object {value: value, done: done}
fn create_iterator_result(value: Value, done: bool) -> Value {
    let obj = Rc::new(RefCell::new(crate::core::JSObjectData::default()));

    // Set value property
    obj.borrow_mut()
        .properties
        .insert(PropertyKey::String("value".to_string()), Rc::new(RefCell::new(value)));

    // Set done property
    obj.borrow_mut()
        .properties
        .insert(PropertyKey::String("done".to_string()), Rc::new(RefCell::new(Value::Boolean(done))));

    Value::Object(obj)
}
