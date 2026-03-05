use crate::Value;
use crate::core::value::value_to_string;
use crate::core::opcode::{Chunk, Opcode};
use std::collections::HashMap;


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
                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Number(a_num + b_num));
                        }
                        _ => return Err("Only numbers supported in VM Add".to_string()),
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
                        _ => self.stack.push(Value::Boolean(false)), // Strict equal for diff types in this demo
                    }
                }
            }
        }
    }
}
