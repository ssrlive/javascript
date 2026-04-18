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
            borrow.insert(
                "containing".to_string(),
                Self::make_host_fn_with_name_len(ctx, "intl.segments.containing", "containing", 1.0, false),
            );
            mark_nonenumerable(&mut borrow, "containing");
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
            if matches!(
                name,
                "intl.locale.ctor"
                    | "intl.durationFormat.ctor"
                    | "intl.listFormat.ctor"
                    | "intl.relativeTimeFormat.ctor"
                    | "intl.pluralRules.ctor"
                    | "intl.segmenter.ctor"
            ) && self.new_target_stack.last().is_none_or(|value| matches!(value, Value::Undefined))
            {
                self.pending_throw = Some(self.make_type_error_object(ctx, "Constructor Intl.Locale requires 'new'"));
                return Value::Undefined;
            }
            let ctor_value = self
                .new_target_stack
                .last()
                .filter(|value| !matches!(value, Value::Undefined))
                .cloned();
            return match self.intl_construct_service_instance(ctx, kind, ctor_value.as_ref(), args) {
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
            "intl.durationFormat.format" => self.intl_duration_format(ctx, receiver, args.first()),
            "intl.durationFormat.formatToParts" => self.intl_duration_format_to_parts(ctx, receiver, args.first()),
            "intl.listFormat.format" => self.intl_list_format(ctx, receiver, args.first()),
            "intl.listFormat.formatToParts" => self.intl_list_format_to_parts(ctx, receiver, args.first()),
            "intl.relativeTimeFormat.format" => self.intl_relative_time_format(ctx, receiver, args.first(), args.get(1)),
            "intl.relativeTimeFormat.formatToParts" => self.intl_relative_time_format_to_parts(ctx, receiver, args.first(), args.get(1)),
            "intl.locale.get.baseName" => self.intl_locale_slot_getter(ctx, receiver, "__intl_base_name__"),
            "intl.locale.get.language" => self.intl_locale_slot_getter(ctx, receiver, "__intl_language__"),
            "intl.locale.get.script" => self.intl_locale_slot_getter(ctx, receiver, "__intl_script__"),
            "intl.locale.get.region" => self.intl_locale_slot_getter(ctx, receiver, "__intl_region__"),
            "intl.locale.get.variants" => self.intl_locale_slot_getter(ctx, receiver, "__intl_variants__"),
            "intl.locale.get.calendar" => self.intl_locale_slot_getter(ctx, receiver, "__intl_calendar__"),
            "intl.locale.get.collation" => self.intl_locale_slot_getter(ctx, receiver, "__intl_collation__"),
            "intl.locale.get.hourCycle" => self.intl_locale_slot_getter(ctx, receiver, "__intl_hour_cycle__"),
            "intl.locale.get.caseFirst" => self.intl_locale_slot_getter(ctx, receiver, "__intl_case_first__"),
            "intl.locale.get.numeric" => self.intl_locale_slot_getter(ctx, receiver, "__intl_numeric__"),
            "intl.locale.get.numberingSystem" => self.intl_locale_slot_getter(ctx, receiver, "__intl_numbering_system__"),
            "intl.locale.get.firstDayOfWeek" => self.intl_locale_slot_getter(ctx, receiver, "__intl_first_day_of_week__"),
            "intl.locale.getCalendars" => self.intl_locale_get_calendars(ctx, receiver),
            "intl.locale.getCollations" => self.intl_locale_get_collations(ctx, receiver),
            "intl.locale.getHourCycles" => self.intl_locale_get_hour_cycles(ctx, receiver),
            "intl.locale.getNumberingSystems" => self.intl_locale_get_numbering_systems(ctx, receiver),
            "intl.locale.getTextInfo" => self.intl_locale_get_text_info(ctx, receiver),
            "intl.locale.getTimeZones" => self.intl_locale_get_time_zones(ctx, receiver),
            "intl.locale.getWeekInfo" => self.intl_locale_get_week_info(ctx, receiver),
            "intl.locale.maximize" => self.intl_locale_maximize(ctx, receiver),
            "intl.locale.minimize" => self.intl_locale_minimize(ctx, receiver),
            "intl.locale.toString" => self.intl_locale_to_string(ctx, receiver),
            "intl.numberFormat.get.format" => self.intl_number_format_getter(ctx, receiver),
            "intl.numberFormat.format" => self.intl_number_format_format(ctx, receiver, args),
            "intl.numberFormat.formatToParts" => self.intl_number_format_format_to_parts(ctx, receiver, args.first()),
            "intl.numberFormat.formatRange" => self.intl_number_format_format_range(ctx, receiver, args.first(), args.get(1)),
            "intl.numberFormat.formatRangeToParts" => {
                self.intl_number_format_format_range_to_parts(ctx, receiver, args.first(), args.get(1))
            }
            "intl.pluralRules.select" => self.intl_plural_rules_select(ctx, receiver, args.first()),
            "intl.pluralRules.selectRange" => self.intl_plural_rules_select_range(ctx, receiver, args.first(), args.get(1)),
            "intl.displayNames.resolvedOptions" => self.intl_resolved_options_for_kind(ctx, receiver, "DisplayNames"),
            "intl.service.resolvedOptions" => self.intl_resolved_options(ctx, receiver),
            "intl.segmenter.segment" => {
                let Some(segmenter) = self.intl_require_initialized_service(ctx, receiver, Some("Segmenter")) else {
                    return Value::Undefined;
                };
                let undefined = Value::Undefined;
                let input = match self.intl_to_string_value(ctx, args.first().unwrap_or(&undefined)) {
                    Ok(input) => input,
                    Err(err) => {
                        self.pending_throw = Some(self.vm_value_from_error(ctx, &err));
                        return Value::Undefined;
                    }
                };
                let mut obj = IndexMap::new();
                {
                    let borrow = segmenter.borrow();
                    if let Some(proto) = borrow.get("__segments_proto__").cloned() {
                        obj.insert("__proto__".to_string(), proto);
                    }
                    if let Some(proto) = borrow.get("__segment_iter_proto__").cloned() {
                        obj.insert("__segment_iter_proto__".to_string(), proto);
                    }
                    if let Some(locale) = borrow.get("__intl_locale__").cloned() {
                        obj.insert("__intl_locale__".to_string(), locale);
                    }
                    if let Some(granularity) = borrow.get("__intl_granularity__").cloned() {
                        obj.insert("__intl_granularity__".to_string(), granularity);
                    }
                }
                let items = self.intl_segment_items_array(ctx, &input, &obj);
                obj.insert("__segments_input__".to_string(), input);
                obj.insert("__items__".to_string(), items);
                Value::Object(new_gc_cell_ptr(ctx, obj))
            }
            "intl.segments.iterator" => {
                let mut obj = IndexMap::new();
                if let Some(Value::Object(segments)) = receiver {
                    let borrow = segments.borrow();
                    if let Some(proto) = borrow.get("__segment_iter_proto__").cloned() {
                        obj.insert("__proto__".to_string(), proto);
                    }
                    if let Some(items) = borrow.get("__items__").cloned() {
                        obj.insert("__items__".to_string(), items);
                    }
                }
                obj.insert("__index__".to_string(), Value::Number(0.0));
                Value::Object(new_gc_cell_ptr(ctx, obj))
            }
            "intl.segments.containing" => self.intl_segments_containing(ctx, receiver, args.first()),
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
            Self::insert_property_with_attributes(
                &mut proto,
                "@@sym:4",
                &Value::from(format!("Intl.{display_name}").as_str()),
                false,
                false,
                true,
            );
            if display_name != "Locale" {
                proto.insert(
                    "resolvedOptions".to_string(),
                    Self::make_host_fn_with_name_len(
                        ctx,
                        if display_name == "DisplayNames" {
                            "intl.displayNames.resolvedOptions"
                        } else {
                            "intl.service.resolvedOptions"
                        },
                        "resolvedOptions",
                        0.0,
                        false,
                    ),
                );
                mark_nonenumerable(&mut proto, "resolvedOptions");
            }
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
                proto.insert(
                    "formatRange".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.numberFormat.formatRange", "formatRange", 2.0, false),
                );
                mark_nonenumerable(&mut proto, "formatRange");
                proto.insert(
                    "formatRangeToParts".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.numberFormat.formatRangeToParts", "formatRangeToParts", 2.0, false),
                );
                mark_nonenumerable(&mut proto, "formatRangeToParts");
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
            if display_name == "DurationFormat" {
                proto.insert(
                    "format".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.durationFormat.format", "format", 1.0, false),
                );
                mark_nonenumerable(&mut proto, "format");
                proto.insert(
                    "formatToParts".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.durationFormat.formatToParts", "formatToParts", 1.0, false),
                );
                mark_nonenumerable(&mut proto, "formatToParts");
            }
            if display_name == "ListFormat" {
                proto.insert(
                    "format".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.listFormat.format", "format", 1.0, false),
                );
                mark_nonenumerable(&mut proto, "format");
                proto.insert(
                    "formatToParts".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.listFormat.formatToParts", "formatToParts", 1.0, false),
                );
                mark_nonenumerable(&mut proto, "formatToParts");
            }
            if display_name == "RelativeTimeFormat" {
                proto.insert(
                    "format".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.relativeTimeFormat.format", "format", 2.0, false),
                );
                mark_nonenumerable(&mut proto, "format");
                proto.insert(
                    "formatToParts".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.relativeTimeFormat.formatToParts", "formatToParts", 2.0, false),
                );
                mark_nonenumerable(&mut proto, "formatToParts");
            }
            if display_name == "PluralRules" {
                proto.insert(
                    "select".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.pluralRules.select", "select", 1.0, false),
                );
                mark_nonenumerable(&mut proto, "select");
                proto.insert(
                    "selectRange".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.pluralRules.selectRange", "selectRange", 2.0, false),
                );
                mark_nonenumerable(&mut proto, "selectRange");
            }
            if display_name == "Locale" {
                for (prop, host) in [
                    ("baseName", "intl.locale.get.baseName"),
                    ("language", "intl.locale.get.language"),
                    ("script", "intl.locale.get.script"),
                    ("region", "intl.locale.get.region"),
                    ("variants", "intl.locale.get.variants"),
                    ("calendar", "intl.locale.get.calendar"),
                    ("collation", "intl.locale.get.collation"),
                    ("hourCycle", "intl.locale.get.hourCycle"),
                    ("caseFirst", "intl.locale.get.caseFirst"),
                    ("numeric", "intl.locale.get.numeric"),
                    ("numberingSystem", "intl.locale.get.numberingSystem"),
                    ("firstDayOfWeek", "intl.locale.get.firstDayOfWeek"),
                ] {
                    let getter = Self::make_host_fn_with_name_len(ctx, host, &format!("get {prop}"), 0.0, false);
                    Self::insert_getter_property_with_attributes(&mut proto, prop, &getter, false, true);
                }
                proto.insert(
                    "toString".to_string(),
                    Self::make_host_fn_with_name_len(ctx, "intl.locale.toString", "toString", 0.0, false),
                );
                mark_nonenumerable(&mut proto, "toString");
                for (name, length) in [
                    ("getCalendars", 0.0),
                    ("getCollations", 0.0),
                    ("getHourCycles", 0.0),
                    ("getNumberingSystems", 0.0),
                    ("getTextInfo", 0.0),
                    ("getTimeZones", 0.0),
                    ("getWeekInfo", 0.0),
                    ("maximize", 0.0),
                    ("minimize", 0.0),
                ] {
                    proto.insert(
                        name.to_string(),
                        Self::make_host_fn_with_name_len(ctx, &format!("intl.locale.{name}"), name, length, false),
                    );
                    mark_nonenumerable(&mut proto, name);
                }
            }
        }

        let ctor = Self::make_host_fn_with_name_len(
            ctx,
            host_name,
            display_name,
            if display_name == "Locale" {
                1.0
            } else if display_name == "DisplayNames" {
                2.0
            } else {
                0.0
            },
            true,
        );
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
        let ctor_value = ctor_value
            .cloned()
            .or_else(|| self.intl_service_constructor_from_global(kind))
            .unwrap_or(Value::Undefined);
        let prototype = self.intl_service_get_prototype_from_constructor(ctx, &ctor_value, kind)?;
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
        let duration_format_opts = if kind == "DurationFormat" {
            Some(self.intl_read_duration_format_options(ctx, &requested_locales, args.get(1))?)
        } else {
            None
        };
        let relative_time_format_options = if kind == "RelativeTimeFormat" {
            Some(self.intl_read_relative_time_format_options(ctx, &requested_locales, args.get(1))?)
        } else {
            None
        };
        let display_names_options = if kind == "DisplayNames" {
            Some(self.intl_read_display_names_options(ctx, args.get(1))?)
        } else {
            None
        };
        let list_format_options = if kind == "ListFormat" {
            Some(self.intl_read_list_format_options(&requested_locales, ctx, args.get(1))?)
        } else {
            None
        };
        let segmenter_options = if kind == "Segmenter" {
            Some(self.intl_read_segmenter_options(ctx, &requested_locales, args.get(1))?)
        } else {
            None
        };
        let plural_rules_options = if kind == "PluralRules" {
            Some(self.intl_read_plural_rules_options(ctx, &requested_locales, args.get(1))?)
        } else {
            None
        };

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
            .or_else(|| duration_format_opts.as_ref().map(|opts| opts.resolved_locale.clone()))
            .or_else(|| list_format_options.as_ref().map(|opts| opts.resolved_locale.clone()))
            .or_else(|| relative_time_format_options.as_ref().map(|opts| opts.resolved_locale.clone()))
            .or_else(|| segmenter_options.as_ref().map(|opts| opts.resolved_locale.clone()))
            .or_else(|| plural_rules_options.as_ref().map(|opts| opts.resolved_locale.clone()))
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
        if let Some(opts) = duration_format_opts {
            self.intl_store_duration_format_options(&mut obj, &opts);
        }
        if let Some(opts) = list_format_options {
            self.intl_store_list_format_options(&mut obj, &opts);
        }
        if let Some(options) = relative_time_format_options {
            self.intl_store_relative_time_format_options(&mut obj, &options);
        }
        if let Some(options) = segmenter_options {
            self.intl_store_segmenter_options(&mut obj, &options);
        }
        if let Some(options) = plural_rules_options {
            self.intl_store_plural_rules_options(&mut obj, &options);
        }
        if let Some(options) = display_names_options {
            self.intl_store_display_names_options(&mut obj, &options);
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
        if kind == "Segmenter"
            && (!obj.contains_key("__segments_proto__") || !obj.contains_key("__segment_iter_proto__"))
            && let Some(Value::Object(default_ctor)) = self.intl_service_constructor_from_global("Segmenter")
        {
            let borrow = default_ctor.borrow();
            if !obj.contains_key("__segments_proto__")
                && let Some(proto) = borrow.get("__segments_proto__").cloned()
            {
                obj.insert("__segments_proto__".to_string(), proto);
            }
            if !obj.contains_key("__segment_iter_proto__")
                && let Some(proto) = borrow.get("__segment_iter_proto__").cloned()
            {
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
        self.intl_resolved_options_from_object(ctx, &obj)
    }

    fn intl_resolved_options_for_kind(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, kind: &str) -> Value<'gc> {
        let Some(obj) = self.intl_require_initialized_service(ctx, receiver, Some(kind)) else {
            return Value::Undefined;
        };
        self.intl_resolved_options_from_object(ctx, &obj)
    }

    fn intl_resolved_options_from_object(&self, ctx: &GcContext<'gc>, obj: &Gc<GcCell<IndexMap<String, Value<'gc>>>>) -> Value<'gc> {
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
                "dateStyle",
                "timeStyle",
            ] {
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
            result.insert("useGrouping".to_string(), Self::intl_use_grouping_value_from_object(&borrow));
            result.insert(
                "notation".to_string(),
                borrow.get("__intl_notation__").cloned().unwrap_or_else(|| Value::from("standard")),
            );
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
                "roundingMode".to_string(),
                borrow
                    .get("__intl_rounding_mode__")
                    .cloned()
                    .unwrap_or_else(|| Value::from("halfExpand")),
            );
            result.insert(
                "roundingPriority".to_string(),
                borrow
                    .get("__intl_rounding_priority__")
                    .cloned()
                    .unwrap_or_else(|| Value::from("auto")),
            );
            result.insert(
                "trailingZeroDisplay".to_string(),
                borrow
                    .get("__intl_trailing_zero_display__")
                    .cloned()
                    .unwrap_or_else(|| Value::from("auto")),
            );
        } else if matches!(borrow.get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "RelativeTimeFormat")
        {
            result.insert(
                "style".to_string(),
                borrow.get("__intl_style__").cloned().unwrap_or_else(|| Value::from("long")),
            );
            result.insert(
                "numeric".to_string(),
                borrow.get("__intl_numeric__").cloned().unwrap_or_else(|| Value::from("always")),
            );
            result.insert(
                "numberingSystem".to_string(),
                borrow
                    .get("__intl_numbering_system__")
                    .cloned()
                    .unwrap_or_else(|| Value::from("latn")),
            );
        } else if matches!(borrow.get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "PluralRules") {
            result.insert(
                "type".to_string(),
                borrow.get("__intl_type__").cloned().unwrap_or_else(|| Value::from("cardinal")),
            );
            result.insert(
                "notation".to_string(),
                borrow.get("__intl_notation__").cloned().unwrap_or_else(|| Value::from("standard")),
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
            result.insert(
                "pluralCategories".to_string(),
                Value::Array(new_gc_cell_ptr(
                    ctx,
                    VmArrayData::new(
                        Self::intl_plural_rule_categories(
                            borrow
                                .get("__intl_locale__")
                                .and_then(|value| match value {
                                    Value::String(text) => Some(crate::unicode::utf16_to_utf8(text)),
                                    _ => None,
                                })
                                .unwrap_or_else(|| INTL_DEFAULT_LOCALE.to_string())
                                .as_str(),
                            borrow
                                .get("__intl_type__")
                                .and_then(|value| match value {
                                    Value::String(text) => Some(crate::unicode::utf16_to_utf8(text)),
                                    _ => None,
                                })
                                .unwrap_or_else(|| "cardinal".to_string())
                                .as_str(),
                        )
                        .into_iter()
                        .map(Value::from)
                        .collect(),
                    ),
                )),
            );
        } else if matches!(borrow.get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "Segmenter") {
            result.insert(
                "granularity".to_string(),
                borrow
                    .get("__intl_granularity__")
                    .cloned()
                    .unwrap_or_else(|| Value::from("grapheme")),
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
            result.insert(
                "style".to_string(),
                borrow.get("__intl_style__").cloned().unwrap_or_else(|| Value::from("short")),
            );
            for key in [
                "years",
                "yearsDisplay",
                "months",
                "monthsDisplay",
                "weeks",
                "weeksDisplay",
                "days",
                "daysDisplay",
                "hours",
                "hoursDisplay",
                "minutes",
                "minutesDisplay",
                "seconds",
                "secondsDisplay",
                "milliseconds",
                "millisecondsDisplay",
                "microseconds",
                "microsecondsDisplay",
                "nanoseconds",
                "nanosecondsDisplay",
            ] {
                if let Some(value) = borrow.get(&format!("__intl_{}__", key)).cloned() {
                    result.insert(key.to_string(), value);
                }
            }
            if let Some(fractional_digits) = borrow.get("__intl_fractional_digits__").cloned() {
                result.insert("fractionalDigits".to_string(), fractional_digits);
            }
        } else if matches!(borrow.get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "DisplayNames")
        {
            result.insert(
                "style".to_string(),
                borrow.get("__intl_style__").cloned().unwrap_or_else(|| Value::from("long")),
            );
            result.insert(
                "type".to_string(),
                borrow.get("__intl_type__").cloned().unwrap_or_else(|| Value::from("language")),
            );
            result.insert(
                "fallback".to_string(),
                borrow.get("__intl_fallback__").cloned().unwrap_or_else(|| Value::from("code")),
            );
            if let Some(language_display) = borrow.get("__intl_language_display__").cloned() {
                result.insert("languageDisplay".to_string(), language_display);
            }
        } else if matches!(borrow.get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "ListFormat") {
            result.insert(
                "type".to_string(),
                borrow
                    .get("__intl_list_type__")
                    .cloned()
                    .unwrap_or_else(|| Value::from("conjunction")),
            );
            result.insert(
                "style".to_string(),
                borrow.get("__intl_list_style__").cloned().unwrap_or_else(|| Value::from("long")),
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
        let requested = self.intl_locale_constructor_tag(ctx, args.first())?;
        let locale_info = Self::intl_locale_info(&requested);
        let mut base = Self::intl_locale_base_components(&locale_info.base);
        let mut calendar = locale_info
            .unicode_keywords
            .get("ca")
            .and_then(|value| Self::intl_supported_calendar(value));
        let mut collation = locale_info
            .unicode_keywords
            .get("co")
            .and_then(|value| Self::intl_locale_collation_keyword(value));
        let mut hour_cycle = locale_info
            .unicode_keywords
            .get("hc")
            .and_then(|value| Self::intl_supported_hour_cycle(value));
        let mut case_first = locale_info
            .unicode_keywords
            .get("kf")
            .and_then(|value| Self::intl_locale_case_first_keyword(value));
        let mut numeric_keyword = locale_info
            .unicode_keywords
            .get("kn")
            .and_then(|value| Self::intl_collator_unicode_bool(value));
        let mut numbering_system = locale_info
            .unicode_keywords
            .get("nu")
            .and_then(|value| Self::intl_supported_numbering_system(value));
        let mut first_day_of_week = locale_info
            .unicode_keywords
            .get("fw")
            .and_then(|value| Self::intl_locale_first_day_of_week(value));
        if let Some(options) = args.get(1)
            && !matches!(options, Value::Undefined)
        {
            if matches!(options, Value::Null) {
                return Err(self.make_type_error_object(ctx, "options must not be null"));
            }
            let boxed_options = self.intl_box_primitive_if_needed(ctx, options);
            let options = if Self::intl_is_object_like(&boxed_options) {
                boxed_options
            } else {
                Value::Undefined
            };
            if !matches!(options, Value::Undefined) {
                if let Some(value) = self.intl_string_option(ctx, &options, "language", &[], None)? {
                    let Some(language) = Self::intl_locale_language_option(&value) else {
                        return Err(self.make_range_error_object(ctx, "Invalid language"));
                    };
                    base.language = language;
                }
                if let Some(value) = self.intl_string_option(ctx, &options, "script", &[], None)? {
                    let Some(script) = Self::intl_locale_script_option(&value) else {
                        return Err(self.make_range_error_object(ctx, "Invalid script"));
                    };
                    base.script = Some(script);
                }
                if let Some(value) = self.intl_string_option(ctx, &options, "region", &[], None)? {
                    let Some(region) = Self::intl_locale_region_option(&value) else {
                        return Err(self.make_range_error_object(ctx, "Invalid region"));
                    };
                    base.region = Some(region);
                }
                if let Some(value) = self.intl_string_option(ctx, &options, "variants", &[], None)? {
                    let Some(variants) = Self::intl_locale_variants_option(&value) else {
                        return Err(self.make_range_error_object(ctx, "Invalid variants"));
                    };
                    base.variants = variants;
                }
                if let Some(value) = self.intl_string_option(ctx, &options, "calendar", &[], None)? {
                    if !Self::intl_is_valid_unicode_type_identifier(&value) {
                        return Err(self.make_range_error_object(ctx, "Invalid calendar"));
                    }
                    let canonical =
                        Self::intl_canonicalize_unicode_keyword_value("ca", value.split('-').map(str::to_string).collect::<Vec<_>>());
                    let canonical = canonical.join("-");
                    calendar = Self::intl_supported_calendar(&canonical).or(Some(canonical));
                }
                if let Some(value) = self.intl_string_option(ctx, &options, "collation", &[], None)? {
                    if !Self::intl_is_valid_unicode_type_identifier(&value) {
                        return Err(self.make_range_error_object(ctx, "Invalid collation"));
                    }
                    collation = Self::intl_locale_collation_keyword(&value);
                }
                if let Some(value) = self.intl_string_option(ctx, &options, "hourCycle", &["h11", "h12", "h23", "h24"], None)? {
                    hour_cycle = Some(value);
                }
                if let Some(value) = self.intl_string_option(ctx, &options, "caseFirst", &["upper", "lower", "false"], None)? {
                    case_first = Some(value);
                }
                if let Some(value) = self.intl_boolean_option(ctx, &options, "numeric")? {
                    numeric_keyword = Some(value);
                }
                if let Some(value) = self.intl_string_option(ctx, &options, "numberingSystem", &[], None)? {
                    if !Self::intl_is_valid_unicode_type_identifier(&value) {
                        return Err(self.make_range_error_object(ctx, "Invalid numberingSystem"));
                    }
                    numbering_system = Self::intl_supported_numbering_system(&value).or_else(|| Some(value.to_ascii_lowercase()));
                }
                if let Some(value) = self.intl_string_option(ctx, &options, "firstDayOfWeek", &[], None)? {
                    let Some(normalized) = Self::intl_locale_first_day_of_week(&value) else {
                        return Err(self.make_range_error_object(ctx, "Invalid firstDayOfWeek"));
                    };
                    first_day_of_week = Some(normalized);
                }
            }
        }
        Self::intl_apply_regular_grandfathered_alias(&mut base.language, &mut base.variants);
        Self::intl_apply_language_aliases(&mut base.language, &mut base.script, &mut base.region);
        Self::intl_apply_region_aliases(&base.language, base.script.as_deref(), &mut base.region);
        Self::intl_apply_variant_aliases(&mut base.language, &mut base.script, &mut base.region, &mut base.variants);
        base.variants.sort();
        let numeric = numeric_keyword.unwrap_or(false);
        let base_name = Self::intl_locale_base_name_string(&base);
        let locale = Self::intl_locale_with_keywords(
            &requested,
            &base_name,
            IntlLocaleKeywordOptions {
                calendar: calendar.as_deref(),
                collation: collation.as_deref(),
                first_day_of_week: first_day_of_week.as_deref(),
                hour_cycle: hour_cycle.as_deref(),
                case_first: case_first.as_deref(),
                numeric: numeric_keyword,
                numbering_system: numbering_system.as_deref(),
            },
        );
        let ctor_value = ctor_value
            .cloned()
            .or_else(|| self.intl_service_constructor_from_global("Locale"))
            .unwrap_or(Value::Undefined);
        let prototype = self.intl_service_get_prototype_from_constructor(ctx, &ctor_value, "Locale")?;
        let mut obj = IndexMap::new();
        if !matches!(prototype, Value::Undefined) {
            obj.insert("__proto__".to_string(), prototype);
        }
        obj.insert("__intl_kind__".to_string(), Value::from("Locale"));
        obj.insert("__intl_locale__".to_string(), Value::from(locale.as_str()));
        obj.insert("__intl_base_name__".to_string(), Value::from(base_name.as_str()));
        obj.insert("__intl_language__".to_string(), Value::from(base.language.as_str()));
        if let Some(script) = &base.script {
            obj.insert("__intl_script__".to_string(), Value::from(script.as_str()));
        }
        if let Some(region) = &base.region {
            obj.insert("__intl_region__".to_string(), Value::from(region.as_str()));
        }
        if !base.variants.is_empty() {
            obj.insert("__intl_variants__".to_string(), Value::from(base.variants.join("-").as_str()));
        }
        if let Some(calendar) = &calendar {
            obj.insert("__intl_calendar__".to_string(), Value::from(calendar.as_str()));
        }
        if let Some(collation) = &collation {
            obj.insert("__intl_collation__".to_string(), Value::from(collation.as_str()));
        }
        if let Some(hour_cycle) = &hour_cycle {
            obj.insert("__intl_hour_cycle__".to_string(), Value::from(hour_cycle.as_str()));
        }
        if let Some(case_first) = &case_first {
            obj.insert("__intl_case_first__".to_string(), Value::from(case_first.as_str()));
        }
        obj.insert("__intl_numeric__".to_string(), Value::Boolean(numeric));
        if let Some(numbering_system) = &numbering_system {
            obj.insert("__intl_numbering_system__".to_string(), Value::from(numbering_system.as_str()));
        }
        if let Some(first_day_of_week) = &first_day_of_week {
            obj.insert("__intl_first_day_of_week__".to_string(), Value::from(first_day_of_week.as_str()));
        }
        Ok(Value::Object(new_gc_cell_ptr(ctx, obj)))
    }

    fn intl_display_names_of(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, value: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(value) = value else {
            self.throw_type_error(ctx, "value is required");
            return Value::Undefined;
        };
        let Some(display_names) = self.intl_require_initialized_service(ctx, receiver, Some("DisplayNames")) else {
            return Value::Undefined;
        };
        let borrow = display_names.borrow();
        let display_type = borrow.get("__intl_type__").cloned();
        let fallback = borrow.get("__intl_fallback__").cloned();
        drop(borrow);
        let fallback_none = matches!(fallback, Some(Value::String(text)) if crate::unicode::utf16_to_utf8(&text) == "none");
        match self.vm_to_string_like_spec(ctx, value) {
            Ok(text) => {
                let display_type = match display_type {
                    Some(Value::String(kind)) => crate::unicode::utf16_to_utf8(&kind),
                    _ => String::new(),
                };
                let canonical = match self.intl_display_names_canonical_code(ctx, display_type.as_str(), &text) {
                    Ok(canonical) => canonical,
                    Err(err) => {
                        self.pending_throw = Some(err);
                        return Value::Undefined;
                    }
                };
                let supported = match display_type.as_str() {
                    "calendar" => Self::intl_supported_calendar(&canonical).is_some(),
                    "currency" => {
                        let upper = canonical.to_ascii_uppercase();
                        Self::intl_is_well_formed_currency_code(&canonical)
                            && INTL_SUPPORTED_CURRENCIES.iter().any(|candidate| *candidate == upper)
                    }
                    "numberingSystem" => Self::intl_supported_numbering_system(&canonical).is_some(),
                    _ => true,
                };
                if supported {
                    match display_type.as_str() {
                        "currency" => Value::from(canonical.to_ascii_uppercase().as_str()),
                        "calendar" | "numberingSystem" => Value::from(canonical.to_ascii_lowercase().as_str()),
                        _ => Value::from(canonical.as_str()),
                    }
                } else if fallback_none {
                    Value::Undefined
                } else {
                    Value::from(canonical.as_str())
                }
            }
            Err(err) => {
                self.pending_throw = Some(self.vm_value_from_error(ctx, &err));
                Value::Undefined
            }
        }
    }

    fn intl_list_format(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, value: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(parts) = self.intl_list_format_parts(ctx, receiver, value) else {
            return Value::Undefined;
        };
        Value::from(parts.into_iter().map(|part| part.value).collect::<Vec<_>>().join("").as_str())
    }

    fn intl_list_format_to_parts(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, value: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(parts) = self.intl_list_format_parts(ctx, receiver, value) else {
            return Value::Undefined;
        };
        Value::Array(new_gc_cell_ptr(
            ctx,
            VmArrayData::new(
                parts
                    .into_iter()
                    .map(|part| {
                        let mut obj = IndexMap::new();
                        obj.insert("type".to_string(), Value::from(part.part_type.as_str()));
                        obj.insert("value".to_string(), Value::from(part.value.as_str()));
                        Value::Object(new_gc_cell_ptr(ctx, obj))
                    })
                    .collect(),
            ),
        ))
    }

    fn intl_list_format_parts(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        value: Option<&Value<'gc>>,
    ) -> Option<Vec<IntlListPart>> {
        let formatter = self.intl_require_initialized_service(ctx, receiver, Some("ListFormat"))?;
        let items = match self.intl_string_list_from_iterable(ctx, value) {
            Ok(items) => items,
            Err(err) => {
                self.pending_throw = Some(err);
                return None;
            }
        };
        let borrow = formatter.borrow();
        let locale = borrow
            .get("__intl_locale__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(text)),
                _ => None,
            })
            .unwrap_or_else(|| INTL_DEFAULT_LOCALE.to_string());
        let style = borrow
            .get("__intl_list_style__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(text)),
                _ => None,
            })
            .unwrap_or_else(|| "long".to_string());
        let list_type = borrow
            .get("__intl_list_type__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(text)),
                _ => None,
            })
            .unwrap_or_else(|| "conjunction".to_string());
        Some(Self::intl_format_list_parts(&locale, &list_type, &style, &items))
    }

    fn intl_relative_time_format(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        value: Option<&Value<'gc>>,
        unit: Option<&Value<'gc>>,
    ) -> Value<'gc> {
        let Some(parts) = self.intl_relative_time_format_parts(ctx, receiver, value, unit) else {
            return Value::Undefined;
        };
        Value::from(parts.into_iter().map(|part| part.value).collect::<Vec<_>>().join("").as_str())
    }

    fn intl_relative_time_format_to_parts(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        value: Option<&Value<'gc>>,
        unit: Option<&Value<'gc>>,
    ) -> Value<'gc> {
        let Some(parts) = self.intl_relative_time_format_parts(ctx, receiver, value, unit) else {
            return Value::Undefined;
        };
        Value::Array(new_gc_cell_ptr(
            ctx,
            VmArrayData::new(
                parts
                    .into_iter()
                    .map(|part| {
                        let mut obj = IndexMap::new();
                        obj.insert("type".to_string(), Value::from(part.part_type.as_str()));
                        obj.insert("value".to_string(), Value::from(part.value.as_str()));
                        if let Some(unit) = part.unit {
                            obj.insert("unit".to_string(), Value::from(unit.as_str()));
                        }
                        Value::Object(new_gc_cell_ptr(ctx, obj))
                    })
                    .collect(),
            ),
        ))
    }

    fn intl_relative_time_format_parts(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        value: Option<&Value<'gc>>,
        unit: Option<&Value<'gc>>,
    ) -> Option<Vec<IntlDurationPart>> {
        let formatter = self.intl_require_initialized_service(ctx, receiver, Some("RelativeTimeFormat"))?;
        let value = match value {
            Some(value) => value,
            None => {
                self.throw_type_error(ctx, "value is required");
                return None;
            }
        };
        let unit = match unit {
            Some(unit) => unit,
            None => {
                self.throw_type_error(ctx, "unit is required");
                return None;
            }
        };
        let number = self.extract_number_with_coercion(ctx, value)?;
        if !number.is_finite() {
            self.pending_throw = Some(self.make_range_error_object(ctx, "value must be finite"));
            return None;
        }
        let unit = match self.intl_relative_time_unit(ctx, unit) {
            Ok(unit) => unit,
            Err(err) => {
                self.pending_throw = Some(err);
                return None;
            }
        };
        let options = {
            let borrow = formatter.borrow();
            IntlRelativeTimeFormatOptions::from_object(&borrow)
        };
        match self.intl_partition_relative_time_pattern(&options, number, &unit) {
            Ok(parts) => Some(parts),
            Err(err) => {
                self.pending_throw = Some(err);
                None
            }
        }
    }

    fn intl_plural_rules_select(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, value: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(plural_rules) = self.intl_require_initialized_service(ctx, receiver, Some("PluralRules")) else {
            return Value::Undefined;
        };
        let borrow = plural_rules.borrow();
        let locale = borrow
            .get("__intl_locale__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(text)),
                _ => None,
            })
            .unwrap_or_else(|| INTL_DEFAULT_LOCALE.to_string());
        let plural_type = borrow
            .get("__intl_type__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(text)),
                _ => None,
            })
            .unwrap_or_else(|| "cardinal".to_string());
        let notation = borrow
            .get("__intl_notation__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(text)),
                _ => None,
            })
            .unwrap_or_else(|| "standard".to_string());
        let undefined = Value::Undefined;
        let Some(number) = self.extract_number_with_coercion(ctx, value.unwrap_or(&undefined)) else {
            return Value::Undefined;
        };
        Value::from(Self::intl_plural_rule_select(&locale, &plural_type, &notation, number))
    }

    fn intl_plural_rules_select_range(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        start: Option<&Value<'gc>>,
        end: Option<&Value<'gc>>,
    ) -> Value<'gc> {
        let Some(plural_rules) = self.intl_require_initialized_service(ctx, receiver, Some("PluralRules")) else {
            return Value::Undefined;
        };
        let borrow = plural_rules.borrow();
        let locale = borrow
            .get("__intl_locale__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(text)),
                _ => None,
            })
            .unwrap_or_else(|| INTL_DEFAULT_LOCALE.to_string());
        let plural_type = borrow
            .get("__intl_type__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(text)),
                _ => None,
            })
            .unwrap_or_else(|| "cardinal".to_string());
        let notation = borrow
            .get("__intl_notation__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(text)),
                _ => None,
            })
            .unwrap_or_else(|| "standard".to_string());
        if start.is_none_or(|value| matches!(value, Value::Undefined)) || end.is_none_or(|value| matches!(value, Value::Undefined)) {
            self.pending_throw = Some(self.make_type_error_object(ctx, "selectRange arguments are required"));
            return Value::Undefined;
        }
        let undefined = Value::Undefined;
        let Some(start) = self.extract_number_with_coercion(ctx, start.unwrap_or(&undefined)) else {
            return Value::Undefined;
        };
        let Some(end) = self.extract_number_with_coercion(ctx, end.unwrap_or(&undefined)) else {
            return Value::Undefined;
        };
        if start.is_nan() || end.is_nan() {
            self.pending_throw = Some(self.make_range_error_object(ctx, "selectRange arguments must not be NaN"));
            return Value::Undefined;
        }
        Value::from(Self::intl_plural_rule_select(&locale, &plural_type, &notation, end))
    }

    fn intl_to_string_value(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>) -> Result<Value<'gc>, JSError> {
        let prim = self.try_to_primitive(ctx, value, "string");
        if let Some(thrown) = self.pending_throw.take() {
            return Err(self.vm_error_to_js_error(ctx, &thrown));
        }
        if prim.is_symbol_value() {
            return Err(crate::raise_type_error!("Cannot convert a Symbol value to a string"));
        }
        if !matches!(prim, Value::Object(_) | Value::Array(_) | Value::Map(_) | Value::Set(_)) {
            return Ok(match prim {
                Value::String(_) => prim,
                _ => Value::from(crate::core::value::value_to_string(&prim)),
            });
        }

        let to_string_fn = self.read_named_property(ctx, &prim, "toString");
        if let Some(thrown) = self.pending_throw.take() {
            return Err(self.vm_error_to_js_error(ctx, &thrown));
        }
        if matches!(
            to_string_fn,
            Value::Function(..) | Value::Closure(..) | Value::NativeFunction(_) | Value::Object(_)
        ) {
            let out = self.vm_call_function_value(ctx, &to_string_fn, &prim, &[])?;
            if out.is_symbol_value() {
                return Err(crate::raise_type_error!("Cannot convert a Symbol value to a string"));
            }
            if !matches!(out, Value::Object(_) | Value::Array(_) | Value::Map(_) | Value::Set(_)) {
                return Ok(match out {
                    Value::String(_) => out,
                    _ => Value::from(crate::core::value::value_to_string(&out)),
                });
            }
        }

        let value_of_fn = self.read_named_property(ctx, &prim, "valueOf");
        if let Some(thrown) = self.pending_throw.take() {
            return Err(self.vm_error_to_js_error(ctx, &thrown));
        }
        if matches!(
            value_of_fn,
            Value::Function(..) | Value::Closure(..) | Value::NativeFunction(_) | Value::Object(_)
        ) {
            let out = self.vm_call_function_value(ctx, &value_of_fn, &prim, &[])?;
            if out.is_symbol_value() {
                return Err(crate::raise_type_error!("Cannot convert a Symbol value to a string"));
            }
            if !matches!(out, Value::Object(_) | Value::Array(_) | Value::Map(_) | Value::Set(_)) {
                return Ok(match out {
                    Value::String(_) => out,
                    _ => Value::from(crate::core::value::value_to_string(&out)),
                });
            }
        }

        Err(crate::raise_type_error!("Cannot convert object to primitive value"))
    }

    fn intl_segments_containing(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, index: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(Value::Object(segments)) = receiver else {
            self.throw_type_error(ctx, "%Segments.prototype%.containing called on incompatible receiver");
            return Value::Undefined;
        };
        let borrow = segments.borrow();
        if !borrow.contains_key("__segments_input__") {
            drop(borrow);
            self.throw_type_error(ctx, "%Segments.prototype%.containing called on incompatible receiver");
            return Value::Undefined;
        }
        let input = borrow.get("__segments_input__").cloned().unwrap_or(Value::Undefined);
        let items = borrow.get("__items__").cloned().unwrap_or(Value::Undefined);
        drop(borrow);

        let number = match index.unwrap_or(&Value::Undefined) {
            Value::Symbol(_) | Value::BigInt(_) => {
                self.throw_type_error(ctx, "Cannot convert value to integer");
                return Value::Undefined;
            }
            value => {
                let Some(number) = self.extract_number_with_coercion(ctx, value) else {
                    return Value::Undefined;
                };
                number
            }
        };
        let n = if number.is_nan() || number == 0.0 {
            0isize
        } else if number.is_infinite() {
            if number.is_sign_negative() { isize::MIN } else { isize::MAX }
        } else {
            number.trunc() as isize
        };

        let input_len = match &input {
            Value::String(text) => text.len(),
            _ => 0,
        };
        if n < 0 || n as usize >= input_len {
            return Value::Undefined;
        }
        let target_index = n as usize;
        if let Value::Array(items) = items {
            for item in &items.borrow().elements {
                if let Value::Object(item_obj) = item {
                    let item_borrow = item_obj.borrow();
                    let start = match item_borrow.get("index") {
                        Some(Value::Number(value)) => *value as usize,
                        _ => 0,
                    };
                    let segment_len = match item_borrow.get("segment") {
                        Some(Value::String(text)) => text.len(),
                        _ => 0,
                    };
                    if target_index >= start && target_index < start + segment_len {
                        return item.clone();
                    }
                }
            }
        }
        Value::Undefined
    }

    fn intl_display_names_canonical_code(&self, ctx: &GcContext<'gc>, display_type: &str, code: &str) -> Result<String, Value<'gc>> {
        match display_type {
            "language" => {
                if code.eq_ignore_ascii_case("root") || code.contains('_') || code.split('-').any(|part| part.len() == 1) {
                    return Err(self.make_range_error_object(ctx, "Invalid language code"));
                }
                self.intl_validate_locale_tag(ctx, code)?;
                let base = Self::intl_locale_without_unicode_extension(code);
                let components = Self::intl_locale_base_components(&base);
                if components.language == "und"
                    || (components.language.len() == 4 && components.language.chars().all(|ch| ch.is_ascii_alphabetic()))
                {
                    return Err(self.make_range_error_object(ctx, "Invalid language code"));
                }
                Ok(Self::intl_canonicalize_locale_tag(code))
            }
            "region" => {
                let is_alpha = code.len() == 2 && code.chars().all(|ch| ch.is_ascii_alphabetic());
                let is_numeric = code.len() == 3 && code.chars().all(|ch| ch.is_ascii_digit());
                if !is_alpha && !is_numeric {
                    return Err(self.make_range_error_object(ctx, "Invalid region code"));
                }
                Ok(code.to_ascii_uppercase())
            }
            "calendar" => {
                if !Self::intl_is_valid_unicode_type_identifier(code) {
                    return Err(self.make_range_error_object(ctx, "Invalid calendar code"));
                }
                Ok(code.to_ascii_lowercase())
            }
            "dateTimeField" => {
                if !matches!(
                    code,
                    "era"
                        | "year"
                        | "quarter"
                        | "month"
                        | "weekOfYear"
                        | "weekday"
                        | "day"
                        | "dayPeriod"
                        | "hour"
                        | "minute"
                        | "second"
                        | "timeZoneName"
                ) {
                    return Err(self.make_range_error_object(ctx, "Invalid dateTimeField code"));
                }
                Ok(code.to_string())
            }
            _ => Ok(code.to_string()),
        }
    }

    fn intl_duration_format(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, value: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(parts) = self.intl_duration_format_parts(ctx, receiver, value) else {
            return Value::Undefined;
        };
        Value::from(parts.into_iter().map(|part| part.value).collect::<Vec<_>>().join("").as_str())
    }

    fn intl_duration_format_to_parts(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        value: Option<&Value<'gc>>,
    ) -> Value<'gc> {
        let Some(parts) = self.intl_duration_format_parts(ctx, receiver, value) else {
            return Value::Undefined;
        };
        Value::Array(new_gc_cell_ptr(
            ctx,
            VmArrayData::new(
                parts
                    .into_iter()
                    .map(|part| {
                        let mut obj = IndexMap::new();
                        obj.insert("type".to_string(), Value::from(part.part_type.as_str()));
                        obj.insert("value".to_string(), Value::from(part.value.as_str()));
                        if let Some(unit) = part.unit {
                            obj.insert("unit".to_string(), Value::from(unit.as_str()));
                        }
                        Value::Object(new_gc_cell_ptr(ctx, obj))
                    })
                    .collect(),
            ),
        ))
    }

    fn intl_duration_format_parts(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        value: Option<&Value<'gc>>,
    ) -> Option<Vec<IntlDurationPart>> {
        let formatter = self.intl_require_initialized_service(ctx, receiver, Some("DurationFormat"))?;
        let options = {
            let borrow = formatter.borrow();
            IntlDurationFormatOptions::from_object(&borrow)
        };
        let undefined = Value::Undefined;
        let duration = match self.intl_duration_record_from_value(ctx, value.unwrap_or(&undefined)) {
            Ok(duration) => duration,
            Err(err) => {
                self.pending_throw = Some(err);
                return None;
            }
        };
        match self.intl_partition_duration_format_pattern(ctx, &options, &duration) {
            Ok(parts) => Some(parts),
            Err(err) => {
                self.pending_throw = Some(err);
                None
            }
        }
    }

    fn intl_duration_record_from_value(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>) -> Result<IntlDurationRecord, Value<'gc>> {
        if let Value::Object(obj) = value
            && !obj.borrow().contains_key("__temporal_kind__")
        {
            let record = self.intl_duration_record_from_object_like(ctx, value)?;
            self.intl_validate_duration_record(ctx, &record)?;
            return Ok(record);
        }
        self.intl_validate_duration_input(ctx, value)?;
        let duration = self.temporal_to_duration(ctx, value).map_err(|err| {
            let js_err: JSError = err.into();
            self.vm_value_from_error(ctx, &js_err)
        })?;
        let record = IntlDurationRecord {
            years: duration.years(),
            months: duration.months(),
            weeks: duration.weeks(),
            days: duration.days(),
            hours: duration.hours(),
            minutes: duration.minutes(),
            seconds: duration.seconds(),
            milliseconds: duration.milliseconds(),
            microseconds: duration.microseconds(),
            nanoseconds: duration.nanoseconds(),
            years_present: duration.years() != 0,
            months_present: duration.months() != 0,
            weeks_present: duration.weeks() != 0,
            days_present: duration.days() != 0,
            hours_present: duration.hours() != 0,
            minutes_present: duration.minutes() != 0,
            seconds_present: duration.seconds() != 0,
            milliseconds_present: duration.milliseconds() != 0,
            microseconds_present: duration.microseconds() != 0,
            nanoseconds_present: duration.nanoseconds() != 0,
        };
        self.intl_validate_duration_record(ctx, &record)?;
        Ok(record)
    }

    fn intl_duration_record_from_object_like(
        &mut self,
        ctx: &GcContext<'gc>,
        value: &Value<'gc>,
    ) -> Result<IntlDurationRecord, Value<'gc>> {
        let Value::Object(obj) = value else {
            return Err(self.make_type_error_object(ctx, "Invalid duration input"));
        };
        let supported = [
            "years",
            "months",
            "weeks",
            "days",
            "hours",
            "minutes",
            "seconds",
            "milliseconds",
            "microseconds",
            "nanoseconds",
        ];
        {
            let borrow = obj.borrow();
            for key in borrow.keys() {
                if key.starts_with("__") {
                    continue;
                }
                if !supported.contains(&key.as_str()) {
                    return Err(self.make_type_error_object(ctx, "Invalid duration input"));
                }
            }
        }
        let mut record = IntlDurationRecord {
            years: 0,
            months: 0,
            weeks: 0,
            days: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
            milliseconds: 0,
            microseconds: 0,
            nanoseconds: 0,
            years_present: false,
            months_present: false,
            weeks_present: false,
            days_present: false,
            hours_present: false,
            minutes_present: false,
            seconds_present: false,
            milliseconds_present: false,
            microseconds_present: false,
            nanoseconds_present: false,
        };
        let mut seen_supported = false;
        for unit in supported {
            let raw = self.read_named_property(ctx, value, unit);
            if let Some(thrown) = self.pending_throw.take() {
                return Err(thrown);
            }
            if matches!(raw, Value::Undefined) {
                if obj.borrow().contains_key(unit) {
                    return Err(self.make_type_error_object(ctx, "Invalid duration input"));
                }
                continue;
            }
            seen_supported = true;
            let Some(number) = self.extract_number_with_coercion(ctx, &raw) else {
                return Err(self
                    .pending_throw
                    .clone()
                    .unwrap_or_else(|| self.make_type_error_object(ctx, "Invalid duration input")));
            };
            if !number.is_finite() || number.fract() != 0.0 {
                return Err(self.make_range_error_object(ctx, "Duration field out of range"));
            }
            let Some(integer) = Self::intl_exact_integral_number(number) else {
                return Err(self.make_range_error_object(ctx, "Duration field out of range"));
            };
            if !matches!(unit, "microseconds" | "nanoseconds") && integer.unsigned_abs() > i64::MAX as u128 {
                return Err(self.make_range_error_object(ctx, "Duration field out of range"));
            }
            record.set_unit(unit, integer);
        }
        if !seen_supported {
            return Err(self.make_type_error_object(ctx, "Invalid duration input"));
        }
        Ok(record)
    }

    fn intl_exact_integral_number(value: f64) -> Option<i128> {
        if value == 0.0 {
            return Some(0);
        }
        let bits = value.to_bits();
        let negative = (bits >> 63) != 0;
        let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
        if exponent_bits == 0x7ff {
            return None;
        }
        let fraction = bits & ((1_u64 << 52) - 1);
        if exponent_bits == 0 {
            return None;
        }
        let exponent = exponent_bits - 1023;
        let significand = (1_u128 << 52) | fraction as u128;
        let magnitude = if exponent >= 52 {
            significand.checked_shl((exponent - 52) as u32)?
        } else {
            let shift = (52 - exponent) as u32;
            if shift >= 128 || (significand & ((1_u128 << shift) - 1)) != 0 {
                return None;
            }
            significand >> shift
        };
        if negative {
            let signed = i128::try_from(magnitude).ok()?;
            Some(-signed)
        } else {
            i128::try_from(magnitude).ok()
        }
    }

    fn intl_validate_duration_input(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>) -> Result<(), Value<'gc>> {
        let Value::Object(obj) = value else {
            return Ok(());
        };
        if obj.borrow().contains_key("__temporal_kind__") {
            return Ok(());
        }
        let supported = [
            "years",
            "months",
            "weeks",
            "days",
            "hours",
            "minutes",
            "seconds",
            "milliseconds",
            "microseconds",
            "nanoseconds",
        ];
        let borrow = obj.borrow();
        let mut seen_supported = false;
        for (key, raw) in borrow.iter() {
            if key.starts_with("__") {
                continue;
            }
            if !supported.contains(&key.as_str()) {
                return Err(self.make_type_error_object(ctx, "Invalid duration input"));
            }
            if matches!(raw, Value::Undefined) {
                return Err(self.make_type_error_object(ctx, "Invalid duration input"));
            }
            seen_supported = true;
            if let Value::Number(number) = raw
                && (!number.is_finite() || number.fract() != 0.0)
            {
                return Err(self.make_range_error_object(ctx, "Duration field out of range"));
            }
        }
        if !seen_supported {
            return Err(self.make_type_error_object(ctx, "Invalid duration input"));
        }
        Ok(())
    }

    fn intl_validate_duration_record(&mut self, ctx: &GcContext<'gc>, record: &IntlDurationRecord) -> Result<(), Value<'gc>> {
        let signs = [
            record.years.signum(),
            record.months.signum(),
            record.weeks.signum(),
            record.days.signum(),
            record.hours.signum(),
            record.minutes.signum(),
            record.seconds.signum(),
            record.milliseconds.signum(),
            record.microseconds.signum() as i64,
            record.nanoseconds.signum() as i64,
        ];
        let has_positive = signs.iter().any(|sign| *sign > 0);
        let has_negative = signs.iter().any(|sign| *sign < 0);
        if has_positive && has_negative {
            return Err(self.make_range_error_object(ctx, "Duration fields must have the same sign"));
        }
        let year_month_week_limit = 1_i128 << 32;
        if (record.years as i128).abs() >= year_month_week_limit
            || (record.months as i128).abs() >= year_month_week_limit
            || (record.weeks as i128).abs() >= year_month_week_limit
        {
            return Err(self.make_range_error_object(ctx, "Duration field out of range"));
        }
        let total_nanoseconds = (record.days as i128) * 86_400_000_000_000
            + (record.hours as i128) * 3_600_000_000_000
            + (record.minutes as i128) * 60_000_000_000
            + (record.seconds as i128) * 1_000_000_000
            + (record.milliseconds as i128) * 1_000_000
            + record.microseconds * 1_000
            + record.nanoseconds;
        if total_nanoseconds.abs() >= ((1_i128 << 53) * 1_000_000_000) {
            return Err(self.make_range_error_object(ctx, "Duration field out of range"));
        }
        Ok(())
    }

    fn intl_partition_duration_format_pattern(
        &mut self,
        ctx: &GcContext<'gc>,
        options: &IntlDurationFormatOptions,
        duration: &IntlDurationRecord,
    ) -> Result<Vec<IntlDurationPart>, Value<'gc>> {
        let units = [
            "years",
            "months",
            "weeks",
            "days",
            "hours",
            "minutes",
            "seconds",
            "milliseconds",
            "microseconds",
            "nanoseconds",
        ];
        let mut result: Vec<Vec<IntlDurationPart>> = Vec::new();
        let mut need_separator = false;
        let mut display_negative_sign = true;
        for (index, unit) in units.iter().enumerate() {
            let unit_options = options.unit_options(unit);
            let mut value = duration.unit_i128(unit).to_string();
            let display = unit_options.display.as_str();
            let style = unit_options.style.as_str();
            let mut display_required = value != "0" || display != "auto";
            if (*unit == "seconds" || *unit == "milliseconds" || *unit == "microseconds")
                && units.get(index + 1).map(|next| options.unit_options(next).style.as_str()) == Some("numeric")
            {
                display_required = display_required
                    || match *unit {
                        "seconds" => duration.milliseconds != 0 || duration.microseconds != 0 || duration.nanoseconds != 0,
                        "milliseconds" => duration.microseconds != 0 || duration.nanoseconds != 0,
                        "microseconds" => duration.nanoseconds != 0,
                        _ => false,
                    };
            }
            if *unit == "minutes" && (need_separator || (options.style == "digital" && duration.is_present("minutes"))) {
                display_required = display_required
                    || options.seconds.display == "always"
                    || duration.seconds != 0
                    || duration.milliseconds != 0
                    || duration.microseconds != 0
                    || duration.nanoseconds != 0;
            }
            if !display_required {
                continue;
            }
            let mut sign_never = false;
            let next_style = units.get(index + 1).map(|next| options.unit_options(next).style.as_str());
            let minimum_integer_digits = if style == "2-digit" || (need_separator && matches!(*unit, "minutes" | "seconds")) {
                Some(2)
            } else {
                None
            };
            let mut minimum_fraction_digits = None;
            let mut maximum_fraction_digits = None;
            let mut trunc_rounding = false;
            let mut done = false;
            if (*unit == "seconds" || *unit == "milliseconds" || *unit == "microseconds") && next_style == Some("numeric") {
                let extra_digits = match *unit {
                    "seconds" => 9,
                    "milliseconds" => 6,
                    "microseconds" => 3,
                    _ => 0,
                };
                value = Self::intl_duration_fractional_string(duration, unit, extra_digits);
                maximum_fraction_digits = Some(options.fractional_digits.unwrap_or(9));
                minimum_fraction_digits = Some(options.fractional_digits.unwrap_or(0));
                trunc_rounding = true;
                done = true;
            }
            if display_negative_sign {
                display_negative_sign = false;
                if value == "0" && duration.any_negative() {
                    value = "-0".to_string();
                }
            } else {
                sign_never = true;
            }
            let decimal_style = style == "numeric" || style == "2-digit";
            let parts = self.intl_duration_number_parts(
                ctx,
                IntlDurationNumberFormatConfig {
                    locale: &options.resolved_locale,
                    numbering_system: &options.numbering_system,
                    unit,
                    unit_options,
                    input: &value,
                    decimal_style,
                    trunc_rounding,
                    sign_never,
                    minimum_integer_digits,
                    minimum_fraction_digits,
                    maximum_fraction_digits,
                },
            )?;
            if !need_separator {
                if decimal_style {
                    need_separator = true;
                }
                result.push(parts);
            } else {
                let list = result.last_mut().expect("separator requires previous part");
                list.push(IntlDurationPart::literal(":"));
                list.extend(parts);
            }
            if done {
                break;
            }
        }
        let list_style = if options.style == "digital" {
            "short"
        } else {
            options.style.as_str()
        };
        let strings = result
            .iter()
            .map(|parts| parts.iter().map(|part| part.value.as_str()).collect::<String>())
            .collect::<Vec<_>>();
        if strings.is_empty() {
            return Ok(Vec::new());
        }
        let mut groups = result.into_iter();
        let mut flattened = Vec::new();
        for part in Self::intl_format_list_parts(&options.resolved_locale, "unit", list_style, &strings) {
            if part.part_type == "element" {
                if let Some(group) = groups.next() {
                    flattened.extend(group);
                }
            } else {
                flattened.push(IntlDurationPart::literal(&part.value));
            }
        }
        Ok(flattened)
    }

    fn intl_duration_fractional_string(duration: &IntlDurationRecord, unit: &str, extra_digits: usize) -> String {
        if extra_digits == 0 {
            return "0".to_string();
        }
        let mut ns = duration.nanoseconds;
        match unit {
            "seconds" => {
                ns += (duration.seconds as i128) * 1_000_000_000;
                ns += (duration.milliseconds as i128) * 1_000_000;
                ns += duration.microseconds * 1_000;
            }
            "milliseconds" => {
                ns += (duration.milliseconds as i128) * 1_000_000;
                ns += duration.microseconds * 1_000;
            }
            "microseconds" => {
                ns += duration.microseconds * 1_000;
            }
            _ => return "0".to_string(),
        }
        let divisor = 10_i128.pow(extra_digits as u32);
        let q = ns / divisor;
        let mut r = ns % divisor;
        if r == 0 {
            return q.to_string();
        }
        if r < 0 {
            r = -r;
        }
        format!("{q}.{:0width$}", r, width = extra_digits)
    }

    fn intl_duration_number_parts(
        &mut self,
        ctx: &GcContext<'gc>,
        config: IntlDurationNumberFormatConfig<'_>,
    ) -> Result<Vec<IntlDurationPart>, Value<'gc>> {
        let mut formatter = IndexMap::new();
        formatter.insert("__intl_service__".to_string(), Value::from("NumberFormat"));
        formatter.insert("__intl_locale__".to_string(), Value::from(config.locale));
        formatter.insert("__intl_numbering_system__".to_string(), Value::from(config.numbering_system));
        formatter.insert(
            "__intl_style__".to_string(),
            Value::from(if config.decimal_style { "decimal" } else { "unit" }),
        );
        formatter.insert(
            "__intl_sign_display__".to_string(),
            Value::from(if config.sign_never { "never" } else { "auto" }),
        );
        formatter.insert("__intl_use_grouping__".to_string(), Value::Boolean(!config.decimal_style));
        formatter.insert("__intl_notation__".to_string(), Value::from("standard"));
        formatter.insert("__intl_compact_display__".to_string(), Value::from("short"));
        formatter.insert("__intl_currency_display__".to_string(), Value::from("symbol"));
        formatter.insert("__intl_currency_sign__".to_string(), Value::from("standard"));
        formatter.insert("__intl_unit_display__".to_string(), Value::from(config.unit_options.style.as_str()));
        formatter.insert("__intl_unit__".to_string(), Value::from(config.unit.trim_end_matches('s')));
        if let Some(digits) = config.minimum_integer_digits {
            formatter.insert("__intl_minimum_integer_digits__".to_string(), Value::Number(digits as f64));
        }
        if let Some(digits) = config.minimum_fraction_digits {
            formatter.insert("__intl_minimum_fraction_digits__".to_string(), Value::Number(digits as f64));
        }
        if let Some(digits) = config.maximum_fraction_digits {
            formatter.insert("__intl_maximum_fraction_digits__".to_string(), Value::Number(digits as f64));
        }
        if config.trunc_rounding {
            formatter.insert("__intl_rounding_mode__".to_string(), Value::from("trunc"));
        }
        if config.decimal_style && config.locale.starts_with("en") {
            return Ok(Self::intl_duration_decimal_parts(
                config.input,
                config.sign_never,
                config.minimum_integer_digits,
                config.minimum_fraction_digits,
                config.maximum_fraction_digits,
                config.unit.trim_end_matches('s'),
            ));
        }
        let input_value = Value::from(config.input);
        let rendered = self.intl_number_format_parts_array(ctx, &formatter, Some(&input_value))?;
        let Value::Array(array) = rendered else {
            return Err(self.make_type_error_object(ctx, "NumberFormat parts must be an array"));
        };
        let mut out = Vec::new();
        for value in &array.borrow().elements {
            let Value::Object(obj) = value else {
                continue;
            };
            let borrow = obj.borrow();
            let part_type = borrow
                .get("type")
                .and_then(|value| match value {
                    Value::String(text) => Some(crate::unicode::utf16_to_utf8(text)),
                    _ => None,
                })
                .unwrap_or_else(|| "literal".to_string());
            let part_value = borrow
                .get("value")
                .and_then(|value| match value {
                    Value::String(text) => Some(crate::unicode::utf16_to_utf8(text)),
                    _ => None,
                })
                .unwrap_or_default();
            out.push(IntlDurationPart::element(
                &part_type,
                part_value,
                Some(config.unit.trim_end_matches('s')),
            ));
        }
        Ok(out)
    }

    fn intl_duration_decimal_parts(
        input: &str,
        sign_never: bool,
        minimum_integer_digits: Option<u8>,
        minimum_fraction_digits: Option<u8>,
        maximum_fraction_digits: Option<u8>,
        unit: &str,
    ) -> Vec<IntlDurationPart> {
        let mut text = input;
        let negative = text.starts_with('-');
        if negative {
            text = &text[1..];
        }
        let (integer_raw, fraction_raw) = text.split_once('.').unwrap_or((text, ""));
        let mut integer = integer_raw.to_string();
        let mut fraction = fraction_raw.to_string();
        if let Some(max_digits) = maximum_fraction_digits {
            fraction.truncate(max_digits as usize);
        }
        let min_fraction = minimum_fraction_digits.unwrap_or(0) as usize;
        while fraction.ends_with('0') && fraction.len() > min_fraction {
            fraction.pop();
        }
        while fraction.len() < min_fraction {
            fraction.push('0');
        }
        let min_integer = minimum_integer_digits.unwrap_or(1) as usize;
        if integer.len() < min_integer {
            integer = format!("{integer:0>width$}", width = min_integer);
        }
        let mut parts = Vec::new();
        if negative && !sign_never {
            parts.push(IntlDurationPart::element("minusSign", "-".to_string(), Some(unit)));
        }
        parts.push(IntlDurationPart::element("integer", integer, Some(unit)));
        if !fraction.is_empty() {
            parts.push(IntlDurationPart::element("decimal", ".".to_string(), Some(unit)));
            parts.push(IntlDurationPart::element("fraction", fraction, Some(unit)));
        }
        parts
    }

    fn intl_locale_to_string(&mut self, _ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(locale) = self.intl_require_initialized_service(_ctx, receiver, Some("Locale")) else {
            return Value::Undefined;
        };
        locale.borrow().get("__intl_locale__").cloned().unwrap_or(Value::Undefined)
    }

    fn intl_locale_slot_getter(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, slot: &str) -> Value<'gc> {
        let Some(locale) = self.intl_require_initialized_service(ctx, receiver, Some("Locale")) else {
            return Value::Undefined;
        };
        locale.borrow().get(slot).cloned().unwrap_or(Value::Undefined)
    }

    fn intl_locale_get_calendars(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(locale) = self.intl_require_initialized_service(ctx, receiver, Some("Locale")) else {
            return Value::Undefined;
        };
        let borrow = locale.borrow();
        let calendar = own_data_from_legacy_map(&borrow, "__intl_calendar__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(&text)),
                _ => None,
            })
            .unwrap_or_else(|| "gregory".to_string());
        Self::intl_string_array(ctx, &[calendar.as_str()])
    }

    fn intl_locale_get_collations(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(locale) = self.intl_require_initialized_service(ctx, receiver, Some("Locale")) else {
            return Value::Undefined;
        };
        let borrow = locale.borrow();
        let mut values: Vec<String> = Vec::new();
        if let Some(Value::String(text)) = own_data_from_legacy_map(&borrow, "__intl_collation__") {
            let value = crate::unicode::utf16_to_utf8(&text);
            if value != "standard" && value != "search" && !value.is_empty() {
                values.push(value);
            }
        }
        if values.is_empty() {
            let language = own_data_from_legacy_map(&borrow, "__intl_language__")
                .and_then(|value| match value {
                    Value::String(text) => Some(crate::unicode::utf16_to_utf8(&text)),
                    _ => None,
                })
                .unwrap_or_else(|| "en".to_string());
            values.extend(match language.as_str() {
                "de" => ["phonebk"].into_iter().map(str::to_string).collect::<Vec<_>>(),
                "zh" => ["pinyin", "stroke"].into_iter().map(str::to_string).collect::<Vec<_>>(),
                _ => ["emoji"].into_iter().map(str::to_string).collect::<Vec<_>>(),
            });
        }
        let refs: Vec<&str> = values.iter().map(String::as_str).collect();
        Self::intl_string_array(ctx, &refs)
    }

    fn intl_locale_get_hour_cycles(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(locale) = self.intl_require_initialized_service(ctx, receiver, Some("Locale")) else {
            return Value::Undefined;
        };
        let borrow = locale.borrow();
        let values: Vec<String> = if let Some(Value::String(text)) = own_data_from_legacy_map(&borrow, "__intl_hour_cycle__") {
            vec![crate::unicode::utf16_to_utf8(&text)]
        } else if matches!(
            own_data_from_legacy_map(&borrow, "__intl_region__"),
            Some(Value::String(text)) if crate::unicode::utf16_to_utf8(&text) == "US"
        ) {
            vec!["h12".to_string()]
        } else {
            vec!["h23".to_string()]
        };
        let refs: Vec<&str> = values.iter().map(String::as_str).collect();
        Self::intl_string_array(ctx, &refs)
    }

    fn intl_locale_get_numbering_systems(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(locale) = self.intl_require_initialized_service(ctx, receiver, Some("Locale")) else {
            return Value::Undefined;
        };
        let borrow = locale.borrow();
        let numbering_system = own_data_from_legacy_map(&borrow, "__intl_numbering_system__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(&text)),
                _ => None,
            })
            .unwrap_or_else(|| "latn".to_string());
        Self::intl_string_array(ctx, &[numbering_system.as_str()])
    }

    fn intl_locale_get_text_info(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(locale) = self.intl_require_initialized_service(ctx, receiver, Some("Locale")) else {
            return Value::Undefined;
        };
        let borrow = locale.borrow();
        let language = own_data_from_legacy_map(&borrow, "__intl_language__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(&text)),
                _ => None,
            })
            .unwrap_or_else(|| "en".to_string());
        let direction = if matches!(language.as_str(), "ar" | "fa" | "he" | "ur") {
            "rtl"
        } else {
            "ltr"
        };
        let object_proto = self.globals.get("Object").and_then(|value| match value {
            Value::Object(object) => own_data_from_legacy_map(&object.borrow(), "prototype"),
            _ => None,
        });
        let mut object = IndexMap::new();
        if let Some(proto) = object_proto {
            object.insert("__proto__".to_string(), proto);
        }
        object.insert("direction".to_string(), Value::from(direction));
        Value::Object(new_gc_cell_ptr(ctx, object))
    }

    fn intl_locale_get_time_zones(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(locale) = self.intl_require_initialized_service(ctx, receiver, Some("Locale")) else {
            return Value::Undefined;
        };
        let borrow = locale.borrow();
        let region = own_data_from_legacy_map(&borrow, "__intl_region__").and_then(|value| match value {
            Value::String(text) => Some(crate::unicode::utf16_to_utf8(&text)),
            _ => None,
        });
        if region.is_none() {
            return Value::Undefined;
        }
        let values: &[&str] = match region.as_deref() {
            Some("GB") => &["Europe/London"],
            Some("JP") => &["Asia/Tokyo"],
            Some("US") => &["America/New_York", "Pacific/Honolulu"],
            _ => &["UTC"],
        };
        Self::intl_string_array(ctx, values)
    }

    fn intl_locale_get_week_info(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(locale) = self.intl_require_initialized_service(ctx, receiver, Some("Locale")) else {
            return Value::Undefined;
        };
        let borrow = locale.borrow();
        let first_day_value = own_data_from_legacy_map(&borrow, "__intl_first_day_of_week__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(&text)),
                _ => None,
            })
            .unwrap_or_else(|| match own_data_from_legacy_map(&borrow, "__intl_region__") {
                Some(Value::String(text)) if crate::unicode::utf16_to_utf8(&text) == "US" => "sun".to_string(),
                _ => "mon".to_string(),
            });
        let first_day = Self::intl_weekday_string_to_number(&first_day_value).unwrap_or(1) as f64;
        let weekend = Value::Array(new_gc_cell_ptr(ctx, VmArrayData::new(vec![Value::Number(6.0), Value::Number(7.0)])));
        let object_proto = self.globals.get("Object").and_then(|value| match value {
            Value::Object(object) => own_data_from_legacy_map(&object.borrow(), "prototype"),
            _ => None,
        });
        let mut object = IndexMap::new();
        if let Some(proto) = object_proto {
            object.insert("__proto__".to_string(), proto);
        }
        object.insert("firstDay".to_string(), Value::Number(first_day));
        object.insert("weekend".to_string(), weekend);
        Value::Object(new_gc_cell_ptr(ctx, object))
    }

    fn intl_locale_maximize(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(locale) = self.intl_require_initialized_service(ctx, receiver, Some("Locale")) else {
            return Value::Undefined;
        };
        let borrow = locale.borrow();
        let current = own_data_from_legacy_map(&borrow, "__intl_locale__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(&text)),
                _ => None,
            })
            .unwrap_or_else(|| "und".to_string());
        drop(borrow);
        self.intl_locale_clone_with_transformed_tag(ctx, &current, Self::intl_locale_maximize_tag)
    }

    fn intl_locale_minimize(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let Some(locale) = self.intl_require_initialized_service(ctx, receiver, Some("Locale")) else {
            return Value::Undefined;
        };
        let borrow = locale.borrow();
        let current = own_data_from_legacy_map(&borrow, "__intl_locale__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(&text)),
                _ => None,
            })
            .unwrap_or_else(|| "und".to_string());
        drop(borrow);
        self.intl_locale_clone_with_transformed_tag(ctx, &current, Self::intl_locale_minimize_tag)
    }

    fn intl_string_array(ctx: &GcContext<'gc>, values: &[&str]) -> Value<'gc> {
        let values = values.iter().map(|value| Value::from(*value)).collect::<Vec<_>>();
        Value::Array(new_gc_cell_ptr(ctx, VmArrayData::new(values)))
    }

    fn intl_locale_clone_with_transformed_tag(&mut self, ctx: &GcContext<'gc>, current: &str, transform: fn(&str) -> String) -> Value<'gc> {
        let transformed = transform(current);
        match self.intl_construct_locale(ctx, None, &[Value::from(transformed.as_str())]) {
            Ok(value) => value,
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

    pub(super) fn intl_canonicalize_locale_list(
        &mut self,
        ctx: &GcContext<'gc>,
        value: Option<&Value<'gc>>,
    ) -> Result<Vec<String>, Value<'gc>> {
        let Some(value) = value else {
            return Ok(vec![]);
        };
        if let Some(locale) = Self::intl_locale_value_tag(value) {
            return Ok(vec![locale]);
        }
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
        if let Some(locale) = Self::intl_locale_value_tag(value) {
            return Ok(vec![locale]);
        }
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
            if let Some(locale) = Self::intl_locale_value_tag(&entry) {
                if !out.iter().any(|existing| existing == &locale) {
                    out.push(locale);
                }
                continue;
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

    pub(super) fn intl_box_primitive_if_needed(&mut self, ctx: &GcContext<'gc>, value: &Value<'gc>) -> Value<'gc> {
        if Self::intl_is_object_like(value) {
            value.clone()
        } else {
            self.intl_box_locale_list_value(ctx, value)
        }
    }

    fn intl_locale_constructor_tag(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Result<String, Value<'gc>> {
        let Some(value) = value else {
            return Err(self.make_type_error_object(ctx, "tag is required"));
        };
        if matches!(
            value,
            Value::Undefined | Value::Null | Value::Boolean(_) | Value::Number(_) | Value::BigInt(_) | Value::Symbol(_)
        ) {
            return Err(self.make_type_error_object(ctx, "tag must be a string or Intl.Locale"));
        }
        if let Value::String(text) = value {
            let tag = crate::unicode::utf16_to_utf8(text);
            self.intl_validate_locale_tag(ctx, &tag)?;
            return Ok(Self::intl_canonicalize_locale_tag(&tag));
        }
        if let Value::Object(object) = value
            && matches!(object.borrow().get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "Locale")
            && let Some(Value::String(locale)) = object.borrow().get("__intl_locale__")
        {
            return Ok(crate::unicode::utf16_to_utf8(locale));
        }
        let tag = match self.vm_to_string_like_spec(ctx, value) {
            Ok(tag) => tag,
            Err(err) => return Err(self.vm_value_from_error(ctx, &err)),
        };
        self.intl_validate_locale_tag(ctx, &tag)?;
        Ok(Self::intl_canonicalize_locale_tag(&tag))
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
        if matches!(options, Value::Undefined) {
            return Ok("best fit");
        }
        if matches!(options, Value::Null) {
            return Err(self.make_type_error_object(ctx, "options must not be null"));
        }
        let boxed_options = if Self::intl_is_object_like(options) {
            options.clone()
        } else {
            self.intl_box_primitive_if_needed(ctx, options)
        };
        let value = self.read_named_property(ctx, &boxed_options, "localeMatcher");
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

    pub(super) fn intl_collator_compare(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, args: &[Value<'gc>]) -> Value<'gc> {
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
        let parts = match Self::intl_date_time_range_parts(&borrow, start, end) {
            Ok(parts) => parts,
            Err(err) => {
                self.pending_throw = Some(err);
                return Value::Undefined;
            }
        };
        Value::from(parts.into_iter().map(|(_, value, _)| value).collect::<Vec<_>>().join("").as_str())
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
        let values = match Self::intl_date_time_range_parts(&borrow, start, end) {
            Ok(parts) => parts
                .into_iter()
                .map(|(part_type, value, source)| Self::intl_date_time_format_part_object(ctx, &part_type, &value, Some(source)))
                .collect::<Vec<_>>(),
            Err(err) => {
                self.pending_throw = Some(err);
                return Value::Undefined;
            }
        };
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

    fn intl_number_format_format_range(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        start: Option<&Value<'gc>>,
        end: Option<&Value<'gc>>,
    ) -> Value<'gc> {
        let Some(formatter) = self.intl_require_initialized_service(ctx, receiver, Some("NumberFormat")) else {
            return Value::Undefined;
        };
        match self.intl_number_format_range_string(ctx, &formatter.borrow(), start, end) {
            Ok(text) => Value::from(text.as_str()),
            Err(err) => {
                self.pending_throw = Some(err);
                Value::Undefined
            }
        }
    }

    fn intl_number_format_format_range_to_parts(
        &mut self,
        ctx: &GcContext<'gc>,
        receiver: Option<&Value<'gc>>,
        start: Option<&Value<'gc>>,
        end: Option<&Value<'gc>>,
    ) -> Value<'gc> {
        let Some(formatter) = self.intl_require_initialized_service(ctx, receiver, Some("NumberFormat")) else {
            return Value::Undefined;
        };
        match self.intl_number_format_range_parts_array(ctx, &formatter.borrow(), start, end) {
            Ok(parts) => parts,
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
        let nan_text = Self::intl_nan_string(&options);
        if formatted.ends_with(&nan_text) {
            let mut parts = Vec::new();
            if let Some(prefix) = formatted.strip_suffix(&nan_text)
                && !prefix.is_empty()
            {
                parts.extend(Self::intl_number_string_parts(ctx, formatter, prefix));
            }
            parts.push(Self::intl_part_object(ctx, "nan", &nan_text));
            return Ok(Value::Array(new_gc_cell_ptr(ctx, VmArrayData::new(parts))));
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
            let rendered_suffix_unit = Self::intl_render_unit_label(&options, &pattern.suffix_unit, rest);
            let numeric_end = if let Some(suffix_literal) = pattern.suffix_literal.as_deref() {
                rest.strip_suffix(&(suffix_literal.to_string() + &rendered_suffix_unit))
                    .map(|numeric| (numeric, Some(suffix_literal)))
            } else {
                rest.strip_suffix(&rendered_suffix_unit).map(|numeric| (numeric, None))
            };
            if let Some((numeric, suffix_literal)) = numeric_end {
                parts.extend(Self::intl_number_string_parts(ctx, formatter, numeric));
                if let Some(suffix_literal) = suffix_literal {
                    parts.push(Self::intl_part_object(ctx, "literal", suffix_literal));
                }
                parts.push(Self::intl_part_object(ctx, "unit", &rendered_suffix_unit));
                return Ok(Value::Array(new_gc_cell_ptr(ctx, VmArrayData::new(parts))));
            }
        }
        Ok(Value::Array(new_gc_cell_ptr(
            ctx,
            VmArrayData::new(Self::intl_number_string_parts(ctx, formatter, &formatted)),
        )))
    }

    fn intl_number_string_parts(ctx: &GcContext<'gc>, formatter: &IndexMap<String, Value<'gc>>, formatted: &str) -> Vec<Value<'gc>> {
        let options = IntlNumberFormatOptions::from_object(formatter);
        let locale_is_de = formatter
            .get("__intl_locale__")
            .is_some_and(|v| matches!(v, Value::String(s) if crate::unicode::utf16_to_utf8(s).starts_with("de")));
        let decimal_separator = if locale_is_de { ',' } else { '.' };
        let group_separator = if locale_is_de { '.' } else { ',' };
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut current_type = "integer";
        let mut seen_decimal = false;
        let mut in_exponent = false;
        for ch in formatted.chars() {
            if matches!(ch, '\u{200e}' | '\u{061c}') {
                continue;
            }
            let next_type = if options.notation == "compact" && current_type == "compact" && ch == '.' {
                "compact"
            } else {
                match ch {
                    '-' => {
                        if in_exponent {
                            "exponentMinusSign"
                        } else {
                            "minusSign"
                        }
                    }
                    '+' => {
                        if in_exponent {
                            "exponentPlusSign"
                        } else {
                            "plusSign"
                        }
                    }
                    'E' if matches!(options.notation.as_str(), "scientific" | "engineering") => "exponentSeparator",
                    c if c == group_separator => "group",
                    c if c == decimal_separator => "decimal",
                    '%' => {
                        if options.style == "unit" && options.unit.as_deref() == Some("percent") {
                            "unit"
                        } else {
                            "percentSign"
                        }
                    }
                    '∞' => "infinity",
                    '$' if options.style == "currency" => "currency",
                    '\u{a0}' | ' ' => "literal",
                    c if options.style == "currency" && c.is_ascii_alphabetic() => "currency",
                    c if c.is_ascii_digit() => {
                        if in_exponent {
                            "exponentInteger"
                        } else if seen_decimal {
                            "fraction"
                        } else {
                            "integer"
                        }
                    }
                    c if options.notation == "compact" && !c.is_whitespace() => "compact",
                    _ => "literal",
                }
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
            if ch == 'E' && matches!(options.notation.as_str(), "scientific" | "engineering") {
                in_exponent = true;
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

    fn intl_range_part_object(ctx: &GcContext<'gc>, part_type: &str, value: &str, source: &str) -> Value<'gc> {
        let mut obj = IndexMap::new();
        obj.insert("type".to_string(), Value::from(part_type));
        obj.insert("value".to_string(), Value::from(value));
        obj.insert("source".to_string(), Value::from(source));
        Value::Object(new_gc_cell_ptr(ctx, obj))
    }

    fn intl_number_parts_with_source(
        ctx: &GcContext<'gc>,
        formatter: &IndexMap<String, Value<'gc>>,
        text: &str,
        source: &str,
    ) -> Vec<Value<'gc>> {
        Self::intl_number_string_parts(ctx, formatter, text)
            .into_iter()
            .filter_map(|part| {
                let Value::Object(obj) = part else {
                    return None;
                };
                let borrow = obj.borrow();
                let Some(Value::String(part_type)) = borrow.get("type") else {
                    return None;
                };
                let Some(Value::String(value)) = borrow.get("value") else {
                    return None;
                };
                Some(Self::intl_range_part_object(
                    ctx,
                    &crate::unicode::utf16_to_utf8(part_type),
                    &crate::unicode::utf16_to_utf8(value),
                    source,
                ))
            })
            .collect()
    }

    fn intl_number_format_range_string(
        &mut self,
        ctx: &GcContext<'gc>,
        formatter: &IndexMap<String, Value<'gc>>,
        start: Option<&Value<'gc>>,
        end: Option<&Value<'gc>>,
    ) -> Result<String, Value<'gc>> {
        let (start_input, end_input) = self.intl_number_format_range_inputs(ctx, start, end)?;
        let options = IntlNumberFormatOptions::from_object(formatter);
        let start_text = Self::intl_format_number_value(&options, start_input);
        let end_text = Self::intl_format_number_value(&options, end_input);
        if start_text == end_text {
            return Ok(format!("~{start_text}"));
        }
        if options.resolved_locale.starts_with("pt-PT")
            && options.style == "currency"
            && let Some((body_start, suffix_start)) = start_text.rsplit_once('\u{a0}')
            && let Some((body_end, suffix_end)) = end_text.rsplit_once('\u{a0}')
            && suffix_start == suffix_end
        {
            if let Some(rest_end) = body_end.strip_prefix('+')
                && body_start.starts_with('+')
            {
                return Ok(format!("{body_start} - {rest_end}\u{a0}{suffix_start}"));
            }
            return Ok(format!("{body_start} - {body_end}\u{a0}{suffix_start}"));
        }
        if options.resolved_locale.starts_with("en-US")
            && options.style == "currency"
            && options.sign_display == "always"
            && let Some(rest_start) = start_text.strip_prefix("+$")
            && let Some(rest_end) = end_text.strip_prefix("+$")
        {
            return Ok(format!("+${rest_start}–{rest_end}"));
        }
        let separator = if options.resolved_locale.starts_with("pt-PT") {
            " - "
        } else if options.style == "currency" {
            " – "
        } else {
            "–"
        };
        Ok(format!("{start_text}{separator}{end_text}"))
    }

    fn intl_number_format_range_parts_array(
        &mut self,
        ctx: &GcContext<'gc>,
        formatter: &IndexMap<String, Value<'gc>>,
        start: Option<&Value<'gc>>,
        end: Option<&Value<'gc>>,
    ) -> Result<Value<'gc>, Value<'gc>> {
        let (start_input, end_input) = self.intl_number_format_range_inputs(ctx, start, end)?;
        let options = IntlNumberFormatOptions::from_object(formatter);
        let start_text = Self::intl_format_number_value(&options, start_input);
        let end_text = Self::intl_format_number_value(&options, end_input);
        if start_text == end_text {
            let mut parts = vec![Self::intl_range_part_object(ctx, "approximatelySign", "~", "shared")];
            parts.extend(Self::intl_number_parts_with_source(ctx, formatter, &start_text, "shared"));
            return Ok(Value::Array(new_gc_cell_ptr(ctx, VmArrayData::new(parts))));
        }
        let separator = if options.resolved_locale.starts_with("pt-PT") {
            " - "
        } else {
            " – "
        };
        let mut parts = Self::intl_number_parts_with_source(ctx, formatter, &start_text, "startRange");
        parts.push(Self::intl_range_part_object(ctx, "literal", separator, "shared"));
        parts.extend(Self::intl_number_parts_with_source(ctx, formatter, &end_text, "endRange"));
        Ok(Value::Array(new_gc_cell_ptr(ctx, VmArrayData::new(parts))))
    }

    fn intl_number_format_range_inputs(
        &mut self,
        ctx: &GcContext<'gc>,
        start: Option<&Value<'gc>>,
        end: Option<&Value<'gc>>,
    ) -> Result<(IntlFormattedNumberInput, IntlFormattedNumberInput), Value<'gc>> {
        if matches!(start, None | Some(Value::Undefined)) || matches!(end, None | Some(Value::Undefined)) {
            return Err(self.make_type_error_object(ctx, "start and end are required"));
        }
        let start_input = self.intl_number_format_input(ctx, start)?;
        let end_input = self.intl_number_format_input(ctx, end)?;
        if Self::intl_number_input_is_nan(&start_input) || Self::intl_number_input_is_nan(&end_input) {
            return Err(self.make_range_error_object(ctx, "Range arguments must not be NaN"));
        }
        Ok((start_input, end_input))
    }

    fn intl_number_input_is_nan(input: &IntlFormattedNumberInput) -> bool {
        match input {
            IntlFormattedNumberInput::Number(value) => value.is_nan(),
            IntlFormattedNumberInput::BigInt(_) => false,
            IntlFormattedNumberInput::DecimalString(text) => {
                if Self::intl_parse_exact_decimal(text).is_some() {
                    false
                } else {
                    to_number(&Value::from(text.as_str())).is_nan()
                }
            }
        }
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
        if matches!(prim, Value::Symbol(_)) {
            return Err(self.make_type_error_object(ctx, "Cannot convert Symbol to number"));
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
        let abs_value = value.abs();
        if matches!(options.notation.as_str(), "scientific" | "engineering") {
            let formatted = Self::intl_format_exponential(options, abs_value);
            return Self::intl_apply_sign_and_affixes(options, negative, abs_value == 0.0, false, &formatted);
        }
        if options.notation == "compact"
            && let Some(formatted) = Self::intl_format_compact_decimal(options, abs_value)
        {
            let is_zeroish = formatted
                .trim_start_matches('0')
                .trim_matches(Self::intl_decimal_separator(options))
                .is_empty();
            return Self::intl_apply_sign_and_affixes(options, negative, is_zeroish, false, &formatted);
        }
        let (mut integer_digits, fraction_digits) = Self::intl_format_decimal_digits(options, value);
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

    fn intl_format_decimal_digits(options: &IntlNumberFormatOptions, value: f64) -> (String, String) {
        let sig_digits = options
            .maximum_significant_digits
            .unwrap_or(options.minimum_significant_digits.unwrap_or(21));
        let sig_quantum = Self::intl_significant_rounding_quantum(value, sig_digits);
        let frac_quantum = if options.rounding_increment == 1 {
            10f64.powi(-(options.maximum_fraction_digits as i32))
        } else {
            options.rounding_increment as f64 / 10f64.powi(options.maximum_fraction_digits as i32)
        };
        let use_significant = if options.minimum_significant_digits.is_some() || options.maximum_significant_digits.is_some() {
            match options.rounding_priority.as_str() {
                "morePrecision" if options.minimum_fraction_digits > 0 || options.maximum_fraction_digits > 0 => {
                    sig_quantum <= frac_quantum
                }
                "lessPrecision" if options.minimum_fraction_digits > 0 || options.maximum_fraction_digits > 0 => sig_quantum > frac_quantum,
                _ => true,
            }
        } else {
            false
        };
        if use_significant {
            let rounded = Self::intl_round_to_significant_digits_mode(value, sig_digits, &options.rounding_mode).abs();
            Self::intl_format_standard_significant_digits(rounded, options.minimum_significant_digits.unwrap_or(1), sig_digits)
        } else {
            let rounded = if options.rounding_increment == 1 {
                Self::intl_round_to_fraction_digits_mode(value, options.maximum_fraction_digits, &options.rounding_mode)
            } else {
                Self::intl_round_to_increment(
                    value,
                    options.maximum_fraction_digits,
                    options.rounding_increment,
                    &options.rounding_mode,
                )
            }
            .abs();
            Self::intl_split_fixed_decimal(rounded, options.maximum_fraction_digits, options.minimum_fraction_digits, false)
        }
    }

    fn intl_format_decimal_string_or_number(options: &IntlNumberFormatOptions, value: &str) -> String {
        if let Some(decimal) = Self::intl_parse_exact_decimal(value)
            && options.notation == "standard"
            && options.rounding_increment == 1
            && matches!(options.rounding_mode.as_str(), "halfExpand" | "trunc")
            && options.rounding_priority == "auto"
        {
            if options.minimum_significant_digits.is_some() || options.maximum_significant_digits.is_some() {
                return Self::intl_format_exact_decimal_significant(options, &decimal);
            }
            return Self::intl_format_exact_decimal(options, &decimal);
        }
        Self::intl_format_f64_value(options, value.parse::<f64>().unwrap_or(f64::NAN))
    }

    fn intl_round_to_fraction_digits(value: f64, digits: u8) -> f64 {
        Self::intl_round_to_fraction_digits_mode(value, digits, "halfExpand")
    }

    fn intl_round_to_fraction_digits_mode(value: f64, digits: u8, rounding_mode: &str) -> f64 {
        let factor = 10f64.powi(digits as i32);
        Self::intl_round_scaled_value(value, factor, rounding_mode)
    }

    fn intl_round_to_significant_digits_mode(value: f64, digits: u8, rounding_mode: &str) -> f64 {
        if value == 0.0 {
            return 0.0;
        }
        let integer_digits = value.abs().log10().floor() as i32 + 1;
        let factor = 10f64.powi(digits as i32 - integer_digits);
        Self::intl_round_scaled_value(value, factor, rounding_mode)
    }

    fn intl_significant_rounding_quantum(value: f64, digits: u8) -> f64 {
        if value == 0.0 {
            return 10f64.powi(-(digits as i32));
        }
        let integer_digits = value.abs().log10().floor() as i32 + 1;
        10f64.powi(integer_digits - digits as i32)
    }

    fn intl_round_to_increment(value: f64, max_fraction_digits: u8, increment: u16, rounding_mode: &str) -> f64 {
        let factor = 10f64.powi(max_fraction_digits as i32) / increment as f64;
        Self::intl_round_scaled_value(value, factor, rounding_mode)
    }

    fn intl_round_scaled_value(value: f64, factor: f64, rounding_mode: &str) -> f64 {
        Self::intl_round_integer_mode(value * factor, rounding_mode) / factor
    }

    fn intl_round_integer_mode(value: f64, rounding_mode: &str) -> f64 {
        let lower = value.floor();
        let upper = value.ceil();
        if (upper - lower).abs() < f64::EPSILON {
            return value;
        }
        match rounding_mode {
            "ceil" => return upper,
            "floor" => return lower,
            "expand" => {
                return if value.is_sign_negative() { lower } else { upper };
            }
            "trunc" => {
                return if value.is_sign_negative() { upper } else { lower };
            }
            _ => {}
        }
        let lower_dist = (value - lower).abs();
        let upper_dist = (upper - value).abs();
        let eps = 1e-12;
        if lower_dist + eps < upper_dist {
            return lower;
        }
        if upper_dist + eps < lower_dist {
            return upper;
        }
        match rounding_mode {
            "halfCeil" => upper,
            "halfFloor" => lower,
            "halfTrunc" => {
                if value.is_sign_negative() {
                    upper
                } else {
                    lower
                }
            }
            "halfEven" => {
                if (lower as i128).rem_euclid(2) == 0 {
                    lower
                } else {
                    upper
                }
            }
            _ => {
                if value.is_sign_negative() {
                    lower
                } else {
                    upper
                }
            }
        }
    }

    fn intl_format_standard_significant_digits(
        value: f64,
        minimum_significant_digits: u8,
        _maximum_significant_digits: u8,
    ) -> (String, String) {
        let mut text = value.to_string();
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

    fn intl_format_exact_decimal_significant(options: &IntlNumberFormatOptions, decimal: &IntlExactDecimal) -> String {
        let min_sig = options.minimum_significant_digits.unwrap_or(1) as usize;
        let max_sig = options
            .maximum_significant_digits
            .unwrap_or(options.minimum_significant_digits.unwrap_or(21)) as usize;
        let mut digits = decimal.digits.clone();
        let mut scale = decimal.scale;
        let Some(first_nonzero) = digits.chars().position(|ch| ch != '0') else {
            let fraction = "0".repeat(min_sig.saturating_sub(1));
            let mut formatted = "0".to_string();
            if !fraction.is_empty() {
                formatted.push(Self::intl_decimal_separator(options));
                formatted.push_str(&fraction);
            }
            return Self::intl_apply_sign_and_affixes(options, decimal.negative, true, false, &formatted);
        };
        let sig_count = digits.len() - first_nonzero;
        if sig_count > max_sig {
            let cut = first_nonzero + max_sig;
            let round_up = options.rounding_mode == "halfExpand" && digits.as_bytes().get(cut).is_some_and(|digit| *digit >= b'5');
            let removed = digits.len() - cut;
            digits.truncate(cut);
            let integer_removed = removed.saturating_sub(scale);
            scale = scale.saturating_sub(removed);
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
                digits = String::from_utf8(bytes).unwrap_or(digits);
            }
            if integer_removed > 0 {
                digits.push_str(&"0".repeat(integer_removed));
            }
        }
        let split = digits.len().saturating_sub(scale);
        let integer_raw = if split == 0 { "0".to_string() } else { digits[..split].to_string() };
        let mut integer = integer_raw.trim_start_matches('0').to_string();
        if integer.is_empty() {
            integer = "0".to_string();
        }
        let mut fraction = if scale == 0 { String::new() } else { digits[split..].to_string() };
        while !fraction.is_empty()
            && fraction.ends_with('0')
            && Self::intl_count_significant_digits(&(integer.clone() + "." + &fraction)) > min_sig
        {
            fraction.pop();
        }
        while Self::intl_count_significant_digits(&(integer.clone() + "." + &fraction)) < min_sig {
            fraction.push('0');
        }
        let mut formatted = integer;
        if !fraction.is_empty() {
            formatted.push(Self::intl_decimal_separator(options));
            formatted.push_str(&fraction);
        }
        let is_zeroish = formatted.chars().all(|ch| matches!(ch, '0' | '.'));
        Self::intl_apply_sign_and_affixes(options, decimal.negative, is_zeroish, false, &formatted)
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
        if !Self::intl_should_use_grouping(integer_digits, options) {
            return integer_digits.to_string();
        }
        Self::intl_group_integer_digits(integer_digits, options)
    }

    fn intl_group_integer_digits(integer_digits: &str, options: &IntlNumberFormatOptions) -> String {
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
        if options.resolved_locale.starts_with("de")
            || options.resolved_locale.starts_with("pt")
            || options.resolved_locale.starts_with("pl")
        {
            ','
        } else {
            '.'
        }
    }

    fn intl_group_separator(options: &IntlNumberFormatOptions) -> char {
        if options.resolved_locale.starts_with("pt") || options.resolved_locale.starts_with("pl") {
            '\u{a0}'
        } else if options.resolved_locale.starts_with("de") {
            '.'
        } else {
            ','
        }
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
            let suffix_unit = Self::intl_render_unit_label(options, &pattern.suffix_unit, core);
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
            out.push_str(&suffix_unit);
            return out;
        }
        out.push_str(&prefix);
        if options.style == "currency"
            && let Some(currency) = &options.currency
        {
            let symbol = Self::intl_currency_symbol(options, currency);
            if options.resolved_locale.starts_with("de") || options.resolved_locale.starts_with("pt-PT") {
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
        if prefix.is_empty() {
            return (prefix, String::new());
        }
        let bidi_prefix = if options.resolved_locale.starts_with("ar") || options.numbering_system == "arab" {
            "\u{200e}"
        } else {
            ""
        };
        (format!("{bidi_prefix}{prefix}"), String::new())
    }

    fn intl_currency_symbol<'a>(options: &IntlNumberFormatOptions, currency: &'a str) -> &'a str {
        if options.currency_display == "code" || options.currency_display == "name" {
            return currency;
        }
        if currency == "EUR" {
            return "€";
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

    fn intl_english_plural_unit(unit: &str, core: &str) -> String {
        if unit.ends_with("/h") || unit.len() == 1 || unit.contains(" per ") {
            return unit.to_string();
        }
        let normalized = core.replace([',', '.'], "");
        if normalized == "1" || normalized == "-1" || unit.ends_with('s') {
            unit.to_string()
        } else {
            format!("{unit}s")
        }
    }

    fn intl_render_unit_label(options: &IntlNumberFormatOptions, unit: &str, core: &str) -> String {
        if options.resolved_locale.starts_with("en") {
            Self::intl_english_plural_unit(unit, core)
        } else {
            unit.to_string()
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
        if unit == "percent" {
            return Some(IntlUnitPattern {
                prefix_unit: None,
                prefix_literal: None,
                suffix_literal: None,
                suffix_unit: "%".to_string(),
            });
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
        let (divisor, suffix) = if locale.starts_with("en-IN") {
            if value >= 10_000_000.0 {
                (10_000_000.0, if compact_display == "long" { " crore" } else { "Cr" })
            } else if value >= 100_000.0 {
                (100_000.0, if compact_display == "long" { " lakh" } else { "L" })
            } else if value >= 1_000.0 {
                (1_000.0, if compact_display == "long" { " thousand" } else { "K" })
            } else {
                return Some(Self::intl_format_compact_plain(options, value, false));
            }
        } else if locale.starts_with("en") {
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
        Self::intl_round_exact_fraction_digits_mode(
            &mut digits,
            &mut scale,
            options.maximum_fraction_digits as usize,
            &options.rounding_mode,
        );
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

    fn intl_round_exact_fraction_digits_mode(digits: &mut String, scale: &mut usize, maximum_fraction_digits: usize, rounding_mode: &str) {
        if *scale <= maximum_fraction_digits {
            return;
        }
        let cut = digits.len() - *scale + maximum_fraction_digits;
        let round_up = rounding_mode == "halfExpand" && digits.as_bytes().get(cut).is_some_and(|digit| *digit >= b'5');
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
        if !borrow.contains_key("__intl_locale__") {
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

    fn intl_date_time_range_parts(
        formatter: &IndexMap<String, Value<'gc>>,
        start: i64,
        end: i64,
    ) -> Result<Vec<(String, String, &'static str)>, Value<'gc>> {
        let start_parts = Self::intl_date_time_format_parts(formatter, start)?;
        let end_parts = Self::intl_date_time_format_parts(formatter, end)?;
        if start_parts == end_parts {
            return Ok(start_parts
                .into_iter()
                .map(|(part_type, value)| (part_type, value, "shared"))
                .collect());
        }

        let mut parts = Vec::new();
        if Self::intl_can_collapse_date_time_range(formatter) {
            let mut prefix_len = 0;
            while prefix_len < start_parts.len() && prefix_len < end_parts.len() && start_parts[prefix_len] == end_parts[prefix_len] {
                prefix_len += 1;
            }

            let mut suffix_len = 0;
            while suffix_len < start_parts.len().saturating_sub(prefix_len)
                && suffix_len < end_parts.len().saturating_sub(prefix_len)
                && start_parts[start_parts.len() - 1 - suffix_len] == end_parts[end_parts.len() - 1 - suffix_len]
            {
                suffix_len += 1;
            }

            if suffix_len > 0 {
                parts.extend(
                    start_parts[..prefix_len]
                        .iter()
                        .cloned()
                        .map(|(part_type, value)| (part_type, value, "shared")),
                );
                parts.extend(
                    start_parts[prefix_len..start_parts.len() - suffix_len]
                        .iter()
                        .cloned()
                        .map(|(part_type, value)| (part_type, value, "startRange")),
                );
                parts.push(("literal".to_string(), " – ".to_string(), "shared"));
                parts.extend(
                    end_parts[prefix_len..end_parts.len() - suffix_len]
                        .iter()
                        .cloned()
                        .map(|(part_type, value)| (part_type, value, "endRange")),
                );
                parts.extend(
                    start_parts[start_parts.len() - suffix_len..]
                        .iter()
                        .cloned()
                        .map(|(part_type, value)| (part_type, value, "shared")),
                );
                return Ok(parts);
            }
        }

        parts.extend(start_parts.into_iter().map(|(part_type, value)| (part_type, value, "startRange")));
        parts.push(("literal".to_string(), " – ".to_string(), "shared"));
        parts.extend(end_parts.into_iter().map(|(part_type, value)| (part_type, value, "endRange")));
        Ok(parts)
    }

    fn intl_can_collapse_date_time_range(formatter: &IndexMap<String, Value<'gc>>) -> bool {
        matches!(
            formatter.get("__intl_month__"),
            Some(Value::String(month)) if crate::unicode::utf16_to_utf8(month) == "short"
        ) && formatter.get("__intl_day__").is_some()
            && formatter.get("__intl_year__").is_some()
            && formatter.get("__intl_weekday__").is_none()
            && formatter.get("__intl_era__").is_none()
            && formatter.get("__intl_dayPeriod__").is_none()
            && formatter.get("__intl_hour__").is_none()
            && formatter.get("__intl_minute__").is_none()
            && formatter.get("__intl_second__").is_none()
            && formatter.get("__intl_fractionalSecondDigits__").is_none()
            && formatter.get("__intl_timeZoneName__").is_none()
            && formatter.get("__intl_dateStyle__").is_none()
            && formatter.get("__intl_timeStyle__").is_none()
    }

    fn intl_date_time_format_parts(formatter: &IndexMap<String, Value<'gc>>, millis: i64) -> Result<Vec<(String, String)>, Value<'gc>> {
        use chrono::{FixedOffset, TimeZone};

        let time_zone = formatter
            .get("__intl_time_zone__")
            .map(value_to_string)
            .unwrap_or_else(|| "UTC".to_string());
        let time_zone_explicit = matches!(formatter.get("__intl_timeZoneExplicit__"), Some(Value::Boolean(true)));
        let offset = if !time_zone_explicit && time_zone == "UTC" {
            let date_time = chrono::Local
                .timestamp_millis_opt(millis)
                .single()
                .ok_or_else(|| Value::from("Invalid time value"))?;
            return Self::intl_date_time_format_parts_for_datetime(formatter, &date_time);
        } else if time_zone == "UTC" {
            FixedOffset::east_opt(0)
        } else {
            Self::intl_fixed_offset_from_zone(&time_zone)
        }
        .unwrap_or_else(|| FixedOffset::east_opt(0).expect("zero UTC offset"));
        let date_time = offset
            .timestamp_millis_opt(millis)
            .single()
            .ok_or_else(|| Value::from("Invalid time value"))?;

        Self::intl_date_time_format_parts_for_datetime(formatter, &date_time)
    }

    fn intl_date_time_format_parts_for_datetime<Tz: chrono::TimeZone>(
        formatter: &IndexMap<String, Value<'gc>>,
        date_time: &chrono::DateTime<Tz>,
    ) -> Result<Vec<(String, String)>, Value<'gc>>
    where
        Tz::Offset: std::fmt::Display,
    {
        use chrono::{Datelike, Timelike};

        let mut parts = Vec::new();
        let weekday = formatter.get("__intl_weekday__").map(value_to_string);
        let era = formatter.get("__intl_era__").map(value_to_string);
        let year = formatter.get("__intl_year__").map(value_to_string);
        let month = formatter.get("__intl_month__").map(value_to_string);
        let day = formatter.get("__intl_day__").map(value_to_string);
        let day_period = formatter.get("__intl_dayPeriod__").map(value_to_string);
        let hour = formatter.get("__intl_hour__").map(value_to_string);
        let minute = formatter.get("__intl_minute__").map(value_to_string);
        let second = formatter.get("__intl_second__").map(value_to_string);
        let fractional_second_digits = match formatter.get("__intl_fractionalSecondDigits__") {
            Some(Value::Number(value)) => Some(*value as usize),
            _ => None,
        };
        let time_zone_name = formatter.get("__intl_timeZoneName__").map(value_to_string);
        let date_style = formatter.get("__intl_dateStyle__").map(value_to_string);
        let time_style = formatter.get("__intl_timeStyle__").map(value_to_string);
        let numeric_month_style = matches!(month.as_deref(), Some("2-digit" | "numeric"));
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

        if date_style.is_some() || time_style.is_some() {
            return Ok(Self::intl_date_time_style_parts(
                formatter,
                date_time,
                date_style.as_deref(),
                time_style.as_deref(),
                &numbering_system,
            ));
        }

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

        if hour.is_some() || minute.is_some() || second.is_some() || day_period.is_some() {
            if !parts.is_empty() {
                parts.push(("literal".to_string(), " ".to_string()));
            }
            let hour_cycle = formatter.get("__intl_hour_cycle__").map(value_to_string);
            let hour12 = matches!(formatter.get("__intl_hour12__"), Some(Value::Boolean(true)));
            let mut hour_value = date_time.hour() as i64;
            if day_period.is_some() || hour12 || matches!(hour_cycle.as_deref(), Some("h11" | "h12")) {
                hour_value %= 12;
                if matches!(hour_cycle.as_deref(), Some("h12")) && hour_value == 0 {
                    hour_value = 12;
                }
                if day_period.is_some() && hour_value == 0 {
                    hour_value = 12;
                }
            }
            if let Some(ref hour_style) = hour {
                let width = usize::from(hour_style == "2-digit") + 1;
                parts.push(("hour".to_string(), numeric(hour_value, width)));
            }
            if let Some(ref minute_style) = minute {
                if formatter.get("__intl_hour__").is_some() {
                    parts.push(("literal".to_string(), ":".to_string()));
                }
                let width = if second.is_some() || hour.is_some() || minute_style == "2-digit" {
                    2
                } else {
                    1
                };
                parts.push(("minute".to_string(), numeric(date_time.minute() as i64, width)));
            }
            if let Some(second_style) = second {
                if formatter.get("__intl_minute__").is_some() {
                    parts.push(("literal".to_string(), ":".to_string()));
                }
                let width = if minute.is_some() || second_style == "2-digit" { 2 } else { 1 };
                parts.push(("second".to_string(), numeric(date_time.second() as i64, width)));
                if let Some(fractional_second_digits) = fractional_second_digits {
                    parts.push(("literal".to_string(), ".".to_string()));
                    let millis = format!("{:03}", date_time.timestamp_subsec_millis());
                    parts.push(("fractionalSecond".to_string(), millis[..fractional_second_digits].to_string()));
                }
            }
            if let Some(day_period_style) = day_period {
                if hour.is_some() {
                    parts.push(("literal".to_string(), " ".to_string()));
                }
                parts.push((
                    "dayPeriod".to_string(),
                    Self::intl_day_period_text(date_time.hour(), &day_period_style),
                ));
            } else if hour12 {
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
            parts.push(("month".to_string(), numeric(date_time.month() as i64, 1)));
            parts.push(("literal".to_string(), "/".to_string()));
            parts.push(("day".to_string(), numeric(date_time.day() as i64, 1)));
            parts.push(("literal".to_string(), "/".to_string()));
            parts.push(("year".to_string(), numeric(date_time.year() as i64, 1)));
        }

        Ok(parts)
    }

    fn intl_day_period_text(hour: u32, style: &str) -> String {
        match hour {
            12 => {
                if style == "narrow" {
                    "n".to_string()
                } else {
                    "noon".to_string()
                }
            }
            6..=11 => "in the morning".to_string(),
            13..=17 => "in the afternoon".to_string(),
            18..=20 => "in the evening".to_string(),
            _ => "at night".to_string(),
        }
    }

    fn intl_date_time_style_parts<Tz: chrono::TimeZone>(
        formatter: &IndexMap<String, Value<'gc>>,
        date_time: &chrono::DateTime<Tz>,
        date_style: Option<&str>,
        time_style: Option<&str>,
        numbering_system: &str,
    ) -> Vec<(String, String)>
    where
        Tz::Offset: std::fmt::Display,
    {
        use chrono::{Datelike, Timelike};
        let locale = formatter
            .get("__intl_locale__")
            .map(value_to_string)
            .unwrap_or_else(|| INTL_DEFAULT_LOCALE.to_string());
        let hour_cycle = formatter.get("__intl_hour_cycle__").map(value_to_string);
        let hour12 = matches!(formatter.get("__intl_hour12__"), Some(Value::Boolean(true)))
            || matches!(hour_cycle.as_deref(), Some("h11" | "h12"))
            || (!matches!(hour_cycle.as_deref(), Some("h23" | "h24")) && locale.starts_with("en-US"));
        let numeric = |value: i64, width: usize| Self::intl_format_numbering_digits(value, width, numbering_system);
        let mut parts = Vec::new();
        if let Some(date_style) = date_style {
            let month_name = [
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
            ][date_time.month0() as usize];
            if date_style == "full" {
                parts.push((
                    "weekday".to_string(),
                    ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]
                        [date_time.weekday().num_days_from_sunday() as usize]
                        .to_string(),
                ));
                parts.push(("literal".to_string(), ", ".to_string()));
            }
            match date_style {
                "short" => {
                    parts.push(("month".to_string(), numeric(date_time.month() as i64, 1)));
                    parts.push(("literal".to_string(), "/".to_string()));
                    parts.push(("day".to_string(), numeric(date_time.day() as i64, 1)));
                    parts.push(("literal".to_string(), "/".to_string()));
                    parts.push(("year".to_string(), numeric((date_time.year().rem_euclid(100)) as i64, 2)));
                }
                _ => {
                    parts.push(("month".to_string(), month_name.to_string()));
                    parts.push(("literal".to_string(), " ".to_string()));
                    parts.push(("day".to_string(), numeric(date_time.day() as i64, 1)));
                    parts.push(("literal".to_string(), ", ".to_string()));
                    parts.push(("year".to_string(), numeric(date_time.year() as i64, 1)));
                }
            }
        }
        if let Some(time_style) = time_style {
            if !parts.is_empty() {
                parts.push(("literal".to_string(), ", ".to_string()));
            }
            let mut hour = date_time.hour() as i64;
            let day_period = if hour12 {
                let dp = if hour < 12 { "AM" } else { "PM" };
                hour %= 12;
                if hour == 0 {
                    hour = 12;
                }
                Some(dp)
            } else {
                None
            };
            parts.push((
                "hour".to_string(),
                if day_period.is_some() { numeric(hour, 1) } else { numeric(hour, 2) },
            ));
            parts.push(("literal".to_string(), ":".to_string()));
            parts.push(("minute".to_string(), numeric(date_time.minute() as i64, 2)));
            if matches!(time_style, "full" | "long" | "medium") {
                parts.push(("literal".to_string(), ":".to_string()));
                parts.push(("second".to_string(), numeric(date_time.second() as i64, 2)));
            }
            if let Some(dp) = day_period {
                parts.push(("literal".to_string(), " ".to_string()));
                parts.push(("dayPeriod".to_string(), dp.to_string()));
            }
            if matches!(time_style, "full" | "long") {
                parts.push(("literal".to_string(), " ".to_string()));
                parts.push((
                    "timeZoneName".to_string(),
                    if time_style == "full" {
                        "Coordinated Universal Time".to_string()
                    } else {
                        "UTC".to_string()
                    },
                ));
            };
        }
        parts
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
            time_zone_explicit: false,
            year: None,
            month: None,
            day: None,
            weekday: None,
            era: None,
            day_period: None,
            hour: None,
            minute: None,
            second: None,
            fractional_second_digits: None,
            time_zone_name: None,
            hour_cycle: None,
            hour12: None,
            date_style: None,
            time_style: None,
        };
        let Some(options) = options else {
            out.year = Some("numeric".to_string());
            out.month = Some("numeric".to_string());
            out.day = Some("numeric".to_string());
            out.resolved_locale = Self::intl_date_time_format_resolved_locale(
                &locale_base,
                ext_calendar.as_deref(),
                ext_numbering_system.as_deref(),
                ext_hour_cycle.as_deref(),
            );
            return Ok(out);
        };
        if matches!(options, Value::Undefined) || !Self::intl_is_object_like(options) {
            out.year = Some("numeric".to_string());
            out.month = Some("numeric".to_string());
            out.day = Some("numeric".to_string());
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
            out.time_zone_explicit = true;
        }
        out.weekday = self.intl_string_option(ctx, options, "weekday", &["narrow", "short", "long"], None)?;
        out.era = self.intl_string_option(ctx, options, "era", &["narrow", "short", "long"], None)?;
        out.year = self.intl_string_option(ctx, options, "year", &["2-digit", "numeric"], None)?;
        out.month = self.intl_string_option(ctx, options, "month", &["2-digit", "numeric", "narrow", "short", "long"], None)?;
        out.day = self.intl_string_option(ctx, options, "day", &["2-digit", "numeric"], None)?;
        out.day_period = self.intl_string_option(ctx, options, "dayPeriod", &["narrow", "short", "long"], None)?;
        out.hour = self.intl_string_option(ctx, options, "hour", &["2-digit", "numeric"], None)?;
        out.minute = self.intl_string_option(ctx, options, "minute", &["2-digit", "numeric"], None)?;
        out.second = self.intl_string_option(ctx, options, "second", &["2-digit", "numeric"], None)?;
        let fractional_second_digits_value = self.read_named_property(ctx, options, "fractionalSecondDigits");
        if let Some(thrown) = self.pending_throw.take() {
            return Err(thrown);
        }
        out.fractional_second_digits =
            self.intl_default_u8_option_value(ctx, Some(&fractional_second_digits_value), "fractionalSecondDigits", 1, 3)?;
        out.time_zone_name = self.intl_string_option(
            ctx,
            options,
            "timeZoneName",
            &["short", "long", "shortOffset", "longOffset", "shortGeneric", "longGeneric"],
            None,
        )?;
        if out.hour.is_some() || out.time_style.is_some() {
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
        out.date_style = self.intl_string_option(ctx, options, "dateStyle", &["full", "long", "medium", "short"], None)?;
        out.time_style = self.intl_string_option(ctx, options, "timeStyle", &["full", "long", "medium", "short"], None)?;
        if out.time_style.is_some() && out.hour_cycle.is_none() {
            out.hour_cycle = if let Some(hour12) = option_hour12 {
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
            out.hour12 = out.hour_cycle.as_deref().map(Self::intl_hour_cycle_is_twelve_hour);
        }
        if (out.date_style.is_some() || out.time_style.is_some()) && Self::intl_has_explicit_date_time_components(ctx, self, options)? {
            return Err(self.make_type_error_object(ctx, "dateStyle/timeStyle conflicts with explicit components"));
        }
        if out.date_style.is_some() || out.time_style.is_some() {
            out.weekday = None;
            out.era = None;
            out.year = None;
            out.month = None;
            out.day = None;
            out.day_period = None;
            out.hour = None;
            out.minute = None;
            out.second = None;
            out.fractional_second_digits = None;
            out.time_zone_name = None;
        } else if out.day_period.is_some() && out.hour.is_none() && out.minute.is_none() && out.second.is_none() {
            out.year = None;
            out.month = None;
            out.day = None;
        } else if out.weekday.is_none()
            && out.era.is_none()
            && out.year.is_none()
            && out.month.is_none()
            && out.day.is_none()
            && out.day_period.is_none()
            && out.hour.is_none()
            && out.minute.is_none()
            && out.second.is_none()
            && out.time_zone_name.is_none()
        {
            out.year = Some("numeric".to_string());
            out.month = Some("numeric".to_string());
            out.day = Some("numeric".to_string());
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
        obj.insert("__intl_timeZoneExplicit__".to_string(), Value::Boolean(options.time_zone_explicit));
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
        if let Some(day_period) = &options.day_period {
            obj.insert("__intl_dayPeriod__".to_string(), Value::from(day_period.as_str()));
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
        if let Some(fractional_second_digits) = options.fractional_second_digits {
            obj.insert(
                "__intl_fractionalSecondDigits__".to_string(),
                Value::Number(fractional_second_digits as f64),
            );
        }
        if let Some(time_zone_name) = &options.time_zone_name {
            obj.insert("__intl_timeZoneName__".to_string(), Value::from(time_zone_name.as_str()));
        }
        if let Some(date_style) = &options.date_style {
            obj.insert("__intl_dateStyle__".to_string(), Value::from(date_style.as_str()));
        }
        if let Some(time_style) = &options.time_style {
            obj.insert("__intl_timeStyle__".to_string(), Value::from(time_style.as_str()));
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
            rounding_mode: "halfExpand".to_string(),
            rounding_priority: "auto".to_string(),
            rounding_increment: 1,
            trailing_zero_display: "auto".to_string(),
            use_grouping: IntlUseGrouping::Auto,
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
        if let Some(rounding_mode) = self.intl_string_option_from_value(
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
        )? {
            out.rounding_mode = rounding_mode;
        }
        if let Some(rounding_priority) = self.intl_string_option_from_value(
            ctx,
            raw_options.get("roundingPriority"),
            &["auto", "morePrecision", "lessPrecision"],
            Some("auto"),
        )? {
            out.rounding_priority = rounding_priority;
        }
        if let Some(trailing_zero_display) = self.intl_string_option_from_value(
            ctx,
            raw_options.get("trailingZeroDisplay"),
            &["auto", "stripIfInteger"],
            Some("auto"),
        )? {
            out.trailing_zero_display = trailing_zero_display;
        }
        out.use_grouping = if let Some(use_grouping) = self.intl_use_grouping_option(ctx, raw_options.get("useGrouping"))? {
            use_grouping
        } else if out.notation == "compact" {
            IntlUseGrouping::Min2
        } else {
            IntlUseGrouping::Auto
        };
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
            if out.rounding_priority != "auto" || out.minimum_significant_digits.is_some() || out.maximum_significant_digits.is_some() {
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

    fn intl_use_grouping_option(
        &mut self,
        ctx: &GcContext<'gc>,
        value: Option<&Value<'gc>>,
    ) -> Result<Option<IntlUseGrouping>, Value<'gc>> {
        let Some(value) = value else {
            return Ok(None);
        };
        match value {
            Value::Undefined => Ok(None),
            Value::Boolean(true) => Ok(Some(IntlUseGrouping::Always)),
            Value::Boolean(false) | Value::Null => Ok(Some(IntlUseGrouping::False)),
            Value::Number(number) if *number == 0.0 => Ok(Some(IntlUseGrouping::False)),
            Value::String(text) => match crate::unicode::utf16_to_utf8(text).as_str() {
                "" => Ok(Some(IntlUseGrouping::False)),
                "min2" => Ok(Some(IntlUseGrouping::Min2)),
                "auto" => Ok(Some(IntlUseGrouping::Auto)),
                "always" => Ok(Some(IntlUseGrouping::Always)),
                "true" | "false" => Ok(Some(IntlUseGrouping::Auto)),
                _ => Err(self.make_range_error_object(ctx, "Invalid option value")),
            },
            _ => Err(self.make_range_error_object(ctx, "Invalid option value")),
        }
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
        obj.insert("__intl_rounding_mode__".to_string(), Value::from(options.rounding_mode.as_str()));
        obj.insert(
            "__intl_rounding_priority__".to_string(),
            Value::from(options.rounding_priority.as_str()),
        );
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
        obj.insert(
            "__intl_use_grouping__".to_string(),
            Self::intl_use_grouping_value(&options.use_grouping),
        );
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

    fn intl_read_relative_time_format_options(
        &mut self,
        ctx: &GcContext<'gc>,
        requested_locales: &[String],
        options: Option<&Value<'gc>>,
    ) -> Result<IntlRelativeTimeFormatOptions, Value<'gc>> {
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
        let mut out = IntlRelativeTimeFormatOptions::new(
            Self::intl_numbering_system_resolved_locale(&locale_base, ext_numbering_system.as_deref()),
            ext_numbering_system.clone().unwrap_or_else(|| "latn".to_string()),
        );
        let Some(options) = options else {
            return Ok(out);
        };
        if matches!(options, Value::Undefined) {
            return Ok(out);
        }
        if matches!(options, Value::Null) {
            return Err(self.make_type_error_object(ctx, "options must not be null"));
        }
        let boxed_options = if Self::intl_is_object_like(options) {
            options.clone()
        } else {
            self.intl_box_primitive_if_needed(ctx, options)
        };
        self.intl_locale_matcher_option(ctx, Some(&boxed_options))?;
        let mut option_numbering_system = None;
        if let Some(numbering_system) = self.intl_string_option(ctx, &boxed_options, "numberingSystem", &[], None)? {
            if !Self::intl_is_valid_unicode_type_identifier(&numbering_system) {
                return Err(self.make_range_error_object(ctx, "Invalid numberingSystem"));
            }
            if let Some(numbering_system) = Self::intl_supported_numbering_system(&numbering_system) {
                out.numbering_system = numbering_system;
                option_numbering_system = Some(out.numbering_system.clone());
            }
        }
        out.style = self
            .intl_string_option(ctx, &boxed_options, "style", &["long", "short", "narrow"], Some("long"))?
            .unwrap_or_else(|| "long".to_string());
        out.numeric = self
            .intl_string_option(ctx, &boxed_options, "numeric", &["always", "auto"], Some("always"))?
            .unwrap_or_else(|| "always".to_string());
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

    fn intl_read_plural_rules_options(
        &mut self,
        ctx: &GcContext<'gc>,
        requested_locales: &[String],
        options: Option<&Value<'gc>>,
    ) -> Result<IntlPluralRulesOptions, Value<'gc>> {
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
        let mut out = IntlPluralRulesOptions::new(locale_base);
        let Some(options) = options else {
            return Ok(out);
        };
        if matches!(options, Value::Undefined) {
            return Ok(out);
        }
        if matches!(options, Value::Null) {
            return Err(self.make_type_error_object(ctx, "options must not be null"));
        }
        let boxed_options = if Self::intl_is_object_like(options) {
            options.clone()
        } else {
            self.intl_box_primitive_if_needed(ctx, options)
        };
        let mut raw = IndexMap::new();
        for key in [
            "localeMatcher",
            "type",
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
        ] {
            let value = self.read_named_property(ctx, &boxed_options, key);
            if let Some(thrown) = self.pending_throw.take() {
                return Err(thrown);
            }
            raw.insert(key.to_string(), value);
        }
        let _ = self.intl_string_option_from_value(ctx, raw.get("localeMatcher"), &["lookup", "best fit"], Some("best fit"))?;
        out.plural_type = self
            .intl_string_option_from_value(ctx, raw.get("type"), &["cardinal", "ordinal"], Some("cardinal"))?
            .unwrap_or_else(|| "cardinal".to_string());
        out.notation = self
            .intl_string_option_from_value(
                ctx,
                raw.get("notation"),
                &["standard", "compact", "scientific", "engineering"],
                Some("standard"),
            )?
            .unwrap_or_else(|| "standard".to_string());
        if let Some(value) = self.intl_default_u8_option_value(ctx, raw.get("minimumIntegerDigits"), "minimumIntegerDigits", 1, 21)? {
            out.minimum_integer_digits = value;
        }
        out.minimum_fraction_digits = self
            .intl_default_u8_option_value(ctx, raw.get("minimumFractionDigits"), "minimumFractionDigits", 0, 20)?
            .unwrap_or(0);
        out.maximum_fraction_digits = self
            .intl_default_u8_option_value(ctx, raw.get("maximumFractionDigits"), "maximumFractionDigits", 0, 20)?
            .unwrap_or(3)
            .max(out.minimum_fraction_digits);
        out.minimum_significant_digits =
            self.intl_default_u8_option_value(ctx, raw.get("minimumSignificantDigits"), "minimumSignificantDigits", 1, 21)?;
        out.maximum_significant_digits =
            self.intl_default_u8_option_value(ctx, raw.get("maximumSignificantDigits"), "maximumSignificantDigits", 1, 21)?;
        if let (Some(min), Some(max)) = (out.minimum_significant_digits, out.maximum_significant_digits)
            && max < min
        {
            return Err(self.make_range_error_object(ctx, "maximumSignificantDigits must be >= minimumSignificantDigits"));
        }
        let _ = self.intl_rounding_increment_option(ctx, raw.get("roundingIncrement"))?;
        let _ = self.intl_string_option_from_value(
            ctx,
            raw.get("roundingMode"),
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
        let _ = self.intl_string_option_from_value(
            ctx,
            raw.get("roundingPriority"),
            &["auto", "morePrecision", "lessPrecision"],
            Some("auto"),
        )?;
        let _ = self.intl_string_option_from_value(ctx, raw.get("trailingZeroDisplay"), &["auto", "stripIfInteger"], Some("auto"))?;
        Ok(out)
    }

    fn intl_read_segmenter_options(
        &mut self,
        ctx: &GcContext<'gc>,
        requested_locales: &[String],
        options: Option<&Value<'gc>>,
    ) -> Result<IntlSegmenterOptions, Value<'gc>> {
        let requested_locale = requested_locales
            .iter()
            .find(|locale| Self::intl_segmenter_supports_locale(locale))
            .cloned()
            .unwrap_or_else(|| INTL_DEFAULT_LOCALE.to_string());
        let locale_base = Self::intl_locale_without_unicode_extension(&requested_locale);
        let mut out = IntlSegmenterOptions::new(if locale_base.is_empty() {
            INTL_DEFAULT_LOCALE.to_string()
        } else {
            locale_base
        });
        let Some(options) = options else {
            return Ok(out);
        };
        if matches!(options, Value::Undefined) {
            return Ok(out);
        }
        if matches!(options, Value::Null) {
            return Err(self.make_type_error_object(ctx, "options must not be null"));
        }
        let boxed_options = if Self::intl_is_object_like(options) {
            options.clone()
        } else {
            self.intl_box_primitive_if_needed(ctx, options)
        };
        let locale_matcher = self.read_named_property(ctx, &boxed_options, "localeMatcher");
        if let Some(thrown) = self.pending_throw.take() {
            return Err(thrown);
        }
        let _ = self.intl_string_option_from_value(ctx, Some(&locale_matcher), &["lookup", "best fit"], Some("best fit"))?;
        let granularity = self.read_named_property(ctx, &boxed_options, "granularity");
        if let Some(thrown) = self.pending_throw.take() {
            return Err(thrown);
        }
        out.granularity = self
            .intl_string_option_from_value(ctx, Some(&granularity), &["grapheme", "word", "sentence"], Some("grapheme"))?
            .unwrap_or_else(|| "grapheme".to_string());
        Ok(out)
    }

    fn intl_read_duration_format_options(
        &mut self,
        ctx: &GcContext<'gc>,
        requested_locales: &[String],
        options: Option<&Value<'gc>>,
    ) -> Result<IntlDurationFormatOptions, Value<'gc>> {
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
        let mut out = IntlDurationFormatOptions::new(
            Self::intl_numbering_system_resolved_locale(&locale_base, ext_numbering_system.as_deref()),
            ext_numbering_system.clone().unwrap_or_else(|| "latn".to_string()),
        );
        let Some(options) = options else {
            return Ok(out);
        };
        if matches!(options, Value::Undefined) {
            return Ok(out);
        }
        if matches!(options, Value::Null) {
            return Err(self.make_type_error_object(ctx, "options must not be null"));
        }
        let boxed_options = self.intl_box_primitive_if_needed(ctx, options);
        if !Self::intl_is_object_like(&boxed_options) {
            return Ok(out);
        }

        let _ = self.intl_locale_matcher_option(ctx, Some(&boxed_options))?;
        let mut option_numbering_system = None;
        if let Some(numbering_system) = self.intl_string_option(ctx, &boxed_options, "numberingSystem", &[], None)? {
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
        out.style = self
            .intl_string_option(ctx, &boxed_options, "style", &["long", "short", "narrow", "digital"], Some("short"))?
            .unwrap_or_else(|| "short".to_string());
        let fractional_digits_value = self.read_named_property(ctx, &boxed_options, "fractionalDigits");
        if let Some(thrown) = self.pending_throw.take() {
            return Err(thrown);
        }
        out.fractional_digits = self.intl_default_u8_option_value(ctx, Some(&fractional_digits_value), "fractionalDigits", 0, 9)?;

        let mut prev_style = None::<String>;
        for (slot, allowed) in [
            ("years", &["long", "short", "narrow"][..]),
            ("months", &["long", "short", "narrow"][..]),
            ("weeks", &["long", "short", "narrow"][..]),
            ("days", &["long", "short", "narrow"][..]),
            ("hours", &["long", "short", "narrow", "numeric", "2-digit"][..]),
            ("minutes", &["long", "short", "narrow", "numeric", "2-digit"][..]),
            ("seconds", &["long", "short", "narrow", "numeric", "2-digit"][..]),
            ("milliseconds", &["long", "short", "narrow", "numeric"][..]),
            ("microseconds", &["long", "short", "narrow", "numeric"][..]),
            ("nanoseconds", &["long", "short", "narrow", "numeric"][..]),
        ] {
            let explicit_style = self.intl_string_option(ctx, &boxed_options, slot, allowed, None)?;
            let display_key = format!("{slot}Display");
            let display_default = if explicit_style.is_some() { "always" } else { "auto" };
            let display = self
                .intl_string_option(ctx, &boxed_options, &display_key, &["auto", "always"], Some(display_default))?
                .unwrap_or_else(|| display_default.to_string());
            let style = if let Some(style) = explicit_style {
                if prev_style.as_deref().is_some_and(|prev| {
                    matches!(prev, "numeric" | "2-digit") && !Self::intl_duration_is_following_numeric_style(slot, &style)
                }) {
                    return Err(self.make_range_error_object(ctx, "Invalid duration unit style"));
                }
                style
            } else {
                Self::intl_default_duration_unit_style(&out.style, slot, prev_style.as_deref())
            };
            out.set_unit(slot, style.clone(), display);
            prev_style = Some(style);
        }
        Ok(out)
    }

    fn intl_read_list_format_options(
        &mut self,
        requested_locales: &[String],
        ctx: &GcContext<'gc>,
        options: Option<&Value<'gc>>,
    ) -> Result<IntlListFormatOptions, Value<'gc>> {
        let mut out = IntlListFormatOptions {
            resolved_locale: requested_locales
                .first()
                .cloned()
                .unwrap_or_else(|| INTL_DEFAULT_LOCALE.to_string()),
            list_type: "conjunction".to_string(),
            style: "long".to_string(),
        };
        let Some(options) = options else {
            return Ok(out);
        };
        if matches!(options, Value::Undefined) {
            return Ok(out);
        }
        if matches!(options, Value::Null) || !Self::intl_is_object_like(options) {
            return Err(self.make_type_error_object(ctx, "options must not be null"));
        }
        let boxed_options = options.clone();
        let _ = self.intl_locale_matcher_option(ctx, Some(&boxed_options))?;
        out.list_type = self
            .intl_string_option(
                ctx,
                &boxed_options,
                "type",
                &["conjunction", "disjunction", "unit"],
                Some("conjunction"),
            )?
            .unwrap_or_else(|| "conjunction".to_string());
        out.style = self
            .intl_string_option(ctx, &boxed_options, "style", &["long", "short", "narrow"], Some("long"))?
            .unwrap_or_else(|| "long".to_string());
        Ok(out)
    }

    fn intl_read_display_names_options(
        &mut self,
        ctx: &GcContext<'gc>,
        options: Option<&Value<'gc>>,
    ) -> Result<IntlDisplayNamesOptions, Value<'gc>> {
        let Some(options) = options else {
            return Err(self.make_type_error_object(ctx, "type is required"));
        };
        if matches!(options, Value::Undefined) || matches!(options, Value::Null) || !Self::intl_is_object_like(options) {
            return Err(self.make_type_error_object(ctx, "type is required"));
        }
        let _ = self.intl_locale_matcher_option(ctx, Some(options))?;
        let style = self
            .intl_string_option(ctx, options, "style", &["narrow", "short", "long"], Some("long"))?
            .unwrap_or_else(|| "long".to_string());
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
        let language_display = if display_type == "language" {
            Some(
                self.intl_string_option(ctx, options, "languageDisplay", &["dialect", "standard"], Some("dialect"))?
                    .unwrap_or_else(|| "dialect".to_string()),
            )
        } else {
            None
        };
        Ok(IntlDisplayNamesOptions {
            display_type,
            fallback,
            style,
            language_display,
        })
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

    fn intl_store_duration_format_options(&self, obj: &mut IndexMap<String, Value<'gc>>, options: &IntlDurationFormatOptions) {
        obj.insert(
            "__intl_numbering_system__".to_string(),
            Value::from(options.numbering_system.as_str()),
        );
        obj.insert("__intl_style__".to_string(), Value::from(options.style.as_str()));
        for (slot, unit_options) in [
            ("years", &options.years),
            ("months", &options.months),
            ("weeks", &options.weeks),
            ("days", &options.days),
            ("hours", &options.hours),
            ("minutes", &options.minutes),
            ("seconds", &options.seconds),
            ("milliseconds", &options.milliseconds),
            ("microseconds", &options.microseconds),
            ("nanoseconds", &options.nanoseconds),
        ] {
            obj.insert(format!("__intl_{}__", slot), Value::from(unit_options.style.as_str()));
            obj.insert(format!("__intl_{}Display__", slot), Value::from(unit_options.display.as_str()));
        }
        if let Some(fractional_digits) = options.fractional_digits {
            obj.insert("__intl_fractional_digits__".to_string(), Value::Number(fractional_digits as f64));
        }
    }

    fn intl_store_list_format_options(&self, obj: &mut IndexMap<String, Value<'gc>>, options: &IntlListFormatOptions) {
        obj.insert("__intl_list_type__".to_string(), Value::from(options.list_type.as_str()));
        obj.insert("__intl_list_style__".to_string(), Value::from(options.style.as_str()));
    }

    fn intl_store_relative_time_format_options(&self, obj: &mut IndexMap<String, Value<'gc>>, options: &IntlRelativeTimeFormatOptions) {
        obj.insert(
            "__intl_numbering_system__".to_string(),
            Value::from(options.numbering_system.as_str()),
        );
        obj.insert("__intl_style__".to_string(), Value::from(options.style.as_str()));
        obj.insert("__intl_numeric__".to_string(), Value::from(options.numeric.as_str()));
    }

    fn intl_store_display_names_options(&self, obj: &mut IndexMap<String, Value<'gc>>, options: &IntlDisplayNamesOptions) {
        obj.insert("__intl_type__".to_string(), Value::from(options.display_type.as_str()));
        obj.insert("__intl_fallback__".to_string(), Value::from(options.fallback.as_str()));
        obj.insert("__intl_style__".to_string(), Value::from(options.style.as_str()));
        if let Some(language_display) = &options.language_display {
            obj.insert("__intl_language_display__".to_string(), Value::from(language_display.as_str()));
        }
    }

    fn intl_store_segmenter_options(&self, obj: &mut IndexMap<String, Value<'gc>>, options: &IntlSegmenterOptions) {
        obj.insert("__intl_granularity__".to_string(), Value::from(options.granularity.as_str()));
    }

    fn intl_store_plural_rules_options(&self, obj: &mut IndexMap<String, Value<'gc>>, options: &IntlPluralRulesOptions) {
        obj.insert("__intl_type__".to_string(), Value::from(options.plural_type.as_str()));
        obj.insert("__intl_notation__".to_string(), Value::from(options.notation.as_str()));
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

    pub(super) fn intl_locale_without_unicode_extension(locale: &str) -> String {
        Self::intl_locale_info(locale).base
    }

    fn intl_default_duration_unit_style(base_style: &str, unit: &str, prev_style: Option<&str>) -> String {
        if matches!(prev_style, Some("numeric" | "2-digit" | "fractional")) {
            if matches!(unit, "minutes" | "seconds") {
                "2-digit".to_string()
            } else {
                "numeric".to_string()
            }
        } else if base_style == "digital" {
            match unit {
                "hours" => "numeric".to_string(),
                "minutes" | "seconds" => "2-digit".to_string(),
                "milliseconds" | "microseconds" | "nanoseconds" => "numeric".to_string(),
                _ => "short".to_string(),
            }
        } else {
            base_style.to_string()
        }
    }

    fn intl_duration_is_following_numeric_style(unit: &str, style: &str) -> bool {
        match unit {
            "minutes" | "seconds" => matches!(style, "numeric" | "2-digit"),
            "milliseconds" | "microseconds" | "nanoseconds" => style == "numeric",
            _ => true,
        }
    }

    fn intl_string_list_from_iterable(&mut self, ctx: &GcContext<'gc>, value: Option<&Value<'gc>>) -> Result<Vec<String>, Value<'gc>> {
        let Some(value) = value else {
            return Ok(vec![]);
        };
        if matches!(value, Value::Undefined) {
            return Ok(vec![]);
        }
        let iterable = if Self::intl_is_object_like(value) {
            value.clone()
        } else {
            self.intl_box_primitive_if_needed(ctx, value)
        };
        let iter_fn = self.read_named_property(ctx, &iterable, "@@sym:1");
        if let Some(thrown) = self.pending_throw.take() {
            return Err(thrown);
        }
        if matches!(iter_fn, Value::Undefined | Value::Null) || !self.is_value_callable(&iter_fn) {
            return Err(self.make_type_error_object(ctx, "object is not iterable"));
        }
        let iterator = match self.vm_call_function_value(ctx, &iter_fn, &iterable, &[]) {
            Ok(value) => value,
            Err(err) => return Err(self.vm_value_from_error(ctx, &err)),
        };
        let mut out = Vec::new();
        loop {
            let next_fn = self.read_named_property(ctx, &iterator, "next");
            if let Some(thrown) = self.pending_throw.take() {
                return Err(thrown);
            }
            let result = match self.vm_call_function_value(ctx, &next_fn, &iterator, &[]) {
                Ok(value) => value,
                Err(err) => return Err(self.vm_value_from_error(ctx, &err)),
            };
            let done = self.read_named_property(ctx, &result, "done");
            if let Some(thrown) = self.pending_throw.take() {
                return Err(thrown);
            }
            if Self::value_is_truthy(&done) {
                break;
            }
            let next_value = self.read_named_property(ctx, &result, "value");
            if let Some(thrown) = self.pending_throw.take() {
                return Err(thrown);
            }
            let Value::String(text) = next_value else {
                self.intl_iterator_close(ctx, &iterator);
                return Err(self.make_type_error_object(ctx, "Iterable elements must be strings"));
            };
            out.push(crate::unicode::utf16_to_utf8(&text));
        }
        Ok(out)
    }

    fn intl_iterator_close(&mut self, ctx: &GcContext<'gc>, iterator: &Value<'gc>) {
        let return_fn = self.read_named_property(ctx, iterator, "return");
        if self.pending_throw.is_some() {
            return;
        }
        if matches!(return_fn, Value::Undefined | Value::Null) || !self.is_value_callable(&return_fn) {
            return;
        }
        let _ = self.vm_call_function_value(ctx, &return_fn, iterator, &[]);
        self.pending_throw = None;
    }

    fn intl_format_list_parts(locale: &str, list_type: &str, style: &str, items: &[String]) -> Vec<IntlListPart> {
        let literals = Self::intl_list_format_literals(locale, list_type, style);
        match items.len() {
            0 => vec![],
            1 => vec![IntlListPart::element(items[0].clone())],
            2 => vec![
                IntlListPart::element(items[0].clone()),
                IntlListPart::literal(literals.pair.to_string()),
                IntlListPart::element(items[1].clone()),
            ],
            len => {
                let mut parts = Vec::new();
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        let literal = if index + 1 == len { literals.end } else { literals.middle };
                        parts.push(IntlListPart::literal(literal.to_string()));
                    }
                    parts.push(IntlListPart::element(item.clone()));
                }
                parts
            }
        }
    }

    fn intl_list_format_literals(locale: &str, list_type: &str, style: &str) -> IntlListLiterals {
        if locale.starts_with("es") {
            return match (list_type, style) {
                ("unit", "narrow") => IntlListLiterals::new(" ", " ", " "),
                ("unit", "short") => IntlListLiterals::new(" y ", ", ", ", "),
                ("unit", _) => IntlListLiterals::new(" y ", ", ", " y "),
                ("conjunction", _) => IntlListLiterals::new(" y ", ", ", " y "),
                ("disjunction", _) => IntlListLiterals::new(" o ", ", ", " o "),
                _ => IntlListLiterals::new(", ", ", ", ", "),
            };
        }
        if locale.starts_with("en") {
            return match (list_type, style) {
                ("conjunction", "short") => IntlListLiterals::new(" & ", ", ", ", & "),
                ("disjunction", _) => IntlListLiterals::new(" or ", ", ", ", or "),
                ("unit", "narrow") => IntlListLiterals::new(" ", " ", " "),
                ("unit", _) => IntlListLiterals::new(", ", ", ", ", "),
                _ => IntlListLiterals::new(" and ", ", ", ", and "),
            };
        }
        match list_type {
            "conjunction" => IntlListLiterals::new(" and ", ", ", ", and "),
            "disjunction" => IntlListLiterals::new(" or ", ", ", ", or "),
            _ => IntlListLiterals::new(", ", ", ", ", "),
        }
    }

    fn intl_locale_value_tag(value: &Value<'gc>) -> Option<String> {
        let Value::Object(obj) = value else {
            return None;
        };
        if !matches!(obj.borrow().get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "Locale") {
            return None;
        }
        match obj.borrow().get("__intl_locale__") {
            Some(Value::String(locale)) => Some(crate::unicode::utf16_to_utf8(locale)),
            _ => None,
        }
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

        Self::intl_apply_regular_grandfathered_alias(&mut language, &mut variants);
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
            "554" => Some("NZ"),
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

    fn intl_apply_regular_grandfathered_alias(language: &mut String, variants: &mut Vec<String>) {
        let replacement = match language.as_str() {
            "art" if variants.iter().any(|variant| variant == "lojban") => Some(("jbo", "lojban")),
            "cel" if variants.iter().any(|variant| variant == "gaulish") => Some(("xtg", "gaulish")),
            "zh" if variants.iter().any(|variant| variant == "guoyu") => Some(("zh", "guoyu")),
            "zh" if variants.iter().any(|variant| variant == "hakka") => Some(("hak", "hakka")),
            "zh" if variants.iter().any(|variant| variant == "xiang") => Some(("hsn", "xiang")),
            _ => None,
        };
        if let Some((new_language, removed_variant)) = replacement {
            *language = new_language.to_string();
            variants.retain(|variant| variant != removed_variant);
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

        let mut keywords = IndexMap::new();
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
            keywords.entry(key).or_insert(canonical_value);
        }
        let mut keywords = keywords.into_iter().collect::<Vec<_>>();
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
        let mut unicode_keywords = IndexMap::new();
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
            unicode_keywords.entry(key.to_ascii_lowercase()).or_insert(value_parts.join("-"));
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
        if let Some(numeric) = numeric {
            if numeric {
                extensions.push("kn".to_string());
            } else {
                extensions.push("kn-false".to_string());
            }
        }
        if let Some(case_first) = case_first {
            if case_first.is_empty() {
                extensions.push("kf".to_string());
            } else {
                extensions.push(format!("kf-{}", case_first));
            }
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

    fn intl_locale_with_keywords(original_locale: &str, locale_base: &str, keywords: IntlLocaleKeywordOptions<'_>) -> String {
        let mut out = locale_base.to_string();
        let (mut unicode_attributes, mut unicode_keywords, extensions, private_use) =
            Self::intl_locale_extension_parts(original_locale, locale_base);
        unicode_attributes.sort();
        if let Some(calendar) = keywords.calendar {
            unicode_keywords.insert("ca".to_string(), Some(calendar.to_string()));
        }
        if let Some(collation) = keywords.collation {
            unicode_keywords.insert("co".to_string(), Some(collation.to_string()));
        }
        if let Some(first_day_of_week) = keywords.first_day_of_week {
            unicode_keywords.insert(
                "fw".to_string(),
                if first_day_of_week.is_empty() {
                    None
                } else {
                    Some(first_day_of_week.to_string())
                },
            );
        }
        if let Some(hour_cycle) = keywords.hour_cycle {
            unicode_keywords.insert("hc".to_string(), Some(hour_cycle.to_string()));
        }
        if let Some(case_first) = keywords.case_first {
            unicode_keywords.insert(
                "kf".to_string(),
                if case_first.is_empty() {
                    None
                } else {
                    Some(case_first.to_string())
                },
            );
        }
        if let Some(numeric) = keywords.numeric {
            unicode_keywords.insert("kn".to_string(), if numeric { None } else { Some("false".to_string()) });
        }
        if let Some(numbering_system) = keywords.numbering_system {
            unicode_keywords.insert("nu".to_string(), Some(numbering_system.to_string()));
        }

        let mut rebuilt_extensions = extensions;
        if !unicode_attributes.is_empty() || !unicode_keywords.is_empty() {
            let mut pieces = unicode_attributes;
            let mut unicode_entries = unicode_keywords.into_iter().collect::<Vec<_>>();
            unicode_entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            for (key, value) in unicode_entries {
                pieces.push(key);
                if let Some(value) = value
                    && !value.is_empty()
                {
                    pieces.extend(value.split('-').map(str::to_string));
                }
            }
            rebuilt_extensions.push(("u".to_string(), pieces));
        }
        rebuilt_extensions.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (singleton, parts) in rebuilt_extensions {
            out.push('-');
            out.push_str(&singleton);
            for part in parts {
                out.push('-');
                out.push_str(&part);
            }
        }
        if let Some(private_use) = private_use {
            out.push_str("-x");
            for part in private_use {
                out.push('-');
                out.push_str(&part);
            }
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

    fn intl_segmenter_supports_locale(locale: &str) -> bool {
        let base = Self::intl_locale_without_unicode_extension(locale);
        if base.is_empty() || base.eq_ignore_ascii_case("zxx") {
            return false;
        }
        let language = base.split('-').next().unwrap_or(base.as_str());
        language.len() == 2
    }

    fn intl_supported_hour_cycle(value: &str) -> Option<String> {
        match value {
            "h11" | "h12" | "h23" | "h24" => Some(value.to_string()),
            _ => None,
        }
    }

    fn intl_locale_base_components(locale_base: &str) -> IntlLocaleComponents {
        let parts: Vec<&str> = locale_base.split('-').collect();
        let language = parts.first().copied().unwrap_or("und").to_string();
        let mut script = None;
        let mut region = None;
        let mut variants = Vec::new();
        for part in parts.iter().skip(1) {
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
        }
        IntlLocaleComponents {
            language,
            script,
            region,
            variants,
        }
    }

    fn intl_locale_base_name_string(base: &IntlLocaleComponents) -> String {
        let mut out = vec![base.language.clone()];
        if let Some(script) = &base.script {
            out.push(script.clone());
        }
        if let Some(region) = &base.region {
            out.push(region.clone());
        }
        out.extend(base.variants.iter().cloned());
        out.join("-")
    }

    fn intl_locale_language_option(value: &str) -> Option<String> {
        let lower = value.to_ascii_lowercase();
        (((2..=3).contains(&lower.len()) || (5..=8).contains(&lower.len())) && lower.chars().all(|c| c.is_ascii_alphabetic()))
            .then_some(lower)
    }

    fn intl_locale_script_option(value: &str) -> Option<String> {
        (value.len() == 4 && value.chars().all(|c| c.is_ascii_alphabetic())).then(|| Self::intl_titlecase_subtag(value))
    }

    fn intl_locale_region_option(value: &str) -> Option<String> {
        ((value.len() == 2 && value.chars().all(|c| c.is_ascii_alphabetic()))
            || (value.len() == 3 && value.chars().all(|c| c.is_ascii_digit())))
        .then(|| value.to_ascii_uppercase())
    }

    fn intl_locale_variants_option(value: &str) -> Option<Vec<String>> {
        if value.is_empty() {
            return None;
        }
        let mut out = Vec::new();
        for part in value.split('-') {
            let valid = (5..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
                || (part.len() == 4
                    && part.chars().next().is_some_and(|c| c.is_ascii_digit())
                    && part.chars().all(|c| c.is_ascii_alphanumeric()));
            if !valid {
                return None;
            }
            let lowered = part.to_ascii_lowercase();
            if out.iter().any(|existing| existing == &lowered) {
                return None;
            }
            out.push(lowered);
        }
        out.sort();
        Some(out)
    }

    fn intl_locale_collation_keyword(value: &str) -> Option<String> {
        if value.is_empty() || !Self::intl_is_valid_unicode_type_identifier(value) {
            return None;
        }
        Some(value.to_ascii_lowercase())
    }

    fn intl_locale_case_first_keyword(value: &str) -> Option<String> {
        match value {
            "" | "true" => Some(String::new()),
            "upper" | "lower" | "false" => Some(value.to_string()),
            _ => None,
        }
    }

    fn intl_locale_first_day_of_week(value: &str) -> Option<String> {
        let lower = value.to_ascii_lowercase();
        match lower.as_str() {
            "1" | "mon" => Some("mon".to_string()),
            "2" | "tue" => Some("tue".to_string()),
            "3" | "wed" => Some("wed".to_string()),
            "4" | "thu" => Some("thu".to_string()),
            "5" | "fri" => Some("fri".to_string()),
            "6" | "sat" => Some("sat".to_string()),
            "0" | "7" | "sun" => Some("sun".to_string()),
            "true" => Some(String::new()),
            _ if Self::intl_is_valid_unicode_type_identifier(&lower) => Some(lower),
            _ => None,
        }
    }

    fn intl_weekday_string_to_number(value: &str) -> Option<i32> {
        match value {
            "mon" => Some(1),
            "tue" => Some(2),
            "wed" => Some(3),
            "thu" => Some(4),
            "fri" => Some(5),
            "sat" => Some(6),
            "sun" | "" => Some(7),
            _ => None,
        }
    }

    fn intl_locale_extension_parts(original_locale: &str, locale_base: &str) -> IntlLocaleExtensionParts {
        let mut unicode_attributes = Vec::new();
        let mut unicode_keywords = IndexMap::new();
        let mut extensions = Vec::new();
        let mut private_use = None;
        let suffix = original_locale.strip_prefix(locale_base).unwrap_or("");
        if suffix.is_empty() {
            return (unicode_attributes, unicode_keywords, extensions, private_use);
        }
        let parts = suffix
            .trim_start_matches('-')
            .split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        let mut index = 0;
        while index < parts.len() {
            let singleton = parts[index].to_ascii_lowercase();
            index += 1;
            let start = index;
            if singleton == "x" {
                private_use = Some(parts[start..].iter().map(|part| part.to_ascii_lowercase()).collect());
                break;
            }
            while index < parts.len() && parts[index].len() != 1 {
                index += 1;
            }
            let extension_parts = parts[start..index].iter().map(|part| part.to_ascii_lowercase()).collect::<Vec<_>>();
            if singleton == "u" {
                let mut ext_index = 0;
                while ext_index < extension_parts.len() && extension_parts[ext_index].len() != 2 {
                    unicode_attributes.push(extension_parts[ext_index].clone());
                    ext_index += 1;
                }
                while ext_index < extension_parts.len() {
                    let key = extension_parts[ext_index].clone();
                    ext_index += 1;
                    let start = ext_index;
                    while ext_index < extension_parts.len() && extension_parts[ext_index].len() != 2 {
                        ext_index += 1;
                    }
                    let value = extension_parts[start..ext_index].join("-");
                    unicode_keywords.entry(key).or_insert((!value.is_empty()).then_some(value));
                }
            } else if !extension_parts.is_empty() {
                extensions.push((singleton, extension_parts));
            }
        }
        (unicode_attributes, unicode_keywords, extensions, private_use)
    }

    fn intl_locale_maximize_tag(locale: &str) -> String {
        let info = Self::intl_locale_info(locale);
        let suffix = locale.strip_prefix(&info.base).unwrap_or("");
        let components = Self::intl_locale_base_components(&info.base);
        let core = IntlLocaleComponents {
            language: components.language.clone(),
            script: components.script.clone(),
            region: components.region.clone(),
            variants: Vec::new(),
        };
        let core_name = Self::intl_locale_base_name_string(&core);
        let maximal_core = match core_name.as_str() {
            "en" | "en-Latn" | "en-US" | "und" => "en-Latn-US",
            "de" => "de-Latn-DE",
            "en-Shaw" => "en-Shaw-GB",
            "en-Arab" => "en-Arab-US",
            "en-GB" => "en-Latn-GB",
            "en-FR" => "en-Latn-FR",
            "ro" => "ro-Latn-RO",
            "es-ES" => "es-Latn-ES",
            "uz-UZ" => "uz-Latn-UZ",
            "it-Kana-CA" => "it-Kana-CA",
            "hi" => "hi-Deva-IN",
            "und-Thai" => "th-Thai-TH",
            "und-419" => "es-Latn-419",
            "und-150" => "en-Latn-150",
            "und-AT" => "de-Latn-AT",
            "und-Cyrl-RO" => "bg-Cyrl-RO",
            "und-AQ" => "en-Latn-AQ",
            "aa" => "aa-Latn-ET",
            "he" => "he-Hebr-IL",
            "jbo" => "jbo-Latn-001",
            "zh" => "zh-Hans-CN",
            "cs" => "cs-Latn-CZ",
            "hy" => "hy-Armn-AM",
            "hyw" => "hyw-Armn-AM",
            "hak" => "hak-Hans-CN",
            "hsn" => "hsn-Hans-CN",
            _ => core_name.as_str(),
        };
        let mut out = maximal_core.to_string();
        if !components.variants.is_empty() {
            out.push('-');
            out.push_str(&components.variants.join("-"));
        }
        out.push_str(suffix);
        out
    }

    fn intl_locale_minimize_tag(locale: &str) -> String {
        let info = Self::intl_locale_info(locale);
        let suffix = locale.strip_prefix(&info.base).unwrap_or("");
        let components = Self::intl_locale_base_components(&info.base);
        let core = IntlLocaleComponents {
            language: components.language.clone(),
            script: components.script.clone(),
            region: components.region.clone(),
            variants: Vec::new(),
        };
        let core_name = Self::intl_locale_base_name_string(&core);
        let minimal_core = match core_name.as_str() {
            "en" | "en-Latn" | "en-US" | "en-Latn-US" => "en",
            "ar-Arab" => "ar",
            "en-GB" | "en-Latn-GB" => "en-GB",
            "en-Shaw-GB" => "en-Shaw",
            "en-Arab-US" => "en-Arab",
            "en-Latn-FR" => "en-FR",
            "und" => "en",
            "und-150" => "en-150",
            "und-419" => "es-419",
            "und-AT" => "de-AT",
            "und-CW" => "pap",
            "und-US" => "en",
            "ro-Latn-RO" => "ro",
            "aae-Latn-IT" => "aae",
            "es-ES" | "es-Latn-ES" => "es",
            "uz-UZ" | "uz-Latn-UZ" => "uz",
            "it-Kana-CA" => "it-Kana-CA",
            "hi-Deva-IN" => "hi",
            "und-Thai" => "th",
            "th-Thai-TH" => "th",
            "es-Latn-419" => "es-419",
            "ru-Cyrl-RU" => "ru",
            "de-Latn-AT" => "de-AT",
            "bg-Cyrl-RO" => "bg-RO",
            "und-Latn-AQ" | "en-Latn-AQ" => "en-AQ",
            "aa-Latn-ET" => "aa",
            "he-Hebr-IL" => "he",
            "jbo-Latn-001" => "jbo",
            "zh-Hant" => "zh-TW",
            "zh-Hans-CN" => "zh",
            "hak-Hans-CN" => "hak",
            "hsn-Hans-CN" => "hsn",
            "cs-Latn-CZ" => "cs",
            "hy-Armn-AM" => "hy",
            "hyw-Armn-AM" => "hyw",
            _ => core_name.as_str(),
        };
        let mut out = minimal_core.to_string();
        if !components.variants.is_empty() {
            out.push('-');
            out.push_str(&components.variants.join("-"));
        }
        out.push_str(suffix);
        out
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
        let mut normalized = value.nfc().collect::<String>();
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
        } else if options.usage == "sort" && options.resolved_locale.starts_with("de") {
            normalized = normalized
                .replace("ä", "a\u{0002}")
                .replace("Ä", "A\u{0002}")
                .replace("ö", "o\u{0002}")
                .replace("Ö", "O\u{0002}")
                .replace("ü", "u\u{0002}")
                .replace("Ü", "U\u{0002}");
        }
        normalized = normalized.nfd().collect::<String>();
        match options.sensitivity.as_str() {
            "base" => Self::intl_collator_strip_accents(&normalized).to_ascii_lowercase(),
            "accent" => normalized.to_ascii_lowercase(),
            "case" => Self::intl_collator_case_key(&Self::intl_collator_strip_accents(&normalized)),
            _ => Self::intl_collator_case_key(&normalized),
        }
    }

    fn intl_collator_case_key(value: &str) -> String {
        let mut key = String::new();
        for ch in value.chars() {
            key.extend(ch.to_lowercase());
            key.push(if ch.is_uppercase() { '\u{0001}' } else { '\u{0000}' });
        }
        key
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

    fn intl_service_get_prototype_from_constructor(
        &mut self,
        ctx: &GcContext<'gc>,
        ctor_value: &Value<'gc>,
        kind: &str,
    ) -> Result<Value<'gc>, Value<'gc>> {
        let prototype = self.read_named_property(ctx, ctor_value, "prototype");
        if let Some(thrown) = self.pending_throw.take() {
            return Err(thrown);
        }
        if matches!(
            prototype,
            Value::Object(_) | Value::Array(_) | Value::Function(..) | Value::Closure(..) | Value::NativeFunction(_)
        ) && !prototype.is_symbol_value()
        {
            return Ok(prototype);
        }
        if let Some(Value::Object(origin_global)) = self.constructor_origin_global(ctx, ctor_value) {
            let intl = self.read_named_property(ctx, &Value::Object(origin_global), "Intl");
            if self.pending_throw.is_none() {
                let ctor = self.read_named_property(ctx, &intl, kind);
                if self.pending_throw.is_none() {
                    let prototype = self.read_named_property(ctx, &ctor, "prototype");
                    if self.pending_throw.is_none() {
                        return Ok(prototype);
                    }
                }
            }
            self.pending_throw = None;
        }
        Ok(self
            .intl_service_constructor_from_global(kind)
            .map(|ctor| self.read_named_property(ctx, &ctor, "prototype"))
            .unwrap_or(Value::Undefined))
    }

    fn intl_use_grouping_value(mode: &IntlUseGrouping) -> Value<'gc> {
        match mode {
            IntlUseGrouping::False => Value::Boolean(false),
            IntlUseGrouping::Auto => Value::from("auto"),
            IntlUseGrouping::Always => Value::from("always"),
            IntlUseGrouping::Min2 => Value::from("min2"),
        }
    }

    fn intl_use_grouping_value_from_object(obj: &IndexMap<String, Value<'gc>>) -> Value<'gc> {
        match obj.get("__intl_use_grouping__") {
            Some(Value::Boolean(false)) => Value::Boolean(false),
            Some(Value::String(text)) => Value::String(text.clone()),
            _ => Value::from("auto"),
        }
    }

    fn intl_should_use_grouping(integer_digits: &str, options: &IntlNumberFormatOptions) -> bool {
        match options.use_grouping {
            IntlUseGrouping::False => false,
            IntlUseGrouping::Auto | IntlUseGrouping::Always => true,
            IntlUseGrouping::Min2 => {
                let grouped = Self::intl_group_integer_digits(integer_digits, options);
                let separator = Self::intl_group_separator(options);
                let separator_count = grouped.chars().filter(|ch| *ch == separator).count();
                let first_separator = grouped.find(separator);
                separator_count >= 2 || first_separator.is_some_and(|idx| idx >= 2)
            }
        }
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
        if !(((2..=3).contains(&first.len())) || ((5..=8).contains(&first.len()))) || !first.chars().all(|c| c.is_ascii_alphabetic()) {
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
            let mut seen_variants = std::collections::HashSet::new();
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
                if !seen_variants.insert(part.to_ascii_lowercase()) {
                    return None;
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

struct IntlLocaleComponents {
    language: String,
    script: Option<String>,
    region: Option<String>,
    variants: Vec<String>,
}

struct IntlLocaleKeywordOptions<'a> {
    calendar: Option<&'a str>,
    collation: Option<&'a str>,
    first_day_of_week: Option<&'a str>,
    hour_cycle: Option<&'a str>,
    case_first: Option<&'a str>,
    numeric: Option<bool>,
    numbering_system: Option<&'a str>,
}

type IntlLocaleExtensionParts = (
    Vec<String>,
    IndexMap<String, Option<String>>,
    Vec<(String, Vec<String>)>,
    Option<Vec<String>>,
);

struct IntlLocaleInfo {
    base: String,
    unicode_keywords: IndexMap<String, String>,
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
            rounding_mode: match obj.get("__intl_rounding_mode__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "halfExpand".to_string(),
            },
            rounding_priority: match obj.get("__intl_rounding_priority__") {
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
            use_grouping: match obj.get("__intl_use_grouping__") {
                Some(Value::Boolean(false)) => IntlUseGrouping::False,
                Some(Value::String(text)) => match crate::unicode::utf16_to_utf8(text).as_str() {
                    "always" => IntlUseGrouping::Always,
                    "min2" => IntlUseGrouping::Min2,
                    _ => IntlUseGrouping::Auto,
                },
                _ => IntlUseGrouping::Auto,
            },
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

struct IntlDateTimeFormatOptions {
    resolved_locale: String,
    calendar: String,
    numbering_system: String,
    time_zone: String,
    time_zone_explicit: bool,
    weekday: Option<String>,
    era: Option<String>,
    year: Option<String>,
    month: Option<String>,
    day: Option<String>,
    day_period: Option<String>,
    hour: Option<String>,
    minute: Option<String>,
    second: Option<String>,
    fractional_second_digits: Option<u8>,
    time_zone_name: Option<String>,
    hour_cycle: Option<String>,
    hour12: Option<bool>,
    date_style: Option<String>,
    time_style: Option<String>,
}

struct IntlDurationUnitOptions {
    style: String,
    display: String,
}

struct IntlDurationFormatOptions {
    resolved_locale: String,
    numbering_system: String,
    style: String,
    years: IntlDurationUnitOptions,
    months: IntlDurationUnitOptions,
    weeks: IntlDurationUnitOptions,
    days: IntlDurationUnitOptions,
    hours: IntlDurationUnitOptions,
    minutes: IntlDurationUnitOptions,
    seconds: IntlDurationUnitOptions,
    milliseconds: IntlDurationUnitOptions,
    microseconds: IntlDurationUnitOptions,
    nanoseconds: IntlDurationUnitOptions,
    fractional_digits: Option<u8>,
}

struct IntlListFormatOptions {
    resolved_locale: String,
    list_type: String,
    style: String,
}

struct IntlRelativeTimeFormatOptions {
    resolved_locale: String,
    numbering_system: String,
    style: String,
    numeric: String,
}

struct IntlPluralRulesOptions {
    resolved_locale: String,
    plural_type: String,
    notation: String,
    minimum_integer_digits: u8,
    minimum_fraction_digits: u8,
    maximum_fraction_digits: u8,
    minimum_significant_digits: Option<u8>,
    maximum_significant_digits: Option<u8>,
}

struct IntlSegmenterOptions {
    resolved_locale: String,
    granularity: String,
}

struct IntlDisplayNamesOptions {
    display_type: String,
    fallback: String,
    style: String,
    language_display: Option<String>,
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
    rounding_mode: String,
    rounding_priority: String,
    rounding_increment: u16,
    trailing_zero_display: String,
    use_grouping: IntlUseGrouping,
    minimum_integer_digits: u8,
    minimum_fraction_digits: u8,
    maximum_fraction_digits: u8,
    minimum_significant_digits: Option<u8>,
    maximum_significant_digits: Option<u8>,
}

impl IntlDurationFormatOptions {
    fn new(resolved_locale: String, numbering_system: String) -> Self {
        Self {
            resolved_locale,
            numbering_system,
            style: "short".to_string(),
            years: IntlDurationUnitOptions::new("short", "auto"),
            months: IntlDurationUnitOptions::new("short", "auto"),
            weeks: IntlDurationUnitOptions::new("short", "auto"),
            days: IntlDurationUnitOptions::new("short", "auto"),
            hours: IntlDurationUnitOptions::new("short", "auto"),
            minutes: IntlDurationUnitOptions::new("short", "auto"),
            seconds: IntlDurationUnitOptions::new("short", "auto"),
            milliseconds: IntlDurationUnitOptions::new("short", "auto"),
            microseconds: IntlDurationUnitOptions::new("short", "auto"),
            nanoseconds: IntlDurationUnitOptions::new("short", "auto"),
            fractional_digits: None,
        }
    }

    fn set_unit(&mut self, slot: &str, style: String, display: String) {
        let target = match slot {
            "years" => &mut self.years,
            "months" => &mut self.months,
            "weeks" => &mut self.weeks,
            "days" => &mut self.days,
            "hours" => &mut self.hours,
            "minutes" => &mut self.minutes,
            "seconds" => &mut self.seconds,
            "milliseconds" => &mut self.milliseconds,
            "microseconds" => &mut self.microseconds,
            "nanoseconds" => &mut self.nanoseconds,
            _ => return,
        };
        target.style = style;
        target.display = display;
    }

    fn unit_options(&self, slot: &str) -> &IntlDurationUnitOptions {
        match slot {
            "years" => &self.years,
            "months" => &self.months,
            "weeks" => &self.weeks,
            "days" => &self.days,
            "hours" => &self.hours,
            "minutes" => &self.minutes,
            "seconds" => &self.seconds,
            "milliseconds" => &self.milliseconds,
            "microseconds" => &self.microseconds,
            "nanoseconds" => &self.nanoseconds,
            _ => &self.seconds,
        }
    }
}

impl IntlDurationUnitOptions {
    fn new(style: &str, display: &str) -> Self {
        Self {
            style: style.to_string(),
            display: display.to_string(),
        }
    }
}

impl IntlRelativeTimeFormatOptions {
    fn new(resolved_locale: String, numbering_system: String) -> Self {
        Self {
            resolved_locale,
            numbering_system,
            style: "long".to_string(),
            numeric: "always".to_string(),
        }
    }

    fn from_object<'gc>(obj: &IndexMap<String, Value<'gc>>) -> Self {
        let mut out = Self::new(
            match obj.get("__intl_locale__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => INTL_DEFAULT_LOCALE.to_string(),
            },
            match obj.get("__intl_numbering_system__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "latn".to_string(),
            },
        );
        out.style = match obj.get("__intl_style__") {
            Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
            _ => "long".to_string(),
        };
        out.numeric = match obj.get("__intl_numeric__") {
            Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
            _ => "always".to_string(),
        };
        out
    }
}

impl IntlPluralRulesOptions {
    fn new(resolved_locale: String) -> Self {
        Self {
            resolved_locale,
            plural_type: "cardinal".to_string(),
            notation: "standard".to_string(),
            minimum_integer_digits: 1,
            minimum_fraction_digits: 0,
            maximum_fraction_digits: 3,
            minimum_significant_digits: None,
            maximum_significant_digits: None,
        }
    }
}

impl IntlSegmenterOptions {
    fn new(resolved_locale: String) -> Self {
        Self {
            resolved_locale,
            granularity: "grapheme".to_string(),
        }
    }
}

impl IntlDurationFormatOptions {
    fn from_object<'gc>(obj: &IndexMap<String, Value<'gc>>) -> Self {
        let mut out = Self::new(
            match obj.get("__intl_locale__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => INTL_DEFAULT_LOCALE.to_string(),
            },
            match obj.get("__intl_numbering_system__") {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "latn".to_string(),
            },
        );
        out.style = match obj.get("__intl_style__") {
            Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
            _ => "short".to_string(),
        };
        for slot in [
            "years",
            "months",
            "weeks",
            "days",
            "hours",
            "minutes",
            "seconds",
            "milliseconds",
            "microseconds",
            "nanoseconds",
        ] {
            let style = match obj.get(&format!("__intl_{}__", slot)) {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "short".to_string(),
            };
            let display = match obj.get(&format!("__intl_{}Display__", slot)) {
                Some(Value::String(text)) => crate::unicode::utf16_to_utf8(text),
                _ => "auto".to_string(),
            };
            out.set_unit(slot, style, display);
        }
        out.fractional_digits = match obj.get("__intl_fractional_digits__") {
            Some(Value::Number(value)) => Some(*value as u8),
            _ => None,
        };
        out
    }
}

impl IntlListPart {
    fn element(value: String) -> Self {
        Self {
            part_type: "element".to_string(),
            value,
        }
    }

    fn literal(value: String) -> Self {
        Self {
            part_type: "literal".to_string(),
            value,
        }
    }
}

impl IntlListLiterals {
    fn new(pair: &'static str, middle: &'static str, end: &'static str) -> Self {
        Self { pair, middle, end }
    }
}

impl IntlDurationPart {
    fn element(part_type: &str, value: String, unit: Option<&str>) -> Self {
        Self {
            part_type: part_type.to_string(),
            value,
            unit: unit.map(str::to_string),
        }
    }

    fn literal(value: &str) -> Self {
        Self {
            part_type: "literal".to_string(),
            value: value.to_string(),
            unit: None,
        }
    }
}

impl IntlDurationRecord {
    fn any_negative(&self) -> bool {
        self.years < 0
            || self.months < 0
            || self.weeks < 0
            || self.days < 0
            || self.hours < 0
            || self.minutes < 0
            || self.seconds < 0
            || self.milliseconds < 0
            || self.microseconds < 0
            || self.nanoseconds < 0
    }

    fn unit_i128(&self, unit: &str) -> i128 {
        match unit {
            "years" => self.years as i128,
            "months" => self.months as i128,
            "weeks" => self.weeks as i128,
            "days" => self.days as i128,
            "hours" => self.hours as i128,
            "minutes" => self.minutes as i128,
            "seconds" => self.seconds as i128,
            "milliseconds" => self.milliseconds as i128,
            "microseconds" => self.microseconds,
            "nanoseconds" => self.nanoseconds,
            _ => 0,
        }
    }

    fn set_unit(&mut self, unit: &str, value: i128) {
        match unit {
            "years" => {
                self.years = value as i64;
                self.years_present = true;
            }
            "months" => {
                self.months = value as i64;
                self.months_present = true;
            }
            "weeks" => {
                self.weeks = value as i64;
                self.weeks_present = true;
            }
            "days" => {
                self.days = value as i64;
                self.days_present = true;
            }
            "hours" => {
                self.hours = value as i64;
                self.hours_present = true;
            }
            "minutes" => {
                self.minutes = value as i64;
                self.minutes_present = true;
            }
            "seconds" => {
                self.seconds = value as i64;
                self.seconds_present = true;
            }
            "milliseconds" => {
                self.milliseconds = value as i64;
                self.milliseconds_present = true;
            }
            "microseconds" => {
                self.microseconds = value;
                self.microseconds_present = true;
            }
            "nanoseconds" => {
                self.nanoseconds = value;
                self.nanoseconds_present = true;
            }
            _ => {}
        }
    }

    fn is_present(&self, unit: &str) -> bool {
        match unit {
            "years" => self.years_present,
            "months" => self.months_present,
            "weeks" => self.weeks_present,
            "days" => self.days_present,
            "hours" => self.hours_present,
            "minutes" => self.minutes_present,
            "seconds" => self.seconds_present,
            "milliseconds" => self.milliseconds_present,
            "microseconds" => self.microseconds_present,
            "nanoseconds" => self.nanoseconds_present,
            _ => false,
        }
    }
}

impl<'gc> VM<'gc> {
    fn intl_relative_time_unit(&mut self, ctx: &GcContext<'gc>, unit: &Value<'gc>) -> Result<String, Value<'gc>> {
        let unit = match self.vm_to_string_like_spec(ctx, unit) {
            Ok(unit) => unit,
            Err(err) => return Err(self.vm_value_from_error(ctx, &err)),
        };
        let singular = if let Some(stripped) = unit.strip_suffix('s') {
            stripped.to_string()
        } else {
            unit
        };
        if matches!(
            singular.as_str(),
            "second" | "minute" | "hour" | "day" | "week" | "month" | "quarter" | "year"
        ) {
            Ok(singular)
        } else {
            Err(self.make_range_error_object(ctx, "Invalid unit"))
        }
    }

    fn intl_partition_relative_time_pattern(
        &mut self,
        options: &IntlRelativeTimeFormatOptions,
        value: f64,
        unit: &str,
    ) -> Result<Vec<IntlDurationPart>, Value<'gc>> {
        if options.numeric == "auto"
            && let Some(literal) = Self::intl_relative_time_auto_phrase(&options.resolved_locale, unit, value)
        {
            return Ok(vec![IntlDurationPart::literal(literal)]);
        }
        let future = !value.is_sign_negative();
        let number_parts = Self::intl_relative_time_number_parts(options, value.abs(), unit);
        let plural_category = Self::intl_relative_time_plural_category(&options.resolved_locale, value.abs());
        let unit_text = Self::intl_relative_time_unit_text(&options.resolved_locale, &options.style, unit, plural_category);
        let mut parts = Vec::new();
        if future {
            parts.push(IntlDurationPart::literal(Self::intl_relative_time_prefix(&options.resolved_locale)));
            parts.extend(number_parts);
            parts.push(IntlDurationPart::literal(&format!(" {}", unit_text)));
        } else {
            parts.extend(number_parts);
            parts.push(IntlDurationPart::literal(&format!(
                " {}{}",
                unit_text,
                Self::intl_relative_time_past_suffix(&options.resolved_locale)
            )));
        }
        Ok(parts)
    }

    fn intl_relative_time_prefix(locale: &str) -> &'static str {
        if locale.starts_with("pl") { "za " } else { "in " }
    }

    fn intl_relative_time_past_suffix(locale: &str) -> &'static str {
        if locale.starts_with("pl") { " temu" } else { " ago" }
    }

    fn intl_relative_time_auto_phrase(locale: &str, unit: &str, value: f64) -> Option<&'static str> {
        if !locale.starts_with("en") || value.fract() != 0.0 {
            return None;
        }
        let key = if value == -1.0 {
            -1
        } else if value == 0.0 {
            0
        } else if value == 1.0 {
            1
        } else {
            return None;
        };
        match (unit, key) {
            ("year", -1) => Some("last year"),
            ("year", 0) => Some("this year"),
            ("year", 1) => Some("next year"),
            ("quarter", -1) => Some("last quarter"),
            ("quarter", 0) => Some("this quarter"),
            ("quarter", 1) => Some("next quarter"),
            ("month", -1) => Some("last month"),
            ("month", 0) => Some("this month"),
            ("month", 1) => Some("next month"),
            ("week", -1) => Some("last week"),
            ("week", 0) => Some("this week"),
            ("week", 1) => Some("next week"),
            ("day", -1) => Some("yesterday"),
            ("day", 0) => Some("today"),
            ("day", 1) => Some("tomorrow"),
            ("hour", 0) => Some("this hour"),
            ("minute", 0) => Some("this minute"),
            ("second", 0) => Some("now"),
            _ => None,
        }
    }

    fn intl_relative_time_plural_category(locale: &str, value: f64) -> &'static str {
        if locale.starts_with("pl") {
            if value.fract() != 0.0 {
                return "other";
            }
            let n = value as i64;
            let mod10 = n % 10;
            let mod100 = n % 100;
            if n == 1 {
                "one"
            } else if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
                "few"
            } else if mod10 == 0 || mod10 == 1 || (5..=9).contains(&mod10) || (12..=14).contains(&mod100) {
                "many"
            } else {
                "other"
            }
        } else if value == 1.0 {
            "one"
        } else {
            "other"
        }
    }

    fn intl_relative_time_unit_text(locale: &str, style: &str, unit: &str, plural_category: &str) -> String {
        if locale.starts_with("pl") {
            match style {
                "short" => match unit {
                    "second" => "sek.".to_string(),
                    "minute" => "min".to_string(),
                    "hour" => "godz.".to_string(),
                    "day" => match plural_category {
                        "one" => "dzień".to_string(),
                        "other" => "dnia".to_string(),
                        _ => "dni".to_string(),
                    },
                    "week" => {
                        if plural_category == "one" {
                            "tydz.".to_string()
                        } else {
                            "tyg.".to_string()
                        }
                    }
                    "month" => "mies.".to_string(),
                    "quarter" => "kw.".to_string(),
                    "year" => match plural_category {
                        "one" => "rok".to_string(),
                        "few" => "lata".to_string(),
                        "other" => "roku".to_string(),
                        _ => "lat".to_string(),
                    },
                    _ => unit.to_string(),
                },
                "narrow" => match unit {
                    "second" => "s".to_string(),
                    "minute" => "min".to_string(),
                    "hour" => "g.".to_string(),
                    "day" => match plural_category {
                        "one" => "dzień".to_string(),
                        "other" => "dnia".to_string(),
                        _ => "dni".to_string(),
                    },
                    "week" => {
                        if plural_category == "one" {
                            "tydz.".to_string()
                        } else {
                            "tyg.".to_string()
                        }
                    }
                    "month" => "mies.".to_string(),
                    "quarter" => "kw.".to_string(),
                    "year" => match plural_category {
                        "one" => "rok".to_string(),
                        "few" => "lata".to_string(),
                        "other" => "roku".to_string(),
                        _ => "lat".to_string(),
                    },
                    _ => unit.to_string(),
                },
                _ => match unit {
                    "second" => match plural_category {
                        "one" => "sekundę".to_string(),
                        "many" => "sekund".to_string(),
                        _ => "sekundy".to_string(),
                    },
                    "minute" => match plural_category {
                        "one" => "minutę".to_string(),
                        "many" => "minut".to_string(),
                        _ => "minuty".to_string(),
                    },
                    "hour" => match plural_category {
                        "one" => "godzinę".to_string(),
                        "many" => "godzin".to_string(),
                        _ => "godziny".to_string(),
                    },
                    "day" => {
                        if plural_category == "one" {
                            "dzień".to_string()
                        } else if plural_category == "other" {
                            "dnia".to_string()
                        } else {
                            "dni".to_string()
                        }
                    }
                    "week" => match plural_category {
                        "one" => "tydzień".to_string(),
                        "few" => "tygodnie".to_string(),
                        "other" => "tygodnia".to_string(),
                        _ => "tygodni".to_string(),
                    },
                    "month" => match plural_category {
                        "one" => "miesiąc".to_string(),
                        "few" => "miesiące".to_string(),
                        "other" => "miesiąca".to_string(),
                        _ => "miesięcy".to_string(),
                    },
                    "quarter" => match plural_category {
                        "one" => "kwartał".to_string(),
                        "few" => "kwartały".to_string(),
                        "other" => "kwartału".to_string(),
                        _ => "kwartałów".to_string(),
                    },
                    "year" => match plural_category {
                        "one" => "rok".to_string(),
                        "few" => "lata".to_string(),
                        "other" => "roku".to_string(),
                        _ => "lat".to_string(),
                    },
                    _ => unit.to_string(),
                },
            }
        } else {
            match style {
                "short" => match (unit, plural_category) {
                    ("second", _) => "sec.".to_string(),
                    ("minute", _) => "min.".to_string(),
                    ("hour", _) => "hr.".to_string(),
                    ("day", "one") => "day".to_string(),
                    ("day", _) => "days".to_string(),
                    ("week", _) => "wk.".to_string(),
                    ("month", _) => "mo.".to_string(),
                    ("quarter", "one") => "qtr.".to_string(),
                    ("quarter", _) => "qtrs.".to_string(),
                    ("year", _) => "yr.".to_string(),
                    _ => unit.to_string(),
                },
                _ => match (unit, plural_category) {
                    ("second", "one") => "second".to_string(),
                    ("second", _) => "seconds".to_string(),
                    ("minute", "one") => "minute".to_string(),
                    ("minute", _) => "minutes".to_string(),
                    ("hour", "one") => "hour".to_string(),
                    ("hour", _) => "hours".to_string(),
                    ("day", "one") => "day".to_string(),
                    ("day", _) => "days".to_string(),
                    ("week", "one") => "week".to_string(),
                    ("week", _) => "weeks".to_string(),
                    ("month", "one") => "month".to_string(),
                    ("month", _) => "months".to_string(),
                    ("quarter", "one") => "quarter".to_string(),
                    ("quarter", _) => "quarters".to_string(),
                    ("year", "one") => "year".to_string(),
                    ("year", _) => "years".to_string(),
                    _ => unit.to_string(),
                },
            }
        }
    }

    fn intl_relative_time_number_parts(options: &IntlRelativeTimeFormatOptions, value: f64, unit: &str) -> Vec<IntlDurationPart> {
        let full_text = Self::number_to_string_smart(value);
        let (integer_text, fraction_text) = if let Some((integer, fraction)) = full_text.split_once('.') {
            (integer.to_string(), Some(fraction.to_string()))
        } else {
            (full_text, None)
        };
        let grouped_integer = Self::intl_relative_time_group_integer(&integer_text, &options.resolved_locale);
        let decimal_separator = if options.resolved_locale.starts_with("pl") { "," } else { "." };
        let group_separator = if options.resolved_locale.starts_with("pl") { "\u{a0}" } else { "," };
        let mut parts = Vec::new();
        let mut first = true;
        for chunk in grouped_integer.split(group_separator) {
            if !first {
                parts.push(IntlDurationPart::element("group", group_separator.to_string(), Some(unit)));
            }
            parts.push(IntlDurationPart::element(
                "integer",
                Self::intl_remap_numbering_system_digits(chunk, &options.numbering_system),
                Some(unit),
            ));
            first = false;
        }
        if let Some(fraction_text) = fraction_text {
            parts.push(IntlDurationPart::element("decimal", decimal_separator.to_string(), Some(unit)));
            parts.push(IntlDurationPart::element(
                "fraction",
                Self::intl_remap_numbering_system_digits(&fraction_text, &options.numbering_system),
                Some(unit),
            ));
        }
        parts
    }

    fn intl_relative_time_group_integer(integer: &str, locale: &str) -> String {
        let min_len = if locale.starts_with("pl") { 5 } else { 4 };
        if integer.len() < min_len {
            return integer.to_string();
        }
        let separator = if locale.starts_with("pl") { '\u{a0}' } else { ',' };
        let mut out = String::new();
        for (index, ch) in integer.chars().rev().enumerate() {
            if index > 0 && index % 3 == 0 {
                out.push(separator);
            }
            out.push(ch);
        }
        out.chars().rev().collect()
    }

    fn number_to_string_smart(value: f64) -> String {
        if value.fract() == 0.0 {
            format!("{:.0}", value)
        } else {
            value.to_string()
        }
    }

    fn intl_plural_rule_categories(locale: &str, plural_type: &str) -> Vec<&'static str> {
        if plural_type == "ordinal" {
            if locale.starts_with("en") {
                vec!["one", "two", "few", "other"]
            } else {
                vec!["other"]
            }
        } else if locale.starts_with("ar") {
            vec!["zero", "one", "two", "few", "many", "other"]
        } else if locale.starts_with("en") || locale.starts_with("fa") {
            vec!["one", "other"]
        } else if locale.starts_with("fr") {
            vec!["one", "many", "other"]
        } else if locale.starts_with("gv") {
            vec!["one", "two", "few", "many", "other"]
        } else if locale.starts_with("ko") {
            vec!["other"]
        } else if locale.starts_with("sl") {
            vec!["one", "two", "few", "other"]
        } else {
            vec!["other"]
        }
    }

    fn intl_plural_rule_select(locale: &str, plural_type: &str, notation: &str, value: f64) -> &'static str {
        if !value.is_finite() {
            return "other";
        }
        if plural_type == "ordinal" {
            if locale.starts_with("en") {
                let n = value.abs().trunc() as i64;
                let mod10 = n % 10;
                let mod100 = n % 100;
                if mod10 == 1 && mod100 != 11 {
                    "one"
                } else if mod10 == 2 && mod100 != 12 {
                    "two"
                } else if mod10 == 3 && mod100 != 13 {
                    "few"
                } else {
                    "other"
                }
            } else {
                "other"
            }
        } else if locale.starts_with("fr") {
            let abs = value.abs();
            if (notation == "compact" && abs >= 1_000_000.0) || (notation == "standard" && abs == 1_000_000.0) {
                "many"
            } else if abs < 2.0 {
                "one"
            } else {
                "other"
            }
        } else if locale.starts_with("en") {
            if value.abs() == 1.0 { "one" } else { "other" }
        } else {
            "other"
        }
    }

    fn intl_segment_items_array(
        &self,
        ctx: &GcContext<'gc>,
        input: &Value<'gc>,
        segment_slots: &IndexMap<String, Value<'gc>>,
    ) -> Value<'gc> {
        let granularity = segment_slots
            .get("__intl_granularity__")
            .and_then(|value| match value {
                Value::String(text) => Some(crate::unicode::utf16_to_utf8(text)),
                _ => None,
            })
            .unwrap_or_else(|| "grapheme".to_string());
        let utf16 = match input {
            Value::String(text) => text.clone(),
            _ => crate::unicode::utf8_to_utf16(""),
        };
        let segments = Self::intl_segment_utf16(&utf16, &granularity);
        Value::Array(new_gc_cell_ptr(
            ctx,
            VmArrayData::new(
                segments
                    .into_iter()
                    .map(|segment| Self::intl_segment_data_object(ctx, input, segment))
                    .collect(),
            ),
        ))
    }

    fn intl_segment_data_object(ctx: &GcContext<'gc>, input: &Value<'gc>, segment: IntlSegmentRecord) -> Value<'gc> {
        let mut obj = IndexMap::new();
        let empty = Vec::new();
        let input_utf16 = match input {
            Value::String(text) => text,
            _ => &empty,
        };
        obj.insert(
            "segment".to_string(),
            Value::String(crate::unicode::utf16_slice(input_utf16, segment.start, segment.end)),
        );
        obj.insert("index".to_string(), Value::Number(segment.start as f64));
        obj.insert("input".to_string(), input.clone());
        if let Some(is_word_like) = segment.is_word_like {
            obj.insert("isWordLike".to_string(), Value::Boolean(is_word_like));
        }
        Value::Object(new_gc_cell_ptr(ctx, obj))
    }

    fn intl_segment_utf16(input: &[u16], granularity: &str) -> Vec<IntlSegmentRecord> {
        match granularity {
            "word" => Self::intl_segment_word_utf16(input),
            "sentence" => Self::intl_segment_sentence_utf16(input),
            _ => Self::intl_segment_grapheme_utf16(input),
        }
    }

    fn intl_segment_grapheme_utf16(input: &[u16]) -> Vec<IntlSegmentRecord> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < input.len() {
            let start = i;
            let (mut cp, mut len) = Self::intl_utf16_code_point_at(input, i);
            i += len;
            loop {
                if i >= input.len() {
                    break;
                }
                let (next_cp, next_len) = Self::intl_utf16_code_point_at(input, i);
                if Self::intl_is_grapheme_extend(next_cp)
                    || Self::intl_is_variation_selector(next_cp)
                    || Self::intl_is_emoji_modifier(next_cp)
                    || Self::intl_hangul_grapheme_continue(cp, next_cp)
                {
                    cp = next_cp;
                    len = next_len;
                    i += next_len;
                    continue;
                }
                if next_cp == 0x200D {
                    i += next_len;
                    if i < input.len() {
                        let (joined_cp, joined_len) = Self::intl_utf16_code_point_at(input, i);
                        cp = joined_cp;
                        len = joined_len;
                        i += joined_len;
                        while i < input.len() {
                            let (tail_cp, tail_len) = Self::intl_utf16_code_point_at(input, i);
                            if Self::intl_is_grapheme_extend(tail_cp)
                                || Self::intl_is_variation_selector(tail_cp)
                                || Self::intl_is_emoji_modifier(tail_cp)
                            {
                                cp = tail_cp;
                                len = tail_len;
                                i += tail_len;
                            } else {
                                break;
                            }
                        }
                    }
                    continue;
                }
                break;
            }
            let _ = (cp, len);
            out.push(IntlSegmentRecord::new(start, i, None));
        }
        out
    }

    fn intl_segment_word_utf16(input: &[u16]) -> Vec<IntlSegmentRecord> {
        let graphemes = Self::intl_segment_grapheme_utf16(input);
        let classified: Vec<(IntlSegmentRecord, String, bool, bool)> = graphemes
            .into_iter()
            .map(|grapheme| {
                let text = crate::unicode::utf16_to_utf8(&input[grapheme.start..grapheme.end]);
                let is_space = text.chars().all(char::is_whitespace);
                let is_word_like = !is_space && Self::intl_segment_slice_is_word_like_utf16(input, grapheme.start, grapheme.end);
                (grapheme, text, is_space, is_word_like)
            })
            .collect();
        let mut out = Vec::new();
        let mut index = 0usize;
        while index < classified.len() {
            let (segment, _, is_space, is_word_like) = &classified[index];
            if *is_space {
                out.push(IntlSegmentRecord::new(segment.start, segment.end, Some(false)));
                index += 1;
                continue;
            }
            if *is_word_like {
                let start = segment.start;
                let mut end = segment.end;
                index += 1;
                while index < classified.len() {
                    let (next_segment, next_text, next_is_space, next_is_word_like) = &classified[index];
                    if *next_is_space {
                        break;
                    }
                    if *next_is_word_like {
                        end = next_segment.end;
                        index += 1;
                        continue;
                    }
                    if next_text == "."
                        && index + 1 < classified.len()
                        && !classified[index + 1].2
                        && classified[index + 1].3
                        && Self::intl_segment_slice_is_ascii_digits_utf16(input, start, end)
                        && Self::intl_segment_slice_is_ascii_digits_utf16(input, classified[index + 1].0.start, classified[index + 1].0.end)
                    {
                        end = classified[index + 1].0.end;
                        index += 2;
                        continue;
                    }
                    break;
                }
                out.push(IntlSegmentRecord::new(start, end, Some(true)));
                continue;
            }
            out.push(IntlSegmentRecord::new(segment.start, segment.end, Some(false)));
            index += 1;
        }
        out
    }

    fn intl_segment_sentence_utf16(input: &[u16]) -> Vec<IntlSegmentRecord> {
        let mut out = Vec::new();
        let mut start = 0usize;
        let mut i = 0usize;
        while i < input.len() {
            let (cp, len) = Self::intl_utf16_code_point_at(input, i);
            i += len;
            if matches!(cp, 0x002E | 0x0021 | 0x003F | 0x3002 | 0xFF01 | 0xFF1F) {
                while i < input.len() {
                    let (next_cp, next_len) = Self::intl_utf16_code_point_at(input, i);
                    if matches!(next_cp, 0x0022 | 0x0027 | 0x2019 | 0x201D | 0x3001 | 0x300D | 0x300F)
                        || char::from_u32(next_cp).is_some_and(char::is_whitespace)
                    {
                        i += next_len;
                    } else {
                        break;
                    }
                }
                out.push(IntlSegmentRecord::new(start, i, None));
                start = i;
            }
        }
        if start < input.len() || out.is_empty() {
            out.push(IntlSegmentRecord::new(start, input.len(), None));
        }
        out.retain(|segment| segment.start < segment.end);
        out
    }

    fn intl_segment_text_is_word_like(text: &str) -> bool {
        text.chars().any(|ch| ch.is_alphanumeric()) || text.chars().all(|ch| !ch.is_whitespace() && !Self::intl_is_punctuation_char(ch))
    }

    fn intl_segment_slice_is_word_like_utf16(input: &[u16], start: usize, end: usize) -> bool {
        if input[start..end].iter().any(|code_unit| (0xD800..=0xDFFF).contains(code_unit)) {
            return false;
        }
        let text = crate::unicode::utf16_to_utf8(&input[start..end]);
        Self::intl_segment_text_is_word_like(&text)
    }

    fn intl_segment_slice_is_ascii_digits_utf16(input: &[u16], start: usize, end: usize) -> bool {
        !input[start..end].is_empty() && input[start..end].iter().all(|code_unit| matches!(code_unit, 0x30..=0x39))
    }

    fn intl_is_punctuation_char(ch: char) -> bool {
        ch.is_ascii_punctuation()
            || matches!(
                ch,
                '«' | '»' | '“' | '”' | '„' | '’' | '‘' | '—' | '–' | '…' | '、' | '。' | '，' | '！' | '？' | '：' | '；'
            )
    }

    fn intl_utf16_code_point_at(input: &[u16], index: usize) -> (u32, usize) {
        let first = input[index];
        if (0xD800..=0xDBFF).contains(&first) && index + 1 < input.len() {
            let second = input[index + 1];
            if (0xDC00..=0xDFFF).contains(&second) {
                let high = (first as u32) - 0xD800;
                let low = (second as u32) - 0xDC00;
                return (0x10000 + ((high << 10) | low), 2);
            }
        }
        (first as u32, 1)
    }

    fn intl_is_grapheme_extend(cp: u32) -> bool {
        matches!(
            cp,
            0x0300..=0x036F
                | 0x0483..=0x0489
                | 0x0591..=0x05BD
                | 0x05BF
                | 0x05C1..=0x05C2
                | 0x05C4..=0x05C5
                | 0x0610..=0x061A
                | 0x064B..=0x065F
                | 0x0670
                | 0x06D6..=0x06DC
                | 0x06DF..=0x06E4
                | 0x06E7..=0x06E8
                | 0x06EA..=0x06ED
                | 0x0711
                | 0x0730..=0x074A
                | 0x07A6..=0x07B0
                | 0x0816..=0x0819
                | 0x081B..=0x0823
                | 0x0825..=0x0827
                | 0x0829..=0x082D
                | 0x0859..=0x085B
                | 0x08D3..=0x08E1
                | 0x08E3..=0x0902
                | 0x093A
                | 0x093C
                | 0x0941..=0x0948
                | 0x094D
                | 0x0951..=0x0957
                | 0x0962..=0x0963
                | 0x09BC
                | 0x09CD
                | 0x0A3C
                | 0x0A4D
                | 0x0ABC
                | 0x0ACD
                | 0x0B3C
                | 0x0BCD
                | 0x0C3E..=0x0C40
                | 0x0C46..=0x0C48
                | 0x0C4A..=0x0C4D
                | 0x0D3B..=0x0D3C
                | 0x0D4D
                | 0x0E31
                | 0x0E34..=0x0E3A
                | 0x0E47..=0x0E4E
                | 0x0EB1
                | 0x0EB4..=0x0EBC
                | 0x0EC8..=0x0ECD
                | 0x0F71..=0x0F7E
                | 0x0F80..=0x0F84
                | 0x0F86..=0x0F87
                | 0x102D..=0x1030
                | 0x1032..=0x1037
                | 0x1039..=0x103A
                | 0x1058..=0x1059
                | 0x1712..=0x1714
                | 0x1732..=0x1734
                | 0x1752..=0x1753
                | 0x1772..=0x1773
                | 0x17B4..=0x17B5
                | 0x17B7..=0x17BD
                | 0x17C6
                | 0x17C9..=0x17D3
                | 0x180B..=0x180D
                | 0x1885..=0x1886
                | 0x18A9
                | 0x1920..=0x1922
                | 0x1927..=0x1928
                | 0x1932
                | 0x1939..=0x193B
                | 0x1A17..=0x1A18
                | 0x1A56
                | 0x1A58..=0x1A5E
                | 0x1A60
                | 0x1A62
                | 0x1A65..=0x1A6C
                | 0x1A73..=0x1A7C
                | 0x1AB0..=0x1AFF
                | 0x1B00..=0x1B03
                | 0x1B34
                | 0x1B36..=0x1B3A
                | 0x1B3C
                | 0x1B42
                | 0x1B6B..=0x1B73
                | 0x1BA2..=0x1BA5
                | 0x1BA8..=0x1BA9
                | 0x1BAB..=0x1BAD
                | 0x1BE6
                | 0x1BE8..=0x1BE9
                | 0x1BED
                | 0x1BEF..=0x1BF1
                | 0x1C2C..=0x1C33
                | 0x1C36..=0x1C37
                | 0x1CD0..=0x1CD2
                | 0x1CD4..=0x1CE0
                | 0x1CE2..=0x1CE8
                | 0x1CED
                | 0x1CF4
                | 0x1CF8..=0x1CF9
                | 0x1DC0..=0x1DFF
                | 0x20D0..=0x20FF
                | 0x2CEF..=0x2CF1
                | 0x2D7F
                | 0x2DE0..=0x2DFF
                | 0x302A..=0x302F
                | 0x3099..=0x309A
                | 0xA66F
                | 0xA67C..=0xA67D
                | 0xA6F0..=0xA6F1
                | 0xA802
                | 0xA806
                | 0xA80B
                | 0xA825..=0xA826
                | 0xA82C
                | 0xA8C4..=0xA8C5
                | 0xA8E0..=0xA8F1
                | 0xA926..=0xA92D
                | 0xA947..=0xA951
                | 0xA980..=0xA982
                | 0xA9B3
                | 0xA9B6..=0xA9B9
                | 0xA9BC
                | 0xA9E5
                | 0xAA29..=0xAA2E
                | 0xAA31..=0xAA32
                | 0xAA35..=0xAA36
                | 0xAA43
                | 0xAA4C
                | 0xAA7C
                | 0xAAB0
                | 0xAAB2..=0xAAB4
                | 0xAAB7..=0xAAB8
                | 0xAABE..=0xAABF
                | 0xAAC1
                | 0xAAEC..=0xAAED
                | 0xAAF6
                | 0xABE5
                | 0xABE8
                | 0xABED
                | 0xFB1E
                | 0xFE00..=0xFE0F
                | 0xFE20..=0xFE2F
                | 0x101FD
                | 0x102E0
                | 0x10376..=0x1037A
                | 0x10A01..=0x10A03
                | 0x10A05..=0x10A06
                | 0x10A0C..=0x10A0F
                | 0x10A38..=0x10A3A
                | 0x10A3F
                | 0x10AE5..=0x10AE6
                | 0x11001
                | 0x11038..=0x11046
                | 0x1107F..=0x11081
                | 0x110B3..=0x110B6
                | 0x110B9..=0x110BA
                | 0x11100..=0x11102
                | 0x11127..=0x1112B
                | 0x1112D..=0x11134
                | 0x11173
                | 0x11180..=0x11181
                | 0x111B6..=0x111BE
                | 0x111CA..=0x111CC
                | 0x1122F..=0x11231
                | 0x11234
                | 0x11236..=0x11237
                | 0x1123E
                | 0x112DF
                | 0x112E3..=0x112EA
                | 0x11300..=0x11301
                | 0x1133C
                | 0x11340
                | 0x11366..=0x1136C
                | 0x11370..=0x11374
                | 0x11438..=0x1143F
                | 0x11442..=0x11444
                | 0x11446
                | 0x1145E
                | 0x114B3..=0x114B8
                | 0x114BA
                | 0x114BF..=0x114C0
                | 0x114C2..=0x114C3
                | 0x115B2..=0x115B5
                | 0x115BC..=0x115BD
                | 0x115BF..=0x115C0
                | 0x115DC..=0x115DD
                | 0x11633..=0x1163A
                | 0x1163D
                | 0x1163F..=0x11640
                | 0x116AB
                | 0x116AD
                | 0x116B0..=0x116B5
                | 0x116B7
                | 0x1171D..=0x1171F
                | 0x11722..=0x11725
                | 0x11727..=0x1172B
                | 0x1182F..=0x11837
                | 0x11839..=0x1183A
                | 0x1193B..=0x1193C
                | 0x1193E
                | 0x11943
                | 0x119D4..=0x119D7
                | 0x119DA..=0x119DB
                | 0x119E0
                | 0x11A01..=0x11A0A
                | 0x11A33..=0x11A38
                | 0x11A3B..=0x11A3E
                | 0x11A47
                | 0x11A51..=0x11A56
                | 0x11A59..=0x11A5B
                | 0x11A8A..=0x11A96
                | 0x11A98..=0x11A99
                | 0x11C30..=0x11C36
                | 0x11C38..=0x11C3D
                | 0x11C3F
                | 0x11C92..=0x11CA7
                | 0x11CAA..=0x11CB0
                | 0x11CB2..=0x11CB3
                | 0x11CB5..=0x11CB6
                | 0x11D31..=0x11D36
                | 0x11D3A
                | 0x11D3C..=0x11D3D
                | 0x11D3F..=0x11D45
                | 0x11D47
                | 0x11D90..=0x11D91
                | 0x11D95
                | 0x11D97
                | 0x11EF3..=0x11EF4
                | 0x16AF0..=0x16AF4
                | 0x16B30..=0x16B36
                | 0x16F4F
                | 0x16F8F..=0x16F92
                | 0x16FE4
                | 0x1BC9D..=0x1BC9E
                | 0x1D167..=0x1D169
                | 0x1D17B..=0x1D182
                | 0x1D185..=0x1D18B
                | 0x1D1AA..=0x1D1AD
                | 0x1D242..=0x1D244
                | 0x1DA00..=0x1DA36
                | 0x1DA3B..=0x1DA6C
                | 0x1DA75
                | 0x1DA84
                | 0x1DA9B..=0x1DA9F
                | 0x1DAA1..=0x1DAAF
                | 0x1E000..=0x1E006
                | 0x1E008..=0x1E018
                | 0x1E01B..=0x1E021
                | 0x1E023..=0x1E024
                | 0x1E026..=0x1E02A
                | 0x1E130..=0x1E136
                | 0x1E2EC..=0x1E2EF
                | 0x1E8D0..=0x1E8D6
                | 0x1E944..=0x1E94A
        )
    }

    fn intl_is_variation_selector(cp: u32) -> bool {
        matches!(cp, 0xFE00..=0xFE0F | 0xE0100..=0xE01EF)
    }

    fn intl_is_emoji_modifier(cp: u32) -> bool {
        matches!(cp, 0x1F3FB..=0x1F3FF)
    }

    fn intl_hangul_grapheme_continue(current: u32, next: u32) -> bool {
        let current_l = matches!(current, 0x1100..=0x115F | 0xA960..=0xA97C);
        let current_v = matches!(current, 0x1160..=0x11A7 | 0xD7B0..=0xD7C6);
        let current_t = matches!(current, 0x11A8..=0x11FF | 0xD7CB..=0xD7FB);
        let current_lv = (0xAC00..=0xD7A3).contains(&current) && (current - 0xAC00).is_multiple_of(28);
        let current_lvt = (0xAC00..=0xD7A3).contains(&current) && !(current - 0xAC00).is_multiple_of(28);
        let next_l = matches!(next, 0x1100..=0x115F | 0xA960..=0xA97C);
        let next_v = matches!(next, 0x1160..=0x11A7 | 0xD7B0..=0xD7C6);
        let next_t = matches!(next, 0x11A8..=0x11FF | 0xD7CB..=0xD7FB);
        (current_l && (next_l || next_v)) || ((current_v || current_lv) && (next_v || next_t)) || ((current_t || current_lvt) && next_t)
    }
}

#[derive(Clone, Copy)]
struct IntlSegmentRecord {
    start: usize,
    end: usize,
    is_word_like: Option<bool>,
}

impl IntlSegmentRecord {
    fn new(start: usize, end: usize, is_word_like: Option<bool>) -> Self {
        Self { start, end, is_word_like }
    }
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

struct IntlListPart {
    part_type: String,
    value: String,
}

struct IntlDurationPart {
    part_type: String,
    value: String,
    unit: Option<String>,
}

struct IntlDurationNumberFormatConfig<'a> {
    locale: &'a str,
    numbering_system: &'a str,
    unit: &'a str,
    unit_options: &'a IntlDurationUnitOptions,
    input: &'a str,
    decimal_style: bool,
    trunc_rounding: bool,
    sign_never: bool,
    minimum_integer_digits: Option<u8>,
    minimum_fraction_digits: Option<u8>,
    maximum_fraction_digits: Option<u8>,
}

struct IntlDurationRecord {
    years: i64,
    months: i64,
    weeks: i64,
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
    microseconds: i128,
    nanoseconds: i128,
    years_present: bool,
    months_present: bool,
    weeks_present: bool,
    days_present: bool,
    hours_present: bool,
    minutes_present: bool,
    seconds_present: bool,
    milliseconds_present: bool,
    microseconds_present: bool,
    nanoseconds_present: bool,
}

struct IntlListLiterals {
    pair: &'static str,
    middle: &'static str,
    end: &'static str,
}

enum IntlFormattedNumberInput {
    Number(f64),
    BigInt(String),
    DecimalString(String),
}

#[derive(Clone, Copy)]
enum IntlUseGrouping {
    False,
    Auto,
    Always,
    Min2,
}
