#![doc = include_str!("../README.md")]

pub(crate) mod core;

#[macro_use]
pub(crate) mod error;
pub(crate) mod js_agent;
pub(crate) mod js_bigint;
pub(crate) mod js_regexp;
#[cfg(feature = "std")]
pub(crate) mod js_std;
pub(crate) mod repl;
pub(crate) mod unicode;

pub use crate::core::{Token, TokenData};
pub use core::{
    JSArrayBuffer, JSDataView, JSObjectData, JSTypedArray, TypedArrayKind, new_js_object_data, parse_object_destructuring_pattern,
    parse_simple_expression, parse_statement, parse_statements, read_script_file,
};
pub use core::{
    PropertyKey, Value, env_set, evaluate_script_with_vm, format_js_number, object_get_key_value, object_set_key_value, tokenize,
};
pub use error::{JSError, JSErrorKind};
// pub use js_promise::set_short_timer_threshold_ms;
// pub use js_promise::set_wait_for_active_handles;
pub use repl::Repl;
pub use unicode::{utf8_to_utf16, utf16_to_utf8};
