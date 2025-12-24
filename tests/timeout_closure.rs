use javascript::*;

#[test]
fn timeout_closure_handles_error_constructor() {
    let script = r#"
    // Ensure an async callback that rejects with `new Error(...)` can resolve Error
    function doSomethingCritical(){ return new Promise((r)=>{ setTimeout(()=>r('crit'), 5); }); }
    function doSomethingOptional(){ return new Promise((resolve,reject)=>{ setTimeout(()=>reject(new Error('optional failed')), 5); }); }
    function doSomethingExtraNice(x){ return new Promise((r)=>{ setTimeout(()=>r('extra-'+x), 5); }); }
    function moreCriticalStuff(){ return new Promise((r)=>{ setTimeout(()=>r('done'), 5); }); }

    // Chain where optional rejects but is caught; ensure no ReferenceError occurs
    doSomethingCritical()
        .then((result) =>
            doSomethingOptional()
                .then((optionalResult) => doSomethingExtraNice(optionalResult))
                .catch((e) => { /* swallow optional error */ }),
        )
        .then(() => moreCriticalStuff())
        .catch((e) => { throw new Error('serious error: ' + e.message); });
    "#;

    let res = evaluate_script(script, None::<&std::path::Path>);
    assert!(res.is_ok(), "evaluate_script failed: {:?}", res.err());
}
