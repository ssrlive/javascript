#![doc = include_str!("../README.md")]

pub(crate) mod core;
#[macro_use]
pub(crate) mod error;
pub(crate) mod js_array;
pub(crate) mod js_async;
// pub(crate) mod js_assert;
pub(crate) mod js_bigint;
pub(crate) mod js_boolean;
pub(crate) mod js_class;
pub(crate) mod js_console;
pub(crate) mod js_date;
pub(crate) mod js_function;
pub(crate) mod js_generator;
pub(crate) mod js_json;
pub(crate) mod js_map;
pub(crate) mod js_math;
pub(crate) mod js_module;
pub(crate) mod js_number;
pub(crate) mod js_object;
#[cfg(feature = "os")]
pub(crate) mod js_os;
pub(crate) mod js_promise;
pub(crate) mod js_proxy;
pub(crate) mod js_reflect;
pub(crate) mod js_regexp;
pub(crate) mod js_set;
#[cfg(feature = "std")]
pub(crate) mod js_std;
pub(crate) mod js_string;
pub(crate) mod js_symbol;
pub(crate) mod timer_thread;
// pub(crate) mod js_testintl;
pub(crate) mod js_typedarray;
pub(crate) mod js_weakmap;
pub(crate) mod js_weakset;
pub(crate) mod repl;
pub(crate) mod unicode;

pub use crate::core::{Token, TokenData};
pub use core::{
    JSArrayBuffer, JSDataView, JSObjectData, JSTypedArray, JsArena, TypedArrayKind, initialize_global_constructors, new_js_object_data,
    parse_object_destructuring_pattern, parse_simple_expression, parse_statement, parse_statements, read_script_file,
};
pub use core::{PropertyKey, Value, env_set, evaluate_script, format_js_number, object_get_key_value, object_set_key_value, tokenize};
pub use error::{JSError, JSErrorKind};
pub use js_promise::set_short_timer_threshold_ms;
pub use js_promise::set_wait_for_active_handles;
pub use repl::Repl;
pub use unicode::{utf8_to_utf16, utf16_to_utf8};
