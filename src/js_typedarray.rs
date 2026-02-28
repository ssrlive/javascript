use crate::core::{
    ClosureData, Gc, InternalSlot, MutationContext, get_property_with_accessors, js_error_to_value, new_gc_cell_ptr, slot_get,
    slot_get_chained, slot_set,
};
use crate::core::{JSObjectDataPtr, PropertyKey, Value, new_js_object_data, object_get_key_value, object_set_key_value};
use crate::js_array::is_array;
use crate::unicode::utf8_to_utf16;
use crate::{JSArrayBuffer, JSDataView, JSTypedArray, TypedArrayKind};
use crate::{JSError, core::EvalError};
use std::collections::HashMap;
use std::sync::Condvar;
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};

// Global waiters registry keyed by (buffer_arc_ptr, byte_index). Each waiter
// is an Arc containing a (Mutex<bool>, Condvar) pair the waiting thread blocks on.
#[allow(clippy::type_complexity)]
static WAITERS: LazyLock<Mutex<HashMap<(usize, usize), Vec<Arc<(Mutex<bool>, Condvar)>>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

// ═══════════════════════════════════════════════════════════════════════════════
// IEEE 754 binary16 (half-precision float) conversion helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert an f64 to IEEE 754 binary16 (half-precision) stored as u16.
/// Implements the spec's "SetValueInBuffer" for Float16: round to nearest, ties to even.
pub(crate) fn f64_to_f16(val: f64) -> u16 {
    let bits = val.to_bits();
    let sign = ((bits >> 63) & 1) as u16;
    let exp = ((bits >> 52) & 0x7FF) as i32;
    let frac = bits & 0x000F_FFFF_FFFF_FFFF;

    // NaN
    if exp == 0x7FF && frac != 0 {
        return (sign << 15) | 0x7E00; // quiet NaN
    }
    // Infinity
    if exp == 0x7FF {
        return (sign << 15) | 0x7C00;
    }

    // Unbias from f64 (bias 1023), rebias to f16 (bias 15)
    let unbiased = exp - 1023;

    // Too large → Infinity
    if unbiased > 15 {
        return (sign << 15) | 0x7C00;
    }

    // Normal range for f16: unbiased in [-14, 15]
    if unbiased >= -14 {
        // Normal f16 number
        let f16_exp = (unbiased + 15) as u16;
        // We have 52 fraction bits, need 10. Take top 10 and use bit 11 for rounding.
        let f16_frac = (frac >> 42) as u16; // top 10 bits
        let round_bit = (frac >> 41) & 1; // bit 11
        let sticky = frac & ((1u64 << 41) - 1); // remaining bits
        let mut result = (sign << 15) | (f16_exp << 10) | f16_frac;
        // Round to nearest, ties to even
        if round_bit != 0 && (sticky != 0 || (f16_frac & 1) != 0) {
            result += 1;
        }
        return result;
    }

    // Subnormal f16: unbiased < -14
    // The number is 1.frac * 2^unbiased, we need to represent as 0.xxx * 2^(-14)
    let shift = -14 - unbiased; // how many positions to shift right
    if shift > 24 {
        // Too small: rounds to zero
        return sign << 15;
    }
    // Add implicit leading 1 bit: significand = (1 << 10) | top-10-frac-bits
    // But we need more precision for rounding, so work with 11 extra bits
    let full_sig = (1u64 << 52) | frac; // 53-bit significand
    // We want 10 bits of mantissa after shifting right by (shift) positions from position 52
    // Total right shift from 52-bit position = 42 + shift
    let total_shift = 42 + shift as u64;
    let f16_frac = if total_shift >= 53 {
        0u16
    } else {
        (full_sig >> total_shift) as u16 & 0x3FF
    };
    let round_bit = if total_shift >= 54 || total_shift == 0 {
        0
    } else {
        (full_sig >> (total_shift - 1)) & 1
    };
    let sticky_mask = if total_shift <= 1 { 0 } else { (1u64 << (total_shift - 1)) - 1 };
    let sticky = full_sig & sticky_mask;
    let mut result = (sign << 15) | f16_frac;
    if round_bit != 0 && (sticky != 0 || (f16_frac & 1) != 0) {
        result += 1;
    }
    result
}

/// Convert an IEEE 754 binary16 (half-precision) u16 to f64.
pub(crate) fn f16_to_f64(bits: u16) -> f64 {
    let sign = ((bits >> 15) & 1) as u64;
    let exp = ((bits >> 10) & 0x1F) as u64;
    let frac = (bits & 0x3FF) as u64;

    if exp == 0x1F {
        // Infinity or NaN
        if frac == 0 {
            return f64::from_bits((sign << 63) | (0x7FFu64 << 52));
        } else {
            // NaN: preserve quiet bit and payload
            return f64::from_bits((sign << 63) | (0x7FFu64 << 52) | (frac << 42));
        }
    }

    if exp == 0 {
        if frac == 0 {
            // ±0
            return f64::from_bits(sign << 63);
        }
        // Subnormal: value = (-1)^sign * 2^(-14) * (frac / 1024)
        // Normalize: find leading 1 bit in frac (10-bit)
        let mut f = frac;
        let mut e = -14i64 + 1023; // start with f16 subnormal exponent rebiased
        // Shift frac left until the leading bit is at position 10
        while f & 0x400 == 0 {
            f <<= 1;
            e -= 1;
        }
        f &= 0x3FF; // remove the implicit leading 1
        return f64::from_bits((sign << 63) | ((e as u64) << 52) | (f << 42));
    }

    // Normal number: rebias exponent from f16 (bias 15) to f64 (bias 1023)
    let f64_exp = (exp as i64 - 15 + 1023) as u64;
    f64::from_bits((sign << 63) | (f64_exp << 52) | (frac << 42))
}

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

    // ArrayBuffer[Symbol.species] — accessor getter returning `this`, non-enumerable, configurable
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_val.borrow()
        && let Some(species_sym_val) = object_get_key_value(sym_obj, "species")
        && let Value::Symbol(species_sym) = &*species_sym_val.borrow()
    {
        let getter_fn = new_js_object_data(mc);
        if let Some(func_ctor_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*proto_val.borrow()
        {
            getter_fn.borrow_mut(mc).prototype = Some(*func_proto);
        }
        let getter_closure = ClosureData {
            env: Some(*env),
            native_target: Some("ArrayBuffer.species".to_string()),
            enforce_strictness_inheritance: true,
            ..ClosureData::default()
        };
        getter_fn
            .borrow_mut(mc)
            .set_closure(Some(new_gc_cell_ptr(mc, Value::Closure(Gc::new(mc, getter_closure)))));
        let gname_desc =
            crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("get [Symbol.species]")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &getter_fn, "name", &gname_desc)?;
        let glen_desc = crate::core::create_descriptor_object(mc, &Value::Number(0.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &getter_fn, "length", &glen_desc)?;

        let species_desc_obj = new_js_object_data(mc);
        object_set_key_value(mc, &species_desc_obj, "get", &Value::Object(getter_fn))?;
        object_set_key_value(mc, &species_desc_obj, "enumerable", &Value::Boolean(false))?;
        object_set_key_value(mc, &species_desc_obj, "configurable", &Value::Boolean(true))?;
        crate::js_object::define_property_internal(mc, &obj, PropertyKey::Symbol(*species_sym), &species_desc_obj)?;
    }

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

/// Convert a num_bigint::BigInt to i64 using modular arithmetic (mod 2^64).
/// This preserves the low 64 bits, correctly implementing ToBigInt64/ToBigUint64.
fn bigint_to_i64_modular(b: &num_bigint::BigInt) -> i64 {
    let (sign, bytes) = b.to_bytes_le();
    let mut raw = [0u8; 8];
    let len = bytes.len().min(8);
    raw[..len].copy_from_slice(&bytes[..len]);
    let unsigned = u64::from_le_bytes(raw);
    if sign == num_bigint::Sign::Minus {
        // Two's complement: negate the absolute value in 64-bit
        0u64.wrapping_sub(unsigned) as i64
    } else {
        unsigned as i64
    }
}

/// ToBigInt coercion: convert a Value to BigInt per spec.
/// Handles BigInt, Boolean, String, and Object (via ToPrimitive).
/// Returns the value as i64 using modular arithmetic for BigInt types.
pub fn to_bigint_i64<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, val: &Value<'gc>) -> Result<i64, EvalError<'gc>> {
    let prim = match val {
        Value::Object(_) => crate::core::to_primitive(mc, val, "number", env)?,
        other => other.clone(),
    };
    match &prim {
        Value::BigInt(b) => Ok(bigint_to_i64_modular(b)),
        Value::Boolean(b) => Ok(if *b { 1 } else { 0 }),
        Value::String(s) => {
            let s_str = crate::unicode::utf16_to_utf8(s);
            // Use parse_bigint_string for proper StringToBigInt (throws SyntaxError)
            match crate::js_bigint::parse_bigint_string(&s_str) {
                Ok(bi) => Ok(bigint_to_i64_modular(&bi)),
                Err(_) => Err(throw_syntax_error(
                    mc,
                    env,
                    &format!("Cannot convert \"{}\" to a BigInt", s_str.trim()),
                )),
            }
        }
        Value::Number(_) => Err(throw_type_error(mc, env, "Cannot convert a Number value to a BigInt")),
        Value::Symbol(_) => Err(throw_type_error(mc, env, "Cannot convert a Symbol value to a BigInt")),
        Value::Undefined => Err(throw_type_error(mc, env, "Cannot convert undefined to a BigInt")),
        Value::Null => Err(throw_type_error(mc, env, "Cannot convert null to a BigInt")),
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
pub fn is_bigint_typed_array(kind: &TypedArrayKind) -> bool {
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

/// Throw a SyntaxError as EvalError::Throw using env's SyntaxError constructor.
fn throw_syntax_error<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, msg: &str) -> EvalError<'gc> {
    let js_err = raise_syntax_error!(msg);
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

/// Convert raw i64 bits to the correct BigInt value based on the TypedArray kind.
/// For BigInt64Array, the i64 is the signed value directly.
/// For BigUint64Array, the i64 bits are reinterpreted as u64 to produce an unsigned BigInt.
fn raw_i64_to_bigint<'gc>(raw: i64, kind: &TypedArrayKind) -> Value<'gc> {
    match kind {
        TypedArrayKind::BigUint64 => {
            let u = raw as u64;
            Value::BigInt(Box::new(num_bigint::BigInt::from(u)))
        }
        _ => Value::BigInt(Box::new(num_bigint::BigInt::from(raw))),
    }
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
            if is_bigint {
                let raw = ta_obj.get_bigint_raw(idx)?;
                Ok(raw_i64_to_bigint(raw, &ta_obj.kind))
            } else {
                let v = ta_obj.get(idx)?;
                Ok(Value::Number(v))
            }
        }
        "store" => {
            let idx_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
            let val_arg = args.get(2).cloned().unwrap_or(Value::Undefined);
            // Per spec: coerce value BEFORE validating index
            if is_bigint {
                let v = to_bigint_i64(mc, env, &val_arg)?;
                let idx = validate_atomic_access(mc, env, &ta_obj, &idx_arg)?;
                ta_obj.set_bigint(mc, idx, v)?;
                // Atomics.store returns the coerced value (ToBigInt result)
                Ok(Value::BigInt(Box::new(num_bigint::BigInt::from(v))))
            } else {
                let n = crate::core::to_number_with_env(mc, env, &val_arg)?;
                let int_n = if n.is_nan() || n == 0.0 { 0.0 } else { n.trunc() };
                let int_n = if int_n == 0.0 { 0.0 } else { int_n };
                let idx = validate_atomic_access(mc, env, &ta_obj, &idx_arg)?;
                ta_obj.set(mc, idx, n)?;
                Ok(Value::Number(int_n))
            }
        }
        "compareExchange" => {
            let idx_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
            let expected_arg = args.get(2).cloned().unwrap_or(Value::Undefined);
            let replacement_arg = args.get(3).cloned().unwrap_or(Value::Undefined);
            // Spec order: ValidateAtomicAccess THEN coerce values
            let idx = validate_atomic_access(mc, env, &ta_obj, &idx_arg)?;
            if is_bigint {
                let expected_i64 = to_bigint_i64(mc, env, &expected_arg)?;
                let replacement_i64 = to_bigint_i64(mc, env, &replacement_arg)?;
                let old_raw = ta_obj.get_bigint_raw(idx)?;
                // Compare raw i64 bits (works for both signed and unsigned)
                if old_raw == expected_i64 {
                    ta_obj.set_bigint(mc, idx, replacement_i64)?;
                }
                Ok(raw_i64_to_bigint(old_raw, &ta_obj.kind))
            } else {
                let expected_f64 = crate::core::to_number_with_env(mc, env, &expected_arg)?;
                let replacement_f64 = crate::core::to_number_with_env(mc, env, &replacement_arg)?;
                let old = ta_obj.get(idx)?;
                let matches = match ta_obj.kind {
                    TypedArrayKind::Int8 => (js_to_int32(old) as i8) == (js_to_int32(expected_f64) as i8),
                    TypedArrayKind::Uint8 => (js_to_int32(old) as u8) == (js_to_int32(expected_f64) as u8),
                    TypedArrayKind::Int16 => (js_to_int32(old) as i16) == (js_to_int32(expected_f64) as i16),
                    TypedArrayKind::Uint16 => (js_to_int32(old) as u16) == (js_to_int32(expected_f64) as u16),
                    TypedArrayKind::Int32 => js_to_int32(old) == js_to_int32(expected_f64),
                    TypedArrayKind::Uint32 => (js_to_int32(old) as u32) == (js_to_int32(expected_f64) as u32),
                    _ => old == expected_f64,
                };
                if matches {
                    ta_obj.set(mc, idx, replacement_f64)?;
                }
                Ok(Value::Number(old))
            }
        }
        "add" | "sub" | "and" | "or" | "xor" | "exchange" => {
            let idx_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
            let val_arg = args.get(2).cloned().unwrap_or(Value::Undefined);
            // Spec order: ValidateAtomicAccess THEN coerce value
            let idx = validate_atomic_access(mc, env, &ta_obj, &idx_arg)?;
            if is_bigint {
                let operand = to_bigint_i64(mc, env, &val_arg)?;
                let old_raw = ta_obj.get_bigint_raw(idx)?;
                let new_raw = match method {
                    "add" => old_raw.wrapping_add(operand),
                    "sub" => old_raw.wrapping_sub(operand),
                    "and" => old_raw & operand,
                    "or" => old_raw | operand,
                    "xor" => old_raw ^ operand,
                    "exchange" => operand,
                    _ => old_raw,
                };
                ta_obj.set_bigint(mc, idx, new_raw)?;
                Ok(raw_i64_to_bigint(old_raw, &ta_obj.kind))
            } else {
                let operand = crate::core::to_number_with_env(mc, env, &val_arg)? as i64;
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
pub fn make_sharedarraybuffer_constructor<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let obj = new_js_object_data(mc);

    // Set [[Prototype]] to Function.prototype
    if let Some(func_ctor_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*proto_val.borrow()
    {
        obj.borrow_mut(mc).prototype = Some(*func_proto);
    }

    let proto = make_sharedarraybuffer_prototype(mc, env, &obj)?;
    object_set_key_value(mc, &obj, "prototype", &Value::Object(proto))?;
    obj.borrow_mut(mc).set_non_writable("prototype");
    obj.borrow_mut(mc).set_non_enumerable("prototype");
    obj.borrow_mut(mc).set_non_configurable("prototype");

    object_set_key_value(mc, &obj, "name", &Value::String(utf8_to_utf16("SharedArrayBuffer")))?;
    obj.borrow_mut(mc).set_non_writable("name");
    obj.borrow_mut(mc).set_non_enumerable("name");
    object_set_key_value(mc, &obj, "length", &Value::Number(1.0))?;
    obj.borrow_mut(mc).set_non_writable("length");
    obj.borrow_mut(mc).set_non_enumerable("length");

    // Mark as SharedArrayBuffer constructor
    slot_set(mc, &obj, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &obj, InternalSlot::ArrayBuffer, &Value::Boolean(true));
    slot_set(mc, &obj, InternalSlot::SharedArrayBuffer, &Value::Boolean(true));
    slot_set(
        mc,
        &obj,
        InternalSlot::NativeCtor,
        &Value::String(utf8_to_utf16("SharedArrayBuffer")),
    );

    // SharedArrayBuffer[Symbol.species] — accessor getter returning `this`
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_val.borrow()
        && let Some(species_sym_val) = object_get_key_value(sym_obj, "species")
        && let Value::Symbol(species_sym) = &*species_sym_val.borrow()
    {
        let getter_fn = new_js_object_data(mc);
        if let Some(func_ctor_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*proto_val.borrow()
        {
            getter_fn.borrow_mut(mc).prototype = Some(*func_proto);
        }
        let getter_closure = ClosureData {
            env: Some(*env),
            native_target: Some("SharedArrayBuffer.species".to_string()),
            enforce_strictness_inheritance: true,
            ..ClosureData::default()
        };
        getter_fn
            .borrow_mut(mc)
            .set_closure(Some(new_gc_cell_ptr(mc, Value::Closure(Gc::new(mc, getter_closure)))));
        let gname_desc =
            crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("get [Symbol.species]")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &getter_fn, "name", &gname_desc)?;
        let glen_desc = crate::core::create_descriptor_object(mc, &Value::Number(0.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &getter_fn, "length", &glen_desc)?;

        let species_desc_obj = new_js_object_data(mc);
        object_set_key_value(mc, &species_desc_obj, "get", &Value::Object(getter_fn))?;
        object_set_key_value(mc, &species_desc_obj, "enumerable", &Value::Boolean(false))?;
        object_set_key_value(mc, &species_desc_obj, "configurable", &Value::Boolean(true))?;
        crate::js_object::define_property_internal(mc, &obj, PropertyKey::Symbol(*species_sym), &species_desc_obj)?;
    }

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
pub fn make_sharedarraybuffer_prototype<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    ctor: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let proto = new_js_object_data(mc);

    // Set [[Prototype]] to Object.prototype
    if let Some(obj_ctor_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_ctor_val.borrow()
        && let Some(obj_proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(obj_proto) = &*obj_proto_val.borrow()
    {
        proto.borrow_mut(mc).prototype = Some(*obj_proto);
    }

    // constructor — actual constructor object reference
    object_set_key_value(mc, &proto, "constructor", &Value::Object(*ctor))?;
    proto.borrow_mut(mc).set_non_enumerable("constructor");

    // byteLength — accessor property with proper function-object getter
    {
        let getter_fn = new_js_object_data(mc);
        if let Some(func_ctor_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(fp_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*fp_val.borrow()
        {
            getter_fn.borrow_mut(mc).prototype = Some(*func_proto);
        }
        getter_fn.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(
            mc,
            Value::Function("SharedArrayBuffer.prototype.byteLength".to_string()),
        )));
        let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("get byteLength")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &getter_fn, "name", &name_desc)?;
        let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(0.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &getter_fn, "length", &len_desc)?;

        let bl_desc = new_js_object_data(mc);
        object_set_key_value(mc, &bl_desc, "get", &Value::Object(getter_fn))?;
        object_set_key_value(mc, &bl_desc, "enumerable", &Value::Boolean(false))?;
        object_set_key_value(mc, &bl_desc, "configurable", &Value::Boolean(true))?;
        crate::js_object::define_property_internal(mc, &proto, "byteLength", &bl_desc)?;
    }

    // slice — method function object
    {
        let slice_fn = new_js_object_data(mc);
        if let Some(func_ctor_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(fp_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*fp_val.borrow()
        {
            slice_fn.borrow_mut(mc).prototype = Some(*func_proto);
        }
        slice_fn.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(
            mc,
            Value::Function("SharedArrayBuffer.prototype.slice".to_string()),
        )));
        let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("slice")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &slice_fn, "name", &name_desc)?;
        let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(2.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &slice_fn, "length", &len_desc)?;

        let sl_desc = crate::core::create_descriptor_object(mc, &Value::Object(slice_fn), true, false, true)?;
        crate::js_object::define_property_internal(mc, &proto, "slice", &sl_desc)?;
    }

    // grow — method function object
    {
        let grow_fn = new_js_object_data(mc);
        if let Some(func_ctor_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(fp_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*fp_val.borrow()
        {
            grow_fn.borrow_mut(mc).prototype = Some(*func_proto);
        }
        grow_fn.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(
            mc,
            Value::Function("SharedArrayBuffer.prototype.grow".to_string()),
        )));
        let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("grow")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &grow_fn, "name", &name_desc)?;
        let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(1.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &grow_fn, "length", &len_desc)?;

        let gr_desc = crate::core::create_descriptor_object(mc, &Value::Object(grow_fn), true, false, true)?;
        crate::js_object::define_property_internal(mc, &proto, "grow", &gr_desc)?;
    }

    // maxByteLength — accessor property with getter
    {
        let getter_fn = new_js_object_data(mc);
        if let Some(func_ctor_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(fp_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*fp_val.borrow()
        {
            getter_fn.borrow_mut(mc).prototype = Some(*func_proto);
        }
        getter_fn.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(
            mc,
            Value::Function("SharedArrayBuffer.prototype.maxByteLength".to_string()),
        )));
        let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("get maxByteLength")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &getter_fn, "name", &name_desc)?;
        let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(0.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &getter_fn, "length", &len_desc)?;

        let mbl_desc = new_js_object_data(mc);
        object_set_key_value(mc, &mbl_desc, "get", &Value::Object(getter_fn))?;
        object_set_key_value(mc, &mbl_desc, "enumerable", &Value::Boolean(false))?;
        object_set_key_value(mc, &mbl_desc, "configurable", &Value::Boolean(true))?;
        crate::js_object::define_property_internal(mc, &proto, "maxByteLength", &mbl_desc)?;
    }

    // growable — accessor property with getter
    {
        let getter_fn = new_js_object_data(mc);
        if let Some(func_ctor_val) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(fp_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*fp_val.borrow()
        {
            getter_fn.borrow_mut(mc).prototype = Some(*func_proto);
        }
        getter_fn.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(
            mc,
            Value::Function("SharedArrayBuffer.prototype.growable".to_string()),
        )));
        let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("get growable")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &getter_fn, "name", &name_desc)?;
        let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(0.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &getter_fn, "length", &len_desc)?;

        let gr_desc = new_js_object_data(mc);
        object_set_key_value(mc, &gr_desc, "get", &Value::Object(getter_fn))?;
        object_set_key_value(mc, &gr_desc, "enumerable", &Value::Boolean(false))?;
        object_set_key_value(mc, &gr_desc, "configurable", &Value::Boolean(true))?;
        crate::js_object::define_property_internal(mc, &proto, "growable", &gr_desc)?;
    }

    // @@toStringTag = "SharedArrayBuffer"
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_val.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("SharedArrayBuffer")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &proto, PropertyKey::Symbol(*tag_sym), &desc)?;
    }

    Ok(proto)
}

/// Create a DataView constructor object
pub fn make_dataview_constructor<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let obj = new_js_object_data(mc);

    // Set [[Prototype]] to Function.prototype
    if let Some(func_ctor_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*proto_val.borrow()
    {
        obj.borrow_mut(mc).prototype = Some(*func_proto);
    }

    let proto = make_dataview_prototype(mc, env, &obj)?;
    object_set_key_value(mc, &obj, "prototype", &Value::Object(proto))?;
    obj.borrow_mut(mc).set_non_writable("prototype");
    obj.borrow_mut(mc).set_non_enumerable("prototype");
    obj.borrow_mut(mc).set_non_configurable("prototype");

    let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("DataView")), false, false, true)?;
    crate::js_object::define_property_internal(mc, &obj, "name", &name_desc)?;

    let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(1.0), false, false, true)?;
    crate::js_object::define_property_internal(mc, &obj, "length", &len_desc)?;

    // Mark as DataView constructor
    slot_set(mc, &obj, InternalSlot::DataView, &Value::Boolean(true));
    slot_set(mc, &obj, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("DataView")));
    slot_set(mc, &obj, InternalSlot::IsConstructor, &Value::Boolean(true));

    Ok(obj)
}

/// Create the DataView prototype
pub fn make_dataview_prototype<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    ctor: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let proto = new_js_object_data(mc);

    // Set [[Prototype]] to Object.prototype
    if let Some(obj_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(obj_proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(obj_proto) = &*obj_proto_val.borrow()
    {
        proto.borrow_mut(mc).prototype = Some(*obj_proto);
    }

    object_set_key_value(mc, &proto, "constructor", &Value::Object(*ctor))?;
    proto.borrow_mut(mc).set_non_enumerable("constructor");

    // Get Function.prototype for method function objects
    let func_proto_opt: Option<JSObjectDataPtr<'gc>> = if let Some(func_ctor_val) = crate::core::env_get(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*proto_val.borrow()
    {
        Some(*func_proto)
    } else {
        None
    };

    // Helper: create a getter function object with proper name and length
    let make_getter = |mc: &MutationContext<'gc>, prop_name: &str, dispatch_name: &str| -> Result<JSObjectDataPtr<'gc>, JSError> {
        let fn_obj = new_js_object_data(mc);
        fn_obj
            .borrow_mut(mc)
            .set_closure(Some(new_gc_cell_ptr(mc, Value::Function(dispatch_name.to_string()))));
        if let Some(fp) = func_proto_opt {
            fn_obj.borrow_mut(mc).prototype = Some(fp);
        }
        let name_str = format!("get {}", prop_name);
        let desc_name = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16(&name_str)), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fn_obj, "name", &desc_name)?;
        let desc_len = crate::core::create_descriptor_object(mc, &Value::Number(0.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fn_obj, "length", &desc_len)?;
        Ok(fn_obj)
    };

    // Accessor properties: buffer, byteLength, byteOffset
    for &(prop_name, dispatch_name) in &[
        ("buffer", "DataView.prototype.buffer"),
        ("byteLength", "DataView.prototype.byteLength"),
        ("byteOffset", "DataView.prototype.byteOffset"),
    ] {
        let getter_obj = make_getter(mc, prop_name, dispatch_name)?;
        object_set_key_value(
            mc,
            &proto,
            prop_name,
            &Value::Property {
                value: None,
                getter: Some(Box::new(Value::Object(getter_obj))),
                setter: None,
            },
        )?;
        proto.borrow_mut(mc).set_non_enumerable(prop_name);
    }

    // DataView getter/setter methods — all non-enumerable per spec
    // Create proper function objects with name/length properties (like Atomics pattern)
    let methods: &[(&str, usize)] = &[
        ("getInt8", 1),
        ("getUint8", 1),
        ("getInt16", 1),
        ("getUint16", 1),
        ("getInt32", 1),
        ("getUint32", 1),
        ("getFloat32", 1),
        ("getFloat64", 1),
        ("getFloat16", 1),
        ("getBigInt64", 1),
        ("getBigUint64", 1),
        ("setInt8", 2),
        ("setUint8", 2),
        ("setInt16", 2),
        ("setUint16", 2),
        ("setInt32", 2),
        ("setUint32", 2),
        ("setFloat32", 2),
        ("setFloat64", 2),
        ("setFloat16", 2),
        ("setBigInt64", 2),
        ("setBigUint64", 2),
    ];

    for &(method_name, length) in methods {
        let fn_obj = new_js_object_data(mc);
        let dispatch_name = format!("DataView.prototype.{method_name}");
        fn_obj
            .borrow_mut(mc)
            .set_closure(Some(new_gc_cell_ptr(mc, Value::Function(dispatch_name))));
        if let Some(fp) = func_proto_opt {
            fn_obj.borrow_mut(mc).prototype = Some(fp);
        }
        let desc_name = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16(method_name)), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fn_obj, "name", &desc_name)?;
        let desc_len = crate::core::create_descriptor_object(mc, &Value::Number(length as f64), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fn_obj, "length", &desc_len)?;
        // writable: true, enumerable: false, configurable: true
        let desc_method = crate::core::create_descriptor_object(mc, &Value::Object(fn_obj), true, false, true)?;
        crate::js_object::define_property_internal(mc, &proto, method_name, &desc_method)?;
    }

    // Symbol.toStringTag = "DataView" (non-writable, non-enumerable, configurable)
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_val.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_ctor, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let tag_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("DataView")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &proto, crate::core::PropertyKey::Symbol(*tag_sym), &tag_desc)?;
    }

    Ok(proto)
}

/// Create TypedArray constructors
/// Create the abstract %TypedArray% intrinsic constructor and %TypedArray%.prototype.
/// Per spec, %TypedArray% is not exposed as a global but is the [[Prototype]] of all
/// concrete TypedArray constructors (Int8Array, Uint8Array, etc).
pub fn make_typedarray_intrinsic<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(JSObjectDataPtr<'gc>, JSObjectDataPtr<'gc>), JSError> {
    let ta_ctor = new_js_object_data(mc);

    // Get Function.prototype once for reuse
    let func_proto_opt: Option<JSObjectDataPtr<'gc>> = if let Some(func_ctor_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*proto_val.borrow()
    {
        Some(*func_proto)
    } else {
        None
    };

    // %TypedArray%.[[Prototype]] = Function.prototype
    if let Some(fp) = func_proto_opt {
        ta_ctor.borrow_mut(mc).prototype = Some(fp);
    }

    // name and length
    let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("TypedArray")), false, false, true)?;
    crate::js_object::define_property_internal(mc, &ta_ctor, "name", &name_desc)?;
    let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(0.0), false, false, true)?;
    crate::js_object::define_property_internal(mc, &ta_ctor, "length", &len_desc)?;

    // Mark as constructor (abstract — cannot be called directly without new_target)
    slot_set(mc, &ta_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("TypedArray")));
    slot_set(mc, &ta_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));

    // --- %TypedArray%.prototype ---
    let ta_proto = new_js_object_data(mc);

    // %TypedArray%.prototype.[[Prototype]] = Object.prototype
    if let Some(obj_val) = crate::core::env_get(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(obj_proto) = &*proto_val.borrow()
    {
        ta_proto.borrow_mut(mc).prototype = Some(*obj_proto);
    }

    // constructor property
    object_set_key_value(mc, &ta_proto, "constructor", &Value::Object(ta_ctor))?;
    ta_proto.borrow_mut(mc).set_non_enumerable("constructor");

    // Helper: create a function object with closure-based dispatch
    let make_fn =
        |mc: &MutationContext<'gc>, display_name: &str, dispatch_name: &str, arity: f64| -> Result<JSObjectDataPtr<'gc>, JSError> {
            let fn_obj = new_js_object_data(mc);
            fn_obj
                .borrow_mut(mc)
                .set_closure(Some(new_gc_cell_ptr(mc, Value::Function(dispatch_name.to_string()))));
            if let Some(fp) = func_proto_opt {
                fn_obj.borrow_mut(mc).prototype = Some(fp);
            }
            let name_d = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16(display_name)), false, false, true)?;
            crate::js_object::define_property_internal(mc, &fn_obj, "name", &name_d)?;
            let len_d = crate::core::create_descriptor_object(mc, &Value::Number(arity), false, false, true)?;
            crate::js_object::define_property_internal(mc, &fn_obj, "length", &len_d)?;
            Ok(fn_obj)
        };

    // --- Shared accessor properties: buffer, byteLength, byteOffset, length ---
    let accessor_names = ["buffer", "byteLength", "byteOffset", "length"];
    for acc_name in &accessor_names {
        let fn_name = format!("get {}", acc_name);
        let dispatch_name = format!("TypedArray.prototype.{}", acc_name);
        let getter_fn = make_fn(mc, &fn_name, &dispatch_name, 0.0)?;

        object_set_key_value(
            mc,
            &ta_proto,
            *acc_name,
            &Value::Property {
                value: None,
                getter: Some(Box::new(Value::Object(getter_fn))),
                setter: None,
            },
        )?;
        ta_proto.borrow_mut(mc).set_non_enumerable(*acc_name);
    }

    // --- Shared methods ---
    let methods: &[(&str, i32)] = &[
        ("set", 1),
        ("subarray", 2),
        ("values", 0),
        ("keys", 0),
        ("entries", 0),
        ("fill", 1),
        ("copyWithin", 2),
        ("every", 1),
        ("filter", 1),
        ("find", 1),
        ("findIndex", 1),
        ("forEach", 1),
        ("includes", 1),
        ("indexOf", 1),
        ("join", 1),
        ("lastIndexOf", 1),
        ("map", 1),
        ("reduce", 1),
        ("reduceRight", 1),
        ("reverse", 0),
        ("slice", 2),
        ("some", 1),
        ("sort", 1),
        ("toLocaleString", 0),
        ("at", 1),
        ("findLast", 1),
        ("findLastIndex", 1),
        ("toReversed", 0),
        ("toSorted", 1),
        ("with", 2),
        ("flat", 0),
        ("flatMap", 1),
    ];
    for (method_name, arity) in methods {
        let dispatch_name = format!("TypedArray.prototype.{}", method_name);
        let method_fn = make_fn(mc, method_name, &dispatch_name, *arity as f64)?;
        let desc = crate::core::create_descriptor_object(mc, &Value::Object(method_fn), true, false, true)?;
        crate::js_object::define_property_internal(mc, &ta_proto, *method_name, &desc)?;
    }

    // toString: share the SAME function object as Array.prototype.toString
    if let Some(arr_ctor_val) = object_get_key_value(env, "Array")
        && let Value::Object(arr_ctor) = &*arr_ctor_val.borrow()
        && let Some(arr_proto_val) = object_get_key_value(arr_ctor, "prototype")
        && let Value::Object(arr_proto) = &*arr_proto_val.borrow()
        && let Some(arr_ts_val) = object_get_key_value(arr_proto, "toString")
    {
        // Unwrap property descriptor if needed to get the raw function
        let ts_fn = match &*arr_ts_val.borrow() {
            Value::Property { value: Some(inner), .. } => inner.borrow().clone(),
            other => other.clone(),
        };
        let desc = crate::core::create_descriptor_object(mc, &ts_fn, true, false, true)?;
        crate::js_object::define_property_internal(mc, &ta_proto, "toString", &desc)?;
    }

    // Symbol.iterator = values (same function object, writable, non-enumerable, configurable)
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_val.borrow()
        && let Some(iter_sym_val) = object_get_key_value(sym_ctor, "iterator")
        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
    {
        // Get the raw values function from the property descriptor
        if let Some(values_val) = object_get_key_value(&ta_proto, "values") {
            let values_fn = match &*values_val.borrow() {
                Value::Property { value: Some(inner), .. } => inner.borrow().clone(),
                other => other.clone(),
            };
            let desc = crate::core::create_descriptor_object(mc, &values_fn, true, false, true)?;
            crate::js_object::define_property_internal(mc, &ta_proto, PropertyKey::Symbol(*iter_sym), &desc)?;
        }
    }

    // Symbol.toStringTag accessor getter (non-enumerable, configurable, no setter)
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_val.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_ctor, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let getter_fn = make_fn(mc, "get [Symbol.toStringTag]", "TypedArray.prototype.toStringTag", 0.0)?;

        // Create accessor descriptor via define_property_internal
        let desc_obj = new_js_object_data(mc);
        object_set_key_value(mc, &desc_obj, "get", &Value::Object(getter_fn))?;
        object_set_key_value(mc, &desc_obj, "set", &Value::Undefined)?;
        object_set_key_value(mc, &desc_obj, "enumerable", &Value::Boolean(false))?;
        object_set_key_value(mc, &desc_obj, "configurable", &Value::Boolean(true))?;
        crate::js_object::define_property_internal(mc, &ta_proto, PropertyKey::Symbol(*tag_sym), &desc_obj)?;
    }

    // Wire prototype property on constructor
    object_set_key_value(mc, &ta_ctor, "prototype", &Value::Object(ta_proto))?;
    ta_ctor.borrow_mut(mc).set_non_writable("prototype");
    ta_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    ta_ctor.borrow_mut(mc).set_non_configurable("prototype");

    // --- Static methods on %TypedArray%: from, of ---
    let statics: &[(&str, i32)] = &[("from", 1), ("of", 0)];
    for (sname, arity) in statics {
        let dispatch_name = format!("TypedArray.{}", sname);
        let sfn = make_fn(mc, sname, &dispatch_name, *arity as f64)?;
        let desc = crate::core::create_descriptor_object(mc, &Value::Object(sfn), true, false, true)?;
        crate::js_object::define_property_internal(mc, &ta_ctor, *sname, &desc)?;
    }

    // Symbol.species accessor — get [Symbol.species]() { return this; }
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_val.borrow()
        && let Some(species_sym_val) = object_get_key_value(sym_ctor, "species")
        && let Value::Symbol(species_sym) = &*species_sym_val.borrow()
    {
        let getter_fn = make_fn(mc, "get [Symbol.species]", "TypedArray.species", 0.0)?;

        object_set_key_value(
            mc,
            &ta_ctor,
            PropertyKey::Symbol(*species_sym),
            &Value::Property {
                value: None,
                getter: Some(Box::new(Value::Object(getter_fn))),
                setter: None,
            },
        )?;
    }

    Ok((ta_ctor, ta_proto))
}

pub fn make_typedarray_constructors<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    ta_intrinsic: &JSObjectDataPtr<'gc>,
    ta_proto_intrinsic: &JSObjectDataPtr<'gc>,
) -> Result<Vec<(String, JSObjectDataPtr<'gc>)>, JSError> {
    let kinds = vec![
        ("Int8Array", TypedArrayKind::Int8),
        ("Uint8Array", TypedArrayKind::Uint8),
        ("Uint8ClampedArray", TypedArrayKind::Uint8Clamped),
        ("Int16Array", TypedArrayKind::Int16),
        ("Uint16Array", TypedArrayKind::Uint16),
        ("Int32Array", TypedArrayKind::Int32),
        ("Uint32Array", TypedArrayKind::Uint32),
        ("Float16Array", TypedArrayKind::Float16),
        ("Float32Array", TypedArrayKind::Float32),
        ("Float64Array", TypedArrayKind::Float64),
        ("BigInt64Array", TypedArrayKind::BigInt64),
        ("BigUint64Array", TypedArrayKind::BigUint64),
    ];

    let mut constructors = Vec::new();

    for (name, kind) in kinds {
        let constructor = make_typedarray_constructor(mc, env, name, kind, ta_intrinsic, ta_proto_intrinsic)?;
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
        TypedArrayKind::Float16 => 7,
        TypedArrayKind::Float32 => 8,
        TypedArrayKind::Float64 => 9,
        TypedArrayKind::BigInt64 => 10,
        TypedArrayKind::BigUint64 => 11,
    }
}

fn make_typedarray_constructor<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    name: &str,
    kind: TypedArrayKind,
    ta_intrinsic: &JSObjectDataPtr<'gc>,
    ta_proto_intrinsic: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let kind_value = typedarray_kind_to_number(&kind);

    let obj = new_js_object_data(mc);

    // Int8Array.[[Prototype]] = %TypedArray%
    obj.borrow_mut(mc).prototype = Some(*ta_intrinsic);

    let proto = make_typedarray_prototype(mc, env, name, kind.clone(), ta_proto_intrinsic)?;
    object_set_key_value(mc, &obj, "prototype", &Value::Object(proto))?;
    obj.borrow_mut(mc).set_non_writable("prototype");
    obj.borrow_mut(mc).set_non_enumerable("prototype");
    obj.borrow_mut(mc).set_non_configurable("prototype");

    let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16(name)), false, false, true)?;
    crate::js_object::define_property_internal(mc, &obj, "name", &name_desc)?;
    let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(3.0), false, false, true)?;
    crate::js_object::define_property_internal(mc, &obj, "length", &len_desc)?;

    slot_set(mc, &obj, InternalSlot::Kind, &Value::Number(kind_value as f64));
    slot_set(mc, &obj, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("TypedArray")));
    slot_set(mc, &obj, InternalSlot::IsConstructor, &Value::Boolean(true));

    // BYTES_PER_ELEMENT on constructor (non-writable, non-enumerable, non-configurable)
    let bytes_per_element = match kind {
        TypedArrayKind::Int8 | TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => 1,
        TypedArrayKind::Int16 | TypedArrayKind::Uint16 | TypedArrayKind::Float16 => 2,
        TypedArrayKind::Int32 | TypedArrayKind::Uint32 | TypedArrayKind::Float32 => 4,
        TypedArrayKind::Float64 | TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => 8,
    } as f64;

    object_set_key_value(mc, &obj, "BYTES_PER_ELEMENT", &Value::Number(bytes_per_element))?;
    obj.borrow_mut(mc).set_non_enumerable("BYTES_PER_ELEMENT");
    obj.borrow_mut(mc).set_non_writable("BYTES_PER_ELEMENT");
    obj.borrow_mut(mc).set_non_configurable("BYTES_PER_ELEMENT");

    // BYTES_PER_ELEMENT on prototype too
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
    _env: &JSObjectDataPtr<'gc>,
    ctor_name: &str,
    kind: TypedArrayKind,
    ta_proto_intrinsic: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let proto = new_js_object_data(mc);

    // Int8Array.prototype.[[Prototype]] = %TypedArray%.prototype
    proto.borrow_mut(mc).prototype = Some(*ta_proto_intrinsic);
    slot_set(mc, &proto, InternalSlot::Proto, &Value::Object(*ta_proto_intrinsic));

    // Store the kind for dispatch
    let kind_value = match kind {
        TypedArrayKind::Int8 => 0,
        TypedArrayKind::Uint8 => 1,
        TypedArrayKind::Uint8Clamped => 2,
        TypedArrayKind::Int16 => 3,
        TypedArrayKind::Uint16 => 4,
        TypedArrayKind::Int32 => 5,
        TypedArrayKind::Uint32 => 6,
        TypedArrayKind::Float16 => 7,
        TypedArrayKind::Float32 => 8,
        TypedArrayKind::Float64 => 9,
        TypedArrayKind::BigInt64 => 10,
        TypedArrayKind::BigUint64 => 11,
    };
    slot_set(mc, &proto, InternalSlot::Kind, &Value::Number(kind_value as f64));

    // constructor property pointing to the specific TA constructor (set later via caller).
    // Use a placeholder Value::Function that will resolve correctly.
    object_set_key_value(mc, &proto, "constructor", &Value::Function(ctor_name.to_string()))?;
    proto.borrow_mut(mc).set_non_enumerable("constructor");

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

    // Parse optional options object for growable buffers
    let mut max_byte_length: Option<usize> = None;
    if args.len() > 1 {
        let opts = args[1].clone();
        if let Value::Object(obj) = opts {
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

    // Create the SharedArrayBuffer object first
    let obj = new_js_object_data(mc);

    // Set prototype from NewTarget.prototype if present; otherwise fallback to
    // the constructor realm's SharedArrayBuffer.prototype per GetPrototypeFromConstructor.
    let mut proto_from_target: Option<JSObjectDataPtr<'gc>> = None;
    if let Some(Value::Object(nt_obj)) = new_target {
        let proto_val = crate::core::get_property_with_accessors(mc, env, nt_obj, "prototype")?;
        if let Value::Object(proto_obj) = proto_val {
            proto_from_target = Some(proto_obj);
        }
    }

    let proto = if let Some(p) = proto_from_target {
        p
    } else {
        // OrdinaryCreateFromConstructor fallback: use the constructor's realm.
        let ctor_realm = if let Some(Value::Object(nt_obj)) = new_target {
            crate::js_class::get_function_realm(nt_obj).ok().flatten().unwrap_or(*env)
        } else {
            *env
        };
        if let Some(ctor_val) = object_get_key_value(&ctor_realm, "SharedArrayBuffer")
            && let Value::Object(ctor_obj) = &*ctor_val.borrow()
            && let Some(p_val) = object_get_key_value(ctor_obj, "prototype")
            && let Value::Object(p_obj) = &*p_val.borrow()
        {
            *p_obj
        } else if let Some(ctor_val) = crate::core::env_get(&ctor_realm, "SharedArrayBuffer")
            && let Value::Object(ctor_obj) = &*ctor_val.borrow()
            && let Some(p_val) = object_get_key_value(ctor_obj, "prototype")
            && let Value::Object(p_obj) = &*p_val.borrow()
        {
            *p_obj
        } else {
            new_js_object_data(mc)
        }
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
            max_byte_length,
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
    env: &JSObjectDataPtr<'gc>,
    new_target: Option<&Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Note: when called from evaluate_new, new_target=None means the
    // constructor itself is the new target (normal `new DataView()` call).
    // Only a direct function-call (not via `new`) would be an error, but
    // that path is caught earlier by the engine.

    // ToIndex helper (spec 7.1.22)
    let to_index = |v: &Value<'gc>| -> Result<usize, EvalError<'gc>> {
        if matches!(v, Value::Undefined) {
            return Ok(0);
        }
        let prim = if let Value::Object(_) = v {
            crate::core::to_primitive(mc, v, "number", env)?
        } else {
            v.clone()
        };
        if matches!(prim, Value::Symbol(_)) {
            return Err(raise_type_error!("Cannot convert a Symbol value to a number").into());
        }
        if matches!(prim, Value::BigInt(_)) {
            return Err(raise_type_error!("Cannot convert a BigInt value to a number").into());
        }
        let n = crate::core::to_number(&prim)?;
        let integer = if n.is_nan() || n == 0.0 { 0.0 } else { n.trunc() };
        const MAX_SAFE_PLUS_ONE: f64 = 9007199254740992.0; // 2^53
        if !(0.0..MAX_SAFE_PLUS_ONE).contains(&integer) {
            return Err(raise_range_error!("Invalid index").into());
        }
        Ok(integer as usize)
    };

    // DataView(buffer [, byteOffset [, byteLength]])
    if args.is_empty() {
        return Err(raise_type_error!("DataView constructor requires a buffer argument").into());
    }

    let buffer_val = args[0].clone();
    let buffer_obj = if let Value::Object(obj) = &buffer_val { Some(*obj) } else { None };
    let buffer = match buffer_val {
        Value::Object(obj) => {
            if let Some(ab_val) = slot_get_chained(&obj, &InternalSlot::ArrayBuffer) {
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    *ab
                } else {
                    return Err(raise_type_error!("First argument to DataView constructor must be an ArrayBuffer").into());
                }
            } else {
                return Err(raise_type_error!("First argument to DataView constructor must be an ArrayBuffer").into());
            }
        }
        _ => return Err(raise_type_error!("First argument to DataView constructor must be an ArrayBuffer").into()),
    };

    // Step 4: Let offset = ? ToIndex(byteOffset)
    let byte_offset = if args.len() > 1 { to_index(&args[1])? } else { 0 };

    // Step 7: If IsDetachedBuffer(buffer) is true, throw a TypeError exception.
    if buffer.borrow().detached {
        return Err(raise_type_error!("Cannot construct DataView on a detached ArrayBuffer").into());
    }

    // Step 8: Let bufferByteLength = ArrayBufferByteLength(buffer)
    let buffer_byte_length = buffer.borrow().data.lock().unwrap().len();

    // Step 9: If offset > bufferByteLength, throw RangeError
    if byte_offset > buffer_byte_length {
        return Err(raise_range_error!("Start offset is outside the bounds of the buffer").into());
    }

    // Step 10-13: Compute viewByteLength
    let (byte_length, dv_length_tracking) = if args.len() > 2 && !matches!(args[2], Value::Undefined) {
        let len = to_index(&args[2])?;
        if byte_offset + len > buffer_byte_length {
            return Err(raise_range_error!("Invalid DataView length").into());
        }
        (len, false)
    } else {
        (buffer_byte_length - byte_offset, buffer.borrow().max_byte_length.is_some())
    };

    // Create the DataView object
    let obj = new_js_object_data(mc);

    // GetPrototypeFromConstructor
    let proto = if let Some(Value::Object(nt_obj)) = new_target
        && let Some(p) = crate::js_class::get_prototype_from_constructor(mc, nt_obj, env, "DataView")?
    {
        p
    } else if let Some(ctor_val) = object_get_key_value(env, "DataView")
        && let Value::Object(ctor_obj) = &*ctor_val.borrow()
        && let Some(p_val) = object_get_key_value(ctor_obj, "prototype")
        && let Value::Object(p_obj) = &*p_val.borrow()
    {
        *p_obj
    } else {
        // Fallback: create a fresh prototype (should not normally happen)
        make_dataview_prototype(mc, env, &new_js_object_data(mc))?
    };
    obj.borrow_mut(mc).prototype = Some(proto);

    // Step 12 (spec): If IsDetachedBuffer(buffer) is true, throw TypeError.
    // The prototype access in GetPrototypeFromConstructor could have detached the buffer.
    if buffer.borrow().detached {
        return Err(raise_type_error!("Cannot construct DataView on a detached ArrayBuffer").into());
    }

    // Spec step 13-14: Re-check buffer bounds after GetPrototypeFromConstructor
    // (prototype access could have resized the buffer)
    let buffer_byte_length2 = buffer.borrow().data.lock().unwrap().len();
    let byte_length = if dv_length_tracking {
        // Auto-length: recompute from current buffer size
        if byte_offset > buffer_byte_length2 {
            return Err(raise_range_error!("Start offset is outside the bounds of the buffer").into());
        }
        buffer_byte_length2 - byte_offset
    } else {
        // Fixed-length: check that offset + byteLength still fits
        if byte_offset + byte_length > buffer_byte_length2 {
            return Err(raise_range_error!("Invalid DataView length").into());
        }
        byte_length
    };

    // Create DataView internal data
    let data_view = Gc::new(
        mc,
        JSDataView {
            buffer,
            byte_offset,
            byte_length,
            length_tracking: dv_length_tracking,
        },
    );

    slot_set(mc, &obj, InternalSlot::DataView, &Value::DataView(data_view));
    if let Some(buf_obj) = buffer_obj {
        slot_set(mc, &obj, InternalSlot::BufferObject, &Value::Object(buf_obj));
    }

    Ok(Value::Object(obj))
}

/// ToIndex per spec (7.1.22): convert to a non-negative integer index or throw RangeError.
fn to_index<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, val: &Value<'gc>) -> Result<usize, EvalError<'gc>> {
    if matches!(val, Value::Undefined) {
        return Ok(0);
    }
    let prim = if let Value::Object(_) = val {
        crate::core::to_primitive(mc, val, "number", env)?
    } else {
        val.clone()
    };
    if matches!(prim, Value::Symbol(_)) {
        return Err(throw_type_error(mc, env, "Cannot convert a Symbol value to a number"));
    }
    if matches!(prim, Value::BigInt(_)) {
        return Err(throw_type_error(mc, env, "Cannot convert a BigInt value to a number"));
    }
    let n = crate::core::to_number(&prim)?;
    let integer_index = if n.is_nan() || n == 0.0 {
        0.0
    } else if n.is_infinite() {
        n
    } else {
        n.trunc()
    };
    if integer_index < 0.0 {
        return Err(throw_range_error(mc, env, "Invalid typed array length"));
    }
    const MAX_SAFE: f64 = 9007199254740991.0; // 2^53-1
    let index = if integer_index > MAX_SAFE { MAX_SAFE } else { integer_index };
    if integer_index != index {
        return Err(throw_range_error(mc, env, "Invalid typed array length"));
    }
    Ok(integer_index as usize)
}

/// CanonicalNumericIndexString(argument) — ES2024 §7.1.21
/// Returns Some(n) if the string is the canonical string representation of a number.
pub fn canonical_numeric_index_string(s: &str) -> Option<f64> {
    if s == "-0" {
        return Some(-0.0_f64);
    }
    // Parse as a number
    let n: f64 = s.parse().ok()?;
    // Check ToString(n) === s
    let back = if n.is_nan() {
        "NaN".to_string()
    } else if n.is_infinite() {
        if n.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        }
    } else {
        crate::core::format_js_number(n)
    };
    if back == s { Some(n) } else { None }
}

/// IsValidIntegerIndex(O, index) — ES2024 §10.4.5.11
/// Returns true if index is a valid element index for the TypedArray.
pub fn is_valid_integer_index(ta: &crate::core::JSTypedArray, index: f64) -> bool {
    // 1. If IsIntegralNumber(index) is false, return false
    if index.is_nan() || index.is_infinite() || index != index.trunc() {
        return false;
    }
    // 2. If index is -0, return false
    if index == 0.0 && index.is_sign_negative() {
        return false;
    }

    // For fixed-length views backed by a resizable buffer, check whether
    // the view has gone out-of-bounds due to a buffer resize.
    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
    let cur_len = if ta.length_tracking {
        if buf_len <= ta.byte_offset {
            0
        } else {
            (buf_len - ta.byte_offset) / ta.element_size()
        }
    } else {
        // Fixed-length: check if the TA is still in-bounds
        let needed = ta.byte_offset + ta.length * ta.element_size();
        if needed > buf_len {
            // The TA is out-of-bounds — all indices are invalid
            return false;
        }
        ta.length
    };
    // 3. If index < 0 or index >= length, return false
    if index < 0.0 || (index as usize) >= cur_len {
        return false;
    }
    true
}

fn kind_from_number(n: i32) -> Option<TypedArrayKind> {
    match n {
        0 => Some(TypedArrayKind::Int8),
        1 => Some(TypedArrayKind::Uint8),
        2 => Some(TypedArrayKind::Uint8Clamped),
        3 => Some(TypedArrayKind::Int16),
        4 => Some(TypedArrayKind::Uint16),
        5 => Some(TypedArrayKind::Int32),
        6 => Some(TypedArrayKind::Uint32),
        7 => Some(TypedArrayKind::Float16),
        8 => Some(TypedArrayKind::Float32),
        9 => Some(TypedArrayKind::Float64),
        10 => Some(TypedArrayKind::BigInt64),
        11 => Some(TypedArrayKind::BigUint64),
        _ => None,
    }
}

fn element_size_for_kind(kind: &TypedArrayKind) -> usize {
    match kind {
        TypedArrayKind::Int8 | TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => 1,
        TypedArrayKind::Int16 | TypedArrayKind::Uint16 | TypedArrayKind::Float16 => 2,
        TypedArrayKind::Int32 | TypedArrayKind::Uint32 | TypedArrayKind::Float32 => 4,
        TypedArrayKind::Float64 | TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => 8,
    }
}

/// Get the TypedArray constructor name for use in GetPrototypeFromConstructor fallback.
fn kind_to_constructor_name(kind: &TypedArrayKind) -> &'static str {
    match kind {
        TypedArrayKind::Int8 => "Int8Array",
        TypedArrayKind::Uint8 => "Uint8Array",
        TypedArrayKind::Uint8Clamped => "Uint8ClampedArray",
        TypedArrayKind::Int16 => "Int16Array",
        TypedArrayKind::Uint16 => "Uint16Array",
        TypedArrayKind::Int32 => "Int32Array",
        TypedArrayKind::Uint32 => "Uint32Array",
        TypedArrayKind::Float16 => "Float16Array",
        TypedArrayKind::Float32 => "Float32Array",
        TypedArrayKind::Float64 => "Float64Array",
        TypedArrayKind::BigInt64 => "BigInt64Array",
        TypedArrayKind::BigUint64 => "BigUint64Array",
    }
}

/// Handle TypedArray constructor calls.
/// `constructor_obj` must carry InternalSlot::Kind (the actual TA constructor).
/// `new_target` is the NewTarget for GetPrototypeFromConstructor. If None, throws TypeError (called without new).
pub fn handle_typedarray_constructor<'gc>(
    mc: &MutationContext<'gc>,
    constructor_obj: &JSObjectDataPtr<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    new_target: Option<&JSObjectDataPtr<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 1: If NewTarget is undefined, throw a TypeError exception.
    let new_target = new_target.ok_or_else(|| throw_type_error(mc, env, "Constructor requires 'new'"))?;

    // Get the kind from the constructor (always from constructor_obj, NOT new_target)
    let kind_val = slot_get_chained(constructor_obj, &InternalSlot::Kind);
    let kind = if let Some(kind_val) = kind_val {
        if let Value::Number(kind_num) = *kind_val.borrow() {
            kind_from_number(kind_num as i32).ok_or_else(|| throw_type_error(mc, env, "Invalid TypedArray kind"))?
        } else {
            return Err(throw_type_error(mc, env, "Invalid TypedArray constructor"));
        }
    } else {
        return Err(throw_type_error(mc, env, "Invalid TypedArray constructor"));
    };

    let element_size = element_size_for_kind(&kind);
    let ctor_name = kind_to_constructor_name(&kind);
    let is_bigint_ta = is_bigint_typed_array(&kind);

    let mut init_values: Option<Vec<Value<'gc>>> = None;
    let mut buffer_obj_opt: Option<JSObjectDataPtr<'gc>> = None;

    // Spec: dispatch based on firstArgument type
    let (buffer, byte_offset, length) = if args.is_empty() {
        // TypedArray() — no args, create empty
        let buffer = new_gc_cell_ptr(
            mc,
            JSArrayBuffer {
                data: Arc::new(Mutex::new(vec![])),
                ..JSArrayBuffer::default()
            },
        );
        (buffer, 0usize, 0usize)
    } else {
        let first = &args[0];
        if let Value::Object(first_obj) = first {
            // First arg is an Object
            if let Some(ta_val) = slot_get_chained(first_obj, &InternalSlot::TypedArray) {
                // TypedArray(typedArray)
                if let Value::TypedArray(src_ta) = &*ta_val.borrow() {
                    // Spec: InitializeTypedArrayFromTypedArray
                    // If IsTypedArrayOutOfBounds(srcRecord), throw TypeError.
                    let buf_len = src_ta.buffer.borrow().data.lock().unwrap().len();
                    let src_length = if src_ta.length_tracking {
                        // Length-tracking: out-of-bounds when byte_offset > buf_len
                        if src_ta.byte_offset > buf_len {
                            return Err(throw_type_error(mc, env, "Source TypedArray is out of bounds"));
                        }
                        (buf_len - src_ta.byte_offset) / src_ta.element_size()
                    } else {
                        // Fixed-length: out-of-bounds when needed bytes exceed buffer
                        let needed = src_ta.byte_offset + src_ta.length * src_ta.element_size();
                        if needed > buf_len {
                            return Err(throw_type_error(mc, env, "Source TypedArray is out of bounds"));
                        }
                        src_ta.length
                    };
                    let src_is_bigint = is_bigint_typed_array(&src_ta.kind);
                    let mut copied = Vec::with_capacity(src_length);
                    for idx in 0..src_length {
                        let val = if src_is_bigint {
                            let size = src_ta.element_size();
                            let bo = src_ta.byte_offset + idx * size;
                            let buf = src_ta.buffer.borrow();
                            let data = buf.data.lock().unwrap();
                            if bo + size <= data.len() {
                                let bytes = &data[bo..bo + size];
                                let big_int = if matches!(src_ta.kind, TypedArrayKind::BigInt64) {
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
                            Value::Number(src_ta.get(idx).unwrap_or(f64::NAN))
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
                    return Err(throw_type_error(mc, env, "Invalid TypedArray constructor argument"));
                }
            } else if let Some(ab_val) = slot_get_chained(first_obj, &InternalSlot::ArrayBuffer) {
                // TypedArray(buffer [, byteOffset [, length]])
                if let Value::ArrayBuffer(ab) = &*ab_val.borrow() {
                    // Step 7: Let offset = ToIndex(byteOffset)
                    let offset = if args.len() > 1 { to_index(mc, env, &args[1])? } else { 0 };

                    // Step 8: If offset modulo elementSize ≠ 0, throw RangeError
                    if element_size > 0 && offset % element_size != 0 {
                        return Err(throw_range_error(mc, env, "Start offset is not a multiple of the element size"));
                    }

                    // Step 9: If IsDetachedBuffer(buffer), throw TypeError
                    if ab.borrow().detached {
                        return Err(throw_type_error(mc, env, "Cannot construct TypedArray on a detached ArrayBuffer"));
                    }

                    let buf_byte_len = ab.borrow().data.lock().unwrap().len();

                    // Step 11: If offset > bufferByteLength, throw RangeError
                    if offset > buf_byte_len {
                        return Err(throw_range_error(mc, env, "Start offset is outside the bounds of the buffer"));
                    }

                    let final_length = if args.len() > 2 && !matches!(args[2], Value::Undefined) {
                        // Explicit length given
                        let new_length = to_index(mc, env, &args[2])?;
                        // Step 12b: If IsDetachedBuffer(buffer) is true, throw a TypeError.
                        // (The length conversion may have detached the buffer via valueOf.)
                        if ab.borrow().detached {
                            return Err(throw_type_error(mc, env, "Cannot construct TypedArray on a detached ArrayBuffer"));
                        }
                        if offset + new_length * element_size > buf_byte_len {
                            return Err(throw_range_error(mc, env, "Invalid typed array length"));
                        }
                        new_length
                    } else {
                        // No explicit length: derive from buffer
                        if ab.borrow().max_byte_length.is_some() {
                            // Resizable buffer with auto-length: per spec step 20,
                            // no byte alignment check. Length computed dynamically as
                            // floor((bufferByteLength - offset) / elementSize).
                            (buf_byte_len - offset) / element_size
                        } else {
                            // Fixed-size buffer: require exact alignment
                            if (buf_byte_len - offset) % element_size != 0 {
                                return Err(throw_range_error(
                                    mc,
                                    env,
                                    "Byte length of buffer minus offset is not a multiple of the element size",
                                ));
                            }
                            (buf_byte_len - offset) / element_size
                        }
                    };

                    buffer_obj_opt = Some(*first_obj);
                    (*ab, offset, final_length)
                } else {
                    return Err(throw_type_error(mc, env, "Invalid TypedArray constructor argument"));
                }
            } else {
                // TypedArray(object) — iterable or array-like
                let mut iterable_values: Option<Vec<Value<'gc>>> = None;
                // Try @@iterator
                let iter_fn_result = (|| -> Result<Option<Value<'gc>>, EvalError<'gc>> {
                    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                        && let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator")
                        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
                    {
                        let iter_fn = crate::core::get_property_with_accessors(mc, env, first_obj, *iter_sym)?;
                        if !matches!(iter_fn, Value::Undefined | Value::Null) {
                            return Ok(Some(iter_fn));
                        }
                    }
                    Ok(None)
                })();
                let iter_fn = iter_fn_result?;

                if let Some(iter_fn) = iter_fn {
                    // Use iterator protocol
                    let iterator = crate::core::evaluate_call_dispatch(mc, env, &iter_fn, Some(&Value::Object(*first_obj)), &[])?;
                    if let Value::Object(iter_obj) = iterator {
                        let mut values = Vec::new();
                        loop {
                            let next_fn = crate::core::get_property_with_accessors(mc, env, &iter_obj, "next")?;
                            let next_res = crate::core::evaluate_call_dispatch(mc, env, &next_fn, Some(&Value::Object(iter_obj)), &[])?;
                            if let Value::Object(next_obj) = next_res {
                                let done_val = crate::core::get_property_with_accessors(mc, env, &next_obj, "done")?;
                                if done_val.to_truthy() {
                                    break;
                                }
                                let value = crate::core::get_property_with_accessors(mc, env, &next_obj, "value")?;
                                values.push(value);
                            } else {
                                break;
                            }
                        }
                        iterable_values = Some(values);
                    }
                }

                if let Some(values) = iterable_values {
                    let src_length = values.len();
                    init_values = Some(values);
                    let buffer = new_gc_cell_ptr(
                        mc,
                        JSArrayBuffer {
                            data: Arc::new(Mutex::new(vec![0; src_length * element_size])),
                            ..JSArrayBuffer::default()
                        },
                    );
                    (buffer, 0, src_length)
                } else {
                    // Array-like source (no iterator)
                    let len_val = crate::core::get_property_with_accessors(mc, env, first_obj, "length")?;
                    let src_length = to_index(mc, env, &len_val)?;

                    let mut copied = Vec::with_capacity(src_length);
                    for idx in 0..src_length {
                        let raw = crate::core::get_property_with_accessors(mc, env, first_obj, idx)?;
                        copied.push(raw);
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
        } else {
            // First arg is NOT an Object → TypedArray(length)
            let element_length = to_index(mc, env, first)?;
            let buffer = new_gc_cell_ptr(
                mc,
                JSArrayBuffer {
                    data: Arc::new(Mutex::new(vec![0; element_length * element_size])),
                    ..JSArrayBuffer::default()
                },
            );
            (buffer, 0, element_length)
        }
    };

    // Create the TypedArray object
    let obj = new_js_object_data(mc);

    // GetPrototypeFromConstructor(newTarget, defaultProto) - use new_target for proto
    if let Some(proto) = crate::js_class::get_prototype_from_constructor(mc, new_target, env, ctor_name)? {
        obj.borrow_mut(mc).prototype = Some(proto);
    } else {
        // Fallback: use the constructor's own prototype
        if let Some(proto_val) = object_get_key_value(constructor_obj, "prototype")
            && let Value::Object(proto_obj) = &*proto_val.borrow()
        {
            obj.borrow_mut(mc).prototype = Some(*proto_obj);
        }
    }

    // Determine length-tracking
    let length_tracking = if !args.is_empty() {
        if let Value::Object(fo) = &args[0] {
            if let Some(ab_val) = slot_get_chained(fo, &InternalSlot::ArrayBuffer)
                && let Value::ArrayBuffer(ab) = &*ab_val.borrow()
                && ab.borrow().max_byte_length.is_some()
                && (args.len() <= 2 || matches!(args.get(2), Some(Value::Undefined)))
            {
                true
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
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

    // Store buffer wrapper object
    let buf_wrapper = if let Some(existing) = buffer_obj_opt {
        existing
    } else {
        let buf_obj = new_js_object_data(mc);
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

    // Initialize elements from init_values
    if let Some(values) = init_values {
        for (idx, v) in values.iter().enumerate() {
            if idx >= length {
                break;
            }
            if is_bigint_ta {
                // Use ToBigInt which properly throws for invalid types
                let n = to_bigint_i64(mc, env, v)?;
                typed_array.set_bigint(mc, idx, n).map_err(|e| {
                    let v = crate::core::js_error_to_value(mc, env, &e);
                    EvalError::Throw(v, None, None)
                })?;
            } else {
                let num = crate::core::to_number_with_env(mc, env, v)?;
                typed_array.set(mc, idx, num).map_err(|e| {
                    let v2 = crate::core::js_error_to_value(mc, env, &e);
                    EvalError::Throw(v2, None, None)
                })?;
            }
        }
    }

    Ok(Value::Object(obj))
}

/// Handle DataView instance method calls
pub fn handle_dataview_method<'gc>(
    mc: &MutationContext<'gc>,
    object: &JSObjectDataPtr<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Get the DataView from the object — TypeError if not a DataView
    let dv_val = slot_get_chained(object, &InternalSlot::DataView);
    let data_view_rc = if let Some(dv_val) = dv_val {
        if let Value::DataView(dv) = &*dv_val.borrow() {
            *dv
        } else {
            return Err(raise_type_error!("Method called on incompatible receiver").into());
        }
    } else {
        return Err(raise_type_error!("Method called on incompatible receiver").into());
    };

    // For accessor properties (buffer, byteLength, byteOffset), check detached after getting the DataView
    // For get/set methods, check detached after ToIndex coercion per spec ordering
    let is_accessor = matches!(method, "buffer" | "byteLength" | "byteOffset");

    // Check for detached buffer (IsDetachedBuffer) — for accessors, check now;
    // for get/set methods, we check after argument coercion per spec
    if is_accessor && method != "buffer" {
        // byteLength and byteOffset throw TypeError on detached buffer
        if data_view_rc.buffer.borrow().detached {
            return Err(raise_type_error!("Cannot perform operation on a detached ArrayBuffer").into());
        }
        // IsViewOutOfBounds check for resizable buffers
        let buf_len = data_view_rc.buffer.borrow().data.lock().unwrap().len();
        if data_view_rc.length_tracking {
            // Auto-length DataView: OOB when byte_offset > buffer length
            if data_view_rc.byte_offset > buf_len {
                return Err(raise_type_error!("DataView is out of bounds").into());
            }
        } else {
            // Fixed-length DataView: OOB when byte_offset + byte_length > buffer length
            if data_view_rc.byte_offset + data_view_rc.byte_length > buf_len {
                return Err(raise_type_error!("DataView is out of bounds").into());
            }
        }
    }

    // ToIndex helper (spec 7.1.22) — uses ToNumber with valueOf support
    let to_index_val = |v: &Value<'gc>| -> Result<usize, EvalError<'gc>> {
        if matches!(v, Value::Undefined) {
            return Ok(0);
        }
        let n = crate::core::to_number_with_env(mc, env, v)?;
        let integer = if n.is_nan() || n == 0.0 { 0.0 } else { n.trunc() };
        const MAX_SAFE_PLUS_ONE: f64 = 9007199254740992.0;
        if !(0.0..MAX_SAFE_PLUS_ONE).contains(&integer) {
            return Err(raise_range_error!("Offset is outside the bounds of the DataView").into());
        }
        Ok(integer as usize)
    };

    // ToBoolean (spec 7.1.2)
    let to_bool = |v: &Value<'gc>| -> bool {
        match v {
            Value::Boolean(b) => *b,
            Value::Undefined | Value::Null => false,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::BigInt(b) => **b != num_bigint::BigInt::from(0),
            _ => true, // objects, symbols → true
        }
    };

    // ECMAScript modular integer conversion helpers (spec §7.1)
    // These use modular arithmetic instead of Rust's saturating `as` casts.
    let f64_to_uint8 = |v: f64| -> u8 {
        if v.is_nan() || v.is_infinite() || v == 0.0 {
            return 0;
        }
        let n = v.trunc();
        let n = n % 256.0;
        (if n < 0.0 { n + 256.0 } else { n }) as u8
    };
    let f64_to_int8 = |v: f64| -> i8 { f64_to_uint8(v) as i8 };
    let f64_to_uint16 = |v: f64| -> u16 {
        if v.is_nan() || v.is_infinite() || v == 0.0 {
            return 0;
        }
        let n = v.trunc();
        let m = 65536.0;
        let n = n % m;
        (if n < 0.0 { n + m } else { n }) as u16
    };
    let f64_to_int16 = |v: f64| -> i16 { f64_to_uint16(v) as i16 };
    let f64_to_uint32 = |v: f64| -> u32 {
        if v.is_nan() || v.is_infinite() || v == 0.0 {
            return 0;
        }
        let n = v.trunc();
        let m = 4294967296.0; // 2^32
        let n = n % m;
        (if n < 0.0 { n + m } else { n }) as u32
    };
    let f64_to_int32 = |v: f64| -> i32 { f64_to_uint32(v) as i32 };

    // Map JSError from check_bounds to EvalError with RangeError
    let bounds_err = |_e: JSError| -> EvalError<'gc> { raise_range_error!("Offset is outside the bounds of the DataView").into() };

    // Check for detached buffer and OOB — must be called after argument coercion per spec ordering
    let check_detached = || -> Result<(), EvalError<'gc>> {
        if data_view_rc.buffer.borrow().detached {
            return Err(raise_type_error!("Cannot perform operation on a detached ArrayBuffer").into());
        }
        // IsViewOutOfBounds check for resizable buffers
        let buf_len = data_view_rc.buffer.borrow().data.lock().unwrap().len();
        if data_view_rc.length_tracking {
            if data_view_rc.byte_offset > buf_len {
                return Err(raise_type_error!("DataView is out of bounds").into());
            }
        } else if data_view_rc.byte_offset + data_view_rc.byte_length > buf_len {
            return Err(raise_type_error!("DataView is out of bounds").into());
        }
        Ok(())
    };

    match method {
        // ---- Get methods ----
        "getInt8" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            check_detached()?;
            data_view_rc.get_int8(offset).map(|v| Value::Number(v as f64)).map_err(bounds_err)
        }
        "getUint8" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            check_detached()?;
            data_view_rc.get_uint8(offset).map(|v| Value::Number(v as f64)).map_err(bounds_err)
        }
        "getInt16" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(1).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc
                .get_int16(offset, le)
                .map(|v| Value::Number(v as f64))
                .map_err(bounds_err)
        }
        "getUint16" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(1).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc
                .get_uint16(offset, le)
                .map(|v| Value::Number(v as f64))
                .map_err(bounds_err)
        }
        "getInt32" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(1).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc
                .get_int32(offset, le)
                .map(|v| Value::Number(v as f64))
                .map_err(bounds_err)
        }
        "getUint32" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(1).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc
                .get_uint32(offset, le)
                .map(|v| Value::Number(v as f64))
                .map_err(bounds_err)
        }
        "getFloat32" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(1).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc
                .get_float32(offset, le)
                .map(|v| Value::Number(v as f64))
                .map_err(bounds_err)
        }
        "getFloat64" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(1).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc.get_float64(offset, le).map(Value::Number).map_err(bounds_err)
        }
        "getFloat16" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(1).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc
                .get_float16(offset, le)
                .map(|v| Value::Number(f16_to_f64(v)))
                .map_err(bounds_err)
        }
        "getBigInt64" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(1).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc
                .get_bigint64(offset, le)
                .map(|v| Value::BigInt(Box::new(num_bigint::BigInt::from(v))))
                .map_err(bounds_err)
        }
        "getBigUint64" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(1).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc
                .get_biguint64(offset, le)
                .map(|v| Value::BigInt(Box::new(num_bigint::BigInt::from(v))))
                .map_err(bounds_err)
        }
        // ---- Set methods ----
        "setInt8" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let val = crate::core::to_number_with_env(mc, env, args.get(1).unwrap_or(&Value::Undefined))?;
            check_detached()?;
            data_view_rc.set_int8(offset, f64_to_int8(val)).map_err(bounds_err)?;
            Ok(Value::Undefined)
        }
        "setUint8" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let val = crate::core::to_number_with_env(mc, env, args.get(1).unwrap_or(&Value::Undefined))?;
            check_detached()?;
            data_view_rc.set_uint8(offset, f64_to_uint8(val)).map_err(bounds_err)?;
            Ok(Value::Undefined)
        }
        "setInt16" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let val = crate::core::to_number_with_env(mc, env, args.get(1).unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(2).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc.set_int16(offset, f64_to_int16(val), le).map_err(bounds_err)?;
            Ok(Value::Undefined)
        }
        "setUint16" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let val = crate::core::to_number_with_env(mc, env, args.get(1).unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(2).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc.set_uint16(offset, f64_to_uint16(val), le).map_err(bounds_err)?;
            Ok(Value::Undefined)
        }
        "setInt32" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let val = crate::core::to_number_with_env(mc, env, args.get(1).unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(2).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc.set_int32(offset, f64_to_int32(val), le).map_err(bounds_err)?;
            Ok(Value::Undefined)
        }
        "setUint32" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let val = crate::core::to_number_with_env(mc, env, args.get(1).unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(2).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc.set_uint32(offset, f64_to_uint32(val), le).map_err(bounds_err)?;
            Ok(Value::Undefined)
        }
        "setFloat32" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let val = crate::core::to_number_with_env(mc, env, args.get(1).unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(2).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc.set_float32(offset, val as f32, le).map_err(bounds_err)?;
            Ok(Value::Undefined)
        }
        "setFloat64" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let val = crate::core::to_number_with_env(mc, env, args.get(1).unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(2).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc.set_float64(offset, val, le).map_err(bounds_err)?;
            Ok(Value::Undefined)
        }
        "setFloat16" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let val = crate::core::to_number_with_env(mc, env, args.get(1).unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(2).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc.set_float16(offset, f64_to_f16(val), le).map_err(bounds_err)?;
            Ok(Value::Undefined)
        }
        "setBigInt64" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let i = to_bigint_i64(mc, env, args.get(1).unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(2).unwrap_or(&Value::Undefined));
            check_detached()?;
            data_view_rc.set_bigint64(offset, i, le).map_err(bounds_err)?;
            Ok(Value::Undefined)
        }
        "setBigUint64" => {
            let offset = to_index_val(args.first().unwrap_or(&Value::Undefined))?;
            let i = to_bigint_i64(mc, env, args.get(1).unwrap_or(&Value::Undefined))?;
            let le = to_bool(args.get(2).unwrap_or(&Value::Undefined));
            check_detached()?;
            let u = i as u64;
            data_view_rc.set_biguint64(offset, u, le).map_err(bounds_err)?;
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
        "byteLength" => {
            if data_view_rc.length_tracking {
                let buf_len = data_view_rc.buffer.borrow().data.lock().unwrap().len();
                Ok(Value::Number((buf_len - data_view_rc.byte_offset) as f64))
            } else {
                Ok(Value::Number(data_view_rc.byte_length as f64))
            }
        }
        "byteOffset" => Ok(Value::Number(data_view_rc.byte_offset as f64)),
        _ => Err(raise_type_error!(format!("DataView method '{method}' not implemented")).into()),
    }
}

impl<'gc> JSDataView<'gc> {
    fn check_bounds(&self, offset: usize, size: usize) -> Result<usize, JSError> {
        let buffer = self.buffer.borrow();
        let buffer_len = buffer.data.lock().unwrap().len();
        // Compute effective byte length: for length-tracking views, use current buffer size
        let effective_byte_length = if self.length_tracking {
            if self.byte_offset > buffer_len {
                return Err(raise_range_error!("Offset is outside the bounds of the DataView"));
            }
            buffer_len - self.byte_offset
        } else {
            self.byte_length
        };
        if offset + size > effective_byte_length {
            return Err(raise_range_error!("Offset is outside the bounds of the DataView"));
        }
        let start = self.byte_offset + offset;
        let end = start + size;
        if end > buffer_len {
            return Err(raise_range_error!("Offset is outside the bounds of the DataView"));
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

    pub fn get_float16(&self, offset: usize, little_endian: bool) -> Result<u16, JSError> {
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

    pub fn set_float16(&self, offset: usize, value: u16, little_endian: bool) -> Result<(), JSError> {
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

    pub fn get_bigint64(&self, offset: usize, little_endian: bool) -> Result<i64, JSError> {
        let idx = self.check_bounds(offset, 8)?;
        let buffer = self.buffer.borrow();
        let data = buffer.data.lock().unwrap();
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&data[idx..idx + 8]);
        Ok(if little_endian {
            i64::from_le_bytes(bytes)
        } else {
            i64::from_be_bytes(bytes)
        })
    }

    pub fn get_biguint64(&self, offset: usize, little_endian: bool) -> Result<u64, JSError> {
        let idx = self.check_bounds(offset, 8)?;
        let buffer = self.buffer.borrow();
        let data = buffer.data.lock().unwrap();
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&data[idx..idx + 8]);
        Ok(if little_endian {
            u64::from_le_bytes(bytes)
        } else {
            u64::from_be_bytes(bytes)
        })
    }

    pub fn set_bigint64(&self, offset: usize, value: i64, little_endian: bool) -> Result<(), JSError> {
        let idx = self.check_bounds(offset, 8)?;
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let buffer = self.buffer.borrow();
        let mut data = buffer.data.lock().unwrap();
        for i in 0..8 {
            data[idx + i] = bytes[i];
        }
        Ok(())
    }

    pub fn set_biguint64(&self, offset: usize, value: u64, little_endian: bool) -> Result<(), JSError> {
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
            TypedArrayKind::Int16 | TypedArrayKind::Uint16 | TypedArrayKind::Float16 => 2,
            TypedArrayKind::Int32 | TypedArrayKind::Uint32 | TypedArrayKind::Float32 => 4,
            TypedArrayKind::Float64 | TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => 8,
        }
    }

    /// Check if a usize index is valid for this TypedArray.
    /// Returns false if the TA is OOB (fixed-length after buffer shrink) or index >= current length.
    pub fn is_valid_integer_index(&self, idx: usize) -> bool {
        let buf_len = self.buffer.borrow().data.lock().unwrap().len();
        if self.length_tracking {
            if buf_len <= self.byte_offset {
                return false; // byte_offset exceeds buffer
            }
            let cur_len = (buf_len - self.byte_offset) / self.element_size();
            idx < cur_len
        } else {
            // Fixed-length: check if the whole TA is still in-bounds
            let needed = self.byte_offset + self.length * self.element_size();
            if needed > buf_len {
                return false; // TA is OOB
            }
            idx < self.length
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
            TypedArrayKind::Float16 => {
                let bytes = [data[byte_offset], data[byte_offset + 1]];
                Ok(f16_to_f64(u16::from_le_bytes(bytes)))
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
        // Per spec IntegerIndexedElementSet: if not IsValidIntegerIndex, silently return.
        if !self.is_valid_integer_index(idx) {
            return Ok(());
        }
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
                // Uint8ClampedArray: clamp to [0, 255], round half to even (banker's rounding)
                #[allow(clippy::if_same_then_else)]
                let v = if val.is_nan() {
                    0u8
                } else if val <= 0.0 {
                    0u8
                } else if val >= 255.0 {
                    255u8
                } else {
                    // Round half to even: 0.5 → 0, 1.5 → 2, 2.5 → 2, 3.5 → 4
                    let f = val.floor();
                    let frac = val - f;
                    let rounded = if frac > 0.5 {
                        f + 1.0
                    } else if frac < 0.5 {
                        f
                    } else {
                        // Exactly 0.5 — round to even
                        if (f as i64) % 2 == 0 { f } else { f + 1.0 }
                    };
                    rounded as u8
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
            TypedArrayKind::Float16 => {
                let b = f64_to_f16(val).to_le_bytes();
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

    /// Set a BigInt value directly using i64 (no f64 intermediary).
    /// This avoids precision loss for large BigInt values.
    pub fn set_bigint(&self, mc: &crate::core::MutationContext<'gc>, idx: usize, val: i64) -> Result<(), crate::error::JSError> {
        // Per spec IntegerIndexedElementSet: if not IsValidIntegerIndex, silently return.
        if !self.is_valid_integer_index(idx) {
            return Ok(());
        }
        let size = self.element_size();
        let byte_offset = self.byte_offset + idx * size;
        let buffer = self.buffer.borrow_mut(mc);
        let mut data = buffer.data.lock().unwrap();

        if byte_offset + size > data.len() {
            return Ok(());
        }

        match self.kind {
            TypedArrayKind::BigInt64 => {
                let b = val.to_le_bytes();
                data[byte_offset..byte_offset + 8].copy_from_slice(&b);
            }
            TypedArrayKind::BigUint64 => {
                let b = (val as u64).to_le_bytes();
                data[byte_offset..byte_offset + 8].copy_from_slice(&b);
            }
            _ => {
                // Fallback: convert to f64 for non-BigInt types
                self.set(mc, idx, val as f64)?;
            }
        }
        Ok(())
    }

    /// Read a BigInt typed array element as raw i64 (preserving all 64 bits).
    /// For BigUint64Array the bits are reinterpreted as i64 for storage/arithmetic;
    /// the caller must convert back to unsigned BigInt when returning to JS.
    pub fn get_bigint_raw(&self, idx: usize) -> Result<i64, crate::error::JSError> {
        let size = self.element_size();
        let byte_offset = self.byte_offset + idx * size;
        let buffer = self.buffer.borrow();
        let data = buffer.data.lock().unwrap();

        if byte_offset + size > data.len() {
            return Ok(0);
        }

        let mut b = [0u8; 8];
        b.copy_from_slice(&data[byte_offset..byte_offset + 8]);
        Ok(i64::from_le_bytes(b))
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
                    // Per spec: If IsSharedArrayBuffer(O) is true, throw TypeError
                    if (**ab).borrow().shared {
                        return Err(raise_type_error!(
                            "Method ArrayBuffer.prototype.byteLength called on incompatible receiver"
                        ));
                    }
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
                    // Per spec: If IsSharedArrayBuffer(O) is true, throw TypeError
                    if (**ab).borrow().shared {
                        return Err(raise_type_error!(
                            "Method ArrayBuffer.prototype.maxByteLength called on incompatible receiver"
                        ));
                    }
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
                    // Per spec: If IsSharedArrayBuffer(O) is true, throw TypeError
                    if (**ab).borrow().shared {
                        return Err(raise_type_error!(
                            "Method ArrayBuffer.prototype.resizable called on incompatible receiver"
                        ));
                    }
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

/// SharedArrayBuffer.prototype.byteLength getter — must be called on a SAB, not a regular AB.
pub fn handle_sharedarraybuffer_bytelength<'gc>(_mc: &MutationContext<'gc>, object: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    if let Some(ab_val) = slot_get_chained(object, &InternalSlot::ArrayBuffer)
        && let Value::ArrayBuffer(ab) = &*ab_val.borrow()
    {
        // Per spec: must be a SharedArrayBuffer. Reject non-shared.
        if !(**ab).borrow().shared {
            return Err(raise_type_error!(
                "Method SharedArrayBuffer.prototype.byteLength called on incompatible receiver"
            ));
        }
        let len = (**ab).borrow().data.lock().unwrap().len();
        Ok(Value::Number(len as f64))
    } else {
        Err(raise_type_error!(
            "Method SharedArrayBuffer.prototype.byteLength called on incompatible receiver"
        ))
    }
}

/// SharedArrayBuffer.prototype.maxByteLength getter
pub fn handle_sharedarraybuffer_maxbytelength<'gc>(
    _mc: &MutationContext<'gc>,
    object: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if let Some(ab_val) = slot_get_chained(object, &InternalSlot::ArrayBuffer)
        && let Value::ArrayBuffer(ab) = &*ab_val.borrow()
    {
        if !(**ab).borrow().shared {
            return Err(raise_type_error!(
                "Method SharedArrayBuffer.prototype.maxByteLength called on incompatible receiver"
            ));
        }
        let b = (**ab).borrow();
        let len = b.data.lock().unwrap().len();
        Ok(Value::Number(b.max_byte_length.unwrap_or(len) as f64))
    } else {
        Err(raise_type_error!(
            "Method SharedArrayBuffer.prototype.maxByteLength called on incompatible receiver"
        ))
    }
}

/// SharedArrayBuffer.prototype.growable getter
pub fn handle_sharedarraybuffer_growable<'gc>(_mc: &MutationContext<'gc>, object: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, JSError> {
    if let Some(ab_val) = slot_get_chained(object, &InternalSlot::ArrayBuffer)
        && let Value::ArrayBuffer(ab) = &*ab_val.borrow()
    {
        if !(**ab).borrow().shared {
            return Err(raise_type_error!(
                "Method SharedArrayBuffer.prototype.growable called on incompatible receiver"
            ));
        }
        Ok(Value::Boolean((**ab).borrow().max_byte_length.is_some()))
    } else {
        Err(raise_type_error!(
            "Method SharedArrayBuffer.prototype.growable called on incompatible receiver"
        ))
    }
}

/// SharedArrayBuffer.prototype.grow(newLength)
pub fn handle_sharedarraybuffer_grow<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    object: &JSObjectDataPtr<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, JSError> {
    // Step 1-2: RequireInternalSlot(O, [[ArrayBufferMaxByteLength]])
    let ab_val = slot_get_chained(object, &InternalSlot::ArrayBuffer)
        .ok_or_else(|| raise_type_error!("SharedArrayBuffer.prototype.grow called on incompatible receiver"))?;
    let ab = match &*ab_val.borrow() {
        Value::ArrayBuffer(ab) => *ab,
        _ => {
            return Err(raise_type_error!(
                "SharedArrayBuffer.prototype.grow called on incompatible receiver"
            ));
        }
    };

    // Step 3: If IsSharedArrayBuffer(O) is false, throw TypeError
    if !ab.borrow().shared {
        return Err(raise_type_error!(
            "SharedArrayBuffer.prototype.grow called on incompatible receiver"
        ));
    }

    // Must have [[ArrayBufferMaxByteLength]] (i.e., be growable)
    let max = ab.borrow().max_byte_length;
    if max.is_none() {
        return Err(raise_type_error!("SharedArrayBuffer is not growable"));
    }
    let max = max.unwrap();

    // Step 4: Let newByteLength be ? ToIntegerOrInfinity(newLength)
    let new_len_val = args.first().cloned().unwrap_or(Value::Undefined);
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

    // Step 5: If newByteLength < 0 or newByteLength > maxByteLength, throw RangeError
    if integer < 0.0 {
        return Err(raise_range_error!("new length must be a non-negative integer"));
    }
    if !integer.is_finite() || integer > max as f64 {
        return Err(raise_range_error!("new length exceeds maxByteLength"));
    }

    let new_len = integer as usize;

    // Step 11: If newByteLength < currentByteLength, throw RangeError (no shrinking)
    let ab_borrow = ab.borrow();
    let mut data = ab_borrow.data.lock().unwrap();
    let cur_len = data.len();
    if new_len < cur_len {
        return Err(raise_range_error!("SharedArrayBuffer cannot be shrunk"));
    }

    // Step 13: Grow the buffer
    if new_len > cur_len {
        data.resize(new_len, 0u8);
    }

    Ok(Value::Undefined)
}

/// SharedArrayBuffer.prototype.slice(start, end)
pub fn handle_sharedarraybuffer_slice<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    object: &JSObjectDataPtr<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, JSError> {
    // Step 1-3: Validate this is a SharedArrayBuffer
    let ab_val = slot_get_chained(object, &InternalSlot::ArrayBuffer)
        .ok_or_else(|| raise_type_error!("SharedArrayBuffer.prototype.slice called on incompatible receiver"))?;
    let ab_ref = ab_val.borrow();
    let ab = match &*ab_ref {
        Value::ArrayBuffer(ab) => *ab,
        _ => {
            return Err(raise_type_error!(
                "SharedArrayBuffer.prototype.slice called on incompatible receiver"
            ));
        }
    };
    if !ab.borrow().shared {
        return Err(raise_type_error!(
            "SharedArrayBuffer.prototype.slice called on incompatible receiver"
        ));
    }

    let data = ab.borrow().data.lock().unwrap().clone();
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
    let new_len = (end - start).max(0) as usize;

    // SpeciesConstructor(O, %SharedArrayBuffer%)
    let species_ctor = get_species_constructor(mc, env, object).map_err(JSError::from)?;

    let new_obj = if let Some(ctor) = species_ctor {
        let new_val = crate::js_class::evaluate_new(mc, env, &ctor, &[Value::Number(new_len as f64)], None)?;
        let new_obj = match new_val {
            Value::Object(o) => o,
            _ => return Err(raise_type_error!("Species constructor must return an object")),
        };

        // Must have [[ArrayBufferData]]
        if slot_get_chained(&new_obj, &InternalSlot::ArrayBuffer).is_none() {
            return Err(raise_type_error!(
                "SharedArrayBuffer species constructor returned a non-ArrayBuffer object"
            ));
        }

        // SameValue(new, O) check
        if Gc::ptr_eq(new_obj, *object) {
            return Err(raise_type_error!("SharedArrayBuffer species constructor returned the same buffer"));
        }

        // new.[[ArrayBufferByteLength]] < newLen check
        if let Some(new_ab_val) = slot_get_chained(&new_obj, &InternalSlot::ArrayBuffer)
            && let Value::ArrayBuffer(new_ab) = &*new_ab_val.borrow()
        {
            let new_byte_len = new_ab.borrow().data.lock().unwrap().len();
            if new_byte_len < new_len {
                return Err(raise_type_error!(
                    "SharedArrayBuffer species constructor returned a buffer that is too small"
                ));
            }
        }

        new_obj
    } else {
        // Default: create a new SharedArrayBuffer
        let new_ab = new_gc_cell_ptr(
            mc,
            JSArrayBuffer {
                data: Arc::new(Mutex::new(vec![0u8; new_len])),
                shared: true,
                ..JSArrayBuffer::default()
            },
        );
        let new_obj = new_js_object_data(mc);
        slot_set(mc, &new_obj, InternalSlot::ArrayBuffer, &Value::ArrayBuffer(new_ab));
        // Set prototype from SharedArrayBuffer.prototype
        if let Some(sab_ctor_val) = crate::core::env_get(env, "SharedArrayBuffer")
            && let Value::Object(sab_ctor) = &*sab_ctor_val.borrow()
            && let Some(proto_val) = object_get_key_value(sab_ctor, "prototype")
            && let Value::Object(proto) = &*proto_val.borrow()
        {
            new_obj.borrow_mut(mc).prototype = Some(*proto);
        } else {
            new_obj.borrow_mut(mc).prototype = object.borrow().prototype;
        }
        new_obj
    };

    // Copy bytes
    let start_usize = start as usize;
    let end_usize = (start + new_len as i64) as usize;
    let slice_bytes = &data[start_usize..end_usize.min(data.len())];

    if let Some(new_ab_val) = slot_get_chained(&new_obj, &InternalSlot::ArrayBuffer)
        && let Value::ArrayBuffer(new_ab) = &*new_ab_val.borrow()
    {
        let new_ab_ref = new_ab.borrow();
        let mut new_data = new_ab_ref.data.lock().unwrap();
        let copy_len = slice_bytes.len().min(new_data.len());
        new_data[..copy_len].copy_from_slice(&slice_bytes[..copy_len]);
    }

    Ok(Value::Object(new_obj))
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
                // Step 3: If IsSharedArrayBuffer(O) is true, throw a TypeError.
                if (**ab).borrow().shared {
                    return Err(raise_type_error!("ArrayBuffer.prototype.slice called on a SharedArrayBuffer"));
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
                let new_len = (final_end - start) as usize;

                // Step 13: Let ctor be ? SpeciesConstructor(O, %ArrayBuffer%).
                let species_ctor = get_species_constructor(mc, env, object).map_err(JSError::from)?;

                let new_obj = if let Some(ctor) = species_ctor {
                    // Step 15: Let new be ? Construct(ctor, «newLen»).
                    let new_val =
                        crate::js_class::evaluate_new(mc, env, &ctor, &[Value::Number(new_len as f64)], None).map_err(JSError::from)?;
                    let new_obj = match new_val {
                        Value::Object(o) => o,
                        _ => return Err(raise_type_error!("Species constructor must return an object")),
                    };

                    // Step 17: If new does not have [[ArrayBufferData]], throw TypeError.
                    if slot_get_chained(&new_obj, &InternalSlot::ArrayBuffer).is_none() {
                        return Err(raise_type_error!(
                            "ArrayBuffer species constructor returned a non-ArrayBuffer object"
                        ));
                    }

                    // Step 19: If SameValue(new, O) is true, throw TypeError.
                    if Gc::ptr_eq(new_obj, *object) {
                        return Err(raise_type_error!("ArrayBuffer species constructor returned the same buffer"));
                    }

                    // Step 20: If new.[[ArrayBufferByteLength]] < newLen, throw TypeError.
                    if let Some(new_ab_val) = slot_get_chained(&new_obj, &InternalSlot::ArrayBuffer)
                        && let Value::ArrayBuffer(new_ab) = &*new_ab_val.borrow()
                    {
                        let new_byte_len = new_ab.borrow().data.lock().unwrap().len();
                        if new_byte_len < new_len {
                            return Err(raise_type_error!(
                                "ArrayBuffer species constructor returned a buffer that is too small"
                            ));
                        }
                    }

                    new_obj
                } else {
                    // Default: create a new ArrayBuffer with newLen bytes
                    let new_ab = new_gc_cell_ptr(
                        mc,
                        JSArrayBuffer {
                            data: Arc::new(Mutex::new(vec![0u8; new_len])),
                            ..JSArrayBuffer::default()
                        },
                    );
                    let new_obj = new_js_object_data(mc);
                    slot_set(mc, &new_obj, InternalSlot::ArrayBuffer, &Value::ArrayBuffer(new_ab));
                    // Set prototype from ArrayBuffer.prototype
                    if let Some(ab_ctor_val) = crate::core::env_get(env, "ArrayBuffer")
                        && let Value::Object(ab_ctor) = &*ab_ctor_val.borrow()
                        && let Some(proto_val) = object_get_key_value(ab_ctor, "prototype")
                        && let Value::Object(proto) = &*proto_val.borrow()
                    {
                        new_obj.borrow_mut(mc).prototype = Some(*proto);
                    } else {
                        new_obj.borrow_mut(mc).prototype = object.borrow().prototype;
                    }
                    new_obj
                };

                // Step 24-25: Copy bytes from source to new buffer.
                let start_usize = start as usize;
                let end_usize = final_end as usize;
                let slice_bytes = &data[start_usize..end_usize];

                if let Some(new_ab_val) = slot_get_chained(&new_obj, &InternalSlot::ArrayBuffer)
                    && let Value::ArrayBuffer(new_ab) = &*new_ab_val.borrow()
                {
                    let new_ab_ref = new_ab.borrow();
                    let mut new_data = new_ab_ref.data.lock().unwrap();
                    let copy_len = slice_bytes.len().min(new_data.len());
                    new_data[..copy_len].copy_from_slice(&slice_bytes[..copy_len]);
                }

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

/// SpeciesConstructor(O, defaultConstructor) — returns Some(species) or None (use default).
/// Reads O.constructor, checks type, then checks Symbol.species.
fn get_species_constructor<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    let ctor = get_property_with_accessors(mc, env, obj, "constructor")?;

    if matches!(ctor, Value::Undefined) {
        return Ok(None); // use default constructor
    }

    let ctor_obj = match &ctor {
        Value::Object(o) => *o,
        _ => return Err(raise_type_error!("constructor is not an object or undefined").into()),
    };

    // Check Symbol.species on the constructor
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(species_sym_val) = object_get_key_value(sym_obj, "species")
        && let Value::Symbol(species_sym) = &*species_sym_val.borrow()
    {
        let species = get_property_with_accessors(mc, env, &ctor_obj, *species_sym)?;
        if matches!(species, Value::Undefined | Value::Null) {
            return Ok(None); // use default constructor
        }
        // Step 9: If IsConstructor(S) is true, return S.
        // Step 10: Throw a TypeError exception.
        let is_ctor = match &species {
            Value::Object(o) => {
                o.borrow().class_def.is_some()
                    || slot_get_chained(o, &InternalSlot::IsConstructor).is_some()
                    || slot_get_chained(o, &InternalSlot::NativeCtor).is_some()
                    || o.borrow().get_closure().is_some()
            }
            Value::Closure(cl) | Value::AsyncClosure(cl) => !cl.is_arrow,
            Value::Function(_) => true,
            _ => false,
        };
        if !is_ctor {
            return Err(raise_type_error!("Species constructor is not a constructor").into());
        }
        return Ok(Some(species));
    }

    // Symbol.species not available or not found — use default constructor
    Ok(None)
}

/// TypedArraySpeciesCreate: create a result TypedArray using SpeciesConstructor.
fn typed_array_species_create<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    exemplar: &JSObjectDataPtr<'gc>,
    length: usize,
) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    let species = get_species_constructor(mc, env, exemplar)?;
    if let Some(ctor) = species {
        // TypedArrayCreate(constructor, « length »)
        // Step 1: Let newTypedArray be ? Construct(constructor, argumentList).
        let new_val = crate::js_class::evaluate_new(mc, env, &ctor, &[Value::Number(length as f64)], None)?;
        let new_obj = match new_val {
            Value::Object(o) => o,
            _ => return Err(raise_type_error!("Species constructor did not return an object").into()),
        };
        // Step 2: Perform ? ValidateTypedArray(newTypedArray).
        if slot_get_chained(&new_obj, &InternalSlot::TypedArray).is_none() {
            return Err(raise_type_error!("TypedArray species constructor did not return a TypedArray").into());
        }
        // Steps 2-5: Check IsTypedArrayOutOfBounds and compare current length
        if let Some(ta_cell) = slot_get_chained(&new_obj, &InternalSlot::TypedArray)
            && let Value::TypedArray(rta) = &*ta_cell.borrow()
        {
            // Check detached
            if rta.buffer.borrow().detached {
                return Err(raise_type_error!("TypedArray species constructor returned a detached TypedArray").into());
            }
            // Check OOB
            let buf_len = rta.buffer.borrow().data.lock().unwrap().len();
            let oob = if rta.length_tracking {
                rta.byte_offset > buf_len
            } else {
                rta.byte_offset + rta.length * rta.element_size() > buf_len
            };
            if oob {
                return Err(raise_type_error!("TypedArray species constructor returned an out-of-bounds TypedArray").into());
            }
            // Compute current length (TypedArrayLength)
            let current_len = if rta.length_tracking {
                buf_len.saturating_sub(rta.byte_offset) / rta.element_size()
            } else {
                rta.length
            };
            // Step 5: If newLength < requested length, throw TypeError
            if current_len < length {
                return Err(raise_type_error!("TypedArray species constructor returned a TypedArray that is too small").into());
            }
        }
        Ok(new_obj)
    } else {
        create_same_type_typedarray(mc, env, exemplar, length).map_err(|e| e.into())
    }
}

/// Create a new TypedArray of the same kind as the source, with the given length
fn create_same_type_typedarray<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    source_obj: &JSObjectDataPtr<'gc>,
    length: usize,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    if let Some(ta_cell) = slot_get_chained(source_obj, &InternalSlot::TypedArray)
        && let Value::TypedArray(src_ta) = &*ta_cell.borrow()
    {
        let kind = src_ta.kind.clone();
        let element_size = src_ta.element_size();
        let byte_len = length * element_size;

        // Create a new ArrayBuffer
        let buffer = new_gc_cell_ptr(
            mc,
            JSArrayBuffer {
                data: Arc::new(Mutex::new(vec![0; byte_len])),
                ..JSArrayBuffer::default()
            },
        );

        // Wrap buffer in a JS object
        let buf_obj = new_js_object_data(mc);
        if let Some(ab_val) = crate::core::env_get(env, "ArrayBuffer")
            && let Value::Object(ab_ctor) = &*ab_val.borrow()
            && let Some(proto_val) = object_get_key_value(ab_ctor, "prototype")
            && let Value::Object(ab_proto) = &*proto_val.borrow()
        {
            buf_obj.borrow_mut(mc).prototype = Some(*ab_proto);
        }
        slot_set(mc, &buf_obj, InternalSlot::ArrayBuffer, &Value::ArrayBuffer(buffer));

        let new_ta = JSTypedArray {
            buffer,
            kind,
            byte_offset: 0,
            length,
            length_tracking: false,
        };

        let result_obj = new_js_object_data(mc);
        // Copy prototype from source
        if let Some(proto) = source_obj.borrow().prototype {
            result_obj.borrow_mut(mc).prototype = Some(proto);
        }
        slot_set(mc, &result_obj, InternalSlot::TypedArray, &Value::TypedArray(Gc::new(mc, new_ta)));
        slot_set(mc, &result_obj, InternalSlot::BufferObject, &Value::Object(buf_obj));
        Ok(result_obj)
    } else {
        Err(raise_type_error!("Source is not a TypedArray"))
    }
}

pub fn handle_typedarray_accessor<'gc>(
    _mc: &MutationContext<'gc>,
    object: &JSObjectDataPtr<'gc>,
    property: &str,
) -> Result<Value<'gc>, JSError> {
    // Per spec, TypedArray prototype accessors check the receiver's OWN
    // [[TypedArrayName]] internal slot — they must NOT walk the prototype chain.
    if let Some(ta_val) = slot_get(object, &InternalSlot::TypedArray) {
        if let Value::TypedArray(ta) = &*ta_val.borrow() {
            let is_detached = ta.buffer.borrow().detached;
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
                    if is_detached {
                        return Ok(Value::Number(0.0));
                    }
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
                "byteOffset" => {
                    if is_detached {
                        return Ok(Value::Number(0.0));
                    }
                    // Per spec: if IsTypedArrayOutOfBounds, return 0
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    if ta.length_tracking {
                        if ta.byte_offset > buf_len {
                            return Ok(Value::Number(0.0));
                        }
                    } else {
                        let needed = ta.byte_offset + ta.length * ta.element_size();
                        if needed > buf_len {
                            return Ok(Value::Number(0.0));
                        }
                    }
                    Ok(Value::Number(ta.byte_offset as f64))
                }
                "length" => {
                    if is_detached {
                        return Ok(Value::Number(0.0));
                    }
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
                        TypedArrayKind::Float16 => "Float16Array",
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
            // toStringTag: return undefined for non-TypedArray receivers per spec
            if property == "toStringTag" {
                return Ok(Value::Undefined);
            }
            Err(raise_type_error!(
                "Method TypedArray.prototype getter called on incompatible receiver"
            ))
        }
    } else {
        // toStringTag: return undefined for non-TypedArray receivers per spec
        if property == "toStringTag" {
            return Ok(Value::Undefined);
        }
        Err(raise_type_error!(
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

/// TypedArrayCreate(constructor, argumentList) — ES2024 §23.2.4.1
/// 1. Let newTypedArray be ? Construct(constructor, argumentList).
/// 2. Perform ? ValidateTypedArray(newTypedArray).
/// 3. If argumentList is a List of a single Number, check newTypedArray.length >= argumentList[0].
fn typedarray_create<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    constructor: &Value<'gc>,
    args: &[Value<'gc>],
    expected_len: Option<usize>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 1: Construct(constructor, argumentList)
    let new_ta = crate::js_class::evaluate_new(mc, env, constructor, args, Some(constructor))?;

    // Step 2: ValidateTypedArray(newTypedArray)
    let ta_obj = match &new_ta {
        Value::Object(o) => o,
        _ => return Err(throw_type_error(mc, env, "TypedArray expected")),
    };
    if slot_get(ta_obj, &InternalSlot::TypedArray).is_none() {
        return Err(throw_type_error(mc, env, "result is not a TypedArray"));
    }

    // Step 3: If argumentList has one Number, check length >= that number
    if let Some(expected) = expected_len
        && let Some(ta_cell) = slot_get(ta_obj, &InternalSlot::TypedArray)
        && let Value::TypedArray(ta) = &*ta_cell.borrow()
        && ta.length < expected
    {
        return Err(throw_type_error(mc, env, "TypedArray is too small"));
    }

    Ok(new_ta)
}

/// Handle %TypedArray%.from() and %TypedArray%.of() static methods
pub fn handle_typedarray_static_method<'gc>(
    mc: &MutationContext<'gc>,
    this_val: &Value<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // `this` must be a constructor (e.g., Int8Array, Float64Array)
    let ctor_obj = match this_val {
        Value::Object(o) => *o,
        _ => return Err(raise_type_error!("TypedArray.from/of requires a constructor as this value").into()),
    };

    // IsConstructor check: must be a constructor function
    let is_ctor = if let Some(v) = slot_get(&ctor_obj, &InternalSlot::IsConstructor) {
        matches!(&*v.borrow(), Value::Boolean(true))
    } else if ctor_obj.borrow().get_closure().is_some() {
        // Closures with .prototype are constructors (regular functions)
        // Arrow functions and concise methods lack .prototype
        object_get_key_value(&ctor_obj, "prototype").is_some()
    } else {
        false
    };
    if !is_ctor {
        return Err(raise_type_error!("TypedArray.from/of: this is not a constructor").into());
    }

    // Check if this is a TypedArray constructor by looking for Kind slot
    let _has_kind = slot_get(&ctor_obj, &InternalSlot::Kind).is_some();

    match method {
        "from" => {
            // %TypedArray%.from(source [, mapfn [, thisArg]])
            let source = args.first().cloned().unwrap_or(Value::Undefined);
            let map_fn = args.get(1).cloned();
            let this_arg = args.get(2).cloned().unwrap_or(Value::Undefined);

            // Validate mapfn
            let mapper = if let Some(ref fn_val) = map_fn {
                if matches!(fn_val, Value::Undefined) {
                    None
                } else {
                    let callable = match fn_val {
                        Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) => true,
                        Value::Object(o) => o.borrow().get_closure().is_some() || slot_get(o, &InternalSlot::BoundTarget).is_some(),
                        _ => false,
                    };
                    if !callable {
                        return Err(raise_type_error!("TypedArray.from mapfn is not callable").into());
                    }
                    Some(fn_val.clone())
                }
            } else {
                None
            };

            let mut values: Vec<Value<'gc>> = Vec::new();

            match &source {
                Value::Object(src_obj) => {
                    // Try iterator first
                    let mut used_iterator = false;
                    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
                        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                        && let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator")
                        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
                    {
                        // Propagate errors from accessing @@iterator (e.g., getter that throws)
                        let iter_fn = get_property_with_accessors(mc, env, src_obj, *iter_sym)?;
                        if !matches!(iter_fn, Value::Undefined | Value::Null) {
                            used_iterator = true;
                            // ES2024 §23.2.2.1 step 5a: IteratorToList — collect ALL values first
                            let iterator = crate::core::evaluate_call_dispatch(mc, env, &iter_fn, Some(&Value::Object(*src_obj)), &[])?;
                            if let Value::Object(iter_obj) = iterator {
                                loop {
                                    let next_fn = get_property_with_accessors(mc, env, &iter_obj, "next")?;
                                    let next_res =
                                        crate::core::evaluate_call_dispatch(mc, env, &next_fn, Some(&Value::Object(iter_obj)), &[])?;
                                    if let Value::Object(next_obj) = next_res {
                                        let done_val = get_property_with_accessors(mc, env, &next_obj, "done")?;
                                        if done_val.to_truthy() {
                                            break;
                                        }
                                        let value = get_property_with_accessors(mc, env, &next_obj, "value")?;
                                        values.push(value);
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    // Iterator path: ES2024 §23.2.2.1 step 6
                    // Step 6c: TypedArrayCreate after collecting raw values
                    if used_iterator {
                        let expected_len = values.len();
                        let len_val = Value::Number(expected_len as f64);
                        let new_ta = typedarray_create(mc, env, this_val, &[len_val], Some(expected_len))?;

                        // Step 6e: loop — map + set interleaved
                        if let Value::Object(ta_obj) = &new_ta
                            && let Some(ta_cell) = slot_get(ta_obj, &InternalSlot::TypedArray)
                            && let Value::TypedArray(ta) = &*ta_cell.borrow()
                        {
                            let is_bigint_ta = is_bigint_typed_array(&ta.kind);
                            for (i, val) in values.iter().enumerate() {
                                let mapped = if let Some(mfn) = &mapper {
                                    crate::js_promise::call_function_with_this(
                                        mc,
                                        mfn,
                                        Some(&this_arg),
                                        &[val.clone(), Value::Number(i as f64)],
                                        env,
                                    )?
                                } else {
                                    val.clone()
                                };
                                if is_bigint_ta {
                                    let n = match &mapped {
                                        Value::BigInt(b) => bigint_to_i64_modular(b),
                                        _ => to_bigint_i64(mc, env, &mapped)?,
                                    };
                                    ta.set_bigint(mc, i, n)?;
                                } else {
                                    let n = crate::core::to_number_with_env(mc, env, &mapped)?;
                                    ta.set(mc, i, n)?;
                                }
                            }
                        }
                        return Ok(new_ta);
                    }

                    if !used_iterator {
                        // Array-like source — spec step 10-12: create TA first, then iterate and set
                        let len_val_raw = get_property_with_accessors(mc, env, src_obj, "length")?;
                        let len = crate::core::to_number_with_env(mc, env, &len_val_raw)?.max(0.0) as usize;

                        // Step 10: TypedArrayCreate(C, «len»)
                        let len_val = Value::Number(len as f64);
                        let new_ta = typedarray_create(mc, env, this_val, &[len_val], Some(len))?;

                        // Step 12: iterate and set each element
                        if let Value::Object(ta_obj) = &new_ta
                            && let Some(ta_cell) = slot_get(ta_obj, &InternalSlot::TypedArray)
                            && let Value::TypedArray(ta) = &*ta_cell.borrow()
                        {
                            let is_bigint_ta = is_bigint_typed_array(&ta.kind);
                            for i in 0..len {
                                let val = get_property_with_accessors(mc, env, src_obj, i)?;
                                let mapped = if let Some(ref mfn) = mapper {
                                    crate::js_promise::call_function_with_this(
                                        mc,
                                        mfn,
                                        Some(&this_arg),
                                        &[val, Value::Number(i as f64)],
                                        env,
                                    )?
                                } else {
                                    val
                                };
                                if is_bigint_ta {
                                    let n = match &mapped {
                                        Value::BigInt(b) => bigint_to_i64_modular(b),
                                        _ => to_bigint_i64(mc, env, &mapped)?,
                                    };
                                    ta.set_bigint(mc, i, n)?;
                                } else {
                                    let n = crate::core::to_number_with_env(mc, env, &mapped)?;
                                    ta.set(mc, i, n)?;
                                }
                            }
                        }

                        return Ok(new_ta);
                    }
                }
                Value::String(s) => {
                    for (i, ch) in s.iter().enumerate() {
                        let ch_val = Value::String(vec![*ch]);
                        let mapped = if let Some(ref mfn) = mapper {
                            crate::js_promise::call_function_with_this(mc, mfn, Some(&this_arg), &[ch_val, Value::Number(i as f64)], env)?
                        } else {
                            ch_val
                        };
                        values.push(mapped);
                    }
                }
                _ => {
                    return Err(raise_type_error!("TypedArray.from requires an array-like or iterable").into());
                }
            }

            // TypedArrayCreate(C, «len»): Construct(C, [len]) then ValidateTypedArray
            let expected_len = values.len();
            let len_val = Value::Number(expected_len as f64);
            let new_ta = typedarray_create(mc, env, this_val, &[len_val], Some(expected_len))?;

            // Set elements via ToNumber/ToBigInt (per spec, Set(newObj, Pk, kValue, true))
            if let Value::Object(ta_obj) = &new_ta
                && let Some(ta_cell) = slot_get(ta_obj, &InternalSlot::TypedArray)
                && let Value::TypedArray(ta) = &*ta_cell.borrow()
            {
                let is_bigint_ta = is_bigint_typed_array(&ta.kind);
                for (i, val) in values.iter().enumerate() {
                    if is_bigint_ta {
                        let n = match val {
                            Value::BigInt(b) => bigint_to_i64_modular(b),
                            _ => to_bigint_i64(mc, env, val)?,
                        };
                        ta.set_bigint(mc, i, n)?;
                    } else {
                        let n = crate::core::to_number_with_env(mc, env, val)?;
                        ta.set(mc, i, n)?;
                    };
                }
            }
            Ok(new_ta)
        }
        "of" => {
            // %TypedArray%.of(...items)
            // TypedArrayCreate(C, «len»): Construct(C, [len]) then ValidateTypedArray
            let expected_len = args.len();
            let len_val = Value::Number(expected_len as f64);
            let new_ta = typedarray_create(mc, env, this_val, &[len_val], Some(expected_len))?;

            if let Value::Object(ta_obj) = &new_ta
                && let Some(ta_cell) = slot_get(ta_obj, &InternalSlot::TypedArray)
                && let Value::TypedArray(ta) = &*ta_cell.borrow()
            {
                let is_bigint_ta = is_bigint_typed_array(&ta.kind);
                for (i, val) in args.iter().enumerate() {
                    if is_bigint_ta {
                        let n = match val {
                            Value::BigInt(b) => bigint_to_i64_modular(b),
                            _ => to_bigint_i64(mc, env, val)?,
                        };
                        ta.set_bigint(mc, i, n)?;
                    } else {
                        let n = crate::core::to_number_with_env(mc, env, val)?;
                        ta.set(mc, i, n)?;
                    };
                }
            }
            Ok(new_ta)
        }
        _ => Err(raise_type_error!(format!("TypedArray.{} is not a function", method)).into()),
    }
}

pub fn handle_typedarray_method<'gc>(
    mc: &MutationContext<'gc>,
    this_val: &Value<'gc>,
    method: &str,
    _args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if let Value::Object(obj) = this_val {
        if let Some(ta_cell) = slot_get(obj, &InternalSlot::TypedArray)
            && let Value::TypedArray(_ta) = &*ta_cell.borrow()
        {
            let ta = *_ta;
            let is_bigint = is_bigint_typed_array(&ta.kind);

            // ValidateTypedArray: check if buffer is detached or out-of-bounds
            // Per spec, subarray does NOT call ValidateTypedArray (it coerces args
            // first, then the species constructor checks detached).
            if method != "subarray" {
                if ta.buffer.borrow().detached {
                    return Err(raise_type_error!("Cannot perform operation on a detached ArrayBuffer").into());
                }
                // IsTypedArrayOutOfBounds check for resizable buffers
                let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                if ta.length_tracking {
                    if ta.byte_offset > buf_len {
                        return Err(raise_type_error!("TypedArray is out of bounds").into());
                    }
                } else {
                    let needed = ta.byte_offset + ta.length * ta.element_size();
                    if needed > buf_len {
                        return Err(raise_type_error!("TypedArray is out of bounds").into());
                    }
                }
            }

            // Helper: get the current length (handles length-tracking and resizable buffers)
            let get_len = || -> usize {
                if ta.length_tracking {
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    buf_len.saturating_sub(ta.byte_offset) / ta.element_size()
                } else {
                    // Fixed-length: check if the TA is still in-bounds after possible resize
                    let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                    let needed = ta.byte_offset + ta.length * ta.element_size();
                    if needed > buf_len { 0 } else { ta.length }
                }
            };

            // Helper: re-validate after argument coercion (which may resize the buffer).
            // Throws TypeError if detached or out-of-bounds. Returns current length.
            let recheck_oob = || -> Result<usize, EvalError<'gc>> {
                if ta.buffer.borrow().detached {
                    return Err(raise_type_error!("Cannot perform operation on a detached ArrayBuffer").into());
                }
                let buf_len = ta.buffer.borrow().data.lock().unwrap().len();
                if ta.length_tracking {
                    if ta.byte_offset > buf_len {
                        return Err(raise_type_error!("TypedArray is out of bounds").into());
                    }
                    Ok(buf_len.saturating_sub(ta.byte_offset) / ta.element_size())
                } else {
                    let needed = ta.byte_offset + ta.length * ta.element_size();
                    if needed > buf_len {
                        return Err(raise_type_error!("TypedArray is out of bounds").into());
                    }
                    Ok(ta.length)
                }
            };

            // Helper: read element at index as Value (BigInt for BigInt arrays, Number otherwise)
            // Uses IsValidIntegerIndex to handle detached buffers and OOB after resize.
            let get_val = |idx: usize| -> Result<Value<'gc>, JSError> {
                if !is_valid_integer_index(&ta, idx as f64) {
                    return Ok(Value::Undefined);
                }
                if is_bigint {
                    let size = ta.element_size();
                    let byte_offset = ta.byte_offset + idx * size;
                    let buffer = ta.buffer.borrow();
                    let data = buffer.data.lock().unwrap();
                    if byte_offset + size > data.len() {
                        return Ok(Value::Undefined);
                    }
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&data[byte_offset..byte_offset + 8]);
                    let big = if matches!(ta.kind, TypedArrayKind::BigInt64) {
                        num_bigint::BigInt::from(i64::from_le_bytes(b))
                    } else {
                        num_bigint::BigInt::from(u64::from_le_bytes(b))
                    };
                    Ok(Value::BigInt(Box::new(big)))
                } else {
                    Ok(Value::Number(ta.get(idx)?))
                }
            };

            // Helper: coerce relative-index argument to absolute index (fallible for Symbol etc.)
            let relative_index = |arg: Option<&Value<'gc>>, default: i64, _len: usize| -> Result<i64, EvalError<'gc>> {
                match arg {
                    Some(Value::Number(n)) => {
                        let n = *n;
                        Ok(if n.is_nan() { 0 } else { n as i64 })
                    }
                    Some(Value::Undefined) | None => Ok(default),
                    Some(v) => {
                        let n = crate::core::to_number_with_env(mc, _env, v)?;
                        Ok(if n.is_nan() { 0 } else { n as i64 })
                    }
                }
            };

            let resolve_index = |rel: i64, len: usize| -> usize {
                if rel < 0 {
                    (len as i64 + rel).max(0) as usize
                } else {
                    (rel as usize).min(len)
                }
            };

            match method {
                // Methods that require a callable first argument
                "every" | "some" | "find" | "findIndex" | "findLast" | "findLastIndex" | "forEach" | "map" | "filter" | "reduce"
                | "reduceRight" => {
                    let callback = _args.first().cloned().unwrap_or(Value::Undefined);
                    let is_callable = match &callback {
                        Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) => true,
                        Value::Object(o) => o.borrow().get_closure().is_some() || slot_get(o, &InternalSlot::BoundTarget).is_some(),
                        _ => false,
                    };
                    if !is_callable {
                        return Err(raise_type_error!("callback is not a function").into());
                    }
                }
                _ => {}
            }

            match method {
                "values" | "keys" | "entries" => {
                    let kind = match method {
                        "keys" => "keys",
                        "entries" => "entries",
                        _ => "values",
                    };
                    Ok(crate::js_array::create_array_iterator(mc, _env, *obj, kind)?)
                }
                "fill" => {
                    let len = get_len();
                    let fill_val = _args.first().cloned().unwrap_or(Value::Undefined);
                    let start_rel = relative_index(_args.get(1), 0, len)?;
                    let end_rel = relative_index(_args.get(2), len as i64, len)?;
                    if is_bigint {
                        let fill_i64 = match &fill_val {
                            Value::BigInt(b) => bigint_to_i64_modular(b),
                            _ => {
                                // ToBigInt coercion
                                crate::js_typedarray::to_bigint_i64(mc, _env, &fill_val)?
                            }
                        };
                        // Per spec: recheck OOB after value/start/end coercion
                        let len = recheck_oob()?;
                        let start = resolve_index(start_rel, len);
                        let end = resolve_index(end_rel, len);
                        for i in start..end {
                            ta.set_bigint(mc, i, fill_i64)?;
                        }
                    } else {
                        let fill_f64 = crate::core::to_number_with_env(mc, _env, &fill_val)?;
                        // Per spec: recheck OOB after value/start/end coercion
                        let len = recheck_oob()?;
                        let start = resolve_index(start_rel, len);
                        let end = resolve_index(end_rel, len);
                        for i in start..end {
                            ta.set(mc, i, fill_f64)?;
                        }
                    };
                    Ok(Value::Object(*obj))
                }
                "at" => {
                    let len = get_len();
                    let idx_arg = _args.first().cloned().unwrap_or(Value::Undefined);
                    let rel = crate::core::to_number_with_env(mc, _env, &idx_arg)?;
                    let rel = if rel.is_nan() { 0i64 } else { rel as i64 };
                    let actual = if rel < 0 { len as i64 + rel } else { rel };
                    if actual < 0 || actual as usize >= len {
                        Ok(Value::Undefined)
                    } else {
                        Ok(get_val(actual as usize)?)
                    }
                }
                "every" => {
                    let len = get_len();
                    let callback = _args.first().cloned().unwrap_or(Value::Undefined);
                    let this_arg = _args.get(1).cloned().unwrap_or(Value::Undefined);
                    for i in 0..len {
                        let val = get_val(i)?;
                        let result = crate::js_promise::call_function_with_this(
                            mc,
                            &callback,
                            Some(&this_arg),
                            &[val, Value::Number(i as f64), Value::Object(*obj)],
                            _env,
                        )?;
                        if !result.to_truthy() {
                            return Ok(Value::Boolean(false));
                        }
                    }
                    Ok(Value::Boolean(true))
                }
                "some" => {
                    let len = get_len();
                    let callback = _args.first().cloned().unwrap_or(Value::Undefined);
                    let this_arg = _args.get(1).cloned().unwrap_or(Value::Undefined);
                    for i in 0..len {
                        let val = get_val(i)?;
                        let result = crate::js_promise::call_function_with_this(
                            mc,
                            &callback,
                            Some(&this_arg),
                            &[val, Value::Number(i as f64), Value::Object(*obj)],
                            _env,
                        )?;
                        if result.to_truthy() {
                            return Ok(Value::Boolean(true));
                        }
                    }
                    Ok(Value::Boolean(false))
                }
                "find" => {
                    let len = get_len();
                    let callback = _args.first().cloned().unwrap_or(Value::Undefined);
                    let this_arg = _args.get(1).cloned().unwrap_or(Value::Undefined);
                    for i in 0..len {
                        let val = get_val(i)?;
                        let result = crate::js_promise::call_function_with_this(
                            mc,
                            &callback,
                            Some(&this_arg),
                            &[val.clone(), Value::Number(i as f64), Value::Object(*obj)],
                            _env,
                        )?;
                        if result.to_truthy() {
                            return Ok(val);
                        }
                    }
                    Ok(Value::Undefined)
                }
                "findIndex" => {
                    let len = get_len();
                    let callback = _args.first().cloned().unwrap_or(Value::Undefined);
                    let this_arg = _args.get(1).cloned().unwrap_or(Value::Undefined);
                    for i in 0..len {
                        let val = get_val(i)?;
                        let result = crate::js_promise::call_function_with_this(
                            mc,
                            &callback,
                            Some(&this_arg),
                            &[val, Value::Number(i as f64), Value::Object(*obj)],
                            _env,
                        )?;
                        if result.to_truthy() {
                            return Ok(Value::Number(i as f64));
                        }
                    }
                    Ok(Value::Number(-1.0))
                }
                "findLast" => {
                    let len = get_len();
                    let callback = _args.first().cloned().unwrap_or(Value::Undefined);
                    let this_arg = _args.get(1).cloned().unwrap_or(Value::Undefined);
                    for i in (0..len).rev() {
                        let val = get_val(i)?;
                        let result = crate::js_promise::call_function_with_this(
                            mc,
                            &callback,
                            Some(&this_arg),
                            &[val.clone(), Value::Number(i as f64), Value::Object(*obj)],
                            _env,
                        )?;
                        if result.to_truthy() {
                            return Ok(val);
                        }
                    }
                    Ok(Value::Undefined)
                }
                "findLastIndex" => {
                    let len = get_len();
                    let callback = _args.first().cloned().unwrap_or(Value::Undefined);
                    let this_arg = _args.get(1).cloned().unwrap_or(Value::Undefined);
                    for i in (0..len).rev() {
                        let val = get_val(i)?;
                        let result = crate::js_promise::call_function_with_this(
                            mc,
                            &callback,
                            Some(&this_arg),
                            &[val, Value::Number(i as f64), Value::Object(*obj)],
                            _env,
                        )?;
                        if result.to_truthy() {
                            return Ok(Value::Number(i as f64));
                        }
                    }
                    Ok(Value::Number(-1.0))
                }
                "forEach" => {
                    let len = get_len();
                    let callback = _args.first().cloned().unwrap_or(Value::Undefined);
                    let this_arg = _args.get(1).cloned().unwrap_or(Value::Undefined);
                    for i in 0..len {
                        let val = get_val(i)?;
                        crate::js_promise::call_function_with_this(
                            mc,
                            &callback,
                            Some(&this_arg),
                            &[val, Value::Number(i as f64), Value::Object(*obj)],
                            _env,
                        )?;
                    }
                    Ok(Value::Undefined)
                }
                "map" => {
                    let len = get_len();
                    let callback = _args.first().cloned().unwrap_or(Value::Undefined);
                    let this_arg = _args.get(1).cloned().unwrap_or(Value::Undefined);
                    // TypedArraySpeciesCreate: use SpeciesConstructor to create result
                    let result_ta = typed_array_species_create(mc, _env, obj, len)?;
                    if let Some(rta_cell) = slot_get_chained(&result_ta, &InternalSlot::TypedArray)
                        && let Value::TypedArray(rta) = &*rta_cell.borrow()
                    {
                        for i in 0..len {
                            let val = get_val(i)?;
                            let mapped = crate::js_promise::call_function_with_this(
                                mc,
                                &callback,
                                Some(&this_arg),
                                &[val, Value::Number(i as f64), Value::Object(*obj)],
                                _env,
                            )?;
                            let n = if is_bigint {
                                match &mapped {
                                    Value::BigInt(b) => bigint_to_i64_modular(b),
                                    _ => to_bigint_i64(mc, _env, &mapped)?,
                                }
                            } else {
                                0i64 // unused
                            };
                            if is_bigint {
                                rta.set_bigint(mc, i, n)?;
                            } else {
                                let nf = crate::core::to_number_with_env(mc, _env, &mapped)?;
                                rta.set(mc, i, nf)?;
                            }
                        }
                    }
                    Ok(Value::Object(result_ta))
                }
                "filter" => {
                    let len = get_len();
                    let callback = _args.first().cloned().unwrap_or(Value::Undefined);
                    let this_arg = _args.get(1).cloned().unwrap_or(Value::Undefined);
                    let mut kept: Vec<Value<'gc>> = Vec::new();
                    for i in 0..len {
                        let val = get_val(i)?;
                        let result = crate::js_promise::call_function_with_this(
                            mc,
                            &callback,
                            Some(&this_arg),
                            &[val.clone(), Value::Number(i as f64), Value::Object(*obj)],
                            _env,
                        )?;
                        if result.to_truthy() {
                            kept.push(val);
                        }
                    }
                    let result_ta = typed_array_species_create(mc, _env, obj, kept.len())?;
                    if let Some(rta_cell) = slot_get_chained(&result_ta, &InternalSlot::TypedArray)
                        && let Value::TypedArray(rta) = &*rta_cell.borrow()
                    {
                        for (i, val) in kept.iter().enumerate() {
                            if is_bigint {
                                let n = match val {
                                    Value::BigInt(b) => bigint_to_i64_modular(b),
                                    _ => 0,
                                };
                                rta.set_bigint(mc, i, n)?;
                            } else {
                                let n = crate::core::to_number(val).unwrap_or(0.0);
                                rta.set(mc, i, n)?;
                            };
                        }
                    }
                    Ok(Value::Object(result_ta))
                }
                "reduce" => {
                    let len = get_len();
                    let callback = _args.first().cloned().unwrap_or(Value::Undefined);
                    let mut accumulator = if _args.len() >= 2 {
                        _args[1].clone()
                    } else {
                        if len == 0 {
                            return Err(raise_type_error!("Reduce of empty array with no initial value").into());
                        }
                        get_val(0)?
                    };
                    let start_idx = if _args.len() >= 2 { 0 } else { 1 };
                    for i in start_idx..len {
                        let val = get_val(i)?;
                        accumulator = crate::js_promise::call_function_with_this(
                            mc,
                            &callback,
                            Some(&Value::Undefined),
                            &[accumulator, val, Value::Number(i as f64), Value::Object(*obj)],
                            _env,
                        )?;
                    }
                    Ok(accumulator)
                }
                "reduceRight" => {
                    let len = get_len();
                    let callback = _args.first().cloned().unwrap_or(Value::Undefined);
                    let mut accumulator = if _args.len() >= 2 {
                        _args[1].clone()
                    } else {
                        if len == 0 {
                            return Err(raise_type_error!("Reduce of empty array with no initial value").into());
                        }
                        get_val(len - 1)?
                    };
                    let end_idx = if _args.len() >= 2 { len } else { len - 1 };
                    for i in (0..end_idx).rev() {
                        let val = get_val(i)?;
                        accumulator = crate::js_promise::call_function_with_this(
                            mc,
                            &callback,
                            Some(&Value::Undefined),
                            &[accumulator, val, Value::Number(i as f64), Value::Object(*obj)],
                            _env,
                        )?;
                    }
                    Ok(accumulator)
                }
                "indexOf" => {
                    let len = get_len();
                    if len == 0 {
                        return Ok(Value::Number(-1.0));
                    }
                    let search = _args.first().cloned().unwrap_or(Value::Undefined);
                    let from = if let Some(f) = _args.get(1) {
                        let n = crate::core::to_number_with_env(mc, _env, f)?;
                        if n.is_nan() { 0i64 } else { n as i64 }
                    } else {
                        0
                    };
                    // Use original len for negative index resolution (no recheck_oob)
                    let start = if from < 0 {
                        (len as i64 + from).max(0) as usize
                    } else {
                        from as usize
                    };
                    for i in start..len {
                        // Per spec step 11a: HasProperty via IsValidIntegerIndex
                        if ta.is_valid_integer_index(i) {
                            let val = get_val(i)?;
                            let eq = match (&val, &search) {
                                (Value::Number(a), Value::Number(b)) => a == b,
                                (Value::BigInt(a), Value::BigInt(b)) => **a == **b,
                                (Value::String(a), Value::String(b)) => a == b,
                                (Value::Boolean(a), Value::Boolean(b)) => a == b,
                                (Value::Null, Value::Null) => true,
                                (Value::Undefined, Value::Undefined) => true,
                                (Value::Object(a), Value::Object(b)) => Gc::ptr_eq(*a, *b),
                                _ => false,
                            };
                            if eq {
                                return Ok(Value::Number(i as f64));
                            }
                        }
                    }
                    Ok(Value::Number(-1.0))
                }
                "lastIndexOf" => {
                    let len = get_len();
                    if len == 0 {
                        return Ok(Value::Number(-1.0));
                    }
                    let search = _args.first().cloned().unwrap_or(Value::Undefined);
                    let from = if let Some(f) = _args.get(1) {
                        let n = crate::core::to_number_with_env(mc, _env, f)?;
                        if n.is_nan() { 0i64 } else { n as i64 }
                    } else {
                        len as i64 - 1
                    };
                    // Use original len for index resolution (no recheck_oob)
                    let k = if from >= 0 {
                        (from as usize).min(len - 1)
                    } else {
                        let k_signed = len as i64 + from;
                        if k_signed < 0 {
                            return Ok(Value::Number(-1.0));
                        }
                        k_signed as usize
                    };
                    for i in (0..=k).rev() {
                        // Per spec: HasProperty via IsValidIntegerIndex before comparing
                        if ta.is_valid_integer_index(i) {
                            let val = get_val(i)?;
                            let eq = match (&val, &search) {
                                (Value::Number(a), Value::Number(b)) => a == b,
                                (Value::BigInt(a), Value::BigInt(b)) => **a == **b,
                                (Value::String(a), Value::String(b)) => a == b,
                                (Value::Boolean(a), Value::Boolean(b)) => a == b,
                                (Value::Null, Value::Null) => true,
                                (Value::Undefined, Value::Undefined) => true,
                                (Value::Object(a), Value::Object(b)) => Gc::ptr_eq(*a, *b),
                                _ => false,
                            };
                            if eq {
                                return Ok(Value::Number(i as f64));
                            }
                        }
                    }
                    Ok(Value::Number(-1.0))
                }
                "includes" => {
                    let len = get_len();
                    if len == 0 {
                        return Ok(Value::Boolean(false));
                    }
                    let search = _args.first().cloned().unwrap_or(Value::Undefined);
                    let from = if let Some(f) = _args.get(1) {
                        let n = crate::core::to_number_with_env(mc, _env, f)?;
                        if n.is_nan() { 0i64 } else { n as i64 }
                    } else {
                        0
                    };
                    let start = if from < 0 {
                        (len as i64 + from).max(0) as usize
                    } else {
                        from as usize
                    };
                    for i in start..len {
                        let val = get_val(i)?;
                        // SameValueZero comparison
                        if crate::core::same_value_zero(&val, &search) {
                            return Ok(Value::Boolean(true));
                        }
                    }
                    Ok(Value::Boolean(false))
                }
                "join" => {
                    let len = get_len();
                    let sep = if let Some(s) = _args.first() {
                        if matches!(s, Value::Undefined) {
                            ",".to_string()
                        } else {
                            // ToString(separator) — call toPrimitive for objects, throw for Symbol
                            match s {
                                Value::Symbol(_) => {
                                    return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
                                }
                                Value::Object(_) => {
                                    let prim = crate::core::to_primitive(mc, s, "string", _env)?;
                                    if matches!(prim, Value::Symbol(_)) {
                                        return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
                                    }
                                    crate::core::value_to_string(&prim)
                                }
                                _ => crate::core::value_to_string(s),
                            }
                        }
                    } else {
                        ",".to_string()
                    };
                    // Use original len (no recheck_oob). Per spec: Get returns undefined
                    // for OOB reads → map to empty string.
                    let mut parts = Vec::with_capacity(len);
                    for i in 0..len {
                        let val = get_val(i)?;
                        if matches!(val, Value::Undefined) {
                            parts.push(String::new());
                        } else {
                            parts.push(crate::core::value_to_string(&val));
                        }
                    }
                    Ok(Value::String(utf8_to_utf16(&parts.join(&sep))))
                }
                "reverse" => {
                    let len = get_len();
                    let mut i = 0usize;
                    let mut j = if len > 0 { len - 1 } else { 0 };
                    while i < j {
                        let vi = ta.get(i)?;
                        let vj = ta.get(j)?;
                        ta.set(mc, i, vj)?;
                        ta.set(mc, j, vi)?;
                        i += 1;
                        j -= 1;
                    }
                    Ok(Value::Object(*obj))
                }
                "slice" => {
                    let len = get_len();
                    let start_rel = relative_index(_args.first(), 0, len)?;
                    let end_rel = relative_index(_args.get(1), len as i64, len)?;
                    let start = resolve_index(start_rel, len);
                    let end = resolve_index(end_rel, len);
                    let count = end.saturating_sub(start);
                    let result_ta = typed_array_species_create(mc, _env, obj, count)?;
                    // Recheck OOB after coercion + species create (which may resize buffer)
                    let len = recheck_oob()?;
                    if let Some(rta_cell) = slot_get_chained(&result_ta, &InternalSlot::TypedArray)
                        && let Value::TypedArray(rta) = &*rta_cell.borrow()
                    {
                        let actual_count = count.min(len.saturating_sub(start));
                        for i in 0..actual_count {
                            let v = get_val(start + i)?;
                            match v {
                                Value::Number(n) => rta.set(mc, i, n)?,
                                Value::BigInt(b) => rta.set_bigint(mc, i, bigint_to_i64_modular(&b))?,
                                _ => rta.set(mc, i, 0.0)?,
                            }
                        }
                    }
                    Ok(Value::Object(result_ta))
                }
                "copyWithin" => {
                    let len = get_len();
                    let target_rel = relative_index(_args.first(), 0, len)?;
                    let start_rel = relative_index(_args.get(1), 0, len)?;
                    let end_rel = relative_index(_args.get(2), len as i64, len)?;
                    // Resolve indices with ORIGINAL len (per spec steps 5-16)
                    let target_idx = resolve_index(target_rel, len);
                    let start = resolve_index(start_rel, len);
                    let end = resolve_index(end_rel, len);
                    let mut count = (end.saturating_sub(start)).min(len.saturating_sub(target_idx));
                    // Recheck OOB after argument coercion (spec steps 17-19)
                    let new_len = recheck_oob()?;
                    // Clamp count with new len (spec steps 20-21)
                    count = count.min(new_len.saturating_sub(target_idx));
                    count = count.min(new_len.saturating_sub(start));
                    if count > 0 {
                        // Collect values first to handle overlap
                        let mut vals = Vec::with_capacity(count);
                        for i in 0..count {
                            if is_valid_integer_index(&ta, (start + i) as f64) {
                                vals.push(ta.get(start + i)?);
                            } else {
                                vals.push(0.0);
                            }
                        }
                        for (i, v) in vals.into_iter().enumerate() {
                            if is_valid_integer_index(&ta, (target_idx + i) as f64) {
                                ta.set(mc, target_idx + i, v)?;
                            }
                        }
                    }
                    Ok(Value::Object(*obj))
                }
                "sort" => {
                    let len = get_len();
                    let comparefn = _args.first().cloned();
                    // Collect all values
                    let mut vals: Vec<f64> = Vec::with_capacity(len);
                    for i in 0..len {
                        vals.push(ta.get(i)?);
                    }

                    // Default TypedArray sort comparator per spec:
                    // NaN sorts to end; -0 sorts before +0; otherwise numeric order
                    let default_sort = |a: &f64, b: &f64| -> std::cmp::Ordering {
                        let a = *a;
                        let b = *b;
                        if a.is_nan() && b.is_nan() {
                            std::cmp::Ordering::Equal
                        } else if a.is_nan() {
                            std::cmp::Ordering::Greater // NaN goes to end
                        } else if b.is_nan() {
                            std::cmp::Ordering::Less // NaN goes to end
                        } else if a == 0.0 && b == 0.0 {
                            // Distinguish -0 and +0
                            if a.is_sign_negative() && !b.is_sign_negative() {
                                std::cmp::Ordering::Less
                            } else if !a.is_sign_negative() && b.is_sign_negative() {
                                std::cmp::Ordering::Greater
                            } else {
                                std::cmp::Ordering::Equal
                            }
                        } else {
                            a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
                        }
                    };

                    // Sort
                    if let Some(ref cmp) = comparefn {
                        if !matches!(cmp, Value::Undefined) {
                            // Use custom comparator
                            let mut err: Option<EvalError<'gc>> = None;
                            vals.sort_by(|a, b| {
                                if err.is_some() {
                                    return std::cmp::Ordering::Equal;
                                }
                                let av = if is_bigint {
                                    Value::BigInt(Box::new(num_bigint::BigInt::from(*a as i64)))
                                } else {
                                    Value::Number(*a)
                                };
                                let bv = if is_bigint {
                                    Value::BigInt(Box::new(num_bigint::BigInt::from(*b as i64)))
                                } else {
                                    Value::Number(*b)
                                };
                                match crate::js_promise::call_function_with_this(mc, cmp, None, &[av, bv], _env) {
                                    Ok(result) => {
                                        // ToNumber on comparefn result
                                        let n = match &result {
                                            Value::Number(n) => *n,
                                            Value::Undefined => f64::NAN,
                                            _ => match crate::core::to_number_with_env(mc, _env, &result) {
                                                Ok(n) => n,
                                                Err(e) => {
                                                    err = Some(e);
                                                    return std::cmp::Ordering::Equal;
                                                }
                                            },
                                        };
                                        if n.is_nan() || n == 0.0 {
                                            std::cmp::Ordering::Equal
                                        } else if n < 0.0 {
                                            std::cmp::Ordering::Less
                                        } else {
                                            std::cmp::Ordering::Greater
                                        }
                                    }
                                    Err(e) => {
                                        err = Some(e);
                                        std::cmp::Ordering::Equal
                                    }
                                }
                            });
                            if let Some(e) = err {
                                return Err(e);
                            }
                        } else {
                            // Default sort
                            vals.sort_by(default_sort);
                        }
                    } else {
                        vals.sort_by(default_sort);
                    }
                    // Write back
                    for (i, v) in vals.into_iter().enumerate() {
                        ta.set(mc, i, v)?;
                    }
                    Ok(Value::Object(*obj))
                }
                "set" => {
                    // TypedArray.prototype.set(source [, offset])
                    let offset = if let Some(off) = _args.get(1) {
                        let n = crate::core::to_number_with_env(mc, _env, off)?;
                        // ToIntegerOrInfinity: NaN → 0
                        let int_n = if n.is_nan() { 0.0 } else { n.trunc() };
                        if int_n < 0.0 || int_n == f64::INFINITY {
                            return Err(raise_range_error!("offset is out of bounds").into());
                        }
                        // Check if buffer was detached during offset coercion
                        if ta.buffer.borrow().detached {
                            return Err(raise_type_error!("Cannot perform operation on a detached ArrayBuffer").into());
                        }
                        int_n as usize
                    } else {
                        0
                    };
                    let source = _args.first().cloned().unwrap_or(Value::Undefined);
                    let len = get_len();
                    match &source {
                        Value::Object(src_obj) => {
                            if let Some(src_ta_cell) = slot_get_chained(src_obj, &InternalSlot::TypedArray)
                                && let Value::TypedArray(src_ta) = &*src_ta_cell.borrow()
                            {
                                // TypedArray source — check BigInt/non-BigInt type mixing
                                let src_is_bigint = is_bigint_typed_array(&src_ta.kind);
                                if src_is_bigint != is_bigint {
                                    return Err(raise_type_error!("Cannot mix BigInt and non-BigInt typed arrays").into());
                                }

                                // Check if source buffer is detached
                                if src_ta.buffer.borrow().detached {
                                    return Err(raise_type_error!("Cannot perform operation on a detached ArrayBuffer").into());
                                }

                                // Check if source is out-of-bounds (spec step 18)
                                let src_buf_len = src_ta.buffer.borrow().data.lock().unwrap().len();
                                let src_oob = if src_ta.length_tracking {
                                    src_ta.byte_offset > src_buf_len
                                } else {
                                    src_ta.byte_offset + src_ta.length * src_ta.element_size() > src_buf_len
                                };
                                if src_oob {
                                    return Err(raise_type_error!("Source typed array is out of bounds").into());
                                }

                                let src_len = if src_ta.length_tracking {
                                    src_buf_len.saturating_sub(src_ta.byte_offset) / src_ta.element_size()
                                } else {
                                    src_ta.length
                                };
                                if offset + src_len > len {
                                    return Err(raise_range_error!("offset is out of bounds").into());
                                }
                                // Clone source values first (handles same-buffer case)
                                let mut vals = Vec::with_capacity(src_len);
                                for i in 0..src_len {
                                    vals.push(src_ta.get(i)?);
                                }
                                for (i, v) in vals.into_iter().enumerate() {
                                    ta.set(mc, offset + i, v)?;
                                }
                            } else {
                                // Array-like source
                                let src_len_val = get_property_with_accessors(mc, _env, src_obj, "length")?;
                                let src_len = crate::core::to_number_with_env(mc, _env, &src_len_val)? as usize;
                                if offset + src_len > len {
                                    return Err(raise_range_error!("offset is out of bounds").into());
                                }
                                for i in 0..src_len {
                                    let v = get_property_with_accessors(mc, _env, src_obj, i)?;
                                    if is_bigint {
                                        let n = match &v {
                                            Value::BigInt(b) => bigint_to_i64_modular(b),
                                            _ => to_bigint_i64(mc, _env, &v)?,
                                        };
                                        ta.set_bigint(mc, offset + i, n)?;
                                    } else {
                                        let n = crate::core::to_number_with_env(mc, _env, &v)?;
                                        ta.set(mc, offset + i, n)?;
                                    };
                                }
                            }
                        }
                        _ => {
                            // ToObject(source) — throws TypeError for undefined/null
                            if matches!(source, Value::Undefined | Value::Null) {
                                return Err(raise_type_error!("Cannot convert undefined or null to object").into());
                            }
                            // Primitive source - per spec, coerce to object which has no indexed properties
                            // Numbers, booleans: the wrapper object has length 0, so nothing happens.
                            // Strings: treat as array-like with .length = string length
                            if let Value::String(s) = &source {
                                let src_len = s.len();
                                if offset + src_len > len {
                                    return Err(raise_range_error!("offset is out of bounds").into());
                                }
                                for i in 0..src_len {
                                    let ch = &s[i..i + 1];
                                    if is_bigint {
                                        let n = to_bigint_i64(mc, _env, &Value::String(ch.to_vec()))?;
                                        ta.set_bigint(mc, offset + i, n)?;
                                    } else {
                                        let ch_str = crate::unicode::utf16_to_utf8(ch);
                                        let n = ch_str.parse::<f64>().unwrap_or(f64::NAN);
                                        ta.set(mc, offset + i, n)?;
                                    };
                                }
                            }
                            // For Number, Boolean, Undefined, Null: length is 0, nothing to copy
                        }
                    }
                    Ok(Value::Undefined)
                }
                "subarray" => {
                    let len = get_len();
                    let begin_rel = relative_index(_args.first(), 0, len)?;
                    let end_arg = _args.get(1);
                    let end_is_undefined = end_arg.is_none() || matches!(end_arg, Some(Value::Undefined));
                    let end_rel = if end_is_undefined {
                        len as i64
                    } else {
                        relative_index(end_arg, len as i64, len)?
                    };
                    let begin = resolve_index(begin_rel, len);
                    let end = resolve_index(end_rel, len);
                    let new_len = end.saturating_sub(begin);

                    // SpeciesConstructor lookup
                    let species = get_species_constructor(mc, _env, obj)?;

                    if let Some(ctor) = species {
                        let new_byte_offset = ta.byte_offset + begin * ta.element_size();
                        let buf_val = if let Some(buf_obj_val) = slot_get_chained(obj, &InternalSlot::BufferObject) {
                            buf_obj_val.borrow().clone()
                        } else {
                            Value::ArrayBuffer(ta.buffer)
                        };
                        // Per spec step 15-16: If O.[[ArrayLength]] is auto and end is undefined,
                        // argumentsList is « buffer, beginByteOffset » (2 args);
                        // otherwise « buffer, beginByteOffset, newLength » (3 args).
                        let args: Vec<Value<'gc>> = if ta.length_tracking && end_is_undefined {
                            vec![buf_val, Value::Number(new_byte_offset as f64)]
                        } else {
                            vec![buf_val, Value::Number(new_byte_offset as f64), Value::Number(new_len as f64)]
                        };
                        let new_val = crate::js_class::evaluate_new(mc, _env, &ctor, &args, None)?;
                        match new_val {
                            Value::Object(o) => {
                                // ValidateTypedArray: result must have [[TypedArrayName]]
                                if slot_get_chained(&o, &InternalSlot::TypedArray).is_none() {
                                    return Err(raise_type_error!("TypedArray species constructor did not return a TypedArray").into());
                                }
                                Ok(Value::Object(o))
                            }
                            _ => Err(raise_type_error!("Species constructor did not return an object").into()),
                        }
                    } else {
                        // Default: create a new TypedArray backed by the same buffer
                        // Per spec: the default TypedArray constructor checks IsDetachedBuffer
                        // and throws TypeError (step 11).
                        if ta.buffer.borrow().detached {
                            return Err(raise_type_error!("Cannot create subarray on a detached ArrayBuffer").into());
                        }
                        let new_byte_offset = ta.byte_offset + begin * ta.element_size();
                        let sa_obj = new_js_object_data(mc);

                        // Set prototype from the original object's constructor
                        if let Some(proto_val) = {
                            let borrowed = obj.borrow();
                            borrowed.prototype
                        } {
                            sa_obj.borrow_mut(mc).prototype = Some(proto_val);
                        }

                        let new_ta = JSTypedArray {
                            buffer: ta.buffer,
                            kind: ta.kind.clone(),
                            byte_offset: new_byte_offset,
                            length: new_len,
                            length_tracking: false,
                        };
                        slot_set(mc, &sa_obj, InternalSlot::TypedArray, &Value::TypedArray(Gc::new(mc, new_ta)));
                        // Copy the BufferObject slot
                        if let Some(buf_obj_val) = slot_get_chained(obj, &InternalSlot::BufferObject) {
                            slot_set(mc, &sa_obj, InternalSlot::BufferObject, &buf_obj_val.borrow());
                        }
                        Ok(Value::Object(sa_obj))
                    }
                }
                "toLocaleString" => {
                    let len = get_len();
                    let mut parts = Vec::with_capacity(len);
                    for i in 0..len {
                        let val = get_val(i)?;
                        // Per spec: Invoke(element, "toLocaleString")
                        if matches!(val, Value::Undefined | Value::Null) {
                            parts.push(String::new());
                        } else {
                            // Look up toLocaleString on Number.prototype or BigInt.prototype
                            let proto_name = if is_bigint { "BigInt" } else { "Number" };
                            let tls_method = if let Some(ctor_val) = object_get_key_value(_env, proto_name)
                                && let Value::Object(ctor_obj) = &*ctor_val.borrow()
                            {
                                if let Some(proto_val) = object_get_key_value(ctor_obj, "prototype")
                                    && let Value::Object(proto_obj) = &*proto_val.borrow()
                                {
                                    get_property_with_accessors(mc, _env, proto_obj, "toLocaleString")?
                                } else {
                                    Value::Undefined
                                }
                            } else {
                                Value::Undefined
                            };
                            let result = crate::js_promise::call_function_with_this(mc, &tls_method, Some(&val), &[], _env)?;
                            // ToString(result) — must call toPrimitive for objects
                            let s = match &result {
                                Value::Object(_) => {
                                    let prim = crate::core::to_primitive(mc, &result, "string", _env)?;
                                    if matches!(prim, Value::Symbol(_)) {
                                        return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
                                    }
                                    crate::core::value_to_string(&prim)
                                }
                                Value::Symbol(_) => {
                                    return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
                                }
                                _ => crate::core::value_to_string(&result),
                            };
                            parts.push(s);
                        }
                    }
                    Ok(Value::String(utf8_to_utf16(&parts.join(","))))
                }
                "toString" => {
                    let len = get_len();
                    let mut parts = Vec::with_capacity(len);
                    for i in 0..len {
                        let val = get_val(i)?;
                        parts.push(crate::core::value_to_string(&val));
                    }
                    Ok(Value::String(utf8_to_utf16(&parts.join(","))))
                }
                "toReversed" => {
                    let len = get_len();
                    let result_ta = create_same_type_typedarray(mc, _env, obj, len)?;
                    if let Some(rta_cell) = slot_get_chained(&result_ta, &InternalSlot::TypedArray)
                        && let Value::TypedArray(rta) = &*rta_cell.borrow()
                    {
                        for i in 0..len {
                            let v = ta.get(i)?;
                            rta.set(mc, len - 1 - i, v)?;
                        }
                    }
                    Ok(Value::Object(result_ta))
                }
                "toSorted" => {
                    let len = get_len();
                    let comparefn = _args.first().cloned();

                    // Step 1: If comparefn is not undefined and IsCallable(comparefn) is false, throw TypeError
                    if let Some(ref cmp) = comparefn
                        && !matches!(cmp, Value::Undefined)
                    {
                        let callable = match cmp {
                            Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) => true,
                            Value::Object(o) => o.borrow().get_closure().is_some() || slot_get(o, &InternalSlot::BoundTarget).is_some(),
                            _ => false,
                        };
                        if !callable {
                            return Err(raise_type_error!("comparefn is not a function").into());
                        }
                    }

                    let mut vals: Vec<f64> = Vec::with_capacity(len);
                    for i in 0..len {
                        vals.push(ta.get(i)?);
                    }

                    // Default TypedArray sort comparator per spec
                    let default_sort = |a: &f64, b: &f64| -> std::cmp::Ordering {
                        let a = *a;
                        let b = *b;
                        if a.is_nan() && b.is_nan() {
                            std::cmp::Ordering::Equal
                        } else if a.is_nan() {
                            std::cmp::Ordering::Greater
                        } else if b.is_nan() {
                            std::cmp::Ordering::Less
                        } else if a == 0.0 && b == 0.0 {
                            if a.is_sign_negative() && !b.is_sign_negative() {
                                std::cmp::Ordering::Less
                            } else if !a.is_sign_negative() && b.is_sign_negative() {
                                std::cmp::Ordering::Greater
                            } else {
                                std::cmp::Ordering::Equal
                            }
                        } else {
                            a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
                        }
                    };

                    if let Some(ref cmp) = comparefn {
                        if !matches!(cmp, Value::Undefined) {
                            let mut err: Option<EvalError<'gc>> = None;
                            vals.sort_by(|a, b| {
                                if err.is_some() {
                                    return std::cmp::Ordering::Equal;
                                }
                                let av = if is_bigint {
                                    Value::BigInt(Box::new(num_bigint::BigInt::from(*a as i64)))
                                } else {
                                    Value::Number(*a)
                                };
                                let bv = if is_bigint {
                                    Value::BigInt(Box::new(num_bigint::BigInt::from(*b as i64)))
                                } else {
                                    Value::Number(*b)
                                };
                                match crate::js_promise::call_function_with_this(mc, cmp, None, &[av, bv], _env) {
                                    Ok(result) => {
                                        let n = match &result {
                                            Value::Number(n) => *n,
                                            Value::Undefined => f64::NAN,
                                            _ => match crate::core::to_number_with_env(mc, _env, &result) {
                                                Ok(n) => n,
                                                Err(e) => {
                                                    err = Some(e);
                                                    return std::cmp::Ordering::Equal;
                                                }
                                            },
                                        };
                                        if n.is_nan() || n == 0.0 {
                                            std::cmp::Ordering::Equal
                                        } else if n < 0.0 {
                                            std::cmp::Ordering::Less
                                        } else {
                                            std::cmp::Ordering::Greater
                                        }
                                    }
                                    Err(e) => {
                                        err = Some(e);
                                        std::cmp::Ordering::Equal
                                    }
                                }
                            });
                            if let Some(e) = err {
                                return Err(e);
                            }
                        } else {
                            vals.sort_by(default_sort);
                        }
                    } else {
                        vals.sort_by(default_sort);
                    }
                    let result_ta = create_same_type_typedarray(mc, _env, obj, len)?;
                    if let Some(rta_cell) = slot_get_chained(&result_ta, &InternalSlot::TypedArray)
                        && let Value::TypedArray(rta) = &*rta_cell.borrow()
                    {
                        for (i, v) in vals.into_iter().enumerate() {
                            rta.set(mc, i, v)?;
                        }
                    }
                    Ok(Value::Object(result_ta))
                }
                "with" => {
                    // Step 3: capture len BEFORE value conversion
                    let orig_len = get_len();

                    // Step 4: Let relativeIndex be ? ToIntegerOrInfinity(index).
                    let idx_arg = _args.first().cloned().unwrap_or(Value::Undefined);
                    let idx_num = crate::core::to_number_with_env(mc, _env, &idx_arg)?;
                    let rel = if idx_num.is_nan() || idx_num == 0.0 {
                        0i64
                    } else if !idx_num.is_finite() {
                        if idx_num.is_sign_negative() { i64::MIN } else { i64::MAX }
                    } else {
                        idx_num.trunc() as i64
                    };

                    // Steps 5-6: compute actualIndex
                    let actual = if rel < 0 { orig_len as i64 + rel } else { rel };

                    // Steps 7-8: convert value BEFORE checking index bounds
                    // (valueOf may resize the buffer)
                    let value = _args.get(1).cloned().unwrap_or(Value::Undefined);
                    let numeric_value = if is_bigint {
                        match &value {
                            Value::BigInt(b) => Value::BigInt(b.clone()),
                            _ => Value::BigInt(Box::new(num_bigint::BigInt::from(to_bigint_i64(mc, _env, &value)?))),
                        }
                    } else {
                        Value::Number(crate::core::to_number_with_env(mc, _env, &value)?)
                    };

                    // Step 9: Use IsValidIntegerIndex (checks current buffer state)
                    if actual < 0 || !ta.is_valid_integer_index(actual as usize) {
                        return Err(raise_range_error!("TypedArray.prototype.with: index out of range").into());
                    }

                    // Step 10: create result with ORIGINAL length
                    let result_ta_obj = create_same_type_typedarray(mc, _env, obj, orig_len)?;
                    if let Some(rta_cell) = slot_get_chained(&result_ta_obj, &InternalSlot::TypedArray)
                        && let Value::TypedArray(rta) = &*rta_cell.borrow()
                    {
                        for i in 0..orig_len {
                            if i == actual as usize {
                                if is_bigint {
                                    if let Value::BigInt(b) = &numeric_value {
                                        rta.set_bigint(mc, i, bigint_to_i64_modular(b))?;
                                    }
                                } else if let Value::Number(n) = numeric_value {
                                    rta.set(mc, i, n)?;
                                }
                            } else {
                                let v = get_val(i)?;
                                match v {
                                    Value::Number(n) => rta.set(mc, i, n)?,
                                    Value::BigInt(b) => rta.set_bigint(mc, i, bigint_to_i64_modular(&b))?,
                                    _ => rta.set(mc, i, 0.0)?,
                                }
                            }
                        }
                    }
                    Ok(Value::Object(result_ta_obj))
                }
                _ => Err(raise_eval_error!(format!("TypedArray.prototype.{} not implemented", method)).into()),
            }
        } else {
            Err(raise_type_error!(format!("Method TypedArray.prototype.{} called on incompatible receiver", method)).into())
        }
    } else {
        Err(raise_type_error!(format!("Method TypedArray.prototype.{} called on incompatible receiver", method)).into())
    }
}

pub fn initialize_typedarray<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let arraybuffer = make_arraybuffer_constructor(mc, env)?;
    crate::core::env_set(mc, env, "ArrayBuffer", &Value::Object(arraybuffer))?;

    let dataview = make_dataview_constructor(mc, env)?;
    crate::core::env_set(mc, env, "DataView", &Value::Object(dataview))?;

    // Create the abstract %TypedArray% intrinsic first
    let (ta_intrinsic, ta_proto_intrinsic) = make_typedarray_intrinsic(mc, env)?;

    let typed_arrays = make_typedarray_constructors(mc, env, &ta_intrinsic, &ta_proto_intrinsic)?;
    for (name, ctor) in &typed_arrays {
        crate::core::env_set(mc, env, name, &Value::Object(*ctor))?;
    }

    // Now fix up constructor properties on per-type prototypes to point to actual constructors
    for (name, ctor) in &typed_arrays {
        if let Some(proto_val) = object_get_key_value(ctor, "prototype")
            && let Value::Object(proto) = &*proto_val.borrow()
        {
            object_set_key_value(mc, proto, "constructor", &Value::Object(*ctor))?;
            proto.borrow_mut(mc).set_non_enumerable("constructor");
        }
        // Also make name and length on prototype available (not needed per spec, but name helps for .name tests)
        let _ = name; // just to avoid unused warning
    }

    let atomics = make_atomics_object(mc, env)?;
    crate::core::env_set(mc, env, "Atomics", &Value::Object(atomics))?;

    let shared_ab = make_sharedarraybuffer_constructor(mc, env)?;
    crate::core::env_set(mc, env, "SharedArrayBuffer", &Value::Object(shared_ab))?;

    Ok(())
}
