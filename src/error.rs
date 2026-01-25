#[derive(thiserror::Error, Debug)]
pub enum JSErrorKind {
    #[error("Tokenization failed: {message}")]
    TokenizationError { message: String },

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

    #[error("Reference error: {message}")]
    ReferenceError { message: String },

    #[error("Runtime error: {message}")]
    RuntimeError { message: String },

    #[error("Thrown value: {0}")]
    Throw(String),

    #[error("std::io error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("std::num::ParseIntError: {0}")]
    ParseIntError(#[from] std::num::ParseIntError),

    #[error("std::num::ParseFloatError: {0}")]
    ParseFloatError(#[from] std::num::ParseFloatError),
}

#[derive(Debug)]
pub struct JSErrorData {
    pub kind: JSErrorKind,
    pub rust_file: String,
    pub rust_line: Option<usize>,
    pub rust_method: String,
    pub js_file: String,
    pub js_method: String,
    pub js_line: Option<usize>,
    pub js_column: Option<usize>,
    pub stack: Vec<String>,
}

#[derive(Debug)]
pub struct JSError {
    pub inner: Box<JSErrorData>,
}

impl JSError {
    // Provide a constructor for use by macros
    pub fn new(kind: JSErrorKind, rust_file: String, rust_method: String, rust_line: Option<usize>) -> Self {
        Self {
            inner: Box::new(JSErrorData {
                kind,
                rust_file,
                rust_method,
                rust_line,
                js_file: "<anonymous>".to_string(),
                js_method: "<anonymous>".to_string(),
                js_line: None,
                js_column: None,
                stack: Vec::new(),
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

    pub fn set_stack(&mut self, stack: Vec<String>) {
        self.inner.stack = stack;
    }

    pub fn stack(&self) -> &Vec<String> {
        &self.inner.stack
    }

    // convenience method to access the kind
    pub fn kind(&self) -> &JSErrorKind {
        &self.inner.kind
    }

    /// Get the error message without location information
    pub fn message(&self) -> String {
        match &self.inner.kind {
            JSErrorKind::TokenizationError { message } => format!("SyntaxError: {message}"),
            JSErrorKind::ParseError { message } => format!("SyntaxError: {message}"),
            JSErrorKind::EvaluationError { message } => {
                if message == "error" {
                    "Error: An error occurred during evaluation".to_string()
                } else {
                    format!("Error: {message}")
                }
            }
            JSErrorKind::InfiniteLoopError { iterations } => format!("Error: Infinite loop detected (executed {iterations} iterations)"),
            JSErrorKind::VariableNotFound { name } => format!("ReferenceError: '{name}' is not defined"),
            JSErrorKind::TypeError { message } => format!("TypeError: {message}"),
            JSErrorKind::RangeError { message } => format!("RangeError: {message}"),
            JSErrorKind::SyntaxError { message } => format!("SyntaxError: {message}"),
            JSErrorKind::ReferenceError { message } => format!("ReferenceError: {message}"),
            JSErrorKind::RuntimeError { message } => format!("Error: {message}"),
            JSErrorKind::Throw(msg) => msg.clone(),
            JSErrorKind::IoError(e) => format!("IOError: {e}"),
            JSErrorKind::ParseIntError(e) => format!("ParseIntError: {e}"),
            JSErrorKind::ParseFloatError(e) => format!("ParseFloatError: {e}"),
        }
    }

    /// Get a user-friendly error message without internal Rust debugging details
    pub fn user_message(&self) -> String {
        let msg = self.message();

        let mut extra = String::new();
        if let Some(js_line) = self.inner.js_line {
            extra.push_str(&format!(" at line {js_line}"));
            if let Some(js_column) = self.inner.js_column {
                extra.push_str(&format!(":{js_column}"));
            }
        }
        msg + &extra
    }

    pub fn native_message(&self) -> String {
        let line = self.inner.rust_line.map_or("".to_string(), |line| format!(":{line}"));
        format!("{} {}{}", self.inner.rust_method, self.inner.rust_file, line)
    }
}

// So that all errors automatically get the format "Error: ... at method file:line"
impl std::fmt::Display for JSError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let data = &self.inner;
        let native_location = self.native_message();
        let mut extra = String::new();
        if let Some(js_line) = data.js_line {
            extra.push_str(&format!(":{js_line}"));
            if let Some(js_column) = data.js_column {
                extra.push_str(&format!(":{js_column}"));
            }
        }
        write!(
            f,
            "{} at {} {}{}\nOccurred at native code {}",
            data.kind, data.js_method, data.js_file, extra, native_location
        )
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
        JSError::new(JSErrorKind::IoError(err), "<unknown>".to_string(), "<unknown>".to_string(), None)
    }
}

impl From<std::num::ParseIntError> for JSError {
    fn from(err: std::num::ParseIntError) -> Self {
        JSError::new(
            JSErrorKind::ParseIntError(err),
            "<unknown>".to_string(),
            "<unknown>".to_string(),
            None,
        )
    }
}

impl From<std::num::ParseFloatError> for JSError {
    fn from(err: std::num::ParseFloatError) -> Self {
        JSError::new(
            JSErrorKind::ParseFloatError(err),
            "<unknown>".to_string(),
            "<unknown>".to_string(),
            None,
        )
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
        $crate::JSError::new(
            $kind,
            file!().to_string(),
            $crate::function_name!().to_string(),
            Some(line!() as usize),
        )
    };
}

// --- These macros use make_js_error! to create specific error types ---

#[macro_export]
#[doc(hidden)]
macro_rules! raise_tokenize_error {
    ($msg:expr, $line:expr, $col:expr) => {{
        let mut err = $crate::make_js_error!($crate::JSErrorKind::TokenizationError { message: $msg.to_string() });
        err.set_js_location($line, $col);
        err
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! raise_parse_error {
    ($msg:expr, $line:expr, $col:expr) => {{
        let mut err = $crate::make_js_error!($crate::JSErrorKind::ParseError { message: $msg.to_string() });
        err.set_js_location($line, $col);
        err
    }};
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
macro_rules! raise_parse_error_at {
    ($token_opt:expr) => {
        match $token_opt {
            Some(tok) => $crate::raise_parse_error_with_token!(tok),
            None => $crate::raise_parse_error!("parse error".to_string()),
        }
    };
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
macro_rules! raise_reference_error {
    ($msg:expr) => {
        $crate::make_js_error!($crate::JSErrorKind::ReferenceError { message: $msg.to_string() })
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
        $crate::make_js_error!($crate::JSErrorKind::Throw($crate::core::value_to_string(&$value)))
    };
}
