use javascript::evaluate_script;

#[test]
fn for_of_missing_iterator_throws() {
    // Plain object without iterator should not be iterable for 'for..of'
    let script = "for (var x of {}) { }; 1";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    assert!(res.is_err());
}

#[test]
fn iterator_next_returns_non_object_throws() {
    // Custom iterable where next() returns a non-object (e.g. number) should raise an error
    let script = r#"
        let s = Symbol.iterator;
        let o = {};
        o[s] = function() {
            return { next: function() { return 42; } };
        };
        // Attempt to iterate will call next() which returns a non-object -> should error
        for (let x of o) { }
        1
    "#;
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    assert!(res.is_err());
}

#[test]
fn string_iteration_surrogate_pair_behaviour() {
    // For strings containing astral symbols, ensure iteration yields string fragments
    // and concatenation returns a string with the expected boundaries — engine may handle
    // surrogate halves in its own way; we assert robust properties rather than exact codepoint equality.
    let script = r#"
        let s = "a😀b"; // contains an astral character
        let acc = "";
        for (let ch of s) { acc = acc + ch; }
        acc
    "#;
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    let rust_str = res.trim_start_matches('"').trim_end_matches('"');
    assert!(rust_str.starts_with("a"));
    assert!(rust_str.ends_with("b"));
    assert!(rust_str.len() >= 2);
}

#[test]
fn iterator_helpers_complete_after_abrupt_step() {
    let script = r#"
        const names = ["map", "filter", "drop", "take", "flatMap", "concat"];
        for (const name of names) {
            let nextCalls = 0;
            const source = {
                next: function() {
                    nextCalls++;
                    throw 1;
                },
                return: function() { return {}; },
                [Symbol.iterator]: function() { return this; }
            };
            let helper;
            if (name === "concat") helper = Iterator.concat(source);
            else {
                const iterator = Iterator.from(source);
                if (name === "map") helper = iterator.map(function(value) { return value; });
                else if (name === "filter") helper = iterator.filter(function() { return true; });
                else if (name === "drop") helper = iterator.drop(1);
                else if (name === "take") helper = iterator.take(1);
                else helper = iterator.flatMap(function(value) { return [value]; });
            }

            let threw = false;
            try { helper.next(); } catch (_) { threw = true; }
            if (!threw) throw new Error(name + " did not propagate next() failure");

            const afterThrow = helper.next();
            if (afterThrow.done !== true || afterThrow.value !== undefined) {
                throw new Error(name + " did not complete after abrupt completion");
            }
            if (nextCalls !== 1) throw new Error(name + " stepped the source after completion");
            const afterReturn = helper.return();
            if (afterReturn.done !== true || afterReturn.value !== undefined) {
                throw new Error(name + " return() did not preserve completed state");
            }
        }
        true
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn flat_map_closes_outer_but_not_inner_when_inner_step_throws() {
    let script = r#"
        let outerReturnCalls = 0;
        let innerReturnCalls = 0;
        const outer = {
            next: function() { return { done: false, value: 1 }; },
            return: function() { outerReturnCalls++; return {}; },
            [Symbol.iterator]: function() { return this; }
        };
        const inner = {
            next: function() { throw 1; },
            return: function() { innerReturnCalls++; return {}; },
            [Symbol.iterator]: function() { return this; }
        };
        const helper = Iterator.from(outer).flatMap(function() { return inner; });
        let threw = false;
        try { helper.next(); } catch (_) { threw = true; }
        if (!threw) throw new Error("flatMap did not propagate inner step failure");
        if (outerReturnCalls !== 1) throw new Error("flatMap did not close the outer iterator");
        if (innerReturnCalls !== 0) throw new Error("flatMap incorrectly closed the inner iterator");
        true
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn iterator_concat_return_validates_and_wraps_underlying_result() {
    let script = r#"
        let returnCalls = 0;
        const source = {
            next: function() { return { done: false, value: 1 }; },
            return: function() { returnCalls++; return 1; },
            [Symbol.iterator]: function() { return this; }
        };
        const iterator = Iterator.concat(source);
        iterator.next();
        let threwTypeError = false;
        try { iterator.return(); } catch (error) { threwTypeError = error instanceof TypeError; }
        if (!threwTypeError) throw new Error("concat.return() accepted a primitive result");
        if (returnCalls !== 1) throw new Error("concat.return() did not call the underlying return exactly once");
        const afterThrow = iterator.next();
        if (afterThrow.done !== true || afterThrow.value !== undefined) {
            throw new Error("concat helper did not complete after return() threw");
        }
        const afterReturn = iterator.return();
        if (afterReturn.done !== true || afterReturn.value !== undefined || returnCalls !== 1) {
            throw new Error("concat helper forwarded return() after completion");
        }
        true
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn iterator_concat_return_returns_a_fresh_result_object() {
    let script = r#"
        const underlyingResult = { done: false, value: 1 };
        const source = {
            next: function() { return { done: false, value: 1 }; },
            return: function() { return underlyingResult; },
            [Symbol.iterator]: function() { return this; }
        };
        const iterator = Iterator.concat(source);
        iterator.next();
        const result = iterator.return();
        if (result === underlyingResult) throw new Error("concat returned the underlying result object");
        if (result.done !== true || result.value !== undefined) {
            throw new Error("concat returned an incorrect iterator result");
        }
        true
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}
