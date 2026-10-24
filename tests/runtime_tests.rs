use javascript::{HostValue, JsRuntime};
use std::cell::Cell;
use std::rc::Rc;

#[test]
fn runtime_eval_persists_state_and_calls_js_functions() {
    let mut rt = JsRuntime::with_gc_opcode_budget(64).unwrap();
    rt.eval("var base = 10; function compute(a, b) { return base + a * b; }").unwrap();

    let result = rt.call("compute", &[HostValue::from(3), HostValue::from(4)]).unwrap();
    assert_eq!(result, HostValue::Number(22.0));
}

#[test]
fn runtime_register_fn_bridges_rust_and_js() {
    let mut rt = JsRuntime::new();
    let counter = Rc::new(Cell::new(0));
    let counter_ref = Rc::clone(&counter);

    rt.register_fn("hostAdd", move |args| {
        counter_ref.set(counter_ref.get() + 1);
        let a = args.first().and_then(HostValue::as_number).unwrap_or(0.0);
        let b = args.get(1).and_then(HostValue::as_number).unwrap_or(0.0);
        Ok(HostValue::from(a + b))
    });

    let eval_res = rt.eval("hostAdd(15, 27)").unwrap();
    assert_eq!(eval_res, HostValue::Number(42.0));

    let call_res = rt.call("hostAdd", &[HostValue::from(5), HostValue::from(6)]).unwrap();
    assert_eq!(call_res, HostValue::Number(11.0));
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

    let extra = HostValue::from_json(&serde_json::json!({ "bonus": 5 }));
    assert_eq!(extra.to_json().unwrap(), serde_json::json!({ "bonus": 5 }));

    let sum = rt.call("sumScores", &[HostValue::from(handle), extra]).unwrap();
    assert_eq!(sum, HostValue::Number(65.0));

    assert!(rt.release_handle(handle));
    assert!(rt.to_json(&HostValue::from(handle)).is_err());
}
