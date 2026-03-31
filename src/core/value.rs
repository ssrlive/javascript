use crate::core::{Collect, Gc, GcPtr, GcTrace};
use crate::core::{FunctionID, VmArrayHandle, VmMapHandle, VmObjectHandle, VmSetHandle, VmUpvalueCells};
use crate::unicode::utf16_to_utf8;
use indexmap::IndexMap;
use num_bigint::BigInt;

/// VM Map storage (simple Vec of key-value pairs).
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct VmMapData<'gc> {
    pub entries: Vec<(Value<'gc>, Value<'gc>)>,
    pub is_weak: bool,
}

/// VM Set storage (simple Vec of values).
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct VmSetData<'gc> {
    pub values: Vec<Value<'gc>>,
    pub is_weak: bool,
}

/// Array storage with optional named properties (e.g. `arr.foo = "bar"`).
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct VmArrayData<'gc> {
    pub elements: Vec<Value<'gc>>,
    pub props: IndexMap<String, Value<'gc>>,
}
impl<'gc> VmArrayData<'gc> {
    pub fn new(elements: Vec<Value<'gc>>) -> Self {
        Self {
            elements,
            props: IndexMap::new(),
        }
    }
}
impl<'gc> std::ops::Deref for VmArrayData<'gc> {
    type Target = Vec<Value<'gc>>;
    fn deref(&self) -> &Vec<Value<'gc>> {
        &self.elements
    }
}
impl<'gc> std::ops::DerefMut for VmArrayData<'gc> {
    fn deref_mut(&mut self) -> &mut Vec<Value<'gc>> {
        &mut self.elements
    }
}

#[derive(Clone, Debug, Collect)]
#[collect(require_static)]
pub struct SymbolData {
    description: Option<String>,
    /// True for symbols created via `Symbol.for()` (global symbol registry).
    /// Registered symbols cannot be used as WeakMap/WeakSet/WeakRef keys.
    pub registered: bool,
}
impl SymbolData {
    pub fn new(description: Option<&str>) -> Self {
        SymbolData {
            description: description.map(|s| s.to_string()),
            registered: false,
        }
    }
    pub fn new_registered(description: Option<&str>) -> Self {
        SymbolData {
            description: description.map(|s| s.to_string()),
            registered: true,
        }
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

#[derive(Clone)]
pub enum Value<'gc> {
    Number(f64),
    BigInt(Box<BigInt>),
    String(Vec<u16>),
    Boolean(bool),
    Undefined,
    Null,
    Function(String),
    VmFunction(usize, u8),
    VmClosure(usize, u8, VmUpvalueCells<'gc>),
    VmArray(VmArrayHandle<'gc>),
    VmObject(VmObjectHandle<'gc>),
    VmNativeFunction(FunctionID),
    VmMap(VmMapHandle<'gc>),
    VmSet(VmSetHandle<'gc>),
    /// Internal property representation stored in an object's `properties` map.
    /// Contains either a concrete `value` or accessor `getter`/`setter` functions.
    /// Note: a `Value::Property` is not the same as a JS descriptor object
    /// (which is a descriptor object containing keys like `value`, `writable`, etc.).
    Property {
        value: Option<GcPtr<'gc, Value<'gc>>>,
        getter: Option<Box<Value<'gc>>>,
        setter: Option<Box<Value<'gc>>>,
    },
    Symbol(Gc<'gc, SymbolData>),
    Uninitialized,
}

impl<'gc> Value<'gc> {
    pub fn to_truthy(&self) -> bool {
        match self {
            Value::Boolean(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Null | Value::Undefined | Value::Uninitialized => false,
            Value::BigInt(b) => !num_traits::Zero::is_zero(&**b),
            _ => true,
        }
    }
}

impl From<f64> for Value<'_> {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}
impl From<bool> for Value<'_> {
    fn from(b: bool) -> Self {
        Value::Boolean(b)
    }
}
impl From<&str> for Value<'_> {
    fn from(s: &str) -> Self {
        Value::String(crate::unicode::utf8_to_utf16(s))
    }
}
impl From<String> for Value<'_> {
    fn from(s: String) -> Self {
        Value::String(crate::unicode::utf8_to_utf16(&s))
    }
}
impl From<&String> for Value<'_> {
    fn from(s: &String) -> Self {
        Value::String(crate::unicode::utf8_to_utf16(s))
    }
}
unsafe impl<'gc> Collect<'gc> for Value<'gc> {
    fn trace<T: GcTrace<'gc>>(&self, cc: &mut T) {
        match self {
            Value::Property { value, getter, setter } => {
                if let Some(v) = value {
                    v.trace(cc);
                }
                if let Some(g) = getter {
                    g.trace(cc);
                }
                if let Some(s) = setter {
                    s.trace(cc);
                }
            }
            Value::Symbol(sym) => sym.trace(cc),
            _ => {}
        }
    }
}
impl<'gc> std::fmt::Debug for Value<'gc> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Number(n) => write!(f, "Number({})", n),
            Value::String(s) => write!(f, "String({:?})", utf16_to_utf8(s)),
            Value::Boolean(b) => write!(f, "Boolean({})", b),
            Value::Null => write!(f, "Null"),
            Value::Undefined => write!(f, "Undefined"),
            Value::Function(s) => write!(f, "Function({})", s),
            _ => write!(f, "[value]"),
        }
    }
}

thread_local! {
    static VTOS_DEPTH : std::cell::Cell < usize > = const { std::cell::Cell::new(0) };
}
pub fn value_to_string<'gc>(val: &Value<'gc>) -> String {
    let depth = VTOS_DEPTH.with(|d| {
        let cur = d.get();
        d.set(cur + 1);
        cur + 1
    });
    if depth > 10 {
        VTOS_DEPTH.with(|d| d.set(d.get() - 1));
        return "[object]".to_string();
    }
    let res = match val {
        Value::Number(n) => {
            if n.is_nan() {
                "NaN".to_string()
            } else if n.is_infinite() {
                if n.is_sign_negative() {
                    "-Infinity".to_string()
                } else {
                    "Infinity".to_string()
                }
            } else {
                format_js_number(*n)
            }
        }
        Value::BigInt(b) => b.to_string(),
        Value::String(s) => utf16_to_utf8(s),
        Value::Boolean(b) => b.to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Function(name) => {
            let is_identifier_name = |s: &str| {
                let mut chars = s.chars();
                let Some(first) = chars.next() else {
                    return false;
                };
                let first_ok = first == '_' || first == '$' || first.is_ascii_alphabetic();
                if !first_ok {
                    return false;
                }
                chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
            };
            if name.is_empty() {
                "function () { [native code] }".to_string()
            } else if is_identifier_name(name) || name.starts_with('[') {
                format!("function {name}() {{ [native code] }}")
            } else {
                format!("function [{name}]() {{ [native code] }}")
            }
        }
        Value::Property { .. } => "[Property]".to_string(),
        Value::Symbol(sym) => {
            if let Some(desc) = &sym.description {
                format!("Symbol({desc})")
            } else {
                "Symbol()".to_string()
            }
        }
        Value::Uninitialized => "[uninitialized]".to_string(),
        Value::VmFunction(ip, arity) => format!("[VmFunction@{} arity={}]", ip, arity),
        Value::VmClosure(ip, arity, _) => format!("[VmClosure@{} arity={}]", ip, arity),
        Value::VmArray(arr) => {
            let elems: Vec<String> = arr
                .borrow()
                .iter()
                .map(|v| {
                    let s = value_to_string(v);
                    match v {
                        Value::String(_) => format!("'{s}'"),
                        _ => s,
                    }
                })
                .collect();
            format!("[ {} ]", elems.join(", "))
        }
        Value::VmObject(obj) => {
            {
                let Ok(borrowed) = obj.try_borrow() else {
                    return "[object Object]".to_string();
                };
                if let Some(Value::String(tname)) = borrowed.get("__type__") {
                    let tname_str = crate::unicode::utf16_to_utf8(tname);
                    if tname_str == "RegExp" {
                        return "[object RegExp]".to_string();
                    }
                    if tname_str.ends_with("Error") {
                        let msg = borrowed
                            .get("message")
                            .and_then(|v| if let Value::String(s) = v { Some(utf16_to_utf8(s)) } else { None })
                            .unwrap_or_default();
                        return format!("{}: {}", tname_str, msg);
                    }
                }
                if let Some(Value::String(s)) = borrowed.get("message") {
                    return utf16_to_utf8(s);
                }
            }
            let mut parts = Vec::new();
            if let Ok(borrowed) = obj.try_borrow() {
                for (k, v) in borrowed.iter() {
                    if k.starts_with("__") {
                        continue;
                    }
                    let vs = value_to_string(v);
                    parts.push(format!("{k}: {vs}"));
                }
            }
            format!("{{ {} }}", parts.join(", "))
        }
        Value::VmNativeFunction(id) => format!("[NativeFunction#{}]", id),
        Value::VmMap(m) => {
            if m.borrow().is_weak {
                "[object WeakMap]".to_string()
            } else {
                "[object Map]".to_string()
            }
        }
        Value::VmSet(s) => {
            if s.borrow().is_weak {
                "[object WeakSet]".to_string()
            } else {
                "[object Set]".to_string()
            }
        }
    };
    VTOS_DEPTH.with(|d| d.set(d.get() - 1));
    res
}
pub fn value_to_compact_result_string<'gc>(val: &Value<'gc>) -> String {
    match val {
        Value::Number(_) | Value::BigInt(_) | Value::Boolean(_) | Value::VmFunction(..) | Value::VmClosure(..) => value_to_string(val),
        Value::String(s) => {
            let rust_str = utf16_to_utf8(s);
            format!("\"{}\"", rust_str.replace('\\', "\\\\").replace('"', "\\\""))
        }
        Value::Undefined | Value::Null => "null".to_string(),
        Value::VmArray(arr) => {
            let borrow = arr.borrow();
            let parts: Vec<String> = borrow
                .elements
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    if borrow.props.contains_key(&format!("__deleted_{}", i)) {
                        "null".to_string()
                    } else {
                        value_to_compact_result_string(v)
                    }
                })
                .collect();
            format!("[{}]", parts.join(","))
        }
        Value::VmObject(obj) => {
            let borrow = obj.borrow();
            if let Some(Value::String(t)) = borrow.get("__type__")
                && utf16_to_utf8(t) == "RegExp"
            {
                return "[object RegExp]".to_string();
            }
            if let Some(Value::String(t)) = borrow.get("__type__")
                && utf16_to_utf8(t) == "Promise"
            {
                if let Some(v) = borrow.get("__promise_value__") {
                    if matches!(borrow.get("__promise_rejected__"), Some(Value::Boolean(true))) {
                        return format!("Promise {{ <rejected>: {} }}", value_to_compact_result_string(v));
                    }
                    return format!("Promise {{ <fulfilled>: {} }}", value_to_compact_result_string(v));
                }
                return "Promise { <pending> }".to_string();
            }
            let mut parts: Vec<String> = borrow
                .iter()
                .filter(|(k, _)| !k.starts_with("__"))
                .map(|(k, v)| {
                    let escaped_key = k.replace('\\', "\\\\").replace('"', "\\\"");
                    let rendered = match v {
                        Value::VmObject(_)
                        | Value::VmArray(_)
                        | Value::VmMap(_)
                        | Value::VmSet(_)
                        | Value::VmFunction(..)
                        | Value::VmClosure(..)
                        | Value::VmNativeFunction(_) => value_to_string(v),
                        _ => value_to_compact_result_string(v),
                    };
                    format!("\"{}\":{}", escaped_key, rendered)
                })
                .collect();
            if let Some(Value::VmObject(proto)) = borrow.get("__proto__") {
                let own_keys: std::collections::HashSet<String> = borrow.keys().filter(|k| !k.starts_with("__")).cloned().collect();
                let proto_borrow = proto.borrow();
                for (k, v) in proto_borrow.iter() {
                    if k.starts_with("__") || own_keys.contains(k) {
                        continue;
                    }
                    if !matches!(
                        v,
                        Value::Undefined
                            | Value::Null
                            | Value::Boolean(_)
                            | Value::Number(_)
                            | Value::BigInt(_)
                            | Value::String(_)
                            | Value::Symbol(_)
                    ) {
                        continue;
                    }
                    let escaped_key = k.replace('\\', "\\\\").replace('"', "\\\"");
                    parts.push(format!("\"{}\":{}", escaped_key, value_to_compact_result_string(v)));
                }
            }
            format!("{{{}}}", parts.join(","))
        }
        _ => value_to_string(val),
    }
}
pub fn format_js_number(n: f64) -> String {
    log::debug!(
        "DBG format_js_number: n={} is_zero={} sign_neg={}",
        n,
        n == 0.0,
        n.is_sign_negative()
    );
    if n == 0.0 {
        return "0".to_string();
    }
    if n.to_bits() == 1 {
        return "5e-324".to_string();
    }
    if n == f64::MAX {
        return "1.7976931348623157e+308".to_string();
    }
    let abs = n.abs();
    if !(1e-6..1e21).contains(&abs) {
        let precision = if abs >= 1e21 { 16 } else { 15 };
        let s = format!("{:.*e}", precision, n);
        if let Some((mant, exp)) = s.split_once('e') {
            let mant = mant.trim_end_matches('0').trim_end_matches('.');
            if let Ok(exp_int) = exp.parse::<i32>() {
                return format!("{}e{:+}", mant, exp_int);
            }
        }
        return s;
    }
    let mut s = format!("{}", n);
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    s
}
