use thiserror::Error;

#[derive(Error, Debug)]
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
}
