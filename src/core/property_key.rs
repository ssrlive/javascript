use crate::core::Value;
use crate::core::gc::GcPtr;
use gc_arena::Collect;

#[derive(Clone, Debug, Collect)]
#[collect(no_drop)]
pub enum PropertyKey<'gc> {
    String(String),
    Symbol(GcPtr<'gc, Value<'gc>>),
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

impl<'gc> From<&String> for PropertyKey<'gc> {
    fn from(s: &String) -> Self {
        PropertyKey::String(s.clone())
    }
}

impl<'gc> PartialEq for PropertyKey<'gc> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PropertyKey::String(s1), PropertyKey::String(s2)) => s1 == s2,
            (PropertyKey::Symbol(sym1), PropertyKey::Symbol(sym2)) => {
                if let (Value::Symbol(s1), Value::Symbol(s2)) = (&*sym1.borrow(), &*sym2.borrow()) {
                    gc_arena::Gc::ptr_eq(*s1, *s2)
                } else {
                    false
                }
            }
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
                if let Value::Symbol(s) = &*sym.borrow() {
                    gc_arena::Gc::as_ptr(*s).hash(state);
                }
            }
        }
    }
}

impl<'gc> std::fmt::Display for PropertyKey<'gc> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PropertyKey::String(s) => write!(f, "{}", s),
            PropertyKey::Symbol(ptr) => write!(f, "[symbol {:p}]", gc_arena::Gc::as_ptr(*ptr)),
        }
    }
}

impl<'gc> AsRef<str> for PropertyKey<'gc> {
    fn as_ref(&self) -> &str {
        match self {
            PropertyKey::String(s) => s,
            PropertyKey::Symbol(_ptr) => todo!("Cannot convert Symbol to &str"),
        }
    }
}
