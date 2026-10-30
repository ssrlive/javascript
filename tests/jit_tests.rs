#![cfg(feature = "jit")]

use javascript::{DEFAULT_GC_OPCODE_BUDGET, JsRuntime, JsValue, evaluate_script_with_jit_required};

fn assert_no_interpreter(runtime: &mut JsRuntime) {
    let stats = runtime.jit_stats();
    assert!(stats.baseline_opcodes > 0, "{stats:?}");
    assert_eq!(stats.interpreter_opcodes, 0, "{stats:?}");
}

fn assert_baseline_matches(source: &str, expected: JsValue) {
    let interpreted = JsRuntime::with_gc_opcode_budget(1).unwrap().eval(source).unwrap();
    assert_eq!(interpreted, expected);
    let mut runtime = JsRuntime::with_gc_opcode_budget(1).unwrap();
    runtime.set_jit_required(true);
    assert_eq!(runtime.eval(source).unwrap(), interpreted);
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_required_script_evaluation_uses_the_native_path() {
    let result = evaluate_script_with_jit_required(
        "function twice(value) { return value * 2; } twice(21);",
        false,
        None::<&std::path::Path>,
        DEFAULT_GC_OPCODE_BUDGET,
    )
    .unwrap();

    assert_eq!(result, "42");
}

#[test]
fn jit_required_mode_executes_compiled_functions_on_the_first_call() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval("function twice(value) { return value * 2; } twice(21);").unwrap();

    assert_eq!(result, JsValue::Number(42.0));
    assert_no_interpreter(&mut runtime);
    assert_eq!(runtime.jit_stats().optimized_calls, 1);
}

#[test]
fn jit_required_mode_includes_completed_agent_execution_in_runtime_stats() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    runtime
        .eval(
            r#"
            __agent_start(`
                function twice(value) { return value * 2; }
                function* values() { yield twice(21); }
                __agent_report(String(values().next().value));
            `);
            "#,
        )
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while runtime.jit_stats().optimized_calls == 0 {
        assert!(std::time::Instant::now() < deadline, "agent did not finish in required-JIT mode");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(runtime.eval("__agent_getReport();").unwrap(), JsValue::String("42".to_string()));
    assert_no_interpreter(&mut runtime);

    let mut other = JsRuntime::new();
    assert_eq!(other.jit_stats(), javascript::JitStats::default());
}

#[test]
fn jit_required_mode_inlines_numeric_math_abs_with_interpreter_equivalent_results() {
    let source = r#"
        function absolute(value) { return Math.abs(value); }
        1 / absolute(-0) + absolute(-42);
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(f64::INFINITY));
}

#[test]
fn jit_baseline_addition_preserves_coercion_order_and_abrupt_completion() {
    let source = r#"
        let trace = "";
        let left = {
            get [Symbol.toPrimitive]() {
                trace += "L";
                return function(hint) { trace += hint; return 40n; };
            }
        };
        let right = {
            [Symbol.toPrimitive](hint) { trace += "R" + hint; return 2n; }
        };
        let sum = left + right;
        let sentinel = {};
        let caught = false;
        let reachedRight = false;
        try {
            ({ valueOf() { throw sentinel; } }) +
                ({ valueOf() { reachedRight = true; return 1; } });
        } catch (error) {
            caught = error === sentinel;
        }
        let mixedTypes = false;
        try { 1n + 1; } catch (error) { mixedTypes = error instanceof TypeError; }
        let invalidPrimitive = false;
        try {
            ({ [Symbol.toPrimitive]() { return {}; } }) + 1;
        } catch (error) {
            invalidPrimitive = error instanceof TypeError;
        }
        sum === 42n && trace === "LdefaultRdefault" && caught &&
            !reachedRight && mixedTypes && invalidPrimitive &&
            ("prefix" + 2n) === "prefix2" &&
            1 / (-0 + -0) === -Infinity;
    "#;
    let expected = JsRuntime::new().eval(source).unwrap();
    let mut runtime = JsRuntime::with_gc_opcode_budget(1).unwrap();
    runtime.set_jit_required(true);

    assert_eq!(runtime.eval(source).unwrap(), expected);
    assert_eq!(expected, JsValue::Boolean(true));
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_baseline_gc_slices_preserve_heap_identity_and_opcode_budget() {
    use javascript::JsRunStatus;

    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    runtime
        .start_eval(
            "let saved = { marker: 42 }; let values = []; \
             for (let i = 0; i < 40; i++) { values[i] = { link: saved }; } \
             values[39].link === saved && values[0].link.marker === 42;",
        )
        .unwrap();
    let mut slices = 0;
    loop {
        let before = runtime.jit_stats().baseline_opcodes;
        let status = runtime.run_slice(1).unwrap();
        let after = runtime.jit_stats().baseline_opcodes;
        assert_eq!(after - before, 1);
        slices += 1;
        assert!(slices < 10000, "baseline failed to complete");
        runtime.collect_garbage();
        if let JsRunStatus::Complete(value) = status {
            assert_eq!(value, JsValue::Boolean(true));
            break;
        }
    }
    assert!(slices > 40);
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_baseline_executes_eval_and_reentrant_callbacks_without_interpreting() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    let result = runtime
        .eval(
            r#"
            function Constructor(value) { this.value = value; }
            function add(a, b) { return a + b; }
            const object = new Constructor(40);
            const bound = add.bind(null, object.value);
            const result = [2].map(value => bound(...[value]))[0];
            result + eval("1 + 1") + (0, eval)("2 + 2");
            "#,
        )
        .unwrap();

    assert_eq!(result, JsValue::Number(48.0));
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_baseline_generator_resume_throw_and_return_preserve_identity() {
    assert_baseline_matches(
        r#"
        const sentinel = {};
        let trace = "";
        function* values(value = sentinel) {
            try {
                trace += "start;";
                const sent = yield value;
                trace += sent === sentinel ? "sent;" : "bad;";
                yield 2;
            } catch (error) {
                trace += error === sentinel ? "caught;" : "bad;";
            } finally {
                trace += "finally;";
            }
            return sentinel;
        }
        const iterator = values();
        const first = iterator.next();
        const second = iterator.next(sentinel);
        const third = iterator.throw(sentinel);
        const completed = iterator.next();
        const early = values();
        early.next();
        const returned = early.return(sentinel);
        first.value === sentinel && !first.done &&
            second.value === 2 && !second.done &&
            third.value === sentinel && third.done &&
            completed.done && completed.value === undefined &&
            returned.value === sentinel && returned.done &&
            trace === "start;sent;caught;finally;start;finally;";
        "#,
        JsValue::Boolean(true),
    );
}

#[test]
fn jit_baseline_generator_delegation_closes_iterators() {
    assert_baseline_matches(
        r#"
        let trace = "";
        const sentinel = {};
        const delegated = {
            [Symbol.iterator]() { return this; },
            next() { trace += "next;"; return { value: sentinel, done: false }; },
            return(value) {
                trace += value === sentinel ? "return;" : "bad;";
                return { value, done: true };
            }
        };
        function* values() {
            try { yield* delegated; }
            finally { trace += "finally;"; }
        }
        const iterator = values();
        const first = iterator.next();
        const last = iterator.return(sentinel);
        first.value === sentinel && !first.done &&
            last.value === sentinel && last.done &&
            trace === "next;return;finally;";
        "#,
        JsValue::Boolean(true),
    );
}

#[test]
fn jit_baseline_suspended_generators_keep_captured_objects_alive() {
    let mut runtime = JsRuntime::with_gc_opcode_budget(1).unwrap();
    runtime.set_jit_required(true);
    runtime
        .eval(
            r#"
            const sentinel = { marker: 42 };
            function* values() {
                const saved = { link: sentinel };
                const read = () => saved;
                yield read;
                return read() === saved && saved.link === sentinel;
            }
            const iterator = values();
            const captured = iterator.next().value;
            "#,
        )
        .unwrap();
    runtime.collect_garbage();
    assert_eq!(runtime.eval("captured().link === sentinel;").unwrap(), JsValue::Boolean(true));
    runtime.collect_garbage();
    assert_eq!(runtime.eval("iterator.next().value;").unwrap(), JsValue::Boolean(true));
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_baseline_classes_preserve_super_receivers_and_descriptors() {
    assert_baseline_matches(
        r#"
        let trace = "";
        class Base {
            constructor(value) { this.value = value; }
            get item() { trace += "get;"; return this.value; }
            set item(value) { trace += "set;"; this.value = value; }
            method() { return this; }
        }
        const key = "computed";
        class Derived extends Base {
            #secret = 2;
            field = (trace += "field;", this);
            constructor(value) { super(value); trace += "constructed;"; }
            read() { return super.item + super["item"] + this.#secret; }
            write(value) { super.item = value; super["item"] = value + 1; }
            [key]() { return super.method() === this; }
            get ["accessor"]() { return this.value; }
            set ["accessor"](value) { this.value = value; }
        }
        const object = new Derived(20);
        const first = object.read();
        object.write(39);
        object.accessor = 41;
        const descriptor = Object.getOwnPropertyDescriptor(Derived.prototype, "accessor");
        first === 42 && object.value === 41 && object.accessor === 41 &&
            object.field === object && object.computed() &&
            object instanceof Base && object instanceof Derived &&
            Object.getPrototypeOf(Derived) === Base &&
            !descriptor.enumerable && descriptor.configurable &&
            typeof descriptor.get === "function" && typeof descriptor.set === "function" &&
            trace === "field;constructed;get;get;set;set;";
        "#,
        JsValue::Boolean(true),
    );
}

#[test]
fn jit_baseline_classes_preserve_heritage_errors_and_private_brands() {
    assert_baseline_matches(
        r#"
        let failures = 0;
        try { class Invalid extends 1 {} } catch (error) {
            if (error instanceof TypeError) failures++;
        }
        function InvalidPrototype() {}
        InvalidPrototype.prototype = 1;
        try { class Invalid extends InvalidPrototype {} } catch (error) {
            if (error instanceof TypeError) failures++;
        }
        class Base {}
        class EarlyThis extends Base { constructor() { this.value = 1; super(); } }
        try { new EarlyThis(); } catch (error) {
            if (error instanceof ReferenceError) failures++;
        }
        function makeClass() {
            return class {
                #value = 42;
                read(object) { return object.#value; }
            };
        }
        const First = makeClass();
        const Second = makeClass();
        const first = new First();
        try { first.read(new Second()); } catch (error) {
            if (error instanceof TypeError) failures++;
        }
        class NullBase extends null { constructor() { return Object.create(new.target.prototype); } }
        failures === 4 && first.read(first) === 42 &&
            Object.getPrototypeOf(NullBase.prototype) === null &&
            new NullBase() instanceof NullBase;
        "#,
        JsValue::Boolean(true),
    );
}

#[test]
fn jit_baseline_async_resumption_preserves_exceptions_and_microtask_order() {
    let source = r#"
        let trace = "";
        const sentinel = { marker: 42 };
        let result;
        async function run() {
            trace += "start;";
            const saved = sentinel;
            try {
                const value = await {
                    then(resolve) { trace += "then;"; resolve(saved); }
                };
                trace += value === saved ? "resume;" : "bad;";
                await Promise.reject(saved);
            } catch (error) {
                trace += error === saved ? "catch;" : "bad;";
                return saved;
            } finally {
                trace += "finally;";
            }
        }
        run().then(value => { result = value; trace += "done;"; });
        trace += "sync;";
        0;
    "#;
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        runtime.eval(source).unwrap();
        runtime.collect_garbage();
        assert_eq!(
            runtime
                .eval("result === sentinel && trace === 'start;sync;then;resume;catch;finally;done;';")
                .unwrap(),
            JsValue::Boolean(true),
            "required={required}, trace={:?}",
            runtime.eval("trace;").unwrap(),
        );
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_baseline_suspended_async_frames_keep_live_values_and_handlers() {
    let source = r#"
        let resume;
        const gate = new Promise(resolve => { resume = resolve; });
        const sentinel = {};
        let result;
        let finalized = false;
        async function run() {
            const saved = { link: sentinel };
            const read = () => saved;
            try {
                await gate;
                throw read().link;
            } catch (error) {
                result = error === sentinel && read() === saved;
            } finally {
                finalized = true;
            }
        }
        run();
    "#;
    for required in [false, true] {
        let mut runtime = JsRuntime::with_gc_opcode_budget(1).unwrap();
        runtime.set_jit_required(required);
        runtime.eval(source).unwrap();
        assert_eq!(runtime.eval("result === undefined && !finalized;").unwrap(), JsValue::Boolean(true));
        runtime.collect_garbage();
        runtime.eval("resume(42);").unwrap();
        runtime.collect_garbage();
        assert_eq!(runtime.eval("result && finalized;").unwrap(), JsValue::Boolean(true));
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_baseline_async_generators_preserve_delegation_and_finalization() {
    let source = r#"
        let trace = "";
        const sentinel = {};
        let result = false;
        async function* values() {
            try {
                yield await Promise.resolve(sentinel);
                yield* [2, 3];
            } finally {
                trace += "finally;";
            }
        }
        async function consume() {
            const iterator = values();
            const first = await iterator.next();
            const second = await iterator.next();
            const last = await iterator.return(sentinel);
            result = first.value === sentinel && !first.done &&
                second.value === 2 && !second.done &&
                last.value === sentinel && last.done;
        }
        consume();
    "#;
    for required in [false, true] {
        let mut runtime = JsRuntime::with_gc_opcode_budget(1).unwrap();
        runtime.set_jit_required(required);
        runtime.eval(source).unwrap();
        runtime.collect_garbage();
        assert_eq!(runtime.eval("result && trace === 'finally;';").unwrap(), JsValue::Boolean(true));
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_baseline_builtin_callbacks_and_overrides_preserve_side_effects() {
    assert_baseline_matches(
        r#"
        let trace = "";
        const originalMap = Array.prototype.map;
        Object.defineProperty(Array.prototype, "map", {
            configurable: true,
            get() {
                trace += "lookup;";
                return function(callback) {
                    trace += "call;";
                    return originalMap.call(this, callback);
                };
            }
        });
        const key = {};
        const input = [40];
        const map = new Map([[key, input]]);
        const set = new Set([key, key]);
        const output = map.get(key).map(value => { trace += "callback;"; return value + 2; });
        const text = String({
            [Symbol.toPrimitive](hint) { trace += hint + ";"; return "abc"; }
        }).toUpperCase();
        output[0] === 42 && map.get(key) === input && set.size === 1 && set.has(key) &&
            text === "ABC" && new Date(0).toISOString() === "1970-01-01T00:00:00.000Z" &&
            trace === "lookup;call;callback;string;";
        "#,
        JsValue::Boolean(true),
    );
}

#[test]
fn jit_baseline_child_realms_execute_classes_and_generators_without_interpreting() {
    assert_baseline_matches(
        r#"
        const realm = __createRealm__().global;
        const Constructor = realm.eval("(class { constructor(value) { this.value = value; } read() { return this.value; } })");
        const object = new Constructor(42);
        const iterator = realm.eval("(function* () { yield { value: 42 }; return 2; })")();
        const first = iterator.next();
        const last = iterator.next();
        object.read() === 42 && first.value.value === 42 &&
            !first.done && last.value === 2 && last.done;
        "#,
        JsValue::Boolean(true),
    );
}

#[test]
fn jit_baseline_modules_preserve_top_level_await_and_live_bindings() {
    let source = r#"
        const sentinel = { value: 40 };
        export let value = await Promise.resolve(sentinel);
        export function increment() { value.value++; }
        increment();
        globalThis.moduleResult = value === sentinel && value.value === 41;
    "#;
    for required in [false, true] {
        let mut runtime = JsRuntime::with_gc_opcode_budget(1).unwrap();
        runtime.set_jit_required(required);
        runtime.eval_module(source, "jit-baseline-module.mjs").unwrap();
        runtime.collect_garbage();
        assert_eq!(runtime.eval("globalThis.moduleResult;").unwrap(), JsValue::Boolean(true));
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_baseline_code_cache_survives_incremental_eval_and_mode_changes() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    runtime.eval("function append(value) { return value + '!'; }").unwrap();
    assert_eq!(runtime.eval("append('first');").unwrap(), JsValue::String("first!".to_string()));
    assert_eq!(runtime.eval("append('second');").unwrap(), JsValue::String("second!".to_string()));
    assert_no_interpreter(&mut runtime);

    runtime.set_jit_required(false);
    assert_eq!(runtime.eval("append('third');").unwrap(), JsValue::String("third!".to_string()));
    let interpreted = runtime.jit_stats().interpreter_opcodes;
    assert!(interpreted > 0);

    runtime.set_jit_required(true);
    assert_eq!(runtime.eval("append('fourth');").unwrap(), JsValue::String("fourth!".to_string()));
    assert_eq!(runtime.jit_stats().interpreter_opcodes, interpreted);
}

#[test]
fn jit_required_mode_matches_math_sqrt_edge_cases() {
    let source = r#"
        function squareRoot(value) { return Math.sqrt(value); }
        squareRoot(81) === 9
            && squareRoot(-1) !== squareRoot(-1)
            && 1 / squareRoot(-0) === -Infinity
            && squareRoot(1 / 0) === Infinity;
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn jit_falls_back_to_interpreter_when_math_sqrt_is_replaced() {
    let mut runtime = JsRuntime::new();
    runtime.eval("function squareRoot(value) { return Math.sqrt(value); }").unwrap();
    runtime.eval("Math.sqrt = function(value) { return 77; };").unwrap();

    let result = runtime.eval("squareRoot(81);").unwrap();

    assert_eq!(result, JsValue::Number(77.0));
}

#[test]
fn jit_required_mode_matches_math_sign_nan_infinity_and_signed_zero() {
    let source = r#"
        function sign(value) { return Math.sign(value); }
        sign(-12) === -1
            && sign(0) === 0
            && 1 / sign(-0) === -Infinity
            && sign(NaN) !== sign(NaN)
            && sign(Infinity) === 1
            && sign(-Infinity) === -1;
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn jit_falls_back_to_interpreter_when_math_sign_is_replaced() {
    let mut runtime = JsRuntime::new();
    runtime.eval("function sign(value) { return Math.sign(value); }").unwrap();
    runtime.eval("Math.sign = function(value) { return 77; };").unwrap();

    let result = runtime.eval("sign(-12);").unwrap();

    assert_eq!(result, JsValue::Number(77.0));
}

#[test]
fn jit_required_mode_matches_remaining_numeric_math_builtins() {
    let source = r#"
        function sin(value) { return Math.sin(value); }
        function cos(value) { return Math.cos(value); }
        function tan(value) { return Math.tan(value); }
        function asin(value) { return Math.asin(value); }
        function acos(value) { return Math.acos(value); }
        function atan(value) { return Math.atan(value); }
        function atan2(y, x) { return Math.atan2(y, x); }
        function sinh(value) { return Math.sinh(value); }
        function cosh(value) { return Math.cosh(value); }
        function tanh(value) { return Math.tanh(value); }
        function asinh(value) { return Math.asinh(value); }
        function acosh(value) { return Math.acosh(value); }
        function atanh(value) { return Math.atanh(value); }
        function exp(value) { return Math.exp(value); }
        function expm1(value) { return Math.expm1(value); }
        function log(value) { return Math.log(value); }
        function log10(value) { return Math.log10(value); }
        function log1p(value) { return Math.log1p(value); }
        function log2(value) { return Math.log2(value); }
        function fround(value) { return Math.fround(value); }
        function f16round(value) { return Math.f16round(value); }
        function cbrt(value) { return Math.cbrt(value); }
        function max(a, b) { return Math.max(a, b); }
        function min(a, b) { return Math.min(a, b); }
        function maxEmpty() { return Math.max(); }
        function minEmpty() { return Math.min(); }
        function hypot(a, b) { return Math.hypot(a, b); }
        function hypotEmpty() { return Math.hypot(); }
        function pow(a, b) { return Math.pow(a, b); }
        function hypotWithNaNAndInfinity(a, b) { return Math.hypot(a, b); }
        function clz32(value) { return Math.clz32(value); }
        function imul(a, b) { return Math.imul(a, b); }
        function sinEmpty() { return Math.sin(); }
        function precise() { return Math.sumPrecise([1e16, 1, -1e16]); }
        function preciseEmpty() { return Math.sumPrecise([]); }
        function preciseLocal() {
            const values = [1e16, 1, -1e16];
            return Math.sumPrecise(values);
        }
        sin(0) === 0
            && cos(0) === 1
            && tan(0) === 0
            && asin(0) === 0
            && acos(1) === 0
            && atan(0) === 0
            && atan2(0, 1) === 0
            && sinh(0) === 0
            && cosh(0) === 1
            && tanh(0) === 0
            && asinh(0) === 0
            && acosh(1) === 0
            && atanh(0) === 0
            && exp(0) === 1
            && expm1(0) === 0
            && log(1) === 0
            && log10(1) === 0
            && log1p(0) === 0
            && log2(1) === 0
            && fround(1.5) === 1.5
            && f16round(1.5) === 1.5
            && cbrt(-8) === -2
            && maxEmpty() === -Infinity
            && max(-0, 0) === 0
            && 1 / max(-0, 0) === Infinity
            && minEmpty() === Infinity
            && 1 / min(-0, 0) === -Infinity
            && hypotEmpty() === 0
            && hypot(3, 4) === 5
            && hypotWithNaNAndInfinity(NaN, Infinity) === Infinity
            && pow(2, 3) === 8
            && pow(42, 0) === 1
            && pow(-1, Infinity) !== pow(-1, Infinity)
            && clz32(1) === 31
            && imul(2147483647, 2) === -2
            && precise() === 1
            && 1 / preciseEmpty() === -Infinity
            && preciseLocal() === 1
            && sinEmpty() !== sinEmpty();
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn jit_required_mode_executes_math_random_through_the_host_helper() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval("function random() { return Math.random(); } random();").unwrap();

    let JsValue::Number(value) = result else {
        panic!("Math.random should return a number");
    };
    assert!((0.0..1.0).contains(&value));
}

#[test]
fn jit_falls_back_to_interpreter_when_math_sin_is_replaced() {
    let mut runtime = JsRuntime::new();
    runtime.eval("function sine(value) { return Math.sin(value); }").unwrap();
    runtime.eval("Math.sin = function(value) { return 77; };").unwrap();

    let result = runtime.eval("sine(0);").unwrap();

    assert_eq!(result, JsValue::Number(77.0));
}

#[test]
fn jit_falls_back_to_interpreter_when_math_sum_precise_is_replaced() {
    let mut runtime = JsRuntime::new();
    runtime.eval("function precise() { return Math.sumPrecise([1, 2, 3]); }").unwrap();
    runtime.eval("Math.sumPrecise = function(values) { return 77; };").unwrap();

    let result = runtime.eval("precise();").unwrap();

    assert_eq!(result, JsValue::Number(77.0));
}

#[test]
fn jit_falls_back_to_interpreter_when_array_iterator_is_replaced_for_sum_precise() {
    let mut runtime = JsRuntime::new();
    runtime.eval("function precise() { return Math.sumPrecise([1, 2, 3]); }").unwrap();
    runtime
        .eval(
            "Array.prototype[Symbol.iterator] = function() { \
                 return { next: function() { return { done: true }; } }; \
             };",
        )
        .unwrap();

    let result = runtime.eval("1 / precise() === -Infinity;").unwrap();

    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn jit_required_mode_uses_baseline_when_sum_precise_iterator_changes() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime
        .eval(
            "function precise() { return Math.sumPrecise([1, 2, 3]); } \
             Array.prototype[Symbol.iterator] = function() { \
                 return { next: function() { return { done: true }; } }; \
             }; \
             precise();",
        )
        .unwrap();

    let JsValue::Number(result) = result else {
        panic!("expected numeric sum")
    };
    assert_eq!(result, 0.0);
    assert!(result.is_sign_negative());
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_required_mode_inlines_numeric_math_floor_and_ceil_with_interpreter_equivalent_results() {
    let source = r#"
        function roundDown(value) { return Math.floor(value); }
        function roundUp(value) { return Math.ceil(value); }
        roundDown(-1.25) === -2
            && 1 / roundUp(-0.25) === -Infinity
            && roundDown(1 / 0) === Infinity
            && roundUp(-1 / 0) === -Infinity;
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn jit_falls_back_to_interpreter_when_math_floor_is_replaced() {
    let mut runtime = JsRuntime::new();
    runtime.eval("function roundDown(value) { return Math.floor(value); }").unwrap();
    runtime.eval("Math.floor = function(value) { return 77; };").unwrap();

    let result = runtime.eval("roundDown(-1.25);").unwrap();

    assert_eq!(result, JsValue::Number(77.0));
}

#[test]
fn jit_required_mode_matches_math_round_ties_and_math_trunc_negative_zero() {
    let source = r#"
        function round(value) { return Math.round(value); }
        function truncate(value) { return Math.trunc(value); }
        round(0.5) === 1
            && round(-1.5) === -1
            && 1 / round(-0.5) === -Infinity
            && 1 / round(-0) === -Infinity
            && truncate(-1.9) === -1
            && 1 / truncate(-0.2) === -Infinity
            && truncate(1 / 0) === Infinity;
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn jit_falls_back_to_interpreter_when_math_round_is_replaced() {
    let mut runtime = JsRuntime::new();
    runtime.eval("function round(value) { return Math.round(value); }").unwrap();
    runtime.eval("Math.round = function(value) { return 77; };").unwrap();

    let result = runtime.eval("round(-1.25);").unwrap();

    assert_eq!(result, JsValue::Number(77.0));
}

#[test]
fn jit_falls_back_to_interpreter_when_math_abs_is_replaced() {
    let mut runtime = JsRuntime::new();
    runtime
        .eval(
            "function absolute(value) { return Math.abs(value); } \
             function wrapped(value) { return absolute(value); }",
        )
        .unwrap();
    runtime.eval("Math.abs = function(value) { return 99; };").unwrap();

    let result = runtime.eval("wrapped(-42);").unwrap();

    assert_eq!(result, JsValue::Number(99.0));
}

#[test]
fn jit_required_mode_uses_baseline_when_math_abs_dependency_changes() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime
        .eval(
            "function absolute(value) { return Math.abs(value); } \
             Math.abs = function(value) { return 99; }; absolute(-42);",
        )
        .unwrap();

    assert_eq!(result, JsValue::Number(99.0));
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_falls_back_when_array_iterator_next_is_replaced_for_sum_precise() {
    let mut runtime = JsRuntime::new();
    runtime.eval("function precise() { return Math.sumPrecise([1, 2, 3]); }").unwrap();
    runtime
        .eval(
            "Array.prototype[Symbol.iterator] = function() { return [1, 2, 3]; }; \
             Object.getPrototypeOf([1, 2, 3][Symbol.iterator]()).next = function() { return { done: true }; };",
        )
        .unwrap();

    let result = runtime.eval("1 / precise() === -Infinity;").unwrap();

    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn jit_required_mode_executes_arrow_closures_without_upvalue_access() {
    let source = "const addOne = value => value + 1; addOne(41);";
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn jit_required_mode_executes_function_expression_closures_without_upvalue_access() {
    let source = "const double = function(value) { return value * 2; }; double(21);";
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn jit_required_mode_executes_generic_arrow_closures_without_upvalue_access() {
    let source = r#"
        const identity = value => value;
        const object = { field: 3 };
        identity(object) === object;
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn jit_required_mode_reads_updated_numeric_closure_upvalues() {
    let setup = r#"
        function makeAdder(captured) {
            const add = function(value) { return value + captured; };
            captured = captured + 1;
            return add;
        }
        var addCaptured = makeAdder(40);
    "#;
    let mut interpreter = JsRuntime::new();
    interpreter.eval(setup).unwrap();
    let expected = interpreter.eval("addCaptured(2);").unwrap();

    let mut runtime = JsRuntime::new();
    runtime.eval(setup).unwrap();
    runtime.set_jit_required(true);
    let result = runtime.eval("addCaptured(2);").unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(43.0));
}

#[test]
fn jit_warmed_numeric_closure_reads_upvalues() {
    let setup = r#"
        function makeAdder(captured) {
            const add = function(value) { return value + captured; };
            captured = captured + 1;
            return add;
        }
        var addCaptured = makeAdder(40);
    "#;
    let mut interpreter = JsRuntime::new();
    interpreter.eval(setup).unwrap();
    let expected = interpreter.eval("addCaptured(2); addCaptured(2);").unwrap();

    let mut runtime = JsRuntime::new();
    runtime.eval(setup).unwrap();
    let result = runtime.eval("addCaptured(2); addCaptured(2);").unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(43.0));
}

#[test]
fn jit_required_mode_writes_numeric_closure_upvalues() {
    let setup = r#"
        function makeCounter(start) {
            let captured = start;
            const increment = function() {
                captured = captured + 1;
                return captured;
            };
            return increment;
        }
        var counter = makeCounter(40);
    "#;
    let mut interpreter = JsRuntime::new();
    interpreter.eval(setup).unwrap();
    let expected = interpreter.eval("counter() + counter();").unwrap();

    let mut runtime = JsRuntime::new();
    runtime.eval(setup).unwrap();
    runtime.set_jit_required(true);
    let result = runtime.eval("counter() + counter();").unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(83.0));
}

#[test]
fn jit_writes_numeric_closure_upvalues_before_propagating_exceptions() {
    let setup = r#"
        function makeCounter() {
            let captured = 40;
            const increment = function(shouldThrow) {
                captured = captured + 1;
                if (shouldThrow) throw 9;
                return captured;
            };
            return increment;
        }
        const counter = makeCounter();
    "#;
    let exercise = r#"
        try {
            counter(1);
        } catch (error) {}
        counter(0);
    "#;
    let mut interpreter = JsRuntime::new();
    interpreter.eval(setup).unwrap();
    let expected = interpreter.eval(exercise).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.eval(setup).unwrap();
    runtime.set_jit_required(true);

    let result = runtime.eval(exercise).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn jit_warmed_numeric_closure_writes_shared_upvalues() {
    let source = r#"
        function makeCounter(start) {
            let captured = start;
            const increment = function() {
                captured = captured + 1;
                return captured;
            };
            return increment;
        }
        const counter = makeCounter(40);
        counter();
        counter() + counter();
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(85.0));
}

#[test]
fn jit_warmed_closures_fall_back_for_non_numeric_upvalues() {
    let setup = r#"
        function makeFormatter(suffix) {
            const format = function(value) { return value + suffix; };
            return format;
        }
        var format = makeFormatter("!");
    "#;
    let mut interpreter = JsRuntime::new();
    interpreter.eval(setup).unwrap();
    let expected = interpreter.eval("format('hello'); format('world');").unwrap();

    let mut runtime = JsRuntime::new();
    runtime.eval(setup).unwrap();
    let result = runtime.eval("format('hello'); format('world');").unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::String("world!".to_string()));
}

#[test]
fn jit_warmed_closure_mutations_fall_back_for_non_numeric_upvalues() {
    let setup = r#"
        function makeSetter(initial) {
            const set = function(next) {
                initial = next;
                return initial;
            };
            return set;
        }
        var setCaptured = makeSetter("initial");
    "#;
    let mut interpreter = JsRuntime::new();
    interpreter.eval(setup).unwrap();
    let expected = interpreter.eval("setCaptured('updated'); setCaptured('again');").unwrap();

    let mut runtime = JsRuntime::new();
    runtime.eval(setup).unwrap();
    let result = runtime.eval("setCaptured('updated'); setCaptured('again');").unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::String("again".to_string()));
}

#[test]
fn jit_required_mode_reads_non_numeric_captured_values() {
    let mut runtime = JsRuntime::new();
    runtime
        .eval(
            r#"
            function makeAdder(captured) {
                const add = function(value) { return value + captured; };
                return add;
            }
            var addCaptured = makeAdder("captured");
            "#,
        )
        .unwrap();
    runtime.set_jit_required(true);
    let before = runtime.jit_stats();

    let result = runtime.eval("addCaptured(2);").unwrap();

    assert_eq!(result, JsValue::String("2captured".to_string()));
    assert_eq!(runtime.jit_stats().interpreter_opcodes, before.interpreter_opcodes);
    assert!(runtime.jit_stats().baseline_opcodes > before.baseline_opcodes);
}

#[test]
fn jit_required_mode_mutates_non_numeric_upvalues() {
    let mut runtime = JsRuntime::new();
    runtime
        .eval(
            r#"
            function makeSetter(initial) {
                const set = function(next) {
                    initial = next;
                    return initial;
                };
                return set;
            }
            var setCaptured = makeSetter("initial");
            "#,
        )
        .unwrap();
    runtime.set_jit_required(true);
    let before = runtime.jit_stats();

    let result = runtime.eval("setCaptured('updated');").unwrap();

    assert_eq!(result, JsValue::String("updated".to_string()));
    assert_eq!(runtime.jit_stats().interpreter_opcodes, before.interpreter_opcodes);
    assert!(runtime.jit_stats().baseline_opcodes > before.baseline_opcodes);
}

#[test]
fn jit_identity_uses_tagged_values_and_rooted_heap_handles() {
    let source = r#"
        function identity(value) { return value; }
        const object = { field: 3 };
        const array = [object];
        const text = "rooted";
        (identity(object) === object ? 1 : 0) +
        (identity(array) === array ? 2 : 0) +
        (identity(text) === text ? 4 : 0) +
        (identity(4n) === 4n ? 8 : 0) +
        (identity(identity) === identity ? 16 : 0) +
        (identity(null) === null ? 32 : 0) +
        (identity(undefined) === undefined ? 64 : 0);
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(127.0));
}

#[test]
fn jit_warmed_identity_preserves_rooted_objects() {
    let mut runtime = JsRuntime::new();

    let result = runtime
        .eval(
            r#"
            function identity(value) { return value; }
            const object = { field: 3 };
            identity(object);
            identity(object) === object;
            "#,
        )
        .unwrap();

    assert_eq!(result, JsValue::Boolean(true));
}

#[test]
fn jit_required_mode_executes_numeric_comparison_branches() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime
        .eval(
            r#"
            function select(value) {
                if (value > 0) {
                    value = 42;
                }
                return value;
            }
            select(5) + select(-2);
            "#,
        )
        .unwrap();

    assert_eq!(result, JsValue::Number(40.0));
}

#[test]
fn jit_required_mode_executes_boolean_conditions_and_not() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime
        .eval(
            r#"
            function choose(value) {
                const isFalsy = !value;
                if (isFalsy) return 7;
                if (false) return 100;
                if (true) return 42;
                return -1;
            }
            choose(0) + choose(1) + choose(NaN);
            "#,
        )
        .unwrap();

    assert_eq!(result, JsValue::Number(56.0));
}

#[test]
fn jit_required_mode_executes_numeric_locals_across_control_flow() {
    let source = r#"
        function calculate(input) {
            let value = input;
            if (value > 0) {
                value = value + 2;
            }
            return value * 3;
        }
        calculate(5) + calculate(-1) + calculate(2);
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(30.0));
}

#[test]
fn jit_required_mode_executes_power_and_numeric_increment_operators() {
    let source = r#"
        function powerAfterRoundTrip(value) {
            let result = value;
            result++;
            result--;
            return result ** 2;
        }
        powerAfterRoundTrip(3) + powerAfterRoundTrip(-2) + powerAfterRoundTrip(0.5);
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(13.25));
}

#[test]
fn jit_required_mode_preserves_power_edge_cases() {
    let source = r#"
        function powerOrNaN(base, exponent) {
            const result = base ** exponent;
            if (result !== result) return 10;
            return result;
        }
        powerOrNaN(1, Infinity) + powerOrNaN(NaN, 0);
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(11.0));
}

#[test]
fn jit_required_mode_matches_number_bitwise_and_shift_semantics() {
    let source = r#"
        function bitwise(value) {
            return (value & 255) + (value | 1) + (value ^ 3) +
                (value << 1) + (value >> 1) + (value >>> 1) + (~value);
        }
        bitwise(-1) + bitwise(4294967297) + bitwise(NaN) + bitwise(Infinity);
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
}

#[test]
fn jit_required_mode_executes_unary_numeric_conversion() {
    let source = "function unary(value) { return +value; } unary(4) + unary(9);";
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(13.0));
}

#[test]
fn jit_compiles_functions_in_chunks_with_try_catch_and_finally() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime
        .eval(
            r#"
            function twice(value) { return value * 2; }
            let cleanup = 0;
            try {
                throw 1;
            } catch (error) {
                cleanup = error;
            } finally {
                cleanup += 1;
            }
            twice(21) + cleanup;
            "#,
        )
        .unwrap();

    assert_eq!(result, JsValue::Number(44.0));
}

#[test]
fn jit_native_throw_reaches_interpreted_catch() {
    let source = r#"
        function guard(value) {
            if (value < 0) throw true;
            return value + 1;
        }
        let caughtValue = false;
        try {
            guard(-1);
        } catch (error) {
            caughtValue = error;
        }
        guard(4) + (caughtValue ? 10 : 0);
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(15.0));
}

#[test]
fn jit_native_undefined_throw_reaches_interpreted_catch() {
    let source = r#"
        function guard(value) {
            if (value < 0) throw void 0;
            return value + 1;
        }
        let caughtUndefined = false;
        try {
            guard(-1);
        } catch (error) {
            caughtUndefined = error === undefined;
        }
        guard(4) + (caughtUndefined ? 10 : 0);
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(15.0));
}

#[test]
fn jit_native_null_throw_reaches_interpreted_catch() {
    let source = r#"
        function guard(value) {
            if (value < 0) throw null;
            return value + 1;
        }
        let caughtNull = false;
        try {
            guard(-1);
        } catch (error) {
            caughtNull = error === null;
        }
        guard(4) + (caughtNull ? 10 : 0);
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(15.0));
}

#[test]
fn jit_native_throw_reaches_supported_in_function_catch() {
    let source = r#"
        function catchWithoutBinding(value) {
            try {
                if (value < -1) throw void 0;
                if (value < 0) throw null;
                return 1;
            } catch {
                return 2;
            }
        }
        catchWithoutBinding(-2) + catchWithoutBinding(-1) + catchWithoutBinding(1);
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(5.0));
}

#[test]
fn jit_required_mode_executes_statically_typed_catch_bindings() {
    let source = r#"
        function catchesLocally() {
            let adjustment = 1;
            try {
                throw 41;
            } catch (error) {
                return error + adjustment;
            }
            return 0;
        }
        function catchesBoolean() {
            try {
                throw true;
            } catch (error) {
                return error ? 42 : 0;
            }
            return 0;
        }
        function catchesUndefined() {
            try {
                throw void 0;
            } catch (error) {
                return error ? 0 : 42;
            }
            return 0;
        }
        function catchesNull() {
            try {
                throw null;
            } catch (error) {
                return !error ? 42 : 0;
            }
            return 0;
        }
        catchesLocally() + catchesBoolean() + catchesUndefined() + catchesNull();
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(168.0));
}

#[test]
fn jit_required_mode_executes_mixed_type_catch_bindings() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime
        .eval(
            r#"
            function catchesMixedValues(condition) {
                try {
                    if (condition) throw 1;
                    throw true;
                } catch (error) {
                    return error ? 1 : 0;
                }
            }
            catchesMixedValues(1);
            "#,
        )
        .unwrap();

    assert_eq!(result, JsValue::Number(1.0));
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_required_mode_executes_catch_binding_when_other_local_is_read_first() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime
        .eval(
            r#"
            function catchesWithLocal() {
                let other = 1;
                try {
                    throw 41;
                } catch (error) {
                    return other + error;
                }
            }
            catchesWithLocal();
            "#,
        )
        .unwrap();

    assert_eq!(result, JsValue::Number(42.0));
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_required_mode_executes_string_addition_in_baseline() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime
        .eval(r#"function append(value) { return value + "!"; } append("js");"#)
        .unwrap();

    assert_eq!(result, JsValue::String("js!".to_string()));
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_required_mode_executes_numeric_coercion_in_baseline() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(r#"function twice(value) { return value * 2; } twice("21");"#).unwrap();

    assert_eq!(result, JsValue::Number(42.0));
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_required_mode_executes_mixed_representation_local_slots() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime
        .eval("function mixed(value) { let result = 1; if (value) result = true; return result; } mixed(1);")
        .unwrap();

    assert_eq!(result, JsValue::Boolean(true));
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_required_mode_executes_bound_functions_in_baseline() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime
        .eval("function twice(value) { return value * 2; } const bound = twice.bind(null, 21); bound();")
        .unwrap();

    assert_eq!(result, JsValue::Number(42.0));
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_required_mode_executes_runtime_callbacks_in_baseline() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime
        .eval("function twice(value) { return value * 2; } [21].map(twice)[0];")
        .unwrap();

    assert_eq!(result, JsValue::Number(42.0));
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_numeric_functions_match_interpreter_results() {
    let mut runtime = JsRuntime::new();
    let result = runtime
        .eval(
            r#"
            function remainder(a, b) {
                return a % b;
            }
            function sumTo(n) {
                let total = 0;
                for (let i = 0; i < n; i++) {
                    total = total + i;
                }
                return total;
            }
            function compoundSum(n) {
                let total = 0;
                for (let i = 0; i < n; i++) {
                    total += i;
                }
                return total;
            }
            function identity(value) {
                return value;
            }
            remainder(-7, 3) + remainder(-8, 3) +
                sumTo(10) + sumTo(5) + compoundSum(10) +
                compoundSum(5) + identity({ value: 5 }).value;
            "#,
        )
        .unwrap();

    assert_eq!(result, JsValue::Number(112.0));
}

#[test]
fn jit_local_arrays_and_objects_match_interpreter_results() {
    let source = r#"
        function objectsAndArrays(n) {
            const values = [3, 5, 7, 11, 13, 17, 19, 23];
            const state = { checksum: 0 };
            for (let i = 0; i < n; i++) {
                const index = i % values.length;
                values[index] = (values[index] + i + state.checksum) % 10007;
                state.checksum += values[index];
            }
            return state.checksum + values[0] + values[values.length - 1];
        }
        objectsAndArrays(12) + objectsAndArrays(5);
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(18222.0));
}

#[test]
fn jit_does_not_assume_reassigned_array_indices_remain_in_bounds() {
    let mut runtime = JsRuntime::new();
    let result = runtime
        .eval(
            r#"
            function growArray(n) {
                const values = [3, 5];
                for (let i = 0; i < n; i++) {
                    let index = i % values.length;
                    index = values.length;
                    values[index] = 17;
                }
                return values.length;
            }
            growArray(1);
            "#,
        )
        .unwrap();

    assert_eq!(result, JsValue::Number(3.0));
}

#[test]
fn jit_inlined_calls_fall_back_when_the_global_helper_changes() {
    let mut runtime = JsRuntime::new();
    let initial_result = runtime
        .eval(
            r#"
            function step(accumulator, i) {
                return (accumulator + (i * 17 % 97) + (accumulator % 31)) % 1000003;
            }
            function functionCalls(n) {
                let accumulator = 1;
                for (let i = 0; i < n; i++) {
                    accumulator = step(accumulator, i);
                }
                return accumulator;
            }
            functionCalls(3) + functionCalls(3);
            "#,
        )
        .unwrap();
    assert_eq!(initial_result, JsValue::Number(152.0));

    let reassigned_result = runtime
        .eval("step = (accumulator, i) => accumulator - i; functionCalls(4);")
        .unwrap();
    assert_eq!(reassigned_result, JsValue::Number(-5.0));
}

#[test]
fn jit_required_mode_executes_statically_inlined_numeric_helpers() {
    let source = r#"
        function offset(value) {
            return value * 2 + 1;
        }
        function combine(value) {
            const first = offset(value);
            return first + offset(first);
        }
        combine(3) + combine(5) + combine(3);
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(78.0));
}

#[test]
fn jit_required_mode_inlines_power_helpers() {
    let source = r#"
        function power(base, exponent) {
            return base ** exponent;
        }
        function combine(value) {
            return power(value, 3) + power(value + 1, 2);
        }
        combine(2) + combine(-2) + power(0.5, 2);
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
    assert_eq!(result, JsValue::Number(10.25));
}

#[test]
fn jit_required_mode_inlines_bitwise_helpers() {
    let source = r#"
        function mask(value, shift) {
            return ((value >>> shift) & 255) ^ (~value);
        }
        function combine(value) {
            return mask(value, 3) + mask(value + 1, 2);
        }
        combine(-1) + combine(4294967297) + mask(NaN, 1);
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval(source).unwrap();

    assert_eq!(result, expected);
}
