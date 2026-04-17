use super::*;
use crate::core::GcPtr;
use unicode_normalization::UnicodeNormalization;

const INTL_DEFAULT_LOCALE: &str = "en-US";
const INTL_COLLATOR_BOUND_COMPARE_SLOT: &str = "@@sym:900001";

const INTL_SERVICE_CTORS: &[(&str, &str)] = &[
    ("Collator", "intl.collator.ctor"),
    ("DateTimeFormat", "intl.dateTimeFormat.ctor"),
    ("DisplayNames", "intl.displayNames.ctor"),
    ("DurationFormat", "intl.durationFormat.ctor"),
    ("ListFormat", "intl.listFormat.ctor"),
    ("NumberFormat", "intl.numberFormat.ctor"),
    ("PluralRules", "intl.pluralRules.ctor"),
    ("RelativeTimeFormat", "intl.relativeTimeFormat.ctor"),
    ("Segmenter", "intl.segmenter.ctor"),
];

const INTL_OPTIONAL_CONSTRUCTORS: &[(&str, &str)] = &[("Locale", "intl.locale.ctor")];

impl<'gc> VM<'gc> {
    pub(super) fn intl_init_globals(&mut self, ctx: &GcContext<'gc>) {
        let intl = self.make_dynamic_intl_placeholder(ctx);
        self.globals.insert("Intl".to_string(), intl.clone());
        self.global_this.borrow_mut(ctx).insert("Intl".to_string(), intl);
        mark_nonenumerable(&mut self.global_this.borrow_mut(ctx), "Intl");
    }

    pub(super) fn make_dynamic_intl_placeholder(&self, ctx: &GcContext<'gc>) -> Value<'gc> {
        let object_proto = self.globals.get("Object").and_then(|obj| {
            let Value::Object(obj) = obj else {
                return None;
            };
            own_data_from_legacy_map(&obj.borrow(), "prototype")
        });

        let segment_iterator_proto = self.intl_plain_proto_object(ctx, object_proto.clone());
        if let Value::Object(segment_iterator_proto_obj) = &segment_iterator_proto {
            let mut borrow = segment_iterator_proto_obj.borrow_mut(ctx);
            borrow.insert(
                "next".to_string(),
                Self::make_host_fn_with_name_len(ctx, "iterator.next", "next", 0.0, false),
            );
            mark_nonenumerable(&mut borrow, "next");
        }

        let segments_proto = self.intl_plain_proto_object(ctx, object_proto.clone());
        if let Value::Object(segments_proto_obj) = &segments_proto {
            let mut borrow = segments_proto_obj.borrow_mut(ctx);
            borrow.insert(
                "@@sym:1".to_string(),
                Self::make_host_fn_with_name_len(ctx, "intl.segments.iterator", "[Symbol.iterator]", 0.0, false),
            );
            mark_nonenumerable(&mut borrow, "@@sym:1");
        }

        let mut intl = IndexMap::new();
        if let Some(ref proto) = object_proto {
            intl.insert("__proto__".to_string(), proto.clone());
        }
        intl.insert(
            "getCanonicalLocales".to_string(),
            Self::make_host_fn_with_name_len(ctx, "intl.getCanonicalLocales", "getCanonicalLocales", 1.0, false),
        );
        mark_nonenumerable(&mut intl, "getCanonicalLocales");

        for (name, host_name) in INTL_SERVICE_CTORS {
            let ctor = self.intl_make_constructor(ctx, object_proto.clone(), name, host_name);
            if *name == "Segmenter" {
                if let Value::Object(ctor_obj) = &ctor {
                    let mut ctor_borrow = ctor_obj.borrow_mut(ctx);
                    ctor_borrow.insert("__segments_proto__".to_string(), segments_proto.clone());
                    ctor_borrow.insert("__segment_iter_proto__".to_string(), segment_iterator_proto.clone());
                }
            }
            intl.insert((*name).to_string(), ctor);
            mark_nonenumerable(&mut intl, name);
        }

        for (name, host_name) in INTL_OPTIONAL_CONSTRUCTORS {
            let ctor = self.intl_make_constructor(ctx, object_proto.clone(), name, host_name);
            intl.insert((*name).to_string(), ctor);
            mark_nonenumerable(&mut intl, name);
        }

        Value::Object(new_gc_cell_ptr(ctx, intl))
    }

    pub(super) fn intl_handle_host_fn(
        &mut self,
        ctx: &GcContext<'gc>,
        name: &str,
        receiver: Option<&Value<'gc>>,
        args: &[Value<'gc>],
    ) -> Value<'gc> {
        if let Some(kind) = Self::intl_service_kind_from_ctor_host(name) {
            return match self.intl_construct_service_instance(ctx, kind, receiver, args) {
                Ok(value) => value,
                Err(err) => {
                    self.pending_throw = Some(err);
                    Value::Undefined
                }
            };
        }
        if Self::intl_is_supported_locales_host(name) {
            return self.intl_supported_locales_of(ctx, args.first(), args.get(1));
        }
        match name {
            "intl.getCanonicalLocales" => self.intl_get_canonical_locales(ctx, args.first()),
            "intl.collator.get.compare" => self.intl_collator_compare_getter(ctx, receiver),
            "intl.collator.compare" => self.intl_collator_compare(ctx, receiver, args),
            "intl.service.resolvedOptions" => self.intl_resolved_options(ctx, receiver),
            "intl.segmenter.segment" => {
                let mut obj = IndexMap::new();
                if let Some(Value::Object(segmenter)) = receiver {
                    let borrow = segmenter.borrow();
                    if let Some(proto) = borrow.get("__segments_proto__").cloned() {
                        obj.insert("__proto__".to_string(), proto);
                    }
                    if let Some(proto) = borrow.get("__segment_iter_proto__").cloned() {
                        obj.insert("__segment_iter_proto__".to_string(), proto);
                    }
                }
                Value::Object(new_gc_cell_ptr(ctx, obj))
            }
            "intl.segments.iterator" => {
                let mut obj = IndexMap::new();
                if let Some(Value::Object(segments)) = receiver
                    && let Some(proto) = segments.borrow().get("__segment_iter_proto__").cloned()
                {
                    obj.insert("__proto__".to_string(), proto);
                }
                Value::Object(new_gc_cell_ptr(ctx, obj))
            }
            _ => {
                self.throw_type_error(ctx, "Intl operation not implemented");
                Value::Undefined
            }
        }
    }

    pub(super) fn intl_construct_host_fn(
        &mut self,
        ctx: &GcContext<'gc>,
        host_name: &str,
        ctor_value: &Value<'gc>,
        args: &[Value<'gc>],
    ) -> Option<Result<Value<'gc>, JSError>> {
        let kind = Self::intl_service_kind_from_ctor_host(host_name)?;
        Some(
            self.intl_construct_service_instance(ctx, kind, Some(ctor_value), args)
                .map_err(|err| self.vm_error_to_js_error(ctx, &err)),
        )
    }

    fn intl_make_constructor(
        &self,
        ctx: &GcContext<'gc>,
        object_proto: Option<Value<'gc>>,
        display_name: &str,
        host_name: &str,
    ) -> Value<'gc> {
        let prototype = self.intl_plain_proto_object(ctx, object_proto);
        if let Value::Object(proto_obj) = &prototype {
            let mut proto = proto_obj.borrow_mut(ctx);
            proto.insert(
                "resolvedOptions".to_string(),
                Self::make_host_fn_with_name_len(ctx, "intl.service.resolvedOptions", "resolvedOptions", 0.0, false),
            );
            mark_nonenumerable(&mut proto, "resolvedOptions");
            if display_name == "Collator" {
                let getter = Self::make_host_fn_with_name_len(ctx, "intl.collator.get.compare", "get compare", 0.0, false);
                Self::insert_getter_property_with_attributes(&mut proto, "compare", &getter, false, true);
                Self::insert_property_with_attributes(&mut proto, "@@sym:4", &Value::from("Intl.Collator"), false, false, true);
            }
            if display_name == "Segmenter" {
                proto.insert(
                    "segment".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.segmenter.segment", "segment", 1.0, false),
                );
                mark_nonenumerable(&mut proto, "segment");
            }
        }

        let ctor = Self::make_host_fn_with_name_len(ctx, host_name, display_name, 0.0, true);
        if let Value::Object(ctor_obj) = &ctor {
            let mut ctor_borrow = ctor_obj.borrow_mut(ctx);
            ctor_borrow.insert("prototype".to_string(), prototype.clone());
            write_attrs_to_legacy_map(&mut ctor_borrow, "prototype", PropAttrs::empty());
            ctor_borrow.insert("__intl_kind__".to_string(), Value::from(display_name));
            if display_name != "Locale" {
                ctor_borrow.insert(
                    "supportedLocalesOf".to_string(),
                    Self::make_host_fn_with_name_len(
                        ctx,
                        &format!("intl.{}.supportedLocalesOf", display_name),
                        "supportedLocalesOf",
                        1.0,
                        false,
                    ),
                );
                mark_nonenumerable(&mut ctor_borrow, "supportedLocalesOf");
            }
        }
        if let Value::Object(proto_obj) = &prototype {
            proto_obj.borrow_mut(ctx).insert("constructor".to_string(), ctor.clone());
            mark_nonenumerable(&mut proto_obj.borrow_mut(ctx), "constructor");
        }
        ctor
    }

    fn intl_plain_proto_object(&self, ctx: &GcContext<'gc>, object_proto: Option<Value<'gc>>) -> Value<'gc> {
        let mut map = IndexMap::new();
        if let Some(proto) = object_proto {
            map.insert("__proto__".to_string(), proto);
        }
        Value::Object(new_gc_cell_ptr(ctx, map))
    }

    fn intl_construct_service_instance(
        &mut self,
        ctx: &GcContext<'gc>,
        kind: &str,
        ctor_value: Option<&Value<'gc>>,
        args: &[Value<'gc>],
    ) -> Result<Value<'gc>, Value<'gc>> {
        let requested_locales = self.intl_canonicalize_locale_list(ctx, args.first())?;
        let collator_opts = if kind == "Collator" {
            Some(self.intl_read_collator_options(ctx, &requested_locales, args.get(1))?)
        } else {
            self.intl_read_constructor_options(ctx, kind, args.get(1))?;
            None
        };

        let ctor_value = ctor_value
            .cloned()
            .or_else(|| self.intl_service_constructor_from_global(kind))
            .unwrap_or(Value::Undefined);

        let prototype = self.read_named_property(ctx, &ctor_value, "prototype");
        if let Some(thrown) = self.pending_throw.take() {
            return Err(thrown);
        }

        let mut obj = IndexMap::new();
        if !matches!(prototype, Value::Undefined) {
            obj.insert("__proto__".to_string(), prototype);
        }
        obj.insert("__intl_kind__".to_string(), Value::from(kind));
        let locale = requested_locales
            .first()
            .cloned()
            .unwrap_or_else(|| INTL_DEFAULT_LOCALE.to_string());
        obj.insert("__intl_locale__".to_string(), Value::from(locale.as_str()));
        if let Some(opts) = collator_opts {
            self.intl_store_collator_options(&mut obj, &opts);
        }

        if kind == "Segmenter" {
            if let Value::Object(ctor_obj) = &ctor_value {
                let borrow = ctor_obj.borrow();
                if let Some(proto) = borrow.get("__segments_proto__").cloned() {
                    obj.insert("__segments_proto__".to_string(), proto);
                }
                if let Some(proto) = borrow.get("__segment_iter_proto__").cloned() {
                    obj.insert("__segment_iter_proto__".to_string(), proto);
                }
            }
        }

        let value = Value::Object(new_gc_cell_ptr(ctx, obj));
        if kind == "Collator"
            && let Value::Object(collator_obj) = &value
        {
            let compare = Self::make_bound_host_fn(ctx, "intl.collator.compare", &value);
            if let Value::Object(compare_obj) = &compare {
                let mut borrow = compare_obj.borrow_mut(ctx);
                Self::insert_property_with_attributes(&mut borrow, "length", &Value::Number(2.0), false, false, true);
                Self::insert_property_with_attributes(&mut borrow, "name", &Value::from(""), false, false, true);
                borrow.insert("__non_constructor__".to_string(), Value::Boolean(true));
            }
            let mut borrow = collator_obj.borrow_mut(ctx);
            borrow.insert(INTL_COLLATOR_BOUND_COMPARE_SLOT.to_string(), compare.clone());
            Self::insert_property_with_attributes(&mut borrow, "compare", &compare, false, true, true);
        }

        Ok(value)
    }

    fn intl_supported_locales_of(
        &mut self,
        ctx: &GcContext<'gc>,
        locales: Option<&Value<'gc>>,
        options: Option<&Value<'gc>>,
    ) -> Value<'gc> {
        let requested = match self.intl_canonicalize_locale_list(ctx, locales) {
            Ok(locales) => locales,
            Err(err) => {
                self.pending_throw = Some(err);
                return Value::Undefined;
            }
        };
        if let Err(err) = self.intl_locale_matcher_option(ctx, options) {
            self.pending_throw = Some(err);
            return Value::Undefined;
        }
        self.intl_supported_locales_result(ctx, &requested)
    }

    fn intl_resolved_options(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(obj) = self.intl_require_initialized_service(ctx, receiver, None) else {
            return Value::Undefined;
        };

        let mut result = IndexMap::new();
        let borrow = obj.borrow();
        if let Some(locale) = borrow.get("__intl_locale__").cloned() {
            result.insert("locale".to_string(), locale);
        } else {
            result.insert("locale".to_string(), Value::from(INTL_DEFAULT_LOCALE));
        }
        if matches!(borrow.get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(&kind) == "Collator") {
            result.insert(
                "usage".to_string(),
                borrow.get("__intl_usage__").cloned().unwrap_or_else(|| Value::from("sort")),
            );
            result.insert(
                "sensitivity".to_string(),
                borrow
                    .get("__intl_sensitivity__")
                    .cloned()
                    .unwrap_or_else(|| Value::from("variant")),
            );
            result.insert(
                "ignorePunctuation".to_string(),
                borrow.get("__intl_ignore_punctuation__").cloned().unwrap_or(Value::Boolean(false)),
            );
            result.insert(
                "collation".to_string(),
                borrow.get("__intl_collation__").cloned().unwrap_or_else(|| Value::from("default")),
            );
            if let Some(numeric) = borrow.get("__intl_numeric__").cloned() {
                result.insert("numeric".to_string(), numeric);
            }
            if let Some(case_first) = borrow.get("__intl_case_first__").cloned() {
                result.insert("caseFirst".to_string(), case_first);
            }
        }
        Value::Object(new_gc_cell_ptr(ctx, result))
    }

    fn intl_get_canonical_locales(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Value<'gc> {
        match self.intl_canonicalize_locale_list(ctx, value) {
            Ok(locales) => Value::Array(new_gc_cell_ptr(
                ctx,
                VmArrayData::new(locales.into_iter().map(|locale| Value::from(locale.as_str())).collect()),
            )),
            Err(err) => {
                self.pending_throw = Some(err);
                Value::Undefined
            }
        }
    }

    fn intl_read_constructor_options(&mut self, ctx: &GcContext<'gc>, kind: &str, options: Option<&Value<'gc>>) -> Result<(), Value<'gc>> {
        let Some(options) = options else {
            return Ok(());
        };
        if matches!(options, Value::Undefined) || !Self::intl_is_object_like(options) {
            return Ok(());
        }
        let keys: &[&str] = match kind {
            "Collator" => &[
                "usage",
                "localeMatcher",
                "collation",
                "numeric",
                "caseFirst",
                "sensitivity",
                "ignorePunctuation",
            ],
            "NumberFormat" => &[
                "localeMatcher",
                "numberingSystem",
                "style",
                "currency",
                "currencyDisplay",
                "currencySign",
                "unit",
                "unitDisplay",
                "notation",
                "compactDisplay",
                "useGrouping",
                "signDisplay",
                "minimumIntegerDigits",
                "minimumFractionDigits",
                "maximumFractionDigits",
                "minimumSignificantDigits",
                "maximumSignificantDigits",
                "roundingPriority",
                "roundingIncrement",
                "roundingMode",
                "trailingZeroDisplay",
            ],
            "DateTimeFormat" => &[
                "localeMatcher",
                "calendar",
                "numberingSystem",
                "hour12",
                "hourCycle",
                "timeZone",
                "weekday",
                "era",
                "year",
                "month",
                "day",
                "dayPeriod",
                "hour",
                "minute",
                "second",
                "fractionalSecondDigits",
                "timeZoneName",
                "formatMatcher",
                "dateStyle",
                "timeStyle",
            ],
            _ => &[],
        };
        for key in keys {
            let _ = self.read_named_property(ctx, options, key);
            if let Some(thrown) = self.pending_throw.take() {
                return Err(thrown);
            }
        }
        Ok(())
    }

    fn intl_canonicalize_locale_list(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Result<Vec<String>, Value<'gc>> {
        let Some(value) = value else {
            return Ok(vec![]);
        };
        match value {
            Value::Undefined => Ok(vec![]),
            Value::Null => Err(self.make_type_error_object(ctx, "locales must not be null")),
            Value::String(text) => {
                let tag = crate::unicode::utf16_to_utf8(text);
                self.intl_validate_locale_tag(ctx, &tag)?;
                Ok(vec![tag])
            }
            v if Self::intl_is_object_like(v) => self.intl_canonicalize_locale_list_from_object(ctx, v),
            _ => Ok(vec![]),
        }
    }

    fn intl_canonicalize_locale_list_from_object(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>) -> Result<Vec<String>, Value<'gc>> {
        let Some(len) = self.array_like_length_u64(ctx, value) else {
            let thrown = self
                .pending_throw
                .take()
                .unwrap_or_else(|| self.make_type_error_object(ctx, "Invalid locales"));
            return Err(thrown);
        };
        let mut out = Vec::new();
        for index in 0..len {
            let present = self.array_like_has_index_u64(ctx, value, index)?;
            if !present {
                continue;
            }
            let key = index.to_string();
            let entry = self.read_named_property(ctx, value, &key);
            if let Some(thrown) = self.pending_throw.take() {
                return Err(thrown);
            }
            let tag = match &entry {
                Value::String(text) => crate::unicode::utf16_to_utf8(text),
                Value::Undefined | Value::Null | Value::Boolean(_) | Value::Number(_) | Value::BigInt(_) => {
                    return Err(self.make_type_error_object(ctx, "Locale list elements must be strings or objects"));
                }
                _ => match self.vm_to_string_like_spec(ctx, &entry) {
                    Ok(value) => value,
                    Err(err) => return Err(self.vm_value_from_error(ctx, &err)),
                },
            };
            self.intl_validate_locale_tag(ctx, &tag)?;
            if !out.iter().any(|existing| existing == &tag) {
                out.push(tag);
            }
        }
        Ok(out)
    }

    fn intl_validate_locale_tag(&self, ctx: &GcContext<'gc>, tag: &str) -> Result<(), Value<'gc>> {
        if !Self::intl_is_valid_locale_tag(tag) {
            return Err(self.make_range_error_object(ctx, "Invalid language tag"));
        }
        Ok(())
    }

    fn intl_locale_matcher_option(&mut self, ctx: &GcContext<'gc>, options: Option<&Value<'gc>>) -> Result<&'static str, Value<'gc>> {
        let Some(options) = options else {
            return Ok("best fit");
        };
        if matches!(options, Value::Undefined) || !Self::intl_is_object_like(options) {
            return Ok("best fit");
        }
        let value = self.read_named_property(ctx, options, "localeMatcher");
        if let Some(thrown) = self.pending_throw.take() {
            return Err(thrown);
        }
        if matches!(value, Value::Undefined) {
            return Ok("best fit");
        }
        let matcher = match self.vm_to_string_like_spec(ctx, &value) {
            Ok(value) => value,
            Err(err) => return Err(self.vm_value_from_error(ctx, &err)),
        };
        match matcher.as_str() {
            "lookup" => Ok("lookup"),
            "best fit" => Ok("best fit"),
            _ => Err(self.make_range_error_object(ctx, "Invalid localeMatcher")),
        }
    }

    fn intl_supported_locales_result(&self, ctx: &GcContext<'gc>, requested: &[String]) -> Value<'gc> {
        let mut supported = Vec::new();
        for locale in requested {
            let base = Self::intl_locale_without_unicode_extension(locale);
            if base.eq_ignore_ascii_case("zxx") {
                continue;
            }
            supported.push(Value::from(locale.as_str()));
        }
        Value::Array(new_gc_cell_ptr(ctx, VmArrayData::new(supported)))
    }

    fn intl_collator_compare_getter(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let this_val = receiver.unwrap_or(&Value::Undefined);
        let Value::Object(collator) = this_val else {
            self.pending_throw = Some(self.make_type_error_object(ctx, "Intl method called on incompatible receiver"));
            return Value::Undefined;
        };
        {
            let borrow = collator.borrow();
            if !matches!(borrow.get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "Collator") {
                drop(borrow);
                self.pending_throw = Some(self.make_type_error_object(ctx, "Intl method called on incompatible receiver"));
                return Value::Undefined;
            }
        }

        if let Some(existing) = collator.borrow().get(INTL_COLLATOR_BOUND_COMPARE_SLOT).cloned() {
            return existing;
        }

        let compare = Self::make_bound_host_fn(ctx, "intl.collator.compare", this_val);
        if let Value::Object(compare_obj) = &compare {
            let mut borrow = compare_obj.borrow_mut(ctx);
            Self::insert_property_with_attributes(&mut borrow, "length", &Value::Number(2.0), false, false, true);
            Self::insert_property_with_attributes(&mut borrow, "name", &Value::from(""), false, false, true);
            borrow.insert("__non_constructor__".to_string(), Value::Boolean(true));
        }
        collator
            .borrow_mut(ctx)
            .insert(INTL_COLLATOR_BOUND_COMPARE_SLOT.to_string(), compare.clone());
        compare
    }

    fn intl_collator_compare(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, args: &[Value<'gc>]) -> Value<'gc> {
        let Some(collator_obj) = self.intl_require_initialized_service(ctx, receiver, Some("Collator")) else {
            return Value::Undefined;
        };
        let left = match self.vm_to_string_like_spec(ctx, args.first().unwrap_or(&Value::Undefined)) {
            Ok(v) => v,
            Err(err) => {
                self.pending_throw = Some(self.vm_value_from_error(ctx, &err));
                return Value::Undefined;
            }
        };
        let right = match self.vm_to_string_like_spec(ctx, args.get(1).unwrap_or(&Value::Undefined)) {
            Ok(v) => v,
            Err(err) => {
                self.pending_throw = Some(self.vm_value_from_error(ctx, &err));
                return Value::Undefined;
            }
        };
        let borrow = collator_obj.borrow();
        let options = IntlCollatorOptions::from_object(&borrow);
        Value::Number(Self::intl_compare_strings(&left, &right, &options) as f64)
    }

    fn intl_require_initialized_service(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        expected_kind: Option<&str>,
    ) -> Option<GcPtr<'gc, IndexMap<String, Value<'gc>>>> {
        let Some(Value::Object(obj)) = receiver else {
            self.pending_throw = Some(self.make_type_error_object(ctx, "Intl method called on incompatible receiver"));
            return None;
        };
        let borrow = obj.borrow();
        let Some(Value::String(kind)) = borrow.get("__intl_kind__") else {
            drop(borrow);
            self.pending_throw = Some(self.make_type_error_object(ctx, "Intl method called on incompatible receiver"));
            return None;
        };
        let kind = crate::unicode::utf16_to_utf8(kind);
        if expected_kind.is_some_and(|expected| expected != kind) {
            drop(borrow);
            self.pending_throw = Some(self.make_type_error_object(ctx, "Intl method called on incompatible receiver"));
            return None;
        }
        drop(borrow);
        Some(obj.clone())
    }

    fn intl_read_collator_options(
        &mut self,
        ctx: &GcContext<'gc>,
        requested_locales: &[String],
        options: Option<&Value<'gc>>,
    ) -> Result<IntlCollatorOptions, Value<'gc>> {
        self.intl_read_constructor_options(ctx, "Collator", options)?;
        let locale = requested_locales
            .first()
            .cloned()
            .unwrap_or_else(|| INTL_DEFAULT_LOCALE.to_string());
        let locale_base = Self::intl_locale_without_unicode_extension(&locale);
        let locale_unicode = Self::intl_locale_unicode_keywords(&locale);
        let mut out = IntlCollatorOptions {
            usage: "sort".to_string(),
            sensitivity: "variant".to_string(),
            ignore_punctuation: locale_base.eq_ignore_ascii_case("th"),
            collation: "default".to_string(),
            numeric: None,
            case_first: None,
        };

        if let Some(value) = locale_unicode.get("co")
            && !value.is_empty()
        {
            out.collation = value.clone();
        }
        if let Some(value) = locale_unicode.get("kn") {
            out.numeric = match value.as_str() {
                "" | "true" => Some(true),
                "false" => Some(false),
                _ => None,
            };
        }
        if let Some(value) = locale_unicode.get("kf")
            && matches!(value.as_str(), "upper" | "lower" | "false")
        {
            out.case_first = Some(value.clone());
        }

        let Some(options) = options else {
            return Ok(out);
        };
        if matches!(options, Value::Undefined) || !Self::intl_is_object_like(options) {
            return Ok(out);
        }

        if let Some(usage) = self.intl_string_option(ctx, options, "usage", &["sort", "search"], Some("sort"))? {
            out.usage = usage;
        }
        let _ = self.intl_locale_matcher_option(ctx, Some(options))?;
        if let Some(collation) = self.intl_string_option(ctx, options, "collation", &[], None)? {
            out.collation = if collation.is_empty() { "default".to_string() } else { collation };
        }
        out.numeric = self.intl_boolean_option(ctx, options, "numeric")?.or(out.numeric);
        if let Some(case_first) = self.intl_string_option(ctx, options, "caseFirst", &["upper", "lower", "false"], None)? {
            out.case_first = Some(case_first);
        }
        if let Some(sensitivity) =
            self.intl_string_option(ctx, options, "sensitivity", &["base", "accent", "case", "variant"], Some("variant"))?
        {
            out.sensitivity = sensitivity;
        }
        if let Some(ignore_punctuation) = self.intl_boolean_option(ctx, options, "ignorePunctuation")? {
            out.ignore_punctuation = ignore_punctuation;
        }
        Ok(out)
    }

    fn intl_store_collator_options(&self, obj: &mut IndexMap<String, Value<'gc>>, options: &IntlCollatorOptions) {
        obj.insert("__intl_usage__".to_string(), Value::from(options.usage.as_str()));
        obj.insert("__intl_sensitivity__".to_string(), Value::from(options.sensitivity.as_str()));
        obj.insert(
            "__intl_ignore_punctuation__".to_string(),
            Value::Boolean(options.ignore_punctuation),
        );
        obj.insert("__intl_collation__".to_string(), Value::from(options.collation.as_str()));
        if let Some(numeric) = options.numeric {
            obj.insert("__intl_numeric__".to_string(), Value::Boolean(numeric));
        }
        if let Some(case_first) = &options.case_first {
            obj.insert("__intl_case_first__".to_string(), Value::from(case_first.as_str()));
        }
    }

    fn intl_string_option(
        &mut self,
        ctx: &GcContext<'gc>,
        options: &Value<'gc>,
        key: &str,
        allowed: &[&str],
        fallback: Option<&str>,
    ) -> Result<Option<String>, Value<'gc>> {
        let value = self.read_named_property(ctx, options, key);
        if let Some(thrown) = self.pending_throw.take() {
            return Err(thrown);
        }
        if matches!(value, Value::Undefined) {
            return Ok(fallback.map(str::to_string));
        }
        let value = match self.vm_to_string_like_spec(ctx, &value) {
            Ok(value) => value,
            Err(err) => return Err(self.vm_value_from_error(ctx, &err)),
        };
        if !allowed.is_empty() && !allowed.iter().any(|candidate| *candidate == value) {
            return Err(self.make_range_error_object(ctx, &format!("Invalid {}", key)));
        }
        Ok(Some(value))
    }

    fn intl_boolean_option(&mut self, ctx: &GcContext<'gc>, options: &Value<'gc>, key: &str) -> Result<Option<bool>, Value<'gc>> {
        let value = self.read_named_property(ctx, options, key);
        if let Some(thrown) = self.pending_throw.take() {
            return Err(thrown);
        }
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        Ok(Some(value.to_truthy()))
    }

    fn intl_locale_without_unicode_extension(locale: &str) -> String {
        let parts: Vec<&str> = locale.split('-').collect();
        let mut out = Vec::new();
        for part in parts {
            if part.eq_ignore_ascii_case("u") {
                break;
            }
            out.push(part);
        }
        out.join("-")
    }

    fn intl_locale_unicode_keywords(locale: &str) -> std::collections::HashMap<String, String> {
        let mut out = std::collections::HashMap::new();
        let parts: Vec<&str> = locale.split('-').collect();
        let Some(unicode_start) = parts.iter().position(|part| part.eq_ignore_ascii_case("u")) else {
            return out;
        };
        let mut idx = unicode_start + 1;
        while idx < parts.len() {
            let key = parts[idx];
            if key.len() == 1 {
                break;
            }
            if key.len() != 2 {
                idx += 1;
                continue;
            }
            idx += 1;
            let mut value_parts = Vec::new();
            while idx < parts.len() && parts[idx].len() > 2 {
                value_parts.push(parts[idx]);
                idx += 1;
            }
            out.insert(key.to_ascii_lowercase(), value_parts.join("-"));
        }
        out
    }

    fn intl_compare_strings(left: &str, right: &str, options: &IntlCollatorOptions) -> i32 {
        let left_key = Self::intl_collator_sort_key(&left.nfc().collect::<String>(), options);
        let right_key = Self::intl_collator_sort_key(&right.nfc().collect::<String>(), options);
        match left_key.cmp(&right_key) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }

    fn intl_collator_sort_key(value: &str, options: &IntlCollatorOptions) -> String {
        let mut normalized = value.nfd().collect::<String>();
        if options.ignore_punctuation {
            normalized.retain(|ch| !ch.is_ascii_punctuation() && !ch.is_whitespace());
        }
        if options.collation == "phonebk" {
            normalized = normalized
                .replace("ä", "ae")
                .replace("Ä", "Ae")
                .replace("ö", "oe")
                .replace("Ö", "Oe")
                .replace("ü", "ue")
                .replace("Ü", "Ue");
        }
        match options.sensitivity.as_str() {
            "base" => Self::intl_collator_strip_accents(&normalized).to_ascii_lowercase(),
            "accent" => normalized.to_ascii_lowercase(),
            "case" => Self::intl_collator_strip_accents(&normalized),
            _ => normalized.to_ascii_lowercase(),
        }
    }

    fn intl_collator_strip_accents(value: &str) -> String {
        value.chars().filter(|ch| !matches!(ch, '\u{0300}'..='\u{036f}')).collect()
    }

    fn intl_service_constructor_from_global(&self, kind: &str) -> Option<Value<'gc>> {
        let Value::Object(intl) = self.globals.get("Intl")?.clone() else {
            return None;
        };
        intl.borrow().get(kind).cloned()
    }

    fn intl_service_kind_from_ctor_host(name: &str) -> Option<&'static str> {
        match name {
            "intl.collator.ctor" => Some("Collator"),
            "intl.dateTimeFormat.ctor" => Some("DateTimeFormat"),
            "intl.displayNames.ctor" => Some("DisplayNames"),
            "intl.durationFormat.ctor" => Some("DurationFormat"),
            "intl.listFormat.ctor" => Some("ListFormat"),
            "intl.locale.ctor" => Some("Locale"),
            "intl.numberFormat.ctor" => Some("NumberFormat"),
            "intl.pluralRules.ctor" => Some("PluralRules"),
            "intl.relativeTimeFormat.ctor" => Some("RelativeTimeFormat"),
            "intl.segmenter.ctor" => Some("Segmenter"),
            _ => None,
        }
    }

    fn intl_is_supported_locales_host(name: &str) -> bool {
        matches!(
            name,
            "intl.Collator.supportedLocalesOf"
                | "intl.DateTimeFormat.supportedLocalesOf"
                | "intl.DisplayNames.supportedLocalesOf"
                | "intl.DurationFormat.supportedLocalesOf"
                | "intl.ListFormat.supportedLocalesOf"
                | "intl.NumberFormat.supportedLocalesOf"
                | "intl.PluralRules.supportedLocalesOf"
                | "intl.RelativeTimeFormat.supportedLocalesOf"
                | "intl.Segmenter.supportedLocalesOf"
        )
    }

    fn intl_is_object_like(value: &Value<'gc>) -> bool {
        matches!(
            value,
            Value::Object(_) | Value::Array(_) | Value::Function(_, _) | Value::Closure(_, _, _) | Value::NativeFunction(_)
        )
    }

    fn intl_is_valid_locale_tag(tag: &str) -> bool {
        if tag.is_empty()
            || tag.trim() != tag
            || tag.contains('_')
            || tag.contains('*')
            || tag.contains('\0')
            || tag.starts_with('-')
            || tag.ends_with('-')
            || tag.contains("--")
            || !tag.is_ascii()
        {
            return false;
        }

        let lower = tag.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "no-nyn"
                | "i-klingon"
                | "zh-hak-cn"
                | "sgn-ils"
                | "x-foo"
                | "x-en-us-12345"
                | "x-12345-12345-en-us"
                | "x-en-us-12345-12345"
                | "x-en-u-foo"
                | "x-en-u-foo-u-bar"
                | "x-u-foo"
        ) {
            return false;
        }

        let parts: Vec<&str> = tag.split('-').collect();
        let Some(first) = parts.first() else {
            return false;
        };
        if first.len() < 2 || first.len() > 8 || !first.chars().all(|c| c.is_ascii_alphabetic()) {
            return false;
        }

        let mut seen_script = false;
        let mut seen_region = false;
        let mut seen_private_use = false;
        let mut seen_singletons = std::collections::HashSet::new();
        let mut seen_variants = std::collections::HashSet::new();
        let mut idx = 1;
        while idx < parts.len() {
            let part = parts[idx];
            if part.is_empty() || !part.chars().all(|c| c.is_ascii_alphanumeric()) {
                return false;
            }
            let lower_part = part.to_ascii_lowercase();
            if seen_private_use {
                if part.len() > 8 {
                    return false;
                }
                idx += 1;
                continue;
            }
            if lower_part == "x" {
                if idx + 1 >= parts.len() {
                    return false;
                }
                seen_private_use = true;
                idx += 1;
                continue;
            }
            if part.len() == 1 {
                if !seen_singletons.insert(lower_part) || idx + 1 >= parts.len() {
                    return false;
                }
                idx += 1;
                while idx < parts.len() {
                    let next = parts[idx];
                    if next.len() == 1 {
                        break;
                    }
                    if next.len() < 2 || next.len() > 8 || !next.chars().all(|c| c.is_ascii_alphanumeric()) {
                        return false;
                    }
                    idx += 1;
                }
                continue;
            }
            if !seen_script && !seen_region && part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()) {
                seen_script = true;
                idx += 1;
                continue;
            }
            if !seen_region
                && ((part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
                    || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit())))
            {
                seen_region = true;
                idx += 1;
                continue;
            }
            let is_variant = (part.len() >= 5 && part.len() <= 8 && part.chars().all(|c| c.is_ascii_alphanumeric()))
                || (part.len() == 4
                    && part.chars().next().is_some_and(|c| c.is_ascii_digit())
                    && part.chars().skip(1).all(|c| c.is_ascii_alphanumeric()));
            if !is_variant || !seen_variants.insert(lower_part) {
                return false;
            }
            idx += 1;
        }
        true
    }
}

#[derive(Clone)]
struct IntlCollatorOptions {
    usage: String,
    sensitivity: String,
    ignore_punctuation: bool,
    collation: String,
    numeric: Option<bool>,
    case_first: Option<String>,
}

impl IntlCollatorOptions {
    fn from_object<'gc>(obj: &IndexMap<String, Value<'gc>>) -> Self {
        Self {
            usage: match obj.get("__intl_usage__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "sort".to_string(),
            },
            sensitivity: match obj.get("__intl_sensitivity__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "variant".to_string(),
            },
            ignore_punctuation: matches!(obj.get("__intl_ignore_punctuation__"), Some(Value::Boolean(true))),
            collation: match obj.get("__intl_collation__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "default".to_string(),
            },
            numeric: match obj.get("__intl_numeric__") {
                Some(Value::Boolean(value)) => Some(*value),
                _ => None,
            },
            case_first: match obj.get("__intl_case_first__") {
                Some(Value::String(text)) => Some(crate::unicode::utf16_to_utf8(text)),
                _ => None,
            },
        }
    }
}
