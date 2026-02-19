use crate::core::{Collect, Gc};
use crate::core::{InternalSlot, SymbolData, Value};
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Debug, Collect)]
#[collect(no_drop)]
pub enum PropertyKey<'gc> {
    String(String),
    Symbol(Gc<'gc, SymbolData>),
    Private(String, u32),
    /// Engine-internal slot key. JS code can never produce this variant,
    /// guaranteeing zero collision with user-defined properties.
    Internal(InternalSlot),
}

static PRIVATE_ID_COUNTER: AtomicU32 = AtomicU32::new(0);

pub fn next_private_id() -> u32 {
    PRIVATE_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn remove_private_identifier_prefix(name: &str) -> &str {
    if let Some(stripped) = name.strip_prefix('#') {
        stripped
    } else {
        name
    }
}

impl<'gc> From<&Gc<'gc, SymbolData>> for PropertyKey<'gc> {
    fn from(s: &Gc<'gc, SymbolData>) -> Self {
        Self::from(*s)
    }
}

impl<'gc> From<Gc<'gc, SymbolData>> for PropertyKey<'gc> {
    fn from(s: Gc<'gc, SymbolData>) -> Self {
        PropertyKey::Symbol(s)
    }
}

impl<'gc> From<usize> for PropertyKey<'gc> {
    fn from(n: usize) -> Self {
        PropertyKey::String(n.to_string())
    }
}

impl<'gc> From<&PropertyKey<'gc>> for PropertyKey<'gc> {
    fn from(pk: &PropertyKey<'gc>) -> Self {
        match pk {
            PropertyKey::String(s) => PropertyKey::String(s.clone()),
            PropertyKey::Symbol(sym) => PropertyKey::Symbol(*sym),
            PropertyKey::Private(s, id) => PropertyKey::Private(s.clone(), *id),
            PropertyKey::Internal(slot) => PropertyKey::Internal(slot.clone()),
        }
    }
}

impl<'gc> From<&str> for PropertyKey<'gc> {
    fn from(s: &str) -> Self {
        PropertyKey::String(s.to_string())
    }
}

impl<'gc> From<String> for PropertyKey<'gc> {
    fn from(s: String) -> Self {
        PropertyKey::String(s)
    }
}

impl<'gc> From<InternalSlot> for PropertyKey<'gc> {
    fn from(slot: InternalSlot) -> Self {
        PropertyKey::Internal(slot)
    }
}

impl<'gc> From<&String> for PropertyKey<'gc> {
    fn from(s: &String) -> Self {
        PropertyKey::String(s.clone())
    }
}

impl<'gc> From<&Value<'gc>> for PropertyKey<'gc> {
    fn from(v: &Value<'gc>) -> Self {
        match v {
            Value::Symbol(sd) => PropertyKey::Symbol(*sd),
            other => PropertyKey::String(crate::core::value_to_string(other)),
        }
    }
}

impl<'gc> From<Value<'gc>> for PropertyKey<'gc> {
    fn from(v: Value<'gc>) -> Self {
        PropertyKey::from(&v)
    }
}

impl<'gc> PartialEq for PropertyKey<'gc> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PropertyKey::String(s1), PropertyKey::String(s2)) => s1 == s2,
            (PropertyKey::Symbol(sym1), PropertyKey::Symbol(sym2)) => Gc::ptr_eq(*sym1, *sym2),
            (PropertyKey::Private(s1, id1), PropertyKey::Private(s2, id2)) => s1 == s2 && id1 == id2,
            (PropertyKey::Internal(a), PropertyKey::Internal(b)) => a == b,
            _ => false,
        }
    }
}

impl<'gc> Eq for PropertyKey<'gc> {}

impl<'gc> std::hash::Hash for PropertyKey<'gc> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            PropertyKey::String(s) => {
                0u8.hash(state);
                s.hash(state);
            }
            PropertyKey::Symbol(sym) => {
                1u8.hash(state);
                Gc::as_ptr(*sym).hash(state);
            }
            PropertyKey::Private(s, id) => {
                2u8.hash(state);
                s.hash(state);
                id.hash(state);
            }
            PropertyKey::Internal(slot) => {
                3u8.hash(state);
                slot.hash(state);
            }
        }
    }
}

impl<'gc> std::fmt::Display for PropertyKey<'gc> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PropertyKey::String(s) => write!(f, "{s}"),
            PropertyKey::Symbol(sym) => write!(f, "[symbol {:p}]", Gc::as_ptr(*sym)),
            PropertyKey::Private(s, _) => write!(f, "#{s}"),
            PropertyKey::Internal(slot) => write!(f, "[internal {slot:?}]"),
        }
    }
}

impl<'gc> AsRef<str> for PropertyKey<'gc> {
    fn as_ref(&self) -> &str {
        match self {
            PropertyKey::String(s) => s,
            PropertyKey::Symbol(_sym) => todo!("Cannot convert Symbol to &str"),
            PropertyKey::Private(s, _) => s,
            PropertyKey::Internal(_) => todo!("Cannot convert Internal slot to &str"),
        }
    }
}
