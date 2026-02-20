use crate::core::{
    ClosureData, Gc, InternalSlot, MutationContext, get_property_with_accessors, js_error_to_value, new_gc_cell_ptr, slot_get_chained,
    slot_set,
};
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

// Global waiters registry keyed by (buffer_arc_ptr, byte_index). Each waiter
// is an Arc containing a (Mutex<bool>, Condvar) pair the waiting thread blocks on.
#[allow(clippy::type_complexity)]
static WAITERS: LazyLock<Mutex<HashMap<(usize, usize), Vec<Arc<(Mutex<bool>, Condvar)>>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Create an ArrayBuffer constructor object
pub fn make_arraybuffer_constructor<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let obj = new_js_object_data(mc);

    if let Some(func_ctor_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*proto_val.borrow()
    {
        obj.borrow_mut(mc).prototype = Some(*func_proto);
    }

    let proto = make_arraybuffer_prototype(mc, env, &obj)?;
    object_set_key_value(mc, &obj, "prototype", &Value::Object(proto))?;
    obj.borrow_mut(mc).set_non_writable("prototype");
    obj.borrow_mut(mc).set_non_enumerable("prototype");
    obj.borrow_mut(mc).set_non_configurable("prototype");

    object_set_key_value(mc, &obj, "name", &Value::String(utf8_to_utf16("ArrayBuffer")))?;
    obj.borrow_mut(mc).set_non_writable("name");
    obj.borrow_mut(mc).set_non_enumerable("name");
    object_set_key_value(mc, &obj, "length", &Value::Number(1.0))?;
    obj.borrow_mut(mc).set_non_writable("length");
    obj.borrow_mut(mc).set_non_enumerable("length");

    let is_view_fn = new_js_object_data(mc);
    if let Some(func_ctor_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*proto_val.borrow()
    {
        is_view_fn.borrow_mut(mc).prototype = Some(*func_proto);
    }
    let closure = ClosureData {
        env: Some(*env),
        native_target: Some("ArrayBuffer.isView".to_string()),
        enforce_strictness_inheritance: true,
        ..ClosureData::default()
    };
    is_view_fn
        .borrow_mut(mc)
        .set_closure(Some(new_gc_cell_ptr(mc, Value::Closure(Gc::new(mc, closure)))));
    object_set_key_value(mc, &is_view_fn, "name", &Value::String(utf8_to_utf16("isView")))?;
    is_view_fn.borrow_mut(mc).set_non_writable("name");
    is_view_fn.borrow_mut(mc).set_non_enumerable("name");
    object_set_key_value(mc, &is_view_fn, "length", &Value::Number(1.0))?;
    is_view_fn.borrow_mut(mc).set_non_writable("length");
    is_view_fn.borrow_mut(mc).set_non_enumerable("length");

    object_set_key_value(mc, &obj, "isView", &Value::Object(is_view_fn))?;
    obj.borrow_mut(mc).set_non_enumerable("isView");

    // Mark as ArrayBuffer constructor
    slot_set(mc, &obj, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &obj, InternalSlot::ArrayBuffer, &Value::Boolean(true));
    slot_set(mc, &obj, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("ArrayBuffer")));

    Ok(obj)
}

/// Create the Atomics object with basic atomic methods
pub fn make_atomics_object<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let obj = new_js_object_data(mc);

    // Set __proto__ to Object.prototype
    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Object");

    // Get Function.prototype for method objects
    let func_proto_opt: Option<JSObjectDataPtr<'gc>> = if let Some(func_ctor_val) = crate::core::env_get(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*proto_val.borrow()
    {
        Some(*func_proto)
    } else {
        None
    };

    // Helper: install a method as a proper function object with name/length
    let methods: &[(&str, &str, usize)] = &[
        ("load", "Atomics.load", 2),
        ("store", "Atomics.store", 3),
        ("compareExchange", "Atomics.compareExchange", 4),
        ("exchange", "Atomics.exchange", 3),
        ("add", "Atomics.add", 3),
        ("sub", "Atomics.sub", 3),
        ("and", "Atomics.and", 3),
        ("or", "Atomics.or", 3),
        ("xor", "Atomics.xor", 3),
        ("wait", "Atomics.wait", 4),
        ("notify", "Atomics.notify", 3),
        ("isLockFree", "Atomics.isLockFree", 1),
    ];

    for &(method_name, dispatch_name, length) in methods {
        let fn_obj = new_js_object_data(mc);
        fn_obj
            .borrow_mut(mc)
            .set_closure(Some(new_gc_cell_ptr(mc, Value::Function(dispatch_name.to_string()))));
        if let Some(fp) = func_proto_opt {
            fn_obj.borrow_mut(mc).prototype = Some(fp);
        }
        let desc_name = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16(method_name)), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fn_obj, "name", &desc_name)?;
        let desc_len = crate::core::create_descriptor_object(mc, &Value::Number(length as f64), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fn_obj, "length", &desc_len)?;
        // writable: true, enumerable: false, configurable: true
        let desc_method = crate::core::create_descriptor_object(mc, &Value::Object(fn_obj), true, false, true)?;
        crate::js_object::define_property_internal(mc, &obj, method_name, &desc_method)?;
    }

    // Set Symbol.toStringTag = "Atomics" { writable: false, enumerable: false, configurable: true }
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let desc_tag = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("Atomics")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &obj, *tag_sym, &desc_tag)?;
    }

    Ok(obj)
}

pub(crate) fn is_typedarray(obj: &JSObjectDataPtr) -> bool {
    slot_get_chained(obj, &InternalSlot::TypedArray).is_some()
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
    if let Some(ta_cell) = crate::core::slot_get(obj, &InternalSlot::TypedArray)
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

/// ToBigInt coercion: convert a Value to BigInt per spec.
/// Handles BigInt, Boolean, String, and Object (via ToPrimitive).
fn to_bigint_i64<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, val: &Value<'gc>) -> Result<i64, EvalError<'gc>> {
    let prim = match val {
        Value::Object(_) => crate::core::to_primitive(mc, val, "number", env)?,
        other => other.clone(),
    };
    match &prim {
        Value::BigInt(b) => Ok(b.to_i64().unwrap_or(0)),
        Value::Boolean(b) => Ok(if *b { 1 } else { 0 }),
        Value::String(s) => {
            let s_str = crate::unicode::utf16_to_utf8(s);
            let s_str = s_str.trim();
            if s_str.is_empty() {
                Ok(0)
            } else {
                match s_str.parse::<i64>() {
                    Ok(n) => Ok(n),
                    Err(_) => Err(throw_type_error(mc, env, &format!("Cannot convert {} to a BigInt", s_str))),
                }
            }
        }
        Value::Number(_) => Err(throw_type_error(mc, env, "Cannot convert a Number value to a BigInt")),
        Value::Symbol(_) => Err(throw_type_error(mc, env, "Cannot convert a Symbol value to a BigInt")),
        _ => Err(throw_type_error(mc, env, "Cannot convert value to a BigInt")),
    }
}

/// Returns true if a TypedArrayKind is valid for Atomics operations
/// (integer types only — not Float32, Float64, or Uint8Clamped).
fn is_valid_atomic_type(kind: &TypedArrayKind) -> bool {
    matches!(
        kind,
        TypedArrayKind::Int8
            | TypedArrayKind::Uint8
            | TypedArrayKind::Int16
            | TypedArrayKind::Uint16
            | TypedArrayKind::Int32
            | TypedArrayKind::Uint32
            | TypedArrayKind::BigInt64
            | TypedArrayKind::BigUint64
    )
}

/// Returns true if a TypedArrayKind is BigInt64 or BigUint64.
fn is_bigint_typed_array(kind: &TypedArrayKind) -> bool {
    matches!(kind, TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64)
}

/// Throw a TypeError as EvalError::Throw using env's TypeError constructor.
fn throw_type_error<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, msg: &str) -> EvalError<'gc> {
    let js_err = raise_type_error!(msg);
    let val = crate::core::js_error_to_value(mc, env, &js_err);
    EvalError::Throw(val, None, None)
}

/// Throw a RangeError as EvalError::Throw using env's RangeError constructor.
fn throw_range_error<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, msg: &str) -> EvalError<'gc> {
    let js_err = raise_range_error!(msg);
    let val = crate::core::js_error_to_value(mc, env, &js_err);
    EvalError::Throw(val, None, None)
}

/// ValidateIntegerTypedArray (spec 25.4.1.1)
/// Extracts the TypedArray, validates it is an integer typed array and not detached.
/// Returns the JSTypedArray copy.
fn validate_integer_typed_array<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    arg: &Value<'gc>,
) -> Result<(Gc<'gc, JSTypedArray<'gc>>, JSObjectDataPtr<'gc>), EvalError<'gc>> {
    let object = match arg {
        Value::Object(o) => *o,
        _ => return Err(throw_type_error(mc, env, "Atomics: first argument must be a TypedArray")),
    };
    let ta_obj = if let Some(ta_rc) = slot_get_chained(&object, &InternalSlot::TypedArray)
        && let Value::TypedArray(ta) = &*ta_rc.borrow()
    {
        *ta
    } else {
        return Err(throw_type_error(mc, env, "Atomics: first argument must be a TypedArray"));
    };
    if !is_valid_atomic_type(&ta_obj.kind) {
        return Err(throw_type_error(
            mc,
            env,
            "Atomics: TypedArray must be an integer type (not Float32, Float64, or Uint8Clamped)",
        ));
    }
    if ta_obj.buffer.borrow().detached {
        return Err(throw_type_error(mc, env, "Atomics: TypedArray buffer is detached"));
    }
    Ok((ta_obj, object))
}

/// ValidateAtomicAccess (spec 25.4.1.2)
/// Coerces index arg to integer and validates it is within bounds.
fn validate_atomic_access<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    ta: &JSTypedArray<'gc>,
    index_arg: &Value<'gc>,
) -> Result<usize, EvalError<'gc>> {
    // Spec: ValidateAtomicAccess(taRecord, requestIndex)
    // 1. Let length = TypedArrayLength(taRecord)   ← read length FIRST
    // 2. Let accessIndex = ToIndex(requestIndex)     ← coerce index SECOND
    // 3. If accessIndex ≥ length, throw RangeError

    // Step 1: capture length BEFORE index coercion (valueOf may resize buffer)
    let length = if ta.length_tracking {
        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
        (buf_len.saturating_sub(ta.byte_offset)) / ta.element_size()
    } else {
        ta.length
    };

    // Step 2: ToIndex(requestIndex)
    let idx = match index_arg {
        Value::Undefined => 0usize,
        Value::BigInt(_) => return Err(throw_type_error(mc, env, "Cannot convert a BigInt value to a number")),
        Value::Symbol(_) => return Err(throw_type_error(mc, env, "Cannot convert a Symbol value to a number")),
        _ => {
            // ToIntegerOrInfinity: ToNumber first, then NaN/±0 → 0, ±∞ stays, else truncate
            let n = match index_arg {
                Value::Number(n) => *n,
                Value::Boolean(b) => {
                    if *b {
                        1.0
                    } else {
                        0.0
                    }
                }
                _ => crate::core::to_number_with_env(mc, env, index_arg)?,
            };
            let integer_index = if n.is_nan() || n == 0.0 {
                0.0
            } else if n.is_infinite() {
                n // +∞ or -∞
            } else {
                n.trunc()
            };
            // If integerIndex < 0, throw RangeError
            if integer_index < 0.0 {
                return Err(throw_range_error(mc, env, "Atomics: index out of range"));
            }
            // ToLength — clamp to [0, 2^53-1]
            const MAX_SAFE: f64 = 9007199254740991.0; // 2^53-1
            let index = if integer_index > MAX_SAFE { MAX_SAFE } else { integer_index };
            // SameValue(integerIndex, index) - catches +∞ since ToLength caps it
            if integer_index != index {
                return Err(throw_range_error(mc, env, "Atomics: index out of range"));
            }
            index as usize
        }
    };

    // Step 3: bounds check against the ORIGINAL length
    if idx >= length {
        return Err(throw_range_error(mc, env, "Atomics: index out of range"));
    }
    Ok(idx)
}

/// Handle Atomics.* calls (minimal mutex-backed implementations)
pub fn handle_atomics_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Special-case Atomics.isLockFree which accepts a size (in bytes)
    // and does not require a TypedArray as the first argument.
    if method == "isLockFree" {
        let size_val = args.first().cloned().unwrap_or(Value::Undefined);
        // Spec: ToIntegerOrInfinity(size)
        let size = match &size_val {
            Value::Number(n) => {
                let n = *n;
                #[allow(clippy::if_same_then_else)]
                if n.is_nan() || n == 0.0 {
                    0
                } else if n.is_infinite() {
                    0
                } else {
                    n.trunc() as i64
                }
            }
            Value::Undefined => 0,
            Value::Boolean(b) => {
                if *b {
                    1
                } else {
                    0
                }
            }
            Value::String(_) | Value::Object(_) => {
                let n = crate::core::to_number_with_env(mc, env, &size_val).unwrap_or(f64::NAN);
                if n.is_nan() || n == 0.0 || n.is_infinite() {
                    0
                } else {
                    n.trunc() as i64
                }
            }
            _ => 0,
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

    // All remaining methods: validate typed array type FIRST (before index coercion)
    let ta_arg = args.first().cloned().unwrap_or(Value::Undefined);
    let (ta_obj, _ta_js_obj) = validate_integer_typed_array(mc, env, &ta_arg)?;
    let is_bigint = is_bigint_typed_array(&ta_obj.kind);
    let is_shared = ta_obj.buffer.borrow().shared;

    match method {
        "load" => {
            let idx_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
            let idx = validate_atomic_access(mc, env, &ta_obj, &idx_arg)?;
            let v = ta_obj.get(idx)?;
            if is_bigint {
                Ok(Value::BigInt(Box::new(num_bigint::BigInt::from(v as i64))))
            } else {
                Ok(Value::Number(v))
            }
        }
        "store" => {
            let idx_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
            let val_arg = args.get(2).cloned().unwrap_or(Value::Undefined);
            // Per spec: coerce value BEFORE validating index
            let (store_val_f64, return_val) = if is_bigint {
                let v = to_bigint_i64(mc, env, &val_arg)?;
                (v as f64, Value::BigInt(Box::new(num_bigint::BigInt::from(v))))
            } else {
                let n = crate::core::to_number_with_env(mc, env, &val_arg)?;
                // Spec: return ToIntegerOrInfinity(v) which normalizes -0 to +0
                // and truncates. For integer types the return value is ToInteger.
                let int_n = if n.is_nan() || n == 0.0 { 0.0 } else { n.trunc() };
                // Normalize: -0.0 → +0.0
                let int_n = if int_n == 0.0 { 0.0 } else { int_n };
                (n, Value::Number(int_n))
            };
            let idx = validate_atomic_access(mc, env, &ta_obj, &idx_arg)?;
            ta_obj.set(mc, idx, store_val_f64)?;
            Ok(return_val)
        }
        "compareExchange" => {
            let idx_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
            let expected_arg = args.get(2).cloned().unwrap_or(Value::Undefined);
            let replacement_arg = args.get(3).cloned().unwrap_or(Value::Undefined);
            // Spec order: ValidateAtomicAccess THEN coerce values
            let idx = validate_atomic_access(mc, env, &ta_obj, &idx_arg)?;
            let (expected_f64, replacement_f64) = if is_bigint {
                let e = to_bigint_i64(mc, env, &expected_arg)? as f64;
                let r = to_bigint_i64(mc, env, &replacement_arg)? as f64;
                (e, r)
            } else {
                let e = crate::core::to_number_with_env(mc, env, &expected_arg)?;
                let r = crate::core::to_number_with_env(mc, env, &replacement_arg)?;
                (e, r)
            };
            let old = ta_obj.get(idx)?;
            // Compare at element-type width: convert expected through the same
            // NumericToRawBytes truncation as the stored value for proper modular comparison.
            let matches = match ta_obj.kind {
                TypedArrayKind::Int8 => (js_to_int32(old) as i8) == (js_to_int32(expected_f64) as i8),
                TypedArrayKind::Uint8 => (js_to_int32(old) as u8) == (js_to_int32(expected_f64) as u8),
                TypedArrayKind::Int16 => (js_to_int32(old) as i16) == (js_to_int32(expected_f64) as i16),
                TypedArrayKind::Uint16 => (js_to_int32(old) as u16) == (js_to_int32(expected_f64) as u16),
                TypedArrayKind::Int32 => js_to_int32(old) == js_to_int32(expected_f64),
                TypedArrayKind::Uint32 => (js_to_int32(old) as u32) == (js_to_int32(expected_f64) as u32),
                TypedArrayKind::BigInt64 => (old as i64) == (expected_f64 as i64),
                TypedArrayKind::BigUint64 => (old as u64) == (expected_f64 as u64),
                _ => old == expected_f64,
            };
            if matches {
                ta_obj.set(mc, idx, replacement_f64)?;
            }
            if is_bigint {
                Ok(Value::BigInt(Box::new(num_bigint::BigInt::from(old as i64))))
            } else {
                Ok(Value::Number(old))
            }
        }
        "add" | "sub" | "and" | "or" | "xor" | "exchange" => {
            let idx_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
            let val_arg = args.get(2).cloned().unwrap_or(Value::Undefined);
            // Spec order: ValidateAtomicAccess THEN coerce value
            let idx = validate_atomic_access(mc, env, &ta_obj, &idx_arg)?;
            let operand = if is_bigint {
                to_bigint_i64(mc, env, &val_arg)?
            } else {
                crate::core::to_number_with_env(mc, env, &val_arg)? as i64
            };
            let old = ta_obj.get(idx)?;
            let new_val = match method {
                "add" => (old as i64).wrapping_add(operand) as f64,
                "sub" => (old as i64).wrapping_sub(operand) as f64,
                "and" => ((old as i64) & operand) as f64,
                "or" => ((old as i64) | operand) as f64,
                "xor" => ((old as i64) ^ operand) as f64,
                "exchange" => operand as f64,
                _ => old,
            };
            ta_obj.set(mc, idx, new_val)?;
            if is_bigint {
                Ok(Value::BigInt(Box::new(num_bigint::BigInt::from(old as i64))))
            } else {
                Ok(Value::Number(old))
            }
        }
        "wait" => {
            // Atomics.wait(typedArray, index, value[, timeout])
            // Must be Int32Array or BigInt64Array on SharedArrayBuffer
            if !matches!(ta_obj.kind, TypedArrayKind::Int32 | TypedArrayKind::BigInt64) {
                return Err(throw_type_error(
                    mc,
                    env,
                    "Atomics.wait: TypedArray must be Int32Array or BigInt64Array",
                ));
            }
            if !is_shared {
                return Err(throw_type_error(
                    mc,
                    env,
                    "Atomics.wait: TypedArray must be backed by a SharedArrayBuffer",
                ));
            }
            // Spec order: index → value → timeout → AgentCanSuspend
            let idx_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
            let idx = validate_atomic_access(mc, env, &ta_obj, &idx_arg)?;

            let expected_arg = args.get(2).cloned().unwrap_or(Value::Undefined);
            // Coerce expected value
            let expected = if is_bigint {
                to_bigint_i64(mc, env, &expected_arg)?
            } else {
                crate::core::to_number_with_env(mc, env, &expected_arg)? as i64
            };

            // Coerce timeout
            let timeout_ms_opt = if args.len() > 3 {
                let tval = args[3].clone();
                match tval {
                    Value::Undefined => None, // +Infinity
                    _ => {
                        let n = crate::core::to_number_with_env(mc, env, &tval)?;
                        if n.is_nan() { None } else { Some(n) }
                    }
                }
            } else {
                None
            };

            // Read the current value at the index and compare with expected
            let byte_index = ta_obj.byte_offset + idx * ta_obj.element_size();
            let current = {
                let buf = ta_obj.buffer.borrow();
                let data = buf.data.lock().unwrap();
                if is_bigint {
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&data[byte_index..byte_index + 8]);
                    i64::from_le_bytes(b)
                } else {
                    let mut b = [0u8; 4];
                    b.copy_from_slice(&data[byte_index..byte_index + 4]);
                    i32::from_le_bytes(b) as i64
                }
            };

            if current != expected {
                return Ok(Value::String(utf8_to_utf16("not-equal")));
            }

            // Value matches — block until notified or timeout
            let timeout_dur = match timeout_ms_opt {
                Some(ms) if ms <= 0.0 => {
                    // Timeout of 0 or negative → immediate timeout
                    return Ok(Value::String(utf8_to_utf16("timed-out")));
                }
                Some(ms) => Some(std::time::Duration::from_millis(ms as u64)),
                None => None, // wait forever
            };

            let buffer_rc = ta_obj.buffer;
            let arc_ptr = Arc::as_ptr(&buffer_rc.borrow().data) as usize;
            let waiter = Arc::new((Mutex::new(false), Condvar::new()));
            {
                let mut map = WAITERS.lock().unwrap();
                map.entry((arc_ptr, byte_index)).or_default().push(waiter.clone());
            }

            let (lock, cvar) = &*waiter;
            let mut notified = lock.lock().unwrap();
            let result = if let Some(dur) = timeout_dur {
                let (guard, timeout_result) = cvar.wait_timeout(notified, dur).unwrap();
                notified = guard;
                if *notified {
                    "ok"
                } else if timeout_result.timed_out() {
                    "timed-out"
                } else {
                    "ok"
                }
            } else {
                // Wait indefinitely
                while !*notified {
                    notified = cvar.wait(notified).unwrap();
                }
                "ok"
            };

            // Clean up: remove this waiter from the registry
            {
                let mut map = WAITERS.lock().unwrap();
                if let Some(vec) = map.get_mut(&(arc_ptr, byte_index)) {
                    vec.retain(|w| !Arc::ptr_eq(w, &waiter));
                    if vec.is_empty() {
                        map.remove(&(arc_ptr, byte_index));
                    }
                }
            }

            Ok(Value::String(utf8_to_utf16(result)))
        }
        "notify" => {
            // Atomics.notify(typedArray, index[, count])
            // Must be Int32Array or BigInt64Array
            if !matches!(ta_obj.kind, TypedArrayKind::Int32 | TypedArrayKind::BigInt64) {
                return Err(throw_type_error(
                    mc,
                    env,
                    "Atomics.notify: TypedArray must be Int32Array or BigInt64Array",
                ));
            }
            let idx_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
            let count_arg = args.get(2).cloned().unwrap_or(Value::Undefined);
            // Spec order: ValidateAtomicAccess THEN coerce count
            let idx = validate_atomic_access(mc, env, &ta_obj, &idx_arg)?;
            // Spec: IntegerOrInfinity(count), then max(intCount, 0)
            // Undefined → +∞ (notify all). Negative → clamp to 0.
            let count = match &count_arg {
                Value::Undefined => usize::MAX,
                _ => {
                    let n = crate::core::to_number_with_env(mc, env, &count_arg)?;
                    if n.is_nan() || n == 0.0 {
                        0usize
                    } else if n.is_infinite() && n > 0.0 {
                        usize::MAX
                    } else {
                        let int_count = n.trunc() as i64;
                        std::cmp::max(int_count, 0) as usize
                    }
                }
            };

            // For non-shared buffers, Atomics.notify just returns 0
            if !is_shared {
                return Ok(Value::Number(0.0));
            }

            let buffer_rc = ta_obj.buffer;
            let arc_ptr = Arc::as_ptr(&buffer_rc.borrow().data) as usize;
            let byte_index = ta_obj.byte_offset + idx * ta_obj.element_size();

            let mut awakened = 0usize;
            let mut map = WAITERS.lock().unwrap();
            if let Some(vec) = map.get_mut(&(arc_ptr, byte_index)) {
                let to_awake = std::cmp::min(count, vec.len());
                for _ in 0..to_awake {
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
        _ => Err(throw_type_error(mc, env, &format!("Atomics.{} is not a function", method))),
    }
}

/// Create a SharedArrayBuffer constructor object
pub fn make_sharedarraybuffer_constructor<'gc>(mc: &MutationContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let obj = new_js_object_data(mc);

    // Set prototype and name
    object_set_key_value(mc, &obj, "prototype", &Value::Object(make_sharedarraybuffer_prototype(mc)?))?;
    object_set_key_value(mc, &obj, "name", &Value::String(utf8_to_utf16("SharedArrayBuffer")))?;

    // Mark as ArrayBuffer constructor and indicate it's the shared variant
    slot_set(mc, &obj, InternalSlot::ArrayBuffer, &Value::Boolean(true));
    slot_set(mc, &obj, InternalSlot::SharedArrayBuffer, &Value::Boolean(true));
    slot_set(
        mc,
        &obj,
        InternalSlot::NativeCtor,
        &Value::String(utf8_to_utf16("SharedArrayBuffer")),
    );

    Ok(obj)
}

/// Create the ArrayBuffer prototype
pub fn make_arraybuffer_prototype<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    ctor: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let proto = new_js_object_data(mc);

    object_set_key_value(mc, &proto, "constructor", &Value::Object(*ctor))?;
    proto.borrow_mut(mc).set_non_enumerable("constructor");

    // byteLength is an accessor property
    let byte_len_getter = Value::Function("get byteLength".to_string());
    let byte_len_prop = Value::Property {
        value: None,
        getter: Some(Box::new(byte_len_getter)),
        setter: None,
    };
    object_set_key_value(mc, &proto, "byteLength", &byte_len_prop)?;
    proto.borrow_mut(mc).set_non_enumerable("byteLength");

    let max_byte_len_prop = Value::Property {
        value: None,
        getter: Some(Box::new(Value::Function("get maxByteLength".to_string()))),
        setter: None,
    };
    object_set_key_value(mc, &proto, "maxByteLength", &max_byte_len_prop)?;
    proto.borrow_mut(mc).set_non_enumerable("maxByteLength");

    let resizable_prop = Value::Property {
        value: None,
        getter: Some(Box::new(Value::Function("get resizable".to_string()))),
        setter: None,
    };
    object_set_key_value(mc, &proto, "resizable", &resizable_prop)?;
    proto.borrow_mut(mc).set_non_enumerable("resizable");

    object_set_key_value(mc, &proto, "slice", &Value::Function("ArrayBuffer.prototype.slice".to_string()))?;
    proto.borrow_mut(mc).set_non_enumerable("slice");
    object_set_key_value(mc, &proto, "resize", &Value::Function("ArrayBuffer.prototype.resize".to_string()))?;
    proto.borrow_mut(mc).set_non_enumerable("resize");

    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_val.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_ctor, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("ArrayBuffer")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &proto, PropertyKey::Symbol(*tag_sym), &desc)?;
    }

    Ok(proto)
}

/// Create the SharedArrayBuffer prototype
pub fn make_sharedarraybuffer_prototype<'gc>(mc: &MutationContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let proto = new_js_object_data(mc);

    // Add methods to prototype
    object_set_key_value(mc, &proto, "constructor", &Value::Function("SharedArrayBuffer".to_string()))?;

    // byteLength is an accessor property
    object_set_key_value(
        mc,
        &proto,
        "byteLength",
        &Value::Property {
            value: None,
            getter: Some(Box::new(Value::Function("SharedArrayBuffer.prototype.byteLength".to_string()))),
            setter: None,
        },
    )?;

    object_set_key_value(
        mc,
        &proto,
        "slice",
        &Value::Function("SharedArrayBuffer.prototype.slice".to_string()),
    )?;

    Ok(proto)
}

/// Create a DataView constructor object
pub fn make_dataview_constructor<'gc>(mc: &MutationContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let obj = new_js_object_data(mc);

    object_set_key_value(mc, &obj, "prototype", &Value::Object(make_dataview_prototype(mc)?))?;
    object_set_key_value(mc, &obj, "name", &Value::String(utf8_to_utf16("DataView")))?;

    // Mark as DataView constructor
    slot_set(mc, &obj, InternalSlot::DataView, &Value::Boolean(true));
    slot_set(mc, &obj, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("DataView")));

    Ok(obj)
}

/// Create the DataView prototype
pub fn make_dataview_prototype<'gc>(mc: &MutationContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let proto = new_js_object_data(mc);

    object_set_key_value(mc, &proto, "constructor", &Value::Function("DataView".to_string()))?;
    object_set_key_value(
        mc,
        &proto,
        "buffer",
        &Value::Property {
            value: None,
            getter: Some(Box::new(Value::Function("DataView.prototype.buffer".to_string()))),
            setter: None,
        },
    )?;
    object_set_key_value(
        mc,
        &proto,
        "byteLength",
        &Value::Property {
            value: None,
            getter: Some(Box::new(Value::Function("DataView.prototype.byteLength".to_string()))),
            setter: None,
        },
    )?;
    object_set_key_value(
        mc,
        &proto,
        "byteOffset",
        &Value::Property {
            value: None,
            getter: Some(Box::new(Value::Function("DataView.prototype.byteOffset".to_string()))),
            setter: None,
        },
    )?;

    // DataView methods for different data types
    object_set_key_value(mc, &proto, "getInt8", &Value::Function("DataView.prototype.getInt8".to_string()))?;
    object_set_key_value(mc, &proto, "getUint8", &Value::Function("DataView.prototype.getUint8".to_string()))?;
    object_set_key_value(mc, &proto, "getInt16", &Value::Function("DataView.prototype.getInt16".to_string()))?;
    object_set_key_value(
        mc,
        &proto,
        "getUint16",
        &Value::Function("DataView.prototype.getUint16".to_string()),
    )?;
    object_set_key_value(mc, &proto, "getInt32", &Value::Function("DataView.prototype.getInt32".to_string()))?;
    object_set_key_value(
        mc,
        &proto,
        "getUint32",
        &Value::Function("DataView.prototype.getUint32".to_string()),
    )?;
    object_set_key_value(
        mc,
        &proto,
        "getFloat32",
        &Value::Function("DataView.prototype.getFloat32".to_string()),
    )?;
    object_set_key_value(
        mc,
        &proto,
        "getFloat64",
        &Value::Function("DataView.prototype.getFloat64".to_string()),
    )?;

    object_set_key_value(mc, &proto, "setInt8", &Value::Function("DataView.prototype.setInt8".to_string()))?;
    object_set_key_value(mc, &proto, "setUint8", &Value::Function("DataView.prototype.setUint8".to_string()))?;
    object_set_key_value(mc, &proto, "setInt16", &Value::Function("DataView.prototype.setInt16".to_string()))?;
    object_set_key_value(
        mc,
        &proto,
        "setUint16",
        &Value::Function("DataView.prototype.setUint16".to_string()),
    )?;
    object_set_key_value(mc, &proto, "setInt32", &Value::Function("DataView.prototype.setInt32".to_string()))?;
    object_set_key_value(
        mc,
        &proto,
        "setUint32",
        &Value::Function("DataView.prototype.setUint32".to_string()),
    )?;
    object_set_key_value(
        mc,
        &proto,
        "setFloat32",
        &Value::Function("DataView.prototype.setFloat32".to_string()),
    )?;
    object_set_key_value(
        mc,
        &proto,
        "setFloat64",
        &Value::Function("DataView.prototype.setFloat64".to_string()),
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
        &Value::Object(make_typedarray_prototype(mc, env, kind.clone(), object_prototype)?),
    )?;
    object_set_key_value(mc, &obj, "name", &Value::String(utf8_to_utf16(name)))?;

    slot_set(mc, &obj, InternalSlot::Kind, &Value::Number(kind_value as f64));
    slot_set(mc, &obj, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("TypedArray")));
    slot_set(mc, &obj, InternalSlot::IsConstructor, &Value::Boolean(true));

    // 22.2.5.1 TypedArray.BYTES_PER_ELEMENT - create constructor and prototype
    let bytes_per_element = match kind {
        TypedArrayKind::Int8 | TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => 1,
        TypedArrayKind::Int16 | TypedArrayKind::Uint16 => 2,
        TypedArrayKind::Int32 | TypedArrayKind::Uint32 | TypedArrayKind::Float32 => 4,
        TypedArrayKind::Float64 | TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => 8,
    } as f64;

    object_set_key_value(mc, &obj, "BYTES_PER_ELEMENT", &Value::Number(bytes_per_element))?;
    obj.borrow_mut(mc).set_non_enumerable("BYTES_PER_ELEMENT");
    obj.borrow_mut(mc).set_non_writable("BYTES_PER_ELEMENT");
    obj.borrow_mut(mc).set_non_configurable("BYTES_PER_ELEMENT");

    // Also set on prototype per spec (TypedArray.prototype.BYTES_PER_ELEMENT)
    if let Some(proto_val) = object_get_key_value(&obj, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        object_set_key_value(mc, proto_obj, "BYTES_PER_ELEMENT", &Value::Number(bytes_per_element))?;
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
        slot_set(mc, &proto, InternalSlot::Proto, &Value::Object(proto_proto));
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

    slot_set(mc, &proto, InternalSlot::Kind, &Value::Number(kind_value as f64));
    object_set_key_value(mc, &proto, "constructor", &Value::Function("TypedArray".to_string()))?;
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
        &Value::Property {
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
        &Value::Property {
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
        &Value::Property {
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
        &Value::Property {
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
    object_set_key_value(mc, &proto, "set", &Value::Function("TypedArray.prototype.set".to_string()))?;
    // set is non-enumerable
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("set".to_string()));
    object_set_key_value(
        mc,
        &proto,
        "subarray",
        &Value::Function("TypedArray.prototype.subarray".to_string()),
    )?;
    // subarray is non-enumerable
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("subarray".to_string()));
    object_set_key_value(mc, &proto, "values", &Value::Function("TypedArray.prototype.values".to_string()))?;
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("values".to_string()));
    object_set_key_value(mc, &proto, "keys", &Value::Function("TypedArray.prototype.keys".to_string()))?;
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("keys".to_string()));
    object_set_key_value(mc, &proto, "entries", &Value::Function("TypedArray.prototype.entries".to_string()))?;
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("entries".to_string()));

    object_set_key_value(mc, &proto, "fill", &Value::Function("TypedArray.prototype.fill".to_string()))?;
    proto
        .borrow_mut(mc)
        .non_enumerable
        .insert(crate::core::PropertyKey::String("fill".to_string()));

    // Register Symbol.iterator on TypedArray.prototype (alias to TypedArray.prototype.values)
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_val.borrow()
        && let Some(iter_sym_val) = object_get_key_value(sym_ctor, "iterator")
        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
    {
        let val = Value::Function("TypedArray.prototype.values".to_string());
        object_set_key_value(mc, &proto, iter_sym, &val)?;
    }

    Ok(proto)
}

/// Handle ArrayBuffer constructor calls
pub fn handle_arraybuffer_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    new_target: Option<&Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let to_index = |v: &Value<'gc>| -> Result<usize, EvalError<'gc>> {
        let prim = if let Value::Object(_) = v {
            crate::core::to_primitive(mc, v, "number", env)?
        } else {
            v.clone()
        };

        if matches!(prim, Value::Undefined) {
            return Ok(0);
        }
        if matches!(prim, Value::Symbol(_) | Value::BigInt(_)) {
            return Err(raise_type_error!("Cannot convert value to index").into());
        }

        let n = crate::core::to_number(&prim)?;
        let integer_index = if n.is_nan() || n == 0.0 {
            0.0
        } else if !n.is_finite() {
            n
        } else {
            n.trunc()
        };

        if integer_index < 0.0 {
            return Err(raise_range_error!("ArrayBuffer length must be a non-negative integer").into());
        }

        let to_length = if !integer_index.is_finite() {
            (1u64 << 53) as f64 - 1.0
        } else {
            integer_index.min((1u64 << 53) as f64 - 1.0)
        };
        if (integer_index - to_length).abs() > 0.0 {
            return Err(raise_range_error!("ArrayBuffer length is too large").into());
        }

        Ok(integer_index as usize)
    };

    let length = if let Some(v) = args.first() { to_index(v)? } else { 0 };

    // Parse optional options object for resizable buffers
    let mut max_byte_length: Option<usize> = None;
    if args.len() > 1 {
        let opts = args[1].clone();
        if let Value::Object(obj) = opts {
            // Look for maxByteLength property
            let max_val = crate::core::get_property_with_accessors(mc, env, &obj, "maxByteLength")?;
            if !matches!(max_val, Value::Undefined) {
                let max = to_index(&max_val)?;
                if max < length {
                    return Err(crate::raise_range_error!("maxByteLength must be >= length").into());
                }
                if max > (u32::MAX as usize) {
                    return Err(crate::raise_range_error!("maxByteLength is too large").into());
                }
                max_byte_length = Some(max);
            }
        }
    }

    // Create the ArrayBuffer object first (AllocateArrayBuffer ordering)
    let obj = new_js_object_data(mc);

    // GetPrototypeFromConstructor for ArrayBuffer
    let proto = if let Some(Value::Object(nt_obj)) = new_target
        && let Some(p) = crate::js_class::get_prototype_from_constructor(mc, nt_obj, env, "ArrayBuffer")?
    {
        p
    } else if let Some(ctor_val) = object_get_key_value(env, "ArrayBuffer")
        && let Value::Object(ctor_obj) = &*ctor_val.borrow()
        && let Some(p_val) = object_get_key_value(ctor_obj, "prototype")
        && let Value::Object(p_obj) = &*p_val.borrow()
    {
        *p_obj
    } else {
        let fallback_ctor = new_js_object_data(mc);
        make_arraybuffer_prototype(mc, env, &fallback_ctor)?
    };
    obj.borrow_mut(mc).prototype = Some(proto);

    // Host guard against unsupported large allocation (after object/prototype creation)
    if length > (u32::MAX as usize) {
        return Err(raise_range_error!("ArrayBuffer length is too large").into());
    }

    let buffer = new_gc_cell_ptr(
        mc,
        JSArrayBuffer {
            data: Arc::new(Mutex::new(vec![0; length])),
            max_byte_length,
            ..JSArrayBuffer::default()
        },
    );
    slot_set(mc, &obj, InternalSlot::ArrayBuffer, &Value::ArrayBuffer(buffer));

    Ok(Value::Object(obj))
}

pub fn handle_arraybuffer_static_method<'gc>(method: &str, args: &[Value<'gc>]) -> Result<Value<'gc>, JSError> {
    match method {
        "isView" => {
            if let Some(Value::Object(obj)) = args.first() {
                let is_typed_array = slot_get_chained(obj, &InternalSlot::TypedArray).is_some();
                let is_dataview_instance = slot_get_chained(obj, &InternalSlot::DataView)
                    .map(|rc| matches!(&*rc.borrow(), Value::DataView(_)))
                    .unwrap_or(false);
                Ok(Value::Boolean(is_typed_array || is_dataview_instance))
            } else {
                Ok(Value::Boolean(false))
            }
        }
        _ => Ok(Value::Undefined),
    }
}

/// Handle SharedArrayBuffer constructor calls (creates a shared buffer)
pub fn handle_sharedarraybuffer_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    new_target: Option<&Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // ToIndex(length) – same logic as ArrayBuffer
    let to_index = |v: &Value<'gc>| -> Result<usize, EvalError<'gc>> {
        let prim = if let Value::Object(_) = v {
            crate::core::to_primitive(mc, v, "number", env)?
        } else {
            v.clone()
        };

        if matches!(prim, Value::Undefined) {
            return Ok(0);
        }
        if matches!(prim, Value::Symbol(_) | Value::BigInt(_)) {
            return Err(raise_type_error!("Cannot convert value to index").into());
        }

        let n = crate::core::to_number(&prim)?;
        let integer_index = if n.is_nan() || n == 0.0 {
            0.0
        } else if !n.is_finite() {
            n
        } else {
            n.trunc()
        };

        if integer_index < 0.0 {
            return Err(raise_range_error!("SharedArrayBuffer length must be a non-negative integer").into());
        }

        let to_length = if !integer_index.is_finite() {
            (1u64 << 53) as f64 - 1.0
        } else {
            integer_index.min((1u64 << 53) as f64 - 1.0)
        };
        if (integer_index - to_length).abs() > 0.0 {
            return Err(raise_range_error!("SharedArrayBuffer length is too large").into());
        }

        Ok(integer_index as usize)
    };

    let length = if let Some(v) = args.first() { to_index(v)? } else { 0 };

    // Create the SharedArrayBuffer object first
    let obj = new_js_object_data(mc);

    // Set prototype from NewTarget.prototype if present; otherwise fallback to SharedArrayBuffer.prototype
    let mut proto_from_target: Option<JSObjectDataPtr<'gc>> = None;
    if let Some(Value::Object(nt_obj)) = new_target {
        let proto_val = crate::core::get_property_with_accessors(mc, env, nt_obj, "prototype")?;
        if let Value::Object(proto_obj) = proto_val {
            proto_from_target = Some(proto_obj);
        }
    }

    let proto = if let Some(p) = proto_from_target {
        p
    } else if let Some(ctor_val) = object_get_key_value(env, "SharedArrayBuffer")
        && let Value::Object(ctor_obj) = &*ctor_val.borrow()
        && let Some(p_val) = object_get_key_value(ctor_obj, "prototype")
        && let Value::Object(p_obj) = &*p_val.borrow()
    {
        *p_obj
    } else {
        make_sharedarraybuffer_prototype(mc)?
    };
    obj.borrow_mut(mc).prototype = Some(proto);

    // Guard against unsupported large allocation
    if length > (u32::MAX as usize) {
        return Err(raise_range_error!("SharedArrayBuffer length is too large").into());
    }

    // Create SharedArrayBuffer instance (mark shared: true)
    let buffer = new_gc_cell_ptr(
        mc,
        JSArrayBuffer {
            data: Arc::new(Mutex::new(vec![0; length])),
            shared: true,
            ..JSArrayBuffer::default()
        },
    );
    slot_set(mc, &obj, InternalSlot::ArrayBuffer, &Value::ArrayBuffer(buffer));

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
        return Err(raise_type_error!("DataView constructor requires a buffer argument"));
    }

    let buffer_val = args[0].clone();
    let buffer_obj = if let Value::Object(obj) = &buffer_val { Some(*obj) } else { None };
    let buffer = match buffer_val {
        Value::Object(obj) => {
            if let Some(ab_val) = slot_get_chained(&obj, &InternalSlot::ArrayBuffer) {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    *ab
                } else {
                    return Err(raise_type_error!("DataView buffer must be an ArrayBuffer"));
                }
            } else {
                return Err(raise_type_error!("DataView buffer must be an ArrayBuffer"));
            }
        }
        _ => return Err(raise_type_error!("DataView buffer must be an ArrayBuffer")),
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
    slot_set(mc, &obj, InternalSlot::DataView, &Value::DataView(data_view));
    if let Some(buf_obj) = buffer_obj {
        slot_set(mc, &obj, InternalSlot::BufferObject, &Value::Object(buf_obj));
    }

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
    let kind_val = slot_get_chained(constructor_obj, &InternalSlot::Kind);
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

    let mut init_values: Option<Vec<Value<'gc>>> = None;

    // Track the source ArrayBuffer wrapper object (if passed), so we can store it as BufferObject.
    let mut buffer_obj_opt: Option<JSObjectDataPtr<'gc>> = None;

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
                if let Some(ta_val) = slot_get_chained(&obj, &InternalSlot::TypedArray) {
                    if let Value::TypedArray(ta) = &*ta_val.borrow() {
                        // new TypedArray(typedArray) - copy constructor
                        let src_length = ta.length;
                        let mut copied = Vec::with_capacity(src_length);
                        for idx in 0..src_length {
                            let val = if matches!(ta.kind, TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64) {
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
                            } else {
                                Value::Number(ta.get(idx).unwrap_or(f64::NAN))
                            };
                            copied.push(val);
                        }
                        init_values = Some(copied);
                        let buffer = new_gc_cell_ptr(
                            mc,
                            JSArrayBuffer {
                                data: Arc::new(Mutex::new(vec![0; src_length * element_size])),
                                ..JSArrayBuffer::default()
                            },
                        );
                        (buffer, 0, src_length)
                    } else {
                        return Err(raise_eval_error!("Invalid TypedArray constructor argument"));
                    }
                } else if let Some(ab_val) = slot_get_chained(&obj, &InternalSlot::ArrayBuffer) {
                    if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                        // new TypedArray(buffer)
                        buffer_obj_opt = Some(obj);
                        (*ab, 0, (**ab).borrow().data.lock().unwrap().len() / element_size)
                    } else {
                        return Err(raise_eval_error!("Invalid TypedArray constructor argument"));
                    }
                } else {
                    let src_length = crate::core::object_get_length(&obj).unwrap_or_else(|| {
                        object_get_key_value(&obj, "length")
                            .and_then(|cell| match &*cell.borrow() {
                                Value::Number(n) if *n >= 0.0 && n.is_finite() => Some(*n as usize),
                                _ => None,
                            })
                            .unwrap_or(0)
                    });

                    let mut copied = Vec::with_capacity(src_length);
                    for idx in 0..src_length {
                        let value = object_get_key_value(&obj, idx)
                            .map(|cell| cell.borrow().clone())
                            .unwrap_or(Value::Undefined);
                        copied.push(value);
                    }
                    init_values = Some(copied);

                    let buffer = new_gc_cell_ptr(
                        mc,
                        JSArrayBuffer {
                            data: Arc::new(Mutex::new(vec![0; src_length * element_size])),
                            ..JSArrayBuffer::default()
                        },
                    );
                    (buffer, 0, src_length)
                }
            }
            _ => return Err(raise_eval_error!("Invalid TypedArray constructor argument")),
        }
    } else if args.len() == 2 {
        // new TypedArray(buffer, byteOffset)
        let buffer_val = args[0].clone();
        let offset_val = args[1].clone();

        if let Value::Object(obj) = buffer_val {
            if let Some(ab_val) = slot_get_chained(&obj, &InternalSlot::ArrayBuffer) {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    if let Value::Number(offset_num) = offset_val {
                        let offset = offset_num as usize;
                        if !offset.is_multiple_of(element_size) {
                            return Err(raise_eval_error!("TypedArray byteOffset must be multiple of element size"));
                        }
                        let remaining_bytes = (**ab).borrow().data.lock().unwrap().len() - offset;
                        let length = remaining_bytes / element_size;
                        buffer_obj_opt = Some(obj);
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
            if let Some(ab_val) = slot_get_chained(&obj, &InternalSlot::ArrayBuffer) {
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
                        buffer_obj_opt = Some(obj);
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
        let proto_candidate = match &*proto_val.borrow() {
            Value::Object(proto_obj) => Some(*proto_obj),
            Value::Property { value: Some(v), .. } => match &*v.borrow() {
                Value::Object(proto_obj) => Some(*proto_obj),
                _ => None,
            },
            _ => None,
        };

        if let Some(proto_obj) = proto_candidate {
            obj.borrow_mut(mc).prototype = Some(proto_obj);
            slot_set(mc, &obj, InternalSlot::Proto, &Value::Object(proto_obj));
        } else {
            // Fallback: create new prototype (legacy behavior, though incorrect for identity)
            let proto = make_typedarray_prototype(mc, env, kind.clone(), None)?;
            obj.borrow_mut(mc).prototype = Some(proto);
            slot_set(mc, &obj, InternalSlot::Proto, &Value::Object(proto));
        }
    } else {
        // Fallback
        let proto = make_typedarray_prototype(mc, env, kind.clone(), None)?;
        obj.borrow_mut(mc).prototype = Some(proto);
        slot_set(mc, &obj, InternalSlot::Proto, &Value::Object(proto));
    }

    // Determine if this TypedArray should be length-tracking (no explicit length argument)
    let length_tracking = match args.len() {
        1 => match &args[0] {
            Value::Object(obj) => slot_get_chained(obj, &InternalSlot::ArrayBuffer).is_some(),
            _ => false,
        },
        2 => match &args[0] {
            Value::Object(obj) => slot_get_chained(obj, &InternalSlot::ArrayBuffer).is_some(),
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

    slot_set(mc, &obj, InternalSlot::TypedArray, &Value::TypedArray(typed_array));

    // Store a proper ArrayBuffer wrapper object so `ta.buffer` returns a spec-compliant object.
    let buf_wrapper = if let Some(existing_buf_obj) = buffer_obj_opt {
        existing_buf_obj
    } else {
        // Create a new wrapper object for the internally-created buffer.
        let buf_obj = new_js_object_data(mc);
        // Set prototype to ArrayBuffer.prototype if available.
        if let Some(ab_ctor_val) = crate::core::env_get(env, "ArrayBuffer")
            && let Value::Object(ab_ctor) = &*ab_ctor_val.borrow()
            && let Some(proto_val) = object_get_key_value(ab_ctor, "prototype")
            && let Value::Object(proto) = &*proto_val.borrow()
        {
            buf_obj.borrow_mut(mc).prototype = Some(*proto);
        }
        slot_set(mc, &buf_obj, InternalSlot::ArrayBuffer, &Value::ArrayBuffer(buffer));
        buf_obj
    };
    slot_set(mc, &obj, InternalSlot::BufferObject, &Value::Object(buf_wrapper));

    log::debug!(
        "created typedarray instance: obj={:p} kind={:?} length_tracking={}",
        &*obj.borrow(),
        kind,
        length_tracking
    );

    if let Some(values) = init_values {
        for (idx, v) in values.iter().enumerate() {
            if idx >= length {
                break;
            }

            if matches!(kind, TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64) {
                let big_value = match v {
                    Value::BigInt(b) => b.to_i64().unwrap_or(0) as f64,
                    Value::Number(n) => *n,
                    Value::Boolean(b) => {
                        if *b {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    Value::Null => 0.0,
                    Value::Undefined => f64::NAN,
                    _ => f64::NAN,
                };
                typed_array.set(mc, idx, big_value)?;
                continue;
            }

            let num = match v {
                Value::Number(n) => *n,
                Value::Boolean(b) => {
                    if *b {
                        1.0
                    } else {
                        0.0
                    }
                }
                Value::Null => 0.0,
                Value::Undefined => f64::NAN,
                Value::BigInt(b) => b.to_f64().unwrap_or(f64::NAN),
                Value::String(s) => {
                    let text = crate::unicode::utf16_to_utf8(s);
                    text.parse::<f64>().unwrap_or(f64::NAN)
                }
                _ => f64::NAN,
            };
            typed_array.set(mc, idx, num)?;
        }
    }

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
    let dv_val = slot_get_chained(object, &InternalSlot::DataView);
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
        "buffer" => {
            if let Some(buffer_obj) = slot_get_chained(object, &InternalSlot::BufferObject)
                && let Value::Object(obj) = &*buffer_obj.borrow()
            {
                Ok(Value::Object(*obj))
            } else {
                Ok(Value::ArrayBuffer(data_view_rc.buffer))
            }
        }
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

/// JavaScript ToInt32: modular conversion from f64 to i32.
/// Handles NaN, Infinity, and wraps via mod 2^32.
pub(crate) fn js_to_int32(val: f64) -> i32 {
    if val.is_nan() || val.is_infinite() || val == 0.0 {
        return 0;
    }
    let n = val.trunc();
    let two32: f64 = 4294967296.0; // 2^32
    let mut int32bit = n % two32;
    if int32bit < 0.0 {
        int32bit += two32;
    }
    // int32bit is in [0, 2^32)
    let int32bit = int32bit as u32;
    int32bit as i32
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
            TypedArrayKind::BigInt64 => {
                let mut b = [0u8; 8];
                b.copy_from_slice(&data[byte_offset..byte_offset + 8]);
                Ok(i64::from_le_bytes(b) as f64)
            }
            TypedArrayKind::BigUint64 => {
                let mut b = [0u8; 8];
                b.copy_from_slice(&data[byte_offset..byte_offset + 8]);
                Ok(u64::from_le_bytes(b) as f64)
            }
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
                let b = (js_to_int32(val) as i8).to_le_bytes();
                data[byte_offset] = b[0];
            }
            TypedArrayKind::Uint8 => {
                let b = (js_to_int32(val) as u8).to_le_bytes();
                data[byte_offset] = b[0];
            }
            TypedArrayKind::Uint8Clamped => {
                // Uint8ClampedArray clamps to [0, 255]
                #[allow(clippy::if_same_then_else)]
                let v = if val.is_nan() {
                    0u8
                } else if val <= 0.0 {
                    0u8
                } else if val >= 255.0 {
                    255u8
                } else {
                    val.round() as u8
                };
                data[byte_offset] = v;
            }
            TypedArrayKind::Int16 => {
                let b = (js_to_int32(val) as i16).to_le_bytes();
                data[byte_offset] = b[0];
                data[byte_offset + 1] = b[1];
            }
            TypedArrayKind::Uint16 => {
                let b = (js_to_int32(val) as u16).to_le_bytes();
                data[byte_offset] = b[0];
                data[byte_offset + 1] = b[1];
            }
            TypedArrayKind::Int32 => {
                let b = js_to_int32(val).to_le_bytes();
                data[byte_offset..byte_offset + 4].copy_from_slice(&b);
            }
            TypedArrayKind::Uint32 => {
                let b = (js_to_int32(val) as u32).to_le_bytes();
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
            TypedArrayKind::BigInt64 => {
                let b = (val as i64).to_le_bytes();
                data[byte_offset..byte_offset + 8].copy_from_slice(&b);
            }
            TypedArrayKind::BigUint64 => {
                let b = (val as u64).to_le_bytes();
                data[byte_offset..byte_offset + 8].copy_from_slice(&b);
            }
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
            if let Some(ab_val) = slot_get_chained(object, &InternalSlot::ArrayBuffer) {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    let len = (**ab).borrow().data.lock().unwrap().len();
                    Ok(Value::Number(len as f64))
                } else {
                    Err(raise_type_error!(
                        "Method ArrayBuffer.prototype.byteLength called on incompatible receiver"
                    ))
                }
            } else {
                Err(raise_type_error!(
                    "Method ArrayBuffer.prototype.byteLength called on incompatible receiver"
                ))
            }
        }
        "maxByteLength" => {
            if let Some(ab_val) = slot_get_chained(object, &InternalSlot::ArrayBuffer) {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    let b = (**ab).borrow();
                    let len = b.data.lock().unwrap().len();
                    Ok(Value::Number(b.max_byte_length.unwrap_or(len) as f64))
                } else {
                    Err(raise_type_error!(
                        "Method ArrayBuffer.prototype.maxByteLength called on incompatible receiver"
                    ))
                }
            } else {
                Err(raise_type_error!(
                    "Method ArrayBuffer.prototype.maxByteLength called on incompatible receiver"
                ))
            }
        }
        "resizable" => {
            if let Some(ab_val) = slot_get_chained(object, &InternalSlot::ArrayBuffer) {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    Ok(Value::Boolean((**ab).borrow().max_byte_length.is_some()))
                } else {
                    Err(raise_type_error!(
                        "Method ArrayBuffer.prototype.resizable called on incompatible receiver"
                    ))
                }
            } else {
                Err(raise_type_error!(
                    "Method ArrayBuffer.prototype.resizable called on incompatible receiver"
                ))
            }
        }
        _ => Ok(Value::Undefined),
    }
}

pub fn handle_arraybuffer_method<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    object: &JSObjectDataPtr<'gc>,
    method: &str,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, JSError> {
    match method {
        "slice" => {
            if let Some(ab_val) = slot_get_chained(object, &InternalSlot::ArrayBuffer)
                && let Value::ArrayBuffer(ab) = &*ab_val.borrow()
            {
                let ctor_val = crate::core::get_property_with_accessors(mc, env, object, "constructor").map_err(JSError::from)?;
                if !matches!(ctor_val, Value::Undefined | Value::Object(_)) {
                    return Err(raise_type_error!("constructor value is not an object"));
                }

                let data = (**ab).borrow().data.lock().unwrap().clone();
                let len = data.len() as i64;

                let to_integer_or_infinity = |v: Option<&Value<'gc>>, default: i64| -> Result<i64, JSError> {
                    let raw = match v {
                        None | Some(Value::Undefined) => default as f64,
                        Some(Value::Object(_)) => {
                            let prim = crate::core::to_primitive(mc, v.unwrap(), "number", env).map_err(JSError::from)?;
                            crate::core::to_number(&prim).map_err(JSError::from)?
                        }
                        Some(other) => crate::core::to_number(other).map_err(JSError::from)?,
                    };

                    let int = if raw.is_nan() || raw == 0.0 {
                        0
                    } else if !raw.is_finite() {
                        if raw.is_sign_negative() { i64::MIN } else { i64::MAX }
                    } else {
                        raw.trunc() as i64
                    };
                    Ok(int)
                };

                let start_raw = to_integer_or_infinity(args.first(), 0)?;
                let end_raw = to_integer_or_infinity(args.get(1), len)?;

                let start = if start_raw < 0 {
                    (len + start_raw).max(0)
                } else {
                    start_raw.min(len)
                };
                let end = if end_raw < 0 { (len + end_raw).max(0) } else { end_raw.min(len) };
                let final_end = end.max(start);
                let start_usize = start as usize;
                let end_usize = final_end as usize;
                let slice_bytes = data[start_usize..end_usize].to_vec();

                let new_ab = new_gc_cell_ptr(
                    mc,
                    JSArrayBuffer {
                        data: Arc::new(Mutex::new(slice_bytes)),
                        ..JSArrayBuffer::default()
                    },
                );

                let new_obj = new_js_object_data(mc);
                slot_set(mc, &new_obj, InternalSlot::ArrayBuffer, &Value::ArrayBuffer(new_ab));
                new_obj.borrow_mut(mc).prototype = object.borrow().prototype;

                return Ok(Value::Object(new_obj));
            }
            Err(raise_type_error!(
                "Method ArrayBuffer.prototype.slice called on incompatible receiver"
            ))
        }
        "resize" => {
            // Get the ArrayBuffer internal
            if let Some(ab_val) = slot_get_chained(object, &InternalSlot::ArrayBuffer) {
                let ab = match &*ab_val.borrow() {
                    Value::ArrayBuffer(ab) => *ab,
                    _ => {
                        return Err(raise_type_error!(
                            "Method ArrayBuffer.prototype.resize called on incompatible receiver"
                        ));
                    }
                };

                let max = { ab.borrow().max_byte_length };
                if let Some(max) = max {
                    if args.is_empty() {
                        return Err(raise_type_error!("resize requires a new length"));
                    }
                    let new_len_val = args[0].clone();
                    let prim = if let Value::Object(_) = &new_len_val {
                        crate::core::to_primitive(mc, &new_len_val, "number", env).map_err(JSError::from)?
                    } else {
                        new_len_val
                    };
                    let n = crate::core::to_number(&prim).map_err(JSError::from)?;
                    let integer = if n.is_nan() || n == 0.0 {
                        0.0
                    } else if !n.is_finite() {
                        n
                    } else {
                        n.trunc()
                    };

                    // Detached check must happen after coercion (per test expectations)
                    if ab.borrow().detached {
                        return Err(raise_type_error!("ArrayBuffer is detached"));
                    }

                    if integer < 0.0 {
                        return Err(raise_range_error!("new length must be a non-negative integer"));
                    }
                    if !integer.is_finite() || integer > (u32::MAX as f64) {
                        return Err(raise_range_error!("new length exceeds maximum"));
                    }

                    let new_len = integer as usize;
                    if new_len > max {
                        return Err(raise_range_error!("new length exceeds maximum"));
                    }
                    let ab_borrow = ab.borrow();
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
                Err(raise_type_error!(
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
    if let Some(ta_val) = slot_get_chained(object, &InternalSlot::TypedArray) {
        if let Value::TypedArray(ta) = &*ta_val.borrow() {
            match property {
                "buffer" => {
                    // Prefer the stored BufferObject wrapper (proper JS object)
                    if let Some(buf_obj_val) = slot_get_chained(object, &InternalSlot::BufferObject)
                        && let Value::Object(buf_obj) = &*buf_obj_val.borrow()
                    {
                        Ok(Value::Object(*buf_obj))
                    } else {
                        // Fallback to raw value (should not happen for properly constructed instances)
                        Ok(Value::ArrayBuffer(ta.buffer))
                    }
                }
                "byteLength" => {
                    let cur_len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        if buf_len <= ta.byte_offset {
                            0
                        } else {
                            (buf_len - ta.byte_offset) / ta.element_size()
                        }
                    } else {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0 } else { ta.length }
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
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if buf_len < needed { 0 } else { ta.length }
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
        if let Some(ta_cell) = slot_get_chained(obj, &InternalSlot::TypedArrayIterator)
            && let Value::TypedArray(ta) = &*ta_cell.borrow()
            && let Some(index_cell) = slot_get_chained(obj, &InternalSlot::Index)
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
                slot_set(mc, obj, InternalSlot::Index, &Value::Number((idx + 1) as f64));

                // Return { value, done: false }
                let result_obj = new_js_object_data(mc);
                object_set_key_value(mc, &result_obj, "value", &value)?;
                object_set_key_value(mc, &result_obj, "done", &Value::Boolean(false))?;
                Ok(Value::Object(result_obj))
            } else {
                // Done
                let result_obj = new_js_object_data(mc);
                object_set_key_value(mc, &result_obj, "value", &Value::Undefined)?;
                object_set_key_value(mc, &result_obj, "done", &Value::Boolean(true))?;
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
        if let Some(ta_cell) = slot_get_chained(obj, &InternalSlot::TypedArray)
            && let Value::TypedArray(_ta) = &*ta_cell.borrow()
        {
            match method {
                "values" | "keys" | "entries" => {
                    // Reuse the standard Array Iterator infrastructure so that the
                    // iterator gets the %ArrayIteratorPrototype% chain and the detach
                    // check inside handle_array_iterator_next fires correctly.
                    let kind = match method {
                        "keys" => "keys",
                        "entries" => "entries",
                        _ => "values",
                    };
                    Ok(crate::js_array::create_array_iterator(mc, _env, *obj, kind)?)
                }
                "fill" => {
                    let ta = *_ta;
                    let len = if ta.length_tracking {
                        let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                        (buf_len.saturating_sub(ta.byte_offset)) / ta.element_size()
                    } else {
                        ta.length
                    };
                    // Coerce fill value
                    let fill_val = _args.first().cloned().unwrap_or(Value::Undefined);
                    let fill_f64 = if is_bigint_typed_array(&ta.kind) {
                        match &fill_val {
                            Value::BigInt(b) => b.to_i64().unwrap_or(0) as f64,
                            _ => 0.0,
                        }
                    } else {
                        match &fill_val {
                            Value::Number(n) => *n,
                            Value::Undefined => f64::NAN,
                            _ => crate::core::to_number_with_env(mc, _env, &fill_val).unwrap_or(0.0),
                        }
                    };
                    // start/end
                    let start = if let Some(s) = _args.get(1) {
                        match s {
                            Value::Number(n) => {
                                let n = *n;
                                if n.is_nan() || n == 0.0 {
                                    0usize
                                } else if n < 0.0 {
                                    (len as i64 + n as i64).max(0) as usize
                                } else {
                                    (n as usize).min(len)
                                }
                            }
                            Value::Undefined => 0,
                            _ => 0,
                        }
                    } else {
                        0
                    };
                    let end = if let Some(e) = _args.get(2) {
                        match e {
                            Value::Number(n) => {
                                let n = *n;
                                if n.is_nan() || n == 0.0 {
                                    0usize
                                } else if n < 0.0 {
                                    (len as i64 + n as i64).max(0) as usize
                                } else {
                                    (n as usize).min(len)
                                }
                            }
                            Value::Undefined => len,
                            _ => len,
                        }
                    } else {
                        len
                    };
                    for i in start..end {
                        ta.set(mc, i, fill_f64)?;
                    }
                    Ok(Value::Object(*obj))
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
    let arraybuffer = make_arraybuffer_constructor(mc, env)?;
    crate::core::env_set(mc, env, "ArrayBuffer", &Value::Object(arraybuffer))?;

    let dataview = make_dataview_constructor(mc)?;
    crate::core::env_set(mc, env, "DataView", &Value::Object(dataview))?;

    let typed_arrays = make_typedarray_constructors(mc, env)?;
    for (name, ctor) in typed_arrays {
        crate::core::env_set(mc, env, &name, &Value::Object(ctor))?;
    }

    let atomics = make_atomics_object(mc, env)?;
    crate::core::env_set(mc, env, "Atomics", &Value::Object(atomics))?;

    let shared_ab = make_sharedarraybuffer_constructor(mc)?;
    crate::core::env_set(mc, env, "SharedArrayBuffer", &Value::Object(shared_ab))?;

    Ok(())
}
