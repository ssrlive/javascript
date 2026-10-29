#![cfg(feature = "jit")]

use javascript::{JsRuntime, JsValue};

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
