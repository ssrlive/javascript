use crate::core::{Chunk, DEFAULT_GC_OPCODE_BUDGET, JsArenaVm, VM};
use crate::error::JSError;
use indexmap::IndexMap;
use num_bigint::BigInt;

pub(crate) type BoxedJsCallback = Box<dyn FnMut(&[JsValue]) -> Result<JsValue, JSError>>;

/// Opaque handle to a JavaScript heap object/function/collection rooted in a [`JsRuntime`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JsObjectHandle(pub(crate) u64);

impl JsObjectHandle {
    /// Returns the raw numeric identifier of this rooted handle.
    pub fn id(self) -> u64 {
        self.0
    }
}

/// Host-facing JavaScript value without any `'gc` arena lifetime.
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
    /// Convert a [`serde_json::Value`] into a [`JsValue`].
    pub fn from_json(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => JsValue::Null,
            serde_json::Value::Bool(b) => JsValue::Boolean(*b),
            serde_json::Value::Number(n) => JsValue::Number(n.as_f64().unwrap_or(f64::NAN)),
            serde_json::Value::String(s) => JsValue::String(s.clone()),
            serde_json::Value::Array(items) => JsValue::Array(items.iter().map(JsValue::from_json).collect()),
            serde_json::Value::Object(entries) => {
                let mut map = IndexMap::with_capacity(entries.len());
                for (k, v) in entries {
                    map.insert(k.clone(), JsValue::from_json(v));
                }
                JsValue::Record(map)
            }
        }
    }

    /// Convert a standalone [`JsValue`] (without runtime-rooted [`JsObjectHandle`]s)
    /// into a [`serde_json::Value`].
    ///
    /// To serialize [`JsValue::Object`] handles rooted in a [`JsRuntime`], use
    /// [`JsRuntime::to_json`].
    pub fn to_json(&self) -> Result<serde_json::Value, JSError> {
        match self {
            JsValue::Undefined | JsValue::Null => Ok(serde_json::Value::Null),
            JsValue::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
            JsValue::Number(n) => {
                if n.is_finite() && n.fract() == 0.0 && *n >= (i64::MIN as f64) && *n <= (i64::MAX as f64) {
                    Ok(serde_json::Value::Number(serde_json::Number::from(*n as i64)))
                } else {
                    Ok(serde_json::Number::from_f64(*n)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null))
                }
            }
            JsValue::BigInt(bi) => Ok(serde_json::Value::String(bi.to_string())),
            JsValue::String(s) => Ok(serde_json::Value::String(s.clone())),
            JsValue::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(item.to_json()?);
                }
                Ok(serde_json::Value::Array(out))
            }
            JsValue::Record(entries) => {
                let mut out = serde_json::Map::with_capacity(entries.len());
                for (k, v) in entries {
                    if !matches!(v, JsValue::Undefined) {
                        out.insert(k.clone(), v.to_json()?);
                    }
                }
                Ok(serde_json::Value::Object(out))
            }
            JsValue::Object(handle) => Err(raise_eval_error!(format!(
                "JsObjectHandle({}) requires JsRuntime::to_json to inspect heap state",
                handle.id()
            ))),
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            JsValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_handle(&self) -> Option<JsObjectHandle> {
        match self {
            JsValue::Object(h) => Some(*h),
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

/// Persistent embeddable JavaScript runtime backed by [`JsArenaVm`].
///
/// Scripts are executed in opcode-budgeted slices with incremental GC
/// (`Arena::collect_debt()`) between slices.
pub struct JsRuntime {
    pub(crate) arena: JsArenaVm,
    gc_opcode_budget: usize,
}

impl Default for JsRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl JsRuntime {
    /// Create a new [`JsRuntime`] with [`DEFAULT_GC_OPCODE_BUDGET`].
    pub fn new() -> Self {
        let arena = JsArenaVm::new(|ctx| VM::new(Chunk::new(), ctx));
        Self {
            arena,
            gc_opcode_budget: DEFAULT_GC_OPCODE_BUDGET,
        }
    }

    /// Create a new [`JsRuntime`] with a custom opcode budget between GC safe points.
    pub fn with_gc_opcode_budget(gc_opcode_budget: usize) -> Result<Self, JSError> {
        if gc_opcode_budget == 0 {
            return Err(raise_eval_error!("GC opcode budget must be greater than zero"));
        }
        let mut runtime = Self::new();
        runtime.gc_opcode_budget = gc_opcode_budget;
        Ok(runtime)
    }

    /// Returns the configured opcode budget between incremental GC safe points.
    pub fn gc_opcode_budget(&self) -> usize {
        self.gc_opcode_budget
    }

    /// Update the opcode budget between incremental GC safe points.
    pub fn set_gc_opcode_budget(&mut self, gc_opcode_budget: usize) -> Result<(), JSError> {
        if gc_opcode_budget == 0 {
            return Err(raise_eval_error!("GC opcode budget must be greater than zero"));
        }
        self.gc_opcode_budget = gc_opcode_budget;
        Ok(())
    }

    /// Explicitly run incremental garbage collection on the underlying arena.
    pub fn collect_garbage(&mut self) {
        self.arena.collect_debt();
    }

    /// Register a host Rust function on the global object.
    pub fn register_fn<F>(&mut self, name: &str, callback: F)
    where
        F: FnMut(&[JsValue]) -> Result<JsValue, JSError> + 'static,
    {
        let boxed: BoxedJsCallback = Box::new(callback);
        self.arena.mutate_root(|ctx, vm| {
            vm.register_host_callback(ctx, name, boxed);
        });
        self.arena.collect_debt();
    }

    /// Evaluate JavaScript source code in the persistent runtime using sliced execution
    /// and incremental GC between slices.
    pub fn eval<T: AsRef<str>>(&mut self, code: T) -> Result<JsValue, JSError> {
        let code_str = code.as_ref();
        let budget = self.gc_opcode_budget;
        let mut initialized = false;

        let result = loop {
            let step = self.arena.mutate_root(|ctx, vm| -> Result<Option<JsValue>, JSError> {
                if !initialized {
                    vm.prepare_merged_script(code_str)?;
                    initialized = true;
                }

                let Some(mut v) = vm.run_slice(ctx, budget)? else {
                    return Ok(None);
                };
                v = vm.unwrap_settled_top_level_promise(ctx, v)?;
                Ok(Some(vm.value_to_host_value(&v)))
            });

            match step {
                Ok(Some(val)) => break Ok(val),
                Ok(None) => self.arena.collect_debt(),
                Err(err) => break Err(err),
            }
        };

        self.arena.collect_debt();
        result
    }

    pub(crate) fn eval_repl<T: AsRef<str>>(&mut self, code: T) -> Result<String, JSError> {
        let script = code.as_ref();
        let result = self.arena.mutate_root(|ctx, vm| {
            let value = vm.eval_repl_snippet(ctx, script)?;

            match value {
                crate::core::Value::String(s) => {
                    let value = crate::unicode::utf16_to_utf8(&s);
                    match serde_json::to_string(&value) {
                        Ok(quoted) => Ok(quoted),
                        Err(_) => Ok(format!("\"{}\"", value)),
                    }
                }
                crate::core::Value::Array(_) | crate::core::Value::Object(_) => Ok(crate::core::value_to_compact_result_string(&value)),
                _ => Ok(crate::core::value_to_string(&value)),
            }
        });
        self.arena.collect_debt();
        result
    }

    /// Invoke a global JavaScript or host function by name with [`JsValue`] arguments.
    pub fn call(&mut self, fn_name: &str, args: &[JsValue]) -> Result<JsValue, JSError> {
        let result = self
            .arena
            .mutate_root(|ctx, vm| vm.call_global_function_by_name(ctx, fn_name, args));
        self.arena.collect_debt();
        result
    }

    /// Convert any [`JsValue`] (including rooted [`JsObjectHandle`]s) into a
    /// [`serde_json::Value`] using the runtime's JSON serializer.
    pub fn to_json(&mut self, value: &JsValue) -> Result<serde_json::Value, JSError> {
        let result = self.arena.mutate_root(|ctx, vm| vm.host_value_to_json(ctx, value));
        self.arena.collect_debt();
        result
    }

    /// Release a rooted [`JsObjectHandle`] so the garbage collector can reclaim the object
    /// once it is no longer reachable from JavaScript.
    pub fn release_handle(&mut self, handle: JsObjectHandle) -> bool {
        let removed = self.arena.mutate_root(|_ctx, vm| vm.host_roots.remove(&handle.id()).is_some());
        self.arena.collect_debt();
        removed
    }
}
