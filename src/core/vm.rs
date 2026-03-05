use crate::Value;
use crate::core::opcode::{Chunk, Opcode};

/// Bytecode VM first stage prototype
pub struct VM<'gc> {
    chunk: Chunk<'gc>,
    ip: usize,                // Instruction Pointer: points to the currently executing byte
    stack: Vec<Value<'gc>>,   // Operand Stack
}

impl<'gc> VM<'gc> {
    pub fn new(chunk: Chunk<'gc>) -> Self {
        Self {
            chunk,
            ip: 0,
            stack: Vec::with_capacity(256), // Reserve stack size
        }
    }

    /// Read a byte from the bytecode array and advance the IP
    fn read_byte(&mut self) -> u8 {
        let byte = self.chunk.code[self.ip];
        self.ip += 1;
        byte
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
                    // Return top of stack if available, otherwise return Undefined
                    return Ok(self.stack.pop().unwrap_or(Value::Undefined));
                }
                Opcode::Constant => {
                    // Read constant pool index and push to stack
                    let constant_index = self.read_byte() as usize;
                    let constant = self.chunk.constants[constant_index].clone();
                    self.stack.push(constant);
                }
                Opcode::Add => {
                    // Pop right and left operands (order: pop right first, then left)
                    let b = self.stack.pop().expect("VM Stack underflow on Add (b)");
                    let a = self.stack.pop().expect("VM Stack underflow on Add (a)");

                    match (a, b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            // Perform actual addition and push result back to stack
                            self.stack.push(Value::Number(a_num + b_num));
                        }
                        // If there are other types (strings, etc.), ToPrimitive conversion will be needed later. This is just for demo.
                        _ => return Err("Only numbers can be added in this basic VM demo".to_string()),
                    }
                }
            }
        }
    }
}

/// Simple 1 + 2 test entry
pub fn run_vm_demo<'gc>() {
    let mut chunk = Chunk::new();

    log::warn!("[VM Demo] Assembling Bytecode for: 1 + 2");

    // Add constant 1.0 to constant pool and get index
    let constant_1_index = chunk.add_constant(Value::Number(1.0));
    // Add constant 2.0 to constant pool and get index
    let constant_2_index = chunk.add_constant(Value::Number(2.0));

    // Emit instruction: Load Constant 1
    chunk.write_opcode(Opcode::Constant);
    chunk.write_byte(constant_1_index);

    // Emit instruction: Load Constant 2
    chunk.write_opcode(Opcode::Constant);
    chunk.write_byte(constant_2_index);

    // Emit instruction: Add
    chunk.write_opcode(Opcode::Add);

    // Emit instruction: Return
    chunk.write_opcode(Opcode::Return);

    // Start the virtual machine!
    let mut vm = VM::new(chunk);
    
    log::warn!("========================================");
    log::warn!("[VM] Starting execution...");
    match vm.run() {
        Ok(result) => {
            if let Value::Number(n) = result {
                log::warn!("[VM] Execution Finished! Result: {}", n);
            }
        },
        Err(e) => log::warn!("[VM] Error: {}", e),
    }
    log::warn!("========================================");
}
