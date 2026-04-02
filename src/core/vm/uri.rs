use super::*;

impl<'gc> VM<'gc> {
    /// Dispatch all URI-related host function calls.
    pub(super) fn uri_handle_host_fn(&mut self, ctx: &GcContext<'gc>, name: &str, args: &[Value<'gc>]) -> Value<'gc> {
        match name {
            "global.encodeURI" | "global.encodeURIComponent" => {
                let arg = args.first().cloned().unwrap_or(Value::Undefined);
                let prim = self.try_to_primitive(ctx, &arg, "string");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                if Self::is_symbol_value(&prim) {
                    self.throw_type_error(ctx, "Cannot convert a Symbol value to a string");
                    return Value::Undefined;
                }
                let utf16 = Self::value_to_utf16(&prim);
                let is_component = name == "global.encodeURIComponent";
                match Self::js_encode_uri_utf16(&utf16, is_component) {
                    Ok(encoded) => Value::from(&encoded),
                    Err(msg) => {
                        self.throw_uri_error(ctx, &msg);
                        Value::Undefined
                    }
                }
            }
            "global.decodeURI" | "global.decodeURIComponent" => {
                let arg = args.first().cloned().unwrap_or(Value::Undefined);
                let prim = self.try_to_primitive(ctx, &arg, "string");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                if Self::is_symbol_value(&prim) {
                    self.throw_type_error(ctx, "Cannot convert a Symbol value to a string");
                    return Value::Undefined;
                }
                let utf16 = Self::value_to_utf16(&prim);
                let is_component = name == "global.decodeURIComponent";
                match Self::js_decode_uri_utf16(&utf16, is_component) {
                    Ok(decoded) => Value::String(decoded),
                    Err(msg) => {
                        self.throw_uri_error(ctx, &msg);
                        Value::Undefined
                    }
                }
            }
            _ => Value::Undefined,
        }
    }

    /// Register the four URI global functions.
    pub(super) fn uri_init_globals(&mut self, ctx: &GcContext<'gc>) {
        self.globals.insert(
            "encodeURI".to_string(),
            Self::make_host_fn_with_name_len(ctx, "global.encodeURI", "encodeURI", 1.0, false),
        );
        self.globals.insert(
            "decodeURI".to_string(),
            Self::make_host_fn_with_name_len(ctx, "global.decodeURI", "decodeURI", 1.0, false),
        );
        self.globals.insert(
            "encodeURIComponent".to_string(),
            Self::make_host_fn_with_name_len(ctx, "global.encodeURIComponent", "encodeURIComponent", 1.0, false),
        );
        self.globals.insert(
            "decodeURIComponent".to_string(),
            Self::make_host_fn_with_name_len(ctx, "global.decodeURIComponent", "decodeURIComponent", 1.0, false),
        );
    }

    // --- URI error helpers ---

    pub(super) fn make_uri_error_object(&self, ctx: &GcContext<'gc>, message: &str) -> Value<'gc> {
        let mut map = IndexMap::new();
        map.insert("__type__".to_string(), Value::from("URIError"));
        map.insert("name".to_string(), Value::from("URIError"));
        map.insert("message".to_string(), Value::from(message));
        if let Some(ctor) = self.globals.get("URIError").cloned() {
            map.insert("constructor".to_string(), ctor.clone());
            if let Value::VmObject(ctor_obj) = ctor
                && let Some(proto) = ctor_obj.borrow().get("prototype").cloned()
            {
                map.insert("__proto__".to_string(), proto);
            }
        }
        Value::VmObject(new_gc_cell_ptr(ctx, map))
    }

    pub(super) fn throw_uri_error(&mut self, ctx: &GcContext<'gc>, message: &str) {
        self.pending_throw = Some(self.make_uri_error_object(ctx, message));
    }

    // --- Core encode/decode logic ---

    fn js_encode_uri_utf16(utf16: &[u16], is_component: bool) -> Result<String, String> {
        let mut result = String::new();
        let len = utf16.len();
        let mut i = 0;

        while i < len {
            let c = utf16[i];
            if Self::is_uri_unescaped(c, is_component) {
                result.push(c as u8 as char);
                i += 1;
            } else {
                if (0xDC00..=0xDFFF).contains(&c) {
                    return Err("URI malformed".to_string());
                }
                let code_point: u32 = if (0xD800..=0xDBFF).contains(&c) {
                    i += 1;
                    if i >= len {
                        return Err("URI malformed".to_string());
                    }
                    let c2 = utf16[i];
                    if !(0xDC00..=0xDFFF).contains(&c2) {
                        return Err("URI malformed".to_string());
                    }
                    0x10000 + ((c as u32 - 0xD800) << 10) + (c2 as u32 - 0xDC00)
                } else {
                    c as u32
                };
                let mut utf8_bytes = [0u8; 4];
                let ch = char::from_u32(code_point).ok_or_else(|| "URI malformed".to_string())?;
                let encoded = ch.encode_utf8(&mut utf8_bytes);
                for b in encoded.bytes() {
                    result.push('%');
                    result.push(Self::hex_digit(b >> 4));
                    result.push(Self::hex_digit(b & 0x0F));
                }
                i += 1;
            }
        }
        Ok(result)
    }

    fn js_decode_uri_utf16(input: &[u16], is_component: bool) -> Result<Vec<u16>, String> {
        let mut result: Vec<u16> = Vec::new();
        let len = input.len();
        let mut i = 0;

        while i < len {
            let c = input[i];
            if c == 0x25 {
                // '%' character
                if i + 2 >= len {
                    return Err("URI malformed".to_string());
                }
                let hi = Self::hex_value_u16(input[i + 1]).ok_or_else(|| "URI malformed".to_string())?;
                let lo = Self::hex_value_u16(input[i + 2]).ok_or_else(|| "URI malformed".to_string())?;
                let b0 = (hi << 4) | lo;
                let start_i = i;
                i += 3;

                if b0 < 0x80 {
                    let ch = b0 as u8 as char;
                    if !is_component && Self::is_uri_reserved_or_hash(ch) {
                        // Keep original %XX
                        result.push(input[start_i]);
                        result.push(input[start_i + 1]);
                        result.push(input[start_i + 2]);
                    } else {
                        result.push(b0);
                    }
                } else {
                    let n = if b0 & 0xE0 == 0xC0 {
                        2
                    } else if b0 & 0xF0 == 0xE0 {
                        3
                    } else if b0 & 0xF8 == 0xF0 {
                        4
                    } else {
                        return Err("URI malformed".to_string());
                    };

                    let mut utf8_bytes = vec![b0 as u8];
                    for _ in 1..n {
                        if i + 2 > len || input[i] != 0x25 {
                            return Err("URI malformed".to_string());
                        }
                        let h = Self::hex_value_u16(input[i + 1]).ok_or_else(|| "URI malformed".to_string())?;
                        let l = Self::hex_value_u16(input[i + 2]).ok_or_else(|| "URI malformed".to_string())?;
                        let b = (h << 4) | l;
                        if b & 0xC0 != 0x80 {
                            return Err("URI malformed".to_string());
                        }
                        utf8_bytes.push(b as u8);
                        i += 3;
                    }

                    let decoded = std::str::from_utf8(&utf8_bytes).map_err(|_| "URI malformed".to_string())?;
                    let code_point = decoded.chars().next().ok_or_else(|| "URI malformed".to_string())? as u32;

                    if (0xD800..=0xDFFF).contains(&code_point) {
                        return Err("URI malformed".to_string());
                    }

                    if code_point <= 0xFFFF {
                        result.push(code_point as u16);
                    } else {
                        let cp = code_point - 0x10000;
                        result.push((0xD800 + (cp >> 10)) as u16);
                        result.push((0xDC00 + (cp & 0x3FF)) as u16);
                    }
                }
            } else {
                result.push(c);
                i += 1;
            }
        }

        Ok(result)
    }

    // --- Character classification helpers ---

    fn hex_value_u16(c: u16) -> Option<u16> {
        match c {
            0x30..=0x39 => Some(c - 0x30),      // '0'-'9'
            0x41..=0x46 => Some(c - 0x41 + 10), // 'A'-'F'
            0x61..=0x66 => Some(c - 0x61 + 10), // 'a'-'f'
            _ => None,
        }
    }

    fn is_uri_unescaped(c: u16, is_component: bool) -> bool {
        // A-Z a-z 0-9
        if (0x41..=0x5A).contains(&c) || (0x61..=0x7A).contains(&c) || (0x30..=0x39).contains(&c) {
            return true;
        }
        // - _ . ! ~ * ' ( )
        if matches!(c, 0x2D | 0x5F | 0x2E | 0x21 | 0x7E | 0x2A | 0x27 | 0x28 | 0x29) {
            return true;
        }
        // For encodeURI only: ; , / ? : @ & = + $ #
        if !is_component && matches!(c, 0x3B | 0x2C | 0x2F | 0x3F | 0x3A | 0x40 | 0x26 | 0x3D | 0x2B | 0x24 | 0x23) {
            return true;
        }
        false
    }

    fn is_uri_reserved_or_hash(c: char) -> bool {
        // Reserved set for decodeURI: ; / ? : @ & = + $ , #
        matches!(c, ';' | '/' | '?' | ':' | '@' | '&' | '=' | '+' | '$' | ',' | '#')
    }

    fn hex_digit(n: u8) -> char {
        if n < 10 { (b'0' + n) as char } else { (b'A' + n - 10) as char }
    }
}
