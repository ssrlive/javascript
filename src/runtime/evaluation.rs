use super::JsRuntime;
use crate::core::{
    ModuleRequest, ReexportSpec, Value, collect_exports_from_ast, collect_hoisted_export_function_defs, collect_hoisted_function_defs,
    collect_hoisted_function_names, collect_module_requests, extract_injected_module_filepath, module_has_top_level_await,
    parse_program_statements, value_to_compact_result_string, value_to_string,
};
use crate::error::JSError;
use std::collections::HashMap;

type MainModuleRecord = (
    String,
    Vec<String>,
    HashMap<String, String>,
    Vec<(String, Vec<ReexportSpec>)>,
    Vec<ModuleRequest>,
    bool,
    std::path::PathBuf,
);

pub fn evaluate_script<T: AsRef<str>, P: AsRef<std::path::Path>>(
    script: T,
    run_as_module: bool,
    script_path: Option<P>,
    gc_opcode_budget: usize,
) -> Result<String, JSError> {
    let unwrap_top_level_promise = script_path.is_none();
    evaluate_script_with_unwrap(script, run_as_module, script_path, unwrap_top_level_promise, gc_opcode_budget)
}

pub fn evaluate_script_with_unwrap<T: AsRef<str>, P: AsRef<std::path::Path>>(
    script: T,
    run_as_module: bool,
    script_path: Option<P>,
    unwrap_top_level_promise: bool,
    gc_opcode_budget: usize,
) -> Result<String, JSError> {
    let mut runtime = JsRuntime::with_gc_opcode_budget(gc_opcode_budget)?;
    runtime.eval_script_with_unwrap(script, run_as_module, script_path, unwrap_top_level_promise)
}

impl JsRuntime {
    pub(crate) fn eval_script_with_unwrap<T: AsRef<str>, P: AsRef<std::path::Path>>(
        &mut self,
        script: T,
        run_as_module: bool,
        script_path: Option<P>,
        unwrap_top_level_promise: bool,
    ) -> Result<String, JSError> {
        let context_id = self.agent_context_id();
        crate::js_agent::with_context(context_id, || {
            self.eval_script_with_unwrap_scoped(script, run_as_module, script_path, unwrap_top_level_promise)
        })
    }

    fn eval_script_with_unwrap_scoped<T: AsRef<str>, P: AsRef<std::path::Path>>(
        &mut self,
        script: T,
        run_as_module: bool,
        script_path: Option<P>,
        unwrap_top_level_promise: bool,
    ) -> Result<String, JSError> {
        let script_str = script.as_ref();
        let statements = parse_program_statements(script_str, run_as_module)?;
        let script_path_buf = script_path.as_ref().map(|p| p.as_ref().to_path_buf());

        let gc_opcode_budget = self.gc_opcode_budget();
        let arena = &mut self.arena;
        #[cfg(feature = "jit")]
        let jit_engine = &mut self.jit_engine;
        let mut initialized = false;
        let mut module_to_finalize: Option<(String, Vec<String>)> = None;

        let result = loop {
            let step = arena.mutate_root(|ctx, vm| -> Result<Option<String>, JSError> {
                if !initialized {
                    if !crate::js_agent::is_agent_thread() {
                        crate::js_agent::reset_agent_state();
                    }

                    let script_path_buf = if let Some(p) = script_path_buf.as_ref() {
                        let mut p_str = p.to_string_lossy().to_string();
                        if run_as_module && let Some(injected_path) = extract_injected_module_filepath(script_str) {
                            p_str = injected_path;
                        }
                        Some(std::path::PathBuf::from(p_str))
                    } else {
                        None
                    };

                    let mut compiler = vm.new_compiler();
                    compiler.set_source_text(script_str.to_string());
                    if run_as_module && let Some(ref p) = script_path_buf {
                        compiler.set_script_filename(p.to_string_lossy().to_string());
                    }
                    let mut main_hoisted_local_defs: Option<HashMap<String, (usize, u8)>> = None;
                    let mut main_hoisted_export_defs: Option<HashMap<String, (usize, u8)>> = None;
                    let mut main_code_offset: Option<usize> = None;
                    let mut main_module_record: Option<MainModuleRecord> = None;

                    if run_as_module && let Some(ref entry_path) = script_path_buf {
                        let main_key = entry_path.to_string_lossy().to_string();
                        let (main_export_names, main_export_name_to_local, main_reexport_sources) = collect_exports_from_ast(&statements);
                        vm.pre_create_module_namespace(ctx, &main_key);
                        vm.seed_module_record(&main_key, &main_export_names, &main_export_name_to_local);
                        vm.seed_module_export_metadata(&main_key, &main_export_name_to_local, &main_reexport_sources, entry_path);
                        let main_has_tla = module_has_top_level_await(&statements);
                        let self_basename = entry_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        let requests = collect_module_requests(&statements, self_basename, vm.host_module_registry());
                        main_module_record = Some((
                            main_key,
                            main_export_names,
                            main_export_name_to_local,
                            main_reexport_sources,
                            requests.clone(),
                            main_has_tla,
                            entry_path.clone(),
                        ));
                        if let Some((
                            main_key,
                            main_export_names,
                            main_export_name_to_local,
                            main_reexport_sources,
                            main_requests,
                            main_has_tla,
                            entry_path,
                        )) = &main_module_record
                        {
                            vm.seed_pending_module_record(
                                ctx,
                                main_key,
                                script_str,
                                entry_path,
                                &statements,
                                main_export_names,
                                main_export_name_to_local,
                                main_reexport_sources,
                                main_requests,
                                *main_has_tla,
                            );
                            vm.mark_module_record_evaluating(main_key);
                        }
                        if !requests.is_empty() {
                            vm.load_module_graph(ctx, entry_path, &requests);
                            vm.check_module_load_health(ctx, entry_path, &requests)?;
                            vm.fixup_circular_reexports();
                            if let Some((ref mk, _, ref metl, ref mrs, _, _, ref ep)) = main_module_record {
                                vm.validate_module_resolution(mk, &statements, metl, mrs, ep)?;
                            }
                            for (path, exports) in &vm.loaded_modules {
                                let mut info = HashMap::new();
                                for k in exports.keys() {
                                    info.insert(k.clone(), path.clone());
                                }
                                compiler.set_loaded_module_exports(path.clone(), info);
                            }
                        }
                    }

                    if let Some((_, _, _, _, _, _, entry_path)) = &main_module_record {
                        vm.apply_module_resolution_overrides(&mut compiler, entry_path);
                    }
                    let chunk = compiler.compile(&statements)?;
                    #[cfg(feature = "jit")]
                    let jit_functions = jit_engine
                        .as_mut()
                        .map(|engine| engine.compile_functions(&statements, &chunk))
                        .unwrap_or_default();
                    if let Some((_, _, main_export_name_to_local, _, _, _, _)) = &main_module_record {
                        let hoisted_locals = collect_hoisted_function_names(&statements);
                        if !hoisted_locals.is_empty() {
                            let hoisted_defs = collect_hoisted_function_defs(&chunk, &hoisted_locals);
                            main_hoisted_export_defs = Some(collect_hoisted_export_function_defs(&hoisted_defs, main_export_name_to_local));
                            main_hoisted_local_defs = Some(hoisted_defs);
                        }
                    }

                    if run_as_module && vm.chunk.code.is_empty() {
                        main_code_offset = Some(0);
                        vm.chunk = chunk;
                        vm.main_module_ip_start = Some(0);
                    } else if run_as_module && !vm.chunk.code.is_empty() {
                        let main_loaded_module_vars = chunk.loaded_module_vars.clone();
                        let main_self_namespace_imports = chunk.self_namespace_imports.clone();
                        let main_self_deferred_namespace_imports = chunk.self_deferred_namespace_imports.clone();
                        let main_self_import_aliases = chunk.self_import_aliases.clone();
                        let main_const_import_bindings = chunk.const_import_bindings.clone();

                        let main_ip = vm.chunk.merge_dependency_chunk(chunk);
                        main_code_offset = Some(main_ip);
                        vm.ip = main_ip;
                        vm.main_module_ip_start = Some(main_ip);
                        vm.chunk.loaded_module_vars = main_loaded_module_vars;
                        vm.chunk.self_namespace_imports = main_self_namespace_imports;
                        vm.chunk.self_deferred_namespace_imports = main_self_deferred_namespace_imports;
                        vm.chunk.self_import_aliases = main_self_import_aliases;
                        vm.chunk.const_import_bindings = main_const_import_bindings;
                    } else {
                        vm.chunk = chunk;
                    }

                    #[cfg(feature = "jit")]
                    for registration in jit_functions {
                        vm.register_jit_function(
                            registration.ip + main_code_offset.unwrap_or(0),
                            registration.arity,
                            registration.function,
                            registration.generic_value,
                            registration.dependencies,
                        );
                    }

                    if run_as_module {
                        vm.set_module_this();
                    }

                    if let Some((
                        main_key,
                        main_export_names,
                        main_export_name_to_local,
                        main_reexport_sources,
                        main_requests,
                        main_has_tla,
                        entry_path,
                    )) = &main_module_record
                    {
                        vm.register_current_module_record(
                            ctx,
                            main_key,
                            script_str,
                            entry_path,
                            &statements,
                            main_export_names,
                            main_export_name_to_local,
                            main_reexport_sources,
                            main_requests,
                            *main_has_tla,
                        );
                        vm.mark_module_record_evaluating(main_key);
                        if let Some(defs) = &main_hoisted_local_defs
                            && !defs.is_empty()
                        {
                            let code_offset = main_code_offset.unwrap_or(0);
                            let seeded_locals = defs
                                .iter()
                                .map(|(local_name, (ip, arity))| (local_name.clone(), Value::Function(ip + code_offset, *arity)))
                                .collect::<HashMap<_, _>>();
                            vm.seed_module_locals(main_key, &seeded_locals);
                        }
                        if let Some(defs) = &main_hoisted_export_defs
                            && !defs.is_empty()
                        {
                            let code_offset = main_code_offset.unwrap_or(0);
                            let seeded_exports = defs
                                .iter()
                                .map(|(export_name, (ip, arity))| (export_name.clone(), Value::Function(ip + code_offset, *arity)))
                                .collect::<HashMap<_, _>>();
                            vm.seed_module_exports(ctx, main_key, &seeded_exports);
                        }
                        if !main_requests.is_empty() {
                            vm.evaluate_module_requests(ctx, entry_path, main_requests);
                        }
                    }

                    if run_as_module {
                        vm.inject_loaded_module_bindings(ctx);
                    }

                    vm.set_source_context(script_str, script_path_buf.as_deref());
                    if let Some((main_key, main_export_names, ..)) = &main_module_record {
                        module_to_finalize = Some((main_key.clone(), main_export_names.clone()));
                    }
                    initialized = true;
                }

                let Some(mut value) = vm.run_slice(ctx, gc_opcode_budget)? else {
                    return Ok(None);
                };
                if let Some((main_key, main_export_names)) = &module_to_finalize {
                    vm.finalize_active_module_record(ctx, main_key, main_export_names);
                }

                if unwrap_top_level_promise {
                    for _ in 0..8 {
                        let step = if let Value::Object(object) = &value {
                            let borrowed = object.borrow();
                            let is_promise = matches!(
                                borrowed.get("__type__"),
                                Some(Value::String(string)) if crate::unicode::utf16_to_utf8(string) == "Promise"
                            );
                            if is_promise {
                                let rejected = matches!(borrowed.get("__promise_rejected__"), Some(Value::Boolean(true)));
                                let next = borrowed.get("__promise_value__").cloned();
                                Some((rejected, next))
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        let Some((rejected, next)) = step else {
                            break;
                        };
                        let Some(next) = next else {
                            break;
                        };
                        if rejected && let Value::Object(object) = &next {
                            let borrowed = object.borrow();
                            if let Some(Value::String(type_name)) = borrowed.get("__type__") {
                                let type_name = crate::unicode::utf16_to_utf8(type_name);
                                if type_name == "Error" || type_name.ends_with("Error") {
                                    drop(borrowed);
                                    return Err(vm.vm_error_to_js_error(ctx, &next));
                                }
                            }
                        }
                        value = next;
                    }
                }

                match value {
                    Value::String(string) => {
                        let string = crate::unicode::utf16_to_utf8(&string);
                        match serde_json::to_string(&string) {
                            Ok(quoted) => Ok(Some(quoted)),
                            Err(_) => Ok(Some(format!("\"{}\"", string))),
                        }
                    }
                    Value::Array(_) | Value::Object(_) => Ok(Some(value_to_compact_result_string(&value))),
                    _ => Ok(Some(value_to_string(&value))),
                }
            });

            match step {
                Ok(Some(result)) => break Ok(result),
                Ok(None) => arena.collect_debt(),
                Err(error) => break Err(error),
            }
        };

        arena.collect_debt();
        result
    }
}
