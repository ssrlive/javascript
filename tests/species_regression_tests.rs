use javascript::*;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod species_regression_tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    static TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn test_species_accessors_exist_on_builtin_constructors() {
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let script = r#"
            let ctors = [Array, ArrayBuffer, Promise, RegExp, TypedArray];
            ctors.map(function(ctor) {
                let desc = Object.getOwnPropertyDescriptor(ctor, Symbol.species);
                return !!desc
                    && typeof desc.get === "function"
                    && desc.set === undefined
                    && desc.enumerable === false
                    && desc.configurable === true
                    && desc.get.call(ctor) === ctor;
            });
        "#;

        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[true,true,true,true,true]");
    }

    #[test]
    fn test_array_species_cross_realm_shortcuts_and_foreign_non_array_constructors() {
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let script = r#"
            let otherArray = __createRealm__().global.Array;
            let foreignArraySpeciesCalls = 0;
            let foreignArraySpeciesDesc = {
                get: function() {
                    foreignArraySpeciesCalls += 1;
                }
            };
            Object.defineProperty(Array, Symbol.species, foreignArraySpeciesDesc);
            Object.defineProperty(otherArray, Symbol.species, foreignArraySpeciesDesc);
            let foreignArray = [];
            foreignArray.constructor = otherArray;
            let mapped = foreignArray.map(function() {});

            let otherObject = __createRealm__().global.Object;
            let currentArraySpeciesCalls = 0;
            let customCtor = function() {};
            otherObject[Symbol.species] = customCtor;
            Object.defineProperty(Array, Symbol.species, {
                get: function() {
                    currentArraySpeciesCalls += 1;
                }
            });
            let nonArray = [];
            nonArray.constructor = otherObject;
            let filtered = nonArray.filter(function() { return true; });

            [
                Object.getPrototypeOf(mapped) === Array.prototype,
                foreignArraySpeciesCalls === 0,
                Object.getPrototypeOf(filtered) === customCtor.prototype,
                currentArraySpeciesCalls === 0
            ];
        "#;

        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[true,true,true,true]");
    }

    #[test]
    fn test_arraybuffer_and_typedarray_species_regressions() {
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let script = r#"
            let produced;
            let buffer = new ArrayBuffer(8);
            buffer.constructor = {
                [Symbol.species]: function(length) {
                    produced = new ArrayBuffer(length + 2);
                    return produced;
                }
            };
            let sliced = buffer.slice();

            let badArrayBufferCtor = false;
            try {
                let x = new ArrayBuffer(8);
                x.constructor = null;
                x.slice();
            } catch (e) {
                badArrayBufferCtor = e instanceof TypeError;
            }

            let badArrayBufferSpecies = false;
            try {
                let x = new ArrayBuffer(8);
                x.constructor = { [Symbol.species]: {} };
                x.slice();
            } catch (e) {
                badArrayBufferSpecies = e instanceof TypeError;
            }

            let badTypedArrayCompat = false;
            try {
                let ta = new Uint8Array(2);
                ta.constructor = {};
                ta.constructor[Symbol.species] = Array;
                ta.map(function() { return 0; });
            } catch (e) {
                badTypedArrayCompat = e instanceof TypeError;
            }

            let badTypedArrayLength = false;
            try {
                let ta = new Uint8Array(2);
                ta.constructor = {};
                ta.constructor[Symbol.species] = function() {
                    return new Uint8Array();
                };
                ta.filter(function() { return true; });
            } catch (e) {
                badTypedArrayLength = e instanceof TypeError;
            }

            [
                sliced === produced,
                sliced.byteLength === 10,
                badArrayBufferCtor,
                badArrayBufferSpecies,
                badTypedArrayCompat,
                badTypedArrayLength
            ];
        "#;

        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[true,true,true,true,true,true]");
    }

    #[test]
    fn test_async_generator_function_cross_realm_instance_chain() {
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let script = r#"
            let realmA = __createRealm__().global;
            realmA.calls = 0;
            let aAsyncGeneratorFunction = realmA.eval("(async function* () {})").constructor;
            let aAsyncGeneratorPrototype = Object.getPrototypeOf(
                realmA.eval("(async function* () {})").prototype
            );

            let realmB = __createRealm__().global;
            let bAsyncGeneratorFunction = realmB.eval("(async function* () {})").constructor;
            let newTarget = new realmB.Function();
            newTarget.prototype = null;

            let fn = Reflect.construct(aAsyncGeneratorFunction, ["calls += 1;"], newTarget);
            let gen = fn();
            gen.next();

            [
                Object.getPrototypeOf(fn) === bAsyncGeneratorFunction.prototype,
                Object.getPrototypeOf(fn.prototype) === aAsyncGeneratorPrototype,
                gen instanceof realmA.Object,
                realmA.calls === 1
            ];
        "#;

        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[true,true,true,true]");
    }

    #[test]
    fn test_array_splice_non_array_invalid_length_throws_before_mutation() {
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let script = r#"
            let callCount = 0;
            let obj = Object.defineProperty({}, "length", {
                get: function() {
                    return Math.pow(2, 32);
                },
                set: function() {
                    callCount += 1;
                }
            });

            let threw = false;
            try {
                Array.prototype.splice.call(obj, 0);
            } catch (e) {
                threw = e instanceof RangeError;
            }

            [threw, callCount === 0];
        "#;

        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[true,true]");
    }
}
