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
            if let Some(currency) = borrow.get("__intl_currency__").cloned() {
                result.insert("currency".to_string(), currency);
            }
            if let Some(unit) = borrow.get("__intl_unit__").cloned() {
                result.insert("unit".to_string(), unit);
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
        let digits = match numbering_system {
            "arab" => Some(['٠', '١', '٢', '٣', '٤', '٥', '٦', '٧', '٨', '٩']),
            "arabext" => Some(['۰', '۱', '۲', '۳', '۴', '۵', '۶', '۷', '۸', '۹']),
            _ => None,
        };
        if let Some(digits) = digits {
            text = text
                .chars()
                .map(|ch| match ch {
                    '0'..='9' => digits[ch as usize - '0' as usize],
                    _ => ch,
                })
                .collect();
        }
        text
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
            unit: None,
            use_grouping: true,
            minimum_integer_digits: 1,
            minimum_fraction_digits: 0,
            maximum_fraction_digits: 3,
        };
        let Some(options) = options else {
            out.resolved_locale = Self::intl_numbering_system_resolved_locale(&locale_base, ext_numbering_system.as_deref());
            return Ok(out);
        };
        if matches!(options, Value::Undefined) || !Self::intl_is_object_like(options) {
            out.resolved_locale = Self::intl_numbering_system_resolved_locale(&locale_base, ext_numbering_system.as_deref());
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
        if out.style == "unit"
            && let Some(unit) = self.intl_string_option(ctx, options, "unit", &[], None)?
            && INTL_SUPPORTED_UNITS.contains(&unit.as_str())
        {
            out.unit = Some(unit);
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

    fn intl_store_number_format_options(&self, obj: &mut IndexMap<String, Value<'gc>>, options: &IntlNumberFormatOptions) {
        obj.insert(
            "__intl_numbering_system__".to_string(),
            Value::from(options.numbering_system.as_str()),
        );
        obj.insert("__intl_style__".to_string(), Value::from(options.style.as_str()));
        if let Some(currency) = &options.currency {
            obj.insert("__intl_currency__".to_string(), Value::from(currency.as_str()));
        }
        if let Some(unit) = &options.unit {
            obj.insert("__intl_unit__".to_string(), Value::from(unit.as_str()));
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
    unit: Option<String>,
    use_grouping: bool,
    minimum_integer_digits: u8,
    minimum_fraction_digits: u8,
    maximum_fraction_digits: u8,
}

struct IntlGenericNumberingSystemOptions {
    resolved_locale: String,
    numbering_system: String,
}
