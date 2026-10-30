#[cfg(feature = "console")]
pub(crate) mod console;
mod evaluation;
#[cfg(feature = "fs")]
pub(crate) mod fs;
#[cfg(feature = "jit")]
mod jit;
#[cfg(any(feature = "process", feature = "os", feature = "fs"))]
mod node_common;
#[cfg(feature = "os")]
pub(crate) mod os;
#[cfg(feature = "process")]
pub(crate) mod process;
use crate::core::{Chunk, DEFAULT_GC_OPCODE_BUDGET, JsArenaVm, VM};
use crate::error::JSError;
pub(crate) use crate::host_api::BoxedJsCallback;
pub use crate::host_api::{JsObjectHandle, JsPromiseResolver, JsValue, ModuleLoader, ModuleSource};
#[cfg(feature = "console")]
pub use console::{ConsoleLevel, ConsoleMessage};
#[cfg(feature = "jit")]
pub use evaluation::evaluate_script_with_jit_required;
pub use evaluation::{evaluate_script, evaluate_script_with_unwrap};
use std::cell::RefCell;
use std::rc::Rc;

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
    #[cfg(feature = "jit")]
    jit_engine: Option<jit::JitEngine>,
    #[cfg(feature = "console")]
    console_state: console::ConsoleStateRef,
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
        #[cfg(feature = "console")]
        let console_state = console::new_state();
        #[cfg(feature = "jit")]
        let jit_engine = match jit::JitEngine::new() {
            Ok(engine) => Some(engine),
            Err(error) => {
                log::warn!("JIT is unavailable; using the interpreter: {error}");
                None
            }
        };
        let arena = JsArenaVm::new(|ctx| {
            #[allow(unused_mut)]
            let mut vm = VM::new(Chunk::new(), ctx);
            #[cfg(feature = "console")]
            console::install(&mut vm, ctx, console_state.clone());
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
            #[cfg(feature = "jit")]
            jit_engine,
            #[cfg(feature = "console")]
            console_state,
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

    /// Require ordinary JavaScript function calls to execute through the JIT.
    ///
    /// When enabled, a function that was not compiled by the JIT, is called
    /// with unsupported arguments, or otherwise cannot enter native code
    /// returns an error instead of silently falling back to the interpreter.
    /// Built-in functions continue to use their existing runtime implementations.
    #[cfg(feature = "jit")]
    pub fn set_jit_required(&mut self, required: bool) {
        self.mutate_root(|_, vm| vm.set_jit_required(required));
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
        self.mutate_root(|_, vm| vm.stop_requested())
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
            self.console_state.borrow_mut().set_stdout(!redirect_console);
        }
        self.host_hooks = hooks;
    }

    /// Take console messages that have not been delivered to a configured host hook.
    #[cfg(feature = "console")]
    pub fn take_console_output(&mut self) -> Vec<ConsoleMessage> {
        self.console_state.borrow_mut().take_output()
    }

    fn dispatch_console_output(&mut self) {
        #[cfg(feature = "console")]
        {
            if self.host_hooks.console.is_none() {
                return;
            }
            let messages = self.console_state.borrow_mut().take_output();
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
        let context_id = self.agent_context_id;
        {
            let arena = &mut self.arena;
            #[cfg(feature = "jit")]
            let jit_engine = &mut self.jit_engine;
            crate::js_agent::with_context(context_id, || {
                arena.mutate_root(|_ctx, vm| {
                    #[cfg(feature = "jit")]
                    {
                        vm.prepare_merged_script_with_jit(code.as_ref(), |statements, chunk| {
                            jit_engine
                                .as_mut()
                                .map(|engine| engine.compile_functions(statements, chunk))
                                .unwrap_or_default()
                        })
                    }
                    #[cfg(not(feature = "jit"))]
                    vm.prepare_merged_script(code.as_ref())
                })
            })?;
        }
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
        let context_id = self.agent_context_id;
        let mut initialized = false;

        let result = loop {
            let step = {
                let arena = &mut self.arena;
                #[cfg(feature = "jit")]
                let jit_engine = &mut self.jit_engine;
                crate::js_agent::with_context(context_id, || {
                    arena.mutate_root(|ctx, vm| -> Result<Option<JsValue>, JSError> {
                        if !initialized {
                            #[cfg(feature = "jit")]
                            vm.prepare_merged_script_with_jit(code_str, |statements, chunk| {
                                jit_engine
                                    .as_mut()
                                    .map(|engine| engine.compile_functions(statements, chunk))
                                    .unwrap_or_default()
                            })?;
                            #[cfg(not(feature = "jit"))]
                            vm.prepare_merged_script(code_str)?;
                            initialized = true;
                        }

                        let Some(mut value) = vm.run_slice(ctx, budget)? else {
                            return Ok(None);
                        };
                        value = vm.unwrap_settled_top_level_promise(ctx, value)?;
                        Ok(Some(vm.value_to_host_value(&value)))
                    })
                })
            };
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
