#![cfg(feature = "jit")]

use javascript::{JsRuntime, JsValue};

#[test]
fn jit_required_mode_executes_compiled_functions_on_the_first_call() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let result = runtime.eval("function twice(value) { return value * 2; } twice(21);").unwrap();

    assert_eq!(result, JsValue::Number(42.0));
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
fn jit_required_mode_rejects_functions_that_would_fall_back() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let error = runtime
        .eval(r#"function append(value) { return value + "!"; } append("js");"#)
        .unwrap_err();

    assert!(error.to_string().contains("JIT-required mode"));
}

#[test]
fn jit_required_mode_rejects_non_numeric_arguments() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let error = runtime
        .eval(r#"function twice(value) { return value * 2; } twice("21");"#)
        .unwrap_err();

    assert!(error.to_string().contains("unsupported arguments"));
}

#[test]
fn jit_required_mode_rejects_wrapped_function_fallbacks() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let error = runtime
        .eval("function twice(value) { return value * 2; } const bound = twice.bind(null, 21); bound();")
        .unwrap_err();

    assert!(error.to_string().contains("wrapped JavaScript function"));
}

#[test]
fn jit_required_mode_rejects_runtime_callback_fallbacks() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let error = runtime
        .eval("function twice(value) { return value * 2; } [21].map(twice);")
        .unwrap_err();

    assert!(error.to_string().contains("runtime callback"));
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
    let mut runtime = JsRuntime::new();
    let result = runtime
        .eval(
            r#"
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
            "#,
        )
        .unwrap();

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
