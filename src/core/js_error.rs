use crate::{
    JSError,
    core::{InternalSlot, PropertyKey, Value, value_to_string},
    object_get_key_value, utf16_to_utf8,
};

#[derive(Debug)]
pub enum EvalError<'gc> {
    Js(JSError),
    Throw(Value<'gc>, Option<usize>, Option<usize>),
}

impl<'gc> From<JSError> for EvalError<'gc> {
    fn from(e: JSError) -> Self {
        EvalError::Js(e)
    }
}

impl<'gc> From<EvalError<'gc>> for JSError {
    fn from(e: EvalError<'gc>) -> Self {
        match e {
            EvalError::Js(j) => j,
            EvalError::Throw(v, line, column) => {
                let msg = value_to_string(&v);
                let mut mapped_kind = None;
                if let Value::Object(obj) = &v
                    && let Some(name_rc) = object_get_key_value(obj, "name")
                    && let Value::String(name_u16) = &*name_rc.borrow()
                {
                    let name = utf16_to_utf8(name_u16);
                    let message = if let Some(message_rc) = object_get_key_value(obj, "message") {
                        match &*message_rc.borrow() {
                            Value::String(m) => utf16_to_utf8(m),
                            other => value_to_string(other),
                        }
                    } else {
                        msg.clone()
                    };

                    mapped_kind = match name.as_str() {
                        "TypeError" => Some(crate::error::JSErrorKind::TypeError { message }),
                        "RangeError" => Some(crate::error::JSErrorKind::RangeError { message }),
                        "SyntaxError" => Some(crate::error::JSErrorKind::SyntaxError { message }),
                        "ReferenceError" => Some(crate::error::JSErrorKind::ReferenceError { message }),
                        "EvalError" | "URIError" => Some(crate::error::JSErrorKind::EvaluationError { message }),
                        _ => None,
                    };
                }

                let mut e = if let Some(kind) = mapped_kind {
                    crate::make_js_error!(kind)
                } else {
                    crate::make_js_error!(crate::error::JSErrorKind::Throw(msg))
                };
                e.inner.js_line = line;
                e.inner.js_column = column;
                e
            }
        }
    }
}

impl<'gc> EvalError<'gc> {
    pub fn message(&self) -> String {
        match self {
            EvalError::Js(e) => e.message(),
            EvalError::Throw(v, ..) => value_to_string(v),
        }
    }
}

/// Check if a value is an Error object.
pub fn is_error<'gc>(val: &Value<'gc>) -> bool {
    if let Value::Object(obj) = val
        && let Ok(borrowed) = obj.try_borrow()
    {
        return borrowed.properties.contains_key(&PropertyKey::Internal(InternalSlot::IsError));
    }
    false
}
