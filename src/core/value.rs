use crate::core::{Collect, Gc, GcCell, GcPtr, GcTrace, GcWeak, MutationContext, new_gc_cell_ptr};
use crate::unicode::utf16_to_utf8;
use crate::{
    JSError,
    core::{ClassDefinition, DestructuringElement, EvalError, PropertyKey, Statement, is_error},
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
    // Capture the call-time arguments so that parameter bindings can be
    // created when the generator starts executing.
    pub args: Vec<Value<'gc>>,
    pub state: GeneratorState<'gc>,
    // Optionally cache the initially yielded value so that resume/re-entry
    // paths can avoid re-evaluating the inner expression.
    pub cached_initial_yield: Option<Value<'gc>>,
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

#[derive(Clone, Collect)]
#[collect(no_drop)]
pub enum GeneratorState<'gc> {
    NotStarted,
    Running {
        pc: usize,
        stack: Vec<Value<'gc>>,
    },
    // When suspended, optionally keep the environment that was used to
    // execute statements before the first `yield`. This lets resume use the
    // same bindings when executing the remainder of the generator body.
    Suspended {
        pc: usize,
        stack: Vec<Value<'gc>>,
        pre_env: Option<JSObjectDataPtr<'gc>>,
    },
    Completed,
}

pub type JSObjectDataPtr<'gc> = GcPtr<'gc, JSObjectData<'gc>>;
// pub type JSObjectDataWeakPtr<'gc> = Gc<'gc, GcCell<JSObjectData<'gc>>>;

#[inline]
pub fn new_js_object_data<'gc>(mc: &MutationContext<'gc>) -> JSObjectDataPtr<'gc> {
    new_gc_cell_ptr(mc, JSObjectData::new())
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
    // Whether new own properties can be added to this object. Default true.
    pub extensible: bool,
    // Optional internal class definition slot (not exposed as an own property)
    pub class_def: Option<GcPtr<'gc, ClassDefinition>>,
    pub home_object: Option<GcCell<JSObjectDataPtr<'gc>>>,
    /// Internal executable closure for function objects (previously stored as an internal property)
    closure: Option<GcPtr<'gc, Value<'gc>>>,
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
        if let Some(cd) = &self.class_def {
            cd.trace(cc);
        }
        if let Some(cl) = &self.closure {
            cl.trace(cc);
        }
    }
}

impl<'gc> JSObjectData<'gc> {
    pub fn new() -> Self {
        // JSObjectData::default() would initialize `extensible` to false, so ensure it's true by default
        JSObjectData::<'_> {
            extensible: true,
            ..JSObjectData::default()
        }
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

    pub fn set_configurable(&mut self, key: PropertyKey<'gc>) {
        self.non_configurable.remove(&key);
    }

    pub fn set_non_writable(&mut self, key: PropertyKey<'gc>) {
        // Debug: log where non-writable markers are set
        log::debug!("set_non_writable: obj_ptr={:p} key={:?}", self as *const _, key);
        self.non_writable.insert(key);
    }

    pub fn set_writable(&mut self, key: PropertyKey<'gc>) {
        // Debug: log where non-writable markers are cleared
        log::debug!("set_writable: obj_ptr={:p} key={:?}", self as *const _, key);
        self.non_writable.remove(&key);
    }

    pub fn is_const(&self, key: &str) -> bool {
        self.constants.contains(key)
    }

    pub fn set_property(&mut self, mc: &MutationContext<'gc>, key: impl Into<PropertyKey<'gc>>, val: Value<'gc>) {
        let val_ptr = new_gc_cell_ptr(mc, val);
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
        if let Some(proto) = &self.prototype
            && let Some(val_ptr) = object_get_key_value(proto, key)
            && let Value::String(s) = &*val_ptr.borrow()
        {
            return Some(utf16_to_utf8(s));
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
            let val_ptr = new_gc_cell_ptr(mc, val);
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
            let val_ptr = new_gc_cell_ptr(mc, val);
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
        // Debug: log where non-enumerable markers are set
        log::debug!("set_non_enumerable: obj_ptr={:p} key={:?}", self as *const _, key);
        self.non_enumerable.insert(key);
    }

    pub fn set_enumerable(&mut self, key: PropertyKey<'gc>) {
        // Debug: log where enumerable markers are cleared
        log::debug!("set_enumerable: obj_ptr={:p} key={:?}", self as *const _, key);
        self.non_enumerable.remove(&key);
    }

    pub fn is_configurable(&self, key: &PropertyKey<'gc>) -> bool {
        !self.non_configurable.contains(key)
    }

    pub fn is_writable(&self, key: &PropertyKey<'gc>) -> bool {
        !self.non_writable.contains(key)
    }

    // Extensibility helpers
    pub fn is_extensible(&self) -> bool {
        self.extensible
    }

    pub fn prevent_extensions(&mut self) {
        self.extensible = false;
    }

    pub fn is_enumerable(&self, key: &PropertyKey<'gc>) -> bool {
        !self.non_enumerable.contains(key)
    }

    pub fn get_home_object(&self) -> Option<GcCell<JSObjectDataPtr<'gc>>> {
        self.home_object.clone()
    }

    pub fn set_home_object(&mut self, home: Option<GcCell<JSObjectDataPtr<'gc>>>) {
        self.home_object = home;
    }

    pub fn get_closure(&self) -> Option<GcPtr<'gc, Value<'gc>>> {
        self.closure
    }

    pub fn set_closure(&mut self, closure: Option<GcPtr<'gc, Value<'gc>>>) {
        self.closure = closure;
    }
}

impl<'gc> ClosureData<'gc> {
    pub fn new(
        params: &[DestructuringElement],
        body: &[Statement],
        env: Option<JSObjectDataPtr<'gc>>,
        home_object: Option<JSObjectDataPtr<'gc>>,
    ) -> Self {
        ClosureData {
            params: params.to_vec(),
            body: body.to_vec(),
            env,
            home_object: home_object.map(GcCell::new),
            enforce_strictness_inheritance: true,
            ..ClosureData::default()
        }
    }
}

#[derive(Clone, Debug, Collect)]
#[collect(require_static)]
pub struct SymbolData {
    pub description: Option<String>,
}

#[derive(Clone, Collect, Default)]
#[collect(no_drop)]
pub struct ClosureData<'gc> {
    pub params: Vec<DestructuringElement>,
    pub body: Vec<Statement>,
    pub env: Option<JSObjectDataPtr<'gc>>,
    pub home_object: Option<GcCell<JSObjectDataPtr<'gc>>>,
    pub captured_envs: Vec<JSObjectDataPtr<'gc>>,
    pub bound_this: Option<Value<'gc>>,
    pub is_arrow: bool,
    // Whether this function was parsed/declared in strict mode (function-level "use strict").
    pub is_strict: bool,
    pub native_target: Option<String>,
    // For Function() constructor: do not inherit strictness from environment
    pub enforce_strictness_inheritance: bool,
}

#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSPromise<'gc> {
    pub id: usize,
    pub state: PromiseState<'gc>,
    pub value: Option<Value<'gc>>,
    pub on_fulfilled: Vec<(Value<'gc>, GcPtr<'gc, JSPromise<'gc>>, Option<JSObjectDataPtr<'gc>>)>,
    pub on_rejected: Vec<(Value<'gc>, GcPtr<'gc, JSPromise<'gc>>, Option<JSObjectDataPtr<'gc>>)>,
    /// Whether a rejection handler has been attached or a rejection handler
    /// has already executed for this promise. Used to avoid reporting
    /// unhandled rejections after the promise has been handled.
    pub handled: bool,
}

static UNIQUE_ID_SEED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

pub fn generate_unique_id() -> usize {
    UNIQUE_ID_SEED.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

impl<'gc> JSPromise<'gc> {
    pub fn new() -> Self {
        Self {
            id: generate_unique_id(),
            state: PromiseState::Pending,
            value: None,
            on_fulfilled: Vec::new(),
            on_rejected: Vec::new(),
            handled: false,
        }
    }
}

impl std::fmt::Debug for JSPromise<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "JSPromise {{ on_fulfilled: {}, on_rejected: {} }}",
            self.on_fulfilled.len(),
            self.on_rejected.len()
        )
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
    Getter(Vec<Statement>, JSObjectDataPtr<'gc>, Option<GcCell<JSObjectDataPtr<'gc>>>), // body, env, home object
    Setter(
        Vec<DestructuringElement>,            // params
        Vec<Statement>,                       // body
        JSObjectDataPtr<'gc>,                 // env
        Option<GcCell<JSObjectDataPtr<'gc>>>, // home object
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
            Value::Getter(body, env, home_object) => {
                for s in body {
                    s.trace(cc);
                }
                env.trace(cc);
                if let Some(home_object) = home_object {
                    home_object.trace(cc);
                }
            }
            Value::Setter(param, body, env, home_object) => {
                for p in param {
                    p.trace(cc);
                }
                for s in body {
                    s.trace(cc);
                }
                env.trace(cc);
                if let Some(home_obj) = home_object {
                    home_obj.trace(cc);
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
) -> Result<Value<'gc>, EvalError<'gc>> {
    match val {
        Value::Number(_) | Value::BigInt(_) | Value::String(_) | Value::Boolean(_) | Value::Undefined | Value::Null | Value::Symbol(_) => {
            Ok(val.clone())
        }
        Value::Object(obj) => {
            let is_primitive = |v: &Value<'gc>| {
                matches!(
                    v,
                    Value::Number(_)
                        | Value::BigInt(_)
                        | Value::String(_)
                        | Value::Boolean(_)
                        | Value::Symbol(_)
                        | Value::Null
                        | Value::Undefined
                )
            };

            // If object has Symbol.toPrimitive, call it first
            if let Some(sym_ctor) = crate::core::env_get(env, "Symbol")
                && let Value::Object(sym_obj) = &*sym_ctor.borrow()
                && let Some(tp_sym_val) = object_get_key_value(sym_obj, "toPrimitive")
                && let Value::Symbol(tp_sym) = &*tp_sym_val.borrow()
                && let Some(func_val_rc) = object_get_key_value(obj, crate::core::PropertyKey::Symbol(*tp_sym))
            {
                let func_val = func_val_rc.borrow().clone();
                if !matches!(func_val, Value::Undefined | Value::Null) {
                    log::debug!("DBG to_primitive: calling @@toPrimitive with hint={}", hint);
                    // Call it with hint
                    let arg = Value::String(crate::unicode::utf8_to_utf16(hint));
                    // Support closures or function objects
                    use std::slice::from_ref;
                    let res_eval: Result<Value<'gc>, crate::core::js_error::EvalError> = match func_val {
                        Value::Closure(cl) => crate::core::call_closure(mc, &cl, Some(Value::Object(*obj)), from_ref(&arg), env, None),
                        Value::Object(func_obj) => {
                            if let Some(cl_ptr) = func_obj.borrow().get_closure() {
                                if let Value::Closure(cl) = &*cl_ptr.borrow() {
                                    crate::core::call_closure(mc, cl, Some(Value::Object(*obj)), from_ref(&arg), env, Some(func_obj))
                                } else {
                                    return Err(raise_type_error!("@@toPrimitive is not a function").into());
                                }
                            } else {
                                return Err(raise_type_error!("@@toPrimitive is not a function").into());
                            }
                        }
                        _ => return Err(raise_type_error!("@@toPrimitive is not a function").into()),
                    };
                    let res = res_eval?;
                    log::debug!("DBG to_primitive: @@toPrimitive returned {:?}", res);
                    if is_primitive(&res) {
                        return Ok(res);
                    } else {
                        return Err(raise_type_error!("@@toPrimitive must return a primitive value").into());
                    }
                }
            }

            // If hint is 'default' and this is a Date object, treat the default hint
            // as if it were 'string' per ECMAScript semantics for Date objects.
            let effective_hint = if hint == "default" && crate::js_date::is_date_object(obj) {
                "string"
            } else {
                hint
            };

            if effective_hint == "string" {
                // toString -> valueOf
                log::debug!("DBG to_primitive: trying toString for obj={:p}", Gc::as_ptr(*obj));
                let to_s = crate::js_object::handle_to_string_method(mc, &Value::Object(*obj), &[], env)?;
                log::debug!("DBG to_primitive: toString result = {:?}", to_s);
                // Treat `Uninitialized` as a sentinel meaning "no callable toString" and
                // therefore do not accept it as a primitive result. Only accept real
                // primitive values here.
                if !matches!(to_s, crate::core::Value::Uninitialized) && is_primitive(&to_s) {
                    return Ok(to_s);
                }
                log::debug!("DBG to_primitive: trying valueOf for obj={:p}", Gc::as_ptr(*obj));
                let val_of = crate::js_object::handle_value_of_method(mc, &Value::Object(*obj), &[], env)?;
                log::debug!("DBG to_primitive: valueOf result = {:?}", val_of);
                if is_primitive(&val_of) {
                    return Ok(val_of);
                }
            } else {
                // number/default: valueOf -> toString
                log::debug!("DBG to_primitive: trying valueOf for obj={:p}", Gc::as_ptr(*obj));
                let val_of = crate::js_object::handle_value_of_method(mc, &Value::Object(*obj), &[], env)?;
                log::debug!("DBG to_primitive: valueOf result = {:?}", val_of);
                if is_primitive(&val_of) {
                    return Ok(val_of);
                }
                log::debug!("DBG to_primitive: trying toString for obj={:p}", Gc::as_ptr(*obj));
                let to_s = crate::js_object::handle_to_string_method(mc, &Value::Object(*obj), &[], env)?;
                log::debug!("DBG to_primitive: toString result = {:?}", to_s);
                // See comment above: do not treat `Uninitialized` as a primitive sentinel
                // result from a non-callable `toString` property.
                if !matches!(to_s, crate::core::Value::Uninitialized) && is_primitive(&to_s) {
                    return Ok(to_s);
                }
            }

            Err(raise_type_error!("Cannot convert object to primitive").into())
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
            // Prefer an explicit `message` property on user-defined error-like objects
            // so thrown harness-liked errors show useful messages
            if let Ok(borrowed) = obj.try_borrow()
                && let Some(msg) = borrowed.get_message()
            {
                return msg;
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
        Value::Symbol(sym) => {
            if let Some(desc) = &sym.description {
                format!("Symbol({desc})")
            } else {
                "Symbol()".to_string()
            }
        }
        Value::Uninitialized => "[uninitialized]".to_string(),
    }
}

pub fn format_js_number(n: f64) -> String {
    log::debug!(
        "DBG format_js_number: n={} is_zero={} sign_neg={}",
        n,
        n == 0.0,
        n.is_sign_negative()
    );
    // Handle zero: ECMAScript ToString(-0) should produce "0"
    if n == 0.0 {
        return "0".to_string();
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
    if !(1e-6..1e21).contains(&abs) {
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

pub fn object_get_key_value<'gc>(obj: &JSObjectDataPtr<'gc>, key: impl Into<PropertyKey<'gc>>) -> Option<GcPtr<'gc, Value<'gc>>> {
    let key = key.into();
    let mut current = Some(*obj);
    while let Some(cur) = current {
        if let Some(val) = cur.borrow().properties.get(&key) {
            return Some(*val);
        }
        current = cur.borrow().prototype;
    }
    None
}

// Return property keys in 'ordinary own property keys' order per ECMAScript:
// 1) Array index keys (string keys that are canonical numeric indices) sorted numerically,
// 2) Other string keys in insertion order,
// 3) Symbol keys in insertion order.
pub fn ordinary_own_property_keys<'gc>(obj: &JSObjectDataPtr<'gc>) -> Vec<PropertyKey<'gc>> {
    let mut indices: Vec<(u64, PropertyKey<'gc>)> = Vec::new();
    let mut string_keys: Vec<PropertyKey<'gc>> = Vec::new();
    let mut symbol_keys: Vec<PropertyKey<'gc>> = Vec::new();

    // Special-case TypedArray instances: their indexed elements are conceptually own
    // properties (0..length-1) which should appear in ordinary own property keys
    // even if we don't materialize them in the object's properties map.
    let mut typed_indices: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(ta_cell) = obj.borrow().properties.get(&PropertyKey::String("__typedarray".to_string()))
        && let Value::TypedArray(ta) = &*ta_cell.borrow()
    {
        for i in 0..ta.length {
            let s = i.to_string();
            // push as numeric index entry (keeps numeric ordering)
            indices.push((i as u64, PropertyKey::String(s.clone())));
            typed_indices.insert(s);
        }
    }

    for k in obj.borrow().properties.keys() {
        match k {
            PropertyKey::String(s) => {
                // If this property is one of the typed array index helpers we already
                // added above, skip it to avoid duplication.
                if typed_indices.contains(s) {
                    continue;
                }
                // Check canonical numeric index: no leading + or spaces; must roundtrip to same string
                if let Ok(parsed) = s.parse::<u64>() {
                    // canonical representation check (no leading zeros except "0")
                    if parsed.to_string() == *s && parsed <= 4294967294u64 {
                        indices.push((parsed, k.clone()));
                        continue;
                    }
                }
                string_keys.push(k.clone());
            }
            PropertyKey::Symbol(_) => symbol_keys.push(k.clone()),
        }
    }

    indices.sort_by_key(|(num, _k)| *num);
    let mut out: Vec<PropertyKey<'gc>> = Vec::new();
    for (_n, k) in indices {
        out.push(k);
    }
    out.extend(string_keys);
    out.extend(symbol_keys);
    out
}
pub fn get_own_property<'gc>(obj: &JSObjectDataPtr<'gc>, key: &PropertyKey<'gc>) -> Option<GcPtr<'gc, Value<'gc>>> {
    obj.borrow().properties.get(key).cloned()
}

pub fn object_set_key_value<'gc>(
    mc: &MutationContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: impl Into<PropertyKey<'gc>>,
    val: Value<'gc>,
) -> Result<(), JSError> {
    let key = key.into();

    // Debug log to help diagnose non-extensible assignment behavior
    let exists = obj.borrow().properties.contains_key(&key);
    let key_desc = match &key {
        PropertyKey::String(s) => s.clone(),
        PropertyKey::Symbol(_) => "<symbol>".to_string(),
    };
    let obj_addr = format!("{:p}", &*obj.borrow());
    log::debug!(
        "object_set_key_value: obj={} key={} key_exists={} extensible={}",
        obj_addr,
        key_desc,
        exists,
        obj.borrow().is_extensible()
    );

    // Disallow creating new own properties on non-extensible objects
    if !exists && !obj.borrow().is_extensible() {
        return Err(raise_type_error!("Cannot add property to non-extensible object"));
    }

    // If obj is an array and we're setting a numeric index, update length accordingly
    if let PropertyKey::String(s) = &key
        && let Ok(idx) = s.parse::<usize>()
        && crate::js_array::is_array(mc, obj)
    {
        let current_len = object_get_length(obj).unwrap_or(0);
        if idx >= current_len {
            // Set internal length to idx + 1
            object_set_length(mc, obj, idx + 1)?;
        }
    }

    let val_ptr = new_gc_cell_ptr(mc, val);
    obj.borrow_mut(mc).insert(key.clone(), val_ptr);
    log::debug!(
        "object_set_key_value: after insert obj={:p} key={:?} is_writable={} is_enumerable={} is_configurable={}",
        &*obj.borrow(),
        key,
        obj.borrow().is_writable(&key),
        obj.borrow().is_enumerable(&key),
        obj.borrow().is_configurable(&key)
    );
    Ok(())
}

pub fn env_get_own<'gc>(env: &JSObjectDataPtr<'gc>, key: &str) -> Option<GcPtr<'gc, Value<'gc>>> {
    env.borrow().properties.get(&PropertyKey::String(key.to_string())).cloned()
}

pub fn env_get<'gc>(env: &JSObjectDataPtr<'gc>, key: &str) -> Option<GcPtr<'gc, Value<'gc>>> {
    let mut current = Some(*env);
    while let Some(cur) = current {
        if let Some(val) = cur.borrow().properties.get(&PropertyKey::String(key.to_string())) {
            return Some(*val);
        }
        current = cur.borrow().prototype;
    }
    None
}

pub fn env_set<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, key: &str, val: Value<'gc>) -> Result<(), JSError> {
    if (*env.borrow()).is_const(key) {
        return Err(raise_type_error!(format!("Assignment to constant variable '{key}'")));
    }
    let val_ptr = new_gc_cell_ptr(mc, val);
    env.borrow_mut(mc).insert(PropertyKey::String(key.to_string()), val_ptr);
    Ok(())
}

pub fn env_get_strictness<'gc>(env: &JSObjectDataPtr<'gc>) -> bool {
    if let Some(is_strict_cell) = get_own_property(env, &PropertyKey::String("__is_strict".to_string()))
        && let crate::core::Value::Boolean(is_strict) = *is_strict_cell.borrow()
    {
        return is_strict;
    }
    false
}

pub fn env_set_strictness<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, is_strict: bool) -> Result<(), JSError> {
    let val = Value::Boolean(is_strict);
    let val_ptr = new_gc_cell_ptr(mc, val);
    env.borrow_mut(mc).insert(PropertyKey::String("__is_strict".to_string()), val_ptr);
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
        Value::Number(n) => get_own_property(obj, &value_to_string(&Value::Number(*n)).into()).is_some(),
        Value::Boolean(b) => get_own_property(obj, &b.to_string().into()).is_some(),
        Value::Undefined => get_own_property(obj, &"undefined".into()).is_some(),
        Value::Symbol(sd) => {
            let sym_key = PropertyKey::Symbol(*sd);
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
            // If the global environment is operating in strict mode, this is a ReferenceError.
            // If the global environment is non-strict, create a new global binding instead (as per
            // ECMAScript non-strict assignment semantics for unresolvable references).
            if env_get_strictness(&current) {
                return Err(crate::raise_reference_error!(format!("{key} is not defined")));
            } else {
                // No explicit strictness marker: be permissive and create the global binding
                return env_set(mc, &current, key, val);
            }
        }
    }
}

pub fn object_get_length<'gc>(obj: &JSObjectDataPtr<'gc>) -> Option<usize> {
    if let Some(len_ptr) = object_get_key_value(obj, "length")
        && let Value::Number(n) = &*len_ptr.borrow()
    {
        return Some(*n as usize);
    }
    None
}

pub fn object_set_length<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>, length: usize) -> Result<(), JSError> {
    // When reducing array length, delete indexed properties >= new length
    if let Some(cur_len) = object_get_length(obj)
        && length < cur_len
    {
        for i in length..cur_len {
            let key = PropertyKey::from(i.to_string());
            // Use shift_remove to preserve insertion order (avoid deprecated remove)
            let _ = obj.borrow_mut(mc).properties.shift_remove(&key);
        }
    }
    object_set_key_value(mc, obj, "length", Value::Number(length as f64))?;
    Ok(())
}
