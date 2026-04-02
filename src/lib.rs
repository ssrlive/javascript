#![doc = include_str!("../README.md")]

pub(crate) mod core;

#[macro_use]
pub(crate) mod error;
pub(crate) mod js_agent;
pub(crate) mod js_bigint;
#[cfg(feature = "std")]
pub(crate) mod js_std;
pub(crate) mod repl;
pub(crate) mod unicode;

pub use crate::core::{Token, TokenData};
pub use core::{PropertyKey, Value, evaluate_script_with_vm, format_js_number, tokenize};
pub use core::{parse_object_destructuring_pattern, parse_simple_expression, parse_statement, parse_statements, read_script_file};
pub use error::{JSError, JSErrorKind};
// pub use js_promise::set_short_timer_threshold_ms;
// pub use js_promise::set_wait_for_active_handles;
pub use repl::Repl;
pub use unicode::{utf8_to_utf16, utf16_to_utf8};
