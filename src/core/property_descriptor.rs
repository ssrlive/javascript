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

    /// Write this descriptor into a legacy hidden-key `IndexMap`.
    ///
    /// This is the inverse of `desc_from_legacy_map`: it sets the data or
    /// accessor value under `key`, plus the appropriate `__readonly_*__`,
    /// `__nonenumerable_*__`, `__nonconfigurable_*__`, `__get_*`, `__set_*`
    /// shadow entries.
    ///
    /// **Existing hidden keys for `key` are NOT cleared first** — callers
    /// should remove stale entries before calling this if needed.
    pub fn write_to_legacy_map(&self, map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
        let getter_key = format!("{}{}", GETTER_PREFIX, key);
        let setter_key = format!("{}{}", SETTER_PREFIX, key);
        let ro_key = format!("{}{}{}", READONLY_PREFIX, key, READONLY_SUFFIX);
        let ne_key = format!("{}{}{}", NONENUMERABLE_PREFIX, key, NONENUMERABLE_SUFFIX);
        let nc_key = format!("{}{}{}", NONCONFIGURABLE_PREFIX, key, NONCONFIGURABLE_SUFFIX);

        match &self.kind {
            PropKind::Data(v) => {
                map.insert(key.to_string(), v.clone());
                // Remove any stale accessor keys
                map.shift_remove(&getter_key);
                map.shift_remove(&setter_key);
            }
            PropKind::Accessor { get, set } => {
                // Remove stale data key
                map.shift_remove(key);
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
            }
        }

        // Attribute flags — only insert the marker when the attribute is OFF.
        if !self.attrs.contains(PropAttrs::WRITABLE) {
            map.insert(ro_key, Value::Boolean(true));
        } else {
            map.shift_remove(&ro_key);
        }
        if !self.attrs.contains(PropAttrs::ENUMERABLE) {
            map.insert(ne_key, Value::Boolean(true));
        } else {
            map.shift_remove(&ne_key);
        }
        if !self.attrs.contains(PropAttrs::CONFIGURABLE) {
            map.insert(nc_key, Value::Boolean(true));
        } else {
            map.shift_remove(&nc_key);
        }
    }

    /// Remove all legacy hidden keys for `key` from the map.
    pub fn remove_legacy_keys(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
        map.shift_remove(key);
        map.shift_remove(&format!("{}{}", GETTER_PREFIX, key));
        map.shift_remove(&format!("{}{}", SETTER_PREFIX, key));
        map.shift_remove(&format!("{}{}{}", READONLY_PREFIX, key, READONLY_SUFFIX));
        map.shift_remove(&format!("{}{}{}", NONENUMERABLE_PREFIX, key, NONENUMERABLE_SUFFIX));
        map.shift_remove(&format!("{}{}{}", NONCONFIGURABLE_PREFIX, key, NONCONFIGURABLE_SUFFIX));
    }
}

// ── Legacy conversion helpers ──────────────────────────────────────

/// Prefix constants matching the legacy hidden-key encoding.
pub const READONLY_PREFIX: &str = "__readonly_";
pub const NONENUMERABLE_PREFIX: &str = "__nonenumerable_";
pub const NONCONFIGURABLE_PREFIX: &str = "__nonconfigurable_";
pub const GETTER_PREFIX: &str = "__get_";
pub const SETTER_PREFIX: &str = "__set_";
pub const READONLY_SUFFIX: &str = "__";
pub const NONENUMERABLE_SUFFIX: &str = "__";
pub const NONCONFIGURABLE_SUFFIX: &str = "__";

/// Read just the attribute flags for `key` from a legacy hidden-key map.
///
/// This is a lightweight alternative to `desc_from_legacy_map` when you
/// only need the W/E/C bits and not the value or accessor functions.
pub fn attrs_from_legacy_map<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> PropAttrs {
    // Fast path: Value::Property carries inline attrs — use them directly.
    if let Some(Value::Property { attrs, .. }) = map.get(key) {
        return *attrs;
    }
    let mut attrs = PropAttrs::empty();
    if !map.contains_key(&format!("{}{}{}", READONLY_PREFIX, key, READONLY_SUFFIX)) {
        attrs |= PropAttrs::WRITABLE;
    }
    if !map.contains_key(&format!("{}{}{}", NONENUMERABLE_PREFIX, key, NONENUMERABLE_SUFFIX)) {
        attrs |= PropAttrs::ENUMERABLE;
    }
    if !map.contains_key(&format!("{}{}{}", NONCONFIGURABLE_PREFIX, key, NONCONFIGURABLE_SUFFIX)) {
        attrs |= PropAttrs::CONFIGURABLE;
    }
    attrs
}

/// Build a `PropDesc` by reading legacy hidden keys from an object map.
///
/// `key` is the user-visible property name.  The function looks up the
/// associated `__readonly_<key>__` / `__nonenumerable_<key>__` /
/// `__nonconfigurable_<key>__` / `__get_<key>` / `__set_<key>` entries
/// in `map` and produces the equivalent descriptor.
///
/// Returns `None` if `key` is not present (neither as data nor accessor).
pub fn desc_from_legacy_map<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<PropDesc<'gc>> {
    let getter_key = format!("{}{}", GETTER_PREFIX, key);
    let setter_key = format!("{}{}", SETTER_PREFIX, key);
    let has_data = map.contains_key(key);
    let has_getter = map.contains_key(&getter_key);
    let has_setter = map.contains_key(&setter_key);

    if !has_data && !has_getter && !has_setter {
        return None;
    }

    // Fast path: Value::Property carries inline attrs — use them directly.
    if let Some(Value::Property { getter, setter, attrs, .. }) = map.get(key) {
        let kind = PropKind::Accessor {
            get: getter.as_ref().map(|g| (**g).clone()),
            set: setter.as_ref().map(|s| (**s).clone()),
        };
        return Some(PropDesc { kind, attrs: *attrs });
    }

    let ro_key = format!("{}{}{}", READONLY_PREFIX, key, READONLY_SUFFIX);
    let ne_key = format!("{}{}{}", NONENUMERABLE_PREFIX, key, NONENUMERABLE_SUFFIX);
    let nc_key = format!("{}{}{}", NONCONFIGURABLE_PREFIX, key, NONCONFIGURABLE_SUFFIX);

    let writable = !map.contains_key(&ro_key);
    let enumerable = !map.contains_key(&ne_key);
    let configurable = !map.contains_key(&nc_key);

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

    let kind = if has_getter || has_setter {
        PropKind::Accessor {
            get: map.get(&getter_key).cloned(),
            set: map.get(&setter_key).cloned(),
        }
    } else {
        match map.get(key) {
            Some(v) => PropKind::Data(v.clone()),
            None => PropKind::Data(Value::Undefined),
        }
    };

    Some(PropDesc { kind, attrs })
}

/// Returns `true` if `key` is an internal hidden-key marker and should be
/// skipped during user-visible enumeration.
pub fn is_hidden_key(key: &str) -> bool {
    key.starts_with(READONLY_PREFIX)
        || key.starts_with(NONENUMERABLE_PREFIX)
        || key.starts_with(NONCONFIGURABLE_PREFIX)
        || key.starts_with(GETTER_PREFIX)
        || key.starts_with(SETTER_PREFIX)
}

// ── Individual flag helpers ─────────────────────────────────────────
//
// Thin wrappers over the legacy hidden-key encoding.  Each function
// touches exactly one marker key so callers don't need to know the
// `__prefix_key__` format strings.  When the underlying storage
// migrates to `PropDesc`, only these functions (and the batch helpers
// above) need to change.

/// Mark `key` as non-enumerable.
#[inline]
pub fn mark_nonenumerable<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    map.insert(
        format!("{}{}{}", NONENUMERABLE_PREFIX, key, NONENUMERABLE_SUFFIX),
        Value::Boolean(true),
    );
}

/// Remove non-enumerable marker for `key` (making it enumerable).
#[inline]
pub fn unmark_nonenumerable<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    map.shift_remove(&format!("{}{}{}", NONENUMERABLE_PREFIX, key, NONENUMERABLE_SUFFIX));
}

/// Returns `true` if `key` has a non-enumerable marker.
#[inline]
pub fn has_nonenumerable_mark<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> bool {
    map.contains_key(&format!("{}{}{}", NONENUMERABLE_PREFIX, key, NONENUMERABLE_SUFFIX))
}

/// Mark `key` as read-only (non-writable).
#[inline]
pub fn mark_readonly<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    map.insert(format!("{}{}{}", READONLY_PREFIX, key, READONLY_SUFFIX), Value::Boolean(true));
}

/// Remove read-only marker for `key` (making it writable).
#[inline]
pub fn unmark_readonly<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    map.shift_remove(&format!("{}{}{}", READONLY_PREFIX, key, READONLY_SUFFIX));
}

/// Returns `true` if `key` has a read-only marker.
#[inline]
pub fn has_readonly_mark<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> bool {
    map.contains_key(&format!("{}{}{}", READONLY_PREFIX, key, READONLY_SUFFIX))
}

/// Mark `key` as non-configurable.
#[inline]
pub fn mark_nonconfigurable<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    map.insert(
        format!("{}{}{}", NONCONFIGURABLE_PREFIX, key, NONCONFIGURABLE_SUFFIX),
        Value::Boolean(true),
    );
}

/// Remove non-configurable marker for `key`.
#[inline]
pub fn unmark_nonconfigurable<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    map.shift_remove(&format!("{}{}{}", NONCONFIGURABLE_PREFIX, key, NONCONFIGURABLE_SUFFIX));
}

/// Returns `true` if `key` has a non-configurable marker.
#[inline]
pub fn has_nonconfigurable_mark<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> bool {
    map.contains_key(&format!("{}{}{}", NONCONFIGURABLE_PREFIX, key, NONCONFIGURABLE_SUFFIX))
}

// ── Accessor helpers ───────────────────────────────────────────────

/// Get the getter function for `key`, if any.
#[inline]
pub fn get_getter<'a, 'gc>(map: &'a indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<&'a Value<'gc>> {
    map.get(&format!("{}{}", GETTER_PREFIX, key))
}

/// Get the setter function for `key`, if any.
#[inline]
pub fn get_setter<'a, 'gc>(map: &'a indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<&'a Value<'gc>> {
    map.get(&format!("{}{}", SETTER_PREFIX, key))
}

/// Returns `true` if `key` has a getter.
#[inline]
pub fn has_getter<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> bool {
    map.contains_key(&format!("{}{}", GETTER_PREFIX, key))
}

/// Returns `true` if `key` has a setter.
#[inline]
pub fn has_setter<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> bool {
    map.contains_key(&format!("{}{}", SETTER_PREFIX, key))
}

/// Set (or replace) the getter for `key`.
#[inline]
pub fn set_getter<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str, val: Value<'gc>) {
    map.insert(format!("{}{}", GETTER_PREFIX, key), val);
}

/// Set (or replace) the setter for `key`.
#[inline]
pub fn set_setter<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str, val: Value<'gc>) {
    map.insert(format!("{}{}", SETTER_PREFIX, key), val);
}

/// Remove the getter for `key`.
#[inline]
pub fn remove_getter<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    map.shift_remove(&format!("{}{}", GETTER_PREFIX, key));
}

/// Remove the setter for `key`.
#[inline]
pub fn remove_setter<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    map.shift_remove(&format!("{}{}", SETTER_PREFIX, key));
}

/// Look up the getter value for `key`, returning a reference if present.
#[inline]
pub fn lookup_getter<'a, 'gc>(map: &'a indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<&'a Value<'gc>> {
    map.get(&format!("{}{}", GETTER_PREFIX, key))
}

/// Look up the setter value for `key`, returning a reference if present.
#[inline]
pub fn lookup_setter<'a, 'gc>(map: &'a indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<&'a Value<'gc>> {
    map.get(&format!("{}{}", SETTER_PREFIX, key))
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

/// Get the readonly marker key string for `key`.
#[inline]
pub fn make_readonly_key(key: impl AsRef<str>) -> String {
    format!("{}{}{}", READONLY_PREFIX, key.as_ref(), READONLY_SUFFIX)
}

/// Get the nonenumerable marker key string for `key`.
#[inline]
pub fn make_nonenumerable_key(key: impl AsRef<str>) -> String {
    format!("{}{}{}", NONENUMERABLE_PREFIX, key.as_ref(), NONENUMERABLE_SUFFIX)
}

/// Get the nonconfigurable marker key string for `key`.
#[inline]
pub fn make_nonconfigurable_key(key: impl AsRef<str>) -> String {
    format!("{}{}{}", NONCONFIGURABLE_PREFIX, key.as_ref(), NONCONFIGURABLE_SUFFIX)
}

// ── Batch attribute write ──────────────────────────────────────────

/// Read the current attribute flags for `key` from a legacy hidden-key map.
///
/// Returns `PropAttrs` in positive sense: WRITABLE if no readonly mark,
/// ENUMERABLE if no nonenumerable mark, CONFIGURABLE if no nonconfigurable mark.
/// Defaults to all-true (writable + enumerable + configurable) when no marks exist.
pub fn read_attrs_from_legacy_map<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> PropAttrs {
    // Fast path: Value::Property carries inline attrs.
    if let Some(Value::Property { attrs, .. }) = map.get(key) {
        return *attrs;
    }
    let mut attrs = PropAttrs::all();
    if has_readonly_mark(map, key) {
        attrs.remove(PropAttrs::WRITABLE);
    }
    if has_nonenumerable_mark(map, key) {
        attrs.remove(PropAttrs::ENUMERABLE);
    }
    if has_nonconfigurable_mark(map, key) {
        attrs.remove(PropAttrs::CONFIGURABLE);
    }
    attrs
}

/// Write attribute flags for `key` into a legacy hidden-key map.
///
/// This is the inverse of `read_attrs_from_legacy_map`.  For each attribute
/// that is *off*, the corresponding marker key is inserted; for each
/// attribute that is *on*, any existing marker is removed.
pub fn write_attrs_to_legacy_map<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str, attrs: PropAttrs) {
    let ro_key = format!("{}{}{}", READONLY_PREFIX, key, READONLY_SUFFIX);
    let ne_key = format!("{}{}{}", NONENUMERABLE_PREFIX, key, NONENUMERABLE_SUFFIX);
    let nc_key = format!("{}{}{}", NONCONFIGURABLE_PREFIX, key, NONCONFIGURABLE_SUFFIX);

    if attrs.contains(PropAttrs::WRITABLE) {
        map.shift_remove(&ro_key);
    } else {
        map.insert(ro_key, Value::Boolean(true));
    }
    if attrs.contains(PropAttrs::ENUMERABLE) {
        map.shift_remove(&ne_key);
    } else {
        map.insert(ne_key, Value::Boolean(true));
    }
    if attrs.contains(PropAttrs::CONFIGURABLE) {
        map.shift_remove(&nc_key);
    } else {
        map.insert(nc_key, Value::Boolean(true));
    }
}

/// Remove a property and all of its associated attribute markers and accessor
/// entries from a legacy hidden-key map.
pub fn remove_property_completely<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    map.shift_remove(key);
    map.shift_remove(&format!("{}{}", GETTER_PREFIX, key));
    map.shift_remove(&format!("{}{}", SETTER_PREFIX, key));
    map.shift_remove(&format!("{}{}{}", READONLY_PREFIX, key, READONLY_SUFFIX));
    map.shift_remove(&format!("{}{}{}", NONENUMERABLE_PREFIX, key, NONENUMERABLE_SUFFIX));
    map.shift_remove(&format!("{}{}{}", NONCONFIGURABLE_PREFIX, key, NONCONFIGURABLE_SUFFIX));
}
