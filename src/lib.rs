#![doc = include_str!("../README.md")]

pub(crate) mod core;
#[macro_use]
pub(crate) mod error;
pub(crate) mod js_array;
pub(crate) mod js_assert;
pub(crate) mod js_bigint;
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
pub(crate) mod js_os;
pub(crate) mod js_promise;
pub(crate) mod js_proxy;
pub(crate) mod js_reflect;
pub(crate) mod js_regexp;
pub(crate) mod js_set;
pub(crate) mod js_std;
pub(crate) mod js_string;
pub(crate) mod js_testintl;
pub(crate) mod js_typedarray;
pub(crate) mod js_weakmap;
pub(crate) mod js_weakset;
pub(crate) mod repl;
pub(crate) mod sprintf;
pub(crate) mod tmpfile;
pub(crate) mod unicode;

pub use core::{JSArrayBuffer, JSDataView, JSTypedArray, TypedArrayKind};
pub use core::{
    JSObjectData, Token, initialize_global_constructors, parse_expression, parse_object_destructuring_pattern, parse_statement,
    parse_statements,
};
pub use core::{PropertyKey, Value, evaluate_script, get_prop_env, obj_get_value, tokenize};
pub use error::{JSError, JSErrorKind};
pub use repl::Repl;
