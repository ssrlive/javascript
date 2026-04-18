use super::*;
use crate::core::GcPtr;
use unicode_normalization::UnicodeNormalization;

const INTL_DEFAULT_LOCALE: &str = "en-US";
const INTL_COLLATOR_BOUND_COMPARE_SLOT: &str = "@@sym:900001";
const INTL_DATE_TIME_FORMAT_BOUND_FORMAT_SLOT: &str = "@@sym:900003";
const INTL_NUMBER_FORMAT_BOUND_FORMAT_SLOT: &str = "@@sym:900004";

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
            if *name == "Segmenter"
                && let Value::Object(ctor_obj) = &ctor
            {
                let mut ctor_borrow = ctor_obj.borrow_mut(ctx);
                ctor_borrow.insert("__segments_proto__".to_string(), segments_proto.clone());
                ctor_borrow.insert("__segment_iter_proto__".to_string(), segment_iterator_proto.clone());
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
            "intl.dateTimeFormat.get.format" => self.intl_date_time_format_getter(ctx, receiver),
            "intl.dateTimeFormat.format" => self.intl_date_time_format_format(ctx, receiver, args),
            "intl.numberFormat.get.format" => self.intl_number_format_getter(ctx, receiver),
            "intl.numberFormat.format" => self.intl_number_format_format(ctx, receiver, args),
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
            if display_name == "DateTimeFormat" {
                let getter = Self::make_host_fn_with_name_len(ctx, "intl.dateTimeFormat.get.format", "get format", 0.0, false);
                Self::insert_getter_property_with_attributes(&mut proto, "format", &getter, false, true);
            }
            if display_name == "NumberFormat" {
                let getter = Self::make_host_fn_with_name_len(ctx, "intl.numberFormat.get.format", "get format", 0.0, false);
                Self::insert_getter_property_with_attributes(&mut proto, "format", &getter, false, true);
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
        #[allow(clippy::if_same_then_else)]
        let collator_opts = if kind == "Collator" {
            Some(self.intl_read_collator_options(ctx, &requested_locales, args.get(1))?)
        } else if kind == "DateTimeFormat" {
            None
        } else if kind == "NumberFormat" {
            None
        } else {
            self.intl_read_constructor_options(ctx, kind, args.get(1))?;
            None
        };
        let date_time_format_opts = if kind == "DateTimeFormat" {
            Some(self.intl_read_date_time_format_options(ctx, &requested_locales, args.get(1))?)
        } else {
            None
        };
        let number_format_opts = if kind == "NumberFormat" {
            Some(self.intl_read_number_format_options(ctx, &requested_locales, args.get(1))?)
        } else {
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
        let locale = collator_opts
            .as_ref()
            .map(|opts| opts.resolved_locale.clone())
            .or_else(|| date_time_format_opts.as_ref().map(|opts| opts.resolved_locale.clone()))
            .or_else(|| number_format_opts.as_ref().map(|opts| opts.resolved_locale.clone()))
            .or_else(|| requested_locales.first().cloned())
            .unwrap_or_else(|| INTL_DEFAULT_LOCALE.to_string());
        obj.insert("__intl_locale__".to_string(), Value::from(locale.as_str()));
        if let Some(opts) = collator_opts {
            self.intl_store_collator_options(&mut obj, &opts);
        }
        if let Some(opts) = date_time_format_opts {
            self.intl_store_date_time_format_options(&mut obj, &opts);
        }
        if let Some(opts) = number_format_opts {
            self.intl_store_number_format_options(&mut obj, &opts);
        }

        if kind == "Segmenter"
            && let Value::Object(ctor_obj) = &ctor_value
        {
            let borrow = ctor_obj.borrow();
            if let Some(proto) = borrow.get("__segments_proto__").cloned() {
                obj.insert("__segments_proto__".to_string(), proto);
            }
            if let Some(proto) = borrow.get("__segment_iter_proto__").cloned() {
                obj.insert("__segment_iter_proto__".to_string(), proto);
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
        if matches!(borrow.get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "Collator") {
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
        } else if matches!(borrow.get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "DateTimeFormat")
        {
            result.insert(
                "calendar".to_string(),
                borrow.get("__intl_calendar__").cloned().unwrap_or_else(|| Value::from("gregory")),
            );
            result.insert(
                "numberingSystem".to_string(),
                borrow
                    .get("__intl_numbering_system__")
                    .cloned()
                    .unwrap_or_else(|| Value::from("latn")),
            );
            result.insert(
                "timeZone".to_string(),
                borrow.get("__intl_time_zone__").cloned().unwrap_or_else(|| Value::from("UTC")),
            );
            if let Some(hour_cycle) = borrow.get("__intl_hour_cycle__").cloned() {
                result.insert("hourCycle".to_string(), hour_cycle);
            }
            if let Some(hour12) = borrow.get("__intl_hour12__").cloned() {
                result.insert("hour12".to_string(), hour12);
            }
            for key in ["weekday", "era", "year", "month", "day", "hour", "minute", "second", "timeZoneName"] {
                if let Some(value) = borrow.get(&format!("__intl_{}__", key)).cloned() {
                    result.insert(key.to_string(), value);
                }
            }
        } else if matches!(borrow.get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "NumberFormat")
        {
            result.insert(
                "numberingSystem".to_string(),
                borrow
                    .get("__intl_numbering_system__")
                    .cloned()
                    .unwrap_or_else(|| Value::from("latn")),
            );
            result.insert(
                "style".to_string(),
                borrow.get("__intl_style__").cloned().unwrap_or_else(|| Value::from("decimal")),
            );
            if let Some(currency) = borrow.get("__intl_currency__").cloned() {
                result.insert("currency".to_string(), currency);
            }
            result.insert(
                "useGrouping".to_string(),
                borrow.get("__intl_use_grouping__").cloned().unwrap_or(Value::Boolean(true)),
            );
            result.insert(
                "minimumIntegerDigits".to_string(),
                borrow.get("__intl_minimum_integer_digits__").cloned().unwrap_or(Value::Number(1.0)),
            );
            result.insert(
                "minimumFractionDigits".to_string(),
                borrow
                    .get("__intl_minimum_fraction_digits__")
                    .cloned()
                    .unwrap_or(Value::Number(0.0)),
            );
            result.insert(
                "maximumFractionDigits".to_string(),
                borrow
                    .get("__intl_maximum_fraction_digits__")
                    .cloned()
                    .unwrap_or(Value::Number(3.0)),
            );
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
        if matches!(options, Value::Undefined) {
            return Ok(());
        }
        if matches!(options, Value::Null) {
            return Err(self.make_type_error_object(ctx, "options must not be null"));
        }
        if !Self::intl_is_object_like(options) {
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
            "DateTimeFormat" => &[],
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

    fn intl_date_time_format_getter(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(formatter) = self.intl_require_initialized_service(ctx, receiver, Some("DateTimeFormat")) else {
            return Value::Undefined;
        };

        if let Some(existing) = formatter.borrow().get(INTL_DATE_TIME_FORMAT_BOUND_FORMAT_SLOT).cloned() {
            return existing;
        }

        let this_val = receiver.unwrap_or(&Value::Undefined);
        let format = Self::make_bound_host_fn(ctx, "intl.dateTimeFormat.format", this_val);
        if let Value::Object(format_obj) = &format {
            let mut borrow = format_obj.borrow_mut(ctx);
            Self::insert_property_with_attributes(&mut borrow, "length", &Value::Number(1.0), false, false, true);
            Self::insert_property_with_attributes(&mut borrow, "name", &Value::from(""), false, false, true);
            borrow.insert("__non_constructor__".to_string(), Value::Boolean(true));
        }
        formatter
            .borrow_mut(ctx)
            .insert(INTL_DATE_TIME_FORMAT_BOUND_FORMAT_SLOT.to_string(), format.clone());
        format
    }

    fn intl_date_time_format_format(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, args: &[Value<'gc>]) -> Value<'gc> {
        let Some(_formatter) = self.intl_require_initialized_service(ctx, receiver, Some("DateTimeFormat")) else {
            return Value::Undefined;
        };
        let x = if args.is_empty() || matches!(args.first(), Some(Value::Undefined)) {
            chrono::Utc::now().timestamp_millis() as f64
        } else {
            let prim = self.try_to_primitive(ctx, args.first().unwrap_or(&Value::Undefined), "number");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            to_number(&prim)
        };
        if !x.is_finite() || x.is_nan() {
            self.pending_throw = Some(self.make_range_error_object(ctx, "Invalid time value"));
            return Value::Undefined;
        }
        Value::from(x.trunc().to_string().as_str())
    }

    fn intl_number_format_getter(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(formatter) = self.intl_require_initialized_service(ctx, receiver, Some("NumberFormat")) else {
            return Value::Undefined;
        };

        if let Some(existing) = formatter.borrow().get(INTL_NUMBER_FORMAT_BOUND_FORMAT_SLOT).cloned() {
            return existing;
        }

        let this_val = receiver.unwrap_or(&Value::Undefined);
        let format = Self::make_bound_host_fn(ctx, "intl.numberFormat.format", this_val);
        if let Value::Object(format_obj) = &format {
            let mut borrow = format_obj.borrow_mut(ctx);
            Self::insert_property_with_attributes(&mut borrow, "length", &Value::Number(1.0), false, false, true);
            Self::insert_property_with_attributes(&mut borrow, "name", &Value::from(""), false, false, true);
            borrow.insert("__non_constructor__".to_string(), Value::Boolean(true));
        }
        formatter
            .borrow_mut(ctx)
            .insert(INTL_NUMBER_FORMAT_BOUND_FORMAT_SLOT.to_string(), format.clone());
        format
    }

    fn intl_number_format_format(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, args: &[Value<'gc>]) -> Value<'gc> {
        let Some(_formatter) = self.intl_require_initialized_service(ctx, receiver, Some("NumberFormat")) else {
            return Value::Undefined;
        };
        let x = if args.is_empty() {
            f64::NAN
        } else {
            let prim = self.try_to_primitive(ctx, args.first().unwrap_or(&Value::Undefined), "number");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
            to_number(&prim)
        };
        if x.is_nan() {
            return Value::from("NaN");
        }
        if x.is_infinite() {
            return if x.is_sign_negative() {
                Value::from("-Infinity")
            } else {
                Value::from("Infinity")
            };
        }
        Value::from(x.to_string().as_str())
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
        Some(*obj)
    }

    fn intl_read_collator_options(
        &mut self,
        ctx: &GcContext<'gc>,
        requested_locales: &[String],
        options: Option<&Value<'gc>>,
    ) -> Result<IntlCollatorOptions, Value<'gc>> {
        self.intl_read_constructor_options(ctx, "Collator", options)?;
        let requested_locale = requested_locales
            .first()
            .cloned()
            .unwrap_or_else(|| INTL_DEFAULT_LOCALE.to_string());
        let locale_info = Self::intl_locale_info(&requested_locale);
        let locale_base = if locale_info.base.is_empty() {
            INTL_DEFAULT_LOCALE.to_string()
        } else {
            locale_info.base.clone()
        };
        let mut out = IntlCollatorOptions {
            resolved_locale: locale_base.clone(),
            usage: "sort".to_string(),
            sensitivity: "variant".to_string(),
            ignore_punctuation: locale_base.eq_ignore_ascii_case("th"),
            collation: "default".to_string(),
            numeric: None,
            case_first: None,
        };

        let ext_collation = locale_info
            .unicode_keywords
            .get("co")
            .and_then(|value| Self::intl_collator_supported_collation(&locale_base, value));
        let ext_numeric = locale_info
            .unicode_keywords
            .get("kn")
            .and_then(|value| Self::intl_collator_unicode_bool(value));
        let ext_case_first = locale_info
            .unicode_keywords
            .get("kf")
            .and_then(|value| Self::intl_collator_supported_case_first(value));

        let Some(options) = options else {
            out.collation = ext_collation.clone().unwrap_or_else(|| "default".to_string());
            out.numeric = ext_numeric;
            out.case_first = ext_case_first.clone();
            out.resolved_locale =
                Self::intl_collator_resolved_locale(&locale_base, out.collation.as_str(), out.numeric, out.case_first.as_deref());
            return Ok(out);
        };
        if matches!(options, Value::Undefined) || !Self::intl_is_object_like(options) {
            out.collation = ext_collation.clone().unwrap_or_else(|| "default".to_string());
            out.numeric = ext_numeric;
            out.case_first = ext_case_first.clone();
            out.resolved_locale =
                Self::intl_collator_resolved_locale(&locale_base, out.collation.as_str(), out.numeric, out.case_first.as_deref());
            return Ok(out);
        }

        if let Some(usage) = self.intl_string_option(ctx, options, "usage", &["sort", "search"], Some("sort"))? {
            out.usage = usage;
        }
        let _ = self.intl_locale_matcher_option(ctx, Some(options))?;
        let option_collation = self
            .intl_string_option(ctx, options, "collation", &[], None)?
            .and_then(|value| Self::intl_collator_supported_collation(&locale_base, &value));
        out.collation = if out.usage == "search" {
            "default".to_string()
        } else if let Some(collation) = option_collation.clone() {
            collation
        } else {
            ext_collation.clone().unwrap_or_else(|| "default".to_string())
        };
        out.numeric = self.intl_boolean_option(ctx, options, "numeric")?.or(ext_numeric);
        if let Some(case_first) = self.intl_string_option(ctx, options, "caseFirst", &["upper", "lower", "false"], None)? {
            out.case_first = Some(case_first);
        } else {
            out.case_first = ext_case_first.clone();
        }
        if let Some(sensitivity) =
            self.intl_string_option(ctx, options, "sensitivity", &["base", "accent", "case", "variant"], Some("variant"))?
        {
            out.sensitivity = sensitivity;
        }
        if let Some(ignore_punctuation) = self.intl_boolean_option(ctx, options, "ignorePunctuation")? {
            out.ignore_punctuation = ignore_punctuation;
        }
        let reflect_collation = ext_collation.as_deref() == Some(out.collation.as_str()) && out.collation != "default";
        let reflect_numeric = ext_numeric == out.numeric && out.numeric == Some(true);
        let reflect_case_first = ext_case_first.as_deref() == out.case_first.as_deref();
        out.resolved_locale = Self::intl_collator_resolved_locale_with_flags(
            &locale_base,
            reflect_collation.then_some(out.collation.as_str()),
            reflect_numeric.then_some(true),
            reflect_case_first.then_some(out.case_first.as_deref()).flatten(),
        );
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

    fn intl_read_date_time_format_options(
        &mut self,
        ctx: &GcContext<'gc>,
        requested_locales: &[String],
        options: Option<&Value<'gc>>,
    ) -> Result<IntlDateTimeFormatOptions, Value<'gc>> {
        self.intl_read_constructor_options(ctx, "DateTimeFormat", options)?;
        let requested_locale = requested_locales
            .first()
            .cloned()
            .unwrap_or_else(|| INTL_DEFAULT_LOCALE.to_string());
        let locale_info = Self::intl_locale_info(&requested_locale);
        let locale_base = if locale_info.base.is_empty() {
            INTL_DEFAULT_LOCALE.to_string()
        } else {
            locale_info.base.clone()
        };
        let ext_calendar = locale_info
            .unicode_keywords
            .get("ca")
            .and_then(|value| Self::intl_supported_calendar(value));
        let ext_numbering_system = locale_info
            .unicode_keywords
            .get("nu")
            .and_then(|value| Self::intl_supported_numbering_system(value));
        let ext_hour_cycle = locale_info
            .unicode_keywords
            .get("hc")
            .and_then(|value| Self::intl_supported_hour_cycle(value));
        let mut out = IntlDateTimeFormatOptions {
            resolved_locale: locale_base.clone(),
            calendar: ext_calendar.clone().unwrap_or_else(|| "gregory".to_string()),
            numbering_system: ext_numbering_system.clone().unwrap_or_else(|| "latn".to_string()),
            time_zone: "UTC".to_string(),
            year: Some("numeric".to_string()),
            month: Some("numeric".to_string()),
            day: Some("numeric".to_string()),
            weekday: None,
            era: None,
            hour: None,
            minute: None,
            second: None,
            time_zone_name: None,
            hour_cycle: None,
            hour12: None,
        };
        let Some(options) = options else {
            out.resolved_locale = Self::intl_date_time_format_resolved_locale(
                &locale_base,
                ext_calendar.as_deref(),
                ext_numbering_system.as_deref(),
                ext_hour_cycle.as_deref(),
            );
            return Ok(out);
        };
        if matches!(options, Value::Undefined) || !Self::intl_is_object_like(options) {
            out.resolved_locale = Self::intl_date_time_format_resolved_locale(
                &locale_base,
                ext_calendar.as_deref(),
                ext_numbering_system.as_deref(),
                ext_hour_cycle.as_deref(),
            );
            return Ok(out);
        }
        let _ = self.intl_string_option(ctx, options, "localeMatcher", &["lookup", "best fit"], Some("best fit"))?;
        let option_hour12 = self.intl_boolean_option(ctx, options, "hour12")?;
        let option_hour_cycle = self.intl_string_option(ctx, options, "hourCycle", &["h11", "h12", "h23", "h24"], None)?;
        if let Some(calendar) = self.intl_string_option(ctx, options, "calendar", &[], None)? {
            if !Self::intl_is_valid_unicode_type_identifier(&calendar) {
                return Err(self.make_range_error_object(ctx, "Invalid calendar"));
            }
            if let Some(calendar) = Self::intl_supported_calendar(&calendar) {
                out.calendar = calendar;
            }
        }
        if let Some(numbering_system) = self.intl_string_option(ctx, options, "numberingSystem", &[], None)? {
            if !Self::intl_is_valid_unicode_type_identifier(&numbering_system) {
                return Err(self.make_range_error_object(ctx, "Invalid numberingSystem"));
            }
            if let Some(numbering_system) = Self::intl_supported_numbering_system(&numbering_system) {
                out.numbering_system = numbering_system;
            }
        }
        if let Some(time_zone) = self.intl_string_option(ctx, options, "timeZone", &[], None)? {
            let Some(normalized_time_zone) = Self::intl_normalize_time_zone_name(&time_zone) else {
                return Err(self.make_range_error_object(ctx, "Invalid timeZone"));
            };
            out.time_zone = normalized_time_zone;
        }
        out.weekday = self.intl_string_option(ctx, options, "weekday", &["narrow", "short", "long"], None)?;
        out.era = self.intl_string_option(ctx, options, "era", &["narrow", "short", "long"], None)?;
        out.year = self.intl_string_option(ctx, options, "year", &["2-digit", "numeric"], out.year.as_deref())?;
        out.month = self.intl_string_option(
            ctx,
            options,
            "month",
            &["2-digit", "numeric", "narrow", "short", "long"],
            out.month.as_deref(),
        )?;
        out.day = self.intl_string_option(ctx, options, "day", &["2-digit", "numeric"], out.day.as_deref())?;
        out.hour = self.intl_string_option(ctx, options, "hour", &["2-digit", "numeric"], None)?;
        out.minute = self.intl_string_option(ctx, options, "minute", &["2-digit", "numeric"], None)?;
        out.second = self.intl_string_option(ctx, options, "second", &["2-digit", "numeric"], None)?;
        out.time_zone_name = self.intl_string_option(
            ctx,
            options,
            "timeZoneName",
            &["short", "long", "shortOffset", "longOffset", "shortGeneric", "longGeneric"],
            None,
        )?;
        if out.hour.is_some() {
            let resolved_hour_cycle = if let Some(hour12) = option_hour12 {
                Some(if hour12 {
                    Self::intl_default_true_hour_cycle(&locale_base)
                } else {
                    "h23".to_string()
                })
            } else if let Some(hour_cycle) = option_hour_cycle.clone() {
                Some(hour_cycle)
            } else if let Some(hour_cycle) = ext_hour_cycle.clone() {
                Some(hour_cycle)
            } else {
                Some(Self::intl_default_true_hour_cycle(&locale_base))
            };
            out.hour_cycle = resolved_hour_cycle;
            out.hour12 = out.hour_cycle.as_deref().map(Self::intl_hour_cycle_is_twelve_hour);
        }
        let _ = self.intl_string_option(ctx, options, "formatMatcher", &["basic", "best fit"], Some("best fit"))?;
        let date_style = self.intl_string_option(ctx, options, "dateStyle", &["full", "long", "medium", "short"], None)?;
        let time_style = self.intl_string_option(ctx, options, "timeStyle", &["full", "long", "medium", "short"], None)?;
        if (date_style.is_some() || time_style.is_some()) && Self::intl_has_explicit_date_time_components(ctx, self, options)? {
            return Err(self.make_type_error_object(ctx, "dateStyle/timeStyle conflicts with explicit components"));
        }
        let reflect_calendar = ext_calendar.as_deref() == Some(out.calendar.as_str());
        let reflect_numbering_system = ext_numbering_system.as_deref() == Some(out.numbering_system.as_str());
        let reflect_hour_cycle = if option_hour12.is_some() {
            false
        } else if let Some(option_hour_cycle) = option_hour_cycle.as_deref() {
            ext_hour_cycle.as_deref() == Some(option_hour_cycle)
        } else if out.hour.is_some() {
            ext_hour_cycle.as_deref() == out.hour_cycle.as_deref()
        } else {
            ext_hour_cycle.is_some()
        };
        out.resolved_locale = Self::intl_date_time_format_resolved_locale(
            &locale_base,
            reflect_calendar.then_some(out.calendar.as_str()),
            reflect_numbering_system.then_some(out.numbering_system.as_str()),
            if out.hour.is_some() {
                reflect_hour_cycle.then_some(out.hour_cycle.as_deref()).flatten()
            } else {
                reflect_hour_cycle.then_some(ext_hour_cycle.as_deref()).flatten()
            },
        );
        Ok(out)
    }

    fn intl_store_date_time_format_options(&self, obj: &mut IndexMap<String, Value<'gc>>, options: &IntlDateTimeFormatOptions) {
        obj.insert("__intl_calendar__".to_string(), Value::from(options.calendar.as_str()));
        obj.insert(
            "__intl_numbering_system__".to_string(),
            Value::from(options.numbering_system.as_str()),
        );
        obj.insert("__intl_time_zone__".to_string(), Value::from(options.time_zone.as_str()));
        if let Some(hour_cycle) = &options.hour_cycle {
            obj.insert("__intl_hour_cycle__".to_string(), Value::from(hour_cycle.as_str()));
        }
        if let Some(hour12) = options.hour12 {
            obj.insert("__intl_hour12__".to_string(), Value::Boolean(hour12));
        }
        if let Some(weekday) = &options.weekday {
            obj.insert("__intl_weekday__".to_string(), Value::from(weekday.as_str()));
        }
        if let Some(era) = &options.era {
            obj.insert("__intl_era__".to_string(), Value::from(era.as_str()));
        }
        if let Some(year) = &options.year {
            obj.insert("__intl_year__".to_string(), Value::from(year.as_str()));
        }
        if let Some(month) = &options.month {
            obj.insert("__intl_month__".to_string(), Value::from(month.as_str()));
        }
        if let Some(day) = &options.day {
            obj.insert("__intl_day__".to_string(), Value::from(day.as_str()));
        }
        if let Some(hour) = &options.hour {
            obj.insert("__intl_hour__".to_string(), Value::from(hour.as_str()));
        }
        if let Some(minute) = &options.minute {
            obj.insert("__intl_minute__".to_string(), Value::from(minute.as_str()));
        }
        if let Some(second) = &options.second {
            obj.insert("__intl_second__".to_string(), Value::from(second.as_str()));
        }
        if let Some(time_zone_name) = &options.time_zone_name {
            obj.insert("__intl_timeZoneName__".to_string(), Value::from(time_zone_name.as_str()));
        }
    }

    fn intl_read_number_format_options(
        &mut self,
        ctx: &GcContext<'gc>,
        requested_locales: &[String],
        options: Option<&Value<'gc>>,
    ) -> Result<IntlNumberFormatOptions, Value<'gc>> {
        self.intl_read_constructor_options(ctx, "NumberFormat", options)?;
        let resolved_locale = requested_locales
            .first()
            .cloned()
            .unwrap_or_else(|| INTL_DEFAULT_LOCALE.to_string());
        let mut out = IntlNumberFormatOptions {
            resolved_locale,
            numbering_system: "latn".to_string(),
            style: "decimal".to_string(),
            currency: None,
            use_grouping: true,
            minimum_integer_digits: 1,
            minimum_fraction_digits: 0,
            maximum_fraction_digits: 3,
        };
        let Some(options) = options else {
            return Ok(out);
        };
        if matches!(options, Value::Undefined) || !Self::intl_is_object_like(options) {
            return Ok(out);
        }
        if let Some(numbering_system) = self.intl_string_option(ctx, options, "numberingSystem", &[], None)? {
            if !Self::intl_is_valid_unicode_type_identifier(&numbering_system) {
                return Err(self.make_range_error_object(ctx, "Invalid numberingSystem"));
            }
            out.numbering_system = numbering_system;
        }
        if let Some(style) = self.intl_string_option(ctx, options, "style", &["decimal", "percent", "currency", "unit"], Some("decimal"))? {
            out.style = style;
        }
        let currency = self.intl_string_option(ctx, options, "currency", &[], None)?;
        if let Some(currency) = currency {
            if !Self::intl_is_well_formed_currency_code(&currency) {
                return Err(self.make_range_error_object(ctx, "Invalid currency code"));
            }
            if out.style == "currency" {
                out.currency = Some(currency.to_ascii_uppercase());
            }
        }
        if out.style == "currency" && out.currency.is_none() {
            return Err(self.make_type_error_object(ctx, "currency style requires currency"));
        }
        Ok(out)
    }

    fn intl_store_number_format_options(&self, obj: &mut IndexMap<String, Value<'gc>>, options: &IntlNumberFormatOptions) {
        obj.insert(
            "__intl_numbering_system__".to_string(),
            Value::from(options.numbering_system.as_str()),
        );
        obj.insert("__intl_style__".to_string(), Value::from(options.style.as_str()));
        if let Some(currency) = &options.currency {
            obj.insert("__intl_currency__".to_string(), Value::from(currency.as_str()));
        }
        obj.insert("__intl_use_grouping__".to_string(), Value::Boolean(options.use_grouping));
        obj.insert(
            "__intl_minimum_integer_digits__".to_string(),
            Value::Number(options.minimum_integer_digits as f64),
        );
        obj.insert(
            "__intl_minimum_fraction_digits__".to_string(),
            Value::Number(options.minimum_fraction_digits as f64),
        );
        obj.insert(
            "__intl_maximum_fraction_digits__".to_string(),
            Value::Number(options.maximum_fraction_digits as f64),
        );
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
        Self::intl_locale_info(locale).base
    }

    fn intl_locale_info(locale: &str) -> IntlLocaleInfo {
        let parts: Vec<&str> = locale.split('-').collect();
        let mut base = Vec::new();
        let mut unicode_keywords = std::collections::HashMap::new();
        let mut idx = 0;
        while idx < parts.len() {
            let part = parts[idx];
            if part.len() == 1 {
                break;
            }
            base.push(part);
            idx += 1;
        }
        if idx >= parts.len() || !parts[idx].eq_ignore_ascii_case("u") {
            return IntlLocaleInfo {
                base: base.join("-"),
                unicode_keywords,
            };
        }
        idx += 1;
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
            unicode_keywords.insert(key.to_ascii_lowercase(), value_parts.join("-"));
        }
        IntlLocaleInfo {
            base: base.join("-"),
            unicode_keywords,
        }
    }

    fn intl_collator_unicode_bool(value: &str) -> Option<bool> {
        match value {
            "" | "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }

    fn intl_collator_supported_case_first(value: &str) -> Option<String> {
        match value {
            "upper" | "lower" | "false" => Some(value.to_string()),
            _ => None,
        }
    }

    fn intl_collator_supported_collation(locale_base: &str, value: &str) -> Option<String> {
        if value.is_empty() || matches!(value, "default" | "search" | "standard" | "invalid") {
            return None;
        }
        match value {
            "phonebk" if locale_base.eq_ignore_ascii_case("de") || locale_base.to_ascii_lowercase().starts_with("de-") => {
                Some("phonebk".to_string())
            }
            "eor" => Some("eor".to_string()),
            _ => None,
        }
    }

    fn intl_collator_resolved_locale(locale_base: &str, collation: &str, numeric: Option<bool>, case_first: Option<&str>) -> String {
        Self::intl_collator_resolved_locale_with_flags(
            locale_base,
            (collation != "default").then_some(collation),
            (numeric == Some(true)).then_some(true),
            case_first,
        )
    }

    fn intl_collator_resolved_locale_with_flags(
        locale_base: &str,
        collation: Option<&str>,
        numeric: Option<bool>,
        case_first: Option<&str>,
    ) -> String {
        let mut out = locale_base.to_string();
        let mut extensions = Vec::new();
        if let Some(collation) = collation {
            extensions.push(format!("co-{}", collation));
        }
        if numeric == Some(true) {
            extensions.push("kn".to_string());
        }
        if let Some(case_first) = case_first {
            extensions.push(format!("kf-{}", case_first));
        }
        if !extensions.is_empty() {
            out.push_str("-u-");
            out.push_str(&extensions.join("-"));
        }
        out
    }

    fn intl_date_time_format_resolved_locale(
        locale_base: &str,
        calendar: Option<&str>,
        numbering_system: Option<&str>,
        hour_cycle: Option<&str>,
    ) -> String {
        let mut out = locale_base.to_string();
        let mut extensions = Vec::new();
        if let Some(calendar) = calendar {
            extensions.push(format!("ca-{}", calendar));
        }
        if let Some(numbering_system) = numbering_system {
            extensions.push(format!("nu-{}", numbering_system));
        }
        if let Some(hour_cycle) = hour_cycle {
            extensions.push(format!("hc-{}", hour_cycle));
        }
        if !extensions.is_empty() {
            out.push_str("-u-");
            out.push_str(&extensions.join("-"));
        }
        out
    }

    fn intl_has_explicit_date_time_components(ctx: &GcContext<'gc>, vm: &mut VM<'gc>, options: &Value<'gc>) -> Result<bool, Value<'gc>> {
        for key in [
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
        ] {
            let value = vm.read_named_property(ctx, options, key);
            if let Some(thrown) = vm.pending_throw.take() {
                return Err(thrown);
            }
            if !matches!(value, Value::Undefined) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn intl_is_valid_unicode_type_identifier(value: &str) -> bool {
        if value.is_empty() || !value.is_ascii() {
            return false;
        }
        value
            .split('-')
            .all(|part| (3..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric()))
    }

    fn intl_normalize_time_zone_name(value: &str) -> Option<String> {
        if value.eq_ignore_ascii_case("utc") {
            return Some("UTC".to_string());
        }
        if let Some(offset) = Self::intl_normalize_offset_time_zone(value) {
            return Some(offset);
        }
        let (left, right) = value.split_once('/')?;
        if left.is_empty() || right.is_empty() {
            return None;
        }
        if !left
            .chars()
            .chain(right.chars())
            .all(|c| c.is_ascii_alphabetic() || c == '_' || c == '-' || c == '/')
        {
            return None;
        }
        Some(value.to_string())
    }

    fn intl_normalize_offset_time_zone(value: &str) -> Option<String> {
        let sign = match value.as_bytes().first().copied()? {
            b'+' => '+',
            b'-' => '-',
            _ => return None,
        };
        let digits = &value[1..];
        let (hours, minutes) = match digits.len() {
            2 => (digits, "00"),
            4 if digits.chars().all(|c| c.is_ascii_digit()) => (&digits[..2], &digits[2..]),
            5 if digits.as_bytes()[2] == b':' => (&digits[..2], &digits[3..]),
            _ => return None,
        };
        if !hours.chars().all(|c| c.is_ascii_digit()) || !minutes.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let hour: u8 = hours.parse().ok()?;
        let minute: u8 = minutes.parse().ok()?;
        if hour > 23 || minute > 59 {
            return None;
        }
        let normalized_sign = if hour == 0 && minute == 0 { '+' } else { sign };
        Some(format!("{}{hours}:{minutes}", normalized_sign))
    }

    fn intl_supported_calendar(value: &str) -> Option<String> {
        match value {
            "gregory" | "iso8601" => Some(value.to_string()),
            _ => None,
        }
    }

    fn intl_supported_numbering_system(value: &str) -> Option<String> {
        match value {
            "latn" | "arab" => Some(value.to_string()),
            _ => None,
        }
    }

    fn intl_supported_hour_cycle(value: &str) -> Option<String> {
        match value {
            "h11" | "h12" | "h23" | "h24" => Some(value.to_string()),
            _ => None,
        }
    }

    fn intl_default_true_hour_cycle(locale_base: &str) -> String {
        if locale_base.eq("ja") || locale_base.starts_with("ja-") {
            "h11".to_string()
        } else {
            "h12".to_string()
        }
    }

    fn intl_hour_cycle_is_twelve_hour(value: &str) -> bool {
        matches!(value, "h11" | "h12")
    }

    fn intl_is_well_formed_currency_code(value: &str) -> bool {
        value.len() == 3 && value.chars().all(|c| c.is_ascii_alphabetic())
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
    resolved_locale: String,
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
            resolved_locale: match obj.get("__intl_locale__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => INTL_DEFAULT_LOCALE.to_string(),
            },
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

struct IntlLocaleInfo {
    base: String,
    unicode_keywords: std::collections::HashMap<String, String>,
}

struct IntlDateTimeFormatOptions {
    resolved_locale: String,
    calendar: String,
    numbering_system: String,
    time_zone: String,
    weekday: Option<String>,
    era: Option<String>,
    year: Option<String>,
    month: Option<String>,
    day: Option<String>,
    hour: Option<String>,
    minute: Option<String>,
    second: Option<String>,
    time_zone_name: Option<String>,
    hour_cycle: Option<String>,
    hour12: Option<bool>,
}

struct IntlNumberFormatOptions {
    resolved_locale: String,
    numbering_system: String,
    style: String,
    currency: Option<String>,
    use_grouping: bool,
    minimum_integer_digits: u8,
    minimum_fraction_digits: u8,
    maximum_fraction_digits: u8,
}
