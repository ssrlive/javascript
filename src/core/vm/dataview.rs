use super::*;

impl<'gc> VM<'gc> {
    /// Dispatch all `"dataview.*"` host function calls.
    pub(super) fn dataview_handle_host_fn(
        &mut self,
        ctx: &GcContext<'gc>,
        name: &str,
        receiver: Option<&Value<'gc>>,
        args: &[Value<'gc>],
    ) -> Value<'gc> {
        match name {
            "dataview.get_buffer" => {
                if let Some(Value::VmObject(view)) = receiver {
                    let b = view.borrow();
                    if matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "DataView") {
                        return b.get("__dv_buffer__").cloned().unwrap_or(Value::Undefined);
                    }
                }
                self.throw_type_error(ctx, "get DataView.prototype.buffer called on incompatible receiver");
                Value::Undefined
            }
            "dataview.get_byteLength" => {
                if let Some(Value::VmObject(view)) = receiver {
                    let b = view.borrow();
                    if matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "DataView") {
                        if let Some(Value::VmObject(buf)) = b.get("__dv_buffer__")
                            && matches!(GcCell::borrow(buf).get("__detached__"), Some(Value::Boolean(true)))
                        {
                            drop(b);
                            self.throw_type_error(ctx, "Cannot perform operation on a detached ArrayBuffer");
                            return Value::Undefined;
                        }
                        return b.get("__dv_byteLength__").cloned().unwrap_or(Value::Number(0.0));
                    }
                }
                self.throw_type_error(ctx, "get DataView.prototype.byteLength called on incompatible receiver");
                Value::Undefined
            }
            "dataview.get_byteOffset" => {
                if let Some(Value::VmObject(view)) = receiver {
                    let b = view.borrow();
                    if matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "DataView") {
                        if let Some(Value::VmObject(buf)) = b.get("__dv_buffer__")
                            && matches!(GcCell::borrow(buf).get("__detached__"), Some(Value::Boolean(true)))
                        {
                            drop(b);
                            self.throw_type_error(ctx, "Cannot perform operation on a detached ArrayBuffer");
                            return Value::Undefined;
                        }
                        return b.get("__dv_byteOffset__").cloned().unwrap_or(Value::Number(0.0));
                    }
                }
                self.throw_type_error(ctx, "get DataView.prototype.byteOffset called on incompatible receiver");
                Value::Undefined
            }
            "dataview.getUint8" => {
                let (base, _byte_len, buffer) = match self.dataview_validate_get(ctx, receiver, args, 1) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let idx = base;
                    if let Some(v) = bytes.borrow().elements.get(idx) {
                        return Value::Number(to_number(v) as u8 as f64);
                    }
                }
                Value::Number(0.0)
            }
            "dataview.getInt8" => {
                let (base, _byte_len, buffer) = match self.dataview_validate_get(ctx, receiver, args, 1) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned()
                    && let Some(v) = bytes.borrow().elements.get(base)
                {
                    return Value::Number((to_number(v) as u8 as i8) as f64);
                }
                Value::Number(0.0)
            }
            "dataview.setUint8" => {
                let (base, _byte_len, buffer, coerced_val) = match self.dataview_validate_set(ctx, receiver, args, 1) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned()
                    && base < bytes.borrow().elements.len()
                {
                    bytes.borrow_mut(ctx).elements[base] = Value::Number(Self::js_to_uint8(coerced_val) as f64);
                }
                Value::Undefined
            }
            "dataview.setInt8" => {
                let (base, _byte_len, buffer, coerced_val) = match self.dataview_validate_set(ctx, receiver, args, 1) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned()
                    && base < bytes.borrow().elements.len()
                {
                    bytes.borrow_mut(ctx).elements[base] = Value::Number(Self::js_to_uint8(coerced_val) as f64);
                }
                Value::Undefined
            }
            "dataview.getUint16" => {
                let (base, _byte_len, buffer) = match self.dataview_validate_get(ctx, receiver, args, 2) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let little = args.get(1).map(|v| v.to_truthy()).unwrap_or(false);
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let b = bytes.borrow();
                    let b0 = to_number(b.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                    let b1 = to_number(b.elements.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8;
                    let v = if little {
                        u16::from_le_bytes([b0, b1])
                    } else {
                        u16::from_be_bytes([b0, b1])
                    };
                    return Value::Number(v as f64);
                }
                Value::Number(0.0)
            }
            "dataview.getInt16" => {
                let (base, _byte_len, buffer) = match self.dataview_validate_get(ctx, receiver, args, 2) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let little = args.get(1).map(|v| v.to_truthy()).unwrap_or(false);
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let b = bytes.borrow();
                    let b0 = to_number(b.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8;
                    let b1 = to_number(b.elements.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8;
                    let v = if little {
                        u16::from_le_bytes([b0, b1])
                    } else {
                        u16::from_be_bytes([b0, b1])
                    };
                    return Value::Number((v as i16) as f64);
                }
                Value::Number(0.0)
            }
            "dataview.setUint16" => {
                let (base, _byte_len, buffer, coerced_val) = match self.dataview_validate_set(ctx, receiver, args, 2) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let little = args.get(2).map(|v| v.to_truthy()).unwrap_or(false);
                let n = Self::js_to_uint16(coerced_val);
                let bs = if little { n.to_le_bytes() } else { n.to_be_bytes() };
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let mut bm = bytes.borrow_mut(ctx);
                    for (i, &b) in bs.iter().enumerate() {
                        if base + i < bm.elements.len() {
                            bm.elements[base + i] = Value::Number(b as f64);
                        }
                    }
                }
                Value::Undefined
            }
            "dataview.setInt16" => {
                let (base, _byte_len, buffer, coerced_val) = match self.dataview_validate_set(ctx, receiver, args, 2) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let little = args.get(2).map(|v| v.to_truthy()).unwrap_or(false);
                let n = Self::js_to_uint16(coerced_val);
                let bs = if little { n.to_le_bytes() } else { n.to_be_bytes() };
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let mut bm = bytes.borrow_mut(ctx);
                    for (i, &b) in bs.iter().enumerate() {
                        if base + i < bm.elements.len() {
                            bm.elements[base + i] = Value::Number(b as f64);
                        }
                    }
                }
                Value::Undefined
            }
            "dataview.getUint32" => {
                let (base, _byte_len, buffer) = match self.dataview_validate_get(ctx, receiver, args, 4) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let little = args.get(1).map(|v| v.to_truthy()).unwrap_or(false);
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let b = bytes.borrow();
                    let arr: [u8; 4] = [
                        to_number(b.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 2).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 3).unwrap_or(&Value::Number(0.0))) as u8,
                    ];
                    let v = if little { u32::from_le_bytes(arr) } else { u32::from_be_bytes(arr) };
                    return Value::Number(v as f64);
                }
                Value::Number(0.0)
            }
            "dataview.getInt32" => {
                let (base, _byte_len, buffer) = match self.dataview_validate_get(ctx, receiver, args, 4) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let little = args.get(1).map(|v| v.to_truthy()).unwrap_or(false);
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let b = bytes.borrow();
                    let arr: [u8; 4] = [
                        to_number(b.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 2).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 3).unwrap_or(&Value::Number(0.0))) as u8,
                    ];
                    let v = if little { u32::from_le_bytes(arr) } else { u32::from_be_bytes(arr) };
                    return Value::Number((v as i32) as f64);
                }
                Value::Number(0.0)
            }
            "dataview.setUint32" => {
                let (base, _byte_len, buffer, coerced_val) = match self.dataview_validate_set(ctx, receiver, args, 4) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let little = args.get(2).map(|v| v.to_truthy()).unwrap_or(false);
                let n = Self::js_to_uint32(coerced_val);
                let bs = if little { n.to_le_bytes() } else { n.to_be_bytes() };
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let mut bm = bytes.borrow_mut(ctx);
                    for (i, &b) in bs.iter().enumerate() {
                        if base + i < bm.elements.len() {
                            bm.elements[base + i] = Value::Number(b as f64);
                        }
                    }
                }
                Value::Undefined
            }
            "dataview.setInt32" => {
                let (base, _byte_len, buffer, coerced_val) = match self.dataview_validate_set(ctx, receiver, args, 4) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let little = args.get(2).map(|v| v.to_truthy()).unwrap_or(false);
                let n = Self::js_to_uint32(coerced_val);
                let bs = if little { n.to_le_bytes() } else { n.to_be_bytes() };
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let mut bm = bytes.borrow_mut(ctx);
                    for (i, &b) in bs.iter().enumerate() {
                        if base + i < bm.elements.len() {
                            bm.elements[base + i] = Value::Number(b as f64);
                        }
                    }
                }
                Value::Undefined
            }
            "dataview.getFloat32" => {
                let (base, _byte_len, buffer) = match self.dataview_validate_get(ctx, receiver, args, 4) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let little = args.get(1).map(|v| v.to_truthy()).unwrap_or(false);
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let b = bytes.borrow();
                    let arr: [u8; 4] = [
                        to_number(b.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 2).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 3).unwrap_or(&Value::Number(0.0))) as u8,
                    ];
                    let v = if little { u32::from_le_bytes(arr) } else { u32::from_be_bytes(arr) };
                    return Value::Number(f32::from_bits(v) as f64);
                }
                Value::Number(0.0)
            }
            "dataview.setFloat32" => {
                let (base, _byte_len, buffer, coerced_val) = match self.dataview_validate_set(ctx, receiver, args, 4) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let little = args.get(2).map(|v| v.to_truthy()).unwrap_or(false);
                let bits = (coerced_val as f32).to_bits();
                let bs = if little { bits.to_le_bytes() } else { bits.to_be_bytes() };
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let mut bm = bytes.borrow_mut(ctx);
                    for (i, &b) in bs.iter().enumerate() {
                        if base + i < bm.elements.len() {
                            bm.elements[base + i] = Value::Number(b as f64);
                        }
                    }
                }
                Value::Undefined
            }
            "dataview.getFloat64" => {
                let (base, _byte_len, buffer) = match self.dataview_validate_get(ctx, receiver, args, 8) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let little = args.get(1).map(|v| v.to_truthy()).unwrap_or(false);
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let b = bytes.borrow();
                    let arr: [u8; 8] = [
                        to_number(b.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 2).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 3).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 4).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 5).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 6).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 7).unwrap_or(&Value::Number(0.0))) as u8,
                    ];
                    let v = if little { u64::from_le_bytes(arr) } else { u64::from_be_bytes(arr) };
                    return Value::Number(f64::from_bits(v));
                }
                Value::Number(0.0)
            }
            "dataview.setFloat64" => {
                let (base, _byte_len, buffer, coerced_val) = match self.dataview_validate_set(ctx, receiver, args, 8) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let little = args.get(2).map(|v| v.to_truthy()).unwrap_or(false);
                let bits = f64::to_bits(coerced_val);
                let bs = if little { bits.to_le_bytes() } else { bits.to_be_bytes() };
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let mut bm = bytes.borrow_mut(ctx);
                    for (i, &b) in bs.iter().enumerate() {
                        if base + i < bm.elements.len() {
                            bm.elements[base + i] = Value::Number(b as f64);
                        }
                    }
                }
                Value::Undefined
            }
            "dataview.getBigInt64" => {
                let (base, _byte_len, buffer) = match self.dataview_validate_get(ctx, receiver, args, 8) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let little = args.get(1).map(|v| v.to_truthy()).unwrap_or(false);
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let b = bytes.borrow();
                    let arr: [u8; 8] = [
                        to_number(b.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 2).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 3).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 4).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 5).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 6).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 7).unwrap_or(&Value::Number(0.0))) as u8,
                    ];
                    let v = if little { i64::from_le_bytes(arr) } else { i64::from_be_bytes(arr) };
                    return Value::BigInt(Box::new(num_bigint::BigInt::from(v)));
                }
                Value::BigInt(Box::new(num_bigint::BigInt::from(0)))
            }
            "dataview.getBigUint64" => {
                let (base, _byte_len, buffer) = match self.dataview_validate_get(ctx, receiver, args, 8) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let little = args.get(1).map(|v| v.to_truthy()).unwrap_or(false);
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let b = bytes.borrow();
                    let arr: [u8; 8] = [
                        to_number(b.elements.get(base).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 1).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 2).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 3).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 4).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 5).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 6).unwrap_or(&Value::Number(0.0))) as u8,
                        to_number(b.elements.get(base + 7).unwrap_or(&Value::Number(0.0))) as u8,
                    ];
                    let v = if little { u64::from_le_bytes(arr) } else { u64::from_be_bytes(arr) };
                    return Value::BigInt(Box::new(num_bigint::BigInt::from(v)));
                }
                Value::BigInt(Box::new(num_bigint::BigInt::from(0)))
            }
            "dataview.setBigInt64" | "dataview.setBigUint64" => {
                let view = match receiver {
                    Some(Value::VmObject(obj)) => *obj,
                    _ => {
                        self.throw_type_error(ctx, "Method called on incompatible receiver");
                        return Value::Undefined;
                    }
                };
                {
                    let b = view.borrow();
                    let is_dv = matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "DataView");
                    if !is_dv {
                        self.throw_type_error(ctx, "Method called on incompatible receiver");
                        return Value::Undefined;
                    }
                }
                let offset_arg = args.first().cloned().unwrap_or(Value::Undefined);
                let get_index = match self.dataview_to_index(ctx, &offset_arg) {
                    Some(v) => v,
                    None => return Value::Undefined,
                };
                let value_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
                let big_val: i128 = match &value_arg {
                    Value::BigInt(b) => {
                        let s = b.to_string();
                        s.parse::<i128>().unwrap_or(0)
                    }
                    _ => {
                        self.throw_type_error(ctx, "Cannot convert a non-BigInt value to a BigInt");
                        return Value::Undefined;
                    }
                };
                let little = args.get(2).map(|v| v.to_truthy()).unwrap_or(false);
                let b = view.borrow();
                let byte_offset = match b.get("__dv_byteOffset__") {
                    Some(Value::Number(n)) => *n as usize,
                    _ => 0,
                };
                let byte_length = match b.get("__dv_byteLength__") {
                    Some(Value::Number(n)) => *n as usize,
                    _ => 0,
                };
                let buffer = match b.get("__dv_buffer__").cloned() {
                    Some(Value::VmObject(buf)) => buf,
                    _ => {
                        drop(b);
                        self.throw_type_error(ctx, "DataView buffer is detached");
                        return Value::Undefined;
                    }
                };
                drop(b);
                if matches!(GcCell::borrow(&buffer).get("__detached__"), Some(Value::Boolean(true))) {
                    self.throw_type_error(ctx, "Cannot perform operation on a detached ArrayBuffer");
                    return Value::Undefined;
                }
                let base = byte_offset + get_index;
                if get_index + 8 > byte_length {
                    let err = self.make_range_error_object(ctx, "Offset is outside the bounds of the DataView");
                    let _ = self.handle_throw(ctx, &err);
                    return Value::Undefined;
                }
                let bs = if name == "dataview.setBigUint64" {
                    let n = big_val as u64;
                    if little { n.to_le_bytes() } else { n.to_be_bytes() }
                } else {
                    let n = big_val as i64;
                    if little {
                        n.to_le_bytes().map(|b| b)
                    } else {
                        n.to_be_bytes().map(|b| b)
                    }
                };
                if let Some(Value::VmArray(bytes)) = GcCell::borrow(&buffer).get("__buffer_bytes__").cloned() {
                    let mut bm = bytes.borrow_mut(ctx);
                    for (i, &byte_val) in bs.iter().enumerate() {
                        if base + i < bm.elements.len() {
                            bm.elements[base + i] = Value::Number(byte_val as f64);
                        }
                    }
                }
                Value::Undefined
            }
            _ => Value::Undefined,
        }
    }

    /// Initialize DataView.prototype and the DataView constructor on the global object.
    pub(super) fn dataview_init_prototype(&mut self, ctx: &GcContext<'gc>) {
        let mut dv_proto = IndexMap::new();
        Self::insert_property_with_attributes(&mut dv_proto, "@@sym:4", &Value::from("DataView"), false, false, true);
        dv_proto.insert(
            "__get_buffer".to_string(),
            Self::make_host_fn_with_name_len(ctx, "dataview.get_buffer", "get buffer", 0.0, false),
        );
        dv_proto.insert("__nonenumerable_buffer__".to_string(), Value::Boolean(true));
        dv_proto.insert(
            "__get_byteLength".to_string(),
            Self::make_host_fn_with_name_len(ctx, "dataview.get_byteLength", "get byteLength", 0.0, false),
        );
        dv_proto.insert("__nonenumerable_byteLength__".to_string(), Value::Boolean(true));
        dv_proto.insert(
            "__get_byteOffset".to_string(),
            Self::make_host_fn_with_name_len(ctx, "dataview.get_byteOffset", "get byteOffset", 0.0, false),
        );
        dv_proto.insert("__nonenumerable_byteOffset__".to_string(), Value::Boolean(true));
        for &(name, host, len) in &[
            ("getInt8", "dataview.getInt8", 1.0),
            ("getUint8", "dataview.getUint8", 1.0),
            ("getInt16", "dataview.getInt16", 1.0),
            ("getUint16", "dataview.getUint16", 1.0),
            ("getInt32", "dataview.getInt32", 1.0),
            ("getUint32", "dataview.getUint32", 1.0),
            ("getFloat32", "dataview.getFloat32", 1.0),
            ("getFloat64", "dataview.getFloat64", 1.0),
            ("getBigInt64", "dataview.getBigInt64", 1.0),
            ("getBigUint64", "dataview.getBigUint64", 1.0),
            ("setInt8", "dataview.setInt8", 2.0),
            ("setUint8", "dataview.setUint8", 2.0),
            ("setInt16", "dataview.setInt16", 2.0),
            ("setUint16", "dataview.setUint16", 2.0),
            ("setInt32", "dataview.setInt32", 2.0),
            ("setUint32", "dataview.setUint32", 2.0),
            ("setFloat32", "dataview.setFloat32", 2.0),
            ("setFloat64", "dataview.setFloat64", 2.0),
            ("setBigInt64", "dataview.setBigInt64", 2.0),
            ("setBigUint64", "dataview.setBigUint64", 2.0),
        ] {
            dv_proto.insert(name.to_string(), Self::make_host_fn_with_name_len(ctx, host, name, len, false));
            dv_proto.insert(format!("__nonenumerable_{}__", name), Value::Boolean(true));
        }
        let dv_proto_val = Value::VmObject(new_gc_cell_ptr(ctx, dv_proto));
        let mut data_view_map = IndexMap::new();
        data_view_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_DATAVIEW as f64));
        Self::insert_property_with_attributes(&mut data_view_map, "name", &Value::from("DataView"), false, false, true);
        Self::insert_property_with_attributes(&mut data_view_map, "length", &Value::Number(1.0), false, false, true);
        data_view_map.insert("prototype".to_string(), dv_proto_val);
        data_view_map.insert("__readonly_prototype__".to_string(), Value::Boolean(true));
        data_view_map.insert("__nonenumerable_prototype__".to_string(), Value::Boolean(true));
        data_view_map.insert("__nonconfigurable_prototype__".to_string(), Value::Boolean(true));
        let data_view_ctor = Value::VmObject(new_gc_cell_ptr(ctx, data_view_map));
        if let Value::VmObject(ctor_obj) = &data_view_ctor
            && let Some(Value::VmObject(proto_obj)) = ctor_obj.borrow().get("prototype").cloned()
        {
            proto_obj.borrow_mut(ctx).insert("constructor".to_string(), data_view_ctor.clone());
            proto_obj
                .borrow_mut(ctx)
                .insert("__nonenumerable_constructor__".to_string(), Value::Boolean(true));
        }
        self.globals.insert("DataView".to_string(), data_view_ctor.clone());
        self.global_this.borrow_mut(ctx).insert("DataView".to_string(), data_view_ctor);
        self.global_this
            .borrow_mut(ctx)
            .insert("__nonenumerable_DataView__".to_string(), Value::Boolean(true));
    }

    /// Handle DataView in `call_builtin` (new DataView(...)).
    pub(super) fn dataview_call_builtin(&mut self, ctx: &GcContext<'gc>, args: &[Value<'gc>]) -> Value<'gc> {
        if self.new_target_stack.is_empty() {
            let err = self.make_type_error_object(ctx, "Constructor DataView requires 'new'");
            let _ = self.handle_throw(ctx, &err);
            return Value::Undefined;
        }
        let buffer = args.first().cloned().unwrap_or(Value::Undefined);
        let is_valid_buffer = match &buffer {
            Value::VmObject(obj) => {
                let b = obj.borrow();
                matches!(
                    b.get("__type__"),
                    Some(Value::String(s))
                        if crate::unicode::utf16_to_utf8(s) == "ArrayBuffer"
                            || crate::unicode::utf16_to_utf8(s) == "SharedArrayBuffer"
                )
            }
            _ => false,
        };
        if !is_valid_buffer {
            let err = self.make_type_error_object(ctx, "First argument to DataView constructor must be an ArrayBuffer");
            let _ = self.handle_throw(ctx, &err);
            return Value::Undefined;
        }
        let buf_byte_len = if let Value::VmObject(obj) = &buffer {
            match obj.borrow().get("byteLength") {
                Some(Value::Number(n)) => *n as usize,
                _ => 0,
            }
        } else {
            0
        };
        let raw_offset = args.get(1).cloned().unwrap_or(Value::Undefined);
        let byte_offset = match self.dataview_to_index(ctx, &raw_offset) {
            Some(v) => v,
            None => return Value::Undefined,
        };
        if let Value::VmObject(buf_obj) = &buffer
            && matches!(buf_obj.borrow().get("__detached__"), Some(Value::Boolean(true)))
        {
            let err = self.make_type_error_object(ctx, "Cannot construct DataView with a detached ArrayBuffer");
            let _ = self.handle_throw(ctx, &err);
            return Value::Undefined;
        }
        if byte_offset > buf_byte_len {
            let err = self.make_range_error_object(ctx, "Start offset is outside the bounds of the buffer");
            let _ = self.handle_throw(ctx, &err);
            return Value::Undefined;
        }
        let raw_len = args.get(2).cloned().unwrap_or(Value::Undefined);
        let view_byte_len = if matches!(raw_len, Value::Undefined) {
            buf_byte_len - byte_offset
        } else {
            match self.dataview_to_index(ctx, &raw_len) {
                Some(v) => v,
                None => return Value::Undefined,
            }
        };
        if byte_offset + view_byte_len > buf_byte_len {
            let err = self.make_range_error_object(ctx, "Invalid DataView length");
            let _ = self.handle_throw(ctx, &err);
            return Value::Undefined;
        }
        let mut map = IndexMap::new();
        map.insert("__type__".to_string(), Value::from("DataView"));
        map.insert("__dv_buffer__".to_string(), buffer);
        map.insert("__buffer__".to_string(), Value::Boolean(true));
        map.insert("__dv_byteLength__".to_string(), Value::Number(view_byte_len as f64));
        map.insert("__dv_byteOffset__".to_string(), Value::Number(byte_offset as f64));
        Value::VmObject(new_gc_cell_ptr(ctx, map))
    }

    /// Handle DataView in `call_method_builtin`.
    pub(super) fn dataview_call_method_builtin(&mut self, ctx: &GcContext<'gc>, receiver: &Value<'gc>, args: &[Value<'gc>]) -> Value<'gc> {
        if let Value::VmObject(recv_obj) = receiver {
            let is_valid_buffer = matches!(
                args.first(),
                Some(Value::VmObject(buf)) if matches!(
                    buf.borrow().get("__type__"),
                    Some(Value::String(s)) if {
                        let t = crate::unicode::utf16_to_utf8(s);
                        t == "ArrayBuffer" || t == "SharedArrayBuffer"
                    }
                )
            );
            if !is_valid_buffer {
                self.throw_type_error(ctx, "DataView constructor requires a buffer");
                return Value::Undefined;
            }
            let out = self.call_builtin(ctx, BUILTIN_CTOR_DATAVIEW, args);
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            if let Value::VmObject(out_obj) = out {
                let out_b = out_obj.borrow();
                let mut recv_b = recv_obj.borrow_mut(ctx);
                for (k, v) in out_b.iter() {
                    if k != "__proto__" {
                        recv_b.insert(k.clone(), v.clone());
                    }
                }
            }
            return receiver.clone();
        }
        Value::Undefined
    }

    /// Construct a DataView with proper spec ordering:
    /// 1. Validate arguments (before GetPrototypeFromConstructor)
    /// 2. OrdinaryCreateFromConstructor (reads newTarget.prototype)
    /// 3. Final detached check (after prototype getter may have detached buffer)
    pub(super) fn construct_dataview(
        &mut self,
        ctx: &GcContext<'gc>,
        target: &Value<'gc>,
        args: &[Value<'gc>],
        new_target: Option<&Value<'gc>>,
    ) -> Result<Value<'gc>, JSError> {
        let buffer = args.first().cloned().unwrap_or(Value::Undefined);
        let is_valid_buffer = match &buffer {
            Value::VmObject(obj) => {
                let b = obj.borrow();
                matches!(
                    b.get("__type__"),
                    Some(Value::String(s))
                        if crate::unicode::utf16_to_utf8(s) == "ArrayBuffer"
                            || crate::unicode::utf16_to_utf8(s) == "SharedArrayBuffer"
                )
            }
            _ => false,
        };
        if !is_valid_buffer {
            let err = self.make_type_error_object(ctx, "First argument to DataView constructor must be an ArrayBuffer");
            return Err(self.vm_error_to_js_error(ctx, &err));
        }
        let raw_offset = args.get(1).cloned().unwrap_or(Value::Undefined);
        let byte_offset = self.to_index_result(ctx, &raw_offset)?;
        if let Value::VmObject(buf_obj) = &buffer
            && matches!(buf_obj.borrow().get("__detached__"), Some(Value::Boolean(true)))
        {
            let err = self.make_type_error_object(ctx, "Cannot construct DataView with a detached ArrayBuffer");
            return Err(self.vm_error_to_js_error(ctx, &err));
        }
        let buf_byte_len = if let Value::VmObject(obj) = &buffer {
            match obj.borrow().get("byteLength") {
                Some(Value::Number(n)) => *n as usize,
                _ => 0,
            }
        } else {
            0
        };
        if byte_offset > buf_byte_len {
            let err = self.make_range_error_object(ctx, "Start offset is outside the bounds of the buffer");
            return Err(self.vm_error_to_js_error(ctx, &err));
        }
        let raw_len = args.get(2).cloned().unwrap_or(Value::Undefined);
        let view_byte_len = if matches!(raw_len, Value::Undefined) {
            buf_byte_len - byte_offset
        } else {
            let len = self.to_index_result(ctx, &raw_len)?;
            if byte_offset + len > buf_byte_len {
                let err = self.make_range_error_object(ctx, "Invalid DataView length");
                return Err(self.vm_error_to_js_error(ctx, &err));
            }
            len
        };
        let ctor_prototype = self.get_prototype_from_constructor_with_intrinsic(ctx, new_target.unwrap_or(target), "DataView")?;
        if let Value::VmObject(buf_obj) = &buffer
            && matches!(buf_obj.borrow().get("__detached__"), Some(Value::Boolean(true)))
        {
            let err = self.make_type_error_object(ctx, "Cannot construct DataView with a detached ArrayBuffer");
            return Err(self.vm_error_to_js_error(ctx, &err));
        }
        let mut map = IndexMap::new();
        map.insert("__type__".to_string(), Value::from("DataView"));
        map.insert("__dv_buffer__".to_string(), buffer);
        map.insert("__buffer__".to_string(), Value::Boolean(true));
        map.insert("__dv_byteLength__".to_string(), Value::Number(view_byte_len as f64));
        map.insert("__dv_byteOffset__".to_string(), Value::Number(byte_offset as f64));
        if let Some(proto) = ctor_prototype {
            map.insert("__proto__".to_string(), proto);
        }
        Ok(Value::VmObject(new_gc_cell_ptr(ctx, map)))
    }

    /// ToIndex conversion that returns Result (for use in construct_value context).
    #[allow(clippy::wrong_self_convention)]
    pub(super) fn to_index_result(&mut self, ctx: &GcContext<'gc>, val: &Value<'gc>) -> Result<usize, JSError> {
        if matches!(val, Value::Undefined) {
            return Ok(0);
        }
        if val.is_symbol_value() {
            let err = self.make_type_error_object(ctx, "Cannot convert a Symbol value to a number");
            return Err(self.vm_error_to_js_error(ctx, &err));
        }
        let prim = self.try_to_primitive(ctx, val, "number");
        if let Some(thrown) = self.pending_throw.take() {
            return Err(self.vm_error_to_js_error(ctx, &thrown));
        }
        if prim.is_symbol_value() {
            let err = self.make_type_error_object(ctx, "Cannot convert a Symbol value to a number");
            return Err(self.vm_error_to_js_error(ctx, &err));
        }
        let n = to_number(&prim);
        if n.is_nan() {
            return Ok(0);
        }
        let integer_index = n.trunc();
        if integer_index < 0.0 || integer_index > ((1u64 << 53) - 1) as f64 {
            let err = self.make_range_error_object(ctx, "Invalid index");
            return Err(self.vm_error_to_js_error(ctx, &err));
        }
        Ok(integer_index as usize)
    }

    /// ToIndex conversion for DataView: undefined→0, Symbol→TypeError, negative/infinity→RangeError.
    fn dataview_to_index(&mut self, ctx: &GcContext<'gc>, val: &Value<'gc>) -> Option<usize> {
        if matches!(val, Value::Undefined) {
            return Some(0);
        }
        let n = self.extract_number_with_coercion(ctx, val)?;
        if n.is_nan() {
            return Some(0);
        }
        let integer_index = n.trunc();
        if integer_index < 0.0 || integer_index > ((1u64 << 53) - 1) as f64 {
            let err = self.make_range_error_object(ctx, "Invalid index");
            let _ = self.handle_throw(ctx, &err);
            return None;
        }
        Some(integer_index as usize)
    }

    /// Validate `this` is a DataView, convert offset via ToIndex, check range.
    #[allow(clippy::type_complexity)]
    fn dataview_validate_get(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        args: &[Value<'gc>],
        element_size: usize,
    ) -> Option<(usize, usize, VmObjectHandle<'gc>)> {
        let view = match receiver {
            Some(Value::VmObject(obj)) => *obj,
            _ => {
                self.throw_type_error(ctx, "Method called on incompatible receiver");
                return None;
            }
        };
        {
            let b = view.borrow();
            let is_dv = matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "DataView");
            if !is_dv {
                self.throw_type_error(ctx, "Method called on incompatible receiver");
                return None;
            }
        }
        let offset_arg = args.first().cloned().unwrap_or(Value::Undefined);
        let get_index = self.dataview_to_index(ctx, &offset_arg)?;
        let b = view.borrow();
        let byte_offset = match b.get("__dv_byteOffset__") {
            Some(Value::Number(n)) => *n as usize,
            _ => 0,
        };
        let byte_length = match b.get("__dv_byteLength__") {
            Some(Value::Number(n)) => *n as usize,
            _ => 0,
        };
        let buffer = match b.get("__dv_buffer__").cloned() {
            Some(Value::VmObject(buf)) => buf,
            _ => {
                drop(b);
                self.throw_type_error(ctx, "DataView buffer is detached");
                return None;
            }
        };
        drop(b);
        if matches!(GcCell::borrow(&buffer).get("__detached__"), Some(Value::Boolean(true))) {
            self.throw_type_error(ctx, "Cannot perform operation on a detached ArrayBuffer");
            return None;
        }
        if get_index + element_size > byte_length {
            let err = self.make_range_error_object(ctx, "Offset is outside the bounds of the DataView");
            let _ = self.handle_throw(ctx, &err);
            return None;
        }
        Some((byte_offset + get_index, byte_length, buffer))
    }

    /// Validate for set methods: same as get, plus coerce value via ToNumber.
    #[allow(clippy::type_complexity)]
    fn dataview_validate_set(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        args: &[Value<'gc>],
        element_size: usize,
    ) -> Option<(usize, usize, VmObjectHandle<'gc>, f64)> {
        let view = match receiver {
            Some(Value::VmObject(obj)) => *obj,
            _ => {
                self.throw_type_error(ctx, "Method called on incompatible receiver");
                return None;
            }
        };
        {
            let b = view.borrow();
            let is_dv = matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "DataView");
            if !is_dv {
                self.throw_type_error(ctx, "Method called on incompatible receiver");
                return None;
            }
        }
        let offset_arg = args.first().cloned().unwrap_or(Value::Undefined);
        let get_index = self.dataview_to_index(ctx, &offset_arg)?;
        let value_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
        let coerced_val = self.extract_number_with_coercion(ctx, &value_arg)?;
        let b = view.borrow();
        let byte_offset = match b.get("__dv_byteOffset__") {
            Some(Value::Number(n)) => *n as usize,
            _ => 0,
        };
        let byte_length = match b.get("__dv_byteLength__") {
            Some(Value::Number(n)) => *n as usize,
            _ => 0,
        };
        let buffer = match b.get("__dv_buffer__").cloned() {
            Some(Value::VmObject(buf)) => buf,
            _ => {
                drop(b);
                self.throw_type_error(ctx, "DataView buffer is detached");
                return None;
            }
        };
        drop(b);
        if matches!(GcCell::borrow(&buffer).get("__detached__"), Some(Value::Boolean(true))) {
            self.throw_type_error(ctx, "Cannot perform operation on a detached ArrayBuffer");
            return None;
        }
        if get_index + element_size > byte_length {
            let err = self.make_range_error_object(ctx, "Offset is outside the bounds of the DataView");
            let _ = self.handle_throw(ctx, &err);
            return None;
        }
        Some((byte_offset + get_index, byte_length, buffer, coerced_val))
    }

    fn js_to_uint8(val: f64) -> u8 {
        if val.is_nan() || val.is_infinite() || val == 0.0 {
            return 0;
        }
        let n = val.trunc();
        let m = n % 256.0;
        let m = if m < 0.0 { m + 256.0 } else { m };
        m as u8
    }

    fn js_to_uint16(val: f64) -> u16 {
        if val.is_nan() || val.is_infinite() || val == 0.0 {
            return 0;
        }
        let n = val.trunc();
        let m = n % 65536.0;
        let m = if m < 0.0 { m + 65536.0 } else { m };
        m as u16
    }

    fn js_to_uint32(val: f64) -> u32 {
        if val.is_nan() || val.is_infinite() || val == 0.0 {
            return 0;
        }
        let n = val.trunc();
        let m = n % 4294967296.0;
        let m = if m < 0.0 { m + 4294967296.0 } else { m };
        m as u32
    }
}
