#![cfg(feature = "jit")]

use javascript::{DEFAULT_GC_OPCODE_BUDGET, JsRuntime, JsValue, evaluate_script_with_jit_required};

fn assert_no_interpreter(runtime: &mut JsRuntime) {
    let stats = runtime.jit_stats();
    assert!(stats.baseline_opcodes > 0, "{stats:?}");
    assert_eq!(stats.interpreter_opcodes, 0, "{stats:?}");
}

fn executed_bytecodes(runtime: &mut JsRuntime) -> u64 {
    let stats = runtime.jit_stats();
    stats.interpreter_opcodes + stats.baseline_opcodes
}

#[test]
fn jit_direct_numeric_callbacks_preserve_throw_boundaries_and_unused_arguments() {
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(
            runtime
                .eval(
                    "function numeric(value,index) { if(index===2) throw value+index; return value*2; } \
             let caught=0; let final=0; try { [3,4,5].map(numeric); } catch(error) { caught=error; } \
             finally { final=11; } caught+final;"
                )
                .unwrap(),
            JsValue::Number(18.0)
        );
        assert_eq!(runtime.jit_stats().optimized_calls, 3);
        runtime.collect_garbage();
        let before = runtime.jit_stats().optimized_calls;
        assert_eq!(
            runtime
                .eval("function total(a,b) { return a+b; } [1,2,3,4].reduce(total,0);")
                .unwrap(),
            JsValue::Number(10.0)
        );
        assert_eq!(runtime.jit_stats().optimized_calls - before, 4);
        assert_eq!(
            runtime
                .eval(
                    "function identity(value) { return value; } const object={n:17}; \
             [object].map(identity)[0]===object;"
                )
                .unwrap(),
            JsValue::Boolean(true)
        );
        assert_eq!(
            runtime
                .eval(
                    "function wide(a,b,c,d,e,f,g,h,i) { return a+b+c+d+e+f+g+h+i; } \
             wide.apply(null,[1,2,3,4,5,6,7,8,9]);"
                )
                .unwrap(),
            JsValue::Number(45.0)
        );
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_array_callback_elements_observe_live_mutations_and_species() {
    let source = r#"
        const result = [];
        const dense = [1,2,3,4];
        result.push(dense.map((value,index,array) => {
            if (index === 0) {
                delete array[1];
                Object.defineProperty(array, "2", {get() { return 17; }, configurable:true});
                array[3] = 23;
                array.push(99);
            }
            return value * 2;
        }).join(","));
        const reduced = [1,2,3,4];
        result.push(reduced.reduce((total,value,index,array) => {
            if (index === 0) { array.length = 2; array[1] = 19; }
            return total + value;
        },0));
        result.push([undefined,2,3].reduce((a,b) => String(a) + b));
        result.push([1,2,3].reduce((a,b) => a+b));
        const inherited = Object.create(Array.prototype);
        Object.defineProperty(inherited, "1", {get() { return 31; }, configurable:true});
        const holes = [7, ,9];
        Object.setPrototypeOf(holes, inherited);
        result.push(holes.map(x=>x+1).join(","), holes.reduce((a,b)=>a+b,0));
        let output;
        const custom = [3,4,5];
        custom.constructor = {[Symbol.species]: function(length) {
            output = new Array(length);
            Object.defineProperty(output, "1", {value:101, configurable:true});
            Object.defineProperty(output, "2", {set() { throw "setter must not run"; }, configurable:true});
            return output;
        }};
        const mapped = custom.map(value=>value*3);
        result.push(mapped===output, mapped.join(","), Object.getOwnPropertyDescriptor(mapped,"1").writable);
        const shared = [11,12,13];
        shared.constructor = {[Symbol.species]: function() { return shared; }};
        result.push(shared.map((value,index) => value+index).join(","));
        const trapped = [];
        const proxySource = [2,4];
        proxySource.constructor = {[Symbol.species]: function(length) {
            return new Proxy(new Array(length), {
                defineProperty(target,key,desc) {
                    trapped.push(key + ":" + desc.writable + ":" + desc.enumerable + ":" + desc.configurable);
                    return Reflect.defineProperty(target,key,desc);
                }
            });
        }};
        result.push(proxySource.map(x=>x+1).join(","), trapped.join(","));
        let caught = false;
        const restricted = [1,2];
        restricted.constructor = {[Symbol.species]: function(length) {
            const out = new Array(length);
            Object.defineProperty(out,"0",{value:7,configurable:false});
            return out;
        }};
        try { restricted.map(x=>x); } catch { caught = true; }
        result.push(caught);
        result.join("|");
    "#;
    let expected = JsRuntime::with_gc_opcode_budget(1).unwrap().eval(source).unwrap();
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(runtime.eval(source).unwrap(), expected);
        runtime.collect_garbage();
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_numeric_calls_preserve_many_arguments_captures_and_errors() {
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(
            runtime
                .eval(
                    "function sum(a,b,c,d,e,f,g,h,i) { return a+b+c+d+e+f+g+h+i; } \
             sum(1,2,3,4,5,6,7,8,9);"
                )
                .unwrap(),
            JsValue::Number(45.0)
        );
        assert!(runtime.jit_stats().optimized_calls > 0);
        let before = runtime.jit_stats().optimized_calls;
        assert_eq!(
            runtime
                .eval(
                    "function make(a,b,c,d,e,f,g,h,i) { \
             return function(x) { a += x; return a+b+c+d+e+f+g+h+i; }; } \
             const update = make(1,2,3,4,5,6,7,8,9); update(10) + update(20);"
                )
                .unwrap(),
            JsValue::Number(130.0)
        );
        assert_eq!(runtime.jit_stats().optimized_calls - before, 2);
        runtime.collect_garbage();
        assert_eq!(runtime.eval("update(0);").unwrap(), JsValue::Number(75.0));
        assert_eq!(
            runtime
                .eval(
                    "function identity(value) { return value; } const object = {value:37}; \
             identity(object) === object && identity('text') === 'text';"
                )
                .unwrap(),
            JsValue::Boolean(true)
        );
        assert_eq!(
            runtime
                .eval(
                    "function makeThrow(value) { return function(delta) { value += delta; \
             if (delta < 0) throw value; return value; }; } \
             const throwing = makeThrow(17); let caught = 0; \
             try { throwing(-3); } catch(error) { caught = error; } \
             caught + throwing(2);"
                )
                .unwrap(),
            JsValue::Number(30.0)
        );
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_array_mutations_preserve_descriptors_prototypes_and_partial_effects() {
    let source = r#"
        const results = [];
        const array = [];
        for (let i = 0; i < 1000; i++) {
            if (array.push(i, i + 1) !== 2 || array.pop() !== i + 1 || array.pop() !== i)
                throw "dense mutation mismatch";
        }
        results.push(array.length, array.pop() === undefined, array.push());
        array.push(1);
        Object.defineProperty(array, "length", {writable:false});
        try { array.push(2); } catch { results.push(array.length, array[0]); }
        try { array.pop(); } catch { results.push(array.length, 0 in array); }

        let calls = 0;
        const prototype = Object.create(Array.prototype);
        Object.defineProperty(prototype, "0", {set(value) { calls += value; }, configurable:true});
        const inherited = [];
        Object.setPrototypeOf(inherited, prototype);
        results.push(inherited.push(7), calls, inherited.hasOwnProperty("0"), inherited.pop());
        delete prototype[0];
        results.push(inherited.push(9), inherited.pop());

        const holes = [1, , 3];
        Object.defineProperty(prototype, "1", {get() { calls++; return 23; }, configurable:true});
        Object.setPrototypeOf(holes, prototype);
        results.push(holes.pop(), holes.pop(), holes.pop(), calls);
        const accessor = [1];
        Object.defineProperty(accessor, "0", {get() { return 19; }, configurable:true});
        results.push(accessor.pop(), accessor.length);
        const fixed = [11];
        Object.defineProperty(fixed, "0", {configurable:false});
        try { fixed.pop(); } catch { results.push(fixed[0], fixed.length); }
        const closed = [];
        Object.preventExtensions(closed);
        try { closed.push(17); } catch { results.push(closed.length); }
        const frozen = Object.freeze([3]);
        try { frozen.pop(); } catch { results.push(frozen[0], frozen.length); }

        const partial = [];
        const partialPrototype = Object.create(Array.prototype);
        Object.defineProperty(partialPrototype, "1", {set() { throw 29; }});
        Object.setPrototypeOf(partial, partialPrototype);
        try { partial.push(5, 6); } catch { results.push(partial[0], partial.length); }
        const target = [];
        const events = [];
        const proxy = new Proxy(target, {
            set(object, key, value) { events.push("set:" + key); return Reflect.set(object, key, value); },
            deleteProperty(object, key) { events.push("delete:" + key); return Reflect.deleteProperty(object, key); }
        });
        proxy.push(31); proxy.pop();
        results.push(events.join(","));
        results.join("|");
    "#;
    let expected = JsRuntime::with_gc_opcode_budget(1).unwrap().eval(source).unwrap();
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(runtime.eval(source).unwrap(), expected);
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_dense_pop_preserves_length_descriptors_and_tail_identity() {
    let source = r#"
        const results = [];
        const object = {value: 7};
        const array = [1, object, undefined];
        array.label = "retained";
        Object.defineProperty(array, "length", {writable: true});
        results.push(array.pop() === undefined, array.pop() === object, array.pop(), array.length,
                     array.label, Object.keys(array).join(","));
        let descriptor = Object.getOwnPropertyDescriptor(array, "length");
        results.push(descriptor.value, descriptor.writable, descriptor.enumerable, descriptor.configurable);
        array.push(9);
        results.push(array.pop(), array.hasOwnProperty("0"), Object.getOwnPropertyDescriptor(array, "0") === undefined);
        array.length = 3;
        results.push(0 in array, 1 in array, 2 in array);
        array[2] = object;
        results.push(array.pop() === object, array.length);

        const prototype = Object.create(Array.prototype);
        let calls = 0;
        Object.defineProperty(prototype, "0", {get() { calls++; return 23; }, configurable:true});
        const inherited = [11];
        Object.setPrototypeOf(inherited, prototype);
        results.push(inherited.pop(), calls);
        inherited.length = 1;
        results.push(inherited.pop(), calls);

        const readonlyElement = [17];
        Object.defineProperty(readonlyElement, "0", {writable:false, configurable:true});
        results.push(readonlyElement.pop(), readonlyElement.length);
        const fixed = [19];
        Object.defineProperty(fixed, "0", {configurable:false});
        try { fixed.pop(); } catch { results.push(fixed[0], fixed.length); }
        const accessor = [29];
        Object.defineProperty(accessor, "0", {get() { calls++; return 31; }, configurable:true});
        results.push(accessor.pop(), accessor.length, calls);
        results.join("|");
    "#;
    let expected = JsValue::String(
        "true|true|1|0|retained|label|0|true|false|false|9|false|true|false|false|false|true|2|11|0|23|1|17|0|19|1|31|0|2".into(),
    );
    for required in [false, true] {
        let mut runtime = JsRuntime::with_gc_opcode_budget(1).unwrap();
        runtime.set_jit_required(required);
        assert_eq!(runtime.eval(source).unwrap(), expected);
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_object_assignment_fastpath_preserves_setter_and_integrity_rules() {
    let source = r#"
        const out = [];
        const plain = { value: 1 };
        plain.value = 5;
        out.push(plain.value);

        Object.preventExtensions(plain);
        plain.value = 9;
        out.push(plain.value);
        try { plain.extra = 1; } catch { out.push("noext", plain.extra === undefined); }

        let setter_calls = 0;
        const proto = { set value(v) { setter_calls += v; } };
        const inherited = Object.create(proto);
        inherited.value = 4;
        out.push(setter_calls, inherited.hasOwnProperty("value"));

        const own_data = Object.create(proto);
        Object.defineProperty(own_data, "value", { value: 3, writable: true, configurable: true, enumerable: true });
        own_data.value = 11;
        out.push(own_data.value, setter_calls);

        const readonly = {};
        Object.defineProperty(readonly, "value", { value: 7, writable: false });
        try { readonly.value = 12; } catch { out.push(readonly.value); }

        const trap_log = [];
        const target = { value: 2 };
        const proxy = new Proxy(target, {
            set(obj, key, value, receiver) {
                trap_log.push(String(key) + ":" + value + ":" + (receiver === proxy));
                return Reflect.set(obj, key, value, receiver);
            }
        });
        proxy.value = 6;
        out.push(target.value, trap_log.join(","));
        out.join("|");
    "#;
    let expected = JsRuntime::with_gc_opcode_budget(1).unwrap().eval(source).unwrap();
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(runtime.eval(source).unwrap(), expected);
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_string_borrowing_preserves_utf16_snapshots_and_coercion_order() {
    let source = r#"
        const results = [];
        const text = "A\ud83d\ude00\ud800Z";
        results.push(text.slice(1, 4).length, text.slice(1, 4).charCodeAt(2),
                     text.charAt(3).charCodeAt(0), text.charCodeAt(2),
                     text.slice(-2).charCodeAt(0), text.substring(3, 1).length,
                     text.slice(Infinity).length, text.slice(-Infinity).length);
        const events = [];
        let current = "abcdef";
        const receiver = {toString() { events.push("receiver"); return current; }};
        const start = {valueOf() {
            events.push("start");
            current = "changed";
            results.push("nested".slice(1, 3));
            return 1;
        }};
        const end = {valueOf() { events.push("end"); return 4; }};
        results.push(String.prototype.slice.call(receiver, start, end), current, events.join(","));
        const boxed = new String("uvwxyz");
        results.push(String.prototype.slice.call(boxed, 1, -1),
                     String.prototype.charCodeAt.call(boxed, 2),
                     String.prototype.indexOf.call(boxed, "xy"),
                     "ab".toUpperCase(), "CD".toLowerCase(), "xy".repeat(2),
                     "ab-cd".split("-").join("|"), "abc".replace("b", () => "X"),
                     "abc".replace("missing", "unused"), " \tX\n".trim());
        let called = 0;
        const numeric = {valueOf() { called++; throw 17; }};
        try { String.prototype.slice.call(null, numeric); } catch { results.push(called); }
        try { "abc".slice(numeric, {valueOf() { called += 100; return 2; }}); }
        catch (error) { results.push(error, called); }
        results.join("|");
    "#;
    let expected = JsValue::String(
        "3|55296|55296|56832|55296|2|0|5|es|bcd|changed|receiver,start,end|vwxy|119|3|AB|cd|xyxy|ab|cd|aXc|abc|X|0|17|1".into(),
    );
    for required in [false, true] {
        let mut runtime = JsRuntime::with_gc_opcode_budget(1).unwrap();
        runtime.set_jit_required(required);
        assert_eq!(runtime.eval(source).unwrap(), expected);
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_shared_regexp_matchers_preserve_live_state_captures_and_reentrancy() {
    let source = r#"
        const first = /(?<word>a+)(b)?/dg;
        const second = /(?<word>a+)(b)?/dg;
        const result = [];
        first.lastIndex = 1;
        const match = first.exec("zaab aa");
        result.push(match[0], match[1], match[2], match.index, first.lastIndex, second.lastIndex,
                    match.groups.word, match.indices[0].join(","), match.indices[1].join(","));
        result.push(RegExp.lastMatch, RegExp.$1, RegExp.$2, RegExp.lastParen, RegExp.leftContext, RegExp.rightContext);
        second.exec("aaa");
        result.push(RegExp.lastMatch, RegExp.$1, RegExp.$2, first.lastIndex, second.lastIndex);
        result.push(first.exec("z") === null, first.lastIndex, RegExp.lastMatch);
        const ordinary = /(x)/;
        let calls = 0;
        ordinary.lastIndex = {valueOf() { calls++; /(nested)/.exec("nested"); return 100; }};
        result.push(ordinary.exec("x")[1], calls, RegExp.lastMatch);
        first.compile("c", "y");
        first.lastIndex = 1;
        result.push(first.exec("ac")[0], first.lastIndex, second.exec("aaaa") === null);
        result.push(/(.)/u.exec("\ud83d\ude00")[1].length, /(.)/.exec("\ud83d\ude00")[1].length);
        result.join("|");
    "#;
    let expected = JsRuntime::with_gc_opcode_budget(1).unwrap().eval(source).unwrap();
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(runtime.eval(source).unwrap(), expected);
        runtime.collect_garbage();
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_native_bitwise_loops_match_conversion_and_shift_semantics() {
    for (value, shift) in [
        ("-1.75", "-1"),
        ("4294967297.75", "32"),
        ("9007199254740991", "33"),
        ("1e30", "1e30"),
        ("NaN", "NaN"),
        ("Infinity", "-Infinity"),
    ] {
        let source = format!(
            "function compute(value, shift) {{ Symbol(); let total = 0; \
             for (let i = 0; i < 2000; i++) total += ((value & i) ^ (value | i)) \
             + (value << shift) + (value >> shift) + (value >>> shift); \
             return total; }} compute({value}, {shift});"
        );
        let expected = JsRuntime::with_gc_opcode_budget(1).unwrap().eval(&source).unwrap();
        for required in [false, true] {
            let mut runtime = JsRuntime::new();
            runtime.set_jit_required(required);
            assert_eq!(runtime.eval(&source).unwrap(), expected, "{value}, {shift}, required={required}");
            assert!(runtime.jit_stats().native_loop_opcodes > 0);
            if required {
                assert_no_interpreter(&mut runtime);
            }
        }
    }
}

#[test]
fn jit_native_uint32_loops_preserve_aliases_offsets_and_assignment_results() {
    let source = r#"
        function compute(n) {
            const buffer = new ArrayBuffer(1056);
            const values = new Uint32Array(buffer, 16, 256);
            const alias = new Uint32Array(buffer);
            const bytes = new Uint8Array(buffer);
            let checksum = 0;
            for (let i = 0; i < n; i++) {
                const index = i & 255;
                values[index] = (values[index] + i * 17 + 3) >>> 0;
                checksum += values[index];
            }
            if (alias[4] !== values[0] || alias[259] !== values[255] || alias[0] !== 0)
                throw "alias/offset mismatch";
            const data = new DataView(buffer);
            if (data.getUint32(16, true) !== values[0] && data.getUint32(16, false) !== values[0])
                throw "buffer bytes mismatch";
            return checksum + values[0] + bytes[0];
        }
        compute(2000);
    "#;
    let mut interpreted = JsRuntime::with_gc_opcode_budget(1).unwrap();
    let expected = interpreted.eval(source).unwrap();
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(runtime.eval(source).unwrap(), expected);
        let stats = runtime.jit_stats();
        assert!(stats.native_loop_opcodes > 50000, "{stats:?}");
        assert_eq!(executed_bytecodes(&mut runtime), executed_bytecodes(&mut interpreted));
        runtime.collect_garbage();
        assert_eq!(runtime.eval("compute(2000);").unwrap(), expected);
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }

    for value in ["-1.5", "4294967297.75", "9007199254740991", "1e30", "NaN", "Infinity", "-Infinity"] {
        let source = format!(
            "function assign(value) {{ const values = new Uint32Array(8); let result = 0; \
             for (let i = 0; i < 2000; i++) {{ const index = i & 7; result = (values[index] = value); }} \
             return [String(result), values[0], values[7]].join(':'); }} assign({value});"
        );
        let expected = JsRuntime::with_gc_opcode_budget(1).unwrap().eval(&source).unwrap();
        for required in [false, true] {
            let mut runtime = JsRuntime::new();
            runtime.set_jit_required(required);
            assert_eq!(runtime.eval(&source).unwrap(), expected, "value={value}, required={required}");
            assert!(runtime.jit_stats().native_loop_opcodes > 0);
        }
    }
}

#[test]
fn jit_native_uint32_loops_guard_unsupported_views_and_effects() {
    for initializer in [
        "new Uint32Array(8)",
        "new Int32Array(256)",
        "new Uint32Array(new SharedArrayBuffer(1024))",
        "new Uint32Array(new ArrayBuffer(1024, {maxByteLength:2048}))",
        "new Proxy(new Uint32Array(256), {get(target, key) { return target[key]; }, set(target,key,value) { target[key]=value; return true; }})",
        "[1,2,3,4,5,6,7,8]",
    ] {
        let source = format!(
            "function compute(n) {{ const values = {initializer}; let total = 0; \
             for (let i = 0; i < n; i++) {{ const index = i & 255; values[index] = i; total += values[index]; }} \
             return String(total); }} compute(2000);"
        );
        let expected = JsRuntime::with_gc_opcode_budget(1).unwrap().eval(&source).unwrap();
        for required in [false, true] {
            let mut runtime = JsRuntime::new();
            runtime.set_jit_required(required);
            assert_eq!(runtime.eval(&source).unwrap(), expected, "{initializer}");
            assert_eq!(runtime.jit_stats().native_loop_opcodes, 0, "{initializer}");
        }
    }
    let source = r#"
        function compute() {
            const values = new Uint32Array(256);
            const alias = new Uint32Array(values.buffer);
            let total = 0;
            for (let i = 0; i < 2000; i++) {
                const index = i & 255;
                values[index] = i;
                total += alias[index];
            }
            return total;
        }
        compute();
    "#;
    let expected = JsRuntime::with_gc_opcode_budget(1).unwrap().eval(source).unwrap();
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(runtime.eval(source).unwrap(), expected);
        assert_eq!(runtime.jit_stats().native_loop_opcodes, 0);
    }
}

#[test]
fn jit_native_uint32_loops_commit_before_each_gc_slice() {
    use javascript::JsRunStatus;
    let source = r#"
        const values = new Uint32Array(256);
        function compute(n) {
            const array = values;
            let total = 0;
            for (let i = 0; i < n; i++) {
                const index = i & 255;
                array[index] = i;
                total += array[index];
            }
            return total;
        }
        compute(1500);
    "#;
    let mut reference = JsRuntime::with_gc_opcode_budget(1).unwrap();
    let expected = reference.eval(source).unwrap();
    for required in [false, true] {
        for budget in [1, 31, 64, 127, 1000] {
            let mut runtime = JsRuntime::new();
            runtime.set_jit_required(required);
            runtime.start_eval(source).unwrap();
            loop {
                let before = executed_bytecodes(&mut runtime);
                let status = runtime.run_slice(budget).unwrap();
                let count = executed_bytecodes(&mut runtime) - before;
                assert!(count > 0 && count <= budget as u64);
                runtime.collect_garbage();
                if let JsRunStatus::Complete(value) = status {
                    assert_eq!(value, expected);
                    break;
                }
                assert_eq!(count, budget as u64);
            }
            assert_eq!(executed_bytecodes(&mut runtime), executed_bytecodes(&mut reference));
            if budget == 1000 {
                assert!(runtime.jit_stats().native_loop_opcodes > 0);
            }
            assert_eq!(runtime.eval("values[0] + values[255];").unwrap(), JsValue::Number(2559.0));
        }
    }
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
fn jit_ordinary_mode_uses_precompiled_functions_on_the_first_eligible_call() {
    let mut runtime = JsRuntime::new();
    assert_eq!(
        runtime.eval("function sum(a, b) { return a + b; } sum(20, 22);").unwrap(),
        JsValue::Number(42.0)
    );
    assert_eq!(runtime.jit_stats().optimized_calls, 1);
    assert!(runtime.jit_stats().interpreter_opcodes > 0);
    assert_eq!(
        runtime.eval("sum('hello', ' world');").unwrap(),
        JsValue::String("hello world".into())
    );
    assert_eq!(runtime.eval("sum(3n, 4n);").unwrap(), JsValue::BigInt(7.into()));
}

#[test]
fn jit_first_call_preserves_inlined_return_dependencies_and_captured_writes() {
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(
            runtime
                .eval(
                    "function absolute(value) { return Math.abs(value); } \
                     function wrapped(value) { return absolute(value); } \
                     Math.abs = value => 17; wrapped(-3);",
                )
                .unwrap(),
            JsValue::Number(17.0)
        );
        let before = runtime.jit_stats().optimized_calls;
        assert_eq!(
            runtime
                .eval(
                    "function make(n) { const update = function(v) { n += v; return n; }; return update; } \
                     const add = make(3); add(4);",
                )
                .unwrap(),
            JsValue::Number(7.0)
        );
        assert!(runtime.jit_stats().optimized_calls > before);
        assert_eq!(runtime.eval("add(2);").unwrap(), JsValue::Number(9.0));
    }
}

#[test]
fn jit_tail_calls_enter_native_functions_without_bypassing_the_callers_return() {
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(
            runtime
                .eval(
                    "function sum(a, b) { return a + b; } \
                 function wrapper(a, b) { if (arguments.length !== 2) throw 17; return sum(a, b); } \
                 const answer = wrapper(20, 22); answer;"
                )
                .unwrap(),
            JsValue::Number(42.0)
        );
        assert_eq!(runtime.jit_stats().optimized_calls, 1);
        assert_eq!(runtime.eval("wrapper('a', 'b');").unwrap(), JsValue::String("ab".into()));
        assert_eq!(
            runtime
                .eval("sum = function(a, b) { throw 37; }; try { wrapper(1, 2); } catch (e) { e; }")
                .unwrap(),
            JsValue::Number(37.0)
        );
    }
}

#[test]
fn jit_tail_called_closures_commit_captures_and_preserve_finally() {
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(
            runtime.eval(
                "function make(start) { let captured = start; const tick = function() { captured = captured + 1; return captured; }; return tick; } \
                 const increment = make(40); \
                 function wrapper() { if (arguments.length) throw 17; return increment(); } \
                 const first = wrapper(); first;"
            ).unwrap(),
            JsValue::Number(41.0)
        );
        assert!(runtime.jit_stats().optimized_calls > 0);
        assert_eq!(
            runtime
                .eval(
                    "let cleanup = 0; function finalizer() { try { return wrapper(); } finally { cleanup++; } } \
                          const next = finalizer(); next + cleanup;"
                )
                .unwrap(),
            JsValue::Number(43.0)
        );
    }
}

#[test]
fn jit_native_tail_calls_preserve_opaque_identity_and_throwing_capture_updates() {
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(
            runtime
                .eval(
                    "function identity(value) { return value; } \
                     function wrapper(value) { if (arguments.length !== 1) throw 17; return identity(value); } \
                     const sentinel = { nested: [1, 2, 3] }; const result = wrapper(sentinel); result === sentinel;",
                )
                .unwrap(),
            JsValue::Boolean(true)
        );
        assert_eq!(runtime.jit_stats().optimized_calls, 1);
        assert_eq!(runtime.eval("result.nested === sentinel.nested;").unwrap(), JsValue::Boolean(true));
        let before = runtime.jit_stats().optimized_calls;
        assert_eq!(
            runtime
                .eval(
                    "function makeCounter() { \
                         let count = 40; \
                         const tick = function(flag) { count = count + 1; if (flag) throw count; return count; }; \
                         return tick; \
                     } \
                     const counter = makeCounter(); \
                     function invoke(flag) { if (arguments.length !== 1) throw 17; return counter(flag); } \
                     let caught = 0; try { invoke(1); } catch (error) { caught = error; } \
                     const next = invoke(0); next + caught;",
                )
                .unwrap(),
            JsValue::Number(83.0)
        );
        assert_eq!(runtime.jit_stats().optimized_calls - before, 2);
    }
}

#[test]
fn jit_integer_remainder_matches_number_semantics_at_conversion_boundaries() {
    let source = "
        function remainder(a, b) { return a % b; }
        function compound(a, b) { a %= b; return a; }
        function loopRemainder(a, b) { let result = 0; for (let i = 0; i < 1; i++) result = a % b; return result; }
        const values = [0, -0, 1, -1, 7, -7, 0.5, -0.5, 97, 2147483648,
                        2147483647, -2147483648, -2147483649, -2147483647,
                        9007199254740991, 9007199254740992, 9223372036854774784,
                        9223372036854775808, -9223372036854775808,
                        -9223372036854777856, 1e100, -1e100, 1e-300, -1e-300,
                        Infinity, -Infinity, NaN];
        let valid = true;
        for (const a of values) {
            for (const b of values) {
                const expected = a % b;
                if (!Object.is(remainder(a, b), expected)
                    || !Object.is(compound(a, b), expected)
                    || !Object.is(loopRemainder(a, b), expected)) valid = false;
            }
        }
        valid;
    ";
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(runtime.eval(source).unwrap(), JsValue::Boolean(true));
        assert!(runtime.jit_stats().optimized_calls >= 27 * 27 * 3);
    }
}

#[test]
fn jit_numeric_index_fastpaths_preserve_array_holes_accessors_and_proxy_receivers() {
    let source = "
        const array = [11, 22, 33];
        const results = [array[0], array[-0], array[1]];
        Object.defineProperty(array, '1', { get() { return this[0] + 9; } });
        results.push(array[1]);
        delete array[2];
        const prototype = Object.create(Array.prototype);
        prototype[2] = 71;
        Object.setPrototypeOf(array, prototype);
        results.push(array[2], array[3], array[1.5]);
        let keys = '';
        const proxy = new Proxy(array, { get(target, key, receiver) { keys += key + ','; return Reflect.get(target, key, receiver); } });
        results.push(proxy[1], proxy[-0]);
        let coercions = 0;
        results.push(array[{ toString() { coercions++; return '0'; } }]);
        results.join(':') + '/' + keys + '/' + coercions;
    ";
    assert_baseline_matches(source, JsValue::String("11:11:22:20:71:::20:11:11/1,0,0,/1".into()));
}

#[test]
fn jit_typed_numeric_index_fastpaths_preserve_keys_coercion_and_aliasing() {
    let source = "
        const buffer = new ArrayBuffer(16);
        const array = new Uint32Array(buffer, 4, 2);
        const alias = new Uint32Array(buffer);
        const value = (array[-0] = 4294967297);
        array[1] = -1;
        let conversions = 0;
        array[99] = { valueOf() { conversions++; return 9; } };
        array[NaN] = { valueOf() { conversions++; return 9; } };
        array[-1] = { valueOf() { conversions++; return 9; } };
        array[Infinity] = { valueOf() { conversions++; return 9; } };
        array[1.5] = { valueOf() { conversions++; return 9; } };
        array['-0'] = 77;
        array['1.0'] = 83;
        const key = { toString() { conversions++; return '1'; } };
        array[key] = 7;
        const results = [value, array[0], alias[1], array[1], alias[2],
                         array['-0'], array['1.0'], array[NaN], array[Infinity], array[-1], array[1.5], conversions];
        let errors = 0;
        try { array[99] = 1n; } catch (error) { if (error instanceof TypeError) errors++; }
        const bigint = new BigInt64Array(1);
        bigint[-0] = 18446744073709551615n;
        try { bigint[99] = 1; } catch (error) { if (error instanceof TypeError) errors++; }
        results.join(':') + '/' + bigint[0] + '/' + errors;
    ";
    assert_baseline_matches(source, JsValue::String("4294967297:1:1:7:7::83:::::6/-1/2".into()));
}

#[test]
fn jit_typed_numeric_index_fastpaths_recheck_buffers_after_value_coercion() {
    let source = "
        const buffer = new ArrayBuffer(16, { maxByteLength: 32 });
        const tracking = new Uint32Array(buffer);
        const fixed = new Uint32Array(buffer, 4, 2);
        tracking[0] = 11; tracking[1] = 22; fixed[1] = 33;
        const results = [fixed[0], fixed[1]];
        tracking[3] = { valueOf() { buffer.resize(8); return 99; } };
        results.push(tracking[0], tracking[1], tracking[2], fixed[0], fixed[1]);
        buffer.resize(16);
        results.push(tracking[2], fixed[0], fixed[1]);
        fixed[1] = { valueOf() { buffer.resize(4); return 88; } };
        results.push(fixed[1], tracking[0]);
        buffer.resize(16);
        results.push(fixed[1]);
        const detached = new Uint8Array([7]);
        const transferred = detached.buffer.transfer();
        let conversions = 0;
        detached[0] = { valueOf() { conversions++; return 9; } };
        results.push(detached[0], conversions, new Uint8Array(transferred)[0]);
        results.join(':');
    ";
    assert_baseline_matches(source, JsValue::String("22:33:11:22::::0:22:0::11:0::1:7".into()));
}

#[test]
fn jit_typed_index_fastpaths_preserve_all_element_encodings_and_shared_aliases() {
    let source = "
        const constructors = [Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array,
                              Int32Array, Uint32Array, Float16Array, Float32Array, Float64Array];
        const values = [-0, -1, 257, 1.5, -2.5, Infinity, NaN];
        let valid = true;
        for (const Constructor of constructors) {
            for (const value of values) {
                const array = new Constructor(2);
                const expected = new Constructor([value]);
                array[1] = value;
                const alias = new Constructor(array.buffer);
                if (!Object.is(array[1], expected[0]) || !Object.is(alias[1], expected[0])) valid = false;
                const bytes = new Uint8Array(array.buffer);
                const expectedBytes = new Uint8Array(expected.buffer);
                for (let i = 0; i < expectedBytes.length; i++) {
                    if (bytes[i + Constructor.BYTES_PER_ELEMENT] !== expectedBytes[i]) valid = false;
                }
            }
        }
        for (const Constructor of [BigInt64Array, BigUint64Array]) {
            const array = new Constructor(2);
            array[1] = -1n;
            const alias = new Constructor(array.buffer);
            const expected = new Constructor([-1n]);
            if (array[1] !== expected[0] || alias[1] !== expected[0]) valid = false;
        }
        const shared = new SharedArrayBuffer(16);
        const first = new Uint32Array(shared);
        const second = new Uint32Array(shared);
        first[-0] = 4294967297;
        Atomics.store(second, 1, 77);
        valid && second[0] === 1 && first[1] === 77;
    ";
    assert_baseline_matches(source, JsValue::Boolean(true));
}

#[test]
fn jit_builtin_callbacks_use_guarded_native_entries_with_extra_arguments() {
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(
            runtime
                .eval(
                    "function triple(value) { return value * 3 + 1; } \
                 function sum(total, value) { return total + value; } \
                 [1, 2, 3, 4].map(triple).reduce(sum, 0);"
                )
                .unwrap(),
            JsValue::Number(34.0)
        );
        assert_eq!(runtime.jit_stats().optimized_calls, 8);
        let before = runtime.jit_stats().optimized_calls;
        assert_eq!(
            runtime
                .eval("[1, 2, 3, 4].map(value => value * 3 + 1).reduce((total, value) => total + value, 0);")
                .unwrap(),
            JsValue::Number(34.0)
        );
        assert_eq!(runtime.jit_stats().optimized_calls - before, 8);
        assert_eq!(
            runtime
                .eval(
                    "function needsArguments(value) { return arguments.length + this.bias + value; } \
                 [1, 2].map(needsArguments, { bias: 10 }).join(',');"
                )
                .unwrap(),
            JsValue::String("14,15".into())
        );
        assert_eq!(
            runtime.eval("[1, 'x', 3].map(triple).join(',');").unwrap(),
            JsValue::String("4,NaN,10".into())
        );
    }
}

#[test]
fn jit_builtin_native_callbacks_preserve_capture_writes_throw_cleanup_and_roots() {
    for required in [false, true] {
        let mut runtime = JsRuntime::with_gc_opcode_budget(1).unwrap();
        runtime.set_jit_required(required);
        assert_eq!(
            runtime
                .eval(
                    "function make() { let state = 40; \
                   const update = function(value) { state = state + value; if (value < 0) throw state; return state; }; \
                   return update; } \
                 const update = make(); let caught = 0; let cleanup = 0; \
                 try { [1, -1, 9].map(update); } catch (error) { caught = error; } finally { cleanup++; } \
                 [caught, cleanup, [2].map(update)[0]].join(',');"
                )
                .unwrap(),
            JsValue::String("40,1,42".into())
        );
        assert!(runtime.jit_stats().optimized_calls >= 3);
        assert_eq!(
            runtime
                .eval(
                    "function identity(value) { return value; } \
                 const object = { link: [7] }; const copied = [object].map(identity); copied[0] === object;"
                )
                .unwrap(),
            JsValue::Boolean(true)
        );
        assert_eq!(runtime.eval("copied[0].link === object.link;").unwrap(), JsValue::Boolean(true));
    }
}

#[test]
fn jit_ordinary_data_properties_preserve_descriptors_and_receiver_semantics() {
    let source = "
        const state = { value: 1, method(delta) { this.value += delta; return this.value; } };
        const output = [state.method(2)];
        Object.defineProperty(state, 'value', { value: 7, writable: true, enumerable: false, configurable: true });
        output.push(state.method(3), Object.getOwnPropertyDescriptor(state, 'value').enumerable);
        Object.preventExtensions(state);
        state.value = 12;
        output.push(state.value);
        Object.defineProperty(state, 'value', { writable: false });
        let errors = 0;
        try { state.value = 99; } catch (error) { if (error instanceof TypeError) errors++; }
        output.push(state.value, errors);
        const target = { value: 17 };
        const receiver = { value: 31 };
        Reflect.set(target, 'value', 47, receiver);
        output.push(target.value, receiver.value);
        let calls = 0;
        const accessor = { get value() { calls++; return this.other; }, set value(v) { calls++; this.other = v; }, other: 5 };
        accessor.value = 8;
        output.push(accessor.value, calls);
        const proxy = new Proxy(target, {
            get(object, key, receiver) { calls++; return Reflect.get(object, key, receiver) + 1; },
            set(object, key, value, receiver) { calls++; return Reflect.set(object, key, value * 2, receiver); }
        });
        proxy.value = 9;
        output.push(proxy.value, target.value, calls);
        const frozen = Object.freeze({ value: 73 });
        try { frozen.value = 99; } catch (error) { if (error instanceof TypeError) errors++; }
        output.push(frozen.value, errors);
        output.join(',');
    ";
    assert_baseline_matches(source, JsValue::String("3,10,false,12,12,1,17,47,8,2,19,18,4,73,2".into()));
}

#[test]
fn jit_ordinary_data_properties_preserve_globals_fields_and_prototype_changes() {
    let source = "
        var globalValue = 7;
        globalThis.globalValue = 11;
        const output = [globalValue];
        const prototype = { value: 17 };
        const object = Object.create(prototype);
        output.push(object.value);
        object.value = 19;
        prototype.value = 23;
        output.push(object.value);
        delete object.value;
        output.push(object.value);
        Object.defineProperty(prototype, 'value', { get() { return this.own + 5; } });
        object.own = 31;
        output.push(object.value);
        class Base { set value(v) { output.push(v); } }
        class Derived extends Base { value = 37; }
        const derived = new Derived();
        output.push(derived.value);
        const boxed = new String('abc');
        output.push(boxed.length, boxed[0]);
        const callable = function(value) { return value; };
        output.push(callable.prototype.constructor === callable);
        output.join(',');
    ";
    assert_baseline_matches(source, JsValue::String("11,17,19,23,36,37,3,a,true".into()));
}

#[test]
fn jit_string_index_of_borrows_primitive_utf16_and_coerces_objects() {
    let source = r#"
        let calls = 0;
        const needle = {toString() { calls++; return "x"; }};
        const text = "\uD800x";
        [text.indexOf("\uD800"), text.indexOf(needle), calls].join("|");
    "#;
    let expected = JsValue::String("0|1|1".into());
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        assert_eq!(runtime.eval(source).unwrap(), expected);
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_numeric_collection_hints_preserve_mutation_order_and_same_value_zero() {
    let source = "
        const map = new Map();
        const set = new Set();
        let valid = true;
        for (let i = 0; i < 64; i++) { map.set(i, i * 3); set.add(i); }
        for (let i = 0; i < 64; i++) {
            if (map.get(i) !== i * 3 || !map.has(i) || !set.has(i)) valid = false;
        }
        for (let i = 0; i < 64; i += 2) {
            if (!map.delete(i) || !set.delete(i) || map.has(i) || set.has(i)) valid = false;
        }
        for (let i = 1; i < 64; i += 2) {
            if (map.get(i) !== i * 3 || !set.has(i)) valid = false;
            map.set(i, i * 5);
        }
        map.set(NaN, 11); set.add(NaN);
        map.set(-0, 17); set.add(-0);
        if (map.get(0) !== 17 || map.get(Number('not a number')) !== 11 || !set.has(0) || !set.has(NaN)) valid = false;
        map.delete(NaN); set.delete(NaN);
        map.set(NaN, 19); set.add(NaN);
        if (map.get(NaN) !== 19 || !set.has(NaN)) valid = false;
        const expectedOrder = [];
        for (let i = 1; i < 64; i += 2) expectedOrder.push(i);
        expectedOrder.push(0, NaN);
        if (Array.from(map.keys()).join(',') !== expectedOrder.join(',') ||
            Array.from(set.values()).join(',') !== expectedOrder.join(',')) valid = false;
        map.clear(); set.clear();
        for (let i = 63; i >= 0; i--) { map.set(i, -i); set.add(i); }
        for (let i = 0; i < 64; i++) {
            if (map.get(i) !== -i || !set.has(i)) valid = false;
        }
        const object = {};
        const symbol = Symbol('key');
        for (const key of [object, symbol, '17', 17n, true, null, undefined]) { map.set(key, key); set.add(key); }
        for (const key of [object, symbol, '17', 17n, true, null, undefined]) {
            if (!Object.is(map.get(key), key) || !set.has(key)) valid = false;
        }
        let noCoercion = 0;
        const key = { valueOf() { noCoercion++; return 17; }, toString() { noCoercion++; return '17'; } };
        if (map.has(key) || set.has(key) || noCoercion !== 0) valid = false;
        valid;
    ";
    assert_baseline_matches(source, JsValue::Boolean(true));
}

#[test]
fn jit_numeric_collection_hints_preserve_reentrant_foreach_and_gc_isolation() {
    for required in [false, true] {
        let mut runtime = JsRuntime::with_gc_opcode_budget(1).unwrap();
        runtime.set_jit_required(required);
        assert_eq!(
            runtime
                .eval(
                    "const map = new Map(); const set = new Set(); \
                 for (let i = 0; i < 32; i++) { map.set(i, i); set.add(i); } \
                 for (let i = 0; i < 32; i++) { map.get(i); set.has(i); } \
                 const visited = []; map.forEach((value, key) => { \
                     visited.push(key); if (key === 0) { map.delete(1); map.set(32, 32); } \
                 }); \
                 const setVisited = []; set.forEach(value => { \
                     setVisited.push(value); if (value === 0) { set.delete(1); set.add(32); } \
                 }); \
                 visited.join(',') === setVisited.join(',') && visited.length === 32 && map.get(32) === 32;"
                )
                .unwrap(),
            JsValue::Boolean(true)
        );
        assert_eq!(
            runtime
                .eval(
                    "let correct = true; for (let round = 0; round < 40; round++) { \
                    const temporary = new Map(); const temporarySet = new Set(); \
                    for (let i = 0; i < 40; i++) { temporary.set(i, round * 100 + i); temporarySet.add(i); } \
                    for (let i = 0; i < 40; i++) { if (temporary.get(i) !== round * 100 + i || !temporarySet.has(i)) correct = false; } \
                 } correct && map.get(17) === 17 && set.has(17);"
                )
                .unwrap(),
            JsValue::Boolean(true)
        );
    }
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
fn jit_baseline_short_templates_preserve_exact_slice_budgets() {
    use javascript::JsRunStatus;

    let source = "let total = 0; const state = { value: 0 }; \
                  for (let i = 0; i < 20; i++) { state.value = i; total += state.value; } \
                  total;";
    for budget in [1, 3, 7, 8, 9, 16, 31, 32, 33] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(true);
        runtime.start_eval(source).unwrap();
        loop {
            let before = runtime.jit_stats().baseline_opcodes;
            let status = runtime.run_slice(budget).unwrap();
            let executed = runtime.jit_stats().baseline_opcodes - before;
            assert!(executed > 0 && executed <= budget as u64, "budget={budget}, executed={executed}");
            runtime.collect_garbage();
            match status {
                JsRunStatus::Yielded => assert_eq!(executed, budget as u64),
                JsRunStatus::Complete(value) => {
                    assert_eq!(value, JsValue::Number(190.0));
                    break;
                }
            }
        }
        assert_no_interpreter(&mut runtime);
    }
}

#[test]
fn jit_lazy_iteration_cells_preserve_shared_mutations_and_old_bindings() {
    let source = r#"
        const readers = [];
        const writers = [];
        function* build() {
            for (let i = 0; i < 4; i++) {
                readers.push(() => i);
                writers.push(value => { i = value; });
                yield i;
            }
        }
        const generator = build();
        generator.next();
        writers[0](1);
        generator.next();
        writers[0](20);
        generator.next();
        generator.next();
        readers.map(read => read()).join(',');
    "#;
    for budget in [1, DEFAULT_GC_OPCODE_BUDGET] {
        let mut interpreter = JsRuntime::with_gc_opcode_budget(budget).unwrap();
        assert_eq!(interpreter.eval(source).unwrap(), JsValue::String("20,2,3".into()));
        let mut runtime = JsRuntime::with_gc_opcode_budget(budget).unwrap();
        runtime.set_jit_required(true);
        assert_eq!(runtime.eval(source).unwrap(), JsValue::String("20,2,3".into()));
        assert_no_interpreter(&mut runtime);
    }
}

#[test]
fn jit_lazy_iteration_bindings_preserve_capture_continue_and_finally() {
    let source = r#"
        function build() {
            const readers = [];
            for (let i = 0; i < 5; i++) {
                try {
                    if (i === 1) continue;
                    if (i === 4) break;
                    readers.push(() => i);
                } finally {
                    if (i === 2) i = 3;
                }
            }
            return readers.map(read => read()).join(',');
        }
        build();
    "#;
    assert_baseline_matches(source, JsValue::String("0,3".into()));
}

#[test]
fn jit_lazy_iteration_bindings_preserve_destructuring_identity_and_tdz() {
    let source = r#"
        const readers = [];
        for (const [value] of [[{ n: 1 }], [{ n: 2 }], [{ n: 3 }]]) {
            readers.push(() => value);
        }
        const first = readers[0]();
        first.n = 9;
        const keys = [];
        for (let key in { a: 1, b: 2 }) keys.push(() => key);
        let tdz = false;
        try {
            for (let value of [value]) {}
        } catch (error) {
            tdz = error instanceof ReferenceError;
        }
        [readers[0]() === first, readers.map(read => read().n).join(','),
         keys.map(read => read()).join(','), tdz].join(';');
    "#;
    assert_baseline_matches(source, JsValue::String("true;9,2,3;a,b;true".into()));
}

#[test]
fn jit_lazy_iteration_bindings_survive_gc_between_yields() {
    let source = r#"
        const readers = [];
        function* build() {
            for (let i = 0; i < 4; i++) {
                if (i > 0) readers.push(() => i);
                yield { value: i };
            }
        }
        const iterator = build();
    "#;
    for required in [false, true] {
        let mut runtime = JsRuntime::with_gc_opcode_budget(1).unwrap();
        runtime.set_jit_required(required);
        runtime.eval(source).unwrap();
        for index in 0..4 {
            assert_eq!(runtime.eval("iterator.next().value.value;").unwrap(), JsValue::Number(index as f64));
            runtime.collect_garbage();
        }
        assert_eq!(runtime.eval("iterator.next().done;").unwrap(), JsValue::Boolean(true));
        runtime.collect_garbage();
        assert_eq!(
            runtime.eval("readers.map(read => read()).join(',');").unwrap(),
            JsValue::String("1,2,3".into())
        );
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_native_numeric_loops_keep_locals_in_registers_across_backedges() {
    let source = r#"
        function* compute() {
            let total = 0;
            for (let i = 0; i < 2000; i++) total = total + i * 2 - i / 2;
            yield total;
        }
        compute().next().value;
    "#;
    let mut interpreter = JsRuntime::new();
    assert_eq!(interpreter.eval(source).unwrap(), JsValue::Number(2998500.0));
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    assert_eq!(runtime.eval(source).unwrap(), JsValue::Number(2998500.0));
    let stats = runtime.jit_stats();
    assert!(stats.native_loop_opcodes > 20000, "{stats:?}");
    assert!(
        interpreter.jit_stats().native_loop_opcodes > 20000,
        "ordinary mode must activate hot loops too"
    );
    assert_eq!(stats.baseline_opcodes, executed_bytecodes(&mut interpreter));
    assert_eq!(stats.optimized_calls, 0);
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_native_numeric_loops_exit_at_exact_slice_budgets() {
    use javascript::JsRunStatus;
    let source = r#"
        function compute() {
            Symbol();
            let total = 0;
            for (let i = 0; i < 1500; i++) total = total + i * 2 - i / 2;
            return total;
        }
        compute();
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    for (required, budget) in [false, true]
        .into_iter()
        .flat_map(|required| [1, 7, 31, 32, 33, 64, 127, 1000].into_iter().map(move |budget| (required, budget)))
    {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        runtime.start_eval(source).unwrap();
        loop {
            let before = executed_bytecodes(&mut runtime);
            let status = runtime.run_slice(budget).unwrap();
            let executed = executed_bytecodes(&mut runtime) - before;
            assert!(executed > 0 && executed <= budget as u64, "budget={budget}, executed={executed}");
            runtime.collect_garbage();
            if let JsRunStatus::Complete(value) = status {
                assert_eq!(value, expected, "budget={budget}");
                break;
            }
            assert_eq!(executed, budget as u64, "budget={budget}");
        }
        let stats = runtime.jit_stats();
        assert_eq!(
            executed_bytecodes(&mut runtime),
            executed_bytecodes(&mut interpreter),
            "required={required}, budget={budget}"
        );
        if budget == 1000 {
            assert!(stats.native_loop_opcodes > 0, "{stats:?}");
        }
        if required {
            assert_no_interpreter(&mut runtime);
        } else {
            assert!(stats.interpreter_opcodes > 0);
        }
    }
}

#[test]
fn jit_native_numeric_loops_read_live_operands_after_incremental_eval() {
    for required in [false, true] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(required);
        for multiplier in [2, 17, -3] {
            let source = format!(
                "function* compute() {{ let total = 0; \
                 for (let i = 0; i < 80; i++) total = total + i * {multiplier}; \
                 yield total; }} compute().next().value;"
            );
            let before = runtime.jit_stats().native_loop_opcodes;
            assert_eq!(runtime.eval(source).unwrap(), JsValue::Number(3160.0 * multiplier as f64));
            assert!(runtime.jit_stats().native_loop_opcodes > before);
        }
        if required {
            assert_no_interpreter(&mut runtime);
        }
    }
}

#[test]
fn jit_native_numeric_loops_preserve_captured_locals_and_observable_coercion() {
    let source = r#"
        let calls = 0;
        function* captured() {
            let total = 0;
            const read = () => total;
            for (let i = 0; i < 120; i++) total += i;
            yield read();
        }
        function* coercion(value) {
            let total = 0;
            for (let i = 0; i < 120; i++) total += value * 2;
            yield total;
        }
        const value = { valueOf() { calls++; return 3; } };
        [captured().next().value, coercion(value).next().value, calls].join(',');
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    assert_eq!(expected, JsValue::String("7140,720,120".into()));
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    assert_eq!(runtime.eval(source).unwrap(), expected);
    assert!(runtime.jit_stats().native_loop_opcodes > 0);
    assert_eq!(runtime.jit_stats().baseline_opcodes, executed_bytecodes(&mut interpreter));
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_native_numeric_loops_preserve_ieee_values_and_cached_type_guards() {
    let source = r#"
        function* compute(seed, limit) {
            let total = seed;
            for (let i = 0; i < limit; i++) total = (total + i) * 2 - i / 2;
            yield total;
        }
        compute(0, 100).next();
        const results = [];
        for (const limit of [0, 1, 31, 32, 33, 1023, 1024, 1025, 2049]) {
            results.push(String(compute(0, limit).next().value));
        }
        for (const value of [-0, NaN, Infinity, -Infinity, '2', undefined, 2n, Symbol()]) {
            try {
                const result = compute(value, 40).next().value;
                results.push(Object.is(result, -0) ? '-0' : String(result));
            } catch (error) {
                results.push(error.name);
            }
        }
        results.join(';');
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    assert_eq!(runtime.eval(source).unwrap(), expected);
    let stats = runtime.jit_stats();
    assert!(stats.native_loop_opcodes > 0);
    assert_eq!(stats.baseline_opcodes, executed_bytecodes(&mut interpreter));
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_native_numeric_loops_preserve_caught_throws_finally_and_coercion_counts() {
    let source = r#"
        const sentinel = {};
        let calls = 0;
        let finalized = 0;
        function* compute(value) {
            let total = 0;
            try {
                for (let i = 0; i < 80; i++) total += value * 2;
                yield total;
            } finally { finalized++; }
        }
        const warm = compute(2);
        warm.next();
        warm.next();
        const throwing = { valueOf() { calls++; throw sentinel; } };
        let caught = false;
        try { compute(throwing).next(); } catch (error) { caught = error === sentinel; }
        [caught, calls, finalized].join(',');
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    assert_eq!(expected, JsValue::String("true,1,2".into()));
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    assert_eq!(runtime.eval(source).unwrap(), expected);
    assert!(runtime.jit_stats().native_loop_opcodes > 0);
    assert_eq!(runtime.jit_stats().baseline_opcodes, executed_bytecodes(&mut interpreter));
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_native_numeric_infinite_loops_keep_yielding_to_gc_slices() {
    use javascript::JsRunStatus;
    let source = "function compute() { Symbol(); let i = 0; while (i >= 0) { i = i + 1; } } compute();";
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    runtime.start_eval(source).unwrap();
    for _ in 0..80 {
        let before = runtime.jit_stats().baseline_opcodes;
        assert!(matches!(runtime.run_slice(128).unwrap(), JsRunStatus::Yielded));
        assert_eq!(runtime.jit_stats().baseline_opcodes - before, 128);
        runtime.collect_garbage();
    }
    assert!(runtime.jit_stats().native_loop_opcodes > 0);
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_numeric_regions_keep_intermediate_values_out_of_the_vm_stack() {
    let source = r#"
        function* compute() {
            let total = 0;
            for (let i = 0; i < 80; i++) {
                total = total + i * 2 - i / 2;
            }
            yield total;
        }
        compute().next().value;
    "#;
    let mut interpreter = JsRuntime::new();
    assert_eq!(interpreter.eval(source).unwrap(), JsValue::Number(4740.0));
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    assert_eq!(runtime.eval(source).unwrap(), JsValue::Number(4740.0));
    let stats = runtime.jit_stats();
    assert!(stats.native_region_opcodes > 0, "{stats:?}");
    assert_eq!(stats.baseline_opcodes, executed_bytecodes(&mut interpreter));
    assert_eq!(
        stats.optimized_calls, 0,
        "exercise the baseline, not the existing function optimizer"
    );
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_numeric_regions_guard_types_without_replaying_coercions() {
    for (value, expected) in [
        ("2", "7"),
        ("'2'", "25"),
        ("2n", "throw:TypeError"),
        ("undefined", "NaN"),
        ("Symbol()", "throw:TypeError"),
        ("{ valueOf() { trace += 'v'; return 2; } }", "7v"),
        ("{ valueOf() { trace += 'v'; throw new Error('stop'); } }", "throw:Errorv"),
    ] {
        let source = format!(
            r#"
            let trace = '';
            function* compute(value) {{
                yield (1 + value) * 2 + 1;
            }}
            for (let i = 0; i < 40; i++) compute(2).next();
            let result;
            try {{ result = String(compute({value}).next().value); }}
            catch (error) {{ result = 'throw:' + error.name; }}
            result + trace;
            "#
        );
        let mut interpreter = JsRuntime::new();
        let reference = interpreter.eval(&source).unwrap();
        assert_eq!(reference, JsValue::String(expected.to_string()), "{value}");
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(true);
        assert_eq!(runtime.eval(&source).unwrap(), reference, "{value}");
        assert!(runtime.jit_stats().native_region_opcodes > 0, "{value}");
        assert_no_interpreter(&mut runtime);
    }
}

#[test]
fn jit_numeric_regions_preserve_ieee_values_comparisons_and_captured_locals() {
    let source = r#"
        function* compute(a, b) {
            let result = (a * 2 + b) / 2;
            let compare = a * 2 + b < 0;
            yield [Object.is(result, -0), Number.isNaN(result), result === Infinity, compare].join(',');
        }
        for (let i = 0; i < 40; i++) compute(i, 1).next();
        function* captured() {
            let value = 2;
            const read = () => value;
            value = (value + 1) * 2;
            yield read();
        }
        [
            compute(-0, -0).next().value,
            compute(NaN, 1).next().value,
            compute(Infinity, 1).next().value,
            compute(-3, 1).next().value,
            captured().next().value
        ].join(';');
    "#;
    let mut interpreter = JsRuntime::new();
    let expected = interpreter.eval(source).unwrap();
    assert_eq!(
        expected,
        JsValue::String("true,false,false,false;false,true,false,false;false,false,true,false;false,false,false,true;6".into())
    );
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    assert_eq!(runtime.eval(source).unwrap(), expected);
    assert!(runtime.jit_stats().native_region_opcodes > 0);
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_numeric_regions_preserve_all_numeric_comparison_results() {
    for operator in ["<", ">", "<=", ">=", "==", "===", "!="] {
        let source = format!(
            r#"
            function* compare(a, b) {{ yield a * 1 {operator} b; }}
            for (let i = 0; i < 40; i++) compare(i, 0).next();
            [
                compare(NaN, 0).next().value,
                compare(0, NaN).next().value,
                compare(-0, 0).next().value,
                compare(Infinity, Infinity).next().value,
                compare(-3, 2).next().value,
                compare(3, 2).next().value
            ].join(',');
            "#
        );
        let reference = JsRuntime::new().eval(&source).unwrap();
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(true);
        assert_eq!(runtime.eval(&source).unwrap(), reference, "{operator}");
        assert!(runtime.jit_stats().native_region_opcodes > 0, "{operator}");
        assert_no_interpreter(&mut runtime);
    }
}

#[test]
fn jit_numeric_region_templates_read_live_operands_after_incremental_eval() {
    let mut interpreter = JsRuntime::new();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    for constant in [2, 17, 99] {
        let source = format!(
            "function* compute(a) {{ let b = 0; \
             for (let i = 0; i < 80; i++) b = b + a * {constant} - i / 2; yield b; }} \
             [compute(2).next().value, compute('2').next().value].join(',');"
        );
        assert_eq!(runtime.eval(&source).unwrap(), interpreter.eval(&source).unwrap());
    }
    assert!(runtime.jit_stats().native_region_opcodes > 0);
    assert_no_interpreter(&mut runtime);
}

#[test]
fn jit_numeric_region_counters_include_eval_realms_and_completed_agents() {
    let script = "function* numeric() { let n = 0; \
                  for (let i = 0; i < 80; i++) n = n + i * 2 - i / 2; yield n; } \
                  numeric().next().value;";
    for source in [format!("eval({script:?});"), format!("__createRealm__().global.eval({script:?});")] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(true);
        assert_eq!(runtime.eval(&source).unwrap(), JsValue::Number(4740.0));
        assert!(runtime.jit_stats().native_region_opcodes > 0);
        assert!(runtime.jit_stats().native_loop_opcodes > 0);
        assert_no_interpreter(&mut runtime);
    }
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    let worker = format!("{script} __agent_report('done');");
    runtime.eval(format!("__agent_start({worker:?});")).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while runtime.jit_stats().native_loop_opcodes == 0 {
        assert!(std::time::Instant::now() < deadline, "agent did not finish numeric execution");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_no_interpreter(&mut runtime);
    assert_eq!(JsRuntime::new().jit_stats().native_region_opcodes, 0);
    assert_eq!(JsRuntime::new().jit_stats().native_loop_opcodes, 0);
}

#[test]
fn jit_numeric_regions_respect_slice_budgets_and_gc_boundaries() {
    use javascript::JsRunStatus;
    let source = r#"
        function compute() {
            Symbol();
            let value = 1;
            for (let i = 0; i < 80; i++) value = (value + i) * 2 - i;
            return value;
        }
        compute();
    "#;
    let expected = JsRuntime::new().eval(source).unwrap();
    for budget in [1, 3, 4, 7, 8, 9, 16, 33] {
        let mut runtime = JsRuntime::new();
        runtime.set_jit_required(true);
        runtime.start_eval(source).unwrap();
        loop {
            let before = runtime.jit_stats().baseline_opcodes;
            let status = runtime.run_slice(budget).unwrap();
            let executed = runtime.jit_stats().baseline_opcodes - before;
            assert!(executed > 0 && executed <= budget as u64, "budget={budget}, executed={executed}");
            runtime.collect_garbage();
            if let JsRunStatus::Complete(value) = status {
                assert_eq!(value, expected, "budget={budget}");
                break;
            }
            assert_eq!(executed, budget as u64);
        }
        assert_eq!(runtime.jit_stats().optimized_calls, 0);
        if budget == 1 {
            assert_eq!(runtime.jit_stats().native_region_opcodes, 0);
        } else if budget == 33 {
            assert!(runtime.jit_stats().native_region_opcodes > 0);
        }
        assert_no_interpreter(&mut runtime);
    }
}

#[test]
fn jit_baseline_batched_counters_include_reentrant_calls_and_caught_throws() {
    let source = r#"
        let total = 0;
        const sentinel = {};
        const output = [1, 2, 3].map(value => {
            total += value;
            return { value };
        });
        try { throw sentinel; } catch (error) {
            if (error !== sentinel) throw new Error("identity lost");
        }
        total + output[2].value;
    "#;
    let mut interpreter = JsRuntime::new();
    assert_eq!(interpreter.eval(source).unwrap(), JsValue::Number(9.0));
    let interpreted = interpreter.jit_stats();
    let mut runtime = JsRuntime::new();
    runtime.set_jit_required(true);
    assert_eq!(runtime.eval(source).unwrap(), JsValue::Number(9.0));
    let compiled = runtime.jit_stats();
    assert_eq!(interpreted.optimized_calls, 0);
    assert_eq!(compiled.optimized_calls, 0);
    assert_eq!(compiled.baseline_opcodes, interpreted.interpreter_opcodes);
    assert!(interpreter.eval("throw new Error('uncaught');").is_err());
    assert!(runtime.eval("throw new Error('uncaught');").is_err());
    assert_eq!(runtime.jit_stats().baseline_opcodes, executed_bytecodes(&mut interpreter));
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
fn jit_baseline_shared_templates_read_current_operands_and_branch_targets() {
    let mut source = String::from("let total = 0; const values = {};");
    for index in 0..80 {
        source.push_str(&format!(
            "values.key{index} = {index}; \
             if (values.key{index} === {index}) {{ total += values.key{index}; }} \
             else {{ throw 'wrong operand {index}'; }}"
        ));
    }
    source.push_str("total;");
    assert_baseline_matches(&source, JsValue::Number(3160.0));
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
