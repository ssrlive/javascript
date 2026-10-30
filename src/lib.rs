#![doc = include_str!("../README.md")]

pub(crate) mod core;

#[macro_use]
pub(crate) mod error;
pub(crate) mod host_api;
pub(crate) mod js_agent;
pub(crate) mod repl;
pub mod runtime;
pub(crate) mod unicode;

pub use crate::core::{Token, TokenData};
pub use core::{DEFAULT_GC_OPCODE_BUDGET, Value, format_js_number, tokenize};
pub use core::{parse_object_destructuring_pattern, parse_simple_expression, parse_statement, parse_statements, read_script_file};
pub use error::{JSError, JSErrorKind};
// pub use js_promise::set_short_timer_threshold_ms;
// pub use js_promise::set_wait_for_active_handles;
pub use repl::Repl;
#[cfg(feature = "jit")]
pub use runtime::evaluate_script_with_jit_required;
#[cfg(feature = "console")]
pub use runtime::{ConsoleLevel, ConsoleMessage};
pub use runtime::{HostHooks, JsObjectHandle, JsPromiseResolver, JsRunStatus, JsRuntime, JsValue, ModuleLoader, ModuleSource};
pub use runtime::{evaluate_script, evaluate_script_with_unwrap};
pub use unicode::{utf8_to_utf16, utf16_to_utf8};
