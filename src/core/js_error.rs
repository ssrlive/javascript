use crate::{
    JSError,
    core::{Value, value_to_string},
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

                let mut e = crate::make_js_error!(crate::error::JSErrorKind::Throw(msg));
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
