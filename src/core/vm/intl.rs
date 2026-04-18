use super::*;
use crate::core::GcPtr;
use unicode_normalization::UnicodeNormalization;

const INTL_DEFAULT_LOCALE: &str = "en-US";
const INTL_COLLATOR_BOUND_COMPARE_SLOT: &str = "@@sym:900001";
const INTL_DATE_TIME_FORMAT_BOUND_FORMAT_SLOT: &str = "@@sym:900003";
const INTL_NUMBER_FORMAT_BOUND_FORMAT_SLOT: &str = "@@sym:900004";
const INTL_SUPPORTED_CALENDARS: &[&str] = &[
    "buddhist",
    "chinese",
    "coptic",
    "dangi",
    "ethioaa",
    "ethiopic",
    "gregory",
    "hebrew",
    "indian",
    "islamic-civil",
    "islamic-tbla",
    "islamic-umalqura",
    "iso8601",
    "japanese",
    "persian",
    "roc",
];
const INTL_SUPPORTED_COLLATIONS: &[&str] = &[
    "compat", "dict", "emoji", "eor", "phonebk", "phonetic", "pinyin", "searchjl", "stroke", "trad", "unihan", "zhuyin",
];
const INTL_SUPPORTED_CURRENCIES: &[&str] = &[
    "AED", "AFN", "ALL", "AMD", "ANG", "AOA", "ARS", "AUD", "AWG", "AZN", "BAM", "BBD", "BDT", "BGN", "BHD", "BIF", "BMD", "BND", "BOB",
    "BRL",
];
const INTL_SUPPORTED_NUMBERING_SYSTEMS: &[&str] = &[
    "adlm", "ahom", "arab", "arabext", "armn", "armnlow", "bali", "beng", "bhks", "brah", "cakm", "cham", "cyrl", "deva", "diak", "ethi",
    "fullwide", "gara", "geor", "gong", "gonm", "grek", "greklow", "gujr", "gukh", "guru", "hanidays", "hanidec", "hans", "hansfin",
    "hant", "hantfin", "hebr", "hmng", "hmnp", "java", "jpan", "jpanfin", "jpanyear", "kali", "kawi", "khmr", "knda", "krai", "lana",
    "lanatham", "laoo", "latn", "lepc", "limb", "mathbold", "mathdbl", "mathmono", "mathsanb", "mathsans", "mlym", "modi", "mong", "mroo",
    "mtei", "mymr", "mymrepka", "mymrpao", "mymrshan", "mymrtlng", "nagm", "newa", "nkoo", "olck", "onao", "orya", "osma", "outlined",
    "rohg", "roman", "romanlow", "saur", "segment", "shrd", "sind", "sinh", "sora", "sund", "sunu", "takr", "talu", "taml", "tamldec",
    "telu", "thai", "tibt", "tirh", "tnsa", "tols", "vaii", "wara", "wcho",
];
const INTL_SUPPORTED_TIME_ZONES: &[&str] = &[
    "Africa/Abidjan",
    "America/New_York",
    "Antarctica/McMurdo",
    "Arctic/Longyearbyen",
    "Asia/Tokyo",
    "Atlantic/Azores",
    "Australia/Sydney",
    "Etc/GMT+1",
    "Etc/GMT+10",
    "Etc/GMT+11",
    "Etc/GMT+12",
    "Etc/GMT+2",
    "Etc/GMT+3",
    "Etc/GMT+4",
    "Etc/GMT+5",
    "Etc/GMT+6",
    "Etc/GMT+7",
    "Etc/GMT+8",
    "Etc/GMT+9",
    "Etc/GMT-1",
    "Etc/GMT-10",
    "Etc/GMT-11",
    "Etc/GMT-12",
    "Etc/GMT-13",
    "Etc/GMT-14",
    "Etc/GMT-2",
    "Etc/GMT-3",
    "Etc/GMT-4",
    "Etc/GMT-5",
    "Etc/GMT-6",
    "Etc/GMT-7",
    "Etc/GMT-8",
    "Etc/GMT-9",
    "Europe/London",
    "Indian/Maldives",
    "Pacific/Apia",
    "Pacific/Honolulu",
    "Pacific/Kiritimati",
    "Pacific/Marquesas",
    "UTC",
];
const INTL_SUPPORTED_UNITS: &[&str] = &[
    "acre",
    "bit",
    "byte",
    "celsius",
    "centimeter",
    "day",
    "degree",
    "fahrenheit",
    "fluid-ounce",
    "foot",
    "gallon",
    "gigabit",
    "gigabyte",
    "gram",
    "hectare",
    "hour",
    "inch",
    "kilobit",
    "kilobyte",
    "kilogram",
    "kilometer",
    "liter",
    "megabit",
    "megabyte",
    "meter",
    "microsecond",
    "mile",
    "mile-scandinavian",
    "milliliter",
    "millimeter",
    "millisecond",
    "minute",
    "month",
    "nanosecond",
    "ounce",
    "percent",
    "petabyte",
    "pound",
    "second",
    "stone",
    "terabit",
    "terabyte",
    "week",
    "yard",
    "year",
];

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
        intl.insert(
            "supportedValuesOf".to_string(),
            Self::make_host_fn_with_name_len(ctx, "intl.supportedValuesOf", "supportedValuesOf", 1.0, false),
        );
        mark_nonenumerable(&mut intl, "supportedValuesOf");
        Self::insert_property_with_attributes(&mut intl, "@@sym:4", &Value::from("Intl"), false, false, true);

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
            return match self.intl_construct_service_instance(ctx, kind, None, args) {
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
            "intl.supportedValuesOf" => self.intl_supported_values_of(ctx, args.first()),
            "intl.displayNames.of" => self.intl_display_names_of(ctx, receiver, args.first()),
            "intl.collator.get.compare" => self.intl_collator_compare_getter(ctx, receiver),
            "intl.collator.compare" => self.intl_collator_compare(ctx, receiver, args),
            "intl.dateTimeFormat.get.format" => self.intl_date_time_format_getter(ctx, receiver),
            "intl.dateTimeFormat.format" => self.intl_date_time_format_format(ctx, receiver, args),
            "intl.dateTimeFormat.formatToParts" => self.intl_date_time_format_format_to_parts(ctx, receiver, args.first()),
            "intl.dateTimeFormat.formatRange" => self.intl_date_time_format_format_range(ctx, receiver, args.first(), args.get(1)),
            "intl.dateTimeFormat.formatRangeToParts" => {
                self.intl_date_time_format_format_range_to_parts(ctx, receiver, args.first(), args.get(1))
            }
            "intl.locale.toString" => self.intl_locale_to_string(ctx, receiver),
            "intl.numberFormat.get.format" => self.intl_number_format_getter(ctx, receiver),
            "intl.numberFormat.format" => self.intl_number_format_format(ctx, receiver, args),
            "intl.numberFormat.formatToParts" => self.intl_number_format_format_to_parts(ctx, receiver, args.first()),
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
            Self::insert_property_with_attributes(
                &mut proto,
                "@@sym:4",
                &Value::from(format!("Intl.{display_name}").as_str()),
                false,
                false,
                true,
            );
            if display_name == "Collator" {
                let getter = Self::make_host_fn_with_name_len(ctx, "intl.collator.get.compare", "get compare", 0.0, false);
                Self::insert_getter_property_with_attributes(&mut proto, "compare", &getter, false, true);
            }
            if display_name == "DateTimeFormat" {
                let getter = Self::make_host_fn_with_name_len(ctx, "intl.dateTimeFormat.get.format", "get format", 0.0, false);
                Self::insert_getter_property_with_attributes(&mut proto, "format", &getter, false, true);
                proto.insert(
                    "formatToParts".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.dateTimeFormat.formatToParts", "formatToParts", 1.0, false),
                );
                mark_nonenumerable(&mut proto, "formatToParts");
                proto.insert(
                    "formatRange".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.dateTimeFormat.formatRange", "formatRange", 2.0, false),
                );
                mark_nonenumerable(&mut proto, "formatRange");
                proto.insert(
                    "formatRangeToParts".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.dateTimeFormat.formatRangeToParts", "formatRangeToParts", 2.0, false),
                );
                mark_nonenumerable(&mut proto, "formatRangeToParts");
            }
            if display_name == "NumberFormat" {
                let getter = Self::make_host_fn_with_name_len(ctx, "intl.numberFormat.get.format", "get format", 0.0, false);
                Self::insert_getter_property_with_attributes(&mut proto, "format", &getter, false, true);
                proto.insert(
                    "formatToParts".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.numberFormat.formatToParts", "formatToParts", 1.0, false),
                );
                mark_nonenumerable(&mut proto, "formatToParts");
            }
            if display_name == "Segmenter" {
                proto.insert(
                    "segment".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.segmenter.segment", "segment", 1.0, false),
                );
                mark_nonenumerable(&mut proto, "segment");
            }
            if display_name == "DisplayNames" {
                proto.insert(
                    "of".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.displayNames.of", "of", 1.0, false),
                );
                mark_nonenumerable(&mut proto, "of");
            }
            if display_name == "Locale" {
                proto.insert(
                    "toString".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.locale.toString", "toString", 0.0, false),
                );
                mark_nonenumerable(&mut proto, "toString");
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

    pub(super) fn intl_construct_service_instance(
        &mut self,
        ctx: &GcContext<'gc>,
        kind: &str,
        ctor_value: Option<&Value<'gc>>,
        args: &[Value<'gc>],
    ) -> Result<Value<'gc>, Value<'gc>> {
        if kind == "Locale" {
            return self.intl_construct_locale(ctx, ctor_value, args);
        }
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
        let generic_numbering_system_options = if matches!(kind, "RelativeTimeFormat" | "DurationFormat") {
            Some(self.intl_read_generic_numbering_system_options(ctx, &requested_locales, args.get(1))?)
        } else {
            None
        };
        let display_names_options = if kind == "DisplayNames" {
            Some(self.intl_read_display_names_options(ctx, args.get(1))?)
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
            .or_else(|| generic_numbering_system_options.as_ref().map(|opts| opts.resolved_locale.clone()))
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
        if let Some(options) = generic_numbering_system_options {
            obj.insert(
                "__intl_numbering_system__".to_string(),
                Value::from(options.numbering_system.as_str()),
            );
        }
        if let Some((display_type, fallback)) = display_names_options {
            obj.insert("__intl_type__".to_string(), Value::from(display_type.as_str()));
            obj.insert("__intl_fallback__".to_string(), Value::from(fallback.as_str()));
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
            result.insert(
                "notation".to_string(),
                borrow.get("__intl_notation__").cloned().unwrap_or_else(|| Value::from("standard")),
            );
            if let Some(currency) = borrow.get("__intl_currency__").cloned() {
                result.insert("currency".to_string(), currency);
            }
            if let Some(currency_display) = borrow.get("__intl_currency_display__").cloned() {
                result.insert("currencyDisplay".to_string(), currency_display);
            }
            if let Some(currency_sign) = borrow.get("__intl_currency_sign__").cloned() {
                result.insert("currencySign".to_string(), currency_sign);
            }
            if let Some(unit) = borrow.get("__intl_unit__").cloned() {
                result.insert("unit".to_string(), unit);
            }
            if let Some(unit_display) = borrow.get("__intl_unit_display__").cloned() {
                result.insert("unitDisplay".to_string(), unit_display);
            }
            if let Some(compact_display) = borrow.get("__intl_compact_display__").cloned() {
                result.insert("compactDisplay".to_string(), compact_display);
            }
            result.insert(
                "signDisplay".to_string(),
                borrow.get("__intl_sign_display__").cloned().unwrap_or_else(|| Value::from("auto")),
            );
            result.insert(
                "roundingIncrement".to_string(),
                borrow.get("__intl_rounding_increment__").cloned().unwrap_or(Value::Number(1.0)),
            );
            result.insert(
                "trailingZeroDisplay".to_string(),
                borrow
                    .get("__intl_trailing_zero_display__")
                    .cloned()
                    .unwrap_or_else(|| Value::from("auto")),
            );
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
            if let Some(minimum_significant_digits) = borrow.get("__intl_minimum_significant_digits__").cloned() {
                result.insert("minimumSignificantDigits".to_string(), minimum_significant_digits);
            }
            if let Some(maximum_significant_digits) = borrow.get("__intl_maximum_significant_digits__").cloned() {
                result.insert("maximumSignificantDigits".to_string(), maximum_significant_digits);
            }
        } else if matches!(borrow.get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "RelativeTimeFormat")
        {
            result.insert(
                "numberingSystem".to_string(),
                borrow
                    .get("__intl_numbering_system__")
                    .cloned()
                    .unwrap_or_else(|| Value::from("latn")),
            );
        } else if matches!(borrow.get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "DurationFormat")
        {
            result.insert(
                "numberingSystem".to_string(),
                borrow
                    .get("__intl_numbering_system__")
                    .cloned()
                    .unwrap_or_else(|| Value::from("latn")),
            );
        } else if matches!(borrow.get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "DisplayNames")
        {
            result.insert(
                "type".to_string(),
                borrow.get("__intl_type__").cloned().unwrap_or_else(|| Value::from("language")),
            );
            result.insert(
                "fallback".to_string(),
                borrow.get("__intl_fallback__").cloned().unwrap_or_else(|| Value::from("code")),
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

    fn intl_supported_values_of(&mut self, ctx: &GcContext<'gc>, key: Option<&Value<'gc>>) -> Value<'gc> {
        let key = match self.vm_to_string_like_spec(ctx, key.unwrap_or(&Value::Undefined)) {
            Ok(key) => key,
            Err(err) => {
                self.pending_throw = Some(self.vm_value_from_error(ctx, &err));
                return Value::Undefined;
            }
        };
        let values = match key.as_str() {
            "calendar" => INTL_SUPPORTED_CALENDARS,
            "collation" => INTL_SUPPORTED_COLLATIONS,
            "currency" => INTL_SUPPORTED_CURRENCIES,
            "numberingSystem" => INTL_SUPPORTED_NUMBERING_SYSTEMS,
            "timeZone" => INTL_SUPPORTED_TIME_ZONES,
            "unit" => INTL_SUPPORTED_UNITS,
            _ => {
                self.pending_throw = Some(self.make_range_error_object(ctx, "Invalid key"));
                return Value::Undefined;
            }
        };
        Value::Array(new_gc_cell_ptr(
            ctx,
            VmArrayData::new(values.iter().map(|value| Value::from(*value)).collect()),
        ))
    }

    fn intl_construct_locale(
        &mut self,
        ctx: &GcContext<'gc>,
        ctor_value: Option<&Value<'gc>>,
        args: &[Value<'gc>],
    ) -> Result<Value<'gc>, Value<'gc>> {
        let requested_locales = self.intl_canonicalize_locale_list(ctx, args.first())?;
        let requested = requested_locales.first().cloned().unwrap_or_else(|| "und".to_string());
        let locale_info = Self::intl_locale_info(&requested);
        let mut calendar = locale_info
            .unicode_keywords
            .get("ca")
            .and_then(|value| Self::intl_supported_calendar(value));
        let mut collation = locale_info
            .unicode_keywords
            .get("co")
            .and_then(|value| Self::intl_collator_supported_collation("", value));
        let mut numbering_system = locale_info
            .unicode_keywords
            .get("nu")
            .and_then(|value| Self::intl_supported_numbering_system(value));
        if let Some(options) = args.get(1)
            && !matches!(options, Value::Undefined)
        {
            if matches!(options, Value::Null) {
                return Err(self.make_type_error_object(ctx, "options must not be null"));
            }
            if Self::intl_is_object_like(options) {
                if let Some(value) = self.intl_string_option(ctx, options, "calendar", &[], None)? {
                    if !Self::intl_is_valid_unicode_type_identifier(&value) {
                        return Err(self.make_range_error_object(ctx, "Invalid calendar"));
                    }
                    calendar = Self::intl_supported_calendar(&value);
                }
                if let Some(value) = self.intl_string_option(ctx, options, "collation", &[], None)? {
                    if !Self::intl_is_valid_unicode_type_identifier(&value) {
                        return Err(self.make_range_error_object(ctx, "Invalid collation"));
                    }
                    collation = Self::intl_collator_supported_collation("", &value);
                }
                if let Some(value) = self.intl_string_option(ctx, options, "numberingSystem", &[], None)? {
                    if !Self::intl_is_valid_unicode_type_identifier(&value) {
                        return Err(self.make_range_error_object(ctx, "Invalid numberingSystem"));
                    }
                    numbering_system = Self::intl_supported_numbering_system(&value);
                }
            }
        }
        let locale = Self::intl_locale_with_extensions(
            &locale_info.base,
            calendar.as_deref(),
            collation.as_deref(),
            numbering_system.as_deref(),
        );
        let ctor_value = ctor_value
            .cloned()
            .or_else(|| self.intl_service_constructor_from_global("Locale"))
            .unwrap_or(Value::Undefined);
        let prototype = self.read_named_property(ctx, &ctor_value, "prototype");
        if let Some(thrown) = self.pending_throw.take() {
            return Err(thrown);
        }
        let mut obj = IndexMap::new();
        if !matches!(prototype, Value::Undefined) {
            obj.insert("__proto__".to_string(), prototype);
        }
        obj.insert("__intl_kind__".to_string(), Value::from("Locale"));
        obj.insert("__intl_locale__".to_string(), Value::from(locale.as_str()));
        obj.insert("baseName".to_string(), Value::from(locale_info.base.as_str()));
        Self::insert_property_with_attributes(
            &mut obj,
            "calendar",
            &calendar.as_deref().map(Value::from).unwrap_or(Value::Undefined),
            false,
            false,
            true,
        );
        Self::insert_property_with_attributes(
            &mut obj,
            "collation",
            &collation.as_deref().map(Value::from).unwrap_or(Value::Undefined),
            false,
            false,
            true,
        );
        Self::insert_property_with_attributes(
            &mut obj,
            "numberingSystem",
            &numbering_system.as_deref().map(Value::from).unwrap_or(Value::Undefined),
            false,
            false,
            true,
        );
        Ok(Value::Object(new_gc_cell_ptr(ctx, obj)))
    }

    fn intl_display_names_of(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, value: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = value else {
            self.throw_type_error(ctx, "value is required");
            return Value::Undefined;
        };
        let display_type = receiver.and_then(|receiver| match receiver {
            Value::Object(obj) => obj.borrow().get("__intl_type__").cloned(),
            _ => None,
        });
        let fallback = receiver.and_then(|receiver| match receiver {
            Value::Object(obj) => obj.borrow().get("__intl_fallback__").cloned(),
            _ => None,
        });
        let fallback_none = matches!(fallback, Some(Value::String(text)) if crate::unicode::utf16_to_utf8(&text) == "none");
        match self.vm_to_string_like_spec(ctx, value) {
            Ok(text) => {
                let display_type = match display_type {
                    Some(Value::String(kind)) => crate::unicode::utf16_to_utf8(&kind),
                    _ => String::new(),
                };
                let supported = match display_type.as_str() {
                    "calendar" => Self::intl_supported_calendar(&text).is_some(),
                    "currency" => {
                        let upper = text.to_ascii_uppercase();
                        Self::intl_is_well_formed_currency_code(&text)
                            && INTL_SUPPORTED_CURRENCIES.iter().any(|candidate| *candidate == upper)
                    }
                    "numberingSystem" => Self::intl_supported_numbering_system(&text).is_some(),
                    _ => true,
                };
                if supported {
                    match display_type.as_str() {
                        "currency" => Value::from(text.to_ascii_uppercase().as_str()),
                        "calendar" | "numberingSystem" => Value::from(text.to_ascii_lowercase().as_str()),
                        _ => Value::from(text.as_str()),
                    }
                } else if fallback_none {
                    Value::Undefined
                } else {
                    Value::from(text.as_str())
                }
            }
            Err(err) => {
                self.pending_throw = Some(self.vm_value_from_error(ctx, &err));
                Value::Undefined
            }
        }
    }

    fn intl_locale_to_string(&mut self, _ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(Value::Object(obj)) = receiver else {
            return Value::Undefined;
        };
        obj.borrow().get("__intl_locale__").cloned().unwrap_or(Value::Undefined)
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
                Ok(vec![Self::intl_canonicalize_locale_tag(&tag)])
            }
            _ => self.intl_canonicalize_locale_list_from_object(ctx, value),
        }
    }

    fn intl_canonicalize_locale_list_from_object(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>) -> Result<Vec<String>, Value<'gc>> {
        let boxed_value;
        let target = if Self::intl_is_object_like(value) {
            value
        } else {
            boxed_value = self.intl_box_locale_list_value(ctx, value);
            &boxed_value
        };
        let Some(len) = self.array_like_length_u64(ctx, target) else {
            let thrown = self
                .pending_throw
                .take()
                .unwrap_or_else(|| self.make_type_error_object(ctx, "Invalid locales"));
            return Err(thrown);
        };
        let mut out = Vec::new();
        for index in 0..len {
            let present = self.array_like_has_index_u64(ctx, target, index)?;
            if !present {
                continue;
            }
            let key = index.to_string();
            let entry = self.read_named_property(ctx, target, &key);
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
            let canonicalized = Self::intl_canonicalize_locale_tag(&tag);
            if !out.iter().any(|existing| existing == &canonicalized) {
                out.push(canonicalized);
            }
        }
        Ok(out)
    }

    fn intl_box_locale_list_value(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>) -> Value<'gc> {
        let mut wrapper = IndexMap::new();
        match value {
            Value::Boolean(boolean) => {
                wrapper.insert("__type__".to_string(), Value::from("Boolean"));
                wrapper.insert("__value__".to_string(), Value::Boolean(*boolean));
                if let Some(proto) = self.ctor_prototype_from_globals(ctx, "Boolean") {
                    wrapper.insert("__proto__".to_string(), proto);
                }
            }
            Value::Number(number) => {
                wrapper.insert("__type__".to_string(), Value::from("Number"));
                wrapper.insert("__value__".to_string(), Value::Number(*number));
                if let Some(proto) = self.ctor_prototype_from_globals(ctx, "Number") {
                    wrapper.insert("__proto__".to_string(), proto);
                }
            }
            Value::BigInt(bigint) => {
                wrapper.insert("__type__".to_string(), Value::from("BigInt"));
                wrapper.insert("__value__".to_string(), Value::BigInt(bigint.clone()));
                if let Some(proto) = self.ctor_prototype_from_globals(ctx, "BigInt") {
                    wrapper.insert("__proto__".to_string(), proto);
                }
            }
            Value::Symbol(symbol) => {
                wrapper.insert("__type__".to_string(), Value::from("Symbol"));
                wrapper.insert("__value__".to_string(), Value::Symbol(*symbol));
                if let Some(proto) = self.ctor_prototype_from_globals(ctx, "Symbol") {
                    wrapper.insert("__proto__".to_string(), proto);
                }
            }
            _ => return value.clone(),
        }
        Value::Object(new_gc_cell_ptr(ctx, wrapper))
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

    pub(super) fn intl_date_time_format_format(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        args: &[Value<'gc>],
    ) -> Value<'gc> {
        let Some(formatter) = self.intl_require_initialized_service(ctx, receiver, Some("DateTimeFormat")) else {
            return Value::Undefined;
        };
        let Some(x) = self.intl_date_time_format_clip_value(ctx, args.first(), false) else {
            return Value::Undefined;
        };
        match Self::intl_date_time_format_render(&formatter.borrow(), x) {
            Ok(text) => Value::from(text.as_str()),
            Err(err) => {
                self.pending_throw = Some(err);
                Value::Undefined
            }
        }
    }

    fn intl_date_time_format_format_to_parts(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        date: Option<&Value<'gc>>,
    ) -> Value<'gc> {
        let Some(formatter) = self.intl_require_initialized_service(ctx, receiver, Some("DateTimeFormat")) else {
            return Value::Undefined;
        };
        let Some(x) = self.intl_date_time_format_clip_value(ctx, date, false) else {
            return Value::Undefined;
        };
        match Self::intl_date_time_format_parts_array(ctx, &formatter.borrow(), x, None) {
            Ok(value) => value,
            Err(err) => {
                self.pending_throw = Some(err);
                Value::Undefined
            }
        }
    }

    fn intl_date_time_format_format_range(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        start: Option<&Value<'gc>>,
        end: Option<&Value<'gc>>,
    ) -> Value<'gc> {
        let Some(formatter) = self.intl_require_initialized_service(ctx, receiver, Some("DateTimeFormat")) else {
            return Value::Undefined;
        };
        let Some(start) = self.intl_date_time_format_clip_value(ctx, start, true) else {
            return Value::Undefined;
        };
        let Some(end) = self.intl_date_time_format_clip_value(ctx, end, true) else {
            return Value::Undefined;
        };
        let borrow = formatter.borrow();
        let start_text = match Self::intl_date_time_format_render(&borrow, start) {
            Ok(text) => text,
            Err(err) => {
                self.pending_throw = Some(err);
                return Value::Undefined;
            }
        };
        let end_text = match Self::intl_date_time_format_render(&borrow, end) {
            Ok(text) => text,
            Err(err) => {
                self.pending_throw = Some(err);
                return Value::Undefined;
            }
        };
        if start_text == end_text {
            return Value::from(start_text.as_str());
        }
        Value::from(format!("{start_text} – {end_text}").as_str())
    }

    fn intl_date_time_format_format_range_to_parts(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        start: Option<&Value<'gc>>,
        end: Option<&Value<'gc>>,
    ) -> Value<'gc> {
        let Some(formatter) = self.intl_require_initialized_service(ctx, receiver, Some("DateTimeFormat")) else {
            return Value::Undefined;
        };
        let Some(start) = self.intl_date_time_format_clip_value(ctx, start, true) else {
            return Value::Undefined;
        };
        let Some(end) = self.intl_date_time_format_clip_value(ctx, end, true) else {
            return Value::Undefined;
        };
        let borrow = formatter.borrow();
        let start_text = match Self::intl_date_time_format_render(&borrow, start) {
            Ok(text) => text,
            Err(err) => {
                self.pending_throw = Some(err);
                return Value::Undefined;
            }
        };
        let end_text = match Self::intl_date_time_format_render(&borrow, end) {
            Ok(text) => text,
            Err(err) => {
                self.pending_throw = Some(err);
                return Value::Undefined;
            }
        };
        if start_text == end_text {
            return match Self::intl_date_time_format_parts_array(ctx, &borrow, start, Some("shared")) {
                Ok(value) => value,
                Err(err) => {
                    self.pending_throw = Some(err);
                    Value::Undefined
                }
            };
        }
        let mut values = match Self::intl_date_time_format_parts(&borrow, start) {
            Ok(parts) => parts
                .into_iter()
                .map(|(part_type, value)| Self::intl_date_time_format_part_object(ctx, &part_type, &value, Some("startRange")))
                .collect::<Vec<_>>(),
            Err(err) => {
                self.pending_throw = Some(err);
                return Value::Undefined;
            }
        };
        values.push(Self::intl_date_time_format_part_object(ctx, "literal", " – ", Some("shared")));
        match Self::intl_date_time_format_parts(&borrow, end) {
            Ok(parts) => values.extend(
                parts
                    .into_iter()
                    .map(|(part_type, value)| Self::intl_date_time_format_part_object(ctx, &part_type, &value, Some("endRange"))),
            ),
            Err(err) => {
                self.pending_throw = Some(err);
                return Value::Undefined;
            }
        }
        Value::Array(new_gc_cell_ptr(ctx, VmArrayData::new(values)))
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

    pub(super) fn intl_number_format_format(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        args: &[Value<'gc>],
    ) -> Value<'gc> {
        let Some(formatter) = self.intl_require_initialized_service(ctx, receiver, Some("NumberFormat")) else {
            return Value::Undefined;
        };
        match self.intl_number_format_format_value(ctx, &formatter.borrow(), args.first()) {
            Ok(text) => Value::from(text.as_str()),
            Err(err) => {
                self.pending_throw = Some(err);
                Value::Undefined
            }
        }
    }

    fn intl_number_format_format_to_parts(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        value: Option<&Value<'gc>>,
    ) -> Value<'gc> {
        let Some(formatter) = self.intl_require_initialized_service(ctx, receiver, Some("NumberFormat")) else {
            return Value::Undefined;
        };
        match self.intl_number_format_parts_array(ctx, &formatter.borrow(), value) {
            Ok(value) => value,
            Err(err) => {
                self.pending_throw = Some(err);
                Value::Undefined
            }
        }
    }

    fn intl_number_format_parts_array(
        &mut self,
        ctx: &GcContext<'gc>,
        formatter: &IndexMap<String, Value<'gc>>,
        value: Option<&Value<'gc>>,
    ) -> Result<Value<'gc>, Value<'gc>> {
        let formatted = self.intl_number_format_format_value(ctx, formatter, value)?;
        let options = IntlNumberFormatOptions::from_object(formatter);
        if formatted == "NaN" {
            return Ok(Value::Array(new_gc_cell_ptr(
                ctx,
                VmArrayData::new(vec![Self::intl_part_object(ctx, "nan", &formatted)]),
            )));
        }
        if formatted == "Infinity" || formatted == "-Infinity" {
            let mut parts = Vec::new();
            if let Some(rest) = formatted.strip_prefix('-') {
                parts.push(Self::intl_part_object(ctx, "minusSign", "-"));
                parts.push(Self::intl_part_object(ctx, "infinity", rest));
            } else {
                parts.push(Self::intl_part_object(ctx, "infinity", &formatted));
            }
            return Ok(Value::Array(new_gc_cell_ptr(ctx, VmArrayData::new(parts))));
        }
        if options.style == "unit"
            && let Some(pattern) = Self::intl_unit_pattern(&options)
        {
            let mut parts = Vec::new();
            let mut rest = formatted.as_str();
            if let Some(prefix_unit) = pattern.prefix_unit.as_deref() {
                rest = rest.strip_prefix(prefix_unit).unwrap_or(rest);
                parts.push(Self::intl_part_object(ctx, "unit", prefix_unit));
            }
            if let Some(prefix_literal) = pattern.prefix_literal.as_deref() {
                rest = rest.strip_prefix(prefix_literal).unwrap_or(rest);
                parts.push(Self::intl_part_object(ctx, "literal", prefix_literal));
            }
            let numeric_end = if let Some(suffix_literal) = pattern.suffix_literal.as_deref() {
                rest.strip_suffix(&(suffix_literal.to_string() + &pattern.suffix_unit))
                    .map(|numeric| (numeric, Some(suffix_literal)))
            } else {
                rest.strip_suffix(&pattern.suffix_unit).map(|numeric| (numeric, None))
            };
            if let Some((numeric, suffix_literal)) = numeric_end {
                parts.extend(Self::intl_number_string_parts(ctx, formatter, numeric));
                if let Some(suffix_literal) = suffix_literal {
                    parts.push(Self::intl_part_object(ctx, "literal", suffix_literal));
                }
                parts.push(Self::intl_part_object(ctx, "unit", &pattern.suffix_unit));
                return Ok(Value::Array(new_gc_cell_ptr(ctx, VmArrayData::new(parts))));
            }
        }
        Ok(Value::Array(new_gc_cell_ptr(
            ctx,
            VmArrayData::new(Self::intl_number_string_parts(ctx, formatter, &formatted)),
        )))
    }

    fn intl_number_string_parts(ctx: &GcContext<'gc>, formatter: &IndexMap<String, Value<'gc>>, formatted: &str) -> Vec<Value<'gc>> {
        let locale_is_de = formatter
            .get("__intl_locale__")
            .is_some_and(|v| matches!(v, Value::String(s) if crate::unicode::utf16_to_utf8(s).starts_with("de")));
        let decimal_separator = if locale_is_de { ',' } else { '.' };
        let group_separator = if locale_is_de { '.' } else { ',' };
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut current_type = "integer";
        let mut seen_decimal = false;
        for ch in formatted.chars() {
            let next_type = match ch {
                '-' => "minusSign",
                c if c == group_separator => "group",
                c if c == decimal_separator => "decimal",
                '%' => "percentSign",
                '\u{a0}' | ' ' => "literal",
                c if c.is_ascii_digit() => {
                    if seen_decimal {
                        "fraction"
                    } else {
                        "integer"
                    }
                }
                _ => "literal",
            };
            if current.is_empty() {
                current_type = next_type;
            } else if current_type != next_type {
                parts.push(Self::intl_part_object(ctx, current_type, &current));
                current.clear();
                current_type = next_type;
            }
            current.push(ch);
            if ch == decimal_separator {
                seen_decimal = true;
            }
        }
        if !current.is_empty() {
            parts.push(Self::intl_part_object(ctx, current_type, &current));
        }
        parts
    }

    fn intl_part_object(ctx: &GcContext<'gc>, part_type: &str, value: &str) -> Value<'gc> {
        let mut obj = IndexMap::new();
        obj.insert("type".to_string(), Value::from(part_type));
        obj.insert("value".to_string(), Value::from(value));
        Value::Object(new_gc_cell_ptr(ctx, obj))
    }

    fn intl_number_format_format_value(
        &mut self,
        ctx: &GcContext<'gc>,
        formatter: &IndexMap<String, Value<'gc>>,
        value: Option<&Value<'gc>>,
    ) -> Result<String, Value<'gc>> {
        let options = IntlNumberFormatOptions::from_object(formatter);
        let input = self.intl_number_format_input(ctx, value)?;
        Ok(Self::intl_format_number_value(&options, input))
    }

    fn intl_number_format_input(
        &mut self,
        ctx: &GcContext<'gc>,
        value: Option<&Value<'gc>>,
    ) -> Result<IntlFormattedNumberInput, Value<'gc>> {
        let Some(value) = value else {
            return Ok(IntlFormattedNumberInput::Number(f64::NAN));
        };
        if let Value::BigInt(bi) = value {
            return Ok(IntlFormattedNumberInput::BigInt(bi.to_string()));
        }
        let prim = self.try_to_primitive(ctx, value, "number");
        if self.pending_throw.is_some() {
            return Err(self.pending_throw.clone().unwrap_or(Value::Undefined));
        }
        if let Value::BigInt(bi) = &prim {
            return Ok(IntlFormattedNumberInput::BigInt(bi.to_string()));
        }
        if let Value::String(text) = &prim {
            return Ok(IntlFormattedNumberInput::DecimalString(crate::unicode::utf16_to_utf8(text)));
        }
        Ok(IntlFormattedNumberInput::Number(to_number(&prim)))
    }

    fn intl_format_number_value(options: &IntlNumberFormatOptions, input: IntlFormattedNumberInput) -> String {
        let formatted = match input {
            IntlFormattedNumberInput::BigInt(value) => Self::intl_format_bigint_value(options, &value),
            IntlFormattedNumberInput::DecimalString(value) => Self::intl_format_decimal_string_or_number(options, &value),
            IntlFormattedNumberInput::Number(value) => Self::intl_format_f64_value(options, value),
        };
        Self::intl_remap_numbering_system_digits(&formatted, &options.numbering_system)
    }

    fn intl_format_bigint_value(options: &IntlNumberFormatOptions, value: &str) -> String {
        let (negative, mut digits) = if let Some(rest) = value.strip_prefix('-') {
            (true, rest.to_string())
        } else {
            (false, value.to_string())
        };
        if options.style == "percent" {
            digits.push_str("00");
        }
        if let Some(maximum_significant_digits) = options.maximum_significant_digits {
            digits = Self::intl_round_integer_digits(&digits, maximum_significant_digits as usize);
        }
        while digits.len() < options.minimum_integer_digits as usize {
            digits.insert(0, '0');
        }
        if options.notation == "compact"
            && let Some(formatted) = Self::intl_format_compact_integer(options, &digits)
        {
            return Self::intl_apply_sign_and_affixes(options, negative, false, false, &formatted);
        }
        let mut formatted = Self::intl_apply_grouping(&digits, options);
        if options.maximum_fraction_digits > 0 || options.minimum_fraction_digits > 0 {
            let fraction = "0".repeat(options.minimum_fraction_digits as usize);
            if !fraction.is_empty() {
                formatted.push(Self::intl_decimal_separator(options));
                formatted.push_str(&fraction);
            }
        }
        let is_zeroish = digits.chars().all(|ch| ch == '0');
        Self::intl_apply_sign_and_affixes(options, negative, is_zeroish, false, &formatted)
    }

    fn intl_format_f64_value(options: &IntlNumberFormatOptions, mut value: f64) -> String {
        if value.is_nan() {
            let nan = Self::intl_nan_string(options);
            let show_plus = options.sign_display == "always";
            return if show_plus { format!("+{}", nan) } else { nan };
        }
        if value.is_infinite() {
            return Self::intl_apply_sign_to_special(options, value.is_sign_negative(), false, "∞");
        }
        if options.style == "percent" {
            value *= 100.0;
        }
        let negative = value.is_sign_negative();
        value = value.abs();
        if matches!(options.notation.as_str(), "scientific" | "engineering") {
            let formatted = Self::intl_format_exponential(options, value);
            return Self::intl_apply_sign_and_affixes(options, negative, value == 0.0, false, &formatted);
        }
        if options.notation == "compact"
            && let Some(formatted) = Self::intl_format_compact_decimal(options, value)
        {
            let is_zeroish = formatted
                .trim_start_matches('0')
                .trim_matches(Self::intl_decimal_separator(options))
                .is_empty();
            return Self::intl_apply_sign_and_affixes(options, negative, is_zeroish, false, &formatted);
        }
        let (mut integer_digits, fraction_digits) = if let Some(maximum_significant_digits) = options.maximum_significant_digits {
            let rounded = Self::intl_round_to_significant_digits(value, maximum_significant_digits);
            Self::intl_format_standard_significant_digits(
                rounded,
                options.minimum_significant_digits.unwrap_or(1),
                options
                    .maximum_significant_digits
                    .unwrap_or(options.minimum_significant_digits.unwrap_or(1)),
            )
        } else {
            let rounded = Self::intl_round_to_fraction_digits(value, options.maximum_fraction_digits);
            Self::intl_split_fixed_decimal(rounded, options.maximum_fraction_digits, options.minimum_fraction_digits, false)
        };
        while integer_digits.len() < options.minimum_integer_digits as usize {
            integer_digits.insert(0, '0');
        }
        let mut formatted = Self::intl_apply_grouping(&integer_digits, options);
        if !fraction_digits.is_empty() {
            formatted.push(Self::intl_decimal_separator(options));
            formatted.push_str(&fraction_digits);
        }
        let is_zeroish = integer_digits.chars().all(|ch| ch == '0') && fraction_digits.chars().all(|ch| ch == '0');
        Self::intl_apply_sign_and_affixes(options, negative, is_zeroish, false, &formatted)
    }

    fn intl_format_decimal_string_or_number(options: &IntlNumberFormatOptions, value: &str) -> String {
        if let Some(decimal) = Self::intl_parse_exact_decimal(value)
            && options.minimum_significant_digits.is_none()
            && options.maximum_significant_digits.is_none()
            && options.notation == "standard"
        {
            return Self::intl_format_exact_decimal(options, &decimal);
        }
        Self::intl_format_f64_value(options, value.parse::<f64>().unwrap_or(f64::NAN))
    }

    fn intl_round_to_fraction_digits(value: f64, digits: u8) -> f64 {
        let factor = 10f64.powi(digits as i32);
        (value * factor).round() / factor
    }

    fn intl_round_to_significant_digits(value: f64, digits: u8) -> f64 {
        if value == 0.0 {
            return 0.0;
        }
        let integer_digits = value.log10().floor() as i32 + 1;
        let factor = 10f64.powi(digits as i32 - integer_digits);
        (value * factor).round() / factor
    }

    fn intl_format_standard_significant_digits(
        value: f64,
        minimum_significant_digits: u8,
        maximum_significant_digits: u8,
    ) -> (String, String) {
        let rounded = Self::intl_round_to_significant_digits(value, maximum_significant_digits);
        let mut text = rounded.to_string();
        let mut significant_digits = Self::intl_count_significant_digits(&text);
        while significant_digits < minimum_significant_digits as usize {
            if !text.contains('.') {
                text.push('.');
            }
            text.push('0');
            significant_digits += 1;
        }
        if let Some((integer, fraction)) = text.split_once('.') {
            (integer.to_string(), fraction.to_string())
        } else {
            (text, String::new())
        }
    }

    fn intl_count_significant_digits(text: &str) -> usize {
        let digits: String = text.chars().filter(|ch| ch.is_ascii_digit()).collect();
        let trimmed = digits.trim_start_matches('0');
        if trimmed.is_empty() { 1 } else { trimmed.len() }
    }

    fn intl_split_fixed_decimal(
        value: f64,
        max_fraction_digits: u8,
        min_fraction_digits: u8,
        keep_all_significant_digits: bool,
    ) -> (String, String) {
        let text = if keep_all_significant_digits {
            value.to_string()
        } else {
            format!("{:.*}", max_fraction_digits as usize, value)
        };
        if let Some((integer, fraction)) = text.split_once('.') {
            let mut fraction = fraction.to_string();
            if !keep_all_significant_digits {
                while fraction.len() > min_fraction_digits as usize && fraction.ends_with('0') {
                    fraction.pop();
                }
            }
            while fraction.len() < min_fraction_digits as usize {
                fraction.push('0');
            }
            (integer.to_string(), fraction)
        } else {
            let mut fraction = String::new();
            while fraction.len() < min_fraction_digits as usize {
                fraction.push('0');
            }
            (text, fraction)
        }
    }

    fn intl_round_integer_digits(value: &str, max_digits: usize) -> String {
        if value.len() <= max_digits {
            return value.to_string();
        }
        let mut prefix: Vec<u8> = value.as_bytes()[..max_digits].to_vec();
        if value.as_bytes()[max_digits] >= b'5' {
            for digit in prefix.iter_mut().rev() {
                if *digit == b'9' {
                    *digit = b'0';
                } else {
                    *digit += 1;
                    break;
                }
            }
            if prefix[0] == b'0' {
                prefix.insert(0, b'1');
            }
        }
        let mut rounded = String::from_utf8(prefix).unwrap_or_else(|_| value[..max_digits].to_string());
        rounded.push_str(&"0".repeat(value.len().saturating_sub(max_digits)));
        rounded
    }

    fn intl_apply_grouping(integer_digits: &str, options: &IntlNumberFormatOptions) -> String {
        if !options.use_grouping {
            return integer_digits.to_string();
        }
        let groups = if options.resolved_locale.starts_with("en-IN") {
            vec![3usize, 2usize]
        } else {
            vec![3usize]
        };
        let separator = Self::intl_group_separator(options);
        let mut parts = Vec::new();
        let mut remaining = integer_digits;
        if groups.len() == 1 {
            while remaining.len() > 3 {
                let split = remaining.len() - 3;
                parts.push(remaining[split..].to_string());
                remaining = &remaining[..split];
            }
            parts.push(remaining.to_string());
            parts.reverse();
            return parts.join(&separator.to_string());
        }
        if remaining.len() > 3 {
            let split = remaining.len() - 3;
            parts.push(remaining[split..].to_string());
            remaining = &remaining[..split];
            while remaining.len() > 2 {
                let split = remaining.len() - 2;
                parts.push(remaining[split..].to_string());
                remaining = &remaining[..split];
            }
        }
        if !remaining.is_empty() {
            parts.push(remaining.to_string());
        }
        parts.reverse();
        parts.join(&separator.to_string())
    }

    fn intl_decimal_separator(options: &IntlNumberFormatOptions) -> char {
        if options.resolved_locale.starts_with("de") { ',' } else { '.' }
    }

    fn intl_group_separator(options: &IntlNumberFormatOptions) -> char {
        if options.resolved_locale.starts_with("de") { '.' } else { ',' }
    }

    fn intl_apply_sign_to_special(options: &IntlNumberFormatOptions, negative: bool, is_zeroish: bool, text: &str) -> String {
        let (prefix, suffix) = Self::intl_sign_parts(options, negative, is_zeroish, false);
        format!("{}{}{}", prefix, text, suffix)
    }

    fn intl_apply_sign_and_affixes(
        options: &IntlNumberFormatOptions,
        negative: bool,
        is_zeroish: bool,
        is_nan: bool,
        core: &str,
    ) -> String {
        let mut out = String::new();
        let (prefix, suffix) = Self::intl_sign_parts(options, negative, is_zeroish, is_nan);
        if options.style == "unit"
            && let Some(pattern) = Self::intl_unit_pattern(options)
        {
            if let Some(prefix_unit) = pattern.prefix_unit {
                out.push_str(&prefix_unit);
            }
            if let Some(prefix_literal) = pattern.prefix_literal {
                out.push_str(&prefix_literal);
            }
            out.push_str(&prefix);
            out.push_str(core);
            out.push_str(&suffix);
            if let Some(suffix_literal) = pattern.suffix_literal {
                out.push_str(&suffix_literal);
            }
            out.push_str(&pattern.suffix_unit);
            return out;
        }
        out.push_str(&prefix);
        if options.style == "currency"
            && let Some(currency) = &options.currency
        {
            let symbol = Self::intl_currency_symbol(options, currency);
            if options.resolved_locale.starts_with("de") {
                out.push_str(core);
                out.push('\u{a0}');
                out.push_str(symbol);
                out.push_str(&suffix);
                return out;
            }
            out.push_str(symbol);
        }
        out.push_str(core);
        if options.style == "percent" {
            if options.resolved_locale.starts_with("de") {
                out.push('\u{a0}');
            }
            out.push('%');
        }
        out.push_str(&suffix);
        out
    }

    fn intl_sign_parts(options: &IntlNumberFormatOptions, negative: bool, is_zeroish: bool, is_nan: bool) -> (String, String) {
        let show_negative = negative
            && match options.sign_display.as_str() {
                "never" => false,
                "exceptZero" | "negative" => !is_zeroish,
                _ => true,
            };
        let show_positive = !negative
            && !is_nan
            && match options.sign_display.as_str() {
                "always" => true,
                "exceptZero" => !is_zeroish,
                _ => false,
            };
        if show_negative
            && options.style == "currency"
            && options.currency_sign == "accounting"
            && !options.resolved_locale.starts_with("de")
        {
            return ("(".to_string(), ")".to_string());
        }
        let prefix = if show_negative {
            "-".to_string()
        } else if show_positive {
            "+".to_string()
        } else {
            String::new()
        };
        (prefix, String::new())
    }

    fn intl_currency_symbol<'a>(options: &IntlNumberFormatOptions, currency: &'a str) -> &'a str {
        if options.currency_display == "code" || options.currency_display == "name" {
            return currency;
        }
        if currency != "USD" {
            return currency;
        }
        if matches!(
            options.resolved_locale.as_str(),
            locale if locale.starts_with("ko") || locale.starts_with("zh-TW")
        ) {
            "US$"
        } else {
            "$"
        }
    }

    fn intl_nan_string(options: &IntlNumberFormatOptions) -> String {
        if options.resolved_locale.starts_with("zh-TW") {
            "非數值".to_string()
        } else {
            "NaN".to_string()
        }
    }

    fn intl_unit_pattern(options: &IntlNumberFormatOptions) -> Option<IntlUnitPattern> {
        let unit = options.unit.as_deref()?;
        if unit == "kilometer-per-hour" {
            let display = options.unit_display.as_str();
            let locale = options.resolved_locale.as_str();
            let pattern = if locale.starts_with("de") {
                IntlUnitPattern {
                    prefix_unit: None,
                    prefix_literal: None,
                    suffix_literal: Some(" ".to_string()),
                    suffix_unit: if display == "long" {
                        "Kilometer pro Stunde".to_string()
                    } else {
                        "km/h".to_string()
                    },
                }
            } else if locale.starts_with("ja") {
                match display {
                    "narrow" => IntlUnitPattern {
                        prefix_unit: None,
                        prefix_literal: None,
                        suffix_literal: None,
                        suffix_unit: "km/h".to_string(),
                    },
                    "long" => IntlUnitPattern {
                        prefix_unit: Some("時速".to_string()),
                        prefix_literal: Some(" ".to_string()),
                        suffix_literal: Some(" ".to_string()),
                        suffix_unit: "キロメートル".to_string(),
                    },
                    _ => IntlUnitPattern {
                        prefix_unit: None,
                        prefix_literal: None,
                        suffix_literal: Some(" ".to_string()),
                        suffix_unit: "km/h".to_string(),
                    },
                }
            } else if locale.starts_with("ko") {
                match display {
                    "long" => IntlUnitPattern {
                        prefix_unit: Some("시속".to_string()),
                        prefix_literal: Some(" ".to_string()),
                        suffix_literal: None,
                        suffix_unit: "킬로미터".to_string(),
                    },
                    _ => IntlUnitPattern {
                        prefix_unit: None,
                        prefix_literal: None,
                        suffix_literal: None,
                        suffix_unit: "km/h".to_string(),
                    },
                }
            } else if locale.starts_with("zh-TW") {
                match display {
                    "narrow" => IntlUnitPattern {
                        prefix_unit: None,
                        prefix_literal: None,
                        suffix_literal: None,
                        suffix_unit: "公里/小時".to_string(),
                    },
                    "long" => IntlUnitPattern {
                        prefix_unit: Some("每小時".to_string()),
                        prefix_literal: Some(" ".to_string()),
                        suffix_literal: Some(" ".to_string()),
                        suffix_unit: "公里".to_string(),
                    },
                    _ => IntlUnitPattern {
                        prefix_unit: None,
                        prefix_literal: None,
                        suffix_literal: Some(" ".to_string()),
                        suffix_unit: "公里/小時".to_string(),
                    },
                }
            } else {
                match display {
                    "narrow" => IntlUnitPattern {
                        prefix_unit: None,
                        prefix_literal: None,
                        suffix_literal: None,
                        suffix_unit: "km/h".to_string(),
                    },
                    "long" => IntlUnitPattern {
                        prefix_unit: None,
                        prefix_literal: None,
                        suffix_literal: Some(" ".to_string()),
                        suffix_unit: "kilometers per hour".to_string(),
                    },
                    _ => IntlUnitPattern {
                        prefix_unit: None,
                        prefix_literal: None,
                        suffix_literal: Some(" ".to_string()),
                        suffix_unit: "km/h".to_string(),
                    },
                }
            };
            return Some(pattern);
        }
        let suffix_unit = if options.unit_display == "narrow" {
            unit.to_string()
        } else {
            unit.replace("-per-", "/")
        };
        Some(IntlUnitPattern {
            prefix_unit: None,
            prefix_literal: None,
            suffix_literal: Some(" ".to_string()),
            suffix_unit,
        })
    }

    fn intl_format_compact_integer(options: &IntlNumberFormatOptions, digits: &str) -> Option<String> {
        let value = digits.parse::<f64>().ok()?;
        Self::intl_format_compact_decimal(options, value)
    }

    fn intl_format_compact_decimal(options: &IntlNumberFormatOptions, value: f64) -> Option<String> {
        let locale = options.resolved_locale.as_str();
        let compact_display = options.compact_display.as_deref().unwrap_or("short");
        let (divisor, suffix) = if locale.starts_with("en") {
            #[allow(clippy::if_same_then_else)]
            if value >= 1_000_000_000.0 {
                (1_000_000.0, if compact_display == "long" { " million" } else { "M" })
            } else if value >= 1_000_000.0 {
                (1_000_000.0, if compact_display == "long" { " million" } else { "M" })
            } else if value >= 1_000.0 {
                (1_000.0, if compact_display == "long" { " thousand" } else { "K" })
            } else {
                return Some(Self::intl_format_compact_plain(options, value, false));
            }
        } else if locale.starts_with("de") {
            if value >= 1_000_000.0 {
                (1_000_000.0, if compact_display == "long" { " Millionen" } else { "\u{a0}Mio." })
            } else if compact_display == "long" && value >= 1_000.0 {
                (1_000.0, " Tausend")
            } else {
                return Some(Self::intl_format_compact_plain(options, value, value >= 10_000.0));
            }
        } else if locale.starts_with("ja") {
            if value >= 100_000_000.0 {
                (100_000_000.0, "億")
            } else if value >= 10_000.0 {
                (10_000.0, "万")
            } else {
                return Some(Self::intl_format_compact_plain(options, value, false));
            }
        } else if locale.starts_with("ko") {
            if value >= 100_000_000.0 {
                (100_000_000.0, "억")
            } else if value >= 10_000.0 {
                (10_000.0, "만")
            } else if value >= 1_000.0 {
                (1_000.0, "천")
            } else {
                return Some(Self::intl_format_compact_plain(options, value, false));
            }
        } else if locale.starts_with("zh-TW") {
            if value >= 100_000_000.0 {
                (100_000_000.0, "億")
            } else if value >= 10_000.0 {
                (10_000.0, "萬")
            } else {
                return Some(Self::intl_format_compact_plain(options, value, false));
            }
        } else {
            return None;
        };
        let scaled = value / divisor;
        let fraction_digits = if scaled < 10.0 { 1 } else { 0 };
        let rounded = Self::intl_round_to_fraction_digits(scaled, fraction_digits);
        let (integer_digits, fraction_digits) = Self::intl_split_fixed_decimal(rounded, fraction_digits, 0, false);
        let mut formatted = integer_digits;
        if !fraction_digits.is_empty() {
            formatted.push(Self::intl_decimal_separator(options));
            formatted.push_str(&fraction_digits);
        }
        formatted.push_str(suffix);
        Some(formatted)
    }

    fn intl_format_compact_plain(options: &IntlNumberFormatOptions, value: f64, use_grouping: bool) -> String {
        let max_fraction_digits = if value >= 10.0 {
            0
        } else if value >= 1.0 {
            1
        } else if value >= 0.1 {
            2
        } else if value >= 0.01 {
            3
        } else {
            4
        };
        let rounded = Self::intl_round_to_fraction_digits(value, max_fraction_digits);
        let (integer_digits, fraction_digits) = Self::intl_split_fixed_decimal(rounded, max_fraction_digits, 0, false);
        let mut formatted = if use_grouping {
            Self::intl_apply_grouping(&integer_digits, options)
        } else {
            integer_digits
        };
        if !fraction_digits.is_empty() {
            formatted.push(Self::intl_decimal_separator(options));
            formatted.push_str(&fraction_digits);
        }
        formatted
    }

    fn intl_format_exponential(options: &IntlNumberFormatOptions, value: f64) -> String {
        if value == 0.0 {
            return "0E0".to_string();
        }
        let scientific_exponent = value.log10().floor() as i32;
        let mut exponent = if options.notation == "engineering" {
            scientific_exponent.div_euclid(3) * 3
        } else {
            scientific_exponent
        };
        let mut mantissa = value / 10f64.powi(exponent);
        mantissa = Self::intl_round_to_fraction_digits(mantissa, 3);
        let rollover = if options.notation == "engineering" { 1000.0 } else { 10.0 };
        if mantissa >= rollover {
            mantissa /= if options.notation == "engineering" { 1000.0 } else { 10.0 };
            exponent += if options.notation == "engineering" { 3 } else { 1 };
        }
        let (integer_digits, fraction_digits) = Self::intl_split_fixed_decimal(mantissa, 3, 0, false);
        let mut formatted = integer_digits;
        if !fraction_digits.is_empty() {
            formatted.push(Self::intl_decimal_separator(options));
            formatted.push_str(&fraction_digits);
        }
        formatted.push('E');
        formatted.push_str(&exponent.to_string());
        formatted
    }

    fn intl_parse_exact_decimal(text: &str) -> Option<IntlExactDecimal> {
        let text = text.trim();
        if text.is_empty() || text.contains(['e', 'E']) {
            return None;
        }
        let (negative, rest) = if let Some(rest) = text.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = text.strip_prefix('+') {
            (false, rest)
        } else {
            (false, text)
        };
        let (integer_part, fraction_part) = if let Some((integer_part, fraction_part)) = rest.split_once('.') {
            (integer_part, fraction_part)
        } else {
            (rest, "")
        };
        if (!integer_part.is_empty() && !integer_part.chars().all(|ch| ch.is_ascii_digit()))
            || !fraction_part.chars().all(|ch| ch.is_ascii_digit())
        {
            return None;
        }
        let digits = format!("{}{}", integer_part, fraction_part);
        if digits.is_empty() || !digits.chars().any(|ch| ch.is_ascii_digit()) {
            return None;
        }
        Some(IntlExactDecimal {
            negative,
            digits,
            scale: fraction_part.len(),
        })
    }

    fn intl_format_exact_decimal(options: &IntlNumberFormatOptions, decimal: &IntlExactDecimal) -> String {
        let mut digits = decimal.digits.clone();
        let mut scale = decimal.scale;
        if options.style == "percent" {
            if scale >= 2 {
                scale -= 2;
            } else {
                digits.push_str(&"0".repeat(2 - scale));
                scale = 0;
            }
        }
        Self::intl_round_exact_fraction_digits(&mut digits, &mut scale, options.maximum_fraction_digits as usize);
        let split = digits.len().saturating_sub(scale);
        let integer_part_raw = if split == 0 { "0".to_string() } else { digits[..split].to_string() };
        let mut integer_part = integer_part_raw.trim_start_matches('0').to_string();
        if integer_part.is_empty() {
            integer_part = "0".to_string();
        }
        while integer_part.len() < options.minimum_integer_digits as usize {
            integer_part.insert(0, '0');
        }
        let mut fraction_part = if scale == 0 { String::new() } else { digits[split..].to_string() };
        while fraction_part.len() > options.minimum_fraction_digits as usize && fraction_part.ends_with('0') {
            fraction_part.pop();
        }
        while fraction_part.len() < options.minimum_fraction_digits as usize {
            fraction_part.push('0');
        }
        let mut formatted = Self::intl_apply_grouping(&integer_part, options);
        if !fraction_part.is_empty() {
            formatted.push(Self::intl_decimal_separator(options));
            formatted.push_str(&fraction_part);
        }
        let is_zeroish = integer_part.chars().all(|ch| ch == '0') && fraction_part.chars().all(|ch| ch == '0');
        Self::intl_apply_sign_and_affixes(options, decimal.negative, is_zeroish, false, &formatted)
    }

    fn intl_round_exact_fraction_digits(digits: &mut String, scale: &mut usize, maximum_fraction_digits: usize) {
        if *scale <= maximum_fraction_digits {
            return;
        }
        let cut = digits.len() - *scale + maximum_fraction_digits;
        let round_up = digits.as_bytes().get(cut).is_some_and(|digit| *digit >= b'5');
        digits.truncate(cut);
        *scale = maximum_fraction_digits;
        if round_up {
            let mut bytes = digits.as_bytes().to_vec();
            let mut index = bytes.len();
            loop {
                if index == 0 {
                    bytes.insert(0, b'1');
                    break;
                }
                index -= 1;
                if bytes[index] == b'9' {
                    bytes[index] = b'0';
                } else {
                    bytes[index] += 1;
                    break;
                }
            }
            *digits = String::from_utf8(bytes).unwrap_or_else(|_| digits.clone());
        }
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

    fn intl_date_time_format_clip_value(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>, required: bool) -> Option<i64> {
        let x = if value.is_none() || matches!(value, Some(Value::Undefined)) {
            if required {
                self.pending_throw = Some(self.make_range_error_object(ctx, "Invalid time value"));
                return None;
            }
            chrono::Utc::now().timestamp_millis() as f64
        } else {
            let prim = self.try_to_primitive(ctx, value.unwrap_or(&Value::Undefined), "number");
            if self.pending_throw.is_some() {
                return None;
            }
            to_number(&prim)
        };
        Self::intl_time_clip(x).or_else(|| {
            self.pending_throw = Some(self.make_range_error_object(ctx, "Invalid time value"));
            None
        })
    }

    fn intl_time_clip(value: f64) -> Option<i64> {
        if !value.is_finite() || value.abs() > 8.64e15 {
            return None;
        }
        let clipped = value.trunc();
        let clipped = if clipped == 0.0 { 0.0 } else { clipped };
        Some(clipped as i64)
    }

    fn intl_date_time_format_parts_array(
        ctx: &GcContext<'gc>,
        formatter: &IndexMap<String, Value<'gc>>,
        millis: i64,
        source: Option<&str>,
    ) -> Result<Value<'gc>, Value<'gc>> {
        let values = Self::intl_date_time_format_parts(formatter, millis)?
            .into_iter()
            .map(|(part_type, value)| Self::intl_date_time_format_part_object(ctx, &part_type, &value, source))
            .collect();
        Ok(Value::Array(new_gc_cell_ptr(ctx, VmArrayData::new(values))))
    }

    fn intl_date_time_format_part_object(ctx: &GcContext<'gc>, part_type: &str, value: &str, source: Option<&str>) -> Value<'gc> {
        let mut map = IndexMap::new();
        map.insert("type".to_string(), Value::from(part_type));
        map.insert("value".to_string(), Value::from(value));
        if let Some(source) = source {
            map.insert("source".to_string(), Value::from(source));
        }
        Value::Object(new_gc_cell_ptr(ctx, map))
    }

    fn intl_date_time_format_render(formatter: &IndexMap<String, Value<'gc>>, millis: i64) -> Result<String, Value<'gc>> {
        Ok(Self::intl_date_time_format_parts(formatter, millis)?
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>()
            .join(""))
    }

    fn intl_date_time_format_parts(formatter: &IndexMap<String, Value<'gc>>, millis: i64) -> Result<Vec<(String, String)>, Value<'gc>> {
        use chrono::{Datelike, FixedOffset, TimeZone, Timelike};

        let numbering_system = formatter
            .get("__intl_numbering_system__")
            .map(value_to_string)
            .unwrap_or_else(|| "latn".to_string());
        let time_zone = formatter
            .get("__intl_time_zone__")
            .map(value_to_string)
            .unwrap_or_else(|| "UTC".to_string());
        let calendar = formatter
            .get("__intl_calendar__")
            .map(value_to_string)
            .unwrap_or_else(|| "gregory".to_string());

        let numeric = |value: i64, width: usize| Self::intl_format_numbering_digits(value, width, &numbering_system);
        let offset = if time_zone == "UTC" {
            FixedOffset::east_opt(0)
        } else {
            Self::intl_fixed_offset_from_zone(&time_zone)
        }
        .unwrap_or_else(|| FixedOffset::east_opt(0).expect("zero UTC offset"));
        let date_time = offset
            .timestamp_millis_opt(millis)
            .single()
            .ok_or_else(|| Value::from("Invalid time value"))?;

        let mut parts = Vec::new();
        let weekday = formatter.get("__intl_weekday__").map(value_to_string);
        let era = formatter.get("__intl_era__").map(value_to_string);
        let year = formatter.get("__intl_year__").map(value_to_string);
        let month = formatter.get("__intl_month__").map(value_to_string);
        let day = formatter.get("__intl_day__").map(value_to_string);
        let hour = formatter.get("__intl_hour__").map(value_to_string);
        let minute = formatter.get("__intl_minute__").map(value_to_string);
        let second = formatter.get("__intl_second__").map(value_to_string);
        let time_zone_name = formatter.get("__intl_timeZoneName__").map(value_to_string);
        let numeric_month_style = matches!(month.as_deref(), Some("2-digit" | "numeric"));

        if let Some(style) = weekday {
            let names = match style.as_str() {
                "narrow" => ["S", "M", "T", "W", "T", "F", "S"],
                "short" => ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
                _ => ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"],
            };
            parts.push((
                "weekday".to_string(),
                names[date_time.weekday().num_days_from_sunday() as usize].to_string(),
            ));
            if month.is_some() || day.is_some() || year.is_some() {
                parts.push(("literal".to_string(), ", ".to_string()));
            }
        }

        if let Some(month_style) = month {
            let month_index = date_time.month0() as usize;
            let month_text = match month_style.as_str() {
                "2-digit" => numeric((month_index + 1) as i64, 2),
                "numeric" => numeric((month_index + 1) as i64, 1),
                "narrow" => ["J", "F", "M", "A", "M", "J", "J", "A", "S", "O", "N", "D"][month_index].to_string(),
                "short" => ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][month_index].to_string(),
                _ => [
                    "January",
                    "February",
                    "March",
                    "April",
                    "May",
                    "June",
                    "July",
                    "August",
                    "September",
                    "October",
                    "November",
                    "December",
                ][month_index]
                    .to_string(),
            };
            parts.push(("month".to_string(), month_text));
            if day.is_some() {
                parts.push((
                    "literal".to_string(),
                    if month_style == "2-digit" || month_style == "numeric" {
                        "/".to_string()
                    } else {
                        " ".to_string()
                    },
                ));
            }
        }

        if let Some(day_style) = day {
            let width = usize::from(day_style == "2-digit") + 1;
            parts.push(("day".to_string(), numeric(date_time.day() as i64, width)));
            if year.is_some() {
                parts.push((
                    "literal".to_string(),
                    if numeric_month_style { "/".to_string() } else { ", ".to_string() },
                ));
            }
        }

        if let Some(year_style) = year {
            let mut year_value = date_time.year();
            if calendar == "buddhist" {
                year_value += 543;
            }
            let year_text = if year_style == "2-digit" {
                numeric((year_value.rem_euclid(100)) as i64, 2)
            } else {
                numeric(year_value as i64, 1)
            };
            parts.push(("year".to_string(), year_text));
        }

        if let Some(era_style) = era {
            let text = if date_time.year() <= 0 {
                match era_style.as_str() {
                    "narrow" => "B",
                    "short" => "BC",
                    _ => "Before Christ",
                }
            } else {
                match era_style.as_str() {
                    "narrow" => "A",
                    "short" => "AD",
                    _ => "Anno Domini",
                }
            };
            if !parts.is_empty() {
                parts.push(("literal".to_string(), " ".to_string()));
            }
            parts.push(("era".to_string(), text.to_string()));
        }

        if hour.is_some() || minute.is_some() || second.is_some() {
            if !parts.is_empty() {
                parts.push(("literal".to_string(), " ".to_string()));
            }
            let hour_cycle = formatter.get("__intl_hour_cycle__").map(value_to_string);
            let hour12 = matches!(formatter.get("__intl_hour12__"), Some(Value::Boolean(true)));
            let mut hour_value = date_time.hour() as i64;
            if hour12 || matches!(hour_cycle.as_deref(), Some("h11" | "h12")) {
                hour_value %= 12;
                if matches!(hour_cycle.as_deref(), Some("h12")) && hour_value == 0 {
                    hour_value = 12;
                }
            }
            if let Some(hour_style) = hour {
                let width = usize::from(hour_style == "2-digit") + 1;
                parts.push(("hour".to_string(), numeric(hour_value, width)));
            }
            if let Some(minute_style) = minute {
                if formatter.get("__intl_hour__").is_some() {
                    parts.push(("literal".to_string(), ":".to_string()));
                }
                let width = usize::from(minute_style == "2-digit") + 1;
                parts.push(("minute".to_string(), numeric(date_time.minute() as i64, width)));
            }
            if let Some(second_style) = second {
                if formatter.get("__intl_minute__").is_some() {
                    parts.push(("literal".to_string(), ":".to_string()));
                }
                let width = usize::from(second_style == "2-digit") + 1;
                parts.push(("second".to_string(), numeric(date_time.second() as i64, width)));
            }
            if hour12 {
                parts.push(("literal".to_string(), " ".to_string()));
                parts.push((
                    "dayPeriod".to_string(),
                    if date_time.hour() < 12 {
                        "AM".to_string()
                    } else {
                        "PM".to_string()
                    },
                ));
            }
        }

        if let Some(style) = time_zone_name {
            if !parts.is_empty() {
                parts.push(("literal".to_string(), " ".to_string()));
            }
            let text = match style.as_str() {
                "long" | "short" if time_zone == "UTC" => "UTC".to_string(),
                "shortOffset" | "longOffset" => time_zone.clone(),
                _ => time_zone.clone(),
            };
            parts.push(("timeZoneName".to_string(), text));
        }

        if parts.is_empty() {
            parts.push(("literal".to_string(), numeric(date_time.month() as i64, 1)));
            parts.push(("literal".to_string(), "/".to_string()));
            parts.push(("literal".to_string(), numeric(date_time.day() as i64, 1)));
            parts.push(("literal".to_string(), "/".to_string()));
            parts.push(("literal".to_string(), numeric(date_time.year() as i64, 1)));
        }

        Ok(parts)
    }

    fn intl_fixed_offset_from_zone(value: &str) -> Option<chrono::FixedOffset> {
        if value.len() != 6 || !matches!(value.as_bytes()[0], b'+' | b'-') || value.as_bytes()[3] != b':' {
            return None;
        }
        let sign = if value.as_bytes()[0] == b'-' { -1 } else { 1 };
        let hour: i32 = value[1..3].parse().ok()?;
        let minute: i32 = value[4..6].parse().ok()?;
        chrono::FixedOffset::east_opt(sign * (hour * 3600 + minute * 60))
    }

    fn intl_format_numbering_digits(value: i64, min_width: usize, numbering_system: &str) -> String {
        let mut text = if min_width > 1 {
            format!("{:0width$}", value, width = min_width)
        } else {
            value.to_string()
        };
        text = Self::intl_remap_numbering_system_digits(&text, numbering_system);
        text
    }

    fn intl_remap_numbering_system_digits(text: &str, numbering_system: &str) -> String {
        let hanidec_digits = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
        if numbering_system == "hanidec" {
            return text
                .chars()
                .map(|ch| match ch {
                    '0'..='9' => hanidec_digits[ch as usize - '0' as usize],
                    _ => ch,
                })
                .collect();
        }
        let Some(zero) = Self::intl_numbering_system_zero(numbering_system) else {
            return text.to_string();
        };
        text.chars()
            .map(|ch| match ch {
                '0'..='9' => char::from_u32(zero + (ch as u32 - '0' as u32)).unwrap_or(ch),
                _ => ch,
            })
            .collect()
    }

    fn intl_numbering_system_zero(numbering_system: &str) -> Option<u32> {
        match numbering_system {
            "adlm" => Some(0x1E950),
            "ahom" => Some(0x11730),
            "arab" => Some(0x0660),
            "arabext" => Some(0x06F0),
            "bali" => Some(0x1B50),
            "beng" => Some(0x09E6),
            "bhks" => Some(0x11C50),
            "brah" => Some(0x11066),
            "cakm" => Some(0x11136),
            "cham" => Some(0xAA50),
            "deva" => Some(0x0966),
            "diak" => Some(0x11950),
            "fullwide" => Some(0xFF10),
            "gara" => Some(0x10D40),
            "gong" => Some(0x11DA0),
            "gonm" => Some(0x11D50),
            "gujr" => Some(0x0AE6),
            "gukh" => Some(0x16130),
            "guru" => Some(0x0A66),
            "hmng" => Some(0x16B50),
            "hmnp" => Some(0x1E140),
            "java" => Some(0xA9D0),
            "kali" => Some(0xA900),
            "kawi" => Some(0x11F50),
            "khmr" => Some(0x17E0),
            "knda" => Some(0x0CE6),
            "krai" => Some(0x16D70),
            "lana" => Some(0x1A80),
            "lanatham" => Some(0x1A90),
            "laoo" => Some(0x0ED0),
            "latn" => Some(0x0030),
            "lepc" => Some(0x1C40),
            "limb" => Some(0x1946),
            "mathbold" => Some(0x1D7CE),
            "mathdbl" => Some(0x1D7D8),
            "mathmono" => Some(0x1D7F6),
            "mathsanb" => Some(0x1D7EC),
            "mathsans" => Some(0x1D7E2),
            "mlym" => Some(0x0D66),
            "modi" => Some(0x11650),
            "mong" => Some(0x1810),
            "mroo" => Some(0x16A60),
            "mtei" => Some(0xABF0),
            "mymr" => Some(0x1040),
            "mymrepka" => Some(0x116DA),
            "mymrpao" => Some(0x116D0),
            "mymrshan" => Some(0x1090),
            "mymrtlng" => Some(0xA9F0),
            "nagm" => Some(0x1E4F0),
            "newa" => Some(0x11450),
            "nkoo" => Some(0x07C0),
            "olck" => Some(0x1C50),
            "onao" => Some(0x1E5F1),
            "orya" => Some(0x0B66),
            "osma" => Some(0x104A0),
            "outlined" => Some(0x1CCF0),
            "rohg" => Some(0x10D30),
            "saur" => Some(0xA8D0),
            "segment" => Some(0x1FBF0),
            "shrd" => Some(0x111D0),
            "sind" => Some(0x112F0),
            "sinh" => Some(0x0DE6),
            "sora" => Some(0x110F0),
            "sund" => Some(0x1BB0),
            "sunu" => Some(0x11BF0),
            "takr" => Some(0x116C0),
            "talu" => Some(0x19D0),
            "tamldec" => Some(0x0BE6),
            "telu" => Some(0x0C66),
            "thai" => Some(0x0E50),
            "tibt" => Some(0x0F20),
            "tirh" => Some(0x114D0),
            "tnsa" => Some(0x16AC0),
            "tols" => Some(0x11DE0),
            "vaii" => Some(0xA620),
            "wara" => Some(0x118E0),
            "wcho" => Some(0x1E2F0),
            _ => None,
        }
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
        let ext_numbering_system = locale_info
            .unicode_keywords
            .get("nu")
            .and_then(|value| Self::intl_supported_numbering_system(value));
        let mut out = IntlNumberFormatOptions {
            resolved_locale: locale_base.clone(),
            numbering_system: ext_numbering_system.clone().unwrap_or_else(|| "latn".to_string()),
            style: "decimal".to_string(),
            currency: None,
            currency_display: "symbol".to_string(),
            unit: None,
            unit_display: "short".to_string(),
            notation: "standard".to_string(),
            compact_display: None,
            currency_sign: "standard".to_string(),
            sign_display: "auto".to_string(),
            rounding_increment: 1,
            trailing_zero_display: "auto".to_string(),
            use_grouping: true,
            minimum_integer_digits: 1,
            minimum_fraction_digits: 0,
            maximum_fraction_digits: 3,
            minimum_significant_digits: None,
            maximum_significant_digits: None,
        };
        let Some(options) = options else {
            out.resolved_locale = Self::intl_numbering_system_resolved_locale(&locale_base, ext_numbering_system.as_deref());
            return Ok(out);
        };
        if matches!(options, Value::Undefined) {
            out.resolved_locale = Self::intl_numbering_system_resolved_locale(&locale_base, ext_numbering_system.as_deref());
            return Ok(out);
        }
        if matches!(options, Value::Null) {
            return Err(self.make_type_error_object(ctx, "options must not be null"));
        }
        if !Self::intl_is_object_like(options) {
            out.resolved_locale = Self::intl_numbering_system_resolved_locale(&locale_base, ext_numbering_system.as_deref());
            return Ok(out);
        }
        let raw_options = self.intl_read_number_format_raw_options(ctx, options)?;
        if let Some(locale_matcher) =
            self.intl_string_option_from_value(ctx, raw_options.get("localeMatcher"), &["lookup", "best fit"], None)?
        {
            let _ = locale_matcher;
        }
        let mut option_numbering_system = None;
        if let Some(numbering_system) = self.intl_string_option_from_value(ctx, raw_options.get("numberingSystem"), &[], None)? {
            if !Self::intl_is_valid_unicode_type_identifier(&numbering_system) {
                return Err(self.make_range_error_object(ctx, "Invalid numberingSystem"));
            }
            if let Some(numbering_system) = Self::intl_supported_numbering_system(&numbering_system) {
                out.numbering_system = numbering_system;
                option_numbering_system = Some(out.numbering_system.clone());
            }
        }
        if let Some(style) = self.intl_string_option_from_value(
            ctx,
            raw_options.get("style"),
            &["decimal", "percent", "currency", "unit"],
            Some("decimal"),
        )? {
            out.style = style;
        }
        let currency = self.intl_string_option_from_value(ctx, raw_options.get("currency"), &[], None)?;
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
        if let Some(currency_display) = self.intl_string_option_from_value(
            ctx,
            raw_options.get("currencyDisplay"),
            &["code", "symbol", "narrowSymbol", "name"],
            Some("symbol"),
        )? && out.style == "currency"
        {
            out.currency_display = currency_display;
        }
        if let Some(unit) = self.intl_string_option_from_value(ctx, raw_options.get("unit"), &[], None)? {
            if !Self::intl_is_well_formed_unit_identifier(&unit) {
                return Err(self.make_range_error_object(ctx, "Invalid unit identifier"));
            }
            if out.style == "unit" {
                out.unit = Some(unit);
            }
        }
        if let Some(unit_display) =
            self.intl_string_option_from_value(ctx, raw_options.get("unitDisplay"), &["short", "narrow", "long"], Some("short"))?
            && out.style == "unit"
        {
            out.unit_display = unit_display;
        }
        if out.style == "unit" && out.unit.is_none() {
            return Err(self.make_type_error_object(ctx, "unit style requires unit"));
        }
        let notation = self.intl_string_option_from_value(
            ctx,
            raw_options.get("notation"),
            &["standard", "scientific", "engineering", "compact"],
            Some("standard"),
        )?;
        out.notation = notation.clone().unwrap_or_else(|| "standard".to_string());
        if out.style == "currency"
            && let Some(currency_sign) =
                self.intl_string_option_from_value(ctx, raw_options.get("currencySign"), &["standard", "accounting"], Some("standard"))?
        {
            out.currency_sign = currency_sign;
        }
        if let Some(sign_display) = self.intl_string_option_from_value(
            ctx,
            raw_options.get("signDisplay"),
            &["auto", "never", "always", "exceptZero", "negative"],
            Some("auto"),
        )? {
            out.sign_display = sign_display;
        }
        if out.notation == "compact" {
            out.compact_display =
                self.intl_string_option_from_value(ctx, raw_options.get("compactDisplay"), &["short", "long"], Some("short"))?;
        }
        if let Some(rounding_increment) = self.intl_rounding_increment_option(ctx, raw_options.get("roundingIncrement"))? {
            out.rounding_increment = rounding_increment;
        }
        let _ = self.intl_string_option_from_value(
            ctx,
            raw_options.get("roundingMode"),
            &[
                "ceil",
                "floor",
                "expand",
                "trunc",
                "halfCeil",
                "halfFloor",
                "halfExpand",
                "halfTrunc",
                "halfEven",
            ],
            Some("halfExpand"),
        )?;
        let rounding_priority = self.intl_string_option_from_value(
            ctx,
            raw_options.get("roundingPriority"),
            &["auto", "morePrecision", "lessPrecision"],
            Some("auto"),
        )?;
        if let Some(trailing_zero_display) = self.intl_string_option_from_value(
            ctx,
            raw_options.get("trailingZeroDisplay"),
            &["auto", "stripIfInteger"],
            Some("auto"),
        )? {
            out.trailing_zero_display = trailing_zero_display;
        }
        if let Some(use_grouping) = self.intl_boolean_option_from_value(ctx, raw_options.get("useGrouping"))? {
            out.use_grouping = use_grouping;
        }
        if let Some(minimum_integer_digits) =
            self.intl_default_u8_option_value(ctx, raw_options.get("minimumIntegerDigits"), "minimumIntegerDigits", 1, 21)?
        {
            out.minimum_integer_digits = minimum_integer_digits;
        }
        let default_currency_digits = out.currency.as_deref().map(Self::intl_currency_digits).unwrap_or(2);
        let notation = out.notation.clone();
        let default_minimum_fraction_digits = if out.style == "currency" && notation == "standard" {
            default_currency_digits
        } else {
            0
        };
        let default_maximum_fraction_digits = if out.style == "percent" {
            0
        } else if out.style == "currency" && notation == "standard" {
            default_currency_digits
        } else if notation == "compact" {
            0
        } else {
            3
        };
        let minimum_fraction_digits =
            self.intl_default_u8_option_value(ctx, raw_options.get("minimumFractionDigits"), "minimumFractionDigits", 0, 100)?;
        let maximum_fraction_digits =
            self.intl_default_u8_option_value(ctx, raw_options.get("maximumFractionDigits"), "maximumFractionDigits", 0, 100)?;
        out.minimum_fraction_digits = if let Some(minimum_fraction_digits) = minimum_fraction_digits {
            minimum_fraction_digits
        } else if out.style == "currency" && notation == "standard" {
            maximum_fraction_digits
                .map(|value| value.min(default_minimum_fraction_digits))
                .unwrap_or(default_minimum_fraction_digits)
        } else {
            default_minimum_fraction_digits
        };
        out.maximum_fraction_digits = if let Some(maximum_fraction_digits) = maximum_fraction_digits {
            maximum_fraction_digits
        } else {
            default_maximum_fraction_digits.max(out.minimum_fraction_digits)
        };
        if out.maximum_fraction_digits < out.minimum_fraction_digits {
            return Err(self.make_range_error_object(ctx, "maximumFractionDigits must be >= minimumFractionDigits"));
        }
        let minimum_significant_digits =
            self.intl_default_u8_option_value(ctx, raw_options.get("minimumSignificantDigits"), "minimumSignificantDigits", 1, 21)?;
        let maximum_significant_digits =
            self.intl_default_u8_option_value(ctx, raw_options.get("maximumSignificantDigits"), "maximumSignificantDigits", 1, 21)?;
        if minimum_significant_digits.is_some() || maximum_significant_digits.is_some() {
            out.minimum_significant_digits = Some(minimum_significant_digits.unwrap_or(1));
            out.maximum_significant_digits = Some(maximum_significant_digits.unwrap_or(21));
            if out.maximum_significant_digits < out.minimum_significant_digits {
                return Err(self.make_range_error_object(ctx, "maximumSignificantDigits must be >= minimumSignificantDigits"));
            }
        }
        if out.rounding_increment != 1 {
            if rounding_priority.as_deref().is_some_and(|value| value != "auto")
                || out.minimum_significant_digits.is_some()
                || out.maximum_significant_digits.is_some()
            {
                return Err(self.make_type_error_object(ctx, "roundingIncrement cannot be combined with this rounding mode"));
            }
            if maximum_fraction_digits.is_some()
                && minimum_fraction_digits.is_some()
                && out.maximum_fraction_digits != out.minimum_fraction_digits
            {
                return Err(self.make_range_error_object(ctx, "maximumFractionDigits must equal minimumFractionDigits"));
            }
        }
        out.resolved_locale = Self::intl_numbering_system_resolved_locale(
            &locale_base,
            if option_numbering_system.is_none() {
                ext_numbering_system.as_deref()
            } else if ext_numbering_system.as_deref() == option_numbering_system.as_deref() {
                option_numbering_system.as_deref()
            } else {
                None
            },
        );
        Ok(out)
    }

    fn intl_read_number_format_raw_options(
        &mut self,
        ctx: &GcContext<'gc>,
        options: &Value<'gc>,
    ) -> Result<IndexMap<String, Value<'gc>>, Value<'gc>> {
        let mut raw = IndexMap::new();
        for key in [
            "localeMatcher",
            "numberingSystem",
            "style",
            "currency",
            "currencyDisplay",
            "currencySign",
            "unit",
            "unitDisplay",
            "notation",
            "minimumIntegerDigits",
            "minimumFractionDigits",
            "maximumFractionDigits",
            "minimumSignificantDigits",
            "maximumSignificantDigits",
            "roundingIncrement",
            "roundingMode",
            "roundingPriority",
            "trailingZeroDisplay",
            "compactDisplay",
            "useGrouping",
            "signDisplay",
        ] {
            let value = self.read_named_property(ctx, options, key);
            if let Some(thrown) = self.pending_throw.take() {
                return Err(thrown);
            }
            raw.insert(key.to_string(), value);
        }
        Ok(raw)
    }

    fn intl_default_u8_option_value(
        &mut self,
        ctx: &GcContext<'gc>,
        value: Option<&Value<'gc>>,
        key: &str,
        min: u8,
        max: u8,
    ) -> Result<Option<u8>, Value<'gc>> {
        let Some(value) = value else {
            return Ok(None);
        };
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        let Some(number) = self.extract_number_with_coercion(ctx, value) else {
            return Err(self.pending_throw.clone().unwrap_or(Value::Undefined));
        };
        if !number.is_finite() {
            return Err(self.make_range_error_object(ctx, &format!("{} must be finite", key)));
        }
        let integer = number.trunc();
        if integer < min as f64 || integer > max as f64 {
            return Err(self.make_range_error_object(ctx, &format!("{} out of range", key)));
        }
        Ok(Some(integer as u8))
    }

    fn intl_string_option_from_value(
        &mut self,
        ctx: &GcContext<'gc>,
        value: Option<&Value<'gc>>,
        allowed: &[&str],
        default: Option<&str>,
    ) -> Result<Option<String>, Value<'gc>> {
        let Some(value) = value else {
            return Ok(default.map(|value| value.to_string()));
        };
        if matches!(value, Value::Undefined) {
            return Ok(default.map(|value| value.to_string()));
        }
        let text = match self.vm_to_string_like_spec(ctx, value) {
            Ok(value) => value,
            Err(err) => return Err(self.vm_value_from_error(ctx, &err)),
        };
        if allowed.is_empty() || allowed.contains(&text.as_str()) {
            Ok(Some(text))
        } else {
            Err(self.make_range_error_object(ctx, "Invalid option value"))
        }
    }

    fn intl_boolean_option_from_value(&mut self, _ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Result<Option<bool>, Value<'gc>> {
        let Some(value) = value else {
            return Ok(None);
        };
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        Ok(Some(value.to_truthy()))
    }

    fn intl_rounding_increment_option(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Result<Option<u16>, Value<'gc>> {
        let Some(value) = value else {
            return Ok(None);
        };
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        let Some(number) = self.extract_number_with_coercion(ctx, value) else {
            return Err(self.pending_throw.clone().unwrap_or(Value::Undefined));
        };
        if !number.is_finite() {
            return Err(self.make_range_error_object(ctx, "roundingIncrement must be finite"));
        }
        let integer = number.trunc();
        let increment = integer as u16;
        if integer != number
            || !matches!(
                increment,
                1 | 2 | 5 | 10 | 20 | 25 | 50 | 100 | 200 | 250 | 500 | 1000 | 2000 | 2500 | 5000
            )
        {
            return Err(self.make_range_error_object(ctx, "Invalid roundingIncrement"));
        }
        Ok(Some(increment))
    }

    fn intl_currency_digits(currency: &str) -> u8 {
        match currency {
            "BHD" | "IQD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND" => 3,
            "CLF" => 4,
            "BIF" | "CLP" | "DJF" | "GNF" | "ISK" | "JPY" | "KMF" | "KRW" | "PYG" | "RWF" | "UGX" | "UYI" | "VND" | "VUV" | "XAF"
            | "XOF" | "XPF" => 0,
            _ => 2,
        }
    }

    fn intl_store_number_format_options(&self, obj: &mut IndexMap<String, Value<'gc>>, options: &IntlNumberFormatOptions) {
        obj.insert(
            "__intl_numbering_system__".to_string(),
            Value::from(options.numbering_system.as_str()),
        );
        obj.insert("__intl_style__".to_string(), Value::from(options.style.as_str()));
        obj.insert("__intl_notation__".to_string(), Value::from(options.notation.as_str()));
        obj.insert("__intl_currency_sign__".to_string(), Value::from(options.currency_sign.as_str()));
        obj.insert("__intl_sign_display__".to_string(), Value::from(options.sign_display.as_str()));
        obj.insert(
            "__intl_rounding_increment__".to_string(),
            Value::Number(options.rounding_increment as f64),
        );
        obj.insert(
            "__intl_trailing_zero_display__".to_string(),
            Value::from(options.trailing_zero_display.as_str()),
        );
        if let Some(currency) = &options.currency {
            obj.insert("__intl_currency__".to_string(), Value::from(currency.as_str()));
            obj.insert(
                "__intl_currency_display__".to_string(),
                Value::from(options.currency_display.as_str()),
            );
        }
        if let Some(unit) = &options.unit {
            obj.insert("__intl_unit__".to_string(), Value::from(unit.as_str()));
            obj.insert("__intl_unit_display__".to_string(), Value::from(options.unit_display.as_str()));
        }
        if let Some(compact_display) = &options.compact_display {
            obj.insert("__intl_compact_display__".to_string(), Value::from(compact_display.as_str()));
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
        if let Some(minimum_significant_digits) = options.minimum_significant_digits {
            obj.insert(
                "__intl_minimum_significant_digits__".to_string(),
                Value::Number(minimum_significant_digits as f64),
            );
        }
        if let Some(maximum_significant_digits) = options.maximum_significant_digits {
            obj.insert(
                "__intl_maximum_significant_digits__".to_string(),
                Value::Number(maximum_significant_digits as f64),
            );
        }
    }

    fn intl_read_generic_numbering_system_options(
        &mut self,
        ctx: &GcContext<'gc>,
        requested_locales: &[String],
        options: Option<&Value<'gc>>,
    ) -> Result<IntlGenericNumberingSystemOptions, Value<'gc>> {
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
        let ext_numbering_system = locale_info
            .unicode_keywords
            .get("nu")
            .and_then(|value| Self::intl_supported_numbering_system(value));
        let mut out = IntlGenericNumberingSystemOptions {
            resolved_locale: Self::intl_numbering_system_resolved_locale(&locale_base, ext_numbering_system.as_deref()),
            numbering_system: ext_numbering_system.clone().unwrap_or_else(|| "latn".to_string()),
        };
        let Some(options) = options else {
            return Ok(out);
        };
        if matches!(options, Value::Undefined) || !Self::intl_is_object_like(options) {
            return Ok(out);
        }
        let mut option_numbering_system = None;
        if let Some(numbering_system) = self.intl_string_option(ctx, options, "numberingSystem", &[], None)? {
            if !Self::intl_is_valid_unicode_type_identifier(&numbering_system) {
                return Err(self.make_range_error_object(ctx, "Invalid numberingSystem"));
            }
            if let Some(numbering_system) = Self::intl_supported_numbering_system(&numbering_system) {
                out.numbering_system = numbering_system;
                option_numbering_system = Some(out.numbering_system.clone());
            }
        }
        out.resolved_locale = Self::intl_numbering_system_resolved_locale(
            &locale_base,
            if option_numbering_system.is_none() {
                ext_numbering_system.as_deref()
            } else if ext_numbering_system.as_deref() == option_numbering_system.as_deref() {
                option_numbering_system.as_deref()
            } else {
                None
            },
        );
        Ok(out)
    }

    fn intl_read_display_names_options(
        &mut self,
        ctx: &GcContext<'gc>,
        options: Option<&Value<'gc>>,
    ) -> Result<(String, String), Value<'gc>> {
        let Some(options) = options else {
            return Err(self.make_type_error_object(ctx, "type is required"));
        };
        if matches!(options, Value::Undefined) || matches!(options, Value::Null) || !Self::intl_is_object_like(options) {
            return Err(self.make_type_error_object(ctx, "type is required"));
        }
        let display_type = self
            .intl_string_option(
                ctx,
                options,
                "type",
                &["language", "region", "script", "currency", "calendar", "dateTimeField"],
                None,
            )?
            .ok_or_else(|| self.make_type_error_object(ctx, "type is required"))?;
        let fallback = self
            .intl_string_option(ctx, options, "fallback", &["code", "none"], Some("code"))?
            .unwrap_or_else(|| "code".to_string());
        Ok((display_type, fallback))
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

    fn intl_canonicalize_locale_tag(tag: &str) -> String {
        let lower = tag.to_ascii_lowercase();
        if let Some(alias) = Self::intl_whole_locale_alias(&lower) {
            return alias.to_string();
        }

        let parts: Vec<&str> = tag.split('-').collect();
        let mut end = parts.len();
        for (index, part) in parts.iter().enumerate().skip(1) {
            if part.len() == 1 {
                end = index;
                break;
            }
        }

        let mut language = parts.first().map_or(String::new(), |part| part.to_ascii_lowercase());
        let mut script = None;
        let mut region = None;
        let mut variants = Vec::new();
        let mut idx = 1;
        while idx < end {
            let part = parts[idx];
            if script.is_none() && part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()) {
                script = Some(Self::intl_titlecase_subtag(part));
            } else if region.is_none()
                && ((part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
                    || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit())))
            {
                region = Some(part.to_ascii_uppercase());
            } else {
                variants.push(part.to_ascii_lowercase());
            }
            idx += 1;
        }

        Self::intl_apply_language_aliases(&mut language, &mut script, &mut region);
        Self::intl_apply_region_aliases(&language, script.as_deref(), &mut region);
        Self::intl_apply_variant_aliases(&mut language, &mut script, &mut region, &mut variants);

        variants.sort();

        let mut out = vec![language];
        if let Some(script) = script {
            out.push(script);
        }
        if let Some(region) = region {
            out.push(region);
        }
        out.extend(variants);

        if end == parts.len() {
            return out.join("-");
        }

        let mut extensions = Vec::new();
        let mut private_use = None;
        let mut index = end;
        while index < parts.len() {
            let singleton = parts[index].to_ascii_lowercase();
            index += 1;
            let start = index;
            if singleton == "x" {
                private_use = Some(parts[start..].iter().map(|part| part.to_ascii_lowercase()).collect::<Vec<_>>());
                break;
            }
            while index < parts.len() && parts[index].len() != 1 {
                index += 1;
            }
            let subtags = &parts[start..index];
            extensions.push((singleton, subtags));
        }

        extensions.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (singleton, subtags) in extensions {
            out.push(singleton);
            match out.last().map(String::as_str) {
                Some("u") => out.extend(Self::intl_canonicalize_unicode_extension(subtags)),
                Some("t") => out.extend(Self::intl_canonicalize_transformed_extension(subtags)),
                _ => out.extend(subtags.iter().map(|part| part.to_ascii_lowercase())),
            }
        }
        if let Some(private_use) = private_use {
            out.push("x".to_string());
            out.extend(private_use);
        }
        out.join("-")
    }

    fn intl_whole_locale_alias(tag: &str) -> Option<&'static str> {
        match tag {
            "art-lojban" => Some("jbo"),
            "cel-gaulish" => Some("xtg"),
            "zh-guoyu" => Some("zh"),
            "zh-hakka" => Some("hak"),
            "zh-xiang" => Some("hsn"),
            "sgn-gr" => Some("gss"),
            _ => None,
        }
    }

    fn intl_apply_language_aliases(language: &mut String, script: &mut Option<String>, region: &mut Option<String>) {
        match language.as_str() {
            "cmn" => *language = "zh".to_string(),
            "ji" => *language = "yi".to_string(),
            "in" => *language = "id".to_string(),
            "iw" => *language = "he".to_string(),
            "mo" => *language = "ro".to_string(),
            "aar" => *language = "aa".to_string(),
            "heb" => *language = "he".to_string(),
            "ces" => *language = "cs".to_string(),
            "sh" => {
                *language = "sr".to_string();
                if script.is_none() {
                    *script = Some("Latn".to_string());
                }
            }
            "cnr" => {
                *language = "sr".to_string();
                if region.is_none() {
                    *region = Some("ME".to_string());
                }
            }
            _ => {}
        }
    }

    fn intl_apply_region_aliases(language: &str, script: Option<&str>, region: &mut Option<String>) {
        let Some(current) = region.clone() else {
            return;
        };
        let replacement = match current.as_str() {
            "DD" => Some("DE"),
            "SU" | "810" => {
                if language == "hy" || script == Some("Armn") {
                    Some("AM")
                } else {
                    Some("RU")
                }
            }
            "CS" => Some("RS"),
            "NT" => Some("SA"),
            _ => None,
        };
        if let Some(replacement) = replacement {
            *region = Some(replacement.to_string());
        }
    }

    fn intl_apply_variant_aliases(
        language: &mut String,
        script: &mut Option<String>,
        _region: &mut Option<String>,
        variants: &mut Vec<String>,
    ) {
        if language == "ja" && script.as_deref() == Some("Latn") {
            let has_hepburn = variants.iter().any(|variant| variant == "hepburn");
            let has_heploc = variants.iter().any(|variant| variant == "heploc");
            if has_hepburn && has_heploc {
                variants.retain(|variant| variant != "hepburn" && variant != "heploc");
                variants.push("alalc97".to_string());
            }
        }
        if language == "hy" {
            if variants.iter().any(|variant| variant == "arevela") {
                variants.retain(|variant| variant != "arevela");
            }
            if variants.iter().any(|variant| variant == "arevmda") {
                variants.retain(|variant| variant != "arevmda");
                *language = "hyw".to_string();
            }
        }
    }

    fn intl_titlecase_subtag(part: &str) -> String {
        let mut chars = part.chars();
        let Some(first) = chars.next() else {
            return String::new();
        };
        first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()
    }

    fn intl_canonicalize_unicode_extension(subtags: &[&str]) -> Vec<String> {
        let mut index = 0;
        let mut attributes = Vec::new();
        while index < subtags.len() && subtags[index].len() != 2 {
            attributes.push(subtags[index].to_ascii_lowercase());
            index += 1;
        }
        attributes.sort();

        let mut keywords: Vec<(String, Vec<String>)> = Vec::new();
        while index < subtags.len() {
            let key = subtags[index].to_ascii_lowercase();
            index += 1;
            let start = index;
            while index < subtags.len() && subtags[index].len() != 2 {
                index += 1;
            }
            let raw_value = subtags[start..index]
                .iter()
                .map(|part| part.to_ascii_lowercase())
                .collect::<Vec<_>>();
            let canonical_value = Self::intl_canonicalize_unicode_keyword_value(&key, raw_value);
            keywords.push((key, canonical_value));
        }
        keywords.sort_by(|(left, _), (right, _)| left.cmp(right));

        let mut out = attributes;
        for (key, value) in keywords {
            out.push(key);
            out.extend(value);
        }
        out
    }

    fn intl_canonicalize_unicode_keyword_value(key: &str, value: Vec<String>) -> Vec<String> {
        let joined = value.join("-");
        let canonical = match (key, joined.as_str()) {
            ("ca", "ethiopic-amete-alem") => Some("ethioaa"),
            ("ca", "islamicc") => Some("islamic-civil"),
            ("ks", "primary") => Some("level1"),
            ("ks", "tertiary") => Some("level3"),
            ("ms", "imperial") => Some("uksystem"),
            ("tz", "cnckg") => Some("cnsha"),
            ("tz", "eire") => Some("iedub"),
            ("tz", "est") => Some("papty"),
            ("tz", "gmt0") => Some("gmt"),
            ("tz", "uct") => Some("utc"),
            ("tz", "zulu") => Some("utc"),
            ("rg", "no23") | ("sd", "no23") => Some("no50"),
            ("rg", "cn11") | ("sd", "cn11") => Some("cnbj"),
            ("rg", "cz10a") | ("sd", "cz10a") => Some("cz110"),
            ("rg", "fra") | ("sd", "fra") => Some("frges"),
            ("rg", "frg") | ("sd", "frg") => Some("frges"),
            ("rg", "lud") | ("sd", "lud") => Some("lucl"),
            ("kb", "yes") | ("kc", "yes") | ("kh", "yes") | ("kk", "yes") | ("kn", "yes") => Some("true"),
            _ => None,
        };
        let canonical = canonical.unwrap_or(joined.as_str());
        if canonical.is_empty() || canonical == "true" {
            Vec::new()
        } else {
            canonical.split('-').map(str::to_string).collect()
        }
    }

    fn intl_canonicalize_transformed_extension(subtags: &[&str]) -> Vec<String> {
        let mut index = 0;
        let mut out = Vec::new();
        if let Some(first) = subtags.first()
            && first.chars().all(|c| c.is_ascii_alphabetic())
            && ((2..=3).contains(&first.len()) || (5..=8).contains(&first.len()))
        {
            let mut tlang_end = 1;
            if tlang_end < subtags.len() && subtags[tlang_end].len() == 4 && subtags[tlang_end].chars().all(|c| c.is_ascii_alphabetic()) {
                tlang_end += 1;
            }
            if tlang_end < subtags.len()
                && ((subtags[tlang_end].len() == 2 && subtags[tlang_end].chars().all(|c| c.is_ascii_alphabetic()))
                    || (subtags[tlang_end].len() == 3 && subtags[tlang_end].chars().all(|c| c.is_ascii_digit())))
            {
                tlang_end += 1;
            }
            while tlang_end < subtags.len() {
                let part = subtags[tlang_end];
                let is_variant =
                    (5..=8).contains(&part.len()) || (part.len() == 4 && part.chars().next().is_some_and(|c| c.is_ascii_digit()));
                if !is_variant {
                    break;
                }
                tlang_end += 1;
            }
            out.extend(
                Self::intl_canonicalize_locale_tag(&subtags[..tlang_end].join("-"))
                    .split('-')
                    .map(|part| part.to_ascii_lowercase()),
            );
            index = tlang_end;
        }

        let mut fields = Vec::new();
        while index < subtags.len() {
            let key = subtags[index].to_ascii_lowercase();
            index += 1;
            let start = index;
            while index < subtags.len() && subtags[index].len() != 2 {
                index += 1;
            }
            let joined = subtags[start..index]
                .iter()
                .map(|part| part.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join("-");
            let canonical = match (key.as_str(), joined.as_str()) {
                ("m0", "names") => "prprname".to_string(),
                _ => joined,
            };
            fields.push((key, canonical));
        }
        fields.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (key, value) in fields {
            out.push(key);
            out.extend(value.split('-').map(str::to_string));
        }
        out
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
        let value = value.to_ascii_lowercase();
        if value.is_empty() || matches!(value.as_str(), "default" | "search" | "standard" | "invalid") {
            return None;
        }
        let language = locale_base.split('-').next().unwrap_or(locale_base).to_ascii_lowercase();
        match value.as_str() {
            "phonebk" | "eor" => (language == "de").then_some(value),
            "pinyin" | "stroke" | "unihan" | "zhuyin" => (language == "zh").then_some(value),
            _ => INTL_SUPPORTED_COLLATIONS.contains(&value.as_str()).then_some(value),
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

    fn intl_numbering_system_resolved_locale(locale_base: &str, numbering_system: Option<&str>) -> String {
        Self::intl_date_time_format_resolved_locale(locale_base, None, numbering_system, None)
    }

    fn intl_locale_with_extensions(
        locale_base: &str,
        calendar: Option<&str>,
        collation: Option<&str>,
        numbering_system: Option<&str>,
    ) -> String {
        let mut out = locale_base.to_string();
        let mut extensions = Vec::new();
        if let Some(calendar) = calendar {
            extensions.push(format!("ca-{}", calendar));
        }
        if let Some(collation) = collation {
            extensions.push(format!("co-{}", collation));
        }
        if let Some(numbering_system) = numbering_system {
            extensions.push(format!("nu-{}", numbering_system));
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
        if let Some(rest) = value.strip_prefix("Etc/GMT").or_else(|| value.strip_prefix("etc/gmt")) {
            let sign = rest.chars().next()?;
            if !matches!(sign, '+' | '-') {
                return None;
            }
            let digits = &rest[1..];
            if digits.is_empty() || digits.len() > 2 || !digits.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            return Some(format!("Etc/GMT{sign}{digits}"));
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
        let value = value.to_ascii_lowercase();
        let canonical = match value.as_str() {
            "islamicc" => "islamic-civil",
            _ => value.as_str(),
        };
        INTL_SUPPORTED_CALENDARS.contains(&canonical).then_some(canonical.to_string())
    }

    fn intl_supported_numbering_system(value: &str) -> Option<String> {
        let value = value.to_ascii_lowercase();
        INTL_SUPPORTED_NUMBERING_SYSTEMS.contains(&value.as_str()).then_some(value)
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

    fn intl_is_well_formed_unit_identifier(value: &str) -> bool {
        if INTL_SUPPORTED_UNITS.contains(&value) {
            return true;
        }
        let Some((numerator, denominator)) = value.split_once("-per-") else {
            return false;
        };
        !denominator.contains("-per-") && INTL_SUPPORTED_UNITS.contains(&numerator) && INTL_SUPPORTED_UNITS.contains(&denominator)
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
        own_data_from_legacy_map(&intl.borrow(), kind)
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
                | "en-gb-oed"
                | "i-ami"
                | "i-bnn"
                | "i-default"
                | "i-enochian"
                | "i-hak"
                | "i-lux"
                | "i-mingo"
                | "i-navajo"
                | "i-pwn"
                | "i-tao"
                | "i-tay"
                | "i-tsu"
                | "sgn-be-fr"
                | "sgn-be-nl"
                | "sgn-ch-de"
                | "no-bok"
                | "zh-min"
                | "zh-min-nan"
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
                idx = match part.to_ascii_lowercase().as_str() {
                    "u" => match Self::intl_validate_unicode_extension(&parts, idx) {
                        Some(next) => next,
                        None => return false,
                    },
                    "t" => match Self::intl_validate_transformed_extension(&parts, idx) {
                        Some(next) => next,
                        None => return false,
                    },
                    _ => {
                        let start = idx;
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
                        if idx == start {
                            return false;
                        }
                        idx
                    }
                };
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

    fn intl_validate_unicode_extension(parts: &[&str], mut idx: usize) -> Option<usize> {
        let start = idx;
        while idx < parts.len() && parts[idx].len() != 1 && parts[idx].len() >= 3 {
            let part = parts[idx];
            if part.len() > 8 || !part.chars().all(|c| c.is_ascii_alphanumeric()) {
                return None;
            }
            idx += 1;
        }
        while idx < parts.len() && parts[idx].len() != 1 {
            let key = parts[idx];
            if key.len() != 2
                || !key.chars().all(|c| c.is_ascii_alphanumeric())
                || !key.chars().nth(1).is_some_and(|c| c.is_ascii_alphabetic())
            {
                return None;
            }
            idx += 1;
            while idx < parts.len() && parts[idx].len() != 1 && parts[idx].len() != 2 {
                let part = parts[idx];
                if part.len() > 8 || !part.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return None;
                }
                idx += 1;
            }
        }
        (idx > start).then_some(idx)
    }

    fn intl_validate_transformed_extension(parts: &[&str], mut idx: usize) -> Option<usize> {
        if idx >= parts.len() || parts[idx].len() == 1 {
            return None;
        }
        let start = idx;
        let first = parts[idx];
        if first.chars().all(|c| c.is_ascii_alphabetic()) && ((2..=3).contains(&first.len()) || (5..=8).contains(&first.len())) {
            idx += 1;
            if idx < parts.len() && parts[idx].len() == 4 && parts[idx].chars().all(|c| c.is_ascii_alphabetic()) {
                idx += 1;
            }
            if idx < parts.len()
                && ((parts[idx].len() == 2 && parts[idx].chars().all(|c| c.is_ascii_alphabetic()))
                    || (parts[idx].len() == 3 && parts[idx].chars().all(|c| c.is_ascii_digit())))
            {
                idx += 1;
            }
            while idx < parts.len() && parts[idx].len() != 1 {
                let part = parts[idx];
                let is_variant = ((5..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric()))
                    || (part.len() == 4
                        && part.chars().next().is_some_and(|c| c.is_ascii_digit())
                        && part.chars().skip(1).all(|c| c.is_ascii_alphanumeric()));
                if !is_variant {
                    break;
                }
                idx += 1;
            }
        }
        while idx < parts.len() && parts[idx].len() != 1 {
            let key = parts[idx];
            if key.len() != 2 || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
                return None;
            }
            idx += 1;
            let value_start = idx;
            while idx < parts.len() && parts[idx].len() != 1 {
                let part = parts[idx];
                if part.len() == 2 {
                    break;
                }
                if part.len() < 3 || part.len() > 8 || !part.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return None;
                }
                idx += 1;
            }
            if idx == value_start {
                return None;
            }
        }
        (idx > start).then_some(idx)
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

impl IntlNumberFormatOptions {
    fn from_object<'gc>(obj: &IndexMap<String, Value<'gc>>) -> Self {
        Self {
            resolved_locale: match obj.get("__intl_locale__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => INTL_DEFAULT_LOCALE.to_string(),
            },
            numbering_system: match obj.get("__intl_numbering_system__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "latn".to_string(),
            },
            style: match obj.get("__intl_style__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "decimal".to_string(),
            },
            currency: match obj.get("__intl_currency__") {
                Some(Value::String(text)) => Some(crate::unicode::utf16_to_utf8(text)),
                _ => None,
            },
            currency_display: match obj.get("__intl_currency_display__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "symbol".to_string(),
            },
            unit: match obj.get("__intl_unit__") {
                Some(Value::String(text)) => Some(crate::unicode::utf16_to_utf8(text)),
                _ => None,
            },
            unit_display: match obj.get("__intl_unit_display__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "short".to_string(),
            },
            notation: match obj.get("__intl_notation__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "standard".to_string(),
            },
            compact_display: match obj.get("__intl_compact_display__") {
                Some(Value::String(text)) => Some(crate::unicode::utf16_to_utf8(text)),
                _ => None,
            },
            currency_sign: match obj.get("__intl_currency_sign__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "standard".to_string(),
            },
            sign_display: match obj.get("__intl_sign_display__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "auto".to_string(),
            },
            rounding_increment: match obj.get("__intl_rounding_increment__") {
                Some(Value::Number(value)) => *value as u16,
                _ => 1,
            },
            trailing_zero_display: match obj.get("__intl_trailing_zero_display__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "auto".to_string(),
            },
            use_grouping: !matches!(obj.get("__intl_use_grouping__"), Some(Value::Boolean(false))),
            minimum_integer_digits: match obj.get("__intl_minimum_integer_digits__") {
                Some(Value::Number(value)) => *value as u8,
                _ => 1,
            },
            minimum_fraction_digits: match obj.get("__intl_minimum_fraction_digits__") {
                Some(Value::Number(value)) => *value as u8,
                _ => 0,
            },
            maximum_fraction_digits: match obj.get("__intl_maximum_fraction_digits__") {
                Some(Value::Number(value)) => *value as u8,
                _ => 3,
            },
            minimum_significant_digits: match obj.get("__intl_minimum_significant_digits__") {
                Some(Value::Number(value)) => Some(*value as u8),
                _ => None,
            },
            maximum_significant_digits: match obj.get("__intl_maximum_significant_digits__") {
                Some(Value::Number(value)) => Some(*value as u8),
                _ => None,
            },
        }
    }
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
    currency_display: String,
    unit: Option<String>,
    unit_display: String,
    notation: String,
    compact_display: Option<String>,
    currency_sign: String,
    sign_display: String,
    rounding_increment: u16,
    trailing_zero_display: String,
    use_grouping: bool,
    minimum_integer_digits: u8,
    minimum_fraction_digits: u8,
    maximum_fraction_digits: u8,
    minimum_significant_digits: Option<u8>,
    maximum_significant_digits: Option<u8>,
}

struct IntlGenericNumberingSystemOptions {
    resolved_locale: String,
    numbering_system: String,
}

struct IntlExactDecimal {
    negative: bool,
    digits: String,
    scale: usize,
}

struct IntlUnitPattern {
    prefix_unit: Option<String>,
    prefix_literal: Option<String>,
    suffix_literal: Option<String>,
    suffix_unit: String,
}

enum IntlFormattedNumberInput {
    Number(f64),
    BigInt(String),
    DecimalString(String),
}
