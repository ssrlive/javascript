use crate::Value;
use crate::core::value::value_to_string;
use crate::core::opcode::{Chunk, Opcode};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;


#[derive(Debug, Clone)]
pub struct CallFrame {
    pub return_ip: usize,
    pub bp: usize, // Base pointer
}

/// Bytecode VM first stage prototype
pub struct VM<'gc> {
    chunk: Chunk<'gc>,
    ip: usize,                // Instruction Pointer: points to the currently executing byte
    stack: Vec<Value<'gc>>,   // Operand Stack
    globals: HashMap<String, Value<'gc>>, // Variables environment
    frames: Vec<CallFrame>,
}

impl<'gc> VM<'gc> {
    pub fn new(chunk: Chunk<'gc>) -> Self {
        Self {
            chunk,
            ip: 0,
            stack: Vec::with_capacity(256), // Reserve stack size
            globals: HashMap::new(),
            frames: Vec::new(),
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
    pub fn run(&mut self) -> Result<Value<'gc>, String> {
        loop {
            // Fetch instruction
            let instruction_byte = self.read_byte();
            let instruction = Opcode::from(instruction_byte);

            // Execute action based on instruction
            match instruction {
                Opcode::Return => {
                    let result = self.stack.pop().unwrap_or(Value::Undefined);
                    if let Some(frame) = self.frames.pop() {
                        // Returning from a function call: pop locals and the function itself
                        self.stack.truncate(frame.bp - 1);
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
                    let arg_count = self.read_byte() as usize;
                    // Stack: [..., callee, arg0, arg1, ...]
                    let callee_idx = self.stack.len() - arg_count - 1;
                    let callee = self.stack[callee_idx].clone();
                    if let Value::VmFunction(target_ip, arity) = callee {
                        if arg_count as u8 != arity {
                            log::warn!("Arity mismatch: expected {}, got {}", arity, arg_count);
                        }
                        let frame = CallFrame {
                            return_ip: self.ip,
                            bp: callee_idx + 1, // First argument sits right after the callee
                        };
                        self.frames.push(frame);
                        self.ip = target_ip;
                    } else {
                        log::warn!("Attempted to call non-function: {}", value_to_string(&callee));
                        self.stack.truncate(callee_idx);
                        self.stack.push(Value::Undefined);
                    }
                }
                Opcode::Constant => {
                    // Read constant pool index and push to stack
                    let constant_index = self.read_byte() as usize;
                    let constant = self.chunk.constants[constant_index].clone();
                    self.stack.push(constant);
                }
                Opcode::Pop => {
                    self.stack.pop();
                }
                Opcode::DefineGlobal => {
                    let name_idx = self.read_byte() as usize;
                    let name_val = &self.chunk.constants[name_idx];
                    if let Value::String(s) = name_val {
                        let name_str = crate::unicode::utf16_to_utf8(s);
                        let val = self.stack.pop().unwrap_or(Value::Undefined);
                        self.globals.insert(name_str, val);
                    }
                }
                Opcode::GetGlobal => {
                    let name_idx = self.read_byte() as usize;
                    let name_val = &self.chunk.constants[name_idx];
                    if let Value::String(s) = name_val {
                        let name_str = crate::unicode::utf16_to_utf8(s);
                        let val = self.globals.get(&name_str).cloned().unwrap_or(Value::Undefined);
                        self.stack.push(val);
                    }
                }
                Opcode::SetGlobal => {
                    let name_idx = self.read_byte() as usize;
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
                        _ => return Err("Unsupported types in VM Add".to_string()),
                    }
                }
                Opcode::Sub => {
                    let b = self.stack.pop().expect("VM Stack underflow on Sub (b)");
                    let a = self.stack.pop().expect("VM Stack underflow on Sub (a)");
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Number(a_num - b_num));
                        }
                        _ => return Err("Only numbers supported in VM Sub".to_string()),
                    }
                }
                Opcode::Mul => {
                    let b = self.stack.pop().expect("VM Stack underflow on Mul (b)");
                    let a = self.stack.pop().expect("VM Stack underflow on Mul (a)");
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Number(a_num * b_num));
                        }
                        _ => return Err("Only numbers supported in VM Mul".to_string()),
                    }
                }
                Opcode::Div => {
                    let b = self.stack.pop().expect("VM Stack underflow on Div (b)");
                    let a = self.stack.pop().expect("VM Stack underflow on Div (a)");
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Number(a_num / b_num));
                        }
                        _ => return Err("Only numbers supported in VM Div".to_string()),
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
                        (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined)
                        | (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null) => {
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
                        (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined)
                        | (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null) => {
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
                        _ => return Err("Only numbers supported in VM Mod".to_string()),
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
                    self.stack.push(Value::VmArray(Rc::new(RefCell::new(elems))));
                }
                Opcode::NewObject => {
                    let count = self.read_byte() as usize;
                    // Stack has pairs: [key, val, key, val, ...]
                    let start = self.stack.len() - count * 2;
                    let pairs: Vec<Value<'gc>> = self.stack.drain(start..).collect();
                    let mut map = HashMap::new();
                    for chunk in pairs.chunks(2) {
                        let key = value_to_string(&chunk[0]);
                        let val = chunk[1].clone();
                        map.insert(key, val);
                    }
                    self.stack.push(Value::VmObject(Rc::new(RefCell::new(map))));
                }
                Opcode::GetProperty => {
                    let name_idx = self.read_byte() as usize;
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
                        Value::VmArray(arr) => {
                            if key == "length" {
                                self.stack.push(Value::Number(arr.borrow().len() as f64));
                            } else {
                                self.stack.push(Value::Undefined);
                            }
                        }
                        _ => {
                            log::warn!("GetProperty on non-object: {}", value_to_string(&obj));
                            self.stack.push(Value::Undefined);
                        }
                    }
                }
                Opcode::SetProperty => {
                    let name_idx = self.read_byte() as usize;
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
            }
        }
    }
}
