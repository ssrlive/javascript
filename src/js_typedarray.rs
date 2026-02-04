use crate::core::{Gc, MutationContext, get_property_with_accessors, js_error_to_value, new_gc_cell_ptr};
use crate::core::{JSObjectDataPtr, PropertyKey, Value, new_js_object_data, object_get_key_value, object_set_key_value};
use crate::js_array::is_array;
use crate::unicode::utf8_to_utf16;
use crate::{JSArrayBuffer, JSDataView, JSTypedArray, TypedArrayKind};
use crate::{JSError, core::EvalError};
use num_traits::ToPrimitive;
use std::collections::HashMap;
use std::sync::Condvar;
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Global waiters registry keyed by (buffer_arc_ptr, byte_index). Each waiter
// is an Arc containing a (Mutex<bool>, Condvar) pair the waiting thread blocks on.
#[allow(clippy::type_complexity)]
static WAITERS: LazyLock<Mutex<HashMap<(usize, usize), Vec<Arc<(Mutex<bool>, Condvar)>>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Create an ArrayBuffer constructor object
pub fn make_arraybuffer_constructor<'gc>(mc: &MutationContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let obj = new_js_object_data(mc);

    // Set the constructor function
    object_set_key_value(mc, &obj, "prototype", Value::Object(make_arraybuffer_prototype(mc)?))?;
    object_set_key_value(mc, &obj, "name", Value::String(utf8_to_utf16("ArrayBuffer")))?;

    // Mark as ArrayBuffer constructor
    object_set_key_value(mc, &obj, "__arraybuffer", Value::Boolean(true))?;
    object_set_key_value(mc, &obj, "__native_ctor", Value::String(utf8_to_utf16("ArrayBuffer")))?;

    Ok(obj)
}

/// Create the Atomics object with basic atomic methods
pub fn make_atomics_object<'gc>(mc: &MutationContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let obj = new_js_object_data(mc);

    object_set_key_value(mc, &obj, "load", Value::Function("Atomics.load".to_string()))?;
    object_set_key_value(mc, &obj, "store", Value::Function("Atomics.store".to_string()))?;
    object_set_key_value(mc, &obj, "compareExchange", Value::Function("Atomics.compareExchange".to_string()))?;
    object_set_key_value(mc, &obj, "exchange", Value::Function("Atomics.exchange".to_string()))?;
    object_set_key_value(mc, &obj, "add", Value::Function("Atomics.add".to_string()))?;
    object_set_key_value(mc, &obj, "sub", Value::Function("Atomics.sub".to_string()))?;
    object_set_key_value(mc, &obj, "and", Value::Function("Atomics.and".to_string()))?;
    object_set_key_value(mc, &obj, "or", Value::Function("Atomics.or".to_string()))?;
    object_set_key_value(mc, &obj, "xor", Value::Function("Atomics.xor".to_string()))?;
    object_set_key_value(mc, &obj, "wait", Value::Function("Atomics.wait".to_string()))?;
    object_set_key_value(mc, &obj, "notify", Value::Function("Atomics.notify".to_string()))?;
    object_set_key_value(mc, &obj, "isLockFree", Value::Function("Atomics.isLockFree".to_string()))?;

    Ok(obj)
}

pub(crate) fn is_typedarray(obj: &JSObjectDataPtr) -> bool {
    object_get_key_value(obj, "__typedarray").is_some()
}

pub(crate) fn get_array_like_element<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    arr_obj: &JSObjectDataPtr<'gc>,
    index: usize,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if is_array(mc, arr_obj) {
        Ok(if let Some(cell) = object_get_key_value(arr_obj, index) {
            cell.borrow().clone()
        } else {
            Value::Undefined
        })
    } else if is_typedarray(arr_obj) {
        get_property_with_accessors(mc, env, arr_obj, index)
    } else {
        Ok(Value::Undefined)
    }
}

pub(crate) fn ensure_typedarray_in_bounds<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    stmt_line: Option<usize>,
    stmt_column: Option<usize>,
    obj: &JSObjectDataPtr<'gc>,
) -> Result<(), EvalError<'gc>> {
    // If the object is a fixed-length TypedArray whose underlying ArrayBuffer
    // has been resized so the view falls outside the buffer, throw TypeError.
    if let Some(ta_cell) = obj.borrow().properties.get(&PropertyKey::String("__typedarray".to_string()))
        && let Value::TypedArray(ta) = &*ta_cell.borrow()
    {
        if !ta.length_tracking {
            let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
            let needed = ta.byte_offset + ta.element_size() * ta.length;
            log::trace!(
                "ensure_typedarray_in_bounds: needed={} buf_len={} byte_offset={} length={}",
                needed,
                buf_len,
                ta.byte_offset,
                ta.length
            );
            if buf_len < needed {
                log::trace!("ensure_typedarray_in_bounds: out of bounds detected");
                let js_err = raise_type_error!("TypedArray is out of bounds");
                let val = js_error_to_value(mc, env, &js_err);
                log::trace!(
                    "ensure_typedarray_in_bounds: throwing TypeError at js_line={:?} js_col={:?}",
                    stmt_line,
                    stmt_column
                );
                return Err(EvalError::Throw(val, stmt_line, stmt_column));
            }
        } else {
            // For length-tracking views: if the byte_offset itself is already
            // beyond the (shrunk) buffer length then the view is entirely out
            // of range and operations should throw a TypeError.
            let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
            log::trace!(
                "ensure_typedarray_in_bounds: length-tracking check: byte_offset={} buf_len={}",
                ta.byte_offset,
                buf_len
            );
            if ta.byte_offset > buf_len {
                log::trace!("ensure_typedarray_in_bounds: length-tracking view out of bounds detected");
                let js_err = raise_type_error!("TypedArray is out of bounds");
                let val = js_error_to_value(mc, env, &js_err);
                log::trace!(
                    "ensure_typedarray_in_bounds: throwing TypeError at js_line={:?} js_col={:?}",
                    stmt_line,
                    stmt_column
                );
                return Err(EvalError::Throw(val, stmt_line, stmt_column));
            }
        }
    }
    Ok(())
}

/// Handle Atomics.* calls (minimal mutex-backed implementations)
pub fn handle_atomics_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Helper to extract TypedArray from first argument
    if args.is_empty() {
        return Err(raise_eval_error!("Atomics method requires arguments").into());
    }
    // Special-case Atomics.isLockFree which accepts a size (in bytes)
    // and does not require a TypedArray as the first argument.
    if method == "isLockFree" {
        if args.len() != 1 {
            return Err(raise_eval_error!("Atomics.isLockFree requires 1 argument").into());
        }
        let size_val = args[0].clone();
        let size = match size_val {
            Value::Number(n) => n as usize,
            _ => return Err(raise_eval_error!("Atomics.isLockFree argument must be a number").into()),
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
    let ta_val = args[0].clone();
    let ta_obj = if let Value::Object(object) = ta_val {
        if let Some(ta_rc) = object_get_key_value(&object, "__typedarray") {
            if let Value::TypedArray(ta) = &*ta_rc.borrow() {
                *ta
            } else {
                return Err(raise_eval_error!("First argument to Atomics must be a TypedArray").into());
            }
        } else {
            return Err(raise_eval_error!("First argument to Atomics must be a TypedArray").into());
        }
    } else {
        return Err(raise_eval_error!("First argument to Atomics must be a TypedArray").into());
    };

    match method {
        "load" => {
            if args.len() != 2 {
                return Err(raise_eval_error!("Atomics.load requires 2 arguments").into());
            }
            let idx_val = args[1].clone();
            let idx = match idx_val {
                Value::Number(n) => n as usize,
                _ => return Err(raise_eval_error!("Atomics index must be a number").into()),
            };
            let v = ta_obj.get(idx)?;
            Ok(Value::Number(v as f64))
        }
        "store" => {
            if args.len() != 3 {
                return Err(raise_eval_error!("Atomics.store requires 3 arguments").into());
            }
            let idx_val = args[1].clone();
            let val_val = args[2].clone();
            let idx = match idx_val {
                Value::Number(n) => n as usize,
                _ => return Err(raise_eval_error!("Atomics index must be a number").into()),
            };
            let v = match val_val {
                Value::Number(n) => n as i64,
                Value::BigInt(b) => b.to_i64().unwrap_or(0),
                _ => return Err(raise_eval_error!("Atomics value must be a number or BigInt").into()),
            };
            let old = ta_obj.get(idx)?;
            ta_obj.set(mc, idx, v as f64)?;
            Ok(Value::Number(old as f64))
        }
        "compareExchange" => {
            if args.len() != 4 {
                return Err(raise_eval_error!("Atomics.compareExchange requires 4 arguments").into());
            }
            let idx_val = args[1].clone();
            let expected_val = args[2].clone();
            let replacement_val = args[3].clone();
            let idx = match idx_val {
                Value::Number(n) => n as usize,
                _ => return Err(raise_eval_error!("Atomics index must be a number").into()),
            };
            let expected = match expected_val {
                Value::Number(n) => n as i64,
                Value::BigInt(b) => b.to_i64().unwrap_or(0),
                _ => return Err(raise_eval_error!("Atomics expected must be a number or BigInt").into()),
            };
            let replacement = match replacement_val {
                Value::Number(n) => n as i64,
                Value::BigInt(b) => b.to_i64().unwrap_or(0),
                _ => return Err(raise_eval_error!("Atomics replacement must be a number or BigInt").into()),
            };
            let old = ta_obj.get(idx)?;
            if (old as i64) == (expected as i64) {
                ta_obj.set(mc, idx, replacement as f64)?;
            }
            Ok(Value::Number(old as f64))
        }
        "add" | "sub" | "and" | "or" | "xor" | "exchange" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!(format!("Atomics.{} invalid args", method)).into());
            }
            let idx_val = args[1].clone();
            let idx = match idx_val {
                Value::Number(n) => n as usize,
                _ => return Err(raise_eval_error!("Atomics index must be a number").into()),
            };
            let operand = if args.len() == 3 {
                let v = args[2].clone();
                match v {
                    Value::Number(n) => n as i64,
                    Value::BigInt(b) => b.to_i64().unwrap_or(0),
                    _ => return Err(raise_eval_error!("Atomics operand must be a number or BigInt").into()),
                }
            } else {
                0
            };
            let old = ta_obj.get(idx)?;
            let new = match method {
                "add" => (old as i64).wrapping_add(operand as i64) as f64,
                "sub" => (old as i64).wrapping_sub(operand as i64) as f64,
                "and" => ((old as i64) & (operand as i64)) as f64,
                "or" => ((old as i64) | (operand as i64)) as f64,
                "xor" => ((old as i64) ^ (operand as i64)) as f64,
                "exchange" => operand as f64,
                _ => old,
            };
            ta_obj.set(mc, idx, new)?;
            Ok(Value::Number(old as f64))
        }
        "wait" => {
            // Atomics.wait(typedArray, index, value[, timeout])
            if args.len() < 3 || args.len() > 4 {
                return Err(raise_eval_error!("Atomics.wait requires 3 or 4 arguments").into());
            }
            let idx_val = args[1].clone();
            let idx = match idx_val {
                Value::Number(n) => n as usize,
                _ => return Err(raise_eval_error!("Atomics index must be a number").into()),
            };
            let expected_val = args[2].clone();
            let expected = match expected_val {
                Value::Number(n) => n as i64,
                Value::BigInt(b) => b.to_i64().unwrap_or(0),
                _ => return Err(raise_eval_error!("Atomics expected must be a number or BigInt").into()),
            };

            // Check current value
            let current = ta_obj.get(idx)?;
            if (current as i64) != (expected as i64) {
                return Ok(Value::String(utf8_to_utf16("not-equal")));
            }

            // Determine timeout (milliseconds)
            let timeout_ms_opt = if args.len() == 4 {
                let tval = args[3].clone();
                match tval {
                    Value::Number(n) => Some(n as i64),
                    _ => None,
                }
            } else {
                None
            };

            // Compute key for waiters: (arc_ptr, byte_index)
            let buffer_rc = ta_obj.buffer;
            let arc_ptr = Arc::as_ptr(&buffer_rc.borrow().data) as usize;
            let byte_index = ta_obj.byte_offset + idx * ta_obj.element_size();

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
                return Err(raise_eval_error!("Atomics.notify requires 2 or 3 arguments").into());
            }
            let idx_val = args[1].clone();
            let idx = match idx_val {
                Value::Number(n) => n as usize,
                _ => return Err(raise_eval_error!("Atomics index must be a number").into()),
            };
            let count = if args.len() == 3 {
                let cval = args[2].clone();
                match cval {
                    Value::Number(n) => n as usize,
                    _ => return Err(raise_eval_error!("Atomics count must be a number").into()),
                }
            } else {
                usize::MAX
            };

            let buffer_rc = ta_obj.buffer;
            let arc_ptr = Arc::as_ptr(&buffer_rc.borrow().data) as usize;
            let byte_index = ta_obj.byte_offset + idx * ta_obj.element_size();

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
        _ => Err(raise_eval_error!(format!("Atomics method '{method}' not implemented")).into()),
    }
}

/// Create a SharedArrayBuffer constructor object
pub fn make_sharedarraybuffer_constructor<'gc>(mc: &MutationContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let obj = new_js_object_data(mc);

    // Set prototype and name
    object_set_key_value(mc, &obj, "prototype", Value::Object(make_sharedarraybuffer_prototype(mc)?))?;
    object_set_key_value(mc, &obj, "name", Value::String(utf8_to_utf16("SharedArrayBuffer")))?;

    // Mark as ArrayBuffer constructor and indicate it's the shared variant
    object_set_key_value(mc, &obj, "__arraybuffer", Value::Boolean(true))?;
    object_set_key_value(mc, &obj, "__sharedarraybuffer", Value::Boolean(true))?;
    object_set_key_value(mc, &obj, "__native_ctor", Value::String(utf8_to_utf16("SharedArrayBuffer")))?;

    Ok(obj)
}

/// Create the ArrayBuffer prototype
pub fn make_arraybuffer_prototype<'gc>(mc: &MutationContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let proto = new_js_object_data(mc);

    // Add methods to prototype
    object_set_key_value(mc, &proto, "constructor", Value::Function("ArrayBuffer".to_string()))?;

    // byteLength is an accessor property
    let byte_len_getter = Value::Function("ArrayBuffer.prototype.byteLength".to_string());
    let byte_len_prop = Value::Property {
        value: None,
        getter: Some(Box::new(byte_len_getter)),
        setter: None,
    };
    object_set_key_value(mc, &proto, "byteLength", byte_len_prop)?;

    object_set_key_value(mc, &proto, "slice", Value::Function("ArrayBuffer.prototype.slice".to_string()))?;
    object_set_key_value(mc, &proto, "resize", Value::Function("ArrayBuffer.prototype.resize".to_string()))?;

    Ok(proto)
}

/// Create the SharedArrayBuffer prototype
pub fn make_sharedarraybuffer_prototype<'gc>(mc: &MutationContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let proto = new_js_object_data(mc);

    // Add methods to prototype
    object_set_key_value(mc, &proto, "constructor", Value::Function("SharedArrayBuffer".to_string()))?;

    // byteLength is an accessor property
    object_set_key_value(
        mc,
        &proto,
        "byteLength",
        Value::Property {
            value: None,
            getter: Some(Box::new(Value::Function("SharedArrayBuffer.prototype.byteLength".to_string()))),
            setter: None,
        },
    )?;

    object_set_key_value(
        mc,
        &proto,
        "slice",
        Value::Function("SharedArrayBuffer.prototype.slice".to_string()),
    )?;

    Ok(proto)
}

/// Create a DataView constructor object
pub fn make_dataview_constructor<'gc>(mc: &MutationContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let obj = new_js_object_data(mc);

    object_set_key_value(mc, &obj, "prototype", Value::Object(make_dataview_prototype(mc)?))?;
    object_set_key_value(mc, &obj, "name", Value::String(utf8_to_utf16("DataView")))?;

    // Mark as DataView constructor
    object_set_key_value(mc, &obj, "__dataview", Value::Boolean(true))?;
    object_set_key_value(mc, &obj, "__native_ctor", Value::String(utf8_to_utf16("DataView")))?;

    Ok(obj)
}

/// Create the DataView prototype
pub fn make_dataview_prototype<'gc>(mc: &MutationContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let proto = new_js_object_data(mc);

    object_set_key_value(mc, &proto, "constructor", Value::Function("DataView".to_string()))?;
    object_set_key_value(
        mc,
        &proto,
        "buffer",
        Value::Property {
            value: None,
            getter: Some(Box::new(Value::Function("DataView.prototype.buffer".to_string()))),
            setter: None,
        },
    )?;
    object_set_key_value(
        mc,
        &proto,
        "byteLength",
        Value::Property {
            value: None,
            getter: Some(Box::new(Value::Function("DataView.prototype.byteLength".to_string()))),
            setter: None,
        },
    )?;
    object_set_key_value(
        mc,
        &proto,
        "byteOffset",
        Value::Property {
            value: None,
            getter: Some(Box::new(Value::Function("DataView.prototype.byteOffset".to_string()))),
            setter: None,
        },
    )?;

    // DataView methods for different data types
    object_set_key_value(mc, &proto, "getInt8", Value::Function("DataView.prototype.getInt8".to_string()))?;
    object_set_key_value(mc, &proto, "getUint8", Value::Function("DataView.prototype.getUint8".to_string()))?;
    object_set_key_value(mc, &proto, "getInt16", Value::Function("DataView.prototype.getInt16".to_string()))?;
    object_set_key_value(mc, &proto, "getUint16", Value::Function("DataView.prototype.getUint16".to_string()))?;
    object_set_key_value(mc, &proto, "getInt32", Value::Function("DataView.prototype.getInt32".to_string()))?;
    object_set_key_value(mc, &proto, "getUint32", Value::Function("DataView.prototype.getUint32".to_string()))?;
    object_set_key_value(
        mc,
        &proto,
        "getFloat32",
        Value::Function("DataView.prototype.getFloat32".to_string()),
    )?;
    object_set_key_value(
        mc,
        &proto,
        "getFloat64",
        Value::Function("DataView.prototype.getFloat64".to_string()),
    )?;

    object_set_key_value(mc, &proto, "setInt8", Value::Function("DataView.prototype.setInt8".to_string()))?;
    object_set_key_value(mc, &proto, "setUint8", Value::Function("DataView.prototype.setUint8".to_string()))?;
    object_set_key_value(mc, &proto, "setInt16", Value::Function("DataView.prototype.setInt16".to_string()))?;
    object_set_key_value(mc, &proto, "setUint16", Value::Function("DataView.prototype.setUint16".to_string()))?;
    object_set_key_value(mc, &proto, "setInt32", Value::Function("DataView.prototype.setInt32".to_string()))?;
    object_set_key_value(mc, &proto, "setUint32", Value::Function("DataView.prototype.setUint32".to_string()))?;
    object_set_key_value(
        mc,
        &proto,
        "setFloat32",
        Value::Function("DataView.prototype.setFloat32".to_string()),
    )?;
    object_set_key_value(
        mc,
        &proto,
        "setFloat64",
        Value::Function("DataView.prototype.setFloat64".to_string()),
    )?;

    Ok(proto)
}

/// Create TypedArray constructors
pub fn make_typedarray_constructors<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Vec<(String, JSObjectDataPtr<'gc>)>, JSError> {
    // Look up Object.prototype for inheritance fallback
    let mut object_prototype = None;
    if let Some(obj_val) = crate::core::env_get(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        object_prototype = Some(*proto);
    }

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
        let constructor = make_typedarray_constructor(mc, env, name, kind, object_prototype)?;
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

fn make_typedarray_constructor<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    name: &str,
    kind: TypedArrayKind,
    object_prototype: Option<JSObjectDataPtr<'gc>>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    // Mark as TypedArray constructor with kind
    let kind_value = typedarray_kind_to_number(&kind);

    let obj = new_js_object_data(mc);

    object_set_key_value(
        mc,
        &obj,
        "prototype",
        Value::Object(make_typedarray_prototype(mc, env, kind.clone(), object_prototype)?),
    )?;
    object_set_key_value(mc, &obj, "name", Value::String(utf8_to_utf16(name)))?;

    object_set_key_value(mc, &obj, "__kind", Value::Number(kind_value as f64))?;
    object_set_key_value(mc, &obj, "__native_ctor", Value::String(utf8_to_utf16("TypedArray")))?;

    // 22.2.5.1 TypedArray.BYTES_PER_ELEMENT - create constructor and prototype
    let bytes_per_element = match kind {
        TypedArrayKind::Int8 | TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => 1,
        TypedArrayKind::Int16 | TypedArrayKind::Uint16 => 2,
        TypedArrayKind::Int32 | TypedArrayKind::Uint32 | TypedArrayKind::Float32 => 4,
        TypedArrayKind::Float64 | TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => 8,
    } as f64;

    object_set_key_value(mc, &obj, "BYTES_PER_ELEMENT", Value::Number(bytes_per_element))?;
    obj.borrow_mut(mc).set_non_enumerable("BYTES_PER_ELEMENT");
    obj.borrow_mut(mc).set_non_writable("BYTES_PER_ELEMENT");
    obj.borrow_mut(mc).set_non_configurable("BYTES_PER_ELEMENT");

    // Also set on prototype per spec (TypedArray.prototype.BYTES_PER_ELEMENT)
    if let Some(proto_val) = object_get_key_value(&obj, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        object_set_key_value(mc, proto_obj, "BYTES_PER_ELEMENT", Value::Number(bytes_per_element))?;
        proto_obj.borrow_mut(mc).set_non_enumerable("BYTES_PER_ELEMENT");
        proto_obj.borrow_mut(mc).set_non_writable("BYTES_PER_ELEMENT");
        proto_obj.borrow_mut(mc).set_non_configurable("BYTES_PER_ELEMENT");
    }

    Ok(obj)
}

fn make_typedarray_prototype<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    kind: TypedArrayKind,
    object_prototype: Option<JSObjectDataPtr<'gc>>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let proto = new_js_object_data(mc);

    if let Some(proto_proto) = object_prototype {
        proto.borrow_mut(mc).prototype = Some(proto_proto);
        object_set_key_value(mc, &proto, "__proto__", Value::Object(proto_proto))?;
        proto.borrow_mut(mc).set_non_enumerable("__proto__");
    }

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

    object_set_key_value(mc, &proto, "__kind", Value::Number(kind_value as f64))?;
    // Make internal kind non-enumerable
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("__kind".to_string()));
    object_set_key_value(mc, &proto, "constructor", Value::Function("TypedArray".to_string()))?;
    // constructor is non-enumerable per spec
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("constructor".to_string()));

    // TypedArray properties and methods
    object_set_key_value(
        mc,
        &proto,
        "buffer",
        Value::Property {
            value: None,
            getter: Some(Box::new(Value::Function("TypedArray.prototype.buffer".to_string()))),
            setter: None,
        },
    )?;
    // buffer accessor is non-enumerable
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("buffer".to_string()));
    object_set_key_value(
        mc,
        &proto,
        "byteLength",
        Value::Property {
            value: None,
            getter: Some(Box::new(Value::Function("TypedArray.prototype.byteLength".to_string()))),
            setter: None,
        },
    )?;
    // byteLength accessor is non-enumerable
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("byteLength".to_string()));
    object_set_key_value(
        mc,
        &proto,
        "byteOffset",
        Value::Property {
            value: None,
            getter: Some(Box::new(Value::Function("TypedArray.prototype.byteOffset".to_string()))),
            setter: None,
        },
    )?;
    // byteOffset accessor is non-enumerable
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("byteOffset".to_string()));
    object_set_key_value(
        mc,
        &proto,
        "length",
        Value::Property {
            value: None,
            getter: Some(Box::new(Value::Function("TypedArray.prototype.length".to_string()))),
            setter: None,
        },
    )?;
    // length accessor is non-enumerable
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("length".to_string()));
    // Array methods that TypedArrays inherit
    object_set_key_value(mc, &proto, "set", Value::Function("TypedArray.prototype.set".to_string()))?;
    // set is non-enumerable
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("set".to_string()));
    object_set_key_value(mc, &proto, "subarray", Value::Function("TypedArray.prototype.subarray".to_string()))?;
    // subarray is non-enumerable
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("subarray".to_string()));
    object_set_key_value(mc, &proto, "values", Value::Function("TypedArray.prototype.values".to_string()))?;
    // values is non-enumerable
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("values".to_string()));

    // Register Symbol.iterator on TypedArray.prototype (alias to TypedArray.prototype.values)
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_val.borrow()
        && let Some(iter_sym_val) = object_get_key_value(sym_ctor, "iterator")
        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
    {
        let val = Value::Function("TypedArray.prototype.values".to_string());
        object_set_key_value(mc, &proto, iter_sym, val)?;
    }

    Ok(proto)
}

/// Handle ArrayBuffer constructor calls
pub fn handle_arraybuffer_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // ArrayBuffer(length)
    if args.is_empty() {
        return Err(raise_eval_error!("ArrayBuffer constructor requires a length argument"));
    }

    let length_val = args[0].clone();
    let length = match length_val {
        Value::Number(n) if n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0 => n as usize,
        _ => return Err(raise_eval_error!("ArrayBuffer length must be a non-negative integer")),
    };

    // Parse optional options object for resizable buffers
    let mut max_byte_length: Option<usize> = None;
    if args.len() > 1 {
        let opts = args[1].clone();
        if let Value::Object(obj) = opts {
            // Look for maxByteLength property
            if let Some(val) = object_get_key_value(&obj, "maxByteLength") {
                match &*val.borrow() {
                    Value::Number(n) if *n >= 0.0 && *n <= u32::MAX as f64 && n.fract() == 0.0 => {
                        let max = *n as usize;
                        if max < length {
                            return Err(crate::raise_range_error!("maxByteLength must be >= length"));
                        }
                        max_byte_length = Some(max);
                    }
                    _ => return Err(crate::raise_range_error!("maxByteLength must be a non-negative integer")),
                }
            }
        }
    }

    // Create ArrayBuffer instance
    let buffer = new_gc_cell_ptr(
        mc,
        JSArrayBuffer {
            data: Arc::new(Mutex::new(vec![0; length])),
            max_byte_length,
            ..JSArrayBuffer::default()
        },
    );

    // Create the ArrayBuffer object
    let obj = new_js_object_data(mc);
    object_set_key_value(mc, &obj, "__arraybuffer", Value::ArrayBuffer(buffer))?;

    // Set prototype
    let proto = make_arraybuffer_prototype(mc)?;
    obj.borrow_mut(mc).prototype = Some(proto);

    Ok(Value::Object(obj))
}

/// Handle SharedArrayBuffer constructor calls (creates a shared buffer)
pub fn handle_sharedarraybuffer_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // SharedArrayBuffer(length)
    if args.is_empty() {
        return Err(raise_eval_error!("SharedArrayBuffer constructor requires a length argument"));
    }

    let length_val = args[0].clone();
    let length = match length_val {
        Value::Number(n) if n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0 => n as usize,
        _ => return Err(raise_eval_error!("SharedArrayBuffer length must be a non-negative integer")),
    };

    // Create SharedArrayBuffer instance (mark shared: true)
    let buffer = new_gc_cell_ptr(
        mc,
        JSArrayBuffer {
            data: Arc::new(Mutex::new(vec![0; length])),
            shared: true,
            ..JSArrayBuffer::default()
        },
    );

    // Create the SharedArrayBuffer object wrapper
    let obj = new_js_object_data(mc);
    object_set_key_value(mc, &obj, "__arraybuffer", Value::ArrayBuffer(buffer))?;

    // Set prototype
    let mut proto_ptr = None;
    if let Some(ctor_val) = object_get_key_value(env, "SharedArrayBuffer")
        && let Value::Object(ctor_obj) = &*ctor_val.borrow()
        && let Some(p_val) = object_get_key_value(ctor_obj, "prototype")
        && let Value::Object(p_obj) = &*p_val.borrow()
    {
        proto_ptr = Some(*p_obj);
    }

    let proto = if let Some(p) = proto_ptr {
        p
    } else {
        // Fallback if constructor not found in env
        make_sharedarraybuffer_prototype(mc)?
    };
    obj.borrow_mut(mc).prototype = Some(proto);

    Ok(Value::Object(obj))
}

/// Handle DataView constructor calls
pub fn handle_dataview_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // DataView(buffer [, byteOffset [, byteLength]])
    if args.is_empty() {
        return Err(raise_eval_error!("DataView constructor requires a buffer argument"));
    }

    let buffer_val = args[0].clone();
    let buffer = match buffer_val {
        Value::Object(obj) => {
            if let Some(ab_val) = object_get_key_value(&obj, "__arraybuffer") {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    *ab
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
        let offset_val = args[1].clone();
        match offset_val {
            Value::Number(n) if n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0 => n as usize,
            _ => return Err(raise_eval_error!("DataView byteOffset must be a non-negative integer")),
        }
    } else {
        0
    };

    let byte_length = if args.len() > 2 {
        let length_val = args[2].clone();
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
    let data_view = Gc::new(
        mc,
        JSDataView {
            buffer,
            byte_offset,
            byte_length,
        },
    );

    // Create the DataView object
    let obj = new_js_object_data(mc);
    object_set_key_value(mc, &obj, "__dataview", Value::DataView(data_view))?;

    // Set prototype
    let proto = make_dataview_prototype(mc)?;
    obj.borrow_mut(mc).prototype = Some(proto);

    Ok(Value::Object(obj))
}

/// Handle TypedArray constructor calls
pub fn handle_typedarray_constructor<'gc>(
    mc: &MutationContext<'gc>,
    constructor_obj: &JSObjectDataPtr<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Get the kind from the constructor
    let kind_val = object_get_key_value(constructor_obj, "__kind");
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
        let buffer = new_gc_cell_ptr(
            mc,
            JSArrayBuffer {
                data: Arc::new(Mutex::new(vec![])),
                ..JSArrayBuffer::default()
            },
        );
        (buffer, 0, 0)
    } else if args.len() == 1 {
        let arg_val = args[0].clone();
        match arg_val {
            Value::Number(n) if n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0 => {
                // new TypedArray(length)
                let length = n as usize;
                let buffer = new_gc_cell_ptr(
                    mc,
                    JSArrayBuffer {
                        data: Arc::new(Mutex::new(vec![0; length * element_size])),
                        ..JSArrayBuffer::default()
                    },
                );
                (buffer, 0, length)
            }
            Value::Object(obj) => {
                // Check if it's another TypedArray or ArrayBuffer
                if let Some(ta_val) = object_get_key_value(&obj, "__typedarray") {
                    if let Value::TypedArray(ta) = &*ta_val.borrow() {
                        // new TypedArray(typedArray) - copy constructor
                        let src_length = ta.length;
                        let buffer = new_gc_cell_ptr(
                            mc,
                            JSArrayBuffer {
                                data: Arc::new(Mutex::new(vec![0; src_length * element_size])),
                                ..JSArrayBuffer::default()
                            },
                        );
                        // TODO: Copy data from source TypedArray
                        (buffer, 0, src_length)
                    } else {
                        return Err(raise_eval_error!("Invalid TypedArray constructor argument"));
                    }
                } else if let Some(ab_val) = object_get_key_value(&obj, "__arraybuffer") {
                    if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                        // new TypedArray(buffer)
                        (*ab, 0, (**ab).borrow().data.lock().unwrap().len() / element_size)
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
        let buffer_val = args[0].clone();
        let offset_val = args[1].clone();

        if let Value::Object(obj) = buffer_val {
            if let Some(ab_val) = object_get_key_value(&obj, "__arraybuffer") {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    if let Value::Number(offset_num) = offset_val {
                        let offset = offset_num as usize;
                        if !offset.is_multiple_of(element_size) {
                            return Err(raise_eval_error!("TypedArray byteOffset must be multiple of element size"));
                        }
                        let remaining_bytes = (**ab).borrow().data.lock().unwrap().len() - offset;
                        let length = remaining_bytes / element_size;
                        (*ab, offset, length)
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
        let buffer_val = args[0].clone();
        let offset_val = args[1].clone();
        let length_val = args[2].clone();

        if let Value::Object(obj) = buffer_val {
            if let Some(ab_val) = object_get_key_value(&obj, "__arraybuffer") {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    if let (Value::Number(offset_num), Value::Number(length_num)) = (offset_val, length_val) {
                        let offset = offset_num as usize;
                        let length = length_num as usize;
                        if !offset.is_multiple_of(element_size) {
                            return Err(raise_eval_error!("TypedArray byteOffset must be multiple of element size"));
                        }
                        if length * element_size + offset > (**ab).borrow().data.lock().unwrap().len() {
                            return Err(raise_eval_error!("TypedArray length exceeds buffer size"));
                        }
                        (*ab, offset, length)
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
    let obj = new_js_object_data(mc);

    // Set prototype from constructor
    if let Some(proto_val) = object_get_key_value(constructor_obj, "prototype") {
        if let Value::Object(proto_obj) = &*proto_val.borrow() {
            obj.borrow_mut(mc).prototype = Some(*proto_obj);
            object_set_key_value(mc, &obj, "__proto__", Value::Object(*proto_obj))?;
            obj.borrow_mut(mc).set_non_enumerable("__proto__");
        } else {
            // Fallback: create new prototype (legacy behavior, though incorrect for identity)
            let proto = make_typedarray_prototype(mc, env, kind.clone(), None)?;
            obj.borrow_mut(mc).prototype = Some(proto);
            object_set_key_value(mc, &obj, "__proto__", Value::Object(proto))?;
            obj.borrow_mut(mc).set_non_enumerable("__proto__");
        }
    } else {
        // Fallback
        let proto = make_typedarray_prototype(mc, env, kind.clone(), None)?;
        obj.borrow_mut(mc).prototype = Some(proto);
        object_set_key_value(mc, &obj, "__proto__", Value::Object(proto))?;
        obj.borrow_mut(mc).set_non_enumerable("__proto__");
    }

    // Determine if this TypedArray should be length-tracking (no explicit length argument)
    let length_tracking = match args.len() {
        1 => match &args[0] {
            Value::Object(obj) => object_get_key_value(obj, "__arraybuffer").is_some(),
            _ => false,
        },
        2 => match &args[0] {
            Value::Object(obj) => object_get_key_value(obj, "__arraybuffer").is_some(),
            _ => false,
        },
        _ => false,
    };

    // Create TypedArray instance
    let typed_array = Gc::new(
        mc,
        JSTypedArray {
            kind: kind.clone(),
            buffer,
            byte_offset,
            length,
            length_tracking,
        },
    );

    object_set_key_value(mc, &obj, "__typedarray", Value::TypedArray(typed_array))?;
    // __typedarray is an internal slot marker and should not be enumerable
    obj.borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("__typedarray".to_string()));
    log::debug!(
        "created typedarray instance: obj={:p} kind={:?} length_tracking={}",
        &*obj.borrow(),
        kind,
        length_tracking
    );

    Ok(Value::Object(obj))
}

/// Handle DataView instance method calls
pub fn handle_dataview_method<'gc>(
    _mc: &MutationContext<'gc>,
    object: &JSObjectDataPtr<'gc>,
    method: &str,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Get the DataView from the object
    let dv_val = object_get_key_value(object, "__dataview");
    let data_view_rc = if let Some(dv_val) = dv_val {
        if let Value::DataView(dv) = &*dv_val.borrow() {
            *dv
        } else {
            return Err(raise_eval_error!("Invalid DataView object").into());
        }
    } else {
        return Err(raise_eval_error!("DataView method called on non-DataView object").into());
    };

    match method {
        // Get methods - use immutable borrow
        "getInt8" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("DataView.getInt8 requires exactly 1 argument").into());
            }
            let offset_val = args[0].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let data_view = data_view_rc;
            data_view
                .get_int8(offset)
                .map(|v| Value::Number(v as f64))
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))
        }
        "getUint8" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("DataView.getUint8 requires exactly 1 argument").into());
            }
            let offset_val = args[0].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let data_view = data_view_rc;
            data_view
                .get_uint8(offset)
                .map(|v| Value::Number(v as f64))
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))
        }
        "getInt16" => {
            if args.is_empty() || args.len() > 2 {
                return Err(raise_eval_error!("DataView.getInt16 requires 1 or 2 arguments").into());
            }
            let offset_val = args[0].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let little_endian = if args.len() > 1 {
                let le_val = args[1].clone();
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean").into()),
                }
            } else {
                false
            };
            let data_view = data_view_rc;
            data_view
                .get_int16(offset, little_endian)
                .map(|v| Value::Number(v as f64))
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))
        }
        "getUint16" => {
            if args.is_empty() || args.len() > 2 {
                return Err(raise_eval_error!("DataView.getUint16 requires 1 or 2 arguments").into());
            }
            let offset_val = args[0].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let little_endian = if args.len() > 1 {
                let le_val = args[1].clone();
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean").into()),
                }
            } else {
                false
            };
            let data_view = data_view_rc;
            data_view
                .get_uint16(offset, little_endian)
                .map(|v| Value::Number(v as f64))
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))
        }
        "getInt32" => {
            if args.is_empty() || args.len() > 2 {
                return Err(raise_eval_error!("DataView.getInt32 requires 1 or 2 arguments").into());
            }
            let offset_val = args[0].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let little_endian = if args.len() > 1 {
                let le_val = args[1].clone();
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean").into()),
                }
            } else {
                false
            };
            let data_view = data_view_rc;
            data_view
                .get_int32(offset, little_endian)
                .map(|v| Value::Number(v as f64))
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))
        }
        "getUint32" => {
            if args.is_empty() || args.len() > 2 {
                return Err(raise_eval_error!("DataView.getUint32 requires 1 or 2 arguments").into());
            }
            let offset_val = args[0].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let little_endian = if args.len() > 1 {
                let le_val = args[1].clone();
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean").into()),
                }
            } else {
                false
            };
            let data_view = data_view_rc;
            data_view
                .get_uint32(offset, little_endian)
                .map(|v| Value::Number(v as f64))
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))
        }
        "getFloat32" => {
            if args.is_empty() || args.len() > 2 {
                return Err(raise_eval_error!("DataView.getFloat32 requires 1 or 2 arguments").into());
            }
            let offset_val = args[0].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let little_endian = if args.len() > 1 {
                let le_val = args[1].clone();
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean").into()),
                }
            } else {
                false
            };
            let data_view = data_view_rc;
            data_view
                .get_float32(offset, little_endian)
                .map(|v| Value::Number(v as f64))
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))
        }
        "getFloat64" => {
            if args.is_empty() || args.len() > 2 {
                return Err(raise_eval_error!("DataView.getFloat64 requires 1 or 2 arguments").into());
            }
            let offset_val = args[0].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let little_endian = if args.len() > 1 {
                let le_val = args[1].clone();
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean").into()),
                }
            } else {
                false
            };
            let data_view = data_view_rc;
            data_view
                .get_float64(offset, little_endian)
                .map(Value::Number)
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))
        }
        // Set methods - use mutable borrow
        "setInt8" => {
            if args.len() != 2 {
                return Err(raise_eval_error!("DataView.setInt8 requires exactly 2 arguments").into());
            }
            let offset_val = args[0].clone();
            let value_val = args[1].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let value = match value_val {
                Value::Number(n) => n as i8,
                _ => return Err(raise_eval_error!("DataView value must be a number").into()),
            };
            data_view_rc
                .set_int8(offset, value)
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))?;
            Ok(Value::Undefined)
        }
        "setUint8" => {
            if args.len() != 2 {
                return Err(raise_eval_error!("DataView.setUint8 requires exactly 2 arguments").into());
            }
            let offset_val = args[0].clone();
            let value_val = args[1].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let value = match value_val {
                Value::Number(n) => n as u8,
                _ => return Err(raise_eval_error!("DataView value must be a number").into()),
            };
            data_view_rc
                .set_uint8(offset, value)
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))?;
            Ok(Value::Undefined)
        }
        "setInt16" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("DataView.setInt16 requires 2 or 3 arguments").into());
            }
            let offset_val = args[0].clone();
            let value_val = args[1].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let value = match value_val {
                Value::Number(n) => n as i16,
                _ => return Err(raise_eval_error!("DataView value must be a number").into()),
            };
            let little_endian = if args.len() > 2 {
                let le_val = args[2].clone();
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean").into()),
                }
            } else {
                false
            };
            data_view_rc
                .set_int16(offset, value, little_endian)
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))?;
            Ok(Value::Undefined)
        }
        "setUint16" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("DataView.setUint16 requires 2 or 3 arguments").into());
            }
            let offset_val = args[0].clone();
            let value_val = args[1].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let value = match value_val {
                Value::Number(n) => n as u16,
                _ => return Err(raise_eval_error!("DataView value must be a number").into()),
            };
            let little_endian = if args.len() > 2 {
                let le_val = args[2].clone();
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean").into()),
                }
            } else {
                false
            };
            data_view_rc
                .set_uint16(offset, value, little_endian)
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))?;
            Ok(Value::Undefined)
        }
        "setInt32" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("DataView.setInt32 requires 2 or 3 arguments").into());
            }
            let offset_val = args[0].clone();
            let value_val = args[1].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let value = match value_val {
                Value::Number(n) => n as i32,
                _ => return Err(raise_eval_error!("DataView value must be a number").into()),
            };
            let little_endian = if args.len() > 2 {
                let le_val = args[2].clone();
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean").into()),
                }
            } else {
                false
            };
            data_view_rc
                .set_int32(offset, value, little_endian)
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))?;
            Ok(Value::Undefined)
        }
        "setUint32" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("DataView.setUint32 requires 2 or 3 arguments").into());
            }
            let offset_val = args[0].clone();
            let value_val = args[1].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let value = match value_val {
                Value::Number(n) => n as u32,
                _ => return Err(raise_eval_error!("DataView value must be a number").into()),
            };
            let little_endian = if args.len() > 2 {
                let le_val = args[2].clone();
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean").into()),
                }
            } else {
                false
            };
            data_view_rc
                .set_uint32(offset, value, little_endian)
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))?;
            Ok(Value::Undefined)
        }
        "setFloat32" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("DataView.setFloat32 requires 2 or 3 arguments").into());
            }
            let offset_val = args[0].clone();
            let value_val = args[1].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let value = match value_val {
                Value::Number(n) => n as f32,
                _ => return Err(raise_eval_error!("DataView value must be a number").into()),
            };
            let little_endian = if args.len() > 2 {
                let le_val = args[2].clone();
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean").into()),
                }
            } else {
                false
            };
            data_view_rc
                .set_float32(offset, value, little_endian)
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))?;
            Ok(Value::Undefined)
        }
        "setFloat64" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("DataView.setFloat64 requires 2 or 3 arguments").into());
            }
            let offset_val = args[0].clone();
            let value_val = args[1].clone();
            let offset = match offset_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => return Err(raise_eval_error!("DataView offset must be a non-negative integer").into()),
            };
            let value = match value_val {
                Value::Number(n) => n,
                _ => return Err(raise_eval_error!("DataView value must be a number").into()),
            };
            let little_endian = if args.len() > 2 {
                let le_val = args[2].clone();
                match le_val {
                    Value::Boolean(b) => b,
                    _ => return Err(raise_eval_error!("DataView littleEndian must be a boolean").into()),
                }
            } else {
                false
            };
            data_view_rc
                .set_float64(offset, value, little_endian)
                .map_err(|e| EvalError::Js(raise_eval_error!(e)))?;
            Ok(Value::Undefined)
        }
        // Property accessors
        "buffer" => Ok(Value::ArrayBuffer(data_view_rc.buffer)),
        "byteLength" => Ok(Value::Number(data_view_rc.byte_length as f64)),
        "byteOffset" => Ok(Value::Number(data_view_rc.byte_offset as f64)),
        _ => Err(raise_eval_error!(format!("DataView method '{method}' not implemented")).into()),
    }
}

impl<'gc> JSDataView<'gc> {
    fn check_bounds(&self, offset: usize, size: usize) -> Result<usize, JSError> {
        let start = self.byte_offset + offset;
        let end = start + size;
        let buffer = self.buffer.borrow();
        let buffer_len = buffer.data.lock().unwrap().len();
        if end > buffer_len {
            return Err(raise_eval_error!("Offset is outside the bounds of the DataView"));
        }
        Ok(start)
    }

    pub fn get_int8(&self, offset: usize) -> Result<i8, JSError> {
        let idx = self.check_bounds(offset, 1)?;
        let buffer = self.buffer.borrow();
        let data = buffer.data.lock().unwrap();
        Ok(data[idx] as i8)
    }

    pub fn get_uint8(&self, offset: usize) -> Result<u8, JSError> {
        let idx = self.check_bounds(offset, 1)?;
        let buffer = self.buffer.borrow();
        let data = buffer.data.lock().unwrap();
        Ok(data[idx])
    }

    pub fn get_int16(&self, offset: usize, little_endian: bool) -> Result<i16, JSError> {
        let idx = self.check_bounds(offset, 2)?;
        let buffer = self.buffer.borrow();
        let data = buffer.data.lock().unwrap();
        let bytes = [data[idx], data[idx + 1]];
        Ok(if little_endian {
            i16::from_le_bytes(bytes)
        } else {
            i16::from_be_bytes(bytes)
        })
    }

    pub fn get_uint16(&self, offset: usize, little_endian: bool) -> Result<u16, JSError> {
        let idx = self.check_bounds(offset, 2)?;
        let buffer = self.buffer.borrow();
        let data = buffer.data.lock().unwrap();
        let bytes = [data[idx], data[idx + 1]];
        Ok(if little_endian {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    }

    pub fn get_int32(&self, offset: usize, little_endian: bool) -> Result<i32, JSError> {
        let idx = self.check_bounds(offset, 4)?;
        let buffer = self.buffer.borrow();
        let data = buffer.data.lock().unwrap();
        let bytes = [data[idx], data[idx + 1], data[idx + 2], data[idx + 3]];
        Ok(if little_endian {
            i32::from_le_bytes(bytes)
        } else {
            i32::from_be_bytes(bytes)
        })
    }

    pub fn get_uint32(&self, offset: usize, little_endian: bool) -> Result<u32, JSError> {
        let idx = self.check_bounds(offset, 4)?;
        let buffer = self.buffer.borrow();
        let data = buffer.data.lock().unwrap();
        let bytes = [data[idx], data[idx + 1], data[idx + 2], data[idx + 3]];
        Ok(if little_endian {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    }

    pub fn get_float32(&self, offset: usize, little_endian: bool) -> Result<f32, JSError> {
        let idx = self.check_bounds(offset, 4)?;
        let buffer = self.buffer.borrow();
        let data = buffer.data.lock().unwrap();
        let bytes = [data[idx], data[idx + 1], data[idx + 2], data[idx + 3]];
        Ok(if little_endian {
            f32::from_le_bytes(bytes)
        } else {
            f32::from_be_bytes(bytes)
        })
    }

    pub fn get_float64(&self, offset: usize, little_endian: bool) -> Result<f64, JSError> {
        let idx = self.check_bounds(offset, 8)?;
        let buffer = self.buffer.borrow();
        let data = buffer.data.lock().unwrap();
        let bytes = [
            data[idx],
            data[idx + 1],
            data[idx + 2],
            data[idx + 3],
            data[idx + 4],
            data[idx + 5],
            data[idx + 6],
            data[idx + 7],
        ];
        Ok(if little_endian {
            f64::from_le_bytes(bytes)
        } else {
            f64::from_be_bytes(bytes)
        })
    }

    pub fn set_int8(&self, offset: usize, value: i8) -> Result<(), JSError> {
        let idx = self.check_bounds(offset, 1)?;
        let buffer = self.buffer.borrow();
        let mut data = buffer.data.lock().unwrap();
        data[idx] = value as u8;
        Ok(())
    }

    pub fn set_uint8(&self, offset: usize, value: u8) -> Result<(), JSError> {
        let idx = self.check_bounds(offset, 1)?;
        let buffer = self.buffer.borrow();
        let mut data = buffer.data.lock().unwrap();
        data[idx] = value;
        Ok(())
    }

    pub fn set_int16(&self, offset: usize, value: i16, little_endian: bool) -> Result<(), JSError> {
        let idx = self.check_bounds(offset, 2)?;
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let buffer = self.buffer.borrow();
        let mut data = buffer.data.lock().unwrap();
        data[idx] = bytes[0];
        data[idx + 1] = bytes[1];
        Ok(())
    }

    pub fn set_uint16(&self, offset: usize, value: u16, little_endian: bool) -> Result<(), JSError> {
        let idx = self.check_bounds(offset, 2)?;
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let buffer = self.buffer.borrow();
        let mut data = buffer.data.lock().unwrap();
        data[idx] = bytes[0];
        data[idx + 1] = bytes[1];
        Ok(())
    }

    pub fn set_int32(&self, offset: usize, value: i32, little_endian: bool) -> Result<(), JSError> {
        let idx = self.check_bounds(offset, 4)?;
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let buffer = self.buffer.borrow();
        let mut data = buffer.data.lock().unwrap();
        for i in 0..4 {
            data[idx + i] = bytes[i];
        }
        Ok(())
    }

    pub fn set_uint32(&self, offset: usize, value: u32, little_endian: bool) -> Result<(), JSError> {
        let idx = self.check_bounds(offset, 4)?;
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let buffer = self.buffer.borrow();
        let mut data = buffer.data.lock().unwrap();
        for i in 0..4 {
            data[idx + i] = bytes[i];
        }
        Ok(())
    }

    pub fn set_float32(&self, offset: usize, value: f32, little_endian: bool) -> Result<(), JSError> {
        let idx = self.check_bounds(offset, 4)?;
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let buffer = self.buffer.borrow();
        let mut data = buffer.data.lock().unwrap();
        for i in 0..4 {
            data[idx + i] = bytes[i];
        }
        Ok(())
    }

    pub fn set_float64(&self, offset: usize, value: f64, little_endian: bool) -> Result<(), JSError> {
        let idx = self.check_bounds(offset, 8)?;
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let buffer = self.buffer.borrow();
        let mut data = buffer.data.lock().unwrap();
        for i in 0..8 {
            data[idx + i] = bytes[i];
        }
        Ok(())
    }
}

impl<'gc> crate::core::JSTypedArray<'gc> {
    pub fn element_size(&self) -> usize {
        match self.kind {
            TypedArrayKind::Int8 | TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => 1,
            TypedArrayKind::Int16 | TypedArrayKind::Uint16 => 2,
            TypedArrayKind::Int32 | TypedArrayKind::Uint32 | TypedArrayKind::Float32 => 4,
            TypedArrayKind::Float64 | TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => 8,
        }
    }

    pub fn get(&self, idx: usize) -> Result<f64, crate::error::JSError> {
        let size = self.element_size();
        let byte_offset = self.byte_offset + idx * size;
        let buffer = self.buffer.borrow();
        let data = buffer.data.lock().unwrap();

        if byte_offset + size > data.len() {
            // If this typed array is a fixed-length view backed by a resizable
            // ArrayBuffer and the access falls outside the current buffer bounds,
            // the operation should throw a TypeError per the spec. For length-tracking
            // views, out-of-bounds reads behave like undefined -> NaN when coerced to Number.
            if !self.length_tracking {
                return Err(raise_type_error!("TypedArray access is out of bounds"));
            } else {
                return Ok(f64::NAN);
            }
        }

        // Very basic implementation:
        match self.kind {
            TypedArrayKind::Int8 => {
                let bytes = [data[byte_offset]];
                Ok(i8::from_ne_bytes(bytes) as f64)
            }
            TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => {
                let bytes = [data[byte_offset]];
                Ok(u8::from_ne_bytes(bytes) as f64)
            }
            TypedArrayKind::Int16 => {
                let bytes = [data[byte_offset], data[byte_offset + 1]];
                Ok(i16::from_le_bytes(bytes) as f64) // Assume LE for now
            }
            TypedArrayKind::Uint16 => {
                let bytes = [data[byte_offset], data[byte_offset + 1]];
                Ok(u16::from_le_bytes(bytes) as f64)
            }
            TypedArrayKind::Int32 => {
                let mut b = [0u8; 4];
                b.copy_from_slice(&data[byte_offset..byte_offset + 4]);
                Ok(i32::from_le_bytes(b) as f64)
            }
            TypedArrayKind::Uint32 => {
                let mut b = [0u8; 4];
                b.copy_from_slice(&data[byte_offset..byte_offset + 4]);
                Ok(u32::from_le_bytes(b) as f64)
            }
            TypedArrayKind::Float32 => {
                let mut b = [0u8; 4];
                b.copy_from_slice(&data[byte_offset..byte_offset + 4]);
                Ok(f32::from_le_bytes(b) as f64)
            }
            TypedArrayKind::Float64 => {
                let mut b = [0u8; 8];
                b.copy_from_slice(&data[byte_offset..byte_offset + 8]);
                Ok(f64::from_le_bytes(b))
            }
            _ => Ok(0.0), // BigInt not supported in this helper yet
        }
    }

    pub fn set(&self, mc: &crate::core::MutationContext<'gc>, idx: usize, val: f64) -> Result<(), crate::error::JSError> {
        let size = self.element_size();
        let byte_offset = self.byte_offset + idx * size;
        let buffer = self.buffer.borrow_mut(mc);
        let mut data = buffer.data.lock().unwrap();

        if byte_offset + size > data.len() {
            return Ok(());
        }

        match self.kind {
            TypedArrayKind::Int8 => {
                let b = (val as i8).to_le_bytes();
                data[byte_offset] = b[0];
            }
            TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => {
                let b = (val as u8).to_le_bytes();
                data[byte_offset] = b[0];
            }
            TypedArrayKind::Int16 => {
                let b = (val as i16).to_le_bytes();
                data[byte_offset] = b[0];
                data[byte_offset + 1] = b[1];
            }
            TypedArrayKind::Uint16 => {
                let b = (val as u16).to_le_bytes();
                data[byte_offset] = b[0];
                data[byte_offset + 1] = b[1];
            }
            TypedArrayKind::Int32 => {
                let b = (val as i32).to_le_bytes();
                data[byte_offset..byte_offset + 4].copy_from_slice(&b);
            }
            TypedArrayKind::Uint32 => {
                let b = (val as u32).to_le_bytes();
                data[byte_offset..byte_offset + 4].copy_from_slice(&b);
            }
            TypedArrayKind::Float32 => {
                let b = (val as f32).to_le_bytes();
                data[byte_offset..byte_offset + 4].copy_from_slice(&b);
            }
            TypedArrayKind::Float64 => {
                let b = val.to_le_bytes();
                data[byte_offset..byte_offset + 8].copy_from_slice(&b);
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn handle_arraybuffer_accessor<'gc>(
    _mc: &MutationContext<'gc>,
    object: &JSObjectDataPtr<'gc>,
    property: &str,
) -> Result<Value<'gc>, JSError> {
    match property {
        "byteLength" => {
            if let Some(ab_val) = object_get_key_value(object, "__arraybuffer") {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    let len = (**ab).borrow().data.lock().unwrap().len();
                    Ok(Value::Number(len as f64))
                } else {
                    Err(raise_eval_error!(
                        "Method ArrayBuffer.prototype.byteLength called on incompatible receiver"
                    ))
                }
            } else {
                Err(raise_eval_error!(
                    "Method ArrayBuffer.prototype.byteLength called on incompatible receiver"
                ))
            }
        }
        _ => Ok(Value::Undefined),
    }
}

pub fn handle_arraybuffer_method<'gc>(
    _mc: &MutationContext<'gc>,
    object: &JSObjectDataPtr<'gc>,
    method: &str,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, JSError> {
    match method {
        "resize" => {
            // Get the ArrayBuffer internal
            if let Some(ab_val) = object_get_key_value(object, "__arraybuffer") {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    // Must be resizable
                    if let Some(max) = (**ab).borrow().max_byte_length {
                        if args.is_empty() {
                            return Err(raise_range_error!("resize requires a new length"));
                        }
                        let new_len_val = args[0].clone();
                        let new_len = match new_len_val {
                            Value::Number(n) if n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0 => n as usize,
                            _ => return Err(raise_range_error!("new length must be a non-negative integer")),
                        };
                        if new_len > max {
                            return Err(raise_range_error!("new length exceeds maximum"));
                        }
                        let ab_borrow = (**ab).borrow();
                        let mut data = ab_borrow.data.lock().unwrap();
                        let cur_len = data.len();
                        if new_len > cur_len {
                            data.resize(new_len, 0u8);
                        } else if new_len < cur_len {
                            data.truncate(new_len);
                        }
                        Ok(Value::Undefined)
                    } else {
                        Err(raise_type_error!("ArrayBuffer is not resizable"))
                    }
                } else {
                    Err(raise_eval_error!(
                        "Method ArrayBuffer.prototype.resize called on incompatible receiver"
                    ))
                }
            } else {
                Err(raise_eval_error!(
                    "Method ArrayBuffer.prototype.resize called on incompatible receiver"
                ))
            }
        }
        _ => Ok(Value::Undefined),
    }
}

pub fn handle_typedarray_accessor<'gc>(
    _mc: &MutationContext<'gc>,
    object: &JSObjectDataPtr<'gc>,
    property: &str,
) -> Result<Value<'gc>, JSError> {
    if let Some(ta_val) = object_get_key_value(object, "__typedarray") {
        if let Value::TypedArray(ta) = &*ta_val.borrow() {
            match property {
                "buffer" => Ok(Value::ArrayBuffer(ta.buffer)),
                "byteLength" => {
                    let cur_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        ta.length
                    };
                    Ok(Value::Number((cur_len * ta.element_size()) as f64))
                }
                "byteOffset" => Ok(Value::Number(ta.byte_offset as f64)),
                "length" => {
                    let cur_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        ta.length
                    };
                    Ok(Value::Number(cur_len as f64))
                }
                "toStringTag" => {
                    let name = match ta.kind {
                        TypedArrayKind::Int8 => "Int8Array",
                        TypedArrayKind::Uint8 => "Uint8Array",
                        TypedArrayKind::Uint8Clamped => "Uint8ClampedArray",
                        TypedArrayKind::Int16 => "Int16Array",
                        TypedArrayKind::Uint16 => "Uint16Array",
                        TypedArrayKind::Int32 => "Int32Array",
                        TypedArrayKind::Uint32 => "Uint32Array",
                        TypedArrayKind::Float32 => "Float32Array",
                        TypedArrayKind::Float64 => "Float64Array",
                        TypedArrayKind::BigInt64 => "BigInt64Array",
                        TypedArrayKind::BigUint64 => "BigUint64Array",
                    };
                    Ok(Value::String(crate::unicode::utf8_to_utf16(name)))
                }
                _ => Ok(Value::Undefined),
            }
        } else {
            Err(raise_eval_error!(
                "Method TypedArray.prototype getter called on incompatible receiver"
            ))
        }
    } else {
        Err(raise_eval_error!(
            "Method TypedArray.prototype getter called on incompatible receiver"
        ))
    }
}

pub fn handle_typedarray_iterator_next<'gc>(mc: &MutationContext<'gc>, this_val: &Value<'gc>) -> Result<Value<'gc>, EvalError<'gc>> {
    if let Value::Object(obj) = this_val {
        if let Some(ta_cell) = object_get_key_value(obj, "__typedarray_iterator")
            && let Value::TypedArray(ta) = &*ta_cell.borrow()
            && let Some(index_cell) = object_get_key_value(obj, "__index")
            && let Value::Number(index) = &*index_cell.borrow()
        {
            // Check for detached buffer or out-of-bounds view (e.g. resizable buffer shrunk)
            let buffer = ta.buffer.borrow();
            if buffer.detached {
                return Err(raise_type_error!("TypedArray buffer is detached").into());
            }
            if !ta.length_tracking {
                let buf_len = buffer.data.lock().unwrap().len();
                let element_size = ta.element_size();
                if ta.byte_offset + ta.length * element_size > buf_len {
                    return Err(raise_type_error!("TypedArray is out of bounds").into());
                }
            }
            // For length-tracking views, if offset > buffer length, it is treated as length 0, not out-of-bounds error
            // (unless we are accessing an index that is out of computed bounds, which is handled by bounds check below)
            if ta.length_tracking {
                let buf_len = buffer.data.lock().unwrap().len();
                if ta.byte_offset > buf_len {
                    // This is technically out of bounds for the start of the view?
                    // Spec says IsTypedArrayOutOfBounds returns true if byteOffset > bufferByteLength
                    // So we should check this too.
                    return Err(raise_type_error!("TypedArray offset is out of bounds").into());
                }
            }

            let cur_len = if ta.length_tracking {
                let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                if buf_len <= ta.byte_offset {
                    0
                } else {
                    (buf_len - ta.byte_offset) / ta.element_size()
                }
            } else {
                ta.length
            };

            let idx = *index as usize;
            if idx < cur_len {
                // Get the value
                let value = match ta.kind {
                    TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => {
                        // For BigInt arrays, read the raw bytes and create a BigInt
                        let size = ta.element_size();
                        let byte_offset = ta.byte_offset + idx * size;
                        let buffer = ta.buffer.borrow();
                        let data = buffer.data.lock().unwrap();
                        if byte_offset + size <= data.len() {
                            let bytes = &data[byte_offset..byte_offset + size];
                            let big_int = if matches!(ta.kind, TypedArrayKind::BigInt64) {
                                let mut b = [0u8; 8];
                                b.copy_from_slice(bytes);
                                num_bigint::BigInt::from(i64::from_le_bytes(b))
                            } else {
                                let mut b = [0u8; 8];
                                b.copy_from_slice(bytes);
                                num_bigint::BigInt::from(u64::from_le_bytes(b))
                            };
                            Value::BigInt(Box::new(big_int))
                        } else {
                            Value::Undefined
                        }
                    }
                    _ => match ta.get(idx) {
                        Ok(n) => Value::Number(n),
                        Err(_) => Value::Undefined,
                    },
                };

                // Update index
                object_set_key_value(mc, obj, "__index", Value::Number((idx + 1) as f64))?;

                // Return { value, done: false }
                let result_obj = new_js_object_data(mc);
                object_set_key_value(mc, &result_obj, "value", value)?;
                object_set_key_value(mc, &result_obj, "done", Value::Boolean(false))?;
                Ok(Value::Object(result_obj))
            } else {
                // Done
                let result_obj = new_js_object_data(mc);
                object_set_key_value(mc, &result_obj, "value", Value::Undefined)?;
                object_set_key_value(mc, &result_obj, "done", Value::Boolean(true))?;
                Ok(Value::Object(result_obj))
            }
        } else {
            Err(raise_eval_error!("TypedArrayIterator.prototype.next called on incompatible receiver").into())
        }
    } else {
        Err(raise_eval_error!("TypedArrayIterator.prototype.next called on incompatible receiver").into())
    }
}

pub fn handle_typedarray_method<'gc>(
    mc: &MutationContext<'gc>,
    this_val: &Value<'gc>,
    method: &str,
    _args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if let Value::Object(obj) = this_val {
        if let Some(ta_cell) = object_get_key_value(obj, "__typedarray")
            && let Value::TypedArray(ta) = &*ta_cell.borrow()
        {
            match method {
                "values" => {
                    // Return an iterator that yields the values
                    // For now, create a simple iterator object
                    let iter_obj = new_js_object_data(mc);
                    object_set_key_value(mc, &iter_obj, "__typedarray_iterator", Value::TypedArray(*ta))?;
                    object_set_key_value(mc, &iter_obj, "__index", Value::Number(0.0))?;
                    object_set_key_value(
                        mc,
                        &iter_obj,
                        "next",
                        Value::Function("TypedArrayIterator.prototype.next".to_string()),
                    )?;
                    Ok(Value::Object(iter_obj))
                }
                _ => Err(raise_eval_error!(format!("TypedArray.prototype.{} not implemented", method))),
            }
        } else {
            Err(raise_eval_error!("Method TypedArray.prototype called on incompatible receiver"))
        }
    } else {
        Err(raise_eval_error!("Method TypedArray.prototype called on incompatible receiver"))
    }
}

pub fn initialize_typedarray<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let arraybuffer = make_arraybuffer_constructor(mc)?;
    crate::core::env_set(mc, env, "ArrayBuffer", Value::Object(arraybuffer))?;

    let dataview = make_dataview_constructor(mc)?;
    crate::core::env_set(mc, env, "DataView", Value::Object(dataview))?;

    let typed_arrays = make_typedarray_constructors(mc, env)?;
    for (name, ctor) in typed_arrays {
        crate::core::env_set(mc, env, &name, Value::Object(ctor))?;
    }

    let atomics = make_atomics_object(mc)?;
    crate::core::env_set(mc, env, "Atomics", Value::Object(atomics))?;

    let shared_ab = make_sharedarraybuffer_constructor(mc)?;
    crate::core::env_set(mc, env, "SharedArrayBuffer", Value::Object(shared_ab))?;

    Ok(())
}
