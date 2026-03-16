use crate::core::{JSError, Value};

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
    Call = 15,
    GetLocal = 16,
    SetLocal = 17,
    Negate = 18,
    Not = 19,
    TypeOf = 20,
    Mod = 21,
    LessEqual = 22,
    GreaterEqual = 23,
    NotEqual = 24,
    StrictNotEqual = 25,
    JumpIfTrue = 26,
    NewArray = 27,
    NewObject = 28,
    GetProperty = 29,
    SetProperty = 30,
    GetIndex = 31,
    SetIndex = 32,
    Increment = 33,
    Decrement = 34,
    Throw = 35,
    SetupTry = 36,
    TeardownTry = 37,
    GetThis = 38,
    GetKeys = 39,          // pop object, push array of its string keys
    GetMethod = 40,        // peek object (keep on stack), push method value on top
    NewError = 41,         // pop message string, push VmObject { message }
    Dup = 42,              // duplicate top of stack
    In = 43,               // pop key and object, push bool (key in object)
    InstanceOf = 44,       // pop constructor and value, push bool
    DeleteProperty = 45,   // pop object, read constant key, delete, push bool
    NewCall = 46,          // new Constructor(args): create obj, push this, call, return obj
    DeleteIndex = 47,      // pop index and object, delete element, push bool
    Swap = 48,             // swap top two stack elements
    ToNumber = 49,         // convert TOS to number
    CollectRest = 50,      // collect excess args into rest array; operand = non_rest_count (u8)
    GetArguments = 51,     // push current function's arguments object (special variable)
    SetSuperProperty = 52, // assign to super.prop using current this as receiver
    GetSuperProperty = 53, // read super.prop using current this as receiver
    TypeOfGlobal = 54,     // typeof a global variable (returns "undefined" if not defined)
    DeleteGlobal = 55,     // delete a global variable by name (for block-scoped fn cleanup)
    Pow = 56,
    BitwiseAnd = 57,
    BitwiseOr = 58,
    BitwiseXor = 59,
    ShiftLeft = 60,
    ShiftRight = 61,
    UnsignedShiftRight = 62,
    BitwiseNot = 63,
    ArrayPush = 64,
    ArraySpread = 65,
    CallSpread = 66,
    NewCallSpread = 67,
    ObjectSpread = 68,
    GetUpvalue = 69,  // operand: u8 upvalue index — read captured variable
    SetUpvalue = 70,  // operand: u8 upvalue index — write captured variable
    MakeClosure = 71, // operand: u16 const_idx, u8 capture_count, then capture_count × (u8 is_local, u8 index)
}

impl TryFrom<u8> for Opcode {
    type Error = JSError;

    fn try_from(byte: u8) -> Result<Self, Self::Error> {
        let v = match byte {
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
            15 => Opcode::Call,
            16 => Opcode::GetLocal,
            17 => Opcode::SetLocal,
            18 => Opcode::Negate,
            19 => Opcode::Not,
            20 => Opcode::TypeOf,
            21 => Opcode::Mod,
            22 => Opcode::LessEqual,
            23 => Opcode::GreaterEqual,
            24 => Opcode::NotEqual,
            25 => Opcode::StrictNotEqual,
            26 => Opcode::JumpIfTrue,
            27 => Opcode::NewArray,
            28 => Opcode::NewObject,
            29 => Opcode::GetProperty,
            30 => Opcode::SetProperty,
            31 => Opcode::GetIndex,
            32 => Opcode::SetIndex,
            33 => Opcode::Increment,
            34 => Opcode::Decrement,
            35 => Opcode::Throw,
            36 => Opcode::SetupTry,
            37 => Opcode::TeardownTry,
            38 => Opcode::GetThis,
            39 => Opcode::GetKeys,
            40 => Opcode::GetMethod,
            41 => Opcode::NewError,
            42 => Opcode::Dup,
            43 => Opcode::In,
            44 => Opcode::InstanceOf,
            45 => Opcode::DeleteProperty,
            46 => Opcode::NewCall,
            47 => Opcode::DeleteIndex,
            48 => Opcode::Swap,
            49 => Opcode::ToNumber,
            50 => Opcode::CollectRest,
            51 => Opcode::GetArguments,
            52 => Opcode::SetSuperProperty,
            53 => Opcode::GetSuperProperty,
            54 => Opcode::TypeOfGlobal,
            55 => Opcode::DeleteGlobal,
            56 => Opcode::Pow,
            57 => Opcode::BitwiseAnd,
            58 => Opcode::BitwiseOr,
            59 => Opcode::BitwiseXor,
            60 => Opcode::ShiftLeft,
            61 => Opcode::ShiftRight,
            62 => Opcode::UnsignedShiftRight,
            63 => Opcode::BitwiseNot,
            64 => Opcode::ArrayPush,
            65 => Opcode::ArraySpread,
            66 => Opcode::CallSpread,
            67 => Opcode::NewCallSpread,
            68 => Opcode::ObjectSpread,
            69 => Opcode::GetUpvalue,
            70 => Opcode::SetUpvalue,
            71 => Opcode::MakeClosure,
            _ => return Err(crate::raise_syntax_error!(format!("Unknown opcode: {byte}"))),
        };
        Ok(v)
    }
}

/// Bytecode chunk (stores instruction array and constant pool)
#[derive(Debug, Clone)]
pub struct Chunk<'gc> {
    pub code: Vec<u8>,
    pub constants: Vec<Value<'gc>>,
    /// Map from function IP to function name (for .name property)
    pub fn_names: std::collections::HashMap<usize, String>,
    /// Recorded strictness flag for functions by their starting IP
    pub fn_strictness: std::collections::HashMap<usize, bool>,
}

impl<'gc> Chunk<'gc> {
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            constants: Vec::new(),
            fn_names: std::collections::HashMap::new(),
            fn_strictness: std::collections::HashMap::new(),
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

    pub fn add_constant(&mut self, value: Value<'gc>) -> u16 {
        self.constants.push(value);
        (self.constants.len() - 1) as u16
    }
}
