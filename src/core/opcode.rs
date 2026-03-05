use crate::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    Return = 0,
    Constant = 1,
    Add = 2,
    Sub = 3,
    Mul = 4,
    Div = 5,
    Pop = 6,
    DefineGlobal = 7,
    GetGlobal = 8,
    SetGlobal = 9,
    Jump = 10,
    JumpIfFalse = 11,
    LessThan = 12,
    GreaterThan = 13,
    Equal = 14,
}

impl From<u8> for Opcode {
    fn from(byte: u8) -> Self {
        match byte {
            0 => Opcode::Return,
            1 => Opcode::Constant,
            2 => Opcode::Add,
            3 => Opcode::Sub,
            4 => Opcode::Mul,
            5 => Opcode::Div,
            6 => Opcode::Pop,
            7 => Opcode::DefineGlobal,
            8 => Opcode::GetGlobal,
            9 => Opcode::SetGlobal,
            10 => Opcode::Jump,
            11 => Opcode::JumpIfFalse,
            12 => Opcode::LessThan,
            13 => Opcode::GreaterThan,
            14 => Opcode::Equal,
            _ => panic!("Unknown opcode: {}", byte),
        }
    }
}

/// Bytecode chunk (stores instruction array and constant pool)
#[derive(Debug, Clone)]
pub struct Chunk<'gc> {
    pub code: Vec<u8>,
    pub constants: Vec<Value<'gc>>,
}

impl<'gc> Chunk<'gc> {
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            constants: Vec::new(),
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        self.code.push(byte);
    }

    pub fn write_opcode(&mut self, opcode: Opcode) {
        self.write_byte(opcode as u8);
    }
    
    pub fn write_u16(&mut self, value: u16) {
        self.code.push((value & 0xff) as u8);
        self.code.push(((value >> 8) & 0xff) as u8);
    }
    
    pub fn add_constant(&mut self, value: Value<'gc>) -> u8 {
        self.constants.push(value);
        (self.constants.len() - 1) as u8
    }
}
