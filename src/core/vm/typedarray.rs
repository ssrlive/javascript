use super::*;

/// Returns true if the given TypedArray name is a BigInt variant.
pub(crate) fn is_bigint_typed_array(name: &str) -> bool {
    matches!(name, "BigInt64Array" | "BigUint64Array")
}

/// Truncate a BigInt to a signed 64-bit value (two's complement).
fn bigint_to_i64(bi: &num_bigint::BigInt) -> i64 {
    let bytes = bi.to_signed_bytes_le();
    let mut arr = [0u8; 8];
    let copy_len = bytes.len().min(8);
    arr[..copy_len].copy_from_slice(&bytes[..copy_len]);
    // Sign-extend if the representation is shorter than 8 bytes and negative
    if bytes.len() < 8 && !bytes.is_empty() && (bytes[bytes.len() - 1] & 0x80) != 0 {
        for b in arr[copy_len..].iter_mut() {
            *b = 0xFF;
        }
    }
    i64::from_le_bytes(arr)
}

/// Truncate a BigInt to an unsigned 64-bit value.
fn bigint_to_u64(bi: &num_bigint::BigInt) -> u64 {
    bigint_to_i64(bi) as u64
}

/// Coerce a BigInt for storage in a BigInt TypedArray (truncate to 64 bits).
pub(crate) fn coerce_bigint_for_ta(bi: &num_bigint::BigInt, ta_name: &str) -> num_bigint::BigInt {
    match ta_name {
        "BigInt64Array" => num_bigint::BigInt::from(bigint_to_i64(bi)),
        "BigUint64Array" => num_bigint::BigInt::from(bigint_to_u64(bi)),
        _ => bi.clone(),
    }
}

// ── IEEE 754 half-precision (float16) conversion ────────────────────
/// Convert an f64 to IEEE 754 binary16 (half-precision) bits.
/// Rounds to nearest, ties to even.
pub(crate) fn f64_to_f16_bits(val: f64) -> u16 {
    let bits = val.to_bits();
    let sign = ((bits >> 63) & 1) as u16;
    let exp = ((bits >> 52) & 0x7FF) as i32;
    let frac = bits & 0x000F_FFFF_FFFF_FFFF;

    // Infinity or NaN
    if exp == 0x7FF {
        if frac == 0 {
            return (sign << 15) | 0x7C00; // ±Infinity
        }
        // NaN — quiet NaN with at least one mantissa bit set
        let f16_frac = (frac >> 42) as u16 & 0x03FF;
        return (sign << 15) | 0x7C00 | f16_frac.max(1);
    }

    // Zero
    if exp == 0 && frac == 0 {
        return sign << 15; // ±0
    }

    let unbiased = exp - 1023; // f64 bias=1023

    // Overflow → ±Infinity
    if unbiased > 15 {
        return (sign << 15) | 0x7C00;
    }

    // Subnormal f16 range
    if unbiased < -14 {
        if unbiased < -24 {
            // Too small — round to ±0
            // But check the round bit for ties (halfway cases)
            if unbiased == -25 {
                let implicit = frac | 0x0010_0000_0000_0000;
                // The entire mantissa is a "round" bit at this shift
                // Round up only if sticky bits exist or mantissa LSB=1
                if implicit > 0x0010_0000_0000_0000 {
                    return (sign << 15) | 1; // smallest subnormal
                }
            }
            return sign << 15;
        }
        // Subnormal: shift = how many positions to shift mantissa right
        let shift = (-14 - unbiased) as u32; // 1..10
        let implicit = frac | 0x0010_0000_0000_0000; // add implicit 1 bit
        let total_shift = 42 + shift; // shift from f64 mantissa to f16 mantissa
        let mantissa = (implicit >> total_shift) as u16;
        let round_bit = (implicit >> (total_shift - 1)) & 1;
        let sticky_mask = if total_shift > 1 { (1u64 << (total_shift - 1)) - 1 } else { 0 };
        let sticky = implicit & sticky_mask;
        let mut result = (sign << 15) | mantissa;
        // Round to nearest even
        if round_bit != 0 && (sticky != 0 || (mantissa & 1) != 0) {
            result += 1;
        }
        return result;
    }

    // Normal f16
    let f16_exp = (unbiased + 15) as u16; // re-bias for f16 (bias=15)
    let mantissa = (frac >> 42) as u16; // top 10 bits of f64 mantissa
    let round_bit = (frac >> 41) & 1;
    let sticky = frac & ((1u64 << 41) - 1);
    let mut result = (sign << 15) | (f16_exp << 10) | mantissa;
    // Round to nearest even
    if round_bit != 0 && (sticky != 0 || (mantissa & 1) != 0) {
        result += 1;
    }
    result
}

/// Convert IEEE 754 binary16 (half-precision) bits to f64.
pub(crate) fn f16_bits_to_f64(bits: u16) -> f64 {
    let sign = ((bits >> 15) & 1) as u64;
    let exp = (bits >> 10) & 0x1F;
    let frac = (bits & 0x03FF) as u64;

    if exp == 0x1F {
        if frac == 0 {
            return f64::from_bits(sign << 63 | 0x7FF0_0000_0000_0000); // ±Infinity
        }
        // NaN — propagate sign and mantissa bits
        return f64::from_bits(sign << 63 | 0x7FF8_0000_0000_0000 | (frac << 42));
    }

    if exp == 0 {
        if frac == 0 {
            return f64::from_bits(sign << 63); // ±0
        }
        // Subnormal f16: value = (-1)^sign × 2^(-14) × (frac/1024)
        let val = frac as f64 * 2.0f64.powi(-24);
        return if sign == 1 { -val } else { val };
    }

    // Normal: value = (-1)^sign × 2^(exp-15) × (1 + frac/1024)
    let f64_exp = ((exp as u64) - 15 + 1023) & 0x7FF; // re-bias for f64
    f64::from_bits(sign << 63 | f64_exp << 52 | frac << 42)
}

/// Round a f64 to the nearest IEEE 754 binary16 value, returned as f64.
/// This is the semantics of Math.f16round().
pub(crate) fn f16round(val: f64) -> f64 {
    if val.is_nan() {
        return f64::NAN;
    }
    f16_bits_to_f64(f64_to_f16_bits(val))
}

/// Coerce a numeric value according to the TypedArray element type.
pub(crate) fn coerce_typed_array_value(n: f64, ta_name: &str) -> f64 {
    if n.is_nan() || !n.is_finite() || n == 0.0 {
        match ta_name {
            "Float16Array" | "Float32Array" | "Float64Array" => {
                return match ta_name {
                    "Float16Array" => f16round(n),
                    "Float32Array" => (n as f32) as f64,
                    _ => n,
                };
            }
            "Uint8ClampedArray" => {
                if n.is_nan() || n <= 0.0 {
                    return 0.0;
                }
                if n >= 255.0 {
                    return 255.0;
                }
                return 0.0;
            }
            _ => return 0.0,
        }
    }
    match ta_name {
        "Int8Array" => {
            let iv = n.trunc() as i64;
            let m = iv.rem_euclid(256);
            if m >= 128 { (m - 256) as f64 } else { m as f64 }
        }
        "Uint8Array" => {
            let iv = n.trunc() as i64;
            (iv.rem_euclid(256)) as f64
        }
        "Uint8ClampedArray" => {
            // Round-half-to-even
            if n <= 0.0 {
                return 0.0;
            }
            if n >= 255.0 {
                return 255.0;
            }
            let f = n.floor();
            if f + 0.5 < n {
                return f + 1.0;
            }
            if n < f + 0.5 {
                return f;
            }
            let fi = f as u8;
            if fi.is_multiple_of(2) { fi as f64 } else { (fi + 1) as f64 }
        }
        "Int16Array" => {
            let iv = n.trunc() as i64;
            let m = iv.rem_euclid(65536);
            if m >= 32768 { (m - 65536) as f64 } else { m as f64 }
        }
        "Uint16Array" => {
            let iv = n.trunc() as i64;
            (iv.rem_euclid(65536)) as f64
        }
        "Int32Array" => {
            let iv = n.trunc() as i128;
            let m = iv.rem_euclid(4294967296);
            if m >= 2147483648 { (m - 4294967296) as f64 } else { m as f64 }
        }
        "Uint32Array" => {
            let iv = n.trunc() as i128;
            (iv.rem_euclid(4294967296)) as f64
        }
        "Float16Array" => f16round(n),
        "Float32Array" => (n as f32) as f64,
        _ => n,
    }
}

impl<'gc> VM<'gc> {
    /// Check whether a TypedArray's underlying ArrayBuffer is detached.
    /// `arr` must be a ArrayHandle that has `__typedarray_name__` in props.
    pub(crate) fn is_typed_array_buffer_detached(arr: &ArrayHandle<'gc>) -> bool {
        let b = arr.borrow();
        if let Some(Value::Object(buf)) = b.props.get("__typedarray_buffer__") {
            matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
        } else if let Some(Value::Object(buf)) = b.props.get("buffer") {
            matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
        } else {
            false
        }
    }

    /// Spec `IsValidIntegerIndex(O, index)`.
    /// Returns true if `numeric_index` is a valid integer index for the typed array.
    pub(crate) fn is_valid_integer_index(arr: &ArrayHandle<'gc>, numeric_index: f64) -> bool {
        if Self::is_typed_array_buffer_detached(arr) {
            return false;
        }
        if numeric_index.is_nan() || numeric_index.is_infinite() {
            return false;
        }
        if numeric_index.fract() != 0.0 {
            return false;
        }
        if numeric_index == 0.0 && numeric_index.is_sign_negative() {
            return false;
        }
        if numeric_index < 0.0 {
            return false;
        }
        let idx = numeric_index as usize;
        idx < arr.borrow().elements.len()
    }

    pub(super) fn typedarray_handle_host_fn(
        &mut self,
        ctx: &GcContext<'gc>,
        name: &str,
        receiver: Option<&Value<'gc>>,
        args: &[Value<'gc>],
    ) -> Value<'gc> {
        match name {
            "typedarray.get_buffer" => {
                let this_val = receiver.unwrap_or(&Value::Undefined);
                match this_val {
                    Value::Array(arr) => {
                        let a = arr.borrow();
                        if a.props.get("__typedarray_name__").is_none() {
                            self.throw_type_error(ctx, "get TypedArray.prototype.buffer called on incompatible receiver");
                            return Value::Undefined;
                        }
                        a.props
                            .get("buffer")
                            .or_else(|| a.props.get("__typedarray_buffer__"))
                            .cloned()
                            .unwrap_or(Value::Undefined)
                    }
                    _ => {
                        self.throw_type_error(ctx, "get TypedArray.prototype.buffer called on incompatible receiver");
                        Value::Undefined
                    }
                }
            }
            "typedarray.get_byteLength" => {
                let this_val = receiver.unwrap_or(&Value::Undefined);
                match this_val {
                    Value::Array(arr) => {
                        let a = arr.borrow();
                        if a.props.get("__typedarray_name__").is_none() {
                            self.throw_type_error(ctx, "get TypedArray.prototype.byteLength called on incompatible receiver");
                            return Value::Undefined;
                        }
                        let bpe = match a.props.get("__bytes_per_element__") {
                            Some(Value::Number(n)) => *n as usize,
                            _ => 1,
                        };
                        // Check for detached buffer
                        if let Some(Value::Object(buf)) = a.props.get("__typedarray_buffer__")
                            && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
                        {
                            return Value::Number(0.0);
                        }
                        // Resizable buffer: compute dynamic length
                        if let Some(Value::Object(buf)) = a.props.get("__typedarray_buffer__")
                            && matches!(buf.borrow().get("__resizable__"), Some(Value::Boolean(true)))
                        {
                            let byte_offset = match a.props.get("__byte_offset__") {
                                Some(Value::Number(n)) => *n as usize,
                                _ => 0,
                            };
                            let buf_byte_len = match buf.borrow().get("byteLength") {
                                Some(Value::Number(n)) => *n as usize,
                                _ => 0,
                            };
                            let is_auto = matches!(a.props.get("__length_tracking__"), Some(Value::Boolean(true)));
                            if is_auto {
                                if byte_offset > buf_byte_len {
                                    return Value::Number(0.0);
                                }
                                let remainder = buf_byte_len - byte_offset;
                                return Value::Number((remainder - remainder % bpe) as f64);
                            } else {
                                let fixed_len = match a.props.get("__fixed_length__") {
                                    Some(Value::Number(n)) => *n as usize,
                                    _ => a.elements.len(),
                                };
                                if byte_offset + fixed_len * bpe > buf_byte_len {
                                    return Value::Number(0.0);
                                }
                                return Value::Number((fixed_len * bpe) as f64);
                            }
                        }
                        Value::Number((a.elements.len() * bpe) as f64)
                    }
                    _ => {
                        self.throw_type_error(ctx, "get TypedArray.prototype.byteLength called on incompatible receiver");
                        Value::Undefined
                    }
                }
            }
            "typedarray.get_byteOffset" => {
                let this_val = receiver.unwrap_or(&Value::Undefined);
                match this_val {
                    Value::Array(arr) => {
                        let a = arr.borrow();
                        if a.props.get("__typedarray_name__").is_none() {
                            self.throw_type_error(ctx, "get TypedArray.prototype.byteOffset called on incompatible receiver");
                            return Value::Undefined;
                        }
                        // Check for detached buffer
                        if let Some(Value::Object(buf)) = a.props.get("__typedarray_buffer__")
                            && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
                        {
                            return Value::Number(0.0);
                        }
                        let byte_offset = match a.props.get("__byte_offset__") {
                            Some(Value::Number(n)) => *n as usize,
                            _ => 0,
                        };
                        // Resizable buffer: out-of-bounds check
                        if let Some(Value::Object(buf)) = a.props.get("__typedarray_buffer__")
                            && matches!(buf.borrow().get("__resizable__"), Some(Value::Boolean(true)))
                        {
                            let bpe = match a.props.get("__bytes_per_element__") {
                                Some(Value::Number(n)) => *n as usize,
                                _ => 1,
                            };
                            let buf_byte_len = match buf.borrow().get("byteLength") {
                                Some(Value::Number(n)) => *n as usize,
                                _ => 0,
                            };
                            let is_auto = matches!(a.props.get("__length_tracking__"), Some(Value::Boolean(true)));
                            if is_auto {
                                if byte_offset > buf_byte_len {
                                    return Value::Number(0.0);
                                }
                            } else {
                                let fixed_len = match a.props.get("__fixed_length__") {
                                    Some(Value::Number(n)) => *n as usize,
                                    _ => 0,
                                };
                                if byte_offset + fixed_len * bpe > buf_byte_len {
                                    return Value::Number(0.0);
                                }
                            }
                        }
                        Value::Number(byte_offset as f64)
                    }
                    _ => {
                        self.throw_type_error(ctx, "get TypedArray.prototype.byteOffset called on incompatible receiver");
                        Value::Undefined
                    }
                }
            }
            "typedarray.get_length" => {
                let this_val = receiver.unwrap_or(&Value::Undefined);
                match this_val {
                    Value::Array(arr) => {
                        let a = arr.borrow();
                        if a.props.get("__typedarray_name__").is_none() {
                            self.throw_type_error(ctx, "get TypedArray.prototype.length called on incompatible receiver");
                            return Value::Undefined;
                        }
                        // Check for detached buffer
                        if let Some(Value::Object(buf)) = a.props.get("__typedarray_buffer__")
                            && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
                        {
                            return Value::Number(0.0);
                        }
                        // Resizable buffer: compute dynamic length
                        if let Some(Value::Object(buf)) = a.props.get("__typedarray_buffer__")
                            && matches!(buf.borrow().get("__resizable__"), Some(Value::Boolean(true)))
                        {
                            let bpe = match a.props.get("__bytes_per_element__") {
                                Some(Value::Number(n)) => *n as usize,
                                _ => 1,
                            };
                            let byte_offset = match a.props.get("__byte_offset__") {
                                Some(Value::Number(n)) => *n as usize,
                                _ => 0,
                            };
                            let buf_byte_len = match buf.borrow().get("byteLength") {
                                Some(Value::Number(n)) => *n as usize,
                                _ => 0,
                            };
                            let is_auto = matches!(a.props.get("__length_tracking__"), Some(Value::Boolean(true)));
                            if is_auto {
                                if byte_offset > buf_byte_len {
                                    return Value::Number(0.0);
                                }
                                return Value::Number(((buf_byte_len - byte_offset) / bpe) as f64);
                            } else {
                                let fixed_len = match a.props.get("__fixed_length__") {
                                    Some(Value::Number(n)) => *n as usize,
                                    _ => a.elements.len(),
                                };
                                if byte_offset + fixed_len * bpe > buf_byte_len {
                                    return Value::Number(0.0);
                                }
                                return Value::Number(fixed_len as f64);
                            }
                        }
                        Value::Number(a.elements.len() as f64)
                    }
                    _ => {
                        self.throw_type_error(ctx, "get TypedArray.prototype.length called on incompatible receiver");
                        Value::Undefined
                    }
                }
            }
            "typedarray.get_toStringTag" => {
                let this_val = receiver.unwrap_or(&Value::Undefined);
                match this_val {
                    Value::Array(arr) => {
                        let a = arr.borrow();
                        match a.props.get("__typedarray_name__") {
                            Some(v) => v.clone(),
                            None => Value::Undefined,
                        }
                    }
                    _ => Value::Undefined,
                }
            }
            "typedarray.set" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "set") {
                    return Value::Undefined;
                }
                if let Value::Array(arr) = &this_val {
                    self.sync_resizable_ta_elements(ctx, arr);
                }
                let source = args.first().cloned().unwrap_or(Value::Undefined);

                // ToInteger for offset
                let offset_val = args.get(1).cloned().unwrap_or(Value::Undefined);
                let offset: usize = if matches!(offset_val, Value::Undefined) {
                    0
                } else {
                    let n = match self.extract_number_with_coercion(ctx, &offset_val) {
                        Some(v) => v,
                        None => return Value::Undefined, // symbol → TypeError already set
                    };
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    // ToInteger: truncate towards zero
                    let int_n = if n.is_nan() { 0.0 } else { n.trunc() };
                    if !int_n.is_finite() || int_n < 0.0 {
                        self.throw_range_error_object(ctx, "offset is out of bounds");
                        return Value::Undefined;
                    }
                    int_n as usize
                };

                // Re-validate after coercion (buffer may have been detached)
                if !self.validate_typed_array(ctx, &this_val, "set") {
                    return Value::Undefined;
                }

                let Value::Array(target_arr) = &this_val else {
                    self.throw_type_error(ctx, "TypedArray.prototype.set called on incompatible receiver");
                    return Value::Undefined;
                };

                let (ta_name, _bpe) = {
                    let a = target_arr.borrow();
                    let name = match a.props.get("__typedarray_name__") {
                        Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                        _ => "Uint8Array".to_string(),
                    };
                    let bpe = match a.props.get("__bytes_per_element__") {
                        Some(Value::Number(n)) => *n as usize,
                        _ => 1,
                    };
                    (name, bpe)
                };

                // Check if source is a TypedArray
                let source_is_ta = match &source {
                    Value::Array(src) => src.borrow().props.contains_key("__typedarray_name__"),
                    _ => false,
                };

                if source_is_ta {
                    // TypedArray source path
                    let Value::Array(src_arr) = &source else { unreachable!() };

                    // Sync source if backed by resizable buffer
                    self.sync_resizable_ta_elements(ctx, src_arr);

                    // Check if source buffer is detached (step 12)
                    {
                        let s = src_arr.borrow();
                        if let Some(Value::Object(buf)) = s.props.get("__typedarray_buffer__")
                            && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
                        {
                            drop(s);
                            self.throw_type_error(ctx, "Cannot perform %TypedArray%.prototype.set - source buffer is detached");
                            return Value::Undefined;
                        }
                    }

                    // Check if source is out of bounds (resizable buffer)
                    if self.is_typed_array_oob(src_arr) {
                        self.throw_type_error(
                            ctx,
                            "Cannot perform %TypedArray%.prototype.set - source TypedArray is out of bounds",
                        );
                        return Value::Undefined;
                    }

                    // Re-sync target too (source and target may share the same buffer)
                    self.sync_resizable_ta_elements(ctx, target_arr);

                    let (src_name, src_bpe, src_len, src_byte_offset) = {
                        let s = src_arr.borrow();
                        let name = match s.props.get("__typedarray_name__") {
                            Some(Value::String(ss)) => crate::unicode::utf16_to_utf8(ss),
                            _ => "Uint8Array".to_string(),
                        };
                        let bpe = match s.props.get("__bytes_per_element__") {
                            Some(Value::Number(n)) => *n as usize,
                            _ => 1,
                        };
                        let bo = match s.props.get("__byte_offset__") {
                            Some(Value::Number(n)) => *n as usize,
                            _ => 0,
                        };
                        (name, bpe, s.elements.len(), bo)
                    };

                    let target_len = target_arr.borrow().elements.len();
                    if offset + src_len > target_len {
                        self.throw_range_error_object(ctx, "offset is out of bounds");
                        return Value::Undefined;
                    }

                    // Get source buffer bytes
                    let src_bytes: Vec<Value<'gc>> = {
                        let s = src_arr.borrow();
                        if let Some(Value::Object(buf)) = s.props.get("__typedarray_buffer__") {
                            if let Some(Value::Array(bb)) = buf.borrow().get("__buffer_bytes__").cloned() {
                                bb.borrow().elements.clone()
                            } else {
                                Vec::new()
                            }
                        } else {
                            Vec::new()
                        }
                    };

                    let _target_byte_offset = {
                        let a = target_arr.borrow();
                        match a.props.get("__byte_offset__") {
                            Some(Value::Number(n)) => *n as usize,
                            _ => 0,
                        }
                    };

                    // Copy element by element: decode from source, encode to target
                    let target_is_bigint = is_bigint_typed_array(&ta_name);
                    let src_is_bigint = is_bigint_typed_array(&src_name);
                    if target_is_bigint != src_is_bigint {
                        self.throw_type_error(ctx, "Cannot mix BigInt and other types, use explicit conversions");
                        return Value::Undefined;
                    }
                    for i in 0..src_len {
                        let src_base = src_byte_offset + i * src_bpe;
                        let val = Self::decode_typed_element(&src_bytes, src_base, src_bpe, &src_name);
                        let target_idx = offset + i;
                        // Bounds check after potential sync
                        if target_idx >= target_arr.borrow().elements.len() {
                            break;
                        }
                        if target_is_bigint {
                            let bi = match &val {
                                Value::BigInt(b) => (**b).clone(),
                                _ => num_bigint::BigInt::from(0),
                            };
                            let coerced = coerce_bigint_for_ta(&bi, &ta_name);
                            {
                                let mut t = target_arr.borrow_mut(ctx);
                                t.elements[target_idx] = Value::BigInt(Box::new(coerced));
                            }
                            self.sync_ta_element_to_buffer(ctx, target_arr, target_idx, 0.0, &ta_name);
                        } else {
                            let num = to_number(&val);
                            let converted = Self::typed_array_coerce_value(num, &ta_name);
                            {
                                let mut t = target_arr.borrow_mut(ctx);
                                t.elements[target_idx] = Value::Number(converted);
                            }
                            self.sync_ta_element_to_buffer(ctx, target_arr, target_idx, num, &ta_name);
                        }
                    }
                } else {
                    // Array / array-like / primitive source path
                    // ToObject for primitives
                    let src_obj = match &source {
                        Value::Number(_) | Value::Boolean(_) | Value::String(_) => {
                            // Primitives have length 0 when ToObject'd for set purposes
                            // (Number/Boolean wrappers have no indexed properties)
                            let len_val = self.read_named_property(ctx, &source, "length");
                            if self.pending_throw.is_some() {
                                return Value::Undefined;
                            }
                            let len = match len_val {
                                Value::Number(n) if n.is_finite() && n >= 0.0 => n as usize,
                                _ => 0,
                            };
                            let target_len = target_arr.borrow().elements.len();
                            if offset + len > target_len {
                                self.throw_range_error_object(ctx, "offset is out of bounds");
                                return Value::Undefined;
                            }
                            // For string source, copy string indices
                            let target_is_bigint = is_bigint_typed_array(&ta_name);
                            for i in 0..len {
                                let v = self.read_named_property(ctx, &source, &i.to_string());
                                if self.pending_throw.is_some() {
                                    return Value::Undefined;
                                }
                                if target_is_bigint {
                                    let bi = match self.value_to_bigint(ctx, &v) {
                                        Some(b) => b,
                                        None => return Value::Undefined,
                                    };
                                    if self.pending_throw.is_some() {
                                        return Value::Undefined;
                                    }
                                    let coerced = coerce_bigint_for_ta(&bi, &ta_name);
                                    {
                                        let mut t = target_arr.borrow_mut(ctx);
                                        t.elements[offset + i] = Value::BigInt(Box::new(coerced));
                                    }
                                    self.sync_ta_element_to_buffer(ctx, target_arr, offset + i, 0.0, &ta_name);
                                } else {
                                    let num = match self.extract_number_with_coercion(ctx, &v) {
                                        Some(n) => n,
                                        None => return Value::Undefined,
                                    };
                                    if self.pending_throw.is_some() {
                                        return Value::Undefined;
                                    }
                                    let converted = Self::typed_array_coerce_value(num, &ta_name);
                                    {
                                        let mut t = target_arr.borrow_mut(ctx);
                                        t.elements[offset + i] = Value::Number(converted);
                                    }
                                    self.sync_ta_element_to_buffer(ctx, target_arr, offset + i, num, &ta_name);
                                }
                            }
                            return Value::Undefined;
                        }
                        Value::Undefined | Value::Null => {
                            self.throw_type_error(ctx, "Cannot convert undefined or null to object");
                            return Value::Undefined;
                        }
                        _ => source.clone(),
                    };

                    // Save original target length BEFORE source.length getter
                    let original_target_len = target_arr.borrow().elements.len();

                    // Get length from source (may trigger resize via Proxy getter)
                    let len_val = self.read_named_property(ctx, &src_obj, "length");
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    let src_len = match self.extract_number_with_coercion(ctx, &len_val) {
                        Some(n) if n.is_finite() && n >= 0.0 => n as usize,
                        Some(_) => 0,
                        None => return Value::Undefined,
                    };
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }

                    // Re-validate target after source.length getter (may have resized buffer)
                    if Self::is_ta_resizable(target_arr) {
                        self.sync_resizable_ta_elements(ctx, target_arr);
                        if self.is_typed_array_oob(target_arr) {
                            return Value::Undefined;
                        }
                    }

                    // Check using ORIGINAL target length per spec
                    if offset + src_len > original_target_len {
                        self.throw_range_error_object(ctx, "offset is out of bounds");
                        return Value::Undefined;
                    }

                    let target_is_bigint = is_bigint_typed_array(&ta_name);
                    for i in 0..src_len {
                        // Step b: Get value from source (may resize via Proxy)
                        let v = self.read_named_property(ctx, &src_obj, &i.to_string());
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }

                        // Step d: TypedArraySetElement — coerce then check bounds
                        if target_is_bigint {
                            let bi = match self.value_to_bigint(ctx, &v) {
                                Some(b) => b,
                                None => return Value::Undefined,
                            };
                            if self.pending_throw.is_some() {
                                return Value::Undefined;
                            }
                            // After coercion, sync and check IsValidIntegerIndex
                            if Self::is_ta_resizable(target_arr) {
                                self.sync_resizable_ta_elements(ctx, target_arr);
                            }
                            let cur_len = target_arr.borrow().elements.len();
                            if offset + i < cur_len {
                                let coerced = coerce_bigint_for_ta(&bi, &ta_name);
                                {
                                    let mut t = target_arr.borrow_mut(ctx);
                                    t.elements[offset + i] = Value::BigInt(Box::new(coerced));
                                }
                                self.sync_ta_element_to_buffer(ctx, target_arr, offset + i, 0.0, &ta_name);
                            }
                        } else {
                            let num = match self.extract_number_with_coercion(ctx, &v) {
                                Some(n) => n,
                                None => return Value::Undefined,
                            };
                            if self.pending_throw.is_some() {
                                return Value::Undefined;
                            }
                            // After coercion, sync and check IsValidIntegerIndex
                            if Self::is_ta_resizable(target_arr) {
                                self.sync_resizable_ta_elements(ctx, target_arr);
                            }
                            let cur_len = target_arr.borrow().elements.len();
                            if offset + i < cur_len {
                                let converted = Self::typed_array_coerce_value(num, &ta_name);
                                {
                                    let mut t = target_arr.borrow_mut(ctx);
                                    t.elements[offset + i] = Value::Number(converted);
                                }
                                self.sync_ta_element_to_buffer(ctx, target_arr, offset + i, num, &ta_name);
                            }
                        }
                    }
                }
                Value::Undefined
            }
            "typedarray.subarray" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                // Don't validate detached buffer here - spec says subarray coerces args first
                let Value::Array(arr) = &this_val else {
                    self.throw_type_error(ctx, "%TypedArray%.prototype.subarray called on incompatible receiver");
                    return Value::Undefined;
                };
                {
                    let a = arr.borrow();
                    if !a.props.contains_key("__typedarray_name__") {
                        drop(a);
                        self.throw_type_error(ctx, "%TypedArray%.prototype.subarray called on incompatible receiver");
                        return Value::Undefined;
                    }
                }
                self.sync_resizable_ta_elements(ctx, arr);
                let (len, ta_name, buffer, byte_offset, bpe, is_auto_length) = {
                    let a = arr.borrow();
                    let len = a.elements.len();
                    let ta_name = match a.props.get("__typedarray_name__") {
                        Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                        _ => "Int8Array".to_string(),
                    };
                    let buffer = a.props.get("__typedarray_buffer__").or_else(|| a.props.get("buffer")).cloned();
                    let byte_offset = match a.props.get("__byte_offset__") {
                        Some(Value::Number(n)) => *n as usize,
                        _ => 0,
                    };
                    let bpe = match a.props.get("__bytes_per_element__") {
                        Some(Value::Number(n)) => *n as usize,
                        _ => 1,
                    };
                    let is_auto = matches!(a.props.get("__length_tracking__"), Some(Value::Boolean(true)));
                    (len, ta_name, buffer, byte_offset, bpe, is_auto)
                };
                let end_is_undefined = matches!(args.get(1), Some(Value::Undefined) | None);
                let begin_raw = match args.first() {
                    None | Some(Value::Undefined) => 0i64,
                    Some(v) => match self.extract_number_with_coercion(ctx, v) {
                        Some(n) if n.is_nan() => 0,
                        Some(n) => n as i64,
                        None => return Value::Undefined,
                    },
                };
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let end_raw = if end_is_undefined {
                    len as i64
                } else {
                    match args.get(1) {
                        Some(v) => match self.extract_number_with_coercion(ctx, v) {
                            Some(n) if n.is_nan() => 0,
                            Some(n) => n as i64,
                            None => return Value::Undefined,
                        },
                        None => len as i64,
                    }
                };
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let begin = if begin_raw < 0 {
                    (len as i64 + begin_raw).max(0) as usize
                } else {
                    (begin_raw as usize).min(len)
                };
                let end = if end_raw < 0 {
                    (len as i64 + end_raw).max(0) as usize
                } else {
                    (end_raw as usize).min(len)
                };
                let count = end.saturating_sub(begin);
                let new_byte_offset = byte_offset + begin * bpe;
                // Per spec step 14: if source is auto-length and end is undefined,
                // create auto-length subarray (no length argument)
                if let Some(buf_val) = buffer {
                    if is_auto_length && end_is_undefined {
                        let args = [buf_val, Value::Number(new_byte_offset as f64)];
                        self.typed_array_species_create(ctx, &this_val, &args).unwrap_or(Value::Undefined)
                    } else {
                        let args = [buf_val, Value::Number(new_byte_offset as f64), Value::Number(count as f64)];
                        self.typed_array_species_create(ctx, &this_val, &args).unwrap_or(Value::Undefined)
                    }
                } else {
                    // No buffer, just slice elements
                    let a = arr.borrow();
                    let elems: Vec<Value<'gc>> = a.elements[begin..end.min(a.elements.len())].to_vec();
                    drop(a);
                    let mut data = VmArrayData::new(elems);
                    data.props.insert("__typedarray_name__".to_string(), Value::from(ta_name.as_str()));
                    data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
                    Value::Array(new_gc_cell_ptr(ctx, data))
                }
            }
            "typedarray.from" => {
                // TypedArray.from(source [, mapFn [, thisArg]])
                let ctor = receiver.unwrap_or(&Value::Undefined).clone();

                // Step 1: If this is not a constructor, throw TypeError
                if !self.is_constructor_value(&ctor) {
                    self.throw_type_error(ctx, "TypedArray.from requires a constructor");
                    return Value::Undefined;
                }

                let source = args.first().cloned().unwrap_or(Value::Undefined);
                let map_fn = args.get(1).cloned();
                let this_arg = args.get(2).cloned().unwrap_or(Value::Undefined);

                // Step 2: If mapFn is not undefined, it must be callable
                if let Some(ref mf) = map_fn
                    && !matches!(mf, Value::Undefined)
                    && !self.is_callable_value(mf)
                {
                    self.throw_type_error(ctx, "mapFn is not a function");
                    return Value::Undefined;
                }

                // Collect source items
                let items: Vec<Value<'gc>> = match &source {
                    Value::Array(src) => src.borrow().elements.clone(),
                    _ => {
                        // Try iterator protocol
                        let iterator_method = self.read_named_property(ctx, &source, "@@sym:1");
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        if !matches!(iterator_method, Value::Undefined | Value::Null) {
                            let mut items = Vec::new();
                            let iterator = self.vm_call_function_value(ctx, &iterator_method, &source, &[]);
                            if let Err(e) = iterator {
                                self.set_pending_throw_from_error(&e);
                                return Value::Undefined;
                            }
                            let iterator = iterator.unwrap();
                            if !matches!(
                                iterator,
                                Value::Object(_) | Value::Array(_) | Value::Function(..) | Value::Closure(..) | Value::NativeFunction(_)
                            ) {
                                self.throw_type_error(ctx, "iterator must return an object");
                                return Value::Undefined;
                            }
                            loop {
                                let next_method = self.read_named_property(ctx, &iterator, "next");
                                if self.pending_throw.is_some() {
                                    return Value::Undefined;
                                }
                                if !self.is_value_callable(&next_method) {
                                    self.throw_type_error(ctx, "iterator.next is not callable");
                                    return Value::Undefined;
                                }
                                let result = self.vm_call_function_value(ctx, &next_method, &iterator, &[]);
                                if let Err(e) = result {
                                    self.set_pending_throw_from_error(&e);
                                    return Value::Undefined;
                                }
                                let result = result.unwrap();
                                if !matches!(
                                    result,
                                    Value::Object(_)
                                        | Value::Array(_)
                                        | Value::Function(..)
                                        | Value::Closure(..)
                                        | Value::NativeFunction(_)
                                ) {
                                    self.throw_type_error(ctx, "iterator result is not an object");
                                    return Value::Undefined;
                                }
                                let done = self.read_named_property(ctx, &result, "done");
                                if done.to_truthy() {
                                    break;
                                }
                                let value = self.read_named_property(ctx, &result, "value");
                                items.push(value);
                            }
                            items
                        } else {
                            // array-like
                            let len_val = self.read_named_property(ctx, &source, "length");
                            if self.pending_throw.is_some() {
                                return Value::Undefined;
                            }
                            let len = match self.extract_number_with_coercion(ctx, &len_val) {
                                Some(n) if n.is_finite() && n >= 0.0 => n as usize,
                                Some(_) => 0,
                                None => return Value::Undefined,
                            };
                            if self.pending_throw.is_some() {
                                return Value::Undefined;
                            }
                            let mut items = Vec::with_capacity(len);
                            for i in 0..len {
                                let v = self.read_named_property(ctx, &source, &i.to_string());
                                if self.pending_throw.is_some() {
                                    return Value::Undefined;
                                }
                                items.push(v);
                            }
                            items
                        }
                    }
                };
                // Construct TypedArray first, then interleave mapfn+Set per element (spec step 6e/8f)
                let has_mapping = matches!(&map_fn, Some(mf) if !matches!(mf, Value::Undefined));
                let result = self.construct_value(ctx, &ctor, &[Value::Number(items.len() as f64)], None);
                match result {
                    Ok(ta @ Value::Array(_)) => {
                        if !self.validate_typed_array(ctx, &ta, "from") {
                            return Value::Undefined;
                        }
                        let ta_len = if let Value::Array(a) = &ta { a.borrow().elements.len() } else { 0 };
                        if ta_len < items.len() {
                            self.throw_type_error(ctx, "TypedArray is too small");
                            return Value::Undefined;
                        }
                        for (i, item) in items.into_iter().enumerate() {
                            let mapped_value = if has_mapping {
                                let mf = map_fn.as_ref().unwrap();
                                match self.vm_call_function_value(ctx, mf, &this_arg, &[item, Value::Number(i as f64)]) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        self.set_pending_throw_from_error(&e);
                                        return Value::Undefined;
                                    }
                                }
                            } else {
                                item
                            };
                            let key = i.to_string();
                            if let Err(e) = self.assign_named_property(ctx, &ta, &key, &mapped_value, None) {
                                self.set_pending_throw_from_error(&e);
                                return Value::Undefined;
                            }
                            if self.pending_throw.is_some() {
                                return Value::Undefined;
                            }
                        }
                        ta
                    }
                    Ok(v) => {
                        if !self.validate_typed_array(ctx, &v, "from") {
                            return Value::Undefined;
                        }
                        v
                    }
                    Err(e) => {
                        self.set_pending_throw_from_error(&e);
                        Value::Undefined
                    }
                }
            }
            "typedarray.of" => {
                // TypedArray.of(...items)
                let ctor = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.is_constructor_value(&ctor) {
                    self.throw_type_error(ctx, "TypedArray.of requires a constructor");
                    return Value::Undefined;
                }
                let len = args.len();
                let result = self.construct_value(ctx, &ctor, &[Value::Number(len as f64)], None);
                match result {
                    Ok(ta @ Value::Array(_)) => {
                        if !self.validate_typed_array(ctx, &ta, "of") {
                            return Value::Undefined;
                        }
                        // TypedArrayCreate: verify length >= required
                        let ta_len = if let Value::Array(a) = &ta { a.borrow().elements.len() } else { 0 };
                        if ta_len < len {
                            self.throw_type_error(ctx, "TypedArray is too small");
                            return Value::Undefined;
                        }
                        // Per spec, each Set call coerces the value, which may resize the buffer.
                        // After coercion, if the index is out of bounds, the Set is a no-op.
                        let ta_name = if let Value::Array(a) = &ta {
                            match a.borrow().props.get("__typedarray_name__") {
                                Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                                _ => "Uint8Array".to_string(),
                            }
                        } else {
                            "Uint8Array".to_string()
                        };
                        let is_bigint = is_bigint_typed_array(&ta_name);
                        for (i, v) in args.iter().enumerate() {
                            if v.is_symbol_value() {
                                self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                                return Value::Undefined;
                            }
                            if is_bigint {
                                let bi = match self.value_to_bigint(ctx, v) {
                                    Some(b) => b,
                                    None => return Value::Undefined,
                                };
                                if self.pending_throw.is_some() {
                                    return Value::Undefined;
                                }
                                // After coercion, sync and check bounds
                                if let Value::Array(a) = &ta {
                                    self.sync_resizable_ta_elements(ctx, a);
                                    if i < a.borrow().elements.len() {
                                        let coerced = coerce_bigint_for_ta(&bi, &ta_name);
                                        a.borrow_mut(ctx).elements[i] = Value::BigInt(Box::new(coerced));
                                        self.sync_ta_element_to_buffer(ctx, a, i, 0.0, &ta_name);
                                    }
                                }
                            } else {
                                let num = match self.extract_number_with_coercion(ctx, v) {
                                    Some(n) => n,
                                    None => return Value::Undefined,
                                };
                                if self.pending_throw.is_some() {
                                    return Value::Undefined;
                                }
                                // After coercion, sync and check bounds
                                if let Value::Array(a) = &ta {
                                    self.sync_resizable_ta_elements(ctx, a);
                                    if i < a.borrow().elements.len() {
                                        let converted = Self::typed_array_coerce_value(num, &ta_name);
                                        a.borrow_mut(ctx).elements[i] = Value::Number(converted);
                                        self.sync_ta_element_to_buffer(ctx, a, i, num, &ta_name);
                                    }
                                }
                            }
                        }
                        ta
                    }
                    Ok(v) => {
                        // Custom constructor returned non-TypedArray → TypeError
                        if !self.validate_typed_array(ctx, &v, "of") {
                            return Value::Undefined;
                        }
                        v
                    }
                    Err(e) => {
                        self.set_pending_throw_from_error(&e);
                        Value::Undefined
                    }
                }
            }
            "typedarray.values" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "values") {
                    return Value::Undefined;
                }
                if let Value::Array(arr) = &this_val {
                    self.sync_resizable_ta_elements(ctx, arr);
                }
                // Create a "value" iterator directly
                let mut obj = IndexMap::new();
                obj.insert("__iter_target__".to_string(), this_val);
                obj.insert("__index__".to_string(), Value::Number(0.0));
                obj.insert("__iter_kind__".to_string(), Value::from("value"));
                obj.insert("@@sym:1".to_string(), Self::make_host_fn(ctx, "iterator.self"));
                if let Some(proto) = self.globals.get("__ArrayIteratorPrototype__").cloned() {
                    obj.insert("__proto__".to_string(), proto);
                }
                Value::Object(new_gc_cell_ptr(ctx, obj))
            }
            "typedarray.entries" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "entries") {
                    return Value::Undefined;
                }
                if let Value::Array(arr) = &this_val {
                    self.sync_resizable_ta_elements(ctx, arr);
                }
                // Create an "entry" iterator directly (key+value pairs)
                let mut obj = IndexMap::new();
                obj.insert("__iter_target__".to_string(), this_val);
                obj.insert("__index__".to_string(), Value::Number(0.0));
                obj.insert("__iter_kind__".to_string(), Value::from("entry"));
                obj.insert("@@sym:1".to_string(), Self::make_host_fn(ctx, "iterator.self"));
                if let Some(proto) = self.globals.get("__ArrayIteratorPrototype__").cloned() {
                    obj.insert("__proto__".to_string(), proto);
                }
                Value::Object(new_gc_cell_ptr(ctx, obj))
            }
            "typedarray.keys_iter" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "keys") {
                    return Value::Undefined;
                }
                if let Value::Array(arr) = &this_val {
                    self.sync_resizable_ta_elements(ctx, arr);
                }
                // Create a "key" iterator directly
                let mut obj = IndexMap::new();
                obj.insert("__iter_target__".to_string(), this_val);
                obj.insert("__index__".to_string(), Value::Number(0.0));
                obj.insert("__iter_kind__".to_string(), Value::from("key"));
                obj.insert("@@sym:1".to_string(), Self::make_host_fn(ctx, "iterator.self"));
                if let Some(proto) = self.globals.get("__ArrayIteratorPrototype__").cloned() {
                    obj.insert("__proto__".to_string(), proto);
                }
                Value::Object(new_gc_cell_ptr(ctx, obj))
            }
            // Delegating methods: validate this is TypedArray, then call Array builtin
            "typedarray.join"
            | "typedarray.indexOf"
            | "typedarray.forEach"
            | "typedarray.reduce"
            | "typedarray.find"
            | "typedarray.findIndex"
            | "typedarray.includes"
            | "typedarray.at"
            | "typedarray.every"
            | "typedarray.some"
            | "typedarray.lastIndexOf"
            | "typedarray.findLast"
            | "typedarray.findLastIndex"
            | "typedarray.reduceRight" => {
                let this_val = receiver.unwrap_or(&Value::Undefined);
                let method = name.strip_prefix("typedarray.").unwrap_or(name);
                if !self.validate_typed_array(ctx, this_val, method) {
                    return Value::Undefined;
                }
                // Sync elements for resizable buffer before delegating
                if let Value::Array(arr) = this_val {
                    self.sync_resizable_ta_elements(ctx, arr);
                }
                // Map to corresponding Array builtin
                let builtin_id = match name {
                    "typedarray.join" => BUILTIN_ARRAY_JOIN,
                    "typedarray.indexOf" => BUILTIN_ARRAY_INDEXOF,
                    "typedarray.forEach" => BUILTIN_ARRAY_FOREACH,
                    "typedarray.reduce" => BUILTIN_ARRAY_REDUCE,
                    "typedarray.find" => BUILTIN_ARRAY_FIND,
                    "typedarray.findIndex" => BUILTIN_ARRAY_FINDINDEX,
                    "typedarray.includes" => BUILTIN_ARRAY_INCLUDES,
                    "typedarray.at" => BUILTIN_ARRAY_AT,
                    "typedarray.every" => BUILTIN_ARRAY_EVERY,
                    "typedarray.some" => BUILTIN_ARRAY_SOME,
                    "typedarray.lastIndexOf" => BUILTIN_ARRAY_LASTINDEXOF,
                    "typedarray.findLast" => BUILTIN_ARRAY_FINDLAST,
                    "typedarray.findLastIndex" => BUILTIN_ARRAY_FINDLASTINDEX,
                    "typedarray.reduceRight" => BUILTIN_ARRAY_REDUCERIGHT,
                    _ => unreachable!(),
                };
                let old_ta_method = self.in_typed_array_method;
                self.in_typed_array_method = true;
                let result = self.call_method_builtin(ctx, builtin_id, this_val, args);
                self.in_typed_array_method = old_ta_method;
                result
            }
            "typedarray.fill" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "fill") {
                    return Value::Undefined;
                }
                let Value::Array(arr) = &this_val else {
                    return Value::Undefined;
                };
                self.sync_resizable_ta_elements(ctx, arr);
                let (ta_name, _bpe) = {
                    let a = arr.borrow();
                    let name = match a.props.get("__typedarray_name__") {
                        Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                        _ => "Uint8Array".to_string(),
                    };
                    let bpe = match a.props.get("__bytes_per_element__") {
                        Some(Value::Number(n)) => *n as usize,
                        _ => 1,
                    };
                    (name, bpe)
                };

                // Convert fill value to Number/BigInt depending on TA type
                let fill_val = args.first().cloned().unwrap_or(Value::Undefined);
                let bigint_fill: Option<num_bigint::BigInt>;
                let num: f64;
                if is_bigint_typed_array(&ta_name) {
                    let bi = match self.value_to_bigint(ctx, &fill_val) {
                        Some(b) => b,
                        None => return Value::Undefined,
                    };
                    bigint_fill = Some(bi);
                    num = 0.0;
                } else {
                    bigint_fill = None;
                    num = match self.extract_number_with_coercion(ctx, &fill_val) {
                        Some(n) => n,
                        None => return Value::Undefined,
                    };
                }
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }

                // Re-validate after coercion (buffer may have been detached)
                if !self.validate_typed_array(ctx, &this_val, "fill") {
                    return Value::Undefined;
                }

                let len = arr.borrow().elements.len();

                // ToInteger for start
                let start_raw = if let Some(s) = args.get(1) {
                    if matches!(s, Value::Undefined) {
                        0i64
                    } else {
                        match self.extract_number_with_coercion(ctx, s) {
                            Some(n) if n.is_nan() => 0,
                            Some(n) => n as i64,
                            None => return Value::Undefined,
                        }
                    }
                } else {
                    0
                };
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }

                // Re-validate after coercion
                if !self.validate_typed_array(ctx, &this_val, "fill") {
                    return Value::Undefined;
                }

                // ToInteger for end
                let end_raw = if let Some(e) = args.get(2) {
                    if matches!(e, Value::Undefined) {
                        len as i64
                    } else {
                        match self.extract_number_with_coercion(ctx, e) {
                            Some(n) if n.is_nan() => 0,
                            Some(n) => n as i64,
                            None => return Value::Undefined,
                        }
                    }
                } else {
                    len as i64
                };
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }

                // Re-validate after coercion
                if !self.validate_typed_array(ctx, &this_val, "fill") {
                    return Value::Undefined;
                }

                let start = if start_raw < 0 {
                    (len as i64 + start_raw).max(0) as usize
                } else {
                    (start_raw as usize).min(len)
                };
                let end = if end_raw < 0 {
                    (len as i64 + end_raw).max(0) as usize
                } else {
                    (end_raw as usize).min(len)
                };

                for i in start..end {
                    if let Some(ref bi) = bigint_fill {
                        let coerced = coerce_bigint_for_ta(bi, &ta_name);
                        {
                            let mut a = arr.borrow_mut(ctx);
                            a.elements[i] = Value::BigInt(Box::new(coerced));
                        }
                        self.sync_ta_element_to_buffer(ctx, arr, i, 0.0, &ta_name);
                    } else {
                        let converted = Self::typed_array_coerce_value(num, &ta_name);
                        {
                            let mut a = arr.borrow_mut(ctx);
                            a.elements[i] = Value::Number(converted);
                        }
                        self.sync_ta_element_to_buffer(ctx, arr, i, num, &ta_name);
                    }
                }
                this_val
            }
            "typedarray.sort" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                let comparefn = args.first().cloned();

                // Step 1: If comparefn is not undefined and not callable, throw
                if let Some(ref cf) = comparefn
                    && !matches!(cf, Value::Undefined)
                    && !self.is_callable_value(cf)
                {
                    self.throw_type_error(ctx, "comparefn is not a function");
                    return Value::Undefined;
                }

                if !self.validate_typed_array(ctx, &this_val, "sort") {
                    return Value::Undefined;
                }
                let Value::Array(arr) = &this_val else {
                    return Value::Undefined;
                };
                self.sync_resizable_ta_elements(ctx, arr);

                let (ta_name, _bpe) = {
                    let a = arr.borrow();
                    let name = match a.props.get("__typedarray_name__") {
                        Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                        _ => "Uint8Array".to_string(),
                    };
                    let bpe = match a.props.get("__bytes_per_element__") {
                        Some(Value::Number(n)) => *n as usize,
                        _ => 1,
                    };
                    (name, bpe)
                };

                let has_custom = matches!(&comparefn, Some(v) if !matches!(v, Value::Undefined));

                if is_bigint_typed_array(&ta_name) {
                    // BigInt sort path
                    let mut bi_elements: Vec<num_bigint::BigInt> = {
                        let a = arr.borrow();
                        a.elements
                            .iter()
                            .map(|v| match v {
                                Value::BigInt(bi) => (**bi).clone(),
                                _ => num_bigint::BigInt::from(0),
                            })
                            .collect()
                    };

                    if has_custom {
                        let cmp_fn = comparefn.unwrap();
                        let mut had_error = false;
                        bi_elements.sort_by(|a, b| {
                            if had_error {
                                return std::cmp::Ordering::Equal;
                            }
                            let result = self.vm_call_function_value(
                                ctx,
                                &cmp_fn,
                                &Value::Undefined,
                                &[Value::BigInt(Box::new(a.clone())), Value::BigInt(Box::new(b.clone()))],
                            );
                            match result {
                                Ok(v) => {
                                    let n = match self.extract_number_with_coercion(ctx, &v) {
                                        Some(n) => n,
                                        None => {
                                            had_error = true;
                                            return std::cmp::Ordering::Equal;
                                        }
                                    };
                                    if self.pending_throw.is_some() {
                                        had_error = true;
                                        return std::cmp::Ordering::Equal;
                                    }
                                    if n.is_nan() {
                                        std::cmp::Ordering::Equal
                                    } else if n < 0.0 {
                                        std::cmp::Ordering::Less
                                    } else if n > 0.0 {
                                        std::cmp::Ordering::Greater
                                    } else {
                                        std::cmp::Ordering::Equal
                                    }
                                }
                                Err(e) => {
                                    had_error = true;
                                    self.set_pending_throw_from_error(&e);
                                    std::cmp::Ordering::Equal
                                }
                            }
                        });
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                    } else {
                        bi_elements.sort();
                    }

                    // Re-sync after sort (comparefn may have resized buffer)
                    self.sync_resizable_ta_elements(ctx, arr);
                    let new_len = arr.borrow().elements.len();
                    {
                        let mut a = arr.borrow_mut(ctx);
                        for (i, bi) in bi_elements.iter().enumerate() {
                            if i >= new_len {
                                break;
                            }
                            let coerced = coerce_bigint_for_ta(bi, &ta_name);
                            a.elements[i] = Value::BigInt(Box::new(coerced));
                        }
                    }
                    for i in 0..new_len.min(bi_elements.len()) {
                        self.sync_ta_element_to_buffer(ctx, arr, i, 0.0, &ta_name);
                    }
                } else {
                    // Number sort path
                    let mut elements: Vec<f64> = {
                        let a = arr.borrow();
                        a.elements.iter().map(|v| to_number(v)).collect()
                    };

                    if has_custom {
                        let cmp_fn = comparefn.unwrap();
                        let mut had_error = false;
                        elements.sort_by(|a, b| {
                            if had_error {
                                return std::cmp::Ordering::Equal;
                            }
                            let result =
                                self.vm_call_function_value(ctx, &cmp_fn, &Value::Undefined, &[Value::Number(*a), Value::Number(*b)]);
                            match result {
                                Ok(v) => {
                                    let n = match self.extract_number_with_coercion(ctx, &v) {
                                        Some(n) => n,
                                        None => {
                                            had_error = true;
                                            return std::cmp::Ordering::Equal;
                                        }
                                    };
                                    if self.pending_throw.is_some() {
                                        had_error = true;
                                        return std::cmp::Ordering::Equal;
                                    }
                                    if n.is_nan() {
                                        std::cmp::Ordering::Equal
                                    } else if n < 0.0 {
                                        std::cmp::Ordering::Less
                                    } else if n > 0.0 {
                                        std::cmp::Ordering::Greater
                                    } else {
                                        std::cmp::Ordering::Equal
                                    }
                                }
                                Err(e) => {
                                    had_error = true;
                                    self.set_pending_throw_from_error(&e);
                                    std::cmp::Ordering::Equal
                                }
                            }
                        });
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                    } else {
                        // Default numeric sort: NaN at end, -0 before +0
                        elements.sort_by(|a, b| {
                            if a.is_nan() && b.is_nan() {
                                return std::cmp::Ordering::Equal;
                            }
                            if a.is_nan() {
                                return std::cmp::Ordering::Greater;
                            }
                            if b.is_nan() {
                                return std::cmp::Ordering::Less;
                            }
                            if *a == 0.0 && *b == 0.0 {
                                let a_neg = a.is_sign_negative();
                                let b_neg = b.is_sign_negative();
                                if a_neg && !b_neg {
                                    return std::cmp::Ordering::Less;
                                }
                                if !a_neg && b_neg {
                                    return std::cmp::Ordering::Greater;
                                }
                                return std::cmp::Ordering::Equal;
                            }
                            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }

                    // Re-sync after sort (comparefn may have resized buffer)
                    self.sync_resizable_ta_elements(ctx, arr);
                    let new_len = arr.borrow().elements.len();

                    // Write back sorted elements and sync to buffer
                    {
                        let mut a = arr.borrow_mut(ctx);
                        for (i, &num) in elements.iter().enumerate() {
                            if i >= new_len {
                                break;
                            }
                            let converted = Self::typed_array_coerce_value(num, &ta_name);
                            a.elements[i] = Value::Number(converted);
                        }
                    }
                    for (i, &num) in elements.iter().enumerate() {
                        if i >= new_len {
                            break;
                        }
                        self.sync_ta_element_to_buffer(ctx, arr, i, num, &ta_name);
                    }
                }
                this_val
            }
            "typedarray.reverse" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "reverse") {
                    return Value::Undefined;
                }
                let Value::Array(arr) = &this_val else {
                    return Value::Undefined;
                };
                self.sync_resizable_ta_elements(ctx, arr);
                let (ta_name, bpe) = {
                    let a = arr.borrow();
                    let name = match a.props.get("__typedarray_name__") {
                        Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                        _ => "Uint8Array".to_string(),
                    };
                    let bpe = match a.props.get("__bytes_per_element__") {
                        Some(Value::Number(n)) => *n as usize,
                        _ => 1,
                    };
                    (name, bpe)
                };

                let len = arr.borrow().elements.len();
                // Sync elements from buffer first (in case shared buffer was modified)
                self.sync_ta_elements_from_buffer(ctx, arr, &ta_name, bpe, len);
                // Reverse elements in-place
                {
                    let mut a = arr.borrow_mut(ctx);
                    a.elements.reverse();
                }
                // Sync all elements to buffer
                for i in 0..len {
                    let num = {
                        let a = arr.borrow();
                        to_number(&a.elements[i])
                    };
                    self.sync_ta_element_to_buffer(ctx, arr, i, num, &ta_name);
                }
                this_val
            }
            // TypedArray map/filter/slice: must return same-type TypedArray
            "typedarray.map" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "map") {
                    return Value::Undefined;
                }
                let callback = match args.first() {
                    Some(cb) if self.is_callable_value(cb) => cb.clone(),
                    _ => {
                        self.throw_type_error(ctx, "TypedArray.prototype.map callback is not a function");
                        return Value::Undefined;
                    }
                };
                let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
                let len = if let Value::Array(arr) = &this_val {
                    self.sync_resizable_ta_elements(ctx, arr);
                    arr.borrow().elements.len()
                } else {
                    return Value::Undefined;
                };
                // Per spec: TypedArraySpeciesCreate BEFORE iteration
                let Some(result) = self.typed_array_species_create(ctx, &this_val, &[Value::Number(len as f64)]) else {
                    return Value::Undefined;
                };
                let res_ta_name = if let Value::Array(res_arr) = &result {
                    res_arr
                        .borrow()
                        .props
                        .get("__typedarray_name__")
                        .map(value_to_string)
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                for k in 0..len {
                    let k_value = if let Value::Array(arr) = &this_val {
                        self.maybe_sync_resizable_ta(ctx, arr);
                        arr.borrow().elements.get(k).cloned().unwrap_or(Value::Undefined)
                    } else {
                        Value::Undefined
                    };
                    let mapped =
                        match self.vm_call_function_value(ctx, &callback, &this_arg, &[k_value, Value::Number(k as f64), this_val.clone()])
                        {
                            Ok(v) => v,
                            Err(e) => {
                                self.set_pending_throw_from_error(&e);
                                return Value::Undefined;
                            }
                        };
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    if let Value::Array(res_arr) = &result {
                        if is_bigint_typed_array(&res_ta_name) {
                            let bi = match &mapped {
                                Value::BigInt(b) => (**b).clone(),
                                _ => match self.value_to_bigint(ctx, &mapped) {
                                    Some(b) => b,
                                    None => return Value::Undefined,
                                },
                            };
                            let coerced = coerce_bigint_for_ta(&bi, &res_ta_name);
                            if k < res_arr.borrow().elements.len() {
                                res_arr.borrow_mut(ctx).elements[k] = Value::BigInt(Box::new(coerced));
                            }
                            self.sync_ta_element_to_buffer(ctx, res_arr, k, 0.0, &res_ta_name);
                        } else {
                            let num = to_number(&mapped);
                            let coerced = Value::Number(Self::typed_array_coerce_value(num, &res_ta_name));
                            if k < res_arr.borrow().elements.len() {
                                res_arr.borrow_mut(ctx).elements[k] = coerced;
                            }
                            self.sync_ta_element_to_buffer(ctx, res_arr, k, num, &res_ta_name);
                        }
                    }
                }
                result
            }
            "typedarray.filter" => {
                let this_val = receiver.unwrap_or(&Value::Undefined);
                if !self.validate_typed_array(ctx, this_val, "filter") {
                    return Value::Undefined;
                }
                if let Value::Array(arr) = this_val {
                    self.sync_resizable_ta_elements(ctx, arr);
                }
                let old_ta_method = self.in_typed_array_method;
                self.in_typed_array_method = true;
                let filtered = self.call_method_builtin(ctx, BUILTIN_ARRAY_FILTER, this_val, args);
                self.in_typed_array_method = old_ta_method;
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                // Extract filtered elements
                let elements = match &filtered {
                    Value::Array(arr) => arr.borrow().elements.clone(),
                    _ => return filtered,
                };
                let len = elements.len();
                // Use species constructor
                let Some(result) = self.typed_array_species_create(ctx, this_val, &[Value::Number(len as f64)]) else {
                    return Value::Undefined;
                };
                // Copy filtered values into result
                if let Value::Array(res_arr) = &result {
                    let ta_name = res_arr
                        .borrow()
                        .props
                        .get("__typedarray_name__")
                        .map(value_to_string)
                        .unwrap_or_default();
                    if is_bigint_typed_array(&ta_name) {
                        for (i, v) in elements.iter().enumerate() {
                            let bi = match v {
                                Value::BigInt(b) => (**b).clone(),
                                _ => num_bigint::BigInt::from(0),
                            };
                            let coerced = coerce_bigint_for_ta(&bi, &ta_name);
                            if i < res_arr.borrow().elements.len() {
                                res_arr.borrow_mut(ctx).elements[i] = Value::BigInt(Box::new(coerced));
                            }
                            self.sync_ta_element_to_buffer(ctx, res_arr, i, 0.0, &ta_name);
                        }
                    } else {
                        for (i, v) in elements.iter().enumerate() {
                            let num = to_number(v);
                            let coerced = Value::Number(Self::typed_array_coerce_value(num, &ta_name));
                            if i < res_arr.borrow().elements.len() {
                                res_arr.borrow_mut(ctx).elements[i] = coerced.clone();
                            }
                            self.sync_ta_element_to_buffer(ctx, res_arr, i, num, &ta_name);
                        }
                    }
                }
                result
            }
            "typedarray.slice" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "slice") {
                    return Value::Undefined;
                }
                // Get source info
                let (len, ta_name, bpe) = if let Value::Array(arr) = &this_val {
                    self.sync_resizable_ta_elements(ctx, arr);
                    let a = arr.borrow();
                    let name = a.props.get("__typedarray_name__").map(value_to_string).unwrap_or_default();
                    let bpe = match a.props.get("__bytes_per_element__") {
                        Some(Value::Number(n)) => *n as usize,
                        _ => 1,
                    };
                    (a.elements.len() as i64, name, bpe)
                } else {
                    return Value::Undefined;
                };

                // Resolve start with proper ToInteger coercion
                let k = match args.first() {
                    None | Some(Value::Undefined) => 0i64,
                    Some(v) => {
                        let Some(rel_start) = self.extract_number_with_coercion(ctx, v) else {
                            return Value::Undefined;
                        };
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        let rel = if rel_start.is_nan() { 0.0 } else { rel_start.trunc() };
                        if rel < 0.0 {
                            (len + rel as i64).max(0)
                        } else {
                            (rel as i64).min(len)
                        }
                    }
                };

                // Resolve end with proper ToInteger coercion
                let fin = match args.get(1) {
                    None | Some(Value::Undefined) => len,
                    Some(v) => {
                        let Some(rel_end) = self.extract_number_with_coercion(ctx, v) else {
                            return Value::Undefined;
                        };
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        let rel = if rel_end.is_nan() { 0.0 } else { rel_end.trunc() };
                        if rel < 0.0 {
                            (len + rel as i64).max(0)
                        } else {
                            (rel as i64).min(len)
                        }
                    }
                };

                let count = (fin - k).max(0) as usize;

                // TypedArraySpeciesCreate
                let Some(result) = self.typed_array_species_create(ctx, &this_val, &[Value::Number(count as f64)]) else {
                    return Value::Undefined;
                };
                if count == 0 {
                    return result;
                }

                // Step 15.a-b: Re-check OOB after species create (buffer may have been
                // resized during argument coercion or species constructor)
                if let Value::Array(src_arr) = &this_val {
                    // Check detached
                    if let Some(Value::Object(buf)) = src_arr.borrow().props.get("__typedarray_buffer__")
                        && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
                    {
                        self.throw_type_error(ctx, "Cannot perform operation on a detached ArrayBuffer");
                        return Value::Undefined;
                    }
                    // Re-sync and check OOB for resizable
                    self.maybe_sync_resizable_ta(ctx, src_arr);
                    if self.is_typed_array_oob(src_arr) {
                        self.throw_type_error(ctx, "Cannot perform operation on an out-of-bounds TypedArray");
                        return Value::Undefined;
                    }
                }

                if let Value::Array(res_arr) = &result {
                    // Step 15.c: srcLength = TypedArrayLength(O) after potential resize
                    let src_len = if let Value::Array(src_arr) = &this_val {
                        src_arr.borrow().elements.len() as i64
                    } else {
                        0
                    };
                    // Step 15.d: endIndex = min(k + count, srcLength)
                    let end_index = (k + count as i64).min(src_len);
                    let actual_count = (end_index - k).max(0) as usize;

                    let res_ta_name = res_arr
                        .borrow()
                        .props
                        .get("__typedarray_name__")
                        .map(value_to_string)
                        .unwrap_or_default();
                    let src_buf = if let Value::Array(src_arr) = &this_val {
                        src_arr.borrow().props.get("__typedarray_buffer__").cloned()
                    } else {
                        None
                    };
                    let res_buf = res_arr.borrow().props.get("__typedarray_buffer__").cloned();

                    let same_buffer = match (&src_buf, &res_buf) {
                        (Some(Value::Object(a)), Some(Value::Object(b))) => Gc::ptr_eq(*a, *b),
                        _ => false,
                    };

                    if ta_name == res_ta_name {
                        // Same element type: copy raw bytes so Float16/NaN payloads preserve
                        // exact bit patterns instead of round-tripping through f64 values.
                        let src_byte_offset = if let Value::Array(src_arr) = &this_val {
                            match src_arr.borrow().props.get("__byte_offset__") {
                                Some(Value::Number(n)) => *n as usize,
                                _ => 0,
                            }
                        } else {
                            0
                        };
                        let res_byte_offset = match res_arr.borrow().props.get("__byte_offset__") {
                            Some(Value::Number(n)) => *n as usize,
                            _ => 0,
                        };

                        let src_buf_bytes = if let Some(Value::Object(buf_obj)) = &src_buf {
                            buf_obj.borrow().get("__buffer_bytes__").cloned()
                        } else {
                            None
                        };
                        let res_buf_bytes = if let Some(Value::Object(buf_obj)) = &res_buf {
                            buf_obj.borrow().get("__buffer_bytes__").cloned()
                        } else {
                            None
                        };
                        if let (Some(Value::Array(src_bytes)), Some(Value::Array(dst_bytes))) = (src_buf_bytes, res_buf_bytes) {
                            let src_start_byte = src_byte_offset + k as usize * bpe;
                            let target_start_byte = res_byte_offset;
                            if same_buffer {
                                let mut db = dst_bytes.borrow_mut(ctx);
                                for i in 0..(actual_count * bpe) {
                                    let src_idx = src_start_byte + i;
                                    let tgt_idx = target_start_byte + i;
                                    if src_idx < db.elements.len() && tgt_idx < db.elements.len() {
                                        let byte_val = db.elements[src_idx].clone();
                                        db.elements[tgt_idx] = byte_val;
                                    }
                                }
                            } else {
                                let copied = {
                                    let sb = src_bytes.borrow();
                                    let mut copied = Vec::with_capacity(actual_count * bpe);
                                    for i in 0..(actual_count * bpe) {
                                        let src_idx = src_start_byte + i;
                                        copied.push(sb.elements.get(src_idx).cloned().unwrap_or(Value::Number(0.0)));
                                    }
                                    copied
                                };
                                {
                                    let mut db = dst_bytes.borrow_mut(ctx);
                                    for (i, byte_val) in copied.into_iter().enumerate() {
                                        let tgt_idx = target_start_byte + i;
                                        if tgt_idx < db.elements.len() {
                                            db.elements[tgt_idx] = byte_val;
                                        }
                                    }
                                }
                            }
                            // Sync result elements from buffer (all borrows released)
                            let res_elem_len = res_arr.borrow().elements.len();
                            self.sync_ta_elements_from_buffer(ctx, res_arr, &res_ta_name, bpe, res_elem_len);
                        }
                    } else {
                        // Different buffer or different type: element-by-element set
                        let src_elements: Vec<Value<'gc>> = if let Value::Array(src_arr) = &this_val {
                            let a = src_arr.borrow();
                            let start = (k as usize).min(a.elements.len());
                            let end = (k as usize + actual_count).min(a.elements.len());
                            a.elements[start..end].to_vec()
                        } else {
                            vec![]
                        };
                        for (i, v) in src_elements.iter().enumerate() {
                            if is_bigint_typed_array(&res_ta_name) {
                                let bi = match v {
                                    Value::BigInt(b) => (**b).clone(),
                                    _ => num_bigint::BigInt::from(0),
                                };
                                let coerced = coerce_bigint_for_ta(&bi, &res_ta_name);
                                if i < res_arr.borrow().elements.len() {
                                    res_arr.borrow_mut(ctx).elements[i] = Value::BigInt(Box::new(coerced));
                                }
                                self.sync_ta_element_to_buffer(ctx, res_arr, i, 0.0, &res_ta_name);
                            } else {
                                let num = to_number(v);
                                let coerced = Value::Number(Self::typed_array_coerce_value(num, &res_ta_name));
                                if i < res_arr.borrow().elements.len() {
                                    res_arr.borrow_mut(ctx).elements[i] = coerced;
                                }
                                self.sync_ta_element_to_buffer(ctx, res_arr, i, num, &res_ta_name);
                            }
                        }
                    }
                }
                result
            }
            "typedarray.copyWithin" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "copyWithin") {
                    return Value::Undefined;
                }
                let Value::Array(arr) = &this_val else {
                    return Value::Undefined;
                };
                self.sync_resizable_ta_elements(ctx, arr);
                let len = arr.borrow().elements.len() as i128;

                let to_integer_or_infinity = |n: f64| -> i128 {
                    if n.is_nan() {
                        0
                    } else if n.is_infinite() {
                        if n.is_sign_negative() { i128::MIN } else { i128::MAX }
                    } else {
                        n.trunc() as i128
                    }
                };

                let relative_target = match args.first() {
                    Some(v) => {
                        let Some(n) = self.extract_number_with_coercion(ctx, v) else {
                            return Value::Undefined;
                        };
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        to_integer_or_infinity(n)
                    }
                    None => 0,
                };
                let relative_start = match args.get(1) {
                    Some(v) => {
                        let Some(n) = self.extract_number_with_coercion(ctx, v) else {
                            return Value::Undefined;
                        };
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        to_integer_or_infinity(n)
                    }
                    None => 0,
                };
                let relative_end = match args.get(2) {
                    None | Some(Value::Undefined) => len,
                    Some(v) => {
                        let Some(n) = self.extract_number_with_coercion(ctx, v) else {
                            return Value::Undefined;
                        };
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        to_integer_or_infinity(n)
                    }
                };

                // Re-check after argument coercion (buffer may have been resized)
                {
                    let a = arr.borrow();
                    if let Some(Value::Object(buf)) = a.props.get("__typedarray_buffer__")
                        && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
                    {
                        drop(a);
                        self.throw_type_error(ctx, "Cannot perform %TypedArray%.prototype.copyWithin on a detached ArrayBuffer");
                        return Value::Undefined;
                    }
                    drop(a);
                }
                self.maybe_sync_resizable_ta(ctx, arr);
                if self.is_typed_array_oob(arr) {
                    self.throw_type_error(ctx, "Cannot perform operation on an out-of-bounds TypedArray");
                    return Value::Undefined;
                }

                // Clamp to/from/final using ORIGINAL len per spec steps 5-17
                let to = if relative_target < 0 {
                    (len + relative_target).max(0)
                } else {
                    relative_target.min(len)
                };
                let from = if relative_start < 0 {
                    (len + relative_start).max(0)
                } else {
                    relative_start.min(len)
                };
                let final_index = if relative_end < 0 {
                    (len + relative_end).max(0)
                } else {
                    relative_end.min(len)
                };
                let count = (final_index - from).max(0).min(len - to);

                if count > 0 {
                    let a = arr.borrow();
                    let ta_name = match a.props.get("__typedarray_name__") {
                        Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                        _ => "Int8Array".to_string(),
                    };
                    let bpe = match a.props.get("__bytes_per_element__") {
                        Some(Value::Number(n)) => *n as usize,
                        _ => 1,
                    };
                    let mut elems = a.elements.clone();
                    drop(a);

                    let (mut from_idx, mut to_idx, direction) = if from < to && to < from + count {
                        (from + count - 1, to + count - 1, -1i128)
                    } else {
                        (from, to, 1i128)
                    };
                    let mut remaining = count;
                    while remaining > 0 {
                        let fi = from_idx as usize;
                        let ti = to_idx as usize;
                        if fi < elems.len() && ti < elems.len() {
                            elems[ti] = elems[fi].clone();
                        }
                        from_idx += direction;
                        to_idx += direction;
                        remaining -= 1;
                    }
                    {
                        let mut a = arr.borrow_mut(ctx);
                        a.elements = elems;
                    }
                    // Sync to buffer
                    let a = arr.borrow();
                    if let Some(Value::Object(buf)) = a.props.get("__typedarray_buffer__")
                        && let Some(Value::Array(buf_bytes)) = buf.borrow().get("__buffer_bytes__").cloned()
                    {
                        let byte_offset = match a.props.get("__byte_offset__") {
                            Some(Value::Number(n)) => *n as usize,
                            _ => 0,
                        };
                        drop(a);
                        let mut bb = buf_bytes.borrow_mut(ctx);
                        let arr_borrow = arr.borrow();
                        for i in 0..arr_borrow.elements.len() {
                            if is_bigint_typed_array(&ta_name) {
                                if let Value::BigInt(bi) = &arr_borrow.elements[i] {
                                    Self::encode_typed_element_bigint(&mut bb.elements, byte_offset + i * bpe, &ta_name, bi);
                                }
                            } else {
                                let num = to_number(&arr_borrow.elements[i]);
                                Self::encode_typed_element(&mut bb.elements, byte_offset + i * bpe, bpe, &ta_name, num);
                            }
                        }
                    }
                }
                this_val
            }
            "typedarray.toLocaleString" => {
                let this_val = receiver.unwrap_or(&Value::Undefined);
                if !self.validate_typed_array(ctx, this_val, "toLocaleString") {
                    return Value::Undefined;
                }
                if let Value::Array(arr) = this_val {
                    self.sync_resizable_ta_elements(ctx, arr);
                }
                self.call_host_fn(ctx, "array.toLocaleString", Some(this_val), args)
            }
            "typedarray.toReversed" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "toReversed") {
                    return Value::Undefined;
                }
                let Value::Array(arr) = &this_val else {
                    return Value::Undefined;
                };
                self.sync_resizable_ta_elements(ctx, arr);
                let len = arr.borrow().elements.len();
                let ta_name = match arr.borrow().props.get("__typedarray_name__") {
                    Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                    _ => "Uint8Array".to_string(),
                };
                let bpe = match arr.borrow().props.get("__bytes_per_element__") {
                    Some(Value::Number(n)) => *n as usize,
                    _ => 1,
                };
                // Sync elements from buffer
                self.sync_ta_elements_from_buffer(ctx, arr, &ta_name, bpe, len);
                // TypedArrayCreateSameType (ignores @@species)
                let result = match self.typed_array_create_same_type(ctx, &this_val, &[Value::Number(len as f64)]) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let Value::Array(res_arr) = &result else {
                    return result;
                };
                let res_ta_name = match res_arr.borrow().props.get("__typedarray_name__") {
                    Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                    _ => ta_name.clone(),
                };
                // Copy elements in reverse order
                for k in 0..len {
                    let from_value = self.ta_get_element(ctx, arr, len - 1 - k);
                    if k < res_arr.borrow().elements.len() {
                        if is_bigint_typed_array(&res_ta_name) {
                            let bi = match &from_value {
                                Value::BigInt(b) => (**b).clone(),
                                _ => num_bigint::BigInt::from(0),
                            };
                            let coerced = coerce_bigint_for_ta(&bi, &res_ta_name);
                            res_arr.borrow_mut(ctx).elements[k] = Value::BigInt(Box::new(coerced));
                            self.sync_ta_element_to_buffer(ctx, res_arr, k, 0.0, &res_ta_name);
                        } else {
                            let num = to_number(&from_value);
                            let coerced = Self::typed_array_coerce_value(num, &res_ta_name);
                            res_arr.borrow_mut(ctx).elements[k] = Value::Number(coerced);
                            self.sync_ta_element_to_buffer(ctx, res_arr, k, num, &res_ta_name);
                        }
                    }
                }
                result
            }
            "typedarray.toSorted" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                let comparefn = args.first().cloned();

                // Step 1: If comparefn is not undefined and not callable, throw
                if let Some(ref cf) = comparefn
                    && !matches!(cf, Value::Undefined)
                    && !self.is_callable_value(cf)
                {
                    self.throw_type_error(ctx, "comparefn is not a function");
                    return Value::Undefined;
                }

                if !self.validate_typed_array(ctx, &this_val, "toSorted") {
                    return Value::Undefined;
                }
                let Value::Array(arr) = &this_val else {
                    return Value::Undefined;
                };
                self.sync_resizable_ta_elements(ctx, arr);
                let len = arr.borrow().elements.len();
                let ta_name = match arr.borrow().props.get("__typedarray_name__") {
                    Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                    _ => "Uint8Array".to_string(),
                };
                let bpe = match arr.borrow().props.get("__bytes_per_element__") {
                    Some(Value::Number(n)) => *n as usize,
                    _ => 1,
                };
                self.sync_ta_elements_from_buffer(ctx, arr, &ta_name, bpe, len);

                // TypedArrayCreateSameType (ignores @@species)
                let result = match self.typed_array_create_same_type(ctx, &this_val, &[Value::Number(len as f64)]) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let Value::Array(res_arr) = &result else {
                    return result;
                };
                let res_ta_name = match res_arr.borrow().props.get("__typedarray_name__") {
                    Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                    _ => ta_name.clone(),
                };

                // Copy elements from source to result
                for k in 0..len {
                    let from_value = self.ta_get_element(ctx, arr, k);
                    if k < res_arr.borrow().elements.len() {
                        if is_bigint_typed_array(&res_ta_name) {
                            let bi = match &from_value {
                                Value::BigInt(b) => (**b).clone(),
                                _ => num_bigint::BigInt::from(0),
                            };
                            let coerced = coerce_bigint_for_ta(&bi, &res_ta_name);
                            res_arr.borrow_mut(ctx).elements[k] = Value::BigInt(Box::new(coerced));
                        } else {
                            let num = to_number(&from_value);
                            let coerced = Self::typed_array_coerce_value(num, &res_ta_name);
                            res_arr.borrow_mut(ctx).elements[k] = Value::Number(coerced);
                        }
                    }
                }

                // Sort the result in-place (reuse the sort logic)
                let has_custom = matches!(&comparefn, Some(v) if !matches!(v, Value::Undefined));

                if is_bigint_typed_array(&res_ta_name) {
                    let mut bi_elements: Vec<num_bigint::BigInt> = {
                        let a = res_arr.borrow();
                        a.elements
                            .iter()
                            .map(|v| match v {
                                Value::BigInt(bi) => (**bi).clone(),
                                _ => num_bigint::BigInt::from(0),
                            })
                            .collect()
                    };
                    if has_custom {
                        let cmp_fn = comparefn.unwrap();
                        let mut had_error = false;
                        bi_elements.sort_by(|a, b| {
                            if had_error {
                                return std::cmp::Ordering::Equal;
                            }
                            let result = self.vm_call_function_value(
                                ctx,
                                &cmp_fn,
                                &Value::Undefined,
                                &[Value::BigInt(Box::new(a.clone())), Value::BigInt(Box::new(b.clone()))],
                            );
                            match result {
                                Ok(v) => {
                                    let n = match self.extract_number_with_coercion(ctx, &v) {
                                        Some(n) => n,
                                        None => {
                                            had_error = true;
                                            return std::cmp::Ordering::Equal;
                                        }
                                    };
                                    if n.is_nan() {
                                        std::cmp::Ordering::Equal
                                    } else if n < 0.0 {
                                        std::cmp::Ordering::Less
                                    } else if n > 0.0 {
                                        std::cmp::Ordering::Greater
                                    } else {
                                        std::cmp::Ordering::Equal
                                    }
                                }
                                Err(e) => {
                                    had_error = true;
                                    self.set_pending_throw_from_error(&e);
                                    std::cmp::Ordering::Equal
                                }
                            }
                        });
                    } else {
                        bi_elements.sort();
                    }
                    let mut a = res_arr.borrow_mut(ctx);
                    for (i, bi) in bi_elements.into_iter().enumerate() {
                        a.elements[i] = Value::BigInt(Box::new(coerce_bigint_for_ta(&bi, &res_ta_name)));
                    }
                    drop(a);
                } else {
                    let mut f64_elements: Vec<f64> = {
                        let a = res_arr.borrow();
                        a.elements.iter().map(|v| to_number(v)).collect()
                    };
                    if has_custom {
                        let cmp_fn = comparefn.unwrap();
                        let mut had_error = false;
                        f64_elements.sort_by(|a, b| {
                            if had_error {
                                return std::cmp::Ordering::Equal;
                            }
                            let result =
                                self.vm_call_function_value(ctx, &cmp_fn, &Value::Undefined, &[Value::Number(*a), Value::Number(*b)]);
                            match result {
                                Ok(v) => {
                                    let n = match self.extract_number_with_coercion(ctx, &v) {
                                        Some(n) => n,
                                        None => {
                                            had_error = true;
                                            return std::cmp::Ordering::Equal;
                                        }
                                    };
                                    if n.is_nan() {
                                        std::cmp::Ordering::Equal
                                    } else if n < 0.0 {
                                        std::cmp::Ordering::Less
                                    } else if n > 0.0 {
                                        std::cmp::Ordering::Greater
                                    } else {
                                        std::cmp::Ordering::Equal
                                    }
                                }
                                Err(e) => {
                                    had_error = true;
                                    self.set_pending_throw_from_error(&e);
                                    std::cmp::Ordering::Equal
                                }
                            }
                        });
                    } else {
                        // Default numeric sort for TypedArrays
                        f64_elements.sort_by(|a, b| {
                            if a.is_nan() && b.is_nan() {
                                return std::cmp::Ordering::Equal;
                            }
                            if a.is_nan() {
                                return std::cmp::Ordering::Greater;
                            }
                            if b.is_nan() {
                                return std::cmp::Ordering::Less;
                            }
                            if *a == 0.0 && *b == 0.0 {
                                if a.is_sign_negative() && !b.is_sign_negative() {
                                    return std::cmp::Ordering::Less;
                                }
                                if !a.is_sign_negative() && b.is_sign_negative() {
                                    return std::cmp::Ordering::Greater;
                                }
                                return std::cmp::Ordering::Equal;
                            }
                            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }
                    let mut a = res_arr.borrow_mut(ctx);
                    for (i, val) in f64_elements.into_iter().enumerate() {
                        a.elements[i] = Value::Number(Self::typed_array_coerce_value(val, &res_ta_name));
                    }
                    drop(a);
                }

                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }

                // Sync all result elements to buffer
                for i in 0..len {
                    let num = {
                        let a = res_arr.borrow();
                        to_number(&a.elements[i])
                    };
                    self.sync_ta_element_to_buffer(ctx, res_arr, i, num, &res_ta_name);
                }
                result
            }
            "typedarray.with" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "with") {
                    return Value::Undefined;
                }
                let Value::Array(arr) = &this_val else {
                    return Value::Undefined;
                };
                self.sync_resizable_ta_elements(ctx, arr);
                let original_len = arr.borrow().elements.len();

                // ToIntegerOrInfinity(index)
                let index_arg = args.first().cloned().unwrap_or(Value::Undefined);
                let relative_index = match self.extract_number_with_coercion(ctx, &index_arg) {
                    Some(n) if n.is_nan() => 0i64,
                    Some(n) => n as i64,
                    None => return Value::Undefined,
                };
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let actual_index = if relative_index < 0 {
                    original_len as i64 + relative_index
                } else {
                    relative_index
                };

                // Coerce value (may trigger resize)
                let value_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
                let ta_name = {
                    match arr.borrow().props.get("__typedarray_name__") {
                        Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                        _ => "Uint8Array".to_string(),
                    }
                };
                let is_bigint = is_bigint_typed_array(&ta_name);
                let numeric_value = if is_bigint {
                    let bi = match self.value_to_bigint(ctx, &value_arg) {
                        Some(b) => b,
                        None => return Value::Undefined,
                    };
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    Value::BigInt(Box::new(coerce_bigint_for_ta(&bi, &ta_name)))
                } else {
                    let num = match self.extract_number_with_coercion(ctx, &value_arg) {
                        Some(n) => n,
                        None => return Value::Undefined,
                    };
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    Value::Number(Self::typed_array_coerce_value(num, &ta_name))
                };

                // Re-validate after coercion (buffer may have been detached/resized)
                if !self.validate_typed_array(ctx, &this_val, "with") {
                    return Value::Undefined;
                }
                // Re-sync and get current length for index validation
                self.sync_resizable_ta_elements(ctx, arr);
                let current_len = arr.borrow().elements.len();

                // Validate index against current length
                if actual_index < 0 || actual_index as usize >= current_len {
                    self.throw_range_error_object(ctx, "Invalid index");
                    return Value::Undefined;
                }

                // TypedArrayCreateSameType with ORIGINAL length (ignores @@species)
                let result = match self.typed_array_create_same_type(ctx, &this_val, &[Value::Number(original_len as f64)]) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let Value::Array(res_arr) = &result else {
                    return result;
                };

                // Copy elements from source, replacing at actual_index
                let res_ta_name = match res_arr.borrow().props.get("__typedarray_name__") {
                    Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                    _ => ta_name.clone(),
                };
                for k in 0..original_len {
                    let from_value = if k == actual_index as usize {
                        numeric_value.clone()
                    } else {
                        self.ta_get_element(ctx, arr, k)
                    };
                    if k < res_arr.borrow().elements.len() {
                        if is_bigint_typed_array(&res_ta_name) {
                            let bi = match &from_value {
                                Value::BigInt(b) => (**b).clone(),
                                _ => num_bigint::BigInt::from(0),
                            };
                            let coerced = coerce_bigint_for_ta(&bi, &res_ta_name);
                            res_arr.borrow_mut(ctx).elements[k] = Value::BigInt(Box::new(coerced));
                            self.sync_ta_element_to_buffer(ctx, res_arr, k, 0.0, &res_ta_name);
                        } else {
                            let num = to_number(&from_value);
                            let coerced = Self::typed_array_coerce_value(num, &res_ta_name);
                            res_arr.borrow_mut(ctx).elements[k] = Value::Number(coerced);
                            self.sync_ta_element_to_buffer(ctx, res_arr, k, num, &res_ta_name);
                        }
                    }
                }
                result
            }
            // ── Uint8Array-specific: base64/hex methods ──
            "typedarray.toBase64" => self.uint8array_to_base64(ctx, receiver, args),
            "typedarray.toHex" => self.uint8array_to_hex(ctx, receiver),
            "typedarray.setFromBase64" => self.uint8array_set_from_base64(ctx, receiver, args),
            "typedarray.setFromHex" => self.uint8array_set_from_hex(ctx, receiver, args),
            "typedarray.fromBase64" => self.uint8array_from_base64(ctx, args),
            "typedarray.fromHex" => self.uint8array_from_hex(ctx, args),
            _ => Value::Undefined,
        }
    }

    /// Validate that a value is a TypedArray with a non-detached buffer.
    /// Returns true if valid. Sets pending_throw and returns false otherwise.
    fn validate_typed_array(&mut self, ctx: &GcContext<'gc>, val: &Value<'gc>, method: &str) -> bool {
        match val {
            Value::Array(arr) => {
                let a = arr.borrow();
                if !a.props.contains_key("__typedarray_name__") {
                    drop(a);
                    self.throw_type_error(ctx, &format!("%TypedArray%.prototype.{} called on incompatible receiver", method));
                    return false;
                }
                // Check for detached buffer
                if let Some(Value::Object(buf)) = a.props.get("__typedarray_buffer__")
                    && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
                {
                    drop(a);
                    self.throw_type_error(ctx, "Cannot perform operation on a detached ArrayBuffer");
                    return false;
                }
                // IsTypedArrayOutOfBounds — resizable buffer
                if let Some(Value::Object(buf)) = a.props.get("__typedarray_buffer__")
                    && matches!(buf.borrow().get("__resizable__"), Some(Value::Boolean(true)))
                {
                    let bpe = match a.props.get("__bytes_per_element__") {
                        Some(Value::Number(n)) => *n as usize,
                        _ => 1,
                    };
                    let byte_offset = match a.props.get("__byte_offset__") {
                        Some(Value::Number(n)) => *n as usize,
                        _ => 0,
                    };
                    let buf_byte_len = match buf.borrow().get("byteLength") {
                        Some(Value::Number(n)) => *n as usize,
                        _ => 0,
                    };
                    let is_auto = matches!(a.props.get("__length_tracking__"), Some(Value::Boolean(true)));
                    let out_of_bounds = if is_auto {
                        byte_offset > buf_byte_len
                    } else {
                        let fixed_len = match a.props.get("__fixed_length__") {
                            Some(Value::Number(n)) => *n as usize,
                            _ => 0,
                        };
                        byte_offset + fixed_len * bpe > buf_byte_len
                    };
                    if out_of_bounds {
                        drop(a);
                        self.throw_type_error(ctx, "Cannot perform operation on an out-of-bounds TypedArray");
                        return false;
                    }
                }
                true
            }
            _ => {
                self.throw_type_error(ctx, &format!("%TypedArray%.prototype.{} called on incompatible receiver", method));
                false
            }
        }
    }

    /// Implements SpeciesConstructor(O, defaultConstructor) for TypedArrays.
    /// Returns the species constructor, or None if a throw was set.
    fn typed_array_species_constructor(&mut self, ctx: &GcContext<'gc>, this_val: &Value<'gc>) -> Option<Value<'gc>> {
        let ta_name = if let Value::Array(arr) = this_val {
            arr.borrow()
                .props
                .get("__typedarray_name__")
                .map(value_to_string)
                .unwrap_or_default()
        } else {
            return None;
        };

        // Default constructor from globals
        let default_ctor = self.globals.get(&ta_name).cloned().unwrap_or(Value::Undefined);

        // Step 2: Let C be ? Get(O, "constructor")
        let ctor = self.read_named_property(ctx, this_val, "constructor");
        if self.pending_throw.is_some() {
            return None;
        }

        // Step 3: If C is undefined, return defaultConstructor
        if matches!(ctor, Value::Undefined) {
            return Some(default_ctor);
        }

        // Step 4: If Type(C) is not Object, throw TypeError
        if !matches!(
            ctor,
            Value::Object(_) | Value::Array(_) | Value::Function(..) | Value::Closure(..) | Value::NativeFunction(_)
        ) || ctor.is_symbol_value()
        {
            self.throw_type_error(ctx, "Constructor is not an object");
            return None;
        }

        // Step 5: Let S be ? Get(C, @@species)
        let mut species_ctor = ctor.clone();
        if let Some(Value::Object(symbol_ctor)) = self.globals.get("Symbol")
            && let Some(species_symbol) = own_data_from_legacy_map(&symbol_ctor.borrow(), "species")
            && let Some(species_key) = self.symbol_key_string(&species_symbol)
        {
            let species = self.read_named_property(ctx, &ctor, &species_key);
            if self.pending_throw.is_some() {
                return None;
            }
            // Step 6: If S is undefined or null, return defaultConstructor
            if matches!(species, Value::Undefined | Value::Null) {
                return Some(default_ctor);
            }
            species_ctor = species;
        }

        // Step 7: If IsConstructor(S), return S
        if self.is_constructor_value(&species_ctor) {
            return Some(species_ctor);
        }

        // Step 8: Throw TypeError
        self.throw_type_error(ctx, "Species constructor is not a constructor");
        None
    }

    /// TypedArrayCreateSameType: creates a new TypedArray of the same type
    /// using the intrinsic constructor (ignores @@species).
    fn typed_array_create_same_type(&mut self, ctx: &GcContext<'gc>, exemplar: &Value<'gc>, args: &[Value<'gc>]) -> Option<Value<'gc>> {
        let ta_name = match exemplar {
            Value::Array(arr) => match arr.borrow().props.get("__typedarray_name__") {
                Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
                _ => return None,
            },
            _ => return None,
        };
        // Look up the intrinsic constructor from globals
        let ctor = self.globals.get(&ta_name).cloned()?;
        let result = match self.construct_value(ctx, &ctor, args, None) {
            Ok(v) => v,
            Err(e) => {
                self.set_pending_throw_from_error(&e);
                return None;
            }
        };
        if !self.validate_typed_array(ctx, &result, &ta_name) {
            return None;
        }
        Some(result)
    }

    /// TypedArraySpeciesCreate: use species constructor to create result,
    /// falling back to wrap_as_typed_array for default constructors.
    fn typed_array_species_create(&mut self, ctx: &GcContext<'gc>, this_val: &Value<'gc>, args: &[Value<'gc>]) -> Option<Value<'gc>> {
        let ctor = self.typed_array_species_constructor(ctx, this_val)?;

        let result = match self.construct_value(ctx, &ctor, args, None) {
            Ok(v) => v,
            Err(e) => {
                self.set_pending_throw_from_error(&e);
                return None;
            }
        };

        if !self.validate_typed_array(ctx, &result, "species constructor") {
            return None;
        }

        if args.len() == 1
            && let Some(Value::Number(requested_len)) = args.first()
            && requested_len.is_finite()
            && *requested_len >= 0.0
        {
            // Sync elements for resizable buffer TAs to get current length
            if let Value::Array(arr) = &result {
                self.maybe_sync_resizable_ta(ctx, arr);
            }
            let actual_len = match &result {
                Value::Array(arr) => {
                    let a = arr.borrow();
                    // Use dynamic TypedArrayLength for resizable buffers
                    if matches!(a.props.get("__length_tracking__"), Some(Value::Boolean(true))) {
                        if let Some(Value::Object(buf)) = a.props.get("__typedarray_buffer__") {
                            let buf_byte_len = match buf.borrow().get("byteLength") {
                                Some(Value::Number(n)) => *n as usize,
                                _ => 0,
                            };
                            let byte_offset = match a.props.get("__byte_offset__") {
                                Some(Value::Number(n)) => *n as usize,
                                _ => 0,
                            };
                            let bpe = match a.props.get("__bytes_per_element__") {
                                Some(Value::Number(n)) => *n as usize,
                                _ => 1,
                            };
                            if buf_byte_len >= byte_offset {
                                (buf_byte_len - byte_offset) / bpe
                            } else {
                                0
                            }
                        } else {
                            a.elements.len()
                        }
                    } else {
                        a.elements.len()
                    }
                }
                _ => 0,
            };
            if actual_len < requested_len.trunc() as usize {
                self.throw_type_error(ctx, "Derived TypedArray constructor created an array which was too small");
                return None;
            }
        }

        Some(result)
    }

    /// Wrap a plain Array result as the same TypedArray type as `source`.
    fn _wrap_as_typed_array(&mut self, ctx: &GcContext<'gc>, source: &Value<'gc>, result: &Value<'gc>) -> Value<'gc> {
        let (ta_name, bpe) = if let Value::Array(arr) = source {
            let a = arr.borrow();
            let name = a.props.get("__typedarray_name__").map(value_to_string).unwrap_or_default();
            let bpe = match a.props.get("__bytes_per_element__") {
                Some(Value::Number(n)) => *n as usize,
                _ => 1,
            };
            (name, bpe)
        } else {
            return result.clone();
        };
        // Get the constructor from globals
        let _ctor_id = match ta_name.as_str() {
            "Int8Array" => BUILTIN_CTOR_INT8ARRAY,
            "Uint8Array" => BUILTIN_CTOR_UINT8ARRAY,
            "Uint8ClampedArray" => BUILTIN_CTOR_UINT8CLAMPEDARRAY,
            "Int16Array" => BUILTIN_CTOR_INT16ARRAY,
            "Uint16Array" => BUILTIN_CTOR_UINT16ARRAY,
            "Int32Array" => BUILTIN_CTOR_INT32ARRAY,
            "Uint32Array" => BUILTIN_CTOR_UINT32ARRAY,
            "Float32Array" => BUILTIN_CTOR_FLOAT32ARRAY,
            "Float64Array" => BUILTIN_CTOR_FLOAT64ARRAY,
            "Float16Array" => BUILTIN_CTOR_FLOAT16ARRAY,
            _ => return result.clone(),
        };
        // Extract elements from result
        let elements = match result {
            Value::Array(arr) => arr.borrow().elements.clone(),
            _ => return result.clone(),
        };
        let len = elements.len();
        // Create a new TypedArray with the same type
        let ta_instance_proto: Option<Value<'gc>> = self.globals.get(&ta_name).and_then(|v| {
            if let Value::Object(o) = v {
                own_data_from_legacy_map(&o.borrow(), "prototype")
            } else {
                None
            }
        });
        let mut data = VmArrayData::new(elements.clone());
        data.props.insert("__typedarray_name__".to_string(), Value::from(&ta_name));
        data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
        // Create backing ArrayBuffer
        let mut bytes = vec![Value::Number(0.0); len * bpe];
        for i in 0..len {
            let num = to_number(elements.get(i).unwrap_or(&Value::Number(0.0)));
            Self::encode_typed_element(&mut bytes, i * bpe, bpe, &ta_name, num);
        }
        let buffer_obj = self.create_ordinary_array_buffer(ctx, bytes);
        data.props.insert("buffer".to_string(), buffer_obj.clone());
        mark_nonenumerable(&mut data.props, "buffer");
        data.props.insert("__typedarray_buffer__".to_string(), buffer_obj);
        data.props.insert("__byte_offset__".to_string(), Value::Number(0.0));
        data.props.insert("__bytes_per_element__".to_string(), Value::Number(bpe as f64));
        if let Some(proto) = &ta_instance_proto {
            data.props.insert("__proto__".to_string(), proto.clone());
        }
        Value::Array(new_gc_cell_ptr(ctx, data))
    }

    // ECMAScript ToUint8 (wrapping)
    pub(super) fn to_uint8(num: f64) -> u8 {
        if num.is_nan() || num == 0.0 || !num.is_finite() {
            return 0;
        }
        let int_val = num.trunc() as i64;
        int_val.rem_euclid(256) as u8
    }

    // ECMAScript ToUint8Clamp (round-half-to-even)
    pub(super) fn to_uint8_clamp(num: f64) -> u8 {
        if num.is_nan() || num <= 0.0 {
            return 0;
        }
        if num >= 255.0 {
            return 255;
        }
        let f = num.floor();
        if f + 0.5 < num {
            return (f + 1.0) as u8;
        }
        if num < f + 0.5 {
            return f as u8;
        }
        // Exactly half: round to even
        let fi = f as u8;
        if fi.is_multiple_of(2) { fi } else { fi + 1 }
    }

    // ECMAScript ToInt8 (wrapping)
    pub(super) fn to_int8(num: f64) -> i8 {
        if num.is_nan() || num == 0.0 || !num.is_finite() {
            return 0;
        }
        let int_val = num.trunc() as i64;
        let m = int_val.rem_euclid(256);
        if m >= 128 { (m - 256) as i8 } else { m as i8 }
    }

    // ECMAScript ToUint16 (wrapping)
    pub(super) fn to_uint16(num: f64) -> u16 {
        if num.is_nan() || num == 0.0 || !num.is_finite() {
            return 0;
        }
        let int_val = num.trunc() as i64;
        int_val.rem_euclid(65536) as u16
    }

    // ECMAScript ToInt16 (wrapping)
    pub(super) fn to_int16(num: f64) -> i16 {
        if num.is_nan() || num == 0.0 || !num.is_finite() {
            return 0;
        }
        let int_val = num.trunc() as i64;
        let m = int_val.rem_euclid(65536);
        if m >= 32768 { (m - 65536) as i16 } else { m as i16 }
    }

    // ECMAScript ToUint32 (wrapping)
    pub(super) fn to_uint32_typed(num: f64) -> u32 {
        if num.is_nan() || num == 0.0 || !num.is_finite() {
            return 0;
        }
        let int_val = num.trunc() as i128;
        int_val.rem_euclid(4294967296) as u32
    }

    // ECMAScript ToInt32 (wrapping)
    pub(super) fn to_int32_typed(num: f64) -> i32 {
        if num.is_nan() || num == 0.0 || !num.is_finite() {
            return 0;
        }
        let int_val = num.trunc() as i128;
        let m = int_val.rem_euclid(4294967296);
        if m >= 2147483648 { (m - 4294967296) as i32 } else { m as i32 }
    }

    /// Encode a numeric value into typed element buffer bytes at a given base offset.
    pub(super) fn encode_typed_element(bytes: &mut [Value<'gc>], base: usize, _bpe: usize, ta_name: &str, num: f64) {
        match ta_name {
            "Uint8Array" | "Uint8ClampedArray" => {
                if base < bytes.len() {
                    let v = if ta_name == "Uint8ClampedArray" {
                        Self::to_uint8_clamp(num)
                    } else {
                        Self::to_uint8(num)
                    };
                    bytes[base] = Value::Number(v as f64);
                }
            }
            "Int8Array" => {
                if base < bytes.len() {
                    let v = Self::to_int8(num);
                    bytes[base] = Value::Number((v as u8) as f64);
                }
            }
            "Uint16Array" | "Int16Array" => {
                let b = if ta_name == "Int16Array" {
                    Self::to_int16(num).to_ne_bytes()
                } else {
                    Self::to_uint16(num).to_ne_bytes()
                };
                for (j, &byte) in b.iter().enumerate() {
                    if base + j < bytes.len() {
                        bytes[base + j] = Value::Number(byte as f64);
                    }
                }
            }
            "Uint32Array" | "Int32Array" => {
                let b = if ta_name == "Int32Array" {
                    Self::to_int32_typed(num).to_ne_bytes()
                } else {
                    Self::to_uint32_typed(num).to_ne_bytes()
                };
                for (j, &byte) in b.iter().enumerate() {
                    if base + j < bytes.len() {
                        bytes[base + j] = Value::Number(byte as f64);
                    }
                }
            }
            "Float16Array" => {
                let b = f64_to_f16_bits(num).to_ne_bytes();
                for (j, &byte) in b.iter().enumerate() {
                    if base + j < bytes.len() {
                        bytes[base + j] = Value::Number(byte as f64);
                    }
                }
            }
            "Float32Array" => {
                let b = (num as f32).to_ne_bytes();
                for (j, &byte) in b.iter().enumerate() {
                    if base + j < bytes.len() {
                        bytes[base + j] = Value::Number(byte as f64);
                    }
                }
            }
            "Float64Array" => {
                let b = num.to_ne_bytes();
                for (j, &byte) in b.iter().enumerate() {
                    if base + j < bytes.len() {
                        bytes[base + j] = Value::Number(byte as f64);
                    }
                }
            }
            _ => {
                if base < bytes.len() {
                    bytes[base] = Value::Number(Self::to_uint8(num) as f64);
                }
            }
        }
    }

    /// Encode a BigInt value into buffer bytes for BigInt64Array/BigUint64Array.
    pub(super) fn encode_typed_element_bigint(bytes: &mut [Value<'gc>], base: usize, ta_name: &str, bigint_val: &num_bigint::BigInt) {
        let raw_bytes = match ta_name {
            "BigInt64Array" => bigint_to_i64(bigint_val).to_ne_bytes(),
            "BigUint64Array" => bigint_to_u64(bigint_val).to_ne_bytes(),
            _ => return,
        };
        for (j, &b) in raw_bytes.iter().enumerate() {
            if base + j < bytes.len() {
                bytes[base + j] = Value::Number(b as f64);
            }
        }
    }

    /// Decode a typed element from buffer bytes at a given base offset.
    pub(super) fn decode_typed_element(bytes: &[Value<'gc>], base: usize, _bpe: usize, ta_name: &str) -> Value<'gc> {
        match ta_name {
            "Uint8Array" | "Uint8ClampedArray" => {
                let b = to_number(bytes.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                Value::Number(b as f64)
            }
            "Int8Array" => {
                let b = to_number(bytes.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                Value::Number((b as i8) as f64)
            }
            "Uint16Array" => {
                let b0 = to_number(bytes.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                let b1 = to_number(bytes.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8;
                Value::Number(u16::from_ne_bytes([b0, b1]) as f64)
            }
            "Int16Array" => {
                let b0 = to_number(bytes.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                let b1 = to_number(bytes.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8;
                Value::Number(i16::from_ne_bytes([b0, b1]) as f64)
            }
            "Uint32Array" => {
                let arr4: [u8; 4] = core::array::from_fn(|j| to_number(bytes.get(base + j).unwrap_or(&Value::Number(0.0))) as u8);
                Value::Number(u32::from_ne_bytes(arr4) as f64)
            }
            "Int32Array" => {
                let arr4: [u8; 4] = core::array::from_fn(|j| to_number(bytes.get(base + j).unwrap_or(&Value::Number(0.0))) as u8);
                Value::Number(i32::from_ne_bytes(arr4) as f64)
            }
            "Float16Array" => {
                let b0 = to_number(bytes.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                let b1 = to_number(bytes.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8;
                Value::Number(f16_bits_to_f64(u16::from_ne_bytes([b0, b1])))
            }
            "Float32Array" => {
                let arr4: [u8; 4] = core::array::from_fn(|j| to_number(bytes.get(base + j).unwrap_or(&Value::Number(0.0))) as u8);
                Value::Number(f32::from_ne_bytes(arr4) as f64)
            }
            "Float64Array" => {
                let arr8: [u8; 8] = core::array::from_fn(|j| to_number(bytes.get(base + j).unwrap_or(&Value::Number(0.0))) as u8);
                Value::Number(f64::from_ne_bytes(arr8))
            }
            "BigInt64Array" => {
                let arr8: [u8; 8] = core::array::from_fn(|j| to_number(bytes.get(base + j).unwrap_or(&Value::Number(0.0))) as u8);
                let val = i64::from_ne_bytes(arr8);
                Value::BigInt(Box::new(num_bigint::BigInt::from(val)))
            }
            "BigUint64Array" => {
                let arr8: [u8; 8] = core::array::from_fn(|j| to_number(bytes.get(base + j).unwrap_or(&Value::Number(0.0))) as u8);
                let val = u64::from_ne_bytes(arr8);
                Value::BigInt(Box::new(num_bigint::BigInt::from(val)))
            }
            _ => {
                let b = to_number(bytes.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                Value::Number(b as f64)
            }
        }
    }

    /// Sync a TypedArray element write to the underlying buffer bytes.
    pub(super) fn sync_ta_element_to_buffer(&self, ctx: &GcContext<'gc>, arr: &ArrayHandle<'gc>, idx: usize, new_num: f64, ta_name: &str) {
        let (buffer, byte_offset, bpe) = {
            let a = arr.borrow();
            let buffer = a.props.get("__typedarray_buffer__").cloned();
            let byte_offset = a
                .props
                .get("__byte_offset__")
                .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None })
                .unwrap_or(0);
            let bpe = a
                .props
                .get("__bytes_per_element__")
                .and_then(|v| {
                    if let Value::Number(n) = v {
                        Some((*n as usize).max(1))
                    } else {
                        None
                    }
                })
                .unwrap_or(1);
            (buffer, byte_offset, bpe)
        };
        if let Some(Value::Object(buf_obj)) = buffer
            && let Some(Value::Array(buf_bytes)) = buf_obj.borrow().get("__buffer_bytes__").cloned()
        {
            let base = byte_offset + idx * bpe;
            let mut bb = buf_bytes.borrow_mut(ctx);
            match ta_name {
                "Uint8Array" | "Uint8ClampedArray" if base < bb.elements.len() => {
                    let v = if ta_name == "Uint8ClampedArray" {
                        Self::to_uint8_clamp(new_num)
                    } else {
                        Self::to_uint8(new_num)
                    };
                    bb.elements[base] = Value::Number(v as f64);
                }
                "Int8Array" if base < bb.elements.len() => {
                    bb.elements[base] = Value::Number((Self::to_int8(new_num) as u8) as f64);
                }
                "Uint16Array" | "Int16Array" => {
                    let bytes = if ta_name == "Int16Array" {
                        Self::to_int16(new_num).to_ne_bytes()
                    } else {
                        Self::to_uint16(new_num).to_ne_bytes()
                    };
                    for (j, b) in bytes.iter().enumerate() {
                        if base + j < bb.elements.len() {
                            bb.elements[base + j] = Value::Number(*b as f64);
                        }
                    }
                }
                "Uint32Array" | "Int32Array" => {
                    let bytes = if ta_name == "Int32Array" {
                        Self::to_int32_typed(new_num).to_ne_bytes()
                    } else {
                        Self::to_uint32_typed(new_num).to_ne_bytes()
                    };
                    for (j, b) in bytes.iter().enumerate() {
                        if base + j < bb.elements.len() {
                            bb.elements[base + j] = Value::Number(*b as f64);
                        }
                    }
                }
                "Float16Array" => {
                    let bytes = f64_to_f16_bits(new_num).to_ne_bytes();
                    for (j, b) in bytes.iter().enumerate() {
                        if base + j < bb.elements.len() {
                            bb.elements[base + j] = Value::Number(*b as f64);
                        }
                    }
                }
                "Float32Array" => {
                    let bytes = (new_num as f32).to_ne_bytes();
                    for (j, b) in bytes.iter().enumerate() {
                        if base + j < bb.elements.len() {
                            bb.elements[base + j] = Value::Number(*b as f64);
                        }
                    }
                }
                "Float64Array" => {
                    let bytes = new_num.to_ne_bytes();
                    for (j, b) in bytes.iter().enumerate() {
                        if base + j < bb.elements.len() {
                            bb.elements[base + j] = Value::Number(*b as f64);
                        }
                    }
                }
                "BigInt64Array" | "BigUint64Array" => {
                    let bigint_val = arr.borrow().elements.get(idx).cloned();
                    if let Some(Value::BigInt(bi)) = bigint_val {
                        let raw_bytes = if ta_name == "BigInt64Array" {
                            bigint_to_i64(&bi).to_ne_bytes()
                        } else {
                            bigint_to_u64(&bi).to_ne_bytes()
                        };
                        for (j, &b) in raw_bytes.iter().enumerate() {
                            if base + j < bb.elements.len() {
                                bb.elements[base + j] = Value::Number(b as f64);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Sync elements array from the backing buffer bytes (for shared buffer scenarios)
    fn sync_ta_elements_from_buffer(&self, ctx: &GcContext<'gc>, arr: &ArrayHandle<'gc>, ta_name: &str, bpe: usize, len: usize) {
        let (buffer, byte_offset) = {
            let a = arr.borrow();
            let buffer = a.props.get("__typedarray_buffer__").cloned();
            let byte_offset = a
                .props
                .get("__byte_offset__")
                .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None })
                .unwrap_or(0);
            (buffer, byte_offset)
        };
        // Extract buf_bytes in its own scope so buf_obj borrow is dropped
        let buf_bytes = if let Some(Value::Object(buf_obj)) = &buffer {
            buf_obj.borrow().get("__buffer_bytes__").cloned()
        } else {
            None
        };
        if let Some(Value::Array(buf_bytes)) = buf_bytes {
            let bb = buf_bytes.borrow();
            let mut a = arr.borrow_mut(ctx);
            for i in 0..len {
                let base = byte_offset + i * bpe;
                if base + bpe <= bb.elements.len() {
                    let val = Self::decode_typed_element(&bb.elements, base, bpe, ta_name);
                    a.elements[i] = val;
                }
            }
        }
    }

    fn create_ordinary_array_buffer(&mut self, ctx: &GcContext<'gc>, bytes: Vec<Value<'gc>>) -> Value<'gc> {
        let mut buf_map = IndexMap::new();
        buf_map.insert("__type__".to_string(), Value::from("ArrayBuffer"));
        buf_map.insert("byteLength".to_string(), Value::Number(bytes.len() as f64));
        buf_map.insert(
            "__buffer_bytes__".to_string(),
            Value::Array(new_gc_cell_ptr(ctx, VmArrayData::new(bytes))),
        );
        if let Some(proto) = self.ctor_prototype_from_globals(ctx, "ArrayBuffer") {
            buf_map.insert("__proto__".to_string(), proto);
        }
        Value::Object(new_gc_cell_ptr(ctx, buf_map))
    }

    fn create_immutable_array_buffer(&mut self, ctx: &GcContext<'gc>, bytes: Vec<Value<'gc>>) -> Value<'gc> {
        let mut buf_map = IndexMap::new();
        buf_map.insert("__type__".to_string(), Value::from("ArrayBuffer"));
        buf_map.insert("byteLength".to_string(), Value::Number(bytes.len() as f64));
        buf_map.insert(
            "__buffer_bytes__".to_string(),
            Value::Array(new_gc_cell_ptr(ctx, VmArrayData::new(bytes))),
        );
        buf_map.insert("__immutable__".to_string(), Value::Boolean(true));
        if let Some(proto) = self.ctor_prototype_from_globals(ctx, "ArrayBuffer") {
            buf_map.insert("__proto__".to_string(), proto);
        }
        Value::Object(new_gc_cell_ptr(ctx, buf_map))
    }

    /// If `arr` is a TypedArray backed by a resizable buffer, sync its
    /// elements vector so that `elements.len()` reflects the current
    /// dynamic length after any buffer resize.  No-op for non-resizable TAs.
    pub(super) fn maybe_sync_resizable_ta(&self, ctx: &GcContext<'gc>, arr: &ArrayHandle<'gc>) {
        let needs_sync = {
            let b = arr.borrow();
            b.props.contains_key("__typedarray_name__")
                && matches!(
                    b.props.get("__typedarray_buffer__"),
                    Some(Value::Object(buf)) if matches!(buf.borrow().get("__resizable__"), Some(Value::Boolean(true)))
                )
        };
        if needs_sync {
            self.sync_resizable_ta_elements(ctx, arr);
        }
    }

    /// Check if a Array is a TypedArray backed by a resizable buffer.
    pub(super) fn is_ta_resizable(arr: &ArrayHandle<'gc>) -> bool {
        let b = arr.borrow();
        b.props.contains_key("__typedarray_name__")
            && matches!(
                b.props.get("__typedarray_buffer__"),
                Some(Value::Object(buf)) if matches!(buf.borrow().get("__resizable__"), Some(Value::Boolean(true)))
            )
    }

    /// Returns true if a resizable-backed TypedArray is out of bounds
    /// (buffer shrank below the TA's view). Non-resizable TAs always return false.
    pub(super) fn is_typed_array_oob(&self, arr: &ArrayHandle<'gc>) -> bool {
        let b = arr.borrow();
        if !b.props.contains_key("__typedarray_name__") {
            return false;
        }
        let buf = match b.props.get("__typedarray_buffer__") {
            Some(Value::Object(o)) => *o,
            _ => return false,
        };
        if !matches!(buf.borrow().get("__resizable__"), Some(Value::Boolean(true))) {
            return false;
        }
        let bpe = match b.props.get("__bytes_per_element__") {
            Some(Value::Number(n)) => *n as usize,
            _ => 1,
        };
        let byte_offset = match b.props.get("__byte_offset__") {
            Some(Value::Number(n)) => *n as usize,
            _ => 0,
        };
        let buf_byte_len = match buf.borrow().get("byteLength") {
            Some(Value::Number(n)) => *n as usize,
            _ => 0,
        };
        let is_auto = matches!(b.props.get("__length_tracking__"), Some(Value::Boolean(true)));
        if is_auto {
            byte_offset > buf_byte_len
        } else {
            let fixed_len = match b.props.get("__fixed_length__") {
                Some(Value::Number(n)) => *n as usize,
                _ => 0,
            };
            byte_offset + fixed_len * bpe > buf_byte_len
        }
    }

    /// Read element `index` from a TypedArray, syncing from buffer first if resizable.
    /// Returns Undefined when out of bounds (e.g. buffer was shrunk).
    pub(super) fn ta_get_element(&self, ctx: &GcContext<'gc>, arr: &ArrayHandle<'gc>, index: usize) -> Value<'gc> {
        self.maybe_sync_resizable_ta(ctx, arr);
        let borrow = arr.borrow();
        if index < borrow.elements.len() {
            borrow.elements[index].clone()
        } else {
            Value::Undefined
        }
    }

    /// For resizable-buffer-backed TypedArrays, sync the elements vector
    /// to reflect the current buffer state (dynamic length after resize).
    /// Returns the new length, or None if not a resizable TA (caller uses elements.len()).
    pub(super) fn sync_resizable_ta_elements(&self, ctx: &GcContext<'gc>, arr: &ArrayHandle<'gc>) -> Option<usize> {
        let a = arr.borrow();
        let buf = match a.props.get("__typedarray_buffer__") {
            Some(Value::Object(b)) => *b,
            _ => return None,
        };
        if !matches!(buf.borrow().get("__resizable__"), Some(Value::Boolean(true))) {
            return None;
        }
        let ta_name = match a.props.get("__typedarray_name__") {
            Some(Value::String(s)) => crate::unicode::utf16_to_utf8(s),
            _ => return None,
        };
        let bpe = match a.props.get("__bytes_per_element__") {
            Some(Value::Number(n)) => (*n as usize).max(1),
            _ => 1,
        };
        let byte_offset = match a.props.get("__byte_offset__") {
            Some(Value::Number(n)) => *n as usize,
            _ => 0,
        };
        let buf_byte_len = match buf.borrow().get("byteLength") {
            Some(Value::Number(n)) => *n as usize,
            _ => 0,
        };
        let is_auto = matches!(a.props.get("__length_tracking__"), Some(Value::Boolean(true)));
        let is_oob;
        let new_len = if is_auto {
            if byte_offset > buf_byte_len {
                is_oob = true;
                0
            } else {
                is_oob = false;
                (buf_byte_len - byte_offset) / bpe
            }
        } else {
            let fixed_len = match a.props.get("__fixed_length__") {
                Some(Value::Number(n)) => *n as usize,
                _ => a.elements.len(),
            };
            if byte_offset + fixed_len * bpe > buf_byte_len {
                is_oob = true;
                0
            } else {
                is_oob = false;
                fixed_len
            }
        };
        let buf_bytes_val = buf.borrow().get("__buffer_bytes__").cloned();
        drop(a);
        if let Some(Value::Array(buf_bytes)) = buf_bytes_val {
            let bb = buf_bytes.borrow();
            let mut a = arr.borrow_mut(ctx);
            a.elements.resize(new_len, Value::Number(0.0));
            for i in 0..new_len {
                let base = byte_offset + i * bpe;
                if base + bpe <= bb.elements.len() {
                    a.elements[i] = Self::decode_typed_element(&bb.elements, base, bpe, &ta_name);
                }
            }
            if is_oob {
                a.props.insert("__out_of_bounds__".to_string(), Value::Boolean(true));
            } else {
                a.props.shift_remove("__out_of_bounds__");
            }
        } else {
            let mut a = arr.borrow_mut(ctx);
            a.elements.resize(new_len, Value::Number(0.0));
            if is_oob {
                a.props.insert("__out_of_bounds__".to_string(), Value::Boolean(true));
            } else {
                a.props.shift_remove("__out_of_bounds__");
            }
        }
        Some(new_len)
    }

    /// Convert a numeric value to the type-specific element value for a TypedArray.
    pub(super) fn typed_array_coerce_value(num: f64, ta_name: &str) -> f64 {
        match ta_name {
            "Float16Array" => return f16round(num),
            "Float32Array" => return (num as f32) as f64,
            "Float64Array" => return num,
            _ => {}
        }
        if num.is_nan() || num == 0.0 || !num.is_finite() {
            match ta_name {
                "Uint8ClampedArray" => {
                    if num.is_nan() {
                        return 0.0;
                    }
                    if num == f64::INFINITY {
                        return 255.0;
                    }
                    if num == f64::NEG_INFINITY {
                        return 0.0;
                    }
                    return 0.0;
                }
                _ => return 0.0,
            }
        }
        match ta_name {
            "Uint8Array" => {
                let int_val = num.trunc() as i64;
                (int_val.rem_euclid(256)) as f64
            }
            "Uint8ClampedArray" => Self::to_uint8_clamp(num) as f64,
            "Int8Array" => {
                let int_val = num.trunc() as i64;
                let m = int_val.rem_euclid(256);
                if m >= 128 { (m - 256) as f64 } else { m as f64 }
            }
            "Uint16Array" => {
                let int_val = num.trunc() as i64;
                (int_val.rem_euclid(65536)) as f64
            }
            "Int16Array" => {
                let int_val = num.trunc() as i64;
                let m = int_val.rem_euclid(65536);
                if m >= 32768 { (m - 65536) as f64 } else { m as f64 }
            }
            "Uint32Array" => {
                let int_val = num.trunc() as i128;
                (int_val.rem_euclid(4294967296)) as f64
            }
            "Int32Array" => {
                let int_val = num.trunc() as i128;
                let m = int_val.rem_euclid(4294967296);
                if m >= 2147483648 { (m - 4294967296) as f64 } else { m as f64 }
            }
            "Float16Array" => f16round(num),
            "Float32Array" => (num as f32) as f64,
            "Float64Array" => num,
            _ => {
                let int_val = num.trunc() as i64;
                (int_val.rem_euclid(256)) as f64
            }
        }
    }

    pub(super) fn typedarray_call_builtin(&mut self, ctx: &GcContext<'gc>, id: FunctionID, args: &[Value<'gc>]) -> Value<'gc> {
        // TypedArray constructors must be called with new
        if self.new_target_stack.is_empty() {
            self.throw_type_error(ctx, "Constructor requires 'new'");
            return Value::Undefined;
        }

        let bytes_per_element = match id {
            BUILTIN_CTOR_INT16ARRAY | BUILTIN_CTOR_UINT16ARRAY | BUILTIN_CTOR_FLOAT16ARRAY => 2usize,
            BUILTIN_CTOR_INT32ARRAY | BUILTIN_CTOR_UINT32ARRAY | BUILTIN_CTOR_FLOAT32ARRAY => 4usize,
            BUILTIN_CTOR_FLOAT64ARRAY | BUILTIN_CTOR_BIGINT64ARRAY | BUILTIN_CTOR_BIGUINT64ARRAY => 8usize,
            _ => 1usize,
        };
        let typedarray_name = match id {
            BUILTIN_CTOR_INT8ARRAY => "Int8Array",
            BUILTIN_CTOR_UINT8ARRAY => "Uint8Array",
            BUILTIN_CTOR_UINT8CLAMPEDARRAY => "Uint8ClampedArray",
            BUILTIN_CTOR_INT16ARRAY => "Int16Array",
            BUILTIN_CTOR_UINT16ARRAY => "Uint16Array",
            BUILTIN_CTOR_INT32ARRAY => "Int32Array",
            BUILTIN_CTOR_UINT32ARRAY => "Uint32Array",
            BUILTIN_CTOR_FLOAT16ARRAY => "Float16Array",
            BUILTIN_CTOR_FLOAT32ARRAY => "Float32Array",
            BUILTIN_CTOR_FLOAT64ARRAY => "Float64Array",
            BUILTIN_CTOR_BIGINT64ARRAY => "BigInt64Array",
            BUILTIN_CTOR_BIGUINT64ARRAY => "BigUint64Array",
            _ => "TypedArray",
        };
        let is_bigint_ta = is_bigint_typed_array(typedarray_name);

        // Get prototype from constructor for __proto__ on instances
        let ta_instance_proto: Option<Value<'gc>> = self.globals.get(typedarray_name).and_then(|v| {
            if let Value::Object(o) = v {
                own_data_from_legacy_map(&o.borrow(), "prototype")
            } else {
                None
            }
        });

        if let Some(Value::Array(src_arr)) = args.first()
            && src_arr.borrow().props.contains_key("__typedarray_name__")
        {
            // Sync resizable source TA and check for out-of-bounds (ValidateTypedArray)
            self.sync_resizable_ta_elements(ctx, src_arr);
            {
                let sb = src_arr.borrow();
                if matches!(sb.props.get("__out_of_bounds__"), Some(Value::Boolean(true))) {
                    self.throw_type_error(ctx, "Source TypedArray is out of bounds");
                    return Value::Undefined;
                }
            }
            let src_ta_name = src_arr
                .borrow()
                .props
                .get("__typedarray_name__")
                .map(value_to_string)
                .unwrap_or_default();
            let src_is_bigint = is_bigint_typed_array(&src_ta_name);
            // Mixing BigInt and non-BigInt TAs is a TypeError
            if is_bigint_ta != src_is_bigint {
                self.throw_type_error(ctx, "Cannot mix BigInt and other types, use explicit conversions");
                return Value::Undefined;
            }
            let elements_clone: Vec<Value<'gc>> = src_arr.borrow().elements.clone();
            let len = elements_clone.len();
            let mut coerced_elements = Vec::with_capacity(len);
            let mut bigint_vals: Vec<num_bigint::BigInt> = Vec::new();
            let mut numeric_vals: Vec<f64> = Vec::new();
            if is_bigint_ta {
                for v in elements_clone.iter() {
                    let bi = match self.value_to_bigint(ctx, v) {
                        Some(b) => b,
                        None => return Value::Undefined,
                    };
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    let coerced = coerce_bigint_for_ta(&bi, typedarray_name);
                    coerced_elements.push(Value::BigInt(Box::new(coerced)));
                    bigint_vals.push(bi);
                }
            } else {
                for v in elements_clone.iter() {
                    let num = match v {
                        Value::Object(_) | Value::Array(_) => match self.extract_number_with_coercion(ctx, v) {
                            Some(n) => n,
                            None => {
                                if self.pending_throw.is_some() {
                                    return Value::Undefined;
                                }
                                f64::NAN
                            }
                        },
                        _ => to_number(v),
                    };
                    numeric_vals.push(num);
                    coerced_elements.push(Value::Number(Self::typed_array_coerce_value(num, typedarray_name)));
                }
            }
            let mut data = VmArrayData::new(coerced_elements);
            data.props.insert("__typedarray_name__".to_string(), Value::from(typedarray_name));
            data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
            // Create backing ArrayBuffer with properly encoded bytes
            let mut bytes = vec![Value::Number(0.0); len * bytes_per_element];
            if is_bigint_ta {
                for (i, bi) in bigint_vals.iter().enumerate() {
                    Self::encode_typed_element_bigint(&mut bytes, i * bytes_per_element, typedarray_name, bi);
                }
            } else {
                for (i, &num) in numeric_vals.iter().enumerate().take(len) {
                    Self::encode_typed_element(&mut bytes, i * bytes_per_element, bytes_per_element, typedarray_name, num);
                }
            }
            let buffer_obj = self.create_ordinary_array_buffer(ctx, bytes);
            data.props.insert("buffer".to_string(), buffer_obj.clone());
            mark_nonenumerable(&mut data.props, "buffer");
            data.props.insert("__typedarray_buffer__".to_string(), buffer_obj);
            data.props.insert("__byte_offset__".to_string(), Value::Number(0.0));
            data.props
                .insert("__bytes_per_element__".to_string(), Value::Number(bytes_per_element as f64));
            if let Some(proto) = &ta_instance_proto {
                data.props.insert("__proto__".to_string(), proto.clone());
            }
            return Value::Array(new_gc_cell_ptr(ctx, data));
        }

        // Regular Array (non-TypedArray) — use iterator protocol like Object
        if let Some(Value::Array(_)) = args.first() {
            let obj_val = args.first().unwrap().clone();
            let iter_fn = self.read_named_property(ctx, &obj_val, "@@sym:1");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            let has_iterator = !matches!(iter_fn, Value::Undefined | Value::Null);
            if has_iterator && !self.is_value_callable(&iter_fn) {
                self.throw_type_error(ctx, "object is not iterable (Symbol.iterator is not a function)");
                return Value::Undefined;
            }
            let elements = if has_iterator {
                let iterator = match self.vm_call_function_value(ctx, &iter_fn, &obj_val, &[]) {
                    Ok(v) => v,
                    Err(e) => {
                        self.set_pending_throw_from_error(&e);
                        return Value::Undefined;
                    }
                };
                let mut elems = Vec::new();
                loop {
                    let next_fn = self.read_named_property(ctx, &iterator, "next");
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    let result = match self.vm_call_function_value(ctx, &next_fn, &iterator, &[]) {
                        Ok(v) => v,
                        Err(e) => {
                            self.set_pending_throw_from_error(&e);
                            return Value::Undefined;
                        }
                    };
                    let done = self.read_named_property(ctx, &result, "done");
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    if Self::value_is_truthy(&done) {
                        break;
                    }
                    let value = self.read_named_property(ctx, &result, "value");
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    elems.push(value);
                }
                elems
            } else {
                let len_val = self.read_named_property(ctx, &obj_val, "length");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let len = match &len_val {
                    Value::Number(n) if n.is_finite() && *n >= 0.0 => *n as usize,
                    _ => 0,
                };
                let mut elems = Vec::with_capacity(len);
                for i in 0..len {
                    let v = self.read_named_property(ctx, &obj_val, &i.to_string());
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    elems.push(v);
                }
                elems
            };
            let len = elements.len();
            let mut coerced_elements = Vec::with_capacity(len);
            let mut bigint_vals: Vec<num_bigint::BigInt> = Vec::new();
            let mut numeric_vals: Vec<f64> = Vec::new();
            if is_bigint_ta {
                for elem in elements.iter() {
                    let bi = match self.value_to_bigint(ctx, elem) {
                        Some(b) => b,
                        None => return Value::Undefined,
                    };
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    let coerced = coerce_bigint_for_ta(&bi, typedarray_name);
                    coerced_elements.push(Value::BigInt(Box::new(coerced)));
                    bigint_vals.push(bi);
                }
            } else {
                for elem in elements.iter() {
                    if elem.is_symbol_value() {
                        self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                        return Value::Undefined;
                    }
                    let num = match self.extract_number_with_coercion(ctx, elem) {
                        Some(n) => n,
                        None => return Value::Undefined,
                    };
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    numeric_vals.push(num);
                    coerced_elements.push(Value::Number(Self::typed_array_coerce_value(num, typedarray_name)));
                }
            }
            let mut data = VmArrayData::new(coerced_elements);
            data.props.insert("__typedarray_name__".to_string(), Value::from(typedarray_name));
            data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
            let mut bytes = vec![Value::Number(0.0); len * bytes_per_element];
            if is_bigint_ta {
                for (i, bi) in bigint_vals.iter().enumerate() {
                    Self::encode_typed_element_bigint(&mut bytes, i * bytes_per_element, typedarray_name, bi);
                }
            } else {
                for (i, &num) in numeric_vals.iter().enumerate() {
                    Self::encode_typed_element(&mut bytes, i * bytes_per_element, bytes_per_element, typedarray_name, num);
                }
            }
            let buffer_obj = self.create_ordinary_array_buffer(ctx, bytes);
            data.props.insert("buffer".to_string(), buffer_obj.clone());
            mark_nonenumerable(&mut data.props, "buffer");
            data.props.insert("__typedarray_buffer__".to_string(), buffer_obj);
            data.props.insert("__byte_offset__".to_string(), Value::Number(0.0));
            data.props
                .insert("__bytes_per_element__".to_string(), Value::Number(bytes_per_element as f64));
            if let Some(proto) = &ta_instance_proto {
                data.props.insert("__proto__".to_string(), proto.clone());
            }
            return Value::Array(new_gc_cell_ptr(ctx, data));
        }

        // Symbol check: must reject Symbols before they reach the Object path
        if let Some(first) = args.first()
            && first.is_symbol_value()
        {
            self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
            return Value::Undefined;
        }

        if let Some(Value::Object(buf_obj)) = args.first() {
            let buffer_type = buf_obj.borrow().get("__type__").map(value_to_string).unwrap_or_default();
            let is_array_buffer = matches!(
                buf_obj.borrow().get("__type__"),
                Some(Value::String(s))
                    if crate::unicode::utf16_to_utf8(s) == "ArrayBuffer"
                        || crate::unicode::utf16_to_utf8(s) == "SharedArrayBuffer"
            );
            if is_array_buffer {
                // Check for detached buffer
                if matches!(buf_obj.borrow().get("__detached__"), Some(Value::Boolean(true))) {
                    self.throw_type_error(ctx, "Cannot construct a TypedArray with a detached ArrayBuffer");
                    return Value::Undefined;
                }
                let byte_len = match buf_obj.borrow().get("byteLength") {
                    Some(Value::Number(n)) if *n >= 0.0 => *n as usize,
                    _ => 0,
                };

                // ToIndex(byteOffset)
                let raw_offset = args.get(1).cloned().unwrap_or(Value::Undefined);
                let byte_offset: usize;
                if matches!(&raw_offset, Value::Undefined) {
                    byte_offset = 0;
                } else if raw_offset.is_symbol_value() {
                    self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                    return Value::Undefined;
                } else {
                    let n = match self.extract_number_with_coercion(ctx, &raw_offset) {
                        Some(v) => v,
                        None => return Value::Undefined,
                    };
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    // ToIndex: ToIntegerOrInfinity then range check
                    let int_n = if n.is_nan() || n == 0.0 {
                        0.0
                    } else if !n.is_finite() {
                        n
                    } else {
                        n.trunc()
                    };
                    if int_n < 0.0 || !int_n.is_finite() || int_n > 9007199254740991.0 {
                        self.throw_range_error_object(ctx, "Invalid typed array byte offset");
                        return Value::Undefined;
                    }
                    byte_offset = int_n as usize;
                }

                // byteOffset must be a multiple of elementSize
                if !byte_offset.is_multiple_of(bytes_per_element) {
                    self.throw_range_error_object(
                        ctx,
                        &format!("Start offset of {} should be a multiple of {}", typedarray_name, bytes_per_element),
                    );
                    return Value::Undefined;
                }

                // Check for detached buffer after ToIndex conversions
                if matches!(buf_obj.borrow().get("__detached__"), Some(Value::Boolean(true))) {
                    self.throw_type_error(ctx, "Cannot construct a TypedArray with a detached ArrayBuffer");
                    return Value::Undefined;
                }

                // ToIndex(length) if provided
                let raw_len = args.get(2).cloned().unwrap_or(Value::Undefined);
                let explicit_len: Option<usize>;
                if matches!(&raw_len, Value::Undefined) {
                    explicit_len = None;
                } else if raw_len.is_symbol_value() {
                    self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                    return Value::Undefined;
                } else {
                    let n = match self.extract_number_with_coercion(ctx, &raw_len) {
                        Some(v) => v,
                        None => return Value::Undefined,
                    };
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    // ToIndex: ToIntegerOrInfinity then range check
                    let int_n = if n.is_nan() || n == 0.0 {
                        0.0
                    } else if !n.is_finite() {
                        n
                    } else {
                        n.trunc()
                    };
                    if int_n < 0.0 || !int_n.is_finite() || int_n > 9007199254740991.0 {
                        self.throw_range_error_object(ctx, "Invalid typed array length");
                        return Value::Undefined;
                    }
                    explicit_len = Some(int_n as usize);
                }

                // Check for detached buffer after ToIndex(length)
                if matches!(buf_obj.borrow().get("__detached__"), Some(Value::Boolean(true))) {
                    self.throw_type_error(ctx, "Cannot construct a TypedArray with a detached ArrayBuffer");
                    return Value::Undefined;
                }

                let buf_is_resizable = matches!(buf_obj.borrow().get("__resizable__"), Some(Value::Boolean(true)))
                    || matches!(buf_obj.borrow().get("__growable__"), Some(Value::Boolean(true)));

                let initial_len;
                if let Some(len) = explicit_len {
                    // Explicit length: check it doesn't exceed buffer
                    let needed = byte_offset + len * bytes_per_element;
                    if needed > byte_len {
                        self.throw_range_error_object(ctx, "Invalid typed array length");
                        return Value::Undefined;
                    }
                    initial_len = len;
                } else if buf_is_resizable {
                    // Resizable buffer + no explicit length: length-tracking TA
                    // No alignment check per spec step 13
                    if byte_offset > byte_len {
                        self.throw_range_error_object(ctx, "Start offset is outside the bounds of the buffer");
                        return Value::Undefined;
                    }
                    initial_len = (byte_len.saturating_sub(byte_offset)) / bytes_per_element;
                } else {
                    // Non-resizable buffer + no explicit length: must be aligned
                    if byte_len % bytes_per_element != 0 {
                        self.throw_range_error_object(
                            ctx,
                            &format!("Byte length of {} should be a multiple of {}", typedarray_name, bytes_per_element),
                        );
                        return Value::Undefined;
                    }
                    if byte_offset > byte_len {
                        self.throw_range_error_object(ctx, "Start offset is outside the bounds of the buffer");
                        return Value::Undefined;
                    }
                    initial_len = (byte_len - byte_offset) / bytes_per_element;
                }
                // Decode elements from buffer bytes
                let elements = if let Some(Value::Array(buf_bytes)) = buf_obj.borrow().get("__buffer_bytes__").cloned() {
                    let bb = buf_bytes.borrow();
                    let mut elems = Vec::with_capacity(initial_len);
                    for i in 0..initial_len {
                        let base = byte_offset + i * bytes_per_element;
                        let val = Self::decode_typed_element(&bb.elements, base, bytes_per_element, typedarray_name);
                        elems.push(val);
                    }
                    elems
                } else if is_bigint_ta {
                    vec![Value::BigInt(Box::new(num_bigint::BigInt::from(0))); initial_len]
                } else {
                    vec![Value::Number(0.0); initial_len]
                };
                let mut data = VmArrayData::new(elements);
                data.props.insert("__typedarray_name__".to_string(), Value::from(typedarray_name));
                data.props.insert("__buffer_type__".to_string(), Value::from(&buffer_type));
                data.props.insert("buffer".to_string(), Value::Object(*buf_obj));
                mark_nonenumerable(&mut data.props, "buffer");
                data.props.insert("__byte_offset__".to_string(), Value::Number(byte_offset as f64));
                data.props
                    .insert("__bytes_per_element__".to_string(), Value::Number(bytes_per_element as f64));
                data.props.insert(
                    "__length_tracking__".to_string(),
                    Value::Boolean(explicit_len.is_none() && buf_is_resizable),
                );
                if let Some(len) = explicit_len {
                    data.props.insert("__fixed_length__".to_string(), Value::Number(len as f64));
                }
                data.props.insert("__typedarray_buffer__".to_string(), Value::Object(*buf_obj));
                if let Some(proto) = &ta_instance_proto {
                    data.props.insert("__proto__".to_string(), proto.clone());
                }
                return Value::Array(new_gc_cell_ptr(ctx, data));
            }
            // Check for Symbol.iterator first (iterable object)
            let obj_val = Value::Object(*buf_obj);
            let iter_fn = self.read_named_property(ctx, &obj_val, "@@sym:1");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            let has_iterator = !matches!(iter_fn, Value::Undefined | Value::Null);
            if has_iterator && !self.is_value_callable(&iter_fn) {
                self.throw_type_error(ctx, "object is not iterable (Symbol.iterator is not a function)");
                return Value::Undefined;
            }
            let elements = if has_iterator {
                // Iterable: call Symbol.iterator, collect all values
                let iterator = match self.vm_call_function_value(ctx, &iter_fn, &obj_val, &[]) {
                    Ok(v) => v,
                    Err(e) => {
                        self.set_pending_throw_from_error(&e);
                        return Value::Undefined;
                    }
                };
                let mut elems = Vec::new();
                loop {
                    let next_fn = self.read_named_property(ctx, &iterator, "next");
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    let result = match self.vm_call_function_value(ctx, &next_fn, &iterator, &[]) {
                        Ok(v) => v,
                        Err(e) => {
                            self.set_pending_throw_from_error(&e);
                            return Value::Undefined;
                        }
                    };
                    let done = self.read_named_property(ctx, &result, "done");
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    if Self::value_is_truthy(&done) {
                        break;
                    }
                    let value = self.read_named_property(ctx, &result, "value");
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    elems.push(value);
                }
                elems
            } else {
                // Array-like: has numeric 'length' property
                let len_val = self.read_named_property(ctx, &obj_val, "length");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                if len_val.is_symbol_value() {
                    self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                    return Value::Undefined;
                }
                let len = match &len_val {
                    Value::Number(n) if n.is_finite() && *n >= 0.0 => {
                        if *n > 9007199254740991.0 {
                            self.throw_range_error_object(ctx, "Invalid typed array length");
                            return Value::Undefined;
                        }
                        *n as usize
                    }
                    _ => {
                        let n = match self.extract_number_with_coercion(ctx, &len_val) {
                            Some(v) => v,
                            None => return Value::Undefined,
                        };
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        if n.is_nan() || n == 0.0 {
                            0
                        } else if n < 0.0 || !n.is_finite() {
                            self.throw_range_error_object(ctx, "Invalid typed array length");
                            return Value::Undefined;
                        } else {
                            n as usize
                        }
                    }
                };
                let mut elems = Vec::with_capacity(len);
                for i in 0..len {
                    let v = self.read_named_property(ctx, &obj_val, &i.to_string());
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    elems.push(v);
                }
                elems
            };
            let len = elements.len();
            // Coerce elements to typed numeric representation (calls valueOf/toString)
            let mut coerced_elements = Vec::with_capacity(len);
            let mut bigint_vals: Vec<num_bigint::BigInt> = Vec::new();
            let mut numeric_vals: Vec<f64> = Vec::new();
            if is_bigint_ta {
                for elem in elements.iter() {
                    let bi = match self.value_to_bigint(ctx, elem) {
                        Some(b) => b,
                        None => return Value::Undefined,
                    };
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    let coerced = coerce_bigint_for_ta(&bi, typedarray_name);
                    coerced_elements.push(Value::BigInt(Box::new(coerced)));
                    bigint_vals.push(bi);
                }
            } else {
                for elem in elements.iter() {
                    if elem.is_symbol_value() {
                        self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                        return Value::Undefined;
                    }
                    let num = match self.extract_number_with_coercion(ctx, elem) {
                        Some(n) => n,
                        None => return Value::Undefined,
                    };
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    numeric_vals.push(num);
                    coerced_elements.push(Value::Number(Self::typed_array_coerce_value(num, typedarray_name)));
                }
            }
            let mut data = VmArrayData::new(coerced_elements);
            data.props.insert("__typedarray_name__".to_string(), Value::from(typedarray_name));
            data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
            // Create backing ArrayBuffer with properly encoded bytes
            let mut bytes = vec![Value::Number(0.0); len * bytes_per_element];
            if is_bigint_ta {
                for (i, bi) in bigint_vals.iter().enumerate() {
                    Self::encode_typed_element_bigint(&mut bytes, i * bytes_per_element, typedarray_name, bi);
                }
            } else {
                for (i, &num) in numeric_vals.iter().enumerate() {
                    Self::encode_typed_element(&mut bytes, i * bytes_per_element, bytes_per_element, typedarray_name, num);
                }
            }
            let buffer_obj = self.create_ordinary_array_buffer(ctx, bytes);
            data.props.insert("buffer".to_string(), buffer_obj.clone());
            mark_nonenumerable(&mut data.props, "buffer");
            data.props.insert("__typedarray_buffer__".to_string(), buffer_obj);
            data.props.insert("__byte_offset__".to_string(), Value::Number(0.0));
            data.props
                .insert("__bytes_per_element__".to_string(), Value::Number(bytes_per_element as f64));
            if let Some(proto) = &ta_instance_proto {
                data.props.insert("__proto__".to_string(), proto.clone());
            }
            return Value::Array(new_gc_cell_ptr(ctx, data));
        }

        // Object-arg catch-all: functions and other object-like values use iterator protocol
        if let Some(first) = args.first() {
            let is_object_like = matches!(first, Value::Closure(..) | Value::Function(..) | Value::NativeFunction(_));
            if is_object_like {
                let obj_val = first.clone();
                let iter_fn = self.read_named_property(ctx, &obj_val, "@@sym:1");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let has_iterator = !matches!(iter_fn, Value::Undefined | Value::Null);
                if has_iterator && !self.is_value_callable(&iter_fn) {
                    self.throw_type_error(ctx, "object is not iterable (Symbol.iterator is not a function)");
                    return Value::Undefined;
                }
                let elements = if has_iterator {
                    let iterator = match self.vm_call_function_value(ctx, &iter_fn, &obj_val, &[]) {
                        Ok(v) => v,
                        Err(e) => {
                            self.set_pending_throw_from_error(&e);
                            return Value::Undefined;
                        }
                    };
                    let mut elems = Vec::new();
                    loop {
                        let next_fn = self.read_named_property(ctx, &iterator, "next");
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        let result = match self.vm_call_function_value(ctx, &next_fn, &iterator, &[]) {
                            Ok(v) => v,
                            Err(e) => {
                                self.set_pending_throw_from_error(&e);
                                return Value::Undefined;
                            }
                        };
                        let done = self.read_named_property(ctx, &result, "done");
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        if Self::value_is_truthy(&done) {
                            break;
                        }
                        let value = self.read_named_property(ctx, &result, "value");
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        elems.push(value);
                    }
                    elems
                } else {
                    let len_val = self.read_named_property(ctx, &obj_val, "length");
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    let len = match &len_val {
                        Value::Number(n) if n.is_finite() && *n >= 0.0 => *n as usize,
                        _ => 0,
                    };
                    let mut elems = Vec::with_capacity(len);
                    for i in 0..len {
                        let v = self.read_named_property(ctx, &obj_val, &i.to_string());
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        elems.push(v);
                    }
                    elems
                };
                let len = elements.len();
                let mut coerced_elements = Vec::with_capacity(len);
                let mut bigint_vals: Vec<num_bigint::BigInt> = Vec::new();
                let mut numeric_vals: Vec<f64> = Vec::new();
                if is_bigint_ta {
                    for elem in elements.iter() {
                        let bi = match self.value_to_bigint(ctx, elem) {
                            Some(b) => b,
                            None => return Value::Undefined,
                        };
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        let coerced = coerce_bigint_for_ta(&bi, typedarray_name);
                        coerced_elements.push(Value::BigInt(Box::new(coerced)));
                        bigint_vals.push(bi);
                    }
                } else {
                    for elem in elements.iter() {
                        if elem.is_symbol_value() {
                            self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                            return Value::Undefined;
                        }
                        let num = match self.extract_number_with_coercion(ctx, elem) {
                            Some(n) => n,
                            None => return Value::Undefined,
                        };
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        numeric_vals.push(num);
                        coerced_elements.push(Value::Number(Self::typed_array_coerce_value(num, typedarray_name)));
                    }
                }
                let mut data = VmArrayData::new(coerced_elements);
                data.props.insert("__typedarray_name__".to_string(), Value::from(typedarray_name));
                data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
                let mut bytes = vec![Value::Number(0.0); len * bytes_per_element];
                if is_bigint_ta {
                    for (i, bi) in bigint_vals.iter().enumerate() {
                        Self::encode_typed_element_bigint(&mut bytes, i * bytes_per_element, typedarray_name, bi);
                    }
                } else {
                    for (i, &num) in numeric_vals.iter().enumerate() {
                        Self::encode_typed_element(&mut bytes, i * bytes_per_element, bytes_per_element, typedarray_name, num);
                    }
                }
                let buffer_obj = self.create_ordinary_array_buffer(ctx, bytes);
                data.props.insert("buffer".to_string(), buffer_obj.clone());
                mark_nonenumerable(&mut data.props, "buffer");
                data.props.insert("__typedarray_buffer__".to_string(), buffer_obj);
                data.props.insert("__byte_offset__".to_string(), Value::Number(0.0));
                data.props
                    .insert("__bytes_per_element__".to_string(), Value::Number(bytes_per_element as f64));
                if let Some(proto) = &ta_instance_proto {
                    data.props.insert("__proto__".to_string(), proto.clone());
                }
                return Value::Array(new_gc_cell_ptr(ctx, data));
            }
        }

        // Length-arg or no-arg path
        let first_arg = args.first().cloned().unwrap_or(Value::Undefined);
        // Check for Symbol
        if first_arg.is_symbol_value() {
            self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
            return Value::Undefined;
        }
        let length = match &first_arg {
            Value::Undefined => 0,
            _ => {
                let n = match self.extract_number_with_coercion(ctx, &first_arg) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                // ToIndex: ToIntegerOrInfinity then range check
                let int_index = if n.is_nan() || n == 0.0 {
                    0.0
                } else if !n.is_finite() {
                    n
                } else {
                    n.trunc()
                };
                if int_index < 0.0 || !int_index.is_finite() || int_index > 9007199254740991.0 {
                    self.throw_range_error_object(ctx, "Invalid typed array length");
                    return Value::Undefined;
                } else {
                    int_index as usize
                }
            }
        };
        let default_elem = if is_bigint_ta {
            Value::BigInt(Box::new(num_bigint::BigInt::from(0)))
        } else {
            Value::Number(0.0)
        };
        let mut data = VmArrayData::new(vec![default_elem; length]);
        data.props.insert("__typedarray_name__".to_string(), Value::from(typedarray_name));
        data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
        let bytes = vec![Value::Number(0.0); length * bytes_per_element];
        let buffer_obj = self.create_ordinary_array_buffer(ctx, bytes);
        data.props.insert("buffer".to_string(), buffer_obj.clone());
        mark_nonenumerable(&mut data.props, "buffer");
        data.props.insert("__typedarray_buffer__".to_string(), buffer_obj);
        data.props.insert("__byte_offset__".to_string(), Value::Number(0.0));
        data.props
            .insert("__bytes_per_element__".to_string(), Value::Number(bytes_per_element as f64));
        data.props.insert("__fixed_length__".to_string(), Value::Number(length as f64));
        data.props.insert("__length_tracking__".to_string(), Value::Boolean(false));
        if let Some(proto) = &ta_instance_proto {
            data.props.insert("__proto__".to_string(), proto.clone());
        }
        Value::Array(new_gc_cell_ptr(ctx, data))
    }

    pub(super) fn initialize_typed_arrays(&mut self, ctx: &GcContext<'gc>, array_to_string_fn_for_ta: Value<'gc>) {
        // ── %TypedArray% intrinsic and shared prototype ──
        // Spec: %TypedArray%.prototype holds all shared TypedArray methods.
        // Chain: instance.__proto__ → XxxArray.prototype → %TypedArray%.prototype → Object.prototype
        //        XxxArray.__proto__ → %TypedArray% → Function.prototype
        let mut ta_proto_map = IndexMap::new();
        if let Some(Value::Object(obj_ctor)) = self.globals.get("Object")
            && let Some(obj_proto) = own_data_from_legacy_map(&obj_ctor.borrow(), "prototype")
        {
            ta_proto_map.insert("__proto__".to_string(), obj_proto);
        }
        // Shared methods — reuse Array builtin implementations
        let ta_values_fn = Self::make_host_fn_with_name_len(ctx, "typedarray.values", "values", 0.0, false);
        let ta_entries_fn = Self::make_host_fn_with_name_len(ctx, "typedarray.entries", "entries", 0.0, false);
        let ta_keys_fn = Self::make_host_fn_with_name_len(ctx, "typedarray.keys_iter", "keys", 0.0, false);
        let ta_copy_within_fn = Self::make_host_fn_with_name_len(ctx, "typedarray.copyWithin", "copyWithin", 2.0, false);
        let ta_to_locale_string_fn = Self::make_host_fn_with_name_len(ctx, "typedarray.toLocaleString", "toLocaleString", 0.0, false);
        ta_proto_map.insert("@@sym:1".to_string(), ta_values_fn.clone()); // Symbol.iterator = values
        ta_proto_map.insert("values".to_string(), ta_values_fn);
        ta_proto_map.insert("entries".to_string(), ta_entries_fn);
        ta_proto_map.insert("keys".to_string(), ta_keys_fn);
        ta_proto_map.insert("copyWithin".to_string(), ta_copy_within_fn);
        ta_proto_map.insert("toString".to_string(), array_to_string_fn_for_ta);
        ta_proto_map.insert("toLocaleString".to_string(), ta_to_locale_string_fn);
        ta_proto_map.insert(
            "join".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.join", "join", 1.0, false),
        );
        ta_proto_map.insert(
            "indexOf".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.indexOf", "indexOf", 1.0, false),
        );
        ta_proto_map.insert(
            "slice".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.slice", "slice", 2.0, false),
        );
        ta_proto_map.insert(
            "map".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.map", "map", 1.0, false),
        );
        ta_proto_map.insert(
            "filter".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.filter", "filter", 1.0, false),
        );
        ta_proto_map.insert(
            "forEach".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.forEach", "forEach", 1.0, false),
        );
        ta_proto_map.insert(
            "reduce".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.reduce", "reduce", 1.0, false),
        );
        ta_proto_map.insert(
            "reverse".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.reverse", "reverse", 0.0, false),
        );
        ta_proto_map.insert(
            "sort".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.sort", "sort", 1.0, false),
        );
        ta_proto_map.insert(
            "find".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.find", "find", 1.0, false),
        );
        ta_proto_map.insert(
            "findIndex".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.findIndex", "findIndex", 1.0, false),
        );
        ta_proto_map.insert(
            "includes".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.includes", "includes", 1.0, false),
        );
        ta_proto_map.insert(
            "fill".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.fill", "fill", 1.0, false),
        );
        ta_proto_map.insert(
            "at".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.at", "at", 1.0, false),
        );
        ta_proto_map.insert(
            "every".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.every", "every", 1.0, false),
        );
        ta_proto_map.insert(
            "some".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.some", "some", 1.0, false),
        );
        ta_proto_map.insert(
            "lastIndexOf".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.lastIndexOf", "lastIndexOf", 1.0, false),
        );
        ta_proto_map.insert(
            "findLast".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.findLast", "findLast", 1.0, false),
        );
        ta_proto_map.insert(
            "findLastIndex".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.findLastIndex", "findLastIndex", 1.0, false),
        );
        ta_proto_map.insert(
            "reduceRight".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.reduceRight", "reduceRight", 1.0, false),
        );
        ta_proto_map.insert(
            "with".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.with", "with", 2.0, false),
        );
        ta_proto_map.insert(
            "toReversed".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.toReversed", "toReversed", 0.0, false),
        );
        ta_proto_map.insert(
            "toSorted".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.toSorted", "toSorted", 1.0, false),
        );
        // TypedArray-specific methods
        ta_proto_map.insert(
            "set".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.set", "set", 1.0, false),
        );
        ta_proto_map.insert(
            "subarray".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.subarray", "subarray", 2.0, false),
        );
        // TypedArray-specific getters: buffer, byteLength, byteOffset, length
        set_getter(
            &mut ta_proto_map,
            "buffer",
            Self::make_host_fn_with_name_len(ctx, "typedarray.get_buffer", "get buffer", 0.0, false),
        );
        set_getter(
            &mut ta_proto_map,
            "byteLength",
            Self::make_host_fn_with_name_len(ctx, "typedarray.get_byteLength", "get byteLength", 0.0, false),
        );
        set_getter(
            &mut ta_proto_map,
            "byteOffset",
            Self::make_host_fn_with_name_len(ctx, "typedarray.get_byteOffset", "get byteOffset", 0.0, false),
        );
        set_getter(
            &mut ta_proto_map,
            "length",
            Self::make_host_fn_with_name_len(ctx, "typedarray.get_length", "get length", 0.0, false),
        );
        // Symbol.toStringTag getter
        set_getter(
            &mut ta_proto_map,
            "@@sym:4",
            Self::make_host_fn_with_name_len(ctx, "typedarray.get_toStringTag", "get [Symbol.toStringTag]", 0.0, false),
        );
        // Mark getter properties as non-enumerable and configurable
        for key in ["buffer", "byteLength", "byteOffset", "length", "@@sym:4"] {
            mark_nonenumerable(&mut ta_proto_map, key);
        }
        // Mark all methods as non-enumerable
        for key in [
            "@@sym:1",
            "values",
            "entries",
            "keys",
            "copyWithin",
            "toString",
            "toLocaleString",
            "join",
            "indexOf",
            "slice",
            "map",
            "filter",
            "forEach",
            "reduce",
            "reverse",
            "sort",
            "find",
            "findIndex",
            "includes",
            "fill",
            "at",
            "every",
            "some",
            "lastIndexOf",
            "findLast",
            "findLastIndex",
            "reduceRight",
            "set",
            "subarray",
            "with",
            "toReversed",
            "toSorted",
        ] {
            mark_nonenumerable(&mut ta_proto_map, key);
        }
        let ta_proto = Value::Object(new_gc_cell_ptr(ctx, ta_proto_map));

        // %TypedArray% constructor (abstract — cannot be called directly)
        let mut typed_array_ctor_map = IndexMap::new();
        typed_array_ctor_map.insert("name".to_string(), Value::from("TypedArray"));
        write_attrs_to_legacy_map(&mut typed_array_ctor_map, "name", PropAttrs::CONFIGURABLE);
        typed_array_ctor_map.insert("length".to_string(), Value::Number(0.0));
        write_attrs_to_legacy_map(&mut typed_array_ctor_map, "length", PropAttrs::CONFIGURABLE);
        typed_array_ctor_map.insert("prototype".to_string(), ta_proto.clone());
        write_attrs_to_legacy_map(&mut typed_array_ctor_map, "prototype", PropAttrs::empty());
        // Mark as constructor (for is_constructor_value)
        typed_array_ctor_map.insert("__native_id__".to_string(), Value::Boolean(true));
        // TypedArray.from() and TypedArray.of() static methods
        typed_array_ctor_map.insert(
            "from".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.from", "from", 1.0, false),
        );
        mark_nonenumerable(&mut typed_array_ctor_map, "from");
        typed_array_ctor_map.insert(
            "of".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.of", "of", 0.0, false),
        );
        mark_nonenumerable(&mut typed_array_ctor_map, "of");
        Self::insert_species_getter(&mut typed_array_ctor_map, ctx);
        // Set constructor backref on prototype
        let typed_array_ctor = Value::Object(new_gc_cell_ptr(ctx, typed_array_ctor_map));
        if let Value::Object(p) = &ta_proto {
            p.borrow_mut(ctx).insert("constructor".to_string(), typed_array_ctor.clone());
            mark_nonenumerable(&mut p.borrow_mut(ctx), "constructor");
        }
        // Expose %TypedArray% as a global (abstract constructor, not directly constructible)
        self.globals.insert("TypedArray".to_string(), typed_array_ctor.clone());

        // Individual TypedArray constructors — all share the %TypedArray%.prototype
        let ta_types: &[(&str, FunctionID, f64)] = &[
            ("Int8Array", BUILTIN_CTOR_INT8ARRAY, 1.0),
            ("Uint8Array", BUILTIN_CTOR_UINT8ARRAY, 1.0),
            ("Uint8ClampedArray", BUILTIN_CTOR_UINT8CLAMPEDARRAY, 1.0),
            ("Int16Array", BUILTIN_CTOR_INT16ARRAY, 2.0),
            ("Uint16Array", BUILTIN_CTOR_UINT16ARRAY, 2.0),
            ("Int32Array", BUILTIN_CTOR_INT32ARRAY, 4.0),
            ("Uint32Array", BUILTIN_CTOR_UINT32ARRAY, 4.0),
            ("Float16Array", BUILTIN_CTOR_FLOAT16ARRAY, 2.0),
            ("Float32Array", BUILTIN_CTOR_FLOAT32ARRAY, 4.0),
            ("Float64Array", BUILTIN_CTOR_FLOAT64ARRAY, 8.0),
            ("BigInt64Array", BUILTIN_CTOR_BIGINT64ARRAY, 8.0),
            ("BigUint64Array", BUILTIN_CTOR_BIGUINT64ARRAY, 8.0),
        ];
        for &(name, ctor_id, bpe) in ta_types {
            let mut ctor_map = IndexMap::new();
            ctor_map.insert("__native_id__".to_string(), Value::Number(ctor_id as f64));
            ctor_map.insert("name".to_string(), Value::from(name));
            write_attrs_to_legacy_map(&mut ctor_map, "name", PropAttrs::CONFIGURABLE);
            ctor_map.insert("length".to_string(), Value::Number(3.0));
            write_attrs_to_legacy_map(&mut ctor_map, "length", PropAttrs::CONFIGURABLE);
            ctor_map.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(bpe));
            write_attrs_to_legacy_map(&mut ctor_map, "BYTES_PER_ELEMENT", PropAttrs::empty());
            // Create per-type prototype with __proto__ → %TypedArray%.prototype
            let mut per_proto = IndexMap::new();
            per_proto.insert("__proto__".to_string(), ta_proto.clone());
            per_proto.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(bpe));
            write_attrs_to_legacy_map(&mut per_proto, "BYTES_PER_ELEMENT", PropAttrs::empty());
            // Uint8Array-specific: base64/hex methods on prototype and constructor
            if name == "Uint8Array" {
                for (method, display, len) in [
                    ("typedarray.toBase64", "toBase64", 0.0),
                    ("typedarray.toHex", "toHex", 0.0),
                    ("typedarray.setFromBase64", "setFromBase64", 1.0),
                    ("typedarray.setFromHex", "setFromHex", 1.0),
                ] {
                    per_proto.insert(
                        display.to_string(),
                        Self::make_host_fn_with_name_len(ctx, method, display, len, false),
                    );
                    mark_nonenumerable(&mut per_proto, display);
                }
                for (method, display, len) in [("typedarray.fromBase64", "fromBase64", 1.0), ("typedarray.fromHex", "fromHex", 1.0)] {
                    ctor_map.insert(
                        display.to_string(),
                        Self::make_host_fn_with_name_len(ctx, method, display, len, false),
                    );
                    mark_nonenumerable(&mut ctor_map, display);
                }
            }
            let per_proto_obj = Value::Object(new_gc_cell_ptr(ctx, per_proto));
            ctor_map.insert("prototype".to_string(), per_proto_obj.clone());
            write_attrs_to_legacy_map(&mut ctor_map, "prototype", PropAttrs::empty());
            // XxxArray.__proto__ = %TypedArray%
            ctor_map.insert("__proto__".to_string(), typed_array_ctor.clone());
            let ctor_val = Value::Object(new_gc_cell_ptr(ctx, ctor_map));
            // constructor backref (must point to same GC object)
            if let Value::Object(p) = &per_proto_obj {
                p.borrow_mut(ctx).insert("constructor".to_string(), ctor_val.clone());
                mark_nonenumerable(&mut p.borrow_mut(ctx), "constructor");
            }
            self.globals.insert(name.to_string(), ctor_val.clone());
            self.global_this.borrow_mut(ctx).insert(name.to_string(), ctor_val);
            mark_nonenumerable(&mut self.global_this.borrow_mut(ctx), name);
        }
    }

    // ── Uint8Array base64/hex helpers ──────────────────────────────────

    const BASE64_CHARS: &'static [u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const BASE64URL_CHARS: &'static [u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    fn base64_decode_char(c: u8, use_url: bool) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' if !use_url => Some(62),
            b'/' if !use_url => Some(63),
            b'-' if use_url => Some(62),
            b'_' if use_url => Some(63),
            _ => None,
        }
    }

    fn read_base64_alphabet_option(&mut self, ctx: &GcContext<'gc>, opts: Option<&Value<'gc>>) -> Option<bool> {
        let opts = match opts {
            Some(v) if !matches!(v, Value::Undefined) => v,
            _ => return Some(false),
        };
        let alphabet_val = self.read_named_property(ctx, opts, "alphabet");
        if self.pending_throw.is_some() {
            return None;
        }
        if matches!(alphabet_val, Value::Undefined) {
            return Some(false);
        }
        match &alphabet_val {
            Value::String(s) => {
                let s = crate::unicode::utf16_to_utf8(s);
                match s.as_str() {
                    "base64" => Some(false),
                    "base64url" => Some(true),
                    _ => {
                        self.throw_type_error(ctx, "expected alphabet to be either \"base64\" or \"base64url\"");
                        None
                    }
                }
            }
            _ => {
                self.throw_type_error(ctx, "expected alphabet to be either \"base64\" or \"base64url\"");
                None
            }
        }
    }

    fn read_last_chunk_handling_option(&mut self, ctx: &GcContext<'gc>, opts: Option<&Value<'gc>>) -> Option<String> {
        let opts = match opts {
            Some(v) if !matches!(v, Value::Undefined) => v,
            _ => return Some("loose".to_string()),
        };
        let val = self.read_named_property(ctx, opts, "lastChunkHandling");
        if self.pending_throw.is_some() {
            return None;
        }
        if matches!(val, Value::Undefined) {
            return Some("loose".to_string());
        }
        match &val {
            Value::String(s) => {
                let s = crate::unicode::utf16_to_utf8(s);
                match s.as_str() {
                    "loose" | "strict" | "stop-before-partial" => Some(s),
                    _ => {
                        self.throw_type_error(
                            ctx,
                            "expected lastChunkHandling to be either \"loose\", \"strict\", or \"stop-before-partial\"",
                        );
                        None
                    }
                }
            }
            _ => {
                self.throw_type_error(
                    ctx,
                    "expected lastChunkHandling to be either \"loose\", \"strict\", or \"stop-before-partial\"",
                );
                None
            }
        }
    }

    /// Core base64 decode per spec's FromBase64.
    /// Returns (decoded_bytes, chars_read, optional_error_message).
    /// Bytes/read always reflect valid data decoded so far.
    /// Caller writes bytes first, then throws error if present.
    fn base64_decode_core(
        &self,
        input: &str,
        use_url: bool,
        last_chunk_handling: &str,
        max_length: Option<usize>,
    ) -> (Vec<u8>, usize, Option<String>) {
        let max_len = max_length.unwrap_or(usize::MAX);
        // Spec step 3: early return for maxLength=0
        if max_len == 0 {
            return (Vec::new(), 0, None);
        }
        let bytes = input.as_bytes();
        let len = bytes.len();
        let mut result: Vec<u8> = Vec::new();
        let mut chunk: Vec<u8> = Vec::new();
        let mut i = 0;
        let mut read = 0; // "handledChunkStart" — past last complete operation

        while i < len {
            let c = bytes[i];
            if matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0C) {
                i += 1;
                continue;
            }
            if c == b'=' {
                if chunk.len() == 2 {
                    let mut j = i + 1;
                    while j < len && matches!(bytes[j], b' ' | b'\t' | b'\n' | b'\r' | 0x0C) {
                        j += 1;
                    }
                    if j >= len || bytes[j] != b'=' {
                        if last_chunk_handling == "stop-before-partial" {
                            return (result, read, None);
                        }
                        return (result, read, Some("Invalid base64: expected second padding character".into()));
                    }
                    // Check trailing content after '==' BEFORE decoding
                    let mut trail = j + 1;
                    while trail < len && matches!(bytes[trail], b' ' | b'\t' | b'\n' | b'\r' | 0x0C) {
                        trail += 1;
                    }
                    if trail < len {
                        return (result, read, Some("Invalid base64: trailing content after padding".into()));
                    }
                    if last_chunk_handling == "strict" && chunk[1] & 0x0F != 0 {
                        return (result, read, Some("Invalid base64: non-zero padding bits".into()));
                    }
                    let b = (chunk[0] << 2) | (chunk[1] >> 4);
                    if result.len() + 1 > max_len {
                        return (result, read, None);
                    }
                    result.push(b);
                    read = j + 1;
                    chunk.clear();
                    i = j + 1;
                    continue;
                } else if chunk.len() == 3 {
                    // Check trailing content after '=' BEFORE decoding
                    let mut trail = i + 1;
                    while trail < len && matches!(bytes[trail], b' ' | b'\t' | b'\n' | b'\r' | 0x0C) {
                        trail += 1;
                    }
                    if trail < len {
                        return (result, read, Some("Invalid base64: trailing content after padding".into()));
                    }
                    if last_chunk_handling == "strict" && chunk[2] & 0x03 != 0 {
                        return (result, read, Some("Invalid base64: non-zero padding bits".into()));
                    }
                    let b0 = (chunk[0] << 2) | (chunk[1] >> 4);
                    let b1 = ((chunk[1] & 0x0F) << 4) | (chunk[2] >> 2);
                    if result.len() + 2 > max_len {
                        return (result, read, None);
                    }
                    result.push(b0);
                    result.push(b1);
                    read = i + 1;
                    chunk.clear();
                    i += 1;
                    continue;
                } else {
                    return (result, read, Some("Invalid base64: unexpected padding".into()));
                }
            }

            let val = match Self::base64_decode_char(c, use_url) {
                Some(v) => v,
                None => {
                    return (result, read, Some(format!("Invalid base64 character: {}", c as char)));
                }
            };
            chunk.push(val);
            i += 1;

            if chunk.len() == 4 {
                let b0 = (chunk[0] << 2) | (chunk[1] >> 4);
                let b1 = ((chunk[1] & 0x0F) << 4) | (chunk[2] >> 2);
                let b2 = ((chunk[2] & 0x03) << 6) | chunk[3];
                // All-or-nothing: can't fit all 3 → stop before this chunk
                if result.len() + 3 > max_len {
                    return (result, read, None);
                }
                result.push(b0);
                result.push(b1);
                result.push(b2);
                read = i;
                chunk.clear();
                // Early return when maxLength reached
                if result.len() >= max_len {
                    return (result, read, None);
                }
            }
        }

        // Handle remaining partial chunk
        if !chunk.is_empty() {
            if chunk.len() == 1 {
                if last_chunk_handling == "stop-before-partial" {
                    return (result, read, None);
                }
                return (result, read, Some("Invalid base64: incomplete chunk".into()));
            }
            if last_chunk_handling == "stop-before-partial" {
                return (result, read, None);
            }
            if last_chunk_handling == "strict" {
                return (result, read, Some("Invalid base64: missing padding".into()));
            }
            // "loose" mode
            if chunk.len() == 2 {
                let b = (chunk[0] << 2) | (chunk[1] >> 4);
                if result.len() + 1 > max_len {
                    return (result, read, None);
                }
                result.push(b);
                read = i;
            } else if chunk.len() == 3 {
                let b0 = (chunk[0] << 2) | (chunk[1] >> 4);
                let b1 = ((chunk[1] & 0x0F) << 4) | (chunk[2] >> 2);
                if result.len() + 2 > max_len {
                    return (result, read, None);
                }
                result.push(b0);
                result.push(b1);
                read = i;
            }
        } else {
            read = i;
        }

        // Trailing garbage check (only for fromBase64 — no maxLength)
        if max_length.is_none() {
            let mut trail = read;
            while trail < len {
                let c = bytes[trail];
                if matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0C) {
                    trail += 1;
                    continue;
                }
                return (result, read, Some("Invalid base64: trailing garbage".into()));
            }
        }

        (result, read, None)
    }

    /// Hex decode per spec's FromHex.
    /// Returns (decoded_bytes, chars_read, optional_error_message).
    fn hex_decode_core(input: &str, max_length: Option<usize>) -> (Vec<u8>, usize, Option<String>) {
        let bytes_in = input.as_bytes();
        // Spec step 2: odd length check FIRST (before any decoding)
        if !bytes_in.len().is_multiple_of(2) {
            return (Vec::new(), 0, Some("Invalid hex string: odd length".into()));
        }
        let max_len = max_length.unwrap_or(usize::MAX);
        let mut result = Vec::new();
        let mut i = 0;
        while i + 1 < bytes_in.len() && result.len() < max_len {
            let hi = match Self::hex_char(bytes_in[i]) {
                Some(v) => v,
                None => {
                    return (result, i, Some(format!("Invalid hex character: {}", bytes_in[i] as char)));
                }
            };
            let lo = match Self::hex_char(bytes_in[i + 1]) {
                Some(v) => v,
                None => {
                    return (result, i, Some(format!("Invalid hex character: {}", bytes_in[i + 1] as char)));
                }
            };
            result.push((hi << 4) | lo);
            i += 2;
        }
        (result, i, None)
    }

    fn hex_char(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }

    fn validate_uint8array(&mut self, ctx: &GcContext<'gc>, val: &Value<'gc>, method: &str) -> bool {
        match val {
            Value::Array(arr) => {
                let a = arr.borrow();
                match a.props.get("__typedarray_name__").map(value_to_string) {
                    Some(ref s) if s == "Uint8Array" => true,
                    _ => {
                        drop(a);
                        self.throw_type_error(ctx, &format!("{} called on non-Uint8Array", method));
                        false
                    }
                }
            }
            _ => {
                self.throw_type_error(ctx, &format!("{} called on non-Uint8Array", method));
                false
            }
        }
    }

    fn check_uint8array_not_detached(&mut self, ctx: &GcContext<'gc>, val: &Value<'gc>) -> bool {
        if let Value::Array(arr) = val {
            let a = arr.borrow();
            if let Some(Value::Object(buf)) = a.props.get("__typedarray_buffer__")
                && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
            {
                drop(a);
                self.throw_type_error(ctx, "Cannot perform operation on a detached ArrayBuffer");
                return false;
            }
        }
        true
    }

    /// Read raw bytes from a Uint8Array's backing buffer.
    fn get_uint8array_bytes(&self, val: &Value<'gc>) -> Vec<u8> {
        if let Value::Array(arr) = val {
            let a = arr.borrow();
            let byte_offset = match a.props.get("__byte_offset__") {
                Some(Value::Number(n)) => *n as usize,
                _ => 0,
            };
            let len = a.elements.len();
            if let Some(Value::Object(buf)) = a.props.get("__typedarray_buffer__") {
                let buf_b = buf.borrow();
                if let Some(Value::Array(buf_bytes)) = buf_b.get("__buffer_bytes__") {
                    let bb = buf_bytes.borrow();
                    let mut result = Vec::with_capacity(len);
                    for i in 0..len {
                        let idx = byte_offset + i;
                        let b = if idx < bb.elements.len() {
                            match &bb.elements[idx] {
                                Value::Number(n) => *n as u8,
                                _ => 0,
                            }
                        } else {
                            0
                        };
                        result.push(b);
                    }
                    return result;
                }
            }
            // Fallback to elements
            return a
                .elements
                .iter()
                .map(|v| match v {
                    Value::Number(n) => *n as u8,
                    _ => 0,
                })
                .collect();
        }
        Vec::new()
    }

    /// Write bytes into a Uint8Array's backing buffer.
    fn write_bytes_to_uint8array(&self, ctx: &GcContext<'gc>, ta: &Value<'gc>, bytes: &[u8]) {
        if let Value::Array(arr) = ta {
            let a = arr.borrow();
            let byte_offset = match a.props.get("__byte_offset__") {
                Some(Value::Number(n)) => *n as usize,
                _ => 0,
            };
            if let Some(Value::Object(buf)) = a.props.get("__typedarray_buffer__").cloned() {
                let buf_b = buf.borrow();
                if let Some(Value::Array(buf_bytes)) = buf_b.get("__buffer_bytes__").cloned() {
                    drop(buf_b);
                    drop(a);
                    let mut bb = buf_bytes.borrow_mut(ctx);
                    for (i, b) in bytes.iter().enumerate() {
                        let idx = byte_offset + i;
                        if idx < bb.elements.len() {
                            bb.elements[idx] = Value::Number(*b as f64);
                        }
                    }
                    // Also sync elements array
                    let mut a = arr.borrow_mut(ctx);
                    for (i, b) in bytes.iter().enumerate() {
                        if i < a.elements.len() {
                            a.elements[i] = Value::Number(*b as f64);
                        }
                    }
                    return;
                }
            }
            // No buffer — write to elements directly
            drop(a);
            let mut a = arr.borrow_mut(ctx);
            for (i, b) in bytes.iter().enumerate() {
                if i < a.elements.len() {
                    a.elements[i] = Value::Number(*b as f64);
                }
            }
        }
    }

    fn create_uint8array_from_bytes(&mut self, ctx: &GcContext<'gc>, bytes: Vec<u8>) -> Value<'gc> {
        let elements: Vec<Value<'gc>> = bytes.iter().map(|b| Value::Number(*b as f64)).collect();
        let len = elements.len();
        let mut data = VmArrayData::new(elements);
        data.props.insert("__typedarray_name__".to_string(), Value::from("Uint8Array"));
        data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
        let buf_bytes: Vec<Value<'gc>> = bytes.iter().map(|b| Value::Number(*b as f64)).collect();
        let buffer_obj = self.create_ordinary_array_buffer(ctx, buf_bytes);
        data.props.insert("buffer".to_string(), buffer_obj.clone());
        mark_nonenumerable(&mut data.props, "buffer");
        data.props.insert("__typedarray_buffer__".to_string(), buffer_obj);
        data.props.insert("__byte_offset__".to_string(), Value::Number(0.0));
        data.props.insert("__bytes_per_element__".to_string(), Value::Number(1.0));
        data.props.insert("__fixed_length__".to_string(), Value::Number(len as f64));
        data.props.insert("__length_tracking__".to_string(), Value::Boolean(false));
        if let Some(Value::Object(ctor)) = self.globals.get("Uint8Array")
            && let Some(proto) = own_data_from_legacy_map(&ctor.borrow(), "prototype")
        {
            data.props.insert("__proto__".to_string(), proto);
        }
        Value::Array(new_gc_cell_ptr(ctx, data))
    }

    pub(super) fn create_immutable_uint8array_from_bytes(&mut self, ctx: &GcContext<'gc>, bytes: Vec<u8>) -> Value<'gc> {
        let elements: Vec<Value<'gc>> = bytes.iter().map(|b| Value::Number(*b as f64)).collect();
        let len = elements.len();
        let mut data = VmArrayData::new(elements);
        data.props.insert("__typedarray_name__".to_string(), Value::from("Uint8Array"));
        data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
        let buf_bytes: Vec<Value<'gc>> = bytes.iter().map(|b| Value::Number(*b as f64)).collect();
        let buffer_obj = self.create_immutable_array_buffer(ctx, buf_bytes);
        data.props.insert("buffer".to_string(), buffer_obj.clone());
        mark_nonenumerable(&mut data.props, "buffer");
        data.props.insert("__typedarray_buffer__".to_string(), buffer_obj);
        data.props.insert("__byte_offset__".to_string(), Value::Number(0.0));
        data.props.insert("__bytes_per_element__".to_string(), Value::Number(1.0));
        data.props.insert("__fixed_length__".to_string(), Value::Number(len as f64));
        data.props.insert("__length_tracking__".to_string(), Value::Boolean(false));
        if let Some(Value::Object(ctor)) = self.globals.get("Uint8Array")
            && let Some(proto) = own_data_from_legacy_map(&ctor.borrow(), "prototype")
        {
            data.props.insert("__proto__".to_string(), proto);
        }
        Value::Array(new_gc_cell_ptr(ctx, data))
    }

    // ── Method implementations ──

    fn uint8array_to_base64(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, args: &[Value<'gc>]) -> Value<'gc> {
        let this = receiver.unwrap_or(&Value::Undefined);
        if !self.validate_uint8array(ctx, this, "Uint8Array.prototype.toBase64") {
            return Value::Undefined;
        }
        let use_url = match self.read_base64_alphabet_option(ctx, args.first()) {
            Some(v) => v,
            None => return Value::Undefined,
        };
        let omit_padding = if let Some(opts) = args.first()
            && !matches!(opts, Value::Undefined)
        {
            let val = self.read_named_property(ctx, opts, "omitPadding");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            val.to_truthy()
        } else {
            false
        };
        // Detached check AFTER option evaluation (per spec)
        if !self.check_uint8array_not_detached(ctx, this) {
            return Value::Undefined;
        }

        let bytes = self.get_uint8array_bytes(this);
        let alphabet = if use_url { Self::BASE64URL_CHARS } else { Self::BASE64_CHARS };
        let mut result = String::new();
        for chunk in bytes.chunks(3) {
            match chunk.len() {
                3 => {
                    let (b0, b1, b2) = (chunk[0] as usize, chunk[1] as usize, chunk[2] as usize);
                    result.push(alphabet[b0 >> 2] as char);
                    result.push(alphabet[((b0 & 3) << 4) | (b1 >> 4)] as char);
                    result.push(alphabet[((b1 & 0xF) << 2) | (b2 >> 6)] as char);
                    result.push(alphabet[b2 & 0x3F] as char);
                }
                2 => {
                    let (b0, b1) = (chunk[0] as usize, chunk[1] as usize);
                    result.push(alphabet[b0 >> 2] as char);
                    result.push(alphabet[((b0 & 3) << 4) | (b1 >> 4)] as char);
                    result.push(alphabet[(b1 & 0xF) << 2] as char);
                    if !omit_padding {
                        result.push('=');
                    }
                }
                1 => {
                    let b0 = chunk[0] as usize;
                    result.push(alphabet[b0 >> 2] as char);
                    result.push(alphabet[(b0 & 3) << 4] as char);
                    if !omit_padding {
                        result.push('=');
                        result.push('=');
                    }
                }
                _ => {}
            }
        }
        Value::from(&result)
    }

    fn uint8array_to_hex(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let this = receiver.unwrap_or(&Value::Undefined);
        if !self.validate_uint8array(ctx, this, "Uint8Array.prototype.toHex") {
            return Value::Undefined;
        }
        if !self.check_uint8array_not_detached(ctx, this) {
            return Value::Undefined;
        }
        let bytes = self.get_uint8array_bytes(this);
        let mut result = String::with_capacity(bytes.len() * 2);
        for b in &bytes {
            result.push_str(&format!("{:02x}", b));
        }
        Value::from(&result)
    }

    fn uint8array_from_base64(&mut self, ctx: &GcContext<'gc>, args: &[Value<'gc>]) -> Value<'gc> {
        let input = args.first().cloned().unwrap_or(Value::Undefined);
        if !matches!(&input, Value::String(_)) {
            self.throw_type_error(ctx, "Uint8Array.fromBase64: first argument must be a string");
            return Value::Undefined;
        }
        let input_str = value_to_string(&input);
        let opts = args.get(1);
        let use_url = match self.read_base64_alphabet_option(ctx, opts) {
            Some(v) => v,
            None => return Value::Undefined,
        };
        let last_chunk = match self.read_last_chunk_handling_option(ctx, opts) {
            Some(v) => v,
            None => return Value::Undefined,
        };
        let (bytes, _read, error) = self.base64_decode_core(&input_str, use_url, &last_chunk, None);
        if let Some(msg) = error {
            self.throw_syntax_error(ctx, &msg);
            return Value::Undefined;
        }
        self.create_uint8array_from_bytes(ctx, bytes)
    }

    fn uint8array_from_hex(&mut self, ctx: &GcContext<'gc>, args: &[Value<'gc>]) -> Value<'gc> {
        let input = args.first().cloned().unwrap_or(Value::Undefined);
        if !matches!(&input, Value::String(_)) {
            self.throw_type_error(ctx, "Uint8Array.fromHex: first argument must be a string");
            return Value::Undefined;
        }
        let input_str = value_to_string(&input);
        let (bytes, _read, error) = Self::hex_decode_core(&input_str, None);
        if let Some(msg) = error {
            self.throw_syntax_error(ctx, &msg);
            return Value::Undefined;
        }
        self.create_uint8array_from_bytes(ctx, bytes)
    }

    fn uint8array_set_from_base64(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, args: &[Value<'gc>]) -> Value<'gc> {
        let this = receiver.unwrap_or(&Value::Undefined);
        if !self.validate_uint8array(ctx, this, "Uint8Array.prototype.setFromBase64") {
            return Value::Undefined;
        }
        let input = args.first().cloned().unwrap_or(Value::Undefined);
        if !matches!(&input, Value::String(_)) {
            self.throw_type_error(ctx, "Uint8Array.prototype.setFromBase64: first argument must be a string");
            return Value::Undefined;
        }
        let input_str = value_to_string(&input);
        let opts = args.get(1);
        let use_url = match self.read_base64_alphabet_option(ctx, opts) {
            Some(v) => v,
            None => return Value::Undefined,
        };
        let last_chunk = match self.read_last_chunk_handling_option(ctx, opts) {
            Some(v) => v,
            None => return Value::Undefined,
        };
        // Detached check AFTER option evaluation
        if !self.check_uint8array_not_detached(ctx, this) {
            return Value::Undefined;
        }
        let target_len = match this {
            Value::Array(arr) => arr.borrow().elements.len(),
            _ => 0,
        };
        let (bytes, read, error) = self.base64_decode_core(&input_str, use_url, &last_chunk, Some(target_len));
        let written = bytes.len();
        self.write_bytes_to_uint8array(ctx, this, &bytes);
        if let Some(msg) = error {
            self.throw_syntax_error(ctx, &msg);
            return Value::Undefined;
        }
        let mut result_map = IndexMap::new();
        result_map.insert("read".to_string(), Value::Number(read as f64));
        result_map.insert("written".to_string(), Value::Number(written as f64));
        Value::Object(new_gc_cell_ptr(ctx, result_map))
    }

    fn uint8array_set_from_hex(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, args: &[Value<'gc>]) -> Value<'gc> {
        let this = receiver.unwrap_or(&Value::Undefined);
        if !self.validate_uint8array(ctx, this, "Uint8Array.prototype.setFromHex") {
            return Value::Undefined;
        }
        let input = args.first().cloned().unwrap_or(Value::Undefined);
        if !matches!(&input, Value::String(_)) {
            self.throw_type_error(ctx, "Uint8Array.prototype.setFromHex: first argument must be a string");
            return Value::Undefined;
        }
        let input_str = value_to_string(&input);
        // Detached check AFTER validation
        if !self.check_uint8array_not_detached(ctx, this) {
            return Value::Undefined;
        }
        let target_len = match this {
            Value::Array(arr) => arr.borrow().elements.len(),
            _ => 0,
        };
        let (bytes, read, error) = Self::hex_decode_core(&input_str, Some(target_len));
        let written = bytes.len();
        self.write_bytes_to_uint8array(ctx, this, &bytes);
        if let Some(msg) = error {
            self.throw_syntax_error(ctx, &msg);
            return Value::Undefined;
        }
        let mut result_map = IndexMap::new();
        result_map.insert("read".to_string(), Value::Number(read as f64));
        result_map.insert("written".to_string(), Value::Number(written as f64));
        Value::Object(new_gc_cell_ptr(ctx, result_map))
    }
}
