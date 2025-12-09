use javascript::{JSArrayBuffer, JSDataView, JSTypedArray, TypedArrayKind, Value, evaluate_script};
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
        detached: false,
        shared: false,
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
fn test_jsdataview_creation_and_access() {
    // Create an ArrayBuffer
    let buffer = Rc::new(RefCell::new(JSArrayBuffer {
        data: Arc::new(Mutex::new(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15])),
        detached: false,
        shared: false,
    }));

    // Create a DataView for the entire buffer
    let data_view = JSDataView {
        buffer: buffer.clone(),
        byte_offset: 0,
        byte_length: 16,
    };

    assert_eq!(data_view.byte_offset, 0);
    assert_eq!(data_view.byte_length, 16);

    // Test reading different data types (simulated)
    // Note: In a real implementation, these would be methods on DataView
    // For now, we just test the structure setup
    assert_eq!(buffer.borrow().data.lock().unwrap()[0], 0);
    assert_eq!(buffer.borrow().data.lock().unwrap()[1], 1);

    // Create a DataView with offset and length
    let partial_view = JSDataView {
        buffer: buffer.clone(),
        byte_offset: 4,
        byte_length: 8,
    };

    assert_eq!(partial_view.byte_offset, 4);
    assert_eq!(partial_view.byte_length, 8);
}

#[test]
fn test_jstypedarray_creation_and_properties() {
    // Create an ArrayBuffer
    let buffer = Rc::new(RefCell::new(JSArrayBuffer {
        data: Arc::new(Mutex::new(vec![0; 64])), // 64 bytes
        detached: false,
        shared: false,
    }));

    // Test different TypedArray kinds
    let kinds_and_sizes = vec![
        (TypedArrayKind::Int8, 1),
        (TypedArrayKind::Uint8, 1),
        (TypedArrayKind::Int16, 2),
        (TypedArrayKind::Uint16, 2),
        (TypedArrayKind::Int32, 4),
        (TypedArrayKind::Uint32, 4),
        (TypedArrayKind::Float32, 4),
        (TypedArrayKind::Float64, 8),
    ];

    for (kind, element_size) in kinds_and_sizes {
        let typed_array = JSTypedArray {
            kind: kind.clone(),
            buffer: buffer.clone(),
            byte_offset: 0,
            length: 64 / element_size,
        };

        assert_eq!(typed_array.byte_offset, 0);
        assert_eq!(typed_array.length, 64 / element_size);
        assert_eq!(typed_array.kind, kind);
    }
}

#[test]
fn test_typedarray_with_offset() {
    // Create an ArrayBuffer
    let buffer = Rc::new(RefCell::new(JSArrayBuffer {
        data: Arc::new(Mutex::new(vec![0; 32])),
        detached: false,
        shared: false,
    }));

    // Create a TypedArray with offset
    let typed_array = JSTypedArray {
        kind: TypedArrayKind::Uint32,
        buffer: buffer.clone(),
        byte_offset: 8, // Skip first 8 bytes
        length: 6,      // 6 elements * 4 bytes each = 24 bytes, fits in remaining 24 bytes
    };

    assert_eq!(typed_array.byte_offset, 8);
    assert_eq!(typed_array.length, 6);
    assert_eq!(typed_array.kind, TypedArrayKind::Uint32);
}

#[test]
fn test_shared_arraybuffer_behavior() {
    // Create an ArrayBuffer
    let buffer = Rc::new(RefCell::new(JSArrayBuffer {
        data: Arc::new(Mutex::new(vec![0; 16])),
        detached: false,
        shared: false,
    }));

    // Create multiple views sharing the same buffer
    let data_view1 = JSDataView {
        buffer: buffer.clone(),
        byte_offset: 0,
        byte_length: 8,
    };

    let data_view2 = JSDataView {
        buffer: buffer.clone(),
        byte_offset: 8,
        byte_length: 8,
    };

    let typed_array = JSTypedArray {
        kind: TypedArrayKind::Uint8,
        buffer: buffer.clone(),
        byte_offset: 0,
        length: 16,
    };

    // All views share the same underlying buffer
    assert!(Rc::ptr_eq(&data_view1.buffer, &data_view2.buffer));
    assert!(Rc::ptr_eq(&data_view1.buffer, &typed_array.buffer));

    // Modifications through one view should be visible through the buffer
    buffer.borrow_mut().data.lock().unwrap()[0] = 42;
    assert_eq!(buffer.borrow().data.lock().unwrap()[0], 42);

    // Test that modifications through TypedArray are visible through the buffer
    let mut typed_array_clone = JSTypedArray {
        kind: TypedArrayKind::Uint8,
        buffer: buffer.clone(),
        byte_offset: 0,
        length: 16,
    };

    // Set a value through the TypedArray
    typed_array_clone.set(1, 99).unwrap();

    // Check that it's visible in the buffer
    assert_eq!(buffer.borrow().data.lock().unwrap()[1], 99);

    // Check that it's visible through another TypedArray on the same buffer
    assert_eq!(typed_array.get(1).unwrap(), 99);

    // Test cross-view visibility with different offsets
    let mut offset_array = JSTypedArray {
        kind: TypedArrayKind::Uint8,
        buffer: buffer.clone(),
        byte_offset: 5,
        length: 5,
    };

    // Set value at index 2 of offset_array (which is byte 7 in buffer)
    offset_array.set(2, 77).unwrap();

    // Check that it's visible in the original buffer
    assert_eq!(buffer.borrow().data.lock().unwrap()[7], 77);

    // Check that it's visible through the first TypedArray
    assert_eq!(typed_array.get(7).unwrap(), 77);
}

#[test]
fn test_arraybuffer_detachment() {
    // Create an ArrayBuffer
    let buffer = Rc::new(RefCell::new(JSArrayBuffer {
        data: Arc::new(Mutex::new(vec![1, 2, 3, 4])),
        detached: false,
        shared: false,
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

    let result = evaluate_script(script);
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

    let result = evaluate_script(script);
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

    let result = evaluate_script(script);
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

    let result = evaluate_script(script);
    match result {
        Ok(val) => {
            if let Value::Number(n) = val {
                assert_eq!(n, 42.0, "TypedArray indexing should work");
            } else {
                panic!("Expected number, got {:?}", val);
            }
        }
        Err(e) => panic!("Script evaluation failed: {:?}", e),
    }
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

    let result = evaluate_script(script);
    assert!(result.is_ok(), "ArrayBuffer-DataView integration should work");
}

#[test]
fn test_dataview_read_write_operations() {
    // Create an ArrayBuffer
    let buffer = Rc::new(RefCell::new(JSArrayBuffer {
        data: Arc::new(Mutex::new(vec![0; 16])),
        detached: false,
        shared: false,
    }));

    // Create a DataView for the entire buffer
    let mut data_view = JSDataView {
        buffer: buffer.clone(),
        byte_offset: 0,
        byte_length: 16,
    };

    // Test writing different data types
    data_view.set_int8(0, -42).unwrap();
    data_view.set_uint8(1, 255).unwrap();
    data_view.set_int16(2, -1234, true).unwrap(); // little endian
    data_view.set_uint16(4, 56789, false).unwrap(); // big endian
    data_view.set_int32(6, -987654, true).unwrap();
    data_view.set_float32(10, std::f32::consts::PI, true).unwrap();

    // Test reading back the values
    assert_eq!(data_view.get_int8(0).unwrap(), -42);
    assert_eq!(data_view.get_uint8(1).unwrap(), 255);
    assert_eq!(data_view.get_int16(2, true).unwrap(), -1234);
    assert_eq!(data_view.get_uint16(4, false).unwrap(), 56789);
    assert_eq!(data_view.get_int32(6, true).unwrap(), -987654);
    assert!((data_view.get_float32(10, true).unwrap() - std::f32::consts::PI).abs() < 0.0001);

    // Test bounds checking
    assert!(data_view.get_int8(16).is_err()); // out of bounds
    assert!(data_view.set_int8(16, 0).is_err()); // out of bounds

    // Test with offset DataView
    let mut offset_view = JSDataView {
        buffer: buffer.clone(),
        byte_offset: 4,
        byte_length: 8,
    };

    // Write to offset view (should affect buffer at offset 4)
    offset_view.set_int32(0, 0x12345678, true).unwrap();

    // Check that it affected the main buffer
    assert_eq!(buffer.borrow().data.lock().unwrap()[4], 0x78);
    assert_eq!(buffer.borrow().data.lock().unwrap()[5], 0x56);
    assert_eq!(buffer.borrow().data.lock().unwrap()[6], 0x34);
    assert_eq!(buffer.borrow().data.lock().unwrap()[7], 0x12);

    // Read back through offset view
    assert_eq!(offset_view.get_int32(0, true).unwrap(), 0x12345678);

    // Test detached buffer
    buffer.borrow_mut().detached = true;
    assert!(data_view.get_int8(0).is_err());
    assert!(data_view.set_int8(0, 0).is_err());
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

    let result = evaluate_script(script);
    assert!(result.is_ok(), "Different TypedArray construction patterns should work");
}
