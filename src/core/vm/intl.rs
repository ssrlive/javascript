use super::*;

const INTL_DEFAULT_LOCALE: &str = "en-US";

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
            return self.intl_supported_locales_of(ctx, args.get(1));
        }
        match name {
            "intl.getCanonicalLocales" => self.intl_supported_locales_of(ctx, None),
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
        self.intl_read_constructor_options(ctx, kind, args.get(1))?;

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
        obj.insert("__intl_locale__".to_string(), Value::from(INTL_DEFAULT_LOCALE));

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

        Ok(Value::Object(new_gc_cell_ptr(ctx, obj)))
    }

    fn intl_supported_locales_of(&mut self, ctx: &GcContext<'gc>, options: Option<&Value<'gc>>) -> Value<'gc> {
        if let Some(options) = options
            && !matches!(options, Value::Undefined)
            && Self::intl_is_object_like(options)
        {
            let _ = self.read_named_property(ctx, options, "localeMatcher");
            if self.pending_throw.is_some() {
                return Value::Undefined;
            }
        }
        Value::Array(new_gc_cell_ptr(ctx, VmArrayData::new(vec![])))
    }

    fn intl_resolved_options(&mut self, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>) -> Value<'gc> {
        let mut result = IndexMap::new();
        if let Some(Value::Object(obj)) = receiver {
            let borrow = obj.borrow();
            if let Some(locale) = borrow.get("__intl_locale__").cloned() {
                result.insert("locale".to_string(), locale);
            } else {
                result.insert("locale".to_string(), Value::from(INTL_DEFAULT_LOCALE));
            }
            if matches!(borrow.get("__intl_kind__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == "Collator") {
                result.insert("sensitivity".to_string(), Value::from("variant"));
            }
        } else {
            result.insert("locale".to_string(), Value::from(INTL_DEFAULT_LOCALE));
        }
        Value::Object(new_gc_cell_ptr(ctx, result))
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
}
