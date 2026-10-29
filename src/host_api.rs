//! Host-facing types shared by the VM and runtime adapters.

use crate::error::JSError;
use indexmap::IndexMap;
use num_bigint::BigInt;

pub(crate) type BoxedJsCallback = Box<dyn FnMut(&[JsValue]) -> Result<JsValue, JSError>>;

/// Source returned by a host-provided module loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleSource {
    Text(String),
    Bytes(Vec<u8>),
}

/// Host interface used to resolve and load ECMAScript modules.
pub trait ModuleLoader: 'static {
    fn resolve(&mut self, specifier: &str, referrer: Option<&str>) -> Result<String, JSError>;
    fn load(&mut self, resolved_key: &str) -> Result<ModuleSource, JSError>;
}

/// Opaque handle to a JavaScript heap object rooted in a `JsRuntime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JsObjectHandle(pub(crate) u64);

impl JsObjectHandle {
    /// Returns the raw numeric identifier of this rooted handle.
    pub fn id(self) -> u64 {
        self.0
    }
}

/// Host-side resolver for a promise created by `JsRuntime::create_promise`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsPromiseResolver {
    pub(crate) resolve: JsObjectHandle,
    pub(crate) reject: JsObjectHandle,
}

impl JsPromiseResolver {
    pub(crate) fn new(resolve: JsObjectHandle, reject: JsObjectHandle) -> Self {
        Self { resolve, reject }
    }
}

/// Host-facing JavaScript value without any garbage-collector arena lifetime.
#[derive(Debug, Clone, PartialEq)]
pub enum JsValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    BigInt(BigInt),
    String(String),
    Array(Vec<JsValue>),
    Record(IndexMap<String, JsValue>),
    Object(JsObjectHandle),
}

impl JsValue {
    /// Converts a `serde_json::Value` into a `JsValue`.
    pub fn from_json(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => JsValue::Null,
            serde_json::Value::Bool(b) => JsValue::Boolean(*b),
            serde_json::Value::Number(n) => JsValue::Number(n.as_f64().unwrap_or(f64::NAN)),
            serde_json::Value::String(s) => JsValue::String(s.clone()),
            serde_json::Value::Array(items) => JsValue::Array(items.iter().map(JsValue::from_json).collect()),
            serde_json::Value::Object(entries) => {
                let mut map = IndexMap::with_capacity(entries.len());
                for (key, value) in entries {
                    map.insert(key.clone(), JsValue::from_json(value));
                }
                JsValue::Record(map)
            }
        }
    }

    /// Converts a standalone value into JSON.
    ///
    /// Object handles require `JsRuntime::to_json` because their heap state is
    /// owned by a runtime.
    pub fn to_json(&self) -> Result<serde_json::Value, JSError> {
        match self {
            JsValue::Undefined | JsValue::Null => Ok(serde_json::Value::Null),
            JsValue::Boolean(value) => Ok(serde_json::Value::Bool(*value)),
            JsValue::Number(value) => {
                if value.is_finite() && value.fract() == 0.0 && *value >= i64::MIN as f64 && *value <= i64::MAX as f64 {
                    Ok(serde_json::Value::Number(serde_json::Number::from(*value as i64)))
                } else {
                    Ok(serde_json::Number::from_f64(*value)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null))
                }
            }
            JsValue::BigInt(value) => Ok(serde_json::Value::String(value.to_string())),
            JsValue::String(value) => Ok(serde_json::Value::String(value.clone())),
            JsValue::Array(items) => {
                let mut output = Vec::with_capacity(items.len());
                for item in items {
                    output.push(item.to_json()?);
                }
                Ok(serde_json::Value::Array(output))
            }
            JsValue::Record(entries) => {
                let mut output = serde_json::Map::with_capacity(entries.len());
                for (key, value) in entries {
                    if !matches!(value, JsValue::Undefined) {
                        output.insert(key.clone(), value.to_json()?);
                    }
                }
                Ok(serde_json::Value::Object(output))
            }
            JsValue::Object(handle) => Err(raise_eval_error!(format!(
                "JsObjectHandle({}) requires JsRuntime::to_json to inspect heap state",
                handle.id()
            ))),
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsValue::Boolean(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            JsValue::Number(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsValue::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_handle(&self) -> Option<JsObjectHandle> {
        match self {
            JsValue::Object(handle) => Some(*handle),
            _ => None,
        }
    }
}

impl From<()> for JsValue {
    fn from(_: ()) -> Self {
        JsValue::Undefined
    }
}

impl From<bool> for JsValue {
    fn from(value: bool) -> Self {
        JsValue::Boolean(value)
    }
}

impl From<f64> for JsValue {
    fn from(value: f64) -> Self {
        JsValue::Number(value)
    }
}

impl From<i32> for JsValue {
    fn from(value: i32) -> Self {
        JsValue::Number(value as f64)
    }
}

impl From<i64> for JsValue {
    fn from(value: i64) -> Self {
        JsValue::Number(value as f64)
    }
}

impl From<usize> for JsValue {
    fn from(value: usize) -> Self {
        JsValue::Number(value as f64)
    }
}

impl From<BigInt> for JsValue {
    fn from(value: BigInt) -> Self {
        JsValue::BigInt(value)
    }
}

impl From<String> for JsValue {
    fn from(value: String) -> Self {
        JsValue::String(value)
    }
}

impl From<&str> for JsValue {
    fn from(value: &str) -> Self {
        JsValue::String(value.to_string())
    }
}

impl From<JsObjectHandle> for JsValue {
    fn from(value: JsObjectHandle) -> Self {
        JsValue::Object(value)
    }
}

impl From<serde_json::Value> for JsValue {
    fn from(value: serde_json::Value) -> Self {
        JsValue::from_json(&value)
    }
}

impl TryFrom<&JsValue> for serde_json::Value {
    type Error = JSError;

    fn try_from(value: &JsValue) -> Result<Self, Self::Error> {
        value.to_json()
    }
}

impl TryFrom<JsValue> for serde_json::Value {
    type Error = JSError;

    fn try_from(value: JsValue) -> Result<Self, Self::Error> {
        value.to_json()
    }
}
