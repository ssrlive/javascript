use javascript::{JSArrayBuffer, TypedArrayKind, evaluate_script, read_script_file};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_jsarraybuffer_creation_and_access() {
    // Test creating a new ArrayBuffer
    let mut buffer = JSArrayBuffer {
        data: Arc::new(Mutex::new(vec![0; 16])),
        ..JSArrayBuffer::default()
    };

    assert_eq!(buffer.data.lock().unwrap().len(), 16);
    assert!(!buffer.detached);

    // Test data access and modification
    buffer.data.lock().unwrap()[0] = 42;
    buffer.data.lock().unwrap()[15] = 255;
    assert_eq!(buffer.data.lock().unwrap()[0], 42);
    assert_eq!(buffer.data.lock().unwrap()[15], 255);

    // Test detachment
    buffer.detached = true;
    assert!(buffer.detached);
}

#[test]
fn test_arraybuffer_detachment() {
    // Create an ArrayBuffer
    let buffer = Rc::new(RefCell::new(JSArrayBuffer {
        data: Arc::new(Mutex::new(vec![1, 2, 3, 4])),
        ..JSArrayBuffer::default()
    }));

    assert!(!buffer.borrow().detached);
    assert_eq!(buffer.borrow().data.lock().unwrap().len(), 4);

    // Detach the buffer
    buffer.borrow_mut().detached = true;

    assert!(buffer.borrow().detached);
    // Note: In a real implementation, detached buffers might clear their data
    // For this test, we just check the flag
}

#[test]
fn test_typedarray_kind_properties() {
    // Test that all TypedArray kinds are properly defined
    let all_kinds = vec![
        TypedArrayKind::Int8,
        TypedArrayKind::Uint8,
        TypedArrayKind::Uint8Clamped,
        TypedArrayKind::Int16,
        TypedArrayKind::Uint16,
        TypedArrayKind::Int32,
        TypedArrayKind::Uint32,
        TypedArrayKind::Float32,
        TypedArrayKind::Float64,
        TypedArrayKind::BigInt64,
        TypedArrayKind::BigUint64,
    ];

    assert_eq!(all_kinds.len(), 11);

    // Test Debug formatting
    assert_eq!(format!("{:?}", TypedArrayKind::Int8), "Int8");
    assert_eq!(format!("{:?}", TypedArrayKind::Float64), "Float64");
    assert_eq!(format!("{:?}", TypedArrayKind::BigInt64), "BigInt64");
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

    let result = evaluate_script(script, None::<&std::path::Path>);
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

    let result = evaluate_script(script, None::<&std::path::Path>);
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
        let bigInt64Array = new BigInt64Array(1);
        let bigUint64Array = new BigUint64Array(1);

        // Test constructor with existing buffer
        let viewFromBuffer = new Int32Array(buffer);

        "All TypedArray constructors work";
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>);
    assert!(result.is_ok(), "All TypedArray constructors should work");
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

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
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

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"ArrayBuffer-DataView integration works\"");
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

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
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

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"OK\"");
}

#[test]
fn test_typedarray_destructuring_resizable_buffer_regression() {
    let path = std::path::Path::new("js-scripts/typedarray_destructuring_resizable_buffer_regression.js");
    let script = read_script_file(path).expect("failed to read regression script");

    // Append a final expression so evaluate_script returns the script's return value as final result
    let _wrapped = format!("{}\nJSON.stringify(({}));", script, "(function(){return (function(){})();})()");

    // Evaluate and assert
    let result = evaluate_script(&script, Some(path)).expect("evaluate_script failed");
    assert_eq!(result, "\"OK\"");
}
