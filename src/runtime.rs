use crate::core::{Chunk, DEFAULT_GC_OPCODE_BUDGET, JsArenaVm, VM};
use crate::error::JSError;
use indexmap::IndexMap;
use num_bigint::BigInt;

pub(crate) type BoxedHostCallback = Box<dyn FnMut(&[HostValue]) -> Result<HostValue, JSError>>;

/// Opaque handle to a JavaScript heap object/function/collection rooted in a [`JsRuntime`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostObjectHandle(pub(crate) u64);

impl HostObjectHandle {
    /// Returns the raw numeric identifier of this rooted handle.
    pub fn id(self) -> u64 {
        self.0
    }
}

/// Host-facing JavaScript value without any `'gc` arena lifetime.
#[derive(Debug, Clone, PartialEq)]
pub enum HostValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    BigInt(BigInt),
    String(String),
    Array(Vec<HostValue>),
    Record(IndexMap<String, HostValue>),
    Object(HostObjectHandle),
}

impl HostValue {
    /// Convert a [`serde_json::Value`] into a [`HostValue`].
    pub fn from_json(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => HostValue::Null,
            serde_json::Value::Bool(b) => HostValue::Boolean(*b),
            serde_json::Value::Number(n) => HostValue::Number(n.as_f64().unwrap_or(f64::NAN)),
            serde_json::Value::String(s) => HostValue::String(s.clone()),
            serde_json::Value::Array(items) => HostValue::Array(items.iter().map(HostValue::from_json).collect()),
            serde_json::Value::Object(entries) => {
                let mut map = IndexMap::with_capacity(entries.len());
                for (k, v) in entries {
                    map.insert(k.clone(), HostValue::from_json(v));
                }
                HostValue::Record(map)
            }
        }
    }

    /// Convert a standalone [`HostValue`] (without runtime-rooted [`HostObjectHandle`]s)
    /// into a [`serde_json::Value`].
    ///
    /// To serialize [`HostValue::Object`] handles rooted in a [`JsRuntime`], use
    /// [`JsRuntime::to_json`].
    pub fn to_json(&self) -> Result<serde_json::Value, JSError> {
        match self {
            HostValue::Undefined | HostValue::Null => Ok(serde_json::Value::Null),
            HostValue::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
            HostValue::Number(n) => {
                if n.is_finite() && n.fract() == 0.0 && *n >= (i64::MIN as f64) && *n <= (i64::MAX as f64) {
                    Ok(serde_json::Value::Number(serde_json::Number::from(*n as i64)))
                } else {
                    Ok(serde_json::Number::from_f64(*n)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null))
                }
            }
            HostValue::BigInt(bi) => Ok(serde_json::Value::String(bi.to_string())),
            HostValue::String(s) => Ok(serde_json::Value::String(s.clone())),
            HostValue::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(item.to_json()?);
                }
                Ok(serde_json::Value::Array(out))
            }
            HostValue::Record(entries) => {
                let mut out = serde_json::Map::with_capacity(entries.len());
                for (k, v) in entries {
                    if !matches!(v, HostValue::Undefined) {
                        out.insert(k.clone(), v.to_json()?);
                    }
                }
                Ok(serde_json::Value::Object(out))
            }
            HostValue::Object(handle) => Err(raise_eval_error!(format!(
                "HostObjectHandle({}) requires JsRuntime::to_json to inspect heap state",
                handle.id()
            ))),
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            HostValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            HostValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            HostValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_handle(&self) -> Option<HostObjectHandle> {
        match self {
            HostValue::Object(h) => Some(*h),
            _ => None,
        }
    }
}

impl From<()> for HostValue {
    fn from(_: ()) -> Self {
        HostValue::Undefined
    }
}

impl From<bool> for HostValue {
    fn from(value: bool) -> Self {
        HostValue::Boolean(value)
    }
}

impl From<f64> for HostValue {
    fn from(value: f64) -> Self {
        HostValue::Number(value)
    }
}

impl From<i32> for HostValue {
    fn from(value: i32) -> Self {
        HostValue::Number(value as f64)
    }
}

impl From<i64> for HostValue {
    fn from(value: i64) -> Self {
        HostValue::Number(value as f64)
    }
}

impl From<usize> for HostValue {
    fn from(value: usize) -> Self {
        HostValue::Number(value as f64)
    }
}

impl From<BigInt> for HostValue {
    fn from(value: BigInt) -> Self {
        HostValue::BigInt(value)
    }
}

impl From<String> for HostValue {
    fn from(value: String) -> Self {
        HostValue::String(value)
    }
}

impl From<&str> for HostValue {
    fn from(value: &str) -> Self {
        HostValue::String(value.to_string())
    }
}

impl From<HostObjectHandle> for HostValue {
    fn from(value: HostObjectHandle) -> Self {
        HostValue::Object(value)
    }
}

impl From<serde_json::Value> for HostValue {
    fn from(value: serde_json::Value) -> Self {
        HostValue::from_json(&value)
    }
}

impl TryFrom<&HostValue> for serde_json::Value {
    type Error = JSError;

    fn try_from(value: &HostValue) -> Result<Self, Self::Error> {
        value.to_json()
    }
}

impl TryFrom<HostValue> for serde_json::Value {
    type Error = JSError;

    fn try_from(value: HostValue) -> Result<Self, Self::Error> {
        value.to_json()
    }
}

/// Persistent embeddable JavaScript runtime backed by [`JsArenaVm`].
///
/// Scripts are executed in opcode-budgeted slices with incremental GC
/// (`Arena::collect_debt()`) between slices.
pub struct JsRuntime {
    arena: JsArenaVm,
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
        F: FnMut(&[HostValue]) -> Result<HostValue, JSError> + 'static,
    {
        let boxed: BoxedHostCallback = Box::new(callback);
        self.arena.mutate_root(|ctx, vm| {
            vm.register_host_callback(ctx, name, boxed);
        });
        self.arena.collect_debt();
    }

    /// Evaluate JavaScript source code in the persistent runtime using sliced execution
    /// and incremental GC between slices.
    pub fn eval<T: AsRef<str>>(&mut self, code: T) -> Result<HostValue, JSError> {
        let code_str = code.as_ref();
        let budget = self.gc_opcode_budget;
        let mut initialized = false;

        let result = loop {
            let step = self.arena.mutate_root(|ctx, vm| -> Result<Option<HostValue>, JSError> {
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

    /// Invoke a global JavaScript or host function by name with [`HostValue`] arguments.
    pub fn call(&mut self, fn_name: &str, args: &[HostValue]) -> Result<HostValue, JSError> {
        let result = self
            .arena
            .mutate_root(|ctx, vm| vm.call_global_function_by_name(ctx, fn_name, args));
        self.arena.collect_debt();
        result
    }

    /// Convert any [`HostValue`] (including rooted [`HostObjectHandle`]s) into a
    /// [`serde_json::Value`] using the runtime's JSON serializer.
    pub fn to_json(&mut self, value: &HostValue) -> Result<serde_json::Value, JSError> {
        let result = self.arena.mutate_root(|ctx, vm| vm.host_value_to_json(ctx, value));
        self.arena.collect_debt();
        result
    }

    /// Release a rooted [`HostObjectHandle`] so the garbage collector can reclaim the object
    /// once it is no longer reachable from JavaScript.
    pub fn release_handle(&mut self, handle: HostObjectHandle) -> bool {
        let removed = self.arena.mutate_root(|_ctx, vm| vm.host_roots.remove(&handle.id()).is_some());
        self.arena.collect_debt();
        removed
    }
}
