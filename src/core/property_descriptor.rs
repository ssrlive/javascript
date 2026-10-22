//! Unified property descriptor model.
//!
//! Property descriptors are stored inline with their property values.

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

    /// Accessor property with explicit attributes.
    pub fn accessor(get: Option<Value<'gc>>, set: Option<Value<'gc>>, attrs: PropAttrs) -> Self {
        Self {
            kind: PropKind::Accessor { get, set },
            attrs,
        }
    }

    #[inline]
    pub fn is_accessor(&self) -> bool {
        matches!(self.kind, PropKind::Accessor { .. })
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

    /// Write this descriptor into the object's property map.
    pub fn write_to_map(&self, map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
        match &self.kind {
            PropKind::Data(v) => {
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

    /// Remove the own property identified by `key`.
    pub fn remove_property_key(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
        map.shift_remove(key);
    }
}

// ── Property map helpers ───────────────────────────────────────────

/// Prefix constants used for private accessor storage slots.
pub const PRIVATE_GETTER_SLOT_PREFIX: &str = "__get_";
pub const PRIVATE_SETTER_SLOT_PREFIX: &str = "__set_";

/// Read just the attribute flags for `key` from a property map.
pub fn attrs_from_map<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> PropAttrs {
    desc_from_map(map, key).map(|d| d.attrs).unwrap_or(PropAttrs::WEC)
}

/// Build a `PropDesc` from a property map.
pub fn desc_from_map<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<PropDesc<'gc>> {
    if let Some(Value::Property {
        value,
        getter,
        setter,
        attrs,
    }) = map.get(key)
    {
        if value.is_none() || getter.is_some() || setter.is_some() {
            return Some(PropDesc {
                kind: PropKind::Accessor {
                    get: getter.as_ref().map(|g| (**g).clone()),
                    set: setter.as_ref().map(|s| (**s).clone()),
                },
                attrs: *attrs,
            });
        }
        let data = value.as_ref().map(|v| (**v).clone()).unwrap_or(Value::Undefined);
        return Some(PropDesc {
            kind: PropKind::Data(data),
            attrs: *attrs,
        });
    }

    map.get(key).cloned().map(|v| PropDesc {
        kind: PropKind::Data(v),
        attrs: PropAttrs::WEC,
    })
}

/// Read the own data value for `key`, unwrapping `Value::Property` when the
/// property is represented inline. Returns `None` for accessors or missing keys.
pub fn own_data_from_map<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<Value<'gc>> {
    match desc_from_map(map, key)?.kind {
        PropKind::Data(v) => Some(v),
        PropKind::Accessor { .. } => None,
    }
}

#[inline]
fn update_attrs_for_key<'gc, F>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str, f: F)
where
    F: FnOnce(PropAttrs) -> PropAttrs,
{
    if let Some(mut desc) = desc_from_map(map, key) {
        desc.attrs = f(desc.attrs);
        desc.write_to_map(map, key);
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
    desc_from_map(map, key)
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
    match desc_from_map(map, key) {
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

/// Returns `true` if `key` is non-configurable.
#[inline]
pub fn has_nonconfigurable_mark<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> bool {
    desc_from_map(map, key)
        .map(|d| !d.attrs.contains(PropAttrs::CONFIGURABLE))
        .unwrap_or(false)
}

// ── Accessor helpers ───────────────────────────────────────────────

/// Get the getter function for `key`, if any.
#[inline]
pub fn get_getter<'a, 'gc>(map: &'a indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<&'a Value<'gc>> {
    match map.get(key) {
        Some(Value::Property { getter: Some(g), .. }) => Some(g.as_ref()),
        _ => None,
    }
}

/// Returns `true` if `key` has a getter.
#[inline]
pub fn has_getter<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> bool {
    matches!(map.get(key), Some(Value::Property { getter: Some(_), .. }))
}

/// Returns `true` if `key` has a setter.
#[inline]
pub fn has_setter<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> bool {
    matches!(map.get(key), Some(Value::Property { setter: Some(_), .. }))
}

/// Set (or replace) the getter for `key`.
#[inline]
pub fn set_getter<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str, val: Value<'gc>) {
    let current_set = lookup_setter(map, key).cloned();
    let attrs = desc_from_map(map, key)
        .map(|d| if d.is_accessor() { d.attrs } else { PropAttrs::EC })
        .unwrap_or(PropAttrs::EC);
    PropDesc::accessor(Some(val), current_set, attrs).write_to_map(map, key);
}

/// Set (or replace) the setter for `key`.
#[inline]
pub fn set_setter<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str, val: Value<'gc>) {
    let current_get = lookup_getter(map, key).cloned();
    let attrs = desc_from_map(map, key)
        .map(|d| if d.is_accessor() { d.attrs } else { PropAttrs::EC })
        .unwrap_or(PropAttrs::EC);
    PropDesc::accessor(current_get, Some(val), attrs).write_to_map(map, key);
}

/// Remove the getter for `key`.
#[inline]
pub fn remove_getter<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    if let Some(desc) = desc_from_map(map, key)
        && let PropKind::Accessor { set, .. } = desc.kind
    {
        if set.is_some() {
            PropDesc::accessor(None, set, desc.attrs).write_to_map(map, key);
        } else {
            map.shift_remove(key);
        }
    }
}

/// Remove the setter for `key`.
#[inline]
pub fn remove_setter<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    if let Some(desc) = desc_from_map(map, key)
        && let PropKind::Accessor { get, .. } = desc.kind
    {
        if get.is_some() {
            PropDesc::accessor(get, None, desc.attrs).write_to_map(map, key);
        } else {
            map.shift_remove(key);
        }
    }
}

/// Look up the getter value for `key`, returning a reference if present.
#[inline]
pub fn lookup_getter<'a, 'gc>(map: &'a indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<&'a Value<'gc>> {
    get_getter(map, key)
}

/// Look up the setter value for `key`, returning a reference if present.
#[inline]
pub fn lookup_setter<'a, 'gc>(map: &'a indexmap::IndexMap<String, Value<'gc>>, key: &str) -> Option<&'a Value<'gc>> {
    match map.get(key) {
        Some(Value::Property { setter: Some(s), .. }) => Some(s.as_ref()),
        _ => None,
    }
}

/// Build the private getter slot name for `key`.
#[inline]
pub fn private_getter_slot_key(key: impl AsRef<str>) -> String {
    format!("{}{}", PRIVATE_GETTER_SLOT_PREFIX, key.as_ref())
}

/// Build the private setter slot name for `key`.
#[inline]
pub fn private_setter_slot_key(key: impl AsRef<str>) -> String {
    format!("{}{}", PRIVATE_SETTER_SLOT_PREFIX, key.as_ref())
}

// ── Batch attribute write ──────────────────────────────────────────

/// Read the current attribute flags for `key` from a property map.
pub fn read_attrs_from_map<'gc>(map: &indexmap::IndexMap<String, Value<'gc>>, key: &str) -> PropAttrs {
    attrs_from_map(map, key)
}

/// Write attribute flags for `key` into a property map.
pub fn write_attrs_to_map<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str, attrs: PropAttrs) {
    if let Some(mut desc) = desc_from_map(map, key) {
        desc.attrs = attrs;
        desc.write_to_map(map, key);
    }
}

/// Remove a property.
pub fn remove_property_completely<'gc>(map: &mut indexmap::IndexMap<String, Value<'gc>>, key: &str) {
    map.shift_remove(key);
}
