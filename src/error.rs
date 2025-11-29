#[derive(thiserror::Error, Debug)]
pub enum JSError {
    #[error("Tokenization failed")]
    TokenizationError,

    #[error("Parsing failed")]
    ParseError,

    #[error("Evaluation failed: {message}")]
    EvaluationError { message: String },

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
