#![doc = include_str!("../README.md")]

// pub mod cutils;
// pub mod libregexp;
// pub mod libunicode;
// pub mod libunicode_table;
pub(crate) mod core;
#[macro_use]
pub(crate) mod error;
pub(crate) mod js_array;
pub(crate) mod js_assert;
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

pub use core::{
    JS_DefinePropertyValue, JS_DupValue, JS_Eval, JS_FreeContext, JS_FreeRuntime, JS_FreeValue, JS_GetProperty, JS_NewContext,
    JS_NewObject, JS_NewRuntime, JS_NewString, JS_SetProperty, JSClassDef, JSObject, JSStackFrame, JSString, JSValue, PropertyKey, Value,
    evaluate_script, get_prop_env, obj_get_value, tokenize,
};
pub use core::{
    JS_FLOAT64_NAN, JS_GC_OBJ_TYPE_ASYNC_FUNCTION, JS_GC_OBJ_TYPE_FUNCTION_BYTECODE, JS_GC_OBJ_TYPE_JS_CONTEXT, JS_GC_OBJ_TYPE_JS_OBJECT,
    JS_GC_OBJ_TYPE_SHAPE, JS_GC_OBJ_TYPE_VAR_REF, JS_TAG_BOOL, JS_TAG_CATCH_OFFSET, JS_TAG_FLOAT64, JS_TAG_INT, JS_TAG_NULL, JS_TAG_OBJECT,
    JS_TAG_SHORT_BIG_INT, JS_TAG_STRING, JS_TAG_STRING_ROPE, JS_TAG_UNDEFINED, JS_UNINITIALIZED,
};
pub use core::{JSArrayBuffer, JSDataView, JSTypedArray, TypedArrayKind};
pub use core::{
    JSObjectData, Token, initialize_global_constructors, parse_expression, parse_object_destructuring_pattern, parse_statement,
    parse_statements,
};
pub use error::{JSError, JSErrorKind};
pub use repl::Repl;
