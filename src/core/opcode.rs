use crate::core::{Collect, GcTrace, JSError, Value};

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
    GetUpvalue = 69,               // operand: u8 upvalue index — read captured variable
    SetUpvalue = 70,               // operand: u8 upvalue index — write captured variable
    MakeClosure = 71,              // operand: u16 const_idx, u8 capture_count, then capture_count × (u8 is_local, u8 index)
    ArrayHole = 72,                // push an empty/hole slot onto TOS array (sparse array support)
    DefineGlobalConst = 73,        // define an immutable global binding
    GetNewTarget = 74,             // push current new.target value onto stack
    Yield = 75,                    // suspend generator: pop yielded value, save state, return {value, done: false}
    SetComputedGetter = 76,        // pop val, pop computed key, peek obj; store val under __get_<key>
    SetComputedSetter = 77,        // pop val, pop computed key, peek obj; store val under __set_<key>
    InitProperty = 78,             // object literal own data property initialization by constant key
    InitIndex = 79,                // object literal own data property initialization by computed key
    GeneratorParamInitDone = 80,   // internal marker: generator parameter initialization completed
    ToPropertyKey = 81,            // coerce top-of-stack value using ToPropertyKey semantics
    ObjectSpreadExcluding = 82,    // like ObjectSpread but pops an excluded keys array first
    ValidateClassHeritage = 83,    // validate that TOS is null or a valid constructible value with valid .prototype
    GetThisSuper = 84,             // like GetThis but bypasses TDZ check (used as receiver for super() calls)
    ClearThisTdz = 85,             // clear this_tdz on the enclosing constructor frame (after super() returns)
    ValidateProtoValue = 86,       // validate TOS is object or null (for class extends prototype check); throws TypeError if not
    GetSuperPropertyComputed = 87, // computed super property: pop key from stack, look up on super prototype
    ThrowTypeError = 88,           // pop message string from stack, construct TypeError, handle_throw
    Await = 89,                    // async suspension point: pop awaited value and resume in a microtask
    EnterFieldInit = 90,           // mark start of class field initializer (for eval restrictions)
    LeaveFieldInit = 91,           // mark end of class field initializer
    AllocBrand = 92,               // push a runtime-unique brand number onto stack (for private member brand checks)
    ResetPrototype = 93,           // create a fresh prototype for the constructor on TOS
    IteratorClose = 94,            // pop iterator from stack; call .return() if callable
    AssertIterResult = 95,         // throw TypeError if TOS is not an object (IteratorResult check)
    BoxLocal = 96,                 // create a shared upvalue cell for a local (for class name heritage scope)
    ToNumeric = 97,                // ToNumeric: like ToNumber but preserves BigInt values
    SetSuperPropertyComputed = 98, // assign to super[expr] using current this as receiver
    DefineComputedMethod = 99,     // like SetIndex but also marks non-enumerable (for class methods)
    IteratorCloseAbrupt = 100,     // best-effort iterator close for throw completions; never throws
    DefineGlobalSoft = 101,        // like DefineGlobal but only defines if key doesn't already exist (for var hoisting)
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
            72 => Opcode::ArrayHole,
            73 => Opcode::DefineGlobalConst,
            74 => Opcode::GetNewTarget,
            75 => Opcode::Yield,
            76 => Opcode::SetComputedGetter,
            77 => Opcode::SetComputedSetter,
            78 => Opcode::InitProperty,
            79 => Opcode::InitIndex,
            80 => Opcode::GeneratorParamInitDone,
            81 => Opcode::ToPropertyKey,
            82 => Opcode::ObjectSpreadExcluding,
            83 => Opcode::ValidateClassHeritage,
            84 => Opcode::GetThisSuper,
            85 => Opcode::ClearThisTdz,
            86 => Opcode::ValidateProtoValue,
            87 => Opcode::GetSuperPropertyComputed,
            88 => Opcode::ThrowTypeError,
            89 => Opcode::Await,
            90 => Opcode::EnterFieldInit,
            91 => Opcode::LeaveFieldInit,
            92 => Opcode::AllocBrand,
            93 => Opcode::ResetPrototype,
            94 => Opcode::IteratorClose,
            95 => Opcode::AssertIterResult,
            96 => Opcode::BoxLocal,
            97 => Opcode::ToNumeric,
            98 => Opcode::SetSuperPropertyComputed,
            99 => Opcode::DefineComputedMethod,
            100 => Opcode::IteratorCloseAbrupt,
            101 => Opcode::DefineGlobalSoft,
            _ => return Err(crate::raise_syntax_error!(format!("Unknown opcode: {byte}"))),
        };
        Ok(v)
    }
}

/// Bytecode chunk (stores instruction array and constant pool)
#[derive(Debug, Clone, Default)]
pub struct Chunk<'gc> {
    pub code: Vec<u8>,
    pub constants: Vec<Value<'gc>>,
    /// Map from function IP to function name (for .name property)
    pub fn_names: std::collections::HashMap<usize, String>,
    /// Map from function IP to observable Function.length value.
    pub fn_lengths: std::collections::HashMap<usize, usize>,
    /// Function IPs that correspond to class constructors.
    pub class_constructor_ips: std::collections::HashSet<usize>,
    /// Function IPs that are constructors of derived (extends) classes.
    pub derived_constructor_ips: std::collections::HashSet<usize>,
    /// Recorded strictness flag for functions by their starting IP
    pub fn_strictness: std::collections::HashMap<usize, bool>,
    /// Function IPs that correspond to async functions and should return Promise values.
    pub async_function_ips: std::collections::HashSet<usize>,
    /// Function IPs that correspond to arrow functions and use lexical this.
    pub arrow_function_ips: std::collections::HashSet<usize>,
    /// Map from function IP to local variable names (for direct eval)
    pub fn_local_names: std::collections::HashMap<usize, Vec<String>>,
    /// Map from Call instruction IP to callee variable name (for error messages)
    pub call_callee_names: std::collections::HashMap<usize, String>,
    /// Function IPs that correspond to generator functions.
    pub generator_function_ips: std::collections::HashSet<usize>,
    /// Function IPs that are method/getter/setter definitions and should not be constructible.
    pub method_function_ips: std::collections::HashSet<usize>,
    /// Bytecode offset → source (line, column) mapping (sorted by offset).
    pub line_map: Vec<(usize, usize, usize)>,
    /// Map from function IP to private name context (for direct eval inside class bodies).
    /// Each entry is a list of (class_id, set_of_private_names) that were in scope.
    pub fn_private_name_context: std::collections::HashMap<usize, Vec<(usize, std::collections::HashSet<String>)>>,
    /// Map from function IP to eval context flags (for PerformEval restrictions).
    /// Bit 0: inside class field initializer (reject `arguments`, `super()`)
    /// Bit 1: inside method (allow `super.property`)
    /// Bit 2: inside constructor (allow `super()`)
    pub fn_eval_context: std::collections::HashMap<usize, u8>,
    /// Map from function IP to (upvalue_index, class_id) for private member brand checks.
    /// Methods in classes with private members capture a brand as an upvalue; this records where.
    pub fn_brand_upvalue: std::collections::HashMap<usize, (u8, usize)>,
    /// Map from function IP to upvalue names (for brand upvalue lookup).
    pub fn_upvalue_names: std::collections::HashMap<usize, Vec<String>>,
    /// Global names declared via `var`/`function`/`class` at the top level of this chunk.
    /// Used by strict-mode eval to avoid leaking declarations back to the caller.
    pub declared_globals: std::collections::HashSet<String>,
    /// Top-level lexical declaration names (`let`/`const`/`class`) in this chunk.
    /// Eval writeback must never leak these to the caller/global object.
    pub lexical_declared_globals: std::collections::HashSet<String>,
    /// Top-level function declaration names in this chunk.
    /// Used by eval writeback to apply CreateGlobalFunctionBinding semantics.
    pub fn_declared_globals: std::collections::HashSet<String>,
    /// Mapping from block-alias key (`__top_block_alias_N__`) to the original
    /// variable name it shadows.  Used at runtime so that direct eval inside a
    /// block can resolve block-scoped variables, while indirect eval sees the
    /// unmodified global value.
    pub block_alias_to_original: std::collections::HashMap<String, String>,
    /// True when this chunk was compiled for eval() code (not a main script).
    /// Affects property descriptor semantics (e.g. var bindings are configurable
    /// in eval but non-configurable in scripts).
    pub is_eval_code: bool,
}

unsafe impl<'gc> Collect<'gc> for Chunk<'gc> {
    fn trace<T: GcTrace<'gc>>(&self, cc: &mut T) {
        self.constants.trace(cc);
    }
}

impl<'gc> Chunk<'gc> {
    pub fn new() -> Self {
        Self::default()
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

    /// Record a source line and column for the current bytecode offset.
    pub fn record_line(&mut self, line: usize, column: usize) {
        if line == 0 {
            return;
        }
        let ip = self.code.len();
        if self.line_map.last().map(|&(_, l, c)| (l, c)) != Some((line, column)) {
            self.line_map.push((ip, line, column));
        }
    }

    /// Look up the source line and column for a bytecode IP.
    pub fn get_line_col_for_ip(&self, ip: usize) -> Option<(usize, usize)> {
        match self.line_map.binary_search_by_key(&ip, |&(offset, _, _)| offset) {
            Ok(idx) => Some((self.line_map[idx].1, self.line_map[idx].2)),
            Err(0) => None,
            Err(idx) => Some((self.line_map[idx - 1].1, self.line_map[idx - 1].2)),
        }
    }
}
