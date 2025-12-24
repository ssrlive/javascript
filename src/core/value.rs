#![allow(clippy::collapsible_if, clippy::collapsible_match)]

use num_bigint::BigInt;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::{cell::RefCell, rc::Rc};

use crate::js_array::{get_array_length, set_array_length};
use crate::js_date::is_date_object;
use crate::{
    JSError,
    core::{
        BinaryOp, DestructuringElement, Expr, PropertyKey, Statement, StatementKind, evaluate_statements, get_well_known_symbol_rc,
        utf8_to_utf16,
    },
    js_array::is_array,
    js_class::ClassDefinition,
    js_promise::JSPromise,
    raise_eval_error, raise_range_error, raise_type_error,
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
    pub params: Vec<DestructuringElement>,
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
    // Use an Arc<Mutex<Vec<u8>>> so the underlying bytes can be shared
    // safely between threads (for SharedArrayBuffer semantics) while
    // remaining compatible with the existing Rc<RefCell<JSArrayBuffer>>
    // wrapper used across the project.
    pub data: Arc<Mutex<Vec<u8>>>, // The underlying byte buffer
    pub detached: bool,            // Whether the buffer has been detached
    pub shared: bool,              // Whether this buffer was created as a SharedArrayBuffer
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

pub fn parse_bigint_string(raw: &str) -> Result<BigInt, JSError> {
    let s = if let Some(st) = raw.strip_suffix('n') { st } else { raw };
    let (radix, num_str) = if s.starts_with("0x") || s.starts_with("0X") {
        (16, &s[2..])
    } else if s.starts_with("0b") || s.starts_with("0B") {
        (2, &s[2..])
    } else if s.starts_with("0o") || s.starts_with("0O") {
        (8, &s[2..])
    } else {
        (10, s)
    };
    BigInt::parse_bytes(num_str.as_bytes(), radix).ok_or(raise_eval_error!("invalid bigint"))
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
        let data_lock = buffer.data.lock().unwrap();
        if byte_index + self.element_size() > data_lock.len() {
            return Err(raise_type_error!("Index out of bounds"));
        }

        match self.kind {
            TypedArrayKind::Int8 => Ok(data_lock[byte_index] as i8 as i64),
            TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => Ok(data_lock[byte_index] as i64),
            TypedArrayKind::Int16 => {
                let bytes = &data_lock[byte_index..byte_index + 2];
                Ok(i16::from_le_bytes([bytes[0], bytes[1]]) as i64)
            }
            TypedArrayKind::Uint16 => {
                let bytes = &data_lock[byte_index..byte_index + 2];
                Ok(u16::from_le_bytes([bytes[0], bytes[1]]) as i64)
            }
            TypedArrayKind::Int32 => {
                let bytes = &data_lock[byte_index..byte_index + 4];
                Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as i64)
            }
            TypedArrayKind::Uint32 => {
                let bytes = &data_lock[byte_index..byte_index + 4];
                Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as i64)
            }
            TypedArrayKind::Float32 => {
                let bytes = &data_lock[byte_index..byte_index + 4];
                let float_val = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                Ok(float_val as i64) // Simplified conversion
            }
            TypedArrayKind::Float64 => {
                let bytes = &data_lock[byte_index..byte_index + 8];
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
        let buffer = self.buffer.borrow_mut();
        if buffer.detached {
            return Err(raise_type_error!("ArrayBuffer is detached"));
        }

        let byte_index = self.byte_offset + index * self.element_size();
        let mut data_lock = buffer.data.lock().unwrap();
        if byte_index + self.element_size() > data_lock.len() {
            return Err(raise_type_error!("Index out of bounds"));
        }

        match self.kind {
            TypedArrayKind::Int8 => {
                data_lock[byte_index] = value as i8 as u8;
            }
            TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => {
                data_lock[byte_index] = value as u8;
            }
            TypedArrayKind::Int16 => {
                let bytes = (value as i16).to_le_bytes();
                data_lock[byte_index..byte_index + 2].copy_from_slice(&bytes);
            }
            TypedArrayKind::Uint16 => {
                let bytes = (value as u16).to_le_bytes();
                data_lock[byte_index..byte_index + 2].copy_from_slice(&bytes);
            }
            TypedArrayKind::Int32 => {
                let bytes = (value as i32).to_le_bytes();
                data_lock[byte_index..byte_index + 4].copy_from_slice(&bytes);
            }
            TypedArrayKind::Uint32 => {
                let bytes = (value as u32).to_le_bytes();
                data_lock[byte_index..byte_index + 4].copy_from_slice(&bytes);
            }
            TypedArrayKind::Float32 => {
                let bytes = (value as f32).to_le_bytes();
                data_lock[byte_index..byte_index + 4].copy_from_slice(&bytes);
            }
            TypedArrayKind::Float64 => {
                let bytes = (value as f64).to_le_bytes();
                data_lock[byte_index..byte_index + 8].copy_from_slice(&bytes);
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
        let data_lock = buffer.data.lock().unwrap();
        Ok(data_lock[self.byte_offset + offset] as i8)
    }

    /// Get an 8-bit unsigned integer at the specified byte offset
    pub fn get_uint8(&self, offset: usize) -> Result<u8, JSError> {
        self.check_bounds(offset, 1)?;
        let buffer = self.buffer.borrow();
        let data_lock = buffer.data.lock().unwrap();
        Ok(data_lock[self.byte_offset + offset])
    }

    /// Get a 16-bit signed integer at the specified byte offset
    pub fn get_int16(&self, offset: usize, little_endian: bool) -> Result<i16, JSError> {
        self.check_bounds(offset, 2)?;
        let buffer = self.buffer.borrow();
        let data_lock = buffer.data.lock().unwrap();
        let bytes = &data_lock[self.byte_offset + offset..self.byte_offset + offset + 2];
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
        let data_lock = buffer.data.lock().unwrap();
        let bytes = &data_lock[self.byte_offset + offset..self.byte_offset + offset + 2];
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
        let data_lock = buffer.data.lock().unwrap();
        let bytes = &data_lock[self.byte_offset + offset..self.byte_offset + offset + 4];
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
        let data_lock = buffer.data.lock().unwrap();
        let bytes = &data_lock[self.byte_offset + offset..self.byte_offset + offset + 4];
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
        let data_lock = buffer.data.lock().unwrap();
        let bytes = &data_lock[self.byte_offset + offset..self.byte_offset + offset + 4];
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
        let data_lock = buffer.data.lock().unwrap();
        let bytes = &data_lock[self.byte_offset + offset..self.byte_offset + offset + 8];
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
        let data_lock = buffer.data.lock().unwrap();
        let bytes = &data_lock[self.byte_offset + offset..self.byte_offset + offset + 8];
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
        let data_lock = buffer.data.lock().unwrap();
        let bytes = &data_lock[self.byte_offset + offset..self.byte_offset + offset + 8];
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
        let buffer = self.buffer.borrow_mut();
        let mut data_lock = buffer.data.lock().unwrap();
        data_lock[self.byte_offset + offset] = value as u8;
        Ok(())
    }

    /// Set an 8-bit unsigned integer at the specified byte offset
    pub fn set_uint8(&mut self, offset: usize, value: u8) -> Result<(), JSError> {
        self.check_bounds(offset, 1)?;
        let buffer = self.buffer.borrow_mut();
        let mut data_lock = buffer.data.lock().unwrap();
        data_lock[self.byte_offset + offset] = value;
        Ok(())
    }

    /// Set a 16-bit signed integer at the specified byte offset
    pub fn set_int16(&mut self, offset: usize, value: i16, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 2)?;
        let buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let mut data_lock = buffer.data.lock().unwrap();
        data_lock[self.byte_offset + offset..self.byte_offset + offset + 2].copy_from_slice(&bytes);
        Ok(())
    }

    /// Set a 16-bit unsigned integer at the specified byte offset
    pub fn set_uint16(&mut self, offset: usize, value: u16, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 2)?;
        let buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let mut data_lock = buffer.data.lock().unwrap();
        data_lock[self.byte_offset + offset..self.byte_offset + offset + 2].copy_from_slice(&bytes);
        Ok(())
    }

    /// Set a 32-bit signed integer at the specified byte offset
    pub fn set_int32(&mut self, offset: usize, value: i32, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 4)?;
        let buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let mut data_lock = buffer.data.lock().unwrap();
        data_lock[self.byte_offset + offset..self.byte_offset + offset + 4].copy_from_slice(&bytes);
        Ok(())
    }

    /// Set a 32-bit unsigned integer at the specified byte offset
    pub fn set_uint32(&mut self, offset: usize, value: u32, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 4)?;
        let buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let mut data_lock = buffer.data.lock().unwrap();
        data_lock[self.byte_offset + offset..self.byte_offset + offset + 4].copy_from_slice(&bytes);
        Ok(())
    }

    /// Set a 32-bit float at the specified byte offset
    pub fn set_float32(&mut self, offset: usize, value: f32, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 4)?;
        let buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let mut data_lock = buffer.data.lock().unwrap();
        data_lock[self.byte_offset + offset..self.byte_offset + offset + 4].copy_from_slice(&bytes);
        Ok(())
    }

    /// Set a 64-bit float at the specified byte offset
    pub fn set_float64(&mut self, offset: usize, value: f64, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 8)?;
        let buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let mut data_lock = buffer.data.lock().unwrap();
        data_lock[self.byte_offset + offset..self.byte_offset + offset + 8].copy_from_slice(&bytes);
        Ok(())
    }

    /// Set a 64-bit signed BigInt at the specified byte offset
    pub fn set_big_int64(&mut self, offset: usize, value: i64, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 8)?;
        let buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let mut data_lock = buffer.data.lock().unwrap();
        data_lock[self.byte_offset + offset..self.byte_offset + offset + 8].copy_from_slice(&bytes);
        Ok(())
    }

    /// Set a 64-bit unsigned BigInt at the specified byte offset
    pub fn set_big_uint64(&mut self, offset: usize, value: u64, little_endian: bool) -> Result<(), JSError> {
        self.check_bounds(offset, 8)?;
        let buffer = self.buffer.borrow_mut();
        let bytes = if little_endian { value.to_le_bytes() } else { value.to_be_bytes() };
        let mut data_lock = buffer.data.lock().unwrap();
        data_lock[self.byte_offset + offset..self.byte_offset + offset + 8].copy_from_slice(&bytes);
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

#[inline]
pub fn new_js_object_data() -> JSObjectDataPtr {
    Rc::new(RefCell::new(JSObjectData::new()))
}

#[derive(Clone, Default)]
pub struct JSObjectData {
    pub properties: indexmap::IndexMap<PropertyKey, Rc<RefCell<Value>>>,
    pub constants: std::collections::HashSet<String>,
    /// Tracks keys that should not be enumerated by `Object.keys` / `Object.values`.
    pub non_enumerable: std::collections::HashSet<PropertyKey>,
    /// Tracks keys that are non-writable (read-only)
    pub non_writable: std::collections::HashSet<PropertyKey>,
    /// Tracks keys that are non-configurable
    pub non_configurable: std::collections::HashSet<PropertyKey>,
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

    /// Mark a property key as non-enumerable on this object
    pub fn set_non_enumerable(&mut self, key: PropertyKey) {
        self.non_enumerable.insert(key);
    }

    /// Mark a property key as non-writable on this object
    pub fn set_non_writable(&mut self, key: PropertyKey) {
        self.non_writable.insert(key);
    }

    /// Mark a property key as non-configurable on this object
    pub fn set_non_configurable(&mut self, key: PropertyKey) {
        self.non_configurable.insert(key);
    }

    /// Check whether a key is writable (default true)
    pub fn is_writable(&self, key: &PropertyKey) -> bool {
        !self.non_writable.contains(key)
    }

    /// Check whether a key is configurable (default true)
    pub fn is_configurable(&self, key: &PropertyKey) -> bool {
        !self.non_configurable.contains(key)
    }

    /// Check whether a key is enumerable (default true)
    pub fn is_enumerable(&self, key: &PropertyKey) -> bool {
        !self.non_enumerable.contains(key)
    }

    pub fn get(&self, key: &PropertyKey) -> Option<Rc<RefCell<Value>>> {
        self.properties.get(key).cloned()
    }

    pub fn contains_key(&self, key: &PropertyKey) -> bool {
        self.properties.contains_key(key)
    }

    pub fn remove(&mut self, key: &PropertyKey) -> Option<Rc<RefCell<Value>>> {
        self.properties.shift_remove(key)
    }

    pub fn keys(&self) -> indexmap::map::Keys<'_, PropertyKey, Rc<RefCell<Value>>> {
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

pub type ValuePtr = Rc<RefCell<Value>>;

#[derive(Clone, Debug)]
pub struct ClosureData {
    pub params: Vec<DestructuringElement>,
    pub body: Vec<Statement>,
    pub env: JSObjectDataPtr,
    pub home_object: RefCell<Option<JSObjectDataPtr>>,
    pub bound_this: Option<Value>,
}

impl ClosureData {
    pub fn new(params: &[DestructuringElement], body: &[Statement], env: &JSObjectDataPtr, home_object: Option<&JSObjectDataPtr>) -> Self {
        ClosureData {
            params: params.to_vec(),
            body: body.to_vec(),
            env: env.clone(),
            home_object: RefCell::new(home_object.cloned()),
            bound_this: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Number(f64),
    BigInt(BigInt),
    String(Vec<u16>), // UTF-16 code units
    Boolean(bool),
    Undefined,
    Null,
    Object(JSObjectDataPtr),                                          // Object with properties
    Function(String),                                                 // Function name
    Closure(Rc<ClosureData>),                                         // parameters, body, captured environment, home object
    AsyncClosure(Rc<ClosureData>),                                    // parameters, body, captured environment, home object
    GeneratorFunction(Option<String>, Rc<ClosureData>),               // optional name, parameters, body, captured environment, home object
    ClassDefinition(Rc<ClassDefinition>),                             // Class definition
    Getter(Vec<Statement>, JSObjectDataPtr, Option<JSObjectDataPtr>), // getter body, captured environment, home object
    Setter(Vec<DestructuringElement>, Vec<Statement>, JSObjectDataPtr, Option<JSObjectDataPtr>), // setter parameter, body, captured environment, home object
    Property {
        // Property descriptor with getter/setter/value
        value: Option<Rc<RefCell<Value>>>,
        getter: Option<(Vec<Statement>, JSObjectDataPtr, Option<JSObjectDataPtr>)>,
        #[allow(clippy::type_complexity)]
        setter: Option<(Vec<DestructuringElement>, Vec<Statement>, JSObjectDataPtr, Option<JSObjectDataPtr>)>,
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
    Uninitialized,                           // TDZ (Temporal Dead Zone) marker
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", value_to_string(self))
    }
}

pub fn is_truthy(val: &Value) -> bool {
    match val {
        Value::BigInt(b) => b != &BigInt::from(0),
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Boolean(b) => *b,
        Value::Undefined => false,
        Value::Null => false,
        Value::Uninitialized => false,
        Value::Object(_) => true,
        Value::Function(_) => true,
        Value::Closure(..) => true,
        Value::AsyncClosure(..) => true,
        Value::GeneratorFunction(..) => true,
        Value::ClassDefinition(_) => true,
        Value::Getter(..) => true,
        Value::Setter(..) => true,
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
        (Value::BigInt(sa), Value::BigInt(sb)) => sa == sb,
        (Value::Number(na), Value::Number(nb)) => na == nb,
        (Value::String(sa), Value::String(sb)) => sa == sb,
        (Value::Boolean(ba), Value::Boolean(bb)) => ba == bb,
        (Value::Undefined, Value::Undefined) => true,
        (Value::Null, Value::Null) => true,
        (Value::Uninitialized, Value::Uninitialized) => true,
        (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b), // Objects equal only if same reference
        (Value::Symbol(sa), Value::Symbol(sb)) => Rc::ptr_eq(sa, sb), // Symbols are equal if same reference
        (Value::Function(sa), Value::Function(sb)) => sa == sb,
        (Value::Closure(a), Value::Closure(b)) => Rc::ptr_eq(a, b),
        (Value::AsyncClosure(a), Value::AsyncClosure(b)) => Rc::ptr_eq(a, b),
        (Value::GeneratorFunction(_, a), Value::GeneratorFunction(_, b)) => Rc::ptr_eq(a, b),
        _ => false, // Different types are not equal
    }
}

// Helper function to convert value to string for display
pub fn value_to_string(val: &Value) -> String {
    match val {
        Value::Number(n) => n.to_string(),
        Value::BigInt(b) => format!("{b}n"),
        Value::String(s) => format!("\"{}\"", String::from_utf16_lossy(s)),
        Value::Boolean(b) => b.to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Uninitialized => "uninitialized".to_string(),
        Value::Object(obj) => {
            // Handle RegExp objects specially so they display as /pattern/flags
            if crate::js_regexp::is_regex_object(obj) {
                if let Ok(pat) = crate::js_regexp::get_regex_literal_pattern(obj) {
                    return pat;
                } else {
                    return "[object RegExp]".to_string();
                }
            }
            // Check if this is a function object (has __closure__ property)
            let has_closure = get_own_property(obj, &"__closure__".into()).is_some();
            log::trace!("DEBUG: has_closure = {has_closure}");
            if has_closure {
                "function".to_string()
            } else if let Some(length_rc) = get_own_property(obj, &"length".into()) {
                if let Value::Number(len) = &*length_rc.borrow() {
                    if len.is_finite() && *len >= 0.0 && len.fract() == 0.0 {
                        let len_usize = *len as usize;
                        let mut parts = Vec::new();
                        for i in 0..len_usize {
                            if let Some(val_rc) = get_own_property(obj, &i.to_string().into()) {
                                let val = val_rc.borrow();
                                let display = match *val {
                                    Value::Null => "null".to_string(),
                                    Value::Undefined => "undefined".to_string(),
                                    _ => value_to_string(&val),
                                };
                                parts.push(display);
                            } else {
                                parts.push("undefined".to_string());
                            }
                        }
                        format!("[{}]", parts.join(", "))
                    } else {
                        "[object Object]".to_string()
                    }
                } else {
                    "[object Object]".to_string()
                }
            } else {
                "[object Object]".to_string()
            }
        }
        Value::Function(name) => format!("function {}", name),
        Value::Closure(..) => "function".to_string(),
        Value::AsyncClosure(..) => "function".to_string(),
        Value::GeneratorFunction(..) => "function".to_string(),
        Value::ClassDefinition(_) => "class".to_string(),
        Value::Getter(..) => "getter".to_string(),
        Value::Setter(..) => "setter".to_string(),
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

// Helper: Check whether the given object has an own property corresponding to a
// given JS `Value` (as passed to hasOwnProperty / propertyIsEnumerable). This
// centralizes conversion from various `Value` variants (String/Number/Boolean/
// Undefined/Symbol/other) to a `PropertyKey` and calls `get_own_property`.
// Returns true if an own property exists.
pub fn has_own_property_value(obj: &JSObjectDataPtr, key_val: &Value) -> bool {
    match key_val {
        Value::String(s) => get_own_property(obj, &String::from_utf16_lossy(s).into()).is_some(),
        Value::Number(n) => get_own_property(obj, &n.to_string().into()).is_some(),
        Value::Boolean(b) => get_own_property(obj, &b.to_string().into()).is_some(),
        Value::Undefined => get_own_property(obj, &"undefined".into()).is_some(),
        Value::Symbol(sd) => {
            let sym_key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sd.clone()))));
            get_own_property(obj, &sym_key).is_some()
        }
        other => get_own_property(obj, &value_to_string(other).into()).is_some(),
    }
}

// Convert a Value into a PropertyKey suitable for use in property access
// (used when evaluating object literal property keys, computed keys, etc.).
// Symbols become Symbol keys; Strings/BigInts become String keys; other values
// are converted to a string representation.
pub fn value_to_property_key(val: &Value) -> PropertyKey {
    match val {
        Value::Symbol(sd) => PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sd.clone())))),
        Value::String(s) => PropertyKey::String(String::from_utf16_lossy(s)),
        Value::BigInt(b) => PropertyKey::String(b.to_string()),
        Value::Number(n) => PropertyKey::String(n.to_string()),
        Value::Boolean(b) => PropertyKey::String(b.to_string()),
        Value::Undefined => PropertyKey::String("undefined".to_string()),
        Value::Null => PropertyKey::String("null".to_string()),
        other => PropertyKey::String(value_to_string(other)),
    }
}

/// Helper: create a function call environment for invoking a closure
/// stored in `captured_env`, bind parameters (if provided), set `this`,
/// and optionally attach `__frame` and `__caller` for stack traces.
pub fn prepare_function_call_env(
    captured_env_opt: Option<&JSObjectDataPtr>,
    this_val_opt: Option<Value>,
    params_opt: Option<&[DestructuringElement]>,
    args: &[Value],
    frame_opt: Option<&str>,
    caller_env_opt: Option<&JSObjectDataPtr>,
) -> Result<JSObjectDataPtr, JSError> {
    let func_env = new_js_object_data();
    if let Some(captured_env) = captured_env_opt {
        func_env.borrow_mut().prototype = Some(captured_env.clone());
    }
    // mark this as a function scope so var-hoisting and env_set_var bind into this frame
    func_env.borrow_mut().is_function_scope = true;
    if let Some(this_val) = this_val_opt {
        obj_set_key_value(&func_env, &"this".into(), this_val)?;
    }
    if let Some(params) = params_opt {
        // bind params to provided args
        crate::core::bind_function_parameters(&func_env, params, args)?;
    }
    if let Some(frame) = frame_opt {
        let _ = obj_set_key_value(&func_env, &"__frame".into(), Value::String(utf8_to_utf16(frame)));
    }
    if let Some(caller) = caller_env_opt {
        let _ = obj_set_key_value(&func_env, &"__caller".into(), Value::Object(caller.clone()));
    }
    // If a caller environment was provided, copy its current statement
    // location into the new function env as `__call_line`/`__call_column`
    // so stack construction can reliably show the call-site.
    if let Some(caller) = caller_env_opt {
        if let Ok(Some(line_rc)) = obj_get_key_value(caller, &"__line".into()) {
            if let Value::Number(n) = &*line_rc.borrow() {
                let _ = obj_set_key_value(&func_env, &"__call_line".into(), Value::Number(*n));
            }
        }
        if let Ok(Some(col_rc)) = obj_get_key_value(caller, &"__column".into()) {
            if let Value::Number(n) = &*col_rc.borrow() {
                let _ = obj_set_key_value(&func_env, &"__call_column".into(), Value::Number(*n));
            }
        }
        if let Ok(Some(sn_rc)) = obj_get_key_value(caller, &"__script_name".into()) {
            if let Value::String(s) = &*sn_rc.borrow() {
                let _ = obj_set_key_value(&func_env, &"__call_script_name".into(), Value::String(s.clone()));
            }
        }
    }
    Ok(func_env)
}

// Helper: extract a closure (params, body, env) from a Value. This accepts
// either a direct `Value::Closure` or an object wrapper that stores the
// executable closure under the internal `"__closure__"` property.
#[allow(clippy::type_complexity)]
pub fn extract_closure_from_value(val: &Value) -> Option<(Vec<DestructuringElement>, Vec<Statement>, JSObjectDataPtr)> {
    match val {
        Value::Closure(data) => Some((data.params.clone(), data.body.clone(), data.env.clone())),
        Value::AsyncClosure(data) => Some((data.params.clone(), data.body.clone(), data.env.clone())),
        Value::GeneratorFunction(_, data) => Some((data.params.clone(), data.body.clone(), data.env.clone())),
        Value::Object(object) => {
            if let Ok(Some(cl_rc)) = obj_get_key_value(object, &"__closure__".into()) {
                match &*cl_rc.borrow() {
                    Value::Closure(data) => Some((data.params.clone(), data.body.clone(), data.env.clone())),
                    Value::AsyncClosure(data) => Some((data.params.clone(), data.body.clone(), data.env.clone())),
                    Value::GeneratorFunction(_, data) => Some((data.params.clone(), data.body.clone(), data.env.clone())),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

// Helper: perform ToPrimitive coercion with a given hint ('string', 'number', 'default')
pub fn to_primitive(val: &Value, hint: &str, env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match val {
        Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::Undefined | Value::Null | Value::Symbol(_) => Ok(val.clone()),
        Value::Object(object) => {
            // Prefer explicit [Symbol.toPrimitive] if present and callable
            if let Some(tp_sym) = get_well_known_symbol_rc("toPrimitive") {
                let key = PropertyKey::Symbol(tp_sym.clone());
                if let Some(method_rc) = obj_get_key_value(object, &key)? {
                    let method_val = method_rc.borrow().clone();
                    // Accept direct closures or function-objects that wrap a closure
                    if let Some((params, body, captured_env)) = extract_closure_from_value(&method_val) {
                        // Pass hint as first param if the function declares params
                        let args = vec![Value::String(utf8_to_utf16(hint))];
                        let func_env = prepare_function_call_env(
                            Some(&captured_env),
                            Some(Value::Object(object.clone())),
                            Some(&params),
                            &args,
                            None,
                            None,
                        )?;
                        let result = evaluate_statements(&func_env, &body)?;
                        match result {
                            Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::BigInt(_) | Value::Symbol(_) => {
                                return Ok(result);
                            }
                            _ => {
                                return Err(raise_type_error!("[Symbol.toPrimitive] must return a primitive"));
                            }
                        }
                    } else {
                        // Not a closure/minimally supported callable - fall through to default algorithm
                    }
                }
            }

            // Default algorithm: order depends on hint
            if hint == "string" {
                // toString -> valueOf
                let to_s = crate::js_object::handle_to_string_method(&Value::Object(object.clone()), &[], env)?;
                if matches!(to_s, Value::String(_) | Value::Number(_) | Value::Boolean(_) | Value::BigInt(_)) {
                    return Ok(to_s);
                }
                let val_of = crate::js_object::handle_value_of_method(&Value::Object(object.clone()), &[], env)?;
                if matches!(val_of, Value::String(_) | Value::Number(_) | Value::Boolean(_) | Value::BigInt(_)) {
                    return Ok(val_of);
                }
            } else {
                // number or default: valueOf -> toString
                let val_of = crate::js_object::handle_value_of_method(&Value::Object(object.clone()), &[], env)?;
                if matches!(val_of, Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::BigInt(_)) {
                    return Ok(val_of);
                }
                let to_s = crate::js_object::handle_to_string_method(&Value::Object(object.clone()), &[], env)?;
                if matches!(to_s, Value::String(_) | Value::Number(_) | Value::Boolean(_) | Value::BigInt(_)) {
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
        Value::BigInt(b) => b.to_string(),
        Value::String(s) => String::from_utf16_lossy(s),
        Value::Boolean(b) => b.to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Uninitialized => "undefined".to_string(),
        Value::Object(_) => "[object Object]".to_string(),
        Value::Function(name) => format!("[function {}]", name),
        Value::Closure(..) | Value::AsyncClosure(..) | Value::GeneratorFunction(..) => "[function]".to_string(),
        Value::ClassDefinition(_) => "[class]".to_string(),
        Value::Getter(..) => "[getter]".to_string(),
        Value::Setter(..) => "[setter]".to_string(),
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
pub fn obj_get_key_value(js_obj: &JSObjectDataPtr, key: &PropertyKey) -> Result<Option<Rc<RefCell<Value>>>, JSError> {
    // Check if this object is a proxy wrapper
    // Avoid holding a Ref borrow across calls by extracting the optional Rc first.
    let proxy_opt = get_own_property(js_obj, &"__proxy__".into());
    if let Some(proxy_val_rc) = proxy_opt {
        if let Value::Proxy(proxy) = &*proxy_val_rc.borrow() {
            return crate::js_proxy::proxy_get_property(proxy, key);
        }
    }

    // Search own properties and then walk the prototype chain until we find
    // a matching property or run out of prototypes. Use a visited set to detect prototype cycles.
    let mut visited: HashSet<*const RefCell<JSObjectData>> = HashSet::new();
    let mut current: Option<JSObjectDataPtr> = Some(js_obj.clone());
    while let Some(cur) = current {
        let ptr = Rc::as_ptr(&cur);
        if visited.contains(&ptr) {
            log::error!("Prototype chain cycle detected at ptr={:p}, breaking traversal", ptr);
            break;
        }
        visited.insert(ptr);
        let val_opt = get_own_property(&cur, key);
        if let Some(val) = val_opt {
            // Found an own/inherited value on `cur`. For getters we bind `this` to
            // the original object (`js_obj`) as per JS semantics.
            let val_clone = val.borrow().clone();
            match val_clone {
                Value::Property { value, getter, setter } => {
                    log::trace!("obj_get_key_value - property descriptor found for key {}", key);
                    if let Some((body, env, home_opt)) = getter {
                        // Create a new function environment with `this` bound to the original object
                        let getter_env = prepare_function_call_env(Some(&env), Some(Value::Object(js_obj.clone())), None, &[], None, None)?;
                        // If the getter is associated with a home object (class prototype), expose it
                        // on the getter environment so private field access (`this.#priv`) can be validated.
                        if let Some(home_obj) = home_opt {
                            crate::core::obj_set_key_value(&getter_env, &"__home_object__".into(), Value::Object(home_obj.clone()))?;
                        }
                        let result = evaluate_statements(&getter_env, &body)?;
                        if let Value::Object(ref obj_ptr) = result {
                            let ptr = Rc::as_ptr(obj_ptr);
                            log::trace!("obj_get_key_value - getter returned object ptr={:p} for key {}", ptr, key);
                        }
                        return Ok(Some(Rc::new(RefCell::new(result))));
                    } else if let Some(val_rc) = value {
                        // If the stored value is an object, log its pointer
                        if let Value::Object(ref obj_ptr) = *val_rc.borrow() {
                            log::trace!("obj_get_key_value - returning object ptr={:p} for key {}", Rc::as_ptr(obj_ptr), key);
                        }
                        return Ok(Some(val_rc));
                    } else if setter.is_some() {
                        // Accessor exists but no getter — write-only accessor
                        // Per ECMAScript spec, reading a write-only accessor returns undefined.
                        return Ok(Some(Rc::new(RefCell::new(Value::Undefined))));
                    } else {
                        return Ok(Some(Rc::new(RefCell::new(Value::Undefined))));
                    }
                }
                Value::Getter(body, env, _) => {
                    log::trace!("obj_get_key_value - getter found for key {}", key);
                    let getter_env = prepare_function_call_env(Some(&env), Some(Value::Object(js_obj.clone())), None, &[], None, None)?;
                    let result = evaluate_statements(&getter_env, &body)?;
                    if let Value::Object(ref obj_ptr) = result {
                        let ptr = Rc::as_ptr(obj_ptr);
                        log::trace!("obj_get_key_value - getter returned object ptr={:p} for key {}", ptr, key);
                    }
                    return Ok(Some(Rc::new(RefCell::new(result))));
                }
                // If we found a raw Setter value (not yet converted to a Property descriptor)
                // reading it should behave like a write-only accessor: throw a TypeError.
                Value::Setter(..) => {
                    // Raw Setter found (not normalized into a Property descriptor).
                    // Reading a setter-only property should return undefined per spec.
                    return Ok(Some(Rc::new(RefCell::new(Value::Undefined))));
                }
                _ => {
                    log::trace!("obj_get_key_value - raw value found for key {}", key);
                    if let Value::Object(ref obj_ptr) = *val.borrow() {
                        log::trace!("obj_get_key_value - returning object ptr={:p} for key {}", Rc::as_ptr(obj_ptr), key);
                    }
                    return Ok(Some(val.clone()));
                }
            }
        }
        // Not found on this object; continue with prototype.
        current = cur.borrow().prototype.clone();
    }

    // No own or inherited property found, fall back to special-case handling
    // (well-known symbol fallbacks, array/string iterator helpers, etc.).

    // Helper: build an iterator closure given the `next` function body and a
    // captured environment. This avoids duplicating the common pattern:
    //   function() { let __i = 0; return { next: function() { ... } } }
    fn make_iterator_closure(next_body: Vec<Statement>, captured_env: JSObjectDataPtr) -> Value {
        let iter_body = vec![
            Statement::from(StatementKind::Let(vec![("__i".to_string(), Some(Expr::Value(Value::Number(0.0))))])),
            Statement::from(StatementKind::Return(Some(Expr::Object(vec![(
                Expr::Value(Value::String(utf8_to_utf16("next"))),
                Expr::Function(None, Vec::new(), next_body),
                false,
            )])))),
        ];
        Value::Closure(Rc::new(ClosureData::new(&[], &iter_body, &captured_env, None)))
    }

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
                    Statement::from(StatementKind::Let(vec![(
                        "idx".to_string(),
                        Some(Expr::Var("__i".to_string(), None, None)),
                    )])),
                    Statement::from(StatementKind::If(
                        Expr::Binary(
                            Box::new(Expr::Var("idx".to_string(), None, None)),
                            BinaryOp::LessThan,
                            Box::new(Expr::Property(
                                Box::new(Expr::Var("__array".to_string(), None, None)),
                                "length".to_string(),
                            )),
                        ),
                        vec![
                            Statement::from(StatementKind::Let(vec![(
                                "v".to_string(),
                                Some(Expr::Index(
                                    Box::new(Expr::Var("__array".to_string(), None, None)),
                                    Box::new(Expr::Var("idx".to_string(), None, None)),
                                )),
                            )])),
                            Statement::from(StatementKind::Expr(Expr::Assign(
                                Box::new(Expr::Var("__i".to_string(), None, None)),
                                Box::new(Expr::Binary(
                                    Box::new(Expr::Var("idx".to_string(), None, None)),
                                    BinaryOp::Add,
                                    Box::new(Expr::Value(Value::Number(1.0))),
                                )),
                            ))),
                            Statement::from(StatementKind::Return(Some(Expr::Object(vec![
                                (
                                    Expr::Value(Value::String(utf8_to_utf16("value"))),
                                    Expr::Var("v".to_string(), None, None),
                                    false,
                                ),
                                (
                                    Expr::Value(Value::String(utf8_to_utf16("done"))),
                                    Expr::Value(Value::Boolean(false)),
                                    false,
                                ),
                            ])))),
                        ],
                        Some(vec![Statement::from(StatementKind::Return(Some(Expr::Object(vec![(
                            Expr::Value(Value::String(utf8_to_utf16("done"))),
                            Expr::Value(Value::Boolean(true)),
                            false,
                        )]))))]),
                    )),
                ];

                let captured_env = new_js_object_data();
                captured_env.borrow_mut().insert(
                    PropertyKey::String("__array".to_string()),
                    Rc::new(RefCell::new(Value::Object(js_obj.clone()))),
                );
                let closure = make_iterator_closure(next_body, captured_env.clone());
                return Ok(Some(Rc::new(RefCell::new(closure))));
            }

            // Map default iterator
            let map_opt = get_own_property(js_obj, &"__map__".into());
            if let Some(map_val) = map_opt {
                if let Value::Map(map_rc) = &*map_val.borrow() {
                    let map_entries = map_rc.borrow().entries.clone();

                    // next function body for Map iteration (returns [key, value] pairs)
                    let next_body = vec![
                        Statement::from(StatementKind::Let(vec![(
                            "idx".to_string(),
                            Some(Expr::Var("__i".to_string(), None, None)),
                        )])),
                        Statement::from(StatementKind::If(
                            Expr::Binary(
                                Box::new(Expr::Var("idx".to_string(), None, None)),
                                BinaryOp::LessThan,
                                Box::new(Expr::Value(Value::Number(map_entries.len() as f64))),
                            ),
                            vec![
                                Statement::from(StatementKind::Let(vec![(
                                    "entry".to_string(),
                                    Some(Expr::Array(vec![
                                        Some(Expr::Property(
                                            Box::new(Expr::Index(
                                                Box::new(Expr::Var("__entries".to_string(), None, None)),
                                                Box::new(Expr::Var("idx".to_string(), None, None)),
                                            )),
                                            "0".to_string(),
                                        )),
                                        Some(Expr::Property(
                                            Box::new(Expr::Index(
                                                Box::new(Expr::Var("__entries".to_string(), None, None)),
                                                Box::new(Expr::Var("idx".to_string(), None, None)),
                                            )),
                                            "1".to_string(),
                                        )),
                                    ])),
                                )])),
                                Statement::from(StatementKind::Expr(Expr::Assign(
                                    Box::new(Expr::Var("__i".to_string(), None, None)),
                                    Box::new(Expr::Binary(
                                        Box::new(Expr::Var("__i".to_string(), None, None)),
                                        BinaryOp::Add,
                                        Box::new(Expr::Value(Value::Number(1.0))),
                                    )),
                                ))),
                                Statement::from(StatementKind::Return(Some(Expr::Object(vec![
                                    (
                                        Expr::Value(Value::String(utf8_to_utf16("value"))),
                                        Expr::Var("entry".to_string(), None, None),
                                        false,
                                    ),
                                    (
                                        Expr::Value(Value::String(utf8_to_utf16("done"))),
                                        Expr::Value(Value::Boolean(false)),
                                        false,
                                    ),
                                ])))),
                            ],
                            Some(vec![Statement::from(StatementKind::Return(Some(Expr::Object(vec![(
                                Expr::Value(Value::String(utf8_to_utf16("done"))),
                                Expr::Value(Value::Boolean(true)),
                                false,
                            )]))))]),
                        )),
                    ];

                    let captured_env = new_js_object_data();
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

                    let closure = make_iterator_closure(next_body, captured_env.clone());
                    return Ok(Some(Rc::new(RefCell::new(closure))));
                }
            }

            // String default iterator
            if let Some(val_rc) = get_own_property(js_obj, &"__value__".into()) {
                if let Value::String(s) = &*val_rc.borrow() {
                    // next function body for string iteration (returns whole Unicode characters)
                    let next_body = vec![
                        Statement::from(StatementKind::Let(vec![(
                            "idx".to_string(),
                            Some(Expr::Var("__i".to_string(), None, None)),
                        )])),
                        // if idx < __str.length then proceed
                        Statement::from(StatementKind::If(
                            Expr::Binary(
                                Box::new(Expr::Var("idx".to_string(), None, None)),
                                BinaryOp::LessThan,
                                Box::new(Expr::Property(
                                    Box::new(Expr::Var("__str".to_string(), None, None)),
                                    "length".to_string(),
                                )),
                            ),
                            vec![
                                // first = __str.charCodeAt(idx)
                                Statement::from(StatementKind::Let(vec![(
                                    "first".to_string(),
                                    Some(Expr::Call(
                                        Box::new(Expr::Property(
                                            Box::new(Expr::Var("__str".to_string(), None, None)),
                                            "charCodeAt".to_string(),
                                        )),
                                        vec![Expr::Var("idx".to_string(), None, None)],
                                    )),
                                )])),
                                // if first is high surrogate (>=0xD800 && <=0xDBFF)
                                Statement::from(StatementKind::If(
                                    Expr::LogicalAnd(
                                        Box::new(Expr::Binary(
                                            Box::new(Expr::Var("first".to_string(), None, None)),
                                            BinaryOp::GreaterEqual,
                                            Box::new(Expr::Value(Value::Number(0xD800 as f64))),
                                        )),
                                        Box::new(Expr::Binary(
                                            Box::new(Expr::Var("first".to_string(), None, None)),
                                            BinaryOp::LessEqual,
                                            Box::new(Expr::Value(Value::Number(0xDBFF as f64))),
                                        )),
                                    ),
                                    vec![
                                        // second = __str.charCodeAt(idx + 1)
                                        Statement::from(StatementKind::Let(vec![(
                                            "second".to_string(),
                                            Some(Expr::Call(
                                                Box::new(Expr::Property(
                                                    Box::new(Expr::Var("__str".to_string(), None, None)),
                                                    "charCodeAt".to_string(),
                                                )),
                                                vec![Expr::Binary(
                                                    Box::new(Expr::Var("idx".to_string(), None, None)),
                                                    BinaryOp::Add,
                                                    Box::new(Expr::Value(Value::Number(1.0))),
                                                )],
                                            )),
                                        )])),
                                        // if second is low surrogate (>=0xDC00 && <=0xDFFF)
                                        Statement::from(StatementKind::If(
                                            Expr::LogicalAnd(
                                                Box::new(Expr::Binary(
                                                    Box::new(Expr::Var("second".to_string(), None, None)),
                                                    BinaryOp::GreaterEqual,
                                                    Box::new(Expr::Value(Value::Number(0xDC00 as f64))),
                                                )),
                                                Box::new(Expr::Binary(
                                                    Box::new(Expr::Var("second".to_string(), None, None)),
                                                    BinaryOp::LessEqual,
                                                    Box::new(Expr::Value(Value::Number(0xDFFF as f64))),
                                                )),
                                            ),
                                            vec![
                                                // ch = __str.substring(idx, idx+2)
                                                Statement::from(StatementKind::Let(vec![(
                                                    "ch".to_string(),
                                                    Some(Expr::Call(
                                                        Box::new(Expr::Property(
                                                            Box::new(Expr::Var("__str".to_string(), None, None)),
                                                            "substring".to_string(),
                                                        )),
                                                        vec![
                                                            Expr::Var("idx".to_string(), None, None),
                                                            Expr::Binary(
                                                                Box::new(Expr::Var("idx".to_string(), None, None)),
                                                                BinaryOp::Add,
                                                                Box::new(Expr::Value(Value::Number(2.0))),
                                                            ),
                                                        ],
                                                    )),
                                                )])),
                                                // __i = idx + 2
                                                Statement::from(StatementKind::Expr(Expr::Assign(
                                                    Box::new(Expr::Var("__i".to_string(), None, None)),
                                                    Box::new(Expr::Binary(
                                                        Box::new(Expr::Var("idx".to_string(), None, None)),
                                                        BinaryOp::Add,
                                                        Box::new(Expr::Value(Value::Number(2.0))),
                                                    )),
                                                ))),
                                                Statement::from(StatementKind::Return(Some(Expr::Object(vec![
                                                    (
                                                        Expr::Value(Value::String(utf8_to_utf16("value"))),
                                                        Expr::Var("ch".to_string(), None, None),
                                                        false,
                                                    ),
                                                    (
                                                        Expr::Value(Value::String(utf8_to_utf16("done"))),
                                                        Expr::Value(Value::Boolean(false)),
                                                        false,
                                                    ),
                                                ])))),
                                            ],
                                            // else: fallthrough to single-unit char
                                            None,
                                        )),
                                    ],
                                    // else: fallthrough to single-unit char
                                    None,
                                )),
                                // Single-unit char fallback: ch = __str.charAt(idx)
                                Statement::from(StatementKind::Let(vec![(
                                    "ch".to_string(),
                                    Some(Expr::Call(
                                        Box::new(Expr::Property(
                                            Box::new(Expr::Var("__str".to_string(), None, None)),
                                            "charAt".to_string(),
                                        )),
                                        vec![Expr::Var("idx".to_string(), None, None)],
                                    )),
                                )])),
                                Statement::from(StatementKind::Expr(Expr::Assign(
                                    Box::new(Expr::Var("__i".to_string(), None, None)),
                                    Box::new(Expr::Binary(
                                        Box::new(Expr::Var("idx".to_string(), None, None)),
                                        BinaryOp::Add,
                                        Box::new(Expr::Value(Value::Number(1.0))),
                                    )),
                                ))),
                                Statement::from(StatementKind::Return(Some(Expr::Object(vec![
                                    (
                                        Expr::Value(Value::String(utf8_to_utf16("value"))),
                                        Expr::Var("ch".to_string(), None, None),
                                        false,
                                    ),
                                    (
                                        Expr::Value(Value::String(utf8_to_utf16("done"))),
                                        Expr::Value(Value::Boolean(false)),
                                        false,
                                    ),
                                ])))),
                            ],
                            Some(vec![Statement::from(StatementKind::Return(Some(Expr::Object(vec![(
                                Expr::Value(Value::String(utf8_to_utf16("done"))),
                                Expr::Value(Value::Boolean(true)),
                                false,
                            )]))))]),
                        )),
                    ];

                    let captured_env = new_js_object_data();
                    captured_env.borrow_mut().insert(
                        PropertyKey::String("__str".to_string()),
                        Rc::new(RefCell::new(Value::String(s.clone()))),
                    );
                    let closure = make_iterator_closure(next_body, captured_env.clone());
                    return Ok(Some(Rc::new(RefCell::new(closure))));
                }
            }

            // Set default iterator
            let set_opt = get_own_property(js_obj, &"__set__".into());
            if let Some(set_val) = set_opt {
                if let Value::Set(set_rc) = &*set_val.borrow() {
                    let set_values = set_rc.borrow().values.clone();
                    // next function body for Set iteration (returns values)
                    let next_body = vec![
                        Statement::from(StatementKind::Let(vec![(
                            "idx".to_string(),
                            Some(Expr::Var("__i".to_string(), None, None)),
                        )])),
                        Statement::from(StatementKind::If(
                            Expr::Binary(
                                Box::new(Expr::Var("idx".to_string(), None, None)),
                                BinaryOp::LessThan,
                                Box::new(Expr::Value(Value::Number(set_values.len() as f64))),
                            ),
                            vec![
                                Statement::from(StatementKind::Let(vec![(
                                    "value".to_string(),
                                    Some(Expr::Index(
                                        Box::new(Expr::Var("__values".to_string(), None, None)),
                                        Box::new(Expr::Var("idx".to_string(), None, None)),
                                    )),
                                )])),
                                Statement::from(StatementKind::Expr(Expr::Assign(
                                    Box::new(Expr::Var("__i".to_string(), None, None)),
                                    Box::new(Expr::Binary(
                                        Box::new(Expr::Var("idx".to_string(), None, None)),
                                        BinaryOp::Add,
                                        Box::new(Expr::Value(Value::Number(1.0))),
                                    )),
                                ))),
                                Statement::from(StatementKind::Return(Some(Expr::Object(vec![
                                    (
                                        Expr::Value(Value::String(utf8_to_utf16("value"))),
                                        Expr::Var("value".to_string(), None, None),
                                        false,
                                    ),
                                    (
                                        Expr::Value(Value::String(utf8_to_utf16("done"))),
                                        Expr::Value(Value::Boolean(false)),
                                        false,
                                    ),
                                ])))),
                            ],
                            Some(vec![Statement::from(StatementKind::Return(Some(Expr::Object(vec![(
                                Expr::Value(Value::String(utf8_to_utf16("done"))),
                                Expr::Value(Value::Boolean(true)),
                                false,
                            )]))))]),
                        )),
                    ];

                    let set_iter_body = vec![
                        Statement::from(StatementKind::Let(vec![("__i".to_string(), Some(Expr::Value(Value::Number(0.0))))])),
                        Statement::from(StatementKind::Return(Some(Expr::Object(vec![(
                            Expr::Value(Value::String(utf8_to_utf16("next"))),
                            Expr::Function(None, Vec::new(), next_body),
                            false,
                        )])))),
                    ];

                    let captured_env = new_js_object_data();
                    // Store set values in the closure environment
                    let mut values_obj = JSObjectData::new();
                    for (i, value) in set_values.iter().enumerate() {
                        values_obj.insert(i.to_string().into(), Rc::new(RefCell::new(value.clone())));
                    }
                    captured_env.borrow_mut().insert(
                        "__values".into(),
                        Rc::new(RefCell::new(Value::Object(Rc::new(RefCell::new(values_obj))))),
                    );

                    let closure = Value::Closure(Rc::new(ClosureData::new(&[], &set_iter_body, &captured_env.clone(), None)));
                    return Ok(Some(Rc::new(RefCell::new(closure))));
                }
            }

            // Wrapped String iterator (for String objects)
            let wrapped_opt = get_own_property(js_obj, &"__value__".into());
            if let Some(wrapped) = wrapped_opt {
                if let Value::String(_) = &*wrapped.borrow() {
                    let next_body = vec![
                        Statement::from(StatementKind::Let(vec![(
                            "idx".to_string(),
                            Some(Expr::Var("__i".to_string(), None, None)),
                        )])),
                        Statement::from(StatementKind::If(
                            Expr::Binary(
                                Box::new(Expr::Var("idx".to_string(), None, None)),
                                BinaryOp::LessThan,
                                Box::new(Expr::Property(
                                    Box::new(Expr::Var("__s".to_string(), None, None)),
                                    "length".to_string(),
                                )),
                            ),
                            vec![
                                Statement::from(StatementKind::Let(vec![(
                                    "v".to_string(),
                                    Some(Expr::Index(
                                        Box::new(Expr::Var("__s".to_string(), None, None)),
                                        Box::new(Expr::Var("idx".to_string(), None, None)),
                                    )),
                                )])),
                                Statement::from(StatementKind::Expr(Expr::Assign(
                                    Box::new(Expr::Var("__i".to_string(), None, None)),
                                    Box::new(Expr::Binary(
                                        Box::new(Expr::Var("idx".to_string(), None, None)),
                                        BinaryOp::Add,
                                        Box::new(Expr::Value(Value::Number(1.0))),
                                    )),
                                ))),
                                Statement::from(StatementKind::Return(Some(Expr::Object(vec![
                                    (
                                        Expr::Value(Value::String(utf8_to_utf16("value"))),
                                        Expr::Var("v".to_string(), None, None),
                                        false,
                                    ),
                                    (
                                        Expr::Value(Value::String(utf8_to_utf16("done"))),
                                        Expr::Value(Value::Boolean(false)),
                                        false,
                                    ),
                                ])))),
                            ],
                            Some(vec![Statement::from(StatementKind::Return(Some(Expr::Object(vec![(
                                Expr::Value(Value::String(utf8_to_utf16("done"))),
                                Expr::Value(Value::Boolean(true)),
                                false,
                            )]))))]),
                        )),
                    ];

                    let str_iter_body = vec![
                        Statement::from(StatementKind::Let(vec![("__i".to_string(), Some(Expr::Value(Value::Number(0.0))))])),
                        Statement::from(StatementKind::Return(Some(Expr::Object(vec![(
                            Expr::Value(Value::String(utf8_to_utf16("next"))),
                            Expr::Function(None, Vec::new(), next_body),
                            false,
                        )])))),
                    ];

                    let captured_env = new_js_object_data();
                    captured_env.borrow_mut().insert(
                        PropertyKey::String("__s".to_string()),
                        Rc::new(RefCell::new(wrapped.borrow().clone())),
                    );
                    let closure = Value::Closure(Rc::new(ClosureData::new(&[], &str_iter_body, &captured_env.clone(), None)));
                    return Ok(Some(Rc::new(RefCell::new(closure))));
                }
            }
        }
        if let Some(tag_sym_rc) = get_well_known_symbol_rc("toStringTag")
            && let (Value::Symbol(tag_sd), Value::Symbol(req_sd)) = (&*tag_sym_rc.borrow(), &*sym_rc.borrow())
            && Rc::ptr_eq(tag_sd, req_sd)
        {
            if is_array(js_obj) {
                return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("Array"))))));
            }
            let wrapped_opt2 = get_own_property(js_obj, &"__value__".into());
            if let Some(wrapped) = wrapped_opt2 {
                match &*wrapped.borrow() {
                    Value::String(_) => return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("String")))))),
                    Value::Number(_) => return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("Number")))))),
                    Value::Boolean(_) => return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("Boolean")))))),
                    _ => {}
                }
            }
            if is_date_object(js_obj) {
                return Ok(Some(Rc::new(RefCell::new(Value::String(utf8_to_utf16("Date"))))));
            }
        }
    }

    Ok(None)
}

pub fn obj_set_key_value(js_obj: &JSObjectDataPtr, key: &PropertyKey, val: Value) -> Result<(), JSError> {
    // Check if this object is a proxy wrapper
    let proxy_opt = get_own_property(js_obj, &"__proxy__".into());
    if let Some(proxy_val) = proxy_opt {
        if let Value::Proxy(proxy) = &*proxy_val.borrow() {
            let success = crate::js_proxy::proxy_set_property(proxy, key, val)?;
            if !success {
                return Err(raise_eval_error!("Proxy set trap returned false"));
            }
            return Ok(());
        }
    }

    // Check if there's a setter for this property
    let existing_opt = get_own_property(js_obj, key);
    if let Some(existing) = existing_opt {
        // If property exists and is non-writable on this object, disallow assignment
        if !js_obj.borrow().is_writable(key) {
            return Err(raise_type_error!(format!("Cannot assign to read-only property '{}'", key)));
        }

        match existing.borrow().clone() {
            Value::Property { value: _, getter, setter } => {
                if let Some((param, body, env, home_opt)) = setter {
                    // Create a new function environment with 'this' bound to the object and bind parameter
                    let args_vals = vec![val];
                    let setter_env = prepare_function_call_env(
                        Some(&env),
                        Some(Value::Object(js_obj.clone())),
                        Some(&param),
                        &args_vals,
                        None,
                        None,
                    )?;
                    // If setter has an associated home object (class prototype), expose it
                    // so private field writes inside the setter can be validated.
                    if let Some(home_obj) = home_opt {
                        crate::core::obj_set_key_value(&setter_env, &"__home_object__".into(), Value::Object(home_obj.clone()))?;
                    }
                    let _v = evaluate_statements(&setter_env, &body)?;
                } else {
                    // No setter, update value
                    let value = Some(Rc::new(RefCell::new(val)));
                    let new_prop = Value::Property { value, getter, setter };
                    if let PropertyKey::String(s) = key {
                        // generic debug: avoid embedding any specific script identifiers
                        if s == "message" {
                            // Try to include any debug id set on the instance for correlation
                            let dbg_id = get_own_property(js_obj, &"__dbg_ptr__".into())
                                .map(|r| format!("{:?}", r.borrow()))
                                .unwrap_or_else(|| "<none>".to_string());
                            log::debug!(
                                "DBG obj_set_key_value - inserting 'message' on obj ptr={js_obj:p} dbg_id={dbg_id} value={new_prop:?}"
                            );
                        }
                    }
                    // Log the property insertion (object target pointer + value info)
                    let val_ptr_info = match &new_prop {
                        Value::Object(o) => format!("object_ptr={:p}", Rc::as_ptr(o)),
                        other => format!("value={:?}", other),
                    };
                    log::debug!(
                        "DBG obj_set_key_value - setting existing prop '{}' on obj ptr={:p} -> {}",
                        key,
                        Rc::as_ptr(js_obj),
                        val_ptr_info
                    );
                    js_obj.borrow_mut().insert(key.clone(), Rc::new(RefCell::new(new_prop)));
                }
                return Ok(());
            }
            Value::Setter(param, body, env, home_opt) => {
                // Create a new environment with this bound to the object and the parameter
                let setter_env = new_js_object_data();
                setter_env.borrow_mut().prototype = Some(env);
                if let Some(home_obj) = home_opt {
                    crate::core::obj_set_key_value(&setter_env, &"__home_object__".into(), Value::Object(home_obj.clone()))?;
                }
                env_set(&setter_env, "this", Value::Object(js_obj.clone()))?;
                let args = vec![val];
                crate::core::bind_function_parameters(&setter_env, &param, &args)?;
                evaluate_statements(&setter_env, &body)?;
                return Ok(());
            }
            _ => {}
        }
    }
    // No setter on the *own* property; check prototype chain for accessors.
    // If a setter exists on a prototype, call it; if the prototype has a getter
    // with no setter (read-only accessor), throw a TypeError (strict mode behavior).
    {
        let mut proto_opt = js_obj.borrow().prototype.clone();
        while let Some(proto) = proto_opt {
            if let Some(proto_prop_rc) = get_own_property(&proto, key) {
                match &*proto_prop_rc.borrow() {
                    Value::Property {
                        setter: Some((param, body, env, home_opt)),
                        ..
                    } => {
                        let args_vals = vec![val.clone()];
                        let setter_env = prepare_function_call_env(
                            Some(&env.clone()),
                            Some(Value::Object(js_obj.clone())),
                            Some(param),
                            &args_vals,
                            None,
                            None,
                        )?;
                        if let Some(home_obj) = home_opt {
                            crate::core::obj_set_key_value(&setter_env, &"__home_object__".into(), Value::Object(home_obj.clone()))?;
                        }
                        let _v = evaluate_statements(&setter_env, body)?;
                        return Ok(());
                    }
                    Value::Setter(param, body, env, home_opt) => {
                        let args_vals = vec![val.clone()];
                        let setter_env = prepare_function_call_env(
                            Some(&env.clone()),
                            Some(Value::Object(js_obj.clone())),
                            Some(param),
                            &args_vals,
                            None,
                            None,
                        )?;
                        if let Some(home_obj) = home_opt {
                            crate::core::obj_set_key_value(&setter_env, &"__home_object__".into(), Value::Object(home_obj.clone()))?;
                        }
                        evaluate_statements(&setter_env, body)?;
                        return Ok(());
                    }
                    Value::Property {
                        getter: Some(_),
                        setter: None,
                        ..
                    } => {
                        return Err(raise_type_error!(format!("Cannot assign to read-only property '{}'", key)));
                    }
                    _ => {}
                }
            }
            proto_opt = proto.borrow().prototype.clone();
        }
    }

    // Special handling for Array length property
    if let PropertyKey::String(s) = key {
        if s == "length" && is_array(js_obj) {
            let new_len_num = match &val {
                Value::Number(n) => *n,
                _ => return Err(raise_range_error!("Invalid array length")),
            };

            if new_len_num < 0.0 || new_len_num.fract() != 0.0 || new_len_num > u32::MAX as f64 {
                return Err(raise_range_error!("Invalid array length"));
            }

            let new_len = new_len_num as usize;

            let old_len = get_array_length(js_obj).unwrap_or(0);

            if new_len < old_len {
                let mut keys_to_remove = Vec::new();
                for k in js_obj.borrow().properties.keys() {
                    if let PropertyKey::String(ks) = k {
                        if let Ok(idx) = ks.parse::<usize>() {
                            if idx >= new_len {
                                keys_to_remove.push(k.clone());
                            }
                        }
                    }
                }
                for k in keys_to_remove {
                    js_obj.borrow_mut().remove(&k);
                }
            }
        }
    }

    // Update array length if setting an indexed property
    if let PropertyKey::String(s) = key {
        if let Ok(index) = s.parse::<usize>() {
            if let Some(current_len) = get_array_length(js_obj) {
                if index >= current_len {
                    set_array_length(js_obj, index + 1)?;
                }
            }
        }
    }
    if let PropertyKey::String(s) = key {
        if s == "message" {
            let dbg_id = get_own_property(js_obj, &"__dbg_ptr__".into())
                .map(|r| format!("{:?}", r.borrow()))
                .unwrap_or_else(|| "<none>".to_string());
            log::debug!("DBG obj_set_key_value - direct insert 'message' on obj ptr={js_obj:p} dbg_id={dbg_id} value={val:?}");
        }
    }
    // Log general property insertion (target pointer + value pointer/type) for diagnostics
    let val_ptr_info = match &val {
        Value::Object(o) => format!("object_ptr={:p}", Rc::as_ptr(o)),
        other => format!("value={:?}", other),
    };
    log::debug!(
        "DBG obj_set_key_value - direct insert prop '{}' on obj ptr={:p} -> {}",
        key,
        Rc::as_ptr(js_obj),
        val_ptr_info
    );
    js_obj.borrow_mut().insert(key.clone(), Rc::new(RefCell::new(val)));
    Ok(())
}

pub fn obj_set_rc(map: &JSObjectDataPtr, key: &PropertyKey, val_rc: Rc<RefCell<Value>>) {
    map.borrow_mut().insert(key.clone(), val_rc);
}

pub fn obj_delete(map: &JSObjectDataPtr, key: &PropertyKey) -> Result<bool, JSError> {
    // Check if this object is a proxy wrapper
    let proxy_opt = get_own_property(map, &"__proxy__".into());
    if let Some(proxy_val) = proxy_opt {
        if let Value::Proxy(proxy) = &*proxy_val.borrow() {
            return crate::js_proxy::proxy_delete_property(proxy, key);
        }
    }

    // If property is non-configurable, deletion fails (return false)
    if !map.borrow().is_configurable(key) {
        return Ok(false);
    }

    map.borrow_mut().remove(key);
    Ok(true)
}

pub fn env_get<T: AsRef<str>>(env: &JSObjectDataPtr, key: T) -> Option<Rc<RefCell<Value>>> {
    get_own_property(env, &key.as_ref().into())
}

/// Helper to get an own property Rc<Value> from an object without holding
/// the object's borrow across later inner borrows. This centralizes the
/// `obj.borrow().get(key)` pattern so callers can safely borrow the inner Rc.
pub fn get_own_property(obj: &JSObjectDataPtr, key: &PropertyKey) -> Option<Rc<RefCell<Value>>> {
    obj.borrow().get(key)
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
        if get_own_property(&current, &key_str.into()).is_some() {
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
            env_set(&current, key, val)?;
            current.borrow_mut().set_non_configurable(PropertyKey::String(key.to_string()));
            return Ok(());
        }
        let parent_opt = current.borrow().prototype.clone();
        if let Some(parent) = parent_opt {
            current = parent;
        } else {
            // If no function scope found, set in current env (global)
            env_set(env, key, val)?;
            env.borrow_mut().set_non_configurable(PropertyKey::String(key.to_string()));
            return Ok(());
        }
    }
}

pub fn env_set_const(env: &JSObjectDataPtr, key: &str, val: Value) {
    let mut env_mut = env.borrow_mut();
    env_mut.insert(PropertyKey::String(key.to_string()), Rc::new(RefCell::new(val)));
    env_mut.set_const(key.to_string());
    env_mut.set_non_configurable(PropertyKey::String(key.to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prototype_cycle_detection() {
        let a = new_js_object_data();
        let b = new_js_object_data();
        a.borrow_mut().prototype = Some(b.clone());
        b.borrow_mut().prototype = Some(a.clone());
        let res = obj_get_key_value(&a, &"nope".into()).unwrap();
        assert!(res.is_none());
    }
}
