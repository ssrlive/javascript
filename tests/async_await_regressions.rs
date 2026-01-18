use javascript::evaluate_script;

// Initialize logger for these integration tests so `RUST_LOG` is honored.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn awaited_block_called_once() {
    let script = r#"
        let cnt = 0;
        function doSomething(){ cnt += 1; return new Promise((r)=>setTimeout(()=>r(1), 0)); }
        async function f(){ { await doSomething(); } }
        f();
        new Promise((resolve)=>setTimeout(()=>resolve(cnt), 20));
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(v) => assert_eq!(v, "1"),
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn awaited_parenthesized_expr_called_once() {
    let script = r#"
        let cnt = 0;
        function doSomething(){ cnt += 1; return new Promise((r)=>setTimeout(()=>r(1), 0)); }
        async function f(){ const x = await (doSomething()); }
        f();
        new Promise((resolve)=>setTimeout(()=>resolve(cnt), 20));
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(v) => assert_eq!(v, "1"),
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn awaited_let_initializer_called_once() {
    let script = r#"
        let cnt = 0;
        function doSomething(){ cnt += 1; return new Promise((r)=>setTimeout(()=>r(1), 0)); }
        async function f(){ let x = await doSomething(); }
        f();
        new Promise((resolve)=>setTimeout(()=>resolve(cnt), 20));
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(v) => assert_eq!(v, "1"),
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn awaited_return_called_once() {
    let script = r#"
        let cnt = 0;
        function doSomething(){ cnt += 1; return new Promise((r)=>setTimeout(()=>r(1), 0)); }
        async function f(){ return await doSomething(); }
        f();
        new Promise((resolve)=>setTimeout(()=>resolve(cnt), 20));
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(v) => assert_eq!(v, "1"),
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn awaited_ternary_called_once() {
    let script = r#"
        let cnt = 0;
        function doSomething(){ cnt += 1; return new Promise((r)=>setTimeout(()=>r(1), 0)); }
        async function f(){ const x = true ? await doSomething() : 0; }
        f();
        new Promise((resolve)=>setTimeout(()=>resolve(cnt), 20));
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(v) => assert_eq!(v, "1"),
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn awaited_if_called_once() {
    let script = r#"
        let cnt = 0;
        function doSomething(){ cnt += 1; return new Promise((r)=>setTimeout(()=>r(1), 0)); }
        async function f(){ if (true) { await doSomething(); } }
        f();
        new Promise((resolve)=>setTimeout(()=>resolve(cnt), 20));
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(v) => assert_eq!(v, "1"),
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn awaited_try_catch_called_once() {
    let script = r#"
        let cnt = 0;
        function doSomething(){ cnt += 1; return new Promise((r)=>setTimeout(()=>r(1), 0)); }
        async function f(){ try { await doSomething(); } catch(e) { } }
        f();
        new Promise((resolve)=>setTimeout(()=>resolve(cnt), 20));
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(v) => assert_eq!(v, "1"),
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn awaited_nested_blocks_called_once() {
    let script = r#"
        let cnt = 0;
        function doSomething(){ cnt += 1; return new Promise((r)=>setTimeout(()=>r(1), 0)); }
        async function f(){ { { await doSomething(); } } }
        f();
        new Promise((resolve)=>setTimeout(()=>resolve(cnt), 20));
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(v) => assert_eq!(v, "1"),
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn awaited_arrow_called_once() {
    let script = r#"
        let cnt = 0;
        function doSomething(){ cnt += 1; return new Promise((r)=>setTimeout(()=>r(1), 0)); }
        let f = async () => { await doSomething(); };
        f();
        new Promise((resolve)=>setTimeout(()=>resolve(cnt), 20));
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(v) => assert_eq!(v, "1"),
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn awaited_for_called_once() {
    let script = r#"
        let cnt = 0;
        function doSomething(){ cnt += 1; return new Promise((r)=>setTimeout(()=>r(1), 0)); }
        async function f(){ for (const v of [1]) { await doSomething(); } }
        f();
        new Promise((resolve)=>setTimeout(()=>resolve(cnt), 20));
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(v) => assert_eq!(v, "1"),
        Err(e) => panic!("{e:?}"),
    }
}
