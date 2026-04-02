use super::*;
use regress::Regex;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static REGEX_CACHE: RefCell<HashMap<(Vec<u16>, String), Regex>> = RefCell::new(HashMap::new());
}

/// Compile a regex, returning a cached copy when the same pattern+flags
/// have been compiled before.
pub(crate) fn get_or_compile_regex(pattern: &[u16], flags: &str) -> Result<Regex, JSError> {
    REGEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let key = (pattern.to_vec(), flags.to_string());
        if let Some(re) = cache.get(&key) {
            return Ok(re.clone());
        }
        let re = create_regex_from_utf16(pattern, flags)?;
        cache.insert(key, re.clone());
        Ok(re)
    })
}

fn create_regex_from_utf16(pattern: &[u16], flags: &str) -> Result<Regex, JSError> {
    if flags.contains('u') || flags.contains('v') {
        let it = std::char::decode_utf16(pattern.iter().cloned()).map(|r| match r {
            Ok(c) => c as u32,
            Err(e) => e.unpaired_surrogate() as u32,
        });
        Regex::from_unicode(it, flags).map_err(|e| crate::raise_regexp_error!(e))
    } else {
        let processed = preprocess_pattern_non_unicode(pattern);
        Regex::from_unicode(processed.into_iter(), flags).map_err(|e| crate::raise_regexp_error!(e))
    }
}

/// For non-unicode regex patterns, pass raw UTF-16 code units to regress so
/// that supplementary characters are matched as two separate code units (via
/// `find_from_ucs2`).  However, named capture group identifiers (`(?<name>`)
/// and named backreferences (`\k<name>`) require valid Unicode identifier
/// characters, so surrogate pairs inside those contexts are decoded into full
/// code points.
fn preprocess_pattern_non_unicode(pattern: &[u16]) -> Vec<u32> {
    let mut result = Vec::with_capacity(pattern.len());
    let mut i = 0;
    let len = pattern.len();
    while i < len {
        if i + 3 <= len && pattern[i] == b'(' as u16 && pattern[i + 1] == b'?' as u16 && pattern[i + 2] == b'<' as u16 {
            if i + 3 < len && (pattern[i + 3] == b'=' as u16 || pattern[i + 3] == b'!' as u16) {
                result.push(pattern[i] as u32);
                i += 1;
                continue;
            }
            result.push(b'(' as u32);
            result.push(b'?' as u32);
            result.push(b'<' as u32);
            i += 3;
            while i < len && pattern[i] != b'>' as u16 {
                if i + 1 < len && (0xD800..=0xDBFF).contains(&pattern[i]) && (0xDC00..=0xDFFF).contains(&pattern[i + 1]) {
                    let hi = pattern[i] as u32;
                    let lo = pattern[i + 1] as u32;
                    result.push(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00));
                    i += 2;
                } else {
                    result.push(pattern[i] as u32);
                    i += 1;
                }
            }
            if i < len {
                result.push(pattern[i] as u32);
                i += 1;
            }
            continue;
        }
        if i + 3 <= len && pattern[i] == b'\\' as u16 && pattern[i + 1] == b'k' as u16 && pattern[i + 2] == b'<' as u16 {
            result.push(b'\\' as u32);
            result.push(b'k' as u32);
            result.push(b'<' as u32);
            i += 3;
            while i < len && pattern[i] != b'>' as u16 {
                if i + 1 < len && (0xD800..=0xDBFF).contains(&pattern[i]) && (0xDC00..=0xDFFF).contains(&pattern[i + 1]) {
                    let hi = pattern[i] as u32;
                    let lo = pattern[i + 1] as u32;
                    result.push(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00));
                    i += 2;
                } else {
                    result.push(pattern[i] as u32);
                    i += 1;
                }
            }
            if i < len {
                result.push(pattern[i] as u32);
                i += 1;
            }
            continue;
        }
        if pattern[i] == b'\\' as u16 && i + 1 < len {
            result.push(pattern[i] as u32);
            result.push(pattern[i + 1] as u32);
            i += 2;
            continue;
        }
        result.push(pattern[i] as u32);
        i += 1;
    }
    result
}

impl<'gc> VM<'gc> {
    /// Dispatch all `"regexp.*"` host function calls.
    pub(super) fn regexp_handle_host_fn(
        &mut self,
        ctx: &GcContext<'gc>,
        name: &str,
        receiver: Option<&Value<'gc>>,
        _args: &[Value<'gc>],
    ) -> Value<'gc> {
        match name {
            "regexp.toString" => match receiver {
                Some(Value::VmObject(re_obj)) => Value::from(&self.regex_to_string(re_obj)),
                _ => {
                    self.throw_type_error(ctx, "RegExp.prototype.toString called on incompatible receiver");
                    Value::Undefined
                }
            },
            "regexp.get_source" => {
                match receiver {
                    Some(Value::VmObject(re_obj)) => {
                        let borrow = re_obj.borrow();
                        if borrow.get("__type__").map(value_to_string).as_deref() == Some("RegExp") {
                            let raw = match borrow.get("__regex_pattern__") {
                                Some(v) => value_to_string(v),
                                None => String::new(),
                            };
                            if raw.is_empty() {
                                Value::from("(?:)")
                            } else {
                                // EscapeRegExpPattern: escape / and line terminators
                                let mut escaped = String::with_capacity(raw.len());
                                for ch in raw.chars() {
                                    match ch {
                                        '/' => escaped.push_str("\\/"),
                                        '\n' => escaped.push_str("\\n"),
                                        '\r' => escaped.push_str("\\r"),
                                        '\u{2028}' => escaped.push_str("\\u2028"),
                                        '\u{2029}' => escaped.push_str("\\u2029"),
                                        _ => escaped.push(ch),
                                    }
                                }
                                Value::from(&escaped)
                            }
                        } else if borrow.contains_key("__get_source") {
                            // RegExp.prototype itself
                            Value::from("(?:)")
                        } else {
                            drop(borrow);
                            self.throw_type_error(ctx, "RegExp.prototype.source getter called on incompatible receiver");
                            Value::Undefined
                        }
                    }
                    _ => {
                        self.throw_type_error(ctx, "RegExp.prototype.source getter called on incompatible receiver");
                        Value::Undefined
                    }
                }
            }
            "regexp.get_global"
            | "regexp.get_ignoreCase"
            | "regexp.get_multiline"
            | "regexp.get_sticky"
            | "regexp.get_dotAll"
            | "regexp.get_unicode"
            | "regexp.get_hasIndices"
            | "regexp.get_unicodeSets" => {
                let prop_name = &name[11..]; // strip "regexp.get_"
                let flag_char = match prop_name {
                    "global" => 'g',
                    "ignoreCase" => 'i',
                    "multiline" => 'm',
                    "sticky" => 'y',
                    "dotAll" => 's',
                    "unicode" => 'u',
                    "hasIndices" => 'd',
                    "unicodeSets" => 'v',
                    _ => unreachable!(),
                };
                match receiver {
                    Some(Value::VmObject(re_obj)) => {
                        let borrow = re_obj.borrow();
                        if borrow.get("__type__").map(value_to_string).as_deref() == Some("RegExp") {
                            let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
                            Value::Boolean(flags.contains(flag_char))
                        } else if borrow.contains_key(&format!("__get_{}", prop_name)) {
                            // RegExp.prototype itself
                            Value::Undefined
                        } else {
                            drop(borrow);
                            self.throw_type_error(
                                ctx,
                                &format!("RegExp.prototype.{} getter called on incompatible receiver", prop_name),
                            );
                            Value::Undefined
                        }
                    }
                    _ => {
                        self.throw_type_error(
                            ctx,
                            &format!("RegExp.prototype.{} getter called on incompatible receiver", prop_name),
                        );
                        Value::Undefined
                    }
                }
            }
            "regexp.get_flags" => {
                match receiver {
                    Some(recv @ Value::VmObject(_)) | Some(recv @ Value::VmArray(..)) => {
                        // Symbol wrapper objects should throw TypeError (they're primitives)
                        if let Value::VmObject(obj) = recv
                            && matches!(obj.borrow().get("__vm_symbol__"), Some(Value::Boolean(true)))
                        {
                            self.throw_type_error(ctx, "RegExp.prototype.flags getter called on incompatible receiver");
                            return Value::Undefined;
                        }
                        let recv = recv.clone();
                        let mut result = String::new();
                        let d = self.read_named_property(ctx, &recv, "hasIndices");
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        if d.to_truthy() {
                            result.push('d');
                        }
                        let g = self.read_named_property(ctx, &recv, "global");
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        if g.to_truthy() {
                            result.push('g');
                        }
                        let i = self.read_named_property(ctx, &recv, "ignoreCase");
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        if i.to_truthy() {
                            result.push('i');
                        }
                        let m = self.read_named_property(ctx, &recv, "multiline");
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        if m.to_truthy() {
                            result.push('m');
                        }
                        let s = self.read_named_property(ctx, &recv, "dotAll");
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        if s.to_truthy() {
                            result.push('s');
                        }
                        let u = self.read_named_property(ctx, &recv, "unicode");
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        if u.to_truthy() {
                            result.push('u');
                        }
                        let v = self.read_named_property(ctx, &recv, "unicodeSets");
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        if v.to_truthy() {
                            result.push('v');
                        }
                        let y = self.read_named_property(ctx, &recv, "sticky");
                        if self.pending_throw.is_some() {
                            return Value::Undefined;
                        }
                        if y.to_truthy() {
                            result.push('y');
                        }
                        Value::from(&result)
                    }
                    _ => {
                        self.throw_type_error(ctx, "RegExp.prototype.flags getter called on incompatible receiver");
                        Value::Undefined
                    }
                }
            }
            _ => {
                log::warn!("Unknown regexp host function: {}", name);
                Value::Undefined
            }
        }
    }

    /// Initialize RegExp prototype and constructor on the global object.
    pub(super) fn regexp_init_prototype(&mut self, ctx: &GcContext<'gc>) {
        let mut regexp_proto = IndexMap::new();
        if let Some(Value::VmObject(obj_ctor)) = self.globals.get("Object")
            && let Some(obj_proto) = obj_ctor.borrow().get("prototype").cloned()
        {
            regexp_proto.insert("__proto__".to_string(), obj_proto);
        }
        regexp_proto.insert("exec".to_string(), Self::make_native_fn(ctx, BUILTIN_REGEX_EXEC, "exec", 1.0));
        regexp_proto.insert("test".to_string(), Self::make_native_fn(ctx, BUILTIN_REGEX_TEST, "test", 1.0));
        regexp_proto.insert(
            "toString".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.toString", "toString", 0.0, false),
        );
        regexp_proto.insert(
            "__get_source".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_source", "get source", 0.0, false),
        );
        regexp_proto.insert(
            "__get_global".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_global", "get global", 0.0, false),
        );
        regexp_proto.insert(
            "__get_ignoreCase".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_ignoreCase", "get ignoreCase", 0.0, false),
        );
        regexp_proto.insert(
            "__get_multiline".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_multiline", "get multiline", 0.0, false),
        );
        regexp_proto.insert(
            "__get_sticky".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_sticky", "get sticky", 0.0, false),
        );
        regexp_proto.insert(
            "__get_dotAll".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_dotAll", "get dotAll", 0.0, false),
        );
        regexp_proto.insert(
            "__get_unicode".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_unicode", "get unicode", 0.0, false),
        );
        regexp_proto.insert(
            "__get_hasIndices".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_hasIndices", "get hasIndices", 0.0, false),
        );
        regexp_proto.insert(
            "__get_unicodeSets".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_unicodeSets", "get unicodeSets", 0.0, false),
        );
        regexp_proto.insert(
            "__get_flags".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_flags", "get flags", 0.0, false),
        );
        regexp_proto.insert("__nonenumerable_source__".to_string(), Value::Boolean(true));
        regexp_proto.insert("__nonenumerable_global__".to_string(), Value::Boolean(true));
        regexp_proto.insert("__nonenumerable_ignoreCase__".to_string(), Value::Boolean(true));
        regexp_proto.insert("__nonenumerable_multiline__".to_string(), Value::Boolean(true));
        regexp_proto.insert("__nonenumerable_sticky__".to_string(), Value::Boolean(true));
        regexp_proto.insert("__nonenumerable_dotAll__".to_string(), Value::Boolean(true));
        regexp_proto.insert("__nonenumerable_unicode__".to_string(), Value::Boolean(true));
        regexp_proto.insert("__nonenumerable_hasIndices__".to_string(), Value::Boolean(true));
        regexp_proto.insert("__nonenumerable_unicodeSets__".to_string(), Value::Boolean(true));
        regexp_proto.insert("__nonenumerable_flags__".to_string(), Value::Boolean(true));
        regexp_proto.insert("__nonenumerable_exec__".to_string(), Value::Boolean(true));
        regexp_proto.insert("__nonenumerable_test__".to_string(), Value::Boolean(true));
        regexp_proto.insert("__nonenumerable_toString__".to_string(), Value::Boolean(true));
        let regexp_proto_obj = new_gc_cell_ptr(ctx, regexp_proto);
        let mut regexp_ctor = IndexMap::new();
        Self::init_native_ctor_header(&mut regexp_ctor, BUILTIN_CTOR_REGEXP, "RegExp", 2.0);
        let regexp_ctor_val = Self::finalize_ctor_with_prototype(ctx, regexp_ctor, regexp_proto_obj);
        self.globals.insert("RegExp".to_string(), regexp_ctor_val);
    }

    /// Handle `RegExp(pattern, flags)` called without `new`.
    pub(super) fn regexp_call_builtin(&mut self, ctx: &GcContext<'gc>, args: &[Value<'gc>]) -> Value<'gc> {
        // Per spec: if pattern is RegExp and flags is undefined, return pattern if pattern.constructor === RegExp
        if let Some(pat @ Value::VmObject(pat_obj)) = args.first()
            && pat_obj.borrow().get("__type__").map(value_to_string).as_deref() == Some("RegExp")
            && matches!(args.get(1), None | Some(Value::Undefined))
        {
            let ctor = self.read_named_property(ctx, pat, "constructor");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            let regexp_ctor = self.globals.get("RegExp").cloned().unwrap_or(Value::Undefined);
            if self.values_same(&ctor, &regexp_ctor) {
                return pat.clone();
            }
        }
        // Otherwise create a new RegExp
        let (pattern, flags) = self.regexp_extract_pattern_flags(ctx, args);
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }
        if let Some(err_msg) = Self::validate_regexp_flags(&flags) {
            self.throw_syntax_error(ctx, &err_msg);
            return Value::Undefined;
        }
        // Validate pattern by attempting compilation
        let pattern_u16 = crate::unicode::utf8_to_utf16(&pattern);
        let regress_flags: String = flags.chars().filter(|c| "gimsuvy".contains(*c)).collect();
        if let Err(e) = get_or_compile_regex(&pattern_u16, &regress_flags) {
            self.throw_syntax_error(ctx, &format!("Invalid regular expression: /{}/: {}", pattern, e));
            return Value::Undefined;
        }
        let mut map = IndexMap::new();
        map.insert("__regex_pattern__".to_string(), Value::from(pattern.as_str()));
        map.insert("__regex_flags__".to_string(), Value::from(flags.as_str()));
        map.insert("__type__".to_string(), Value::from("RegExp"));
        map.insert("__toStringTag__".to_string(), Value::from("RegExp"));
        map.insert("lastIndex".to_string(), Value::Number(0.0));
        if let Some(Value::VmObject(ctor)) = self.globals.get("RegExp")
            && let Some(proto) = ctor.borrow().get("prototype").cloned()
        {
            map.insert("__proto__".to_string(), proto);
        }
        map.insert("__nonconfigurable_lastIndex__".to_string(), Value::Boolean(true));
        map.insert("__nonenumerable_lastIndex__".to_string(), Value::Boolean(true));
        Value::VmObject(new_gc_cell_ptr(ctx, map))
    }

    /// Handle `new RegExp(pattern, flags)` — initialize the receiver object.
    pub(super) fn regexp_call_method_builtin(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: &Value<'gc>,
        args: &[Value<'gc>],
    ) -> Option<Value<'gc>> {
        if let Value::VmObject(obj) = receiver {
            let (pattern, flags) = self.regexp_extract_pattern_flags(ctx, args);
            if self.pending_throw.is_some() {
                return Some(Value::Undefined);
            }
            if let Some(err_msg) = Self::validate_regexp_flags(&flags) {
                self.throw_syntax_error(ctx, &err_msg);
                return Some(Value::Undefined);
            }
            // Validate pattern by attempting compilation
            let pattern_u16 = crate::unicode::utf8_to_utf16(&pattern);
            let regress_flags: String = flags.chars().filter(|c| "gimsuvy".contains(*c)).collect();
            if let Err(e) = get_or_compile_regex(&pattern_u16, &regress_flags) {
                self.throw_syntax_error(ctx, &format!("Invalid regular expression: /{}/: {}", pattern, e));
                return Some(Value::Undefined);
            }
            let mut borrow = obj.borrow_mut(ctx);
            borrow.insert("__regex_pattern__".to_string(), Value::from(pattern.as_str()));
            borrow.insert("__regex_flags__".to_string(), Value::from(flags.as_str()));
            borrow.insert("__type__".to_string(), Value::from("RegExp"));
            borrow.insert("__toStringTag__".to_string(), Value::from("RegExp"));
            borrow.insert("lastIndex".to_string(), Value::Number(0.0));
            borrow.insert("__nonconfigurable_lastIndex__".to_string(), Value::Boolean(true));
            borrow.insert("__nonenumerable_lastIndex__".to_string(), Value::Boolean(true));
            return Some(receiver.clone());
        }
        None
    }

    /// Handle `RegExp.prototype.exec` dispatch.
    pub(super) fn regexp_exec_dispatch(&mut self, ctx: &GcContext<'gc>, receiver: &Value<'gc>, args: &[Value<'gc>]) -> Value<'gc> {
        if let Value::VmObject(map) = receiver {
            if map.borrow().get("__type__").map(value_to_string).as_deref() != Some("RegExp") {
                self.throw_type_error(ctx, "RegExp.prototype.exec called on incompatible receiver");
                return Value::Undefined;
            }
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            let prim = self.try_to_primitive(ctx, &arg, "string");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            let input = value_to_string(&prim);
            return self.regex_exec(ctx, map, &input);
        }
        self.throw_type_error(ctx, "RegExp.prototype.exec called on incompatible receiver");
        Value::Undefined
    }

    /// Handle `RegExp.prototype.test` dispatch.
    pub(super) fn regexp_test_dispatch(&mut self, ctx: &GcContext<'gc>, receiver: &Value<'gc>, args: &[Value<'gc>]) -> Value<'gc> {
        if let Value::VmObject(map) = receiver {
            if map.borrow().get("__type__").map(value_to_string).as_deref() != Some("RegExp") {
                self.throw_type_error(ctx, "RegExp.prototype.test called on incompatible receiver");
                return Value::Undefined;
            }
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            let prim = self.try_to_primitive(ctx, &arg, "string");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            let input = value_to_string(&prim);
            let result = self.regex_exec(ctx, map, &input);
            return Value::Boolean(!matches!(result, Value::Null));
        }
        self.throw_type_error(ctx, "RegExp.prototype.test called on incompatible receiver");
        Value::Undefined
    }

    /// String.prototype.split with RegExp separator.
    pub(super) fn regexp_string_split(
        &self,
        ctx: &GcContext<'gc>,
        rust_str: &str,
        re_obj: &VmObjectHandle<'gc>,
        limit: Option<usize>,
    ) -> Value<'gc> {
        let parts = self.regex_split_string(rust_str, re_obj, limit);
        let arr = new_gc_cell_ptr(ctx, VmArrayData::new(parts));
        if let Some(Value::VmObject(arr_ctor)) = self.globals.get("Array")
            && let Some(proto) = arr_ctor.borrow().get("prototype").cloned()
        {
            arr.borrow_mut(ctx).props.insert("__proto__".to_string(), proto);
        }
        Value::VmArray(arr)
    }

    /// String.prototype.replace with RegExp pattern.
    pub(super) fn regexp_string_replace(&self, rust_str: &str, re_obj: &VmObjectHandle<'gc>, replacement: &str) -> Value<'gc> {
        let result = self.regex_replace_string(rust_str, re_obj, replacement, false);
        Value::from(&result)
    }

    /// String.prototype.replaceAll with RegExp pattern.
    pub(super) fn regexp_string_replace_all(&self, rust_str: &str, re_obj: &VmObjectHandle<'gc>, replacement: &str) -> Value<'gc> {
        let result = self.regex_replace_string(rust_str, re_obj, replacement, true);
        Value::from(&result)
    }

    /// String.prototype.match with RegExp.
    pub(super) fn regexp_string_match(&mut self, ctx: &GcContext<'gc>, rust_str: &str, re_obj: &VmObjectHandle<'gc>) -> Value<'gc> {
        let borrow = re_obj.borrow();
        let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
        drop(borrow);
        if flags.contains('g') {
            self.regex_match_all(ctx, rust_str, re_obj)
        } else {
            self.regex_exec(ctx, re_obj, rust_str)
        }
    }

    /// String.prototype.search with RegExp.
    pub(super) fn regexp_string_search(&self, rust_str: &str, re_obj: &VmObjectHandle<'gc>) -> Value<'gc> {
        let borrow = re_obj.borrow();
        let pattern = borrow.get("__regex_pattern__").map(value_to_string).unwrap_or_default();
        let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
        drop(borrow);
        let pattern_u16 = crate::unicode::utf8_to_utf16(&pattern);
        if let Ok(re) = get_or_compile_regex(&pattern_u16, &flags) {
            let input_u16: Vec<u16> = rust_str.encode_utf16().collect();
            let use_unicode = flags.contains('u') || flags.contains('v');
            let m = if use_unicode {
                re.find_from_utf16(&input_u16, 0).next()
            } else {
                re.find_from_ucs2(&input_u16, 0).next()
            };
            return Value::Number(m.map(|m| m.range.start as f64).unwrap_or(-1.0));
        }
        Value::Number(-1.0)
    }

    // ── Internal helpers ──────────────────────────────────────────────

    /// Extract (pattern, flags) from constructor arguments, handling RegExp-as-source.
    fn regexp_extract_pattern_flags(&mut self, ctx: &GcContext<'gc>, args: &[Value<'gc>]) -> (String, String) {
        match args.first() {
            Some(Value::VmObject(pat_obj)) if pat_obj.borrow().get("__type__").map(value_to_string).as_deref() == Some("RegExp") => {
                let p = pat_obj.borrow().get("__regex_pattern__").map(value_to_string).unwrap_or_default();
                let f = if matches!(args.get(1), None | Some(Value::Undefined)) {
                    pat_obj.borrow().get("__regex_flags__").map(value_to_string).unwrap_or_default()
                } else {
                    self.vm_to_string(ctx, args.get(1).unwrap())
                };
                (p, f)
            }
            _ => {
                let p = match args.first() {
                    None | Some(Value::Undefined) => String::new(),
                    Some(v) => self.vm_to_string(ctx, v),
                };
                if self.pending_throw.is_some() {
                    return (p, String::new());
                }
                let f = match args.get(1) {
                    None | Some(Value::Undefined) => String::new(),
                    Some(v) => self.vm_to_string(ctx, v),
                };
                (p, f)
            }
        }
    }

    /// Validate RegExp flags per spec: only d,g,i,m,s,u,v,y allowed; no duplicates; u+v not together
    pub(super) fn validate_regexp_flags(flags: &str) -> Option<String> {
        let valid = "dgimsuy"; // v handled separately
        let mut seen = [false; 128];
        for ch in flags.chars() {
            if ch == 'v' {
                // v is valid
            } else if !valid.contains(ch) {
                return Some(format!("Invalid flags supplied to RegExp constructor '{}'", flags));
            }
            let c = ch as usize;
            if c < 128 {
                if seen[c] {
                    return Some(format!("Invalid flags supplied to RegExp constructor '{}'", flags));
                }
                seen[c] = true;
            }
        }
        // u and v cannot appear together
        if flags.contains('u') && flags.contains('v') {
            return Some(format!("Invalid flags supplied to RegExp constructor '{}'", flags));
        }
        None
    }

    fn regex_to_string(&self, re_obj: &VmObjectHandle<'gc>) -> String {
        let borrow = re_obj.borrow();
        let raw_pattern = borrow
            .get("__regex_pattern__")
            .map(value_to_string)
            .unwrap_or_else(|| borrow.get("source").map(value_to_string).unwrap_or_default());
        let flags = borrow
            .get("__regex_flags__")
            .map(value_to_string)
            .unwrap_or_else(|| borrow.get("flags").map(value_to_string).unwrap_or_default());
        // EscapeRegExpPattern
        let source = if raw_pattern.is_empty() {
            "(?:)".to_string()
        } else {
            let mut escaped = String::with_capacity(raw_pattern.len());
            for ch in raw_pattern.chars() {
                match ch {
                    '/' => escaped.push_str("\\/"),
                    '\n' => escaped.push_str("\\n"),
                    '\r' => escaped.push_str("\\r"),
                    '\u{2028}' => escaped.push_str("\\u2028"),
                    '\u{2029}' => escaped.push_str("\\u2029"),
                    _ => escaped.push(ch),
                }
            }
            escaped
        };
        format!("/{}/{}", source, flags)
    }

    fn regex_prepare_input(&self, input: &str, flags: &str) -> (Vec<u16>, bool) {
        let input_u16: Vec<u16> = input.encode_utf16().collect();
        if !flags.contains('R') {
            return (input_u16, false);
        }

        let mut normalized = Vec::with_capacity(input_u16.len());
        let mut index = 0usize;
        while index < input_u16.len() {
            if input_u16[index] == '\r' as u16 && index + 1 < input_u16.len() && input_u16[index + 1] == '\n' as u16 {
                normalized.push('\n' as u16);
                index += 2;
            } else {
                normalized.push(input_u16[index]);
                index += 1;
            }
        }
        (normalized, true)
    }

    fn regex_map_index_back(original: &[u16], normalized_index: usize) -> usize {
        let mut original_index = 0usize;
        let mut normalized_pos = 0usize;
        while normalized_pos < normalized_index && original_index < original.len() {
            if original[original_index] == '\r' as u16 && original_index + 1 < original.len() && original[original_index + 1] == '\n' as u16
            {
                original_index += 2;
            } else {
                original_index += 1;
            }
            normalized_pos += 1;
        }
        original_index
    }

    /// Execute a regex match, returning an array result or Null
    pub(super) fn regex_exec(&mut self, ctx: &GcContext<'gc>, re_obj: &VmObjectHandle<'gc>, input: &str) -> Value<'gc> {
        let borrow = re_obj.borrow();
        let pattern = borrow.get("__regex_pattern__").map(value_to_string).unwrap_or_default();
        let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
        let is_global = flags.contains('g');
        let is_sticky = flags.contains('y');
        let last_index_val = borrow.get("lastIndex").cloned().unwrap_or(Value::Number(0.0));
        drop(borrow);

        // ToLength(lastIndex) — must call valueOf on objects
        let last_index_num = match &last_index_val {
            Value::Number(n) => *n,
            Value::VmObject(_) | Value::VmArray(_) => {
                let prim = self.try_to_primitive(ctx, &last_index_val, "number");
                if self.pending_throw.is_some() {
                    return Value::Null;
                }
                to_number(&prim)
            }
            other => to_number(other),
        };
        // ToLength: clamp to [0, 2^53-1]
        let last_index_len = if last_index_num.is_nan() || last_index_num <= 0.0 {
            0usize
        } else {
            last_index_num.min(9007199254740991.0) as usize
        };
        let last_index = if is_global || is_sticky { last_index_len } else { 0 };

        let pattern_u16 = crate::unicode::utf8_to_utf16(&pattern);
        let regress_flags: String = flags.chars().filter(|flag| "gimsuvy".contains(*flag)).collect();
        let re = match get_or_compile_regex(&pattern_u16, &regress_flags) {
            Ok(r) => r,
            Err(_) => return Value::Null,
        };

        let input_u16: Vec<u16> = input.encode_utf16().collect();
        let (working_input, mapped_input) = self.regex_prepare_input(input, &flags);
        let match_result = if flags.contains('u') || flags.contains('v') {
            re.find_from_utf16(&working_input, last_index).next()
        } else {
            re.find_from_ucs2(&working_input, last_index).next()
        };

        match match_result {
            Some(m) if !is_sticky || m.range.start == last_index => {
                let (match_start, match_end) = if mapped_input {
                    (
                        Self::regex_map_index_back(&input_u16, m.range.start),
                        Self::regex_map_index_back(&input_u16, m.range.end),
                    )
                } else {
                    (m.range.start, m.range.end)
                };
                let matched_str = &input_u16[match_start..match_end];
                let matched = crate::unicode::utf16_to_utf8(matched_str);

                let mut result_items: Vec<Value<'gc>> = vec![Value::from(&matched)];
                // Add capturing groups
                for cap in &m.captures {
                    match cap {
                        Some(r) => {
                            let (cap_start, cap_end) = if mapped_input {
                                (
                                    Self::regex_map_index_back(&input_u16, r.start),
                                    Self::regex_map_index_back(&input_u16, r.end),
                                )
                            } else {
                                (r.start, r.end)
                            };
                            let s = &input_u16[cap_start..cap_end];
                            result_items.push(Value::String(s.to_vec()));
                        }
                        None => result_items.push(Value::Undefined),
                    }
                }

                let mut arr_data = VmArrayData::new(result_items);
                arr_data.props.insert("index".to_string(), Value::Number(match_start as f64));
                arr_data.props.insert("input".to_string(), Value::from(input));

                // Add indices array when 'd' (hasIndices) flag is set
                if flags.contains('d') {
                    let mut indices_items: Vec<Value<'gc>> = Vec::new();
                    // Full match indices
                    let pair = vec![Value::Number(match_start as f64), Value::Number(match_end as f64)];
                    indices_items.push(Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(pair))));
                    // Capturing group indices
                    for cap in &m.captures {
                        match cap {
                            Some(r) => {
                                let (cap_start, cap_end) = if mapped_input {
                                    (
                                        Self::regex_map_index_back(&input_u16, r.start),
                                        Self::regex_map_index_back(&input_u16, r.end),
                                    )
                                } else {
                                    (r.start, r.end)
                                };
                                let pair = vec![Value::Number(cap_start as f64), Value::Number(cap_end as f64)];
                                indices_items.push(Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(pair))));
                            }
                            None => indices_items.push(Value::Undefined),
                        }
                    }
                    arr_data.props.insert(
                        "indices".to_string(),
                        Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(indices_items))),
                    );
                }

                let arr = Value::VmArray(new_gc_cell_ptr(ctx, arr_data));

                // Update lastIndex for global/sticky
                if is_global || is_sticky {
                    if matches!(re_obj.borrow().get("__readonly_lastIndex__"), Some(Value::Boolean(true))) {
                        self.throw_type_error(ctx, "Cannot set property lastIndex of RegExp which has only a getter");
                        return Value::Null;
                    }
                    re_obj
                        .borrow_mut(ctx)
                        .insert("lastIndex".to_string(), Value::Number(match_end as f64));
                }

                arr
            }
            _ => {
                if is_global || is_sticky {
                    if matches!(re_obj.borrow().get("__readonly_lastIndex__"), Some(Value::Boolean(true))) {
                        self.throw_type_error(ctx, "Cannot set property lastIndex of RegExp which has only a getter");
                        return Value::Null;
                    }
                    re_obj.borrow_mut(ctx).insert("lastIndex".to_string(), Value::Number(0.0));
                }
                Value::Null
            }
        }
    }

    /// Global match: return array of all full match strings
    fn regex_match_all(&self, ctx: &GcContext<'gc>, input: &str, re_obj: &VmObjectHandle<'gc>) -> Value<'gc> {
        let borrow = re_obj.borrow();
        let pattern = borrow.get("__regex_pattern__").map(value_to_string).unwrap_or_default();
        let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
        drop(borrow);

        let pattern_u16 = crate::unicode::utf8_to_utf16(&pattern);
        let re = match get_or_compile_regex(&pattern_u16, &flags) {
            Ok(r) => r,
            Err(_) => return Value::Null,
        };

        let input_u16: Vec<u16> = input.encode_utf16().collect();
        let use_unicode = flags.contains('u') || flags.contains('v');
        let mut results: Vec<Value<'gc>> = Vec::new();
        let mut pos = 0usize;
        loop {
            let m = if use_unicode {
                re.find_from_utf16(&input_u16, pos).next()
            } else {
                re.find_from_ucs2(&input_u16, pos).next()
            };
            match m {
                Some(m) => {
                    let matched = &input_u16[m.range.start..m.range.end];
                    results.push(Value::String(matched.to_vec()));
                    pos = if m.range.end == m.range.start {
                        m.range.end + 1
                    } else {
                        m.range.end
                    };
                    if pos > input_u16.len() {
                        break;
                    }
                }
                None => break,
            }
        }
        if results.is_empty() {
            Value::Null
        } else {
            Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(results)))
        }
    }

    /// Replace string content using a regex pattern
    fn regex_replace_string(&self, input: &str, re_obj: &VmObjectHandle<'gc>, replacement: &str, replace_all: bool) -> String {
        let borrow = re_obj.borrow();
        let pattern = borrow.get("__regex_pattern__").map(value_to_string).unwrap_or_default();
        let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
        drop(borrow);

        let is_global = flags.contains('g');
        let pattern_u16 = crate::unicode::utf8_to_utf16(&pattern);
        let re = match get_or_compile_regex(&pattern_u16, &flags) {
            Ok(r) => r,
            Err(_) => return input.to_string(),
        };

        let input_u16: Vec<u16> = input.encode_utf16().collect();
        let use_unicode = flags.contains('u') || flags.contains('v');
        let mut result_u16: Vec<u16> = Vec::new();
        let mut pos = 0usize;
        let mut replaced = false;

        loop {
            let m = if use_unicode {
                re.find_from_utf16(&input_u16, pos).next()
            } else {
                re.find_from_ucs2(&input_u16, pos).next()
            };
            match m {
                Some(m) => {
                    // Append text before match
                    result_u16.extend_from_slice(&input_u16[pos..m.range.start]);
                    // Process replacement string with backreferences
                    let repl = self.apply_replacement(replacement, &input_u16, &m);
                    result_u16.extend_from_slice(&crate::unicode::utf8_to_utf16(&repl));
                    pos = m.range.end;
                    if pos == m.range.start {
                        pos += 1;
                    } // prevent infinite loop on zero-width match
                    replaced = true;
                    if !is_global && !replace_all {
                        break;
                    }
                    if pos > input_u16.len() {
                        break;
                    }
                }
                None => break,
            }
        }
        // Append remainder
        if pos <= input_u16.len() {
            result_u16.extend_from_slice(&input_u16[pos..]);
        }
        if !replaced {
            return input.to_string();
        }
        crate::unicode::utf16_to_utf8(&result_u16)
    }

    /// Apply replacement string backreferences ($1, $2, $&, etc.)
    fn apply_replacement(&self, replacement: &str, input_u16: &[u16], m: &regress::Match) -> String {
        let matched = crate::unicode::utf16_to_utf8(&input_u16[m.range.start..m.range.end]);
        let mut result = String::new();
        let chars: Vec<char> = replacement.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '$' && i + 1 < chars.len() {
                match chars[i + 1] {
                    '&' => {
                        result.push_str(&matched);
                        i += 2;
                    }
                    '`' => {
                        result.push_str(&crate::unicode::utf16_to_utf8(&input_u16[..m.range.start]));
                        i += 2;
                    }
                    '\'' => {
                        result.push_str(&crate::unicode::utf16_to_utf8(&input_u16[m.range.end..]));
                        i += 2;
                    }
                    '$' => {
                        result.push('$');
                        i += 2;
                    }
                    d if d.is_ascii_digit() => {
                        // Check for two-digit group reference ($10, $11, etc.)
                        let mut num_str = String::new();
                        num_str.push(d);
                        if i + 2 < chars.len() && chars[i + 2].is_ascii_digit() {
                            let two_digit = format!("{}{}", d, chars[i + 2]);
                            let two_num: usize = two_digit.parse().unwrap_or(0);
                            if two_num >= 1 && two_num <= m.captures.len() {
                                if let Some(Some(r)) = m.captures.get(two_num - 1) {
                                    result.push_str(&crate::unicode::utf16_to_utf8(&input_u16[r.start..r.end]));
                                }
                                i += 3;
                                continue;
                            }
                        }
                        let num: usize = num_str.parse().unwrap_or(0);
                        if num >= 1
                            && num <= m.captures.len()
                            && let Some(Some(r)) = m.captures.get(num - 1)
                        {
                            result.push_str(&crate::unicode::utf16_to_utf8(&input_u16[r.start..r.end]));
                        }
                        i += 2;
                    }
                    _ => {
                        result.push('$');
                        i += 1;
                    }
                }
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }
        result
    }

    /// Split a string using a regex separator, with optional capturing groups
    fn regex_split_string(&self, input: &str, re_obj: &VmObjectHandle<'gc>, limit: Option<usize>) -> Vec<Value<'gc>> {
        let borrow = re_obj.borrow();
        let pattern = borrow.get("__regex_pattern__").map(value_to_string).unwrap_or_default();
        let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
        drop(borrow);

        let pattern_u16 = crate::unicode::utf8_to_utf16(&pattern);
        let re = match get_or_compile_regex(&pattern_u16, &flags) {
            Ok(r) => r,
            Err(_) => return vec![Value::from(input)],
        };

        let input_u16: Vec<u16> = input.encode_utf16().collect();
        let use_unicode = flags.contains('u') || flags.contains('v');
        let mut results: Vec<Value<'gc>> = Vec::new();
        let max = limit.unwrap_or(usize::MAX);
        let mut pos = 0usize;

        loop {
            if results.len() >= max {
                break;
            }
            let m = if use_unicode {
                re.find_from_utf16(&input_u16, pos).next()
            } else {
                re.find_from_ucs2(&input_u16, pos).next()
            };
            match m {
                Some(m) if m.range.start < input_u16.len() => {
                    // Prevent infinite loop on zero-width match at same position
                    if m.range.start == m.range.end && m.range.start == pos {
                        pos += 1;
                        continue;
                    }
                    results.push(Value::String(input_u16[pos..m.range.start].to_vec()));
                    // Add capturing groups
                    for cap in &m.captures {
                        if results.len() >= max {
                            break;
                        }
                        match cap {
                            Some(r) => results.push(Value::String(input_u16[r.start..r.end].to_vec())),
                            None => results.push(Value::Undefined),
                        }
                    }
                    pos = m.range.end;
                }
                _ => break,
            }
        }
        if results.len() < max {
            results.push(Value::String(input_u16[pos..].to_vec()));
        }
        results
    }
}
