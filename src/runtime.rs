#[cfg(feature = "console")]
pub(crate) mod console;
#[cfg(feature = "fs")]
pub(crate) mod fs;
#[cfg(any(feature = "process", feature = "os", feature = "fs"))]
mod node_common;
#[cfg(feature = "os")]
pub(crate) mod os;
#[cfg(feature = "process")]
pub(crate) mod process;
use crate::core::{Chunk, DEFAULT_GC_OPCODE_BUDGET, JsArenaVm, VM};
use crate::error::JSError;
#[cfg(feature = "console")]
pub use console::{ConsoleLevel, ConsoleMessage};
use indexmap::IndexMap;
use num_bigint::BigInt;
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) fn builtin_module_global(_specifier: &str) -> Option<&'static str> {
    #[cfg(feature = "process")]
    if let Some(global) = process::builtin_global(_specifier) {
        return Some(global);
    }
    #[cfg(feature = "os")]
    if let Some(global) = os::builtin_global(_specifier) {
        return Some(global);
    }
    #[cfg(feature = "fs")]
    if let Some(global) = fs::builtin_global(_specifier) {
        return Some(global);
    }
    None
}

pub(crate) fn is_builtin_host_module(specifier: &str) -> bool {
    builtin_module_global(specifier).is_some()
}

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

/// Opaque handle to a JavaScript heap object/function/collection rooted in a [`JsRuntime`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JsObjectHandle(pub(crate) u64);

impl JsObjectHandle {
    /// Returns the raw numeric identifier of this rooted handle.
    pub fn id(self) -> u64 {
        self.0
    }
}

/// Host-side resolver for a promise created by [`JsRuntime::create_promise`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsPromiseResolver {
    resolve: JsObjectHandle,
    reject: JsObjectHandle,
}

impl JsPromiseResolver {
    pub(crate) fn new(resolve: JsObjectHandle, reject: JsObjectHandle) -> Self {
        Self { resolve, reject }
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

/// Result of executing one host-controlled VM slice.
#[derive(Debug, Clone, PartialEq)]
pub enum JsRunStatus {
    Yielded,
    Complete(JsValue),
}

/// Optional host integrations for runtime operations.
#[derive(Default)]
pub struct HostHooks {
    #[cfg(feature = "console")]
    console: Option<Box<dyn FnMut(ConsoleMessage) + 'static>>,
}

impl HostHooks {
    #[cfg(feature = "console")]
    pub fn set_console_handler<F>(&mut self, handler: F)
    where
        F: FnMut(ConsoleMessage) + 'static,
    {
        self.console = Some(Box::new(handler));
    }
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

/// Persistent embeddable JavaScript runtime backed by the engine's GC arena.
///
/// Scripts are executed in opcode-budgeted slices with incremental GC
/// (`Arena::collect_debt()`) between slices.
pub struct JsRuntime {
    pub(crate) arena: JsArenaVm,
    gc_opcode_budget: usize,
    module_loader: Option<Rc<RefCell<Box<dyn ModuleLoader>>>>,
    eval_in_progress: bool,
    host_hooks: HostHooks,
    agent_context_id: u64,
}

impl Default for JsRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl JsRuntime {
    /// Create a new [`JsRuntime`] with [`DEFAULT_GC_OPCODE_BUDGET`].
    pub fn new() -> Self {
        let agent_context_id = if crate::js_agent::is_agent_thread() {
            crate::js_agent::current_context_id()
        } else {
            crate::js_agent::new_context_id()
        };
        let arena = JsArenaVm::new(|ctx| {
            #[allow(unused_mut)]
            let mut vm = VM::new(Chunk::new(), ctx);
            #[cfg(feature = "console")]
            console::install(&mut vm, ctx);
            #[cfg(feature = "process")]
            process::install(&mut vm, ctx);
            #[cfg(feature = "os")]
            os::install(&mut vm, ctx);
            #[cfg(feature = "fs")]
            fs::install(&mut vm, ctx);
            vm
        });
        Self {
            arena,
            gc_opcode_budget: DEFAULT_GC_OPCODE_BUDGET,
            module_loader: None,
            eval_in_progress: false,
            host_hooks: HostHooks::default(),
            agent_context_id,
        }
    }

    pub(crate) fn mutate_root<R>(&mut self, f: impl for<'gc> FnOnce(&crate::core::GcContext<'gc>, &mut VM<'gc>) -> R) -> R {
        let context_id = self.agent_context_id;
        crate::js_agent::with_context(context_id, || self.arena.mutate_root(f))
    }

    pub(crate) fn agent_context_id(&self) -> u64 {
        self.agent_context_id
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

    /// Returns the integer value currently assigned to `process.exitCode`.
    #[cfg(feature = "process")]
    pub fn process_exit_code(&mut self) -> Option<i32> {
        self.mutate_root(|_, vm| {
            let Some(crate::core::Value::Object(process)) = vm.host_global("process") else {
                return None;
            };
            match process.borrow().get("exitCode") {
                Some(crate::core::Value::Number(code)) => Some(*code as i32),
                _ => None,
            }
        })
    }

    /// Returns whether JavaScript called `process.exit()` in this runtime.
    ///
    /// The request stops JavaScript execution but never terminates the embedding
    /// process. Hosts can inspect the exit code and decide how to handle it.
    #[cfg(feature = "process")]
    pub fn process_exit_requested(&mut self) -> bool {
        self.mutate_root(|_, vm| vm.host_stop_requested())
    }

    /// Explicitly run incremental garbage collection on the underlying arena.
    pub fn collect_garbage(&mut self) {
        self.arena.collect_debt();
    }

    /// Install a host module resolver/loader for subsequent module evaluations.
    pub fn set_module_loader<L: ModuleLoader>(&mut self, loader: L) {
        let loader = Rc::new(RefCell::new(Box::new(loader) as Box<dyn ModuleLoader>));
        self.mutate_root(|_ctx, vm| vm.set_module_loader(Some(loader.clone())));
        self.module_loader = Some(loader);
    }

    /// Remove the host module resolver/loader and restore filesystem module loading.
    pub fn clear_module_loader(&mut self) {
        self.mutate_root(|_ctx, vm| vm.set_module_loader(None));
        self.module_loader = None;
    }

    /// Replace the host callbacks used by this runtime.
    pub fn set_host_hooks(&mut self, hooks: HostHooks) {
        #[cfg(feature = "console")]
        {
            let redirect_console = hooks.console.is_some();
            self.mutate_root(|_ctx, vm| vm.console.set_stdout(!redirect_console));
        }
        self.host_hooks = hooks;
    }

    /// Take console messages that have not been delivered to a configured host hook.
    #[cfg(feature = "console")]
    pub fn take_console_output(&mut self) -> Vec<ConsoleMessage> {
        self.mutate_root(|_ctx, vm| vm.console.take_output())
    }

    fn dispatch_console_output(&mut self) {
        #[cfg(feature = "console")]
        {
            if self.host_hooks.console.is_none() {
                return;
            }
            let messages = self.mutate_root(|_ctx, vm| vm.console.take_output());
            let Some(handler) = self.host_hooks.console.as_mut() else {
                return;
            };
            for message in messages {
                handler(message);
            }
        }
    }

    /// Prepare a script for host-controlled, nonblocking execution.
    ///
    /// Call [`JsRuntime::run_slice`] repeatedly until it returns
    /// [`JsRunStatus::Complete`]. Unlike [`JsRuntime::eval`], this does not
    /// drain timers automatically.
    pub fn start_eval<T: AsRef<str>>(&mut self, code: T) -> Result<(), JSError> {
        if self.eval_in_progress {
            return Err(raise_eval_error!("A script evaluation is already in progress"));
        }
        self.mutate_root(|_ctx, vm| vm.prepare_merged_script(code.as_ref()))?;
        self.eval_in_progress = true;
        Ok(())
    }

    /// Execute up to `opcode_budget` opcodes from the active script.
    pub fn run_slice(&mut self, opcode_budget: usize) -> Result<JsRunStatus, JSError> {
        if opcode_budget == 0 {
            return Err(raise_eval_error!("Opcode budget must be greater than zero"));
        }
        if !self.eval_in_progress {
            return Err(raise_eval_error!("No script evaluation is in progress"));
        }

        let result = self.mutate_root(|ctx, vm| {
            let Some(value) = vm.run_slice_nonblocking(ctx, opcode_budget)? else {
                return Ok(None);
            };
            let value = vm.unwrap_settled_top_level_promise(ctx, value)?;
            Ok(Some(vm.value_to_host_value(&value)))
        });
        self.dispatch_console_output();
        match result {
            Ok(Some(value)) => {
                self.eval_in_progress = false;
                self.arena.collect_debt();
                Ok(JsRunStatus::Complete(value))
            }
            Ok(None) => {
                self.arena.collect_debt();
                Ok(JsRunStatus::Yielded)
            }
            Err(error) => {
                self.eval_in_progress = false;
                self.arena.collect_debt();
                Err(error)
            }
        }
    }

    /// Run queued promise reactions without waiting for timers or external work.
    pub fn drain_microtasks(&mut self) -> Result<usize, JSError> {
        let result = self.mutate_root(|ctx, vm| vm.drain_microtasks_for_host(ctx));
        self.dispatch_console_output();
        self.arena.collect_debt();
        result
    }

    /// Execute timers whose deadlines are at or before the host's monotonic `now_ms`.
    pub fn step_timers(&mut self, now_ms: f64) -> Result<usize, JSError> {
        let result = self.mutate_root(|ctx, vm| vm.step_timers_for_host(ctx, now_ms));
        self.dispatch_console_output();
        self.arena.collect_debt();
        result
    }

    /// Register a host Rust function on the global object.
    pub fn register_fn<F>(&mut self, name: &str, callback: F)
    where
        F: FnMut(&[JsValue]) -> Result<JsValue, JSError> + 'static,
    {
        let boxed: BoxedJsCallback = Box::new(callback);
        self.mutate_root(|ctx, vm| {
            vm.register_host_callback(ctx, name, boxed);
        });
        self.arena.collect_debt();
    }

    /// Evaluate JavaScript source code in the persistent runtime using sliced execution
    /// and incremental GC between slices.
    pub fn eval<T: AsRef<str>>(&mut self, code: T) -> Result<JsValue, JSError> {
        if self.eval_in_progress {
            return Err(raise_eval_error!("A script evaluation is already in progress"));
        }
        let code_str = code.as_ref();
        let budget = self.gc_opcode_budget;
        let mut initialized = false;

        let result = loop {
            let step = self.mutate_root(|ctx, vm| -> Result<Option<JsValue>, JSError> {
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
            self.dispatch_console_output();

            match step {
                Ok(Some(val)) => break Ok(val),
                Ok(None) => self.arena.collect_debt(),
                Err(err) => break Err(err),
            }
        };

        self.arena.collect_debt();
        result
    }

    /// Evaluate an ECMAScript module in this persistent runtime.
    ///
    /// `path` identifies the entry module and is passed to the configured
    /// [`ModuleLoader`] as its referrer key.
    pub fn eval_module<T: AsRef<str>, P: AsRef<std::path::Path>>(&mut self, code: T, path: P) -> Result<JsValue, JSError> {
        if self.eval_in_progress {
            return Err(raise_eval_error!("A script evaluation is already in progress"));
        }
        self.eval_script_with_unwrap(code, true, Some(path), false)?;
        self.dispatch_console_output();
        Ok(JsValue::Undefined)
    }

    pub(crate) fn eval_repl<T: AsRef<str>>(&mut self, code: T) -> Result<String, JSError> {
        if self.eval_in_progress {
            return Err(raise_eval_error!("A script evaluation is already in progress"));
        }
        let script = code.as_ref();
        let result = self.mutate_root(|ctx, vm| {
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
        self.dispatch_console_output();
        self.arena.collect_debt();
        result
    }

    /// Invoke a global JavaScript or host function by name with [`JsValue`] arguments.
    pub fn call(&mut self, fn_name: &str, args: &[JsValue]) -> Result<JsValue, JSError> {
        let result = self.mutate_root(|ctx, vm| vm.call_global_function_by_name(ctx, fn_name, args));
        self.dispatch_console_output();
        self.arena.collect_debt();
        result
    }

    /// Convert any [`JsValue`] (including rooted [`JsObjectHandle`]s) into a
    /// [`serde_json::Value`] using the runtime's JSON serializer.
    pub fn to_json(&mut self, value: &JsValue) -> Result<serde_json::Value, JSError> {
        let result = self.mutate_root(|ctx, vm| vm.host_value_to_json(ctx, value));
        self.dispatch_console_output();
        self.arena.collect_debt();
        result
    }

    /// Release a rooted [`JsObjectHandle`] so the garbage collector can reclaim the object
    /// once it is no longer reachable from JavaScript.
    pub fn release_handle(&mut self, handle: JsObjectHandle) -> bool {
        let removed = self.mutate_root(|_ctx, vm| vm.host_roots.remove(&handle.id()).is_some());
        self.arena.collect_debt();
        removed
    }

    /// Create a JavaScript Promise and host-side resolve/reject functions.
    pub fn create_promise(&mut self) -> Result<(JsValue, JsPromiseResolver), JSError> {
        let result = self.mutate_root(|ctx, vm| vm.create_host_promise(ctx));
        self.dispatch_console_output();
        self.arena.collect_debt();
        result
    }

    /// Resolve a promise created by [`JsRuntime::create_promise`].
    pub fn resolve_promise(&mut self, resolver: JsPromiseResolver, value: JsValue) -> Result<(), JSError> {
        self.settle_host_promise(resolver.resolve, value)
    }

    /// Reject a promise created by [`JsRuntime::create_promise`].
    pub fn reject_promise(&mut self, resolver: JsPromiseResolver, reason: JsValue) -> Result<(), JSError> {
        self.settle_host_promise(resolver.reject, reason)
    }

    /// Release the rooted resolver functions after a host promise has settled.
    pub fn release_promise_resolver(&mut self, resolver: JsPromiseResolver) {
        self.release_handle(resolver.resolve);
        self.release_handle(resolver.reject);
    }

    fn settle_host_promise(&mut self, function: JsObjectHandle, value: JsValue) -> Result<(), JSError> {
        let result = self.mutate_root(|ctx, vm| vm.call_host_root_function(ctx, function, value));
        self.dispatch_console_output();
        self.arena.collect_debt();
        result
    }
}

impl Drop for JsRuntime {
    fn drop(&mut self) {
        if !crate::js_agent::is_agent_thread() {
            crate::js_agent::release_context(self.agent_context_id);
        }
    }
}
