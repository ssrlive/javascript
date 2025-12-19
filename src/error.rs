#[derive(thiserror::Error, Debug)]
pub enum JSErrorKind {
    #[error("Tokenization failed")]
    TokenizationError,

    #[error("Parsing failed: {message}")]
    ParseError { message: String },

    #[error("Evaluation failed: {message}")]
    EvaluationError { message: String },

    #[error("Infinite loop detected (executed {iterations} iterations)")]
    InfiniteLoopError { iterations: usize },

    #[error("Variable '{name}' not found")]
    VariableNotFound { name: String },

    #[error("Type error: {message}")]
    TypeError { message: String },

    #[error("Range error: {message}")]
    RangeError { message: String },

    #[error("Syntax error: {message}")]
    SyntaxError { message: String },

    #[error("Runtime error: {message}")]
    RuntimeError { message: String },

    #[error("Thrown value")]
    Throw { value: crate::core::Value },

    #[error("std::io error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Debug)]
pub struct JSErrorData {
    pub kind: JSErrorKind,
    pub file: String,
    pub line: usize,
    pub method: String,
    pub js_line: Option<usize>,
    pub js_column: Option<usize>,
}

#[derive(Debug)]
pub struct JSError {
    pub inner: Box<JSErrorData>,
}

impl JSError {
    // Provide a constructor for use by macros
    pub fn new(kind: JSErrorKind, file: String, line: usize, method: String) -> Self {
        Self {
            inner: Box::new(JSErrorData {
                kind,
                file,
                line,
                method,
                js_line: None,
                js_column: None,
            }),
        }
    }

    pub fn set_js_location(&mut self, line: usize, column: usize) {
        self.inner.js_line = Some(line);
        self.inner.js_column = Some(column);
    }

    pub fn js_line(&self) -> Option<usize> {
        self.inner.js_line
    }

    pub fn js_column(&self) -> Option<usize> {
        self.inner.js_column
    }

    // convenience method to access the kind
    pub fn kind(&self) -> &JSErrorKind {
        &self.inner.kind
    }

    /// Get a user-friendly error message without internal Rust debugging details
    pub fn user_message(&self) -> String {
        let msg = match &self.inner.kind {
            JSErrorKind::TokenizationError => "SyntaxError: Failed to parse input".to_string(),
            JSErrorKind::ParseError { message } => format!("SyntaxError: {}", message),
            JSErrorKind::EvaluationError { message } => {
                if message == "error" {
                    "Error: An error occurred during evaluation".to_string()
                } else {
                    format!("Error: {}", message)
                }
            }
            JSErrorKind::InfiniteLoopError { iterations } => {
                format!("Error: Infinite loop detected (executed {} iterations)", iterations)
            }
            JSErrorKind::VariableNotFound { name } => {
                format!("ReferenceError: '{}' is not defined", name)
            }
            JSErrorKind::TypeError { message } => format!("TypeError: {}", message),
            JSErrorKind::RangeError { message } => format!("RangeError: {}", message),
            JSErrorKind::SyntaxError { message } => format!("SyntaxError: {}", message),
            JSErrorKind::RuntimeError { message } => format!("Error: {}", message),
            JSErrorKind::Throw { value } => {
                // If the thrown value is an object, prefer a human-friendly
                // message that includes the constructor name and/or message
                // property instead of the generic [object Object] string.
                let mut result = None;
                if let crate::core::Value::Object(obj) = value {
                    // Try constructor.name
                    if let Ok(Some(ctor_val_rc)) = crate::core::obj_get_key_value(obj, &"constructor".into())
                        && let crate::core::Value::Object(ctor_obj) = &*ctor_val_rc.borrow()
                        && let Ok(Some(name_val_rc)) = crate::core::obj_get_key_value(ctor_obj, &"name".into())
                        && let crate::core::Value::String(name_utf16) = &*name_val_rc.borrow()
                    {
                        let ctor_name = crate::unicode::utf16_to_utf8(name_utf16);
                        // prefer a message property if present
                        if let Ok(Some(msg_val_rc)) = crate::core::obj_get_key_value(obj, &"message".into())
                            && let crate::core::Value::String(msg_utf16) = &*msg_val_rc.borrow()
                        {
                            let msg = crate::unicode::utf16_to_utf8(msg_utf16);
                            result = Some(format!("Uncaught {}: {}", ctor_name, msg));
                        } else {
                            result = Some(format!("Uncaught {}", ctor_name));
                        }
                    }
                    // Fallback: if object has a message property, use it
                    if result.is_none()
                        && let Ok(Some(msg_val_rc)) = crate::core::obj_get_key_value(obj, &"message".into())
                        && let crate::core::Value::String(msg_utf16) = &*msg_val_rc.borrow()
                    {
                        let msg = crate::unicode::utf16_to_utf8(msg_utf16);
                        result = Some(format!("Uncaught {}", msg));
                    }
                }
                result.unwrap_or_else(|| format!("Uncaught {}", crate::core::value_to_string(value)))
            }
            JSErrorKind::IoError(e) => format!("IOError: {}", e),
        };

        if let Some(line) = self.inner.js_line {
            if let Some(col) = self.inner.js_column {
                format!("{} at line {}:{}", msg, line, col)
            } else {
                format!("{} at line {}", msg, line)
            }
        } else {
            msg
        }
    }
}

// So that all errors automatically get the format "Error: ... at method file:line"
impl std::fmt::Display for JSError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let data = &self.inner;
        write!(f, "{} at {} {}:{}", data.kind, data.method, data.file, data.line)
    }
}

// Let JSError be used as a standard Error trait
impl std::error::Error for JSError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.inner.kind.source()
    }
}

// Allow direct conversion from std::io::Error to JSError, but since we lack context,
// we fill in "<unknown>" for file and method, and set line to 0
impl From<std::io::Error> for JSError {
    fn from(err: std::io::Error) -> Self {
        JSError::new(JSErrorKind::IoError(err), "<unknown>".to_string(), 0, "<unknown>".to_string())
    }
}

// Helper macro to get the current function name
#[macro_export]
#[doc(hidden)]
macro_rules! function_name {
    () => {{
        fn f() {}
        fn type_name_of<T>(_: T) -> &'static str {
            std::any::type_name::<T>()
        }
        let name = type_name_of(f);
        &name[..name.len() - 3]
    }};
}

// Kernel macro: this is the base for all specific error macros
// It takes a JSErrorKind and auto-fills file, line, method
#[macro_export]
#[doc(hidden)]
macro_rules! make_js_error {
    ($kind:expr) => {
        $crate::JSError::new($kind, file!().to_string(), line!() as usize, $crate::function_name!().to_string())
    };
}

// --- These macros use make_js_error! to create specific error types ---

#[macro_export]
#[doc(hidden)]
macro_rules! raise_tokenize_error {
    () => {
        $crate::make_js_error!($crate::JSErrorKind::TokenizationError)
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! raise_parse_error {
    () => {
        $crate::make_js_error!($crate::JSErrorKind::ParseError {
            message: "parse error".to_string()
        })
    };
    ($msg:expr) => {
        $crate::make_js_error!($crate::JSErrorKind::ParseError { message: $msg.to_string() })
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! raise_parse_error_with_token {
    ($token:expr, $msg:expr) => {{
        let mut err = $crate::make_js_error!($crate::JSErrorKind::ParseError { message: $msg.to_string() });
        err.set_js_location($token.line, $token.column);
        err
    }};
    ($token:expr) => {{
        let mut err = $crate::make_js_error!($crate::JSErrorKind::ParseError {
            message: "Unexpected token".to_string()
        });
        err.set_js_location($token.line, $token.column);
        err
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! raise_eval_error {
    ($msg:expr) => {
        $crate::make_js_error!($crate::JSErrorKind::EvaluationError { message: $msg.to_string() })
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! raise_infinite_loop_error {
    ($iterations:expr) => {
        $crate::make_js_error!($crate::JSErrorKind::InfiniteLoopError { iterations: $iterations })
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! raise_variable_not_found_error {
    ($name:expr) => {
        $crate::make_js_error!($crate::JSErrorKind::VariableNotFound { name: $name.to_string() })
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! raise_type_error {
    ($msg:expr) => {
        $crate::make_js_error!($crate::JSErrorKind::TypeError { message: $msg.to_string() })
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! raise_range_error {
    ($msg:expr) => {
        $crate::make_js_error!($crate::JSErrorKind::RangeError { message: $msg.to_string() })
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! raise_syntax_error {
    ($msg:expr) => {
        $crate::make_js_error!($crate::JSErrorKind::SyntaxError { message: $msg.to_string() })
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! raise_runtime_error {
    ($msg:expr) => {
        $crate::make_js_error!($crate::JSErrorKind::RuntimeError { message: $msg.to_string() })
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! raise_throw_error {
    ($value:expr) => {
        $crate::make_js_error!($crate::JSErrorKind::Throw { value: $value })
    };
}
