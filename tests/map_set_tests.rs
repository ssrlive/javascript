use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_map_constructor() {
    let result = evaluate_script("Object.prototype.toString.call(new Map())", false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"[object Map]\"");
}

#[test]
fn test_map_set_and_get() {
    let result = evaluate_script(
        r#"
        let map = new Map();
        map.set("key1", "value1");
        map.set("key2", "value2");
        map.get("key1")
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "\"value1\"");
}

#[test]
fn test_map_has() {
    let result = evaluate_script(
        r#"
        let map = new Map();
        map.set("key", "value");
        map.has("key")
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_size() {
    let result = evaluate_script(
        r#"
        let map = new Map();
        map.set("a", 1);
        map.set("b", 2);
        map.size
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "2");
}

#[test]
fn test_map_delete() {
    let result = evaluate_script(
        r#"
        let map = new Map();
        map.set("key", "value");
        let deleted = map.delete("key");
        let has = map.has("key");
        console.log(JSON.stringify([deleted, has]));
        [deleted, has]
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    // This should return an array [true, false]
    assert_eq!(result, "[true,false]");
}

#[test]
fn test_map_clear() {
    let result = evaluate_script(
        r#"
        let map = new Map();
        map.set("a", 1);
        map.set("b", 2);
        map.clear();
        map.size
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "0");
}

#[test]
fn test_set_constructor() {
    let result = evaluate_script("Object.prototype.toString.call(new Set())", false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"[object Set]\"");
}

#[test]
fn test_set_add_and_has() {
    let result = evaluate_script(
        r#"
        let set = new Set();
        set.add("item1");
        set.add("item2");
        set.has("item1")
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_size() {
    let result = evaluate_script(
        r#"
        let set = new Set();
        set.add(1);
        set.add(2);
        set.add(2); // duplicate
        set.size
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "2");
}

#[test]
fn test_set_delete() {
    let result = evaluate_script(
        r#"
        let set = new Set();
        set.add("item");
        let deleted = set.delete("item");
        let has = set.has("item");
        JSON.stringify([deleted, has])
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    // This should return an array [true, false]
    assert_eq!(result, "\"[true,false]\"");
}

#[test]
fn test_set_clear() {
    let result = evaluate_script(
        r#"
        let set = new Set();
        set.add(1);
        set.add(2);
        set.clear();
        set.size
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "0");
}

#[test]
fn test_map_keys_values_entries() {
    let result = evaluate_script(
        r#"
        let map = new Map();
        map.set("a", 1);
        map.set("b", 2);
        let k = []; for (let x of map.keys()) k.push(x);
        let v = []; for (let x of map.values()) v.push(x);
        let e = []; for (let x of map.entries()) e.push(x);
        JSON.stringify([k.length, v.length, e.length])
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    // Should return [2, 2, 2]
    assert_eq!(result, "\"[2,2,2]\"");
}

#[test]
fn test_set_values() {
    let result = evaluate_script(
        r#"
        let set = new Set();
        set.add(1);
        set.add(2);
        let vals = [];
        for (let x of set.values()) vals.push(x);
        JSON.stringify(vals.length)
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "\"2\"");
}

#[test]
fn test_set_add_rejects_invalid_receivers() {
    let result = evaluate_script(
        r#"
        const values = [undefined, null, true, 1, "x", {}];
        values.every(function(value) {
            try {
                Set.prototype.add.call(value, 1);
                return false;
            } catch (err) {
                return err instanceof TypeError;
            }
        })
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_add_has_standard_name() {
    let result = evaluate_script("Set.prototype.add.name", false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"add\"");
}

#[test]
fn test_set_other_builtin_methods_reject_invalid_receivers() {
    let result = evaluate_script(
        r#"
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
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_builtin_method_metadata() {
    let result = evaluate_script(
        r#"
        Set.prototype.has.name === "has" &&
        Set.prototype.delete.name === "delete" &&
        Set.prototype.values.name === "values" &&
        Set.prototype.entries.name === "entries" &&
        Set.prototype.clear.name === "clear" &&
        Set.prototype.values.length === 0 &&
        Set.prototype.entries.length === 0 &&
        Set.prototype.clear.length === 0
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_has_uses_same_value_zero_for_nan() {
    let result = evaluate_script(
        r#"
        const set = new Set();
        set.add(NaN);
        set.has(NaN)
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_symbol_iterator_is_values_alias() {
    let result = evaluate_script(
        r#"
        Set.prototype[Symbol.iterator] === Set.prototype.values &&
        typeof Set.prototype[Symbol.iterator] === "function"
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_prototype_methods_reject_weakset_receivers() {
    let result = evaluate_script(
        r#"
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
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_size_descriptor_uses_getter() {
    let result = evaluate_script(
        r#"
        const descriptor = Object.getOwnPropertyDescriptor(Set.prototype, "size");
        typeof descriptor.get === "function" &&
        descriptor.get.name === "get size" &&
        descriptor.get.length === 0 &&
        descriptor.set === undefined &&
        descriptor.enumerable === false &&
        descriptor.configurable === true
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_for_each_requires_callable_callback() {
    let result = evaluate_script(
        r#"
        const set = new Set([1]);
        [undefined, null, true, 1, "x", Symbol("x")].every(value => {
          try {
            set.forEach(value);
            return false;
          } catch (err) {
            return err instanceof TypeError;
          }
        })
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_for_each_visits_values_added_during_iteration() {
    let result = evaluate_script(
        r#"
        const set = new Set([1]);
        const seen = [];
        set.forEach(function(value) {
          seen.push(value);
          if (value === 1) set.add(2);
          if (value === 2) set.add(3);
        });
        JSON.stringify(seen)
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "\"[1,2,3]\"");
}

#[test]
fn test_set_for_each_revisits_delete_readd() {
    let result = evaluate_script(
        r#"
        const set = new Set([1, 2, 3]);
        const seen = [];
        set.forEach(function(value) {
          seen.push(value);
          if (value === 2) set.delete(1);
          if (value === 3) set.add(1);
        });
        JSON.stringify(seen)
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "\"[1,2,3,1]\"");
}

#[test]
fn test_set_for_each_propagates_callback_errors() {
    let result = evaluate_script(
        r#"
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
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_constructor_metadata_matches_spec() {
    let result = evaluate_script(
        r#"
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
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_constructor_metadata_matches_spec() {
    let result = evaluate_script(
        r#"
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
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_constructor_requires_new() {
    let result = evaluate_script(
        r#"
        try {
          Map();
          false
        } catch (err) {
          err instanceof TypeError
        }
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_new_map_uses_map_prototype() {
    let result = evaluate_script(
        r#"
        Object.getPrototypeOf(new Map()) === Map.prototype
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_constructor_uses_set_for_iterables() {
    let result = evaluate_script(
        r#"
        const originalSet = Map.prototype.set;
        let counter = 0;
        Map.prototype.set = function(key, value) {
          counter++;
          return originalSet.call(this, key, value);
        };
        new Map([["a", 1], ["b", 2]]);
        counter === 2
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_constructor_closes_iterator_when_set_throws() {
    let result = evaluate_script(
        r#"
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
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_map_symbol_iterator_aliases_entries() {
    let result = evaluate_script(
        r#"
        Map.prototype[Symbol.iterator] === Map.prototype.entries
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_constructor_uses_add_for_iterables() {
    let result = evaluate_script(
        r#"
        const originalAdd = Set.prototype.add;
        let counter = 0;
        Set.prototype.add = function(value) {
          counter++;
          return originalAdd.call(this, value);
        };
        new Set([1, 2]);
        counter
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "2");
}

#[test]
fn test_set_constructor_rejects_non_callable_add() {
    let result = evaluate_script(
        r#"
        Set.prototype.add = null;
        try {
          new Set([1, 2]);
          false
        } catch (err) {
          err instanceof TypeError
        }
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_constructor_propagates_iterator_value_errors() {
    let result = evaluate_script(
        r#"
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
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_constructor_closes_iterator_when_add_throws() {
    let result = evaluate_script(
        r#"
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
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_set_constructor_requires_new() {
    let result = evaluate_script(
        r#"
        try {
          Set();
          false
        } catch (err) {
          err instanceof TypeError
        }
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_new_set_uses_set_prototype() {
    let result = evaluate_script(
        r#"
        Object.getPrototypeOf(new Set()) === Set.prototype
    "#,
        false,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}
