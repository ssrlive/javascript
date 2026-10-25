use javascript::{
    ConsoleLevel, ConsoleMessage, HostHooks, JSError, JSErrorKind, JsRunStatus, JsRuntime, JsValue, ModuleLoader, ModuleSource,
};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

#[test]
fn runtime_eval_persists_state_and_calls_js_functions() {
    let mut rt = JsRuntime::with_gc_opcode_budget(64).unwrap();
    rt.eval("var base = 10; function compute(a, b) { return base + a * b; }").unwrap();

    let result = rt.call("compute", &[JsValue::from(3), JsValue::from(4)]).unwrap();
    assert_eq!(result, JsValue::Number(22.0));
}

#[test]
fn runtime_register_fn_bridges_rust_and_js() {
    let mut rt = JsRuntime::new();
    let counter = Rc::new(Cell::new(0));
    let counter_ref = Rc::clone(&counter);

    rt.register_fn("hostAdd", move |args| {
        counter_ref.set(counter_ref.get() + 1);
        let a = args.first().and_then(JsValue::as_number).unwrap_or(0.0);
        let b = args.get(1).and_then(JsValue::as_number).unwrap_or(0.0);
        Ok(JsValue::from(a + b))
    });

    let eval_res = rt.eval("hostAdd(15, 27)").unwrap();
    assert_eq!(eval_res, JsValue::Number(42.0));

    let call_res = rt.call("hostAdd", &[JsValue::from(5), JsValue::from(6)]).unwrap();
    assert_eq!(call_res, JsValue::Number(11.0));
    assert_eq!(counter.get(), 2);
}

#[test]
fn runtime_handles_and_json_roundtrip() {
    let mut rt = JsRuntime::new();

    let obj_val = rt.eval("({ user: 'alice', scores: [10, 20, 30] })").unwrap();
    let handle = obj_val.as_handle().expect("expected object handle");

    let json = rt.to_json(&obj_val).unwrap();
    assert_eq!(json, serde_json::json!({ "user": "alice", "scores": [10, 20, 30] }));

    rt.eval("function sumScores(payload, extra) { return payload.scores.reduce((a, b) => a + b, extra.bonus); }")
        .unwrap();

    let extra = JsValue::from_json(&serde_json::json!({ "bonus": 5 }));
    assert_eq!(extra.to_json().unwrap(), serde_json::json!({ "bonus": 5 }));

    let sum = rt.call("sumScores", &[JsValue::from(handle), extra]).unwrap();
    assert_eq!(sum, JsValue::Number(65.0));

    assert!(rt.release_handle(handle));
    assert!(rt.to_json(&JsValue::from(handle)).is_err());
}

struct MemoryModuleLoader {
    sources: HashMap<String, String>,
}

impl ModuleLoader for MemoryModuleLoader {
    fn resolve(&mut self, specifier: &str, _referrer: Option<&str>) -> Result<String, JSError> {
        match specifier {
            "./dep.js" => Ok("mem/dep.js".to_string()),
            _ => Err(JSError::new(
                JSErrorKind::EvaluationError {
                    message: format!("Unknown test module: {specifier}"),
                },
                "runtime_tests.rs".to_string(),
                "MemoryModuleLoader::resolve".to_string(),
                None,
            )),
        }
    }

    fn load(&mut self, resolved_key: &str) -> Result<ModuleSource, JSError> {
        self.sources.get(resolved_key).cloned().map(ModuleSource::Text).ok_or_else(|| {
            JSError::new(
                JSErrorKind::EvaluationError {
                    message: format!("Missing test module: {resolved_key}"),
                },
                "runtime_tests.rs".to_string(),
                "MemoryModuleLoader::load".to_string(),
                None,
            )
        })
    }
}

#[test]
fn runtime_loads_modules_through_host_module_loader() {
    let mut rt = JsRuntime::new();
    rt.set_module_loader(MemoryModuleLoader {
        sources: HashMap::from([("mem/dep.js".to_string(), "export const value = 42;".to_string())]),
    });

    let result = rt
        .eval_module("import { value } from './dep.js'; globalThis.answer = value;", "mem/main.js")
        .unwrap();
    assert_eq!(result, JsValue::Undefined);
    assert_eq!(rt.eval("answer").unwrap(), JsValue::Number(42.0));
}

#[test]
fn runtime_runs_host_controlled_slices_and_timers() {
    let mut rt = JsRuntime::new();
    let fired = Rc::new(std::cell::RefCell::new(Vec::new()));
    let fired_ref = Rc::clone(&fired);
    rt.register_fn("onTimer", move |args| {
        if let Some(value) = args.first().and_then(JsValue::as_number) {
            fired_ref.borrow_mut().push(value);
        }
        Ok(JsValue::Undefined)
    });

    rt.start_eval("setTimeout(() => onTimer(1), 10); setTimeout(() => onTimer(2), 20);")
        .unwrap();
    loop {
        match rt.run_slice(2).unwrap() {
            JsRunStatus::Yielded => {}
            JsRunStatus::Complete(value) => {
                assert_eq!(value, JsValue::Number(2.0));
                break;
            }
        }
    }

    assert!(fired.borrow().is_empty());
    assert_eq!(rt.step_timers(9.0).unwrap(), 0);
    assert_eq!(rt.step_timers(10.0).unwrap(), 1);
    assert_eq!(&*fired.borrow(), &[1.0]);
    assert_eq!(rt.step_timers(20.0).unwrap(), 1);
    assert_eq!(&*fired.borrow(), &[1.0, 2.0]);
}

#[test]
fn runtime_redirects_console_output_to_host_hook() {
    let mut rt = JsRuntime::new();
    let messages = Rc::new(std::cell::RefCell::new(Vec::<ConsoleMessage>::new()));
    let messages_ref = Rc::clone(&messages);
    let mut hooks = HostHooks::default();
    hooks.set_console_handler(move |message| messages_ref.borrow_mut().push(message));
    rt.set_host_hooks(hooks);

    rt.eval("console.log('hello'); console.warn('careful'); console.error('failed');")
        .unwrap();

    assert_eq!(
        &*messages.borrow(),
        &[
            ConsoleMessage {
                level: ConsoleLevel::Log,
                text: "hello".to_string()
            },
            ConsoleMessage {
                level: ConsoleLevel::Warn,
                text: "careful".to_string()
            },
            ConsoleMessage {
                level: ConsoleLevel::Error,
                text: "failed".to_string()
            }
        ]
    );
}

#[test]
fn runtime_host_can_resolve_promise_after_async_work_completes() {
    let mut rt = JsRuntime::new();
    let (promise, resolver) = rt.create_promise().unwrap();
    let promise_handle = promise.as_handle().unwrap();
    rt.register_fn("getExternalPromise", move |_| Ok(JsValue::Object(promise_handle)));

    rt.eval("globalThis.asyncResult = 'pending'; getExternalPromise().then(value => { globalThis.asyncResult = value; });")
        .unwrap();
    assert_eq!(rt.eval("asyncResult").unwrap(), JsValue::String("pending".to_string()));

    rt.resolve_promise(resolver, JsValue::Number(99.0)).unwrap();
    assert_eq!(rt.eval("asyncResult").unwrap(), JsValue::Number(99.0));
    rt.release_promise_resolver(resolver);
}
