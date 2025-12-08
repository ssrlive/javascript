use num_bigint::BigInt;
use std::{cell::RefCell, rc::Rc};

use crate::{
    JSError,
    core::{BinaryOp, Expr, PropertyKey, Statement, evaluate_statements, get_well_known_symbol_rc, utf8_to_utf16},
    js_array::is_array,
    js_class::ClassDefinition,
    js_promise::JSPromise,
    raise_eval_error, raise_type_error,
};

#[derive(Clone, Debug)]
pub struct JSMap {
    pub entries: Vec<(Value, Value)>, // key-value pairs
}

#[derive(Clone, Debug)]
pub struct JSSet {
    pub values: Vec<Value>,
}

#[derive(Clone, Debug)]
pub struct JSWeakMap {
    pub entries: Vec<(std::rc::Weak<RefCell<JSObjectData>>, Value)>, // weak key-value pairs
}

#[derive(Clone, Debug)]
pub struct JSWeakSet {
    pub values: Vec<std::rc::Weak<RefCell<JSObjectData>>>, // weak values
}

#[derive(Clone, Debug)]
pub struct JSGenerator {
    pub params: Vec<String>,
    pub body: Vec<Statement>,
    pub env: JSObjectDataPtr, // captured environment
    pub state: GeneratorState,
}

#[derive(Clone, Debug)]
pub struct JSProxy {
    pub target: Value,  // The target object being proxied
    pub handler: Value, // The handler object with traps
    pub revoked: bool,  // Whether this proxy has been revoked
}

#[derive(Clone, Debug)]
pub struct JSArrayBuffer {
    pub data: Vec<u8>,  // The underlying byte buffer
    pub detached: bool, // Whether the buffer has been detached
}

#[derive(Clone, Debug)]
pub struct JSDataView {
    pub buffer: Rc<RefCell<JSArrayBuffer>>, // Reference to the underlying ArrayBuffer
    pub byte_offset: usize,                 // Starting byte offset in the buffer
    pub byte_length: usize,                 // Length in bytes
}

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug)]
pub struct JSTypedArray {
    pub kind: TypedArrayKind,
    pub buffer: Rc<RefCell<JSArrayBuffer>>, // Reference to the underlying ArrayBuffer
    pub byte_offset: usize,                 // Starting byte offset in the buffer
    pub length: usize,                      // Number of elements
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigIntHolder {
    pub raw: String,
    pub parsed: Option<Rc<BigInt>>,
}

impl std::fmt::Display for BigIntHolder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.raw)
    }
}

impl From<BigInt> for BigIntHolder {
    fn from(bigint: BigInt) -> Self {
        BigIntHolder {
            raw: bigint.to_string(),
            parsed: Some(Rc::new(bigint)),
        }
    }
}

impl TryFrom<&str> for BigIntHolder {
    type Error = JSError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(BigIntHolder {
            raw: value.trim().to_string(),
            parsed: Some(Rc::new(Self::parse_bigint_string(value.trim())?)),
        })
    }
}

impl BigIntHolder {
    pub fn refresh_parsed(&mut self, force: bool) -> Result<BigInt, JSError> {
        if !force && let Some(rc) = &self.parsed {
            return Ok((**rc).clone());
        }
        let p = Self::parse_bigint_string(&self.raw)?;
        self.parsed = Some(Rc::new(p.clone()));
        Ok(p)
    }

    fn parse_bigint_string(raw: &str) -> Result<BigInt, JSError> {
        let s = if let Some(st) = raw.strip_suffix('n') { st } else { raw };
        let parsed = BigInt::parse_bytes(s.as_bytes(), 10).ok_or(raise_eval_error!("invalid bigint"))?;
        Ok(parsed)
    }
}

impl JSTypedArray {
    /// Get the size in bytes of each element in this TypedArray
    pub fn element_size(&self) -> usize {
        match self.kind {
            TypedArrayKind::Int8 | TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => 1,
            TypedArrayKind::Int16 | TypedArrayKind::Uint16 => 2,
            TypedArrayKind::Int32 | TypedArrayKind::Uint32 | TypedArrayKind::Float32 => 4,
            TypedArrayKind::Float64 | TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => 8,
        }
    }

    /// Get a value at the specified index
    pub fn get(&self, index: usize) -> Result<i64, JSError> {
        if index >= self.length {
            return Err(raise_type_error!("Index out of bounds"));
        }

        let buffer = self.buffer.borrow();
        if buffer.detached {
            return Err(raise_type_error!("ArrayBuffer is detached"));
        }

        let byte_index = self.byte_offset + index * self.element_size();
        if byte_index + self.element_size() > buffer.data.len() {
            return Err(raise_type_error!("Index out of bounds"));
        }

        match self.kind {
            TypedArrayKind::Int8 => Ok(buffer.data[byte_index] as i8 as i64),
            TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => Ok(buffer.data[byte_index] as i64),
            TypedArrayKind::Int16 => {
                let bytes = &buffer.data[byte_index..byte_index + 2];
                Ok(i16::from_le_bytes([bytes[0], bytes[1]]) as i64)
            }
            TypedArrayKind::Uint16 => {
                let bytes = &buffer.data[byte_index..byte_index + 2];
                Ok(u16::from_le_bytes([bytes[0], bytes[1]]) as i64)
            }
            TypedArrayKind::Int32 => {
                let bytes = &buffer.data[byte_index..byte_index + 4];
                Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as i64)
            }
            TypedArrayKind::Uint32 => {
                let bytes = &buffer.data[byte_index..byte_index + 4];
                Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as i64)
            }
            TypedArrayKind::Float32 => {
                let bytes = &buffer.data[byte_index..byte_index + 4];
                let float_val = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                Ok(float_val as i64) // Simplified conversion
            }
            TypedArrayKind::Float64 => {
                let bytes = &buffer.data[byte_index..byte_index + 8];
                let float_val = f64::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]]);
                Ok(float_val as i64) // Simplified conversion
            }
            TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => {
                // For BigInt types, return 0 for now (simplified)
                Ok(0)
            }
        }
    }

    /// Set a value at the specified index
    pub fn set(&mut self, index: usize, value: i64) -> Result<(), JSError> {
        if index >= self.length {
            return Err(raise_type_error!("Index out of bounds"));
        }

        let mut buffer = self.buffer.borrow_mut();
        if buffer.detached {
            return Err(raise_type_error!("ArrayBuffer is detached"));
        }

        let byte_index = self.byte_offset + index * self.element_size();
        if byte_index + self.element_size() > buffer.data.len() {
            return Err(raise_type_error!("Index out of bounds"));
        }

        match self.kind {
            TypedArrayKind::Int8 => {
                buffer.data[byte_index] = value as i8 as u8;
            }
            TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => {
                buffer.data[byte_index] = value as u8;
            }
            TypedArrayKind::Int16 => {
                let bytes = (value as i16).to_le_bytes();
                buffer.data[byte_index..byte_index + 2].copy_from_slice(&bytes);
            }
            TypedArrayKind::Uint16 => {
                let bytes = (value as u16).to_le_bytes();
                buffer.data[byte_index..byte_index + 2].copy_from_slice(&bytes);
            }
            TypedArrayKind::Int32 => {
                let bytes = (value as i32).to_le_bytes();
                buffer.data[byte_index..byte_index + 4].copy_from_slice(&bytes);
            }
            TypedArrayKind::Uint32 => {
                let bytes = (value as u32).to_le_bytes();
                buffer.data[byte_index..byte_index + 4].copy_from_slice(&bytes);
            }
            TypedArrayKind::Float32 => {
                let bytes = (value as f32).to_le_bytes();
                buffer.data[byte_index..byte_index + 4].copy_from_slice(&bytes);
            }
            TypedArrayKind::Float64 => {
                let bytes = (value as f64).to_le_bytes();
                buffer.data[byte_index..byte_index + 8].copy_from_slice(&bytes);
            }
            TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => {
                // For BigInt types, do nothing for now (simplified)
            }
        }

        Ok(())
    }
}

impl JSDataView {
    /// Get an 8-bit signed integer at the specified byte offset
    pub fn get_int8(&self, offset: usize) -> Result<i8, JSError> {
        self.check_bounds(offset, 1)?;
        let buffer = self.buffer.borrow();
        Ok(buffer.data[self.byte_offset + offset] as i8)
    }

    /// Get an 8-bit unsigned integer at the specified byte offset
    pub fn get_uint8(&self, offset: usize) -> Result<u8, JSError> {
        self.check_bounds(offset, 1)?;
        let buffer = self.buffer.borrow();
        Ok(buffer.data[self.byte_offset + offset])
    }

    /// Get a 16-bit signed integer at the specified byte offset
    pub fn get_int16(&self, offset: usize, little_endian: bool) -> Result<i16, JSError> {
        self.check_bounds(offset, 2)?;
        let buffer = self.buffer.borrow();
        let bytes = &buffer.data[self.byte_offset + offset..self.byte_offset + offset + 2];
        if little_endian {
            Ok(i16::from_le_bytes([bytes[0], bytes[1]]))
        } else {
            Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
        }
    }

    /// Get a 16-bit unsigned integer at the specified byte offset
    pub fn get_uint16(&self, offset: usize, little_endian: bool) -> Result<u16, JSError> {
        self.check_bounds(offset, 2)?;
        let buffer = self.buffer.borrow();
        let bytes = &buffer.data[self.byte_offset + offset..self.byte_offset + offset + 2];
        if little_endian {
            Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
        } else {
            Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
        }
    }

    /// Get a 32-bit signed integer at the specified byte offset
    pub fn get_int32(&self, offset: usize, little_endian: bool) -> Result<i32, JSError> {
        self.check_bounds(offset, 4)?;
        let buffer = self.buffer.borrow();
        let bytes = &buffer.data[self.byte_offset + offset..self.byte_offset + offset + 4];
        if little_endian {
            Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        } else {
            Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
    }

    /// Get a 32-bit unsigned integer at the specified byte offset
    pub fn get_uint32(&self, offset: usize, little_endian: bool) -> Result<u32, JSError> {
        self.check_bounds(offset, 4)?;
        let buffer = self.buffer.borrow();
        let bytes = &buffer.data[self.byte_offset + offset..self.byte_offset + offset + 4];
        if little_endian {
            Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        } else {
            Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
    }

    /// Get a 32-bit float at the specified byte offset
    pub fn get_float32(&self, offset: usize, little_endian: bool) -> Result<f32, JSError> {
        self.check_bounds(offset, 4)?;
        let buffer = self.buffer.borrow();
        let bytes = &buffer.data[self.byte_offset + offset..self.byte_offset + offset + 4];
        if little_endian {
            Ok(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        } else {
            Ok(f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
    }

    /// Get a 64-bit float at the specified byte offset
    pub fn get_float64(&self, offset: usize, little_endian: bool) -> Result<f64, JSError> {
        self.check_bounds(offset, 8)?;
        let buffer = self.buffer.borrow();
        let bytes = &buffer.data[self.byte_offset + offset..self.byte_offset + offset + 8];
        if little_endian {
            Ok(f64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]))
        } else {
            Ok(f64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]))
        }
    }

    /// Get a 64-bit signed BigInt at the specified byte offset
    pub fn get_big_int64(&self, offset: usize, little_endian: bool) -> Result<i64, JSError> {
        self.check_bounds(offset, 8)?;
        let buffer = self.buffer.borrow();
        let bytes = &buffer.data[self.byte_offset + offset..self.byte_offset + offset + 8];
        if little_endian {
            Ok(i64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]))
        } else {
            Ok(i64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]))
        }
    }

    /// Get a 64-bit unsigned BigInt at the specified byte offset
    pub fn get_big_uint64(&self, offset: usize, little_endian: bool) -> Result<u64, JSError> {
        self.check_bounds(offset, 8)?;
        let buffer = self.buffer.borrow();
        let bytes = &buffer.data[self.byte_offset + offset..self.byte_offset + offset + 8];
        if little_endian {
            Ok(u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]))
        } else {
            Ok(u64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]))
        }
    }

    /// Set an 8-bit signed integer at the specified byte offset
    pub fn set_int8(&mut self, offset: usize, value: i8) -> Result<(), JSError> {
        self.check_bounds(offset, 1)?;
        let mut buffer = self.buffer.borrow_mut();
        buffer.data[self.byte_offset + offset] = value as u8;
        Ok(())
    }

    /// Set an 8-bit unsigned integer at the specified byte offset
    pub fn set_uint8(&mut self, offset: usize, value: u8) -> Result<(), JSError> {
        self.check_bounds(offset, 1)?;
        let mut buffer = self.buffer.borrow_mut();
        buffer.data[self.byte_offset + offset] = value;
        Ok(())
    }

    /// Set a 16-bit signed integer at the specified byte offset
    pub fn set_int16(&mut self, offset: usize, value: i16, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 2)?;
        let mut buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        buffer.data[self.byte_offset + offset..self.byte_offset + offset + 2].copy_from_slice(&bytes);
        Ok(())
    }

    /// Set a 16-bit unsigned integer at the specified byte offset
    pub fn set_uint16(&mut self, offset: usize, value: u16, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 2)?;
        let mut buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        buffer.data[self.byte_offset + offset..self.byte_offset + offset + 2].copy_from_slice(&bytes);
        Ok(())
    }

    /// Set a 32-bit signed integer at the specified byte offset
    pub fn set_int32(&mut self, offset: usize, value: i32, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 4)?;
        let mut buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        buffer.data[self.byte_offset + offset..self.byte_offset + offset + 4].copy_from_slice(&bytes);
        Ok(())
    }

    /// Set a 32-bit unsigned integer at the specified byte offset
    pub fn set_uint32(&mut self, offset: usize, value: u32, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 4)?;
        let mut buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        buffer.data[self.byte_offset + offset..self.byte_offset + offset + 4].copy_from_slice(&bytes);
        Ok(())
    }

    /// Set a 32-bit float at the specified byte offset
    pub fn set_float32(&mut self, offset: usize, value: f32, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 4)?;
        let mut buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        buffer.data[self.byte_offset + offset..self.byte_offset + offset + 4].copy_from_slice(&bytes);
        Ok(())
    }

    /// Set a 64-bit float at the specified byte offset
    pub fn set_float64(&mut self, offset: usize, value: f64, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 8)?;
        let mut buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        buffer.data[self.byte_offset + offset..self.byte_offset + offset + 8].copy_from_slice(&bytes);
        Ok(())
    }

    /// Set a 64-bit signed BigInt at the specified byte offset
    pub fn set_big_int64(&mut self, offset: usize, value: i64, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 8)?;
        let mut buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        buffer.data[self.byte_offset + offset..self.byte_offset + offset + 8].copy_from_slice(&bytes);
        Ok(())
    }

    /// Set a 64-bit unsigned BigInt at the specified byte offset
    pub fn set_big_uint64(&mut self, offset: usize, value: u64, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 8)?;
        let mut buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        buffer.data[self.byte_offset + offset..self.byte_offset + offset + 8].copy_from_slice(&bytes);
        Ok(())
    }

    /// Helper method to check bounds and buffer state
    fn check_bounds(&self, offset: usize, size: usize) -> Result<(), JSError> {
        let buffer = self.buffer.borrow();
        if buffer.detached {
            return Err(raise_type_error!("ArrayBuffer is detached"));
        }
        if offset + size > self.byte_length {
            return Err(raise_type_error!("Offset out of bounds"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum GeneratorState {
    NotStarted,
    Running { pc: usize, stack: Vec<Value> },   // program counter and value stack
    Suspended { pc: usize, stack: Vec<Value> }, // suspended at yield
    Completed,
}

pub type JSObjectDataPtr = Rc<RefCell<JSObjectData>>;

#[derive(Clone, Default)]
pub struct JSObjectData {
    pub properties: std::collections::HashMap<PropertyKey, Rc<RefCell<Value>>>,
    pub constants: std::collections::HashSet<String>,
    pub prototype: Option<Rc<RefCell<JSObjectData>>>,
    pub is_function_scope: bool,
}

impl std::fmt::Debug for JSObjectData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "JSObjectData {{ properties: {}, constants: {}, prototype: {}, is_function_scope: {} }}",
            self.properties.len(),
            self.constants.len(),
            self.prototype.is_some(),
            self.is_function_scope
        )
    }
}

impl JSObjectData {
    pub fn new() -> Self {
        JSObjectData::default()
    }

    pub fn insert(&mut self, key: PropertyKey, val: Rc<RefCell<Value>>) {
        self.properties.insert(key, val);
    }

    pub fn get(&self, key: &PropertyKey) -> Option<Rc<RefCell<Value>>> {
        self.properties.get(key).cloned()
    }

    pub fn contains_key(&self, key: &PropertyKey) -> bool {
        self.properties.contains_key(key)
    }

    pub fn remove(&mut self, key: &PropertyKey) -> Option<Rc<RefCell<Value>>> {
        self.properties.remove(key)
    }

    pub fn keys(&self) -> std::collections::hash_map::Keys<'_, PropertyKey, Rc<RefCell<Value>>> {
        self.properties.keys()
    }

    pub fn is_const(&self, key: &str) -> bool {
        self.constants.contains(key)
    }

    pub fn set_const(&mut self, key: String) {
        self.constants.insert(key);
    }
}

#[derive(Clone, Debug)]
pub struct SymbolData {
    pub description: Option<String>,
}

#[derive(Clone)]
pub enum Value {
    Number(f64),
    /// BigInt literal stored with raw string and optional parsed cache
    BigInt(BigIntHolder),
    String(Vec<u16>), // UTF-16 code units
    Boolean(bool),
    Undefined,
    Object(JSObjectDataPtr),                                         // Object with properties
    Function(String),                                                // Function name
    Closure(Vec<String>, Vec<Statement>, JSObjectDataPtr),           // parameters, body, captured environment
    AsyncClosure(Vec<String>, Vec<Statement>, JSObjectDataPtr),      // parameters, body, captured environment
    GeneratorFunction(Vec<String>, Vec<Statement>, JSObjectDataPtr), // parameters, body, captured environment
    ClassDefinition(Rc<ClassDefinition>),                            // Class definition
    Getter(Vec<Statement>, JSObjectDataPtr),                         // getter body, captured environment
    Setter(Vec<String>, Vec<Statement>, JSObjectDataPtr),            // setter parameter, body, captured environment
    Property {
        // Property descriptor with getter/setter/value
        value: Option<Rc<RefCell<Value>>>,
        getter: Option<(Vec<Statement>, JSObjectDataPtr)>,
        setter: Option<(Vec<String>, Vec<Statement>, JSObjectDataPtr)>,
    },
    Promise(Rc<RefCell<JSPromise>>),         // Promise object
    Symbol(Rc<SymbolData>),                  // Symbol primitive with description
    Map(Rc<RefCell<JSMap>>),                 // Map object
    Set(Rc<RefCell<JSSet>>),                 // Set object
    WeakMap(Rc<RefCell<JSWeakMap>>),         // WeakMap object
    WeakSet(Rc<RefCell<JSWeakSet>>),         // WeakSet object
    Generator(Rc<RefCell<JSGenerator>>),     // Generator object
    Proxy(Rc<RefCell<JSProxy>>),             // Proxy object
    ArrayBuffer(Rc<RefCell<JSArrayBuffer>>), // ArrayBuffer object
    DataView(Rc<RefCell<JSDataView>>),       // DataView object
    TypedArray(Rc<RefCell<JSTypedArray>>),   // TypedArray object
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Number(n) => write!(f, "Number({})", n),
            Value::BigInt(h) => write!(f, "BigInt({})", h.raw),
            Value::String(s) => write!(f, "String({})", String::from_utf16_lossy(s)),
            Value::Boolean(b) => write!(f, "Boolean({})", b),
            Value::Undefined => write!(f, "Undefined"),
            Value::Object(obj) => write!(f, "Object({:p})", Rc::as_ptr(obj)),
            Value::Function(name) => write!(f, "Function({})", name),
            Value::Closure(_, _, _) => write!(f, "Closure"),
            Value::AsyncClosure(_, _, _) => write!(f, "AsyncClosure"),
            Value::GeneratorFunction(_, _, _) => write!(f, "GeneratorFunction"),
            Value::ClassDefinition(_) => write!(f, "ClassDefinition"),
            Value::Getter(_, _) => write!(f, "Getter"),
            Value::Setter(_, _, _) => write!(f, "Setter"),
            Value::Property { .. } => write!(f, "Property"),
            Value::Promise(p) => write!(f, "Promise({:p})", Rc::as_ptr(p)),
            Value::Symbol(_) => write!(f, "Symbol"),
            Value::Map(m) => write!(f, "Map({:p})", Rc::as_ptr(m)),
            Value::Set(s) => write!(f, "Set({:p})", Rc::as_ptr(s)),
            Value::WeakMap(wm) => write!(f, "WeakMap({:p})", Rc::as_ptr(wm)),
            Value::WeakSet(ws) => write!(f, "WeakSet({:p})", Rc::as_ptr(ws)),
            Value::Generator(g) => write!(f, "Generator({:p})", Rc::as_ptr(g)),
            Value::Proxy(p) => write!(f, "Proxy({:p})", Rc::as_ptr(p)),
            Value::ArrayBuffer(ab) => write!(f, "ArrayBuffer({:p})", Rc::as_ptr(ab)),
            Value::DataView(dv) => write!(f, "DataView({:p})", Rc::as_ptr(dv)),
            Value::TypedArray(ta) => write!(f, "TypedArray({:p})", Rc::as_ptr(ta)),
        }
    }
}

pub fn is_truthy(val: &Value) -> bool {
    match val {
        Value::BigInt(h) => {
            // Simple check: treat bigint as falsy only when it's zero (0n, 0x0n, 0b0n, 0o0n).
            let s = &h.raw;
            let s_no_n = if s.ends_with('n') { &s[..s.len() - 1] } else { s.as_str() };
            #[allow(clippy::if_same_then_else)]
            let s_no_prefix = if s_no_n.starts_with("0x") || s_no_n.starts_with("0X") {
                &s_no_n[2..]
            } else if s_no_n.starts_with("0b") || s_no_n.starts_with("0B") {
                &s_no_n[2..]
            } else if s_no_n.starts_with("0o") || s_no_n.starts_with("0O") {
                &s_no_n[2..]
            } else {
                s_no_n
            };
            !s_no_prefix.chars().all(|c| c == '0')
        }
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Boolean(b) => *b,
        Value::Undefined => false,
        Value::Object(_) => true,
        Value::Function(_) => true,
        Value::Closure(_, _, _) => true,
        Value::AsyncClosure(_, _, _) => true,
        Value::GeneratorFunction(_, _, _) => true,
        Value::ClassDefinition(_) => true,
        Value::Getter(_, _) => true,
        Value::Setter(_, _, _) => true,
        Value::Property { .. } => true,
        Value::Promise(_) => true,
        Value::Symbol(_) => true,
        Value::Map(_) => true,
        Value::Set(_) => true,
        Value::WeakMap(_) => true,
        Value::WeakSet(_) => true,
        Value::Generator(_) => true,
        Value::Proxy(_) => true,
        Value::ArrayBuffer(_) => true,
        Value::DataView(_) => true,
        Value::TypedArray(_) => true,
    }
}

// Helper function to compare two values for equality
pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::BigInt(sa), Value::BigInt(sb)) => sa.raw == sb.raw,
        (Value::Number(na), Value::Number(nb)) => na == nb,
        (Value::String(sa), Value::String(sb)) => sa == sb,
        (Value::Boolean(ba), Value::Boolean(bb)) => ba == bb,
        (Value::Undefined, Value::Undefined) => true,
        (Value::Object(_), Value::Object(_)) => false, // Objects are not equal unless same reference
        (Value::Symbol(sa), Value::Symbol(sb)) => Rc::ptr_eq(sa, sb), // Symbols are equal if same reference
        _ => false,                                    // Different types are not equal
    }
}

// Helper function to convert value to string for display
pub fn value_to_string(val: &Value) -> String {
    match val {
        Value::Number(n) => n.to_string(),
        Value::BigInt(h) => h.raw.clone(),
        Value::String(s) => String::from_utf16_lossy(s),
        Value::Boolean(b) => b.to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Object(_) => "[object Object]".to_string(),
        Value::Function(name) => format!("function {}", name),
        Value::Closure(_, _, _) => "function".to_string(),
        Value::AsyncClosure(_, _, _) => "function".to_string(),
        Value::GeneratorFunction(_, _, _) => "function".to_string(),
        Value::ClassDefinition(_) => "class".to_string(),
        Value::Getter(_, _) => "getter".to_string(),
        Value::Setter(_, _, _) => "setter".to_string(),
        Value::Property { .. } => "[property]".to_string(),
        Value::Promise(_) => "[object Promise]".to_string(),
        Value::Symbol(desc) => match desc.description.as_ref() {
            Some(d) => format!("Symbol({})", d),
            None => "Symbol()".to_string(),
        },
        Value::Map(_) => "[object Map]".to_string(),
        Value::Set(_) => "[object Set]".to_string(),
        Value::WeakMap(_) => "[object WeakMap]".to_string(),
        Value::WeakSet(_) => "[object WeakSet]".to_string(),
        Value::Generator(_) => "[object Generator]".to_string(),
        Value::Proxy(_) => "[object Proxy]".to_string(),
        Value::ArrayBuffer(_) => "[object ArrayBuffer]".to_string(),
        Value::DataView(_) => "[object DataView]".to_string(),
        Value::TypedArray(_) => "[object TypedArray]".to_string(),
    }
}

// Helper: perform ToPrimitive coercion with a given hint ('string', 'number', 'default')
pub fn to_primitive(val: &Value, hint: &str) -> Result<Value, JSError> {
    match val {
        Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::Undefined | Value::Symbol(_) => Ok(val.clone()),
        Value::Object(obj_map) => {
            // Prefer explicit [Symbol.toPrimitive] if present and callable
            if let Some(tp_sym) = get_well_known_symbol_rc("toPrimitive") {
                let key = PropertyKey::Symbol(tp_sym.clone());
                if let Some(method_rc) = obj_get_value(obj_map, &key)? {
                    let method_val = method_rc.borrow().clone();
                    match method_val {
                        Value::Closure(params, body, captured_env) | Value::AsyncClosure(params, body, captured_env) => {
                            // Create a new execution env and bind this
                            let func_env = Rc::new(RefCell::new(JSObjectData::new()));
                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                            env_set(&func_env, "this", Value::Object(obj_map.clone()))?;
                            // Pass hint as first param if the function declares params
                            if !params.is_empty() {
                                env_set(&func_env, &params[0], Value::String(utf8_to_utf16(hint)))?;
                            }
                            let result = evaluate_statements(&func_env, &body)?;
                            match result {
                                Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::Symbol(_) => return Ok(result),
                                _ => {
                                    return Err(raise_type_error!("[Symbol.toPrimitive] must return a primitive"));
                                }
                            }
                        }
                        _ => {
                            // Not a closure/minimally supported callable - fall through to default algorithm
                        }
                    }
                }
            }

            // Default algorithm: order depends on hint
            if hint == "string" {
                // toString -> valueOf
                let to_s = crate::js_object::handle_to_string_method(&Value::Object(obj_map.clone()), &[])?;
                if matches!(to_s, Value::String(_) | Value::Number(_) | Value::Boolean(_)) {
                    return Ok(to_s);
                }
                let val_of = crate::js_object::handle_value_of_method(&Value::Object(obj_map.clone()), &[])?;
                if matches!(val_of, Value::String(_) | Value::Number(_) | Value::Boolean(_)) {
                    return Ok(val_of);
                }
            } else {
                // number or default: valueOf -> toString
                let val_of = crate::js_object::handle_value_of_method(&Value::Object(obj_map.clone()), &[])?;
                if matches!(val_of, Value::Number(_) | Value::String(_) | Value::Boolean(_)) {
                    return Ok(val_of);
                }
                let to_s = crate::js_object::handle_to_string_method(&Value::Object(obj_map.clone()), &[])?;
                if matches!(to_s, Value::String(_) | Value::Number(_) | Value::Boolean(_)) {
                    return Ok(to_s);
                }
            }

            Err(raise_type_error!("Cannot convert object to primitive"))
        }
        _ => Ok(val.clone()),
    }
}

// Helper function to convert value to string for sorting
pub fn value_to_sort_string(val: &Value) -> String {
    match val {
        Value::Number(n) => {
            if n.is_nan() {
                "NaN".to_string()
            } else if *n == f64::INFINITY {
                "Infinity".to_string()
            } else if *n == f64::NEG_INFINITY {
                "-Infinity".to_string()
            } else {
                n.to_string()
            }
        }
        Value::BigInt(h) => h.raw.clone(),
        Value::String(s) => String::from_utf16_lossy(s),
        Value::Boolean(b) => b.to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Object(_) => "[object Object]".to_string(),
        Value::Function(name) => format!("[function {}]", name),
        Value::Closure(_, _, _) | Value::AsyncClosure(_, _, _) | Value::GeneratorFunction(_, _, _) => "[function]".to_string(),
        Value::ClassDefinition(_) => "[class]".to_string(),
        Value::Getter(_, _) => "[getter]".to_string(),
        Value::Setter(_, _, _) => "[setter]".to_string(),
        Value::Property { .. } => "[property]".to_string(),
        Value::Promise(_) => "[object Promise]".to_string(),
        Value::Symbol(_) => "[object Symbol]".to_string(),
        Value::Map(_) => "[object Map]".to_string(),
        Value::Set(_) => "[object Set]".to_string(),
        Value::WeakMap(_) => "[object WeakMap]".to_string(),
        Value::WeakSet(_) => "[object WeakSet]".to_string(),
        Value::Generator(_) => "[object Generator]".to_string(),
        Value::Proxy(_) => "[object Proxy]".to_string(),
        Value::ArrayBuffer(_) => "[object ArrayBuffer]".to_string(),
        Value::DataView(_) => "[object DataView]".to_string(),
        Value::TypedArray(_) => "[object TypedArray]".to_string(),
    }
}

// Helper accessors for objects and environments
pub fn obj_get_value(js_obj: &JSObjectDataPtr, key: &PropertyKey) -> Result<Option<Rc<RefCell<Value>>>, JSError> {
    // Check if this object is a proxy wrapper
    if let Some(proxy_val) = js_obj.borrow().get(&"__proxy__".into())
        && let Value::Proxy(proxy) = &*proxy_val.borrow()
    {
        return crate::js_proxy::proxy_get_property(proxy, key);
    }

    // Search own properties and then walk the prototype chain until we find
    // a matching property or run out of prototypes.
    let mut current: Option<JSObjectDataPtr> = Some(js_obj.clone());
    while let Some(cur) = current {
        if let Some(val) = cur.borrow().get(key) {
            // Found an own/inherited value on `cur`. For getters we bind `this` to
            // the original object (`js_obj`) as per JS semantics.
            let val_clone = val.borrow().clone();
            match val_clone {
                Value::Property { value, getter, .. } => {
                    log::trace!("obj_get_value - property descriptor found for key {}", key);
                    if let Some((body, env)) = getter {
                        // Create a new environment with this bound to the original object
                        let getter_env = Rc::new(RefCell::new(JSObjectData::new()));
                        getter_env.borrow_mut().prototype = Some(env);
                        env_set(&getter_env, "this", Value::Object(js_obj.clone()))?;
                        let result = evaluate_statements(&getter_env, &body)?;
                        return Ok(Some(Rc::new(RefCell::new(result))));
                    } else if let Some(val_rc) = value {
                        return Ok(Some(val_rc));
                    } else {
                        return Ok(Some(Rc::new(RefCell::new(Value::Undefined))));
                    }
                }
                Value::Getter(body, env) => {
                    log::trace!("obj_get_value - getter found for key {}", key);
                    let getter_env = Rc::new(RefCell::new(JSObjectData::new()));
                    getter_env.borrow_mut().prototype = Some(env);
                    env_set(&getter_env, "this", Value::Object(js_obj.clone()))?;
                    let result = evaluate_statements(&getter_env, &body)?;
                    return Ok(Some(Rc::new(RefCell::new(result))));
                }
                _ => {
                    log::trace!("obj_get_value - raw value found for key {}", key);
                    return Ok(Some(val.clone()));
                }
            }
        }
        // Not found on this object; continue with prototype.
        current = cur.borrow().prototype.clone();
    }

    // No own or inherited property found, fall back to special-case handling
    // (well-known symbol fallbacks, array/string iterator helpers, etc.).

    // Provide default well-known symbol fallbacks (non-own) for some built-ins.
    if let PropertyKey::Symbol(sym_rc) = key {
        // Support default Symbol.iterator for built-ins like Array and wrapped String objects.
        if let Some(iter_sym_rc) = get_well_known_symbol_rc("iterator")
            && let (Value::Symbol(iter_sd), Value::Symbol(req_sd)) = (&*iter_sym_rc.borrow(), &*sym_rc.borrow())
            && Rc::ptr_eq(iter_sd, req_sd)
        {
            // Array default iterator
            if is_array(js_obj) {
                // next function body
                let next_body = vec![
                    Statement::Let("idx".to_string(), Some(Expr::Var("__i".to_string()))),
                    Statement::If(
                        Expr::Binary(
                            Box::new(Expr::Var("idx".to_string())),
                            BinaryOp::LessThan,
                            Box::new(Expr::Property(Box::new(Expr::Var("__array".to_string())), "length".to_string())),
                        ),
                        vec![
                            Statement::Let(
                                "v".to_string(),
                                Some(Expr::Index(
                                    Box::new(Expr::Var("__array".to_string())),
                                    Box::new(Expr::Var("idx".to_string())),
                                )),
                            ),
                            Statement::Expr(Expr::Assign(
                                Box::new(Expr::Var("__i".to_string())),
                                Box::new(Expr::Binary(
                                    Box::new(Expr::Var("idx".to_string())),
                                    BinaryOp::Add,
                                    Box::new(Expr::Value(Value::Number(1.0))),
                                )),
                            )),
                            Statement::Return(Some(Expr::Object(vec![
                                ("value".to_string(), Expr::Var("v".to_string())),
                                ("done".to_string(), Expr::Value(Value::Boolean(false))),
                            ]))),
                        ],
                        Some(vec![Statement::Return(Some(Expr::Object(vec![(
                            "done".to_string(),
                            Expr::Value(Value::Boolean(true)),
                        )])))]),
                    ),
                ];

                let arr_iter_body = vec![
                    Statement::Let("__i".to_string(), Some(Expr::Value(Value::Number(0.0)))),
                    Statement::Return(Some(Expr::Object(vec![(
                        "next".to_string(),
                        Expr::Function(Vec::new(), next_body),
                    )]))),
                ];

                let captured_env = Rc::new(RefCell::new(JSObjectData::new()));
                captured_env.borrow_mut().insert(
                    PropertyKey::String("__array".to_string()),
                    Rc::new(RefCell::new(Value::Object(js_obj.clone()))),
                );
                let closure = Value::Closure(Vec::new(), arr_iter_body, captured_env.clone());
                return Ok(Some(Rc::new(RefCell::new(closure))));
            }

            // Map default iterator
            if let Some(map_val) = js_obj.borrow().get(&"__map__".into())
                && let Value::Map(map_rc) = &*map_val.borrow()
            {
                let map_entries = map_rc.borrow().entries.clone();

                // next function body for Map iteration (returns [key, value] pairs)
                let next_body = vec![
                    Statement::Let("idx".to_string(), Some(Expr::Var("__i".to_string()))),
                    Statement::If(
                        Expr::Binary(
                            Box::new(Expr::Var("idx".to_string())),
                            BinaryOp::LessThan,
                            Box::new(Expr::Value(Value::Number(map_entries.len() as f64))),
                        ),
                        vec![
                            Statement::Let(
                                "entry".to_string(),
                                Some(Expr::Array(vec![
                                    Expr::Property(
                                        Box::new(Expr::Index(
                                            Box::new(Expr::Var("__entries".to_string())),
                                            Box::new(Expr::Var("idx".to_string())),
                                        )),
                                        "0".to_string(),
                                    ),
                                    Expr::Property(
                                        Box::new(Expr::Index(
                                            Box::new(Expr::Var("__entries".to_string())),
                                            Box::new(Expr::Var("idx".to_string())),
                                        )),
                                        "1".to_string(),
                                    ),
                                ])),
                            ),
                            Statement::Expr(Expr::Assign(
                                Box::new(Expr::Var("__i".to_string())),
                                Box::new(Expr::Binary(
                                    Box::new(Expr::Var("__i".to_string())),
                                    BinaryOp::Add,
                                    Box::new(Expr::Value(Value::Number(1.0))),
                                )),
                            )),
                            Statement::Return(Some(Expr::Object(vec![
                                ("value".to_string(), Expr::Var("entry".to_string())),
                                ("done".to_string(), Expr::Value(Value::Boolean(false))),
                            ]))),
                        ],
                        Some(vec![Statement::Return(Some(Expr::Object(vec![(
                            "done".to_string(),
                            Expr::Value(Value::Boolean(true)),
                        )])))]),
                    ),
                ];

                let map_iter_body = vec![
                    Statement::Let("__i".to_string(), Some(Expr::Value(Value::Number(0.0)))),
                    Statement::Return(Some(Expr::Object(vec![(
                        "next".to_string(),
                        Expr::Function(Vec::new(), next_body),
                    )]))),
                ];

                let captured_env = Rc::new(RefCell::new(JSObjectData::new()));
                // Store map entries in the closure environment
                let mut entries_obj = JSObjectData::new();
                for (i, (key, value)) in map_entries.iter().enumerate() {
                    let mut entry_obj = JSObjectData::new();
                    entry_obj.insert("0".into(), Rc::new(RefCell::new(key.clone())));
                    entry_obj.insert("1".into(), Rc::new(RefCell::new(value.clone())));
                    entries_obj.insert(
                        i.to_string().into(),
                        Rc::new(RefCell::new(Value::Object(Rc::new(RefCell::new(entry_obj))))),
                    );
                }
                captured_env.borrow_mut().insert(
                    "__entries".into(),
                    Rc::new(RefCell::new(Value::Object(Rc::new(RefCell::new(entries_obj))))),
                );

                let closure = Value::Closure(Vec::new(), map_iter_body, captured_env.clone());
                return Ok(Some(Rc::new(RefCell::new(closure))));
            }

            // Set default iterator
            if let Some(set_val) = js_obj.borrow().get(&"__set__".into())
                && let Value::Set(set_rc) = &*set_val.borrow()
            {
                let set_values = set_rc.borrow().values.clone();
                // next function body for Set iteration (returns values)
                let next_body = vec![
                    Statement::Let("idx".to_string(), Some(Expr::Var("__i".to_string()))),
                    Statement::If(
                        Expr::Binary(
                            Box::new(Expr::Var("idx".to_string())),
                            BinaryOp::LessThan,
                            Box::new(Expr::Value(Value::Number(set_values.len() as f64))),
                        ),
                        vec![
                            Statement::Let(
                                "value".to_string(),
                                Some(Expr::Index(
                                    Box::new(Expr::Var("__values".to_string())),
                                    Box::new(Expr::Var("idx".to_string())),
                                )),
                            ),
                            Statement::Expr(Expr::Assign(
                                Box::new(Expr::Var("__i".to_string())),
                                Box::new(Expr::Binary(
                                    Box::new(Expr::Var("idx".to_string())),
                                    BinaryOp::Add,
                                    Box::new(Expr::Value(Value::Number(1.0))),
                                )),
                            )),
                            Statement::Return(Some(Expr::Object(vec![
                                ("value".to_string(), Expr::Var("value".to_string())),
                                ("done".to_string(), Expr::Value(Value::Boolean(false))),
                            ]))),
                        ],
                        Some(vec![Statement::Return(Some(Expr::Object(vec![(
                            "done".to_string(),
                            Expr::Value(Value::Boolean(true)),
                        )])))]),
                    ),
                ];

                let set_iter_body = vec![
                    Statement::Let("__i".to_string(), Some(Expr::Value(Value::Number(0.0)))),
                    Statement::Return(Some(Expr::Object(vec![(
                        "next".to_string(),
                        Expr::Function(Vec::new(), next_body),
                    )]))),
                ];

                let captured_env = Rc::new(RefCell::new(JSObjectData::new()));
                // Store set values in the closure environment
                let mut values_obj = JSObjectData::new();
                for (i, value) in set_values.iter().enumerate() {
                    values_obj.insert(i.to_string().into(), Rc::new(RefCell::new(value.clone())));
                }
                captured_env.borrow_mut().insert(
                    "__values".into(),
                    Rc::new(RefCell::new(Value::Object(Rc::new(RefCell::new(values_obj))))),
                );

                let closure = Value::Closure(Vec::new(), set_iter_body, captured_env.clone());
                return Ok(Some(Rc::new(RefCell::new(closure))));
            }

            // Wrapped String iterator (for String objects)
            if let Some(wrapped) = js_obj.borrow().get(&"__value__".into())
                && let Value::String(_) = &*wrapped.borrow()
            {
                let next_body = vec![
                    Statement::Let("idx".to_string(), Some(Expr::Var("__i".to_string()))),
                    Statement::If(
                        Expr::Binary(
                            Box::new(Expr::Var("idx".to_string())),
                            BinaryOp::LessThan,
                            Box::new(Expr::Property(Box::new(Expr::Var("__s".to_string())), "length".to_string())),
                        ),
                        vec![
                            Statement::Let(
                                "v".to_string(),
                                Some(Expr::Index(
                                    Box::new(Expr::Var("__s".to_string())),
                                    Box::new(Expr::Var("idx".to_string())),
                                )),
                            ),
                            Statement::Expr(Expr::Assign(
                                Box::new(Expr::Var("__i".to_string())),
                                Box::new(Expr::Binary(
                                    Box::new(Expr::Var("idx".to_string())),
                                    BinaryOp::Add,
                                    Box::new(Expr::Value(Value::Number(1.0))),
                                )),
                            )),
                            Statement::Return(Some(Expr::Object(vec![
                                ("value".to_string(), Expr::Var("v".to_string())),
                                ("done".to_string(), Expr::Value(Value::Boolean(false))),
                            ]))),
                        ],
                        Some(vec![Statement::Return(Some(Expr::Object(vec![(
                            "done".to_string(),
                            Expr::Value(Value::Boolean(true)),
                        )])))]),
                    ),
                ];

                let str_iter_body = vec![
                    Statement::Let("__i".to_string(), Some(Expr::Value(Value::Number(0.0)))),
                    Statement::Return(Some(Expr::Object(vec![(
                        "next".to_string(),
                        Expr::Function(Vec::new(), next_body),
                    )]))),
                ];

                let captured_env = Rc::new(RefCell::new(JSObjectData::new()));
                captured_env.borrow_mut().insert(
                    PropertyKey::String("__s".to_string()),
                    Rc::new(RefCell::new(wrapped.borrow().clone())),
                );
                let closure = Value::Closure(Vec::new(), str_iter_body, captured_env.clone());
                return Ok(Some(Rc::new(RefCell::new(closure))));
            }
        }
        if let Some(tag_sym_rc) = get_well_known_symbol_rc("toStringTag")
            && let (Value::Symbol(tag_sd), Value::Symbol(req_sd)) = (&*tag_sym_rc.borrow(), &*sym_rc.borrow())
            && Rc::ptr_eq(tag_sd, req_sd)
        {
            if is_array(js_obj) {
                return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("Array"))))));
            }
            if let Some(wrapped) = js_obj.borrow().get(&"__value__".into()) {
                match &*wrapped.borrow() {
                    Value::String(_) => return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("String")))))),
                    Value::Number(_) => return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("Number")))))),
                    Value::Boolean(_) => return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("Boolean")))))),
                    _ => {}
                }
            }
            if js_obj.borrow().contains_key(&"__timestamp".into()) {
                return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("Date"))))));
            }
        }
    }

    Ok(None)
}

pub fn obj_set_value(js_obj: &JSObjectDataPtr, key: &PropertyKey, val: Value) -> Result<(), JSError> {
    // Check if this object is a proxy wrapper
    if let Some(proxy_val) = js_obj.borrow().get(&"__proxy__".into())
        && let Value::Proxy(proxy) = &*proxy_val.borrow()
    {
        let success = crate::js_proxy::proxy_set_property(proxy, key, val)?;
        if !success {
            return Err(raise_eval_error!("Proxy set trap returned false"));
        }
        return Ok(());
    }

    // Check if there's a setter for this property
    let existing_opt = js_obj.borrow().get(key);
    if let Some(existing) = existing_opt {
        match existing.borrow().clone() {
            Value::Property { value: _, getter, setter } => {
                if let Some((param, body, env)) = setter {
                    // Create a new environment with this bound to the object and the parameter
                    let setter_env = Rc::new(RefCell::new(JSObjectData::new()));
                    setter_env.borrow_mut().prototype = Some(env);
                    env_set(&setter_env, "this", Value::Object(js_obj.clone()))?;
                    env_set(&setter_env, &param[0], val)?;
                    let _v = evaluate_statements(&setter_env, &body)?;
                } else {
                    // No setter, update value
                    let value = Some(Rc::new(RefCell::new(val)));
                    let new_prop = Value::Property { value, getter, setter };
                    js_obj.borrow_mut().insert(key.clone(), Rc::new(RefCell::new(new_prop)));
                }
                return Ok(());
            }
            Value::Setter(param, body, env) => {
                // Create a new environment with this bound to the object and the parameter
                let setter_env = Rc::new(RefCell::new(JSObjectData::new()));
                setter_env.borrow_mut().prototype = Some(env);
                env_set(&setter_env, "this", Value::Object(js_obj.clone()))?;
                env_set(&setter_env, &param[0], val)?;
                evaluate_statements(&setter_env, &body)?;
                return Ok(());
            }
            _ => {}
        }
    }
    // No setter, just set the value normally
    js_obj.borrow_mut().insert(key.clone(), Rc::new(RefCell::new(val)));
    Ok(())
}

pub fn obj_set_rc(map: &JSObjectDataPtr, key: &PropertyKey, val_rc: Rc<RefCell<Value>>) {
    map.borrow_mut().insert(key.clone(), val_rc);
}

pub fn obj_delete(map: &JSObjectDataPtr, key: &PropertyKey) -> Result<bool, JSError> {
    // Check if this object is a proxy wrapper
    if let Some(proxy_val) = map.borrow().get(&"__proxy__".into())
        && let Value::Proxy(proxy) = &*proxy_val.borrow()
    {
        return crate::js_proxy::proxy_delete_property(proxy, key);
    }

    map.borrow_mut().remove(key);
    Ok(true) // In JavaScript, delete always returns true
}

pub fn env_get<T: AsRef<str>>(env: &JSObjectDataPtr, key: T) -> Option<Rc<RefCell<Value>>> {
    env.borrow().get(&key.as_ref().into())
}

pub fn env_set<T: AsRef<str>>(env: &JSObjectDataPtr, key: T, val: Value) -> Result<(), JSError> {
    let key = key.as_ref();
    if env.borrow().is_const(key) {
        return Err(raise_type_error!(format!("Assignment to constant variable '{key}'")));
    }
    env.borrow_mut()
        .insert(PropertyKey::String(key.to_string()), Rc::new(RefCell::new(val)));
    Ok(())
}

pub fn env_set_recursive<T: AsRef<str>>(env: &JSObjectDataPtr, key: T, val: Value) -> Result<(), JSError> {
    let key_str = key.as_ref();
    let mut current = env.clone();
    loop {
        if current.borrow().contains_key(&key_str.into()) {
            return env_set(&current, key_str, val);
        }
        let parent_opt = current.borrow().prototype.clone();
        if let Some(parent) = parent_opt {
            current = parent;
        } else {
            // if not found, set in current env
            return env_set(env, key_str, val);
        }
    }
}

pub fn env_set_var(env: &JSObjectDataPtr, key: &str, val: Value) -> Result<(), JSError> {
    let mut current = env.clone();
    loop {
        if current.borrow().is_function_scope {
            return env_set(&current, key, val);
        }
        let parent_opt = current.borrow().prototype.clone();
        if let Some(parent) = parent_opt {
            current = parent;
        } else {
            // If no function scope found, set in current env (global)
            return env_set(env, key, val);
        }
    }
}

pub fn env_set_const(env: &JSObjectDataPtr, key: &str, val: Value) {
    let mut env_mut = env.borrow_mut();
    env_mut.insert(PropertyKey::String(key.to_string()), Rc::new(RefCell::new(val)));
    env_mut.set_const(key.to_string());
}
