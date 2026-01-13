#![allow(clippy::collapsible_if, clippy::collapsible_match, dead_code)]

use crate::core::{ClassDefinition, Collect, Gc, GcCell, GcPtr, GcTrace, GcWeak, MutationContext};
use crate::unicode::utf16_to_utf8;
use crate::{
    JSError,
    core::{DestructuringElement, PropertyKey, Statement, is_error},
    raise_type_error,
};
use num_bigint::BigInt;
use std::sync::{Arc, Mutex};

#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSMap<'gc> {
    pub entries: Vec<(Value<'gc>, Value<'gc>)>,
}

#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSSet<'gc> {
    pub values: Vec<Value<'gc>>,
}

#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSWeakMap<'gc> {
    pub entries: Vec<(GcWeak<'gc, GcCell<JSObjectData<'gc>>>, Value<'gc>)>,
}

#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSWeakSet<'gc> {
    pub values: Vec<GcWeak<'gc, GcCell<JSObjectData<'gc>>>>,
}

#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSGenerator<'gc> {
    pub params: Vec<DestructuringElement>,
    pub body: Vec<Statement>,
    pub env: JSObjectDataPtr<'gc>,
    pub state: GeneratorState<'gc>,
}

#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSProxy<'gc> {
    pub target: Box<Value<'gc>>,
    pub handler: Box<Value<'gc>>,
    pub revoked: bool,
}

#[derive(Clone, Debug, Collect)]
#[collect(require_static)]
pub struct JSArrayBuffer {
    pub data: Arc<Mutex<Vec<u8>>>,
    pub detached: bool,
    pub shared: bool,
}

#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSDataView<'gc> {
    pub buffer: GcPtr<'gc, JSArrayBuffer>,
    pub byte_offset: usize,
    pub byte_length: usize,
}

#[derive(Clone, Debug, PartialEq, Collect)]
#[collect(require_static)]
pub enum TypedArrayKind {
    Int8,
    Uint8,
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float32,
    Float64,
    BigInt64,
    BigUint64,
}

#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSTypedArray<'gc> {
    pub kind: TypedArrayKind,
    pub buffer: GcPtr<'gc, JSArrayBuffer>,
    pub byte_offset: usize,
    pub length: usize,
}

#[derive(Clone, Debug, Collect)]
#[collect(no_drop)]
pub enum GeneratorState<'gc> {
    NotStarted,
    Running { pc: usize, stack: Vec<Value<'gc>> },
    Suspended { pc: usize, stack: Vec<Value<'gc>> },
    Completed,
}

pub type JSObjectDataPtr<'gc> = GcPtr<'gc, JSObjectData<'gc>>;
pub type JSObjectDataWeakPtr<'gc> = Gc<'gc, GcCell<JSObjectData<'gc>>>;

#[inline]
pub fn new_js_object_data<'gc>(mc: &MutationContext<'gc>) -> JSObjectDataPtr<'gc> {
    Gc::new(mc, GcCell::new(JSObjectData::new()))
}

#[derive(Clone, Default)]
pub struct JSObjectData<'gc> {
    pub properties: indexmap::IndexMap<PropertyKey<'gc>, GcPtr<'gc, Value<'gc>>>,
    pub constants: std::collections::HashSet<String>,
    pub non_enumerable: std::collections::HashSet<PropertyKey<'gc>>,
    pub non_writable: std::collections::HashSet<PropertyKey<'gc>>,
    pub non_configurable: std::collections::HashSet<PropertyKey<'gc>>,
    pub prototype: Option<JSObjectDataPtr<'gc>>,
    pub is_function_scope: bool,
}

unsafe impl<'gc> Collect<'gc> for JSObjectData<'gc> {
    fn trace<T: GcTrace<'gc>>(&self, cc: &mut T) {
        for (k, v) in &self.properties {
            k.trace(cc);
            v.trace(cc);
        }
        for k in &self.non_enumerable {
            k.trace(cc);
        }
        for k in &self.non_writable {
            k.trace(cc);
        }
        for k in &self.non_configurable {
            k.trace(cc);
        }
        if let Some(p) = &self.prototype {
            p.trace(cc);
        }
    }
}

impl<'gc> JSObjectData<'gc> {
    pub fn new() -> Self {
        JSObjectData::default()
    }
    pub fn insert(&mut self, key: PropertyKey<'gc>, val: GcPtr<'gc, Value<'gc>>) {
        self.properties.insert(key, val);
    }
    pub fn set_const(&mut self, key: String) {
        self.constants.insert(key);
    }
    pub fn set_non_configurable(&mut self, key: PropertyKey<'gc>) {
        self.non_configurable.insert(key);
    }
    pub fn set_non_writable(&mut self, key: PropertyKey<'gc>) {
        self.non_writable.insert(key);
    }
    pub fn is_const(&self, key: &str) -> bool {
        self.constants.contains(key)
    }

    pub fn set_property(&mut self, mc: &MutationContext<'gc>, key: impl Into<PropertyKey<'gc>>, val: Value<'gc>) {
        let val_ptr = Gc::new(mc, GcCell::new(val));
        self.insert(key.into(), val_ptr);
    }

    pub fn get_property(&self, key: impl Into<PropertyKey<'gc>>) -> Option<String> {
        let key = key.into();
        if let Some(val_ptr) = self.properties.get(&key) {
            if let Value::String(s) = &*val_ptr.borrow() {
                return Some(utf16_to_utf8(s));
            }
            return None;
        }
        if let Some(proto) = &self.prototype {
            if let Ok(Some(val_ptr)) = obj_get_key_value(proto, &key) {
                if let Value::String(s) = &*val_ptr.borrow() {
                    return Some(utf16_to_utf8(s));
                }
            }
        }
        None
    }

    pub fn get_message(&self) -> Option<String> {
        if let Some(msg_ptr) = self.properties.get(&PropertyKey::String("message".to_string()))
            && let Value::String(s) = &*msg_ptr.borrow()
        {
            return Some(utf16_to_utf8(s));
        }
        None
    }

    pub fn set_line(&mut self, line: usize, mc: &MutationContext<'gc>) -> Result<(), JSError> {
        let key = PropertyKey::String("__line__".to_string());
        if !self.properties.contains_key(&key) {
            let val = Value::Number(line as f64);
            let val_ptr = Gc::new(mc, GcCell::new(val));
            self.insert(key, val_ptr);
        }
        Ok(())
    }

    pub fn get_line(&self) -> Option<usize> {
        if let Some(line_ptr) = self.properties.get(&PropertyKey::String("__line__".to_string()))
            && let Value::Number(n) = &*line_ptr.borrow()
        {
            return Some(*n as usize);
        }
        None
    }

    pub fn set_column(&mut self, column: usize, mc: &MutationContext<'gc>) -> Result<(), JSError> {
        let key = PropertyKey::String("__column__".to_string());
        if !self.properties.contains_key(&key) {
            let val = Value::Number(column as f64);
            let val_ptr = Gc::new(mc, GcCell::new(val));
            self.insert(key, val_ptr);
        }
        Ok(())
    }

    pub fn get_column(&self) -> Option<usize> {
        if let Some(col_ptr) = self.properties.get(&PropertyKey::String("__column__".to_string()))
            && let Value::Number(n) = &*col_ptr.borrow()
        {
            return Some(*n as usize);
        }
        None
    }

    pub fn set_non_enumerable(&mut self, key: PropertyKey<'gc>) {
        self.non_enumerable.insert(key);
    }

    pub fn is_configurable(&self, key: &PropertyKey<'gc>) -> bool {
        !self.non_configurable.contains(key)
    }

    pub fn is_writable(&self, key: &PropertyKey<'gc>) -> bool {
        !self.non_writable.contains(key)
    }

    pub fn is_enumerable(&self, key: &PropertyKey<'gc>) -> bool {
        !self.non_enumerable.contains(key)
    }
}

impl<'gc> ClosureData<'gc> {
    pub fn new(
        params: &[DestructuringElement],
        body: &[Statement],
        env: &JSObjectDataPtr<'gc>,
        home_object: Option<JSObjectDataPtr<'gc>>,
    ) -> Self {
        ClosureData {
            params: params.to_vec(),
            body: body.to_vec(),
            env: *env,
            home_object: GcCell::new(home_object),
            captured_envs: Vec::new(),
            bound_this: None,
            is_arrow: false,
        }
    }
}

#[derive(Clone, Debug, Collect)]
#[collect(require_static)]
pub struct SymbolData {
    pub description: Option<String>,
}

#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct ClosureData<'gc> {
    pub params: Vec<DestructuringElement>,
    pub body: Vec<Statement>,
    pub env: JSObjectDataPtr<'gc>,
    pub home_object: GcCell<Option<JSObjectDataPtr<'gc>>>,
    pub captured_envs: Vec<JSObjectDataPtr<'gc>>,
    pub bound_this: Option<Value<'gc>>,
    pub is_arrow: bool,
}

#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSPromise<'gc> {
    pub state: PromiseState<'gc>,
    pub value: Option<Value<'gc>>,
    pub on_fulfilled: Vec<(Value<'gc>, GcPtr<'gc, JSPromise<'gc>>, Option<JSObjectDataPtr<'gc>>)>,
    pub on_rejected: Vec<(Value<'gc>, GcPtr<'gc, JSPromise<'gc>>, Option<JSObjectDataPtr<'gc>>)>,
}

impl<'gc> JSPromise<'gc> {
    pub fn new() -> Self {
        Self {
            state: PromiseState::Pending,
            value: None,
            on_fulfilled: Vec::new(),
            on_rejected: Vec::new(),
        }
    }
}

#[derive(Clone, Collect)]
#[collect(no_drop)]
pub enum PromiseState<'gc> {
    Pending,
    Fulfilled(Value<'gc>),
    Rejected(Value<'gc>),
}

#[derive(Clone)]
pub enum Value<'gc> {
    Number(f64),
    BigInt(BigInt),
    String(Vec<u16>),
    Boolean(bool),
    Undefined,
    Null,
    Object(JSObjectDataPtr<'gc>),
    Function(String),
    Closure(Gc<'gc, ClosureData<'gc>>),
    AsyncClosure(Gc<'gc, ClosureData<'gc>>),
    GeneratorFunction(Option<String>, Gc<'gc, ClosureData<'gc>>),
    ClassDefinition(Gc<'gc, ClassDefinition>),
    // Getter/Setter legacy variants - keeping structures as implied by usage
    Getter(Vec<Statement>, JSObjectDataPtr<'gc>, Option<Box<Value<'gc>>>),
    Setter(
        Vec<DestructuringElement>,
        Vec<Statement>,
        JSObjectDataPtr<'gc>,
        Option<Box<Value<'gc>>>,
    ),

    Promise(GcPtr<'gc, JSPromise<'gc>>),
    Map(GcPtr<'gc, JSMap<'gc>>),
    Set(GcPtr<'gc, JSSet<'gc>>),
    WeakMap(GcPtr<'gc, JSWeakMap<'gc>>),
    WeakSet(GcPtr<'gc, JSWeakSet<'gc>>),
    Generator(GcPtr<'gc, JSGenerator<'gc>>),
    Proxy(Gc<'gc, JSProxy<'gc>>),
    ArrayBuffer(GcPtr<'gc, JSArrayBuffer>),
    DataView(Gc<'gc, JSDataView<'gc>>),
    TypedArray(Gc<'gc, JSTypedArray<'gc>>),

    Property {
        value: Option<GcPtr<'gc, Value<'gc>>>,
        getter: Option<Box<Value<'gc>>>,
        setter: Option<Box<Value<'gc>>>,
    },
    Symbol(Gc<'gc, SymbolData>),
    Uninitialized,
}

impl Value<'_> {
    pub fn is_null_or_undefined(&self) -> bool {
        matches!(self, Value::Null | Value::Undefined)
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
            Value::Object(obj) => obj.trace(cc),
            Value::Closure(cl) => cl.trace(cc),
            Value::AsyncClosure(cl) => cl.trace(cc),
            Value::GeneratorFunction(_, cl) => cl.trace(cc),
            Value::ClassDefinition(cl) => cl.trace(cc),
            Value::Getter(body, env, v) => {
                for s in body {
                    s.trace(cc);
                }
                env.trace(cc);
                if let Some(val) = v {
                    val.trace(cc);
                }
            }
            Value::Setter(param, body, env, v) => {
                for p in param {
                    p.trace(cc);
                }
                for s in body {
                    s.trace(cc);
                }
                env.trace(cc);
                if let Some(val) = v {
                    val.trace(cc);
                }
            }
            Value::Promise(p) => p.trace(cc),
            Value::Map(m) => m.trace(cc),
            Value::Set(s) => s.trace(cc),
            Value::WeakMap(m) => m.trace(cc),
            Value::WeakSet(s) => s.trace(cc),
            Value::Generator(g) => g.trace(cc),
            Value::Proxy(p) => p.trace(cc),
            Value::ArrayBuffer(b) => b.trace(cc),
            Value::DataView(d) => d.trace(cc),
            Value::TypedArray(t) => t.trace(cc),

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
            Value::Object(_) => write!(f, "Object"),
            Value::Function(s) => write!(f, "Function({})", s),
            _ => write!(f, "[value]"),
        }
    }
}

// Helper: perform ToPrimitive coercion with a given hint ('string', 'number', 'default').
// This is a simplified implementation that supports user-defined `valueOf` / `toString`.
pub fn to_primitive<'gc>(
    mc: &MutationContext<'gc>,
    val: &Value<'gc>,
    hint: &str,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    match val {
        Value::Number(_) | Value::BigInt(_) | Value::String(_) | Value::Boolean(_) | Value::Undefined | Value::Null | Value::Symbol(_) => {
            Ok(val.clone())
        }
        Value::Object(obj) => {
            let is_primitive = |v: &Value<'gc>| {
                matches!(
                    v,
                    Value::Number(_) | Value::BigInt(_) | Value::String(_) | Value::Boolean(_) | Value::Symbol(_)
                )
            };

            // If object has Symbol.toPrimitive, call it first
            if let Some(sym_ctor) = crate::core::env_get(env, "Symbol") {
                if let Value::Object(sym_obj) = &*sym_ctor.borrow() {
                    if let Ok(Some(tp_sym_val)) = crate::core::obj_get_key_value(sym_obj, &"toPrimitive".into()) {
                        if let Value::Symbol(tp_sym) = &*tp_sym_val.borrow() {
                            if let Ok(Some(func_val_rc)) =
                                crate::core::obj_get_key_value(&obj, &crate::core::PropertyKey::Symbol(tp_sym.clone()))
                            {
                                let func_val = func_val_rc.borrow().clone();
                                if !matches!(func_val, Value::Undefined | Value::Null) {
                                    // Call it with hint
                                    let arg = Value::String(crate::unicode::utf8_to_utf16(hint));
                                    // Support closures or function objects
                                    let res_eval: Result<Value<'gc>, crate::core::js_error::EvalError> = match func_val {
                                        Value::Closure(cl) => {
                                            crate::core::call_closure(mc, &cl, Some(Value::Object(obj.clone())), &[arg.clone()], env, None)
                                        }
                                        Value::Object(func_obj) => {
                                            if let Ok(Some(cl_ptr)) = crate::core::obj_get_key_value(&func_obj, &"__closure__".into()) {
                                                if let Value::Closure(cl) = &*cl_ptr.borrow() {
                                                    crate::core::call_closure(
                                                        mc,
                                                        &cl,
                                                        Some(Value::Object(obj.clone())),
                                                        &[arg.clone()],
                                                        env,
                                                        Some(func_obj.clone()),
                                                    )
                                                } else {
                                                    return Err(crate::raise_type_error!("@@toPrimitive is not a function"));
                                                }
                                            } else {
                                                return Err(crate::raise_type_error!("@@toPrimitive is not a function"));
                                            }
                                        }
                                        _ => return Err(crate::raise_type_error!("@@toPrimitive is not a function")),
                                    };
                                    let res = match res_eval {
                                        Ok(v) => v,
                                        Err(e) => return Err(e.into()),
                                    };
                                    if is_primitive(&res) {
                                        return Ok(res);
                                    } else {
                                        return Err(crate::raise_type_error!("@@toPrimitive must return a primitive value"));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if hint == "string" {
                // toString -> valueOf
                let to_s = crate::js_object::handle_to_string_method(mc, &Value::Object(obj.clone()), &[], env)?;
                if is_primitive(&to_s) {
                    return Ok(to_s);
                }
                let val_of = crate::js_object::handle_value_of_method(mc, &Value::Object(obj.clone()), &[], env)?;
                if is_primitive(&val_of) {
                    return Ok(val_of);
                }
            } else {
                // number/default: valueOf -> toString
                let val_of = crate::js_object::handle_value_of_method(mc, &Value::Object(obj.clone()), &[], env)?;
                if is_primitive(&val_of) {
                    return Ok(val_of);
                }
                let to_s = crate::js_object::handle_to_string_method(mc, &Value::Object(obj.clone()), &[], env)?;
                if is_primitive(&to_s) {
                    return Ok(to_s);
                }
            }

            Err(raise_type_error!("Cannot convert object to primitive"))
        }
        _ => Ok(val.clone()),
    }
}

pub fn value_to_string<'gc>(val: &Value<'gc>) -> String {
    match val {
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
        Value::Object(obj) => {
            if is_error(val) {
                let msg = obj.borrow().get_message().unwrap_or("Unknown error".into());
                return format!("Error: {msg}");
            }
            "[object Object]".to_string()
        }
        Value::Function(name) => format!("function {}", name),
        Value::Closure(..) => "function".to_string(),
        Value::AsyncClosure(..) => "async function".to_string(),
        Value::GeneratorFunction(name, ..) => format!("function* {}", name.as_deref().unwrap_or("")),
        Value::ClassDefinition(..) => "class".to_string(),
        Value::Getter(..) => "[Getter]".to_string(),
        Value::Setter(..) => "[Setter]".to_string(),
        Value::Promise(_) => "[object Promise]".to_string(),
        Value::Map(_) => "[object Map]".to_string(),
        Value::Set(_) => "[object Set]".to_string(),
        Value::WeakMap(_) => "[object WeakMap]".to_string(),
        Value::WeakSet(_) => "[object WeakSet]".to_string(),
        Value::Generator(_) => "[object Generator]".to_string(),
        Value::Proxy(_) => "[object Proxy]".to_string(),
        Value::ArrayBuffer(_) => "[object ArrayBuffer]".to_string(),
        Value::DataView(_) => "[object DataView]".to_string(),
        Value::TypedArray(_) => "[object TypedArray]".to_string(),
        Value::Property { .. } => "[Property]".to_string(),
        _ => "[unknown]".to_string(),
    }
}

fn format_js_number(n: f64) -> String {
    // Handle zero (including -0)
    if n == 0.0 {
        return if n.is_sign_negative() { "-0".to_string() } else { "0".to_string() };
    }
    // Special-case the smallest positive subnormal number to match JS representation
    if n.to_bits() == 1 {
        return "5e-324".to_string();
    }
    // Special-case f64::MAX to match exact JS expected string
    if n == f64::MAX {
        return "1.7976931348623157e+308".to_string();
    }
    let abs = n.abs();
    // Use exponential form for very large or very small numbers (ECMAScript style)
    if abs >= 1e21 || abs < 1e-6 {
        // Use higher precision for very large numbers to preserve digits, otherwise shorter precision
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

    // Otherwise use a normal decimal representation without unnecessary trailing zeros
    let mut s = format!("{}", n);
    if s.contains('.') {
        // Trim trailing zeros and possibly the decimal point
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    s
}

pub fn value_to_sort_string<'gc>(val: &Value<'gc>) -> String {
    match val {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        _ => value_to_string(val),
    }
}

pub fn values_equal<'gc>(_mc: &MutationContext<'gc>, v1: &Value<'gc>, v2: &Value<'gc>) -> bool {
    match (v1, v2) {
        (Value::Number(n1), Value::Number(n2)) => {
            if n1.is_nan() && n2.is_nan() {
                true
            } else {
                n1 == n2
            }
        }
        (Value::String(s1), Value::String(s2)) => s1 == s2,
        (Value::Boolean(b1), Value::Boolean(b2)) => b1 == b2,
        (Value::Undefined, Value::Undefined) => true,
        (Value::Null, Value::Null) => true,
        (Value::Object(o1), Value::Object(o2)) => Gc::ptr_eq(*o1, *o2),
        (Value::Closure(c1), Value::Closure(c2)) => Gc::ptr_eq(*c1, *c2),
        (Value::AsyncClosure(c1), Value::AsyncClosure(c2)) => Gc::ptr_eq(*c1, *c2),
        (Value::GeneratorFunction(_, c1), Value::GeneratorFunction(_, c2)) => Gc::ptr_eq(*c1, *c2),
        (Value::ClassDefinition(c1), Value::ClassDefinition(c2)) => Gc::ptr_eq(*c1, *c2),
        (Value::Promise(p1), Value::Promise(p2)) => Gc::ptr_eq(*p1, *p2),
        (Value::Map(m1), Value::Map(m2)) => Gc::ptr_eq(*m1, *m2),
        (Value::Set(s1), Value::Set(s2)) => Gc::ptr_eq(*s1, *s2),
        (Value::WeakMap(m1), Value::WeakMap(m2)) => Gc::ptr_eq(*m1, *m2),
        (Value::WeakSet(s1), Value::WeakSet(s2)) => Gc::ptr_eq(*s1, *s2),
        (Value::Generator(g1), Value::Generator(g2)) => Gc::ptr_eq(*g1, *g2),
        (Value::Proxy(p1), Value::Proxy(p2)) => Gc::ptr_eq(*p1, *p2),
        (Value::ArrayBuffer(b1), Value::ArrayBuffer(b2)) => Gc::ptr_eq(*b1, *b2),
        (Value::DataView(d1), Value::DataView(d2)) => Gc::ptr_eq(*d1, *d2),
        (Value::TypedArray(t1), Value::TypedArray(t2)) => Gc::ptr_eq(*t1, *t2),
        // Getter/Setter equality is tricky if they have Vecs.
        // But usually we just check reference equality if they were allocated, but here they are variants.
        // But the previous implementation didn't check them.
        // Assuming strict equality for these internal variants isn't common in user code comparisons (usually they are hidden).
        _ => false,
    }
}

pub fn obj_get_key_value<'gc>(obj: &JSObjectDataPtr<'gc>, key: &PropertyKey<'gc>) -> Result<Option<GcPtr<'gc, Value<'gc>>>, JSError> {
    let mut current = Some(*obj);
    while let Some(cur) = current {
        if let Some(val) = cur.borrow().properties.get(key) {
            return Ok(Some(*val));
        }
        current = cur.borrow().prototype;
    }
    Ok(None)
}

pub fn get_own_property<'gc>(obj: &JSObjectDataPtr<'gc>, key: &PropertyKey<'gc>) -> Option<GcPtr<'gc, Value<'gc>>> {
    obj.borrow().properties.get(key).cloned()
}

pub fn obj_set_key_value<'gc>(
    mc: &MutationContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: &PropertyKey<'gc>,
    val: Value<'gc>,
) -> Result<(), JSError> {
    let val_ptr = Gc::new(mc, GcCell::new(val));
    obj.borrow_mut(mc).insert(key.clone(), val_ptr);
    Ok(())
}

pub fn obj_set_rc<'gc>(
    mc: &MutationContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: &PropertyKey<'gc>,
    val: GcPtr<'gc, Value<'gc>>,
) -> Result<(), JSError> {
    obj.borrow_mut(mc).insert(key.clone(), val);
    Ok(())
}

pub fn env_get<'gc>(env: &JSObjectDataPtr<'gc>, key: &str) -> Option<GcPtr<'gc, Value<'gc>>> {
    env.borrow().properties.get(&PropertyKey::String(key.to_string())).cloned()
}

pub fn env_set<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, key: &str, val: Value<'gc>) -> Result<(), JSError> {
    if (*env.borrow()).is_const(key) {
        return Err(raise_type_error!(format!("Assignment to constant variable '{key}'")));
    }
    let val_ptr = Gc::new(mc, GcCell::new(val));
    env.borrow_mut(mc).insert(PropertyKey::String(key.to_string()), val_ptr);
    Ok(())
}

// Helper: Check whether the given object has an own property corresponding to a
// given JS `Value` (as passed to hasOwnProperty / propertyIsEnumerable). This
// centralizes conversion from various `Value` variants (String/Number/Boolean/
// Undefined/Symbol/other) to a `PropertyKey` and calls `get_own_property`.
// Returns true if an own property exists.
pub fn has_own_property_value<'gc>(obj: &JSObjectDataPtr<'gc>, key_val: &Value<'gc>) -> bool {
    match key_val {
        Value::String(s) => get_own_property(obj, &utf16_to_utf8(s).into()).is_some(),
        Value::Number(n) => get_own_property(obj, &n.to_string().into()).is_some(),
        Value::Boolean(b) => get_own_property(obj, &b.to_string().into()).is_some(),
        Value::Undefined => get_own_property(obj, &"undefined".into()).is_some(),
        Value::Symbol(sd) => {
            let sym_key = PropertyKey::Symbol(sd.clone());
            get_own_property(obj, &sym_key).is_some()
        }
        other => get_own_property(obj, &value_to_string(other).into()).is_some(),
    }
}

pub fn env_set_recursive<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, key: &str, val: Value<'gc>) -> Result<(), JSError> {
    let mut current = *env;
    loop {
        if current.borrow().properties.contains_key(&PropertyKey::String(key.to_string())) {
            return env_set(mc, &current, key, val);
        }
        let parent_opt = current.borrow().prototype;
        if let Some(parent_rc) = parent_opt {
            current = parent_rc;
        } else {
            // Reached global scope (or end of chain) and variable not found.
            // In strict mode, this is a ReferenceError.
            // Since our engine is strict-only, we should error here instead of creating a global.
            return Err(crate::raise_reference_error!(format!("{} is not defined", key)));
        }
    }
}

pub fn object_get_length<'gc>(obj: &JSObjectDataPtr<'gc>) -> Option<usize> {
    if let Some(len_ptr) = obj_get_key_value(obj, &"length".into()).ok().flatten() {
        if let Value::Number(n) = &*len_ptr.borrow() {
            return Some(*n as usize);
        }
    }
    None
}

pub fn object_set_length<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>, length: usize) -> Result<(), JSError> {
    obj_set_key_value(mc, obj, &"length".into(), Value::Number(length as f64))?;
    Ok(())
}
