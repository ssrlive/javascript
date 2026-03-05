use crate::Value;
use crate::core::opcode::{Chunk, Opcode};
use crate::core::value::value_to_string;
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
        let mut console_map = HashMap::new();
        console_map.insert("log".to_string(), Value::VmNativeFunction(BUILTIN_CONSOLE_LOG));
        console_map.insert("warn".to_string(), Value::VmNativeFunction(BUILTIN_CONSOLE_WARN));
        console_map.insert("error".to_string(), Value::VmNativeFunction(BUILTIN_CONSOLE_ERROR));
        self.globals
            .insert("console".to_string(), Value::VmObject(Rc::new(RefCell::new(console_map))));

        // Math object
        let mut math_map = HashMap::new();
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
            BUILTIN_ARRAY_PUSH => {
                // arg0 = array, rest = elements to push
                // handled directly in Call for method calls
                Value::Undefined
            }
            _ => {
                log::warn!("Unknown builtin ID: {}", id);
                Value::Undefined
            }
        }
    }

    /// Handle a thrown value: unwind to nearest try/catch or return error
    fn handle_throw(&mut self, thrown: Value<'gc>) -> Result<(), String> {
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
            Err(format!("Uncaught: {}", value_to_string(&thrown)))
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
                    match callee {
                        Value::VmFunction(target_ip, arity) => {
                            if arg_count as u8 != arity {
                                log::warn!("Arity mismatch: expected {}, got {}", arity, arg_count);
                            }
                            let frame = CallFrame {
                                return_ip: self.ip,
                                bp: callee_idx + 1,
                            };
                            self.frames.push(frame);
                            self.ip = target_ip;
                        }
                        Value::VmNativeFunction(id) => {
                            let args: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                            let result = self.call_builtin(id, args);
                            self.stack.pop(); // pop the callee
                            self.stack.push(result);
                        }
                        _ => {
                            log::warn!("Attempted to call non-function: {}", value_to_string(&callee));
                            self.stack.truncate(callee_idx);
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
            }
        }
    }
}
