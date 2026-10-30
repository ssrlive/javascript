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
fn jit_required_mode_rejects_closure_functions_that_read_captured_values() {
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

    let error = runtime.eval("addCaptured(2);").unwrap_err();

    assert!(error.to_string().contains("JIT-required mode"));
}

#[test]
fn jit_required_mode_rejects_closure_mutations_of_non_numeric_upvalues() {
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

    let error = runtime.eval("setCaptured('updated');").unwrap_err();

    assert!(error.to_string().contains("JIT-required mode"));
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
fn jit_required_mode_rejects_handlers_inside_compiled_functions() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let error = runtime
        .eval(
            r#"
            function catchesLocally() {
                try {
                    throw 1;
                } catch (error) {
                    return error;
                }
                return 0;
            }
            catchesLocally();
            "#,
        )
        .unwrap_err();

    assert!(error.to_string().contains("JIT-required mode"));
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
fn jit_required_mode_rejects_mixed_representation_local_slots() {
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);

    let error = runtime
        .eval("function mixed(value) { let result = 1; if (value) result = true; return result; } mixed(1);")
        .unwrap_err();

    assert!(error.to_string().contains("JIT-required mode"));
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
