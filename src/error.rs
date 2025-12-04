#[derive(thiserror::Error, Debug)]
pub enum JSError {
    #[error("Tokenization failed")]
    TokenizationError,

    #[error("Parsing failed at {method} {file}:{line}")]
    ParseError { file: String, line: usize, method: String },

    #[error("Evaluation failed at {method} {file}:{line}: {message}")]
    EvaluationError {
        message: String,
        file: String,
        line: usize,
        method: String,
    },

    #[error("Infinite loop detected (executed {iterations} iterations)")]
    InfiniteLoopError { iterations: usize },

    #[error("Variable '{name}' not found")]
    VariableNotFound { name: String },

    #[error("Type error: {message}")]
    TypeError { message: String },

    #[error("Syntax error: {message}")]
    SyntaxError { message: String },

    #[error("Runtime error: {message}")]
    RuntimeError { message: String },

    #[error("Thrown value: {value:?}")]
    Throw { value: crate::core::Value },

    #[error("std::io error: {0}")]
    IoError(#[from] std::io::Error),
}

impl From<JSError> for std::io::Error {
    fn from(err: JSError) -> std::io::Error {
        match err {
            JSError::IoError(io_err) => io_err,
            _ => std::io::Error::other(err.to_string()),
        }
    }
}

// Macro that constructs a ParseError using the compile-time caller
// location. Using a macro (rather than a function) ensures `file!()` and
// `line!()` expand to the site where the macro is invoked.
#[macro_export]
macro_rules! parse_error_here {
    () => {
        $crate::JSError::ParseError {
            file: file!().to_string(),
            line: line!() as usize,
            method: $crate::function_name!().to_string(),
        }
    };
}

// Macro that constructs an EvaluationError using the compile-time caller
// location and the provided message. Using a macro (rather than a
// function) ensures `file!()` and `line!()` expand to the site where the
// macro is invoked.
#[macro_export]
macro_rules! eval_error_here {
    ($msg:expr) => {
        $crate::JSError::EvaluationError {
            message: $msg.to_string(),
            file: file!().to_string(),
            line: line!() as usize,
            method: $crate::function_name!().to_string(),
        }
    };
}

#[macro_export]
macro_rules! function_name {
    () => {{
        fn f() {}
        fn type_name_of<T>(_: T) -> &'static str {
            std::any::type_name::<T>()
        }
        let name = type_name_of(f);
        // remove the trailing "::f"
        &name[..name.len() - 3]
    }};
}
