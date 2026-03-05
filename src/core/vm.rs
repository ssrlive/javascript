use crate::core::opcode::{Chunk, Opcode};
use crate::core::value::{VmArrayData, value_to_string};
use crate::core::{JSError, Value};
use indexmap::IndexMap;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// Builtin function IDs
const BUILTIN_CONSOLE_LOG: u8 = 0;
const BUILTIN_CONSOLE_WARN: u8 = 1;
const BUILTIN_CONSOLE_ERROR: u8 = 2;
const BUILTIN_MATH_FLOOR: u8 = 3;
const BUILTIN_MATH_CEIL: u8 = 4;
const BUILTIN_MATH_ROUND: u8 = 5;
const BUILTIN_MATH_ABS: u8 = 6;
const BUILTIN_MATH_SQRT: u8 = 7;
const BUILTIN_MATH_MAX: u8 = 8;
const BUILTIN_MATH_MIN: u8 = 9;
const BUILTIN_ISNAN: u8 = 10;
const BUILTIN_PARSEINT: u8 = 11;
const BUILTIN_PARSEFLOAT: u8 = 12;
const BUILTIN_ARRAY_PUSH: u8 = 13;
const BUILTIN_ARRAY_POP: u8 = 14;
const BUILTIN_ARRAY_JOIN: u8 = 15;
const BUILTIN_ARRAY_INDEXOF: u8 = 16;
const BUILTIN_ARRAY_SLICE: u8 = 17;
const BUILTIN_ARRAY_CONCAT: u8 = 18;
const BUILTIN_ARRAY_MAP: u8 = 19;
const BUILTIN_ARRAY_FILTER: u8 = 20;
const BUILTIN_ARRAY_FOREACH: u8 = 21;
const BUILTIN_ARRAY_ISARRAY: u8 = 22;
const BUILTIN_STRING_SPLIT: u8 = 30;
const BUILTIN_STRING_INDEXOF: u8 = 31;
const BUILTIN_STRING_SLICE: u8 = 32;
const BUILTIN_STRING_TOUPPERCASE: u8 = 33;
const BUILTIN_STRING_TOLOWERCASE: u8 = 34;
const BUILTIN_STRING_TRIM: u8 = 35;
const BUILTIN_STRING_CHARAT: u8 = 36;
const BUILTIN_STRING_INCLUDES: u8 = 37;
const BUILTIN_STRING_REPLACE: u8 = 38;
const BUILTIN_STRING_STARTSWITH: u8 = 39;
const BUILTIN_STRING_ENDSWITH: u8 = 40;
const BUILTIN_STRING_SUBSTRING: u8 = 41;
const BUILTIN_JSON_STRINGIFY: u8 = 50;
const BUILTIN_JSON_PARSE: u8 = 51;
const BUILTIN_ARRAY_REDUCE: u8 = 52;

#[derive(Debug, Clone)]
pub struct CallFrame {
    pub return_ip: usize,
    pub bp: usize, // Base pointer
}

#[derive(Debug, Clone)]
pub struct TryFrame {
    pub catch_ip: usize,               // where to jump on throw
    pub stack_depth: usize,            // stack depth at try entry
    pub frame_depth: usize,            // call frame depth at try entry
    pub catch_binding: Option<String>, // variable name for caught value
}

/// Bytecode VM first stage prototype
pub struct VM<'gc> {
    chunk: Chunk<'gc>,
    ip: usize,
    stack: Vec<Value<'gc>>,
    globals: HashMap<String, Value<'gc>>,
    frames: Vec<CallFrame>,
    try_stack: Vec<TryFrame>,
    this_stack: Vec<Value<'gc>>, // this binding stack
    output: Vec<String>,         // captured output for console.log etc.
}

impl<'gc> VM<'gc> {
    pub fn new(chunk: Chunk<'gc>) -> Self {
        let mut vm = Self {
            chunk,
            ip: 0,
            stack: Vec::with_capacity(256),
            globals: HashMap::new(),
            frames: Vec::new(),
            try_stack: Vec::new(),
            this_stack: vec![Value::Undefined], // global this = undefined
            output: Vec::new(),
        };
        vm.register_builtins();
        vm
    }

    /// Get captured console output
    #[allow(dead_code)]
    pub fn take_output(&mut self) -> Vec<String> {
        std::mem::take(&mut self.output)
    }

    /// Register built-in global objects (console, Math, isNaN, parseInt, etc.)
    fn register_builtins(&mut self) {
        // console object
        let mut console_map = IndexMap::new();
        console_map.insert("log".to_string(), Value::VmNativeFunction(BUILTIN_CONSOLE_LOG));
        console_map.insert("warn".to_string(), Value::VmNativeFunction(BUILTIN_CONSOLE_WARN));
        console_map.insert("error".to_string(), Value::VmNativeFunction(BUILTIN_CONSOLE_ERROR));
        self.globals
            .insert("console".to_string(), Value::VmObject(Rc::new(RefCell::new(console_map))));

        // Math object
        let mut math_map = IndexMap::new();
        math_map.insert("floor".to_string(), Value::VmNativeFunction(BUILTIN_MATH_FLOOR));
        math_map.insert("ceil".to_string(), Value::VmNativeFunction(BUILTIN_MATH_CEIL));
        math_map.insert("round".to_string(), Value::VmNativeFunction(BUILTIN_MATH_ROUND));
        math_map.insert("abs".to_string(), Value::VmNativeFunction(BUILTIN_MATH_ABS));
        math_map.insert("sqrt".to_string(), Value::VmNativeFunction(BUILTIN_MATH_SQRT));
        math_map.insert("max".to_string(), Value::VmNativeFunction(BUILTIN_MATH_MAX));
        math_map.insert("min".to_string(), Value::VmNativeFunction(BUILTIN_MATH_MIN));
        math_map.insert("PI".to_string(), Value::Number(std::f64::consts::PI));
        math_map.insert("E".to_string(), Value::Number(std::f64::consts::E));
        self.globals
            .insert("Math".to_string(), Value::VmObject(Rc::new(RefCell::new(math_map))));

        // Global functions
        self.globals.insert("isNaN".to_string(), Value::VmNativeFunction(BUILTIN_ISNAN));
        self.globals
            .insert("parseInt".to_string(), Value::VmNativeFunction(BUILTIN_PARSEINT));
        self.globals
            .insert("parseFloat".to_string(), Value::VmNativeFunction(BUILTIN_PARSEFLOAT));

        // JSON object
        let mut json_map = IndexMap::new();
        json_map.insert("stringify".to_string(), Value::VmNativeFunction(BUILTIN_JSON_STRINGIFY));
        json_map.insert("parse".to_string(), Value::VmNativeFunction(BUILTIN_JSON_PARSE));
        self.globals
            .insert("JSON".to_string(), Value::VmObject(Rc::new(RefCell::new(json_map))));

        // Array.isArray
        let mut array_obj = IndexMap::new();
        array_obj.insert("isArray".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_ISARRAY));
        self.globals
            .insert("Array".to_string(), Value::VmObject(Rc::new(RefCell::new(array_obj))));
    }

    /// Execute a native/built-in function
    fn call_builtin(&mut self, id: u8, args: Vec<Value<'gc>>) -> Value<'gc> {
        match id {
            BUILTIN_CONSOLE_LOG | BUILTIN_CONSOLE_WARN | BUILTIN_CONSOLE_ERROR => {
                let parts: Vec<String> = args.iter().map(|v| value_to_string(v)).collect();
                let msg = parts.join(" ");
                self.output.push(msg.clone());
                // Match existing console behavior: print to stdout
                println!("{}", msg);
                Value::Undefined
            }
            BUILTIN_MATH_FLOOR => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.floor())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_CEIL => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.ceil())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_ROUND => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.round())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_ABS => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.abs())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_SQRT => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.sqrt())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_MAX => {
                let mut result = f64::NEG_INFINITY;
                for a in &args {
                    if let Value::Number(n) = a {
                        if *n > result {
                            result = *n;
                        }
                    } else {
                        return Value::Number(f64::NAN);
                    }
                }
                Value::Number(result)
            }
            BUILTIN_MATH_MIN => {
                let mut result = f64::INFINITY;
                for a in &args {
                    if let Value::Number(n) = a {
                        if *n < result {
                            result = *n;
                        }
                    } else {
                        return Value::Number(f64::NAN);
                    }
                }
                Value::Number(result)
            }
            BUILTIN_ISNAN => match args.first() {
                Some(Value::Number(n)) => Value::Boolean(n.is_nan()),
                Some(Value::Undefined) => Value::Boolean(true),
                _ => Value::Boolean(false),
            },
            BUILTIN_PARSEINT => {
                let s = args.first().map(value_to_string).unwrap_or_default();
                let trimmed = s.trim();
                match trimmed.parse::<f64>() {
                    Ok(n) => Value::Number(n.trunc()),
                    Err(_) => Value::Number(f64::NAN),
                }
            }
            BUILTIN_PARSEFLOAT => {
                let s = args.first().map(value_to_string).unwrap_or_default();
                match s.trim().parse::<f64>() {
                    Ok(n) => Value::Number(n),
                    Err(_) => Value::Number(f64::NAN),
                }
            }
            BUILTIN_ARRAY_PUSH => Value::Undefined,
            BUILTIN_ARRAY_ISARRAY => match args.first() {
                Some(Value::VmArray(_)) => Value::Boolean(true),
                _ => Value::Boolean(false),
            },
            BUILTIN_JSON_STRINGIFY => {
                let s = args.first().map(|v| self.json_stringify(v)).unwrap_or_default();
                Value::String(crate::unicode::utf8_to_utf16(&s))
            }
            BUILTIN_JSON_PARSE => {
                let s = args.first().map(value_to_string).unwrap_or_default();
                self.json_parse(&s)
            }
            _ => {
                log::warn!("Unknown builtin ID: {}", id);
                Value::Undefined
            }
        }
    }

    /// Execute a method call (receiver.method(args))
    fn call_method_builtin(&mut self, id: u8, receiver: Value<'gc>, args: Vec<Value<'gc>>) -> Value<'gc> {
        match id {
            // Also handle global builtins called as methods (e.g. console.log)
            BUILTIN_CONSOLE_LOG | BUILTIN_CONSOLE_WARN | BUILTIN_CONSOLE_ERROR => {
                return self.call_builtin(id, args);
            }
            BUILTIN_JSON_STRINGIFY
            | BUILTIN_JSON_PARSE
            | BUILTIN_ARRAY_ISARRAY
            | BUILTIN_MATH_FLOOR
            | BUILTIN_MATH_CEIL
            | BUILTIN_MATH_ROUND
            | BUILTIN_MATH_ABS
            | BUILTIN_MATH_SQRT
            | BUILTIN_MATH_MAX
            | BUILTIN_MATH_MIN
            | BUILTIN_ISNAN
            | BUILTIN_PARSEINT
            | BUILTIN_PARSEFLOAT => {
                return self.call_builtin(id, args);
            }
            _ => {}
        }

        // Array methods
        if let Value::VmArray(ref arr) = receiver {
            match id {
                BUILTIN_ARRAY_PUSH => {
                    let mut a = arr.borrow_mut();
                    for arg in &args {
                        a.push(arg.clone());
                    }
                    return Value::Number(a.len() as f64);
                }
                BUILTIN_ARRAY_POP => {
                    return arr.borrow_mut().pop().unwrap_or(Value::Undefined);
                }
                BUILTIN_ARRAY_JOIN => {
                    let sep = args.first().map(value_to_string).unwrap_or_else(|| ",".to_string());
                    let parts: Vec<String> = arr.borrow().iter().map(|v| value_to_string(v)).collect();
                    return Value::String(crate::unicode::utf8_to_utf16(&parts.join(&sep)));
                }
                BUILTIN_ARRAY_INDEXOF => {
                    let needle = args.first().cloned().unwrap_or(Value::Undefined);
                    let a = arr.borrow();
                    for (i, v) in a.iter().enumerate() {
                        if self.values_equal(v, &needle) {
                            return Value::Number(i as f64);
                        }
                    }
                    return Value::Number(-1.0);
                }
                BUILTIN_ARRAY_SLICE => {
                    let a = arr.borrow();
                    let len = a.len() as i64;
                    let start = match args.first() {
                        Some(Value::Number(n)) => {
                            let s = *n as i64;
                            if s < 0 { (len + s).max(0) as usize } else { s.min(len) as usize }
                        }
                        _ => 0,
                    };
                    let end = match args.get(1) {
                        Some(Value::Number(n)) => {
                            let e = *n as i64;
                            if e < 0 { (len + e).max(0) as usize } else { e.min(len) as usize }
                        }
                        _ => len as usize,
                    };
                    let sliced: Vec<Value<'gc>> = if start < end { a[start..end].to_vec() } else { Vec::new() };
                    return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(sliced))));
                }
                BUILTIN_ARRAY_CONCAT => {
                    let mut result = arr.borrow().clone();
                    for arg in &args {
                        if let Value::VmArray(other) = arg {
                            result.extend(other.borrow().iter().cloned());
                        } else {
                            result.push(arg.clone());
                        }
                    }
                    return Value::VmArray(Rc::new(RefCell::new(result)));
                }
                BUILTIN_ARRAY_MAP => {
                    if let Some(Value::VmFunction(ip, _arity)) = args.first() {
                        let elements = arr.borrow().elements.clone();
                        let mut result = Vec::new();
                        for (i, elem) in elements.iter().enumerate() {
                            let r = self.call_vm_function(*ip, &[elem.clone(), Value::Number(i as f64)]);
                            result.push(r);
                        }
                        return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(result))));
                    }
                    return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(Vec::new()))));
                }
                BUILTIN_ARRAY_FILTER => {
                    if let Some(Value::VmFunction(ip, _arity)) = args.first() {
                        let elements = arr.borrow().elements.clone();
                        let mut result = Vec::new();
                        for (i, elem) in elements.iter().enumerate() {
                            let r = self.call_vm_function(*ip, &[elem.clone(), Value::Number(i as f64)]);
                            if r.to_truthy() {
                                result.push(elem.clone());
                            }
                        }
                        return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(result))));
                    }
                    return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(Vec::new()))));
                }
                BUILTIN_ARRAY_FOREACH => {
                    if let Some(Value::VmFunction(ip, _arity)) = args.first() {
                        let elements = arr.borrow().elements.clone();
                        for (i, elem) in elements.iter().enumerate() {
                            self.call_vm_function(*ip, &[elem.clone(), Value::Number(i as f64)]);
                        }
                    }
                    return Value::Undefined;
                }
                BUILTIN_ARRAY_REDUCE => {
                    if let Some(Value::VmFunction(ip, _arity)) = args.first() {
                        let elements = arr.borrow().elements.clone();
                        let mut acc = if args.len() > 1 {
                            args[1].clone()
                        } else if !elements.is_empty() {
                            elements[0].clone()
                        } else {
                            Value::Undefined
                        };
                        let start_i = if args.len() > 1 { 0 } else { 1 };
                        for (i, element) in elements.iter().enumerate().skip(start_i) {
                            acc = self.call_vm_function(*ip, &[acc, element.clone(), Value::Number(i as f64)]);
                        }
                        return acc;
                    }
                    return Value::Undefined;
                }
                _ => {}
            }
        }

        // String methods
        if let Value::String(ref s) = receiver {
            let rust_str = crate::unicode::utf16_to_utf8(s);
            match id {
                BUILTIN_STRING_SPLIT => {
                    let sep = args.first().map(value_to_string).unwrap_or_default();
                    let parts: Vec<Value<'gc>> = if sep.is_empty() {
                        rust_str
                            .chars()
                            .map(|c| Value::String(crate::unicode::utf8_to_utf16(&c.to_string())))
                            .collect()
                    } else {
                        rust_str
                            .split(&sep)
                            .map(|p| Value::String(crate::unicode::utf8_to_utf16(p)))
                            .collect()
                    };
                    return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(parts))));
                }
                BUILTIN_STRING_INDEXOF => {
                    let needle = args.first().map(value_to_string).unwrap_or_default();
                    return match rust_str.find(&needle) {
                        Some(pos) => Value::Number(pos as f64),
                        None => Value::Number(-1.0),
                    };
                }
                BUILTIN_STRING_SLICE => {
                    let len = rust_str.len() as i64;
                    let start = match args.first() {
                        Some(Value::Number(n)) => {
                            let s = *n as i64;
                            if s < 0 { (len + s).max(0) as usize } else { s.min(len) as usize }
                        }
                        _ => 0,
                    };
                    let end = match args.get(1) {
                        Some(Value::Number(n)) => {
                            let e = *n as i64;
                            if e < 0 { (len + e).max(0) as usize } else { e.min(len) as usize }
                        }
                        _ => len as usize,
                    };
                    let sliced = if start < end { &rust_str[start..end] } else { "" };
                    return Value::String(crate::unicode::utf8_to_utf16(sliced));
                }
                BUILTIN_STRING_TOUPPERCASE => {
                    return Value::String(crate::unicode::utf8_to_utf16(&rust_str.to_uppercase()));
                }
                BUILTIN_STRING_TOLOWERCASE => {
                    return Value::String(crate::unicode::utf8_to_utf16(&rust_str.to_lowercase()));
                }
                BUILTIN_STRING_TRIM => {
                    return Value::String(crate::unicode::utf8_to_utf16(rust_str.trim()));
                }
                BUILTIN_STRING_CHARAT => {
                    let idx = match args.first() {
                        Some(Value::Number(n)) => *n as usize,
                        _ => 0,
                    };
                    let ch = rust_str.chars().nth(idx).map(|c| c.to_string()).unwrap_or_default();
                    return Value::String(crate::unicode::utf8_to_utf16(&ch));
                }
                BUILTIN_STRING_INCLUDES => {
                    let needle = args.first().map(value_to_string).unwrap_or_default();
                    return Value::Boolean(rust_str.contains(&needle));
                }
                BUILTIN_STRING_REPLACE => {
                    let pattern = args.first().map(value_to_string).unwrap_or_default();
                    let replacement = args.get(1).map(value_to_string).unwrap_or_default();
                    let result = rust_str.replacen(&pattern, &replacement, 1);
                    return Value::String(crate::unicode::utf8_to_utf16(&result));
                }
                BUILTIN_STRING_STARTSWITH => {
                    let prefix = args.first().map(value_to_string).unwrap_or_default();
                    return Value::Boolean(rust_str.starts_with(&prefix));
                }
                BUILTIN_STRING_ENDSWITH => {
                    let suffix = args.first().map(value_to_string).unwrap_or_default();
                    return Value::Boolean(rust_str.ends_with(&suffix));
                }
                BUILTIN_STRING_SUBSTRING => {
                    let len = rust_str.len() as i64;
                    let start = match args.first() {
                        Some(Value::Number(n)) => (*n as i64).max(0).min(len) as usize,
                        _ => 0,
                    };
                    let end = match args.get(1) {
                        Some(Value::Number(n)) => (*n as i64).max(0).min(len) as usize,
                        _ => len as usize,
                    };
                    let (s, e) = if start <= end { (start, end) } else { (end, start) };
                    return Value::String(crate::unicode::utf8_to_utf16(&rust_str[s..e]));
                }
                _ => {}
            }
        }

        log::warn!("Unknown method builtin ID {} on {}", id, value_to_string(&receiver));
        Value::Undefined
    }

    /// Compare two values for equality (used by indexOf etc.)
    fn values_equal(&self, a: &Value<'gc>, b: &Value<'gc>) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => x == y,
            (Value::String(x), Value::String(y)) => x == y,
            (Value::Boolean(x), Value::Boolean(y)) => x == y,
            (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => true,
            _ => false,
        }
    }

    /// Call a VM function inline (used by map/filter/forEach/reduce)
    fn call_vm_function(&mut self, ip: usize, args: &[Value<'gc>]) -> Value<'gc> {
        // Push a dummy callee so Return's truncate(bp-1) removes it
        self.stack.push(Value::Undefined);
        let bp = self.stack.len();
        for arg in args {
            self.stack.push(arg.clone());
        }
        let saved_ip = self.ip;
        let target_depth = self.frames.len();
        self.frames.push(CallFrame {
            return_ip: 0, // sentinel
            bp,
        });
        self.ip = ip;
        let result = self.run_inner(target_depth + 1);
        self.ip = saved_ip;
        result.unwrap_or(Value::Undefined)
    }

    /// JSON.stringify helper
    fn json_stringify(&self, val: &Value<'gc>) -> String {
        match val {
            Value::Number(n) => {
                if n.is_nan() || n.is_infinite() {
                    "null".to_string()
                } else if *n == (*n as i64) as f64 {
                    format!("{}", *n as i64)
                } else {
                    format!("{}", n)
                }
            }
            Value::String(s) => {
                let rust_str = crate::unicode::utf16_to_utf8(s);
                format!("\"{}\"", rust_str.replace('\\', "\\\\").replace('"', "\\\""))
            }
            Value::Boolean(b) => b.to_string(),
            Value::Null | Value::Undefined => "null".to_string(),
            Value::VmArray(arr) => {
                let parts: Vec<String> = arr.borrow().iter().map(|v| self.json_stringify(v)).collect();
                format!("[{}]", parts.join(","))
            }
            Value::VmObject(map) => {
                let m = map.borrow();
                let mut parts: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("\"{}\":{}", k.replace('\\', "\\\\").replace('"', "\\\""), self.json_stringify(v)))
                    .collect();
                parts.sort(); // deterministic output
                format!("{{{}}}", parts.join(","))
            }
            _ => "null".to_string(),
        }
    }

    /// JSON.parse helper (simple subset)
    fn json_parse(&self, s: &str) -> Value<'gc> {
        let trimmed = s.trim();
        if trimmed == "null" {
            return Value::Null;
        }
        if trimmed == "true" {
            return Value::Boolean(true);
        }
        if trimmed == "false" {
            return Value::Boolean(false);
        }
        if let Ok(n) = trimmed.parse::<f64>() {
            return Value::Number(n);
        }
        if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
            let inner = &trimmed[1..trimmed.len() - 1];
            return Value::String(crate::unicode::utf8_to_utf16(inner));
        }
        // For complex objects/arrays, fall back to undefined
        Value::Undefined
    }

    /// Handle a thrown value: unwind to nearest try/catch or return error
    fn handle_throw(&mut self, thrown: Value<'gc>) -> Result<(), JSError> {
        if let Some(try_frame) = self.try_stack.pop() {
            // Unwind stack and call frames
            self.stack.truncate(try_frame.stack_depth);
            self.frames.truncate(try_frame.frame_depth);
            self.ip = try_frame.catch_ip;
            // If catch has a binding, store thrown value as global
            if let Some(name) = try_frame.catch_binding {
                self.globals.insert(name, thrown);
            }
            Ok(())
        } else {
            Err(crate::raise_syntax_error!(format!("Uncaught: {}", value_to_string(&thrown))))
        }
    }

    /// Read a byte from the bytecode array and advance the IP
    fn read_byte(&mut self) -> u8 {
        let byte = self.chunk.code[self.ip];
        self.ip += 1;
        byte
    }

    /// Read a u16 from the bytecode array (little endian) and advance the IP
    fn read_u16(&mut self) -> u16 {
        let lo = self.read_byte() as u16;
        let hi = self.read_byte() as u16;
        (hi << 8) | lo
    }

    /// Core execution loop of the VM (Fetch-Decode-Execute)
    pub fn run(&mut self) -> Result<Value<'gc>, JSError> {
        self.run_inner(0)
    }

    /// Execute VM until frames drop below `min_depth` or top-level returns
    fn run_inner(&mut self, min_depth: usize) -> Result<Value<'gc>, JSError> {
        loop {
            // Fetch instruction
            let instruction_byte = self.read_byte();
            let instruction = Opcode::try_from(instruction_byte)?;

            // Execute action based on instruction
            match instruction {
                Opcode::Return => {
                    let result = self.stack.pop().unwrap_or(Value::Undefined);
                    if let Some(frame) = self.frames.pop() {
                        self.stack.truncate(frame.bp - 1);
                        if self.frames.len() < min_depth {
                            // Returning from an injected call (call_vm_function)
                            return Ok(result);
                        }
                        // Returning from a function call: pop locals and the function itself
                        self.stack.push(result);
                        self.ip = frame.return_ip;
                    } else {
                        // Return from top-level script
                        return Ok(result);
                    }
                }
                Opcode::GetLocal => {
                    let index = self.read_byte() as usize;
                    let bp = self.frames.last().map(|f| f.bp).unwrap_or(0);
                    let val = self.stack[bp + index].clone();
                    self.stack.push(val);
                }
                Opcode::SetLocal => {
                    let index = self.read_byte() as usize;
                    let bp = self.frames.last().map(|f| f.bp).unwrap_or(0);
                    let val = self.stack.last().expect("VM Stack underflow").clone();
                    self.stack[bp + index] = val;
                }
                Opcode::Call => {
                    let raw_arg_byte = self.read_byte();
                    let is_method = (raw_arg_byte & 0x80) != 0;
                    let arg_count = (raw_arg_byte & 0x7f) as usize;
                    // Stack for method call: [..., receiver, callee, arg0, arg1, ...]
                    // Stack for regular call: [..., callee, arg0, arg1, ...]
                    let callee_idx = self.stack.len() - arg_count - 1;
                    let callee = self.stack[callee_idx].clone();
                    match callee {
                        Value::VmFunction(target_ip, arity) => {
                            if arg_count as u8 != arity {
                                log::warn!("Arity mismatch: expected {}, got {}", arity, arg_count);
                            }
                            // For method calls, pop receiver from under callee
                            if is_method {
                                // Remove receiver (one slot below callee)
                                self.stack.remove(callee_idx - 1);
                                let callee_idx = callee_idx - 1;
                                let frame = CallFrame {
                                    return_ip: self.ip,
                                    bp: callee_idx + 1,
                                };
                                self.frames.push(frame);
                            } else {
                                let frame = CallFrame {
                                    return_ip: self.ip,
                                    bp: callee_idx + 1,
                                };
                                self.frames.push(frame);
                            }
                            self.ip = target_ip;
                        }
                        Value::VmNativeFunction(id) => {
                            let args: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                            self.stack.pop(); // pop the callee
                            if is_method {
                                let recv = self.stack.pop().unwrap_or(Value::Undefined);
                                let result = self.call_method_builtin(id, recv, args);
                                self.stack.push(result);
                            } else {
                                let result = self.call_builtin(id, args);
                                self.stack.push(result);
                            }
                        }
                        _ => {
                            log::warn!("Attempted to call non-function: {}", value_to_string(&callee));
                            let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                            self.stack.truncate(base);
                            self.stack.push(Value::Undefined);
                        }
                    }
                }
                Opcode::Constant => {
                    // Read constant pool index and push to stack
                    let constant_index = self.read_u16() as usize;
                    let constant = self.chunk.constants[constant_index].clone();
                    self.stack.push(constant);
                }
                Opcode::Pop => {
                    self.stack.pop();
                }
                Opcode::DefineGlobal => {
                    let name_idx = self.read_u16() as usize;
                    let name_val = &self.chunk.constants[name_idx];
                    if let Value::String(s) = name_val {
                        let name_str = crate::unicode::utf16_to_utf8(s);
                        let val = self.stack.pop().unwrap_or(Value::Undefined);
                        self.globals.insert(name_str, val);
                    }
                }
                Opcode::GetGlobal => {
                    let name_idx = self.read_u16() as usize;
                    let name_val = &self.chunk.constants[name_idx];
                    if let Value::String(s) = name_val {
                        let name_str = crate::unicode::utf16_to_utf8(s);
                        let val = self.globals.get(&name_str).cloned().unwrap_or(Value::Undefined);
                        self.stack.push(val);
                    }
                }
                Opcode::SetGlobal => {
                    let name_idx = self.read_u16() as usize;
                    let name_val = &self.chunk.constants[name_idx];
                    if let Value::String(s) = name_val {
                        let name_str = crate::unicode::utf16_to_utf8(s);
                        // Assignment leaves the value on the stack, so just peek
                        let val = self.stack.last().cloned().unwrap_or(Value::Undefined);
                        // In strict JS, assigning to undefined global throws. Here we just set or define.
                        self.globals.insert(name_str, val);
                    }
                }
                Opcode::Jump => {
                    let offset = self.read_u16();
                    self.ip = offset as usize;
                }
                Opcode::JumpIfFalse => {
                    let offset = self.read_u16();
                    let val = self.stack.pop().unwrap_or(Value::Undefined);
                    if !val.to_truthy() {
                        self.ip = offset as usize;
                    }
                }
                Opcode::Add => {
                    let b = self.stack.pop().expect("VM Stack underflow on Add (b)");
                    let a = self.stack.pop().expect("VM Stack underflow on Add (a)");
                    match (&a, &b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Number(a_num + b_num));
                        }
                        // String concatenation
                        (Value::String(a_str), Value::String(b_str)) => {
                            let mut result = a_str.clone();
                            result.extend_from_slice(b_str);
                            self.stack.push(Value::String(result));
                        }
                        // Mixed: coerce to string
                        (Value::String(a_str), _) => {
                            let b_s = crate::unicode::utf8_to_utf16(&value_to_string(&b));
                            let mut result = a_str.clone();
                            result.extend_from_slice(&b_s);
                            self.stack.push(Value::String(result));
                        }
                        (_, Value::String(b_str)) => {
                            let a_s = crate::unicode::utf8_to_utf16(&value_to_string(&a));
                            let mut result = a_s;
                            result.extend_from_slice(b_str);
                            self.stack.push(Value::String(result));
                        }
                        _ => return Err(crate::raise_syntax_error!("Unsupported types in VM Add")),
                    }
                }
                Opcode::Sub => {
                    let b = self.stack.pop().expect("VM Stack underflow on Sub (b)");
                    let a = self.stack.pop().expect("VM Stack underflow on Sub (a)");
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Number(a_num - b_num));
                        }
                        _ => return Err(crate::raise_syntax_error!("Only numbers supported in VM Sub")),
                    }
                }
                Opcode::Mul => {
                    let b = self.stack.pop().expect("VM Stack underflow on Mul (b)");
                    let a = self.stack.pop().expect("VM Stack underflow on Mul (a)");
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Number(a_num * b_num));
                        }
                        _ => return Err(crate::raise_syntax_error!("Only numbers supported in VM Mul")),
                    }
                }
                Opcode::Div => {
                    let b = self.stack.pop().expect("VM Stack underflow on Div (b)");
                    let a = self.stack.pop().expect("VM Stack underflow on Div (a)");
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Number(a_num / b_num));
                        }
                        _ => return Err(crate::raise_syntax_error!("Only numbers supported in VM Div")),
                    }
                }
                Opcode::LessThan => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Boolean(a_num < b_num));
                        }
                        _ => self.stack.push(Value::Boolean(false)), // Simplified for demo
                    }
                }
                Opcode::GreaterThan => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Boolean(a_num > b_num));
                        }
                        _ => self.stack.push(Value::Boolean(false)),
                    }
                }
                Opcode::Equal => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Boolean(a_num == b_num));
                        }
                        (Value::Boolean(a_bool), Value::Boolean(b_bool)) => {
                            self.stack.push(Value::Boolean(a_bool == b_bool));
                        }
                        (Value::String(ref a_s), Value::String(ref b_s)) => {
                            self.stack.push(Value::Boolean(a_s == b_s));
                        }
                        (Value::Null, Value::Null)
                        | (Value::Undefined, Value::Undefined)
                        | (Value::Null, Value::Undefined)
                        | (Value::Undefined, Value::Null) => {
                            self.stack.push(Value::Boolean(true));
                        }
                        _ => self.stack.push(Value::Boolean(false)),
                    }
                }
                Opcode::NotEqual => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Boolean(a_num != b_num));
                        }
                        (Value::Boolean(a_bool), Value::Boolean(b_bool)) => {
                            self.stack.push(Value::Boolean(a_bool != b_bool));
                        }
                        (Value::String(ref a_s), Value::String(ref b_s)) => {
                            self.stack.push(Value::Boolean(a_s != b_s));
                        }
                        (Value::Null, Value::Null)
                        | (Value::Undefined, Value::Undefined)
                        | (Value::Null, Value::Undefined)
                        | (Value::Undefined, Value::Null) => {
                            self.stack.push(Value::Boolean(false));
                        }
                        _ => self.stack.push(Value::Boolean(true)),
                    }
                }
                Opcode::StrictNotEqual => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Boolean(a_num != b_num));
                        }
                        (Value::Boolean(a_bool), Value::Boolean(b_bool)) => {
                            self.stack.push(Value::Boolean(a_bool != b_bool));
                        }
                        (Value::String(ref a_s), Value::String(ref b_s)) => {
                            self.stack.push(Value::Boolean(a_s != b_s));
                        }
                        _ => self.stack.push(Value::Boolean(true)),
                    }
                }
                Opcode::LessEqual => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Boolean(a_num <= b_num));
                        }
                        _ => self.stack.push(Value::Boolean(false)),
                    }
                }
                Opcode::GreaterEqual => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Boolean(a_num >= b_num));
                        }
                        _ => self.stack.push(Value::Boolean(false)),
                    }
                }
                Opcode::Mod => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Number(a_num % b_num));
                        }
                        _ => return Err(crate::raise_syntax_error!("Only numbers supported in VM Mod")),
                    }
                }
                Opcode::Negate => {
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match a {
                        Value::Number(n) => self.stack.push(Value::Number(-n)),
                        _ => self.stack.push(Value::Number(f64::NAN)),
                    }
                }
                Opcode::Not => {
                    let a = self.stack.pop().expect("VM Stack underflow");
                    self.stack.push(Value::Boolean(!a.to_truthy()));
                }
                Opcode::TypeOf => {
                    let a = self.stack.pop().expect("VM Stack underflow");
                    let type_str = match &a {
                        Value::Number(_) => "number",
                        Value::String(_) => "string",
                        Value::Boolean(_) => "boolean",
                        Value::Undefined => "undefined",
                        Value::Null => "object",
                        Value::VmFunction(..) | Value::Closure(..) | Value::Function(..) => "function",
                        _ => "object",
                    };
                    self.stack.push(Value::String(crate::unicode::utf8_to_utf16(type_str)));
                }
                Opcode::JumpIfTrue => {
                    let offset = self.read_u16();
                    let val = self.stack.pop().unwrap_or(Value::Undefined);
                    if val.to_truthy() {
                        self.ip = offset as usize;
                    }
                }
                Opcode::NewArray => {
                    let count = self.read_byte() as usize;
                    let start = self.stack.len() - count;
                    let elems: Vec<Value<'gc>> = self.stack.drain(start..).collect();
                    self.stack.push(Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(elems)))));
                }
                Opcode::NewObject => {
                    let count = self.read_byte() as usize;
                    // Stack has pairs: [key, val, key, val, ...]
                    let start = self.stack.len() - count * 2;
                    let pairs: Vec<Value<'gc>> = self.stack.drain(start..).collect();
                    let mut map = IndexMap::new();
                    for chunk in pairs.chunks(2) {
                        let key = value_to_string(&chunk[0]);
                        let val = chunk[1].clone();
                        map.insert(key, val);
                    }
                    self.stack.push(Value::VmObject(Rc::new(RefCell::new(map))));
                }
                Opcode::GetProperty => {
                    let name_idx = self.read_u16() as usize;
                    let name_val = &self.chunk.constants[name_idx];
                    let key = if let Value::String(s) = name_val {
                        crate::unicode::utf16_to_utf8(s)
                    } else {
                        value_to_string(name_val)
                    };
                    let obj = self.stack.pop().expect("VM Stack underflow on GetProperty");
                    match &obj {
                        Value::VmObject(map) => {
                            let val = map.borrow().get(&key).cloned().unwrap_or(Value::Undefined);
                            self.stack.push(val);
                        }
                        Value::VmArray(arr) => match key.as_str() {
                            "length" => self.stack.push(Value::Number(arr.borrow().len() as f64)),
                            "push" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_PUSH)),
                            "pop" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_POP)),
                            "join" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_JOIN)),
                            "indexOf" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_INDEXOF)),
                            "slice" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_SLICE)),
                            "concat" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_CONCAT)),
                            "map" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_MAP)),
                            "filter" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_FILTER)),
                            "forEach" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_FOREACH)),
                            "reduce" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_REDUCE)),
                            _ => {
                                // Check custom named properties
                                let val = arr.borrow().props.get(&key).cloned().unwrap_or(Value::Undefined);
                                self.stack.push(val);
                            }
                        },
                        Value::String(_) => match key.as_str() {
                            "length" => {
                                if let Value::String(s) = &obj {
                                    self.stack.push(Value::Number(s.len() as f64));
                                }
                            }
                            "split" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_SPLIT)),
                            "indexOf" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_INDEXOF)),
                            "slice" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_SLICE)),
                            "toUpperCase" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_TOUPPERCASE)),
                            "toLowerCase" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_TOLOWERCASE)),
                            "trim" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_TRIM)),
                            "charAt" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_CHARAT)),
                            "includes" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_INCLUDES)),
                            "replace" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_REPLACE)),
                            "startsWith" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_STARTSWITH)),
                            "endsWith" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_ENDSWITH)),
                            "substring" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_SUBSTRING)),
                            _ => self.stack.push(Value::Undefined),
                        },
                        _ => {
                            log::warn!("GetProperty on non-object: {}", value_to_string(&obj));
                            self.stack.push(Value::Undefined);
                        }
                    }
                }
                Opcode::SetProperty => {
                    let name_idx = self.read_u16() as usize;
                    let name_val = &self.chunk.constants[name_idx];
                    let key = if let Value::String(s) = name_val {
                        crate::unicode::utf16_to_utf8(s)
                    } else {
                        value_to_string(name_val)
                    };
                    let val = self.stack.pop().expect("VM Stack underflow on SetProperty (val)");
                    let obj = self.stack.pop().expect("VM Stack underflow on SetProperty (obj)");
                    if let Value::VmObject(map) = &obj {
                        map.borrow_mut().insert(key, val.clone());
                    } else if let Value::VmArray(arr) = &obj {
                        arr.borrow_mut().props.insert(key, val.clone());
                    } else {
                        log::warn!("SetProperty on non-object: {}", value_to_string(&obj));
                    }
                    // Leave the assigned value on stack
                    self.stack.push(val);
                }
                Opcode::GetIndex => {
                    let index = self.stack.pop().expect("VM Stack underflow on GetIndex (index)");
                    let obj = self.stack.pop().expect("VM Stack underflow on GetIndex (obj)");
                    match &obj {
                        Value::VmArray(arr) => {
                            if let Value::Number(n) = &index {
                                let i = *n as usize;
                                let val = arr.borrow().get(i).cloned().unwrap_or(Value::Undefined);
                                self.stack.push(val);
                            } else {
                                self.stack.push(Value::Undefined);
                            }
                        }
                        Value::VmObject(map) => {
                            let key = value_to_string(&index);
                            let val = map.borrow().get(&key).cloned().unwrap_or(Value::Undefined);
                            self.stack.push(val);
                        }
                        _ => {
                            log::warn!("GetIndex on non-indexable: {}", value_to_string(&obj));
                            self.stack.push(Value::Undefined);
                        }
                    }
                }
                Opcode::SetIndex => {
                    let val = self.stack.pop().expect("VM Stack underflow on SetIndex (val)");
                    let index = self.stack.pop().expect("VM Stack underflow on SetIndex (index)");
                    let obj = self.stack.pop().expect("VM Stack underflow on SetIndex (obj)");
                    match &obj {
                        Value::VmArray(arr) => {
                            if let Value::Number(n) = &index {
                                let i = *n as usize;
                                let mut a = arr.borrow_mut();
                                // Grow array if needed
                                while a.len() <= i {
                                    a.push(Value::Undefined);
                                }
                                a[i] = val.clone();
                            }
                        }
                        Value::VmObject(map) => {
                            let key = value_to_string(&index);
                            map.borrow_mut().insert(key, val.clone());
                        }
                        _ => {
                            log::warn!("SetIndex on non-indexable: {}", value_to_string(&obj));
                        }
                    }
                    self.stack.push(val);
                }
                Opcode::Increment => {
                    let a = self.stack.pop().expect("VM Stack underflow on Increment");
                    match a {
                        Value::Number(n) => self.stack.push(Value::Number(n + 1.0)),
                        _ => self.stack.push(Value::Number(f64::NAN)),
                    }
                }
                Opcode::Decrement => {
                    let a = self.stack.pop().expect("VM Stack underflow on Decrement");
                    match a {
                        Value::Number(n) => self.stack.push(Value::Number(n - 1.0)),
                        _ => self.stack.push(Value::Number(f64::NAN)),
                    }
                }
                Opcode::Throw => {
                    let thrown = self.stack.pop().unwrap_or(Value::Undefined);
                    self.handle_throw(thrown)?;
                }
                Opcode::SetupTry => {
                    let catch_ip = self.read_u16() as usize;
                    let binding_idx = self.read_u16();
                    let catch_binding = if binding_idx == 0xffff {
                        None
                    } else {
                        let name_val = &self.chunk.constants[binding_idx as usize];
                        if let Value::String(s) = name_val {
                            Some(crate::unicode::utf16_to_utf8(s))
                        } else {
                            None
                        }
                    };
                    self.try_stack.push(TryFrame {
                        catch_ip,
                        stack_depth: self.stack.len(),
                        frame_depth: self.frames.len(),
                        catch_binding,
                    });
                }
                Opcode::TeardownTry => {
                    self.try_stack.pop();
                }
                Opcode::GetThis => {
                    let this_val = self.this_stack.last().cloned().unwrap_or(Value::Undefined);
                    self.stack.push(this_val);
                }
                Opcode::GetKeys => {
                    let obj = self.stack.pop().expect("VM Stack underflow on GetKeys");
                    let keys = match &obj {
                        Value::VmObject(map) => map
                            .borrow()
                            .keys()
                            .map(|k| Value::String(crate::unicode::utf8_to_utf16(k)))
                            .collect(),
                        Value::VmArray(arr) => {
                            let a = arr.borrow();
                            let mut k: Vec<Value<'gc>> = (0..a.elements.len())
                                .map(|i| Value::String(crate::unicode::utf8_to_utf16(&i.to_string())))
                                .collect();
                            for prop_key in a.props.keys() {
                                k.push(Value::String(crate::unicode::utf8_to_utf16(prop_key)));
                            }
                            k
                        }
                        _ => Vec::new(),
                    };
                    self.stack.push(Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(keys)))));
                }
                Opcode::GetMethod => {
                    // Stack: [..., obj] -> [..., obj, method]
                    // Peek at object on TOS, resolve method, push on top
                    let name_idx = self.read_u16() as usize;
                    let name_val = &self.chunk.constants[name_idx];
                    let key = if let Value::String(s) = name_val {
                        crate::unicode::utf16_to_utf8(s)
                    } else {
                        value_to_string(name_val)
                    };
                    let obj = self.stack.last().expect("VM Stack underflow on GetMethod");
                    let method = match obj {
                        Value::VmObject(map) => map.borrow().get(&key).cloned().unwrap_or(Value::Undefined),
                        Value::VmArray(_arr) => match key.as_str() {
                            "push" => Value::VmNativeFunction(BUILTIN_ARRAY_PUSH),
                            "pop" => Value::VmNativeFunction(BUILTIN_ARRAY_POP),
                            "join" => Value::VmNativeFunction(BUILTIN_ARRAY_JOIN),
                            "indexOf" => Value::VmNativeFunction(BUILTIN_ARRAY_INDEXOF),
                            "slice" => Value::VmNativeFunction(BUILTIN_ARRAY_SLICE),
                            "concat" => Value::VmNativeFunction(BUILTIN_ARRAY_CONCAT),
                            "map" => Value::VmNativeFunction(BUILTIN_ARRAY_MAP),
                            "filter" => Value::VmNativeFunction(BUILTIN_ARRAY_FILTER),
                            "forEach" => Value::VmNativeFunction(BUILTIN_ARRAY_FOREACH),
                            "reduce" => Value::VmNativeFunction(BUILTIN_ARRAY_REDUCE),
                            _ => Value::Undefined,
                        },
                        Value::String(_) => match key.as_str() {
                            "split" => Value::VmNativeFunction(BUILTIN_STRING_SPLIT),
                            "indexOf" => Value::VmNativeFunction(BUILTIN_STRING_INDEXOF),
                            "slice" => Value::VmNativeFunction(BUILTIN_STRING_SLICE),
                            "toUpperCase" => Value::VmNativeFunction(BUILTIN_STRING_TOUPPERCASE),
                            "toLowerCase" => Value::VmNativeFunction(BUILTIN_STRING_TOLOWERCASE),
                            "trim" => Value::VmNativeFunction(BUILTIN_STRING_TRIM),
                            "charAt" => Value::VmNativeFunction(BUILTIN_STRING_CHARAT),
                            "includes" => Value::VmNativeFunction(BUILTIN_STRING_INCLUDES),
                            "replace" => Value::VmNativeFunction(BUILTIN_STRING_REPLACE),
                            "startsWith" => Value::VmNativeFunction(BUILTIN_STRING_STARTSWITH),
                            "endsWith" => Value::VmNativeFunction(BUILTIN_STRING_ENDSWITH),
                            "substring" => Value::VmNativeFunction(BUILTIN_STRING_SUBSTRING),
                            _ => Value::Undefined,
                        },
                        _ => Value::Undefined,
                    };
                    self.stack.push(method);
                }
                Opcode::NewError => {
                    // Pop message from stack, create VmObject { message: msg }
                    let msg = self.stack.pop().unwrap_or(Value::Undefined);
                    let mut map = IndexMap::new();
                    map.insert("message".to_string(), msg);
                    self.stack.push(Value::VmObject(Rc::new(RefCell::new(map))));
                }
            }
        }
    }
}
