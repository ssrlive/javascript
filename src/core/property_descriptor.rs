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

use crate::core::value::Value;
use crate::core::{Collect, GcPtr, GcTrace};
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
    Accessor {
        get: Option<Value<'gc>>,
        set: Option<Value<'gc>>,
    },
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
    pub fn accessor(
        get: Option<Value<'gc>>,
        set: Option<Value<'gc>>,
        attrs: PropAttrs,
    ) -> Self {
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

/// Build a `PropDesc` by reading legacy hidden keys from an object map.
///
/// `key` is the user-visible property name.  The function looks up the
/// associated `__readonly_<key>__` / `__nonenumerable_<key>__` /
/// `__nonconfigurable_<key>__` / `__get_<key>` / `__set_<key>` entries
/// in `map` and produces the equivalent descriptor.
///
/// Returns `None` if `key` is not present (neither as data nor accessor).
pub fn desc_from_legacy_map<'gc>(
    map: &indexmap::IndexMap<String, Value<'gc>>,
    key: &str,
) -> Option<PropDesc<'gc>> {
    let getter_key = format!("{}{}", GETTER_PREFIX, key);
    let setter_key = format!("{}{}", SETTER_PREFIX, key);
    let has_data = map.contains_key(key);
    let has_getter = map.contains_key(&getter_key);
    let has_setter = map.contains_key(&setter_key);

    if !has_data && !has_getter && !has_setter {
        return None;
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
        PropKind::Data(map.get(key).cloned().unwrap_or(Value::Undefined))
    };

    Some(PropDesc { kind, attrs })
}

/// Returns `true` if `key` is an internal hidden-key marker and should be
/// skipped during user-visible enumeration.
pub fn is_hidden_key(key: &str) -> bool {
    key.starts_with("__readonly_")
        || key.starts_with("__nonenumerable_")
        || key.starts_with("__nonconfigurable_")
        || key.starts_with("__get_")
        || key.starts_with("__set_")
}
