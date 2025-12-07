use crate::core::{Expr, JSObjectData, JSObjectDataPtr, Value, evaluate_expr, obj_get_value, obj_set_value};
use crate::error::JSError;
use crate::unicode::utf8_to_utf16;
use crate::{JSArrayBuffer, JSDataView, JSTypedArray, TypedArrayKind};
use std::cell::RefCell;
use std::rc::Rc;

/// Create an ArrayBuffer constructor object
pub fn make_arraybuffer_constructor() -> Result<JSObjectDataPtr, JSError> {
    let obj = Rc::new(RefCell::new(JSObjectData::new()));

    // Set the constructor function
    obj_set_value(&obj, &"prototype".into(), Value::Object(make_arraybuffer_prototype()?))?;
    obj_set_value(&obj, &"name".into(), Value::String(utf8_to_utf16("ArrayBuffer")))?;

    // Mark as ArrayBuffer constructor
    obj_set_value(&obj, &"__arraybuffer".into(), Value::Boolean(true))?;

    Ok(obj)
}

/// Create the ArrayBuffer prototype
pub fn make_arraybuffer_prototype() -> Result<JSObjectDataPtr, JSError> {
    let proto = Rc::new(RefCell::new(JSObjectData::new()));

    // Add methods to prototype
    obj_set_value(&proto, &"constructor".into(), Value::Function("ArrayBuffer".to_string()))?;
    obj_set_value(
        &proto,
        &"byteLength".into(),
        Value::Function("ArrayBuffer.prototype.byteLength".to_string()),
    )?;
    obj_set_value(&proto, &"slice".into(), Value::Function("ArrayBuffer.prototype.slice".to_string()))?;

    Ok(proto)
}

/// Create a DataView constructor object
pub fn make_dataview_constructor() -> Result<JSObjectDataPtr, JSError> {
    let obj = Rc::new(RefCell::new(JSObjectData::new()));

    obj_set_value(&obj, &"prototype".into(), Value::Object(make_dataview_prototype()?))?;
    obj_set_value(&obj, &"name".into(), Value::String(utf8_to_utf16("DataView")))?;

    // Mark as DataView constructor
    obj_set_value(&obj, &"__dataview".into(), Value::Boolean(true))?;

    Ok(obj)
}

/// Create the DataView prototype
pub fn make_dataview_prototype() -> Result<JSObjectDataPtr, JSError> {
    let proto = Rc::new(RefCell::new(JSObjectData::new()));

    obj_set_value(&proto, &"constructor".into(), Value::Function("DataView".to_string()))?;
    obj_set_value(&proto, &"buffer".into(), Value::Function("DataView.prototype.buffer".to_string()))?;
    obj_set_value(
        &proto,
        &"byteLength".into(),
        Value::Function("DataView.prototype.byteLength".to_string()),
    )?;
    obj_set_value(
        &proto,
        &"byteOffset".into(),
        Value::Function("DataView.prototype.byteOffset".to_string()),
    )?;

    // DataView methods for different data types
    obj_set_value(&proto, &"getInt8".into(), Value::Function("DataView.prototype.getInt8".to_string()))?;
    obj_set_value(
        &proto,
        &"getUint8".into(),
        Value::Function("DataView.prototype.getUint8".to_string()),
    )?;
    obj_set_value(
        &proto,
        &"getInt16".into(),
        Value::Function("DataView.prototype.getInt16".to_string()),
    )?;
    obj_set_value(
        &proto,
        &"getUint16".into(),
        Value::Function("DataView.prototype.getUint16".to_string()),
    )?;
    obj_set_value(
        &proto,
        &"getInt32".into(),
        Value::Function("DataView.prototype.getInt32".to_string()),
    )?;
    obj_set_value(
        &proto,
        &"getUint32".into(),
        Value::Function("DataView.prototype.getUint32".to_string()),
    )?;
    obj_set_value(
        &proto,
        &"getFloat32".into(),
        Value::Function("DataView.prototype.getFloat32".to_string()),
    )?;
    obj_set_value(
        &proto,
        &"getFloat64".into(),
        Value::Function("DataView.prototype.getFloat64".to_string()),
    )?;

    obj_set_value(&proto, &"setInt8".into(), Value::Function("DataView.prototype.setInt8".to_string()))?;
    obj_set_value(
        &proto,
        &"setUint8".into(),
        Value::Function("DataView.prototype.setUint8".to_string()),
    )?;
    obj_set_value(
        &proto,
        &"setInt16".into(),
        Value::Function("DataView.prototype.setInt16".to_string()),
    )?;
    obj_set_value(
        &proto,
        &"setUint16".into(),
        Value::Function("DataView.prototype.setUint16".to_string()),
    )?;
    obj_set_value(
        &proto,
        &"setInt32".into(),
        Value::Function("DataView.prototype.setInt32".to_string()),
    )?;
    obj_set_value(
        &proto,
        &"setUint32".into(),
        Value::Function("DataView.prototype.setUint32".to_string()),
    )?;
    obj_set_value(
        &proto,
        &"setFloat32".into(),
        Value::Function("DataView.prototype.setFloat32".to_string()),
    )?;
    obj_set_value(
        &proto,
        &"setFloat64".into(),
        Value::Function("DataView.prototype.setFloat64".to_string()),
    )?;

    Ok(proto)
}

/// Create TypedArray constructors
pub fn make_typedarray_constructors() -> Result<Vec<(String, JSObjectDataPtr)>, JSError> {
    let kinds = vec![
        ("Int8Array", TypedArrayKind::Int8),
        ("Uint8Array", TypedArrayKind::Uint8),
        ("Uint8ClampedArray", TypedArrayKind::Uint8Clamped),
        ("Int16Array", TypedArrayKind::Int16),
        ("Uint16Array", TypedArrayKind::Uint16),
        ("Int32Array", TypedArrayKind::Int32),
        ("Uint32Array", TypedArrayKind::Uint32),
        ("Float32Array", TypedArrayKind::Float32),
        ("Float64Array", TypedArrayKind::Float64),
        ("BigInt64Array", TypedArrayKind::BigInt64),
        ("BigUint64Array", TypedArrayKind::BigUint64),
    ];

    let mut constructors = Vec::new();

    for (name, kind) in kinds {
        let constructor = make_typedarray_constructor(name, kind)?;
        constructors.push((name.to_string(), constructor));
    }

    Ok(constructors)
}

fn typedarray_kind_to_number(kind: &TypedArrayKind) -> i32 {
    match kind {
        TypedArrayKind::Int8 => 0,
        TypedArrayKind::Uint8 => 1,
        TypedArrayKind::Uint8Clamped => 2,
        TypedArrayKind::Int16 => 3,
        TypedArrayKind::Uint16 => 4,
        TypedArrayKind::Int32 => 5,
        TypedArrayKind::Uint32 => 6,
        TypedArrayKind::Float32 => 7,
        TypedArrayKind::Float64 => 8,
        TypedArrayKind::BigInt64 => 9,
        TypedArrayKind::BigUint64 => 10,
    }
}

fn make_typedarray_constructor(name: &str, kind: TypedArrayKind) -> Result<JSObjectDataPtr, JSError> {
    // Mark as TypedArray constructor with kind
    let kind_value = typedarray_kind_to_number(&kind);

    let obj = Rc::new(RefCell::new(JSObjectData::new()));

    obj_set_value(&obj, &"prototype".into(), Value::Object(make_typedarray_prototype(kind)?))?;
    obj_set_value(&obj, &"name".into(), Value::String(utf8_to_utf16(name)))?;

    obj_set_value(&obj, &"__kind".into(), Value::Number(kind_value as f64))?;

    Ok(obj)
}

fn make_typedarray_prototype(kind: TypedArrayKind) -> Result<JSObjectDataPtr, JSError> {
    let proto = Rc::new(RefCell::new(JSObjectData::new()));

    // Store the kind in the prototype for later use
    let kind_value = match kind {
        TypedArrayKind::Int8 => 0,
        TypedArrayKind::Uint8 => 1,
        TypedArrayKind::Uint8Clamped => 2,
        TypedArrayKind::Int16 => 3,
        TypedArrayKind::Uint16 => 4,
        TypedArrayKind::Int32 => 5,
        TypedArrayKind::Uint32 => 6,
        TypedArrayKind::Float32 => 7,
        TypedArrayKind::Float64 => 8,
        TypedArrayKind::BigInt64 => 9,
        TypedArrayKind::BigUint64 => 10,
    };

    obj_set_value(&proto, &"__kind".into(), Value::Number(kind_value as f64))?;
    obj_set_value(&proto, &"constructor".into(), Value::Function("TypedArray".to_string()))?;

    // TypedArray properties and methods
    obj_set_value(&proto, &"buffer".into(), Value::Function("TypedArray.prototype.buffer".to_string()))?;
    obj_set_value(
        &proto,
        &"byteLength".into(),
        Value::Function("TypedArray.prototype.byteLength".to_string()),
    )?;
    obj_set_value(
        &proto,
        &"byteOffset".into(),
        Value::Function("TypedArray.prototype.byteOffset".to_string()),
    )?;
    obj_set_value(&proto, &"length".into(), Value::Function("TypedArray.prototype.length".to_string()))?;

    // Array methods that TypedArrays inherit
    obj_set_value(&proto, &"set".into(), Value::Function("TypedArray.prototype.set".to_string()))?;
    obj_set_value(
        &proto,
        &"subarray".into(),
        Value::Function("TypedArray.prototype.subarray".to_string()),
    )?;

    Ok(proto)
}

/// Handle ArrayBuffer constructor calls
pub fn handle_arraybuffer_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // ArrayBuffer(length)
    if args.is_empty() {
        return Err(raise_eval_error!("ArrayBuffer constructor requires a length argument"));
    }

    let length_val = evaluate_expr(env, &args[0])?;
    let length = match length_val {
        Value::Number(n) if n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0 => n as usize,
        _ => return Err(raise_eval_error!("ArrayBuffer length must be a non-negative integer")),
    };

    // Create ArrayBuffer instance
    let buffer = Rc::new(RefCell::new(JSArrayBuffer {
        data: vec![0; length],
        detached: false,
    }));

    // Create the ArrayBuffer object
    let obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(&obj, &"__arraybuffer".into(), Value::ArrayBuffer(buffer))?;

    // Set prototype
    let proto = make_arraybuffer_prototype()?;
    obj.borrow_mut().prototype = Some(proto);

    Ok(Value::Object(obj))
}

/// Handle DataView constructor calls
pub fn handle_dataview_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // DataView(buffer [, byteOffset [, byteLength]])
    if args.is_empty() {
        return Err(raise_eval_error!("DataView constructor requires a buffer argument"));
    }

    let buffer_val = evaluate_expr(env, &args[0])?;
    let buffer = match buffer_val {
        Value::Object(obj) => {
            if let Some(ab_val) = obj_get_value(&obj, &"__arraybuffer".into())? {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    ab.clone()
                } else {
                    return Err(raise_eval_error!("DataView buffer must be an ArrayBuffer"));
                }
            } else {
                return Err(raise_eval_error!("DataView buffer must be an ArrayBuffer"));
            }
        }
        _ => return Err(raise_eval_error!("DataView buffer must be an ArrayBuffer")),
    };

    let byte_offset = if args.len() > 1 {
        let offset_val = evaluate_expr(env, &args[1])?;
        match offset_val {
            Value::Number(n) if n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0 => n as usize,
            _ => return Err(raise_eval_error!("DataView byteOffset must be a non-negative integer")),
        }
    } else {
        0
    };

    let byte_length = if args.len() > 2 {
        let length_val = evaluate_expr(env, &args[2])?;
        match length_val {
            Value::Number(n) if n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0 => n as usize,
            _ => return Err(raise_eval_error!("DataView byteLength must be a non-negative integer")),
        }
    } else {
        buffer.borrow().data.len() - byte_offset
    };

    // Validate bounds
    if byte_offset + byte_length > buffer.borrow().data.len() {
        return Err(raise_eval_error!("DataView bounds exceed buffer size"));
    }

    // Create DataView instance
    let data_view = Rc::new(RefCell::new(JSDataView {
        buffer,
        byte_offset,
        byte_length,
    }));

    // Create the DataView object
    let obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(&obj, &"__dataview".into(), Value::DataView(data_view))?;

    // Set prototype
    let proto = make_dataview_prototype()?;
    obj.borrow_mut().prototype = Some(proto);

    Ok(Value::Object(obj))
}

/// Handle TypedArray constructor calls
pub fn handle_typedarray_constructor(constructor_obj: &JSObjectDataPtr, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Get the kind from the constructor
    let kind_val = obj_get_value(constructor_obj, &"__kind".into())?;
    let kind = if let Some(kind_val) = kind_val {
        if let Value::Number(kind_num) = *kind_val.borrow() {
            match kind_num as i32 {
                0 => TypedArrayKind::Int8,
                1 => TypedArrayKind::Uint8,
                2 => TypedArrayKind::Uint8Clamped,
                3 => TypedArrayKind::Int16,
                4 => TypedArrayKind::Uint16,
                5 => TypedArrayKind::Int32,
                6 => TypedArrayKind::Uint32,
                7 => TypedArrayKind::Float32,
                8 => TypedArrayKind::Float64,
                9 => TypedArrayKind::BigInt64,
                10 => TypedArrayKind::BigUint64,
                _ => return Err(raise_eval_error!("Invalid TypedArray kind")),
            }
        } else {
            return Err(raise_eval_error!("Invalid TypedArray constructor"));
        }
    } else {
        return Err(raise_eval_error!("Invalid TypedArray constructor"));
    };

    let element_size = match kind {
        TypedArrayKind::Int8 | TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => 1,
        TypedArrayKind::Int16 | TypedArrayKind::Uint16 => 2,
        TypedArrayKind::Int32 | TypedArrayKind::Uint32 | TypedArrayKind::Float32 => 4,
        TypedArrayKind::Float64 | TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => 8,
    };

    let (buffer, byte_offset, length) = if args.is_empty() {
        // new TypedArray() - create empty array
        let buffer = Rc::new(RefCell::new(JSArrayBuffer {
            data: vec![],
            detached: false,
        }));
        (buffer, 0, 0)
    } else if args.len() == 1 {
        let arg_val = evaluate_expr(env, &args[0])?;
        match arg_val {
            Value::Number(n) if n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0 => {
                // new TypedArray(length)
                let length = n as usize;
                let buffer = Rc::new(RefCell::new(JSArrayBuffer {
                    data: vec![0; length * element_size],
                    detached: false,
                }));
                (buffer, 0, length)
            }
            Value::Object(obj) => {
                // Check if it's another TypedArray or ArrayBuffer
                if let Some(ta_val) = obj_get_value(&obj, &"__typedarray".into())? {
                    if let Value::TypedArray(ta) = &*ta_val.borrow() {
                        // new TypedArray(typedArray) - copy constructor
                        let src_length = ta.borrow().length;
                        let buffer = Rc::new(RefCell::new(JSArrayBuffer {
                            data: vec![0; src_length * element_size],
                            detached: false,
                        }));
                        // TODO: Copy data from source TypedArray
                        (buffer, 0, src_length)
                    } else {
                        return Err(raise_eval_error!("Invalid TypedArray constructor argument"));
                    }
                } else if let Some(ab_val) = obj_get_value(&obj, &"__arraybuffer".into())? {
                    if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                        // new TypedArray(buffer)
                        (ab.clone(), 0, ab.borrow().data.len() / element_size)
                    } else {
                        return Err(raise_eval_error!("Invalid TypedArray constructor argument"));
                    }
                } else {
                    return Err(raise_eval_error!("Invalid TypedArray constructor argument"));
                }
            }
            _ => return Err(raise_eval_error!("Invalid TypedArray constructor argument")),
        }
    } else if args.len() == 2 {
        // new TypedArray(buffer, byteOffset)
        let buffer_val = evaluate_expr(env, &args[0])?;
        let offset_val = evaluate_expr(env, &args[1])?;

        if let Value::Object(obj) = buffer_val {
            if let Some(ab_val) = obj_get_value(&obj, &"__arraybuffer".into())? {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    if let Value::Number(offset_num) = offset_val {
                        let offset = offset_num as usize;
                        if !offset.is_multiple_of(element_size) {
                            return Err(raise_eval_error!("TypedArray byteOffset must be multiple of element size"));
                        }
                        let remaining_bytes = ab.borrow().data.len() - offset;
                        let length = remaining_bytes / element_size;
                        (ab.clone(), offset, length)
                    } else {
                        return Err(raise_eval_error!("TypedArray byteOffset must be a number"));
                    }
                } else {
                    return Err(raise_eval_error!("First argument must be an ArrayBuffer"));
                }
            } else {
                return Err(raise_eval_error!("First argument must be an ArrayBuffer"));
            }
        } else {
            return Err(raise_eval_error!("First argument must be an ArrayBuffer"));
        }
    } else if args.len() == 3 {
        // new TypedArray(buffer, byteOffset, length)
        let buffer_val = evaluate_expr(env, &args[0])?;
        let offset_val = evaluate_expr(env, &args[1])?;
        let length_val = evaluate_expr(env, &args[2])?;

        if let Value::Object(obj) = buffer_val {
            if let Some(ab_val) = obj_get_value(&obj, &"__arraybuffer".into())? {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    if let (Value::Number(offset_num), Value::Number(length_num)) = (offset_val, length_val) {
                        let offset = offset_num as usize;
                        let length = length_num as usize;
                        if !offset.is_multiple_of(element_size) {
                            return Err(raise_eval_error!("TypedArray byteOffset must be multiple of element size"));
                        }
                        if length * element_size + offset > ab.borrow().data.len() {
                            return Err(raise_eval_error!("TypedArray length exceeds buffer size"));
                        }
                        (ab.clone(), offset, length)
                    } else {
                        return Err(raise_eval_error!("TypedArray byteOffset and length must be numbers"));
                    }
                } else {
                    return Err(raise_eval_error!("First argument must be an ArrayBuffer"));
                }
            } else {
                return Err(raise_eval_error!("First argument must be an ArrayBuffer"));
            }
        } else {
            return Err(raise_eval_error!("First argument must be an ArrayBuffer"));
        }
    } else {
        return Err(raise_eval_error!("TypedArray constructor with more than 3 arguments not supported"));
    };

    // Create the TypedArray object
    let obj = Rc::new(RefCell::new(JSObjectData::new()));

    // Set prototype first
    let proto = make_typedarray_prototype(kind.clone())?;
    obj.borrow_mut().prototype = Some(proto);

    // Create TypedArray instance
    let typed_array = Rc::new(RefCell::new(JSTypedArray {
        kind,
        buffer,
        byte_offset,
        length,
    }));

    obj_set_value(&obj, &"__typedarray".into(), Value::TypedArray(typed_array))?;

    Ok(Value::Object(obj))
}

/// Handle DataView instance method calls
pub fn handle_dataview_method(obj_map: &JSObjectDataPtr, method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Get the DataView from the object
    let dv_val = obj_get_value(obj_map, &"__dataview".into())?;
    let data_view_rc = if let Some(dv_val) = dv_val {
        if let Value::DataView(dv) = &*dv_val.borrow() {
            dv.clone()
        } else {
            return Err(raise_eval_error!("Invalid DataView object"));
        }
    } else {
        return Err(raise_eval_error!("DataView method called on non-DataView object"));
    };

    match method {
        // Get methods - use immutable borrow
        "getInt8" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("DataView.getInt8 requires exactly 1 argument"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let data_view = data_view_rc.borrow();
            data_view
                .get_int8(offset)
                .map(|v| Value::Number(v as f64))
                .map_err(|e| raise_eval_error!(e))
        }
        "getUint8" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("DataView.getUint8 requires exactly 1 argument"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let data_view = data_view_rc.borrow();
            data_view
                .get_uint8(offset)
                .map(|v| Value::Number(v as f64))
                .map_err(|e| raise_eval_error!(e))
        }
        "getInt16" => {
            if args.is_empty() || args.len() > 2 {
                return Err(raise_eval_error!("DataView.getInt16 requires 1 or 2 arguments"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let little_endian = if args.len() > 1 {
                let le_val = evaluate_expr(env, &args[1])?;
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean")),
                }
            } else {
                false
            };
            let data_view = data_view_rc.borrow();
            data_view
                .get_int16(offset, little_endian)
                .map(|v| Value::Number(v as f64))
                .map_err(|e| raise_eval_error!(e))
        }
        "getUint16" => {
            if args.is_empty() || args.len() > 2 {
                return Err(raise_eval_error!("DataView.getUint16 requires 1 or 2 arguments"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let little_endian = if args.len() > 1 {
                let le_val = evaluate_expr(env, &args[1])?;
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean")),
                }
            } else {
                false
            };
            let data_view = data_view_rc.borrow();
            data_view
                .get_uint16(offset, little_endian)
                .map(|v| Value::Number(v as f64))
                .map_err(|e| raise_eval_error!(e))
        }
        "getInt32" => {
            if args.is_empty() || args.len() > 2 {
                return Err(raise_eval_error!("DataView.getInt32 requires 1 or 2 arguments"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let little_endian = if args.len() > 1 {
                let le_val = evaluate_expr(env, &args[1])?;
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean")),
                }
            } else {
                false
            };
            let data_view = data_view_rc.borrow();
            data_view
                .get_int32(offset, little_endian)
                .map(|v| Value::Number(v as f64))
                .map_err(|e| raise_eval_error!(e))
        }
        "getUint32" => {
            if args.is_empty() || args.len() > 2 {
                return Err(raise_eval_error!("DataView.getUint32 requires 1 or 2 arguments"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let little_endian = if args.len() > 1 {
                let le_val = evaluate_expr(env, &args[1])?;
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean")),
                }
            } else {
                false
            };
            let data_view = data_view_rc.borrow();
            data_view
                .get_uint32(offset, little_endian)
                .map(|v| Value::Number(v as f64))
                .map_err(|e| raise_eval_error!(e))
        }
        "getFloat32" => {
            if args.is_empty() || args.len() > 2 {
                return Err(raise_eval_error!("DataView.getFloat32 requires 1 or 2 arguments"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let little_endian = if args.len() > 1 {
                let le_val = evaluate_expr(env, &args[1])?;
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean")),
                }
            } else {
                false
            };
            let data_view = data_view_rc.borrow();
            data_view
                .get_float32(offset, little_endian)
                .map(|v| Value::Number(v as f64))
                .map_err(|e| raise_eval_error!(e))
        }
        "getFloat64" => {
            if args.is_empty() || args.len() > 2 {
                return Err(raise_eval_error!("DataView.getFloat64 requires 1 or 2 arguments"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let little_endian = if args.len() > 1 {
                let le_val = evaluate_expr(env, &args[1])?;
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean")),
                }
            } else {
                false
            };
            let data_view = data_view_rc.borrow();
            data_view
                .get_float64(offset, little_endian)
                .map(Value::Number)
                .map_err(|e| raise_eval_error!(e))
        }
        // Set methods - use mutable borrow
        "setInt8" => {
            if args.len() != 2 {
                return Err(raise_eval_error!("DataView.setInt8 requires exactly 2 arguments"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let value_val = evaluate_expr(env, &args[1])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let value = match value_val {
                Value::Number(n) => n as i8,
                _ => return Err(raise_eval_error!("DataView value must be a number")),
            };
            let mut data_view = data_view_rc.borrow_mut();
            data_view.set_int8(offset, value).map_err(|e| raise_eval_error!(e))?;
            Ok(Value::Undefined)
        }
        "setUint8" => {
            if args.len() != 2 {
                return Err(raise_eval_error!("DataView.setUint8 requires exactly 2 arguments"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let value_val = evaluate_expr(env, &args[1])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let value = match value_val {
                Value::Number(n) => n as u8,
                _ => return Err(raise_eval_error!("DataView value must be a number")),
            };
            let mut data_view = data_view_rc.borrow_mut();
            data_view.set_uint8(offset, value).map_err(|e| raise_eval_error!(e))?;
            Ok(Value::Undefined)
        }
        "setInt16" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("DataView.setInt16 requires 2 or 3 arguments"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let value_val = evaluate_expr(env, &args[1])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let value = match value_val {
                Value::Number(n) => n as i16,
                _ => return Err(raise_eval_error!("DataView value must be a number")),
            };
            let little_endian = if args.len() > 2 {
                let le_val = evaluate_expr(env, &args[2])?;
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean")),
                }
            } else {
                false
            };
            let mut data_view = data_view_rc.borrow_mut();
            data_view
                .set_int16(offset, value, little_endian)
                .map_err(|e| raise_eval_error!(e))?;
            Ok(Value::Undefined)
        }
        "setUint16" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("DataView.setUint16 requires 2 or 3 arguments"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let value_val = evaluate_expr(env, &args[1])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let value = match value_val {
                Value::Number(n) => n as u16,
                _ => return Err(raise_eval_error!("DataView value must be a number")),
            };
            let little_endian = if args.len() > 2 {
                let le_val = evaluate_expr(env, &args[2])?;
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean")),
                }
            } else {
                false
            };
            let mut data_view = data_view_rc.borrow_mut();
            data_view
                .set_uint16(offset, value, little_endian)
                .map_err(|e| raise_eval_error!(e))?;
            Ok(Value::Undefined)
        }
        "setInt32" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("DataView.setInt32 requires 2 or 3 arguments"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let value_val = evaluate_expr(env, &args[1])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let value = match value_val {
                Value::Number(n) => n as i32,
                _ => return Err(raise_eval_error!("DataView value must be a number")),
            };
            let little_endian = if args.len() > 2 {
                let le_val = evaluate_expr(env, &args[2])?;
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean")),
                }
            } else {
                false
            };
            let mut data_view = data_view_rc.borrow_mut();
            data_view
                .set_int32(offset, value, little_endian)
                .map_err(|e| raise_eval_error!(e))?;
            Ok(Value::Undefined)
        }
        "setUint32" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("DataView.setUint32 requires 2 or 3 arguments"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let value_val = evaluate_expr(env, &args[1])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let value = match value_val {
                Value::Number(n) => n as u32,
                _ => return Err(raise_eval_error!("DataView value must be a number")),
            };
            let little_endian = if args.len() > 2 {
                let le_val = evaluate_expr(env, &args[2])?;
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean")),
                }
            } else {
                false
            };
            let mut data_view = data_view_rc.borrow_mut();
            data_view
                .set_uint32(offset, value, little_endian)
                .map_err(|e| raise_eval_error!(e))?;
            Ok(Value::Undefined)
        }
        "setFloat32" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("DataView.setFloat32 requires 2 or 3 arguments"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let value_val = evaluate_expr(env, &args[1])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let value = match value_val {
                Value::Number(n) => n as f32,
                _ => return Err(raise_eval_error!("DataView value must be a number")),
            };
            let little_endian = if args.len() > 2 {
                let le_val = evaluate_expr(env, &args[2])?;
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean")),
                }
            } else {
                false
            };
            let mut data_view = data_view_rc.borrow_mut();
            data_view
                .set_float32(offset, value, little_endian)
                .map_err(|e| raise_eval_error!(e))?;
            Ok(Value::Undefined)
        }
        "setFloat64" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("DataView.setFloat64 requires 2 or 3 arguments"));
            }
            let offset_val = evaluate_expr(env, &args[0])?;
            let value_val = evaluate_expr(env, &args[1])?;
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer")),
            };
            let value = match value_val {
                Value::Number(n) => n,
                _ => return Err(raise_eval_error!("DataView value must be a number")),
            };
            let little_endian = if args.len() > 2 {
                let le_val = evaluate_expr(env, &args[2])?;
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean")),
                }
            } else {
                false
            };
            let mut data_view = data_view_rc.borrow_mut();
            data_view
                .set_float64(offset, value, little_endian)
                .map_err(|e| raise_eval_error!(e))?;
            Ok(Value::Undefined)
        }
        // Property accessors
        "buffer" => {
            let data_view = data_view_rc.borrow();
            Ok(Value::ArrayBuffer(data_view.buffer.clone()))
        }
        "byteLength" => {
            let data_view = data_view_rc.borrow();
            Ok(Value::Number(data_view.byte_length as f64))
        }
        "byteOffset" => {
            let data_view = data_view_rc.borrow();
            Ok(Value::Number(data_view.byte_offset as f64))
        }
        _ => Err(raise_eval_error!(format!("DataView method '{method}' not implemented"))),
    }
}
