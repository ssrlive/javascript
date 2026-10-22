use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor(unsafe)]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_map_constructor() {
    let script = "Object.prototype.toString.call(new Map())";
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"[object Map]\"");
}

#[test]
fn test_map_set_and_get() {
    let script = r#"
        let map = new Map();
        map.set("key1", "value1");
        map.set("key2", "value2");
        map.get("key1")
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"value1\"");
}

#[test]
fn test_map_has() {
    let script = r#"
        let map = new Map();
        map.set("key", "value");
        map.has("key")
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_size() {
    let script = r#"
        let map = new Map();
        map.set("a", 1);
        map.set("b", 2);
        map.size
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "2");
}

#[test]
fn test_map_delete() {
    let script = r#"
        let map = new Map();
        map.set("key", "value");
        let deleted = map.delete("key");
        let has = map.has("key");
        console.log(JSON.stringify([deleted, has]));
        [deleted, has]
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    // This should return an array [true, false]
    assert_eq!(result, "[true,false]");
}

#[test]
fn test_map_clear() {
    let script = r#"
        let map = new Map();
        map.set("a", 1);
        map.set("b", 2);
        map.clear();
        map.size
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "0");
}

#[test]
fn test_set_constructor() {
    let script = "Object.prototype.toString.call(new Set())";
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"[object Set]\"");
}

#[test]
fn test_set_add_and_has() {
    let script = r#"
        let set = new Set();
        set.add("item1");
        set.add("item2");
        set.has("item1")
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_size() {
    let script = r#"
        let set = new Set();
        set.add(1);
        set.add(2);
        set.add(2); // duplicate
        set.size
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "2");
}

#[test]
fn test_set_delete() {
    let script = r#"
        let set = new Set();
        set.add("item");
        let deleted = set.delete("item");
        let has = set.has("item");
        JSON.stringify([deleted, has])
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    // This should return an array [true, false]
    assert_eq!(result, "\"[true,false]\"");
}

#[test]
fn test_set_clear() {
    let script = r#"
        let set = new Set();
        set.add(1);
        set.add(2);
        set.clear();
        set.size
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "0");
}

#[test]
fn test_map_keys_values_entries() {
    let script = r#"
        let map = new Map();
        map.set("a", 1);
        map.set("b", 2);
        let k = []; for (let x of map.keys()) k.push(x);
        let v = []; for (let x of map.values()) v.push(x);
        let e = []; for (let x of map.entries()) e.push(x);
        JSON.stringify([k.length, v.length, e.length])
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    // Should return [2, 2, 2]
    assert_eq!(result, "\"[2,2,2]\"");
}

#[test]
fn test_set_values() {
    let script = r#"
        let set = new Set();
        set.add(1);
        set.add(2);
        let vals = [];
        for (let x of set.values()) vals.push(x);
        JSON.stringify(vals.length)
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"2\"");
}

#[test]
fn test_set_add_rejects_invalid_receivers() {
    let script = r#"
        const values = [undefined, null, true, 1, "x", {}];
        values.every(function(value) {
            try {
                Set.prototype.add.call(value, 1);
                return false;
            } catch (err) {
                return err instanceof TypeError;
            }
        })
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_add_has_standard_name() {
    let script = "Set.prototype.add.name";
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"add\"");
}

#[test]
fn test_set_methods_probe_surface() {
    let script = r#"
        const a = new Set([1, 2, 3]);
        const b = new Set([2, 3, 4]);
        const u = a.union(b);
        const i = a.intersection(b);
        const d = a.difference(b);
        const sd = a.symmetricDifference(b);
        typeof Set.prototype.union === "function" &&
        typeof Set.prototype.intersection === "function" &&
        typeof Set.prototype.difference === "function" &&
        typeof Set.prototype.symmetricDifference === "function" &&
        typeof Set.prototype.isSubsetOf === "function" &&
        typeof Set.prototype.isSupersetOf === "function" &&
        typeof Set.prototype.isDisjointFrom === "function" &&
        u instanceof Set && u.size === 4 &&
        i.size === 2 &&
        d.size === 1 &&
        sd.size === 2 &&
        a.isSubsetOf(new Set([1, 2, 3, 4])) &&
        a.isSupersetOf(new Set([1, 2])) &&
        a.isDisjointFrom(new Set([4, 5]))
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_union_allows_set_like_object() {
    let script = r#"
        const s1 = new Set([1, 2]);
        const s2 = {
          size: 2,
          has() { throw new Error("union should not call has"); },
          keys: function* () {
            yield 2;
            yield 3;
          }
        };
        JSON.stringify([...s1.union(s2)])
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"[1,2,3]\"");
}

#[test]
fn test_set_other_builtin_methods_reject_invalid_receivers() {
    let script = r#"
        const cases = [
          () => Set.prototype.clear.call({}),
          () => Set.prototype.delete.call({}, 1),
          () => Set.prototype.has.call({}, 1),
          () => Set.prototype.values.call({}),
          () => Set.prototype.entries.call({}),
          () => Set.prototype.clear.call(null),
          () => Set.prototype.delete.call(undefined, 1),
          () => Set.prototype.has.call(true, 1),
          () => Set.prototype.values.call("x"),
          () => Set.prototype.entries.call(1),
        ];
        cases.every(fn => {
          try {
            fn();
            return false;
          } catch (err) {
            return err instanceof TypeError;
          }
        })
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_builtin_method_metadata() {
    let script = r#"
        Set.prototype.has.name === "has" &&
        Set.prototype.delete.name === "delete" &&
        Set.prototype.values.name === "values" &&
        Set.prototype.entries.name === "entries" &&
        Set.prototype.clear.name === "clear" &&
        Set.prototype.values.length === 0 &&
        Set.prototype.entries.length === 0 &&
        Set.prototype.clear.length === 0
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_has_uses_same_value_zero_for_nan() {
    let script = r#"
        const set = new Set();
        set.add(NaN);
        set.has(NaN)
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_symbol_iterator_is_values_alias() {
    let script = r#"
        Set.prototype[Symbol.iterator] === Set.prototype.values &&
        typeof Set.prototype[Symbol.iterator] === "function"
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_prototype_methods_reject_weakset_receivers() {
    let script = r#"
        const weak = new WeakSet();
        const cases = [
          () => Set.prototype.add.call(weak, {}),
          () => Set.prototype.has.call(weak, {}),
          () => Set.prototype.delete.call(weak, {}),
          () => Set.prototype.clear.call(weak),
          () => Set.prototype.values.call(weak),
          () => Set.prototype.entries.call(weak),
        ];
        cases.every(fn => {
          try {
            fn();
            return false;
          } catch (err) {
            return err instanceof TypeError;
          }
        })
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_size_descriptor_uses_getter() {
    let script = r#"
        const descriptor = Object.getOwnPropertyDescriptor(Set.prototype, "size");
        typeof descriptor.get === "function" &&
        descriptor.get.name === "get size" &&
        descriptor.get.length === 0 &&
        descriptor.set === undefined &&
        descriptor.enumerable === false &&
        descriptor.configurable === true
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_for_each_requires_callable_callback() {
    let script = r#"
        const set = new Set([1]);
        [undefined, null, true, 1, "x", Symbol("x")].every(value => {
          try {
            set.forEach(value);
            return false;
          } catch (err) {
            return err instanceof TypeError;
          }
        })
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_for_each_visits_values_added_during_iteration() {
    let script = r#"
        const set = new Set([1]);
        const seen = [];
        set.forEach(function(value) {
          seen.push(value);
          if (value === 1) set.add(2);
          if (value === 2) set.add(3);
        });
        JSON.stringify(seen)
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"[1,2,3]\"");
}

#[test]
fn test_set_for_each_revisits_delete_readd() {
    let script = r#"
        const set = new Set([1, 2, 3]);
        const seen = [];
        set.forEach(function(value) {
          seen.push(value);
          if (value === 2) set.delete(1);
          if (value === 3) set.add(1);
        });
        JSON.stringify(seen)
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"[1,2,3,1]\"");
}

#[test]
fn test_set_for_each_propagates_callback_errors() {
    let script = r#"
        const set = new Set([1]);
        let counter = 0;
        try {
          set.forEach(function() {
            counter++;
            throw new Error("boom");
          });
          false
        } catch (err) {
          counter === 1 && err instanceof Error
        }
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_constructor_metadata_matches_spec() {
    let script = r#"
        const globalDesc = Object.getOwnPropertyDescriptor(this, "Set");
        const speciesDesc = Object.getOwnPropertyDescriptor(Set, Symbol.species);
        Set.name === "Set" &&
        Set.length === 0 &&
        globalDesc &&
        globalDesc.writable === true &&
        globalDesc.enumerable === false &&
        globalDesc.configurable === true &&
        Object.getPrototypeOf(Set) === Function.prototype &&
        speciesDesc &&
        typeof speciesDesc.get === "function" &&
        speciesDesc.get.name === "get [Symbol.species]" &&
        speciesDesc.get.length === 0 &&
        speciesDesc.set === undefined &&
        speciesDesc.enumerable === false &&
        speciesDesc.configurable === true &&
        Set[Symbol.species] === Set
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_constructor_metadata_matches_spec() {
    let script = r#"
        const globalDesc = Object.getOwnPropertyDescriptor(this, "Map");
        const speciesDesc = Object.getOwnPropertyDescriptor(Map, Symbol.species);
        Map.name === "Map" &&
        Map.length === 0 &&
        globalDesc &&
        globalDesc.writable === true &&
        globalDesc.enumerable === false &&
        globalDesc.configurable === true &&
        Object.getPrototypeOf(Map) === Function.prototype &&
        speciesDesc &&
        typeof speciesDesc.get === "function" &&
        speciesDesc.get.name === "get [Symbol.species]" &&
        speciesDesc.get.length === 0 &&
        speciesDesc.set === undefined &&
        speciesDesc.enumerable === false &&
        speciesDesc.configurable === true &&
        Map[Symbol.species] === Map
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_constructor_requires_new() {
    let script = r#"
        try {
          Map();
          false
        } catch (err) {
          err instanceof TypeError
        }
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_new_map_uses_map_prototype() {
    let script = r#"
        Object.getPrototypeOf(new Map()) === Map.prototype
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_constructor_uses_set_for_iterables() {
    let script = r#"
        const originalSet = Map.prototype.set;
        let counter = 0;
        Map.prototype.set = function(key, value) {
          counter++;
          return originalSet.call(this, key, value);
        };
        new Map([["a", 1], ["b", 2]]);
        counter === 2
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_constructor_closes_iterator_when_set_throws() {
    let script = r#"
        let closed = 0;
        const iterable = {
          [Symbol.iterator]() {
            return {
              next() { return { value: ["a", 1], done: false }; },
              return() { closed++; }
            };
          }
        };
        Map.prototype.set = function() { throw new Error("boom"); };
        try {
          new Map(iterable);
          false
        } catch (err) {
          closed === 1 && err instanceof Error
        }
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_symbol_iterator_aliases_entries() {
    let script = r#"
        Map.prototype[Symbol.iterator] === Map.prototype.entries
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_groupby_metadata_matches_spec() {
    let script = r#"
        Map.groupBy.name === "groupBy" &&
        Map.groupBy.length === 2 &&
        Object.prototype.propertyIsEnumerable.call(Map, "groupBy") === false
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_groupby_returns_map_with_arbitrary_keys() {
    let script = r#"
        const obj = { toString() { return "nope"; } };
        const map = Map.groupBy([1, "1", obj], (value) => value);
        map instanceof Map &&
        map.size === 3 &&
        map.get(1).length === 1 &&
        map.get("1").length === 1 &&
        map.get(obj).length === 1
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_groupby_normalizes_negative_zero_keys() {
    let script = r#"
        const map = Map.groupBy([-0, 0], (value) => value);
        map.size === 1 &&
        1 / Array.from(map.keys())[0] === Infinity &&
        map.get(0).length === 2
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_prototype_methods_have_expected_metadata() {
    let script = r#"
        const sizeDesc = Object.getOwnPropertyDescriptor(Map.prototype, "size");
        Map.prototype.get.name === "get" &&
        Map.prototype.get.length === 1 &&
        Map.prototype.set.name === "set" &&
        Map.prototype.set.length === 2 &&
        Map.prototype.clear.name === "clear" &&
        Map.prototype.clear.length === 0 &&
        typeof sizeDesc.get === "function" &&
        sizeDesc.get.name === "get size" &&
        sizeDesc.get.length === 0
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_methods_reject_weakmap_receiver_and_use_same_value_zero() {
    let script = r#"
        const map = new Map();
        map.set(NaN, 1);
        let weakMapRejected = false;
        try {
          Map.prototype.get.call(new WeakMap(), {});
        } catch (err) {
          weakMapRejected = err instanceof TypeError;
        }
        weakMapRejected && map.get(NaN) === 1 && map.has(NaN) === true
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_for_each_validates_callback_and_propagates_errors() {
    let script = r#"
        const map = new Map([[1, 2]]);
        let invalidRejected = false;
        try {
          map.forEach({});
        } catch (err) {
          invalidRejected = err instanceof TypeError;
        }

        let abruptRejected = false;
        try {
          map.forEach(function() { throw new Error("boom"); });
        } catch (err) {
          abruptRejected = err instanceof Error;
        }

        invalidRejected && abruptRejected
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_for_each_tracks_live_mutations() {
    let script = r#"
        const map = new Map();
        map.set("foo", 0);
        map.set("bar", 1);
        const seen = [];
        let count = 0;
        map.forEach(function(value, key) {
          if (count === 0) {
            map.delete("foo");
            map.set("foo", "baz");
            map.set("baz", 2);
            map.delete("baz");
          }
          seen.push(key + ":" + value);
          count++;
        });
        seen.join("|") === "foo:0|bar:1|foo:baz"
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_constructor_uses_add_for_iterables() {
    let script = r#"
        const originalAdd = Set.prototype.add;
        let counter = 0;
        Set.prototype.add = function(value) {
          counter++;
          return originalAdd.call(this, value);
        };
        new Set([1, 2]);
        counter
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "2");
}

#[test]
fn test_set_constructor_rejects_non_callable_add() {
    let script = r#"
        Set.prototype.add = null;
        try {
          new Set([1, 2]);
          false
        } catch (err) {
          err instanceof TypeError
        }
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_constructor_propagates_iterator_value_errors() {
    let script = r#"
        function MyError() {}
        const iterable = {
          [Symbol.iterator]() {
            return {
              next() {
                return {
                  get value() { throw new MyError(); },
                  done: false
                };
              }
            };
          }
        };
        try {
          new Set(iterable);
          false
        } catch (err) {
          err instanceof MyError
        }
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_constructor_closes_iterator_when_add_throws() {
    let script = r#"
        let closed = 0;
        const iterable = {
          [Symbol.iterator]() {
            return {
              next() { return { value: 1, done: false }; },
              return() { closed++; }
            };
          }
        };
        Set.prototype.add = function() { throw new Error("boom"); };
        try {
          new Set(iterable);
          false
        } catch (err) {
          closed === 1 && err instanceof Error
        }
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_constructor_requires_new() {
    let script = r#"
        try {
          Set();
          false
        } catch (err) {
          err instanceof TypeError
        }
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_new_set_uses_set_prototype() {
    let script = r#"
        Object.getPrototypeOf(new Set()) === Set.prototype
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}
