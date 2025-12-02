use crate::{
    JSError,
    core::{
        JSObjectData, JSObjectDataPtr, PropertyKey, Value, evaluate_statements, filter_input_script, initialize_global_constructors,
        obj_get_value, obj_set_value, parse_statements, tokenize, value_to_string,
    },
    js_promise::{PromiseState, run_event_loop},
};
use std::{cell::RefCell, rc::Rc};

/// A small persistent REPL environment wrapper.
///
/// Notes:
/// - `Repl::new()` creates a persistent environment and initializes built-ins.
/// - `Repl::eval(&self, code)` evaluates the provided code in the persistent env
///   so variables, functions and imports persist between calls.
pub struct Repl {
    env: JSObjectDataPtr,
}

impl Default for Repl {
    fn default() -> Self {
        Self::new()
    }
}

impl Repl {
    /// Create a new persistent REPL environment (with built-ins initialized).
    pub fn new() -> Self {
        let env: JSObjectDataPtr = Rc::new(RefCell::new(JSObjectData::new()));
        env.borrow_mut().is_function_scope = true;
        // Initialize built-in constructors once for the persistent environment
        initialize_global_constructors(&env);
        Repl { env }
    }

    /// Evaluate a script in the persistent environment.
    /// Returns the evaluation result or an error.
    pub fn eval<T: AsRef<str>>(&self, script: T) -> Result<Value, JSError> {
        let script = script.as_ref();
        let filtered = filter_input_script(script);

        // Parse tokens and statements
        let mut tokens = tokenize(&filtered)?;
        let statements = parse_statements(&mut tokens)?;

        // Inject simple host `std` / `os` shims when importing with the pattern:
        //   import * as NAME from "std";
        for line in script.lines() {
            let l = line.trim();
            if l.starts_with("import * as")
                && l.contains("from")
                && let (Some(as_idx), Some(from_idx)) = (l.find("as"), l.find("from"))
            {
                let name_part = &l[as_idx + 2..from_idx].trim();
                let name = PropertyKey::String(name_part.trim().to_string());
                if let Some(start_quote) = l[from_idx..].find(|c: char| ['"', '\''].contains(&c)) {
                    let quote_char = l[from_idx + start_quote..].chars().next().unwrap();
                    let rest = &l[from_idx + start_quote + 1..];
                    if let Some(end_quote) = rest.find(quote_char) {
                        let module = &rest[..end_quote];
                        if module == "std" {
                            obj_set_value(&self.env, &name, Value::Object(crate::js_std::make_std_object()?))?;
                        } else if module == "os" {
                            obj_set_value(&self.env, &name, Value::Object(crate::js_os::make_os_object()?))?;
                        }
                    }
                }
            }
        }

        match evaluate_statements(&self.env, &statements) {
            Ok(v) => {
                // If the result is a Promise object (wrapped in Object with __promise property), wait for it to resolve
                if let Value::Object(obj) = &v
                    && let Some(promise_val_rc) = obj_get_value(obj, &"__promise".into())?
                    && let Value::Promise(promise) = &*promise_val_rc.borrow()
                {
                    // Run the event loop until the promise is resolved
                    loop {
                        run_event_loop()?;
                        let promise_borrow = promise.borrow();
                        match &promise_borrow.state {
                            PromiseState::Fulfilled(val) => return Ok(val.clone()),
                            PromiseState::Rejected(reason) => {
                                return Err(JSError::EvaluationError {
                                    message: format!("Promise rejected: {}", value_to_string(reason)),
                                });
                            }
                            PromiseState::Pending => {
                                // Continue running the event loop
                            }
                        }
                    }
                }
                // Run event loop once to process any queued asynchronous tasks
                run_event_loop()?;
                Ok(v)
            }
            Err(e) => Err(e),
        }
    }
}
