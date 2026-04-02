use super::*;

/// Coerce a numeric value according to the TypedArray element type.
pub(crate) fn coerce_typed_array_value(n: f64, ta_name: &str) -> f64 {
    if n.is_nan() || !n.is_finite() || n == 0.0 {
        match ta_name {
            "Float32Array" | "Float64Array" => return if ta_name == "Float64Array" { n } else { (n as f32) as f64 },
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
        "Float32Array" => (n as f32) as f64,
        _ => n,
    }
}

impl<'gc> VM<'gc> {
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
                    Value::VmArray(arr) => {
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
                    Value::VmArray(arr) => {
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
                        if let Some(Value::VmObject(buf)) = a.props.get("__typedarray_buffer__")
                            && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
                        {
                            return Value::Number(0.0);
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
                    Value::VmArray(arr) => {
                        let a = arr.borrow();
                        if a.props.get("__typedarray_name__").is_none() {
                            self.throw_type_error(ctx, "get TypedArray.prototype.byteOffset called on incompatible receiver");
                            return Value::Undefined;
                        }
                        // Check for detached buffer
                        if let Some(Value::VmObject(buf)) = a.props.get("__typedarray_buffer__")
                            && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
                        {
                            return Value::Number(0.0);
                        }
                        a.props.get("__byte_offset__").cloned().unwrap_or(Value::Number(0.0))
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
                    Value::VmArray(arr) => {
                        let a = arr.borrow();
                        if a.props.get("__typedarray_name__").is_none() {
                            self.throw_type_error(ctx, "get TypedArray.prototype.length called on incompatible receiver");
                            return Value::Undefined;
                        }
                        // Check for detached buffer
                        if let Some(Value::VmObject(buf)) = a.props.get("__typedarray_buffer__")
                            && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
                        {
                            return Value::Number(0.0);
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
                    Value::VmArray(arr) => {
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

                let Value::VmArray(target_arr) = &this_val else {
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
                    Value::VmArray(src) => src.borrow().props.contains_key("__typedarray_name__"),
                    _ => false,
                };

                if source_is_ta {
                    // TypedArray source path
                    let Value::VmArray(src_arr) = &source else { unreachable!() };

                    // Check if source buffer is detached (step 12)
                    {
                        let s = src_arr.borrow();
                        if let Some(Value::VmObject(buf)) = s.props.get("__typedarray_buffer__")
                            && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
                        {
                            drop(s);
                            self.throw_type_error(ctx, "Cannot perform %TypedArray%.prototype.set - source buffer is detached");
                            return Value::Undefined;
                        }
                    }

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
                        if let Some(Value::VmObject(buf)) = s.props.get("__typedarray_buffer__") {
                            if let Some(Value::VmArray(bb)) = buf.borrow().get("__buffer_bytes__").cloned() {
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
                    for i in 0..src_len {
                        let src_base = src_byte_offset + i * src_bpe;
                        let val = Self::decode_typed_element(&src_bytes, src_base, src_bpe, &src_name);
                        let num = to_number(&val);
                        // Store in target elements
                        let target_idx = offset + i;
                        let converted = Self::typed_array_coerce_value(num, &ta_name);
                        {
                            let mut t = target_arr.borrow_mut(ctx);
                            t.elements[target_idx] = Value::Number(converted);
                        }
                        self.sync_ta_element_to_buffer(ctx, target_arr, target_idx, num, &ta_name);
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
                            for i in 0..len {
                                let v = self.read_named_property(ctx, &source, &i.to_string());
                                if self.pending_throw.is_some() {
                                    return Value::Undefined;
                                }
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
                            return Value::Undefined;
                        }
                        Value::Undefined | Value::Null => {
                            self.throw_type_error(ctx, "Cannot convert undefined or null to object");
                            return Value::Undefined;
                        }
                        _ => source.clone(),
                    };

                    // Get length from source
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

                    let target_len = target_arr.borrow().elements.len();
                    if offset + src_len > target_len {
                        self.throw_range_error_object(ctx, "offset is out of bounds");
                        return Value::Undefined;
                    }

                    for i in 0..src_len {
                        let v = self.read_named_property(ctx, &src_obj, &i.to_string());
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
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
                Value::Undefined
            }
            "typedarray.subarray" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                // Don't validate detached buffer here - spec says subarray coerces args first
                let Value::VmArray(arr) = &this_val else {
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
                drop(a);
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
                let end_raw = match args.get(1) {
                    Some(Value::Undefined) | None => len as i64,
                    Some(v) => match self.extract_number_with_coercion(ctx, v) {
                        Some(n) if n.is_nan() => 0,
                        Some(n) => n as i64,
                        None => return Value::Undefined,
                    },
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
                // Use TypedArraySpeciesCreate
                if let Some(buf_val) = buffer {
                    let Some(ctor) = self.typed_array_species_constructor(ctx, &this_val) else {
                        return Value::Undefined;
                    };
                    let result = self.construct_value(
                        ctx,
                        &ctor,
                        &[buf_val, Value::Number(new_byte_offset as f64), Value::Number(count as f64)],
                        None,
                    );
                    match result {
                        Ok(v) => v,
                        Err(e) => {
                            let msg = e.message();
                            self.throw_type_error(ctx, &msg);
                            Value::Undefined
                        }
                    }
                } else {
                    // No buffer, just slice elements
                    let a = arr.borrow();
                    let elems: Vec<Value<'gc>> = a.elements[begin..end].to_vec();
                    drop(a);
                    let mut data = VmArrayData::new(elems);
                    data.props.insert("__typedarray_name__".to_string(), Value::from(ta_name.as_str()));
                    data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
                    Value::VmArray(new_gc_cell_ptr(ctx, data))
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
                    Value::VmArray(src) => src.borrow().elements.clone(),
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
                            loop {
                                let next_method = self.read_named_property(ctx, &iterator, "next");
                                if self.pending_throw.is_some() {
                                    return Value::Undefined;
                                }
                                let result = self.vm_call_function_value(ctx, &next_method, &iterator, &[]);
                                if let Err(e) = result {
                                    self.set_pending_throw_from_error(&e);
                                    return Value::Undefined;
                                }
                                let result = result.unwrap();
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
                    Ok(ta @ Value::VmArray(_)) => {
                        if !self.validate_typed_array(ctx, &ta, "from") {
                            return Value::Undefined;
                        }
                        let ta_len = if let Value::VmArray(a) = &ta {
                            a.borrow().elements.len()
                        } else {
                            0
                        };
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
                    Ok(ta @ Value::VmArray(_)) => {
                        if !self.validate_typed_array(ctx, &ta, "of") {
                            return Value::Undefined;
                        }
                        // TypedArrayCreate: verify length >= required
                        let ta_len = if let Value::VmArray(a) = &ta {
                            a.borrow().elements.len()
                        } else {
                            0
                        };
                        if ta_len < len {
                            self.throw_type_error(ctx, "TypedArray is too small");
                            return Value::Undefined;
                        }
                        for (i, v) in args.iter().enumerate() {
                            if Self::is_symbol_value(v) {
                                self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                                return Value::Undefined;
                            }
                            let key = i.to_string();
                            if let Err(e) = self.assign_named_property(ctx, &ta, &key, v, None) {
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
                // Create a "value" iterator directly
                let mut obj = IndexMap::new();
                obj.insert("__iter_target__".to_string(), this_val);
                obj.insert("__index__".to_string(), Value::Number(0.0));
                obj.insert("__iter_kind__".to_string(), Value::from("value"));
                obj.insert("@@sym:1".to_string(), Self::make_host_fn(ctx, "iterator.self"));
                if let Some(proto) = self.globals.get("__ArrayIteratorPrototype__").cloned() {
                    obj.insert("__proto__".to_string(), proto);
                }
                Value::VmObject(new_gc_cell_ptr(ctx, obj))
            }
            "typedarray.entries" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "entries") {
                    return Value::Undefined;
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
                Value::VmObject(new_gc_cell_ptr(ctx, obj))
            }
            "typedarray.keys_iter" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "keys") {
                    return Value::Undefined;
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
                Value::VmObject(new_gc_cell_ptr(ctx, obj))
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
                self.call_method_builtin(ctx, builtin_id, this_val, args)
            }
            "typedarray.fill" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "fill") {
                    return Value::Undefined;
                }
                let Value::VmArray(arr) = &this_val else {
                    return Value::Undefined;
                };
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

                // Convert fill value to Number (ToNumber with coercion)
                let fill_val = args.first().cloned().unwrap_or(Value::Undefined);
                let num = match self.extract_number_with_coercion(ctx, &fill_val) {
                    Some(n) => n,
                    None => return Value::Undefined,
                };
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }

                // Re-validate after coercion (buffer may have been detached)
                if !self.validate_typed_array(ctx, &this_val, "fill") {
                    return Value::Undefined;
                }

                let len = arr.borrow().elements.len();
                let converted = Self::typed_array_coerce_value(num, &ta_name);

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
                    {
                        let mut a = arr.borrow_mut(ctx);
                        a.elements[i] = Value::Number(converted);
                    }
                    self.sync_ta_element_to_buffer(ctx, arr, i, num, &ta_name);
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
                let Value::VmArray(arr) = &this_val else {
                    return Value::Undefined;
                };

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

                let mut elements: Vec<f64> = {
                    let a = arr.borrow();
                    a.elements.iter().map(|v| to_number(v)).collect()
                };

                let has_custom = matches!(&comparefn, Some(v) if !matches!(v, Value::Undefined));

                if has_custom {
                    let cmp_fn = comparefn.unwrap();
                    let mut had_error = false;
                    elements.sort_by(|a, b| {
                        if had_error {
                            return std::cmp::Ordering::Equal;
                        }
                        let result = self.vm_call_function_value(ctx, &cmp_fn, &Value::Undefined, &[Value::Number(*a), Value::Number(*b)]);
                        match result {
                            Ok(v) => {
                                // ToNumber coercion of result
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

                // Write back sorted elements and sync to buffer
                {
                    let mut a = arr.borrow_mut(ctx);
                    for (i, &num) in elements.iter().enumerate() {
                        let converted = Self::typed_array_coerce_value(num, &ta_name);
                        a.elements[i] = Value::Number(converted);
                    }
                }
                for (i, &num) in elements.iter().enumerate() {
                    self.sync_ta_element_to_buffer(ctx, arr, i, num, &ta_name);
                }
                this_val
            }
            "typedarray.reverse" => {
                let this_val = receiver.unwrap_or(&Value::Undefined).clone();
                if !self.validate_typed_array(ctx, &this_val, "reverse") {
                    return Value::Undefined;
                }
                let Value::VmArray(arr) = &this_val else {
                    return Value::Undefined;
                };
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
                let len = if let Value::VmArray(arr) = &this_val {
                    arr.borrow().elements.len()
                } else {
                    return Value::Undefined;
                };
                // Per spec: TypedArraySpeciesCreate BEFORE iteration
                let Some(result) = self.typed_array_species_create(ctx, &this_val, &[Value::Number(len as f64)]) else {
                    return Value::Undefined;
                };
                let res_ta_name = if let Value::VmArray(res_arr) = &result {
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
                    let k_value = if let Value::VmArray(arr) = &this_val {
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
                    if let Value::VmArray(res_arr) = &result {
                        let num = to_number(&mapped);
                        let coerced = Value::Number(Self::typed_array_coerce_value(num, &res_ta_name));
                        if k < res_arr.borrow().elements.len() {
                            res_arr.borrow_mut(ctx).elements[k] = coerced;
                        }
                        self.sync_ta_element_to_buffer(ctx, res_arr, k, num, &res_ta_name);
                    }
                }
                result
            }
            "typedarray.filter" => {
                let this_val = receiver.unwrap_or(&Value::Undefined);
                if !self.validate_typed_array(ctx, this_val, "filter") {
                    return Value::Undefined;
                }
                let filtered = self.call_method_builtin(ctx, BUILTIN_ARRAY_FILTER, this_val, args);
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                // Extract filtered elements
                let elements = match &filtered {
                    Value::VmArray(arr) => arr.borrow().elements.clone(),
                    _ => return filtered,
                };
                let len = elements.len();
                // Use species constructor
                let Some(result) = self.typed_array_species_create(ctx, this_val, &[Value::Number(len as f64)]) else {
                    return Value::Undefined;
                };
                // Copy filtered values into result
                if let Value::VmArray(res_arr) = &result {
                    let ta_name = res_arr
                        .borrow()
                        .props
                        .get("__typedarray_name__")
                        .map(value_to_string)
                        .unwrap_or_default();
                    for (i, v) in elements.iter().enumerate() {
                        let num = to_number(v);
                        let coerced = Value::Number(Self::typed_array_coerce_value(num, &ta_name));
                        if i < res_arr.borrow().elements.len() {
                            res_arr.borrow_mut(ctx).elements[i] = coerced.clone();
                        }
                        self.sync_ta_element_to_buffer(ctx, res_arr, i, num, &ta_name);
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
                let (len, ta_name, bpe) = if let Value::VmArray(arr) = &this_val {
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

                // Check if source buffer is detached after species create
                if let Value::VmArray(src_arr) = &this_val
                    && let Some(Value::VmObject(buf)) = src_arr.borrow().props.get("__typedarray_buffer__")
                    && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
                {
                    self.throw_type_error(ctx, "Cannot perform operation on a detached ArrayBuffer");
                    return Value::Undefined;
                }

                if let Value::VmArray(res_arr) = &result {
                    let res_ta_name = res_arr
                        .borrow()
                        .props
                        .get("__typedarray_name__")
                        .map(value_to_string)
                        .unwrap_or_default();
                    let src_buf = if let Value::VmArray(src_arr) = &this_val {
                        src_arr.borrow().props.get("__typedarray_buffer__").cloned()
                    } else {
                        None
                    };
                    let res_buf = res_arr.borrow().props.get("__typedarray_buffer__").cloned();

                    // Check if same buffer and same element type
                    let same_buffer = match (&src_buf, &res_buf) {
                        (Some(Value::VmObject(a)), Some(Value::VmObject(b))) => Gc::ptr_eq(*a, *b),
                        _ => false,
                    };

                    if same_buffer && ta_name == res_ta_name {
                        // Same buffer, same type: byte-by-byte copy (spec deliberately does NOT
                        // handle overlap, so we copy directly without an intermediate buffer)
                        let src_byte_offset = if let Value::VmArray(src_arr) = &this_val {
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

                        if let Some(Value::VmObject(buf_obj)) = &src_buf {
                            let src_start_byte = src_byte_offset + k as usize * bpe;
                            let target_start_byte = res_byte_offset;
                            if let Some(Value::VmArray(buf_bytes)) = buf_obj.borrow().get("__buffer_bytes__").cloned() {
                                let mut bb = buf_bytes.borrow_mut(ctx);
                                for i in 0..(count * bpe) {
                                    let src_idx = src_start_byte + i;
                                    let tgt_idx = target_start_byte + i;
                                    if src_idx < bb.elements.len() && tgt_idx < bb.elements.len() {
                                        let byte_val = bb.elements[src_idx].clone();
                                        bb.elements[tgt_idx] = byte_val;
                                    }
                                }
                                drop(bb);
                                // Sync elements from buffer
                                self.sync_ta_elements_from_buffer(ctx, res_arr, &res_ta_name, bpe, count);
                            }
                        }
                    } else {
                        // Different buffer or different type: element-by-element set
                        let src_elements: Vec<Value<'gc>> = if let Value::VmArray(src_arr) = &this_val {
                            let a = src_arr.borrow();
                            let start = k as usize;
                            let end = (k as usize + count).min(a.elements.len());
                            a.elements[start..end].to_vec()
                        } else {
                            vec![]
                        };
                        for (i, v) in src_elements.iter().enumerate() {
                            let num = to_number(v);
                            let coerced = Value::Number(Self::typed_array_coerce_value(num, &res_ta_name));
                            if i < res_arr.borrow().elements.len() {
                                res_arr.borrow_mut(ctx).elements[i] = coerced;
                            }
                            self.sync_ta_element_to_buffer(ctx, res_arr, i, num, &res_ta_name);
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
                let Value::VmArray(arr) = &this_val else {
                    return Value::Undefined;
                };
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

                // Check for detached buffer AFTER argument coercion
                {
                    let a = arr.borrow();
                    if let Some(Value::VmObject(buf)) = a.props.get("__typedarray_buffer__")
                        && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
                    {
                        drop(a);
                        self.throw_type_error(ctx, "Cannot perform %TypedArray%.prototype.copyWithin on a detached ArrayBuffer");
                        return Value::Undefined;
                    }
                }

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
                    if let Some(Value::VmObject(buf)) = a.props.get("__typedarray_buffer__")
                        && let Some(Value::VmArray(buf_bytes)) = buf.borrow().get("__buffer_bytes__").cloned()
                    {
                        let byte_offset = match a.props.get("__byte_offset__") {
                            Some(Value::Number(n)) => *n as usize,
                            _ => 0,
                        };
                        drop(a);
                        let mut bb = buf_bytes.borrow_mut(ctx);
                        let arr_borrow = arr.borrow();
                        for i in 0..arr_borrow.elements.len() {
                            let num = to_number(&arr_borrow.elements[i]);
                            Self::encode_typed_element(&mut bb.elements, byte_offset + i * bpe, bpe, &ta_name, num);
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
                self.call_host_fn(ctx, "array.toLocaleString", Some(this_val), args)
            }
            _ => Value::Undefined,
        }
    }

    /// Validate that a value is a TypedArray with a non-detached buffer.
    /// Returns true if valid. Sets pending_throw and returns false otherwise.
    fn validate_typed_array(&mut self, ctx: &GcContext<'gc>, val: &Value<'gc>, method: &str) -> bool {
        match val {
            Value::VmArray(arr) => {
                let a = arr.borrow();
                if !a.props.contains_key("__typedarray_name__") {
                    drop(a);
                    self.throw_type_error(ctx, &format!("%TypedArray%.prototype.{} called on incompatible receiver", method));
                    return false;
                }
                // Check for detached buffer
                if let Some(Value::VmObject(buf)) = a.props.get("__typedarray_buffer__")
                    && matches!(buf.borrow().get("__detached__"), Some(Value::Boolean(true)))
                {
                    drop(a);
                    self.throw_type_error(ctx, "Cannot perform operation on a detached ArrayBuffer");
                    return false;
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
        let ta_name = if let Value::VmArray(arr) = this_val {
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
            Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_)
        ) || Self::is_symbol_value(&ctor)
        {
            self.throw_type_error(ctx, "Constructor is not an object");
            return None;
        }

        // Step 5: Let S be ? Get(C, @@species)
        let mut species_ctor = ctor.clone();
        if let Some(Value::VmObject(symbol_ctor)) = self.globals.get("Symbol")
            && let Some(species_symbol) = symbol_ctor.borrow().get("species").cloned()
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

    /// TypedArraySpeciesCreate: use species constructor to create result,
    /// falling back to wrap_as_typed_array for default constructors.
    fn typed_array_species_create(&mut self, ctx: &GcContext<'gc>, this_val: &Value<'gc>, args: &[Value<'gc>]) -> Option<Value<'gc>> {
        let ctor = self.typed_array_species_constructor(ctx, this_val)?;

        // Check if ctor is the default constructor for this TypedArray type
        let ta_name = if let Value::VmArray(arr) = this_val {
            arr.borrow()
                .props
                .get("__typedarray_name__")
                .map(value_to_string)
                .unwrap_or_default()
        } else {
            String::new()
        };
        let default_ctor = self.globals.get(&ta_name).cloned().unwrap_or(Value::Undefined);
        let is_default = self.same_constructor_identity(&ctor, &default_ctor);

        if is_default {
            // Use the default constructor
            match self.construct_value(ctx, &ctor, args, None) {
                Ok(v) => Some(v),
                Err(e) => {
                    self.set_pending_throw_from_error(&e);
                    None
                }
            }
        } else {
            // Species constructor — call it as a function (it might not be a native ctor)
            match self.construct_value(ctx, &ctor, args, None) {
                Ok(v) => Some(v),
                Err(_) => {
                    // Fall back to calling as a function
                    match self.vm_call_function_value(ctx, &ctor, &Value::Undefined, args) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            self.set_pending_throw_from_error(&e);
                            None
                        }
                    }
                }
            }
        }
    }

    /// Wrap a plain Array result as the same TypedArray type as `source`.
    fn _wrap_as_typed_array(&mut self, ctx: &GcContext<'gc>, source: &Value<'gc>, result: &Value<'gc>) -> Value<'gc> {
        let (ta_name, bpe) = if let Value::VmArray(arr) = source {
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
            _ => return result.clone(),
        };
        // Extract elements from result
        let elements = match result {
            Value::VmArray(arr) => arr.borrow().elements.clone(),
            _ => return result.clone(),
        };
        let len = elements.len();
        // Create a new TypedArray with the same type
        let ta_instance_proto = self
            .globals
            .get(&ta_name)
            .and_then(|v| {
                if let Value::VmObject(o) = v {
                    Some(o.borrow().get("prototype").cloned())
                } else {
                    None
                }
            })
            .flatten();
        let mut data = VmArrayData::new(elements.clone());
        data.props.insert("__typedarray_name__".to_string(), Value::from(&ta_name));
        data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
        // Create backing ArrayBuffer
        let mut buf_map = IndexMap::new();
        buf_map.insert("__type__".to_string(), Value::from("ArrayBuffer"));
        buf_map.insert("byteLength".to_string(), Value::Number((len * bpe) as f64));
        let mut bytes = vec![Value::Number(0.0); len * bpe];
        for i in 0..len {
            let num = to_number(elements.get(i).unwrap_or(&Value::Number(0.0)));
            Self::encode_typed_element(&mut bytes, i * bpe, bpe, &ta_name, num);
        }
        buf_map.insert(
            "__buffer_bytes__".to_string(),
            Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(bytes))),
        );
        let buffer_obj = Value::VmObject(new_gc_cell_ptr(ctx, buf_map));
        data.props.insert("buffer".to_string(), buffer_obj.clone());
        data.props.insert("__nonenumerable_buffer__".to_string(), Value::Boolean(true));
        data.props.insert("__typedarray_buffer__".to_string(), buffer_obj);
        data.props.insert("__byte_offset__".to_string(), Value::Number(0.0));
        data.props.insert("__bytes_per_element__".to_string(), Value::Number(bpe as f64));
        if let Some(proto) = &ta_instance_proto {
            data.props.insert("__proto__".to_string(), proto.clone());
        }
        Value::VmArray(new_gc_cell_ptr(ctx, data))
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
            "Float32Array" => {
                let arr4: [u8; 4] = core::array::from_fn(|j| to_number(bytes.get(base + j).unwrap_or(&Value::Number(0.0))) as u8);
                Value::Number(f32::from_ne_bytes(arr4) as f64)
            }
            "Float64Array" => {
                let arr8: [u8; 8] = core::array::from_fn(|j| to_number(bytes.get(base + j).unwrap_or(&Value::Number(0.0))) as u8);
                Value::Number(f64::from_ne_bytes(arr8))
            }
            _ => {
                let b = to_number(bytes.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                Value::Number(b as f64)
            }
        }
    }

    /// Sync a TypedArray element write to the underlying buffer bytes.
    pub(super) fn sync_ta_element_to_buffer(
        &self,
        ctx: &GcContext<'gc>,
        arr: &VmArrayHandle<'gc>,
        idx: usize,
        new_num: f64,
        ta_name: &str,
    ) {
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
        if let Some(Value::VmObject(buf_obj)) = buffer
            && let Some(Value::VmArray(buf_bytes)) = buf_obj.borrow().get("__buffer_bytes__").cloned()
        {
            let base = byte_offset + idx * bpe;
            let mut bb = buf_bytes.borrow_mut(ctx);
            match ta_name {
                "Uint8Array" | "Uint8ClampedArray" => {
                    if base < bb.elements.len() {
                        let v = if ta_name == "Uint8ClampedArray" {
                            Self::to_uint8_clamp(new_num)
                        } else {
                            Self::to_uint8(new_num)
                        };
                        bb.elements[base] = Value::Number(v as f64);
                    }
                }
                "Int8Array" => {
                    if base < bb.elements.len() {
                        bb.elements[base] = Value::Number((Self::to_int8(new_num) as u8) as f64);
                    }
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
                _ => {}
            }
        }
    }

    /// Sync elements array from the backing buffer bytes (for shared buffer scenarios)
    fn sync_ta_elements_from_buffer(&self, ctx: &GcContext<'gc>, arr: &VmArrayHandle<'gc>, ta_name: &str, bpe: usize, len: usize) {
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
        if let Some(Value::VmObject(buf_obj)) = buffer
            && let Some(Value::VmArray(buf_bytes)) = buf_obj.borrow().get("__buffer_bytes__").cloned()
        {
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

    /// Convert a numeric value to the type-specific element value for a TypedArray.
    pub(super) fn typed_array_coerce_value(num: f64, ta_name: &str) -> f64 {
        match ta_name {
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
            BUILTIN_CTOR_INT16ARRAY | BUILTIN_CTOR_UINT16ARRAY => 2usize,
            BUILTIN_CTOR_INT32ARRAY | BUILTIN_CTOR_UINT32ARRAY | BUILTIN_CTOR_FLOAT32ARRAY => 4usize,
            BUILTIN_CTOR_FLOAT64ARRAY => 8usize,
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
            BUILTIN_CTOR_FLOAT32ARRAY => "Float32Array",
            BUILTIN_CTOR_FLOAT64ARRAY => "Float64Array",
            _ => "TypedArray",
        };

        // Get prototype from constructor for __proto__ on instances
        let ta_instance_proto = self
            .globals
            .get(typedarray_name)
            .and_then(|v| {
                if let Value::VmObject(o) = v {
                    Some(o.borrow().get("prototype").cloned())
                } else {
                    None
                }
            })
            .flatten();

        if let Some(Value::VmArray(src_arr)) = args.first()
            && src_arr.borrow().props.contains_key("__typedarray_name__")
        {
            let elements_clone: Vec<Value<'gc>> = src_arr.borrow().elements.clone();
            let len = elements_clone.len();
            // Coerce each source element to the typed numeric representation
            let mut coerced_elements = Vec::with_capacity(len);
            let mut numeric_vals = Vec::with_capacity(len);
            for elements_clone_i in elements_clone.iter().take(len) {
                let v = elements_clone_i;
                let num = match v {
                    Value::VmObject(_) | Value::VmArray(_) => match self.extract_number_with_coercion(ctx, v) {
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
            let mut data = VmArrayData::new(coerced_elements);
            data.props.insert("__typedarray_name__".to_string(), Value::from(typedarray_name));
            data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
            // Create backing ArrayBuffer with properly encoded bytes
            let mut buf_map = IndexMap::new();
            buf_map.insert("__type__".to_string(), Value::from("ArrayBuffer"));
            buf_map.insert("byteLength".to_string(), Value::Number((len * bytes_per_element) as f64));
            let mut bytes = vec![Value::Number(0.0); len * bytes_per_element];
            for (i, &numeric_vals_i) in numeric_vals.iter().enumerate().take(len) {
                let num = numeric_vals_i;
                Self::encode_typed_element(&mut bytes, i * bytes_per_element, bytes_per_element, typedarray_name, num);
            }
            buf_map.insert(
                "__buffer_bytes__".to_string(),
                Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(bytes))),
            );
            let buffer_obj = Value::VmObject(new_gc_cell_ptr(ctx, buf_map));
            data.props.insert("buffer".to_string(), buffer_obj.clone());
            data.props.insert("__nonenumerable_buffer__".to_string(), Value::Boolean(true));
            data.props.insert("__typedarray_buffer__".to_string(), buffer_obj);
            data.props.insert("__byte_offset__".to_string(), Value::Number(0.0));
            data.props
                .insert("__bytes_per_element__".to_string(), Value::Number(bytes_per_element as f64));
            if let Some(proto) = &ta_instance_proto {
                data.props.insert("__proto__".to_string(), proto.clone());
            }
            return Value::VmArray(new_gc_cell_ptr(ctx, data));
        }

        // Regular VmArray (non-TypedArray) — use iterator protocol like VmObject
        if let Some(Value::VmArray(_)) = args.first() {
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
            let mut numeric_vals = Vec::with_capacity(len);
            for elem in elements.iter() {
                if Self::is_symbol_value(elem) {
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
            let mut data = VmArrayData::new(coerced_elements);
            data.props.insert("__typedarray_name__".to_string(), Value::from(typedarray_name));
            data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
            let mut buf_map = IndexMap::new();
            buf_map.insert("__type__".to_string(), Value::from("ArrayBuffer"));
            buf_map.insert("byteLength".to_string(), Value::Number((len * bytes_per_element) as f64));
            let mut bytes = vec![Value::Number(0.0); len * bytes_per_element];
            for (i, &num) in numeric_vals.iter().enumerate() {
                Self::encode_typed_element(&mut bytes, i * bytes_per_element, bytes_per_element, typedarray_name, num);
            }
            buf_map.insert(
                "__buffer_bytes__".to_string(),
                Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(bytes))),
            );
            let buffer_obj = Value::VmObject(new_gc_cell_ptr(ctx, buf_map));
            data.props.insert("buffer".to_string(), buffer_obj.clone());
            data.props.insert("__nonenumerable_buffer__".to_string(), Value::Boolean(true));
            data.props.insert("__typedarray_buffer__".to_string(), buffer_obj);
            data.props.insert("__byte_offset__".to_string(), Value::Number(0.0));
            data.props
                .insert("__bytes_per_element__".to_string(), Value::Number(bytes_per_element as f64));
            if let Some(proto) = &ta_instance_proto {
                data.props.insert("__proto__".to_string(), proto.clone());
            }
            return Value::VmArray(new_gc_cell_ptr(ctx, data));
        }

        // Symbol check: must reject Symbols before they reach the VmObject path
        if let Some(first) = args.first()
            && Self::is_symbol_value(first)
        {
            self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
            return Value::Undefined;
        }

        if let Some(Value::VmObject(buf_obj)) = args.first() {
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
                } else if Self::is_symbol_value(&raw_offset) {
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
                } else if Self::is_symbol_value(&raw_len) {
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

                let initial_len;
                if let Some(len) = explicit_len {
                    // Explicit length: check it doesn't exceed buffer
                    let needed = byte_offset + len * bytes_per_element;
                    if needed > byte_len {
                        self.throw_range_error_object(ctx, "Invalid typed array length");
                        return Value::Undefined;
                    }
                    initial_len = len;
                } else {
                    // No explicit length: buffer must be aligned
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
                let elements = if let Some(Value::VmArray(buf_bytes)) = buf_obj.borrow().get("__buffer_bytes__").cloned() {
                    let bb = buf_bytes.borrow();
                    let mut elems = Vec::with_capacity(initial_len);
                    for i in 0..initial_len {
                        let base = byte_offset + i * bytes_per_element;
                        let val = Self::decode_typed_element(&bb.elements, base, bytes_per_element, typedarray_name);
                        elems.push(val);
                    }
                    elems
                } else {
                    vec![Value::Number(0.0); initial_len]
                };
                let mut data = VmArrayData::new(elements);
                data.props.insert("__typedarray_name__".to_string(), Value::from(typedarray_name));
                data.props.insert("__buffer_type__".to_string(), Value::from(&buffer_type));
                data.props.insert("buffer".to_string(), Value::VmObject(*buf_obj));
                data.props.insert("__nonenumerable_buffer__".to_string(), Value::Boolean(true));
                data.props.insert("__byte_offset__".to_string(), Value::Number(byte_offset as f64));
                data.props
                    .insert("__bytes_per_element__".to_string(), Value::Number(bytes_per_element as f64));
                data.props
                    .insert("__length_tracking__".to_string(), Value::Boolean(explicit_len.is_none()));
                if let Some(len) = explicit_len {
                    data.props.insert("__fixed_length__".to_string(), Value::Number(len as f64));
                }
                data.props.insert("__typedarray_buffer__".to_string(), Value::VmObject(*buf_obj));
                if let Some(proto) = &ta_instance_proto {
                    data.props.insert("__proto__".to_string(), proto.clone());
                }
                return Value::VmArray(new_gc_cell_ptr(ctx, data));
            }
            // Check for Symbol.iterator first (iterable object)
            let obj_val = Value::VmObject(*buf_obj);
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
                if Self::is_symbol_value(&len_val) {
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
            let mut numeric_vals = Vec::with_capacity(len);
            for elem in elements.iter() {
                if Self::is_symbol_value(elem) {
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
            let mut data = VmArrayData::new(coerced_elements);
            data.props.insert("__typedarray_name__".to_string(), Value::from(typedarray_name));
            data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
            // Create backing ArrayBuffer with properly encoded bytes
            let mut buf_map = IndexMap::new();
            buf_map.insert("__type__".to_string(), Value::from("ArrayBuffer"));
            buf_map.insert("byteLength".to_string(), Value::Number((len * bytes_per_element) as f64));
            let mut bytes = vec![Value::Number(0.0); len * bytes_per_element];
            for (i, &num) in numeric_vals.iter().enumerate() {
                Self::encode_typed_element(&mut bytes, i * bytes_per_element, bytes_per_element, typedarray_name, num);
            }
            buf_map.insert(
                "__buffer_bytes__".to_string(),
                Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(bytes))),
            );
            let buffer_obj = Value::VmObject(new_gc_cell_ptr(ctx, buf_map));
            data.props.insert("buffer".to_string(), buffer_obj.clone());
            data.props.insert("__nonenumerable_buffer__".to_string(), Value::Boolean(true));
            data.props.insert("__typedarray_buffer__".to_string(), buffer_obj);
            data.props.insert("__byte_offset__".to_string(), Value::Number(0.0));
            data.props
                .insert("__bytes_per_element__".to_string(), Value::Number(bytes_per_element as f64));
            if let Some(proto) = &ta_instance_proto {
                data.props.insert("__proto__".to_string(), proto.clone());
            }
            return Value::VmArray(new_gc_cell_ptr(ctx, data));
        }

        // Object-arg catch-all: functions and other object-like values use iterator protocol
        if let Some(first) = args.first() {
            let is_object_like = matches!(first, Value::VmClosure(..) | Value::VmFunction(..) | Value::VmNativeFunction(_));
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
                let mut numeric_vals = Vec::with_capacity(len);
                for elem in elements.iter() {
                    if Self::is_symbol_value(elem) {
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
                let mut data = VmArrayData::new(coerced_elements);
                data.props.insert("__typedarray_name__".to_string(), Value::from(typedarray_name));
                data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
                let mut buf_map = IndexMap::new();
                buf_map.insert("__type__".to_string(), Value::from("ArrayBuffer"));
                buf_map.insert("byteLength".to_string(), Value::Number((len * bytes_per_element) as f64));
                let mut bytes = vec![Value::Number(0.0); len * bytes_per_element];
                for (i, &num) in numeric_vals.iter().enumerate() {
                    Self::encode_typed_element(&mut bytes, i * bytes_per_element, bytes_per_element, typedarray_name, num);
                }
                buf_map.insert(
                    "__buffer_bytes__".to_string(),
                    Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(bytes))),
                );
                let buffer_obj = Value::VmObject(new_gc_cell_ptr(ctx, buf_map));
                data.props.insert("buffer".to_string(), buffer_obj.clone());
                data.props.insert("__nonenumerable_buffer__".to_string(), Value::Boolean(true));
                data.props.insert("__typedarray_buffer__".to_string(), buffer_obj);
                data.props.insert("__byte_offset__".to_string(), Value::Number(0.0));
                data.props
                    .insert("__bytes_per_element__".to_string(), Value::Number(bytes_per_element as f64));
                if let Some(proto) = &ta_instance_proto {
                    data.props.insert("__proto__".to_string(), proto.clone());
                }
                return Value::VmArray(new_gc_cell_ptr(ctx, data));
            }
        }

        // Length-arg or no-arg path
        let first_arg = args.first().cloned().unwrap_or(Value::Undefined);
        // Check for Symbol
        if Self::is_symbol_value(&first_arg) {
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
        let mut data = VmArrayData::new(vec![Value::Number(0.0); length]);
        data.props.insert("__typedarray_name__".to_string(), Value::from(typedarray_name));
        data.props.insert("__buffer_type__".to_string(), Value::from("ArrayBuffer"));
        let mut buf_map = IndexMap::new();
        buf_map.insert("__type__".to_string(), Value::from("ArrayBuffer"));
        buf_map.insert("byteLength".to_string(), Value::Number((length * bytes_per_element) as f64));
        let bytes = vec![Value::Number(0.0); length * bytes_per_element];
        buf_map.insert(
            "__buffer_bytes__".to_string(),
            Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(bytes))),
        );
        let buffer_obj = Value::VmObject(new_gc_cell_ptr(ctx, buf_map));
        data.props.insert("buffer".to_string(), buffer_obj.clone());
        data.props.insert("__nonenumerable_buffer__".to_string(), Value::Boolean(true));
        data.props.insert("__typedarray_buffer__".to_string(), buffer_obj);
        data.props.insert("__byte_offset__".to_string(), Value::Number(0.0));
        data.props
            .insert("__bytes_per_element__".to_string(), Value::Number(bytes_per_element as f64));
        data.props.insert("__fixed_length__".to_string(), Value::Number(length as f64));
        data.props.insert("__length_tracking__".to_string(), Value::Boolean(false));
        if let Some(proto) = &ta_instance_proto {
            data.props.insert("__proto__".to_string(), proto.clone());
        }
        Value::VmArray(new_gc_cell_ptr(ctx, data))
    }

    pub(super) fn initialize_typed_arrays(&mut self, ctx: &GcContext<'gc>, array_to_string_fn_for_ta: Value<'gc>) {
        // ── %TypedArray% intrinsic and shared prototype ──
        // Spec: %TypedArray%.prototype holds all shared TypedArray methods.
        // Chain: instance.__proto__ → XxxArray.prototype → %TypedArray%.prototype → Object.prototype
        //        XxxArray.__proto__ → %TypedArray% → Function.prototype
        let mut ta_proto_map = IndexMap::new();
        if let Some(Value::VmObject(obj_ctor)) = self.globals.get("Object")
            && let Some(obj_proto) = obj_ctor.borrow().get("prototype").cloned()
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
        ta_proto_map.insert(
            "__get_buffer".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.get_buffer", "get buffer", 0.0, false),
        );
        ta_proto_map.insert(
            "__get_byteLength".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.get_byteLength", "get byteLength", 0.0, false),
        );
        ta_proto_map.insert(
            "__get_byteOffset".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.get_byteOffset", "get byteOffset", 0.0, false),
        );
        ta_proto_map.insert(
            "__get_length".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.get_length", "get length", 0.0, false),
        );
        // Symbol.toStringTag getter
        ta_proto_map.insert(
            "__get_@@sym:4".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.get_toStringTag", "get [Symbol.toStringTag]", 0.0, false),
        );
        // Mark getter properties as non-enumerable and configurable
        for key in ["buffer", "byteLength", "byteOffset", "length", "@@sym:4"] {
            ta_proto_map.insert(format!("__nonenumerable_{}__", key), Value::Boolean(true));
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
        ] {
            ta_proto_map.insert(format!("__nonenumerable_{}__", key), Value::Boolean(true));
        }
        let ta_proto = Value::VmObject(new_gc_cell_ptr(ctx, ta_proto_map));

        // %TypedArray% constructor (abstract — cannot be called directly)
        let mut typed_array_ctor_map = IndexMap::new();
        typed_array_ctor_map.insert("name".to_string(), Value::from("TypedArray"));
        typed_array_ctor_map.insert("__readonly_name__".to_string(), Value::Boolean(true));
        typed_array_ctor_map.insert("__nonenumerable_name__".to_string(), Value::Boolean(true));
        typed_array_ctor_map.insert("length".to_string(), Value::Number(0.0));
        typed_array_ctor_map.insert("__readonly_length__".to_string(), Value::Boolean(true));
        typed_array_ctor_map.insert("__nonenumerable_length__".to_string(), Value::Boolean(true));
        typed_array_ctor_map.insert("prototype".to_string(), ta_proto.clone());
        typed_array_ctor_map.insert("__readonly_prototype__".to_string(), Value::Boolean(true));
        typed_array_ctor_map.insert("__nonenumerable_prototype__".to_string(), Value::Boolean(true));
        typed_array_ctor_map.insert("__nonconfigurable_prototype__".to_string(), Value::Boolean(true));
        // Mark as constructor (for is_constructor_value)
        typed_array_ctor_map.insert("__native_id__".to_string(), Value::Boolean(true));
        // TypedArray.from() and TypedArray.of() static methods
        typed_array_ctor_map.insert(
            "from".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.from", "from", 1.0, false),
        );
        typed_array_ctor_map.insert("__nonenumerable_from__".to_string(), Value::Boolean(true));
        typed_array_ctor_map.insert(
            "of".to_string(),
            Self::make_host_fn_with_name_len(ctx, "typedarray.of", "of", 0.0, false),
        );
        typed_array_ctor_map.insert("__nonenumerable_of__".to_string(), Value::Boolean(true));
        // Set constructor backref on prototype
        let typed_array_ctor = Value::VmObject(new_gc_cell_ptr(ctx, typed_array_ctor_map));
        if let Value::VmObject(p) = &ta_proto {
            p.borrow_mut(ctx).insert("constructor".to_string(), typed_array_ctor.clone());
            p.borrow_mut(ctx)
                .insert("__nonenumerable_constructor__".to_string(), Value::Boolean(true));
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
            ("Float32Array", BUILTIN_CTOR_FLOAT32ARRAY, 4.0),
            ("Float64Array", BUILTIN_CTOR_FLOAT64ARRAY, 8.0),
        ];
        for &(name, ctor_id, bpe) in ta_types {
            let mut ctor_map = IndexMap::new();
            ctor_map.insert("__native_id__".to_string(), Value::Number(ctor_id as f64));
            ctor_map.insert("name".to_string(), Value::from(name));
            ctor_map.insert("__readonly_name__".to_string(), Value::Boolean(true));
            ctor_map.insert("__nonenumerable_name__".to_string(), Value::Boolean(true));
            ctor_map.insert("length".to_string(), Value::Number(3.0));
            ctor_map.insert("__readonly_length__".to_string(), Value::Boolean(true));
            ctor_map.insert("__nonenumerable_length__".to_string(), Value::Boolean(true));
            ctor_map.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(bpe));
            ctor_map.insert("__readonly_BYTES_PER_ELEMENT__".to_string(), Value::Boolean(true));
            ctor_map.insert("__nonenumerable_BYTES_PER_ELEMENT__".to_string(), Value::Boolean(true));
            ctor_map.insert("__nonconfigurable_BYTES_PER_ELEMENT__".to_string(), Value::Boolean(true));
            // Create per-type prototype with __proto__ → %TypedArray%.prototype
            let mut per_proto = IndexMap::new();
            per_proto.insert("__proto__".to_string(), ta_proto.clone());
            per_proto.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(bpe));
            per_proto.insert("__readonly_BYTES_PER_ELEMENT__".to_string(), Value::Boolean(true));
            per_proto.insert("__nonenumerable_BYTES_PER_ELEMENT__".to_string(), Value::Boolean(true));
            per_proto.insert("__nonconfigurable_BYTES_PER_ELEMENT__".to_string(), Value::Boolean(true));
            let per_proto_obj = Value::VmObject(new_gc_cell_ptr(ctx, per_proto));
            ctor_map.insert("prototype".to_string(), per_proto_obj.clone());
            ctor_map.insert("__readonly_prototype__".to_string(), Value::Boolean(true));
            ctor_map.insert("__nonenumerable_prototype__".to_string(), Value::Boolean(true));
            ctor_map.insert("__nonconfigurable_prototype__".to_string(), Value::Boolean(true));
            // XxxArray.__proto__ = %TypedArray%
            ctor_map.insert("__proto__".to_string(), typed_array_ctor.clone());
            let ctor_val = Value::VmObject(new_gc_cell_ptr(ctx, ctor_map));
            // constructor backref (must point to same GC object)
            if let Value::VmObject(p) = &per_proto_obj {
                p.borrow_mut(ctx).insert("constructor".to_string(), ctor_val.clone());
                p.borrow_mut(ctx)
                    .insert("__nonenumerable_constructor__".to_string(), Value::Boolean(true));
            }
            self.globals.insert(name.to_string(), ctor_val);
        }

        // BigInt typed arrays (host-fn based)
        for &(name, host_fn) in &[("BigInt64Array", "typedarray.bigint64"), ("BigUint64Array", "typedarray.biguint64")] {
            let mut ctor_map = IndexMap::new();
            ctor_map.insert("__host_fn__".to_string(), Value::from(host_fn));
            ctor_map.insert("__constructible__".to_string(), Value::Boolean(true));
            ctor_map.insert("name".to_string(), Value::from(name));
            ctor_map.insert("__readonly_name__".to_string(), Value::Boolean(true));
            ctor_map.insert("__nonenumerable_name__".to_string(), Value::Boolean(true));
            ctor_map.insert("__nonconfigurable_name__".to_string(), Value::Boolean(true));
            ctor_map.insert("length".to_string(), Value::Number(0.0));
            ctor_map.insert("__readonly_length__".to_string(), Value::Boolean(true));
            ctor_map.insert("__nonenumerable_length__".to_string(), Value::Boolean(true));
            ctor_map.insert("__nonconfigurable_length__".to_string(), Value::Boolean(true));
            ctor_map.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(8.0));
            ctor_map.insert("__readonly_BYTES_PER_ELEMENT__".to_string(), Value::Boolean(true));
            ctor_map.insert("__nonenumerable_BYTES_PER_ELEMENT__".to_string(), Value::Boolean(true));
            ctor_map.insert("__nonconfigurable_BYTES_PER_ELEMENT__".to_string(), Value::Boolean(true));
            let mut per_proto = IndexMap::new();
            per_proto.insert("__proto__".to_string(), ta_proto.clone());
            per_proto.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(8.0));
            per_proto.insert("__readonly_BYTES_PER_ELEMENT__".to_string(), Value::Boolean(true));
            per_proto.insert("__nonenumerable_BYTES_PER_ELEMENT__".to_string(), Value::Boolean(true));
            per_proto.insert("__nonconfigurable_BYTES_PER_ELEMENT__".to_string(), Value::Boolean(true));
            let per_proto_obj = Value::VmObject(new_gc_cell_ptr(ctx, per_proto));
            ctor_map.insert("prototype".to_string(), per_proto_obj.clone());
            ctor_map.insert("__readonly_prototype__".to_string(), Value::Boolean(true));
            ctor_map.insert("__nonenumerable_prototype__".to_string(), Value::Boolean(true));
            ctor_map.insert("__nonconfigurable_prototype__".to_string(), Value::Boolean(true));
            ctor_map.insert("__proto__".to_string(), typed_array_ctor.clone());
            let ctor_val = Value::VmObject(new_gc_cell_ptr(ctx, ctor_map));
            if let Value::VmObject(p) = &per_proto_obj {
                p.borrow_mut(ctx).insert("constructor".to_string(), ctor_val.clone());
                p.borrow_mut(ctx)
                    .insert("__nonenumerable_constructor__".to_string(), Value::Boolean(true));
            }
            self.globals.insert(name.to_string(), ctor_val);
        }
    }
}
