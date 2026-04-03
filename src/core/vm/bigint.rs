use super::*;
use crate::{error::JSError, raise_syntax_error};
use num_bigint::BigInt;

pub fn parse_bigint_string(raw: &str) -> Result<BigInt, JSError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(BigInt::from(0));
    }
    let (sign, after_sign) = if let Some(rest) = trimmed.strip_prefix('-') {
        (-1i8, rest)
    } else if let Some(rest) = trimmed.strip_prefix('+') {
        (1i8, rest)
    } else {
        (1i8, trimmed)
    };
    let (radix, digits) = if after_sign.starts_with("0x") || after_sign.starts_with("0X") {
        if sign != 1 {
            return Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw)));
        }
        (16, &after_sign[2..])
    } else if after_sign.starts_with("0b") || after_sign.starts_with("0B") {
        if sign != 1 {
            return Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw)));
        }
        (2, &after_sign[2..])
    } else if after_sign.starts_with("0o") || after_sign.starts_with("0O") {
        if sign != 1 {
            return Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw)));
        }
        (8, &after_sign[2..])
    } else {
        (10, after_sign)
    };
    if digits.is_empty() {
        return Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw)));
    }
    match BigInt::parse_bytes(digits.as_bytes(), radix) {
        Some(mut val) => {
            if sign < 0 {
                val = -val;
            }
            Ok(val)
        }
        None => Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw))),
    }
}

fn bigint_from_integral_number(n: f64) -> Option<num_bigint::BigInt> {
    if !n.is_finite() || n != n.trunc() {
        return None;
    }
    if n == 0.0 {
        return Some(num_bigint::BigInt::from(0));
    }

    let bits = n.to_bits();
    let sign_negative = (bits >> 63) != 0;
    let exp_bits = ((bits >> 52) & 0x7ff) as i32;
    let frac_bits = bits & ((1u64 << 52) - 1);

    if exp_bits == 0 {
        return None;
    }

    let exponent = exp_bits - 1023;
    let mut sig = num_bigint::BigInt::from((1u64 << 52) | frac_bits);
    if exponent >= 52 {
        sig <<= (exponent - 52) as usize;
    } else {
        let rshift = (52 - exponent) as u32;
        let mask = (1u64 << rshift) - 1;
        if frac_bits & mask != 0 {
            return None;
        }
        sig >>= rshift as usize;
    }

    if sign_negative {
        sig = -sig;
    }
    Some(sig)
}

pub(crate) fn compare_bigint_number(a: &num_bigint::BigInt, b: f64) -> Option<std::cmp::Ordering> {
    if b.is_nan() {
        return None;
    }
    if b == f64::INFINITY {
        return Some(std::cmp::Ordering::Less);
    }
    if b == f64::NEG_INFINITY {
        return Some(std::cmp::Ordering::Greater);
    }

    if let Some(bi) = bigint_from_integral_number(b) {
        return Some(a.cmp(&bi));
    }

    let floor_bi = bigint_from_integral_number(b.floor())?;
    if a <= &floor_bi {
        Some(std::cmp::Ordering::Less)
    } else {
        Some(std::cmp::Ordering::Greater)
    }
}

impl<'gc> VM<'gc> {
    pub(super) fn bigint_init_prototype(&mut self, ctx: &GcContext<'gc>) {
        let object_proto = if let Some(Value::VmObject(o)) = self.globals.get("Object").and_then(|v| {
            if let Value::VmObject(obj) = v {
                obj.borrow().get("prototype").cloned()
            } else {
                None
            }
        }) {
            o
        } else {
            self.global_this
        };

        let mut bigint_map = IndexMap::new();
        bigint_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_BIGINT as f64));
        Self::insert_property_with_attributes(&mut bigint_map, "name", &Value::from("BigInt"), false, false, true);
        Self::insert_property_with_attributes(&mut bigint_map, "length", &Value::Number(1.0), false, false, true);
        bigint_map.insert("asUintN".to_string(), Value::VmNativeFunction(BUILTIN_BIGINT_ASUINTN));
        bigint_map.insert("asIntN".to_string(), Value::VmNativeFunction(BUILTIN_BIGINT_ASINTN));
        Self::set_property_attributes(&mut bigint_map, "asUintN", true, false, true);
        Self::set_property_attributes(&mut bigint_map, "asIntN", true, false, true);

        let mut bigint_proto = IndexMap::new();
        bigint_proto.insert("__type__".to_string(), Value::from("BigInt"));
        bigint_proto.insert("__proto__".to_string(), Value::VmObject(object_proto));
        bigint_proto.insert("toString".to_string(), Value::VmNativeFunction(BUILTIN_BIGINT_TOSTRING));
        bigint_proto.insert("valueOf".to_string(), Value::VmNativeFunction(BUILTIN_BIGINT_VALUEOF));
        bigint_proto.insert("toLocaleString".to_string(), Value::VmNativeFunction(BUILTIN_BIGINT_TOLOCALESTRING));
        Self::set_property_attributes(&mut bigint_proto, "toString", true, false, true);
        Self::set_property_attributes(&mut bigint_proto, "valueOf", true, false, true);
        Self::set_property_attributes(&mut bigint_proto, "toLocaleString", true, false, true);
        Self::insert_property_with_attributes(&mut bigint_proto, "@@sym:4", &Value::from("BigInt"), false, false, true);
        let bigint_proto_ptr = new_gc_cell_ptr(ctx, bigint_proto);
        bigint_map.insert("prototype".to_string(), Value::VmObject(bigint_proto_ptr));
        Self::set_property_attributes(&mut bigint_map, "prototype", false, false, false);
        let bigint_ctor = Value::VmObject(new_gc_cell_ptr(ctx, bigint_map));

        bigint_proto_ptr
            .borrow_mut(ctx)
            .insert("constructor".to_string(), bigint_ctor.clone());
        Self::set_property_attributes(&mut bigint_proto_ptr.borrow_mut(ctx), "constructor", true, false, true);
        self.globals.insert("BigInt".to_string(), bigint_ctor);
    }

    pub(super) fn bigint_call_builtin(&mut self, ctx: &GcContext<'gc>, id: FunctionID, args: &[Value<'gc>]) -> Option<Value<'gc>> {
        match id {
            BUILTIN_BIGINT => {
                let arg = args.first().cloned().unwrap_or(Value::Undefined);
                let prim = self.try_to_primitive(ctx, &arg, "number");
                if self.pending_throw.is_some() {
                    return Some(Value::Undefined);
                }
                let out = match &prim {
                    Value::BigInt(bi) => Value::BigInt(bi.clone()),
                    Value::Boolean(b) => Value::BigInt(Box::new(num_bigint::BigInt::from(if *b { 1 } else { 0 }))),
                    Value::String(s) => {
                        let text = crate::unicode::utf16_to_utf8(s);
                        match parse_bigint_string(&text) {
                            Ok(bi) => Value::BigInt(Box::new(bi)),
                            Err(_) => {
                                self.throw_syntax_error(ctx, &format!("Cannot convert {} to a BigInt", text));
                                Value::Undefined
                            }
                        }
                    }
                    Value::Number(n) => {
                        if n.is_finite() && *n == n.trunc() {
                            Value::BigInt(Box::new(num_bigint::BigInt::from(*n as i64)))
                        } else {
                            self.throw_range_error_object(
                                ctx,
                                "The number is not safe to convert to a BigInt because it is not an integer",
                            );
                            Value::Undefined
                        }
                    }
                    Value::Undefined | Value::Null => {
                        self.throw_type_error(ctx, "Cannot convert undefined to a BigInt");
                        Value::Undefined
                    }
                    _ => {
                        self.throw_type_error(ctx, "Cannot convert value to a BigInt");
                        Value::Undefined
                    }
                };
                Some(out)
            }
            BUILTIN_BIGINT_ASUINTN | BUILTIN_BIGINT_ASINTN => {
                let bits_arg = args.first().cloned().unwrap_or(Value::Undefined);
                let bits: usize = if matches!(bits_arg, Value::Undefined) {
                    0
                } else {
                    let prim = self.try_to_primitive(ctx, &bits_arg, "number");
                    if self.pending_throw.is_some() {
                        return Some(Value::Undefined);
                    }
                    if matches!(prim, Value::BigInt(_)) {
                        self.throw_type_error(ctx, "Cannot convert a BigInt value to a number");
                        return Some(Value::Undefined);
                    }
                    if prim.is_symbol_value() {
                        self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                        return Some(Value::Undefined);
                    }
                    let bits_num = to_number(&prim);
                    if self.pending_throw.is_some() {
                        return Some(Value::Undefined);
                    }
                    let integer_index = if bits_num.is_nan() { 0.0 } else { bits_num.trunc() };
                    if !(0.0..=9007199254740991.0).contains(&integer_index) || integer_index.is_infinite() {
                        self.throw_range_error_object(ctx, "Invalid index");
                        return Some(Value::Undefined);
                    }
                    integer_index as usize
                };

                let bigint_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
                let bigint_prim = self.try_to_primitive(ctx, &bigint_arg, "number");
                if self.pending_throw.is_some() {
                    return Some(Value::Undefined);
                }
                let as_bigint = match &bigint_prim {
                    Value::BigInt(bi) => (**bi).clone(),
                    Value::Boolean(b) => num_bigint::BigInt::from(if *b { 1 } else { 0 }),
                    Value::String(s) => {
                        let text = crate::unicode::utf16_to_utf8(s);
                        match parse_bigint_string(&text) {
                            Ok(v) => v,
                            Err(_) => {
                                self.throw_syntax_error(ctx, &format!("Cannot convert {} to a BigInt", text));
                                return Some(Value::Undefined);
                            }
                        }
                    }
                    Value::Number(_) => {
                        self.throw_type_error(ctx, "Cannot convert a Number to a BigInt");
                        return Some(Value::Undefined);
                    }
                    _ => {
                        self.throw_type_error(ctx, "Cannot convert value to a BigInt");
                        return Some(Value::Undefined);
                    }
                };

                if bits == 0 {
                    return Some(Value::BigInt(Box::new(num_bigint::BigInt::from(0))));
                }

                let modulus = num_bigint::BigInt::from(1u8) << bits;
                let mut uint = as_bigint % &modulus;
                if uint < num_bigint::BigInt::from(0) {
                    uint += &modulus;
                }
                let out = if id == BUILTIN_BIGINT_ASUINTN {
                    Value::BigInt(Box::new(uint))
                } else {
                    let sign_bit = num_bigint::BigInt::from(1u8) << (bits - 1);
                    if uint >= sign_bit {
                        Value::BigInt(Box::new(uint - modulus))
                    } else {
                        Value::BigInt(Box::new(uint))
                    }
                };
                Some(out)
            }
            _ => None,
        }
    }

    pub(super) fn bigint_call_method_builtin(
        &mut self,
        ctx: &GcContext<'gc>,
        id: FunctionID,
        receiver: &Value<'gc>,
        args: &[Value<'gc>],
    ) -> Option<Value<'gc>> {
        if !matches!(id, BUILTIN_BIGINT_TOSTRING | BUILTIN_BIGINT_TOLOCALESTRING | BUILTIN_BIGINT_VALUEOF) {
            return None;
        }

        let bi_val = match receiver {
            Value::BigInt(b) => Some((**b).clone()),
            Value::VmObject(map) => {
                let b = map.borrow();
                if b.get("__type__").map(value_to_string).as_deref() == Some("BigInt") {
                    match b.get("__value__") {
                        Some(Value::BigInt(inner)) => Some((**inner).clone()),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            _ => None,
        };

        if let Some(bi) = bi_val {
            if id == BUILTIN_BIGINT_VALUEOF {
                return Some(Value::BigInt(Box::new(bi)));
            }
            let radix = if args.is_empty() || matches!(args.first(), Some(Value::Undefined)) {
                10u32
            } else {
                let radix_arg = args.first().unwrap();
                if matches!(radix_arg, Value::BigInt(_)) {
                    self.throw_type_error(ctx, "Cannot convert a BigInt value to a number");
                    return Some(Value::Undefined);
                }
                if radix_arg.is_symbol_value() {
                    self.throw_type_error(ctx, "Cannot convert a Symbol value to a number");
                    return Some(Value::Undefined);
                }
                match self.extract_number_with_coercion(ctx, radix_arg) {
                    Some(v) => v as u32,
                    None => return Some(Value::Undefined),
                }
            };
            if !(2..=36).contains(&radix) {
                let err = self.make_range_error_object(ctx, "toString() radix must be between 2 and 36");
                self.pending_throw = Some(err);
                return Some(Value::Undefined);
            }
            return Some(Value::from(&bi.to_str_radix(radix)));
        }

        self.throw_type_error(ctx, "BigInt.prototype method called on incompatible receiver");
        Some(Value::Undefined)
    }

    pub(super) fn value_to_bigint(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>) -> Option<num_bigint::BigInt> {
        let prim = self.try_to_primitive(ctx, value, "number");
        if self.pending_throw.is_some() {
            return None;
        }
        match &prim {
            Value::BigInt(bi) => Some((**bi).clone()),
            Value::Boolean(b) => Some(num_bigint::BigInt::from(if *b { 1 } else { 0 })),
            Value::String(s) => {
                let text = crate::unicode::utf16_to_utf8(s);
                match parse_bigint_string(&text) {
                    Ok(bi) => Some(bi),
                    Err(_) => {
                        self.throw_syntax_error(ctx, &format!("Cannot convert {} to a BigInt", text));
                        None
                    }
                }
            }
            Value::Number(_) => {
                self.throw_type_error(ctx, "Cannot convert a Number to a BigInt");
                None
            }
            Value::Undefined | Value::Null => {
                self.throw_type_error(ctx, "Cannot convert undefined to a BigInt");
                None
            }
            _ => {
                if prim.is_symbol_value() {
                    self.throw_type_error(ctx, "Cannot convert a Symbol value to a BigInt");
                } else {
                    self.throw_type_error(ctx, "Cannot convert value to a BigInt");
                }
                None
            }
        }
    }
}
