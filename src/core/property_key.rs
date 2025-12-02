use std::{cell::RefCell, rc::Rc};

use crate::core::Value;

#[derive(Clone, Debug)]
pub enum PropertyKey {
    String(String),
    Symbol(Rc<RefCell<Value>>),
}

impl From<&str> for PropertyKey {
    fn from(s: &str) -> Self {
        PropertyKey::String(s.to_string())
    }
}

impl From<String> for PropertyKey {
    fn from(s: String) -> Self {
        PropertyKey::String(s)
    }
}

impl From<&String> for PropertyKey {
    fn from(s: &String) -> Self {
        PropertyKey::String(s.clone())
    }
}

impl PartialEq for PropertyKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PropertyKey::String(s1), PropertyKey::String(s2)) => s1 == s2,
            (PropertyKey::Symbol(sym1), PropertyKey::Symbol(sym2)) => {
                if let (Value::Symbol(s1), Value::Symbol(s2)) = (&*sym1.borrow(), &*sym2.borrow()) {
                    Rc::ptr_eq(s1, s2)
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

impl Eq for PropertyKey {}

impl std::hash::Hash for PropertyKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            PropertyKey::String(s) => {
                0u8.hash(state);
                s.hash(state);
            }
            PropertyKey::Symbol(sym) => {
                1u8.hash(state);
                if let Value::Symbol(s) = &*sym.borrow() {
                    Rc::as_ptr(s).hash(state);
                }
            }
        }
    }
}

impl std::fmt::Display for PropertyKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PropertyKey::String(s) => write!(f, "{}", s),
            PropertyKey::Symbol(_) => write!(f, "[symbol]"),
        }
    }
}

impl AsRef<str> for PropertyKey {
    fn as_ref(&self) -> &str {
        match self {
            PropertyKey::String(s) => s,
            PropertyKey::Symbol(_) => "[symbol]",
        }
    }
}
