//! Unified property descriptor model.
//!
//! Replaces the legacy hidden-key encoding (`__readonly_*__`,
//! `__nonenumerable_*__`, `__nonconfigurable_*__`, `__get_*`, `__set_*`)
//! with a single `PropertyDescriptor` that lives alongside each property.
//!
//! ## Migration strategy
//!
//! The types here are introduced first; existing storage is untouched.
//! Conversion helpers bridge between legacy hidden-key maps and the new
//! model so both representations can coexist during the dual-track phase.

#![allow(dead_code)]

use crate::core::value::Value;
use crate::core::{Collect, GcTrace};
use bitflags::bitflags;

// ── Attribute flags ────────────────────────────────────────────────

bitflags! {
    /// ECMAScript property attribute flags.
    ///
    /// Default for user-created data properties: all three set (WEC).
    /// Default for built-in / class methods: only WRITABLE | CONFIGURABLE.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PropAttrs: u8 {
        const WRITABLE      = 0b0000_0001;
        const ENUMERABLE    = 0b0000_0010;
        const CONFIGURABLE  = 0b0000_0100;
    }
}

impl PropAttrs {
    /// `{ writable: true, enumerable: true, configurable: true }` –
    /// the default for ordinary assignment (`obj.key = val`).
    pub const WEC: Self = Self::WRITABLE.union(Self::ENUMERABLE).union(Self::CONFIGURABLE);

    /// `{ writable: true, configurable: true }` –
    /// typical for class prototype methods and built-in function properties.
    pub const WC: Self = Self::WRITABLE.union(Self::CONFIGURABLE);

    /// `{ enumerable: true, configurable: true }` –
    /// accessor descriptors created by user code default to this.
    pub const EC: Self = Self::ENUMERABLE.union(Self::CONFIGURABLE);

    /// All bits clear – fully locked (non-writable, non-enumerable,
    /// non-configurable).
    pub const NONE: Self = Self::empty();
}

impl Default for PropAttrs {
    /// Ordinary assignment defaults: writable + enumerable + configurable.
    fn default() -> Self {
        Self::WEC
    }
}

// SAFETY: PropAttrs contains no GC pointers.
unsafe impl<'gc> Collect<'gc> for PropAttrs {
    fn trace<T: GcTrace<'gc>>(&self, _cc: &mut T) {}
}

// ── Property kind ──────────────────────────────────────────────────

/// Whether the property holds a data value or accessor functions.
#[derive(Clone)]
pub enum PropKind<'gc> {
    /// Data property with a concrete value.
    Data(Value<'gc>),

    /// Accessor property with optional getter / setter.
    Accessor { get: Option<Value<'gc>>, set: Option<Value<'gc>> },
}

// SAFETY: delegates tracing to the contained Value(s).
unsafe impl<'gc> Collect<'gc> for PropKind<'gc> {
    fn trace<T: GcTrace<'gc>>(&self, cc: &mut T) {
        match self {
            PropKind::Data(v) => v.trace(cc),
            PropKind::Accessor { get, set } => {
                if let Some(g) = get {
                    g.trace(cc);
                }
                if let Some(s) = set {
                    s.trace(cc);
                }
            }
        }
    }
}

impl<'gc> Default for PropKind<'gc> {
    fn default() -> Self {
        PropKind::Data(Value::Undefined)
    }
}

// ── Property descriptor ────────────────────────────────────────────

/// Full internal property descriptor (spec §6.2.6).
///
/// Combines a `PropKind` (data vs accessor) with attribute flags.
/// This is the *internal* representation; the JS-visible descriptor
/// object (`{ value, writable, enumerable, configurable }`) is built
/// on demand by helper methods on `VM`.
#[derive(Clone)]
pub struct PropDesc<'gc> {
    pub kind: PropKind<'gc>,
    pub attrs: PropAttrs,
}

// SAFETY: delegates tracing to PropKind; PropAttrs has no GC ptrs.
unsafe impl<'gc> Collect<'gc> for PropDesc<'gc> {
    fn trace<T: GcTrace<'gc>>(&self, cc: &mut T) {
        self.kind.trace(cc);
    }
}

impl<'gc> PropDesc<'gc> {
    // ── Constructors ───────────────────────────────────────────

    /// Data property with explicit attributes.
    pub fn data(value: Value<'gc>, attrs: PropAttrs) -> Self {
        Self {
            kind: PropKind::Data(value),
            attrs,
        }
    }

    /// Data property with default attributes (writable + enumerable + configurable).
    pub fn data_default(value: Value<'gc>) -> Self {
        Self::data(value, PropAttrs::WEC)
    }

    /// Data property that is writable + configurable but **not** enumerable.
    /// Used for class prototype methods, built-in function properties, etc.
    pub fn data_wc(value: Value<'gc>) -> Self {
        Self::data(value, PropAttrs::WC)
    }

    /// Fully locked data property (none of W/E/C).
    pub fn data_frozen(value: Value<'gc>) -> Self {
        Self::data(value, PropAttrs::NONE)
    }

    /// Accessor property with explicit attributes.
    pub fn accessor(get: Option<Value<'gc>>, set: Option<Value<'gc>>, attrs: PropAttrs) -> Self {
        Self {
            kind: PropKind::Accessor { get, set },
            attrs,
        }
    }

    /// Accessor property with enumerable + configurable (user-defined default).
    pub fn accessor_ec(get: Option<Value<'gc>>, set: Option<Value<'gc>>) -> Self {
        Self::accessor(get, set, PropAttrs::EC)
    }

    /// Accessor property that is configurable only (not enumerable).
    /// Common for class getter/setter definitions.
    pub fn accessor_c(get: Option<Value<'gc>>, set: Option<Value<'gc>>) -> Self {
        Self::accessor(get, set, PropAttrs::CONFIGURABLE)
    }

    // ── Queries ────────────────────────────────────────────────

    #[inline]
    pub fn is_data(&self) -> bool {
        matches!(self.kind, PropKind::Data(_))
    }

    #[inline]
    pub fn is_accessor(&self) -> bool {
        matches!(self.kind, PropKind::Accessor { .. })
    }

    #[inline]
    pub fn is_writable(&self) -> bool {
        self.attrs.contains(PropAttrs::WRITABLE)
    }

    #[inline]
    pub fn is_enumerable(&self) -> bool {
        self.attrs.contains(PropAttrs::ENUMERABLE)
    }

    #[inline]
    pub fn is_configurable(&self) -> bool {
        self.attrs.contains(PropAttrs::CONFIGURABLE)
    }

    /// Return the data value, or `None` if this is an accessor.
    pub fn value(&self) -> Option<&Value<'gc>> {
        match &self.kind {
            PropKind::Data(v) => Some(v),
            PropKind::Accessor { .. } => None,
        }
    }

    /// Return the getter, or `None`.
    pub fn getter(&self) -> Option<&Value<'gc>> {
        match &self.kind {
            PropKind::Accessor { get, .. } => get.as_ref(),
            PropKind::Data(_) => None,
        }
    }

    /// Return the setter, or `None`.
    pub fn setter(&self) -> Option<&Value<'gc>> {
        match &self.kind {
            PropKind::Accessor { set, .. } => set.as_ref(),
            PropKind::Data(_) => None,
        }
    }

    // ── Mutations ──────────────────────────────────────────────

    /// Set or clear an individual attribute bit.
    pub fn set_attr(&mut self, flag: PropAttrs, on: bool) {
        if on {
            self.attrs.insert(flag);
        } else {
            self.attrs.remove(flag);
        }
    }

    /// Replace the data value (no-op if this is an accessor).
    pub fn set_value(&mut self, value: Value<'gc>) {
        if let PropKind::Data(ref mut v) = self.kind {
            *v = value;
        }
    }
}

// ── JS-visible descriptor conversion ───────────────────────────────

impl<'gc> PropDesc<'gc> {
    /// Build the `IndexMap` that a JS-visible descriptor object would contain.
    ///
    /// Data descriptors produce `{ value, writable, enumerable, configurable }`.
    /// Accessor descriptors produce `{ get, set, enumerable, configurable }`.
    pub fn to_descriptor_map(&self) -> indexmap::IndexMap<String, Value<'gc>> {
        let mut map = indexmap::IndexMap::new();
        match &self.kind {
            PropKind::Data(v) => {
                map.insert("value".to_string(), v.clone());
                map.insert("writable".to_string(), Value::Boolean(self.attrs.contains(PropAttrs::WRITABLE)));
            }
            PropKind::Accessor { get, set } => {
                map.insert("get".to_string(), get.clone().unwrap_or(Value::Undefined));
                map.insert("set".to_string(), set.clone().unwrap_or(Value::Undefined));
            }
        }
        map.insert("enumerable".to_string(), Value::Boolean(self.attrs.contains(PropAttrs::ENUMERABLE)));
        map.insert(
            "configurable".to_string(),
            Value::Boolean(self.attrs.contains(PropAttrs::CONFIGURABLE)),
        );
        map
    }

    /// Create a `PropDesc` from a JS descriptor `IndexMap`
    /// (the output of `extract_property_descriptor`).
    ///
    /// Missing boolean fields default to `false` (per spec §6.2.6.1
    /// "CompletePropertyDescriptor"). Missing `value` defaults to `Undefined`.
    pub fn from_descriptor_map(map: &indexmap::IndexMap<String, Value<'gc>>) -> Self {
        let has_get = map.contains_key("get");
        let has_set = map.contains_key("set");
        let is_accessor = has_get || has_set;

        let kind = if is_accessor {
            PropKind::Accessor {
                get: map.get("get").cloned(),
                set: map.get("set").cloned(),
            }
        } else {
            PropKind::Data(map.get("value").cloned().unwrap_or(Value::Undefined))
        };

        let writable = matches!(map.get("writable"), Some(Value::Boolean(true)));
        let enumerable = matches!(map.get("enumerable"), Some(Value::Boolean(true)));
        let configurable = matches!(map.get("configurable"), Some(Value::Boolean(true)));

        let mut attrs = PropAttrs::empty();
        if writable {
            attrs |= PropAttrs::WRITABLE;
        }
        if enumerable {
            attrs |= PropAttrs::ENUMERABLE;
        }
        if configurable {
            attrs |= PropAttrs::CONFIGURABLE;
        }

        Self { kind, attrs }
    }

    /// Write this descriptor into the object map.
    ///
    /// Accessor compatibility keys (`__get_*` / `__set_*`) are still kept for
    /// legacy lookup paths, but attribute flags are stored inline on
    /// `Value::Property` and no longer use `__readonly_*__` /
    /// `__nonenumerable_*__` / `__nonconfigurable_*__` marker keys.
    pub fn write_to_legacy_map(&self, map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
        let getter_key = make_getter_key(key);
        let setter_key = make_setter_key(key);

        match &self.kind {
            PropKind::Data(v) => {
                map.shift_remove(&getter_key);
                map.shift_remove(&setter_key);
                if self.attrs == PropAttrs::WEC {
                    map.insert(key.to_string(), v.clone());
                } else {
                    map.insert(
                        key.to_string(),
                        Value::Property {
                            value: Some(Box::new(v.clone())),
                            getter: None,
                            setter: None,
                            attrs: self.attrs,
                        },
                    );
                }
            }
            PropKind::Accessor { get, set } => {
                if let Some(g) = get {
                    map.insert(getter_key.clone(), g.clone());
                } else {
                    map.shift_remove(&getter_key);
                }
                if let Some(s) = set {
                    map.insert(setter_key.clone(), s.clone());
                } else {
                    map.shift_remove(&setter_key);
                }
                map.insert(
                    key.to_string(),
                    Value::Property {
                        value: None,
                        getter: get.clone().map(Box::new),
                        setter: set.clone().map(Box::new),
                        attrs: self.attrs,
                    },
                );
            }
        }
    }

    /// Remove all hidden keys and the visible key itself.
    pub fn remove_legacy_keys(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
        map.shift_remove(key);
        map.shift_remove(&make_getter_key(key));
        map.shift_remove(&make_setter_key(key));
    }
}

// ── Legacy conversion helpers ──────────────────────────────────────

/// Prefix constants used for accessor compatibility keys.
pub const GETTER_PREFIX: &str = "__get_";
pub const SETTER_PREFIX: &str = "__set_";

/// Read just the attribute flags for `key` from a property map.
pub fn attrs_from_legacy_map<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> PropAttrs {
    desc_from_legacy_map(map, key).map(|d| d.attrs).unwrap_or(PropAttrs::WEC)
}

/// Build a `PropDesc` from a property map.
pub fn desc_from_legacy_map<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<PropDesc<'gc>> {
    let getter_key = make_getter_key(key);
    let setter_key = make_setter_key(key);
    let hidden_get = map.get(&getter_key).cloned();
    let hidden_set = map.get(&setter_key).cloned();

    if let Some(Value::Property {
        value,
        getter,
        setter,
        attrs,
    }) = map.get(key)
    {
        let get = getter.as_ref().map(|g| (**g).clone()).or_else(|| hidden_get.clone());
        let set = setter.as_ref().map(|s| (**s).clone()).or_else(|| hidden_set.clone());
        if value.is_none() || get.is_some() || set.is_some() {
            return Some(PropDesc {
                kind: PropKind::Accessor { get, set },
                attrs: *attrs,
            });
        }
        let data = value.as_ref().map(|v| (**v).clone()).unwrap_or(Value::Undefined);
        return Some(PropDesc {
            kind: PropKind::Data(data),
            attrs: *attrs,
        });
    }

    if hidden_get.is_some() || hidden_set.is_some() {
        return Some(PropDesc {
            kind: PropKind::Accessor {
                get: hidden_get,
                set: hidden_set,
            },
            attrs: PropAttrs::EC,
        });
    }

    map.get(key).cloned().map(|v| PropDesc {
        kind: PropKind::Data(v),
        attrs: PropAttrs::WEC,
    })
}

/// Read the own data value for `key`, unwrapping `Value::Property` when the
/// property is represented inline. Returns `None` for accessors or missing keys.
pub fn own_data_from_legacy_map<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<Value<'gc>> {
    match desc_from_legacy_map(map, key)?.kind {
        PropKind::Data(v) => Some(v),
        PropKind::Accessor { .. } => None,
    }
}

/// Returns `true` if `key` is an internal hidden-key marker and should be
/// skipped during user-visible enumeration.
pub fn is_hidden_key(key: &str) -> bool {
    key.starts_with(GETTER_PREFIX) || key.starts_with(SETTER_PREFIX)
}

#[inline]
fn update_attrs_for_key<'gc, F>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str, f: F)
where
    F: FnOnce(PropAttrs) -> PropAttrs,
{
    if let Some(mut desc) = desc_from_legacy_map(map, key) {
        desc.attrs = f(desc.attrs);
        desc.write_to_legacy_map(map, key);
    }
}

// ── Individual flag helpers ─────────────────────────────────────────

/// Mark `key` as non-enumerable.
#[inline]
pub fn mark_nonenumerable<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    update_attrs_for_key(map, key, |mut attrs| {
        attrs.remove(PropAttrs::ENUMERABLE);
        attrs
    });
}

/// Remove non-enumerable marker for `key` (making it enumerable).
#[inline]
pub fn unmark_nonenumerable<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    update_attrs_for_key(map, key, |mut attrs| {
        attrs.insert(PropAttrs::ENUMERABLE);
        attrs
    });
}

/// Returns `true` if `key` is non-enumerable.
#[inline]
pub fn has_nonenumerable_mark<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> bool {
    desc_from_legacy_map(map, key)
        .map(|d| !d.attrs.contains(PropAttrs::ENUMERABLE))
        .unwrap_or(false)
}

/// Mark `key` as read-only (non-writable).
#[inline]
pub fn mark_readonly<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    update_attrs_for_key(map, key, |mut attrs| {
        attrs.remove(PropAttrs::WRITABLE);
        attrs
    });
}

/// Remove read-only marker for `key` (making it writable).
#[inline]
pub fn unmark_readonly<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    update_attrs_for_key(map, key, |mut attrs| {
        attrs.insert(PropAttrs::WRITABLE);
        attrs
    });
}

/// Returns `true` if `key` is read-only.
#[inline]
pub fn has_readonly_mark<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> bool {
    match desc_from_legacy_map(map, key) {
        Some(desc) if desc.is_accessor() => false,
        Some(desc) => !desc.attrs.contains(PropAttrs::WRITABLE),
        None => false,
    }
}

/// Mark `key` as non-configurable.
#[inline]
pub fn mark_nonconfigurable<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    update_attrs_for_key(map, key, |mut attrs| {
        attrs.remove(PropAttrs::CONFIGURABLE);
        attrs
    });
}

/// Remove non-configurable marker for `key`.
#[inline]
pub fn unmark_nonconfigurable<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    update_attrs_for_key(map, key, |mut attrs| {
        attrs.insert(PropAttrs::CONFIGURABLE);
        attrs
    });
}

/// Returns `true` if `key` is non-configurable.
#[inline]
pub fn has_nonconfigurable_mark<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> bool {
    desc_from_legacy_map(map, key)
        .map(|d| !d.attrs.contains(PropAttrs::CONFIGURABLE))
        .unwrap_or(false)
}

// ── Accessor helpers ───────────────────────────────────────────────

/// Get the getter function for `key`, if any.
#[inline]
pub fn get_getter<'a, 'gc>(map: &'a indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<&'a Value<'gc>> {
    if let Some(Value::Property { getter: Some(g), .. }) = map.get(key) {
        return Some(g.as_ref());
    }
    map.get(&make_getter_key(key))
}

/// Get the setter function for `key`, if any.
#[inline]
pub fn get_setter<'a, 'gc>(map: &'a indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<&'a Value<'gc>> {
    if let Some(Value::Property { setter: Some(s), .. }) = map.get(key) {
        return Some(s.as_ref());
    }
    map.get(&make_setter_key(key))
}

/// Returns `true` if `key` has a getter.
#[inline]
pub fn has_getter<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> bool {
    if let Some(Value::Property { getter: Some(_), .. }) = map.get(key) {
        return true;
    }
    map.contains_key(&make_getter_key(key))
}

/// Returns `true` if `key` has a setter.
#[inline]
pub fn has_setter<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> bool {
    if let Some(Value::Property { setter: Some(_), .. }) = map.get(key) {
        return true;
    }
    map.contains_key(&make_setter_key(key))
}

/// Set (or replace) the getter for `key`.
#[inline]
pub fn set_getter<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str, val: Value<'gc>) {
    let current_set = lookup_setter(map, key).cloned();
    let attrs = desc_from_legacy_map(map, key)
        .map(|d| if d.is_accessor() { d.attrs } else { PropAttrs::EC })
        .unwrap_or(PropAttrs::EC);
    PropDesc::accessor(Some(val), current_set, attrs).write_to_legacy_map(map, key);
}

/// Set (or replace) the setter for `key`.
#[inline]
pub fn set_setter<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str, val: Value<'gc>) {
    let current_get = lookup_getter(map, key).cloned();
    let attrs = desc_from_legacy_map(map, key)
        .map(|d| if d.is_accessor() { d.attrs } else { PropAttrs::EC })
        .unwrap_or(PropAttrs::EC);
    PropDesc::accessor(current_get, Some(val), attrs).write_to_legacy_map(map, key);
}

/// Remove the getter for `key`.
#[inline]
pub fn remove_getter<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    if let Some(desc) = desc_from_legacy_map(map, key)
        && let PropKind::Accessor { set, .. } = desc.kind
    {
        if set.is_some() {
            PropDesc::accessor(None, set, desc.attrs).write_to_legacy_map(map, key);
        } else {
            map.shift_remove(key);
            map.shift_remove(&make_getter_key(key));
            map.shift_remove(&make_setter_key(key));
        }
        return;
    }
    map.shift_remove(&make_getter_key(key));
}

/// Remove the setter for `key`.
#[inline]
pub fn remove_setter<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    if let Some(desc) = desc_from_legacy_map(map, key)
        && let PropKind::Accessor { get, .. } = desc.kind
    {
        if get.is_some() {
            PropDesc::accessor(get, None, desc.attrs).write_to_legacy_map(map, key);
        } else {
            map.shift_remove(key);
            map.shift_remove(&make_getter_key(key));
            map.shift_remove(&make_setter_key(key));
        }
        return;
    }
    map.shift_remove(&make_setter_key(key));
}

/// Look up the getter value for `key`, returning a reference if present.
#[inline]
pub fn lookup_getter<'a, 'gc>(map: &'a indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<&'a Value<'gc>> {
    if let Some(Value::Property { getter: Some(g), .. }) = map.get(key) {
        return Some(g.as_ref());
    }
    map.get(&make_getter_key(key))
}

/// Look up the setter value for `key`, returning a reference if present.
#[inline]
pub fn lookup_setter<'a, 'gc>(map: &'a indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<&'a Value<'gc>> {
    if let Some(Value::Property { setter: Some(s), .. }) = map.get(key) {
        return Some(s.as_ref());
    }
    map.get(&make_setter_key(key))
}

/// Get the getter key string for `key`.
#[inline]
pub fn make_getter_key(key: impl AsRef<str>) -> String {
    format!("{}{}", GETTER_PREFIX, key.as_ref())
}

/// Get the setter key string for `key`.
#[inline]
pub fn make_setter_key(key: impl AsRef<str>) -> String {
    format!("{}{}", SETTER_PREFIX, key.as_ref())
}

// ── Batch attribute write ──────────────────────────────────────────

/// Read the current attribute flags for `key` from a property map.
pub fn read_attrs_from_legacy_map<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> PropAttrs {
    attrs_from_legacy_map(map, key)
}

/// Write attribute flags for `key` into a property map.
pub fn write_attrs_to_legacy_map<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str, attrs: PropAttrs) {
    if let Some(mut desc) = desc_from_legacy_map(map, key) {
        desc.attrs = attrs;
        desc.write_to_legacy_map(map, key);
    }
}

/// Remove a property and its associated accessor hidden-keys.
pub fn remove_property_completely<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    map.shift_remove(key);
    clear_attr_markers(map, key);
}

/// Clear accessor hidden-keys for `key` WITHOUT removing the key itself.
pub fn clear_attr_markers<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    map.shift_remove(&make_getter_key(key));
    map.shift_remove(&make_setter_key(key));
}
