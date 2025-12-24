use crate::core::{Expr, JSObjectDataPtr, Value, evaluate_expr, new_js_object_data, obj_get_key_value, obj_set_key_value};
use crate::error::JSError;
use crate::unicode::utf8_to_utf16;
use crate::{JSArrayBuffer, JSDataView, JSTypedArray, TypedArrayKind};
use num_traits::ToPrimitive;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Condvar;
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Global waiters registry keyed by (buffer_arc_ptr, byte_index). Each waiter
// is an Arc containing a (Mutex<bool>, Condvar) pair the waiting thread blocks on.
#[allow(clippy::type_complexity)]
static WAITERS: LazyLock<Mutex<HashMap<(usize, usize), Vec<Arc<(Mutex<bool>, Condvar)>>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Create an ArrayBuffer constructor object
pub fn make_arraybuffer_constructor() -> Result<JSObjectDataPtr, JSError> {
    let obj = new_js_object_data();

    // Set the constructor function
    obj_set_key_value(&obj, &"prototype".into(), Value::Object(make_arraybuffer_prototype()?))?;
    obj_set_key_value(&obj, &"name".into(), Value::String(utf8_to_utf16("ArrayBuffer")))?;

    // Mark as ArrayBuffer constructor
    obj_set_key_value(&obj, &"__arraybuffer".into(), Value::Boolean(true))?;

    Ok(obj)
}

/// Create the Atomics object with basic atomic methods
pub fn make_atomics_object() -> Result<JSObjectDataPtr, JSError> {
    let obj = new_js_object_data();

    obj_set_key_value(&obj, &"load".into(), Value::Function("Atomics.load".to_string()))?;
    obj_set_key_value(&obj, &"store".into(), Value::Function("Atomics.store".to_string()))?;
    obj_set_key_value(
        &obj,
        &"compareExchange".into(),
        Value::Function("Atomics.compareExchange".to_string()),
    )?;
    obj_set_key_value(&obj, &"exchange".into(), Value::Function("Atomics.exchange".to_string()))?;
    obj_set_key_value(&obj, &"add".into(), Value::Function("Atomics.add".to_string()))?;
    obj_set_key_value(&obj, &"sub".into(), Value::Function("Atomics.sub".to_string()))?;
    obj_set_key_value(&obj, &"and".into(), Value::Function("Atomics.and".to_string()))?;
    obj_set_key_value(&obj, &"or".into(), Value::Function("Atomics.or".to_string()))?;
    obj_set_key_value(&obj, &"xor".into(), Value::Function("Atomics.xor".to_string()))?;
    obj_set_key_value(&obj, &"wait".into(), Value::Function("Atomics.wait".to_string()))?;
    obj_set_key_value(&obj, &"notify".into(), Value::Function("Atomics.notify".to_string()))?;
    obj_set_key_value(&obj, &"isLockFree".into(), Value::Function("Atomics.isLockFree".to_string()))?;

    Ok(obj)
}

/// Handle Atomics.* calls (minimal mutex-backed implementations)
pub fn handle_atomics_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Helper to extract TypedArray from first argument
    if args.is_empty() {
        return Err(raise_eval_error!("Atomics method requires arguments"));
    }
    // Special-case Atomics.isLockFree which accepts a size (in bytes)
    // and does not require a TypedArray as the first argument.
    if method == "isLockFree" {
        if args.len() != 1 {
            return Err(raise_eval_error!("Atomics.isLockFree requires 1 argument"));
        }
        let size_val = evaluate_expr(env, &args[0])?;
        let size = match size_val {
            Value::Number(n) => n as usize,
            _ => return Err(raise_eval_error!("Atomics.isLockFree argument must be a number")),
        };

        #[allow(clippy::match_like_matches_macro, clippy::needless_bool)]
        let res = match size {
            1 => cfg!(target_has_atomic = "8"),
            2 => cfg!(target_has_atomic = "16"),
            4 => cfg!(target_has_atomic = "32"),
            8 => cfg!(target_has_atomic = "64"),
            _ => false,
        };
        return Ok(Value::Boolean(res));
    }
    let ta_val = evaluate_expr(env, &args[0])?;
    let ta_obj = if let Value::Object(object) = ta_val {
        if let Some(ta_rc) = obj_get_key_value(&object, &"__typedarray".into())? {
            if let Value::TypedArray(ta) = &*ta_rc.borrow() {
                ta.clone()
            } else {
                return Err(raise_eval_error!("First argument to Atomics must be a TypedArray"));
            }
        } else {
            return Err(raise_eval_error!("First argument to Atomics must be a TypedArray"));
        }
    } else {
        return Err(raise_eval_error!("First argument to Atomics must be a TypedArray"));
    };

    match method {
        "load" => {
            if args.len() != 2 {
                return Err(raise_eval_error!("Atomics.load requires 2 arguments"));
            }
            let idx_val = evaluate_expr(env, &args[1])?;
            let idx = match idx_val {
                Value::Number(n) => n as usize,
                _ => return Err(raise_eval_error!("Atomics index must be a number")),
            };
            let v = ta_obj.borrow().get(idx)?;
            Ok(Value::Number(v as f64))
        }
        "store" => {
            if args.len() != 3 {
                return Err(raise_eval_error!("Atomics.store requires 3 arguments"));
            }
            let idx_val = evaluate_expr(env, &args[1])?;
            let val_val = evaluate_expr(env, &args[2])?;
            let idx = match idx_val {
                Value::Number(n) => n as usize,
                _ => return Err(raise_eval_error!("Atomics index must be a number")),
            };
            let v = match val_val {
                Value::Number(n) => n as i64,
                Value::BigInt(b) => b.to_i64().unwrap_or(0),
                _ => return Err(raise_eval_error!("Atomics value must be a number or BigInt")),
            };
            let old = ta_obj.borrow().get(idx)?;
            ta_obj.borrow_mut().set(idx, v)?;
            Ok(Value::Number(old as f64))
        }
        "compareExchange" => {
            if args.len() != 4 {
                return Err(raise_eval_error!("Atomics.compareExchange requires 4 arguments"));
            }
            let idx_val = evaluate_expr(env, &args[1])?;
            let expected_val = evaluate_expr(env, &args[2])?;
            let replacement_val = evaluate_expr(env, &args[3])?;
            let idx = match idx_val {
                Value::Number(n) => n as usize,
                _ => return Err(raise_eval_error!("Atomics index must be a number")),
            };
            let expected = match expected_val {
                Value::Number(n) => n as i64,
                Value::BigInt(b) => b.to_i64().unwrap_or(0),
                _ => return Err(raise_eval_error!("Atomics expected must be a number or BigInt")),
            };
            let replacement = match replacement_val {
                Value::Number(n) => n as i64,
                Value::BigInt(b) => b.to_i64().unwrap_or(0),
                _ => return Err(raise_eval_error!("Atomics replacement must be a number or BigInt")),
            };
            let old = ta_obj.borrow().get(idx)?;
            if old == expected {
                ta_obj.borrow_mut().set(idx, replacement)?;
            }
            Ok(Value::Number(old as f64))
        }
        "add" | "sub" | "and" | "or" | "xor" | "exchange" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!(format!("Atomics.{} invalid args", method)));
            }
            let idx_val = evaluate_expr(env, &args[1])?;
            let idx = match idx_val {
                Value::Number(n) => n as usize,
                _ => return Err(raise_eval_error!("Atomics index must be a number")),
            };
            let operand = if args.len() == 3 {
                let v = evaluate_expr(env, &args[2])?;
                match v {
                    Value::Number(n) => n as i64,
                    Value::BigInt(b) => b.to_i64().unwrap_or(0),
                    _ => return Err(raise_eval_error!("Atomics operand must be a number or BigInt")),
                }
            } else {
                0
            };
            let old = ta_obj.borrow().get(idx)?;
            let new = match method {
                "add" => old.wrapping_add(operand),
                "sub" => old.wrapping_sub(operand),
                "and" => old & operand,
                "or" => old | operand,
                "xor" => old ^ operand,
                "exchange" => operand,
                _ => old,
            };
            ta_obj.borrow_mut().set(idx, new)?;
            Ok(Value::Number(old as f64))
        }
        "wait" => {
            // Atomics.wait(typedArray, index, value[, timeout])
            if args.len() < 3 || args.len() > 4 {
                return Err(raise_eval_error!("Atomics.wait requires 3 or 4 arguments"));
            }
            let idx_val = evaluate_expr(env, &args[1])?;
            let idx = match idx_val {
                Value::Number(n) => n as usize,
                _ => return Err(raise_eval_error!("Atomics index must be a number")),
            };
            let expected_val = evaluate_expr(env, &args[2])?;
            let expected = match expected_val {
                Value::Number(n) => n as i64,
                Value::BigInt(b) => b.to_i64().unwrap_or(0),
                _ => return Err(raise_eval_error!("Atomics expected must be a number or BigInt")),
            };

            // Check current value
            let current = ta_obj.borrow().get(idx)?;
            if current != expected {
                return Ok(Value::String(utf8_to_utf16("not-equal")));
            }

            // Determine timeout (milliseconds)
            let timeout_ms_opt = if args.len() == 4 {
                let tval = evaluate_expr(env, &args[3])?;
                match tval {
                    Value::Number(n) => Some(n as i64),
                    _ => None,
                }
            } else {
                None
            };

            // Compute key for waiters: (arc_ptr, byte_index)
            let buffer_rc = ta_obj.borrow().buffer.clone();
            let arc_ptr = Arc::as_ptr(&buffer_rc.borrow().data) as usize;
            let byte_index = ta_obj.borrow().byte_offset + idx * ta_obj.borrow().element_size();

            // Create waiter and register
            let waiter = Arc::new((Mutex::new(false), Condvar::new()));
            {
                let mut map = WAITERS.lock().unwrap();
                let entry = map.entry((arc_ptr, byte_index)).or_default();
                entry.push(waiter.clone());
            }

            // Block on the condvar
            let (m, cv) = &*waiter;
            let mut signaled = m.lock().unwrap();
            if let Some(ms) = timeout_ms_opt {
                let dur = if ms <= 0 {
                    Duration::from_millis(0)
                } else {
                    Duration::from_millis(ms as u64)
                };
                let (guard, res) = cv.wait_timeout(signaled, dur).unwrap();
                signaled = guard;
                if *signaled {
                    Ok(Value::String(utf8_to_utf16("ok")))
                } else if res.timed_out() {
                    // remove self from WAITERS
                    let mut map = WAITERS.lock().unwrap();
                    if let Some(v) = map.get_mut(&(arc_ptr, byte_index)) {
                        v.retain(|h| !Arc::ptr_eq(h, &waiter));
                        if v.is_empty() {
                            map.remove(&(arc_ptr, byte_index));
                        }
                    }
                    Ok(Value::String(utf8_to_utf16("timed-out")))
                } else {
                    // Spurious wake, treat as timed-out
                    let mut map = WAITERS.lock().unwrap();
                    if let Some(v) = map.get_mut(&(arc_ptr, byte_index)) {
                        v.retain(|h| !Arc::ptr_eq(h, &waiter));
                        if v.is_empty() {
                            map.remove(&(arc_ptr, byte_index));
                        }
                    }
                    Ok(Value::String(utf8_to_utf16("timed-out")))
                }
            } else {
                // Wait indefinitely
                while !*signaled {
                    signaled = cv.wait(signaled).unwrap();
                }
                Ok(Value::String(utf8_to_utf16("ok")))
            }
        }
        "notify" => {
            // Atomics.notify(typedArray, index[, count])
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("Atomics.notify requires 2 or 3 arguments"));
            }
            let idx_val = evaluate_expr(env, &args[1])?;
            let idx = match idx_val {
                Value::Number(n) => n as usize,
                _ => return Err(raise_eval_error!("Atomics index must be a number")),
            };
            let count = if args.len() == 3 {
                let cval = evaluate_expr(env, &args[2])?;
                match cval {
                    Value::Number(n) => n as usize,
                    _ => return Err(raise_eval_error!("Atomics count must be a number")),
                }
            } else {
                usize::MAX
            };

            let buffer_rc = ta_obj.borrow().buffer.clone();
            let arc_ptr = Arc::as_ptr(&buffer_rc.borrow().data) as usize;
            let byte_index = ta_obj.borrow().byte_offset + idx * ta_obj.borrow().element_size();

            let mut awakened = 0usize;
            let mut map = WAITERS.lock().unwrap();
            if let Some(vec) = map.get_mut(&(arc_ptr, byte_index)) {
                let to_awake = std::cmp::min(count, vec.len());
                for _ in 0..to_awake {
                    // wake oldest
                    if vec.is_empty() {
                        break;
                    }
                    let handle = vec.remove(0);
                    let (m, cv) = &*handle;
                    let mut g = m.lock().unwrap();
                    *g = true;
                    cv.notify_one();
                    awakened += 1;
                }
                if vec.is_empty() {
                    map.remove(&(arc_ptr, byte_index));
                }
            }
            Ok(Value::Number(awakened as f64))
        }
        "isLockFree" => {
            // For simplicity, always return false (no native lock-free guarantees)
            if args.len() != 1 {
                return Err(raise_eval_error!("Atomics.isLockFree requires 1 argument"));
            }
            Ok(Value::Boolean(false))
        }
        _ => Err(raise_eval_error!(format!("Atomics method '{method}' not implemented"))),
    }
}

/// Create a SharedArrayBuffer constructor object
pub fn make_sharedarraybuffer_constructor() -> Result<JSObjectDataPtr, JSError> {
    let obj = new_js_object_data();

    // Set prototype and name
    obj_set_key_value(&obj, &"prototype".into(), Value::Object(make_arraybuffer_prototype()?))?;
    obj_set_key_value(&obj, &"name".into(), Value::String(utf8_to_utf16("SharedArrayBuffer")))?;

    // Mark as ArrayBuffer constructor and indicate it's the shared variant
    obj_set_key_value(&obj, &"__arraybuffer".into(), Value::Boolean(true))?;
    obj_set_key_value(&obj, &"__sharedarraybuffer".into(), Value::Boolean(true))?;

    Ok(obj)
}

/// Create the ArrayBuffer prototype
pub fn make_arraybuffer_prototype() -> Result<JSObjectDataPtr, JSError> {
    let proto = new_js_object_data();

    // Add methods to prototype
    obj_set_key_value(&proto, &"constructor".into(), Value::Function("ArrayBuffer".to_string()))?;
    obj_set_key_value(
        &proto,
        &"byteLength".into(),
        Value::Function("ArrayBuffer.prototype.byteLength".to_string()),
    )?;
    obj_set_key_value(&proto, &"slice".into(), Value::Function("ArrayBuffer.prototype.slice".to_string()))?;

    Ok(proto)
}

/// Create a DataView constructor object
pub fn make_dataview_constructor() -> Result<JSObjectDataPtr, JSError> {
    let obj = new_js_object_data();

    obj_set_key_value(&obj, &"prototype".into(), Value::Object(make_dataview_prototype()?))?;
    obj_set_key_value(&obj, &"name".into(), Value::String(utf8_to_utf16("DataView")))?;

    // Mark as DataView constructor
    obj_set_key_value(&obj, &"__dataview".into(), Value::Boolean(true))?;

    Ok(obj)
}

/// Create the DataView prototype
pub fn make_dataview_prototype() -> Result<JSObjectDataPtr, JSError> {
    let proto = new_js_object_data();

    obj_set_key_value(&proto, &"constructor".into(), Value::Function("DataView".to_string()))?;
    obj_set_key_value(&proto, &"buffer".into(), Value::Function("DataView.prototype.buffer".to_string()))?;
    obj_set_key_value(
        &proto,
        &"byteLength".into(),
        Value::Function("DataView.prototype.byteLength".to_string()),
    )?;
    obj_set_key_value(
        &proto,
        &"byteOffset".into(),
        Value::Function("DataView.prototype.byteOffset".to_string()),
    )?;

    // DataView methods for different data types
    obj_set_key_value(&proto, &"getInt8".into(), Value::Function("DataView.prototype.getInt8".to_string()))?;
    obj_set_key_value(
        &proto,
        &"getUint8".into(),
        Value::Function("DataView.prototype.getUint8".to_string()),
    )?;
    obj_set_key_value(
        &proto,
        &"getInt16".into(),
        Value::Function("DataView.prototype.getInt16".to_string()),
    )?;
    obj_set_key_value(
        &proto,
        &"getUint16".into(),
        Value::Function("DataView.prototype.getUint16".to_string()),
    )?;
    obj_set_key_value(
        &proto,
        &"getInt32".into(),
        Value::Function("DataView.prototype.getInt32".to_string()),
    )?;
    obj_set_key_value(
        &proto,
        &"getUint32".into(),
        Value::Function("DataView.prototype.getUint32".to_string()),
    )?;
    obj_set_key_value(
        &proto,
        &"getFloat32".into(),
        Value::Function("DataView.prototype.getFloat32".to_string()),
    )?;
    obj_set_key_value(
        &proto,
        &"getFloat64".into(),
        Value::Function("DataView.prototype.getFloat64".to_string()),
    )?;

    obj_set_key_value(&proto, &"setInt8".into(), Value::Function("DataView.prototype.setInt8".to_string()))?;
    obj_set_key_value(
        &proto,
        &"setUint8".into(),
        Value::Function("DataView.prototype.setUint8".to_string()),
    )?;
    obj_set_key_value(
        &proto,
        &"setInt16".into(),
        Value::Function("DataView.prototype.setInt16".to_string()),
    )?;
    obj_set_key_value(
        &proto,
        &"setUint16".into(),
        Value::Function("DataView.prototype.setUint16".to_string()),
    )?;
    obj_set_key_value(
        &proto,
        &"setInt32".into(),
        Value::Function("DataView.prototype.setInt32".to_string()),
    )?;
    obj_set_key_value(
        &proto,
        &"setUint32".into(),
        Value::Function("DataView.prototype.setUint32".to_string()),
    )?;
    obj_set_key_value(
        &proto,
        &"setFloat32".into(),
        Value::Function("DataView.prototype.setFloat32".to_string()),
    )?;
    obj_set_key_value(
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

    let obj = new_js_object_data();

    obj_set_key_value(&obj, &"prototype".into(), Value::Object(make_typedarray_prototype(kind)?))?;
    obj_set_key_value(&obj, &"name".into(), Value::String(utf8_to_utf16(name)))?;

    obj_set_key_value(&obj, &"__kind".into(), Value::Number(kind_value as f64))?;

    Ok(obj)
}

fn make_typedarray_prototype(kind: TypedArrayKind) -> Result<JSObjectDataPtr, JSError> {
    let proto = new_js_object_data();

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

    obj_set_key_value(&proto, &"__kind".into(), Value::Number(kind_value as f64))?;
    obj_set_key_value(&proto, &"constructor".into(), Value::Function("TypedArray".to_string()))?;

    // TypedArray properties and methods
    obj_set_key_value(&proto, &"buffer".into(), Value::Function("TypedArray.prototype.buffer".to_string()))?;
    obj_set_key_value(
        &proto,
        &"byteLength".into(),
        Value::Function("TypedArray.prototype.byteLength".to_string()),
    )?;
    obj_set_key_value(
        &proto,
        &"byteOffset".into(),
        Value::Function("TypedArray.prototype.byteOffset".to_string()),
    )?;
    obj_set_key_value(&proto, &"length".into(), Value::Function("TypedArray.prototype.length".to_string()))?;

    // Array methods that TypedArrays inherit
    obj_set_key_value(&proto, &"set".into(), Value::Function("TypedArray.prototype.set".to_string()))?;
    obj_set_key_value(
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
        data: Arc::new(Mutex::new(vec![0; length])),
        detached: false,
        shared: false,
    }));

    // Create the ArrayBuffer object
    let obj = new_js_object_data();
    obj_set_key_value(&obj, &"__arraybuffer".into(), Value::ArrayBuffer(buffer))?;

    // Set prototype
    let proto = make_arraybuffer_prototype()?;
    obj.borrow_mut().prototype = Some(Rc::downgrade(&proto));

    Ok(Value::Object(obj))
}

/// Handle SharedArrayBuffer constructor calls (creates a shared buffer)
pub fn handle_sharedarraybuffer_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // SharedArrayBuffer(length)
    if args.is_empty() {
        return Err(raise_eval_error!("SharedArrayBuffer constructor requires a length argument"));
    }

    let length_val = evaluate_expr(env, &args[0])?;
    let length = match length_val {
        Value::Number(n) if n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0 => n as usize,
        _ => return Err(raise_eval_error!("SharedArrayBuffer length must be a non-negative integer")),
    };

    // Create SharedArrayBuffer instance (mark shared: true)
    let buffer = Rc::new(RefCell::new(JSArrayBuffer {
        data: Arc::new(Mutex::new(vec![0; length])),
        detached: false,
        shared: true,
    }));

    // Create the SharedArrayBuffer object wrapper
    let obj = new_js_object_data();
    obj_set_key_value(&obj, &"__arraybuffer".into(), Value::ArrayBuffer(buffer))?;

    // Set prototype to ArrayBuffer.prototype
    let proto = make_arraybuffer_prototype()?;
    obj.borrow_mut().prototype = Some(Rc::downgrade(&proto));

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
            if let Some(ab_val) = obj_get_key_value(&obj, &"__arraybuffer".into())? {
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
        buffer.borrow().data.lock().unwrap().len() - byte_offset
    };

    // Validate bounds
    if byte_offset + byte_length > buffer.borrow().data.lock().unwrap().len() {
        return Err(raise_eval_error!("DataView bounds exceed buffer size"));
    }

    // Create DataView instance
    let data_view = Rc::new(RefCell::new(JSDataView {
        buffer,
        byte_offset,
        byte_length,
    }));

    // Create the DataView object
    let obj = new_js_object_data();
    obj_set_key_value(&obj, &"__dataview".into(), Value::DataView(data_view))?;

    // Set prototype
    let proto = make_dataview_prototype()?;
    obj.borrow_mut().prototype = Some(Rc::downgrade(&proto));

    Ok(Value::Object(obj))
}

/// Handle TypedArray constructor calls
pub fn handle_typedarray_constructor(constructor_obj: &JSObjectDataPtr, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Get the kind from the constructor
    let kind_val = obj_get_key_value(constructor_obj, &"__kind".into())?;
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
            data: Arc::new(Mutex::new(vec![])),
            detached: false,
            shared: false,
        }));
        (buffer, 0, 0)
    } else if args.len() == 1 {
        let arg_val = evaluate_expr(env, &args[0])?;
        match arg_val {
            Value::Number(n) if n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0 => {
                // new TypedArray(length)
                let length = n as usize;
                let buffer = Rc::new(RefCell::new(JSArrayBuffer {
                    data: Arc::new(Mutex::new(vec![0; length * element_size])),
                    detached: false,
                    shared: false,
                }));
                (buffer, 0, length)
            }
            Value::Object(obj) => {
                // Check if it's another TypedArray or ArrayBuffer
                if let Some(ta_val) = obj_get_key_value(&obj, &"__typedarray".into())? {
                    if let Value::TypedArray(ta) = &*ta_val.borrow() {
                        // new TypedArray(typedArray) - copy constructor
                        let src_length = ta.borrow().length;
                        let buffer = Rc::new(RefCell::new(JSArrayBuffer {
                            data: Arc::new(Mutex::new(vec![0; src_length * element_size])),
                            detached: false,
                            shared: false,
                        }));
                        // TODO: Copy data from source TypedArray
                        (buffer, 0, src_length)
                    } else {
                        return Err(raise_eval_error!("Invalid TypedArray constructor argument"));
                    }
                } else if let Some(ab_val) = obj_get_key_value(&obj, &"__arraybuffer".into())? {
                    if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                        // new TypedArray(buffer)
                        (ab.clone(), 0, ab.borrow().data.lock().unwrap().len() / element_size)
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
            if let Some(ab_val) = obj_get_key_value(&obj, &"__arraybuffer".into())? {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    if let Value::Number(offset_num) = offset_val {
                        let offset = offset_num as usize;
                        if !offset.is_multiple_of(element_size) {
                            return Err(raise_eval_error!("TypedArray byteOffset must be multiple of element size"));
                        }
                        let remaining_bytes = ab.borrow().data.lock().unwrap().len() - offset;
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
            if let Some(ab_val) = obj_get_key_value(&obj, &"__arraybuffer".into())? {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    if let (Value::Number(offset_num), Value::Number(length_num)) = (offset_val, length_val) {
                        let offset = offset_num as usize;
                        let length = length_num as usize;
                        if !offset.is_multiple_of(element_size) {
                            return Err(raise_eval_error!("TypedArray byteOffset must be multiple of element size"));
                        }
                        if length * element_size + offset > ab.borrow().data.lock().unwrap().len() {
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
    let obj = new_js_object_data();

    // Set prototype first
    let proto = make_typedarray_prototype(kind.clone())?;
    obj.borrow_mut().prototype = Some(Rc::downgrade(&proto));

    // Create TypedArray instance
    let typed_array = Rc::new(RefCell::new(JSTypedArray {
        kind,
        buffer,
        byte_offset,
        length,
    }));

    obj_set_key_value(&obj, &"__typedarray".into(), Value::TypedArray(typed_array))?;

    Ok(Value::Object(obj))
}

/// Handle DataView instance method calls
pub fn handle_dataview_method(object: &JSObjectDataPtr, method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Get the DataView from the object
    let dv_val = obj_get_key_value(object, &"__dataview".into())?;
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

#[cfg(test)]
mod atomics_thread_tests {
    use super::*;
    use crate::core::{evaluate_statements, initialize_global_constructors, parse_statements};
    use crate::unicode::utf16_to_utf8;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn atomics_wait_notify_multithreaded() {
        // Create a shared ArrayBuffer (shared = true)
        let buffer = Rc::new(RefCell::new(JSArrayBuffer {
            data: Arc::new(Mutex::new(vec![0u8; 16])),
            detached: false,
            shared: true,
        }));

        // Create a typed array view (Int32Array) referencing the same buffer
        let _ = Rc::new(RefCell::new(JSTypedArray {
            kind: TypedArrayKind::Int32,
            buffer: buffer.clone(),
            byte_offset: 0,
            length: 4,
        }));

        // Extract the inner Arc for sharing between threads. Each thread will
        // create its own `JSArrayBuffer` wrapper using the same `Arc<Mutex<...>>`.
        let shared_arc = buffer.borrow().data.clone();

        // Ensure initial int32 value at index 0 is 0
        {
            let data_arc = buffer.borrow().data.clone();
            let mut d = data_arc.lock().unwrap();
            let b = 0i32.to_le_bytes();
            d[0] = b[0];
            d[1] = b[1];
            d[2] = b[2];
            d[3] = b[3];
        }

        // Spawn a thread that will call Atomics.wait(ia, 0, 0) and block until notified.
        // The thread creates its own JS environment but uses the same underlying
        // Arc-backed byte buffer so wait/notify will match on the same key.
        let shared_for_wait = shared_arc.clone();
        let waiter = std::thread::spawn(move || {
            // Build a local JSArrayBuffer wrapper around the shared Arc
            let local_buffer = Rc::new(RefCell::new(JSArrayBuffer {
                data: shared_for_wait.clone(),
                detached: false,
                shared: true,
            }));
            let local_ta = Rc::new(RefCell::new(JSTypedArray {
                kind: TypedArrayKind::Int32,
                buffer: local_buffer.clone(),
                byte_offset: 0,
                length: 4,
            }));
            let obj_local = new_js_object_data();
            obj_set_key_value(&obj_local, &"__typedarray".into(), Value::TypedArray(local_ta)).unwrap();

            let env_local = new_js_object_data();
            env_local.borrow_mut().is_function_scope = true;
            initialize_global_constructors(&env_local).unwrap();
            obj_set_key_value(&env_local, &"ia".into(), Value::Object(obj_local)).unwrap();

            let mut tokens = crate::tokenize("Atomics.wait(ia, 0, 0)").unwrap();
            let stmts = parse_statements(&mut tokens).unwrap();
            let v = evaluate_statements(&env_local, &stmts).unwrap();
            // Expect a string result ("ok" when woken)
            if let Value::String(s) = v {
                utf16_to_utf8(&s)
            } else {
                "".to_string()
            }
        });

        // Give the waiter a moment to block in Atomics.wait
        std::thread::sleep(Duration::from_millis(100));

        // In the notifier context, store a new value and notify the waiter
        // Build a separate notifier environment that shares the same Arc
        let local_buffer2 = Rc::new(RefCell::new(JSArrayBuffer {
            data: shared_arc.clone(),
            detached: false,
            shared: true,
        }));
        let local_ta2 = Rc::new(RefCell::new(JSTypedArray {
            kind: TypedArrayKind::Int32,
            buffer: local_buffer2.clone(),
            byte_offset: 0,
            length: 4,
        }));
        let obj_notify = new_js_object_data();
        obj_set_key_value(&obj_notify, &"__typedarray".into(), Value::TypedArray(local_ta2)).unwrap();
        let env_notify = new_js_object_data();
        env_notify.borrow_mut().is_function_scope = true;
        initialize_global_constructors(&env_notify).unwrap();
        obj_set_key_value(&env_notify, &"ia".into(), Value::Object(obj_notify)).unwrap();

        let mut tokens2 = crate::tokenize("Atomics.store(ia, 0, 1); Atomics.notify(ia, 0, 1)").unwrap();
        let stmts2 = parse_statements(&mut tokens2).unwrap();
        let _ = evaluate_statements(&env_notify, &stmts2).unwrap();

        // Join waiter and assert it observed an "ok" wakeup
        let res = waiter.join().unwrap();
        assert_eq!(res, "ok");
    }
}
