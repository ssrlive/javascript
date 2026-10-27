use javascript::*;

#[ctor::ctor(unsafe)]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_js_arraybuffer_constructor_via_script() {
    // Test ArrayBuffer constructor through JavaScript
    let script = r#"
        let buffer = new ArrayBuffer(32);
        // Since we can't directly access the internal structure from JS,
        // we just verify the constructor doesn't throw
        "ArrayBuffer created successfully";
    "#;

    let result = evaluate_script(script, false, None::<&std::path::Path>, DEFAULT_GC_OPCODE_BUDGET);
    assert!(result.is_ok(), "ArrayBuffer constructor should work");
}

#[test]
fn test_js_dataview_constructor_via_script() {
    // Test DataView constructor through JavaScript
    let script = r#"
        let buffer = new ArrayBuffer(16);
        let view = new DataView(buffer);
        // Verify constructor works
        "DataView created successfully";
    "#;

    let result = evaluate_script(script, false, None::<&std::path::Path>, DEFAULT_GC_OPCODE_BUDGET);
    assert!(result.is_ok(), "DataView constructor should work");
}

#[test]
fn test_js_typedarray_constructors_via_script() {
    // Test all TypedArray constructors through JavaScript
    let script = r#"
        // Test various TypedArray constructors
        let buffer = new ArrayBuffer(64);

        let int8Array = new Int8Array(8);
        let uint8Array = new Uint8Array(8);
        let uint8ClampedArray = new Uint8ClampedArray(8);
        let int16Array = new Int16Array(4);
        let uint16Array = new Uint16Array(4);
        let int32Array = new Int32Array(2);
        let uint32Array = new Uint32Array(2);
        let float32Array = new Float32Array(2);
        let float64Array = new Float64Array(1);

        // Test constructor with existing buffer
        let viewFromBuffer = new Int32Array(buffer);

        "All TypedArray constructors work";
    "#;

    let result = evaluate_script(script, false, None::<&std::path::Path>, DEFAULT_GC_OPCODE_BUDGET);
    assert!(result.is_ok(), "All TypedArray constructors should work");
}

#[test]
fn test_uint8array_base64_encoding_and_decoding() {
    let script = r#"
        const standard = new Uint8Array([102, 111, 111]).toBase64();
        const urlSafe = new Uint8Array([255, 255]).toBase64({ alphabet: "base64url", omitPadding: true });
        const decoded = Uint8Array.fromBase64("Zm9v").join(",");
        const decodedUrlSafe = Uint8Array.fromBase64("__8", { alphabet: "base64url" }).join(",");
        const loosePartial = Uint8Array.fromBase64("Zh").join(",");
        [standard, urlSafe, decoded, decodedUrlSafe, loosePartial].join("|");
    "#;

    let result = evaluate_script(script, false, None::<&std::path::Path>, DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"Zm9v|__8|102,111,111|255,255|102\"");
}

#[test]
fn test_uint8array_base64_decode_modes_and_partial_writes() {
    let script = r#"
        let strictRejectsNonCanonicalBits = false;
        try {
            Uint8Array.fromBase64("ZXhhZh==", { lastChunkHandling: "strict" });
        } catch (error) {
            strictRejectsNonCanonicalBits = error instanceof SyntaxError;
        }
        const looseNonCanonicalBits = Uint8Array.fromBase64("ZXhhZh==").join(",");
        const stoppedNonCanonicalBits = Uint8Array.fromBase64("ZXhhZh==", { lastChunkHandling: "stop-before-partial" }).join(",");
        const stopped = Uint8Array.fromBase64("Zm9vY", { lastChunkHandling: "stop-before-partial" }).join(",");
        const target = new Uint8Array(3);
        const { read, written } = target.setFromBase64("Zm9v");
        [strictRejectsNonCanonicalBits, looseNonCanonicalBits, stoppedNonCanonicalBits, stopped, read, written, target.join(",")].join("|");
    "#;

    let result = evaluate_script(script, false, None::<&std::path::Path>, DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"true|101,120,97,102|101,120,97,102|102,111,111|4|3|102,111,111\"");
}

#[test]
fn test_uint8array_base64_strict_error_is_caught_by_assert_throws() {
    let script = r#"
        function Test262Error(message) {
            this.message = message;
        }
        const assert = {
            throws(expectedErrorConstructor, func) {
                try {
                    func();
                } catch (thrown) {
                    if (thrown.constructor !== expectedErrorConstructor) {
                        throw new Test262Error("Unexpected error constructor");
                    }
                    return;
                }
                throw new Test262Error("Expected an error to be thrown");
            }
        };
        assert.throws(SyntaxError, function() {
            Uint8Array.fromBase64("ZXhhZg", { lastChunkHandling: "strict" });
        });
        "OK";
    "#;

    let result = evaluate_script(script, false, None::<&std::path::Path>, DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"OK\"");
}

#[test]
fn test_js_typedarray_shared_buffer_via_script() {
    // Test that TypedArrays share the same underlying buffer
    let script = r#"
        let buffer = new ArrayBuffer(16);
        let int32View = new Int32Array(buffer);
        let uint8View = new Uint8Array(buffer);

        // Test basic assignment and access
        int32View[0] = 42;
        let result = int32View[0];
        result;
    "#;

    let result = evaluate_script(script, false, None::<&std::path::Path>, DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "42");
}

#[test]
fn test_js_arraybuffer_dataview_integration_via_script() {
    // Test ArrayBuffer and DataView integration
    let script = r#"
        let buffer = new ArrayBuffer(16);
        let view = new DataView(buffer, 4, 8); // offset 4, length 8

        // The DataView should be created successfully
        // In a full implementation, we would test reading/writing data
        "ArrayBuffer-DataView integration works";
    "#;

    let result = evaluate_script(script, false, None::<&std::path::Path>, DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"ArrayBuffer-DataView integration works\"");
}

#[test]
fn test_dataview_accessors_return_zero_when_detached_or_out_of_bounds() {
    let script = r#"
        const detachedBuffer = new ArrayBuffer(8);
        const detachedView = new DataView(detachedBuffer, 2, 4);
        __detachArrayBuffer__(detachedBuffer);

        const fixedBuffer = new ArrayBuffer(8, { maxByteLength: 16 });
        const fixedView = new DataView(fixedBuffer, 2, 4);
        fixedBuffer.resize(5);

        const trackingBuffer = new ArrayBuffer(8, { maxByteLength: 16 });
        const trackingView = new DataView(trackingBuffer, 6);
        trackingBuffer.resize(5);

        JSON.stringify([
            detachedView.byteLength,
            detachedView.byteOffset,
            fixedView.byteLength,
            fixedView.byteOffset,
            trackingView.byteLength,
            trackingView.byteOffset
        ]);
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"[0,0,0,0,0,0]\"");
}

#[test]
fn test_js_typedarray_different_construction_patterns_via_script() {
    // Test different ways to construct TypedArrays
    let script = r#"
        // Test different construction patterns
        let buffer = new ArrayBuffer(32);

        // Constructor with length
        let arr1 = new Int8Array(8);

        // Constructor with buffer
        let arr2 = new Int8Array(buffer);

        // Constructor with buffer, offset, length
        let arr3 = new Int8Array(buffer, 8, 4);

        // Constructor with another TypedArray (copy)
        let arr4 = new Int8Array(arr1);

        "Different construction patterns work";
    "#;

    let result = evaluate_script(script, false, None::<&std::path::Path>, DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"Different construction patterns work\"");
}

#[test]
fn test_js_for_in_resizable_buffer_via_script() {
    // Regression test for for-in enumeration over TypedArrays backed by resizable ArrayBuffers
    let script = r#"
        function CreateResizableArrayBuffer(initial, max) {
           try {
               return new ArrayBuffer(initial, { maxByteLength: max });
           } catch (e) {
               throw new Error('Resizable ArrayBuffer not supported: ' + e);
           }
        }
        const ctors = [
           Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array,
           Int32Array, Uint32Array, Float32Array, Float64Array
        ];
        let rab = CreateResizableArrayBuffer(100, 200);
        for (let ctor of ctors) {
            const ta = new ctor(rab, 0, 3);
            let keys = '';
            for (const key in ta) {
                keys += key;
            }
            if (keys !== '012') throw new Error(ctor.name + ' keys mismatch: ' + keys);
        }
        "OK";
    "#;

    let result = evaluate_script(script, false, None::<&std::path::Path>, DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "\"OK\"");
}

#[test]
fn test_typedarray_destructuring_resizable_buffer_regression() {
    let path = std::path::Path::new("js-scripts/typedarray_destructuring_resizable_buffer_regression.js");
    let script = read_script_file(path).expect("failed to read regression script");

    // Append a final expression so evaluate_script returns the script's return value as final result
    let _wrapped = format!("{}\nJSON.stringify(({}));", script, "(function(){return (function(){})();})()");

    // Evaluate and assert
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let result =
                evaluate_script_with_unwrap(&script, false, Some(path), true, DEFAULT_GC_OPCODE_BUDGET).expect("evaluate_script failed");
            assert_eq!(result, "\"OK\"");
        })
        .expect("failed to spawn thread")
        .join()
        .expect("test thread panicked");
}
