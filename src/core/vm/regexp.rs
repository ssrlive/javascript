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
    /// Check if `re_obj` is the "home" RegExp.prototype for the currently
    /// executing getter.  Uses `regexp_home_proto_temp` (set by the call-site
    /// before dispatching the host function) when available; falls back to
    /// `self.globals["RegExp"].prototype` for the common single-realm case.
    fn is_home_regexp_prototype(&self, re_obj: &VmObjectHandle<'gc>) -> bool {
        if let Some(Value::VmObject(home)) = &self.regexp_home_proto_temp {
            return Gc::ptr_eq(*re_obj, *home);
        }
        if let Some(Value::VmObject(regexp_ctor)) = self.globals.get("RegExp")
            && let Some(Value::VmObject(proto)) = regexp_ctor.borrow().get("prototype")
        {
            return Gc::ptr_eq(*re_obj, *proto);
        }
        false
    }

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
                            let raw_u16 = match borrow.get("__regex_pattern__") {
                                Some(Value::String(s)) => s.clone(),
                                Some(v) => crate::unicode::utf8_to_utf16(&value_to_string(v)),
                                None => Vec::new(),
                            };
                            if raw_u16.is_empty() {
                                Value::from("(?:)")
                            } else {
                                // EscapeRegExpPattern: escape / and line terminators
                                // Work directly on UTF-16 to preserve lone surrogates
                                let mut escaped: Vec<u16> = Vec::with_capacity(raw_u16.len());
                                for &cu in &raw_u16 {
                                    match cu {
                                        0x002F => {
                                            escaped.push(0x005C);
                                            escaped.push(0x002F);
                                        } // \/
                                        0x000A => {
                                            escaped.push(0x005C);
                                            escaped.push(b'n' as u16);
                                        } // \n
                                        0x000D => {
                                            escaped.push(0x005C);
                                            escaped.push(b'r' as u16);
                                        } // \r
                                        0x2028 => {
                                            // \u2028
                                            for c in "\\u2028".encode_utf16() {
                                                escaped.push(c);
                                            }
                                        }
                                        0x2029 => {
                                            // \u2029
                                            for c in "\\u2029".encode_utf16() {
                                                escaped.push(c);
                                            }
                                        }
                                        _ => escaped.push(cu),
                                    }
                                }
                                Value::String(escaped)
                            }
                        } else if self.is_home_regexp_prototype(re_obj) {
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
                        } else if self.is_home_regexp_prototype(re_obj) {
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
            "regexp.symbolMatch" => {
                let rx = match receiver {
                    Some(
                        v @ (Value::VmObject(_)
                        | Value::VmArray(_)
                        | Value::VmFunction(..)
                        | Value::VmClosure(..)
                        | Value::VmNativeFunction(_)),
                    ) if !v.is_symbol_value() => v.clone(),
                    _ => {
                        self.throw_type_error(ctx, "RegExp.prototype[Symbol.match] requires that 'this' be an Object");
                        return Value::Undefined;
                    }
                };
                let s_val = _args.first().cloned().unwrap_or(Value::Undefined);
                let s_str = match self.vm_to_string_like_spec(ctx, &s_val) {
                    Ok(s) => s,
                    Err(e) => {
                        self.pending_throw = Some(self.vm_value_from_error(ctx, &e));
                        return Value::Undefined;
                    }
                };
                self.regexp_symbol_match(ctx, &rx, &s_str)
            }
            "regexp.symbolMatchAll" => {
                let rx = match receiver {
                    Some(
                        v @ (Value::VmObject(_)
                        | Value::VmArray(_)
                        | Value::VmFunction(..)
                        | Value::VmClosure(..)
                        | Value::VmNativeFunction(_)),
                    ) if !v.is_symbol_value() => v.clone(),
                    _ => {
                        self.throw_type_error(ctx, "RegExp.prototype[Symbol.matchAll] requires that 'this' be an Object");
                        return Value::Undefined;
                    }
                };
                let s_val = _args.first().cloned().unwrap_or(Value::Undefined);
                let s_str = match self.vm_to_string_like_spec(ctx, &s_val) {
                    Ok(s) => s,
                    Err(e) => {
                        self.pending_throw = Some(self.vm_value_from_error(ctx, &e));
                        return Value::Undefined;
                    }
                };
                self.regexp_symbol_match_all(ctx, &rx, &s_str)
            }
            "regexp.symbolReplace" => {
                let rx = match receiver {
                    Some(
                        v @ (Value::VmObject(_)
                        | Value::VmArray(_)
                        | Value::VmFunction(..)
                        | Value::VmClosure(..)
                        | Value::VmNativeFunction(_)),
                    ) if !v.is_symbol_value() => v.clone(),
                    _ => {
                        self.throw_type_error(ctx, "RegExp.prototype[Symbol.replace] requires that 'this' be an Object");
                        return Value::Undefined;
                    }
                };
                let s_val = _args.first().cloned().unwrap_or(Value::Undefined);
                let s_str = match self.vm_to_string_like_spec(ctx, &s_val) {
                    Ok(s) => s,
                    Err(e) => {
                        self.pending_throw = Some(self.vm_value_from_error(ctx, &e));
                        return Value::Undefined;
                    }
                };
                let replace_value = _args.get(1).cloned().unwrap_or(Value::Undefined);
                self.regexp_symbol_replace(ctx, &rx, &s_str, &replace_value)
            }
            "regexp.symbolSearch" => {
                let rx = match receiver {
                    Some(
                        v @ (Value::VmObject(_)
                        | Value::VmArray(_)
                        | Value::VmFunction(..)
                        | Value::VmClosure(..)
                        | Value::VmNativeFunction(_)),
                    ) if !v.is_symbol_value() => v.clone(),
                    _ => {
                        self.throw_type_error(ctx, "RegExp.prototype[Symbol.search] requires that 'this' be an Object");
                        return Value::Undefined;
                    }
                };
                let s_val = _args.first().cloned().unwrap_or(Value::Undefined);
                let s_str = match self.vm_to_string_like_spec(ctx, &s_val) {
                    Ok(s) => s,
                    Err(e) => {
                        self.pending_throw = Some(self.vm_value_from_error(ctx, &e));
                        return Value::Undefined;
                    }
                };
                self.regexp_symbol_search(ctx, &rx, &s_str)
            }
            "regexp.symbolSplit" => {
                let rx = match receiver {
                    Some(
                        v @ (Value::VmObject(_)
                        | Value::VmArray(_)
                        | Value::VmFunction(..)
                        | Value::VmClosure(..)
                        | Value::VmNativeFunction(_)),
                    ) if !v.is_symbol_value() => v.clone(),
                    _ => {
                        self.throw_type_error(ctx, "RegExp.prototype[Symbol.split] requires that 'this' be an Object");
                        return Value::Undefined;
                    }
                };
                let s_val = _args.first().cloned().unwrap_or(Value::Undefined);
                let s_str = match self.vm_to_string_like_spec(ctx, &s_val) {
                    Ok(s) => s,
                    Err(e) => {
                        self.pending_throw = Some(self.vm_value_from_error(ctx, &e));
                        return Value::Undefined;
                    }
                };
                let limit = _args.get(1).cloned().unwrap_or(Value::Undefined);
                self.regexp_symbol_split(ctx, &rx, &s_str, &limit)
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
            make_getter_key("source"),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_source", "get source", 0.0, false),
        );
        regexp_proto.insert(
            make_getter_key("global"),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_global", "get global", 0.0, false),
        );
        regexp_proto.insert(
            make_getter_key("ignoreCase"),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_ignoreCase", "get ignoreCase", 0.0, false),
        );
        regexp_proto.insert(
            make_getter_key("multiline"),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_multiline", "get multiline", 0.0, false),
        );
        regexp_proto.insert(
            make_getter_key("sticky"),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_sticky", "get sticky", 0.0, false),
        );
        regexp_proto.insert(
            make_getter_key("dotAll"),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_dotAll", "get dotAll", 0.0, false),
        );
        regexp_proto.insert(
            make_getter_key("unicode"),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_unicode", "get unicode", 0.0, false),
        );
        regexp_proto.insert(
            make_getter_key("hasIndices"),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_hasIndices", "get hasIndices", 0.0, false),
        );
        regexp_proto.insert(
            make_getter_key("unicodeSets"),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_unicodeSets", "get unicodeSets", 0.0, false),
        );
        regexp_proto.insert(
            make_getter_key("flags"),
            Self::make_host_fn_with_name_len(ctx, "regexp.get_flags", "get flags", 0.0, false),
        );
        regexp_proto.insert(
            "@@sym:7".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.symbolMatch", "[Symbol.match]", 1.0, false),
        );
        regexp_proto.insert(
            "@@sym:8".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.symbolReplace", "[Symbol.replace]", 2.0, false),
        );
        regexp_proto.insert(
            "@@sym:9".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.symbolSearch", "[Symbol.search]", 1.0, false),
        );
        regexp_proto.insert(
            "@@sym:10".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.symbolSplit", "[Symbol.split]", 2.0, false),
        );
        regexp_proto.insert(
            "@@sym:11".to_string(),
            Self::make_host_fn_with_name_len(ctx, "regexp.symbolMatchAll", "[Symbol.matchAll]", 1.0, false),
        );
        regexp_proto.insert(make_nonenumerable_key("source"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("global"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("ignoreCase"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("multiline"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("sticky"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("dotAll"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("unicode"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("hasIndices"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("unicodeSets"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("flags"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("exec"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("test"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("toString"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("@@sym:7"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("@@sym:8"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("@@sym:9"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("@@sym:10"), Value::Boolean(true));
        regexp_proto.insert(make_nonenumerable_key("@@sym:11"), Value::Boolean(true));
        let regexp_proto_obj = new_gc_cell_ptr(ctx, regexp_proto);
        // Stamp each getter with a back-reference to this prototype so that
        // cross-realm identity checks work (spec: SameValue(R, %RegExpPrototype%)).
        Self::stamp_regexp_getters_with_home_proto(ctx, regexp_proto_obj);
        let mut regexp_ctor = IndexMap::new();
        Self::init_native_ctor_header(&mut regexp_ctor, BUILTIN_CTOR_REGEXP, "RegExp", 2.0);
        Self::insert_species_getter(&mut regexp_ctor, ctx);
        let regexp_ctor_val = Self::finalize_ctor_with_prototype(ctx, regexp_ctor, regexp_proto_obj);
        self.globals.insert("RegExp".to_string(), regexp_ctor_val);

        // Create %RegExpStringIteratorPrototype%
        // Prototype chain: RegExpStringIteratorPrototype → %IteratorPrototype% → Object.prototype
        let mut iter_proto = IndexMap::new();
        // Set __proto__ to %IteratorPrototype%
        if let Some(iter_proto_val) = self.globals.get("__IteratorPrototype__").cloned() {
            iter_proto.insert("__proto__".to_string(), iter_proto_val);
        } else if let Some(Value::VmObject(obj_ctor)) = self.globals.get("Object")
            && let Some(obj_proto) = obj_ctor.borrow().get("prototype").cloned()
        {
            iter_proto.insert("__proto__".to_string(), obj_proto);
        }
        // next method
        iter_proto.insert("next".to_string(), Self::make_native_fn(ctx, BUILTIN_ITERATOR_NEXT, "next", 0.0));
        // Symbol.toStringTag = "RegExp String Iterator" (non-writable, non-enumerable, configurable)
        iter_proto.insert("@@sym:4".to_string(), Value::from("RegExp String Iterator"));
        iter_proto.insert(make_nonenumerable_key("@@sym:4"), Value::Boolean(true));
        iter_proto.insert(make_readonly_key("@@sym:4"), Value::Boolean(true));
        iter_proto.insert("__configurable_@@sym:4__".to_string(), Value::Boolean(true));
        // Mark next as non-enumerable, writable, configurable
        iter_proto.insert(make_nonenumerable_key("next"), Value::Boolean(true));
        let iter_proto_val = Value::VmObject(new_gc_cell_ptr(ctx, iter_proto));
        self.globals.insert("RegExpStringIteratorPrototype".to_string(), iter_proto_val);
    }

    /// Set `__regexp_home_proto__` on each getter function in the given prototype
    /// so cross-realm identity checks can find the correct %RegExpPrototype%.
    pub(super) fn stamp_regexp_getters_with_home_proto(ctx: &GcContext<'gc>, proto: VmObjectHandle<'gc>) {
        let proto_val = Value::VmObject(proto);
        let getter_keys: Vec<String> = proto.borrow().keys().filter(|k| k.starts_with("__get_")).cloned().collect();
        for key in getter_keys {
            if let Some(Value::VmObject(getter_obj)) = proto.borrow().get(&key) {
                getter_obj
                    .borrow_mut(ctx)
                    .insert("__regexp_home_proto__".to_string(), proto_val.clone());
            }
        }
    }

    /// Handle `RegExp(pattern, flags)` called without `new`.
    pub(super) fn regexp_call_builtin(&mut self, ctx: &GcContext<'gc>, args: &[Value<'gc>]) -> Value<'gc> {
        let pattern = args.first().cloned().unwrap_or(Value::Undefined);
        let flags_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

        // Step 1: Let patternIsRegExp be ? IsRegExp(pattern).
        let pattern_is_regexp = match self.is_regexp_check(ctx, &pattern) {
            Ok(b) => b,
            Err(thrown) => {
                self.pending_throw = Some(thrown);
                return Value::Undefined;
            }
        };

        // Step 2: Called as function — if patternIsRegExp and flags undefined,
        //         check pattern.constructor === RegExp → return pattern.
        if pattern_is_regexp && matches!(flags_arg, Value::Undefined) {
            let ctor = match self.host_fn_read_property(ctx, &pattern, "constructor") {
                Ok(v) => v,
                Err(thrown) => {
                    self.pending_throw = Some(thrown);
                    return Value::Undefined;
                }
            };
            let regexp_ctor = self.globals.get("RegExp").cloned().unwrap_or(Value::Undefined);
            if self.values_same(&ctor, &regexp_ctor) {
                return pattern;
            }
        }

        // Steps 3-5: Resolve pattern and flags
        let (pattern_u16, flags) = self.regexp_extract_pattern_flags(ctx, &pattern, &flags_arg, pattern_is_regexp);
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }
        if let Some(err_msg) = Self::validate_regexp_flags(&flags) {
            self.throw_syntax_error(ctx, &err_msg);
            return Value::Undefined;
        }
        let regress_flags: String = flags.chars().filter(|c| "gimsuvy".contains(*c)).collect();
        if let Err(e) = get_or_compile_regex(&pattern_u16, &regress_flags) {
            let pat_str = crate::unicode::utf16_to_utf8(&pattern_u16);
            self.throw_syntax_error(ctx, &format!("Invalid regular expression: /{}/: {}", pat_str, e));
            return Value::Undefined;
        }
        let mut map = IndexMap::new();
        map.insert("__regex_pattern__".to_string(), Value::String(pattern_u16));
        map.insert("__regex_flags__".to_string(), Value::from(flags.as_str()));
        map.insert("__type__".to_string(), Value::from("RegExp"));
        map.insert("__toStringTag__".to_string(), Value::from("RegExp"));
        map.insert("lastIndex".to_string(), Value::Number(0.0));
        if let Some(Value::VmObject(ctor)) = self.globals.get("RegExp")
            && let Some(proto) = ctor.borrow().get("prototype").cloned()
        {
            map.insert("__proto__".to_string(), proto);
        }
        map.insert(make_nonconfigurable_key("lastIndex"), Value::Boolean(true));
        map.insert(make_nonenumerable_key("lastIndex"), Value::Boolean(true));
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
            let pattern = args.first().cloned().unwrap_or(Value::Undefined);
            let flags_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

            // Step 1: Let patternIsRegExp be ? IsRegExp(pattern).
            let pattern_is_regexp = match self.is_regexp_check(ctx, &pattern) {
                Ok(b) => b,
                Err(thrown) => {
                    self.pending_throw = Some(thrown);
                    return Some(Value::Undefined);
                }
            };

            let (pattern_u16, flags) = self.regexp_extract_pattern_flags(ctx, &pattern, &flags_arg, pattern_is_regexp);
            if self.pending_throw.is_some() {
                return Some(Value::Undefined);
            }
            if let Some(err_msg) = Self::validate_regexp_flags(&flags) {
                self.throw_syntax_error(ctx, &err_msg);
                return Some(Value::Undefined);
            }
            let regress_flags: String = flags.chars().filter(|c| "gimsuvy".contains(*c)).collect();
            if let Err(e) = get_or_compile_regex(&pattern_u16, &regress_flags) {
                let pat_str = crate::unicode::utf16_to_utf8(&pattern_u16);
                self.throw_syntax_error(ctx, &format!("Invalid regular expression: /{}/: {}", pat_str, e));
                return Some(Value::Undefined);
            }
            let mut borrow = obj.borrow_mut(ctx);
            borrow.insert("__regex_pattern__".to_string(), Value::String(pattern_u16));
            borrow.insert("__regex_flags__".to_string(), Value::from(flags.as_str()));
            borrow.insert("__type__".to_string(), Value::from("RegExp"));
            borrow.insert("__toStringTag__".to_string(), Value::from("RegExp"));
            borrow.insert("lastIndex".to_string(), Value::Number(0.0));
            borrow.insert(make_nonconfigurable_key("lastIndex"), Value::Boolean(true));
            borrow.insert(make_nonenumerable_key("lastIndex"), Value::Boolean(true));
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
        let pattern_u16 = Self::regexp_get_pattern_u16(re_obj);
        let flags = re_obj.borrow().get("__regex_flags__").map(value_to_string).unwrap_or_default();
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

    // ── Spec-compliant RegExpExec (ES2024 §22.2.5.2.1) ──────────────

    /// Abstract RegExpExec(R, S): Checks for a custom `exec` method on the
    /// object, calls it if callable (validating result), else falls back to
    /// RegExpBuiltinExec.
    fn regexp_abstract_exec(&mut self, ctx: &GcContext<'gc>, rx: &Value<'gc>, s: &str) -> Value<'gc> {
        // 1. Let exec be ? Get(R, "exec").
        let exec_val = self.read_named_property(ctx, rx, "exec");
        if self.pending_throw.is_some() {
            return Value::Null;
        }
        // 2. If IsCallable(exec) is true, then
        if self.is_value_callable(&exec_val) {
            let s_val = Value::from(s);
            let result = match self.vm_call_function_value(ctx, &exec_val, rx, &[s_val]) {
                Ok(v) => v,
                Err(e) => {
                    self.pending_throw = Some(self.vm_value_from_error(ctx, &e));
                    return Value::Null;
                }
            };
            if self.pending_throw.is_some() {
                return Value::Null;
            }
            // a. If result is not an Object and result is not null, throw a TypeError.
            // Note: Symbols are stored as VmObject with __vm_symbol__ but are primitives per spec.
            let is_object = match &result {
                Value::Null => true,
                Value::VmObject(obj) => !obj.borrow().contains_key("__vm_symbol__"),
                Value::VmArray(_) | Value::VmFunction(..) | Value::VmClosure(..) => true,
                _ => false,
            };
            if is_object {
                return result;
            }
            self.throw_type_error(ctx, "RegExp exec method returned something other than an Object or null");
            return Value::Null;
        }
        // 3. If R does not have a [[RegExpMatcher]] internal slot, throw a TypeError.
        match rx {
            Value::VmObject(obj) if obj.borrow().get("__regex_pattern__").is_some() => self.regex_exec(ctx, obj, s),
            _ => {
                self.throw_type_error(ctx, "RegExp.prototype.exec called on incompatible receiver");
                Value::Null
            }
        }
    }

    // ── Spec-compliant @@search (ES2024 §22.2.6.10) ────────────────

    fn regexp_symbol_search(&mut self, ctx: &GcContext<'gc>, rx: &Value<'gc>, s: &str) -> Value<'gc> {
        // 1. Let previousLastIndex be ? Get(rx, "lastIndex").
        let previous_last_index = self.read_named_property(ctx, rx, "lastIndex");
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }

        // 2. If SameValue(previousLastIndex, +0𝔽) is false, then Perform ? Set(rx, "lastIndex", +0𝔽, true).
        let is_zero = match &previous_last_index {
            Value::Number(n) => *n == 0.0 && !n.is_sign_negative(),
            _ => false,
        };
        if !is_zero && let Err(thrown) = self.host_fn_set_property(ctx, rx, "lastIndex", &Value::Number(0.0)) {
            self.pending_throw = Some(thrown);
            return Value::Undefined;
        }

        // 3. Let result be ? RegExpExec(rx, S).
        let result = self.regexp_abstract_exec(ctx, rx, s);
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }

        // 4. Let currentLastIndex be ? Get(rx, "lastIndex").
        let current_last_index = self.read_named_property(ctx, rx, "lastIndex");
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }

        // 5. If SameValue(currentLastIndex, previousLastIndex) is false, then
        //    Perform ? Set(rx, "lastIndex", previousLastIndex, true).
        if !self.values_same(&current_last_index, &previous_last_index)
            && let Err(thrown) = self.host_fn_set_property(ctx, rx, "lastIndex", &previous_last_index)
        {
            self.pending_throw = Some(thrown);
            return Value::Undefined;
        }

        // 6. If result is null, return -1𝔽.
        if matches!(result, Value::Null) {
            return Value::Number(-1.0);
        }

        // 7. Return ? Get(result, "index").
        let index = self.read_named_property(ctx, &result, "index");
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }
        index
    }

    // ── Spec-compliant @@match (ES2024 §22.2.6.8) ──────────────────

    fn regexp_symbol_match(&mut self, ctx: &GcContext<'gc>, rx: &Value<'gc>, s: &str) -> Value<'gc> {
        // 1. Let flags be ? ToString(? Get(rx, "flags")).
        let flags_val = self.read_named_property(ctx, rx, "flags");
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }
        let flags = match self.vm_to_string_like_spec(ctx, &flags_val) {
            Ok(s) => s,
            Err(e) => {
                self.pending_throw = Some(self.vm_value_from_error(ctx, &e));
                return Value::Undefined;
            }
        };

        let global = flags.contains('g');

        // 2. If global is false, then
        if !global {
            // a. Return ? RegExpExec(rx, S).
            return self.regexp_abstract_exec(ctx, rx, s);
        }

        // 3. Else (global is true)
        let full_unicode = flags.contains('u') || flags.contains('v');

        // a. Perform ? Set(rx, "lastIndex", +0𝔽, true).
        if let Err(thrown) = self.host_fn_set_property(ctx, rx, "lastIndex", &Value::Number(0.0)) {
            self.pending_throw = Some(thrown);
            return Value::Undefined;
        }

        let mut results: Vec<Value<'gc>> = Vec::new();
        loop {
            // b. Let result be ? RegExpExec(rx, S).
            let result = self.regexp_abstract_exec(ctx, rx, s);
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }

            // c. If result is null, then
            if matches!(result, Value::Null) {
                return if results.is_empty() {
                    Value::Null
                } else {
                    Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(results)))
                };
            }

            // d. Let matchStr be ? ToString(? Get(result, "0")).
            let match_val = self.read_named_property(ctx, &result, "0");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            let match_str = self.vm_to_string(ctx, &match_val);
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }

            results.push(Value::from(&match_str));

            // e. If matchStr is the empty String, then advance lastIndex
            if match_str.is_empty() {
                let this_index_val = self.read_named_property(ctx, rx, "lastIndex");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                // ToLength: first ToPrimitive→ToNumber, then clamp
                let prim = self.try_to_primitive(ctx, &this_index_val, "number");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let n = to_number(&prim);
                let this_index = if n.is_nan() || n <= 0.0 {
                    0usize
                } else {
                    n.min(9007199254740991.0) as usize
                };
                let next_index = if full_unicode {
                    self.advance_string_index_unicode(s, this_index)
                } else {
                    this_index + 1
                };
                if let Err(thrown) = self.host_fn_set_property(ctx, rx, "lastIndex", &Value::Number(next_index as f64)) {
                    self.pending_throw = Some(thrown);
                    return Value::Undefined;
                }
            }
        }
    }

    // ── Spec-compliant @@replace (ES2024 §22.2.6.9) ────────────────

    fn regexp_symbol_replace(&mut self, ctx: &GcContext<'gc>, rx: &Value<'gc>, s: &str, replace_value: &Value<'gc>) -> Value<'gc> {
        let s_u16: Vec<u16> = s.encode_utf16().collect();
        let length_s = s_u16.len();

        // 1. Let flags be ? ToString(? Get(rx, "flags")).
        let flags_val = self.read_named_property(ctx, rx, "flags");
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }
        let flags = match self.vm_to_string_like_spec(ctx, &flags_val) {
            Ok(s) => s,
            Err(e) => {
                self.pending_throw = Some(self.vm_value_from_error(ctx, &e));
                return Value::Undefined;
            }
        };

        let global = flags.contains('g');
        let full_unicode = flags.contains('u') || flags.contains('v');
        let functional_replace = self.is_value_callable(replace_value);

        // Step 7: If functionalReplace is false, let replaceValue be ? ToString(replaceValue).
        let replace_str_owned: Option<String> = if !functional_replace {
            let rv_str = match self.vm_to_string_like_spec(ctx, replace_value) {
                Ok(s) => s,
                Err(e) => {
                    self.pending_throw = Some(self.vm_value_from_error(ctx, &e));
                    return Value::Undefined;
                }
            };
            Some(rv_str)
        } else {
            None
        };

        if global && let Err(thrown) = self.host_fn_set_property(ctx, rx, "lastIndex", &Value::Number(0.0)) {
            self.pending_throw = Some(thrown);
            return Value::Undefined;
        }

        // Collect results
        let mut results: Vec<Value<'gc>> = Vec::new();
        loop {
            let result = self.regexp_abstract_exec(ctx, rx, s);
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }

            if matches!(result, Value::Null) {
                break;
            }
            results.push(result.clone());

            if !global {
                break;
            }

            // If matchStr is "", advance lastIndex
            let match_val = self.read_named_property(ctx, &result, "0");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            let match_str = self.vm_to_string(ctx, &match_val);
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            if match_str.is_empty() {
                let this_index_val = self.read_named_property(ctx, rx, "lastIndex");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let prim = self.try_to_primitive(ctx, &this_index_val, "number");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let n = to_number(&prim);
                let this_index = if n.is_nan() || n <= 0.0 {
                    0usize
                } else {
                    n.min(9007199254740991.0) as usize
                };
                let next_index = if full_unicode {
                    self.advance_string_index_unicode(s, this_index)
                } else {
                    this_index + 1
                };
                if let Err(thrown) = self.host_fn_set_property(ctx, rx, "lastIndex", &Value::Number(next_index as f64)) {
                    self.pending_throw = Some(thrown);
                    return Value::Undefined;
                }
            }
        }

        // Build accumulated result
        let mut acc_u16: Vec<u16> = Vec::new();
        let mut next_source_position: usize = 0;

        for result in &results {
            // Get nCaptures = max(ToLength(Get(result, "length")) - 1, 0)
            let n_captures = {
                let len_val = self.read_named_property(ctx, result, "length");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let prim = self.try_to_primitive(ctx, &len_val, "number");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let n = to_number(&prim);
                let len = if n.is_nan() || n <= 0.0 {
                    0i64
                } else {
                    n.min(9007199254740991.0) as i64
                };
                if len > 1 { (len - 1) as usize } else { 0 }
            };

            let matched_val = self.read_named_property(ctx, result, "0");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            let matched = self.vm_to_string(ctx, &matched_val);
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            let matched_u16: Vec<u16> = matched.encode_utf16().collect();
            let match_length = matched_u16.len();

            // position = max(min(ToIntegerOrInfinity(Get(result, "index")), lengthS), 0)
            let position_val = self.read_named_property(ctx, result, "index");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            let prim = self.try_to_primitive(ctx, &position_val, "number");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            let pos_n = to_number(&prim);
            let position = if pos_n.is_nan() || pos_n <= 0.0 {
                0usize
            } else {
                (pos_n as usize).min(length_s)
            };

            // Collect captures
            let mut captures: Vec<Value<'gc>> = Vec::new();
            for i in 1..=n_captures {
                let cap = self.read_named_property(ctx, result, &i.to_string());
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let cap = if matches!(cap, Value::Undefined) {
                    cap
                } else {
                    let s = self.vm_to_string(ctx, &cap);
                    if self.pending_throw.is_some() {
                        return Value::Undefined;
                    }
                    Value::from(&s)
                };
                captures.push(cap);
            }

            // Get named captures
            let named_captures = self.read_named_property(ctx, result, "groups");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }

            let replacement: String;
            if functional_replace {
                // Build args: matched, ...captures, position, S[, namedCaptures]
                let mut call_args: Vec<Value<'gc>> = Vec::new();
                call_args.push(Value::from(&matched));
                call_args.extend(captures.iter().cloned());
                call_args.push(Value::Number(position as f64));
                call_args.push(Value::from(s));
                if !matches!(named_captures, Value::Undefined) {
                    call_args.push(named_captures);
                }
                let replace_result = match self.vm_call_function_value(ctx, replace_value, &Value::Undefined, &call_args) {
                    Ok(v) => v,
                    Err(e) => {
                        self.pending_throw = Some(self.vm_value_from_error(ctx, &e));
                        return Value::Undefined;
                    }
                };
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                replacement = self.vm_to_string(ctx, &replace_result);
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
            } else {
                let replace_str = replace_str_owned.as_ref().unwrap();
                replacement = self.get_substitution(&matched, s, position, &captures, &named_captures, replace_str);
            }

            if position >= next_source_position {
                acc_u16.extend_from_slice(&s_u16[next_source_position..position]);
                acc_u16.extend(replacement.encode_utf16());
                next_source_position = position + match_length;
            }
        }

        if next_source_position < length_s {
            acc_u16.extend_from_slice(&s_u16[next_source_position..]);
        }

        Value::String(acc_u16)
    }

    /// GetSubstitution (ES2024 §22.1.3.18.1)
    fn get_substitution(
        &mut self,
        matched: &str,
        s: &str,
        position: usize,
        captures: &[Value<'gc>],
        named_captures: &Value<'gc>,
        replacement: &str,
    ) -> String {
        let s_u16: Vec<u16> = s.encode_utf16().collect();
        let matched_u16: Vec<u16> = matched.encode_utf16().collect();
        let chars: Vec<char> = replacement.chars().collect();
        let mut result = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '$' && i + 1 < chars.len() {
                match chars[i + 1] {
                    '$' => {
                        result.push('$');
                        i += 2;
                    }
                    '&' => {
                        result.push_str(matched);
                        i += 2;
                    }
                    '`' => {
                        let before = &s_u16[..position.min(s_u16.len())];
                        result.push_str(&crate::unicode::utf16_to_utf8(before));
                        i += 2;
                    }
                    '\'' => {
                        let tail_pos = (position + matched_u16.len()).min(s_u16.len());
                        let after = &s_u16[tail_pos..];
                        result.push_str(&crate::unicode::utf16_to_utf8(after));
                        i += 2;
                    }
                    '<' => {
                        // Named capture: $<name>
                        if matches!(named_captures, Value::Undefined) {
                            result.push_str("$<");
                            i += 2;
                        } else if let Some(close) = chars[i + 2..].iter().position(|&c| c == '>') {
                            let name: String = chars[i + 2..i + 2 + close].iter().collect();
                            let capture_val = self.read_named_property_str(named_captures, &name);
                            if !matches!(capture_val, Value::Undefined) {
                                result.push_str(&value_to_string(&capture_val));
                            }
                            i += 2 + close + 1; // skip $< name >
                        } else {
                            result.push_str("$<");
                            i += 2;
                        }
                    }
                    d if d.is_ascii_digit() => {
                        // Check for two-digit group reference
                        let d1 = d.to_digit(10).unwrap() as usize;
                        if i + 2 < chars.len() && chars[i + 2].is_ascii_digit() {
                            let d2 = chars[i + 2].to_digit(10).unwrap() as usize;
                            let two_digit = d1 * 10 + d2;
                            if two_digit >= 1 && two_digit <= captures.len() {
                                let cap = &captures[two_digit - 1];
                                if !matches!(cap, Value::Undefined) {
                                    result.push_str(&value_to_string(cap));
                                }
                                i += 3;
                                continue;
                            }
                        }
                        if d1 >= 1 && d1 <= captures.len() {
                            let cap = &captures[d1 - 1];
                            if !matches!(cap, Value::Undefined) {
                                result.push_str(&value_to_string(cap));
                            }
                            i += 2;
                        } else if d1 == 0 {
                            result.push_str("$0");
                            i += 2;
                        } else {
                            result.push('$');
                            result.push(d);
                            i += 2;
                        }
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

    // ── Spec-compliant @@split (ES2024 §22.2.6.11) ─────────────────

    fn regexp_symbol_split(&mut self, ctx: &GcContext<'gc>, rx: &Value<'gc>, s: &str, limit: &Value<'gc>) -> Value<'gc> {
        // For @@split, we use the built-in regex directly (similar to current approach)
        // but follow the spec's observable steps for type checking and limit handling.

        let s_u16: Vec<u16> = s.encode_utf16().collect();
        let size = s_u16.len();

        // 1. Let flags be ? ToString(? Get(rx, "flags")).
        let flags_val = self.read_named_property(ctx, rx, "flags");
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }
        let flags = match self.vm_to_string_like_spec(ctx, &flags_val) {
            Ok(s) => s,
            Err(e) => {
                self.pending_throw = Some(self.vm_value_from_error(ctx, &e));
                return Value::Undefined;
            }
        };

        let unicode_matching = flags.contains('u') || flags.contains('v');

        // Build new flags with 'y' (sticky) added
        let new_flags = if flags.contains('y') {
            flags.clone()
        } else {
            format!("{}y", flags)
        };

        // Step 5: Let C be ? SpeciesConstructor(rx, %RegExp%).
        let regexp_ctor = self.globals.get("RegExp").cloned().unwrap_or(Value::Undefined);
        let c = self.species_constructor(ctx, rx, &regexp_ctor);
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }

        // Step 6: Let splitter be ? Construct(C, « rx, newFlags »).
        let rx_arg = rx.clone();
        let flags_arg = Value::from(&*new_flags);
        let splitter = match self.construct_value(ctx, &c, &[rx_arg, flags_arg], None) {
            Ok(v) => v,
            Err(e) => {
                self.pending_throw = Some(self.vm_value_from_error(ctx, &e));
                return Value::Undefined;
            }
        };
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }

        let mut results: Vec<Value<'gc>> = Vec::new();
        let lim = match limit {
            Value::Undefined => 0xFFFFFFFFu32,
            v => {
                // ToUint32(limit): ToNumber, then modulo 2^32
                let n = match self.host_fn_to_number(ctx, v) {
                    Ok(n) => n,
                    Err(thrown) => {
                        self.pending_throw = Some(thrown);
                        return Value::Undefined;
                    }
                };
                if n.is_nan() || n.is_infinite() || n == 0.0 {
                    0u32
                } else {
                    let int_val = n.signum() * n.abs().floor();
                    let modulo = int_val % 4294967296.0;
                    let result = if modulo < 0.0 { modulo + 4294967296.0 } else { modulo };
                    result as u32
                }
            }
        };

        if lim == 0 {
            return Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(results)));
        }

        if size == 0 {
            let z = self.regexp_abstract_exec(ctx, &splitter, s);
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            if !matches!(z, Value::Null) {
                return Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(results)));
            }
            results.push(Value::from(s));
            return Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(results)));
        }

        let mut p: usize = 0; // previous match end
        let mut q: usize = 0; // current position

        while q < size {
            // Set splitter.lastIndex = q
            if let Err(thrown) = self.host_fn_set_property(ctx, &splitter, "lastIndex", &Value::Number(q as f64)) {
                self.pending_throw = Some(thrown);
                return Value::Undefined;
            }

            let z = self.regexp_abstract_exec(ctx, &splitter, s);
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }

            if matches!(z, Value::Null) {
                q = if unicode_matching {
                    self.advance_string_index_unicode(s, q)
                } else {
                    q + 1
                };
                continue;
            }

            // Get the actual lastIndex after exec — ToLength coercion
            let e_val = self.read_named_property(ctx, &splitter, "lastIndex");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            let e = match self.host_fn_to_length(ctx, &e_val) {
                Ok(n) => (n as usize).min(size),
                Err(thrown) => {
                    self.pending_throw = Some(thrown);
                    return Value::Undefined;
                }
            };

            if e == p {
                q = if unicode_matching {
                    self.advance_string_index_unicode(s, q)
                } else {
                    q + 1
                };
                continue;
            }

            // Add the substring before this match
            results.push(Value::String(s_u16[p..q].to_vec()));
            if results.len() as u32 >= lim {
                return Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(results)));
            }

            // Add capturing groups
            let n_captures = {
                let len_val = self.read_named_property(ctx, &z, "length");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let len = match self.host_fn_to_length(ctx, &len_val) {
                    Ok(n) => n as i64,
                    Err(thrown) => {
                        self.pending_throw = Some(thrown);
                        return Value::Undefined;
                    }
                };
                if len > 1 { (len - 1) as usize } else { 0 }
            };

            for i in 1..=n_captures {
                let cap = self.read_named_property(ctx, &z, &i.to_string());
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                results.push(cap);
                if results.len() as u32 >= lim {
                    return Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(results)));
                }
            }

            p = e;
            q = p;
        }

        // Add the tail
        results.push(Value::String(s_u16[p..size].to_vec()));
        Value::VmArray(new_gc_cell_ptr(ctx, VmArrayData::new(results)))
    }

    // ── Spec-compliant @@matchAll (ES2024 §22.2.6.9) ───────────────

    fn regexp_symbol_match_all(&mut self, ctx: &GcContext<'gc>, rx: &Value<'gc>, s: &str) -> Value<'gc> {
        // ES2024 §22.2.6.9 RegExp.prototype[@@matchAll]

        // 1. Let flags be ? ToString(? Get(rx, "flags")).
        let flags_val = self.read_named_property(ctx, rx, "flags");
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }
        let flags = match self.vm_to_string_like_spec(ctx, &flags_val) {
            Ok(s) => s,
            Err(e) => {
                self.pending_throw = Some(self.vm_value_from_error(ctx, &e));
                return Value::Undefined;
            }
        };

        let global = flags.contains('g');
        let full_unicode = flags.contains('u') || flags.contains('v');

        // Step 3: Let C be ? SpeciesConstructor(rx, %RegExp%).
        let regexp_ctor = self.globals.get("RegExp").cloned().unwrap_or(Value::Undefined);
        let c = self.species_constructor(ctx, rx, &regexp_ctor);
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }

        // Step 4: Let matcher be ? Construct(C, « rx, flags »).
        let rx_arg = rx.clone();
        let flags_arg = Value::from(&*flags);
        let matcher = match self.construct_value(ctx, &c, &[rx_arg, flags_arg], None) {
            Ok(v) => v,
            Err(e) => {
                self.pending_throw = Some(self.vm_value_from_error(ctx, &e));
                return Value::Undefined;
            }
        };
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }

        // Set matcher.lastIndex = ? ToLength(? Get(rx, "lastIndex"))
        let last_index_val = self.read_named_property(ctx, rx, "lastIndex");
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }
        // ToLength coercion
        let prim = self.try_to_primitive(ctx, &last_index_val, "number");
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }
        let n = to_number(&prim);
        let last_index_len = if n.is_nan() || n <= 0.0 {
            0.0
        } else {
            n.min(9007199254740991.0).floor()
        };
        if let Err(thrown) = self.host_fn_set_property(ctx, &matcher, "lastIndex", &Value::Number(last_index_len)) {
            self.pending_throw = Some(thrown);
            return Value::Undefined;
        }

        // Build a RegExpStringIterator object
        let iter_obj = new_gc_cell_ptr(ctx, IndexMap::new());
        {
            let mut b = iter_obj.borrow_mut(ctx);
            b.insert("__type__".to_string(), Value::from("RegExpStringIterator"));
            b.insert("__iter_regexp__".to_string(), matcher);
            b.insert("__iter_string__".to_string(), Value::from(s));
            b.insert("__iter_global__".to_string(), Value::Boolean(global));
            b.insert("__iter_unicode__".to_string(), Value::Boolean(full_unicode));
            b.insert("__iter_done__".to_string(), Value::Boolean(false));
        }

        // Set prototype
        if let Some(proto) = self.globals.get("RegExpStringIteratorPrototype").cloned() {
            iter_obj.borrow_mut(ctx).insert("__proto__".to_string(), proto);
        }

        Value::VmObject(iter_obj)
    }

    /// %RegExpStringIteratorPrototype%.next() — ES2024 §22.2.9.1
    pub(super) fn regexp_string_iterator_next(&mut self, ctx: &GcContext<'gc>, obj: &VmObjectHandle<'gc>) -> Value<'gc> {
        // Read internal slots
        let (done, global, full_unicode, regexp, string_val) = {
            let borrow = obj.borrow();
            let done = matches!(borrow.get("__iter_done__"), Some(Value::Boolean(true)));
            let global = matches!(borrow.get("__iter_global__"), Some(Value::Boolean(true)));
            let full_unicode = matches!(borrow.get("__iter_unicode__"), Some(Value::Boolean(true)));
            let regexp = borrow.get("__iter_regexp__").cloned().unwrap_or(Value::Undefined);
            let string_val = borrow.get("__iter_string__").cloned().unwrap_or(Value::Undefined);
            (done, global, full_unicode, regexp, string_val)
        };

        // Step 4: If done is true, return CreateIterResultObject(undefined, true)
        if done {
            return self.create_iter_result(ctx, Value::Undefined, true);
        }

        // Step 9: Let match be ? RegExpExec(R, S)
        let s = self.vm_to_string(ctx, &string_val);
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }
        let match_val = self.regexp_abstract_exec(ctx, &regexp, &s);
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }

        // Step 10: If match is null
        if matches!(match_val, Value::Null) {
            obj.borrow_mut(ctx).insert("__iter_done__".to_string(), Value::Boolean(true));
            return self.create_iter_result(ctx, Value::Undefined, true);
        }

        // Step 11: Else
        if global {
            // Step 11a: global mode
            // i. Let matchStr be ? ToString(? Get(match, "0"))
            let match_str_val = self.read_named_property(ctx, &match_val, "0");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            let match_str = self.vm_to_string(ctx, &match_str_val);
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }

            // ii. If matchStr is the empty String, advance lastIndex
            if match_str.is_empty() {
                let this_index_val = self.read_named_property(ctx, &regexp, "lastIndex");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let prim = self.try_to_primitive(ctx, &this_index_val, "number");
                if self.pending_throw.is_some() {
                    return Value::Undefined;
                }
                let n = to_number(&prim);
                let this_index = if n.is_nan() || n <= 0.0 {
                    0usize
                } else {
                    n.min(9007199254740991.0) as usize
                };
                let next_index = if full_unicode {
                    self.advance_string_index_unicode(&s, this_index)
                } else {
                    this_index + 1
                };
                if let Err(thrown) = self.host_fn_set_property(ctx, &regexp, "lastIndex", &Value::Number(next_index as f64)) {
                    self.pending_throw = Some(thrown);
                    return Value::Undefined;
                }
            }
            // iii. Return CreateIterResultObject(match, false)
            self.create_iter_result(ctx, match_val, false)
        } else {
            // Step 11b: non-global mode
            // i. Set O.[[Done]] to true
            obj.borrow_mut(ctx).insert("__iter_done__".to_string(), Value::Boolean(true));
            // ii. Return CreateIterResultObject(match, false)
            self.create_iter_result(ctx, match_val, false)
        }
    }

    /// CreateIterResultObject helper
    fn create_iter_result(&mut self, ctx: &GcContext<'gc>, value: Value<'gc>, done: bool) -> Value<'gc> {
        let mut result = IndexMap::new();
        result.insert("value".to_string(), value);
        result.insert("done".to_string(), Value::Boolean(done));
        Value::VmObject(new_gc_cell_ptr(ctx, result))
    }

    /// Create a new RegExp from an existing RegExp-like object, copying its
    /// pattern/flags.  Used by @@split and @@matchAll to create a "splitter" /
    /// "matcher" copy.
    /// SpeciesConstructor(O, defaultConstructor) — ES2024 §7.3.20
    /// Returns the species constructor or the default constructor.
    /// Used by @@split and @@matchAll to create the splitter/matcher copy.
    fn species_constructor(&mut self, ctx: &GcContext<'gc>, o: &Value<'gc>, default_ctor: &Value<'gc>) -> Value<'gc> {
        // 1. Let C be ? Get(O, "constructor").
        let c = self.read_named_property(ctx, o, "constructor");
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }
        // 2. If C is undefined, return defaultConstructor.
        if matches!(c, Value::Undefined) {
            return default_ctor.clone();
        }
        // 3. If Type(C) is not Object, throw a TypeError.
        if !matches!(
            c,
            Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_)
        ) || c.is_symbol_value()
        {
            self.throw_type_error(ctx, "Species constructor is not an Object");
            return Value::Undefined;
        }
        // 4. Let S be ? Get(C, @@species).
        let s = self.read_named_property(ctx, &c, "@@sym:5");
        if self.pending_throw.is_some() {
            return Value::Undefined;
        }
        // 5. If S is undefined or null, return defaultConstructor.
        if matches!(s, Value::Undefined | Value::Null) {
            return default_ctor.clone();
        }
        // 6. If IsConstructor(S), return S.
        if self.is_value_callable(&s) {
            return s;
        }
        // 7. Throw a TypeError exception.
        self.throw_type_error(ctx, "Species constructor is not a constructor");
        Value::Undefined
    }

    /// Helper: Read a named property given a &str key (avoids converting to index).
    fn read_named_property_str(&mut self, obj: &Value<'gc>, key: &str) -> Value<'gc> {
        // Simple wrapper — we can't call read_named_property without ctx in some
        // contexts, but value_to_string can fetch from VmObject directly.
        match obj {
            Value::VmObject(o) => o.borrow().get(key).cloned().unwrap_or(Value::Undefined),
            Value::VmArray(a) => a.borrow().props.get(key).cloned().unwrap_or(Value::Undefined),
            _ => Value::Undefined,
        }
    }

    /// Set a property from within a host function, ensuring errors return as
    /// `Err(thrown_value)` instead of being consumed by `handle_throw`.
    ///
    /// When `assign_named_property` is called inside a host function and the
    /// property is readonly, `handle_throw` may find a try-catch from the
    /// *caller's* JS code and silently consume the error, corrupting VM state.
    /// This helper temporarily hides `try_stack` so errors always propagate
    /// as `Err`, which the host function can convert to `pending_throw`.
    fn host_fn_set_property(&mut self, ctx: &GcContext<'gc>, obj: &Value<'gc>, key: &str, val: &Value<'gc>) -> Result<(), Value<'gc>> {
        let saved = std::mem::take(&mut self.try_stack);
        let result = self.assign_named_property(ctx, obj, key, val, None);
        self.try_stack = saved;
        match result {
            Ok(_) => {
                if let Some(thrown) = self.pending_throw.take() {
                    return Err(thrown);
                }
                Ok(())
            }
            Err(e) => {
                if let Some(thrown) = self.take_preserved_thrown_value_for_error(&e) {
                    Err(thrown)
                } else {
                    Err(self.vm_value_from_error(ctx, &e))
                }
            }
        }
    }

    /// Read a property from within a host function, hiding try_stack to prevent
    /// handle_throw from corrupting VM state when getters throw.
    fn host_fn_read_property(&mut self, ctx: &GcContext<'gc>, obj: &Value<'gc>, key: &str) -> Result<Value<'gc>, Value<'gc>> {
        let saved = std::mem::take(&mut self.try_stack);
        let val = self.read_named_property(ctx, obj, key);
        self.try_stack = saved;
        if let Some(thrown) = self.pending_throw.take() {
            return Err(thrown);
        }
        Ok(val)
    }

    /// IsRegExp(argument) — ES2024 §7.2.8
    pub(super) fn is_regexp_check(&mut self, ctx: &GcContext<'gc>, argument: &Value<'gc>) -> Result<bool, Value<'gc>> {
        // Step 1: If Type(argument) is not Object, return false.
        if !matches!(
            argument,
            Value::VmObject(_) | Value::VmArray(_) | Value::VmFunction(..) | Value::VmClosure(..)
        ) || argument.is_symbol_value()
        {
            return Ok(false);
        }
        // Step 2: Let matcher be ? Get(argument, @@match).
        let saved = std::mem::take(&mut self.try_stack);
        let matcher = self.read_named_property(ctx, argument, "@@sym:7");
        self.try_stack = saved;
        if let Some(thrown) = self.pending_throw.take() {
            return Err(thrown);
        }
        // Step 3: If matcher is not undefined, return ToBoolean(matcher).
        if !matches!(matcher, Value::Undefined) {
            return Ok(matcher.to_truthy());
        }
        // Step 4: If argument has a [[RegExpMatcher]] internal slot, return true.
        if let Value::VmObject(map) = argument
            && map.borrow().get("__type__").map(value_to_string).as_deref() == Some("RegExp")
        {
            return Ok(true);
        }
        Ok(false)
    }

    /// ToNumber with proper Symbol rejection for host functions.
    /// Hides try_stack to prevent handle_throw from corrupting VM state.
    /// Returns `Err(thrown_value)` if the value cannot be coerced to a number.
    fn host_fn_to_number(&mut self, ctx: &GcContext<'gc>, val: &Value<'gc>) -> Result<f64, Value<'gc>> {
        if val.is_symbol_value() {
            let err = self.make_type_error_object(ctx, "Cannot convert a Symbol value to a number");
            return Err(err);
        }
        let saved = std::mem::take(&mut self.try_stack);
        let prim = self.try_to_primitive(ctx, val, "number");
        self.try_stack = saved;
        if let Some(thrown) = self.pending_throw.take() {
            return Err(thrown);
        }
        if prim.is_symbol_value() {
            let err = self.make_type_error_object(ctx, "Cannot convert a Symbol value to a number");
            return Err(err);
        }
        Ok(to_number(&prim))
    }

    /// ToLength with proper Symbol rejection for host functions.
    /// Returns `Err(thrown_value)` if the value cannot be coerced to a number.
    fn host_fn_to_length(&mut self, ctx: &GcContext<'gc>, val: &Value<'gc>) -> Result<f64, Value<'gc>> {
        let n = self.host_fn_to_number(ctx, val)?;
        let result = if n.is_nan() || n <= 0.0 {
            0.0
        } else {
            n.min(9007199254740991.0).floor()
        };
        Ok(result)
    }

    /// AdvanceStringIndex (ES2024 §22.2.5.2.3)
    fn advance_string_index_unicode(&self, s: &str, index: usize) -> usize {
        let u16s: Vec<u16> = s.encode_utf16().collect();
        if index + 1 >= u16s.len() {
            return index + 1;
        }
        let first = u16s[index];
        // If it's a leading surrogate, advance by 2
        if (0xD800..=0xDBFF).contains(&first) {
            let second = u16s[index + 1];
            if (0xDC00..=0xDFFF).contains(&second) {
                return index + 2;
            }
        }
        index + 1
    }

    // ── Internal helpers ──────────────────────────────────────────────

    /// Extract stored regex pattern as UTF-16 directly, preserving lone surrogates.
    pub(super) fn regexp_get_pattern_u16(re_obj: &VmObjectHandle<'gc>) -> Vec<u16> {
        match re_obj.borrow().get("__regex_pattern__") {
            Some(Value::String(s)) => s.clone(),
            Some(v) => crate::unicode::utf8_to_utf16(&value_to_string(v)),
            None => Vec::new(),
        }
    }

    /// Extract (pattern, flags) from constructor arguments, handling RegExp-as-source.
    /// Extract pattern (UTF-16) and flags from constructor arguments.
    /// ES2024 §22.2.4.1 steps 3-5: handles actual RegExp, regexp-like, and plain values.
    pub(super) fn regexp_extract_pattern_flags(
        &mut self,
        ctx: &GcContext<'gc>,
        pattern: &Value<'gc>,
        flags_arg: &Value<'gc>,
        pattern_is_regexp: bool,
    ) -> (Vec<u16>, String) {
        // Step 3: If pattern has [[RegExpMatcher]] internal slot (actual RegExp)
        if let Value::VmObject(pat_obj) = pattern
            && pat_obj.borrow().get("__type__").map(value_to_string).as_deref() == Some("RegExp")
        {
            let p = match pat_obj.borrow().get("__regex_pattern__") {
                Some(Value::String(s)) => s.clone(),
                Some(v) => crate::unicode::utf8_to_utf16(&value_to_string(v)),
                None => Vec::new(),
            };
            let f = if matches!(flags_arg, Value::Undefined) {
                pat_obj.borrow().get("__regex_flags__").map(value_to_string).unwrap_or_default()
            } else {
                self.vm_to_string(ctx, flags_arg)
            };
            return (p, f);
        }

        // Step 4: If patternIsRegExp (regexp-like object, not actual RegExp)
        if pattern_is_regexp {
            let p_val = match self.host_fn_read_property(ctx, pattern, "source") {
                Ok(v) => v,
                Err(thrown) => {
                    self.pending_throw = Some(thrown);
                    return (Vec::new(), String::new());
                }
            };
            let p = match &p_val {
                Value::String(s) => s.clone(),
                Value::Undefined => crate::unicode::utf8_to_utf16("undefined"),
                v => {
                    let s = self.vm_to_string(ctx, v);
                    crate::unicode::utf8_to_utf16(&s)
                }
            };
            if self.pending_throw.is_some() {
                return (Vec::new(), String::new());
            }
            let f = if matches!(flags_arg, Value::Undefined) {
                let f_val = match self.host_fn_read_property(ctx, pattern, "flags") {
                    Ok(v) => v,
                    Err(thrown) => {
                        self.pending_throw = Some(thrown);
                        return (Vec::new(), String::new());
                    }
                };
                self.vm_to_string(ctx, &f_val)
            } else {
                self.vm_to_string(ctx, flags_arg)
            };
            return (p, f);
        }

        // Step 5: Plain value
        let p = match pattern {
            Value::Undefined => Vec::new(),
            Value::String(s) => s.clone(),
            v => {
                let s = self.vm_to_string(ctx, v);
                crate::unicode::utf8_to_utf16(&s)
            }
        };
        if self.pending_throw.is_some() {
            return (p, String::new());
        }
        let f = match flags_arg {
            Value::Undefined => String::new(),
            v => self.vm_to_string(ctx, v),
        };
        (p, f)
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
        let raw_u16 = match borrow.get("__regex_pattern__") {
            Some(Value::String(s)) => s.clone(),
            Some(v) => crate::unicode::utf8_to_utf16(&value_to_string(v)),
            None => match borrow.get("source") {
                Some(Value::String(s)) => s.clone(),
                Some(v) => crate::unicode::utf8_to_utf16(&value_to_string(v)),
                None => Vec::new(),
            },
        };
        let flags = borrow
            .get("__regex_flags__")
            .map(value_to_string)
            .unwrap_or_else(|| borrow.get("flags").map(value_to_string).unwrap_or_default());
        // EscapeRegExpPattern
        let source = if raw_u16.is_empty() {
            "(?:)".to_string()
        } else {
            let mut escaped = String::with_capacity(raw_u16.len());
            for &cu in &raw_u16 {
                match cu {
                    0x002F => escaped.push_str("\\/"), // /
                    0x000A => escaped.push_str("\\n"), // \n
                    0x000D => escaped.push_str("\\r"), // \r
                    0x2028 => escaped.push_str("\\u2028"),
                    0x2029 => escaped.push_str("\\u2029"),
                    _ => {
                        if let Some(ch) = char::from_u32(cu as u32) {
                            escaped.push(ch);
                        } else {
                            // Lone surrogate — emit \uXXXX escape
                            escaped.push_str(&format!("\\u{:04x}", cu));
                        }
                    }
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

        let pattern_u16 = Self::regexp_get_pattern_u16(re_obj);
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
        let pattern_u16 = Self::regexp_get_pattern_u16(re_obj);
        let flags = re_obj.borrow().get("__regex_flags__").map(value_to_string).unwrap_or_default();

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
        let pattern_u16 = Self::regexp_get_pattern_u16(re_obj);
        let flags = re_obj.borrow().get("__regex_flags__").map(value_to_string).unwrap_or_default();

        let is_global = flags.contains('g');
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
        let pattern_u16 = Self::regexp_get_pattern_u16(re_obj);
        let flags = re_obj.borrow().get("__regex_flags__").map(value_to_string).unwrap_or_default();

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
