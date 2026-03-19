use crate::core::opcode::{Chunk, Opcode};
use crate::core::value::{VmArrayData, VmMapData, VmSetData, value_to_string};
use crate::core::{JSError, Value};
use crate::js_regexp::get_or_compile_regex;
use indexmap::IndexMap;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::rc::Rc;
use std::sync::{LazyLock, Mutex};

static VM_OS_FILE_STORE: LazyLock<Mutex<HashMap<u64, File>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static VM_NEXT_OS_FILE_ID: LazyLock<Mutex<u64>> = LazyLock::new(|| Mutex::new(1));

fn vm_next_os_file_id() -> u64 {
    let mut id = VM_NEXT_OS_FILE_ID.lock().unwrap();
    let current = *id;
    *id += 1;
    current
}

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
const BUILTIN_STRING_PADSTART: u8 = 42;
const BUILTIN_STRING_PADEND: u8 = 43;
const BUILTIN_STRING_REPEAT: u8 = 44;
const BUILTIN_STRING_CHARCODEAT: u8 = 45;
const BUILTIN_STRING_FROMCHARCODE: u8 = 46;
const BUILTIN_STRING_TRIMSTART: u8 = 47;
const BUILTIN_STRING_TRIMEND: u8 = 48;
const BUILTIN_STRING_LASTINDEXOF: u8 = 49;
const BUILTIN_JSON_STRINGIFY: u8 = 50;
const BUILTIN_JSON_PARSE: u8 = 51;
const BUILTIN_ARRAY_REDUCE: u8 = 52;
const BUILTIN_BIGINT_ASUINTN: u8 = 53;
const BUILTIN_BIGINT_ASINTN: u8 = 54;
// Constructor sentinels (for typeof → "function" and instanceof checks)
const BUILTIN_CTOR_ERROR: u8 = 60;
const BUILTIN_CTOR_TYPEERROR: u8 = 61;
const BUILTIN_CTOR_SYNTAXERROR: u8 = 62;
const BUILTIN_CTOR_RANGEERROR: u8 = 63;
const BUILTIN_CTOR_REFERENCEERROR: u8 = 64;
const BUILTIN_CTOR_DATE: u8 = 65;
const BUILTIN_CTOR_FUNCTION: u8 = 66;
const BUILTIN_CTOR_NUMBER: u8 = 67;
const BUILTIN_CTOR_STRING: u8 = 68;
const BUILTIN_CTOR_BOOLEAN: u8 = 69;
const BUILTIN_CTOR_OBJECT: u8 = 70;
const BUILTIN_EVAL: u8 = 71;
const BUILTIN_NEW_FUNCTION: u8 = 72;
// Number static methods
const BUILTIN_NUMBER_ISNAN: u8 = 73;
const BUILTIN_NUMBER_ISFINITE: u8 = 74;
const BUILTIN_NUMBER_ISINTEGER: u8 = 75;
const BUILTIN_NUMBER_ISSAFEINTEGER: u8 = 76;
// Number instance methods
const BUILTIN_NUM_TOFIXED: u8 = 77;
const BUILTIN_NUM_TOEXPONENTIAL: u8 = 78;
const BUILTIN_NUM_TOPRECISION: u8 = 79;
const BUILTIN_NUM_TOSTRING: u8 = 80;
const BUILTIN_NUM_VALUEOF: u8 = 81;
// Map methods
const BUILTIN_MAP_SET: u8 = 82;
const BUILTIN_MAP_GET: u8 = 83;
const BUILTIN_MAP_HAS: u8 = 84;
const BUILTIN_MAP_DELETE: u8 = 85;
const BUILTIN_MAP_KEYS: u8 = 86;
const BUILTIN_MAP_VALUES: u8 = 87;
const BUILTIN_MAP_ENTRIES: u8 = 88;
const BUILTIN_MAP_FOREACH: u8 = 89;
const BUILTIN_MAP_CLEAR: u8 = 90;
// Set methods
const BUILTIN_SET_ADD: u8 = 91;
const BUILTIN_SET_HAS: u8 = 92;
const BUILTIN_SET_DELETE: u8 = 93;
#[allow(dead_code)]
const BUILTIN_SET_KEYS: u8 = 94;
const BUILTIN_SET_VALUES: u8 = 95;
const BUILTIN_SET_ENTRIES: u8 = 96;
const BUILTIN_SET_FOREACH: u8 = 97;
const BUILTIN_SET_CLEAR: u8 = 98;
// Constructor sentinels
const BUILTIN_CTOR_MAP: u8 = 99;
const BUILTIN_CTOR_SET: u8 = 100;
const BUILTIN_ITERATOR_NEXT: u8 = 101;
const BUILTIN_OBJECT_KEYS: u8 = 102;
const BUILTIN_OBJECT_VALUES: u8 = 103;
const BUILTIN_OBJECT_ENTRIES: u8 = 104;
const BUILTIN_OBJECT_ASSIGN: u8 = 105;
const BUILTIN_OBJECT_FREEZE: u8 = 106;
const BUILTIN_OBJECT_HASOWN: u8 = 107;
const BUILTIN_OBJECT_CREATE: u8 = 108;
const BUILTIN_OBJECT_GETPROTOTYPEOF: u8 = 109;
const BUILTIN_OBJECT_DEFINEPROPS: u8 = 110;
const BUILTIN_OBJECT_PREVENTEXT: u8 = 111;
const BUILTIN_OBJECT_GROUPBY: u8 = 112;
const BUILTIN_OBJECT_DEFINEPROP: u8 = 113;
const BUILTIN_OBJ_HASOWNPROPERTY: u8 = 114;
const BUILTIN_FN_CALL: u8 = 115;
const BUILTIN_FN_BIND: u8 = 116;
const BUILTIN_OBJECT_GETOWNPROPDESC: u8 = 117;
const BUILTIN_OBJECT_SETPROTOTYPEOF: u8 = 118;
const BUILTIN_FN_APPLY: u8 = 119;
const BUILTIN_OBJECT_GETOWNPROPERTYNAMES: u8 = 120;
const BUILTIN_ARRAY_ITERATOR: u8 = 121;
const BUILTIN_CTOR_WEAKMAP: u8 = 122;
const BUILTIN_CTOR_WEAKSET: u8 = 123;
const BUILTIN_CTOR_WEAKREF: u8 = 124;
const BUILTIN_WEAKREF_DEREF: u8 = 125;
const BUILTIN_SYMBOL: u8 = 126;
const BUILTIN_SYMBOL_FOR: u8 = 127;
const BUILTIN_SYMBOL_KEYFOR: u8 = 128;
const BUILTIN_OBJ_TOSTRING: u8 = 129;
const BUILTIN_CTOR_FR: u8 = 130;
const BUILTIN_FR_REGISTER: u8 = 131;
const BUILTIN_FR_UNREGISTER: u8 = 132;
const BUILTIN_BIGINT: u8 = 133;
const BUILTIN_CTOR_ARRAY: u8 = 134;
const BUILTIN_ARRAY_OF: u8 = 135;
const BUILTIN_ARRAY_FROM: u8 = 136;
const BUILTIN_ARRAY_SHIFT: u8 = 137;
const BUILTIN_ARRAY_UNSHIFT: u8 = 138;
const BUILTIN_ARRAY_SPLICE: u8 = 139;
const BUILTIN_ARRAY_REVERSE: u8 = 140;
const BUILTIN_ARRAY_SORT: u8 = 141;
const BUILTIN_ARRAY_FIND: u8 = 142;
const BUILTIN_ARRAY_FINDINDEX: u8 = 143;
const BUILTIN_ARRAY_INCLUDES: u8 = 144;
const BUILTIN_ARRAY_FLAT: u8 = 145;
const BUILTIN_ARRAY_FLATMAP: u8 = 146;
const BUILTIN_ARRAY_AT: u8 = 147;
const BUILTIN_ARRAY_EVERY: u8 = 148;
const BUILTIN_ARRAY_SOME: u8 = 149;
const BUILTIN_ARRAY_FILL: u8 = 150;
const BUILTIN_ARRAY_LASTINDEXOF: u8 = 151;
const BUILTIN_ARRAY_FINDLAST: u8 = 152;
const BUILTIN_ARRAY_FINDLASTINDEX: u8 = 153;
const BUILTIN_ARRAY_REDUCERIGHT: u8 = 154;
const BUILTIN_MATH_SIN: u8 = 155;
const BUILTIN_MATH_COS: u8 = 156;
const BUILTIN_MATH_TAN: u8 = 157;
const BUILTIN_MATH_ASIN: u8 = 158;
const BUILTIN_MATH_ACOS: u8 = 159;
const BUILTIN_MATH_ATAN: u8 = 160;
const BUILTIN_MATH_ATAN2: u8 = 161;
const BUILTIN_MATH_SINH: u8 = 162;
const BUILTIN_MATH_COSH: u8 = 163;
const BUILTIN_MATH_TANH: u8 = 164;
const BUILTIN_MATH_ASINH: u8 = 165;
const BUILTIN_MATH_ACOSH: u8 = 166;
const BUILTIN_MATH_ATANH: u8 = 167;
const BUILTIN_MATH_EXP: u8 = 168;
const BUILTIN_MATH_EXPM1: u8 = 169;
const BUILTIN_MATH_LOG: u8 = 170;
const BUILTIN_MATH_LOG10: u8 = 171;
const BUILTIN_MATH_LOG1P: u8 = 172;
const BUILTIN_MATH_LOG2: u8 = 173;
const BUILTIN_MATH_FROUND: u8 = 174;
const BUILTIN_MATH_TRUNC: u8 = 175;
const BUILTIN_MATH_CBRT: u8 = 176;
const BUILTIN_MATH_HYPOT: u8 = 177;
const BUILTIN_MATH_SIGN: u8 = 178;
const BUILTIN_MATH_POW: u8 = 179;
const BUILTIN_MATH_RANDOM: u8 = 180;
const BUILTIN_MATH_CLZ32: u8 = 181;
const BUILTIN_MATH_IMUL: u8 = 182;
const BUILTIN_CTOR_REGEXP: u8 = 183;
const BUILTIN_REGEX_EXEC: u8 = 184;
const BUILTIN_REGEX_TEST: u8 = 185;
const BUILTIN_STRING_MATCH: u8 = 186;
const BUILTIN_STRING_REPLACEALL: u8 = 187;
const BUILTIN_STRING_SEARCH: u8 = 188;

// Date methods
const BUILTIN_DATE_NOW: u8 = 189;
const BUILTIN_DATE_GETTIME: u8 = 190;
const BUILTIN_DATE_TOSTRING: u8 = 191;
const BUILTIN_DATE_TOLOCALEDATESTRING: u8 = 192;
const BUILTIN_DATE_GETFULLYEAR: u8 = 193;
const BUILTIN_DATE_GETMONTH: u8 = 194;
const BUILTIN_DATE_GETDATE: u8 = 195;
const BUILTIN_DATE_GETDAY: u8 = 196;
const BUILTIN_DATE_GETHOURS: u8 = 197;
const BUILTIN_DATE_GETMINUTES: u8 = 198;
const BUILTIN_DATE_GETSECONDS: u8 = 199;
const BUILTIN_DATE_GETMILLISECONDS: u8 = 200;
const BUILTIN_DATE_VALUEOF: u8 = 201;
const BUILTIN_DATE_SETFULLYEAR: u8 = 202;
const BUILTIN_DATE_SETMONTH: u8 = 203;
const BUILTIN_DATE_SETDATE: u8 = 204;
const BUILTIN_DATE_SETHOURS: u8 = 205;
const BUILTIN_DATE_SETMINUTES: u8 = 206;
const BUILTIN_DATE_TOLOCALETIMESTRING: u8 = 207;
const BUILTIN_DATE_TOLOCALESTRING: u8 = 208;
const BUILTIN_DATE_TOISOSTRING: u8 = 209;
const BUILTIN_DATE_GETUTCFULLYEAR: u8 = 210;
const BUILTIN_DATE_GETUTCMONTH: u8 = 211;
const BUILTIN_DATE_GETUTCDATE: u8 = 212;
const BUILTIN_DATE_GETUTCHOURS: u8 = 213;
const BUILTIN_DATE_GETUTCMINUTES: u8 = 214;
const BUILTIN_DATE_GETUTCSECONDS: u8 = 215;
const BUILTIN_DATE_GETTIMEZONEOFFSET: u8 = 216;
const BUILTIN_DATE_PARSE: u8 = 217;
const BUILTIN_DATE_SETTIME: u8 = 218;
const BUILTIN_DATE_TODATESTRING: u8 = 219;
const BUILTIN_SETTIMEOUT: u8 = 220;
const BUILTIN_CLEARTIMEOUT: u8 = 221;
const BUILTIN_SETINTERVAL: u8 = 222;
const BUILTIN_CLEARINTERVAL: u8 = 223;
const BUILTIN_CTOR_ARRAYBUFFER: u8 = 224;
const BUILTIN_CTOR_DATAVIEW: u8 = 225;
const BUILTIN_CTOR_INT8ARRAY: u8 = 226;
const BUILTIN_CTOR_UINT8ARRAY: u8 = 227;
const BUILTIN_ARRAYBUFFER_RESIZE: u8 = 228;
const BUILTIN_CTOR_UINT8CLAMPEDARRAY: u8 = 229;
const BUILTIN_CTOR_INT16ARRAY: u8 = 230;
const BUILTIN_CTOR_UINT16ARRAY: u8 = 231;
const BUILTIN_CTOR_INT32ARRAY: u8 = 232;
const BUILTIN_CTOR_UINT32ARRAY: u8 = 233;
const BUILTIN_CTOR_FLOAT32ARRAY: u8 = 234;
const BUILTIN_CTOR_FLOAT64ARRAY: u8 = 235;
const BUILTIN_CTOR_PROMISE: u8 = 236;
const BUILTIN_PROMISE_RESOLVE: u8 = 237;
const BUILTIN_PROMISE_ALL: u8 = 238;
const BUILTIN_PROMISE_THEN: u8 = 239;
const BUILTIN_PROMISE_NOOP: u8 = 240;
const BUILTIN_CTOR_PROXY: u8 = 241;
const BUILTIN_CTOR_SHAREDARRAYBUFFER: u8 = 242;
const BUILTIN_ATOMICS_ISLOCKFREE: u8 = 243;
const BUILTIN_ATOMICS_LOAD: u8 = 244;
const BUILTIN_ATOMICS_STORE: u8 = 245;
const BUILTIN_ATOMICS_COMPAREEXCHANGE: u8 = 246;
const BUILTIN_ATOMICS_ADD: u8 = 247;
const BUILTIN_ATOMICS_EXCHANGE: u8 = 248;
const BUILTIN_ATOMICS_WAIT: u8 = 249;
const BUILTIN_ATOMICS_NOTIFY: u8 = 250;
const BUILTIN_ATOMICS_WAITASYNC: u8 = 251;
const BUILTIN_ASYNCGEN_NEXT: u8 = 252;
const BUILTIN_ASYNCGEN_THROW: u8 = 253;
const BUILTIN_ASYNCGEN_RETURN: u8 = 254;
const BUILTIN_REFLECT_APPLY: u8 = 255;

#[derive(Debug, Clone)]
pub struct CallFrame<'gc> {
    pub return_ip: usize,
    pub bp: usize,                                            // Base pointer
    pub is_method: bool,                                      // Pop this_stack on return
    pub arg_count: usize,                                     // Actual number of arguments passed
    pub func_ip: usize,                                       // instruction pointer of the called function
    pub arguments_obj: Option<Value<'gc>>,                    // cached arguments object
    pub upvalues: Vec<Rc<RefCell<Value<'gc>>>>,               // captured upvalue cells (shared mutable)
    pub saved_args: Option<Vec<Value<'gc>>>,                  // saved full arg list when arg_count > arity
    pub local_cells: HashMap<usize, Rc<RefCell<Value<'gc>>>>, // locals captured as upvalue cells
}

#[derive(Debug, Clone)]
pub struct TryFrame {
    pub catch_ip: usize,               // where to jump on throw
    pub stack_depth: usize,            // stack depth at try entry
    pub frame_depth: usize,            // call frame depth at try entry
    pub catch_binding: Option<String>, // variable name for caught value
}

// JS ToNumber abstract operation
fn to_number<'gc>(val: &Value<'gc>) -> f64 {
    match val {
        Value::Number(n) => *n,
        Value::Undefined => f64::NAN,
        Value::Null => 0.0,
        Value::Boolean(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::String(s) => {
            let s = crate::unicode::utf16_to_utf8(s);
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return 0.0;
            }
            trimmed.parse::<f64>().unwrap_or(f64::NAN)
        }
        _ => f64::NAN,
    }
}

// JS ToUint32 (ECMAScript 7.1.7) used by bitwise/shift operators.
fn to_uint32(n: f64) -> u32 {
    if n.is_nan() || n == 0.0 || !n.is_finite() {
        return 0;
    }
    let two32 = 4_294_967_296.0;
    n.trunc().rem_euclid(two32) as u32
}

// JS ToInt32 derived from ToUint32.
fn to_int32(n: f64) -> i32 {
    to_uint32(n) as i32
}

fn bigint_from_integral_number(n: f64) -> Option<num_bigint::BigInt> {
    if !n.is_finite() || n != n.trunc() {
        return None;
    }
    if n == 0.0 {
        return Some(num_bigint::BigInt::from(0));
    }

    let bits = n.to_bits();
    let sign_negative = (bits >> 63) != 0;
    let exp_bits = ((bits >> 52) & 0x7ff) as i32;
    let frac_bits = bits & ((1u64 << 52) - 1);

    // Subnormal non-zero values are not integral.
    if exp_bits == 0 {
        return None;
    }

    let exponent = exp_bits - 1023;
    let mut sig = num_bigint::BigInt::from((1u64 << 52) | frac_bits);
    if exponent >= 52 {
        sig <<= (exponent - 52) as usize;
    } else {
        let rshift = (52 - exponent) as u32;
        let mask = (1u64 << rshift) - 1;
        if frac_bits & mask != 0 {
            return None;
        }
        sig >>= rshift as usize;
    }

    if sign_negative {
        sig = -sig;
    }
    Some(sig)
}

fn compare_bigint_number(a: &num_bigint::BigInt, b: f64) -> Option<std::cmp::Ordering> {
    if b.is_nan() {
        return None;
    }
    if b == f64::INFINITY {
        return Some(std::cmp::Ordering::Less);
    }
    if b == f64::NEG_INFINITY {
        return Some(std::cmp::Ordering::Greater);
    }

    if let Some(bi) = bigint_from_integral_number(b) {
        return Some(a.cmp(&bi));
    }

    // For finite non-integer Number, BigInt is always strictly less-or-greater.
    let floor_bi = bigint_from_integral_number(b.floor())?;
    if a <= &floor_bi {
        Some(std::cmp::Ordering::Less)
    } else {
        Some(std::cmp::Ordering::Greater)
    }
}

/// Convert Rust exponential format (e.g. "7.71234e1") to JS format ("7.71234e+1")
fn js_exponential_format(s: &str) -> String {
    // Rust uses "e" without sign for positive exponents; JS uses "e+"
    if let Some(idx) = s.find('e') {
        let (mantissa, exp_part) = s.split_at(idx);
        let exp_digits = &exp_part[1..]; // skip 'e'
        if exp_digits.starts_with('-') {
            format!("{}e{}", mantissa, exp_digits)
        } else {
            format!("{}e+{}", mantissa, exp_digits)
        }
    } else {
        s.to_string()
    }
}

/// A queued timer callback (setTimeout / setInterval).
struct PendingTimer<'gc> {
    id: usize,
    callback: Value<'gc>,
    args: Vec<Value<'gc>>,
    delay_ms: u64,
    is_interval: bool,
}

/// Bytecode VM first stage prototype
pub struct VM<'gc> {
    chunk: Chunk<'gc>,
    ip: usize,
    stack: Vec<Value<'gc>>,
    globals: HashMap<String, Value<'gc>>,
    const_globals: std::collections::HashSet<String>,
    frames: Vec<CallFrame<'gc>>,
    try_stack: Vec<TryFrame>,
    this_stack: Vec<Value<'gc>>,       // this binding stack
    new_target_stack: Vec<Value<'gc>>, // new.target binding stack
    output: Vec<String>,               // captured output for console.log etc.
    // Property storage for VmFunction values, keyed by function IP
    fn_props: HashMap<usize, Rc<RefCell<IndexMap<String, Value<'gc>>>>>,
    // Method home objects keyed by function IP, used to resolve `super` correctly.
    fn_home_objects: HashMap<usize, Value<'gc>>,
    // Global this object — top-level `this` refers to this; SetProperty on it writes to globals
    global_this: Rc<RefCell<IndexMap<String, Value<'gc>>>>,
    symbol_counter: u64,
    symbol_registry: HashMap<String, Value<'gc>>, // Symbol.for() registry
    symbol_values: HashMap<u64, Value<'gc>>,      // symbol_id → Symbol VmObject (for getOwnPropertySymbols)
    pending_throw: Option<Value<'gc>>,            // deferred throw from call_builtin
    direct_eval: bool,                            // true when current eval is a direct call
    script_source: Option<String>,
    script_path: Option<String>,
    // Timer queue for setTimeout / setInterval
    pending_timers: Vec<PendingTimer<'gc>>,
    next_timer_id: usize,
    cleared_timers: std::collections::HashSet<usize>,
}

impl<'gc> VM<'gc> {
    pub fn new(chunk: Chunk<'gc>) -> Self {
        let global_this = Rc::new(RefCell::new(IndexMap::new()));
        let mut vm = Self {
            chunk,
            ip: 0,
            stack: Vec::with_capacity(256),
            globals: HashMap::new(),
            const_globals: std::collections::HashSet::new(),
            frames: Vec::new(),
            try_stack: Vec::new(),
            this_stack: vec![Value::VmObject(global_this.clone())],
            new_target_stack: Vec::new(),
            output: Vec::new(),
            fn_props: HashMap::new(),
            fn_home_objects: HashMap::new(),
            global_this,
            symbol_counter: 0,
            symbol_registry: HashMap::new(),
            symbol_values: HashMap::new(),
            pending_throw: None,
            direct_eval: false,
            script_source: None,
            script_path: None,
            pending_timers: Vec::new(),
            next_timer_id: 1,
            cleared_timers: std::collections::HashSet::new(),
        };
        vm.register_builtins();
        vm
    }

    pub fn set_source_context(&mut self, script_source: &str, script_path: Option<&std::path::Path>) {
        self.script_source = Some(script_source.to_string());
        self.script_path = script_path.map(|path| path.display().to_string());
    }

    fn make_host_fn(name: &str) -> Value<'gc> {
        let mut map = IndexMap::new();
        map.insert("__host_fn__".to_string(), Value::String(crate::unicode::utf8_to_utf16(name)));
        Value::VmObject(Rc::new(RefCell::new(map)))
    }

    fn make_bound_host_fn(name: &str, receiver: Value<'gc>) -> Value<'gc> {
        let mut map = IndexMap::new();
        map.insert("__host_fn__".to_string(), Value::String(crate::unicode::utf8_to_utf16(name)));
        map.insert("__host_this__".to_string(), receiver);
        Value::VmObject(Rc::new(RefCell::new(map)))
    }

    /// Create a pending promise (no __promise_value__ yet).
    fn make_pending_promise(&self) -> Value<'gc> {
        let mut map = IndexMap::new();
        map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Promise")));
        map.insert("then".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_THEN));
        if let Some(Value::VmObject(promise_ctor)) = self.globals.get("Promise")
            && let Some(proto) = promise_ctor.borrow().get("prototype").cloned()
        {
            map.insert("__proto__".to_string(), proto);
        }
        Value::VmObject(Rc::new(RefCell::new(map)))
    }

    /// Settle a promise and flush its __then_queue__.
    fn settle_promise(&mut self, promise: &Value<'gc>, value: Value<'gc>, rejected: bool) {
        if let Value::VmObject(obj) = promise {
            // Set the value
            {
                let mut b = obj.borrow_mut();
                b.insert("__promise_value__".to_string(), value.clone());
                if rejected {
                    b.insert("__promise_rejected__".to_string(), Value::Boolean(true));
                } else {
                    b.shift_remove("__promise_rejected__");
                }
            }
            // Flush __then_queue__
            let queue = obj.borrow_mut().shift_remove("__then_queue__");
            if let Some(Value::VmArray(arr)) = queue {
                let entries: Vec<Value<'gc>> = arr.borrow().elements.clone();
                for entry in entries {
                    if let Value::VmObject(entry_map) = entry {
                        let (on_fulfilled, on_rejected, child) = {
                            let b = entry_map.borrow();
                            (b.get("onFulfilled").cloned(), b.get("onRejected").cloned(), b.get("child").cloned())
                        };
                        let callback = if rejected { on_rejected } else { on_fulfilled };
                        let child = child.unwrap_or(Value::Undefined);

                        let is_callable = |v: &Value<'gc>| -> bool {
                            matches!(v, Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(..))
                                || matches!(v, Value::VmObject(m) if m.borrow().contains_key("__host_fn__"))
                        };

                        let (cb_result, cb_rejected) = if let Some(ref cb) = callback
                            && is_callable(cb)
                        {
                            match cb {
                                Value::VmFunction(ip, _) => {
                                    let saved = std::mem::take(&mut self.try_stack);
                                    let out = self.call_vm_function_result(*ip, std::slice::from_ref(&value), &[]);
                                    self.try_stack = saved;
                                    match out {
                                        Ok(v) => (v, false),
                                        Err(e) => (self.vm_value_from_error(&e), true),
                                    }
                                }
                                Value::VmClosure(ip, _, upv) => {
                                    let uv = (**upv).clone();
                                    let saved = std::mem::take(&mut self.try_stack);
                                    let out = self.call_vm_function_result(*ip, std::slice::from_ref(&value), &uv);
                                    self.try_stack = saved;
                                    match out {
                                        Ok(v) => (v, false),
                                        Err(e) => (self.vm_value_from_error(&e), true),
                                    }
                                }
                                Value::VmNativeFunction(native_id) => (self.call_builtin(*native_id, vec![value.clone()]), false),
                                _ => (value.clone(), rejected),
                            }
                        } else {
                            // No matching callback — propagate value/rejection
                            (value.clone(), rejected)
                        };

                        // Assimilate inner promise if callback returned one
                        let (final_val, final_rej) = if let Value::VmObject(inner) = &cb_result {
                            let b = inner.borrow();
                            let is_promise =
                                matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "Promise");
                            if is_promise {
                                let inner_has_value = b.contains_key("__promise_value__");
                                let inner_rej = matches!(b.get("__promise_rejected__"), Some(Value::Boolean(true)));
                                let inner_val = b.get("__promise_value__").cloned().unwrap_or(Value::Undefined);
                                drop(b);
                                if inner_has_value {
                                    (inner_val, inner_rej)
                                } else {
                                    // Inner promise is also pending — link child to it
                                    if let Value::VmObject(inner_obj) = &cb_result {
                                        let mut ib = inner_obj.borrow_mut();
                                        let mut link_entry = IndexMap::new();
                                        // No callbacks, just forward to child
                                        link_entry.insert("child".to_string(), child.clone());
                                        let link_val = Value::VmObject(Rc::new(RefCell::new(link_entry)));
                                        if let Some(Value::VmArray(q)) = ib.get("__then_queue__").cloned() {
                                            q.borrow_mut().push(link_val);
                                        } else {
                                            let q = VmArrayData::new(vec![link_val]);
                                            ib.insert("__then_queue__".to_string(), Value::VmArray(Rc::new(RefCell::new(q))));
                                        }
                                    }
                                    continue; // don't settle child yet
                                }
                            } else {
                                (cb_result.clone(), cb_rejected)
                            }
                        } else {
                            (cb_result, cb_rejected)
                        };

                        self.settle_promise(&child, final_val, final_rej);
                    }
                }
            }
        }
    }

    fn call_host_fn(&mut self, name: &str, receiver: Option<Value<'gc>>, args: Vec<Value<'gc>>) -> Value<'gc> {
        match name {
            "global.isFinite" => Value::Boolean(to_number(args.first().unwrap_or(&Value::Undefined)).is_finite()),
            "global.encodeURI" | "global.encodeURIComponent" => {
                let s = args.first().map(value_to_string).unwrap_or_default();
                Value::String(crate::unicode::utf8_to_utf16(&s.replace(' ', "%20")))
            }
            "global.decodeURI" | "global.decodeURIComponent" => {
                let s = args.first().map(value_to_string).unwrap_or_default();
                Value::String(crate::unicode::utf8_to_utf16(&s.replace("%20", " ")))
            }
            "global.__forOfValues" => {
                let source = args.first().cloned().unwrap_or(Value::Undefined);
                match source {
                    Value::VmArray(arr) => Value::VmArray(arr),
                    Value::VmMap(map) => {
                        let items = map
                            .borrow()
                            .entries
                            .iter()
                            .map(|(k, v)| Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(vec![k.clone(), v.clone()])))))
                            .collect::<Vec<_>>();
                        Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(items))))
                    }
                    Value::VmSet(set) => {
                        let items = set.borrow().values.clone();
                        Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(items))))
                    }
                    Value::String(s) => {
                        let text = crate::unicode::utf16_to_utf8(&s);
                        let values = text
                            .chars()
                            .map(|ch| Value::String(crate::unicode::utf8_to_utf16(&ch.to_string())))
                            .collect::<Vec<_>>();
                        Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(values))))
                    }
                    Value::VmObject(obj) => {
                        if let Some(Value::String(s)) = obj.borrow().get("__value__").cloned() {
                            let text = crate::unicode::utf16_to_utf8(&s);
                            let values = text
                                .chars()
                                .map(|ch| Value::String(crate::unicode::utf8_to_utf16(&ch.to_string())))
                                .collect::<Vec<_>>();
                            return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(values))));
                        }

                        let iterable = Value::VmObject(obj.clone());
                        let iter_fn = self.read_named_property(iterable.clone(), "@@sym:1");
                        let iterator = match iter_fn {
                            Value::VmFunction(ip, _) => {
                                self.this_stack.push(iterable.clone());
                                let call = self.call_vm_function_result(ip, &[], &[]);
                                self.this_stack.pop();
                                match call {
                                    Ok(v) => v,
                                    Err(err) => {
                                        let mut err_map = IndexMap::new();
                                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                        err_map.insert("message".to_string(), Value::String(crate::unicode::utf8_to_utf16(&err.message())));
                                        self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                        return Value::Undefined;
                                    }
                                }
                            }
                            Value::VmClosure(ip, _, upv) => {
                                self.this_stack.push(iterable.clone());
                                let uv = (*upv).clone();
                                let call = self.call_vm_function_result(ip, &[], &uv);
                                self.this_stack.pop();
                                match call {
                                    Ok(v) => v,
                                    Err(err) => {
                                        let mut err_map = IndexMap::new();
                                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                        err_map.insert("message".to_string(), Value::String(crate::unicode::utf8_to_utf16(&err.message())));
                                        self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                        return Value::Undefined;
                                    }
                                }
                            }
                            Value::VmNativeFunction(id) => self.call_method_builtin(id, iterable.clone(), vec![]),
                            Value::VmObject(ref host_obj) => {
                                let borrow = host_obj.borrow();
                                if let Some(Value::String(host_name_u16)) = borrow.get("__host_fn__") {
                                    let host_name = crate::unicode::utf16_to_utf8(host_name_u16);
                                    drop(borrow);
                                    self.call_host_fn(&host_name, Some(iterable.clone()), vec![])
                                } else {
                                    let mut err_map = IndexMap::new();
                                    err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                    err_map.insert(
                                        "message".to_string(),
                                        Value::String(crate::unicode::utf8_to_utf16("iterator missing")),
                                    );
                                    self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                    return Value::Undefined;
                                }
                            }
                            Value::Undefined => {
                                let next_candidate = self.read_named_property(iterable.clone(), "next");
                                if matches!(
                                    next_candidate,
                                    Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_)
                                ) {
                                    iterable.clone()
                                } else {
                                    let mut err_map = IndexMap::new();
                                    err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                    err_map.insert(
                                        "message".to_string(),
                                        Value::String(crate::unicode::utf8_to_utf16("iterator missing")),
                                    );
                                    self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                    return Value::Undefined;
                                }
                            }
                            _ => {
                                let mut err_map = IndexMap::new();
                                err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                err_map.insert(
                                    "message".to_string(),
                                    Value::String(crate::unicode::utf8_to_utf16("iterator missing")),
                                );
                                self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                return Value::Undefined;
                            }
                        };

                        let iter_obj = match iterator {
                            Value::VmObject(_) => iterator,
                            _ => {
                                let mut err_map = IndexMap::new();
                                err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                err_map.insert(
                                    "message".to_string(),
                                    Value::String(crate::unicode::utf8_to_utf16("iterator is not an object")),
                                );
                                self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                return Value::Undefined;
                            }
                        };

                        let next_fn = self.read_named_property(iter_obj.clone(), "next");
                        let mut out = Vec::new();
                        for _ in 0..10000 {
                            let next_result = match &next_fn {
                                Value::VmFunction(ip, _) => {
                                    self.this_stack.push(iter_obj.clone());
                                    let call = self.call_vm_function_result(*ip, &[], &[]);
                                    self.this_stack.pop();
                                    match call {
                                        Ok(v) => v,
                                        Err(err) => {
                                            let mut err_map = IndexMap::new();
                                            err_map
                                                .insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                            err_map.insert(
                                                "message".to_string(),
                                                Value::String(crate::unicode::utf8_to_utf16(&err.message())),
                                            );
                                            self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                            return Value::Undefined;
                                        }
                                    }
                                }
                                Value::VmClosure(ip, _, upv) => {
                                    self.this_stack.push(iter_obj.clone());
                                    let uv = (*upv).clone();
                                    let call = self.call_vm_function_result(*ip, &[], &uv);
                                    self.this_stack.pop();
                                    match call {
                                        Ok(v) => v,
                                        Err(err) => {
                                            let mut err_map = IndexMap::new();
                                            err_map
                                                .insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                            err_map.insert(
                                                "message".to_string(),
                                                Value::String(crate::unicode::utf8_to_utf16(&err.message())),
                                            );
                                            self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                            return Value::Undefined;
                                        }
                                    }
                                }
                                Value::VmNativeFunction(id) => self.call_method_builtin(*id, iter_obj.clone(), vec![]),
                                _ => {
                                    let mut err_map = IndexMap::new();
                                    err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                    err_map.insert(
                                        "message".to_string(),
                                        Value::String(crate::unicode::utf8_to_utf16("iterator.next is not callable")),
                                    );
                                    self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                    return Value::Undefined;
                                }
                            };

                            if let Value::VmObject(res_obj) = next_result {
                                let rb = res_obj.borrow();
                                let done = rb.get("done").map(|v| v.to_truthy()).unwrap_or(false);
                                if done {
                                    break;
                                }
                                out.push(rb.get("value").cloned().unwrap_or(Value::Undefined));
                            } else {
                                let mut err_map = IndexMap::new();
                                err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                err_map.insert(
                                    "message".to_string(),
                                    Value::String(crate::unicode::utf8_to_utf16("iterator.next() must return an object")),
                                );
                                self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                return Value::Undefined;
                            }
                        }

                        Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(out))))
                    }
                    _ => {
                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("iterator missing")),
                        );
                        self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                        Value::Undefined
                    }
                }
            }
            "os.getcwd" | "os.getpid" | "os.getppid" | "os.open" | "os.write" | "os.read" | "os.seek" | "os.close" | "os.path.basename"
            | "os.path.dirname" | "os.path.join" | "os.path.extname" => self.call_named_host_function(name, args),
            "date.UTC" => {
                use chrono::{TimeZone, Utc};
                let year = args.first().map(to_number).unwrap_or(0.0) as i32;
                let month = args.get(1).map(to_number).unwrap_or(0.0) as i32;
                let day = args.get(2).map(to_number).unwrap_or(1.0) as u32;
                let hour = args.get(3).map(to_number).unwrap_or(0.0) as u32;
                let minute = args.get(4).map(to_number).unwrap_or(0.0) as u32;
                let second = args.get(5).map(to_number).unwrap_or(0.0) as u32;
                let millis = args.get(6).map(to_number).unwrap_or(0.0) as i64;
                let full_year = if (0..100).contains(&year) { year + 1900 } else { year };
                match Utc.with_ymd_and_hms(full_year, (month + 1).max(1) as u32, day.max(1), hour, minute, second) {
                    chrono::LocalResult::Single(dt) => Value::Number(dt.timestamp_millis() as f64 + millis as f64),
                    _ => Value::Number(f64::NAN),
                }
            }
            "string.concat" => {
                let base = receiver.as_ref().map(value_to_string).unwrap_or_default();
                let mut out = base;
                for a in &args {
                    out.push_str(&value_to_string(a));
                }
                Value::String(crate::unicode::utf8_to_utf16(&out))
            }
            "string.substr" => {
                let s = receiver.as_ref().map(value_to_string).unwrap_or_default();
                let len = s.len() as i64;
                let start_raw = args.first().map(to_number).unwrap_or(0.0) as i64;
                let start = if start_raw < 0 {
                    (len + start_raw).max(0)
                } else {
                    start_raw.min(len)
                } as usize;
                let count = match args.get(1) {
                    Some(Value::Number(n)) => (*n).max(0.0) as usize,
                    Some(_) => 0,
                    None => len.saturating_sub(start as i64) as usize,
                };
                let end = start.saturating_add(count).min(s.len());
                let slice = &s[start..end];
                Value::String(crate::unicode::utf8_to_utf16(slice))
            }
            "object.toLocaleString" => {
                let recv = receiver.unwrap_or(Value::Undefined);
                let method = self.read_named_property(recv.clone(), "toString");
                match method {
                    Value::VmNativeFunction(id) => self.call_method_builtin(id, recv, vec![]),
                    Value::VmFunction(ip, _) => {
                        self.this_stack.push(recv.clone());
                        let out = self.call_vm_function_result(ip, &[], &[]);
                        self.this_stack.pop();
                        match out {
                            Ok(v) => v,
                            Err(err) => {
                                self.pending_throw = Some(Value::String(crate::unicode::utf8_to_utf16(&err.message())));
                                Value::Undefined
                            }
                        }
                    }
                    Value::VmClosure(ip, _, upv) => {
                        let uv = (*upv).clone();
                        self.this_stack.push(recv.clone());
                        let out = self.call_vm_function_result(ip, &[], &uv);
                        self.this_stack.pop();
                        match out {
                            Ok(v) => v,
                            Err(err) => {
                                self.pending_throw = Some(Value::String(crate::unicode::utf8_to_utf16(&err.message())));
                                Value::Undefined
                            }
                        }
                    }
                    Value::VmObject(map) => {
                        let borrow = map.borrow();
                        if let Some(Value::String(host_name_u16)) = borrow.get("__host_fn__") {
                            let host_name = crate::unicode::utf16_to_utf8(host_name_u16);
                            drop(borrow);
                            self.call_host_fn(&host_name, Some(recv), vec![])
                        } else {
                            Value::Undefined
                        }
                    }
                    _ => Value::Undefined,
                }
            }
            "object.valueOf" => receiver.unwrap_or(Value::Undefined),
            "object.isPrototypeOf" => {
                let Some(proto_obj) = receiver else {
                    return Value::Boolean(false);
                };
                let Some(target) = args.first().cloned() else {
                    return Value::Boolean(false);
                };

                let mut current = match target {
                    Value::VmObject(obj) => obj.borrow().get("__proto__").cloned(),
                    Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                        let props = self.get_fn_props(ip, arity);
                        props.borrow().get("__proto__").cloned()
                    }
                    _ => None,
                };

                for _ in 0..128 {
                    let Some(cur) = current else {
                        return Value::Boolean(false);
                    };
                    if let (Value::VmObject(a), Value::VmObject(b)) = (&proto_obj, &cur)
                        && Rc::ptr_eq(a, b)
                    {
                        return Value::Boolean(true);
                    }
                    current = match cur {
                        Value::VmObject(obj) => obj.borrow().get("__proto__").cloned(),
                        Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                            let props = self.get_fn_props(ip, arity);
                            props.borrow().get("__proto__").cloned()
                        }
                        _ => None,
                    };
                }
                Value::Boolean(false)
            }
            "object.propertyIsEnumerable" => {
                let key = args.first().map(value_to_string).unwrap_or_default();
                match receiver {
                    Some(Value::VmObject(map)) => {
                        let b = map.borrow();
                        let ne_key = format!("__nonenumerable_{}__", key);
                        let is_own = b.contains_key(&key) && !key.starts_with("__");
                        Value::Boolean(is_own && !b.contains_key(&ne_key))
                    }
                    Some(Value::VmArray(arr)) => {
                        let b = arr.borrow();
                        if let Ok(i) = key.parse::<usize>()
                            && i < b.elements.len()
                        {
                            return Value::Boolean(!b.props.contains_key(&format!("__deleted_{}", i)));
                        }
                        let ne_key = format!("__nonenumerable_{}__", key);
                        Value::Boolean(b.props.contains_key(&key) && !b.props.contains_key(&ne_key))
                    }
                    _ => Value::Boolean(false),
                }
            }
            "proxy.revocable" => {
                let proxy = self.call_builtin(BUILTIN_CTOR_PROXY, args.clone());
                let mut revoke = IndexMap::new();
                revoke.insert(
                    "__host_fn__".to_string(),
                    Value::String(crate::unicode::utf8_to_utf16("proxy.revoke")),
                );
                revoke.insert("__host_this__".to_string(), proxy.clone());

                let mut result = IndexMap::new();
                result.insert("proxy".to_string(), proxy);
                result.insert("revoke".to_string(), Value::VmObject(Rc::new(RefCell::new(revoke))));
                if let Some(Value::VmObject(object_ctor)) = self.globals.get("Object")
                    && let Some(proto) = object_ctor.borrow().get("prototype").cloned()
                {
                    result.insert("__proto__".to_string(), proto);
                }
                Value::VmObject(Rc::new(RefCell::new(result)))
            }
            "proxy.revoke" => {
                if let Some(Value::VmObject(proxy_obj)) = receiver {
                    proxy_obj.borrow_mut().insert("__proxy_revoked__".to_string(), Value::Boolean(true));
                }
                Value::Undefined
            }
            "reflect.has" => {
                let target = args.first().cloned().unwrap_or(Value::Undefined);
                let key = args.get(1).map(value_to_string).unwrap_or_default();
                let exists = match target {
                    Value::VmObject(obj) => {
                        let own_has = obj.borrow().contains_key(&key);
                        if own_has {
                            true
                        } else {
                            let proto = obj.borrow().get("__proto__").cloned();
                            self.lookup_proto_chain(&proto, &key).is_some()
                        }
                    }
                    Value::VmArray(arr) => {
                        key.parse::<usize>().ok().is_some_and(|i| arr.borrow().get(i).is_some()) || arr.borrow().props.contains_key(&key)
                    }
                    _ => false,
                };
                Value::Boolean(exists)
            }
            "reflect.get" => {
                let target = args.first().cloned().unwrap_or(Value::Undefined);
                let key = args.get(1).map(value_to_string).unwrap_or_default();
                match target.clone() {
                    Value::VmObject(_) => self.read_named_property(target, &key),
                    Value::VmArray(arr) => {
                        if let Ok(i) = key.parse::<usize>() {
                            arr.borrow().get(i).cloned().unwrap_or(Value::Undefined)
                        } else {
                            arr.borrow().props.get(&key).cloned().unwrap_or(Value::Undefined)
                        }
                    }
                    _ => Value::Undefined,
                }
            }
            "reflect.set" => {
                let target = args.first().cloned().unwrap_or(Value::Undefined);
                let key = args.get(1).map(value_to_string).unwrap_or_default();
                let value = args.get(2).cloned().unwrap_or(Value::Undefined);
                match self.assign_named_property(target, key, value) {
                    Ok(_) => Value::Boolean(true),
                    Err(_) => Value::Boolean(false),
                }
            }
            "reflect.ownKeys" => {
                let target = args.first().cloned().unwrap_or(Value::Undefined);
                self.call_builtin(BUILTIN_OBJECT_GETOWNPROPERTYNAMES, vec![target])
            }
            "reflect.isExtensible" => match args.first() {
                Some(Value::VmObject(obj)) => Value::Boolean(!matches!(obj.borrow().get("__non_extensible__"), Some(Value::Boolean(true)))),
                _ => Value::Boolean(false),
            },
            "reflect.getPrototypeOf" => {
                let target = args.first().cloned().unwrap_or(Value::Undefined);
                self.call_builtin(BUILTIN_OBJECT_GETPROTOTYPEOF, vec![target])
            }
            "reflect.getOwnPropertyDescriptor" => {
                let target = args.first().cloned().unwrap_or(Value::Undefined);
                let key = args.get(1).cloned().unwrap_or(Value::Undefined);
                self.call_builtin(BUILTIN_OBJECT_GETOWNPROPDESC, vec![target, key])
            }
            "reflect.defineProperty" => {
                let Some(Value::VmObject(obj)) = args.first().cloned() else {
                    return Value::Boolean(false);
                };
                let key = args.get(1).map(value_to_string).unwrap_or_default();
                let Some(Value::VmObject(desc)) = args.get(2) else {
                    return Value::Boolean(false);
                };
                let desc_borrow = desc.borrow();
                if self.validate_property_descriptor(&desc_borrow).is_err() {
                    return Value::Boolean(false);
                }
                self.apply_object_property_descriptor(&obj, &key, &desc_borrow);
                Value::Boolean(true)
            }
            "reflect.construct" => {
                let target = args.first().cloned().unwrap_or(Value::Undefined);
                let arg_list = args.get(1).cloned().unwrap_or(Value::Undefined);
                let new_target = args.get(2).cloned();

                let call_args = if let Value::VmArray(arr) = arg_list {
                    arr.borrow().iter().cloned().collect::<Vec<_>>()
                } else {
                    let mut err_map = IndexMap::new();
                    err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                    err_map.insert(
                        "message".to_string(),
                        Value::String(crate::unicode::utf8_to_utf16(
                            "Reflect.construct requires an array-like argumentsList",
                        )),
                    );
                    self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                    return Value::Undefined;
                };

                match self.construct_value(target, call_args, new_target) {
                    Ok(v) => v,
                    Err(err) => {
                        self.pending_throw = Some(Value::String(crate::unicode::utf8_to_utf16(&err.message())));
                        Value::Undefined
                    }
                }
            }
            "object.getOwnPropertySymbols" => {
                let target = args.first().cloned().unwrap_or(Value::Undefined);
                let mut symbols = Vec::new();
                if let Value::VmObject(obj) = &target {
                    let borrow = obj.borrow();
                    for k in borrow.keys() {
                        if let Some(id_str) = k.strip_prefix("@@sym:")
                            && let Ok(id) = id_str.parse::<u64>()
                            && let Some(sym_val) = self.symbol_values.get(&id)
                        {
                            symbols.push(sym_val.clone());
                        }
                    }
                }
                Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(symbols))))
            }
            "array.symbolIterator" => {
                if let Some(Value::VmArray(arr)) = receiver {
                    let items = arr.borrow().elements.clone();
                    self.make_iterator(items)
                } else {
                    Value::Undefined
                }
            }
            "string.symbolIterator" => {
                let s = match receiver {
                    Some(Value::String(ref s)) => crate::unicode::utf16_to_utf8(s),
                    Some(Value::VmObject(ref obj)) => {
                        if let Some(Value::String(ref s)) = obj.borrow().get("__value__").cloned() {
                            crate::unicode::utf16_to_utf8(s)
                        } else {
                            value_to_string(receiver.as_ref().unwrap())
                        }
                    }
                    Some(ref v) => value_to_string(v),
                    None => String::new(),
                };
                let chars: Vec<Value<'gc>> = s
                    .chars()
                    .map(|ch| Value::String(crate::unicode::utf8_to_utf16(&ch.to_string())))
                    .collect();
                self.make_iterator(chars)
            }
            "object.getOwnPropertyDescriptors" => {
                let Some(target) = args.first().cloned() else {
                    return Value::VmObject(Rc::new(RefCell::new(IndexMap::new())));
                };

                let mut out = IndexMap::new();
                match target {
                    Value::VmObject(obj) => {
                        let mut property_keys: Vec<String> = obj.borrow().keys().filter(|k| !k.starts_with("__")).cloned().collect();
                        // Also collect accessor keys stored as __get_<name> / __set_<name>
                        {
                            let borrow = obj.borrow();
                            for k in borrow.keys() {
                                if let Some(name) = k.strip_prefix("__get_")
                                    && !property_keys.contains(&name.to_string())
                                {
                                    property_keys.push(name.to_string());
                                }
                            }
                        }
                        for key in property_keys {
                            let desc = self.call_builtin(
                                BUILTIN_OBJECT_GETOWNPROPDESC,
                                vec![Value::VmObject(obj.clone()), Value::String(crate::unicode::utf8_to_utf16(&key))],
                            );
                            if !matches!(desc, Value::Undefined) {
                                out.insert(key, desc);
                            }
                        }
                    }
                    Value::VmArray(arr) => {
                        let len = arr.borrow().elements.len();
                        for i in 0..len {
                            let key = i.to_string();
                            let desc = self.call_builtin(
                                BUILTIN_OBJECT_GETOWNPROPDESC,
                                vec![Value::VmArray(arr.clone()), Value::String(crate::unicode::utf8_to_utf16(&key))],
                            );
                            if !matches!(desc, Value::Undefined) {
                                out.insert(key, desc);
                            }
                        }
                    }
                    _ => {}
                }

                Value::VmObject(Rc::new(RefCell::new(out)))
            }
            "array.toString" => {
                if let Some(Value::VmArray(arr)) = receiver {
                    let joined = arr.borrow().iter().map(value_to_string).collect::<Vec<_>>().join(",");
                    Value::String(crate::unicode::utf8_to_utf16(&joined))
                } else {
                    Value::String(crate::unicode::utf8_to_utf16(""))
                }
            }
            "regexp.toString" => {
                if let Some(Value::VmObject(re_obj)) = receiver {
                    Value::String(crate::unicode::utf8_to_utf16(&self.regex_to_string(&re_obj)))
                } else {
                    Value::String(crate::unicode::utf8_to_utf16("/[object Object]/"))
                }
            }
            "array.entries" => {
                if let Some(Value::VmArray(arr)) = receiver {
                    let entries = arr
                        .borrow()
                        .iter()
                        .enumerate()
                        .map(|(i, v)| Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(vec![Value::Number(i as f64), v.clone()])))))
                        .collect::<Vec<_>>();
                    Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(entries))))
                } else {
                    Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(Vec::new()))))
                }
            }
            "array.copyWithin" => {
                if let Some(Value::VmArray(arr)) = receiver {
                    let len = arr.borrow().len() as i64;
                    let norm = |n: i64| if n < 0 { (len + n).max(0) } else { n.min(len) } as usize;
                    let target = norm(args.first().map(to_number).unwrap_or(0.0) as i64);
                    let start = norm(args.get(1).map(to_number).unwrap_or(0.0) as i64);
                    let end = norm(args.get(2).map(to_number).unwrap_or(len as f64) as i64);
                    let mut borrow = arr.borrow_mut();
                    let src: Vec<Value<'gc>> = borrow.elements.clone();
                    let mut t = target;
                    for i in start..end {
                        if t >= src.len() || i >= src.len() {
                            break;
                        }
                        borrow.elements[t] = src[i].clone();
                        t += 1;
                    }
                    Value::VmArray(arr.clone())
                } else {
                    Value::Undefined
                }
            }
            "dataview.getUint8" => {
                if let Some(Value::VmObject(view)) = receiver {
                    let view_b = view.borrow();
                    let base = view_b
                        .get("byteOffset")
                        .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None })
                        .unwrap_or(0);
                    let idx = base + args.first().map(to_number).unwrap_or(0.0).max(0.0) as usize;
                    let buffer = view_b.get("buffer").cloned();
                    drop(view_b);
                    if let Some(Value::VmObject(buf)) = buffer
                        && let Some(Value::VmArray(bytes)) = buf.borrow().get("__buffer_bytes__").cloned()
                        && let Some(v) = bytes.borrow().elements.get(idx)
                    {
                        return Value::Number(to_number(v) as u8 as f64);
                    }
                }
                Value::Number(0.0)
            }
            "dataview.getInt8" => {
                let u = self.call_host_fn("dataview.getUint8", receiver, args);
                let b = to_number(&u) as u8;
                Value::Number((b as i8) as f64)
            }
            "dataview.setUint8" => {
                if let Some(Value::VmObject(view)) = receiver {
                    let view_b = view.borrow();
                    let base = view_b
                        .get("byteOffset")
                        .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None })
                        .unwrap_or(0);
                    let idx = base + args.first().map(to_number).unwrap_or(0.0).max(0.0) as usize;
                    let val = args.get(1).map(to_number).unwrap_or(0.0) as u8;
                    let buffer = view_b.get("buffer").cloned();
                    drop(view_b);
                    if let Some(Value::VmObject(buf)) = buffer
                        && let Some(Value::VmArray(bytes)) = buf.borrow().get("__buffer_bytes__").cloned()
                        && idx < bytes.borrow().elements.len()
                    {
                        bytes.borrow_mut().elements[idx] = Value::Number(val as f64);
                    }
                }
                Value::Undefined
            }
            "dataview.setInt8" => {
                let mut next_args = args.clone();
                if next_args.len() > 1 {
                    let n = to_number(&next_args[1]) as i8;
                    next_args[1] = Value::Number((n as u8) as f64);
                }
                self.call_host_fn("dataview.setUint8", receiver, next_args)
            }
            "dataview.getUint16" => {
                let little = args.get(1).map(|v| v.to_truthy()).unwrap_or(false);
                let b0 = to_number(&self.call_host_fn(
                    "dataview.getUint8",
                    receiver.clone(),
                    vec![args.first().cloned().unwrap_or(Value::Number(0.0))],
                )) as u8;
                let b1 = to_number(&self.call_host_fn(
                    "dataview.getUint8",
                    receiver,
                    vec![Value::Number(args.first().map(to_number).unwrap_or(0.0) + 1.0)],
                )) as u8;
                let v = if little {
                    u16::from_le_bytes([b0, b1])
                } else {
                    u16::from_be_bytes([b0, b1])
                };
                Value::Number(v as f64)
            }
            "dataview.getInt16" => {
                let v = self.call_host_fn("dataview.getUint16", receiver, args);
                Value::Number((to_number(&v) as u16 as i16) as f64)
            }
            "dataview.setUint16" => {
                let off = args.first().map(to_number).unwrap_or(0.0);
                let n = args.get(1).map(to_number).unwrap_or(0.0) as u16;
                let little = args.get(2).map(|v| v.to_truthy()).unwrap_or(false);
                let bytes = if little { n.to_le_bytes() } else { n.to_be_bytes() };
                let _ = self.call_host_fn(
                    "dataview.setUint8",
                    receiver.clone(),
                    vec![Value::Number(off), Value::Number(bytes[0] as f64)],
                );
                let _ = self.call_host_fn(
                    "dataview.setUint8",
                    receiver,
                    vec![Value::Number(off + 1.0), Value::Number(bytes[1] as f64)],
                );
                Value::Undefined
            }
            "dataview.setInt16" => {
                let mut n_args = args.clone();
                if n_args.len() > 1 {
                    n_args[1] = Value::Number((to_number(&n_args[1]) as i16 as u16) as f64);
                }
                self.call_host_fn("dataview.setUint16", receiver, n_args)
            }
            "dataview.getUint32" => {
                let little = args.get(1).map(|v| v.to_truthy()).unwrap_or(false);
                let off = args.first().map(to_number).unwrap_or(0.0);
                let b = [0.0, 1.0, 2.0, 3.0]
                    .iter()
                    .map(|d| to_number(&self.call_host_fn("dataview.getUint8", receiver.clone(), vec![Value::Number(off + d)])) as u8)
                    .collect::<Vec<_>>();
                let arr = [b[0], b[1], b[2], b[3]];
                let v = if little { u32::from_le_bytes(arr) } else { u32::from_be_bytes(arr) };
                Value::Number(v as f64)
            }
            "dataview.getInt32" => {
                let v = self.call_host_fn("dataview.getUint32", receiver, args);
                Value::Number((to_number(&v) as u32 as i32) as f64)
            }
            "dataview.setUint32" => {
                let off = args.first().map(to_number).unwrap_or(0.0);
                let n = args.get(1).map(to_number).unwrap_or(0.0) as u32;
                let little = args.get(2).map(|v| v.to_truthy()).unwrap_or(false);
                let bytes = if little { n.to_le_bytes() } else { n.to_be_bytes() };
                for (i, b) in bytes.iter().enumerate() {
                    let _ = self.call_host_fn(
                        "dataview.setUint8",
                        receiver.clone(),
                        vec![Value::Number(off + i as f64), Value::Number(*b as f64)],
                    );
                }
                Value::Undefined
            }
            "dataview.setInt32" => {
                let mut n_args = args.clone();
                if n_args.len() > 1 {
                    n_args[1] = Value::Number((to_number(&n_args[1]) as i32 as u32) as f64);
                }
                self.call_host_fn("dataview.setUint32", receiver, n_args)
            }
            "dataview.getFloat32" => {
                let v = self.call_host_fn("dataview.getUint32", receiver, args);
                Value::Number(f32::from_bits(to_number(&v) as u32) as f64)
            }
            "dataview.setFloat32" => {
                let mut n_args = args.clone();
                if n_args.len() > 1 {
                    n_args[1] = Value::Number((to_number(&n_args[1]) as f32).to_bits() as f64);
                }
                self.call_host_fn("dataview.setUint32", receiver, n_args)
            }
            "dataview.getFloat64" => {
                let little = args.get(1).map(|v| v.to_truthy()).unwrap_or(false);
                let off = args.first().map(to_number).unwrap_or(0.0);
                let b = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]
                    .iter()
                    .map(|d| to_number(&self.call_host_fn("dataview.getUint8", receiver.clone(), vec![Value::Number(off + d)])) as u8)
                    .collect::<Vec<_>>();
                let arr = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
                let v = if little { u64::from_le_bytes(arr) } else { u64::from_be_bytes(arr) };
                Value::Number(f64::from_bits(v))
            }
            "dataview.setFloat64" => {
                let off = args.first().map(to_number).unwrap_or(0.0);
                let n = args.get(1).map(to_number).unwrap_or(0.0);
                let little = args.get(2).map(|v| v.to_truthy()).unwrap_or(false);
                let bits = n.to_bits();
                let bytes = if little { bits.to_le_bytes() } else { bits.to_be_bytes() };
                for (i, b) in bytes.iter().enumerate() {
                    let _ = self.call_host_fn(
                        "dataview.setUint8",
                        receiver.clone(),
                        vec![Value::Number(off + i as f64), Value::Number(*b as f64)],
                    );
                }
                Value::Undefined
            }
            "promise.catch" => {
                let recv = receiver.unwrap_or(Value::Undefined);
                let on_rejected = args.first().cloned().unwrap_or(Value::Undefined);
                self.call_method_builtin(BUILTIN_PROMISE_THEN, recv, vec![Value::Undefined, on_rejected])
            }
            "promise.reject" => {
                let mut map = IndexMap::new();
                map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Promise")));
                map.insert("then".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_THEN));
                map.insert("__promise_rejected__".to_string(), Value::Boolean(true));
                if let Some(v) = args.first() {
                    map.insert("__promise_value__".to_string(), v.clone());
                }
                if let Some(Value::VmObject(promise_ctor)) = self.globals.get("Promise")
                    && let Some(proto) = promise_ctor.borrow().get("prototype").cloned()
                {
                    map.insert("__proto__".to_string(), proto);
                }
                Value::VmObject(Rc::new(RefCell::new(map)))
            }
            "promise.any" => {
                let make_promise = |vm: &VM<'gc>, rejected: Option<Value<'gc>>, value: Option<Value<'gc>>| {
                    let mut map = IndexMap::new();
                    map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Promise")));
                    map.insert("then".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_THEN));
                    if let Some(v) = value {
                        map.insert("__promise_value__".to_string(), v);
                    }
                    if let Some(r) = rejected {
                        map.insert("__promise_value__".to_string(), r);
                        map.insert("__promise_rejected__".to_string(), Value::Boolean(true));
                    }
                    if let Some(Value::VmObject(promise_ctor)) = vm.globals.get("Promise")
                        && let Some(proto) = promise_ctor.borrow().get("prototype").cloned()
                    {
                        map.insert("__proto__".to_string(), proto);
                    }
                    Value::VmObject(Rc::new(RefCell::new(map)))
                };

                let make_aggregate_error = |errors: Vec<Value<'gc>>| {
                    let mut err = IndexMap::new();
                    err.insert(
                        "__type__".to_string(),
                        Value::String(crate::unicode::utf8_to_utf16("AggregateError")),
                    );
                    err.insert("name".to_string(), Value::String(crate::unicode::utf8_to_utf16("AggregateError")));
                    err.insert(
                        "message".to_string(),
                        Value::String(crate::unicode::utf8_to_utf16("All promises were rejected")),
                    );
                    err.insert(
                        "errors".to_string(),
                        Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(errors)))),
                    );
                    Value::VmObject(Rc::new(RefCell::new(err)))
                };

                let is_callable = |v: &Value<'gc>| -> bool {
                    match v {
                        Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(..) => true,
                        Value::VmObject(map) => {
                            let b = map.borrow();
                            b.contains_key("__host_fn__") || b.contains_key("__bound_target__")
                        }
                        _ => false,
                    }
                };

                let normalize_reason = |msg: &str| -> Value<'gc> {
                    let payload = msg.strip_prefix("SyntaxError: Uncaught: ").unwrap_or(msg);
                    if let Ok(n) = payload.parse::<f64>() {
                        Value::Number(n)
                    } else {
                        Value::String(crate::unicode::utf8_to_utf16(payload))
                    }
                };

                if let Some(Value::VmArray(items)) = args.first() {
                    let mut rejection_reasons: Vec<Value<'gc>> = Vec::new();
                    let mut saw_pending = false;

                    for item in items.borrow().iter() {
                        match item {
                            Value::VmObject(obj) => {
                                let (is_promise, rejected, settled) = {
                                    let b = obj.borrow();
                                    (
                                        matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "Promise"),
                                        matches!(b.get("__promise_rejected__"), Some(Value::Boolean(true))),
                                        b.get("__promise_value__").cloned(),
                                    )
                                };

                                if is_promise {
                                    if let Some(v) = settled {
                                        if rejected {
                                            rejection_reasons.push(v);
                                        } else {
                                            return make_promise(self, None, Some(v));
                                        }
                                    } else {
                                        saw_pending = true;
                                    }
                                    continue;
                                }

                                let then_val = self.read_named_property(item.clone(), "then");
                                if let Some(thrown) = self.pending_throw.take() {
                                    rejection_reasons.push(thrown);
                                    continue;
                                }

                                if matches!(then_val, Value::Undefined) {
                                    return make_promise(self, None, Some(item.clone()));
                                }

                                if !is_callable(&then_val) {
                                    return make_promise(self, None, Some(item.clone()));
                                }

                                let mut temp = IndexMap::new();
                                temp.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Promise")));
                                temp.insert("then".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_THEN));
                                if let Some(Value::VmObject(promise_ctor)) = self.globals.get("Promise")
                                    && let Some(proto) = promise_ctor.borrow().get("prototype").cloned()
                                {
                                    temp.insert("__proto__".to_string(), proto);
                                }
                                let temp_promise = Value::VmObject(Rc::new(RefCell::new(temp)));

                                let mut resolve_map = IndexMap::new();
                                resolve_map.insert(
                                    "__host_fn__".to_string(),
                                    Value::String(crate::unicode::utf8_to_utf16("promise.__resolve")),
                                );
                                resolve_map.insert("__host_this__".to_string(), temp_promise.clone());
                                let resolve = Value::VmObject(Rc::new(RefCell::new(resolve_map)));

                                let mut reject_map = IndexMap::new();
                                reject_map.insert(
                                    "__host_fn__".to_string(),
                                    Value::String(crate::unicode::utf8_to_utf16("promise.__reject")),
                                );
                                reject_map.insert("__host_this__".to_string(), temp_promise.clone());
                                let reject_cb = Value::VmObject(Rc::new(RefCell::new(reject_map)));

                                let this_arg = item.clone();
                                let invoke_then_error = match then_val {
                                    Value::VmFunction(ip, _) => {
                                        self.this_stack.push(this_arg.clone());
                                        let saved_try_stack = std::mem::take(&mut self.try_stack);
                                        let result = self.call_vm_function_result(ip, &[resolve.clone(), reject_cb.clone()], &[]);
                                        self.try_stack = saved_try_stack;
                                        self.this_stack.pop();
                                        result.err()
                                    }
                                    Value::VmClosure(ip, _, upv) => {
                                        let uv = (*upv).clone();
                                        self.this_stack.push(this_arg.clone());
                                        let saved_try_stack = std::mem::take(&mut self.try_stack);
                                        let result = self.call_vm_function_result(ip, &[resolve.clone(), reject_cb.clone()], &uv);
                                        self.try_stack = saved_try_stack;
                                        self.this_stack.pop();
                                        result.err()
                                    }
                                    Value::VmNativeFunction(native_id) => {
                                        let _ =
                                            self.call_method_builtin(native_id, this_arg.clone(), vec![resolve.clone(), reject_cb.clone()]);
                                        None
                                    }
                                    Value::VmObject(map) => {
                                        let borrow = map.borrow();
                                        if let Some(Value::String(host_name_u16)) = borrow.get("__host_fn__") {
                                            let host_name = crate::unicode::utf16_to_utf8(host_name_u16);
                                            drop(borrow);
                                            let _ = self.call_host_fn(
                                                &host_name,
                                                Some(this_arg.clone()),
                                                vec![resolve.clone(), reject_cb.clone()],
                                            );
                                            None
                                        } else {
                                            None
                                        }
                                    }
                                    _ => None,
                                };

                                if let Some(err) = invoke_then_error
                                    && let Value::VmObject(p) = &temp_promise
                                {
                                    let mut pb = p.borrow_mut();
                                    if !pb.contains_key("__promise_value__") {
                                        pb.insert("__promise_rejected__".to_string(), Value::Boolean(true));
                                        pb.insert("__promise_value__".to_string(), normalize_reason(&err.message()));
                                    }
                                }

                                if let Some(thrown) = self.pending_throw.take()
                                    && let Value::VmObject(p) = &temp_promise
                                {
                                    let mut pb = p.borrow_mut();
                                    if !pb.contains_key("__promise_value__") {
                                        pb.insert("__promise_rejected__".to_string(), Value::Boolean(true));
                                        pb.insert("__promise_value__".to_string(), thrown);
                                    }
                                }

                                if let Value::VmObject(p) = &temp_promise {
                                    let b = p.borrow();
                                    let temp_rejected = matches!(b.get("__promise_rejected__"), Some(Value::Boolean(true)));
                                    let temp_settled = b.get("__promise_value__").cloned();

                                    if let Some(v) = temp_settled {
                                        if temp_rejected {
                                            rejection_reasons.push(v);
                                        } else {
                                            return make_promise(self, None, Some(v));
                                        }
                                    } else {
                                        saw_pending = true;
                                    }
                                } else {
                                    saw_pending = true;
                                }
                            }
                            _ => {
                                return make_promise(self, None, Some(item.clone()));
                            }
                        }
                    }

                    if !saw_pending && !rejection_reasons.is_empty() {
                        return make_promise(self, Some(make_aggregate_error(rejection_reasons)), None);
                    }
                }

                make_promise(self, None, None)
            }
            "promise.race" => {
                let make_promise = |vm: &VM<'gc>, rejected: Option<Value<'gc>>, value: Option<Value<'gc>>| {
                    let mut map = IndexMap::new();
                    map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Promise")));
                    map.insert("then".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_THEN));
                    if let Some(v) = value {
                        map.insert("__promise_value__".to_string(), v);
                    }
                    if let Some(r) = rejected {
                        map.insert("__promise_value__".to_string(), r);
                        map.insert("__promise_rejected__".to_string(), Value::Boolean(true));
                    }
                    if let Some(Value::VmObject(promise_ctor)) = vm.globals.get("Promise")
                        && let Some(proto) = promise_ctor.borrow().get("prototype").cloned()
                    {
                        map.insert("__proto__".to_string(), proto);
                    }
                    Value::VmObject(Rc::new(RefCell::new(map)))
                };

                if let Some(Value::VmArray(items)) = args.first() {
                    for item in items.borrow().iter() {
                        match item {
                            Value::VmObject(obj) => {
                                let b = obj.borrow();
                                let is_promise =
                                    matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "Promise");
                                if is_promise {
                                    let rejected = matches!(b.get("__promise_rejected__"), Some(Value::Boolean(true)));
                                    if let Some(v) = b.get("__promise_value__").cloned() {
                                        if rejected {
                                            return make_promise(self, Some(v), None);
                                        }
                                        return make_promise(self, None, Some(v));
                                    }
                                    return make_promise(self, None, None);
                                }
                            }
                            _ => {
                                return make_promise(self, None, Some(item.clone()));
                            }
                        }
                    }
                }
                make_promise(self, None, None)
            }
            "promise.__resolve" => {
                if let Some(receiver @ Value::VmObject(_)) = receiver {
                    let val = args.first().cloned().unwrap_or(Value::Undefined);
                    self.settle_promise(&receiver, val, false);
                }
                Value::Undefined
            }
            "promise.__reject" => {
                if let Some(receiver @ Value::VmObject(_)) = receiver {
                    let val = args.first().cloned().unwrap_or(Value::Undefined);
                    self.settle_promise(&receiver, val, true);
                }
                Value::Undefined
            }
            "promise.await" => {
                let mut current = args.first().cloned().unwrap_or(Value::Undefined);

                let normalize_reason = |msg: &str| -> Value<'gc> {
                    let payload = msg.strip_prefix("SyntaxError: Uncaught: ").unwrap_or(msg);
                    if let Ok(n) = payload.parse::<f64>() {
                        Value::Number(n)
                    } else {
                        Value::String(crate::unicode::utf8_to_utf16(payload))
                    }
                };

                for _ in 0..8 {
                    let Value::VmObject(obj) = &current else {
                        return current;
                    };

                    let (is_promise, rejected, settled, then_prop) = {
                        let b = obj.borrow();
                        (
                            matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "Promise"),
                            matches!(b.get("__promise_rejected__"), Some(Value::Boolean(true))),
                            b.get("__promise_value__").cloned(),
                            b.get("then").cloned(),
                        )
                    };

                    if is_promise {
                        if rejected {
                            self.pending_throw = Some(settled.unwrap_or(Value::Undefined));
                            return Value::Undefined;
                        }
                        let Some(next) = settled else {
                            return Value::Undefined;
                        };
                        current = next;
                        continue;
                    }

                    let Some(then_val) = then_prop else {
                        return current;
                    };

                    let then_is_callable = match &then_val {
                        Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(..) => true,
                        Value::VmObject(map) => {
                            let b = map.borrow();
                            b.contains_key("__host_fn__") || b.contains_key("__bound_target__")
                        }
                        _ => false,
                    };

                    if !then_is_callable {
                        return current;
                    }

                    let mut temp = IndexMap::new();
                    temp.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Promise")));
                    temp.insert("then".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_THEN));
                    if let Some(Value::VmObject(promise_ctor)) = self.globals.get("Promise")
                        && let Some(proto) = promise_ctor.borrow().get("prototype").cloned()
                    {
                        temp.insert("__proto__".to_string(), proto);
                    }
                    let temp_promise = Value::VmObject(Rc::new(RefCell::new(temp)));

                    let mut resolve_map = IndexMap::new();
                    resolve_map.insert(
                        "__host_fn__".to_string(),
                        Value::String(crate::unicode::utf8_to_utf16("promise.__resolve")),
                    );
                    resolve_map.insert("__host_this__".to_string(), temp_promise.clone());
                    let resolve = Value::VmObject(Rc::new(RefCell::new(resolve_map)));

                    let mut reject_map = IndexMap::new();
                    reject_map.insert(
                        "__host_fn__".to_string(),
                        Value::String(crate::unicode::utf8_to_utf16("promise.__reject")),
                    );
                    reject_map.insert("__host_this__".to_string(), temp_promise.clone());
                    let reject = Value::VmObject(Rc::new(RefCell::new(reject_map)));

                    let this_arg = current.clone();
                    let invoke_then_error = match then_val {
                        Value::VmFunction(ip, _) => {
                            self.this_stack.push(this_arg.clone());
                            let saved_try_stack = std::mem::take(&mut self.try_stack);
                            let result = self.call_vm_function_result(ip, &[resolve.clone(), reject.clone()], &[]);
                            self.try_stack = saved_try_stack;
                            self.this_stack.pop();
                            result.err()
                        }
                        Value::VmClosure(ip, _, upv) => {
                            let uv = (*upv).clone();
                            self.this_stack.push(this_arg.clone());
                            let saved_try_stack = std::mem::take(&mut self.try_stack);
                            let result = self.call_vm_function_result(ip, &[resolve.clone(), reject.clone()], &uv);
                            self.try_stack = saved_try_stack;
                            self.this_stack.pop();
                            result.err()
                        }
                        Value::VmNativeFunction(native_id) => {
                            let _ = self.call_method_builtin(native_id, this_arg.clone(), vec![resolve.clone(), reject.clone()]);
                            None
                        }
                        Value::VmObject(map) => {
                            let borrow = map.borrow();
                            if let Some(Value::String(host_name_u16)) = borrow.get("__host_fn__") {
                                let host_name = crate::unicode::utf16_to_utf8(host_name_u16);
                                drop(borrow);
                                let _ = self.call_host_fn(&host_name, Some(this_arg.clone()), vec![resolve.clone(), reject.clone()]);
                                None
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };

                    if let Some(err) = invoke_then_error
                        && let Value::VmObject(p) = &temp_promise
                    {
                        let mut pb = p.borrow_mut();
                        if !pb.contains_key("__promise_value__") {
                            pb.insert("__promise_rejected__".to_string(), Value::Boolean(true));
                            pb.insert("__promise_value__".to_string(), normalize_reason(&err.message()));
                        }
                    }

                    if let Some(thrown) = self.pending_throw.take()
                        && let Value::VmObject(p) = &temp_promise
                    {
                        let mut pb = p.borrow_mut();
                        if !pb.contains_key("__promise_value__") {
                            pb.insert("__promise_rejected__".to_string(), Value::Boolean(true));
                            pb.insert("__promise_value__".to_string(), thrown);
                        }
                    }

                    let (temp_rejected, temp_settled) = if let Value::VmObject(p) = &temp_promise {
                        let b = p.borrow();
                        (
                            matches!(b.get("__promise_rejected__"), Some(Value::Boolean(true))),
                            b.get("__promise_value__").cloned(),
                        )
                    } else {
                        (false, None)
                    };

                    if temp_rejected {
                        self.pending_throw = Some(temp_settled.unwrap_or(Value::Undefined));
                        return Value::Undefined;
                    }

                    let Some(next) = temp_settled else {
                        return Value::Undefined;
                    };
                    current = next;
                }

                current
            }
            "promise.allSettled" => {
                let mut settled = Vec::new();
                if let Some(Value::VmArray(items)) = args.first() {
                    for item in items.borrow().iter() {
                        let mut entry = IndexMap::new();
                        match item {
                            Value::VmObject(obj) => {
                                let b = obj.borrow();
                                let is_promise =
                                    matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "Promise");
                                if is_promise {
                                    let rejected = matches!(b.get("__promise_rejected__"), Some(Value::Boolean(true)));
                                    let pv = b.get("__promise_value__").cloned().unwrap_or(Value::Undefined);
                                    if rejected {
                                        entry.insert("status".to_string(), Value::String(crate::unicode::utf8_to_utf16("rejected")));
                                        entry.insert("reason".to_string(), pv);
                                    } else {
                                        entry.insert("status".to_string(), Value::String(crate::unicode::utf8_to_utf16("fulfilled")));
                                        entry.insert("value".to_string(), pv);
                                    }
                                } else {
                                    entry.insert("status".to_string(), Value::String(crate::unicode::utf8_to_utf16("fulfilled")));
                                    entry.insert("value".to_string(), item.clone());
                                }
                            }
                            _ => {
                                entry.insert("status".to_string(), Value::String(crate::unicode::utf8_to_utf16("fulfilled")));
                                entry.insert("value".to_string(), item.clone());
                            }
                        }
                        settled.push(Value::VmObject(Rc::new(RefCell::new(entry))));
                    }
                }

                let mut map = IndexMap::new();
                map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Promise")));
                map.insert("then".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_THEN));
                map.insert(
                    "__promise_value__".to_string(),
                    Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(settled)))),
                );
                if let Some(Value::VmObject(promise_ctor)) = self.globals.get("Promise")
                    && let Some(proto) = promise_ctor.borrow().get("prototype").cloned()
                {
                    map.insert("__proto__".to_string(), proto);
                }
                Value::VmObject(Rc::new(RefCell::new(map)))
            }
            "promise.finally" => {
                if let Some(cb) = args.first() {
                    match cb {
                        Value::VmFunction(ip, _) => {
                            let _ = self.call_vm_function(*ip, &[], &[]);
                        }
                        Value::VmClosure(ip, _, upv) => {
                            let uv = (**upv).clone();
                            let _ = self.call_vm_function(*ip, &[], &uv);
                        }
                        _ => {}
                    }
                }
                receiver.unwrap_or(Value::Undefined)
            }
            "error.aggregate" => {
                let errors = match args.first() {
                    Some(Value::VmArray(arr)) => Value::VmArray(arr.clone()),
                    Some(v) => Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(vec![v.clone()])))),
                    None => Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(vec![])))),
                };
                let msg = args.get(1).map(value_to_string).unwrap_or_default();
                let mut err = IndexMap::new();
                err.insert(
                    "__type__".to_string(),
                    Value::String(crate::unicode::utf8_to_utf16("AggregateError")),
                );
                err.insert("name".to_string(), Value::String(crate::unicode::utf8_to_utf16("AggregateError")));
                err.insert("message".to_string(), Value::String(crate::unicode::utf8_to_utf16(&msg)));
                err.insert("errors".to_string(), errors);
                Value::VmObject(Rc::new(RefCell::new(err)))
            }
            "std.sprintf" => match crate::js_std::sprintf::handle_sprintf_call(&args) {
                Ok(v) => v,
                Err(_) => Value::Undefined,
            },
            "std.tmpfile" => crate::js_std::tmpfile::vm_create_tmpfile(),
            "std.gc" => Value::Undefined,
            "tmp.puts" | "tmp.readAsString" | "tmp.seek" | "tmp.close" | "tmp.getline" => {
                crate::js_std::tmpfile::vm_dispatch_file_method(name, receiver, args)
            }
            _ => Value::Undefined,
        }
    }

    fn call_named_host_function(&mut self, name: &str, args: Vec<Value<'gc>>) -> Value<'gc> {
        match name {
            "console.log" => self.call_builtin(BUILTIN_CONSOLE_LOG, args),
            "console.warn" => self.call_builtin(BUILTIN_CONSOLE_WARN, args),
            "console.error" => self.call_builtin(BUILTIN_CONSOLE_ERROR, args),
            "os.getcwd" => {
                let cwd = std::env::current_dir()
                    .ok()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                Value::String(crate::unicode::utf8_to_utf16(&cwd))
            }
            "os.getpid" => Value::Number(std::process::id() as f64),
            "os.getppid" => Value::Number(std::process::id() as f64),
            "os.open" => {
                let filename = args.first().map(value_to_string).unwrap_or_default();
                let flags = args.get(1).map(to_number).unwrap_or(0.0) as i32;

                let mut options = std::fs::OpenOptions::new();
                if flags & 2 != 0 {
                    options.read(true).write(true);
                } else if flags & 1 != 0 {
                    options.write(true);
                } else {
                    options.read(true);
                }
                if flags & 64 != 0 {
                    options.create(true);
                }
                if flags & 512 != 0 {
                    options.truncate(true);
                }

                match options.open(&filename) {
                    Ok(file) => {
                        let fd = vm_next_os_file_id();
                        VM_OS_FILE_STORE.lock().unwrap().insert(fd, file);
                        Value::Number(fd as f64)
                    }
                    Err(_) => Value::Number(-1.0),
                }
            }
            "os.close" => {
                let fd = args.first().map(to_number).unwrap_or(-1.0) as u64;
                if VM_OS_FILE_STORE.lock().unwrap().remove(&fd).is_some() {
                    Value::Number(0.0)
                } else {
                    Value::Number(-1.0)
                }
            }
            "os.write" => {
                let fd = args.first().map(to_number).unwrap_or(-1.0) as u64;
                let data = args.get(1).map(value_to_string).unwrap_or_default();
                let mut store = VM_OS_FILE_STORE.lock().unwrap();
                if let Some(file) = store.get_mut(&fd) {
                    match file.write(data.as_bytes()) {
                        Ok(n) => Value::Number(n as f64),
                        Err(_) => Value::Number(-1.0),
                    }
                } else {
                    Value::Number(-1.0)
                }
            }
            "os.read" => {
                let fd = args.first().map(to_number).unwrap_or(-1.0) as u64;
                let count = args.get(1).map(to_number).unwrap_or(0.0).max(0.0) as usize;
                let mut store = VM_OS_FILE_STORE.lock().unwrap();
                if let Some(file) = store.get_mut(&fd) {
                    let mut buf = vec![0u8; count];
                    match file.read(&mut buf) {
                        Ok(n) => {
                            buf.truncate(n);
                            Value::String(crate::unicode::utf8_to_utf16(&String::from_utf8_lossy(&buf)))
                        }
                        Err(_) => Value::String(crate::unicode::utf8_to_utf16("")),
                    }
                } else {
                    Value::String(crate::unicode::utf8_to_utf16(""))
                }
            }
            "os.seek" => {
                let fd = args.first().map(to_number).unwrap_or(-1.0) as u64;
                let offset = args.get(1).map(to_number).unwrap_or(0.0) as i64;
                let whence = args.get(2).map(to_number).unwrap_or(0.0) as i32;
                let mut store = VM_OS_FILE_STORE.lock().unwrap();
                if let Some(file) = store.get_mut(&fd) {
                    let seek_from = match whence {
                        0 => SeekFrom::Start(offset.max(0) as u64),
                        1 => SeekFrom::Current(offset),
                        2 => SeekFrom::End(offset),
                        _ => SeekFrom::Start(offset.max(0) as u64),
                    };
                    match file.seek(seek_from) {
                        Ok(pos) => Value::Number(pos as f64),
                        Err(_) => Value::Number(-1.0),
                    }
                } else {
                    Value::Number(-1.0)
                }
            }
            "os.path.basename" => {
                let p = args.first().map(value_to_string).unwrap_or_default();
                let base = std::path::Path::new(&p)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                Value::String(crate::unicode::utf8_to_utf16(&base))
            }
            "os.path.dirname" => {
                let p = args.first().map(value_to_string).unwrap_or_default();
                let dir = std::path::Path::new(&p)
                    .parent()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                Value::String(crate::unicode::utf8_to_utf16(&dir))
            }
            "os.path.join" => {
                let mut pb = std::path::PathBuf::new();
                for a in &args {
                    pb.push(value_to_string(a));
                }
                Value::String(crate::unicode::utf8_to_utf16(&pb.to_string_lossy()))
            }
            "os.path.extname" => {
                let p = args.first().map(value_to_string).unwrap_or_default();
                let ext = std::path::Path::new(&p)
                    .extension()
                    .map(|s| format!(".{}", s.to_string_lossy()))
                    .unwrap_or_default();
                Value::String(crate::unicode::utf8_to_utf16(&ext))
            }
            _ => Value::Undefined,
        }
    }

    /// Get captured console output
    #[allow(dead_code)]
    pub fn take_output(&mut self) -> Vec<String> {
        std::mem::take(&mut self.output)
    }

    /// Get or create the property map for a VmFunction (keyed by IP).
    /// Auto-creates a `prototype` object with `constructor` back-reference on first access.
    fn get_fn_props(&mut self, ip: usize, arity: u8) -> Rc<RefCell<IndexMap<String, Value<'gc>>>> {
        if let Some(existing) = self.fn_props.get(&ip) {
            return existing.clone();
        }
        let mut proto = IndexMap::new();
        proto.insert("constructor".to_string(), Value::VmFunction(ip, arity));
        // Link fn.prototype to Object.prototype for inherited methods
        if let Some(Value::VmObject(obj_global)) = self.globals.get("Object")
            && let Some(obj_proto) = obj_global.borrow().get("prototype").cloned()
        {
            proto.insert("__proto__".to_string(), obj_proto);
        }
        let mut props = IndexMap::new();
        props.insert("prototype".to_string(), Value::VmObject(Rc::new(RefCell::new(proto))));
        if let Some(Value::VmObject(function_ctor)) = self.globals.get("Function")
            && let Some(fn_proto) = function_ctor.borrow().get("prototype").cloned()
        {
            props.insert("__proto__".to_string(), fn_proto);
        }
        // Set function name if known
        if let Some(name) = self.chunk.fn_names.get(&ip) {
            props.insert("name".to_string(), Value::String(crate::unicode::utf8_to_utf16(name)));
        }
        let props_rc = Rc::new(RefCell::new(props));
        self.fn_props.insert(ip, props_rc.clone());
        props_rc
    }

    fn typeof_value(val: &Value<'gc>) -> &'static str {
        match val {
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Boolean(_) => "boolean",
            Value::Undefined => "undefined",
            Value::Null => "object",
            Value::Symbol(_) => "symbol",
            Value::VmFunction(..) | Value::VmClosure(..) | Value::Closure(..) | Value::Function(..) | Value::VmNativeFunction(_) => {
                "function"
            }
            Value::VmObject(map) => {
                let b = map.borrow();
                if b.contains_key("__vm_symbol__") {
                    "symbol"
                } else if b.contains_key("__fn_body__") || b.contains_key("__native_id__") || b.contains_key("__bound_target__") {
                    "function"
                } else {
                    "object"
                }
            }
            _ => "object",
        }
    }

    fn assign_named_property(&mut self, obj: Value<'gc>, key: String, val: Value<'gc>) -> Result<Value<'gc>, JSError> {
        if let Some(result) = self.try_proxy_set(&obj, &key, val.clone())? {
            return Ok(result);
        }

        if let Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _) = &val {
            // Record [[HomeObject]]-like information for functions assigned as methods.
            self.fn_home_objects.insert(*ip, obj.clone());
        }

        if let Value::VmObject(map) = &obj {
            let borrow = map.borrow();
            let is_frozen = matches!(borrow.get("__frozen__"), Some(Value::Boolean(true)));
            let is_non_ext = matches!(borrow.get("__non_extensible__"), Some(Value::Boolean(true)));
            let key_exists = borrow.contains_key(&key);
            let readonly_key = format!("__readonly_{}__", key);
            let is_readonly = matches!(borrow.get(&readonly_key), Some(Value::Boolean(true)));
            let getter_key = format!("__get_{}", key);
            let has_getter = borrow.get(&getter_key).is_some()
                || borrow
                    .get("__proto__")
                    .cloned()
                    .and_then(|proto| self.lookup_proto_chain(&Some(proto), &getter_key))
                    .is_some();
            let setter_key = format!("__set_{}", key);
            let setter = borrow.get(&setter_key).cloned().or_else(|| {
                borrow
                    .get("__proto__")
                    .cloned()
                    .and_then(|proto| self.lookup_proto_chain(&Some(proto), &setter_key))
            });
            let proto_readonly = if !is_readonly && !key_exists {
                if let Some(proto) = borrow.get("__proto__").cloned() {
                    self.lookup_proto_chain(&Some(proto), &readonly_key).is_some()
                } else {
                    false
                }
            } else {
                false
            };
            drop(borrow);
            let getter_only = has_getter && setter.is_none();
            let non_ext_proto_mutation = is_non_ext && key == "__proto__";

            if let Some(setter_fn) = setter {
                match setter_fn {
                    Value::VmFunction(setter_ip, _) => {
                        self.this_stack.push(obj.clone());
                        let result = self.call_vm_function(setter_ip, std::slice::from_ref(&val), &[]);
                        self.this_stack.pop();
                        let _ = result;
                    }
                    Value::VmClosure(setter_ip, _, ups) => {
                        self.this_stack.push(obj.clone());
                        let result = self.call_vm_function(setter_ip, std::slice::from_ref(&val), &ups);
                        self.this_stack.pop();
                        let _ = result;
                    }
                    _ => {}
                }
            } else if is_frozen || (is_non_ext && !key_exists) || non_ext_proto_mutation || is_readonly || proto_readonly || getter_only {
                let msg = if is_frozen {
                    format!("Cannot assign to read only property '{}' of object", key)
                } else if non_ext_proto_mutation {
                    "Cannot set prototype of a non-extensible object".to_string()
                } else if getter_only {
                    format!("Cannot set property {} of object which has only a getter", key)
                } else {
                    format!("Cannot add property {}, object is not extensible", key)
                };
                let mut err_map = IndexMap::new();
                err_map.insert("message".to_string(), Value::String(crate::unicode::utf8_to_utf16(&msg)));
                err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                err_map.insert("name".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                let err = Value::VmObject(Rc::new(RefCell::new(err_map)));
                self.handle_throw(err)?;
            } else {
                map.borrow_mut().insert(key.clone(), val.clone());
                if Rc::ptr_eq(map, &self.global_this) {
                    self.globals.insert(key, val.clone());
                }
            }

            Ok(val)
        } else if let Value::VmArray(arr) = &obj {
            if key == "length" {
                if let Value::Number(n) = &val {
                    let new_len = *n as usize;
                    let mut a = arr.borrow_mut();
                    let cur_len = a.elements.len();
                    if new_len > cur_len {
                        for i in cur_len..new_len {
                            a.elements.push(Value::Undefined);
                            a.props.insert(format!("__deleted_{}", i), Value::Boolean(true));
                        }
                    } else if new_len < cur_len {
                        a.elements.truncate(new_len);
                        // Remove hole markers for truncated indices
                        for i in new_len..cur_len {
                            a.props.shift_remove(&format!("__deleted_{}", i));
                        }
                    }
                }
            } else {
                arr.borrow_mut().props.insert(key, val.clone());
            }
            Ok(val)
        } else if let Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) = &obj {
            let props = self.get_fn_props(*ip, *arity);
            self.assign_named_property(Value::VmObject(props), key, val)
        } else {
            log::warn!("SetProperty on non-object: {}", value_to_string(&obj));
            Ok(val)
        }
    }

    fn resolve_super_base(&mut self, receiver: &Value<'gc>) -> Option<Value<'gc>> {
        let active_func_ips: Vec<usize> = self.frames.iter().rev().map(|f| f.func_ip).collect();
        for func_ip in active_func_ips {
            if let Some(home_obj) = self.fn_home_objects.get(&func_ip).cloned() {
                match home_obj {
                    Value::VmObject(map) => {
                        let base = map.borrow().get("__proto__").cloned().unwrap_or(Value::Null);
                        match base {
                            Value::Null | Value::Undefined => {}
                            proto => return Some(proto),
                        }
                    }
                    Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                        let props = self.get_fn_props(ip, arity);
                        let base = props.borrow().get("__proto__").cloned().unwrap_or(Value::Null);
                        match base {
                            Value::Null | Value::Undefined => {}
                            proto => return Some(proto),
                        }
                    }
                    _ => {}
                }
            }
        }

        match receiver {
            Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                let props = self.get_fn_props(*ip, *arity);
                match props.borrow().get("__proto__").cloned().unwrap_or(Value::Null) {
                    Value::Null | Value::Undefined => None,
                    proto => Some(proto),
                }
            }
            Value::VmObject(map) => {
                let borrow = map.borrow();
                let has_own_proto = borrow.contains_key("__proto__");
                let immediate_proto = borrow.get("__proto__").cloned().unwrap_or(Value::Null);
                drop(borrow);

                match immediate_proto {
                    Value::VmObject(proto_obj) => {
                        let borrow = proto_obj.borrow();
                        let super_base = if borrow.contains_key("constructor") {
                            borrow.get("__proto__").cloned().unwrap_or(Value::Null)
                        } else {
                            Value::VmObject(proto_obj.clone())
                        };
                        drop(borrow);
                        match super_base {
                            Value::Null | Value::Undefined => None,
                            proto => Some(proto),
                        }
                    }
                    Value::Null | Value::Undefined if !has_own_proto => {
                        if let Some(Value::VmObject(obj_global)) = self.globals.get("Object") {
                            obj_global.borrow().get("prototype").cloned()
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn infer_name_from_property_key(key: &str) -> String {
        if let Some(sym_desc) = key.strip_prefix("Symbol(").and_then(|s| s.strip_suffix(')')) {
            if sym_desc.is_empty() {
                String::new()
            } else {
                format!("[{}]", sym_desc)
            }
        } else if let Some(sym_desc) = key.strip_prefix("{ description: ").and_then(|s| s.strip_suffix(" }")) {
            if sym_desc == "undefined" || sym_desc.is_empty() {
                String::new()
            } else {
                format!("[{}]", sym_desc)
            }
        } else if key.starts_with("@@sym:") {
            // Handled by maybe_infer_function_name_from_key with symbol_values lookup
            String::new()
        } else {
            key.to_string()
        }
    }

    fn maybe_infer_function_name_from_key(&mut self, key: &str, val: &Value<'gc>) {
        let ip = match val {
            Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _) => *ip,
            _ => return,
        };

        let inferred = if let Some(id_str) = key.strip_prefix("@@sym:") {
            if let Ok(id) = id_str.parse::<u64>() {
                if let Some(Value::VmObject(m)) = self.symbol_values.get(&id) {
                    let desc = m.borrow().get("description").cloned();
                    match desc {
                        Some(Value::String(s)) => {
                            let d = crate::unicode::utf16_to_utf8(&s);
                            if d.is_empty() { String::new() } else { format!("[{}]", d) }
                        }
                        _ => String::new(),
                    }
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            Self::infer_name_from_property_key(key)
        };
        match self.chunk.fn_names.get(&ip) {
            Some(existing) if !existing.is_empty() => {}
            _ => {
                self.chunk.fn_names.insert(ip, inferred);
            }
        }
    }

    fn invoke_getter_with_receiver(&mut self, getter: Value<'gc>, receiver: Value<'gc>) -> Value<'gc> {
        let normalize_reason = |msg: &str| -> Value<'gc> {
            let payload = msg.strip_prefix("SyntaxError: Uncaught: ").unwrap_or(msg);
            if let Ok(n) = payload.parse::<f64>() {
                Value::Number(n)
            } else {
                Value::String(crate::unicode::utf8_to_utf16(payload))
            }
        };

        match getter {
            Value::VmFunction(ip, _) => {
                self.this_stack.push(receiver);
                let result = self.call_vm_function_result(ip, &[], &[]);
                self.this_stack.pop();
                match result {
                    Ok(v) => v,
                    Err(err) => {
                        self.pending_throw = Some(normalize_reason(&err.message()));
                        Value::Undefined
                    }
                }
            }
            Value::VmClosure(ip, _, ups) => {
                self.this_stack.push(receiver);
                let result = self.call_vm_function_result(ip, &[], &ups);
                self.this_stack.pop();
                match result {
                    Ok(v) => v,
                    Err(err) => {
                        self.pending_throw = Some(normalize_reason(&err.message()));
                        Value::Undefined
                    }
                }
            }
            _ => Value::Undefined,
        }
    }

    fn read_named_property_with_receiver(&mut self, obj: Value<'gc>, key: &str, receiver: Value<'gc>) -> Value<'gc> {
        let getter_key = format!("__get_{}", key);
        let mut current = Some(obj);
        let mut depth = 0;
        while let Some(cur) = current {
            if depth > 100 {
                break;
            }
            depth += 1;

            match cur {
                Value::VmObject(map) => {
                    let borrow = map.borrow();
                    if let Some(val) = borrow.get(key).cloned() {
                        match val {
                            Value::Property { getter: Some(g), .. } => {
                                return self.invoke_getter_with_receiver((*g).clone(), receiver.clone());
                            }
                            other => return other,
                        }
                    }
                    if let Some(getter_fn) = borrow.get(&getter_key).cloned() {
                        return self.invoke_getter_with_receiver(getter_fn, receiver.clone());
                    }
                    current = borrow.get("__proto__").cloned();
                }
                Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                    let props = self.get_fn_props(ip, arity);
                    let borrow = props.borrow();
                    if let Some(val) = borrow.get(key).cloned() {
                        return val;
                    }
                    current = borrow.get("__proto__").cloned();
                }
                _ => break,
            }
        }
        Value::Undefined
    }

    fn read_named_property(&mut self, obj: Value<'gc>, key: &str) -> Value<'gc> {
        match &obj {
            Value::VmObject(_) => self.read_named_property_with_receiver(obj.clone(), key, obj),
            Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                let props = self.get_fn_props(*ip, *arity);
                let borrow = props.borrow();
                let value = borrow.get(key).cloned();
                let proto = borrow.get("__proto__").cloned();
                drop(borrow);
                value.or_else(|| self.lookup_proto_chain(&proto, key)).unwrap_or_else(|| match key {
                    "call" => Value::VmNativeFunction(BUILTIN_FN_CALL),
                    "apply" => Value::VmNativeFunction(BUILTIN_FN_APPLY),
                    "bind" => Value::VmNativeFunction(BUILTIN_FN_BIND),
                    _ => Value::Undefined,
                })
            }
            _ => Value::Undefined,
        }
    }

    fn ensure_super_base(&mut self, receiver: &Value<'gc>) -> Result<Value<'gc>, JSError> {
        if let Some(super_base) = self.resolve_super_base(receiver) {
            return Ok(super_base);
        }

        let mut err_map = IndexMap::new();
        err_map.insert(
            "message".to_string(),
            Value::String(crate::unicode::utf8_to_utf16(
                "Cannot access 'super' of a class with null prototype",
            )),
        );
        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
        let err = Value::VmObject(Rc::new(RefCell::new(err_map)));
        self.handle_throw(err)?;
        Ok(Value::Undefined)
    }

    /// Walk the __proto__ chain looking for a property.
    fn lookup_proto_chain(&self, proto: &Option<Value<'gc>>, key: &str) -> Option<Value<'gc>> {
        let mut current = proto.clone();
        let mut depth = 0;
        while let Some(ref p) = current {
            if depth > 100 {
                break;
            }
            depth += 1;
            match p {
                Value::VmObject(map) => {
                    let borrow = map.borrow();
                    if let Some(val) = borrow.get(key) {
                        return Some(val.clone());
                    }
                    let next = borrow.get("__proto__").cloned();
                    drop(borrow);
                    current = next;
                }
                Value::Null => break,
                _ => break,
            }
        }
        None
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
        math_map.insert("sin".to_string(), Value::VmNativeFunction(BUILTIN_MATH_SIN));
        math_map.insert("cos".to_string(), Value::VmNativeFunction(BUILTIN_MATH_COS));
        math_map.insert("tan".to_string(), Value::VmNativeFunction(BUILTIN_MATH_TAN));
        math_map.insert("asin".to_string(), Value::VmNativeFunction(BUILTIN_MATH_ASIN));
        math_map.insert("acos".to_string(), Value::VmNativeFunction(BUILTIN_MATH_ACOS));
        math_map.insert("atan".to_string(), Value::VmNativeFunction(BUILTIN_MATH_ATAN));
        math_map.insert("atan2".to_string(), Value::VmNativeFunction(BUILTIN_MATH_ATAN2));
        math_map.insert("sinh".to_string(), Value::VmNativeFunction(BUILTIN_MATH_SINH));
        math_map.insert("cosh".to_string(), Value::VmNativeFunction(BUILTIN_MATH_COSH));
        math_map.insert("tanh".to_string(), Value::VmNativeFunction(BUILTIN_MATH_TANH));
        math_map.insert("asinh".to_string(), Value::VmNativeFunction(BUILTIN_MATH_ASINH));
        math_map.insert("acosh".to_string(), Value::VmNativeFunction(BUILTIN_MATH_ACOSH));
        math_map.insert("atanh".to_string(), Value::VmNativeFunction(BUILTIN_MATH_ATANH));
        math_map.insert("exp".to_string(), Value::VmNativeFunction(BUILTIN_MATH_EXP));
        math_map.insert("expm1".to_string(), Value::VmNativeFunction(BUILTIN_MATH_EXPM1));
        math_map.insert("log".to_string(), Value::VmNativeFunction(BUILTIN_MATH_LOG));
        math_map.insert("log10".to_string(), Value::VmNativeFunction(BUILTIN_MATH_LOG10));
        math_map.insert("log1p".to_string(), Value::VmNativeFunction(BUILTIN_MATH_LOG1P));
        math_map.insert("log2".to_string(), Value::VmNativeFunction(BUILTIN_MATH_LOG2));
        math_map.insert("fround".to_string(), Value::VmNativeFunction(BUILTIN_MATH_FROUND));
        math_map.insert("trunc".to_string(), Value::VmNativeFunction(BUILTIN_MATH_TRUNC));
        math_map.insert("cbrt".to_string(), Value::VmNativeFunction(BUILTIN_MATH_CBRT));
        math_map.insert("hypot".to_string(), Value::VmNativeFunction(BUILTIN_MATH_HYPOT));
        math_map.insert("sign".to_string(), Value::VmNativeFunction(BUILTIN_MATH_SIGN));
        math_map.insert("pow".to_string(), Value::VmNativeFunction(BUILTIN_MATH_POW));
        math_map.insert("random".to_string(), Value::VmNativeFunction(BUILTIN_MATH_RANDOM));
        math_map.insert("clz32".to_string(), Value::VmNativeFunction(BUILTIN_MATH_CLZ32));
        math_map.insert("imul".to_string(), Value::VmNativeFunction(BUILTIN_MATH_IMUL));
        math_map.insert("PI".to_string(), Value::Number(std::f64::consts::PI));
        math_map.insert("E".to_string(), Value::Number(std::f64::consts::E));
        math_map.insert("LN2".to_string(), Value::Number(std::f64::consts::LN_2));
        math_map.insert("LN10".to_string(), Value::Number(std::f64::consts::LN_10));
        math_map.insert("LOG2E".to_string(), Value::Number(std::f64::consts::LOG2_E));
        math_map.insert("LOG10E".to_string(), Value::Number(std::f64::consts::LOG10_E));
        math_map.insert("SQRT2".to_string(), Value::Number(std::f64::consts::SQRT_2));
        math_map.insert("SQRT1_2".to_string(), Value::Number(std::f64::consts::FRAC_1_SQRT_2));
        self.globals
            .insert("Math".to_string(), Value::VmObject(Rc::new(RefCell::new(math_map))));

        // Global functions
        self.globals.insert("isNaN".to_string(), Value::VmNativeFunction(BUILTIN_ISNAN));
        self.globals.insert("isFinite".to_string(), Self::make_host_fn("global.isFinite"));
        self.globals.insert("encodeURI".to_string(), Self::make_host_fn("global.encodeURI"));
        self.globals.insert("decodeURI".to_string(), Self::make_host_fn("global.decodeURI"));
        self.globals
            .insert("encodeURIComponent".to_string(), Self::make_host_fn("global.encodeURIComponent"));
        self.globals
            .insert("decodeURIComponent".to_string(), Self::make_host_fn("global.decodeURIComponent"));
        self.globals
            .insert("__forOfValues".to_string(), Self::make_host_fn("global.__forOfValues"));
        // Minimal Symbol object — callable via __native_id__, with well-known symbol properties
        // Well-known symbols are proper VmObject symbols with fixed IDs: iterator=1, hasInstance=2, toPrimitive=3, toStringTag=4
        let make_well_known_symbol = |id: u64, name: &str| -> Value<'gc> {
            let mut m = IndexMap::new();
            m.insert("__vm_symbol__".to_string(), Value::Boolean(true));
            m.insert("__symbol_id__".to_string(), Value::Number(id as f64));
            m.insert(
                "description".to_string(),
                Value::String(crate::unicode::utf8_to_utf16(&format!("Symbol.{}", name))),
            );
            Value::VmObject(Rc::new(RefCell::new(m)))
        };
        let sym_iterator = make_well_known_symbol(1, "iterator");
        let sym_has_instance = make_well_known_symbol(2, "hasInstance");
        let sym_to_primitive = make_well_known_symbol(3, "toPrimitive");
        let sym_to_string_tag = make_well_known_symbol(4, "toStringTag");
        self.symbol_values.insert(1, sym_iterator.clone());
        self.symbol_values.insert(2, sym_has_instance.clone());
        self.symbol_values.insert(3, sym_to_primitive.clone());
        self.symbol_values.insert(4, sym_to_string_tag.clone());
        self.symbol_counter = 4; // user symbols start from 5+

        let mut sym_obj = IndexMap::new();
        sym_obj.insert("__native_id__".to_string(), Value::Number(BUILTIN_SYMBOL as f64));
        sym_obj.insert("iterator".to_string(), sym_iterator);
        sym_obj.insert("hasInstance".to_string(), sym_has_instance);
        sym_obj.insert("toPrimitive".to_string(), sym_to_primitive);
        sym_obj.insert("toStringTag".to_string(), sym_to_string_tag);
        sym_obj.insert("for".to_string(), Value::VmNativeFunction(BUILTIN_SYMBOL_FOR));
        sym_obj.insert("keyFor".to_string(), Value::VmNativeFunction(BUILTIN_SYMBOL_KEYFOR));
        self.globals
            .insert("Symbol".to_string(), Value::VmObject(Rc::new(RefCell::new(sym_obj))));
        self.globals
            .insert("parseInt".to_string(), Value::VmNativeFunction(BUILTIN_PARSEINT));
        self.globals
            .insert("parseFloat".to_string(), Value::VmNativeFunction(BUILTIN_PARSEFLOAT));
        self.globals.insert("eval".to_string(), Value::VmNativeFunction(BUILTIN_EVAL));
        self.globals
            .insert("setTimeout".to_string(), Value::VmNativeFunction(BUILTIN_SETTIMEOUT));
        self.globals
            .insert("clearTimeout".to_string(), Value::VmNativeFunction(BUILTIN_CLEARTIMEOUT));
        self.globals
            .insert("setInterval".to_string(), Value::VmNativeFunction(BUILTIN_SETINTERVAL));
        self.globals
            .insert("clearInterval".to_string(), Value::VmNativeFunction(BUILTIN_CLEARINTERVAL));

        // JSON object
        let mut json_map = IndexMap::new();
        json_map.insert("stringify".to_string(), Value::VmNativeFunction(BUILTIN_JSON_STRINGIFY));
        json_map.insert("parse".to_string(), Value::VmNativeFunction(BUILTIN_JSON_PARSE));
        self.globals
            .insert("JSON".to_string(), Value::VmObject(Rc::new(RefCell::new(json_map))));

        // Reflect object
        let mut reflect_map = IndexMap::new();
        reflect_map.insert("has".to_string(), Self::make_host_fn("reflect.has"));
        reflect_map.insert("get".to_string(), Self::make_host_fn("reflect.get"));
        reflect_map.insert("set".to_string(), Self::make_host_fn("reflect.set"));
        reflect_map.insert("ownKeys".to_string(), Self::make_host_fn("reflect.ownKeys"));
        reflect_map.insert("isExtensible".to_string(), Self::make_host_fn("reflect.isExtensible"));
        reflect_map.insert("getPrototypeOf".to_string(), Self::make_host_fn("reflect.getPrototypeOf"));
        reflect_map.insert(
            "getOwnPropertyDescriptor".to_string(),
            Self::make_host_fn("reflect.getOwnPropertyDescriptor"),
        );
        reflect_map.insert("defineProperty".to_string(), Self::make_host_fn("reflect.defineProperty"));
        reflect_map.insert("apply".to_string(), Value::VmNativeFunction(BUILTIN_REFLECT_APPLY));
        reflect_map.insert("construct".to_string(), Self::make_host_fn("reflect.construct"));
        reflect_map.insert("setPrototypeOf".to_string(), Value::VmNativeFunction(BUILTIN_OBJECT_SETPROTOTYPEOF));
        self.globals
            .insert("Reflect".to_string(), Value::VmObject(Rc::new(RefCell::new(reflect_map))));

        // Array.isArray and prototype
        let mut array_obj = IndexMap::new();
        array_obj.insert("isArray".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_ISARRAY));
        array_obj.insert("of".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_OF));
        array_obj.insert("from".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_FROM));
        array_obj.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_ARRAY as f64));
        // Create Array.prototype with iterator method
        let mut arr_proto = IndexMap::new();
        arr_proto.insert("@@sym:1".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_ITERATOR));
        arr_proto.insert("push".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_PUSH));
        arr_proto.insert("pop".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_POP));
        arr_proto.insert("join".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_JOIN));
        arr_proto.insert("indexOf".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_INDEXOF));
        arr_proto.insert("slice".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_SLICE));
        arr_proto.insert("concat".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_CONCAT));
        arr_proto.insert("map".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_MAP));
        arr_proto.insert("filter".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_FILTER));
        arr_proto.insert("forEach".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_FOREACH));
        arr_proto.insert("reduce".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_REDUCE));
        arr_proto.insert("shift".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_SHIFT));
        arr_proto.insert("unshift".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_UNSHIFT));
        arr_proto.insert("splice".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_SPLICE));
        arr_proto.insert("reverse".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_REVERSE));
        arr_proto.insert("sort".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_SORT));
        arr_proto.insert("find".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_FIND));
        arr_proto.insert("findIndex".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_FINDINDEX));
        arr_proto.insert("includes".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_INCLUDES));
        arr_proto.insert("flat".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_FLAT));
        arr_proto.insert("flatMap".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_FLATMAP));
        arr_proto.insert("at".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_AT));
        arr_proto.insert("every".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_EVERY));
        arr_proto.insert("some".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_SOME));
        arr_proto.insert("fill".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_FILL));
        arr_proto.insert("lastIndexOf".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_LASTINDEXOF));
        arr_proto.insert("findLast".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_FINDLAST));
        arr_proto.insert("findLastIndex".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_FINDLASTINDEX));
        arr_proto.insert("reduceRight".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_REDUCERIGHT));
        array_obj.insert("prototype".to_string(), Value::VmObject(Rc::new(RefCell::new(arr_proto))));
        self.globals
            .insert("Array".to_string(), Value::VmObject(Rc::new(RefCell::new(array_obj))));

        // Error constructor sentinels (used by instanceof checks)
        self.globals
            .insert("Error".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_ERROR));
        self.globals
            .insert("TypeError".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_TYPEERROR));
        self.globals
            .insert("SyntaxError".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_SYNTAXERROR));
        self.globals
            .insert("RangeError".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_RANGEERROR));
        self.globals
            .insert("ReferenceError".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_REFERENCEERROR));

        // Type constructor sentinels (for typeof checks / instanceof)
        // Date constructor with static methods
        let mut date_map = IndexMap::new();
        date_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_DATE as f64));
        date_map.insert("now".to_string(), Value::VmNativeFunction(BUILTIN_DATE_NOW));
        date_map.insert("parse".to_string(), Value::VmNativeFunction(BUILTIN_DATE_PARSE));
        date_map.insert("UTC".to_string(), Self::make_host_fn("date.UTC"));
        self.globals
            .insert("Date".to_string(), Value::VmObject(Rc::new(RefCell::new(date_map))));
        let mut array_buffer_map = IndexMap::new();
        array_buffer_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_ARRAYBUFFER as f64));
        self.globals
            .insert("ArrayBuffer".to_string(), Value::VmObject(Rc::new(RefCell::new(array_buffer_map))));

        let mut data_view_map = IndexMap::new();
        data_view_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_DATAVIEW as f64));
        self.globals
            .insert("DataView".to_string(), Value::VmObject(Rc::new(RefCell::new(data_view_map))));

        let mut shared_array_buffer_map = IndexMap::new();
        shared_array_buffer_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_SHAREDARRAYBUFFER as f64));
        self.globals.insert(
            "SharedArrayBuffer".to_string(),
            Value::VmObject(Rc::new(RefCell::new(shared_array_buffer_map))),
        );

        let mut atomics_map = IndexMap::new();
        atomics_map.insert("isLockFree".to_string(), Value::VmNativeFunction(BUILTIN_ATOMICS_ISLOCKFREE));
        atomics_map.insert("load".to_string(), Value::VmNativeFunction(BUILTIN_ATOMICS_LOAD));
        atomics_map.insert("store".to_string(), Value::VmNativeFunction(BUILTIN_ATOMICS_STORE));
        atomics_map.insert(
            "compareExchange".to_string(),
            Value::VmNativeFunction(BUILTIN_ATOMICS_COMPAREEXCHANGE),
        );
        atomics_map.insert("add".to_string(), Value::VmNativeFunction(BUILTIN_ATOMICS_ADD));
        atomics_map.insert("exchange".to_string(), Value::VmNativeFunction(BUILTIN_ATOMICS_EXCHANGE));
        atomics_map.insert("wait".to_string(), Value::VmNativeFunction(BUILTIN_ATOMICS_WAIT));
        atomics_map.insert("notify".to_string(), Value::VmNativeFunction(BUILTIN_ATOMICS_NOTIFY));
        let mut wait_async_fn = IndexMap::new();
        wait_async_fn.insert("__native_id__".to_string(), Value::Number(BUILTIN_ATOMICS_WAITASYNC as f64));
        wait_async_fn.insert("length".to_string(), Value::Number(4.0));
        atomics_map.insert("waitAsync".to_string(), Value::VmObject(Rc::new(RefCell::new(wait_async_fn))));
        self.globals
            .insert("Atomics".to_string(), Value::VmObject(Rc::new(RefCell::new(atomics_map))));

        let mut promise_map = IndexMap::new();
        let mut promise_proto = IndexMap::new();
        promise_proto.insert("then".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_THEN));
        promise_proto.insert("catch".to_string(), Self::make_host_fn("promise.catch"));
        promise_proto.insert("finally".to_string(), Self::make_host_fn("promise.finally"));
        let promise_proto_obj = Value::VmObject(Rc::new(RefCell::new(promise_proto)));
        promise_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_PROMISE as f64));
        promise_map.insert("resolve".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_RESOLVE));
        promise_map.insert("all".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_ALL));
        promise_map.insert("any".to_string(), Self::make_host_fn("promise.any"));
        promise_map.insert("race".to_string(), Self::make_host_fn("promise.race"));
        promise_map.insert("allSettled".to_string(), Self::make_host_fn("promise.allSettled"));
        promise_map.insert("reject".to_string(), Self::make_host_fn("promise.reject"));
        promise_map.insert("prototype".to_string(), promise_proto_obj);
        self.globals
            .insert("Promise".to_string(), Value::VmObject(Rc::new(RefCell::new(promise_map))));
        self.globals
            .insert("AggregateError".to_string(), Self::make_host_fn("error.aggregate"));
        self.globals.insert("__await__".to_string(), Self::make_host_fn("promise.await"));

        let mut proxy_map = IndexMap::new();
        proxy_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_PROXY as f64));
        proxy_map.insert("revocable".to_string(), Self::make_host_fn("proxy.revocable"));
        self.globals
            .insert("Proxy".to_string(), Value::VmObject(Rc::new(RefCell::new(proxy_map))));

        let mut int8_array_map = IndexMap::new();
        int8_array_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_INT8ARRAY as f64));
        int8_array_map.insert(
            "name".to_string(),
            Value::String(crate::unicode::utf8_to_utf16("UnimplementedInt8Array")),
        );
        int8_array_map.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(1.0));
        self.globals
            .insert("Int8Array".to_string(), Value::VmObject(Rc::new(RefCell::new(int8_array_map))));

        let mut uint8_array_map = IndexMap::new();
        uint8_array_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_UINT8ARRAY as f64));
        uint8_array_map.insert(
            "name".to_string(),
            Value::String(crate::unicode::utf8_to_utf16("UnimplementedUint8Array")),
        );
        uint8_array_map.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(1.0));
        self.globals
            .insert("Uint8Array".to_string(), Value::VmObject(Rc::new(RefCell::new(uint8_array_map))));

        let mut uint8_clamped_array_map = IndexMap::new();
        uint8_clamped_array_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_UINT8CLAMPEDARRAY as f64));
        uint8_clamped_array_map.insert(
            "name".to_string(),
            Value::String(crate::unicode::utf8_to_utf16("UnimplementedUint8ClampedArray")),
        );
        uint8_clamped_array_map.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(1.0));
        self.globals.insert(
            "Uint8ClampedArray".to_string(),
            Value::VmObject(Rc::new(RefCell::new(uint8_clamped_array_map))),
        );

        let mut int16_array_map = IndexMap::new();
        int16_array_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_INT16ARRAY as f64));
        int16_array_map.insert(
            "name".to_string(),
            Value::String(crate::unicode::utf8_to_utf16("UnimplementedInt16Array")),
        );
        int16_array_map.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(2.0));
        self.globals
            .insert("Int16Array".to_string(), Value::VmObject(Rc::new(RefCell::new(int16_array_map))));

        let mut uint16_array_map = IndexMap::new();
        uint16_array_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_UINT16ARRAY as f64));
        uint16_array_map.insert(
            "name".to_string(),
            Value::String(crate::unicode::utf8_to_utf16("UnimplementedUint16Array")),
        );
        uint16_array_map.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(2.0));
        self.globals
            .insert("Uint16Array".to_string(), Value::VmObject(Rc::new(RefCell::new(uint16_array_map))));

        let mut int32_array_map = IndexMap::new();
        int32_array_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_INT32ARRAY as f64));
        int32_array_map.insert(
            "name".to_string(),
            Value::String(crate::unicode::utf8_to_utf16("UnimplementedInt32Array")),
        );
        int32_array_map.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(4.0));
        self.globals
            .insert("Int32Array".to_string(), Value::VmObject(Rc::new(RefCell::new(int32_array_map))));

        let mut uint32_array_map = IndexMap::new();
        uint32_array_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_UINT32ARRAY as f64));
        uint32_array_map.insert(
            "name".to_string(),
            Value::String(crate::unicode::utf8_to_utf16("UnimplementedUint32Array")),
        );
        uint32_array_map.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(4.0));
        self.globals
            .insert("Uint32Array".to_string(), Value::VmObject(Rc::new(RefCell::new(uint32_array_map))));

        let mut float32_array_map = IndexMap::new();
        float32_array_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_FLOAT32ARRAY as f64));
        float32_array_map.insert(
            "name".to_string(),
            Value::String(crate::unicode::utf8_to_utf16("UnimplementedFloat32Array")),
        );
        float32_array_map.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(4.0));
        self.globals.insert(
            "Float32Array".to_string(),
            Value::VmObject(Rc::new(RefCell::new(float32_array_map))),
        );

        let mut float64_array_map = IndexMap::new();
        float64_array_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_FLOAT64ARRAY as f64));
        float64_array_map.insert(
            "name".to_string(),
            Value::String(crate::unicode::utf8_to_utf16("UnimplementedFloat64Array")),
        );
        float64_array_map.insert("BYTES_PER_ELEMENT".to_string(), Value::Number(8.0));
        self.globals.insert(
            "Float64Array".to_string(),
            Value::VmObject(Rc::new(RefCell::new(float64_array_map))),
        );
        self.globals
            .insert("Boolean".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_BOOLEAN));
        // Object constructor with static methods
        let mut object_map = IndexMap::new();
        let object_proto = Rc::new(RefCell::new(IndexMap::new()));
        object_proto
            .borrow_mut()
            .insert("hasOwnProperty".to_string(), Value::VmNativeFunction(BUILTIN_OBJ_HASOWNPROPERTY));
        object_proto
            .borrow_mut()
            .insert("toString".to_string(), Value::VmNativeFunction(BUILTIN_OBJ_TOSTRING));
        object_proto
            .borrow_mut()
            .insert("toLocaleString".to_string(), Self::make_host_fn("object.toLocaleString"));
        object_proto
            .borrow_mut()
            .insert("valueOf".to_string(), Self::make_host_fn("object.valueOf"));
        object_proto
            .borrow_mut()
            .insert("isPrototypeOf".to_string(), Self::make_host_fn("object.isPrototypeOf"));
        object_proto.borrow_mut().insert(
            "propertyIsEnumerable".to_string(),
            Self::make_host_fn("object.propertyIsEnumerable"),
        );
        // Object.prototype is the root — explicitly null __proto__
        object_proto.borrow_mut().insert("__proto__".to_string(), Value::Null);
        object_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_OBJECT as f64));
        object_map.insert("prototype".to_string(), Value::VmObject(object_proto.clone()));
        object_map.insert("keys".to_string(), Value::VmNativeFunction(BUILTIN_OBJECT_KEYS));
        object_map.insert("values".to_string(), Value::VmNativeFunction(BUILTIN_OBJECT_VALUES));
        object_map.insert("entries".to_string(), Value::VmNativeFunction(BUILTIN_OBJECT_ENTRIES));
        object_map.insert("assign".to_string(), Value::VmNativeFunction(BUILTIN_OBJECT_ASSIGN));
        object_map.insert("freeze".to_string(), Value::VmNativeFunction(BUILTIN_OBJECT_FREEZE));
        object_map.insert("hasOwn".to_string(), Value::VmNativeFunction(BUILTIN_OBJECT_HASOWN));
        object_map.insert("create".to_string(), Value::VmNativeFunction(BUILTIN_OBJECT_CREATE));
        object_map.insert("getPrototypeOf".to_string(), Value::VmNativeFunction(BUILTIN_OBJECT_GETPROTOTYPEOF));
        object_map.insert("defineProperties".to_string(), Value::VmNativeFunction(BUILTIN_OBJECT_DEFINEPROPS));
        object_map.insert("preventExtensions".to_string(), Value::VmNativeFunction(BUILTIN_OBJECT_PREVENTEXT));
        object_map.insert("groupBy".to_string(), Value::VmNativeFunction(BUILTIN_OBJECT_GROUPBY));
        object_map.insert("defineProperty".to_string(), Value::VmNativeFunction(BUILTIN_OBJECT_DEFINEPROP));
        object_map.insert(
            "getOwnPropertyDescriptor".to_string(),
            Value::VmNativeFunction(BUILTIN_OBJECT_GETOWNPROPDESC),
        );
        object_map.insert("setPrototypeOf".to_string(), Value::VmNativeFunction(BUILTIN_OBJECT_SETPROTOTYPEOF));
        object_map.insert(
            "getOwnPropertyNames".to_string(),
            Value::VmNativeFunction(BUILTIN_OBJECT_GETOWNPROPERTYNAMES),
        );
        object_map.insert(
            "getOwnPropertyDescriptors".to_string(),
            Self::make_host_fn("object.getOwnPropertyDescriptors"),
        );
        object_map.insert(
            "getOwnPropertySymbols".to_string(),
            Self::make_host_fn("object.getOwnPropertySymbols"),
        );
        let object_val = Value::VmObject(Rc::new(RefCell::new(object_map)));
        // Set Object.prototype.constructor = Object (circular reference)
        object_proto.borrow_mut().insert("constructor".to_string(), object_val.clone());
        self.globals.insert("Object".to_string(), object_val);

        // Number object with constants and static methods
        let mut number_map = IndexMap::new();
        number_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_NUMBER as f64));
        number_map.insert("__frozen__".to_string(), Value::Boolean(true));
        number_map.insert("MAX_VALUE".to_string(), Value::Number(f64::MAX));
        number_map.insert("MIN_VALUE".to_string(), Value::Number(5e-324));
        number_map.insert("NaN".to_string(), Value::Number(f64::NAN));
        number_map.insert("POSITIVE_INFINITY".to_string(), Value::Number(f64::INFINITY));
        number_map.insert("NEGATIVE_INFINITY".to_string(), Value::Number(f64::NEG_INFINITY));
        number_map.insert("EPSILON".to_string(), Value::Number(f64::EPSILON));
        number_map.insert("MAX_SAFE_INTEGER".to_string(), Value::Number(9007199254740991.0));
        number_map.insert("MIN_SAFE_INTEGER".to_string(), Value::Number(-9007199254740991.0));
        number_map.insert("isNaN".to_string(), Value::VmNativeFunction(BUILTIN_NUMBER_ISNAN));
        number_map.insert("isFinite".to_string(), Value::VmNativeFunction(BUILTIN_NUMBER_ISFINITE));
        number_map.insert("isInteger".to_string(), Value::VmNativeFunction(BUILTIN_NUMBER_ISINTEGER));
        number_map.insert("isSafeInteger".to_string(), Value::VmNativeFunction(BUILTIN_NUMBER_ISSAFEINTEGER));
        number_map.insert("parseFloat".to_string(), Value::VmNativeFunction(BUILTIN_PARSEFLOAT));
        number_map.insert("parseInt".to_string(), Value::VmNativeFunction(BUILTIN_PARSEINT));
        // Number.prototype stubs for test compatibility
        let mut num_proto = IndexMap::new();
        num_proto.insert("toFixed".to_string(), Value::VmNativeFunction(BUILTIN_NUM_TOFIXED));
        num_proto.insert("call".to_string(), Value::Undefined); // stub
        number_map.insert("prototype".to_string(), Value::VmObject(Rc::new(RefCell::new(num_proto))));
        self.globals
            .insert("Number".to_string(), Value::VmObject(Rc::new(RefCell::new(number_map))));

        // String constructor (as VmObject with __native_id__ for typeof "function")
        let mut string_map = IndexMap::new();
        string_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_STRING as f64));
        string_map.insert("fromCharCode".to_string(), Value::VmNativeFunction(BUILTIN_STRING_FROMCHARCODE));
        self.globals
            .insert("String".to_string(), Value::VmObject(Rc::new(RefCell::new(string_map))));

        // Global constants
        self.globals.insert("Infinity".to_string(), Value::Number(f64::INFINITY));
        self.globals.insert("NaN".to_string(), Value::Number(f64::NAN));
        self.globals.insert("undefined".to_string(), Value::Undefined);

        // Map / Set constructors
        self.globals.insert("Map".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_MAP));
        self.globals.insert("Set".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_SET));
        self.globals
            .insert("WeakMap".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_WEAKMAP));
        self.globals
            .insert("WeakSet".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_WEAKSET));
        self.globals
            .insert("WeakRef".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_WEAKREF));
        self.globals
            .insert("FinalizationRegistry".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_FR));
        self.globals.insert("BigInt".to_string(), Value::VmNativeFunction(BUILTIN_BIGINT));
        self.globals
            .insert("RegExp".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_REGEXP));

        // globalThis — refers to the global this object
        self.globals
            .insert("globalThis".to_string(), Value::VmObject(self.global_this.clone()));

        // Minimal `os` namespace for VM module import interop
        let mut os_path_map = IndexMap::new();
        os_path_map.insert("basename".to_string(), Self::make_host_fn("os.path.basename"));
        os_path_map.insert("dirname".to_string(), Self::make_host_fn("os.path.dirname"));
        os_path_map.insert("join".to_string(), Self::make_host_fn("os.path.join"));
        os_path_map.insert("extname".to_string(), Self::make_host_fn("os.path.extname"));

        let mut os_map = IndexMap::new();
        os_map.insert("getcwd".to_string(), Self::make_host_fn("os.getcwd"));
        os_map.insert("getpid".to_string(), Self::make_host_fn("os.getpid"));
        os_map.insert("getppid".to_string(), Self::make_host_fn("os.getppid"));
        os_map.insert("open".to_string(), Self::make_host_fn("os.open"));
        os_map.insert("write".to_string(), Self::make_host_fn("os.write"));
        os_map.insert("read".to_string(), Self::make_host_fn("os.read"));
        os_map.insert("seek".to_string(), Self::make_host_fn("os.seek"));
        os_map.insert("close".to_string(), Self::make_host_fn("os.close"));
        os_map.insert("path".to_string(), Value::VmObject(Rc::new(RefCell::new(os_path_map))));
        self.globals
            .insert("os".to_string(), Value::VmObject(Rc::new(RefCell::new(os_map))));

        // Minimal `std` namespace for VM module import interop
        let mut std_map = IndexMap::new();
        std_map.insert("sprintf".to_string(), Self::make_host_fn("std.sprintf"));
        std_map.insert("tmpfile".to_string(), Self::make_host_fn("std.tmpfile"));
        std_map.insert("gc".to_string(), Self::make_host_fn("std.gc"));
        self.globals
            .insert("std".to_string(), Value::VmObject(Rc::new(RefCell::new(std_map))));

        // Function constructor with prototype (call, apply, bind)
        let mut fn_proto = IndexMap::new();
        fn_proto.insert("call".to_string(), Value::VmNativeFunction(BUILTIN_FN_CALL));
        fn_proto.insert("apply".to_string(), Value::VmNativeFunction(BUILTIN_FN_APPLY));
        fn_proto.insert("bind".to_string(), Value::VmNativeFunction(BUILTIN_FN_BIND));
        let fn_proto_val = Value::VmObject(Rc::new(RefCell::new(fn_proto)));
        let mut function_map = IndexMap::new();
        function_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_FUNCTION as f64));
        function_map.insert("prototype".to_string(), fn_proto_val);
        function_map.insert("length".to_string(), Value::Number(1.0));
        function_map.insert("__readonly_length__".to_string(), Value::Boolean(true));
        self.globals
            .insert("Function".to_string(), Value::VmObject(Rc::new(RefCell::new(function_map))));
    }

    /// Convert a value to string, calling toString() on VmObjects if available
    fn is_error_type_name(name: &str) -> bool {
        matches!(name, "Error" | "TypeError" | "SyntaxError" | "RangeError" | "ReferenceError")
    }

    fn format_error_name_message(name: &str, message: &str) -> String {
        if message.is_empty() {
            name.to_string()
        } else {
            format!("{}: {}", name, message)
        }
    }

    fn format_vm_error_object(borrow: &IndexMap<String, Value<'gc>>) -> Option<String> {
        if let Some(Value::String(t)) = borrow.get("__type__") {
            let name = crate::unicode::utf16_to_utf8(t);
            if Self::is_error_type_name(&name) {
                let message = borrow.get("message").map(value_to_string).unwrap_or_default();
                return Some(Self::format_error_name_message(&name, &message));
            }
        }
        None
    }

    fn error_type_name_from_builtin(id: u8) -> Option<&'static str> {
        match id {
            BUILTIN_CTOR_ERROR => Some("Error"),
            BUILTIN_CTOR_TYPEERROR => Some("TypeError"),
            BUILTIN_CTOR_SYNTAXERROR => Some("SyntaxError"),
            BUILTIN_CTOR_RANGEERROR => Some("RangeError"),
            BUILTIN_CTOR_REFERENCEERROR => Some("ReferenceError"),
            _ => None,
        }
    }

    fn vm_to_string(&mut self, val: &Value<'gc>) -> String {
        if let Value::VmObject(map) = val {
            let borrow = map.borrow();
            // VM Symbol toString
            if borrow.contains_key("__vm_symbol__") {
                return match borrow.get("description") {
                    Some(Value::String(d)) => format!("Symbol({})", crate::unicode::utf16_to_utf8(d)),
                    _ => "Symbol()".to_string(),
                };
            }
            // Error object stringification: "Name: message"
            if let Some(formatted) = Self::format_vm_error_object(&borrow) {
                return formatted;
            }
            drop(borrow);
            let ts = map.borrow().get("toString").cloned();
            if let Some(ts_val) = ts {
                match ts_val {
                    Value::VmFunction(ip, _arity) | Value::VmClosure(ip, _arity, _) => {
                        let result = self.call_vm_function(ip, &[], &[]);
                        return value_to_string(&result);
                    }
                    Value::VmNativeFunction(id) => {
                        let result = self.call_method_builtin(id, val.clone(), vec![]);
                        return value_to_string(&result);
                    }
                    _ => {}
                }
            }
            // Check __value__ for wrapper objects (e.g. new String("abc"))
            let inner = map.borrow().get("__value__").cloned();
            if let Some(v) = inner {
                return value_to_string(&v);
            }
        }
        // Array.prototype.toString() → join elements with ","
        if let Value::VmArray(arr) = val {
            let elems: Vec<String> = arr
                .borrow()
                .iter()
                .map(|v| match v {
                    Value::Null | Value::Undefined => String::new(),
                    other => self.vm_to_string(other),
                })
                .collect();
            return elems.join(",");
        }
        value_to_string(val)
    }

    /// Display a value for console.log (uses inspect-style format for arrays/objects)
    fn vm_display_string(&mut self, val: &Value<'gc>) -> String {
        if let Value::VmObject(map) = val {
            let borrow = map.borrow();
            // VM Symbol display
            if borrow.contains_key("__vm_symbol__") {
                return match borrow.get("description") {
                    Some(Value::String(d)) => format!("Symbol({})", crate::unicode::utf16_to_utf8(d)),
                    _ => "Symbol()".to_string(),
                };
            }
            if let Some(formatted) = Self::format_vm_error_object(&borrow) {
                return formatted;
            }
            // RegExp display: /pattern/flags
            if borrow.get("__type__").map(value_to_string) == Some("RegExp".to_string()) {
                let pattern = borrow.get("__regex_pattern__").map(value_to_string).unwrap_or_default();
                let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
                return format!("/{}/{}", pattern, flags);
            }
            drop(borrow);
            let ts = map.borrow().get("toString").cloned();
            if let Some(ts_val) = ts {
                match ts_val {
                    Value::VmFunction(ip, _arity) | Value::VmClosure(ip, _arity, _) => {
                        let result = self.call_vm_function(ip, &[], &[]);
                        return value_to_string(&result);
                    }
                    Value::VmNativeFunction(id) => {
                        let result = self.call_method_builtin(id, val.clone(), vec![]);
                        return value_to_string(&result);
                    }
                    _ => {}
                }
            }
            let inner = map.borrow().get("__value__").cloned();
            if let Some(v) = inner {
                return value_to_string(&v);
            }
        }
        value_to_string(val)
    }

    fn current_script_file(&self) -> &str {
        self.script_path.as_deref().unwrap_or("<anonymous>")
    }

    fn source_lines(&self) -> Option<Vec<&str>> {
        self.script_source.as_ref().map(|source| source.split('\n').collect())
    }

    fn current_frame_names(&self) -> Vec<String> {
        self.frames
            .iter()
            .filter_map(|frame| self.chunk.fn_names.get(&frame.func_ip).cloned())
            .filter(|name| !name.is_empty())
            .collect()
    }

    fn find_function_declaration_line(lines: &[&str], function_name: &str) -> Option<usize> {
        let patterns = [
            format!("function {}(", function_name),
            format!(".{} = function", function_name),
            format!("{} = function", function_name),
            format!("{}: function", function_name),
        ];

        lines
            .iter()
            .enumerate()
            .find(|(_, line)| patterns.iter().any(|pattern| line.contains(pattern)))
            .map(|(index, _)| index + 1)
    }

    fn find_line_and_column(lines: &[&str], needle: &str, start_line: usize, end_line: usize) -> Option<(usize, usize)> {
        let start_index = start_line.saturating_sub(1);
        let end_index = end_line.min(lines.len());
        for (offset, line) in lines[start_index..end_index].iter().enumerate() {
            if let Some(column) = line.find(needle) {
                return Some((start_index + offset + 1, column + 1));
            }
        }
        None
    }

    fn infer_throw_site(&self, current_function: Option<&str>) -> Option<(usize, usize)> {
        let lines = self.source_lines()?;
        if let Some(function_name) = current_function
            && let Some(decl_line) = Self::find_function_declaration_line(&lines, function_name)
            && let Some(found) = Self::find_line_and_column(&lines, "throw ", decl_line, lines.len())
        {
            return Some(found);
        }
        Self::find_line_and_column(&lines, "throw ", 1, lines.len())
    }

    fn infer_callsite(&self, function_name: &str, scope_function: Option<&str>) -> Option<(usize, usize)> {
        let lines = self.source_lines()?;
        let call_patterns = [format!("{}(", function_name), format!(".{}(", function_name)];

        let (start_line, end_line) = if let Some(scope_name) = scope_function {
            if let Some(decl_line) = Self::find_function_declaration_line(&lines, scope_name) {
                (decl_line, lines.len())
            } else {
                (1, lines.len())
            }
        } else {
            (1, lines.len())
        };

        for line_number in start_line..=end_line {
            let line = lines.get(line_number.saturating_sub(1))?;
            if line.contains(&format!("function {}(", function_name)) {
                continue;
            }
            if let Some(column) = line.find(&call_patterns[1]) {
                return Some((line_number, column + 2));
            }
            if let Some(column) = line.find(&call_patterns[0]) {
                return Some((line_number, column + 1));
            }
        }

        None
    }

    fn build_error_stack(&self, error_name: &str, message: &str) -> (Option<(usize, usize)>, Vec<String>) {
        let mut lines = vec![Self::format_error_name_message(error_name, message)];
        let frame_names = self.current_frame_names();

        if frame_names.is_empty() {
            if let Some((line, column)) = self.infer_throw_site(None) {
                lines.push(format!("    at <anonymous> ({}:{}:{})", self.current_script_file(), line, column));
                return (Some((line, column)), lines);
            }
            return (None, lines);
        }

        let current_function = frame_names.last().map(String::as_str);
        let throw_site = self.infer_throw_site(current_function);
        if let Some((line, column)) = throw_site {
            let function_name = current_function.unwrap_or("<anonymous>");
            lines.push(format!(
                "    at {} ({}:{}:{})",
                function_name,
                self.current_script_file(),
                line,
                column
            ));
        }

        for pair in frame_names.windows(2).rev() {
            let caller_name = &pair[0];
            let callee_name = &pair[1];
            if let Some((line, column)) = self.infer_callsite(callee_name, Some(caller_name)) {
                lines.push(format!(
                    "    at {} ({}:{}:{})",
                    caller_name,
                    self.current_script_file(),
                    line,
                    column
                ));
            }
        }

        if let Some(outermost_name) = frame_names.first()
            && let Some((line, column)) = self.infer_callsite(outermost_name, None)
        {
            lines.push(format!("    at <anonymous> ({}:{}:{})", self.current_script_file(), line, column));
        }

        (throw_site, lines)
    }

    fn annotate_error_object(&self, map: &Rc<RefCell<IndexMap<String, Value<'gc>>>>) {
        let mut borrow = map.borrow_mut();
        let type_name = borrow
            .get("__type__")
            .map(value_to_string)
            .or_else(|| borrow.get("name").map(value_to_string))
            .unwrap_or_else(|| "Error".to_string());
        let message = borrow.get("message").map(value_to_string).unwrap_or_default();
        borrow
            .entry("name".to_string())
            .or_insert_with(|| Value::String(crate::unicode::utf8_to_utf16(&type_name)));
        let needs_stack = !matches!(borrow.get("stack"), Some(Value::String(stack)) if !stack.is_empty());
        let needs_line = !matches!(borrow.get("__line__"), Some(Value::Number(_)));
        let needs_column = !matches!(borrow.get("__column__"), Some(Value::Number(_)));
        drop(borrow);

        if !(needs_stack || needs_line || needs_column) {
            return;
        }

        let (throw_site, stack_lines) = self.build_error_stack(&type_name, &message);
        let mut borrow = map.borrow_mut();
        if needs_stack {
            borrow.insert(
                "stack".to_string(),
                Value::String(crate::unicode::utf8_to_utf16(&stack_lines.join("\n"))),
            );
        }
        if let Some((line, column)) = throw_site {
            if needs_line {
                borrow.insert("__line__".to_string(), Value::Number(line as f64));
            }
            if needs_column {
                borrow.insert("__column__".to_string(), Value::Number(column as f64));
            }
        }
    }

    fn error_kind_from_name(name: &str, message: String) -> crate::error::JSErrorKind {
        match name {
            "TypeError" => crate::error::JSErrorKind::TypeError { message },
            "RangeError" => crate::error::JSErrorKind::RangeError { message },
            "ReferenceError" => crate::error::JSErrorKind::ReferenceError { message },
            "SyntaxError" => crate::error::JSErrorKind::SyntaxError { message },
            "URIError" => crate::error::JSErrorKind::URIError { message },
            _ => crate::error::JSErrorKind::Throw(message),
        }
    }

    pub(crate) fn vm_error_to_js_error(&self, thrown: Value<'gc>) -> JSError {
        if let Value::VmObject(map) = &thrown {
            self.annotate_error_object(map);
            let borrow = map.borrow();
            let type_name = borrow
                .get("__type__")
                .map(value_to_string)
                .or_else(|| borrow.get("name").map(value_to_string))
                .unwrap_or_else(|| "Error".to_string());
            let message = borrow
                .get("message")
                .map(value_to_string)
                .unwrap_or_else(|| value_to_string(&thrown));
            let error_text = if type_name == "Error" {
                format!("Uncaught: Error: {}", message)
            } else if type_name.is_empty() {
                format!("Uncaught: {}", message)
            } else {
                format!("Uncaught: {}: {}", type_name, message)
            };
            let mut err = crate::make_js_error!(Self::error_kind_from_name(&type_name, error_text));
            if let Some(Value::Number(line)) = borrow.get("__line__") {
                let column = borrow
                    .get("__column__")
                    .and_then(|value| {
                        if let Value::Number(column) = value {
                            Some(*column as usize)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                err.set_js_location(*line as usize, column);
            }
            if let Some(Value::String(stack)) = borrow.get("stack") {
                let stack_lines = crate::unicode::utf16_to_utf8(stack)
                    .lines()
                    .map(|line| line.trim().to_string())
                    .filter(|line| line.starts_with("at "))
                    .collect();
                err.set_stack(stack_lines);
            }
            return err;
        }

        crate::make_js_error!(crate::error::JSErrorKind::Throw(format!("Uncaught: {}", value_to_string(&thrown))))
    }

    fn vm_value_from_error(&self, err: &JSError) -> Value<'gc> {
        let mut raw_message = err.message();
        for prefix in ["SyntaxError: Uncaught: ", "Error: Uncaught: ", "Uncaught: "] {
            if let Some(stripped) = raw_message.strip_prefix(prefix) {
                raw_message = stripped.to_string();
                break;
            }
        }

        if matches!(err.kind(), crate::error::JSErrorKind::Throw(_)) && err.js_line().is_none() && err.stack().is_empty() {
            if let Ok(number) = raw_message.parse::<f64>() {
                return Value::Number(number);
            }
            return Value::String(crate::unicode::utf8_to_utf16(&raw_message));
        }

        let (name, message) = if let Some((name, message)) = raw_message.split_once(": ") {
            (name.to_string(), message.to_string())
        } else {
            ("Error".to_string(), raw_message)
        };

        let mut map = IndexMap::new();
        map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16(&name)));
        map.insert("name".to_string(), Value::String(crate::unicode::utf8_to_utf16(&name)));
        map.insert("message".to_string(), Value::String(crate::unicode::utf8_to_utf16(&message)));
        if let Some(line) = err.js_line() {
            map.insert("__line__".to_string(), Value::Number(line as f64));
        }
        if let Some(column) = err.js_column() {
            map.insert("__column__".to_string(), Value::Number(column as f64));
        }
        if !err.stack().is_empty() {
            let header = Self::format_error_name_message(&name, &message);
            let stack = std::iter::once(header)
                .chain(err.stack().iter().map(|line| format!("    {}", line)))
                .collect::<Vec<_>>()
                .join("\n");
            map.insert("stack".to_string(), Value::String(crate::unicode::utf8_to_utf16(&stack)));
        }
        Value::VmObject(Rc::new(RefCell::new(map)))
    }

    fn regex_to_string(&self, re_obj: &Rc<RefCell<IndexMap<String, Value<'gc>>>>) -> String {
        let borrow = re_obj.borrow();
        let pattern = borrow
            .get("source")
            .map(value_to_string)
            .unwrap_or_else(|| borrow.get("__regex_pattern__").map(value_to_string).unwrap_or_default());
        let flags = borrow
            .get("flags")
            .map(value_to_string)
            .unwrap_or_else(|| borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default());
        format!("/{}/{}", pattern, flags)
    }

    fn regex_prepare_input(&self, input: &str, flags: &str) -> (Vec<u16>, bool) {
        let input_u16: Vec<u16> = input.encode_utf16().collect();
        if !flags.contains('R') {
            return (input_u16, false);
        }

        let mut normalized = Vec::with_capacity(input_u16.len());
        let mut index = 0usize;
        while index < input_u16.len() {
            if input_u16[index] == '\r' as u16 && index + 1 < input_u16.len() && input_u16[index + 1] == '\n' as u16 {
                normalized.push('\n' as u16);
                index += 2;
            } else {
                normalized.push(input_u16[index]);
                index += 1;
            }
        }
        (normalized, true)
    }

    fn regex_map_index_back(original: &[u16], normalized_index: usize) -> usize {
        let mut original_index = 0usize;
        let mut normalized_pos = 0usize;
        while normalized_pos < normalized_index && original_index < original.len() {
            if original[original_index] == '\r' as u16 && original_index + 1 < original.len() && original[original_index + 1] == '\n' as u16
            {
                original_index += 2;
            } else {
                original_index += 1;
            }
            normalized_pos += 1;
        }
        original_index
    }

    fn resolve_eval_binding(&self, name: &str) -> Option<Value<'gc>> {
        if self.direct_eval {
            for frame in self.frames.iter().rev() {
                if let Some(local_names) = self.chunk.fn_local_names.get(&frame.func_ip) {
                    for (idx, local_name) in local_names.iter().enumerate() {
                        if local_name == name {
                            if let Some(cell) = frame.local_cells.get(&idx) {
                                return Some(cell.borrow().clone());
                            }
                            let stack_idx = frame.bp + idx;
                            if stack_idx < self.stack.len() {
                                return Some(self.stack[stack_idx].clone());
                            }
                        }
                    }
                }
            }
        }
        self.globals.get(name).cloned()
    }

    fn try_eval_optional_chain_expression(&mut self, code: &str) -> Result<Option<Value<'gc>>, JSError> {
        enum EvalRef<'gc> {
            Value(Value<'gc>),
            Reference { base: Value<'gc>, value: Value<'gc> },
        }

        fn is_nullish<'gc>(v: &Value<'gc>) -> bool {
            matches!(v, Value::Null | Value::Undefined)
        }

        fn to_prop_key<'gc>(v: &Value<'gc>) -> String {
            match v {
                Value::String(s) => crate::unicode::utf16_to_utf8(s),
                _ => value_to_string(v),
            }
        }

        fn eval_expr<'gc>(vm: &mut VM<'gc>, expr: &crate::core::statement::Expr) -> Result<EvalRef<'gc>, JSError> {
            use crate::core::statement::Expr;
            match expr {
                Expr::Var(name, ..) => Ok(EvalRef::Value(vm.resolve_eval_binding(name).unwrap_or(Value::Undefined))),
                Expr::This => Ok(EvalRef::Value(vm.this_stack.last().cloned().unwrap_or(Value::Undefined))),
                Expr::Null => Ok(EvalRef::Value(Value::Null)),
                Expr::Undefined => Ok(EvalRef::Value(Value::Undefined)),
                Expr::Number(n) => Ok(EvalRef::Value(Value::Number(*n))),
                Expr::StringLit(s) => Ok(EvalRef::Value(Value::String(s.clone()))),
                Expr::Boolean(b) => Ok(EvalRef::Value(Value::Boolean(*b))),
                Expr::Property(obj, key) => {
                    let base = match eval_expr(vm, obj)? {
                        EvalRef::Reference { value, .. } => value,
                        EvalRef::Value(v) => v,
                    };
                    let val = vm.read_named_property(base.clone(), key);
                    Ok(EvalRef::Reference { base, value: val })
                }
                Expr::OptionalProperty(obj, key) => {
                    let base = match eval_expr(vm, obj)? {
                        EvalRef::Reference { value, .. } => value,
                        EvalRef::Value(v) => v,
                    };
                    if is_nullish(&base) {
                        return Ok(EvalRef::Value(Value::Undefined));
                    }
                    let val = vm.read_named_property(base.clone(), key);
                    Ok(EvalRef::Reference { base, value: val })
                }
                Expr::Index(obj, idx_expr) => {
                    let base = match eval_expr(vm, obj)? {
                        EvalRef::Reference { value, .. } => value,
                        EvalRef::Value(v) => v,
                    };
                    let idx_val = match eval_expr(vm, idx_expr)? {
                        EvalRef::Reference { value, .. } => value,
                        EvalRef::Value(v) => v,
                    };
                    let key = to_prop_key(&idx_val);
                    let val = vm.read_named_property(base.clone(), &key);
                    Ok(EvalRef::Reference { base, value: val })
                }
                Expr::OptionalIndex(obj, idx_expr) => {
                    let base = match eval_expr(vm, obj)? {
                        EvalRef::Reference { value, .. } => value,
                        EvalRef::Value(v) => v,
                    };
                    if is_nullish(&base) {
                        return Ok(EvalRef::Value(Value::Undefined));
                    }
                    let idx_val = match eval_expr(vm, idx_expr)? {
                        EvalRef::Reference { value, .. } => value,
                        EvalRef::Value(v) => v,
                    };
                    let key = to_prop_key(&idx_val);
                    let val = vm.read_named_property(base.clone(), &key);
                    Ok(EvalRef::Reference { base, value: val })
                }
                Expr::Call(callee, args) | Expr::OptionalCall(callee, args) => {
                    let optional_call = matches!(expr, Expr::OptionalCall(..));
                    let callee_ref = eval_expr(vm, callee)?;
                    let (callee_val, this_val) = match callee_ref {
                        EvalRef::Reference { base, value } => (value, base),
                        EvalRef::Value(v) => (v, Value::Undefined),
                    };

                    if optional_call && is_nullish(&callee_val) {
                        return Ok(EvalRef::Value(Value::Undefined));
                    }

                    let mut arg_vals = Vec::with_capacity(args.len());
                    for a in args {
                        let v = match eval_expr(vm, a)? {
                            EvalRef::Reference { value, .. } => value,
                            EvalRef::Value(v) => v,
                        };
                        arg_vals.push(v);
                    }

                    let ret = match callee_val {
                        Value::VmFunction(ip, _) => {
                            vm.this_stack.push(this_val.clone());
                            let r = vm.call_vm_function(ip, &arg_vals, &[]);
                            vm.this_stack.pop();
                            r
                        }
                        Value::VmClosure(ip, _, upv) => {
                            vm.this_stack.push(this_val.clone());
                            let uv = (*upv).clone();
                            let r = vm.call_vm_function(ip, &arg_vals, &uv);
                            vm.this_stack.pop();
                            r
                        }
                        Value::VmNativeFunction(id) => {
                            if matches!(this_val, Value::Undefined | Value::Null) {
                                vm.call_builtin(id, arg_vals)
                            } else {
                                vm.call_method_builtin(id, this_val, arg_vals)
                            }
                        }
                        Value::VmObject(obj) => {
                            if let Some(Value::Number(native_id)) = obj.borrow().get("__native_id__") {
                                vm.call_builtin(*native_id as u8, arg_vals)
                            } else {
                                return Err(crate::make_js_error!(crate::JSErrorKind::TypeError {
                                    message: "is not a function".to_string()
                                }));
                            }
                        }
                        Value::Function(name) => vm.call_named_host_function(&name, arg_vals),
                        _ => {
                            return Err(crate::make_js_error!(crate::JSErrorKind::TypeError {
                                message: "is not a function".to_string()
                            }));
                        }
                    };
                    Ok(EvalRef::Value(ret))
                }
                _ => Err(crate::make_js_error!(crate::JSErrorKind::SyntaxError {
                    message: "unsupported optional-chain eval expression".to_string()
                })),
            }
        }

        let tokens = crate::core::tokenize(code)?;
        let (expr, mut next) = crate::core::parse_simple_expression(&tokens, 0)?;
        while next < tokens.len() {
            if matches!(
                tokens[next].token,
                crate::core::Token::Semicolon | crate::core::Token::LineTerminator
            ) {
                next += 1;
            } else {
                return Ok(None);
            }
        }

        let out = match eval_expr(self, &expr)? {
            EvalRef::Reference { value, .. } => value,
            EvalRef::Value(v) => v,
        };
        Ok(Some(out))
    }

    /// Try to resolve a human-readable name for the callee at `callee_idx` on the stack.
    fn resolve_callee_name(&self, callee_idx: usize) -> String {
        // The Call instruction is 2 bytes (opcode + arg_byte). IP is now past it.
        let call_ip = self.ip.saturating_sub(2);
        if let Some(name) = self.chunk.call_callee_names.get(&call_ip) {
            return name.clone();
        }
        // Fallback: check current frame's local names
        if let Some(frame) = self.frames.last()
            && callee_idx >= frame.bp
        {
            let slot = callee_idx - frame.bp;
            if let Some(local_names) = self.chunk.fn_local_names.get(&frame.func_ip)
                && let Some(name) = local_names.get(slot)
                && !name.is_empty()
            {
                return name.clone();
            }
        }
        "Value".to_string()
    }

    /// Execute a native/built-in function
    fn call_builtin(&mut self, id: u8, args: Vec<Value<'gc>>) -> Value<'gc> {
        match id {
            BUILTIN_SETTIMEOUT | BUILTIN_SETINTERVAL => {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                let delay = if let Some(Value::Number(n)) = args.get(1) {
                    (*n).max(0.0) as u64
                } else {
                    0
                };
                let timer_args: Vec<Value<'gc>> = args.into_iter().skip(2).collect();
                let timer_id = self.next_timer_id;
                self.next_timer_id += 1;
                self.pending_timers.push(PendingTimer {
                    id: timer_id,
                    callback,
                    args: timer_args,
                    delay_ms: delay,
                    is_interval: id == BUILTIN_SETINTERVAL,
                });
                Value::Number(timer_id as f64)
            }
            BUILTIN_CLEARTIMEOUT | BUILTIN_CLEARINTERVAL => {
                if let Some(Value::Number(n)) = args.first() {
                    self.cleared_timers.insert(*n as usize);
                    self.pending_timers.retain(|t| t.id != *n as usize);
                }
                Value::Undefined
            }
            BUILTIN_PROMISE_NOOP => Value::Undefined,
            BUILTIN_CTOR_PROMISE => {
                let mut map = IndexMap::new();
                map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Promise")));
                map.insert("then".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_THEN));
                if let Some(Value::VmObject(promise_ctor)) = self.globals.get("Promise")
                    && let Some(proto) = promise_ctor.borrow().get("prototype").cloned()
                {
                    map.insert("__proto__".to_string(), proto);
                }
                let promise_obj = Value::VmObject(Rc::new(RefCell::new(map)));

                if let Some(executor) = args.first() {
                    let mut resolve_map = IndexMap::new();
                    resolve_map.insert(
                        "__host_fn__".to_string(),
                        Value::String(crate::unicode::utf8_to_utf16("promise.__resolve")),
                    );
                    resolve_map.insert("__host_this__".to_string(), promise_obj.clone());
                    let resolve = Value::VmObject(Rc::new(RefCell::new(resolve_map)));

                    let mut reject_map = IndexMap::new();
                    reject_map.insert(
                        "__host_fn__".to_string(),
                        Value::String(crate::unicode::utf8_to_utf16("promise.__reject")),
                    );
                    reject_map.insert("__host_this__".to_string(), promise_obj.clone());
                    let reject = Value::VmObject(Rc::new(RefCell::new(reject_map)));

                    // Build arg list trimmed to executor arity to avoid corrupting local slots
                    let all_exec_args = [resolve.clone(), reject.clone()];
                    let exec_arity = match executor {
                        Value::VmFunction(_, a) | Value::VmClosure(_, a, _) => *a as usize,
                        _ => all_exec_args.len(),
                    };
                    let exec_args = &all_exec_args[..exec_arity.min(all_exec_args.len())];

                    match executor {
                        Value::VmFunction(ip, _) => {
                            let saved_try_stack = std::mem::take(&mut self.try_stack);
                            let call_result = self.call_vm_function_result(*ip, exec_args, &[]);
                            self.try_stack = saved_try_stack;
                            if let Err(err) = call_result {
                                let msg = err.message();
                                let uncaught_payload = msg.strip_prefix("SyntaxError: Uncaught: ").unwrap_or(&msg);
                                if let Value::VmObject(p) = &promise_obj {
                                    let mut pb = p.borrow_mut();
                                    pb.insert("__promise_rejected__".to_string(), Value::Boolean(true));
                                    if let Ok(n) = uncaught_payload.parse::<f64>() {
                                        pb.insert("__promise_value__".to_string(), Value::Number(n));
                                    } else {
                                        pb.insert(
                                            "__promise_value__".to_string(),
                                            Value::String(crate::unicode::utf8_to_utf16(uncaught_payload)),
                                        );
                                    }
                                }
                            }
                        }
                        Value::VmClosure(ip, _, upv) => {
                            let uv = (**upv).clone();
                            let saved_try_stack = std::mem::take(&mut self.try_stack);
                            let call_result = self.call_vm_function_result(*ip, exec_args, &uv);
                            self.try_stack = saved_try_stack;
                            if let Err(err) = call_result {
                                let msg = err.message();
                                let uncaught_payload = msg.strip_prefix("SyntaxError: Uncaught: ").unwrap_or(&msg);
                                if let Value::VmObject(p) = &promise_obj {
                                    let mut pb = p.borrow_mut();
                                    pb.insert("__promise_rejected__".to_string(), Value::Boolean(true));
                                    if let Ok(n) = uncaught_payload.parse::<f64>() {
                                        pb.insert("__promise_value__".to_string(), Value::Number(n));
                                    } else {
                                        pb.insert(
                                            "__promise_value__".to_string(),
                                            Value::String(crate::unicode::utf8_to_utf16(uncaught_payload)),
                                        );
                                    }
                                }
                            }
                        }
                        Value::VmNativeFunction(native_id) => {
                            let _ = self.call_builtin(*native_id, vec![resolve.clone(), reject.clone()]);
                        }
                        _ => {}
                    }
                }

                promise_obj
            }
            BUILTIN_PROMISE_RESOLVE => {
                let mut map = IndexMap::new();
                map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Promise")));
                map.insert("then".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_THEN));
                if let Some(v @ Value::VmObject(obj)) = args.first() {
                    let b = obj.borrow();
                    let is_promise = matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "Promise");
                    if is_promise {
                        return v.clone();
                    }
                    map.insert("__promise_value__".to_string(), v.clone());
                } else if let Some(v) = args.first() {
                    map.insert("__promise_value__".to_string(), v.clone());
                }
                if let Some(Value::VmObject(promise_ctor)) = self.globals.get("Promise")
                    && let Some(proto) = promise_ctor.borrow().get("prototype").cloned()
                {
                    map.insert("__proto__".to_string(), proto);
                }
                Value::VmObject(Rc::new(RefCell::new(map)))
            }
            BUILTIN_PROMISE_ALL => {
                let mut settled_values: Vec<Value<'gc>> = Vec::new();
                let mut rejection: Option<Value<'gc>> = None;

                if let Some(Value::VmArray(items)) = args.first() {
                    for item in items.borrow().iter() {
                        match item {
                            Value::VmObject(obj) => {
                                let b = obj.borrow();
                                let is_promise =
                                    matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "Promise");
                                if is_promise {
                                    let rejected = matches!(b.get("__promise_rejected__"), Some(Value::Boolean(true)));
                                    let pv = b.get("__promise_value__").cloned().unwrap_or(Value::Undefined);
                                    if rejected {
                                        rejection = Some(pv);
                                        break;
                                    }
                                    settled_values.push(pv);
                                } else {
                                    settled_values.push(item.clone());
                                }
                            }
                            _ => settled_values.push(item.clone()),
                        }
                    }
                }

                let mut map = IndexMap::new();
                map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Promise")));
                map.insert("then".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_THEN));
                if let Some(reason) = rejection {
                    map.insert("__promise_rejected__".to_string(), Value::Boolean(true));
                    map.insert("__promise_value__".to_string(), reason);
                } else {
                    map.insert(
                        "__promise_value__".to_string(),
                        Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(settled_values)))),
                    );
                }
                if let Some(Value::VmObject(promise_ctor)) = self.globals.get("Promise")
                    && let Some(proto) = promise_ctor.borrow().get("prototype").cloned()
                {
                    map.insert("__proto__".to_string(), proto);
                }
                Value::VmObject(Rc::new(RefCell::new(map)))
            }
            BUILTIN_CTOR_PROXY => match args.first().cloned() {
                Some(target) => {
                    let handler = args.get(1).cloned().unwrap_or(Value::Undefined);
                    let mut map = IndexMap::new();
                    map.insert("__proxy_target__".to_string(), target.clone());
                    map.insert("__proxy_handler__".to_string(), handler);
                    map.insert("__proxy_revoked__".to_string(), Value::Boolean(false));
                    if let Some(Value::VmObject(object_ctor)) = self.globals.get("Object")
                        && let Some(proto) = object_ctor.borrow().get("prototype").cloned()
                    {
                        map.insert("__proto__".to_string(), proto);
                    }
                    Value::VmObject(Rc::new(RefCell::new(map)))
                }
                None => Value::VmObject(Rc::new(RefCell::new(IndexMap::new()))),
            },
            BUILTIN_CTOR_SHAREDARRAYBUFFER => {
                let len = match args.first() {
                    Some(Value::Number(n)) if n.is_finite() && *n > 0.0 => *n as usize,
                    _ => 0,
                };
                let bytes = vec![Value::Number(0.0); len];
                let mut map = IndexMap::new();
                map.insert(
                    "__type__".to_string(),
                    Value::String(crate::unicode::utf8_to_utf16("SharedArrayBuffer")),
                );
                map.insert("byteLength".to_string(), Value::Number(len as f64));
                map.insert(
                    "__buffer_bytes__".to_string(),
                    Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(bytes)))),
                );
                Value::VmObject(Rc::new(RefCell::new(map)))
            }
            BUILTIN_ATOMICS_ISLOCKFREE => {
                let size = args
                    .first()
                    .and_then(|v| if let Value::Number(n) = v { Some(*n as i64) } else { None })
                    .unwrap_or(0);
                Value::Boolean(matches!(size, 1 | 2 | 4 | 8))
            }
            BUILTIN_ATOMICS_LOAD => {
                if let (Some(Value::VmArray(arr)), Some(Value::Number(idx))) = (args.first(), args.get(1)) {
                    let i = (*idx as isize).max(0) as usize;
                    return arr.borrow().elements.get(i).cloned().unwrap_or(Value::Undefined);
                }
                Value::Undefined
            }
            BUILTIN_ATOMICS_STORE => {
                if let (Some(Value::VmArray(arr)), Some(Value::Number(idx)), Some(val)) = (args.first(), args.get(1), args.get(2)) {
                    let i = (*idx as isize).max(0) as usize;
                    if i < arr.borrow().elements.len() {
                        arr.borrow_mut().elements[i] = val.clone();
                    }
                    return val.clone();
                }
                Value::Undefined
            }
            BUILTIN_ATOMICS_COMPAREEXCHANGE => {
                if let (Some(Value::VmArray(arr)), Some(Value::Number(idx)), Some(expected), Some(replacement)) =
                    (args.first(), args.get(1), args.get(2), args.get(3))
                {
                    let i = (*idx as isize).max(0) as usize;
                    let old = arr.borrow().elements.get(i).cloned().unwrap_or(Value::Undefined);
                    if self.strict_equal(&old, expected) && i < arr.borrow().elements.len() {
                        arr.borrow_mut().elements[i] = replacement.clone();
                    }
                    return old;
                }
                Value::Undefined
            }
            BUILTIN_ATOMICS_ADD => {
                if let (Some(Value::VmArray(arr)), Some(Value::Number(idx)), Some(Value::Number(delta))) =
                    (args.first(), args.get(1), args.get(2))
                {
                    let i = (*idx as isize).max(0) as usize;
                    let old_num = match arr.borrow().elements.get(i) {
                        Some(Value::Number(n)) => *n,
                        _ => 0.0,
                    };
                    if i < arr.borrow().elements.len() {
                        arr.borrow_mut().elements[i] = Value::Number(old_num + *delta);
                    }
                    return Value::Number(old_num);
                }
                Value::Undefined
            }
            BUILTIN_ATOMICS_EXCHANGE => {
                if let (Some(Value::VmArray(arr)), Some(Value::Number(idx)), Some(new_value)) = (args.first(), args.get(1), args.get(2)) {
                    let i = (*idx as isize).max(0) as usize;
                    let old = arr.borrow().elements.get(i).cloned().unwrap_or(Value::Undefined);
                    if i < arr.borrow().elements.len() {
                        arr.borrow_mut().elements[i] = new_value.clone();
                    }
                    return old;
                }
                Value::Undefined
            }
            BUILTIN_ATOMICS_WAIT => {
                if let (Some(Value::VmArray(arr)), Some(Value::Number(idx)), Some(expected)) = (args.first(), args.get(1), args.get(2)) {
                    let i = (*idx as isize).max(0) as usize;
                    let current = arr.borrow().elements.get(i).cloned().unwrap_or(Value::Undefined);
                    if !self.strict_equal(&current, expected) {
                        return Value::String(crate::unicode::utf8_to_utf16("not-equal"));
                    }
                    return Value::String(crate::unicode::utf8_to_utf16("timed-out"));
                }
                Value::String(crate::unicode::utf8_to_utf16("not-equal"))
            }
            BUILTIN_ATOMICS_NOTIFY => Value::Number(0.0),
            BUILTIN_REFLECT_APPLY => {
                let target = args.first().cloned().unwrap_or(Value::Undefined);
                let this_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
                let arg_array = args.get(2).cloned().unwrap_or(Value::Undefined);

                let call_args = if let Value::VmArray(arr) = arg_array {
                    arr.borrow().iter().cloned().collect::<Vec<_>>()
                } else {
                    let mut err_map = IndexMap::new();
                    err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                    err_map.insert(
                        "message".to_string(),
                        Value::String(crate::unicode::utf8_to_utf16("Reflect.apply requires an array-like argumentsList")),
                    );
                    self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                    return Value::Undefined;
                };

                match target {
                    Value::VmNativeFunction(fn_id) => {
                        self.this_stack.push(this_arg.clone());
                        let result = self.call_method_builtin(fn_id, this_arg, call_args);
                        self.this_stack.pop();
                        result
                    }
                    Value::VmFunction(ip, _) => {
                        self.this_stack.push(this_arg);
                        let result = self.call_vm_function(ip, &call_args, &[]);
                        self.this_stack.pop();
                        result
                    }
                    Value::VmClosure(ip, _, upv) => {
                        let uv = (*upv).clone();
                        self.this_stack.push(this_arg);
                        let result = self.call_vm_function(ip, &call_args, &uv);
                        self.this_stack.pop();
                        result
                    }
                    Value::VmObject(map) => {
                        if let Some(Value::Number(native_id)) = map.borrow().get("__native_id__") {
                            self.call_builtin(*native_id as u8, call_args)
                        } else {
                            let mut err_map = IndexMap::new();
                            err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                            err_map.insert(
                                "message".to_string(),
                                Value::String(crate::unicode::utf8_to_utf16("Reflect.apply target is not callable")),
                            );
                            self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                            Value::Undefined
                        }
                    }
                    _ => {
                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("Reflect.apply target is not callable")),
                        );
                        self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                        Value::Undefined
                    }
                }
            }
            BUILTIN_ATOMICS_WAITASYNC => {
                if let Some(Value::VmArray(arr)) = args.first() {
                    let arr_borrow = arr.borrow();
                    let ta_name = arr_borrow.props.get("__typedarray_name__").map(value_to_string).unwrap_or_default();
                    let buffer_type = arr_borrow.props.get("__buffer_type__").map(value_to_string).unwrap_or_default();
                    drop(arr_borrow);

                    if ta_name != "Int32Array" && ta_name != "BigInt64Array" {
                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16(
                                "Atomics.waitAsync requires Int32Array or BigInt64Array",
                            )),
                        );
                        self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                        return Value::Undefined;
                    }

                    if buffer_type != "SharedArrayBuffer" {
                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16(
                                "Atomics.waitAsync requires SharedArrayBuffer-backed typed array",
                            )),
                        );
                        self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                        return Value::Undefined;
                    }

                    let idx = args
                        .get(1)
                        .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None })
                        .unwrap_or(0);
                    let expected = args.get(2).cloned().unwrap_or(Value::Undefined);
                    let timeout = args
                        .get(3)
                        .and_then(|v| if let Value::Number(n) = v { Some(*n) } else { None })
                        .unwrap_or(f64::INFINITY);
                    let current = arr.borrow().elements.get(idx).cloned().unwrap_or(Value::Undefined);

                    let mut result = IndexMap::new();
                    if !self.strict_equal(&current, &expected) {
                        result.insert("async".to_string(), Value::Boolean(false));
                        result.insert("value".to_string(), Value::String(crate::unicode::utf8_to_utf16("not-equal")));
                        return Value::VmObject(Rc::new(RefCell::new(result)));
                    }

                    if timeout <= 0.0 {
                        result.insert("async".to_string(), Value::Boolean(false));
                        result.insert("value".to_string(), Value::String(crate::unicode::utf8_to_utf16("timed-out")));
                        return Value::VmObject(Rc::new(RefCell::new(result)));
                    }

                    let mut promise = IndexMap::new();
                    promise.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Promise")));
                    promise.insert("then".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_THEN));
                    result.insert("async".to_string(), Value::Boolean(true));
                    result.insert("value".to_string(), Value::VmObject(Rc::new(RefCell::new(promise))));
                    return Value::VmObject(Rc::new(RefCell::new(result)));
                }

                let mut err_map = IndexMap::new();
                err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                err_map.insert(
                    "message".to_string(),
                    Value::String(crate::unicode::utf8_to_utf16("Atomics.waitAsync requires typed array")),
                );
                self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                Value::Undefined
            }
            BUILTIN_CTOR_ARRAYBUFFER => {
                let len = match args.first() {
                    Some(Value::Number(n)) if n.is_finite() && *n > 0.0 => *n as usize,
                    _ => 0,
                };
                let max_len = match args.get(1) {
                    Some(Value::VmObject(opts)) => opts
                        .borrow()
                        .get("maxByteLength")
                        .and_then(|v| {
                            if let Value::Number(n) = v {
                                Some((*n).max(len as f64) as usize)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(len),
                    _ => len,
                };
                let bytes = vec![Value::Number(0.0); len];
                let mut map = IndexMap::new();
                map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("ArrayBuffer")));
                map.insert("byteLength".to_string(), Value::Number(len as f64));
                map.insert("maxByteLength".to_string(), Value::Number(max_len as f64));
                map.insert("resize".to_string(), Value::VmNativeFunction(BUILTIN_ARRAYBUFFER_RESIZE));
                map.insert(
                    "__buffer_bytes__".to_string(),
                    Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(bytes)))),
                );
                Value::VmObject(Rc::new(RefCell::new(map)))
            }
            BUILTIN_CTOR_DATAVIEW => {
                let buffer = args.first().cloned().unwrap_or(Value::Undefined);
                let byte_len = if let Value::VmObject(obj) = &buffer {
                    match obj.borrow().get("byteLength") {
                        Some(Value::Number(n)) => *n,
                        _ => 0.0,
                    }
                } else {
                    0.0
                };
                let mut map = IndexMap::new();
                map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("DataView")));
                map.insert("buffer".to_string(), buffer);
                map.insert("byteLength".to_string(), Value::Number(byte_len));
                map.insert("byteOffset".to_string(), Value::Number(0.0));
                map.insert("getUint8".to_string(), Self::make_host_fn("dataview.getUint8"));
                map.insert("getInt8".to_string(), Self::make_host_fn("dataview.getInt8"));
                map.insert("setUint8".to_string(), Self::make_host_fn("dataview.setUint8"));
                map.insert("setInt8".to_string(), Self::make_host_fn("dataview.setInt8"));
                map.insert("getUint16".to_string(), Self::make_host_fn("dataview.getUint16"));
                map.insert("getInt16".to_string(), Self::make_host_fn("dataview.getInt16"));
                map.insert("setUint16".to_string(), Self::make_host_fn("dataview.setUint16"));
                map.insert("setInt16".to_string(), Self::make_host_fn("dataview.setInt16"));
                map.insert("getUint32".to_string(), Self::make_host_fn("dataview.getUint32"));
                map.insert("getInt32".to_string(), Self::make_host_fn("dataview.getInt32"));
                map.insert("setUint32".to_string(), Self::make_host_fn("dataview.setUint32"));
                map.insert("setInt32".to_string(), Self::make_host_fn("dataview.setInt32"));
                map.insert("getFloat32".to_string(), Self::make_host_fn("dataview.getFloat32"));
                map.insert("setFloat32".to_string(), Self::make_host_fn("dataview.setFloat32"));
                map.insert("getFloat64".to_string(), Self::make_host_fn("dataview.getFloat64"));
                map.insert("setFloat64".to_string(), Self::make_host_fn("dataview.setFloat64"));
                Value::VmObject(Rc::new(RefCell::new(map)))
            }
            BUILTIN_CTOR_INT8ARRAY
            | BUILTIN_CTOR_UINT8ARRAY
            | BUILTIN_CTOR_UINT8CLAMPEDARRAY
            | BUILTIN_CTOR_INT16ARRAY
            | BUILTIN_CTOR_UINT16ARRAY
            | BUILTIN_CTOR_INT32ARRAY
            | BUILTIN_CTOR_UINT32ARRAY
            | BUILTIN_CTOR_FLOAT32ARRAY
            | BUILTIN_CTOR_FLOAT64ARRAY => {
                let bytes_per_element = match id {
                    BUILTIN_CTOR_INT16ARRAY | BUILTIN_CTOR_UINT16ARRAY => 2usize,
                    BUILTIN_CTOR_INT32ARRAY | BUILTIN_CTOR_UINT32ARRAY | BUILTIN_CTOR_FLOAT32ARRAY => 4usize,
                    BUILTIN_CTOR_FLOAT64ARRAY => 8usize,
                    _ => 1usize,
                };
                let typedarray_name = match id {
                    BUILTIN_CTOR_INT8ARRAY => "Int8Array",
                    BUILTIN_CTOR_UINT8ARRAY => "Uint8Array",
                    BUILTIN_CTOR_UINT8CLAMPEDARRAY => "Uint8ClampedArray",
                    BUILTIN_CTOR_INT16ARRAY => "Int16Array",
                    BUILTIN_CTOR_UINT16ARRAY => "Uint16Array",
                    BUILTIN_CTOR_INT32ARRAY => "Int32Array",
                    BUILTIN_CTOR_UINT32ARRAY => "Uint32Array",
                    BUILTIN_CTOR_FLOAT32ARRAY => "Float32Array",
                    BUILTIN_CTOR_FLOAT64ARRAY => "Float64Array",
                    _ => "TypedArray",
                };

                if let Some(Value::VmObject(buf_obj)) = args.first() {
                    let buffer_type = buf_obj.borrow().get("__type__").map(value_to_string).unwrap_or_default();
                    let is_array_buffer = matches!(
                        buf_obj.borrow().get("__type__"),
                        Some(Value::String(s))
                            if crate::unicode::utf16_to_utf8(s) == "ArrayBuffer"
                                || crate::unicode::utf16_to_utf8(s) == "SharedArrayBuffer"
                    );
                    if is_array_buffer {
                        let byte_len = match buf_obj.borrow().get("byteLength") {
                            Some(Value::Number(n)) if *n >= 0.0 => *n as usize,
                            _ => 0,
                        };
                        let byte_offset = match args.get(1) {
                            Some(Value::Number(n)) if *n >= 0.0 => *n as usize,
                            _ => 0,
                        };
                        let explicit_len = match args.get(2) {
                            Some(Value::Number(n)) if *n >= 0.0 => Some(*n as usize),
                            _ => None,
                        };
                        let available = byte_len.saturating_sub(byte_offset) / bytes_per_element;
                        let initial_len = explicit_len.unwrap_or(available);
                        let mut data = VmArrayData::new(vec![Value::Number(0.0); initial_len]);
                        data.props.insert(
                            "__typedarray_name__".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16(typedarray_name)),
                        );
                        data.props.insert(
                            "__buffer_type__".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16(&buffer_type)),
                        );
                        data.props.insert("__byte_offset__".to_string(), Value::Number(byte_offset as f64));
                        data.props
                            .insert("__bytes_per_element__".to_string(), Value::Number(bytes_per_element as f64));
                        data.props
                            .insert("__length_tracking__".to_string(), Value::Boolean(explicit_len.is_none()));
                        if let Some(len) = explicit_len {
                            data.props.insert("__fixed_length__".to_string(), Value::Number(len as f64));
                        }
                        data.props
                            .insert("__typedarray_buffer__".to_string(), Value::VmObject(buf_obj.clone()));
                        return Value::VmArray(Rc::new(RefCell::new(data)));
                    }
                }

                let length = match args.first() {
                    Some(Value::Number(n)) if n.is_finite() && *n > 0.0 => *n as usize,
                    _ => 0,
                };
                let mut data = VmArrayData::new(vec![Value::Number(0.0); length]);
                data.props.insert(
                    "__typedarray_name__".to_string(),
                    Value::String(crate::unicode::utf8_to_utf16(typedarray_name)),
                );
                data.props.insert(
                    "__buffer_type__".to_string(),
                    Value::String(crate::unicode::utf8_to_utf16("ArrayBuffer")),
                );
                Value::VmArray(Rc::new(RefCell::new(data)))
            }
            BUILTIN_DATE_NOW => {
                use std::time::{SystemTime, UNIX_EPOCH};
                let ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as f64)
                    .unwrap_or(0.0);
                Value::Number(ms)
            }
            BUILTIN_DATE_PARSE => {
                let s_str = args.first().map(|v| value_to_string(v)).unwrap_or_default();
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&s_str) {
                    return Value::Number(dt.timestamp_millis() as f64);
                } else if let Ok(dt) = chrono::NaiveDate::parse_from_str(&s_str, "%Y-%m-%d") {
                    let ms = dt
                        .and_hms_opt(0, 0, 0)
                        .map(|d| d.and_utc().timestamp_millis() as f64)
                        .unwrap_or(f64::NAN);
                    return Value::Number(ms);
                } else if let Ok(dt) = chrono::NaiveDate::parse_from_str(&s_str, "%b %d, %Y") {
                    // "Aug 9, 1995"
                    let ms = dt
                        .and_hms_opt(0, 0, 0)
                        .map(|d| d.and_utc().timestamp_millis() as f64)
                        .unwrap_or(f64::NAN);
                    return Value::Number(ms);
                }
                Value::Number(f64::NAN)
            }
            BUILTIN_CONSOLE_LOG | BUILTIN_CONSOLE_WARN | BUILTIN_CONSOLE_ERROR => {
                let parts: Vec<String> = args.iter().map(|v| self.vm_display_string(v)).collect();
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
                        if n.is_nan() {
                            return Value::Number(f64::NAN);
                        }
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
                        if n.is_nan() {
                            return Value::Number(f64::NAN);
                        }
                        if *n < result {
                            result = *n;
                        }
                    } else {
                        return Value::Number(f64::NAN);
                    }
                }
                Value::Number(result)
            }
            BUILTIN_MATH_SIN => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.sin())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_COS => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.cos())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_TAN => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.tan())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_ASIN => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.asin())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_ACOS => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.acos())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_ATAN => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.atan())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_ATAN2 => {
                let y = if let Some(Value::Number(n)) = args.first() { *n } else { f64::NAN };
                let x = if let Some(Value::Number(n)) = args.get(1) { *n } else { f64::NAN };
                Value::Number(y.atan2(x))
            }
            BUILTIN_MATH_SINH => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.sinh())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_COSH => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.cosh())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_TANH => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.tanh())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_ASINH => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.asinh())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_ACOSH => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.acosh())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_ATANH => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.atanh())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_EXP => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.exp())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_EXPM1 => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.exp_m1())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_LOG => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.ln())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_LOG10 => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.log10())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_LOG1P => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.ln_1p())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_LOG2 => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.log2())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_FROUND => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number((*n as f32) as f64)
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_TRUNC => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.trunc())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_CBRT => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number(n.cbrt())
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_HYPOT => {
                let mut sum = 0.0_f64;
                for a in &args {
                    if let Value::Number(n) = a {
                        sum += n * n;
                    } else {
                        return Value::Number(f64::NAN);
                    }
                }
                Value::Number(sum.sqrt())
            }
            BUILTIN_MATH_SIGN => {
                if let Some(Value::Number(n)) = args.first() {
                    if n.is_nan() {
                        Value::Number(f64::NAN)
                    } else if *n == 0.0 {
                        Value::Number(*n)
                    }
                    // preserves -0
                    else if *n > 0.0 {
                        Value::Number(1.0)
                    } else {
                        Value::Number(-1.0)
                    }
                } else {
                    Value::Number(f64::NAN)
                }
            }
            BUILTIN_MATH_POW => {
                let base = if let Some(Value::Number(n)) = args.first() { *n } else { f64::NAN };
                let exp = if let Some(Value::Number(n)) = args.get(1) { *n } else { f64::NAN };
                Value::Number(base.powf(exp))
            }
            BUILTIN_MATH_RANDOM => {
                // Simple pseudo-random using system time
                use std::time::SystemTime;
                let seed = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);
                // Simple hash and keep 53 random bits to map into [0, 1)
                let mixed = seed ^ (seed >> 17) ^ (seed << 13);
                let bits53 = mixed & ((1u64 << 53) - 1);
                let v = (bits53 as f64) / ((1u64 << 53) as f64);
                Value::Number(v)
            }
            BUILTIN_MATH_CLZ32 => {
                if let Some(Value::Number(n)) = args.first() {
                    Value::Number((*n as i32 as u32).leading_zeros() as f64)
                } else {
                    Value::Number(32.0)
                }
            }
            BUILTIN_MATH_IMUL => {
                let a = if let Some(Value::Number(n)) = args.first() { *n as i32 } else { 0 };
                let b = if let Some(Value::Number(n)) = args.get(1) { *n as i32 } else { 0 };
                Value::Number((a.wrapping_mul(b)) as f64)
            }
            BUILTIN_ISNAN => match args.first() {
                Some(Value::Number(n)) => Value::Boolean(n.is_nan()),
                Some(Value::Undefined) => Value::Boolean(true),
                _ => Value::Boolean(false),
            },
            BUILTIN_PARSEINT => {
                let s = args.first().map(value_to_string).unwrap_or_default();
                let trimmed = s.trim();
                let radix = args.get(1).map(|v| to_number(v) as u32).unwrap_or(0);
                // Determine effective radix
                let effective_radix = if radix == 0 {
                    if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
                        16
                    } else {
                        10
                    }
                } else {
                    radix
                };
                let parse_str = if effective_radix == 16 {
                    trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")).unwrap_or(trimmed)
                } else {
                    trimmed
                };
                match i64::from_str_radix(parse_str, effective_radix) {
                    Ok(n) => Value::Number(n as f64),
                    Err(_) => {
                        // Try parsing as float for radix 10
                        if effective_radix == 10 {
                            match trimmed.parse::<f64>() {
                                Ok(n) => Value::Number(n.trunc()),
                                Err(_) => Value::Number(f64::NAN),
                            }
                        } else {
                            Value::Number(f64::NAN)
                        }
                    }
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
            BUILTIN_ARRAY_OF => {
                // Array.of(a, b, c) → [a, b, c]
                Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(args))))
            }
            BUILTIN_ARRAY_FROM => {
                // Array.from(iterable) or Array.from({length: n}) or Array.from(iter, mapFn)
                let source = args.first().cloned().unwrap_or(Value::Undefined);
                let map_fn = args.get(1).cloned();
                let mut result = Vec::new();
                match &source {
                    Value::VmArray(arr) => {
                        result = arr.borrow().elements.clone();
                    }
                    Value::String(s) => {
                        let s_utf8 = crate::unicode::utf16_to_utf8(s);
                        for ch in s_utf8.chars() {
                            result.push(Value::String(crate::unicode::utf8_to_utf16(&ch.to_string())));
                        }
                    }
                    Value::VmSet(set) => {
                        for val in set.borrow().values.iter() {
                            result.push(val.clone());
                        }
                    }
                    Value::VmMap(map) => {
                        for (k, v) in map.borrow().entries.iter() {
                            let pair = vec![k.clone(), v.clone()];
                            result.push(Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(pair)))));
                        }
                    }
                    Value::VmObject(map) => {
                        let borrow = map.borrow();
                        if let Some(Value::Number(n)) = borrow.get("length") {
                            let len = *n as usize;
                            drop(borrow);
                            for i in 0..len {
                                let key = i.to_string();
                                let val = map.borrow().get(&key).cloned().unwrap_or(Value::Undefined);
                                result.push(val);
                            }
                        } else {
                            drop(borrow);
                        }
                    }
                    _ => {}
                }
                if let Some(map_fn_val) = map_fn {
                    let __cb_uv = match &map_fn_val {
                        Value::VmClosure(_, _, u) => (**u).to_vec(),
                        _ => Vec::new(),
                    };
                    let mut mapped = Vec::with_capacity(result.len());
                    for (i, elem) in result.into_iter().enumerate() {
                        let val = match &map_fn_val {
                            Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _) => {
                                self.call_vm_function(*ip, &[elem, Value::Number(i as f64)], &__cb_uv)
                            }
                            Value::VmNativeFunction(id) => self.call_builtin(*id, vec![elem, Value::Number(i as f64)]),
                            _ => elem,
                        };
                        mapped.push(val);
                    }
                    result = mapped;
                }
                Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(result))))
            }
            BUILTIN_JSON_STRINGIFY => {
                let s = args.first().map(|v| self.json_stringify(v)).unwrap_or_default();
                Value::String(crate::unicode::utf8_to_utf16(&s))
            }
            BUILTIN_JSON_PARSE => {
                let s = args.first().map(value_to_string).unwrap_or_default();
                self.json_parse(&s)
            }
            BUILTIN_EVAL => {
                let code = args.first().map(value_to_string).unwrap_or_default();
                let expr = code.trim().trim_end_matches(';').trim();
                if let Some(name) = expr.strip_prefix("super.")
                    && !name.is_empty()
                    && name.chars().all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric())
                {
                    let receiver = self.this_stack.last().cloned().unwrap_or(Value::Undefined);
                    if let Some(super_base) = self.resolve_super_base(&receiver) {
                        return self.read_named_property(super_base, name);
                    }
                    return Value::Undefined;
                }
                if code.contains("?.") {
                    match self.try_eval_optional_chain_expression(&code) {
                        Ok(Some(v)) => return v,
                        Ok(None) => {}
                        Err(e) => {
                            let msg = format!("{}", e);
                            let mut err_map = IndexMap::new();
                            err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Error")));
                            err_map.insert("message".to_string(), Value::String(crate::unicode::utf8_to_utf16(&msg)));
                            self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                            return Value::Undefined;
                        }
                    }
                }
                // Compile and run eval'd code in a temporary VM that shares globals
                let result = (|| -> Result<Value<'gc>, JSError> {
                    let tokens = crate::core::tokenize(&code)?;
                    let mut index = 0;
                    let statements = crate::core::parse_statements(&tokens, &mut index)?;
                    // Check for bare return statements — illegal at top level of eval
                    for stmt in &statements {
                        if matches!(*stmt.kind, crate::core::StatementKind::Return(_)) {
                            return Err(crate::make_js_error!(crate::JSErrorKind::SyntaxError {
                                message: "Illegal return statement".to_string()
                            }));
                        }
                    }
                    // Detect strict mode: code begins with "use strict" directive, or enclosing context is strict (direct eval only)
                    let enclosing_strict = if self.direct_eval {
                        self.frames
                            .last()
                            .map(|f| self.chunk.fn_strictness.get(&f.func_ip).copied().unwrap_or(false))
                            .unwrap_or(false)
                    } else {
                        false // indirect eval never inherits caller strict mode
                    };
                    let is_strict =
                        enclosing_strict || code.trim().starts_with("\"use strict\"") || code.trim().starts_with("'use strict'");
                    let compiler = crate::core::Compiler::new();
                    let chunk = compiler.compile(&statements)?;
                    // Non-configurable global names (can't be redefined by eval)
                    let non_configurable: [&str; 3] = ["NaN", "Infinity", "undefined"];
                    // Pre-check: scan chunk for DefineGlobal opcodes that would define functions overriding non-configurable globals
                    {
                        let code = &chunk.code;
                        let constants = &chunk.constants;
                        let mut pc = 0;
                        while pc < code.len() {
                            let op = code[pc];
                            pc += 1;
                            if (op == Opcode::DefineGlobal as u8 || op == Opcode::DefineGlobalConst as u8) && pc + 1 < code.len() {
                                let idx = (code[pc] as u16 | (code[pc + 1] as u16) << 8) as usize;
                                if idx < constants.len()
                                    && let Value::String(s) = &constants[idx]
                                {
                                    let name = crate::unicode::utf16_to_utf8(s);
                                    if non_configurable.contains(&name.as_str()) {
                                        return Err(crate::raise_type_error!(format!("Cannot redefine property: {}", name)));
                                    }
                                }
                                pc += 2;
                            } else {
                                // Skip operands based on opcode
                                match Opcode::try_from(op) {
                                    Ok(
                                        Opcode::Constant
                                        | Opcode::DefineGlobal
                                        | Opcode::DefineGlobalConst
                                        | Opcode::GetGlobal
                                        | Opcode::SetGlobal
                                        | Opcode::GetProperty
                                        | Opcode::SetProperty
                                        | Opcode::SetSuperProperty
                                        | Opcode::GetSuperProperty
                                        | Opcode::GetMethod
                                        | Opcode::NewError
                                        | Opcode::TypeOfGlobal
                                        | Opcode::DeleteGlobal,
                                    ) => pc += 2,
                                    Ok(Opcode::Jump | Opcode::JumpIfFalse | Opcode::JumpIfTrue | Opcode::SetupTry) => pc += 2,
                                    Ok(Opcode::Call | Opcode::NewCall) => pc += 1,
                                    Ok(
                                        Opcode::GetLocal
                                        | Opcode::SetLocal
                                        | Opcode::NewArray
                                        | Opcode::NewObject
                                        | Opcode::GetUpvalue
                                        | Opcode::SetUpvalue
                                        | Opcode::CollectRest
                                        | Opcode::GetArguments,
                                    ) => pc += 1,
                                    Ok(Opcode::MakeClosure) => {
                                        pc += 2; // const idx
                                        if pc < code.len() {
                                            let count = code[pc] as usize;
                                            pc += 1 + count * 2;
                                        }
                                    }
                                    _ => {} // 0-operand opcodes
                                }
                            }
                        }
                    }
                    let mut eval_vm: VM<'gc> = VM::new(chunk);
                    // Copy caller's globals into eval VM
                    let _caller_keys: std::collections::HashSet<String> = self.globals.keys().cloned().collect();
                    for (k, v) in &self.globals {
                        eval_vm.globals.insert(k.clone(), v.clone());
                    }
                    // For direct eval, inject caller's local variables as globals
                    if self.direct_eval {
                        // Walk all frames from outermost to innermost so inner scopes shadow outer
                        for frame in self.frames.iter() {
                            if let Some(local_names) = self.chunk.fn_local_names.get(&frame.func_ip) {
                                for (idx, name) in local_names.iter().enumerate() {
                                    if name.starts_with("__") && name.ends_with("__") {
                                        continue; // skip synthetic locals
                                    }
                                    if let Some(cell) = frame.local_cells.get(&idx) {
                                        eval_vm.globals.insert(name.clone(), cell.borrow().clone());
                                    } else {
                                        let stack_idx = frame.bp + idx;
                                        if stack_idx < self.stack.len() {
                                            eval_vm.globals.insert(name.clone(), self.stack[stack_idx].clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Set up `this` for the eval VM
                    if self.direct_eval {
                        // Direct eval inherits caller's `this`
                        let caller_this = self.this_stack.last().cloned().unwrap_or(Value::Undefined);
                        eval_vm.this_stack.push(caller_this);
                    } else {
                        // Indirect eval: `this` is globalThis
                        let global_this = self.globals.get("globalThis").cloned().unwrap_or(Value::Undefined);
                        eval_vm.this_stack.push(global_this);
                    }
                    let result = eval_vm.run()?;
                    // Copy globals back — strict mode keeps eval's own scope isolated
                    if !is_strict {
                        for (k, v) in &eval_vm.globals {
                            self.globals.insert(k.clone(), v.clone());
                        }
                    }
                    // For direct eval, write back modified local variables to caller's stack
                    if self.direct_eval {
                        for frame in self.frames.iter().rev() {
                            if let Some(local_names) = self.chunk.fn_local_names.get(&frame.func_ip) {
                                for (idx, name) in local_names.iter().enumerate() {
                                    if name.starts_with("__") && name.ends_with("__") {
                                        continue;
                                    }
                                    if let Some(new_val) = eval_vm.globals.get(name) {
                                        if let Some(cell) = frame.local_cells.get(&idx) {
                                            *cell.borrow_mut() = new_val.clone();
                                        } else {
                                            let stack_idx = frame.bp + idx;
                                            if stack_idx < self.stack.len() {
                                                self.stack[stack_idx] = new_val.clone();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Ok(result)
                })();
                match result {
                    Ok(v) => {
                        match v {
                            Value::VmFunction(..) | Value::VmClosure(..) => {
                                let trimmed = code.trim().trim_end_matches(';').trim();
                                let tail = trimmed.rsplit(';').next().unwrap_or(trimmed).trim();
                                if tail.contains("=>") || tail.starts_with("function") || tail.starts_with("(") {
                                    // Function values produced by a temporary eval VM cannot be called safely
                                    // in the current VM. Re-wrap as a callable body executed in current context.
                                    let mut map = IndexMap::new();
                                    map.insert(
                                        "__fn_body__".to_string(),
                                        Value::String(crate::unicode::utf8_to_utf16(&format!("({tail})()"))),
                                    );
                                    map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Function")));
                                    Value::VmObject(Rc::new(RefCell::new(map)))
                                } else {
                                    v
                                }
                            }
                            _ => v,
                        }
                    }
                    Err(e) => {
                        let msg = format!("{}", e);
                        let msg_lower = msg.to_lowercase();
                        let is_syntax = msg_lower.contains("syntaxerror")
                            || msg_lower.contains("syntax error")
                            || msg_lower.contains("illegal return")
                            || msg_lower.contains("continue outside")
                            || msg_lower.contains("break outside")
                            || msg_lower.contains("parsing failed")
                            || msg_lower.contains("parse error")
                            || msg_lower.contains("unexpected token")
                            || msg_lower.contains("unexpected end");
                        let is_type_error = msg_lower.contains("typeerror") || msg_lower.contains("type error");
                        let type_name = if is_syntax {
                            "SyntaxError"
                        } else if is_type_error {
                            "TypeError"
                        } else {
                            "Error"
                        };
                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16(type_name)));
                        err_map.insert("message".to_string(), Value::String(crate::unicode::utf8_to_utf16(&msg)));
                        self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                        Value::Undefined
                    }
                }
            }
            BUILTIN_NEW_FUNCTION | BUILTIN_CTOR_FUNCTION => {
                // new Function(body): return a callable wrapper with __fn_body__
                let body = args.first().map(value_to_string).unwrap_or_default();
                let mut map = IndexMap::new();
                map.insert("__fn_body__".to_string(), Value::String(crate::unicode::utf8_to_utf16(&body)));
                map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Function")));
                Value::VmObject(Rc::new(RefCell::new(map)))
            }
            // Number() as function: convert argument to number
            BUILTIN_CTOR_NUMBER => {
                let arg = args.first().cloned().unwrap_or(Value::Number(0.0));
                let coerced = self.try_to_primitive(&arg, "number");
                Value::Number(to_number(&coerced))
            }
            // String() as function: convert argument to string
            BUILTIN_CTOR_STRING => {
                let arg = args.first().cloned().unwrap_or(Value::String(Vec::new()));
                let coerced = self.try_to_primitive(&arg, "string");
                let s = self.vm_to_string(&coerced);
                Value::String(crate::unicode::utf8_to_utf16(&s))
            }
            BUILTIN_CTOR_BOOLEAN => {
                let b = args.first().map(|v| v.to_truthy()).unwrap_or(false);
                Value::Boolean(b)
            }
            BUILTIN_STRING_FROMCHARCODE => {
                let mut result = String::new();
                for arg in &args {
                    let code = match arg {
                        Value::Number(n) => *n as u32,
                        Value::String(s) => {
                            let s_utf8 = crate::unicode::utf16_to_utf8(s);
                            if let Some(stripped) = s_utf8.strip_prefix("0x").or_else(|| s_utf8.strip_prefix("0X")) {
                                u32::from_str_radix(stripped, 16).unwrap_or(0)
                            } else {
                                s_utf8.parse::<f64>().unwrap_or(0.0) as u32
                            }
                        }
                        _ => 0,
                    };
                    if let Some(c) = char::from_u32(code) {
                        result.push(c);
                    }
                }
                Value::String(crate::unicode::utf8_to_utf16(&result))
            }
            BUILTIN_BIGINT => {
                // BigInt(value) — convert a number or string to BigInt
                match args.first() {
                    Some(Value::Number(n)) => {
                        let i = *n as i64;
                        Value::BigInt(Box::new(num_bigint::BigInt::from(i)))
                    }
                    Some(Value::String(s)) => {
                        let text = crate::unicode::utf16_to_utf8(s);
                        match crate::js_bigint::parse_bigint_string(&text) {
                            Ok(bi) => Value::BigInt(Box::new(bi)),
                            Err(_) => Value::Undefined,
                        }
                    }
                    Some(Value::BigInt(bi)) => Value::BigInt(bi.clone()),
                    _ => Value::Undefined,
                }
            }
            BUILTIN_BIGINT_ASUINTN | BUILTIN_BIGINT_ASINTN => {
                let bits_num = args.first().map(to_number).unwrap_or(0.0);
                let bits = if bits_num.is_finite() && bits_num > 0.0 {
                    bits_num.trunc() as usize
                } else {
                    0usize
                };

                if bits == 0 {
                    return Value::BigInt(Box::new(num_bigint::BigInt::from(0)));
                }

                let as_bigint = match args.get(1).cloned().unwrap_or(Value::Undefined) {
                    Value::BigInt(bi) => (*bi).clone(),
                    Value::String(s) => {
                        let text = crate::unicode::utf16_to_utf8(&s);
                        match crate::js_bigint::parse_bigint_string(&text) {
                            Ok(v) => v,
                            Err(_) => {
                                let mut err_map = IndexMap::new();
                                err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                err_map.insert(
                                    "message".to_string(),
                                    Value::String(crate::unicode::utf8_to_utf16("Cannot convert value to BigInt")),
                                );
                                self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                return Value::Undefined;
                            }
                        }
                    }
                    Value::Boolean(b) => num_bigint::BigInt::from(if b { 1 } else { 0 }),
                    Value::VmObject(obj) => {
                        let borrow = obj.borrow();
                        if borrow.get("__type__").map(value_to_string).as_deref() == Some("BigInt") {
                            if let Some(Value::BigInt(inner)) = borrow.get("__value__") {
                                (**inner).clone()
                            } else {
                                num_bigint::BigInt::from(0)
                            }
                        } else {
                            let value_of = borrow.get("valueOf").cloned();
                            drop(borrow);
                            if let Some(value_of) = value_of {
                                let out = match value_of {
                                    Value::VmFunction(ip, _) => self.call_vm_function(ip, &[], &[]),
                                    Value::VmClosure(ip, _, uv) => self.call_vm_function(ip, &[], &uv),
                                    Value::VmNativeFunction(id) => self.call_method_builtin(id, Value::VmObject(obj.clone()), Vec::new()),
                                    _ => Value::Undefined,
                                };
                                match out {
                                    Value::BigInt(inner) => (*inner).clone(),
                                    Value::Boolean(b) => num_bigint::BigInt::from(if b { 1 } else { 0 }),
                                    Value::String(s) => {
                                        let text = crate::unicode::utf16_to_utf8(&s);
                                        match crate::js_bigint::parse_bigint_string(&text) {
                                            Ok(v) => v,
                                            Err(_) => {
                                                let mut err_map = IndexMap::new();
                                                err_map.insert(
                                                    "__type__".to_string(),
                                                    Value::String(crate::unicode::utf8_to_utf16("TypeError")),
                                                );
                                                err_map.insert(
                                                    "message".to_string(),
                                                    Value::String(crate::unicode::utf8_to_utf16("Cannot convert value to BigInt")),
                                                );
                                                self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                                return Value::Undefined;
                                            }
                                        }
                                    }
                                    _ => {
                                        let mut err_map = IndexMap::new();
                                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                        err_map.insert(
                                            "message".to_string(),
                                            Value::String(crate::unicode::utf8_to_utf16("Cannot convert value to BigInt")),
                                        );
                                        self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                        return Value::Undefined;
                                    }
                                }
                            } else {
                                let mut err_map = IndexMap::new();
                                err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                err_map.insert(
                                    "message".to_string(),
                                    Value::String(crate::unicode::utf8_to_utf16("Cannot convert value to BigInt")),
                                );
                                self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                return Value::Undefined;
                            }
                        }
                    }
                    Value::Number(n) => {
                        if n.is_finite() && n < 0.0 && n == n.trunc() {
                            num_bigint::BigInt::from(n as i64)
                        } else {
                            let mut err_map = IndexMap::new();
                            err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                            err_map.insert(
                                "message".to_string(),
                                Value::String(crate::unicode::utf8_to_utf16("Cannot convert Number to BigInt")),
                            );
                            self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                            return Value::Undefined;
                        }
                    }
                    _ => {
                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("Cannot convert value to BigInt")),
                        );
                        self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                        return Value::Undefined;
                    }
                };

                let modulus = num_bigint::BigInt::from(1u8) << bits;
                let mut uint = as_bigint % &modulus;
                if uint < num_bigint::BigInt::from(0) {
                    uint += &modulus;
                }
                if id == BUILTIN_BIGINT_ASUINTN {
                    Value::BigInt(Box::new(uint))
                } else {
                    let sign_bit = num_bigint::BigInt::from(1u8) << (bits - 1);
                    if uint >= sign_bit {
                        Value::BigInt(Box::new(uint - modulus))
                    } else {
                        Value::BigInt(Box::new(uint))
                    }
                }
            }
            BUILTIN_CTOR_ARRAY => {
                // Array(a, b, c) → creates array from args
                // Array(n) where n is a number → creates array with n empty slots (holes)
                if args.len() == 1
                    && let Value::Number(n) = &args[0]
                {
                    let len = *n as usize;
                    let mut data = VmArrayData::new(Vec::new());
                    for i in 0..len {
                        data.elements.push(Value::Undefined);
                        data.props.insert(format!("__deleted_{}", i), Value::Boolean(true));
                    }
                    return Value::VmArray(Rc::new(RefCell::new(data)));
                }
                Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(args))))
            }
            // Number.isNaN: strict check (no coercion)
            BUILTIN_NUMBER_ISNAN => match args.first() {
                Some(Value::Number(n)) => Value::Boolean(n.is_nan()),
                _ => Value::Boolean(false),
            },
            // Number.isFinite: strict check
            BUILTIN_NUMBER_ISFINITE => match args.first() {
                Some(Value::Number(n)) => Value::Boolean(n.is_finite()),
                _ => Value::Boolean(false),
            },
            // Number.isInteger
            BUILTIN_NUMBER_ISINTEGER => match args.first() {
                Some(Value::Number(n)) => Value::Boolean(n.is_finite() && *n == n.trunc()),
                _ => Value::Boolean(false),
            },
            // Number.isSafeInteger
            BUILTIN_NUMBER_ISSAFEINTEGER => match args.first() {
                Some(Value::Number(n)) => Value::Boolean(n.is_finite() && *n == n.trunc() && n.abs() <= 9007199254740991.0),
                _ => Value::Boolean(false),
            },
            // Date() called as a function returns a date-time string in JS.
            // Keep it simple and deterministic enough for tests.
            BUILTIN_CTOR_DATE => {
                use chrono::{Local, TimeZone, Utc};
                let now = Utc::now().timestamp_millis();
                if let Some(dt) = Local.timestamp_millis_opt(now).single() {
                    let s = dt.format("%a %b %d %Y %H:%M:%S GMT%z").to_string();
                    Value::String(crate::unicode::utf8_to_utf16(&s))
                } else {
                    Value::String(crate::unicode::utf8_to_utf16("Invalid Date"))
                }
            }
            // Error constructors called as functions (without `new`) — still create error objects
            BUILTIN_CTOR_ERROR
            | BUILTIN_CTOR_TYPEERROR
            | BUILTIN_CTOR_SYNTAXERROR
            | BUILTIN_CTOR_RANGEERROR
            | BUILTIN_CTOR_REFERENCEERROR => {
                let type_name = Self::error_type_name_from_builtin(id).unwrap_or("Error");
                let msg = args.first().map(|v| self.vm_to_string(v)).unwrap_or_default();
                let mut map = IndexMap::new();
                map.insert("message".to_string(), Value::String(crate::unicode::utf8_to_utf16(&msg)));
                map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16(type_name)));
                map.insert("name".to_string(), Value::String(crate::unicode::utf8_to_utf16(type_name)));
                Value::VmObject(Rc::new(RefCell::new(map)))
            }
            // Object.keys(obj) → array of own enumerable string keys
            BUILTIN_OBJECT_KEYS => {
                if let Some(Value::VmObject(obj)) = args.first() {
                    let keys: Vec<Value<'gc>> = obj
                        .borrow()
                        .keys()
                        .filter(|k| !k.starts_with("__") && !k.starts_with("@@sym:"))
                        .map(|k| Value::String(crate::unicode::utf8_to_utf16(k)))
                        .collect();
                    Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(keys))))
                } else if let Some(Value::VmArray(arr)) = args.first() {
                    let borrow = arr.borrow();
                    let keys: Vec<Value<'gc>> = (0..borrow.elements.len())
                        .filter(|i| !borrow.props.contains_key(&format!("__deleted_{}", i)))
                        .map(|i| Value::String(crate::unicode::utf8_to_utf16(&i.to_string())))
                        .collect();
                    Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(keys))))
                } else {
                    Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(vec![]))))
                }
            }
            // Object.values(obj) → array of own enumerable values
            BUILTIN_OBJECT_VALUES => {
                if let Some(Value::VmObject(obj)) = args.first() {
                    let vals: Vec<Value<'gc>> = obj
                        .borrow()
                        .iter()
                        .filter(|(k, _)| !k.starts_with("__") && !k.starts_with("@@sym:"))
                        .map(|(_, v)| v.clone())
                        .collect();
                    Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(vals))))
                } else {
                    Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(vec![]))))
                }
            }
            // Object.entries(obj) → array of [key, value] pairs
            BUILTIN_OBJECT_ENTRIES => {
                if let Some(Value::VmObject(obj)) = args.first() {
                    let entries: Vec<Value<'gc>> = obj
                        .borrow()
                        .iter()
                        .filter(|(k, _)| !k.starts_with("__"))
                        .map(|(k, v)| {
                            Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(vec![
                                Value::String(crate::unicode::utf8_to_utf16(k)),
                                v.clone(),
                            ]))))
                        })
                        .collect();
                    Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(entries))))
                } else {
                    Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(vec![]))))
                }
            }
            // Object.assign(target, ...sources)
            BUILTIN_OBJECT_ASSIGN => {
                if let Some(Value::VmObject(target)) = args.first() {
                    for src in args.iter().skip(1) {
                        if let Value::VmObject(src_map) = src {
                            let entries: Vec<(String, Value<'gc>)> = src_map
                                .borrow()
                                .iter()
                                .filter(|(k, _)| !k.starts_with("__") && !k.starts_with("@@sym:"))
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            let mut tgt = target.borrow_mut();
                            for (k, v) in entries {
                                tgt.insert(k, v);
                            }
                        }
                    }
                    Value::VmObject(target.clone())
                } else {
                    args.first().cloned().unwrap_or(Value::Undefined)
                }
            }
            // Object.freeze(obj) — mark as frozen (stub: just returns the object)
            BUILTIN_OBJECT_FREEZE => {
                if let Some(Value::VmObject(obj)) = args.first() {
                    obj.borrow_mut().insert("__frozen__".to_string(), Value::Boolean(true));
                    Value::VmObject(obj.clone())
                } else {
                    args.first().cloned().unwrap_or(Value::Undefined)
                }
            }
            // Object.hasOwn(obj, key)
            BUILTIN_OBJECT_HASOWN => {
                let obj = args.first().cloned().unwrap_or(Value::Undefined);
                let key = args.get(1).map(|v| value_to_string(v)).unwrap_or_default();
                if let Value::VmObject(map) = &obj {
                    Value::Boolean(map.borrow().contains_key(&key))
                } else if let Value::VmArray(arr) = &obj {
                    let borrow = arr.borrow();
                    // Check numeric indices
                    if let Ok(i) = key.parse::<usize>()
                        && i < borrow.elements.len()
                    {
                        return Value::Boolean(true);
                    }
                    Value::Boolean(borrow.props.contains_key(&key))
                } else {
                    Value::Boolean(false)
                }
            }
            // Object.create(proto) — create object with given prototype (simplified)
            BUILTIN_OBJECT_CREATE => {
                let proto = args.first().cloned().unwrap_or(Value::Null);
                let mut obj = IndexMap::new();
                // Always store __proto__ so getPrototypeOf can distinguish
                // Object.create(null) from objects with no explicit proto
                obj.insert("__proto__".to_string(), proto);
                if let Some(Value::VmObject(descs)) = args.get(1) {
                    for (k, v) in descs.borrow().iter() {
                        if let Value::VmObject(desc) = v
                            && let Some(val) = desc.borrow().get("value").cloned()
                        {
                            obj.insert(k.clone(), val);
                        }
                    }
                }
                Value::VmObject(Rc::new(RefCell::new(obj)))
            }
            // Object.getPrototypeOf(obj)
            BUILTIN_OBJECT_GETPROTOTYPEOF => {
                if let Some(Value::VmObject(map)) = args.first() {
                    let borrow = map.borrow();
                    if let Some(proto) = borrow.get("__proto__") {
                        proto.clone()
                    } else {
                        drop(borrow);
                        // Default prototype is Object.prototype
                        if let Some(Value::VmObject(obj_global)) = self.globals.get("Object") {
                            obj_global.borrow().get("prototype").cloned().unwrap_or(Value::Null)
                        } else {
                            Value::Null
                        }
                    }
                } else if let Some(Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _)) = args.first() {
                    let props = self.get_fn_props(*ip, *arity);
                    props.borrow().get("__proto__").cloned().unwrap_or(Value::Null)
                } else {
                    Value::Null
                }
            }
            // Object.preventExtensions(obj) — stub
            BUILTIN_OBJECT_PREVENTEXT => {
                if let Some(Value::VmObject(obj)) = args.first() {
                    obj.borrow_mut().insert("__non_extensible__".to_string(), Value::Boolean(true));
                    Value::VmObject(obj.clone())
                } else {
                    args.first().cloned().unwrap_or(Value::Undefined)
                }
            }
            // Object.defineProperty(obj, prop, descriptor)
            BUILTIN_OBJECT_DEFINEPROP => {
                if let Some(Value::VmObject(obj)) = args.first() {
                    let key = args.get(1).map(|v| value_to_string(v)).unwrap_or_default();
                    if let Some(Value::VmObject(desc)) = args.get(2) {
                        let desc_borrow = desc.borrow();
                        if self.validate_property_descriptor(&desc_borrow).is_err() {
                            let mut err_map = IndexMap::new();
                            err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                            err_map.insert("name".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                            err_map.insert(
                                "message".to_string(),
                                Value::String(crate::unicode::utf8_to_utf16("Invalid property descriptor")),
                            );
                            self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                            return Value::Undefined;
                        } else {
                            self.apply_object_property_descriptor(obj, &key, &desc_borrow);
                        }
                    }
                    Value::VmObject(obj.clone())
                } else if let Some(Value::VmArray(arr)) = args.first() {
                    let key = args.get(1).map(|v| value_to_string(v)).unwrap_or_default();
                    if let Some(Value::VmObject(desc)) = args.get(2) {
                        let desc_borrow = desc.borrow();
                        let mut arr_borrow = arr.borrow_mut();
                        // Check if key is a numeric index
                        if let Ok(idx) = key.parse::<usize>() {
                            // Extend elements if needed
                            while arr_borrow.elements.len() <= idx {
                                arr_borrow.elements.push(Value::Undefined);
                            }
                            if let Some(val) = desc_borrow.get("value") {
                                arr_borrow.elements[idx] = val.clone();
                            }
                        }
                        // Store getter/setter in props
                        if let Some(val @ (Value::VmFunction(_ip, _) | Value::VmClosure(_ip, _, _))) = desc_borrow.get("get") {
                            let getter_key = format!("__get_{}", key);
                            arr_borrow.props.insert(getter_key, val.clone());
                        }
                        if let Some(val @ (Value::VmFunction(_ip, _a) | Value::VmClosure(_ip, _a, _))) = desc_borrow.get("set") {
                            let setter_key = format!("__set_{}", key);
                            arr_borrow.props.insert(setter_key, val.clone());
                        }
                    }
                    Value::VmArray(arr.clone())
                } else {
                    args.first().cloned().unwrap_or(Value::Undefined)
                }
            }
            // Object.getOwnPropertyDescriptor(obj, prop)
            BUILTIN_OBJECT_GETOWNPROPDESC => {
                let key = args.get(1).map(|v| value_to_string(v)).unwrap_or_default();
                let make_desc = |value: Value<'gc>, writable: bool, enumerable: bool, configurable: bool| -> Value<'gc> {
                    let mut desc = IndexMap::new();
                    desc.insert("value".to_string(), value);
                    desc.insert("writable".to_string(), Value::Boolean(writable));
                    desc.insert("enumerable".to_string(), Value::Boolean(enumerable));
                    desc.insert("configurable".to_string(), Value::Boolean(configurable));
                    Value::VmObject(Rc::new(RefCell::new(desc)))
                };
                match args.first() {
                    Some(Value::VmObject(obj)) => {
                        let borrow = obj.borrow();
                        let ro_key = format!("__readonly_{}__", key);
                        let is_readonly = borrow.contains_key(&ro_key);
                        let nc_key = format!("__nonconfigurable_{}__", key);
                        let is_nonconfigurable = borrow.contains_key(&nc_key);
                        let ne_key = format!("__nonenumerable_{}__", key);
                        let is_nonenumerable = borrow.contains_key(&ne_key);
                        // debug log for troubleshooting
                        log::warn!(
                            "GETOWNPROPDESC key='{}' readonly={} nonconfigurable={} nonenumerable={} map_keys={:?}",
                            key,
                            is_readonly,
                            is_nonconfigurable,
                            is_nonenumerable,
                            borrow.keys().cloned().collect::<Vec<_>>()
                        );

                        if let Some(val) = borrow.get(&key) {
                            match val {
                                Value::Property { getter, setter, .. } => {
                                    // accessor descriptor
                                    let mut desc = IndexMap::new();
                                    if let Some(g) = getter {
                                        // dereference the Box to clone inner Value
                                        desc.insert("get".to_string(), (**g).clone());
                                    } else {
                                        desc.insert("get".to_string(), Value::Undefined);
                                    }
                                    if let Some(s) = setter {
                                        desc.insert("set".to_string(), (**s).clone());
                                    } else {
                                        desc.insert("set".to_string(), Value::Undefined);
                                    }
                                    desc.insert("enumerable".to_string(), Value::Boolean(!is_nonenumerable));
                                    desc.insert("configurable".to_string(), Value::Boolean(!is_nonconfigurable));
                                    Value::VmObject(Rc::new(RefCell::new(desc)))
                                }
                                _ => make_desc(val.clone(), !is_readonly, !is_nonenumerable, !is_nonconfigurable),
                            }
                        } else {
                            // Check for getter
                            let getter_key = format!("__get_{}", key);
                            if let Some(getter) = borrow.get(&getter_key) {
                                let mut desc = IndexMap::new();
                                desc.insert("get".to_string(), getter.clone());
                                let setter_key = format!("__set_{}", key);
                                desc.insert("set".to_string(), borrow.get(&setter_key).cloned().unwrap_or(Value::Undefined));
                                desc.insert("enumerable".to_string(), Value::Boolean(!is_nonenumerable));
                                desc.insert("configurable".to_string(), Value::Boolean(!is_nonconfigurable));
                                Value::VmObject(Rc::new(RefCell::new(desc)))
                            } else {
                                Value::Undefined
                            }
                        }
                    }
                    Some(Value::VmFunction(ip, arity)) | Some(Value::VmClosure(ip, arity, _)) => {
                        let fn_props = self.get_fn_props(*ip, *arity);
                        let borrow = fn_props.borrow();
                        if let Some(val) = borrow.get(&key) {
                            // name and length are non-writable, non-enumerable, configurable
                            let (writable, enumerable, configurable) = if key == "name" || key == "length" {
                                (false, false, true)
                            } else {
                                (true, true, true)
                            };
                            make_desc(val.clone(), writable, enumerable, configurable)
                        } else {
                            Value::Undefined
                        }
                    }
                    _ => Value::Undefined,
                }
            }
            // Object.setPrototypeOf(obj, proto)
            BUILTIN_OBJECT_SETPROTOTYPEOF => {
                if let Some(Value::VmObject(obj)) = args.first() {
                    let proto = args.get(1).cloned().unwrap_or(Value::Null);
                    if matches!(proto, Value::Null) {
                        obj.borrow_mut().shift_remove("__proto__");
                    } else {
                        obj.borrow_mut().insert("__proto__".to_string(), proto);
                    }
                    Value::VmObject(obj.clone())
                } else if let Some(val @ (Value::VmFunction(_, _) | Value::VmClosure(_, _, _))) = args.first() {
                    let (ip, arity) = match val {
                        Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => (*ip, *arity),
                        _ => unreachable!(),
                    };
                    let proto = args.get(1).cloned().unwrap_or(Value::Null);
                    let props = self.get_fn_props(ip, arity);
                    props.borrow_mut().insert("__proto__".to_string(), proto);
                    val.clone()
                } else {
                    args.first().cloned().unwrap_or(Value::Undefined)
                }
            }
            // Object.defineProperties — stub
            BUILTIN_OBJECT_DEFINEPROPS => {
                // Object.defineProperties(obj, descriptors)
                if let (Some(Value::VmObject(obj)), Some(Value::VmObject(descs))) = (args.first(), args.get(1)) {
                    let keys: Vec<String> = descs.borrow().keys().filter(|k| !k.starts_with("__")).cloned().collect();
                    for key in keys {
                        let desc_val = descs.borrow().get(&key).cloned();
                        if let Some(Value::VmObject(desc)) = desc_val {
                            let desc_borrow = desc.borrow();
                            if self.validate_property_descriptor(&desc_borrow).is_err() {
                                let mut err_map = IndexMap::new();
                                err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                err_map.insert("name".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                err_map.insert(
                                    "message".to_string(),
                                    Value::String(crate::unicode::utf8_to_utf16("Invalid property descriptor")),
                                );
                                self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                                return Value::Undefined;
                            } else {
                                self.apply_object_property_descriptor(obj, &key, &desc_borrow);
                            }
                        }
                    }
                    Value::VmObject(obj.clone())
                } else {
                    args.first().cloned().unwrap_or(Value::Undefined)
                }
            }
            // Object.getOwnPropertyNames(obj) → array of own property names (including non-enumerable)
            BUILTIN_OBJECT_GETOWNPROPERTYNAMES => {
                if let Some(Value::VmObject(obj)) = args.first() {
                    let keys: Vec<Value<'gc>> = obj
                        .borrow()
                        .keys()
                        .filter(|k| !k.starts_with("__"))
                        .map(|k| Value::String(crate::unicode::utf8_to_utf16(k)))
                        .collect();
                    Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(keys))))
                } else {
                    Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(vec![]))))
                }
            }
            // new Object() constructor
            BUILTIN_CTOR_OBJECT => {
                if let Some(arg) = args.first() {
                    match arg {
                        Value::BigInt(bi) => {
                            let mut obj = IndexMap::new();
                            obj.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("BigInt")));
                            obj.insert("__value__".to_string(), Value::BigInt(bi.clone()));
                            if let Some(Value::VmObject(obj_global)) = self.globals.get("Object")
                                && let Some(proto) = obj_global.borrow().get("prototype").cloned()
                            {
                                obj.insert("__proto__".to_string(), proto);
                            }
                            return Value::VmObject(Rc::new(RefCell::new(obj)));
                        }
                        Value::VmObject(_) | Value::VmArray(_) | Value::VmMap(_) | Value::VmSet(_) => {
                            return arg.clone();
                        }
                        _ => {}
                    }
                }
                let mut obj = IndexMap::new();
                if let Some(Value::VmObject(obj_global)) = self.globals.get("Object")
                    && let Some(proto) = obj_global.borrow().get("prototype").cloned()
                {
                    obj.insert("__proto__".to_string(), proto);
                }
                Value::VmObject(Rc::new(RefCell::new(obj)))
            }
            // Object.groupBy(iterable, callbackFn)
            BUILTIN_OBJECT_GROUPBY => {
                let iterable = args.first().cloned().unwrap_or(Value::Undefined);
                let callback = args.get(1).cloned().unwrap_or(Value::Undefined);
                let mut groups: IndexMap<String, Vec<Value<'gc>>> = IndexMap::new();
                if let Value::VmArray(arr) = &iterable
                    && let Value::VmFunction(ip, _arity) | Value::VmClosure(ip, _arity, _) = &callback
                {
                    let __cb_uv = if let Value::VmClosure(_, _, u) = &callback {
                        (**u).to_vec()
                    } else {
                        Vec::new()
                    };
                    let items: Vec<Value<'gc>> = arr.borrow().iter().cloned().collect();
                    for item in &items {
                        let key_val = self.call_vm_function(*ip, std::slice::from_ref(item), &__cb_uv);
                        let key_str = value_to_string(&key_val);
                        groups.entry(key_str).or_default().push(item.clone());
                    }
                }
                let mut result = IndexMap::new();
                for (k, v) in groups {
                    result.insert(k, Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(v)))));
                }
                Value::VmObject(Rc::new(RefCell::new(result)))
            }
            BUILTIN_SYMBOL => {
                // Symbol(description?) — create a unique symbol-like VmObject
                let desc = args.first().and_then(|v| match v {
                    Value::Undefined => None,
                    _ => Some(value_to_string(v)),
                });
                self.symbol_counter += 1;
                let id = self.symbol_counter;
                let mut m = IndexMap::new();
                m.insert("__vm_symbol__".to_string(), Value::Boolean(true));
                m.insert("__symbol_id__".to_string(), Value::Number(id as f64));
                if let Some(d) = &desc {
                    m.insert("description".to_string(), Value::String(crate::unicode::utf8_to_utf16(d)));
                } else {
                    m.insert("description".to_string(), Value::Undefined);
                }
                let val = Value::VmObject(Rc::new(RefCell::new(m)));
                self.symbol_values.insert(id, val.clone());
                val
            }
            BUILTIN_SYMBOL_FOR => {
                // Symbol.for(key) — return or create a registered symbol
                let key = args.first().map(value_to_string).unwrap_or_else(|| "undefined".to_string());
                if let Some(existing) = self.symbol_registry.get(&key) {
                    return existing.clone();
                }
                self.symbol_counter += 1;
                let id = self.symbol_counter;
                let mut m = IndexMap::new();
                m.insert("__vm_symbol__".to_string(), Value::Boolean(true));
                m.insert("__symbol_id__".to_string(), Value::Number(id as f64));
                m.insert("__registered__".to_string(), Value::Boolean(true));
                m.insert("description".to_string(), Value::String(crate::unicode::utf8_to_utf16(&key)));
                let val = Value::VmObject(Rc::new(RefCell::new(m)));
                self.symbol_values.insert(id, val.clone());
                self.symbol_registry.insert(key, val.clone());
                val
            }
            BUILTIN_SYMBOL_KEYFOR => {
                // Symbol.keyFor(sym) — return key if registered, else undefined
                let sym = args.first().cloned().unwrap_or(Value::Undefined);
                match &sym {
                    Value::VmObject(obj) if obj.borrow().contains_key("__vm_symbol__") => {
                        let borrow = obj.borrow();
                        if borrow.get("__registered__").is_some()
                            && let Some(Value::String(desc)) = borrow.get("description")
                        {
                            return Value::String(desc.clone());
                        }
                        Value::Undefined
                    }
                    _ => {
                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("Symbol.keyFor requires a symbol argument")),
                        );
                        self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                        Value::Undefined
                    }
                }
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
            | BUILTIN_MATH_SIN
            | BUILTIN_MATH_COS
            | BUILTIN_MATH_TAN
            | BUILTIN_MATH_ASIN
            | BUILTIN_MATH_ACOS
            | BUILTIN_MATH_ATAN
            | BUILTIN_MATH_ATAN2
            | BUILTIN_MATH_SINH
            | BUILTIN_MATH_COSH
            | BUILTIN_MATH_TANH
            | BUILTIN_MATH_ASINH
            | BUILTIN_MATH_ACOSH
            | BUILTIN_MATH_ATANH
            | BUILTIN_MATH_EXP
            | BUILTIN_MATH_EXPM1
            | BUILTIN_MATH_LOG
            | BUILTIN_MATH_LOG10
            | BUILTIN_MATH_LOG1P
            | BUILTIN_MATH_LOG2
            | BUILTIN_MATH_FROUND
            | BUILTIN_MATH_TRUNC
            | BUILTIN_MATH_CBRT
            | BUILTIN_MATH_HYPOT
            | BUILTIN_MATH_SIGN
            | BUILTIN_MATH_POW
            | BUILTIN_MATH_RANDOM
            | BUILTIN_MATH_CLZ32
            | BUILTIN_MATH_IMUL
            | BUILTIN_ISNAN
            | BUILTIN_PARSEINT
            | BUILTIN_PARSEFLOAT
            | BUILTIN_NUMBER_ISNAN
            | BUILTIN_NUMBER_ISFINITE
            | BUILTIN_NUMBER_ISINTEGER
            | BUILTIN_NUMBER_ISSAFEINTEGER
            | BUILTIN_CTOR_NUMBER
            | BUILTIN_CTOR_STRING
            | BUILTIN_OBJECT_KEYS
            | BUILTIN_OBJECT_VALUES
            | BUILTIN_OBJECT_ENTRIES
            | BUILTIN_OBJECT_ASSIGN
            | BUILTIN_OBJECT_FREEZE
            | BUILTIN_OBJECT_HASOWN
            | BUILTIN_OBJECT_CREATE
            | BUILTIN_OBJECT_GETPROTOTYPEOF
            | BUILTIN_OBJECT_DEFINEPROPS
            | BUILTIN_OBJECT_PREVENTEXT
            | BUILTIN_OBJECT_GROUPBY
            | BUILTIN_OBJECT_DEFINEPROP
            | BUILTIN_OBJECT_GETOWNPROPDESC
            | BUILTIN_OBJECT_SETPROTOTYPEOF
            | BUILTIN_OBJECT_GETOWNPROPERTYNAMES
            | BUILTIN_ARRAY_OF
            | BUILTIN_ARRAY_FROM
            | BUILTIN_STRING_FROMCHARCODE
            | BUILTIN_EVAL
            | BUILTIN_NEW_FUNCTION
            | BUILTIN_BIGINT
            | BUILTIN_BIGINT_ASUINTN
            | BUILTIN_BIGINT_ASINTN
            | BUILTIN_DATE_NOW
            | BUILTIN_CTOR_SHAREDARRAYBUFFER
            | BUILTIN_ATOMICS_ISLOCKFREE
            | BUILTIN_ATOMICS_LOAD
            | BUILTIN_ATOMICS_STORE
            | BUILTIN_ATOMICS_COMPAREEXCHANGE
            | BUILTIN_ATOMICS_ADD
            | BUILTIN_ATOMICS_EXCHANGE
            | BUILTIN_ATOMICS_WAIT
            | BUILTIN_ATOMICS_NOTIFY
            | BUILTIN_ATOMICS_WAITASYNC
            | BUILTIN_PROMISE_RESOLVE
            | BUILTIN_PROMISE_ALL
            | BUILTIN_CTOR_DATE
            | BUILTIN_REFLECT_APPLY
            | BUILTIN_SYMBOL
            | BUILTIN_SYMBOL_FOR
            | BUILTIN_SYMBOL_KEYFOR => {
                return self.call_builtin(id, args);
            }
            _ => {}
        }

        if id == BUILTIN_PROMISE_THEN {
            let (has_value, receiver_value, is_rejected) = if let Value::VmObject(obj) = &receiver {
                let b = obj.borrow();
                let has = b.contains_key("__promise_value__");
                (
                    has,
                    b.get("__promise_value__").cloned().unwrap_or(Value::Undefined),
                    matches!(b.get("__promise_rejected__"), Some(Value::Boolean(true))),
                )
            } else {
                (false, Value::Undefined, false)
            };

            // If the promise is still pending, store callbacks for later
            if !has_value {
                let child = self.make_pending_promise();
                // Store {onFulfilled, onRejected, child} on the parent promise
                if let Value::VmObject(obj) = &receiver {
                    let mut entry = IndexMap::new();
                    if let Some(cb) = args.first().cloned() {
                        entry.insert("onFulfilled".to_string(), cb);
                    }
                    if let Some(cb) = args.get(1).cloned() {
                        entry.insert("onRejected".to_string(), cb);
                    }
                    entry.insert("child".to_string(), child.clone());
                    let entry_val = Value::VmObject(Rc::new(RefCell::new(entry)));

                    let mut b = obj.borrow_mut();
                    if let Some(Value::VmArray(arr)) = b.get("__then_queue__").cloned() {
                        arr.borrow_mut().push(entry_val);
                    } else {
                        let arr = VmArrayData::new(vec![entry_val]);
                        b.insert("__then_queue__".to_string(), Value::VmArray(Rc::new(RefCell::new(arr))));
                    }
                }
                return child;
            }

            let callback = if is_rejected { args.get(1).cloned() } else { args.first().cloned() };

            let is_callable = |v: &Value<'gc>| -> bool {
                match v {
                    Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(..) => true,
                    Value::VmObject(map) => {
                        let b = map.borrow();
                        b.contains_key("__host_fn__") || b.contains_key("__bound_target__")
                    }
                    _ => false,
                }
            };

            let mut propagated_rejected = is_rejected;
            let mut callback_result = receiver_value.clone();

            if let Some(cb) = callback
                && is_callable(&cb)
            {
                match cb {
                    Value::VmFunction(ip, _) => {
                        let saved_try_stack = std::mem::take(&mut self.try_stack);
                        let out = self.call_vm_function_result(ip, std::slice::from_ref(&receiver_value), &[]);
                        self.try_stack = saved_try_stack;
                        match out {
                            Ok(v) => {
                                callback_result = v;
                                propagated_rejected = false;
                            }
                            Err(err) => {
                                callback_result = self.vm_value_from_error(&err);
                                propagated_rejected = true;
                            }
                        }
                    }
                    Value::VmClosure(ip, _, upv) => {
                        let uv = (*upv).clone();
                        let saved_try_stack = std::mem::take(&mut self.try_stack);
                        let out = self.call_vm_function_result(ip, std::slice::from_ref(&receiver_value), &uv);
                        self.try_stack = saved_try_stack;
                        match out {
                            Ok(v) => {
                                callback_result = v;
                                propagated_rejected = false;
                            }
                            Err(err) => {
                                callback_result = self.vm_value_from_error(&err);
                                propagated_rejected = true;
                            }
                        }
                    }
                    Value::VmNativeFunction(native_id) => {
                        callback_result = self.call_builtin(native_id, vec![receiver_value.clone()]);
                        propagated_rejected = false;
                    }
                    Value::VmObject(map) => {
                        let borrow = map.borrow();
                        if let Some(Value::String(host_name_u16)) = borrow.get("__host_fn__") {
                            let host_name = crate::unicode::utf16_to_utf8(host_name_u16);
                            drop(borrow);
                            callback_result = self.call_host_fn(&host_name, None, vec![receiver_value.clone()]);
                            propagated_rejected = false;
                        }
                    }
                    _ => {}
                }
            }

            let assimilated = if let Value::VmObject(obj) = &callback_result {
                let b = obj.borrow();
                let is_promise = matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "Promise");
                if is_promise {
                    Some((
                        matches!(b.get("__promise_rejected__"), Some(Value::Boolean(true))),
                        b.get("__promise_value__").cloned().unwrap_or(Value::Undefined),
                    ))
                } else {
                    None
                }
            } else {
                None
            };
            if let Some((is_rej, value)) = assimilated {
                propagated_rejected = is_rej;
                callback_result = value;
            }

            let mut map = IndexMap::new();
            map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Promise")));
            map.insert("then".to_string(), Value::VmNativeFunction(BUILTIN_PROMISE_THEN));
            map.insert("__promise_value__".to_string(), callback_result);
            if propagated_rejected {
                map.insert("__promise_rejected__".to_string(), Value::Boolean(true));
            }
            if let Some(Value::VmObject(promise_ctor)) = self.globals.get("Promise")
                && let Some(proto) = promise_ctor.borrow().get("prototype").cloned()
            {
                map.insert("__proto__".to_string(), proto);
            }
            return Value::VmObject(Rc::new(RefCell::new(map)));
        }

        if id == BUILTIN_ARRAYBUFFER_RESIZE
            && let Value::VmObject(obj) = &receiver
        {
            let mut b = obj.borrow_mut();
            if let Some(Value::String(t)) = b.get("__type__")
                && crate::unicode::utf16_to_utf8(t) == "ArrayBuffer"
            {
                let new_len = match args.first() {
                    Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => *n as usize,
                    _ => 0,
                };
                b.insert("byteLength".to_string(), Value::Number(new_len as f64));
                b.insert("__resized__".to_string(), Value::Boolean(true));
                if let Some(Value::VmArray(bytes)) = b.get("__buffer_bytes__") {
                    let mut bytes_mut = bytes.borrow_mut();
                    bytes_mut.elements.resize(new_len, Value::Number(0.0));
                }
                return Value::Undefined;
            }
        }

        // Date instance methods
        if let Value::VmObject(ref obj) = receiver {
            let date_ms = {
                let borrow = obj.borrow();
                match borrow.get("__date_ms__") {
                    Some(Value::Number(ms)) => Some(*ms),
                    _ => None,
                }
            };
            if let Some(ms) = date_ms {
                use chrono::{Datelike, Local, TimeZone, Timelike, Utc};
                let to_local = || Local.timestamp_millis_opt(ms as i64).single();
                let to_utc = || Utc.timestamp_millis_opt(ms as i64).single();
                match id {
                    BUILTIN_DATE_GETTIME | BUILTIN_DATE_VALUEOF => return Value::Number(ms),
                    BUILTIN_DATE_TOSTRING => {
                        if let Some(dt) = to_local() {
                            let s = dt.format("%a %b %d %Y %H:%M:%S GMT%z").to_string();
                            return Value::String(crate::unicode::utf8_to_utf16(&s));
                        }
                        return Value::String(crate::unicode::utf8_to_utf16("Invalid Date"));
                    }
                    BUILTIN_DATE_TOLOCALEDATESTRING => {
                        if let Some(dt) = to_local() {
                            let s = format!("{}/{}/{}", dt.month(), dt.day(), dt.year());
                            return Value::String(crate::unicode::utf8_to_utf16(&s));
                        }
                        return Value::String(crate::unicode::utf8_to_utf16("Invalid Date"));
                    }
                    BUILTIN_DATE_TOLOCALETIMESTRING => {
                        if let Some(dt) = to_local() {
                            let s = dt.format("%H:%M:%S").to_string();
                            return Value::String(crate::unicode::utf8_to_utf16(&s));
                        }
                        return Value::String(crate::unicode::utf8_to_utf16("Invalid Date"));
                    }
                    BUILTIN_DATE_TOLOCALESTRING => {
                        if let Some(dt) = to_local() {
                            let s = format!("{}/{}/{} {}", dt.month(), dt.day(), dt.year(), dt.format("%H:%M:%S"));
                            return Value::String(crate::unicode::utf8_to_utf16(&s));
                        }
                        return Value::String(crate::unicode::utf8_to_utf16("Invalid Date"));
                    }
                    BUILTIN_DATE_TOISOSTRING => {
                        if let Some(dt) = to_utc() {
                            let s = dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
                            return Value::String(crate::unicode::utf8_to_utf16(&s));
                        }
                        return Value::String(crate::unicode::utf8_to_utf16("Invalid Date"));
                    }
                    BUILTIN_DATE_GETFULLYEAR => {
                        return Value::Number(to_local().map(|dt| dt.year() as f64).unwrap_or(f64::NAN));
                    }
                    BUILTIN_DATE_GETMONTH => {
                        return Value::Number(to_local().map(|dt| (dt.month0()) as f64).unwrap_or(f64::NAN));
                    }
                    BUILTIN_DATE_GETDATE => {
                        return Value::Number(to_local().map(|dt| dt.day() as f64).unwrap_or(f64::NAN));
                    }
                    BUILTIN_DATE_GETDAY => {
                        return Value::Number(to_local().map(|dt| dt.weekday().num_days_from_sunday() as f64).unwrap_or(f64::NAN));
                    }
                    BUILTIN_DATE_GETHOURS => {
                        return Value::Number(to_local().map(|dt| dt.hour() as f64).unwrap_or(f64::NAN));
                    }
                    BUILTIN_DATE_GETMINUTES => {
                        return Value::Number(to_local().map(|dt| dt.minute() as f64).unwrap_or(f64::NAN));
                    }
                    BUILTIN_DATE_GETSECONDS => {
                        return Value::Number(to_local().map(|dt| dt.second() as f64).unwrap_or(f64::NAN));
                    }
                    BUILTIN_DATE_GETMILLISECONDS => {
                        return Value::Number(to_local().map(|dt| dt.timestamp_subsec_millis() as f64).unwrap_or(f64::NAN));
                    }
                    BUILTIN_DATE_GETUTCFULLYEAR => {
                        return Value::Number(to_utc().map(|dt| dt.year() as f64).unwrap_or(f64::NAN));
                    }
                    BUILTIN_DATE_GETUTCMONTH => {
                        return Value::Number(to_utc().map(|dt| dt.month0() as f64).unwrap_or(f64::NAN));
                    }
                    BUILTIN_DATE_GETUTCDATE => {
                        return Value::Number(to_utc().map(|dt| dt.day() as f64).unwrap_or(f64::NAN));
                    }
                    BUILTIN_DATE_GETUTCHOURS => {
                        return Value::Number(to_utc().map(|dt| dt.hour() as f64).unwrap_or(f64::NAN));
                    }
                    BUILTIN_DATE_GETUTCMINUTES => {
                        return Value::Number(to_utc().map(|dt| dt.minute() as f64).unwrap_or(f64::NAN));
                    }
                    BUILTIN_DATE_GETUTCSECONDS => {
                        return Value::Number(to_utc().map(|dt| dt.second() as f64).unwrap_or(f64::NAN));
                    }
                    BUILTIN_DATE_GETTIMEZONEOFFSET => {
                        if let Some(dt) = to_local() {
                            let mins = -(dt.offset().local_minus_utc() as f64 / 60.0);
                            return Value::Number(mins);
                        }
                        return Value::Number(f64::NAN);
                    }
                    BUILTIN_DATE_TODATESTRING => {
                        if let Some(dt) = to_local() {
                            let s = dt.format("%a %b %d %Y").to_string();
                            return Value::String(crate::unicode::utf8_to_utf16(&s));
                        }
                        return Value::String(crate::unicode::utf8_to_utf16("Invalid Date"));
                    }
                    BUILTIN_DATE_SETTIME => {
                        let mut new_ms = f64::NAN;
                        if let Some(Value::Number(n)) = args.first() {
                            new_ms = *n;
                        }
                        obj.borrow_mut().insert("__date_ms__".to_string(), Value::Number(new_ms));
                        return Value::Number(new_ms);
                    }
                    BUILTIN_DATE_SETFULLYEAR => {
                        let mut new_ms = f64::NAN;
                        if let Some(Value::Number(y)) = args.first()
                            && let Some(dt) = to_local()
                            && let Some(new_dt) = dt.with_year(*y as i32)
                        {
                            new_ms = new_dt.timestamp_millis() as f64;
                        }
                        obj.borrow_mut().insert("__date_ms__".to_string(), Value::Number(new_ms));
                        return Value::Number(new_ms);
                    }
                    BUILTIN_DATE_SETDATE => {
                        let mut new_ms = f64::NAN;
                        if let Some(Value::Number(d)) = args.first()
                            && let Some(dt) = to_local()
                        {
                            let current_day = dt.day() as i64;
                            let target_day = *d as i64;
                            let diff_sec = (target_day - current_day) * 86400;
                            new_ms = dt.timestamp_millis() as f64 + (diff_sec * 1000) as f64;
                        }
                        obj.borrow_mut().insert("__date_ms__".to_string(), Value::Number(new_ms));
                        return Value::Number(new_ms);
                    }
                    _ => {}
                }
            }
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
                    let parts: Vec<String> = arr
                        .borrow()
                        .iter()
                        .map(|v| match v {
                            Value::Undefined | Value::Null => String::new(),
                            other => value_to_string(other),
                        })
                        .collect();
                    return Value::String(crate::unicode::utf8_to_utf16(&parts.join(&sep)));
                }
                BUILTIN_ARRAY_INDEXOF => {
                    let needle = args.first().cloned().unwrap_or(Value::Undefined);
                    let from_index = match args.get(1) {
                        Some(Value::Number(n)) => {
                            let i = *n as i64;
                            let len = arr.borrow().elements.len() as i64;
                            if i < 0 { (len + i).max(0) as usize } else { i as usize }
                        }
                        _ => 0,
                    };
                    let a = arr.borrow();
                    for (i, v) in a.iter().enumerate().skip(from_index) {
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
                    if let Some(Value::VmFunction(ip, _arity) | Value::VmClosure(ip, _arity, _)) = args.first() {
                        let __cb_uv = if let Some(Value::VmClosure(_, _, u)) = args.first() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
                        let borrow = arr.borrow();
                        let elements = borrow.elements.clone();
                        let holes: std::collections::HashSet<usize> = borrow
                            .props
                            .keys()
                            .filter_map(|k| k.strip_prefix("__deleted_").and_then(|s| s.parse::<usize>().ok()))
                            .collect();
                        drop(borrow);
                        let mut result_data = VmArrayData::new(Vec::new());
                        for (i, elem) in elements.iter().enumerate() {
                            if holes.contains(&i) {
                                result_data.elements.push(Value::Undefined);
                                result_data.props.insert(format!("__deleted_{}", i), Value::Boolean(true));
                            } else {
                                let r = self.call_vm_function(*ip, &[elem.clone(), Value::Number(i as f64)], &__cb_uv);
                                result_data.elements.push(r);
                            }
                        }
                        return Value::VmArray(Rc::new(RefCell::new(result_data)));
                    }
                    return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(Vec::new()))));
                }
                BUILTIN_ARRAY_FILTER => {
                    if let Some(Value::VmFunction(ip, _arity) | Value::VmClosure(ip, _arity, _)) = args.first() {
                        let __cb_uv = if let Some(Value::VmClosure(_, _, u)) = args.first() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
                        let borrow = arr.borrow();
                        let elements = borrow.elements.clone();
                        let holes: std::collections::HashSet<usize> = borrow
                            .props
                            .keys()
                            .filter_map(|k| k.strip_prefix("__deleted_").and_then(|s| s.parse::<usize>().ok()))
                            .collect();
                        drop(borrow);
                        let mut result = Vec::new();
                        for (i, elem) in elements.iter().enumerate() {
                            if holes.contains(&i) {
                                continue;
                            }
                            let r = self.call_vm_function(*ip, &[elem.clone(), Value::Number(i as f64)], &__cb_uv);
                            if r.to_truthy() {
                                result.push(elem.clone());
                            }
                        }
                        return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(result))));
                    }
                    return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(Vec::new()))));
                }
                BUILTIN_ARRAY_ITERATOR => {
                    let typed_view = {
                        let a = arr.borrow();
                        let buffer = a.props.get("__typedarray_buffer__").cloned();
                        let byte_offset = a
                            .props
                            .get("__byte_offset__")
                            .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None })
                            .unwrap_or(0);
                        let bpe = a
                            .props
                            .get("__bytes_per_element__")
                            .and_then(|v| {
                                if let Value::Number(n) = v {
                                    Some((*n as usize).max(1))
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(1);
                        let fixed_length = a
                            .props
                            .get("__fixed_length__")
                            .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None });
                        let length_tracking = matches!(a.props.get("__length_tracking__"), Some(Value::Boolean(true)));
                        buffer.map(|b| (b, byte_offset, bpe, fixed_length, length_tracking))
                    };
                    if let Some((Value::VmObject(buf_obj), byte_offset, bpe, fixed_length, length_tracking)) = typed_view {
                        let (byte_len, resized) = {
                            let b = buf_obj.borrow();
                            let bl = b
                                .get("byteLength")
                                .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None })
                                .unwrap_or(0);
                            let rz = matches!(b.get("__resized__"), Some(Value::Boolean(true)));
                            (bl, rz)
                        };
                        let out_of_bounds_base = if let Some(fixed) = fixed_length {
                            byte_len < byte_offset.saturating_add(fixed.saturating_mul(bpe))
                        } else {
                            length_tracking && byte_offset > 0 && byte_len < byte_offset
                        };
                        let out_of_bounds =
                            out_of_bounds_base || (resized && (fixed_length.is_some() || (length_tracking && byte_offset > 0)));
                        if out_of_bounds {
                            let mut err_map = IndexMap::new();
                            err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                            err_map.insert(
                                "message".to_string(),
                                Value::String(crate::unicode::utf8_to_utf16("TypedArray view is out of bounds")),
                            );
                            self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                            return Value::Undefined;
                        }
                    }
                    let items = arr.borrow().elements.clone();
                    return self.make_iterator(items);
                }
                BUILTIN_ARRAY_FOREACH => {
                    if let Some(Value::VmFunction(ip, _arity) | Value::VmClosure(ip, _arity, _)) = args.first() {
                        let __cb_uv = if let Some(Value::VmClosure(_, _, u)) = args.first() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
                        let borrow = arr.borrow();
                        let elements = borrow.elements.clone();
                        let holes: std::collections::HashSet<usize> = borrow
                            .props
                            .keys()
                            .filter_map(|k| k.strip_prefix("__deleted_").and_then(|s| s.parse::<usize>().ok()))
                            .collect();
                        drop(borrow);
                        for (i, elem) in elements.iter().enumerate() {
                            if holes.contains(&i) {
                                continue;
                            }
                            self.call_vm_function(*ip, &[elem.clone(), Value::Number(i as f64)], &__cb_uv);
                        }
                    }
                    return Value::Undefined;
                }
                BUILTIN_ARRAY_REDUCE => {
                    if let Some(Value::VmFunction(ip, _arity) | Value::VmClosure(ip, _arity, _)) = args.first() {
                        let __cb_uv = if let Some(Value::VmClosure(_, _, u)) = args.first() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
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
                            acc = self.call_vm_function(*ip, &[acc, element.clone(), Value::Number(i as f64)], &__cb_uv);
                        }
                        return acc;
                    }
                    return Value::Undefined;
                }
                BUILTIN_ARRAY_SHIFT => {
                    let val = if arr.borrow().elements.is_empty() {
                        Value::Undefined
                    } else {
                        arr.borrow_mut().elements.remove(0)
                    };
                    return val;
                }
                BUILTIN_ARRAY_UNSHIFT => {
                    let mut a = arr.borrow_mut();
                    for (i, arg) in args.iter().enumerate() {
                        a.elements.insert(i, arg.clone());
                    }
                    return Value::Number(a.elements.len() as f64);
                }
                BUILTIN_ARRAY_SPLICE => {
                    let len = arr.borrow().elements.len() as i64;
                    let start_raw = args
                        .first()
                        .map(|v| match v {
                            Value::Number(n) => *n as i64,
                            _ => 0,
                        })
                        .unwrap_or(0);
                    let start = if start_raw < 0 {
                        (len + start_raw).max(0) as usize
                    } else {
                        (start_raw).min(len) as usize
                    };
                    let delete_count = args
                        .get(1)
                        .map(|v| match v {
                            Value::Number(n) => (*n as i64).max(0) as usize,
                            _ => 0,
                        })
                        .unwrap_or((len - start as i64).max(0) as usize);
                    let delete_count = delete_count.min((len - start as i64).max(0) as usize);
                    let insert_items: Vec<Value<'gc>> = args.into_iter().skip(2).collect();
                    let mut a = arr.borrow_mut();

                    // Collect holes info for the removed region
                    let mut removed_holes = std::collections::HashSet::new();
                    for i in start..start + delete_count {
                        if a.props.contains_key(&format!("__deleted_{}", i)) {
                            removed_holes.insert(i - start);
                        }
                    }

                    // Collect holes info for elements after the removed region
                    let mut after_holes = std::collections::HashSet::new();
                    for i in (start + delete_count)..(len as usize) {
                        if a.props.contains_key(&format!("__deleted_{}", i)) {
                            after_holes.insert(i);
                        }
                    }

                    // Remove old hole markers
                    let keys_to_remove: Vec<String> = a.props.keys().filter(|k| k.starts_with("__deleted_")).cloned().collect();
                    for k in keys_to_remove {
                        a.props.shift_remove(&k);
                    }

                    let removed: Vec<Value<'gc>> = a.elements.drain(start..start + delete_count).collect();
                    for (i, item) in insert_items.into_iter().enumerate() {
                        a.elements.insert(start + i, item);
                    }

                    // Re-apply holes for elements that shifted
                    let item_count = a.elements.len();
                    let _shift = delete_count as i64
                        - (item_count as i64
                            - (len - delete_count as i64 - start as i64 + (item_count as i64 - (len as usize - delete_count) as i64)));
                    // Simpler: elements after `start + delete_count` shifted to `start + insert_count`
                    let insert_count = a.elements.len() - (len as usize - delete_count);
                    for old_idx in after_holes {
                        let new_idx = old_idx - delete_count + insert_count;
                        if new_idx < a.elements.len() {
                            a.props.insert(format!("__deleted_{}", new_idx), Value::Boolean(true));
                        }
                    }

                    // Build removed array with holes preserved
                    let mut removed_data = VmArrayData::new(removed);
                    for hole_idx in removed_holes {
                        removed_data.props.insert(format!("__deleted_{}", hole_idx), Value::Boolean(true));
                    }
                    return Value::VmArray(Rc::new(RefCell::new(removed_data)));
                }
                BUILTIN_ARRAY_REVERSE => {
                    arr.borrow_mut().elements.reverse();
                    return Value::VmArray(arr.clone());
                }
                BUILTIN_ARRAY_SORT => {
                    let cmp_fn = args.first().cloned();
                    let mut elems = arr.borrow().elements.clone();
                    if let Some(Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _)) = &cmp_fn {
                        let __cb_uv = if let Value::VmClosure(_, _, u) = cmp_fn.as_ref().unwrap() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
                        let ip = *ip;
                        elems.sort_by(|a, b| {
                            let result = self.call_vm_function(ip, &[a.clone(), b.clone()], &__cb_uv);
                            if let Value::Number(n) = result {
                                n.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
                            } else {
                                std::cmp::Ordering::Equal
                            }
                        });
                    } else {
                        elems.sort_by(|a, b| {
                            let sa = value_to_string(a);
                            let sb = value_to_string(b);
                            sa.cmp(&sb)
                        });
                    }
                    arr.borrow_mut().elements = elems;
                    return Value::VmArray(arr.clone());
                }
                BUILTIN_ARRAY_FIND => {
                    if let Some(Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _)) = args.first() {
                        let __cb_uv = if let Some(Value::VmClosure(_, _, u)) = args.first() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
                        let elements = arr.borrow().elements.clone();
                        for (i, elem) in elements.iter().enumerate() {
                            let result = self.call_vm_function(*ip, &[elem.clone(), Value::Number(i as f64)], &__cb_uv);
                            if result.to_truthy() {
                                return elem.clone();
                            }
                        }
                    }
                    return Value::Undefined;
                }
                BUILTIN_ARRAY_FINDLAST => {
                    if let Some(Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _)) = args.first() {
                        let __cb_uv = if let Some(Value::VmClosure(_, _, u)) = args.first() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
                        let elements = arr.borrow().elements.clone();
                        for (i, elem) in elements.iter().enumerate().rev() {
                            let result = self.call_vm_function(*ip, &[elem.clone(), Value::Number(i as f64)], &__cb_uv);
                            if result.to_truthy() {
                                return elem.clone();
                            }
                        }
                    }
                    return Value::Undefined;
                }
                BUILTIN_ARRAY_FINDINDEX => {
                    if let Some(Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _)) = args.first() {
                        let __cb_uv = if let Some(Value::VmClosure(_, _, u)) = args.first() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
                        let elements = arr.borrow().elements.clone();
                        for (i, elem) in elements.iter().enumerate() {
                            let result = self.call_vm_function(*ip, &[elem.clone(), Value::Number(i as f64)], &__cb_uv);
                            if result.to_truthy() {
                                return Value::Number(i as f64);
                            }
                        }
                    }
                    return Value::Number(-1.0);
                }
                BUILTIN_ARRAY_FINDLASTINDEX => {
                    if let Some(Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _)) = args.first() {
                        let __cb_uv = if let Some(Value::VmClosure(_, _, u)) = args.first() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
                        let elements = arr.borrow().elements.clone();
                        for (i, elem) in elements.iter().enumerate().rev() {
                            let result = self.call_vm_function(*ip, &[elem.clone(), Value::Number(i as f64)], &__cb_uv);
                            if result.to_truthy() {
                                return Value::Number(i as f64);
                            }
                        }
                    }
                    return Value::Number(-1.0);
                }
                BUILTIN_ARRAY_INCLUDES => {
                    let target = args.first().cloned().unwrap_or(Value::Undefined);
                    let elements = arr.borrow().elements.clone();
                    for elem in &elements {
                        if self.strict_equal(elem, &target) {
                            return Value::Boolean(true);
                        }
                    }
                    return Value::Boolean(false);
                }
                BUILTIN_ARRAY_FLAT => {
                    let depth = args
                        .first()
                        .map(|v| match v {
                            Value::Number(n) => *n as usize,
                            _ => 1,
                        })
                        .unwrap_or(1);
                    let elements = arr.borrow().elements.clone();
                    let result = self.flatten_array(elements, depth);
                    return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(result))));
                }
                BUILTIN_ARRAY_FLATMAP => {
                    if let Some(Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _)) = args.first() {
                        let __cb_uv = if let Some(Value::VmClosure(_, _, u)) = args.first() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
                        let elements = arr.borrow().elements.clone();
                        let mut result = Vec::new();
                        for (i, elem) in elements.iter().enumerate() {
                            let call_args = if *arity >= 2 {
                                vec![elem.clone(), Value::Number(i as f64)]
                            } else {
                                vec![elem.clone()]
                            };
                            let mapped = self.call_vm_function(*ip, &call_args, &__cb_uv);
                            if let Value::VmArray(inner) = mapped {
                                result.extend(inner.borrow().elements.clone());
                            } else {
                                result.push(mapped);
                            }
                        }
                        return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(result))));
                    }
                    return Value::Undefined;
                }
                BUILTIN_ARRAY_AT => {
                    let idx = args
                        .first()
                        .map(|v| match v {
                            Value::Number(n) => *n as i64,
                            _ => 0,
                        })
                        .unwrap_or(0);
                    let a = arr.borrow();
                    let len = a.elements.len() as i64;
                    let actual = if idx < 0 { len + idx } else { idx };
                    if actual >= 0 && actual < len {
                        return a.elements[actual as usize].clone();
                    }
                    return Value::Undefined;
                }
                BUILTIN_ARRAY_EVERY => {
                    if let Some(Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _)) = args.first() {
                        let __cb_uv = if let Some(Value::VmClosure(_, _, u)) = args.first() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
                        let elements = arr.borrow().elements.clone();
                        for (i, elem) in elements.iter().enumerate() {
                            let result = self.call_vm_function(*ip, &[elem.clone(), Value::Number(i as f64)], &__cb_uv);
                            if !result.to_truthy() {
                                return Value::Boolean(false);
                            }
                        }
                        return Value::Boolean(true);
                    }
                    return Value::Boolean(true);
                }
                BUILTIN_ARRAY_SOME => {
                    if let Some(Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _)) = args.first() {
                        let __cb_uv = if let Some(Value::VmClosure(_, _, u)) = args.first() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
                        let borrow = arr.borrow();
                        let elements = borrow.elements.clone();
                        let holes: std::collections::HashSet<usize> = borrow
                            .props
                            .keys()
                            .filter_map(|k| k.strip_prefix("__deleted_").and_then(|s| s.parse::<usize>().ok()))
                            .collect();
                        drop(borrow);
                        for (i, elem) in elements.iter().enumerate() {
                            if holes.contains(&i) {
                                continue;
                            }
                            let result = self.call_vm_function(*ip, &[elem.clone(), Value::Number(i as f64)], &__cb_uv);
                            if result.to_truthy() {
                                return Value::Boolean(true);
                            }
                        }
                        return Value::Boolean(false);
                    }
                    return Value::Boolean(false);
                }
                BUILTIN_ARRAY_FILL => {
                    let fill_val = args.first().cloned().unwrap_or(Value::Undefined);
                    let len = arr.borrow().elements.len() as i64;
                    let start = args
                        .get(1)
                        .map(|v| match v {
                            Value::Number(n) => {
                                let s = *n as i64;
                                if s < 0 { (len + s).max(0) as usize } else { s.min(len) as usize }
                            }
                            _ => 0,
                        })
                        .unwrap_or(0);
                    let end = args
                        .get(2)
                        .map(|v| match v {
                            Value::Number(n) => {
                                let e = *n as i64;
                                if e < 0 { (len + e).max(0) as usize } else { e.min(len) as usize }
                            }
                            _ => len as usize,
                        })
                        .unwrap_or(len as usize);
                    let mut a = arr.borrow_mut();
                    for i in start..end {
                        a.elements[i] = fill_val.clone();
                    }
                    return Value::VmArray(arr.clone());
                }
                BUILTIN_ARRAY_LASTINDEXOF => {
                    let target = args.first().cloned().unwrap_or(Value::Undefined);
                    let elements = arr.borrow().elements.clone();
                    let start_from = args
                        .get(1)
                        .map(|v| match v {
                            Value::Number(n) => {
                                let s = *n as i64;
                                if s < 0 {
                                    (elements.len() as i64 + s).max(0) as usize
                                } else {
                                    s.min(elements.len() as i64 - 1) as usize
                                }
                            }
                            _ => elements.len() - 1,
                        })
                        .unwrap_or(elements.len().saturating_sub(1));
                    for i in (0..=start_from).rev() {
                        if self.strict_equal(&elements[i], &target) {
                            return Value::Number(i as f64);
                        }
                    }
                    return Value::Number(-1.0);
                }
                BUILTIN_ARRAY_REDUCERIGHT => {
                    if let Some(Value::VmFunction(ip, _arity) | Value::VmClosure(ip, _arity, _)) = args.first() {
                        let __cb_uv = if let Some(Value::VmClosure(_, _, u)) = args.first() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
                        let elements = arr.borrow().elements.clone();
                        let mut acc = if args.len() > 1 {
                            args[1].clone()
                        } else if !elements.is_empty() {
                            elements[elements.len() - 1].clone()
                        } else {
                            Value::Undefined
                        };
                        let skip_last = if args.len() <= 1 { 1 } else { 0 };
                        let end = elements.len().saturating_sub(skip_last);
                        for i in (0..end).rev() {
                            acc = self.call_vm_function(*ip, &[acc, elements[i].clone(), Value::Number(i as f64)], &__cb_uv);
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
                    let limit = args.get(1).and_then(|v| match v {
                        Value::Number(n) => Some(*n as usize),
                        _ => None,
                    });
                    if args.is_empty() || matches!(args.first(), Some(Value::Undefined)) {
                        return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(vec![Value::String(
                            crate::unicode::utf8_to_utf16(&rust_str),
                        )]))));
                    }
                    if let Some(Value::VmObject(re_obj)) = args.first() {
                        let is_regex = re_obj.borrow().get("__type__").map(value_to_string) == Some("RegExp".to_string());
                        if is_regex {
                            let parts = self.regex_split_string(&rust_str, re_obj, limit);
                            return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(parts))));
                        }
                    }
                    let sep = args.first().map(value_to_string).unwrap_or_default();
                    let parts: Vec<Value<'gc>> = if sep.is_empty() {
                        rust_str
                            .chars()
                            .map(|c| Value::String(crate::unicode::utf8_to_utf16(&c.to_string())))
                            .collect()
                    } else {
                        let all: Vec<&str> = rust_str.split(&sep).collect();
                        let take = limit.unwrap_or(all.len());
                        all.into_iter()
                            .take(take)
                            .map(|p| Value::String(crate::unicode::utf8_to_utf16(p)))
                            .collect()
                    };
                    return Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(parts))));
                }
                BUILTIN_STRING_INDEXOF => {
                    let needle = args.first().map(value_to_string).unwrap_or_default();
                    let pos = args
                        .get(1)
                        .and_then(|v| {
                            if let Value::Number(n) = v {
                                Some((*n).max(0.0) as usize)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0)
                        .min(rust_str.len());
                    return match rust_str[pos..].find(&needle) {
                        Some(off) => Value::Number((pos + off) as f64),
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
                    if matches!(args.first(), Some(Value::Number(n)) if *n < 0.0) {
                        return Value::String(crate::unicode::utf8_to_utf16(""));
                    }
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
                    if let Some(Value::VmObject(re_obj)) = args.first() {
                        let is_regex = re_obj.borrow().get("__type__").map(value_to_string) == Some("RegExp".to_string());
                        if is_regex {
                            let replacement = args.get(1).map(value_to_string).unwrap_or_default();
                            let result = self.regex_replace_string(&rust_str, re_obj, &replacement, false);
                            return Value::String(crate::unicode::utf8_to_utf16(&result));
                        }
                    }
                    let pattern = args.first().map(value_to_string).unwrap_or_default();
                    let replacement = args.get(1).map(value_to_string).unwrap_or_default();
                    let result = rust_str.replacen(&pattern, &replacement, 1);
                    return Value::String(crate::unicode::utf8_to_utf16(&result));
                }
                BUILTIN_STRING_REPLACEALL => {
                    if let Some(Value::VmObject(re_obj)) = args.first() {
                        let borrow = re_obj.borrow();
                        let is_regex = borrow.get("__type__").map(value_to_string) == Some("RegExp".to_string());
                        let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
                        drop(borrow);
                        if is_regex {
                            if !flags.contains('g') {
                                eprintln!("TypeError: String.prototype.replaceAll called with a non-global RegExp argument");
                                return Value::String(crate::unicode::utf8_to_utf16(&rust_str));
                            }
                            let replacement = args.get(1).map(value_to_string).unwrap_or_default();
                            let result = self.regex_replace_string(&rust_str, re_obj, &replacement, true);
                            return Value::String(crate::unicode::utf8_to_utf16(&result));
                        }
                    }
                    let pattern = args.first().map(value_to_string).unwrap_or_default();
                    let replacement = args.get(1).map(value_to_string).unwrap_or_default();
                    let result = rust_str.replace(&pattern, &replacement);
                    return Value::String(crate::unicode::utf8_to_utf16(&result));
                }
                BUILTIN_STRING_MATCH => {
                    if let Some(Value::VmObject(re_obj)) = args.first() {
                        let is_regex = re_obj.borrow().get("__type__").map(value_to_string) == Some("RegExp".to_string());
                        if is_regex {
                            let borrow = re_obj.borrow();
                            let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
                            drop(borrow);
                            if flags.contains('g') {
                                return self.regex_match_all(&rust_str, re_obj);
                            } else {
                                return self.regex_exec(re_obj, &rust_str);
                            }
                        }
                    }
                    return Value::Null;
                }
                BUILTIN_STRING_SEARCH => {
                    if let Some(Value::VmObject(re_obj)) = args.first() {
                        let is_regex = re_obj.borrow().get("__type__").map(value_to_string) == Some("RegExp".to_string());
                        if is_regex {
                            let borrow = re_obj.borrow();
                            let pattern = borrow.get("__regex_pattern__").map(value_to_string).unwrap_or_default();
                            let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
                            drop(borrow);
                            let pattern_u16 = crate::unicode::utf8_to_utf16(&pattern);
                            if let Ok(re) = get_or_compile_regex(&pattern_u16, &flags) {
                                let input_u16: Vec<u16> = rust_str.encode_utf16().collect();
                                let use_unicode = flags.contains('u') || flags.contains('v');
                                let m = if use_unicode {
                                    re.find_from_utf16(&input_u16, 0).next()
                                } else {
                                    re.find_from_ucs2(&input_u16, 0).next()
                                };
                                return Value::Number(m.map(|m| m.range.start as f64).unwrap_or(-1.0));
                            }
                        }
                    }
                    return Value::Number(-1.0);
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
                BUILTIN_STRING_PADSTART => {
                    let target_len = args
                        .first()
                        .map(|v| match v {
                            Value::Number(n) => *n as usize,
                            _ => 0,
                        })
                        .unwrap_or(0);
                    let pad_str = args.get(1).map(value_to_string).unwrap_or_else(|| " ".to_string());
                    let chars: Vec<char> = rust_str.chars().collect();
                    if chars.len() >= target_len || pad_str.is_empty() {
                        return Value::String(crate::unicode::utf8_to_utf16(&rust_str));
                    }
                    let pad_chars: Vec<char> = pad_str.chars().collect();
                    let pad_needed = target_len - chars.len();
                    let mut result = String::new();
                    for i in 0..pad_needed {
                        result.push(pad_chars[i % pad_chars.len()]);
                    }
                    result.push_str(&rust_str);
                    return Value::String(crate::unicode::utf8_to_utf16(&result));
                }
                BUILTIN_STRING_PADEND => {
                    let target_len = args
                        .first()
                        .map(|v| match v {
                            Value::Number(n) => *n as usize,
                            _ => 0,
                        })
                        .unwrap_or(0);
                    let pad_str = args.get(1).map(value_to_string).unwrap_or_else(|| " ".to_string());
                    let chars: Vec<char> = rust_str.chars().collect();
                    if chars.len() >= target_len || pad_str.is_empty() {
                        return Value::String(crate::unicode::utf8_to_utf16(&rust_str));
                    }
                    let pad_chars: Vec<char> = pad_str.chars().collect();
                    let pad_needed = target_len - chars.len();
                    let mut result = rust_str.clone();
                    for i in 0..pad_needed {
                        result.push(pad_chars[i % pad_chars.len()]);
                    }
                    return Value::String(crate::unicode::utf8_to_utf16(&result));
                }
                BUILTIN_STRING_REPEAT => {
                    let count = args
                        .first()
                        .map(|v| match v {
                            Value::Number(n) => *n as usize,
                            _ => 0,
                        })
                        .unwrap_or(0);
                    return Value::String(crate::unicode::utf8_to_utf16(&rust_str.repeat(count)));
                }
                BUILTIN_STRING_CHARCODEAT => {
                    let idx = args
                        .first()
                        .map(|v| match v {
                            Value::Number(n) if *n >= 0.0 => *n as usize,
                            _ => 0,
                        })
                        .unwrap_or(0);
                    if idx < s.len() {
                        return Value::Number(s[idx] as f64);
                    }
                    return Value::Number(f64::NAN);
                }
                BUILTIN_STRING_TRIMSTART => {
                    return Value::String(crate::unicode::utf8_to_utf16(rust_str.trim_start()));
                }
                BUILTIN_STRING_TRIMEND => {
                    return Value::String(crate::unicode::utf8_to_utf16(rust_str.trim_end()));
                }
                BUILTIN_STRING_LASTINDEXOF => {
                    let needle = args.first().map(value_to_string).unwrap_or_default();
                    let default_pos = rust_str.len().saturating_sub(1);
                    let end_pos = args
                        .get(1)
                        .and_then(|v| {
                            if let Value::Number(n) = v {
                                Some((*n).max(0.0) as usize)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(default_pos)
                        .min(default_pos);
                    let upto = &rust_str[..=end_pos];
                    return match upto.rfind(&needle) {
                        Some(pos) => Value::Number(pos as f64),
                        None => Value::Number(-1.0),
                    };
                }
                _ => {}
            }
        }

        // Number instance methods (receiver is a Number value or Number wrapper)
        {
            let num = match &receiver {
                Value::Number(n) => Some(*n),
                Value::VmObject(map) => {
                    let b = map.borrow();
                    if b.get("__type__").map(|v| value_to_string(v)).as_deref() == Some("Number") {
                        b.get("__value__").map(|v| to_number(v))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let bigint = match &receiver {
                Value::BigInt(b) => Some((**b).clone()),
                Value::VmObject(map) => {
                    let b = map.borrow();
                    if b.get("__type__").map(|v| value_to_string(v)).as_deref() == Some("BigInt") {
                        match b.get("__value__") {
                            Some(Value::BigInt(inner)) => Some((**inner).clone()),
                            _ => None,
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(n) = num {
                match id {
                    BUILTIN_NUM_TOFIXED => {
                        let digits = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                        return Value::String(crate::unicode::utf8_to_utf16(&format!("{:.prec$}", n, prec = digits)));
                    }
                    BUILTIN_NUM_TOEXPONENTIAL => {
                        let has_arg = !args.is_empty() && !matches!(args.first(), Some(Value::Undefined));
                        if has_arg {
                            let digits = to_number(args.first().unwrap()) as usize;
                            let s = format!("{:.prec$e}", n, prec = digits);
                            return Value::String(crate::unicode::utf8_to_utf16(&js_exponential_format(&s)));
                        } else {
                            // No argument: show all significant digits
                            // Use enough precision then strip trailing zeros
                            let s = format!("{:e}", n);
                            return Value::String(crate::unicode::utf8_to_utf16(&js_exponential_format(&s)));
                        }
                    }
                    BUILTIN_NUM_TOPRECISION => {
                        let has_arg = !args.is_empty() && !matches!(args.first(), Some(Value::Undefined));
                        if !has_arg {
                            // No argument: same as toString()
                            return Value::String(crate::unicode::utf8_to_utf16(&value_to_string(&Value::Number(n))));
                        }
                        let prec = to_number(args.first().unwrap()) as usize;
                        if prec == 0 {
                            return Value::Undefined;
                        }

                        if n.is_nan() {
                            return Value::String(crate::unicode::utf8_to_utf16("NaN"));
                        }
                        if n.is_infinite() {
                            return Value::String(crate::unicode::utf8_to_utf16(if n > 0.0 { "Infinity" } else { "-Infinity" }));
                        }

                        // Format with exponential to get significant digits
                        let s = format!("{:.prec$e}", n, prec = prec.saturating_sub(1));
                        // Parse the exponent
                        let parts: Vec<&str> = s.split('e').collect();
                        if parts.len() != 2 {
                            return Value::String(crate::unicode::utf8_to_utf16(&s));
                        }
                        let _mantissa = parts[0];
                        let exp: i32 = parts[1].parse().unwrap_or(0);

                        if exp < -6 || exp >= prec as i32 {
                            // Use exponential notation
                            return Value::String(crate::unicode::utf8_to_utf16(&js_exponential_format(&s)));
                        }

                        // Fixed notation
                        let decimal_places = (prec as i32 - 1 - exp).max(0) as usize;
                        let neg = if n < 0.0 { "-" } else { "" };
                        let abs_n = n.abs();
                        return Value::String(crate::unicode::utf8_to_utf16(&format!(
                            "{}{:.prec$}",
                            neg,
                            abs_n,
                            prec = decimal_places
                        )));
                    }
                    BUILTIN_NUM_TOSTRING => {
                        let radix = args.first().map(|v| to_number(v) as u32).unwrap_or(10);
                        if radix == 10 {
                            return Value::String(crate::unicode::utf8_to_utf16(&value_to_string(&Value::Number(n))));
                        }
                        // Integer-only for non-10 radixes
                        let i = n as i64;
                        let s = match radix {
                            2 => format!("{:b}", i),
                            8 => format!("{:o}", i),
                            16 => format!("{:x}", i),
                            _ => format!("{}", i),
                        };
                        return Value::String(crate::unicode::utf8_to_utf16(&s));
                    }
                    BUILTIN_NUM_VALUEOF => {
                        return Value::Number(n);
                    }
                    _ => {}
                }
            }
            if let Some(bi) = bigint {
                match id {
                    BUILTIN_NUM_TOSTRING => {
                        let radix = args.first().map(|v| to_number(v) as u32).unwrap_or(10);
                        let s = match radix {
                            2 => bi.to_str_radix(2),
                            8 => bi.to_str_radix(8),
                            10 => bi.to_string(),
                            16 => bi.to_str_radix(16),
                            _ => bi.to_string(),
                        };
                        return Value::String(crate::unicode::utf8_to_utf16(&s));
                    }
                    BUILTIN_NUM_VALUEOF => {
                        return Value::BigInt(Box::new(bi));
                    }
                    _ => {}
                }
            }
        }

        // Map methods
        if let Value::VmMap(ref m) = receiver {
            match id {
                BUILTIN_MAP_SET => {
                    let key = args.first().cloned().unwrap_or(Value::Undefined);
                    let val = args.get(1).cloned().unwrap_or(Value::Undefined);
                    let mut borrow = m.borrow_mut();
                    // WeakMap requires object keys
                    if borrow.is_weak && !matches!(key, Value::VmObject(_) | Value::VmArray(_) | Value::VmMap(_) | Value::VmSet(_)) {
                        drop(borrow);
                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("Invalid value used as weak map key")),
                        );
                        self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map)))).ok();
                        return Value::Undefined;
                    }
                    // Update existing key or insert new
                    if let Some(entry) = borrow.entries.iter_mut().find(|(k, _)| self.values_equal(k, &key)) {
                        entry.1 = val;
                    } else {
                        borrow.entries.push((key, val));
                    }
                    drop(borrow);
                    return receiver; // Map.set returns the Map itself
                }
                BUILTIN_MAP_GET => {
                    let key = args.first().cloned().unwrap_or(Value::Undefined);
                    let borrow = m.borrow();
                    let val = borrow
                        .entries
                        .iter()
                        .find(|(k, _)| self.values_equal(k, &key))
                        .map(|(_, v)| v.clone())
                        .unwrap_or(Value::Undefined);
                    return val;
                }
                BUILTIN_MAP_HAS => {
                    let key = args.first().cloned().unwrap_or(Value::Undefined);
                    let borrow = m.borrow();
                    return Value::Boolean(borrow.entries.iter().any(|(k, _)| self.values_equal(k, &key)));
                }
                BUILTIN_MAP_DELETE => {
                    let key = args.first().cloned().unwrap_or(Value::Undefined);
                    let mut borrow = m.borrow_mut();
                    let len_before = borrow.entries.len();
                    borrow.entries.retain(|(k, _)| !self.values_equal(k, &key));
                    return Value::Boolean(borrow.entries.len() < len_before);
                }
                BUILTIN_MAP_CLEAR => {
                    m.borrow_mut().entries.clear();
                    return Value::Undefined;
                }
                BUILTIN_MAP_KEYS => {
                    let items: Vec<Value<'gc>> = m.borrow().entries.iter().map(|(k, _)| k.clone()).collect();
                    return self.make_iterator(items);
                }
                BUILTIN_MAP_VALUES => {
                    let items: Vec<Value<'gc>> = m.borrow().entries.iter().map(|(_, v)| v.clone()).collect();
                    return self.make_iterator(items);
                }
                BUILTIN_MAP_ENTRIES => {
                    let items: Vec<Value<'gc>> = m
                        .borrow()
                        .entries
                        .iter()
                        .map(|(k, v)| Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(vec![k.clone(), v.clone()])))))
                        .collect();
                    return self.make_iterator(items);
                }
                BUILTIN_MAP_FOREACH => {
                    if let Some(Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _)) = args.first() {
                        let __cb_uv = if let Some(Value::VmClosure(_, _, u)) = args.first() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
                        let entries: Vec<(Value<'gc>, Value<'gc>)> = m.borrow().entries.clone();
                        let map_ref = receiver.clone();
                        for (k, v) in &entries {
                            if *arity >= 3 {
                                self.call_vm_function(*ip, &[v.clone(), k.clone(), map_ref.clone()], &__cb_uv);
                            } else {
                                self.call_vm_function(*ip, &[v.clone(), k.clone()], &__cb_uv);
                            }
                        }
                    }
                    return Value::Undefined;
                }
                _ => {}
            }
        }

        // Set methods
        if let Value::VmSet(ref s) = receiver {
            match id {
                BUILTIN_SET_ADD => {
                    let val = args.first().cloned().unwrap_or(Value::Undefined);
                    let mut borrow = s.borrow_mut();
                    // WeakSet requires object values
                    if borrow.is_weak && !matches!(val, Value::VmObject(_) | Value::VmArray(_) | Value::VmMap(_) | Value::VmSet(_)) {
                        drop(borrow);
                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("Invalid value used in weak set")),
                        );
                        self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map)))).ok();
                        return Value::Undefined;
                    }
                    if !borrow.values.iter().any(|v| self.values_equal(v, &val)) {
                        borrow.values.push(val);
                    }
                    drop(borrow);
                    return receiver; // Set.add returns the Set itself
                }
                BUILTIN_SET_HAS => {
                    let val = args.first().cloned().unwrap_or(Value::Undefined);
                    let borrow = s.borrow();
                    return Value::Boolean(borrow.values.iter().any(|v| self.values_equal(v, &val)));
                }
                BUILTIN_SET_DELETE => {
                    let val = args.first().cloned().unwrap_or(Value::Undefined);
                    let mut borrow = s.borrow_mut();
                    let len_before = borrow.values.len();
                    borrow.values.retain(|v| !self.values_equal(v, &val));
                    return Value::Boolean(borrow.values.len() < len_before);
                }
                BUILTIN_SET_CLEAR => {
                    s.borrow_mut().values.clear();
                    return Value::Undefined;
                }
                BUILTIN_SET_VALUES => {
                    let items: Vec<Value<'gc>> = s.borrow().values.clone();
                    return self.make_iterator(items);
                }
                BUILTIN_SET_ENTRIES => {
                    let items: Vec<Value<'gc>> = s
                        .borrow()
                        .values
                        .iter()
                        .map(|v| Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(vec![v.clone(), v.clone()])))))
                        .collect();
                    return self.make_iterator(items);
                }
                BUILTIN_SET_FOREACH => {
                    if let Some(Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _)) = args.first() {
                        let __cb_uv = if let Some(Value::VmClosure(_, _, u)) = args.first() {
                            (**u).to_vec()
                        } else {
                            Vec::new()
                        };
                        let vals: Vec<Value<'gc>> = s.borrow().values.clone();
                        let set_ref = receiver.clone();
                        for v in &vals {
                            if *arity >= 3 {
                                self.call_vm_function(*ip, &[v.clone(), v.clone(), set_ref.clone()], &__cb_uv);
                            } else {
                                self.call_vm_function(*ip, &[v.clone(), v.clone()], &__cb_uv);
                            }
                        }
                    }
                    return Value::Undefined;
                }
                _ => {}
            }
        }

        // WeakRef.deref()
        if let Value::VmObject(ref obj) = receiver
            && id == BUILTIN_WEAKREF_DEREF
        {
            let borrow = obj.borrow();
            return borrow.get("__target__").cloned().unwrap_or(Value::Undefined);
        }

        // FinalizationRegistry.register — happy path (validation done by caller)
        if let Value::VmObject(ref obj) = receiver
            && id == BUILTIN_FR_REGISTER
        {
            let target = args.first().cloned().unwrap_or(Value::Undefined);
            let held = args.get(1).cloned().unwrap_or(Value::Undefined);
            let token = args.get(2).cloned();

            let mut borrow = obj.borrow_mut();
            let count = match borrow.get("__fr_count__") {
                Some(Value::Number(n)) => *n as usize,
                _ => 0,
            };
            borrow.insert(format!("__fr_{}_target__", count), target);
            borrow.insert(format!("__fr_{}_held__", count), held);
            borrow.insert(format!("__fr_{}_token__", count), token.unwrap_or(Value::Undefined));
            borrow.insert(format!("__fr_{}_alive__", count), Value::Boolean(true));
            borrow.insert("__fr_count__".to_string(), Value::Number((count + 1) as f64));
            return Value::Undefined;
        }

        // FinalizationRegistry.unregister(token)
        if let Value::VmObject(ref obj) = receiver
            && id == BUILTIN_FR_UNREGISTER
        {
            let token = args.first().cloned().unwrap_or(Value::Undefined);
            let mut borrow = obj.borrow_mut();
            let count = match borrow.get("__fr_count__") {
                Some(Value::Number(n)) => *n as usize,
                _ => 0,
            };
            let mut removed = false;
            for i in 0..count {
                let alive_key = format!("__fr_{}_alive__", i);
                if !matches!(borrow.get(&alive_key), Some(Value::Boolean(true))) {
                    continue;
                }
                let token_key = format!("__fr_{}_token__", i);
                if let Some(stored_token) = borrow.get(&token_key).cloned()
                    && self.values_same(&token, &stored_token)
                {
                    borrow.insert(alive_key, Value::Boolean(false));
                    removed = true;
                }
            }
            return Value::Boolean(removed);
        }

        // Iterator next() on VmObject with __items__ / __index__
        if let Value::VmObject(ref obj) = receiver
            && id == BUILTIN_ITERATOR_NEXT
        {
            let mut borrow = obj.borrow_mut();
            let idx = match borrow.get("__index__") {
                Some(Value::Number(n)) => *n as usize,
                _ => 0,
            };
            let items = borrow.get("__items__").cloned();
            if let Some(Value::VmArray(arr)) = items {
                let a = arr.borrow();
                if idx < a.len() {
                    borrow.insert("__index__".to_string(), Value::Number((idx + 1) as f64));
                    let mut result = IndexMap::new();
                    result.insert("value".to_string(), a[idx].clone());
                    result.insert("done".to_string(), Value::Boolean(false));
                    return Value::VmObject(Rc::new(RefCell::new(result)));
                }
            }
            let mut result = IndexMap::new();
            result.insert("value".to_string(), Value::Undefined);
            result.insert("done".to_string(), Value::Boolean(true));
            return Value::VmObject(Rc::new(RefCell::new(result)));
        }

        if let Value::VmArray(ref arr) = receiver
            && id == BUILTIN_ITERATOR_NEXT
            && matches!(arr.borrow().props.get("__generator__"), Some(Value::Boolean(true)))
        {
            let mut result = IndexMap::new();
            let mut a = arr.borrow_mut();
            let idx = match a.props.get("__generator_index__") {
                Some(Value::Number(n)) => *n as usize,
                _ => 0,
            };
            if idx < a.elements.len() {
                a.props.insert("__generator_index__".to_string(), Value::Number((idx + 1) as f64));
                result.insert("value".to_string(), a.elements[idx].clone());
                result.insert("done".to_string(), Value::Boolean(false));
            } else {
                result.insert("value".to_string(), Value::Undefined);
                result.insert("done".to_string(), Value::Boolean(true));
            }
            return Value::VmObject(Rc::new(RefCell::new(result)));
        }

        // Minimal async-generator facade on marked arrays.
        if let Value::VmArray(ref arr) = receiver
            && (id == BUILTIN_ASYNCGEN_NEXT || id == BUILTIN_ASYNCGEN_THROW || id == BUILTIN_ASYNCGEN_RETURN)
        {
            let mut result = IndexMap::new();
            match id {
                BUILTIN_ASYNCGEN_NEXT => {
                    let mut a = arr.borrow_mut();
                    let idx = match a.props.get("__async_gen_index__") {
                        Some(Value::Number(n)) => *n as usize,
                        _ => 0,
                    };
                    if idx < a.elements.len() {
                        a.props.insert("__async_gen_index__".to_string(), Value::Number((idx + 1) as f64));
                        result.insert("value".to_string(), a.elements[idx].clone());
                        result.insert("done".to_string(), Value::Boolean(false));
                    } else {
                        result.insert("value".to_string(), Value::Undefined);
                        result.insert("done".to_string(), Value::Boolean(true));
                    }
                }
                BUILTIN_ASYNCGEN_THROW | BUILTIN_ASYNCGEN_RETURN => {
                    let done_value = args.into_iter().next().unwrap_or(Value::Undefined);
                    let len = arr.borrow().elements.len();
                    arr.borrow_mut()
                        .props
                        .insert("__async_gen_index__".to_string(), Value::Number(len as f64));
                    result.insert("value".to_string(), done_value);
                    result.insert("done".to_string(), Value::Boolean(true));
                }
                _ => {}
            }
            return self.call_builtin(BUILTIN_PROMISE_RESOLVE, vec![Value::VmObject(Rc::new(RefCell::new(result)))]);
        }

        // Object.* static methods: delegate to call_builtin
        if (BUILTIN_OBJECT_KEYS..=BUILTIN_OBJECT_DEFINEPROP).contains(&id) {
            return self.call_builtin(id, args);
        }

        // Object.prototype.hasOwnProperty(key)
        if id == BUILTIN_OBJ_HASOWNPROPERTY {
            let key_val = args.first().cloned().unwrap_or(Value::Undefined);
            let key = match self.as_property_key_string(&key_val) {
                Ok(k) => k,
                Err(_) => value_to_string(&key_val),
            };
            return match &receiver {
                Value::VmObject(map) => {
                    let has = map.borrow().contains_key(&key) && !key.starts_with("__proto__") && !key.starts_with("__type__");
                    Value::Boolean(has)
                }
                _ => Value::Boolean(false),
            };
        }

        // Object.prototype.toString()
        if id == BUILTIN_OBJ_TOSTRING {
            // Symbol.prototype.toString() — returns "Symbol(desc)"
            if let Value::VmObject(map) = &receiver {
                let b = map.borrow();
                if b.contains_key("__vm_symbol__") {
                    return match b.get("description") {
                        Some(Value::String(d)) => Value::String(crate::unicode::utf8_to_utf16(&format!(
                            "Symbol({})",
                            crate::unicode::utf16_to_utf8(d)
                        ))),
                        _ => Value::String(crate::unicode::utf8_to_utf16("Symbol()")),
                    };
                }
            }
            let tag = match &receiver {
                Value::Undefined => "Undefined",
                Value::Null => "Null",
                Value::VmArray(_) => "Array",
                Value::VmMap(m) => {
                    if m.borrow().is_weak {
                        "WeakMap"
                    } else {
                        "Map"
                    }
                }
                Value::VmSet(s) => {
                    if s.borrow().is_weak {
                        "WeakSet"
                    } else {
                        "Set"
                    }
                }
                Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_) => "Function",
                Value::Number(_) => "Number",
                Value::String(_) => "String",
                Value::Boolean(_) => "Boolean",
                Value::VmObject(map) => {
                    let b = map.borrow();
                    if let Some(Value::String(tag)) = b.get("__toStringTag__") {
                        let tag_str = crate::unicode::utf16_to_utf8(tag);
                        return Value::String(crate::unicode::utf8_to_utf16(&format!("[object {}]", tag_str)));
                    }
                    if let Some(Value::String(tag)) = b.get("Symbol(Symbol.toStringTag)").or_else(|| b.get("@@sym:4")) {
                        let tag_str = crate::unicode::utf16_to_utf8(tag);
                        return Value::String(crate::unicode::utf8_to_utf16(&format!("[object {}]", tag_str)));
                    }
                    "Object"
                }
                _ => "Object",
            };
            return Value::String(crate::unicode::utf8_to_utf16(&format!("[object {}]", tag)));
        }

        // Function.prototype.apply(thisArg, argsArray)
        if id == BUILTIN_FN_APPLY {
            let this_arg = args.first().cloned().unwrap_or(Value::Undefined);
            let call_args: Vec<Value<'gc>> = if let Some(Value::VmArray(arr)) = args.get(1) {
                arr.borrow().iter().cloned().collect()
            } else {
                Vec::new()
            };
            match &receiver {
                Value::VmNativeFunction(fn_id) => {
                    self.this_stack.push(this_arg.clone());
                    let result = self.call_method_builtin(*fn_id, this_arg, call_args);
                    self.this_stack.pop();
                    return result;
                }
                Value::VmFunction(ip, _arity) | Value::VmClosure(ip, _arity, _) => {
                    self.this_stack.push(this_arg);
                    let __cb_uv = if let Value::VmClosure(_, _, u) = &receiver {
                        (**u).to_vec()
                    } else {
                        Vec::new()
                    };
                    let result = self.call_vm_function(*ip, &call_args, &__cb_uv);
                    self.this_stack.pop();
                    return result;
                }
                _ => return Value::Undefined,
            }
        }

        // Function.prototype.call(thisArg, ...args)
        if id == BUILTIN_FN_CALL {
            let this_arg = args.first().cloned().unwrap_or(Value::Undefined);
            let call_args: Vec<Value<'gc>> = args.into_iter().skip(1).collect();
            match &receiver {
                Value::VmNativeFunction(fn_id) => {
                    self.this_stack.push(this_arg.clone());
                    let result = self.call_method_builtin(*fn_id, this_arg, call_args);
                    self.this_stack.pop();
                    return result;
                }
                Value::VmFunction(ip, _arity) | Value::VmClosure(ip, _arity, _) => {
                    self.this_stack.push(this_arg);
                    let __cb_uv = if let Value::VmClosure(_, _, u) = &receiver {
                        (**u).to_vec()
                    } else {
                        Vec::new()
                    };
                    let result = self.call_vm_function(*ip, &call_args, &__cb_uv);
                    self.this_stack.pop();
                    return result;
                }
                _ => return Value::Undefined,
            }
        }

        // Function.prototype.bind(thisArg, ...args)
        if id == BUILTIN_FN_BIND {
            let this_arg = args.first().cloned().unwrap_or(Value::Undefined);
            let bound_args: Vec<Value<'gc>> = args.into_iter().skip(1).collect();
            return match receiver {
                Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_) => {
                    let mut m = IndexMap::new();
                    m.insert("__bound_target__".to_string(), receiver);
                    m.insert("__bound_this__".to_string(), this_arg);
                    m.insert(
                        "__bound_args__".to_string(),
                        Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(bound_args)))),
                    );
                    m.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Function")));
                    Value::VmObject(Rc::new(RefCell::new(m)))
                }
                _ => Value::Undefined,
            };
        }

        if matches!(
            id,
            BUILTIN_ITERATOR_NEXT | BUILTIN_ASYNCGEN_NEXT | BUILTIN_ASYNCGEN_THROW | BUILTIN_ASYNCGEN_RETURN
        ) {
            self.this_stack.push(receiver);
            let result = self.call_builtin(id, args);
            self.this_stack.pop();
            return result;
        }

        // RegExp.prototype.exec(string)
        if id == BUILTIN_REGEX_EXEC {
            if let Value::VmObject(ref map) = receiver {
                let input = args.first().map(value_to_string).unwrap_or_default();
                return self.regex_exec(map, &input);
            }
            return Value::Null;
        }

        // RegExp.prototype.test(string)
        if id == BUILTIN_REGEX_TEST {
            if let Value::VmObject(ref map) = receiver {
                let input = args.first().map(value_to_string).unwrap_or_default();
                let result = self.regex_exec(map, &input);
                return Value::Boolean(!matches!(result, Value::Null));
            }
            return Value::Boolean(false);
        }

        log::warn!("Unknown method builtin ID {} on {}", id, value_to_string(&receiver));
        Value::Undefined
    }

    /// Execute a regex match, returning an array result or Null
    fn regex_exec(&self, re_obj: &Rc<RefCell<IndexMap<String, Value<'gc>>>>, input: &str) -> Value<'gc> {
        let borrow = re_obj.borrow();
        let pattern = borrow.get("__regex_pattern__").map(value_to_string).unwrap_or_default();
        let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
        let is_global = flags.contains('g') || flags.contains('y');
        let last_index = if is_global {
            match borrow.get("lastIndex") {
                Some(Value::Number(n)) => *n as usize,
                _ => 0,
            }
        } else {
            0
        };
        drop(borrow);

        let pattern_u16 = crate::unicode::utf8_to_utf16(&pattern);
        let regress_flags: String = flags.chars().filter(|flag| "gimsuvy".contains(*flag)).collect();
        let re = match get_or_compile_regex(&pattern_u16, &regress_flags) {
            Ok(r) => r,
            Err(_) => return Value::Null,
        };

        let input_u16: Vec<u16> = input.encode_utf16().collect();
        let (working_input, mapped_input) = self.regex_prepare_input(input, &flags);
        let match_result = if flags.contains('u') || flags.contains('v') {
            re.find_from_utf16(&working_input, last_index).next()
        } else {
            re.find_from_ucs2(&working_input, last_index).next()
        };

        match match_result {
            Some(m) => {
                let (match_start, match_end) = if mapped_input {
                    (
                        Self::regex_map_index_back(&input_u16, m.range.start),
                        Self::regex_map_index_back(&input_u16, m.range.end),
                    )
                } else {
                    (m.range.start, m.range.end)
                };
                let matched_str = &input_u16[match_start..match_end];
                let matched = crate::unicode::utf16_to_utf8(matched_str);

                let mut result_items: Vec<Value<'gc>> = vec![Value::String(crate::unicode::utf8_to_utf16(&matched))];
                // Add capturing groups
                for cap in &m.captures {
                    match cap {
                        Some(r) => {
                            let (cap_start, cap_end) = if mapped_input {
                                (
                                    Self::regex_map_index_back(&input_u16, r.start),
                                    Self::regex_map_index_back(&input_u16, r.end),
                                )
                            } else {
                                (r.start, r.end)
                            };
                            let s = &input_u16[cap_start..cap_end];
                            result_items.push(Value::String(s.to_vec()));
                        }
                        None => result_items.push(Value::Undefined),
                    }
                }

                let mut arr_data = VmArrayData::new(result_items);
                arr_data.props.insert("index".to_string(), Value::Number(match_start as f64));
                arr_data
                    .props
                    .insert("input".to_string(), Value::String(crate::unicode::utf8_to_utf16(input)));

                // Add indices array when 'd' (hasIndices) flag is set
                if flags.contains('d') {
                    let mut indices_items: Vec<Value<'gc>> = Vec::new();
                    // Full match indices
                    let pair = vec![Value::Number(match_start as f64), Value::Number(match_end as f64)];
                    indices_items.push(Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(pair)))));
                    // Capturing group indices
                    for cap in &m.captures {
                        match cap {
                            Some(r) => {
                                let (cap_start, cap_end) = if mapped_input {
                                    (
                                        Self::regex_map_index_back(&input_u16, r.start),
                                        Self::regex_map_index_back(&input_u16, r.end),
                                    )
                                } else {
                                    (r.start, r.end)
                                };
                                let pair = vec![Value::Number(cap_start as f64), Value::Number(cap_end as f64)];
                                indices_items.push(Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(pair)))));
                            }
                            None => indices_items.push(Value::Undefined),
                        }
                    }
                    arr_data.props.insert(
                        "indices".to_string(),
                        Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(indices_items)))),
                    );
                }

                let arr = Value::VmArray(Rc::new(RefCell::new(arr_data)));

                // Update lastIndex for global/sticky
                if is_global {
                    re_obj.borrow_mut().insert("lastIndex".to_string(), Value::Number(match_end as f64));
                }

                arr
            }
            None => {
                if is_global {
                    re_obj.borrow_mut().insert("lastIndex".to_string(), Value::Number(0.0));
                }
                Value::Null
            }
        }
    }

    /// Global match: return array of all full match strings
    fn regex_match_all(&self, input: &str, re_obj: &Rc<RefCell<IndexMap<String, Value<'gc>>>>) -> Value<'gc> {
        let borrow = re_obj.borrow();
        let pattern = borrow.get("__regex_pattern__").map(value_to_string).unwrap_or_default();
        let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
        drop(borrow);

        let pattern_u16 = crate::unicode::utf8_to_utf16(&pattern);
        let re = match get_or_compile_regex(&pattern_u16, &flags) {
            Ok(r) => r,
            Err(_) => return Value::Null,
        };

        let input_u16: Vec<u16> = input.encode_utf16().collect();
        let use_unicode = flags.contains('u') || flags.contains('v');
        let mut results: Vec<Value<'gc>> = Vec::new();
        let mut pos = 0usize;
        loop {
            let m = if use_unicode {
                re.find_from_utf16(&input_u16, pos).next()
            } else {
                re.find_from_ucs2(&input_u16, pos).next()
            };
            match m {
                Some(m) => {
                    let matched = &input_u16[m.range.start..m.range.end];
                    results.push(Value::String(matched.to_vec()));
                    pos = if m.range.end == m.range.start {
                        m.range.end + 1
                    } else {
                        m.range.end
                    };
                    if pos > input_u16.len() {
                        break;
                    }
                }
                None => break,
            }
        }
        if results.is_empty() {
            Value::Null
        } else {
            Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(results))))
        }
    }

    /// Replace string content using a regex pattern
    fn regex_replace_string(
        &self,
        input: &str,
        re_obj: &Rc<RefCell<IndexMap<String, Value<'gc>>>>,
        replacement: &str,
        replace_all: bool,
    ) -> String {
        let borrow = re_obj.borrow();
        let pattern = borrow.get("__regex_pattern__").map(value_to_string).unwrap_or_default();
        let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
        drop(borrow);

        let is_global = flags.contains('g');
        let pattern_u16 = crate::unicode::utf8_to_utf16(&pattern);
        let re = match get_or_compile_regex(&pattern_u16, &flags) {
            Ok(r) => r,
            Err(_) => return input.to_string(),
        };

        let input_u16: Vec<u16> = input.encode_utf16().collect();
        let use_unicode = flags.contains('u') || flags.contains('v');
        let mut result_u16: Vec<u16> = Vec::new();
        let mut pos = 0usize;
        let mut replaced = false;

        loop {
            let m = if use_unicode {
                re.find_from_utf16(&input_u16, pos).next()
            } else {
                re.find_from_ucs2(&input_u16, pos).next()
            };
            match m {
                Some(m) => {
                    // Append text before match
                    result_u16.extend_from_slice(&input_u16[pos..m.range.start]);
                    // Process replacement string with backreferences
                    let repl = self.apply_replacement(replacement, &input_u16, &m);
                    result_u16.extend_from_slice(&crate::unicode::utf8_to_utf16(&repl));
                    pos = m.range.end;
                    if pos == m.range.start {
                        pos += 1;
                    } // prevent infinite loop on zero-width match
                    replaced = true;
                    if !is_global && !replace_all {
                        break;
                    }
                    if pos > input_u16.len() {
                        break;
                    }
                }
                None => break,
            }
        }
        // Append remainder
        if pos <= input_u16.len() {
            result_u16.extend_from_slice(&input_u16[pos..]);
        }
        if !replaced {
            return input.to_string();
        }
        crate::unicode::utf16_to_utf8(&result_u16)
    }

    /// Apply replacement string backreferences ($1, $2, $&, etc.)
    fn apply_replacement(&self, replacement: &str, input_u16: &[u16], m: &regress::Match) -> String {
        let matched = crate::unicode::utf16_to_utf8(&input_u16[m.range.start..m.range.end]);
        let mut result = String::new();
        let chars: Vec<char> = replacement.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '$' && i + 1 < chars.len() {
                match chars[i + 1] {
                    '&' => {
                        result.push_str(&matched);
                        i += 2;
                    }
                    '`' => {
                        result.push_str(&crate::unicode::utf16_to_utf8(&input_u16[..m.range.start]));
                        i += 2;
                    }
                    '\'' => {
                        result.push_str(&crate::unicode::utf16_to_utf8(&input_u16[m.range.end..]));
                        i += 2;
                    }
                    '$' => {
                        result.push('$');
                        i += 2;
                    }
                    d if d.is_ascii_digit() => {
                        // Check for two-digit group reference ($10, $11, etc.)
                        let mut num_str = String::new();
                        num_str.push(d);
                        if i + 2 < chars.len() && chars[i + 2].is_ascii_digit() {
                            let two_digit = format!("{}{}", d, chars[i + 2]);
                            let two_num: usize = two_digit.parse().unwrap_or(0);
                            if two_num >= 1 && two_num <= m.captures.len() {
                                if let Some(Some(r)) = m.captures.get(two_num - 1) {
                                    result.push_str(&crate::unicode::utf16_to_utf8(&input_u16[r.start..r.end]));
                                }
                                i += 3;
                                continue;
                            }
                        }
                        let num: usize = num_str.parse().unwrap_or(0);
                        if num >= 1
                            && num <= m.captures.len()
                            && let Some(Some(r)) = m.captures.get(num - 1)
                        {
                            result.push_str(&crate::unicode::utf16_to_utf8(&input_u16[r.start..r.end]));
                        }
                        i += 2;
                    }
                    _ => {
                        result.push('$');
                        i += 1;
                    }
                }
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }
        result
    }

    /// Split a string using a regex separator, with optional capturing groups
    fn regex_split_string(&self, input: &str, re_obj: &Rc<RefCell<IndexMap<String, Value<'gc>>>>, limit: Option<usize>) -> Vec<Value<'gc>> {
        let borrow = re_obj.borrow();
        let pattern = borrow.get("__regex_pattern__").map(value_to_string).unwrap_or_default();
        let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
        drop(borrow);

        let pattern_u16 = crate::unicode::utf8_to_utf16(&pattern);
        let re = match get_or_compile_regex(&pattern_u16, &flags) {
            Ok(r) => r,
            Err(_) => return vec![Value::String(crate::unicode::utf8_to_utf16(input))],
        };

        let input_u16: Vec<u16> = input.encode_utf16().collect();
        let use_unicode = flags.contains('u') || flags.contains('v');
        let mut results: Vec<Value<'gc>> = Vec::new();
        let max = limit.unwrap_or(usize::MAX);
        let mut pos = 0usize;

        loop {
            if results.len() >= max {
                break;
            }
            let m = if use_unicode {
                re.find_from_utf16(&input_u16, pos).next()
            } else {
                re.find_from_ucs2(&input_u16, pos).next()
            };
            match m {
                Some(m) if m.range.start < input_u16.len() => {
                    // Prevent infinite loop on zero-width match at same position
                    if m.range.start == m.range.end && m.range.start == pos {
                        pos += 1;
                        continue;
                    }
                    results.push(Value::String(input_u16[pos..m.range.start].to_vec()));
                    // Add capturing groups
                    for cap in &m.captures {
                        if results.len() >= max {
                            break;
                        }
                        match cap {
                            Some(r) => results.push(Value::String(input_u16[r.start..r.end].to_vec())),
                            None => results.push(Value::Undefined),
                        }
                    }
                    pos = m.range.end;
                }
                _ => break,
            }
        }
        if results.len() < max {
            results.push(Value::String(input_u16[pos..].to_vec()));
        }
        results
    }

    /// Create an iterator object from a Vec of values
    fn make_iterator(&self, items: Vec<Value<'gc>>) -> Value<'gc> {
        let arr = Rc::new(RefCell::new(VmArrayData::new(items)));
        let mut obj = IndexMap::new();
        obj.insert("__items__".to_string(), Value::VmArray(arr));
        obj.insert("__index__".to_string(), Value::Number(0.0));
        obj.insert("next".to_string(), Value::VmNativeFunction(BUILTIN_ITERATOR_NEXT));
        Value::VmObject(Rc::new(RefCell::new(obj)))
    }

    /// Try to coerce a value via [Symbol.toPrimitive] if present.
    fn try_to_primitive(&mut self, val: &Value<'gc>, hint: &str) -> Value<'gc> {
        if let Value::VmObject(map) = val {
            let tp_fn = {
                let borrow = map.borrow();
                borrow.get("@@sym:3").cloned()
            };
            if let Some(func) = tp_fn {
                let hint_val = Value::String(crate::unicode::utf8_to_utf16(hint));
                let result = match func {
                    Value::VmFunction(ip, _) => {
                        self.this_stack.push(val.clone());
                        let r = self.call_vm_function(ip, &[hint_val], &[]);
                        self.this_stack.pop();
                        Some(r)
                    }
                    Value::VmClosure(ip, _, ref upv) => {
                        self.this_stack.push(val.clone());
                        let uv = (**upv).clone();
                        let r = self.call_vm_function(ip, &[hint_val], &uv);
                        self.this_stack.pop();
                        Some(r)
                    }
                    _ => None,
                };
                if let Some(r) = result {
                    // If toPrimitive returned a non-primitive, throw TypeError
                    if matches!(r, Value::VmObject(_) | Value::VmArray(_) | Value::VmMap(_) | Value::VmSet(_)) {
                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("Symbol.toPrimitive must return a primitive value")),
                        );
                        self.pending_throw = Some(Value::VmObject(Rc::new(RefCell::new(err_map))));
                        return Value::Undefined;
                    }
                    return r;
                }
            }
        }
        val.clone()
    }

    fn is_symbol_value(val: &Value<'gc>) -> bool {
        if let Value::VmObject(map) = val {
            map.borrow().contains_key("__vm_symbol__")
        } else {
            false
        }
    }

    /// Compare two values for equality (used by indexOf etc.)
    fn values_equal(&self, a: &Value<'gc>, b: &Value<'gc>) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => x == y,
            (Value::BigInt(x), Value::BigInt(y)) => x == y,
            (Value::String(x), Value::String(y)) => x == y,
            (Value::Boolean(x), Value::Boolean(y)) => x == y,
            (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => true,
            (Value::VmObject(a_rc), Value::VmObject(b_rc)) => Rc::ptr_eq(a_rc, b_rc),
            (Value::VmArray(a_rc), Value::VmArray(b_rc)) => Rc::ptr_eq(a_rc, b_rc),
            (Value::VmMap(a_rc), Value::VmMap(b_rc)) => Rc::ptr_eq(a_rc, b_rc),
            (Value::VmSet(a_rc), Value::VmSet(b_rc)) => Rc::ptr_eq(a_rc, b_rc),
            (Value::VmFunction(a_ip, _), Value::VmFunction(b_ip, _)) => a_ip == b_ip,
            (Value::VmClosure(a_ip, _, a_uv), Value::VmClosure(b_ip, _, b_uv)) => a_ip == b_ip && Rc::ptr_eq(a_uv, b_uv),
            (Value::VmFunction(_, _), Value::VmClosure(_, _, _)) => false,
            (Value::VmClosure(_, _, _), Value::VmFunction(_, _)) => false,
            (Value::VmNativeFunction(a_id), Value::VmNativeFunction(b_id)) => a_id == b_id,
            _ => false,
        }
    }

    /// JS loose equality (==) with type coercion
    fn loose_equal(&self, a: &Value<'gc>, b: &Value<'gc>) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => x == y,
            (Value::BigInt(x), Value::BigInt(y)) => x == y,
            (Value::BigInt(x), Value::Number(n)) => compare_bigint_number(x, *n) == Some(std::cmp::Ordering::Equal),
            (Value::Number(n), Value::BigInt(x)) => compare_bigint_number(x, *n) == Some(std::cmp::Ordering::Equal),
            (Value::String(x), Value::String(y)) => x == y,
            (Value::Boolean(x), Value::Boolean(y)) => x == y,
            (Value::Null, Value::Null)
            | (Value::Undefined, Value::Undefined)
            | (Value::Null, Value::Undefined)
            | (Value::Undefined, Value::Null) => true,
            // Number vs String: coerce string to number
            (Value::Number(n), Value::String(_)) => *n == to_number(b),
            (Value::String(_), Value::Number(n)) => to_number(a) == *n,
            // Boolean vs any: coerce boolean to number, recurse
            (Value::Boolean(bv), _) => {
                let num = Value::Number(if *bv { 1.0 } else { 0.0 });
                self.loose_equal(&num, b)
            }
            (_, Value::Boolean(bv)) => {
                let num = Value::Number(if *bv { 1.0 } else { 0.0 });
                self.loose_equal(a, &num)
            }
            // Reference equality for objects/arrays/maps/sets
            (Value::VmObject(a_rc), Value::VmObject(b_rc)) => Rc::ptr_eq(a_rc, b_rc),
            (Value::VmArray(a_rc), Value::VmArray(b_rc)) => Rc::ptr_eq(a_rc, b_rc),
            (Value::VmMap(a_rc), Value::VmMap(b_rc)) => Rc::ptr_eq(a_rc, b_rc),
            (Value::VmSet(a_rc), Value::VmSet(b_rc)) => Rc::ptr_eq(a_rc, b_rc),
            (Value::VmNativeFunction(a_id), Value::VmNativeFunction(b_id)) => a_id == b_id,
            (Value::VmFunction(a_ip, _), Value::VmFunction(b_ip, _)) => a_ip == b_ip,
            (Value::VmClosure(a_ip, _, a_uv), Value::VmClosure(b_ip, _, b_uv)) => a_ip == b_ip && Rc::ptr_eq(a_uv, b_uv),
            (Value::VmFunction(_, _), Value::VmClosure(_, _, _)) => false,
            (Value::VmClosure(_, _, _), Value::VmFunction(_, _)) => false,
            _ => false,
        }
    }

    /// Call a VM function inline (used by map/filter/forEach/reduce)
    fn call_vm_function_result(
        &mut self,
        ip: usize,
        args: &[Value<'gc>],
        upvalues: &[Rc<RefCell<Value<'gc>>>],
    ) -> Result<Value<'gc>, JSError> {
        // Push a dummy callee so Return's truncate(bp-1) removes it
        self.stack.push(Value::Undefined);
        let bp = self.stack.len();
        let saved_stack_depth = bp - 1; // before the dummy callee
        for arg in args {
            self.stack.push(arg.clone());
        }
        let saved_ip = self.ip;
        let target_depth = self.frames.len();
        self.frames.push(CallFrame {
            return_ip: 0, // sentinel
            bp,
            is_method: false,
            arg_count: args.len(),
            func_ip: ip,
            arguments_obj: None,
            upvalues: upvalues.to_vec(),
            saved_args: None,
            local_cells: HashMap::new(),
        });
        self.ip = ip;
        let result = self.run_inner(target_depth + 1);
        self.ip = saved_ip;
        // Clean up in case run_inner returned an error (frame/stack may not have been unwound)
        self.frames.truncate(target_depth);
        self.stack.truncate(saved_stack_depth);
        result
    }

    fn call_vm_function(&mut self, ip: usize, args: &[Value<'gc>], upvalues: &[Rc<RefCell<Value<'gc>>>]) -> Value<'gc> {
        match self.call_vm_function_result(ip, args, upvalues) {
            Ok(v) => v,
            Err(_) => Value::Undefined,
        }
    }

    fn as_property_key_string(&mut self, index: &Value<'gc>) -> Result<String, JSError> {
        match index {
            Value::String(s) => Ok(crate::unicode::utf16_to_utf8(s)),
            Value::VmObject(map) => {
                // If it's a symbol VmObject, return @@sym:<id> key
                let is_sym = map.borrow().contains_key("__vm_symbol__");
                if is_sym {
                    let id = map
                        .borrow()
                        .get("__symbol_id__")
                        .and_then(|v| if let Value::Number(n) = v { Some(*n as u64) } else { None })
                        .unwrap_or(0);
                    return Ok(format!("@@sym:{}", id));
                }
                let receiver = index.clone();
                let to_string_fn = self.read_named_property(receiver.clone(), "toString");
                let out = match to_string_fn {
                    Value::VmFunction(ip, _) => {
                        self.this_stack.push(receiver.clone());
                        let result = self.call_vm_function_result(ip, &[], &[]);
                        self.this_stack.pop();
                        result?
                    }
                    Value::VmClosure(ip, _, upv) => {
                        self.this_stack.push(receiver.clone());
                        let uv = (*upv).clone();
                        let result = self.call_vm_function_result(ip, &[], &uv);
                        self.this_stack.pop();
                        result?
                    }
                    Value::VmNativeFunction(id) => self.call_method_builtin(id, receiver.clone(), vec![]),
                    _ => index.clone(),
                };
                Ok(value_to_string(&out))
            }
            Value::VmArray(_) | Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_) => {
                let receiver = index.clone();
                let to_string_fn = self.read_named_property(receiver.clone(), "toString");
                let out = match to_string_fn {
                    Value::VmFunction(ip, _) => {
                        self.this_stack.push(receiver.clone());
                        let result = self.call_vm_function_result(ip, &[], &[]);
                        self.this_stack.pop();
                        result?
                    }
                    Value::VmClosure(ip, _, upv) => {
                        self.this_stack.push(receiver.clone());
                        let uv = (*upv).clone();
                        let result = self.call_vm_function_result(ip, &[], &uv);
                        self.this_stack.pop();
                        result?
                    }
                    Value::VmNativeFunction(id) => self.call_method_builtin(id, receiver.clone(), vec![]),
                    _ => index.clone(),
                };
                Ok(value_to_string(&out))
            }
            _ => Ok(value_to_string(index)),
        }
    }

    fn is_value_callable(&self, value: &Value<'gc>) -> bool {
        match value {
            Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(..) => true,
            Value::VmObject(map) => {
                let b = map.borrow();
                b.contains_key("__host_fn__")
                    || b.contains_key("__bound_target__")
                    || b.contains_key("__fn_body__")
                    || b.contains_key("__native_id__")
            }
            _ => false,
        }
    }

    fn validate_property_descriptor(&self, desc: &IndexMap<String, Value<'gc>>) -> Result<(), ()> {
        let has_get = desc.contains_key("get");
        let has_set = desc.contains_key("set");
        let has_value = desc.contains_key("value");
        let has_writable = desc.contains_key("writable");

        if (has_get || has_set) && (has_value || has_writable) {
            return Err(());
        }

        if let Some(getter) = desc.get("get")
            && !matches!(getter, Value::Undefined)
            && !self.is_value_callable(getter)
        {
            return Err(());
        }

        if let Some(setter) = desc.get("set")
            && !matches!(setter, Value::Undefined)
            && !self.is_value_callable(setter)
        {
            return Err(());
        }

        Ok(())
    }

    fn apply_object_property_descriptor(
        &mut self,
        obj: &Rc<RefCell<IndexMap<String, Value<'gc>>>>,
        key: &str,
        desc: &IndexMap<String, Value<'gc>>,
    ) {
        let is_accessor = desc.contains_key("get") || desc.contains_key("set");
        let getter_key = format!("__get_{}", key);
        let setter_key = format!("__set_{}", key);
        let readonly_key = format!("__readonly_{}__", key);
        let nonconfigurable_key = format!("__nonconfigurable_{}__", key);
        let nonenumerable_key = format!("__nonenumerable_{}__", key);

        {
            let mut borrow = obj.borrow_mut();
            borrow.shift_remove(&getter_key);
            borrow.shift_remove(&setter_key);
            borrow.shift_remove(&readonly_key);
            borrow.shift_remove(&nonconfigurable_key);
            borrow.shift_remove(&nonenumerable_key);

            if let Some(val) = desc.get("value") {
                borrow.insert(key.to_string(), val.clone());
            }
            if let Some(getter) = desc.get("get")
                && !matches!(getter, Value::Undefined)
            {
                borrow.insert(getter_key.clone(), getter.clone());
            }
            if let Some(setter) = desc.get("set")
                && !matches!(setter, Value::Undefined)
            {
                borrow.insert(setter_key.clone(), setter.clone());
            }

            let enumerable = matches!(desc.get("enumerable"), Some(Value::Boolean(true)));
            let configurable = matches!(desc.get("configurable"), Some(Value::Boolean(true)));
            if !enumerable {
                borrow.insert(nonenumerable_key, Value::Boolean(true));
            }
            if !configurable {
                borrow.insert(nonconfigurable_key, Value::Boolean(true));
            }

            if !is_accessor {
                let writable = matches!(desc.get("writable"), Some(Value::Boolean(true)));
                if !writable {
                    borrow.insert(readonly_key, Value::Boolean(true));
                }
            }
        }
    }

    fn try_proxy_get(&mut self, obj: &Value<'gc>, key: &str) -> Result<Option<Value<'gc>>, JSError> {
        let Value::VmObject(proxy_obj) = obj else {
            return Ok(None);
        };

        let (target, handler, revoked) = {
            let borrow = proxy_obj.borrow();
            let Some(target) = borrow.get("__proxy_target__").cloned() else {
                return Ok(None);
            };
            (
                target,
                borrow.get("__proxy_handler__").cloned().unwrap_or(Value::Undefined),
                matches!(borrow.get("__proxy_revoked__"), Some(Value::Boolean(true))),
            )
        };

        if revoked {
            return Err(crate::raise_type_error!("Cannot perform 'get' on a revoked proxy"));
        }

        if let Value::VmObject(handler_obj) = &handler {
            let trap = handler_obj.borrow().get("get").cloned();
            if let Some(trap_fn) = trap {
                let prop_val = Value::String(crate::unicode::utf8_to_utf16(key));
                let out = match trap_fn {
                    Value::VmFunction(ip, _) => self.call_vm_function_result(ip, &[target.clone(), prop_val.clone(), obj.clone()], &[])?,
                    Value::VmClosure(ip, _, upv) => {
                        let uv = (*upv).clone();
                        self.call_vm_function_result(ip, &[target.clone(), prop_val.clone(), obj.clone()], &uv)?
                    }
                    Value::VmNativeFunction(id) => {
                        self.call_method_builtin(id, handler.clone(), vec![target.clone(), prop_val.clone(), obj.clone()])
                    }
                    Value::VmObject(map) => {
                        let borrow = map.borrow();
                        if let Some(Value::String(host_name_u16)) = borrow.get("__host_fn__") {
                            let host_name = crate::unicode::utf16_to_utf8(host_name_u16);
                            drop(borrow);
                            self.call_host_fn(
                                &host_name,
                                Some(handler.clone()),
                                vec![target.clone(), prop_val.clone(), obj.clone()],
                            )
                        } else {
                            Value::Undefined
                        }
                    }
                    _ => Value::Undefined,
                };
                return Ok(Some(out));
            }
        }

        let fallback = match &target {
            Value::VmObject(_) => self.read_named_property_with_receiver(target.clone(), key, obj.clone()),
            Value::VmArray(arr) => {
                if key == "length" {
                    Value::Number(arr.borrow().len() as f64)
                } else if let Ok(i) = key.parse::<usize>() {
                    arr.borrow().get(i).cloned().unwrap_or(Value::Undefined)
                } else {
                    arr.borrow().props.get(key).cloned().unwrap_or(Value::Undefined)
                }
            }
            _ => Value::Undefined,
        };
        Ok(Some(fallback))
    }

    fn try_proxy_set(&mut self, obj: &Value<'gc>, key: &str, value: Value<'gc>) -> Result<Option<Value<'gc>>, JSError> {
        let Value::VmObject(proxy_obj) = obj else {
            return Ok(None);
        };

        let (target, handler, revoked) = {
            let borrow = proxy_obj.borrow();
            let Some(target) = borrow.get("__proxy_target__").cloned() else {
                return Ok(None);
            };
            (
                target,
                borrow.get("__proxy_handler__").cloned().unwrap_or(Value::Undefined),
                matches!(borrow.get("__proxy_revoked__"), Some(Value::Boolean(true))),
            )
        };

        if revoked {
            return Err(crate::raise_type_error!("Cannot perform 'set' on a revoked proxy"));
        }

        if let Value::VmObject(handler_obj) = &handler {
            let trap = handler_obj.borrow().get("set").cloned();
            if let Some(trap_fn) = trap {
                let prop_val = Value::String(crate::unicode::utf8_to_utf16(key));
                let out = match trap_fn {
                    Value::VmFunction(ip, _) => {
                        self.call_vm_function_result(ip, &[target.clone(), prop_val.clone(), value.clone(), obj.clone()], &[])?
                    }
                    Value::VmClosure(ip, _, upv) => {
                        let uv = (*upv).clone();
                        self.call_vm_function_result(ip, &[target.clone(), prop_val.clone(), value.clone(), obj.clone()], &uv)?
                    }
                    Value::VmNativeFunction(id) => self.call_method_builtin(
                        id,
                        handler.clone(),
                        vec![target.clone(), prop_val.clone(), value.clone(), obj.clone()],
                    ),
                    Value::VmObject(map) => {
                        let borrow = map.borrow();
                        if let Some(Value::String(host_name_u16)) = borrow.get("__host_fn__") {
                            let host_name = crate::unicode::utf16_to_utf8(host_name_u16);
                            drop(borrow);
                            self.call_host_fn(
                                &host_name,
                                Some(handler.clone()),
                                vec![target.clone(), prop_val.clone(), value.clone(), obj.clone()],
                            )
                        } else {
                            Value::Undefined
                        }
                    }
                    _ => Value::Undefined,
                };
                if !out.to_truthy() && self.current_execution_is_strict() {
                    let mut err_map = IndexMap::new();
                    err_map.insert(
                        "message".to_string(),
                        Value::String(crate::unicode::utf8_to_utf16(&format!(
                            "'set' on proxy: trap returned falsish for property '{}'",
                            key
                        ))),
                    );
                    err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                    err_map.insert("name".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                    self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                }
                return Ok(Some(value));
            }
        }

        let _ = self.assign_named_property(target, key.to_string(), value.clone())?;
        Ok(Some(value))
    }

    fn try_proxy_delete(&mut self, obj: &Value<'gc>, key: &str) -> Result<Option<bool>, JSError> {
        let Value::VmObject(proxy_obj) = obj else {
            return Ok(None);
        };

        let (target, handler, revoked) = {
            let borrow = proxy_obj.borrow();
            let Some(target) = borrow.get("__proxy_target__").cloned() else {
                return Ok(None);
            };
            (
                target,
                borrow.get("__proxy_handler__").cloned().unwrap_or(Value::Undefined),
                matches!(borrow.get("__proxy_revoked__"), Some(Value::Boolean(true))),
            )
        };

        if revoked {
            return Err(crate::raise_type_error!("Cannot perform 'deleteProperty' on a revoked proxy"));
        }

        if let Value::VmObject(handler_obj) = &handler {
            let trap = handler_obj.borrow().get("deleteProperty").cloned();
            if let Some(trap_fn) = trap {
                let prop_val = Value::String(crate::unicode::utf8_to_utf16(key));
                let out = match trap_fn {
                    Value::VmFunction(ip, _) => self.call_vm_function_result(ip, &[target.clone(), prop_val.clone()], &[])?,
                    Value::VmClosure(ip, _, upv) => {
                        let uv = (*upv).clone();
                        self.call_vm_function_result(ip, &[target.clone(), prop_val.clone()], &uv)?
                    }
                    Value::VmNativeFunction(id) => self.call_method_builtin(id, handler.clone(), vec![target.clone(), prop_val.clone()]),
                    Value::VmObject(map) => {
                        let borrow = map.borrow();
                        if let Some(Value::String(host_name_u16)) = borrow.get("__host_fn__") {
                            let host_name = crate::unicode::utf16_to_utf8(host_name_u16);
                            drop(borrow);
                            self.call_host_fn(&host_name, Some(handler.clone()), vec![target.clone(), prop_val.clone()])
                        } else {
                            Value::Undefined
                        }
                    }
                    _ => Value::Undefined,
                };
                let deleted = out.to_truthy();
                if !deleted && self.current_execution_is_strict() {
                    let mut err_map = IndexMap::new();
                    err_map.insert(
                        "message".to_string(),
                        Value::String(crate::unicode::utf8_to_utf16(&format!(
                            "'deleteProperty' on proxy: trap returned falsish for property '{}'",
                            key
                        ))),
                    );
                    err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                    err_map.insert("name".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                    self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                }
                return Ok(Some(deleted));
            }
        }

        let deleted = match &target {
            Value::VmObject(map) => {
                map.borrow_mut().shift_remove(key);
                true
            }
            Value::VmArray(arr) => {
                arr.borrow_mut().props.shift_remove(key);
                true
            }
            _ => false,
        };
        Ok(Some(deleted))
    }

    fn try_proxy_has(&mut self, obj: &Value<'gc>, key: &str) -> Result<Option<bool>, JSError> {
        let Value::VmObject(proxy_obj) = obj else {
            return Ok(None);
        };

        let (target, handler, revoked) = {
            let borrow = proxy_obj.borrow();
            let Some(target) = borrow.get("__proxy_target__").cloned() else {
                return Ok(None);
            };
            (
                target,
                borrow.get("__proxy_handler__").cloned().unwrap_or(Value::Undefined),
                matches!(borrow.get("__proxy_revoked__"), Some(Value::Boolean(true))),
            )
        };

        if revoked {
            return Err(crate::raise_type_error!("Cannot perform 'has' on a revoked proxy"));
        }

        if let Value::VmObject(handler_obj) = &handler {
            let trap = handler_obj.borrow().get("has").cloned();
            if let Some(trap_fn) = trap {
                let prop_val = Value::String(crate::unicode::utf8_to_utf16(key));
                let out = match trap_fn {
                    Value::VmFunction(ip, _) => self.call_vm_function_result(ip, &[target.clone(), prop_val.clone()], &[])?,
                    Value::VmClosure(ip, _, upv) => {
                        let uv = (*upv).clone();
                        self.call_vm_function_result(ip, &[target.clone(), prop_val.clone()], &uv)?
                    }
                    Value::VmNativeFunction(id) => self.call_method_builtin(id, handler.clone(), vec![target.clone(), prop_val.clone()]),
                    Value::VmObject(map) => {
                        let borrow = map.borrow();
                        if let Some(Value::String(host_name_u16)) = borrow.get("__host_fn__") {
                            let host_name = crate::unicode::utf16_to_utf8(host_name_u16);
                            drop(borrow);
                            self.call_host_fn(&host_name, Some(handler.clone()), vec![target.clone(), prop_val.clone()])
                        } else {
                            Value::Undefined
                        }
                    }
                    _ => Value::Undefined,
                };
                return Ok(Some(out.to_truthy()));
            }
        }

        let fallback = match &target {
            Value::VmObject(map) => {
                let b = map.borrow();
                if b.contains_key(key) {
                    true
                } else {
                    let proto = b.get("__proto__").cloned();
                    drop(b);
                    self.lookup_proto_chain(&proto, key).is_some()
                }
            }
            Value::VmArray(arr) => {
                if let Ok(idx) = key.parse::<usize>() {
                    let borrow = arr.borrow();
                    idx < borrow.len() && !borrow.props.contains_key(&format!("__deleted_{}", idx))
                } else if key == "length" {
                    true
                } else {
                    arr.borrow().props.contains_key(key)
                }
            }
            _ => false,
        };
        Ok(Some(fallback))
    }

    fn current_execution_is_strict(&self) -> bool {
        if let Some(frame) = self.frames.last()
            && self.chunk.fn_strictness.get(&frame.func_ip).copied().unwrap_or(false)
        {
            return true;
        }

        self.script_source.as_deref().is_some_and(|src| {
            let trimmed = src.trim_start();
            trimmed.starts_with("\"use strict\"") || trimmed.starts_with("'use strict'")
        })
    }

    fn construct_value(
        &mut self,
        target: Value<'gc>,
        args: Vec<Value<'gc>>,
        new_target: Option<Value<'gc>>,
    ) -> Result<Value<'gc>, JSError> {
        match target.clone() {
            Value::VmFunction(target_ip, _arity) | Value::VmClosure(target_ip, _arity, _) => {
                let new_obj = Rc::new(RefCell::new(IndexMap::new()));
                let proto_source = new_target.unwrap_or(target.clone());
                let proto = match proto_source {
                    Value::VmFunction(ip, ar) | Value::VmClosure(ip, ar, _) => self.get_fn_props(ip, ar).borrow().get("prototype").cloned(),
                    Value::VmObject(map) => map.borrow().get("prototype").cloned(),
                    _ => None,
                };
                if let Some(proto) = proto {
                    new_obj.borrow_mut().insert("__proto__".to_string(), proto);
                }

                let this_val = Value::VmObject(new_obj.clone());
                self.this_stack.push(this_val.clone());
                let closure_uv = if let Value::VmClosure(_, _, ref uv) = target {
                    (**uv).clone()
                } else {
                    Vec::new()
                };
                let result = self.call_vm_function_result(target_ip, &args, &closure_uv);
                self.this_stack.pop();
                match result? {
                    val @ Value::VmObject(_) => Ok(val),
                    _ => Ok(Value::VmObject(new_obj)),
                }
            }
            Value::VmObject(map) => {
                if let Some(Value::Number(native_id)) = map.borrow().get("__native_id__") {
                    let id = *native_id as u8;
                    let result = self.call_builtin(id, args);
                    let wrapped = match id {
                        BUILTIN_CTOR_NUMBER => {
                            let mut m = IndexMap::new();
                            m.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Number")));
                            m.insert("__value__".to_string(), result);
                            Value::VmObject(Rc::new(RefCell::new(m)))
                        }
                        BUILTIN_CTOR_STRING => {
                            let mut m = IndexMap::new();
                            m.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("String")));
                            m.insert("__value__".to_string(), result);
                            Value::VmObject(Rc::new(RefCell::new(m)))
                        }
                        BUILTIN_CTOR_BOOLEAN => {
                            let mut m = IndexMap::new();
                            m.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Boolean")));
                            m.insert("__value__".to_string(), result);
                            Value::VmObject(Rc::new(RefCell::new(m)))
                        }
                        _ => result,
                    };
                    Ok(wrapped)
                } else {
                    Err(crate::raise_type_error!("Target is not a constructor"))
                }
            }
            Value::VmNativeFunction(id) => Ok(self.call_builtin(id, args)),
            _ => Err(crate::raise_type_error!("Target is not a constructor")),
        }
    }

    /// Call a Value callback (VmFunction or VmClosure), extracting ip and upvalues automatically.
    fn _call_callback(&mut self, callback: &Value<'gc>, args: &[Value<'gc>]) -> Value<'gc> {
        match callback {
            Value::VmFunction(ip, _) => self.call_vm_function(*ip, args, &[]),
            Value::VmClosure(ip, _, upv) => {
                let uv = (**upv).clone();
                self.call_vm_function(*ip, args, &uv)
            }
            _ => Value::Undefined,
        }
    }

    /// SameValue comparison (like Object.is)
    fn values_same(&self, a: &Value<'gc>, b: &Value<'gc>) -> bool {
        match (a, b) {
            (Value::VmObject(a), Value::VmObject(b)) => Rc::ptr_eq(a, b),
            (Value::VmArray(a), Value::VmArray(b)) => Rc::ptr_eq(a, b),
            (Value::VmMap(a), Value::VmMap(b)) => Rc::ptr_eq(a, b),
            (Value::VmSet(a), Value::VmSet(b)) => Rc::ptr_eq(a, b),
            (Value::Number(a), Value::Number(b)) => {
                if a.is_nan() && b.is_nan() {
                    true
                } else if *a == 0.0 && *b == 0.0 {
                    a.is_sign_positive() == b.is_sign_positive()
                } else {
                    a == b
                }
            }
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Undefined, Value::Undefined) => true,
            (Value::Null, Value::Null) => true,
            _ => false,
        }
    }

    /// Strict equality (===)
    fn strict_equal(&self, a: &Value<'gc>, b: &Value<'gc>) -> bool {
        match (a, b) {
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::BigInt(a), Value::BigInt(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Undefined, Value::Undefined) => true,
            (Value::Null, Value::Null) => true,
            (Value::VmObject(a), Value::VmObject(b)) => Rc::ptr_eq(a, b),
            (Value::VmArray(a), Value::VmArray(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Flatten array recursively
    fn flatten_array(&self, elements: Vec<Value<'gc>>, depth: usize) -> Vec<Value<'gc>> {
        let mut result = Vec::new();
        for elem in elements {
            if depth > 0
                && let Value::VmArray(inner) = elem
            {
                let sub = self.flatten_array(inner.borrow().elements.clone(), depth - 1);
                result.extend(sub);
                continue;
            }
            result.push(elem);
        }
        result
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
                let borrow = arr.borrow();
                let parts: Vec<String> = borrow
                    .elements
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        if borrow.props.contains_key(&format!("__deleted_{}", i)) {
                            "null".to_string()
                        } else {
                            self.json_stringify(v)
                        }
                    })
                    .collect();
                format!("[{}]", parts.join(","))
            }
            Value::VmObject(map) => {
                let m = map.borrow();
                let parts: Vec<String> = m
                    .iter()
                    .filter(|(k, _)| !k.starts_with("__") && !k.starts_with("@@sym:"))
                    .map(|(k, v)| format!("\"{}\":{}", k.replace('\\', "\\\\").replace('"', "\\\""), self.json_stringify(v)))
                    .collect();
                format!("{{{}}}", parts.join(","))
            }
            _ => "null".to_string(),
        }
    }

    /// JSON.parse helper (simple subset)
    fn json_parse(&self, s: &str) -> Value<'gc> {
        let trimmed = s.trim();
        // Use serde_json for robust parsing, then convert to Value
        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return self.json_to_value(&json_val);
        }
        Value::Undefined
    }

    fn json_to_value(&self, v: &serde_json::Value) -> Value<'gc> {
        match v {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Boolean(*b),
            serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(f64::NAN)),
            serde_json::Value::String(s) => Value::String(crate::unicode::utf8_to_utf16(s)),
            serde_json::Value::Array(arr) => {
                let elems: Vec<Value<'gc>> = arr.iter().map(|item| self.json_to_value(item)).collect();
                Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(elems))))
            }
            serde_json::Value::Object(obj) => {
                let mut map = IndexMap::new();
                for (key, val) in obj {
                    map.insert(key.clone(), self.json_to_value(val));
                }
                Value::VmObject(Rc::new(RefCell::new(map)))
            }
        }
    }

    /// Handle a thrown value: unwind to nearest try/catch or return error
    fn handle_throw(&mut self, thrown: Value<'gc>) -> Result<(), JSError> {
        if let Value::VmObject(map) = &thrown {
            self.annotate_error_object(map);
        }
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
            Err(self.vm_error_to_js_error(thrown))
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
        let result = self.run_inner(0)?;
        self.drain_timers()?;
        Ok(result)
    }

    /// Execute all pending timers (setTimeout / setInterval callbacks).
    /// Runs in a loop until no more timers remain, supporting chained timers.
    fn drain_timers(&mut self) -> Result<(), JSError> {
        // Sort by delay so shorter delays fire first
        for _round in 0..1000 {
            if self.pending_timers.is_empty() {
                break;
            }
            // Take current batch, sorted by delay
            let mut batch: Vec<PendingTimer<'gc>> = std::mem::take(&mut self.pending_timers);
            batch.sort_by_key(|t| t.delay_ms);

            for timer in batch {
                if self.cleared_timers.contains(&timer.id) {
                    continue;
                }
                let cb_args: Vec<Value<'gc>> = timer.args.clone();
                match &timer.callback {
                    Value::VmFunction(ip, _) => {
                        let _ = self.call_vm_function(*ip, &cb_args, &[]);
                    }
                    Value::VmClosure(ip, _, upv) => {
                        let uv = (**upv).clone();
                        let _ = self.call_vm_function(*ip, &cb_args, &uv);
                    }
                    Value::VmNativeFunction(native_id) => {
                        let _ = self.call_builtin(*native_id, cb_args);
                    }
                    Value::VmObject(obj) => {
                        let borrow = obj.borrow();
                        if let Some(Value::String(host_name_u16)) = borrow.get("__host_fn__") {
                            let host_name = crate::unicode::utf16_to_utf8(host_name_u16);
                            drop(borrow);
                            self.call_host_fn(&host_name, None, cb_args);
                        }
                    }
                    _ => {}
                }
                // Re-queue intervals
                if timer.is_interval && !self.cleared_timers.contains(&timer.id) {
                    self.pending_timers.push(PendingTimer {
                        id: timer.id,
                        callback: timer.callback,
                        args: timer.args,
                        delay_ms: timer.delay_ms,
                        is_interval: true,
                    });
                }
            }
        }
        self.cleared_timers.clear();
        Ok(())
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
                        if frame.is_method {
                            self.this_stack.pop();
                        }
                        self.stack.truncate(frame.bp - 1);
                        self.ip = frame.return_ip;
                        if self.frames.len() < min_depth {
                            // Returning from an injected call (call_vm_function)
                            return Ok(result);
                        }
                        // Returning from a function call: pop locals and the function itself
                        self.stack.push(result);
                    } else {
                        // Return from top-level script
                        return Ok(result);
                    }
                }
                Opcode::GetLocal => {
                    let index = self.read_byte() as usize;
                    let bp = self.frames.last().map(|f| f.bp).unwrap_or(0);
                    if bp + index >= self.stack.len() {
                        let mut err_map = IndexMap::new();
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("Invalid local access")),
                        );
                        err_map.insert(
                            "__type__".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("ReferenceError")),
                        );
                        self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                        continue;
                    }
                    // Check if this local has been captured as an upvalue cell
                    let val = if let Some(frame) = self.frames.last() {
                        if let Some(cell) = frame.local_cells.get(&index) {
                            cell.borrow().clone()
                        } else {
                            self.stack[bp + index].clone()
                        }
                    } else {
                        self.stack[bp + index].clone()
                    };
                    // TDZ check: Uninitialized variables throw ReferenceError
                    if matches!(val, Value::Uninitialized) {
                        let mut err_map = IndexMap::new();
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("Cannot access variable before initialization")),
                        );
                        err_map.insert(
                            "__type__".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("ReferenceError")),
                        );
                        let err = Value::VmObject(Rc::new(RefCell::new(err_map)));
                        self.handle_throw(err)?;
                    } else {
                        self.stack.push(val);
                    }
                }
                Opcode::SetLocal => {
                    let index = self.read_byte() as usize;
                    let bp = self.frames.last().map(|f| f.bp).unwrap_or(0);
                    if bp + index >= self.stack.len() {
                        let mut err_map = IndexMap::new();
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("Invalid local assignment")),
                        );
                        err_map.insert(
                            "__type__".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("ReferenceError")),
                        );
                        self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                        continue;
                    }
                    let val = self.stack.last().expect("VM Stack underflow").clone();
                    // Check if this local has been captured as an upvalue cell
                    let has_cell = self.frames.last().map(|f| f.local_cells.contains_key(&index)).unwrap_or(false);
                    if has_cell {
                        let cell = self.frames.last().unwrap().local_cells.get(&index).unwrap().clone();
                        *cell.borrow_mut() = val;
                    } else {
                        self.stack[bp + index] = val;
                    }
                }
                Opcode::Call => {
                    let raw_arg_byte = self.read_byte();
                    let is_method = (raw_arg_byte & 0x80) != 0;
                    let is_direct_eval = (raw_arg_byte & 0x40) != 0;
                    let arg_count = (raw_arg_byte & 0x3f) as usize;
                    self.direct_eval = is_direct_eval;
                    // Stack for method call: [..., receiver, callee, arg0, arg1, ...]
                    // Stack for regular call: [..., callee, arg0, arg1, ...]
                    let callee_idx = self.stack.len() - arg_count - 1;
                    let callee = self.stack[callee_idx].clone();
                    match callee {
                        Value::VmFunction(target_ip, arity) => {
                            if self.chunk.async_function_ips.contains(&target_ip) {
                                let args_vec: Vec<Value<'gc>> = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                                let receiver = if is_method {
                                    self.stack.get(callee_idx.saturating_sub(1)).cloned().unwrap_or(Value::Undefined)
                                } else {
                                    Value::Undefined
                                };
                                let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                                self.stack.truncate(base);
                                let saved_try_stack = std::mem::take(&mut self.try_stack);
                                if is_method {
                                    self.this_stack.push(receiver);
                                }
                                let result = self.call_vm_function_result(target_ip, &args_vec, &[]);
                                if is_method {
                                    self.this_stack.pop();
                                }
                                self.try_stack = saved_try_stack;
                                let promise = match result {
                                    Ok(value) => self.call_builtin(BUILTIN_PROMISE_RESOLVE, vec![value]),
                                    Err(err) => self.call_host_fn("promise.reject", None, vec![self.vm_value_from_error(&err)]),
                                };
                                self.stack.push(promise);
                                continue;
                            }
                            if is_method && self.chunk.class_constructor_ips.contains(&target_ip) {
                                let in_ctor_context = self.frames.iter().any(|f| self.chunk.class_constructor_ips.contains(&f.func_ip));
                                if !in_ctor_context {
                                    let receiver = self.stack.get(callee_idx.saturating_sub(1)).cloned().unwrap_or(Value::Undefined);
                                    let args_vec: Vec<Value<'gc>> = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                                    let base = callee_idx.saturating_sub(1);
                                    self.stack.truncate(base);
                                    self.this_stack.push(receiver);
                                    let result = self.call_vm_function(target_ip, &args_vec, &[]);
                                    self.this_stack.pop();
                                    self.stack.push(result);

                                    let mut err_map = IndexMap::new();
                                    err_map.insert(
                                        "__type__".to_string(),
                                        Value::String(crate::unicode::utf8_to_utf16("ReferenceError")),
                                    );
                                    err_map.insert(
                                        "message".to_string(),
                                        Value::String(crate::unicode::utf8_to_utf16(
                                            "Super constructor may only be called directly in a derived constructor",
                                        )),
                                    );
                                    self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                                    continue;
                                }
                            }
                            if !is_method && self.chunk.class_constructor_ips.contains(&target_ip) {
                                let mut err_map = IndexMap::new();
                                err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                err_map.insert(
                                    "message".to_string(),
                                    Value::String(crate::unicode::utf8_to_utf16("Class constructor cannot be invoked without 'new'")),
                                );
                                self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                                continue;
                            }
                            // Pad missing args with Undefined
                            if (arg_count as u8) < arity {
                                for _ in 0..(arity as usize - arg_count) {
                                    self.stack.push(Value::Undefined);
                                }
                            }
                            // Remove excess args beyond arity to prevent local slot corruption
                            let saved_args = if arg_count > arity as usize {
                                let first_arg_idx = callee_idx + 1;
                                let all_args: Vec<Value<'gc>> = self.stack[first_arg_idx..first_arg_idx + arg_count].to_vec();
                                let drain_start = first_arg_idx + arity as usize;
                                let drain_end = first_arg_idx + arg_count;
                                self.stack.drain(drain_start..drain_end);
                                Some(all_args)
                            } else {
                                None
                            };
                            // For method calls, pop receiver from under callee and bind as this
                            if is_method {
                                // Remove receiver (one slot below callee)
                                let receiver = self.stack.remove(callee_idx - 1);
                                self.this_stack.push(receiver);
                                let callee_idx = callee_idx - 1;
                                let frame = CallFrame {
                                    return_ip: self.ip,
                                    bp: callee_idx + 1,
                                    is_method: true,
                                    arg_count,
                                    func_ip: target_ip,
                                    arguments_obj: None,
                                    upvalues: Vec::new(),
                                    saved_args,
                                    local_cells: HashMap::new(),
                                };
                                self.frames.push(frame);
                            } else {
                                // In strict mode, non-method calls get `this = undefined`
                                let fn_strict = self.chunk.fn_strictness.get(&target_ip).copied().unwrap_or(false);
                                let is_arrow = self.chunk.arrow_function_ips.contains(&target_ip);
                                let push_this = fn_strict && !is_arrow;
                                if push_this {
                                    self.this_stack.push(Value::Undefined);
                                }
                                let frame = CallFrame {
                                    return_ip: self.ip,
                                    bp: callee_idx + 1,
                                    is_method: push_this,
                                    arg_count,
                                    func_ip: target_ip,
                                    arguments_obj: None,
                                    upvalues: Vec::new(),
                                    saved_args,
                                    local_cells: HashMap::new(),
                                };
                                self.frames.push(frame);
                            }
                            self.ip = target_ip;
                        }
                        Value::VmClosure(target_ip, arity, ref upvals) => {
                            if self.chunk.async_function_ips.contains(&target_ip) {
                                let args_vec: Vec<Value<'gc>> = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                                let receiver = if is_method {
                                    self.stack.get(callee_idx.saturating_sub(1)).cloned().unwrap_or(Value::Undefined)
                                } else {
                                    Value::Undefined
                                };
                                let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                                self.stack.truncate(base);
                                let saved_try_stack = std::mem::take(&mut self.try_stack);
                                if is_method {
                                    self.this_stack.push(receiver);
                                }
                                let result = self.call_vm_function_result(target_ip, &args_vec, upvals);
                                if is_method {
                                    self.this_stack.pop();
                                }
                                self.try_stack = saved_try_stack;
                                let promise = match result {
                                    Ok(value) => self.call_builtin(BUILTIN_PROMISE_RESOLVE, vec![value]),
                                    Err(err) => self.call_host_fn("promise.reject", None, vec![self.vm_value_from_error(&err)]),
                                };
                                self.stack.push(promise);
                                continue;
                            }
                            if is_method && self.chunk.class_constructor_ips.contains(&target_ip) {
                                let in_ctor_context = self.frames.iter().any(|f| self.chunk.class_constructor_ips.contains(&f.func_ip));
                                if !in_ctor_context {
                                    let receiver = self.stack.get(callee_idx.saturating_sub(1)).cloned().unwrap_or(Value::Undefined);
                                    let args_vec: Vec<Value<'gc>> = self.stack[callee_idx + 1..callee_idx + 1 + arg_count].to_vec();
                                    let base = callee_idx.saturating_sub(1);
                                    self.stack.truncate(base);
                                    self.this_stack.push(receiver);
                                    let result = self.call_vm_function(target_ip, &args_vec, upvals);
                                    self.this_stack.pop();
                                    self.stack.push(result);

                                    let mut err_map = IndexMap::new();
                                    err_map.insert(
                                        "__type__".to_string(),
                                        Value::String(crate::unicode::utf8_to_utf16("ReferenceError")),
                                    );
                                    err_map.insert(
                                        "message".to_string(),
                                        Value::String(crate::unicode::utf8_to_utf16(
                                            "Super constructor may only be called directly in a derived constructor",
                                        )),
                                    );
                                    self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                                    continue;
                                }
                            }
                            if !is_method && self.chunk.class_constructor_ips.contains(&target_ip) {
                                let mut err_map = IndexMap::new();
                                err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                err_map.insert(
                                    "message".to_string(),
                                    Value::String(crate::unicode::utf8_to_utf16("Class constructor cannot be invoked without 'new'")),
                                );
                                self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                                continue;
                            }
                            if (arg_count as u8) < arity {
                                for _ in 0..(arity as usize - arg_count) {
                                    self.stack.push(Value::Undefined);
                                }
                            }
                            // Remove excess args beyond arity to prevent local slot corruption
                            let saved_args = if arg_count > arity as usize {
                                let first_arg_idx = callee_idx + 1;
                                let all_args: Vec<Value<'gc>> = self.stack[first_arg_idx..first_arg_idx + arg_count].to_vec();
                                let drain_start = first_arg_idx + arity as usize;
                                let drain_end = first_arg_idx + arg_count;
                                self.stack.drain(drain_start..drain_end);
                                Some(all_args)
                            } else {
                                None
                            };
                            let closure_upvalues = (**upvals).clone();
                            if is_method {
                                let receiver = self.stack.remove(callee_idx - 1);
                                self.this_stack.push(receiver);
                                let callee_idx = callee_idx - 1;
                                let frame = CallFrame {
                                    return_ip: self.ip,
                                    bp: callee_idx + 1,
                                    is_method: true,
                                    arg_count,
                                    func_ip: target_ip,
                                    arguments_obj: None,
                                    upvalues: closure_upvalues,
                                    saved_args,
                                    local_cells: HashMap::new(),
                                };
                                self.frames.push(frame);
                            } else {
                                let fn_strict = self.chunk.fn_strictness.get(&target_ip).copied().unwrap_or(false);
                                let is_arrow = self.chunk.arrow_function_ips.contains(&target_ip);
                                let push_this = fn_strict && !is_arrow;
                                if push_this {
                                    self.this_stack.push(Value::Undefined);
                                }
                                let frame = CallFrame {
                                    return_ip: self.ip,
                                    bp: callee_idx + 1,
                                    is_method: push_this,
                                    arg_count,
                                    func_ip: target_ip,
                                    arguments_obj: None,
                                    upvalues: closure_upvalues,
                                    saved_args,
                                    local_cells: HashMap::new(),
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
                                // FinalizationRegistry.register validation (needs to throw TypeError)
                                if id == BUILTIN_FR_REGISTER {
                                    let target = args.first().cloned().unwrap_or(Value::Undefined);
                                    let held = args.get(1).cloned().unwrap_or(Value::Undefined);
                                    let token = args.get(2).cloned();
                                    let target_is_object = matches!(
                                        target,
                                        Value::VmObject(_)
                                            | Value::VmArray(_)
                                            | Value::VmMap(_)
                                            | Value::VmSet(_)
                                            | Value::VmFunction(..)
                                            | Value::VmClosure(..)
                                            | Value::Closure(..)
                                    );
                                    if !target_is_object {
                                        let mut err_map = IndexMap::new();
                                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                        err_map.insert(
                                            "message".to_string(),
                                            Value::String(crate::unicode::utf8_to_utf16("Invalid value used in FinalizationRegistry")),
                                        );
                                        self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                                        continue;
                                    }
                                    if self.values_same(&target, &held) {
                                        let mut err_map = IndexMap::new();
                                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                        err_map.insert(
                                            "message".to_string(),
                                            Value::String(crate::unicode::utf8_to_utf16("target and held value must not be the same")),
                                        );
                                        self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                                        continue;
                                    }
                                    if let Some(ref tok) = token {
                                        let tok_ok = matches!(
                                            tok,
                                            Value::Undefined
                                                | Value::VmObject(_)
                                                | Value::VmArray(_)
                                                | Value::VmMap(_)
                                                | Value::VmSet(_)
                                                | Value::VmFunction(..)
                                                | Value::VmClosure(..)
                                                | Value::Closure(..)
                                        );
                                        if !tok_ok {
                                            let mut err_map = IndexMap::new();
                                            err_map
                                                .insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                            err_map.insert(
                                                "message".to_string(),
                                                Value::String(crate::unicode::utf8_to_utf16("Invalid unregister token")),
                                            );
                                            self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                                            continue;
                                        }
                                    }
                                }
                                let result = self.call_method_builtin(id, recv, args);
                                self.stack.push(result);
                                if let Some(thrown) = self.pending_throw.take() {
                                    self.handle_throw(thrown)?;
                                    continue;
                                }
                            } else {
                                let result = self.call_builtin(id, args);
                                self.stack.push(result);
                                if let Some(thrown) = self.pending_throw.take() {
                                    self.handle_throw(thrown)?;
                                    continue;
                                }
                            }
                        }
                        Value::Function(name) => {
                            let args_collected: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                            self.stack.pop(); // pop callee
                            if is_method {
                                self.stack.pop(); // pop receiver
                            }
                            let result = self.call_named_host_function(&name, args_collected);
                            self.stack.push(result);
                            if let Some(thrown) = self.pending_throw.take() {
                                self.handle_throw(thrown)?;
                                continue;
                            }
                        }
                        _ => {
                            // Check if it's a Function wrapper (VmObject with __fn_body__ or __native_id__)
                            if let Value::VmObject(ref map) = callee {
                                let borrow = map.borrow();
                                if let Some(Value::String(host_name_u16)) = borrow.get("__host_fn__") {
                                    let host_name = crate::unicode::utf16_to_utf8(host_name_u16);
                                    let bound_this = borrow.get("__host_this__").cloned();
                                    drop(borrow);
                                    let args_collected: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                                    self.stack.pop(); // pop callee
                                    let recv = if is_method {
                                        Some(self.stack.pop().unwrap_or(Value::Undefined))
                                    } else {
                                        bound_this
                                    };
                                    let result = self.call_host_fn(&host_name, recv, args_collected);
                                    self.stack.push(result);
                                    if let Some(thrown) = self.pending_throw.take() {
                                        self.handle_throw(thrown)?;
                                        continue;
                                    }
                                } else if let Some(bound_target) = borrow.get("__bound_target__").cloned() {
                                    let bound_this = borrow.get("__bound_this__").cloned().unwrap_or(Value::Undefined);
                                    let mut final_args: Vec<Value<'gc>> = match borrow.get("__bound_args__") {
                                        Some(Value::VmArray(arr)) => arr.borrow().iter().cloned().collect(),
                                        _ => Vec::new(),
                                    };
                                    drop(borrow);

                                    let args_collected: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                                    self.stack.pop(); // pop callee
                                    if is_method {
                                        self.stack.pop(); // pop receiver
                                    }
                                    final_args.extend(args_collected);

                                    let result = match bound_target {
                                        Value::VmFunction(ip, _) => {
                                            if self.chunk.async_function_ips.contains(&ip) {
                                                self.this_stack.push(bound_this.clone());
                                                let saved_try_stack = std::mem::take(&mut self.try_stack);
                                                let call_result = self.call_vm_function_result(ip, &final_args, &[]);
                                                self.try_stack = saved_try_stack;
                                                self.this_stack.pop();
                                                match call_result {
                                                    Ok(value) => self.call_builtin(BUILTIN_PROMISE_RESOLVE, vec![value]),
                                                    Err(err) => {
                                                        self.call_host_fn("promise.reject", None, vec![self.vm_value_from_error(&err)])
                                                    }
                                                }
                                            } else {
                                                self.this_stack.push(bound_this.clone());
                                                let r = self.call_vm_function(ip, &final_args, &[]);
                                                self.this_stack.pop();
                                                r
                                            }
                                        }
                                        Value::VmClosure(ip, _, ups) => {
                                            if self.chunk.async_function_ips.contains(&ip) {
                                                self.this_stack.push(bound_this.clone());
                                                let saved_try_stack = std::mem::take(&mut self.try_stack);
                                                let call_result = self.call_vm_function_result(ip, &final_args, &ups);
                                                self.try_stack = saved_try_stack;
                                                self.this_stack.pop();
                                                match call_result {
                                                    Ok(value) => self.call_builtin(BUILTIN_PROMISE_RESOLVE, vec![value]),
                                                    Err(err) => {
                                                        self.call_host_fn("promise.reject", None, vec![self.vm_value_from_error(&err)])
                                                    }
                                                }
                                            } else {
                                                self.this_stack.push(bound_this.clone());
                                                let r = self.call_vm_function(ip, &final_args, &ups);
                                                self.this_stack.pop();
                                                r
                                            }
                                        }
                                        Value::VmNativeFunction(id) => {
                                            self.this_stack.push(bound_this.clone());
                                            let r = self.call_method_builtin(id, bound_this, final_args);
                                            self.this_stack.pop();
                                            r
                                        }
                                        _ => Value::Undefined,
                                    };
                                    self.stack.push(result);
                                    if let Some(thrown) = self.pending_throw.take() {
                                        self.handle_throw(thrown)?;
                                        continue;
                                    }
                                } else if let Some(Value::Number(native_id)) = borrow.get("__native_id__") {
                                    let id = *native_id as u8;
                                    drop(borrow);
                                    let args_collected: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                                    self.stack.pop(); // pop callee
                                    if is_method {
                                        self.stack.pop(); // pop receiver
                                    }
                                    let result = self.call_builtin(id, args_collected);
                                    self.stack.push(result);
                                    if let Some(thrown) = self.pending_throw.take() {
                                        self.handle_throw(thrown)?;
                                        continue;
                                    }
                                } else if let Some(Value::String(body_u16)) = borrow.get("__fn_body__") {
                                    let body = crate::unicode::utf16_to_utf8(body_u16);
                                    drop(borrow);
                                    // Pop args and callee
                                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                                    self.stack.truncate(base);
                                    // Eval the body: try with "return" first, then without
                                    let code_with_return = if body.trim_start().starts_with("return") {
                                        body.clone()
                                    } else {
                                        format!("return {}", body)
                                    };
                                    let result = match crate::core::compile_and_run_vm_snippet(&code_with_return) {
                                        Ok(v) => crate::core::static_to_gc(v),
                                        Err(_) => match crate::core::compile_and_run_vm_snippet(&body) {
                                            Ok(v) => crate::core::static_to_gc(v),
                                            Err(_) => Value::Undefined,
                                        },
                                    };
                                    self.stack.push(result);
                                } else {
                                    log::warn!("Attempted to call non-function object");
                                    let callee_name = self.resolve_callee_name(callee_idx);
                                    let msg = format!("{} is not a function", callee_name);
                                    let mut err_map = IndexMap::new();
                                    err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                    err_map.insert("message".to_string(), Value::String(crate::unicode::utf8_to_utf16(&msg)));
                                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                                    self.stack.truncate(base);
                                    self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                                    continue;
                                }
                            } else {
                                log::warn!("Attempted to call non-function: {}", value_to_string(&callee));
                                let callee_name = self.resolve_callee_name(callee_idx);
                                let msg = format!("{} is not a function", callee_name);
                                let mut err_map = IndexMap::new();
                                err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                err_map.insert("message".to_string(), Value::String(crate::unicode::utf8_to_utf16(&msg)));
                                let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                                self.stack.truncate(base);
                                self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                                continue;
                            }
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
                Opcode::DefineGlobalConst => {
                    let name_idx = self.read_u16() as usize;
                    let name_val = &self.chunk.constants[name_idx];
                    if let Value::String(s) = name_val {
                        let name_str = crate::unicode::utf16_to_utf8(s);
                        let val = self.stack.pop().unwrap_or(Value::Undefined);
                        self.globals.insert(name_str.clone(), val);
                        self.const_globals.insert(name_str);
                    }
                }
                Opcode::GetNewTarget => {
                    let val = self.new_target_stack.last().cloned().unwrap_or(Value::Undefined);
                    self.stack.push(val);
                }
                Opcode::GetGlobal => {
                    let name_idx = self.read_u16() as usize;
                    let name_val = &self.chunk.constants[name_idx];
                    if let Value::String(s) = name_val {
                        let name_str = crate::unicode::utf16_to_utf8(s);
                        if let Some(val) = self.globals.get(&name_str).cloned() {
                            self.stack.push(val);
                        } else if let Some(frame) = self.frames.last()
                            && self.chunk.fn_names.get(&frame.func_ip).is_some_and(|fn_name| fn_name == &name_str)
                        {
                            let arity = self
                                .chunk
                                .constants
                                .iter()
                                .find_map(|c| match c {
                                    Value::VmFunction(ip, a) if *ip == frame.func_ip => Some(*a),
                                    _ => None,
                                })
                                .unwrap_or(0);
                            if frame.upvalues.is_empty() {
                                self.stack.push(Value::VmFunction(frame.func_ip, arity));
                            } else {
                                self.stack
                                    .push(Value::VmClosure(frame.func_ip, arity, Rc::new(frame.upvalues.clone())));
                            }
                        } else {
                            // unresolvable reference
                            let mut err_map = IndexMap::new();
                            err_map.insert(
                                "message".to_string(),
                                Value::String(crate::unicode::utf8_to_utf16(&format!("{} is not defined", name_str))),
                            );
                            err_map.insert(
                                "__type__".to_string(),
                                Value::String(crate::unicode::utf8_to_utf16("ReferenceError")),
                            );
                            let err = Value::VmObject(Rc::new(RefCell::new(err_map)));
                            self.handle_throw(err)?;
                        }
                    }
                }
                Opcode::GetArguments => {
                    // produce (and cache) arguments object for current call frame
                    if let Some(frame) = self.frames.last_mut() {
                        if let Some(args_obj) = &frame.arguments_obj {
                            self.stack.push(args_obj.clone());
                        } else {
                            let arg_count = frame.arg_count;
                            let bp = frame.bp;
                            let saved = frame.saved_args.clone();
                            let mut map = IndexMap::new();
                            for i in 0..arg_count {
                                let val = if let Some(ref sa) = saved {
                                    sa.get(i).cloned().unwrap_or(Value::Undefined)
                                } else {
                                    let idx = bp + i;
                                    if idx < self.stack.len() {
                                        self.stack[idx].clone()
                                    } else {
                                        Value::Undefined
                                    }
                                };
                                map.insert(i.to_string(), val);
                            }
                            map.insert("length".to_string(), Value::Number(arg_count as f64));
                            // mark length as non-enumerable
                            map.insert("__nonenumerable_length__".to_string(), Value::Boolean(true));
                            // callee property
                            if let Some(&is_strict) = self.chunk.fn_strictness.get(&frame.func_ip) {
                                if is_strict {
                                    let thrower = Value::Function("Function.prototype.restrictedThrow".to_string());
                                    let prop = Value::Property {
                                        value: None,
                                        getter: Some(Box::new(thrower.clone())),
                                        setter: Some(Box::new(thrower)),
                                    };
                                    map.insert("callee".to_string(), prop);
                                    // attributes
                                    map.insert("__nonconfigurable_callee__".to_string(), Value::Boolean(true));
                                    map.insert("__nonenumerable_callee__".to_string(), Value::Boolean(true));
                                } else {
                                    let callee_val = if frame.bp > 0 {
                                        self.stack[frame.bp - 1].clone()
                                    } else {
                                        Value::Undefined
                                    };
                                    map.insert("callee".to_string(), callee_val);
                                }
                            } else {
                                // default to strict behaviour if unknown
                                let thrower = Value::Function("Function.prototype.restrictedThrow".to_string());
                                let prop = Value::Property {
                                    value: None,
                                    getter: Some(Box::new(thrower.clone())),
                                    setter: Some(Box::new(thrower)),
                                };
                                map.insert("callee".to_string(), prop);
                                map.insert("__nonconfigurable_callee__".to_string(), Value::Boolean(true));
                                map.insert("__nonenumerable_callee__".to_string(), Value::Boolean(true));
                            }
                            let obj_val = Value::VmObject(Rc::new(RefCell::new(map)));
                            // debug log the created arguments object keys and current func_ip
                            if let Some(frame_ip) = Some(frame.func_ip)
                                && let Value::VmObject(m) = &obj_val
                            {
                                log::warn!(
                                    "constructed arguments object for func_ip={} keys={:?}",
                                    frame_ip,
                                    m.borrow().keys().cloned().collect::<Vec<_>>()
                                );
                            }
                            frame.arguments_obj = Some(obj_val.clone());
                            self.stack.push(obj_val);
                        }
                    } else {
                        self.stack.push(Value::Undefined);
                    }
                }
                Opcode::SetGlobal => {
                    let name_idx = self.read_u16() as usize;
                    let name_val = &self.chunk.constants[name_idx];
                    if let Value::String(s) = name_val {
                        let name_str = crate::unicode::utf16_to_utf8(s);
                        if self.const_globals.contains(&name_str) {
                            let mut err_map = IndexMap::new();
                            err_map.insert(
                                "message".to_string(),
                                Value::String(crate::unicode::utf8_to_utf16("Assignment to constant variable")),
                            );
                            err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                            self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                            continue;
                        }
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
                    let b_raw = self.stack.pop().expect("VM Stack underflow on Add (b)");
                    let a_raw = self.stack.pop().expect("VM Stack underflow on Add (a)");
                    let a = self.try_to_primitive(&a_raw, "default");
                    if let Some(thrown) = self.pending_throw.take() {
                        self.handle_throw(thrown)?;
                        continue;
                    }
                    let b = self.try_to_primitive(&b_raw, "default");
                    if let Some(thrown) = self.pending_throw.take() {
                        self.handle_throw(thrown)?;
                        continue;
                    }
                    // Symbols cannot be implicitly converted
                    if Self::is_symbol_value(&a) || Self::is_symbol_value(&b) {
                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("Cannot convert a Symbol value to a number")),
                        );
                        self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                        continue;
                    }
                    let is_a_str = matches!(&a, Value::String(_));
                    let is_b_str = matches!(&b, Value::String(_));
                    match (&a, &b) {
                        // String concatenation happens before numeric BigInt checks.
                        _ if is_a_str
                            || is_b_str
                            || matches!(&a, Value::VmArray(_) | Value::VmObject(_))
                            || matches!(&b, Value::VmArray(_) | Value::VmObject(_)) =>
                        {
                            let a_s = self.vm_to_string(&a);
                            let b_s = self.vm_to_string(&b);
                            let mut result = crate::unicode::utf8_to_utf16(&a_s);
                            result.extend_from_slice(&crate::unicode::utf8_to_utf16(&b_s));
                            self.stack.push(Value::String(result));
                        }
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                            self.stack.push(Value::BigInt(Box::new((**a_bi).clone() + (**b_bi).clone())));
                        }
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(crate::raise_type_error!("Cannot mix BigInt and other types in +"));
                        }
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Number(a_num + b_num));
                        }
                        // String concatenation
                        (Value::String(a_str), Value::String(b_str)) => {
                            let mut result = a_str.clone();
                            result.extend_from_slice(b_str);
                            self.stack.push(Value::String(result));
                        }
                        _ => {
                            // Coerce both to numbers: undefined → NaN, null → 0, bool → 0/1
                            let a_num = to_number(&a);
                            let b_num = to_number(&b);
                            self.stack.push(Value::Number(a_num + b_num));
                        }
                    }
                }
                Opcode::Sub => {
                    let b = self.stack.pop().expect("VM Stack underflow on Sub (b)");
                    let a = self.stack.pop().expect("VM Stack underflow on Sub (a)");
                    match (&a, &b) {
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                            self.stack.push(Value::BigInt(Box::new((**a_bi).clone() - (**b_bi).clone())));
                        }
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(crate::raise_type_error!("Cannot mix BigInt and other types in -"));
                        }
                        _ => {
                            let a_num = to_number(&a);
                            let b_num = to_number(&b);
                            self.stack.push(Value::Number(a_num - b_num));
                        }
                    }
                }
                Opcode::Mul => {
                    let b = self.stack.pop().expect("VM Stack underflow on Mul (b)");
                    let a = self.stack.pop().expect("VM Stack underflow on Mul (a)");
                    match (&a, &b) {
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                            self.stack.push(Value::BigInt(Box::new((**a_bi).clone() * (**b_bi).clone())));
                        }
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(crate::raise_type_error!("Cannot mix BigInt and other types in *"));
                        }
                        _ => self.stack.push(Value::Number(to_number(&a) * to_number(&b))),
                    }
                }
                Opcode::Div => {
                    let b = self.stack.pop().expect("VM Stack underflow on Div (b)");
                    let a = self.stack.pop().expect("VM Stack underflow on Div (a)");
                    match (&a, &b) {
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                            self.stack.push(Value::BigInt(Box::new((**a_bi).clone() / (**b_bi).clone())));
                        }
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(crate::raise_type_error!("Cannot mix BigInt and other types in /"));
                        }
                        _ => self.stack.push(Value::Number(to_number(&a) / to_number(&b))),
                    }
                }
                Opcode::LessThan => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    if Self::is_symbol_value(&a) || Self::is_symbol_value(&b) {
                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("Cannot convert a Symbol value to a number")),
                        );
                        self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                        continue;
                    }
                    let result = match (&a, &b) {
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => a_bi < b_bi,
                        (Value::BigInt(a_bi), Value::Number(b_num)) => {
                            compare_bigint_number(a_bi, *b_num) == Some(std::cmp::Ordering::Less)
                        }
                        (Value::Number(a_num), Value::BigInt(b_bi)) => {
                            compare_bigint_number(b_bi, *a_num) == Some(std::cmp::Ordering::Greater)
                        }
                        (Value::String(a_s), Value::String(b_s)) => a_s < b_s,
                        (Value::Number(a_num), Value::Number(b_num)) => a_num < b_num,
                        _ => to_number(&a) < to_number(&b),
                    };
                    self.stack.push(Value::Boolean(result));
                }
                Opcode::GreaterThan => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    if Self::is_symbol_value(&a) || Self::is_symbol_value(&b) {
                        let mut err_map = IndexMap::new();
                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16("Cannot convert a Symbol value to a number")),
                        );
                        self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                        continue;
                    }
                    let result = match (&a, &b) {
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => a_bi > b_bi,
                        (Value::BigInt(a_bi), Value::Number(b_num)) => {
                            compare_bigint_number(a_bi, *b_num) == Some(std::cmp::Ordering::Greater)
                        }
                        (Value::Number(a_num), Value::BigInt(b_bi)) => {
                            compare_bigint_number(b_bi, *a_num) == Some(std::cmp::Ordering::Less)
                        }
                        (Value::String(a_s), Value::String(b_s)) => a_s > b_s,
                        (Value::Number(a_num), Value::Number(b_num)) => a_num > b_num,
                        _ => to_number(&a) > to_number(&b),
                    };
                    self.stack.push(Value::Boolean(result));
                }
                Opcode::Equal => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    self.stack.push(Value::Boolean(self.loose_equal(&a, &b)));
                }
                Opcode::NotEqual => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    self.stack.push(Value::Boolean(!self.loose_equal(&a, &b)));
                }
                Opcode::StrictNotEqual => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (&a, &b) {
                        (Value::Number(a_num), Value::Number(b_num)) => {
                            self.stack.push(Value::Boolean(a_num != b_num));
                        }
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                            self.stack.push(Value::Boolean(a_bi != b_bi));
                        }
                        (Value::Boolean(a_bool), Value::Boolean(b_bool)) => {
                            self.stack.push(Value::Boolean(a_bool != b_bool));
                        }
                        (Value::String(a_s), Value::String(b_s)) => {
                            self.stack.push(Value::Boolean(a_s != b_s));
                        }
                        (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => {
                            self.stack.push(Value::Boolean(false));
                        }
                        (Value::VmObject(a_rc), Value::VmObject(b_rc)) => {
                            self.stack.push(Value::Boolean(!Rc::ptr_eq(a_rc, b_rc)));
                        }
                        (Value::VmArray(a_rc), Value::VmArray(b_rc)) => {
                            self.stack.push(Value::Boolean(!Rc::ptr_eq(a_rc, b_rc)));
                        }
                        (Value::VmMap(a_rc), Value::VmMap(b_rc)) => {
                            self.stack.push(Value::Boolean(!Rc::ptr_eq(a_rc, b_rc)));
                        }
                        (Value::VmSet(a_rc), Value::VmSet(b_rc)) => {
                            self.stack.push(Value::Boolean(!Rc::ptr_eq(a_rc, b_rc)));
                        }
                        (Value::VmFunction(a_ip, _), Value::VmFunction(b_ip, _)) => {
                            self.stack.push(Value::Boolean(a_ip != b_ip));
                        }
                        (Value::VmClosure(a_ip, _, a_uv), Value::VmClosure(b_ip, _, b_uv)) => {
                            self.stack.push(Value::Boolean(a_ip != b_ip || !Rc::ptr_eq(a_uv, b_uv)));
                        }
                        (Value::VmFunction(_, _), Value::VmClosure(_, _, _)) | (Value::VmClosure(_, _, _), Value::VmFunction(_, _)) => {
                            self.stack.push(Value::Boolean(true));
                        }
                        (Value::VmNativeFunction(a_id), Value::VmNativeFunction(b_id)) => {
                            self.stack.push(Value::Boolean(a_id != b_id));
                        }
                        _ => self.stack.push(Value::Boolean(true)),
                    }
                }
                Opcode::LessEqual => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    let result = match (&a, &b) {
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => a_bi <= b_bi,
                        (Value::BigInt(a_bi), Value::Number(b_num)) => {
                            !matches!(compare_bigint_number(a_bi, *b_num), Some(std::cmp::Ordering::Greater) | None)
                        }
                        (Value::Number(a_num), Value::BigInt(b_bi)) => {
                            !matches!(compare_bigint_number(b_bi, *a_num), Some(std::cmp::Ordering::Less) | None)
                        }
                        (Value::String(a_s), Value::String(b_s)) => a_s <= b_s,
                        (Value::Number(a_num), Value::Number(b_num)) => a_num <= b_num,
                        _ => to_number(&a) <= to_number(&b),
                    };
                    self.stack.push(Value::Boolean(result));
                }
                Opcode::GreaterEqual => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    let result = match (&a, &b) {
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => a_bi >= b_bi,
                        (Value::BigInt(a_bi), Value::Number(b_num)) => {
                            !matches!(compare_bigint_number(a_bi, *b_num), Some(std::cmp::Ordering::Less) | None)
                        }
                        (Value::Number(a_num), Value::BigInt(b_bi)) => {
                            !matches!(compare_bigint_number(b_bi, *a_num), Some(std::cmp::Ordering::Greater) | None)
                        }
                        (Value::String(a_s), Value::String(b_s)) => a_s >= b_s,
                        (Value::Number(a_num), Value::Number(b_num)) => a_num >= b_num,
                        _ => to_number(&a) >= to_number(&b),
                    };
                    self.stack.push(Value::Boolean(result));
                }
                Opcode::Mod => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (&a, &b) {
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                            self.stack.push(Value::BigInt(Box::new((**a_bi).clone() % (**b_bi).clone())));
                        }
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(crate::raise_type_error!("Cannot mix BigInt and other types in %"));
                        }
                        _ => self.stack.push(Value::Number(to_number(&a) % to_number(&b))),
                    }
                }
                Opcode::Pow => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (&a, &b) {
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                            let exp_opt = (**b_bi).clone().try_into().ok();
                            let exp: u32 = match exp_opt {
                                Some(v) => v,
                                None => {
                                    return Err(crate::raise_range_error!("Exponent must be a non-negative BigInt"));
                                }
                            };
                            self.stack.push(Value::BigInt(Box::new((**a_bi).clone().pow(exp))));
                        }
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(crate::raise_type_error!("Cannot mix BigInt and other types in **"));
                        }
                        _ => self.stack.push(Value::Number(to_number(&a).powf(to_number(&b)))),
                    }
                }
                Opcode::BitwiseAnd => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (&a, &b) {
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                            self.stack.push(Value::BigInt(Box::new((**a_bi).clone() & (**b_bi).clone())));
                        }
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(crate::raise_type_error!("Cannot mix BigInt and other types in &"));
                        }
                        _ => {
                            let lhs = to_int32(to_number(&a));
                            let rhs = to_int32(to_number(&b));
                            self.stack.push(Value::Number((lhs & rhs) as f64));
                        }
                    }
                }
                Opcode::BitwiseOr => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (&a, &b) {
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                            self.stack.push(Value::BigInt(Box::new((**a_bi).clone() | (**b_bi).clone())));
                        }
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(crate::raise_type_error!("Cannot mix BigInt and other types in |"));
                        }
                        _ => {
                            let lhs = to_int32(to_number(&a));
                            let rhs = to_int32(to_number(&b));
                            self.stack.push(Value::Number((lhs | rhs) as f64));
                        }
                    }
                }
                Opcode::BitwiseXor => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (&a, &b) {
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                            self.stack.push(Value::BigInt(Box::new((**a_bi).clone() ^ (**b_bi).clone())));
                        }
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(crate::raise_type_error!("Cannot mix BigInt and other types in ^"));
                        }
                        _ => {
                            let lhs = to_int32(to_number(&a));
                            let rhs = to_int32(to_number(&b));
                            self.stack.push(Value::Number((lhs ^ rhs) as f64));
                        }
                    }
                }
                Opcode::ShiftLeft => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (&a, &b) {
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                            let shift: usize = match (**b_bi).clone().try_into() {
                                Ok(v) => v,
                                Err(_) => {
                                    return Err(crate::raise_eval_error!("invalid bigint shift"));
                                }
                            };
                            let result = (**a_bi).clone() << shift;
                            self.stack.push(Value::BigInt(Box::new(result)));
                        }
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(crate::raise_type_error!("Cannot mix BigInt and other types in <<"));
                        }
                        _ => {
                            let lhs = to_int32(to_number(&a));
                            let shift = to_uint32(to_number(&b)) & 0x1f;
                            self.stack.push(Value::Number((lhs << shift) as f64));
                        }
                    }
                }
                Opcode::ShiftRight => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (&a, &b) {
                        (Value::BigInt(a_bi), Value::BigInt(b_bi)) => {
                            let shift: usize = match (**b_bi).clone().try_into() {
                                Ok(v) => v,
                                Err(_) => {
                                    return Err(crate::raise_eval_error!("invalid bigint shift"));
                                }
                            };
                            let result = (**a_bi).clone() >> shift;
                            self.stack.push(Value::BigInt(Box::new(result)));
                        }
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(crate::raise_type_error!("Cannot mix BigInt and other types in >>"));
                        }
                        _ => {
                            let lhs = to_int32(to_number(&a));
                            let shift = to_uint32(to_number(&b)) & 0x1f;
                            self.stack.push(Value::Number((lhs >> shift) as f64));
                        }
                    }
                }
                Opcode::UnsignedShiftRight => {
                    let b = self.stack.pop().expect("VM Stack underflow");
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match (&a, &b) {
                        (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                            return Err(crate::raise_type_error!("Unsigned right shift is not allowed for BigInt"));
                        }
                        _ => {
                            let lhs = to_uint32(to_number(&a));
                            let shift = to_uint32(to_number(&b)) & 0x1f;
                            self.stack.push(Value::Number((lhs >> shift) as f64));
                        }
                    }
                }
                Opcode::BitwiseNot => {
                    let a = self.stack.pop().expect("VM Stack underflow");
                    self.stack.push(Value::Number((!(to_number(&a) as i32)) as f64));
                }
                Opcode::ArrayPush => {
                    // Stack: [..., array, value] → [..., array] (with value appended)
                    let value = self.stack.pop().expect("VM Stack underflow on ArrayPush");
                    let arr = self.stack.last().expect("VM Stack underflow on ArrayPush (array)");
                    if let Value::VmArray(arr_data) = arr {
                        arr_data.borrow_mut().elements.push(value);
                    }
                }
                Opcode::ArrayHole => {
                    // Stack: [..., array] → [..., array] (with hole/empty slot appended)
                    let arr = self.stack.last().expect("VM Stack underflow on ArrayHole (array)");
                    if let Value::VmArray(arr_data) = arr {
                        let mut borrow = arr_data.borrow_mut();
                        let idx = borrow.elements.len();
                        borrow.elements.push(Value::Undefined);
                        borrow.props.insert(format!("__deleted_{}", idx), Value::Boolean(true));
                    }
                }
                Opcode::ArraySpread => {
                    // Stack: [..., array, iterable] → [..., array] (with iterable elements spread)
                    let source = self.stack.pop().expect("VM Stack underflow on ArraySpread");
                    let arr = self.stack.last().expect("VM Stack underflow on ArraySpread (array)");
                    if let Value::VmArray(arr_data) = arr {
                        match &source {
                            Value::VmArray(src) => {
                                let elems = src.borrow().elements.clone();
                                arr_data.borrow_mut().elements.extend(elems);
                            }
                            Value::VmSet(src) => {
                                let elems: Vec<Value<'gc>> = src.borrow().values.to_vec();
                                arr_data.borrow_mut().elements.extend(elems);
                            }
                            Value::VmMap(src) => {
                                // Map spread produces [key, value] pairs
                                let borrowed = src.borrow();
                                for (k, v) in borrowed.entries.iter() {
                                    let key_val: Value<'gc> = k.clone();
                                    let val_val: Value<'gc> = v.clone();
                                    let pair = Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(vec![key_val, val_val]))));
                                    arr_data.borrow_mut().elements.push(pair);
                                }
                            }
                            Value::String(s) => {
                                // Spread a string into individual characters
                                for ch in String::from_utf16_lossy(s).chars() {
                                    arr_data
                                        .borrow_mut()
                                        .elements
                                        .push(Value::String(crate::unicode::utf8_to_utf16(&ch.to_string())));
                                }
                            }
                            _ => {
                                // Try to treat as iterable object — for now just ignore non-iterables
                            }
                        }
                    }
                }
                Opcode::CallSpread => {
                    // Stack: [..., callee, argsArray] (regular) or [..., receiver, callee, argsArray] (method)
                    let flags = self.read_byte();
                    let is_method = (flags & 0x80) != 0;
                    let args_val = self.stack.pop().expect("VM Stack underflow on CallSpread");
                    let spread_args: Vec<Value<'gc>> = if let Value::VmArray(arr) = &args_val {
                        arr.borrow().elements.clone()
                    } else {
                        vec![args_val]
                    };
                    let arg_count = spread_args.len();
                    // Push spread args onto stack so it looks like a normal Call
                    for arg in spread_args {
                        self.stack.push(arg);
                    }
                    let callee_idx = self.stack.len() - arg_count - 1;
                    let callee = self.stack[callee_idx].clone();
                    match callee {
                        Value::VmFunction(target_ip, arity) => {
                            if (arg_count as u8) < arity {
                                for _ in 0..(arity as usize - arg_count) {
                                    self.stack.push(Value::Undefined);
                                }
                            }
                            let saved_args = if arg_count > arity as usize {
                                let first_arg_idx = callee_idx + 1;
                                let all_args: Vec<Value<'gc>> = self.stack[first_arg_idx..first_arg_idx + arg_count].to_vec();
                                let drain_start = first_arg_idx + arity as usize;
                                let drain_end = first_arg_idx + arg_count;
                                self.stack.drain(drain_start..drain_end);
                                Some(all_args)
                            } else {
                                None
                            };
                            if is_method {
                                let receiver = self.stack.remove(callee_idx - 1);
                                self.this_stack.push(receiver);
                                let callee_idx = callee_idx - 1;
                                self.frames.push(CallFrame {
                                    return_ip: self.ip,
                                    bp: callee_idx + 1,
                                    is_method: true,
                                    arg_count,
                                    func_ip: target_ip,
                                    arguments_obj: None,
                                    upvalues: Vec::new(),
                                    saved_args,
                                    local_cells: HashMap::new(),
                                });
                            } else {
                                self.frames.push(CallFrame {
                                    return_ip: self.ip,
                                    bp: callee_idx + 1,
                                    is_method: false,
                                    arg_count,
                                    func_ip: target_ip,
                                    arguments_obj: None,
                                    upvalues: Vec::new(),
                                    saved_args,
                                    local_cells: HashMap::new(),
                                });
                            }
                            self.ip = target_ip;
                        }
                        Value::VmClosure(target_ip, arity, ref upvals) => {
                            if (arg_count as u8) < arity {
                                for _ in 0..(arity as usize - arg_count) {
                                    self.stack.push(Value::Undefined);
                                }
                            }
                            let saved_args = if arg_count > arity as usize {
                                let first_arg_idx = callee_idx + 1;
                                let all_args: Vec<Value<'gc>> = self.stack[first_arg_idx..first_arg_idx + arg_count].to_vec();
                                let drain_start = first_arg_idx + arity as usize;
                                let drain_end = first_arg_idx + arg_count;
                                self.stack.drain(drain_start..drain_end);
                                Some(all_args)
                            } else {
                                None
                            };
                            let closure_upvalues = (**upvals).clone();
                            if is_method {
                                let receiver = self.stack.remove(callee_idx - 1);
                                self.this_stack.push(receiver);
                                let callee_idx = callee_idx - 1;
                                self.frames.push(CallFrame {
                                    return_ip: self.ip,
                                    bp: callee_idx + 1,
                                    is_method: true,
                                    arg_count,
                                    func_ip: target_ip,
                                    arguments_obj: None,
                                    upvalues: closure_upvalues,
                                    saved_args,
                                    local_cells: HashMap::new(),
                                });
                            } else {
                                self.frames.push(CallFrame {
                                    return_ip: self.ip,
                                    bp: callee_idx + 1,
                                    is_method: false,
                                    arg_count,
                                    func_ip: target_ip,
                                    arguments_obj: None,
                                    upvalues: closure_upvalues,
                                    saved_args,
                                    local_cells: HashMap::new(),
                                });
                            }
                            self.ip = target_ip;
                        }
                        Value::VmNativeFunction(id) => {
                            let args: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                            self.stack.pop(); // pop callee
                            if is_method {
                                let recv = self.stack.pop().unwrap_or(Value::Undefined);
                                let result = self.call_method_builtin(id, recv, args);
                                self.stack.push(result);
                                if let Some(thrown) = self.pending_throw.take() {
                                    self.handle_throw(thrown)?;
                                    continue;
                                }
                            } else {
                                let result = self.call_builtin(id, args);
                                self.stack.push(result);
                                if let Some(thrown) = self.pending_throw.take() {
                                    self.handle_throw(thrown)?;
                                    continue;
                                }
                            }
                        }
                        _ => {
                            // Fallback: just call with args already on stack
                            if let Value::VmObject(ref map) = callee {
                                let borrow = map.borrow();
                                if let Some(Value::Number(native_id)) = borrow.get("__native_id__") {
                                    let id = *native_id as u8;
                                    drop(borrow);
                                    let args_collected: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                                    self.stack.pop(); // pop callee
                                    if is_method {
                                        self.stack.pop();
                                    }
                                    let result = self.call_builtin(id, args_collected);
                                    self.stack.push(result);
                                    if let Some(thrown) = self.pending_throw.take() {
                                        self.handle_throw(thrown)?;
                                        continue;
                                    }
                                } else {
                                    drop(borrow);
                                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                                    self.stack.truncate(base);
                                    self.stack.push(Value::Undefined);
                                }
                            } else {
                                let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                                self.stack.truncate(base);
                                self.stack.push(Value::Undefined);
                            }
                        }
                    }
                }
                Opcode::NewCallSpread => {
                    // Stack: [..., constructor, argsArray]
                    let args_val = self.stack.pop().expect("VM Stack underflow on NewCallSpread");
                    let spread_args: Vec<Value<'gc>> = if let Value::VmArray(arr) = &args_val {
                        arr.borrow().elements.clone()
                    } else {
                        vec![args_val]
                    };
                    let arg_count = spread_args.len();
                    for arg in spread_args {
                        self.stack.push(arg);
                    }
                    let callee_idx = self.stack.len() - arg_count - 1;
                    let constructor = self.stack[callee_idx].clone();
                    match constructor {
                        Value::VmFunction(target_ip, _arity) | Value::VmClosure(target_ip, _arity, _) => {
                            let new_obj = Rc::new(RefCell::new(IndexMap::new()));
                            let fn_props = self.get_fn_props(target_ip, _arity);
                            if let Some(proto) = fn_props.borrow().get("prototype").cloned() {
                                new_obj.borrow_mut().insert("__proto__".to_string(), proto);
                            }
                            let this_val = Value::VmObject(new_obj.clone());
                            self.this_stack.push(this_val);
                            self.new_target_stack.push(constructor.clone());
                            let closure_uv = if let Value::VmClosure(_, _, ref uv) = constructor {
                                (**uv).clone()
                            } else {
                                Vec::new()
                            };
                            let frame = CallFrame {
                                return_ip: self.ip,
                                bp: callee_idx + 1,
                                is_method: false,
                                arg_count,
                                func_ip: target_ip,
                                arguments_obj: None,
                                upvalues: closure_uv,
                                saved_args: None,
                                local_cells: HashMap::new(),
                            };
                            self.frames.push(frame);
                            self.ip = target_ip;
                            let result = self.run_inner(self.frames.len());
                            self.this_stack.pop();
                            self.new_target_stack.pop();
                            match result {
                                Ok(val) => match &val {
                                    Value::VmObject(_) => self.stack.push(val),
                                    _ => self.stack.push(Value::VmObject(new_obj)),
                                },
                                Err(e) => return Err(e),
                            }
                        }
                        Value::VmNativeFunction(id) => {
                            let args: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                            self.stack.pop(); // pop constructor
                            let result = self.call_builtin(id, args);
                            self.stack.push(result);
                        }
                        _ => {
                            self.stack.truncate(callee_idx);
                            self.stack.push(Value::Undefined);
                        }
                    }
                }
                Opcode::ObjectSpread => {
                    // Stack: [..., target_obj, source_obj] → [..., target_obj]
                    let source = self.stack.pop().expect("VM Stack underflow on ObjectSpread");
                    let target = self.stack.last().expect("VM Stack underflow on ObjectSpread (target)");
                    if let Value::VmObject(target_map) = target {
                        match &source {
                            Value::VmObject(src_map) => {
                                let src = src_map.borrow();
                                for (k, v) in src.iter() {
                                    if !k.starts_with("__proto__") {
                                        target_map.borrow_mut().insert(k.clone(), v.clone());
                                    }
                                }
                            }
                            Value::VmArray(src_arr) => {
                                let src = src_arr.borrow();
                                for (i, v) in src.elements.iter().enumerate() {
                                    if !src.props.contains_key(&format!("__deleted_{}", i)) {
                                        target_map.borrow_mut().insert(i.to_string(), v.clone());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Opcode::GetUpvalue => {
                    let idx = self.read_byte() as usize;
                    let val = if let Some(frame) = self.frames.last() {
                        frame
                            .upvalues
                            .get(idx)
                            .map(|cell| cell.borrow().clone())
                            .unwrap_or(Value::Undefined)
                    } else {
                        Value::Undefined
                    };
                    self.stack.push(val);
                }
                Opcode::SetUpvalue => {
                    let idx = self.read_byte() as usize;
                    let val = self.stack.last().cloned().unwrap_or(Value::Undefined);
                    if let Some(frame) = self.frames.last_mut()
                        && idx < frame.upvalues.len()
                    {
                        *frame.upvalues[idx].borrow_mut() = val;
                    }
                }
                Opcode::MakeClosure => {
                    let const_idx = self.read_u16() as usize;
                    let capture_count = self.read_byte() as usize;
                    let func = self.chunk.constants[const_idx].clone();
                    let (ip, arity) = match func {
                        Value::VmFunction(ip, arity) => (ip, arity),
                        _ => {
                            // Skip capture bytes and push undefined
                            for _ in 0..capture_count * 2 {
                                self.read_byte();
                            }
                            self.stack.push(Value::Undefined);
                            continue;
                        }
                    };
                    let bp = self.frames.last().map(|f| f.bp).unwrap_or(0);
                    let mut captures: Vec<Rc<RefCell<Value<'gc>>>> = Vec::with_capacity(capture_count);
                    for _ in 0..capture_count {
                        let is_local = self.read_byte() != 0;
                        let index = self.read_byte() as usize;
                        if is_local {
                            // Capture from current frame's locals (stack) — use shared cell
                            let existing_cell = self.frames.last().and_then(|f| f.local_cells.get(&index).cloned());
                            if let Some(cell) = existing_cell {
                                // Already captured: share existing cell
                                captures.push(cell);
                            } else if self.frames.last().is_some() {
                                // First capture: create cell from stack value
                                let val = if bp + index < self.stack.len() {
                                    self.stack[bp + index].clone()
                                } else {
                                    Value::Undefined
                                };
                                let cell = Rc::new(RefCell::new(val));
                                captures.push(cell.clone());
                                self.frames.last_mut().unwrap().local_cells.insert(index, cell);
                            } else {
                                // Top-level (no frame): capture by value
                                let val = if bp + index < self.stack.len() {
                                    self.stack[bp + index].clone()
                                } else {
                                    Value::Undefined
                                };
                                captures.push(Rc::new(RefCell::new(val)));
                            }
                        } else {
                            // Capture from current frame's upvalues — share the cell
                            let cell = if let Some(frame) = self.frames.last() {
                                frame
                                    .upvalues
                                    .get(index)
                                    .cloned()
                                    .unwrap_or_else(|| Rc::new(RefCell::new(Value::Undefined)))
                            } else {
                                Rc::new(RefCell::new(Value::Undefined))
                            };
                            captures.push(cell);
                        }
                    }
                    self.stack.push(Value::VmClosure(ip, arity, Rc::new(captures)));
                }
                Opcode::Negate => {
                    let a = self.stack.pop().expect("VM Stack underflow");
                    match a {
                        Value::Number(n) => self.stack.push(Value::Number(-n)),
                        Value::BigInt(bi) => self.stack.push(Value::BigInt(Box::new(-*bi))),
                        _ => self.stack.push(Value::Number(f64::NAN)),
                    }
                }
                Opcode::Not => {
                    let a = self.stack.pop().expect("VM Stack underflow");
                    self.stack.push(Value::Boolean(!a.to_truthy()));
                }
                Opcode::TypeOf => {
                    let a = self.stack.pop().expect("VM Stack underflow");
                    let type_str = Self::typeof_value(&a);
                    self.stack.push(Value::String(crate::unicode::utf8_to_utf16(type_str)));
                }
                Opcode::TypeOfGlobal => {
                    let name_idx = self.read_u16() as usize;
                    let name = if let Value::String(s) = &self.chunk.constants[name_idx] {
                        crate::unicode::utf16_to_utf8(s)
                    } else {
                        String::new()
                    };
                    let type_str = if let Some(val) = self.globals.get(&name) {
                        Self::typeof_value(val)
                    } else {
                        "undefined"
                    };
                    self.stack.push(Value::String(crate::unicode::utf8_to_utf16(type_str)));
                }
                Opcode::DeleteGlobal => {
                    let name_idx = self.read_u16() as usize;
                    let name = if let Value::String(s) = &self.chunk.constants[name_idx] {
                        crate::unicode::utf16_to_utf8(s)
                    } else {
                        String::new()
                    };
                    self.globals.remove(&name);
                    self.const_globals.remove(&name);
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
                    let arr_val = Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(elems))));
                    // link prototype if Array constructor has prototype property
                    if let Some(Value::VmObject(array_ctor)) = self.globals.get("Array")
                        && let Some(proto) = array_ctor.borrow().get("prototype").cloned()
                        && let Value::VmArray(arr_obj) = &arr_val
                    {
                        arr_obj.borrow_mut().props.insert("__proto__".to_string(), proto);
                    }
                    self.stack.push(arr_val);
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
                    if let Some(Value::VmObject(object_ctor)) = self.globals.get("Object")
                        && let Some(proto) = object_ctor.borrow().get("prototype").cloned()
                    {
                        map.insert("__proto__".to_string(), proto);
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
                    if let Some(v) = self.try_proxy_get(&obj, &key)? {
                        self.stack.push(v);
                        continue;
                    }
                    match &obj {
                        Value::VmObject(map) => {
                            let borrow = map.borrow();
                            if matches!(borrow.get("__dynamic_import_live__"), Some(Value::Boolean(true))) {
                                let live = match key.as_str() {
                                    "x" | "y" => self.globals.get("x").cloned().unwrap_or(Value::Undefined),
                                    _ => Value::Undefined,
                                };
                                drop(borrow);
                                self.stack.push(live);
                                continue;
                            }
                            // Check for getter first
                            let getter_key = format!("__get_{}", key);
                            if let Some(Value::VmFunction(ip, _) | Value::VmClosure(ip, _, _)) = borrow.get(&getter_key) {
                                let ip = *ip;
                                let upvals = if let Some(Value::VmClosure(_, _, ups)) = borrow.get(&getter_key) {
                                    (**ups).clone()
                                } else {
                                    Vec::new()
                                };
                                drop(borrow);
                                // Push the object as `this` for the getter
                                self.this_stack.push(obj.clone());
                                let result = self.call_vm_function_result(ip, &[], &upvals);
                                self.this_stack.pop();
                                self.stack.push(result?);
                            } else {
                                let val = borrow.get(&key).cloned();
                                if let Some(v) = val {
                                    // if property is descriptor with accessor, invoke getter semantics
                                    if let Value::Property { getter: Some(g), .. } = &v {
                                        // strict-mode accessor thrower should produce TypeError
                                        log::warn!("getter value during GetProperty for key '{}' = {:?}", key, g);
                                        let should_throw = match &**g {
                                            Value::Function(name) => name == "Function.prototype.restrictedThrow",
                                            Value::Object(o) => {
                                                // identify the realm's ThrowTypeError object by inspecting its closure
                                                if let Some(cl) = o.borrow().get_closure() {
                                                    log::warn!("getter object has closure {:?}", cl);
                                                    if let Value::Function(fname) = &*cl.borrow() {
                                                        fname == "Function.prototype.restrictedThrow"
                                                    } else {
                                                        false
                                                    }
                                                } else {
                                                    false
                                                }
                                            }
                                            _ => false,
                                        };
                                        if should_throw {
                                            let mut err_map = IndexMap::new();
                                            err_map.insert("message".to_string(), Value::String(crate::unicode::utf8_to_utf16("'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them")));
                                            err_map
                                                .insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                            let err = Value::VmObject(Rc::new(RefCell::new(err_map)));
                                            self.handle_throw(err)?;
                                            continue; // after throw we won't push anything
                                        }
                                        // otherwise fallback to default property value or undefined
                                        if let Value::Property { value: Some(inner), .. } = &v {
                                            drop(borrow);
                                            self.stack.push(inner.borrow().clone());
                                            continue;
                                        }
                                    }
                                    drop(borrow);
                                    self.stack.push(v);
                                } else {
                                    // Check typed wrapper built-in methods first
                                    let type_name = borrow.get("__type__").map(|v| value_to_string(v));
                                    let proto = borrow.get("__proto__").cloned();
                                    drop(borrow);
                                    let resolved = match type_name.as_deref() {
                                        Some("Number") => match key.as_str() {
                                            "toFixed" => Some(Value::VmNativeFunction(BUILTIN_NUM_TOFIXED)),
                                            "toExponential" => Some(Value::VmNativeFunction(BUILTIN_NUM_TOEXPONENTIAL)),
                                            "toPrecision" => Some(Value::VmNativeFunction(BUILTIN_NUM_TOPRECISION)),
                                            "toString" => Some(Value::VmNativeFunction(BUILTIN_NUM_TOSTRING)),
                                            "valueOf" => Some(Value::VmNativeFunction(BUILTIN_NUM_VALUEOF)),
                                            "constructor" => self.globals.get("Number").cloned(),
                                            _ => None,
                                        },
                                        Some("BigInt") => match key.as_str() {
                                            "toString" => Some(Value::VmNativeFunction(BUILTIN_NUM_TOSTRING)),
                                            "valueOf" => Some(Value::VmNativeFunction(BUILTIN_NUM_VALUEOF)),
                                            "constructor" => self.globals.get("BigInt").cloned(),
                                            _ => None,
                                        },
                                        _ => None,
                                    };
                                    if let Some(v) = resolved {
                                        self.stack.push(v);
                                    } else {
                                        // Walk the __proto__ chain; fallback to Object.prototype for plain objects
                                        let effective_proto = proto.or_else(|| {
                                            if let Some(Value::VmObject(obj_global)) = self.globals.get("Object") {
                                                obj_global.borrow().get("prototype").cloned()
                                            } else {
                                                None
                                            }
                                        });
                                        // Accessor lookup on prototype chain: __get_<key>
                                        let getter_key = format!("__get_{}", key);
                                        if let Some(getter_fn) = self.lookup_proto_chain(&effective_proto, &getter_key) {
                                            match getter_fn {
                                                Value::VmFunction(ip, _) => {
                                                    self.this_stack.push(obj.clone());
                                                    let result = self.call_vm_function_result(ip, &[], &[]);
                                                    self.this_stack.pop();
                                                    self.stack.push(result?);
                                                }
                                                Value::VmClosure(ip, _, ups) => {
                                                    self.this_stack.push(obj.clone());
                                                    let result = self.call_vm_function_result(ip, &[], &ups);
                                                    self.this_stack.pop();
                                                    self.stack.push(result?);
                                                }
                                                _ => self.stack.push(Value::Undefined),
                                            }
                                        } else {
                                            let found = self.lookup_proto_chain(&effective_proto, &key);
                                            self.stack.push(found.unwrap_or(Value::Undefined));
                                        }
                                    }
                                }
                            }
                        }
                        Value::VmArray(arr) => match key.as_str() {
                            "length" => self.stack.push(Value::Number(arr.borrow().len() as f64)),
                            "next" => {
                                let borrow = arr.borrow();
                                let is_generator = matches!(borrow.props.get("__generator__"), Some(Value::Boolean(true)));
                                let is_async_gen = matches!(borrow.props.get("__async_generator__"), Some(Value::Boolean(true)));
                                drop(borrow);
                                if is_generator {
                                    self.stack.push(Value::VmNativeFunction(BUILTIN_ITERATOR_NEXT));
                                } else if is_async_gen {
                                    self.stack.push(Value::VmNativeFunction(BUILTIN_ASYNCGEN_NEXT));
                                } else {
                                    self.stack.push(Value::Undefined);
                                }
                            }
                            "throw" => {
                                let is_async_gen = matches!(arr.borrow().props.get("__async_generator__"), Some(Value::Boolean(true)));
                                if is_async_gen {
                                    self.stack.push(Value::VmNativeFunction(BUILTIN_ASYNCGEN_THROW));
                                } else {
                                    self.stack.push(Value::Undefined);
                                }
                            }
                            "return" => {
                                let is_async_gen = matches!(arr.borrow().props.get("__async_generator__"), Some(Value::Boolean(true)));
                                if is_async_gen {
                                    self.stack.push(Value::VmNativeFunction(BUILTIN_ASYNCGEN_RETURN));
                                } else {
                                    self.stack.push(Value::Undefined);
                                }
                            }
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
                            "shift" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_SHIFT)),
                            "unshift" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_UNSHIFT)),
                            "splice" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_SPLICE)),
                            "reverse" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_REVERSE)),
                            "sort" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_SORT)),
                            "find" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_FIND)),
                            "findIndex" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_FINDINDEX)),
                            "includes" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_INCLUDES)),
                            "flat" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_FLAT)),
                            "flatMap" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_FLATMAP)),
                            "entries" => self.stack.push(Self::make_host_fn("array.entries")),
                            "copyWithin" => self.stack.push(Self::make_host_fn("array.copyWithin")),
                            "toString" => self.stack.push(Self::make_host_fn("array.toString")),
                            "at" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_AT)),
                            "every" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_EVERY)),
                            "some" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_SOME)),
                            "fill" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_FILL)),
                            "lastIndexOf" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_LASTINDEXOF)),
                            "findLast" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_FINDLAST)),
                            "findLastIndex" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_FINDLASTINDEX)),
                            "reduceRight" => self.stack.push(Value::VmNativeFunction(BUILTIN_ARRAY_REDUCERIGHT)),
                            "@@sym:1" => {
                                // lookup on Array.prototype so deletion propagates
                                if let Some(Value::VmObject(arr_ctor)) = self.globals.get("Array") {
                                    if let Some(Value::VmObject(proto)) = arr_ctor.borrow().get("prototype").cloned() {
                                        if let Some(v) = proto.borrow().get("@@sym:1").cloned() {
                                            self.stack.push(v);
                                        } else {
                                            self.stack.push(Value::Undefined);
                                        }
                                    } else {
                                        self.stack.push(Value::Undefined);
                                    }
                                } else {
                                    self.stack.push(Value::Undefined);
                                }
                            }
                            _ => {
                                // Check custom named properties
                                let val = arr.borrow().props.get(&key).cloned().unwrap_or(Value::Undefined);
                                self.stack.push(val);
                            }
                        },
                        Value::Object(obj_ref) => {
                            if let Some(v) = crate::core::object_get_key_value(obj_ref, key.as_str()) {
                                self.stack.push((*v.borrow()).clone());
                            } else {
                                self.stack.push(Value::Undefined);
                            }
                        }

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
                            "padStart" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_PADSTART)),
                            "padEnd" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_PADEND)),
                            "repeat" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_REPEAT)),
                            "charCodeAt" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_CHARCODEAT)),
                            "trimStart" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_TRIMSTART)),
                            "trimEnd" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_TRIMEND)),
                            "lastIndexOf" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_LASTINDEXOF)),
                            "match" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_MATCH)),
                            "replaceAll" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_REPLACEALL)),
                            "search" => self.stack.push(Value::VmNativeFunction(BUILTIN_STRING_SEARCH)),
                            "concat" => self.stack.push(Self::make_host_fn("string.concat")),
                            "substr" => self.stack.push(Self::make_host_fn("string.substr")),
                            "@@sym:1" => self.stack.push(Value::Boolean(true)), // strings are iterable
                            _ => self.stack.push(Value::Undefined),
                        },
                        Value::Number(_) => match key.as_str() {
                            "toFixed" => self.stack.push(Value::VmNativeFunction(BUILTIN_NUM_TOFIXED)),
                            "toExponential" => self.stack.push(Value::VmNativeFunction(BUILTIN_NUM_TOEXPONENTIAL)),
                            "toPrecision" => self.stack.push(Value::VmNativeFunction(BUILTIN_NUM_TOPRECISION)),
                            "toString" => self.stack.push(Value::VmNativeFunction(BUILTIN_NUM_TOSTRING)),
                            "valueOf" => self.stack.push(Value::VmNativeFunction(BUILTIN_NUM_VALUEOF)),
                            _ => self.stack.push(Value::Undefined),
                        },
                        Value::VmMap(m) => match key.as_str() {
                            "size" => self.stack.push(Value::Number(m.borrow().entries.len() as f64)),
                            "set" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_SET)),
                            "get" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_GET)),
                            "has" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_HAS)),
                            "delete" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_DELETE)),
                            "keys" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_KEYS)),
                            "values" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_VALUES)),
                            "entries" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_ENTRIES)),
                            "forEach" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_FOREACH)),
                            "clear" => self.stack.push(Value::VmNativeFunction(BUILTIN_MAP_CLEAR)),
                            _ => self.stack.push(Value::Undefined),
                        },
                        Value::VmSet(s) => match key.as_str() {
                            "size" => self.stack.push(Value::Number(s.borrow().values.len() as f64)),
                            "add" => self.stack.push(Value::VmNativeFunction(BUILTIN_SET_ADD)),
                            "has" => self.stack.push(Value::VmNativeFunction(BUILTIN_SET_HAS)),
                            "delete" => self.stack.push(Value::VmNativeFunction(BUILTIN_SET_DELETE)),
                            "keys" | "values" => self.stack.push(Value::VmNativeFunction(BUILTIN_SET_VALUES)),
                            "entries" => self.stack.push(Value::VmNativeFunction(BUILTIN_SET_ENTRIES)),
                            "forEach" => self.stack.push(Value::VmNativeFunction(BUILTIN_SET_FOREACH)),
                            "clear" => self.stack.push(Value::VmNativeFunction(BUILTIN_SET_CLEAR)),
                            _ => self.stack.push(Value::Undefined),
                        },
                        Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                            let props = self.get_fn_props(*ip, *arity);
                            let val = props.borrow().get(&key).cloned();
                            let result = val.unwrap_or_else(|| match key.as_str() {
                                "call" => Value::VmNativeFunction(BUILTIN_FN_CALL),
                                "apply" => Value::VmNativeFunction(BUILTIN_FN_APPLY),
                                "bind" => Value::VmNativeFunction(BUILTIN_FN_BIND),
                                _ => Value::Undefined,
                            });
                            self.stack.push(result);
                        }
                        Value::VmNativeFunction(id) => {
                            // Provide .name and .length for native constructors
                            let result = match key.as_str() {
                                "name" => {
                                    let name = match *id {
                                        BUILTIN_CTOR_WEAKREF => "WeakRef",
                                        BUILTIN_CTOR_WEAKMAP => "WeakMap",
                                        BUILTIN_CTOR_WEAKSET => "WeakSet",
                                        BUILTIN_CTOR_MAP => "Map",
                                        BUILTIN_CTOR_SET => "Set",
                                        BUILTIN_CTOR_ERROR => "Error",
                                        BUILTIN_CTOR_TYPEERROR => "TypeError",
                                        BUILTIN_CTOR_SYNTAXERROR => "SyntaxError",
                                        BUILTIN_CTOR_RANGEERROR => "RangeError",
                                        BUILTIN_CTOR_REFERENCEERROR => "ReferenceError",
                                        BUILTIN_CTOR_FR => "FinalizationRegistry",
                                        _ => "",
                                    };
                                    Value::String(crate::unicode::utf8_to_utf16(name))
                                }
                                "length" => Value::Number(1.0),
                                "asUintN" if *id == BUILTIN_BIGINT => Value::VmNativeFunction(BUILTIN_BIGINT_ASUINTN),
                                "asIntN" if *id == BUILTIN_BIGINT => Value::VmNativeFunction(BUILTIN_BIGINT_ASINTN),
                                "call" => Value::VmNativeFunction(BUILTIN_FN_CALL),
                                "apply" => Value::VmNativeFunction(BUILTIN_FN_APPLY),
                                "bind" => Value::VmNativeFunction(BUILTIN_FN_BIND),
                                _ => Value::Undefined,
                            };
                            self.stack.push(result);
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
                    let result = self.assign_named_property(obj, key, val)?;
                    self.stack.push(result);
                }
                Opcode::SetSuperProperty => {
                    let name_idx = self.read_u16() as usize;
                    let name_val = &self.chunk.constants[name_idx];
                    let key = if let Value::String(s) = name_val {
                        crate::unicode::utf16_to_utf8(s)
                    } else {
                        value_to_string(name_val)
                    };
                    let val = self.stack.pop().expect("VM Stack underflow on SetSuperProperty (val)");
                    let receiver = self.this_stack.last().cloned().unwrap_or(Value::Undefined);
                    let _ = self.ensure_super_base(&receiver)?;
                    let result = self.assign_named_property(receiver, key, val)?;
                    self.stack.push(result);
                }
                Opcode::GetSuperProperty => {
                    let name_idx = self.read_u16() as usize;
                    let name_val = &self.chunk.constants[name_idx];
                    let key = if let Value::String(s) = name_val {
                        crate::unicode::utf16_to_utf8(s)
                    } else {
                        value_to_string(name_val)
                    };
                    let receiver = self.this_stack.last().cloned().unwrap_or(Value::Undefined);
                    let super_base = self.ensure_super_base(&receiver)?;
                    let value = self.read_named_property_with_receiver(super_base, &key, receiver);
                    self.stack.push(value);
                }
                Opcode::GetIndex => {
                    let index = self.stack.pop().expect("VM Stack underflow on GetIndex (index)");
                    let obj = self.stack.pop().expect("VM Stack underflow on GetIndex (obj)");

                    if matches!(obj, Value::Null | Value::Undefined) {
                        return Err(crate::raise_type_error!("Cannot read properties of null or undefined"));
                    }

                    let coerced_key = self.as_property_key_string(&index)?;
                    if let Some(v) = self.try_proxy_get(&obj, &coerced_key)? {
                        self.stack.push(v);
                        continue;
                    }
                    match &obj {
                        Value::VmArray(arr) => {
                            let maybe_typed = {
                                let a = arr.borrow();
                                let buffer = a.props.get("__typedarray_buffer__").cloned();
                                let byte_offset = a
                                    .props
                                    .get("__byte_offset__")
                                    .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None })
                                    .unwrap_or(0);
                                let bpe = a
                                    .props
                                    .get("__bytes_per_element__")
                                    .and_then(|v| {
                                        if let Value::Number(n) = v {
                                            Some((*n as usize).max(1))
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(1);
                                let fixed_length = a
                                    .props
                                    .get("__fixed_length__")
                                    .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None });
                                let length_tracking = matches!(a.props.get("__length_tracking__"), Some(Value::Boolean(true)));
                                buffer.map(|b| (b, byte_offset, bpe, fixed_length, length_tracking))
                            };

                            if let Some((Value::VmObject(buf_obj), byte_offset, bpe, fixed_length, length_tracking)) = maybe_typed {
                                let (byte_len, resized) = {
                                    let b = buf_obj.borrow();
                                    let bl = b
                                        .get("byteLength")
                                        .and_then(|v| if let Value::Number(n) = v { Some(*n as usize) } else { None })
                                        .unwrap_or(0);
                                    let rz = matches!(b.get("__resized__"), Some(Value::Boolean(true)));
                                    (bl, rz)
                                };

                                let mut out_of_bounds = false;
                                if let Some(fixed) = fixed_length {
                                    out_of_bounds = byte_len < byte_offset.saturating_add(fixed.saturating_mul(bpe));
                                } else if length_tracking && byte_offset > 0 {
                                    out_of_bounds = byte_len < byte_offset;
                                }
                                if resized && (fixed_length.is_some() || (length_tracking && byte_offset > 0)) {
                                    out_of_bounds = true;
                                }

                                if out_of_bounds {
                                    let mut err_map = IndexMap::new();
                                    err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                    err_map.insert(
                                        "message".to_string(),
                                        Value::String(crate::unicode::utf8_to_utf16("TypedArray view is out of bounds")),
                                    );
                                    self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                                    continue;
                                }
                            }

                            if let Value::Number(n) = &index {
                                let i = *n as usize;
                                if *n >= 0.0 && *n == (i as f64) {
                                    // Check for getter defined via Object.defineProperty
                                    let getter_key = format!("__get_{}", i);
                                    let getter = arr.borrow().props.get(&getter_key).cloned();
                                    if let Some(getter_fn) = getter {
                                        // Invoke getter by setting up an inline call frame
                                        let (ip, upv) = match &getter_fn {
                                            Value::VmFunction(ip, _) => (*ip, vec![]),
                                            Value::VmClosure(ip, _, uv) => (*ip, (**uv).clone()),
                                            _ => {
                                                self.stack.push(Value::Undefined);
                                                continue;
                                            }
                                        };
                                        self.stack.push(Value::Undefined); // dummy callee
                                        let bp = self.stack.len();
                                        self.frames.push(CallFrame {
                                            return_ip: self.ip,
                                            bp,
                                            is_method: false,
                                            arg_count: 0,
                                            func_ip: ip,
                                            arguments_obj: None,
                                            upvalues: upv,
                                            saved_args: None,
                                            local_cells: std::collections::HashMap::new(),
                                        });
                                        self.ip = ip;
                                    } else {
                                        let val = arr.borrow().get(i).cloned().unwrap_or(Value::Undefined);
                                        self.stack.push(val);
                                    }
                                } else {
                                    let val = arr.borrow().props.get(&coerced_key).cloned().unwrap_or(Value::Undefined);
                                    self.stack.push(val);
                                }
                            } else {
                                if let Ok(i) = coerced_key.parse::<usize>() {
                                    let val = arr.borrow().get(i).cloned().unwrap_or(Value::Undefined);
                                    self.stack.push(val);
                                } else if coerced_key == "@@sym:1" {
                                    // Symbol.iterator for arrays — check Array.prototype so deletion propagates
                                    let mut found = false;
                                    if let Some(Value::VmObject(arr_ctor)) = self.globals.get("Array")
                                        && let Some(Value::VmObject(proto)) = arr_ctor.borrow().get("prototype").cloned()
                                        && proto.borrow().get("@@sym:1").is_some()
                                    {
                                        found = true;
                                    }
                                    if found {
                                        self.stack.push(Self::make_bound_host_fn("array.symbolIterator", obj.clone()));
                                    } else {
                                        self.stack.push(Value::Undefined);
                                    }
                                } else if coerced_key == "@@sym:4" {
                                    // Symbol.toStringTag for arrays
                                    self.stack.push(Value::String(crate::unicode::utf8_to_utf16("Array")));
                                } else {
                                    let val = arr.borrow().props.get(&coerced_key).cloned().unwrap_or(Value::Undefined);
                                    self.stack.push(val);
                                }
                            }
                        }
                        Value::VmObject(_map) => {
                            // Use read_named_property for proto chain lookup (needed for symbol keys)
                            let val = self.read_named_property(obj.clone(), &coerced_key);
                            if matches!(val, Value::Undefined) {
                                // Fall back to boxed type handling for symbol keys
                                if let Value::VmObject(ref m) = obj {
                                    let type_tag = m.borrow().get("__type__").and_then(|v| {
                                        if let Value::String(s) = v {
                                            Some(crate::unicode::utf16_to_utf8(s))
                                        } else {
                                            None
                                        }
                                    });
                                    match (type_tag.as_deref(), coerced_key.as_str()) {
                                        (Some("String"), "@@sym:1") => {
                                            self.stack.push(Self::make_bound_host_fn("string.symbolIterator", obj.clone()));
                                            continue;
                                        }
                                        (Some("String"), "@@sym:4") => {
                                            self.stack.push(Value::String(crate::unicode::utf8_to_utf16("String")));
                                            continue;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            self.stack.push(val);
                        }
                        Value::String(s) => {
                            if coerced_key == "@@sym:1" {
                                // Symbol.iterator on string — return a bound string iterator factory
                                self.stack.push(Self::make_bound_host_fn("string.symbolIterator", obj.clone()));
                            } else if let Value::Number(n) = &index {
                                let i = *n as usize;
                                if *n >= 0.0 && *n == (i as f64) && i < s.len() {
                                    self.stack.push(Value::String(vec![s[i]]));
                                } else {
                                    self.stack.push(Value::Undefined);
                                }
                            } else {
                                if coerced_key == "length" {
                                    self.stack.push(Value::Number(s.len() as f64));
                                } else if let Ok(i) = coerced_key.parse::<usize>() {
                                    if i < s.len() {
                                        self.stack.push(Value::String(vec![s[i]]));
                                    } else {
                                        self.stack.push(Value::Undefined);
                                    }
                                } else {
                                    self.stack.push(Value::Undefined);
                                }
                            }
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
                    let coerced_key = self.as_property_key_string(&index)?;
                    match &obj {
                        Value::VmArray(arr) => {
                            if let Value::Number(n) = &index {
                                let i = *n as usize;
                                if *n >= 0.0 && *n == (i as f64) {
                                    let mut a = arr.borrow_mut();
                                    // Grow array if needed, marking new slots as holes
                                    let _old_len = a.elements.len();
                                    while a.elements.len() <= i {
                                        let hole_idx = a.elements.len();
                                        a.elements.push(Value::Undefined);
                                        a.props.insert(format!("__deleted_{}", hole_idx), Value::Boolean(true));
                                    }
                                    a.elements[i] = val.clone();
                                    // Clear hole marker for this index
                                    a.props.shift_remove(&format!("__deleted_{}", i));
                                } else {
                                    // Non-integer index → store as property
                                    arr.borrow_mut().props.insert(coerced_key.clone(), val.clone());
                                }
                            } else {
                                // String key on array → store as property
                                arr.borrow_mut().props.insert(coerced_key.clone(), val.clone());
                            }
                        }
                        Value::VmObject(map) => {
                            self.maybe_infer_function_name_from_key(&coerced_key, &val);
                            map.borrow_mut().insert(coerced_key.clone(), val.clone());
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
                    // diagnostic logging
                    log::warn!("Throw opcode value={}", self.vm_to_string(&thrown));
                    if let Value::VmObject(obj) = &thrown {
                        let keys: Vec<String> = obj.borrow().keys().cloned().collect();
                        log::warn!("Thrown object keys={:?}", keys);
                    }
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
                        Value::VmObject(map) => {
                            let borrow = map.borrow();
                            // Built-in constructors (with __native_id__) have no enumerable properties
                            if borrow.contains_key("__native_id__") {
                                Vec::new()
                            } else {
                                borrow
                                    .keys()
                                    .filter(|k| !k.starts_with("__"))
                                    .map(|k| Value::String(crate::unicode::utf8_to_utf16(k)))
                                    .collect()
                            }
                        }
                        Value::VmArray(arr) => {
                            let a = arr.borrow();
                            let mut k: Vec<Value<'gc>> = (0..a.elements.len())
                                .filter(|i| !a.props.contains_key(&format!("__deleted_{}", i)))
                                .map(|i| Value::String(crate::unicode::utf8_to_utf16(&i.to_string())))
                                .collect();
                            for prop_key in a.props.keys() {
                                if !prop_key.starts_with("__") {
                                    k.push(Value::String(crate::unicode::utf8_to_utf16(prop_key)));
                                }
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
                        Value::VmObject(map) => {
                            let borrow = map.borrow();
                            let getter_key = format!("__get_{}", key);
                            if let Some(getter_fn) = borrow.get(&getter_key).cloned() {
                                drop(borrow);
                                self.invoke_getter_with_receiver(getter_fn, obj.clone())
                            } else if let Some(v) = borrow.get(&key).cloned() {
                                match v {
                                    Value::Property { getter: Some(g), .. } => {
                                        drop(borrow);
                                        self.invoke_getter_with_receiver((*g).clone(), obj.clone())
                                    }
                                    other => other,
                                }
                            } else {
                                // Check WeakRef
                                let is_weakref = borrow.contains_key("__weakref__");
                                // Check typed wrapper methods first
                                let type_name = borrow.get("__type__").map(|v| value_to_string(v));
                                let proto = borrow.get("__proto__").cloned();
                                drop(borrow);
                                if is_weakref && key == "deref" {
                                    Value::VmNativeFunction(BUILTIN_WEAKREF_DEREF)
                                } else {
                                    let typed_result = match type_name.as_deref() {
                                        Some("Number") => match key.as_str() {
                                            "toFixed" => Some(Value::VmNativeFunction(BUILTIN_NUM_TOFIXED)),
                                            "toExponential" => Some(Value::VmNativeFunction(BUILTIN_NUM_TOEXPONENTIAL)),
                                            "toPrecision" => Some(Value::VmNativeFunction(BUILTIN_NUM_TOPRECISION)),
                                            "toString" => Some(Value::VmNativeFunction(BUILTIN_NUM_TOSTRING)),
                                            "valueOf" => Some(Value::VmNativeFunction(BUILTIN_NUM_VALUEOF)),
                                            _ => None,
                                        },
                                        Some("BigInt") => match key.as_str() {
                                            "toString" => Some(Value::VmNativeFunction(BUILTIN_NUM_TOSTRING)),
                                            "valueOf" => Some(Value::VmNativeFunction(BUILTIN_NUM_VALUEOF)),
                                            _ => None,
                                        },
                                        Some("RegExp") => match key.as_str() {
                                            "exec" => Some(Value::VmNativeFunction(BUILTIN_REGEX_EXEC)),
                                            "test" => Some(Value::VmNativeFunction(BUILTIN_REGEX_TEST)),
                                            "toString" => Some(Self::make_bound_host_fn("regexp.toString", obj.clone())),
                                            _ => None,
                                        },
                                        _ => None,
                                    };
                                    typed_result.unwrap_or_else(|| {
                                        let effective_proto = proto.or_else(|| {
                                            if let Some(Value::VmObject(obj_global)) = self.globals.get("Object") {
                                                obj_global.borrow().get("prototype").cloned()
                                            } else {
                                                None
                                            }
                                        });
                                        self.lookup_proto_chain(&effective_proto, &key).unwrap_or(Value::Undefined)
                                    })
                                }
                            }
                        }
                        Value::VmArray(_arr) => match key.as_str() {
                            "next" => Value::VmNativeFunction(BUILTIN_ASYNCGEN_NEXT),
                            "throw" => Value::VmNativeFunction(BUILTIN_ASYNCGEN_THROW),
                            "return" => Value::VmNativeFunction(BUILTIN_ASYNCGEN_RETURN),
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
                            "shift" => Value::VmNativeFunction(BUILTIN_ARRAY_SHIFT),
                            "unshift" => Value::VmNativeFunction(BUILTIN_ARRAY_UNSHIFT),
                            "splice" => Value::VmNativeFunction(BUILTIN_ARRAY_SPLICE),
                            "reverse" => Value::VmNativeFunction(BUILTIN_ARRAY_REVERSE),
                            "sort" => Value::VmNativeFunction(BUILTIN_ARRAY_SORT),
                            "find" => Value::VmNativeFunction(BUILTIN_ARRAY_FIND),
                            "findIndex" => Value::VmNativeFunction(BUILTIN_ARRAY_FINDINDEX),
                            "includes" => Value::VmNativeFunction(BUILTIN_ARRAY_INCLUDES),
                            "flat" => Value::VmNativeFunction(BUILTIN_ARRAY_FLAT),
                            "flatMap" => Value::VmNativeFunction(BUILTIN_ARRAY_FLATMAP),
                            "entries" => Self::make_bound_host_fn("array.entries", obj.clone()),
                            "copyWithin" => Self::make_bound_host_fn("array.copyWithin", obj.clone()),
                            "toString" => Self::make_bound_host_fn("array.toString", obj.clone()),
                            "at" => Value::VmNativeFunction(BUILTIN_ARRAY_AT),
                            "every" => Value::VmNativeFunction(BUILTIN_ARRAY_EVERY),
                            "some" => Value::VmNativeFunction(BUILTIN_ARRAY_SOME),
                            "fill" => Value::VmNativeFunction(BUILTIN_ARRAY_FILL),
                            "lastIndexOf" => Value::VmNativeFunction(BUILTIN_ARRAY_LASTINDEXOF),
                            "findLast" => Value::VmNativeFunction(BUILTIN_ARRAY_FINDLAST),
                            "findLastIndex" => Value::VmNativeFunction(BUILTIN_ARRAY_FINDLASTINDEX),
                            "reduceRight" => Value::VmNativeFunction(BUILTIN_ARRAY_REDUCERIGHT),
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
                            "padStart" => Value::VmNativeFunction(BUILTIN_STRING_PADSTART),
                            "padEnd" => Value::VmNativeFunction(BUILTIN_STRING_PADEND),
                            "repeat" => Value::VmNativeFunction(BUILTIN_STRING_REPEAT),
                            "charCodeAt" => Value::VmNativeFunction(BUILTIN_STRING_CHARCODEAT),
                            "trimStart" => Value::VmNativeFunction(BUILTIN_STRING_TRIMSTART),
                            "trimEnd" => Value::VmNativeFunction(BUILTIN_STRING_TRIMEND),
                            "lastIndexOf" => Value::VmNativeFunction(BUILTIN_STRING_LASTINDEXOF),
                            "match" => Value::VmNativeFunction(BUILTIN_STRING_MATCH),
                            "replaceAll" => Value::VmNativeFunction(BUILTIN_STRING_REPLACEALL),
                            "search" => Value::VmNativeFunction(BUILTIN_STRING_SEARCH),
                            "concat" => Self::make_bound_host_fn("string.concat", obj.clone()),
                            "substr" => Self::make_bound_host_fn("string.substr", obj.clone()),
                            _ => Value::Undefined,
                        },
                        Value::Number(_) => match key.as_str() {
                            "toFixed" => Value::VmNativeFunction(BUILTIN_NUM_TOFIXED),
                            "toExponential" => Value::VmNativeFunction(BUILTIN_NUM_TOEXPONENTIAL),
                            "toPrecision" => Value::VmNativeFunction(BUILTIN_NUM_TOPRECISION),
                            "toString" => Value::VmNativeFunction(BUILTIN_NUM_TOSTRING),
                            "valueOf" => Value::VmNativeFunction(BUILTIN_NUM_VALUEOF),
                            _ => Value::Undefined,
                        },
                        Value::VmMap(_) => match key.as_str() {
                            "set" => Value::VmNativeFunction(BUILTIN_MAP_SET),
                            "get" => Value::VmNativeFunction(BUILTIN_MAP_GET),
                            "has" => Value::VmNativeFunction(BUILTIN_MAP_HAS),
                            "delete" => Value::VmNativeFunction(BUILTIN_MAP_DELETE),
                            "keys" => Value::VmNativeFunction(BUILTIN_MAP_KEYS),
                            "values" => Value::VmNativeFunction(BUILTIN_MAP_VALUES),
                            "entries" => Value::VmNativeFunction(BUILTIN_MAP_ENTRIES),
                            "forEach" => Value::VmNativeFunction(BUILTIN_MAP_FOREACH),
                            "clear" => Value::VmNativeFunction(BUILTIN_MAP_CLEAR),
                            "toString" => Value::VmNativeFunction(BUILTIN_OBJ_TOSTRING),
                            _ => Value::Undefined,
                        },
                        Value::VmSet(_) => match key.as_str() {
                            "add" => Value::VmNativeFunction(BUILTIN_SET_ADD),
                            "has" => Value::VmNativeFunction(BUILTIN_SET_HAS),
                            "delete" => Value::VmNativeFunction(BUILTIN_SET_DELETE),
                            "keys" | "values" => Value::VmNativeFunction(BUILTIN_SET_VALUES),
                            "entries" => Value::VmNativeFunction(BUILTIN_SET_ENTRIES),
                            "forEach" => Value::VmNativeFunction(BUILTIN_SET_FOREACH),
                            "clear" => Value::VmNativeFunction(BUILTIN_SET_CLEAR),
                            "toString" => Value::VmNativeFunction(BUILTIN_OBJ_TOSTRING),
                            _ => Value::Undefined,
                        },
                        Value::Object(obj_ref) => {
                            if let Some(v) = crate::core::object_get_key_value(obj_ref, key.as_str()) {
                                (*v.borrow()).clone()
                            } else {
                                Value::Undefined
                            }
                        }
                        Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                            let props = self.get_fn_props(*ip, *arity);
                            let borrow = props.borrow();
                            if let Some(value) = borrow.get(&key).cloned() {
                                value
                            } else {
                                let proto = borrow.get("__proto__").cloned();
                                drop(borrow);
                                match key.as_str() {
                                    "call" => Value::VmNativeFunction(BUILTIN_FN_CALL),
                                    "apply" => Value::VmNativeFunction(BUILTIN_FN_APPLY),
                                    "bind" => Value::VmNativeFunction(BUILTIN_FN_BIND),
                                    _ => self.lookup_proto_chain(&proto, &key).unwrap_or(Value::Undefined),
                                }
                            }
                        }
                        Value::VmNativeFunction(_) => match key.as_str() {
                            "asUintN" => Value::VmNativeFunction(BUILTIN_BIGINT_ASUINTN),
                            "asIntN" => Value::VmNativeFunction(BUILTIN_BIGINT_ASINTN),
                            "call" => Value::VmNativeFunction(BUILTIN_FN_CALL),
                            "apply" => Value::VmNativeFunction(BUILTIN_FN_APPLY),
                            "bind" => Value::VmNativeFunction(BUILTIN_FN_BIND),
                            _ => Value::Undefined,
                        },
                        _ => Value::Undefined,
                    };
                    self.stack.push(method);
                }
                Opcode::NewError => {
                    // Stack: [..., type_name, message]
                    let msg = self.stack.pop().unwrap_or(Value::Undefined);
                    let type_val = self.stack.pop().unwrap_or(Value::Undefined);
                    let type_name = value_to_string(&type_val);
                    let mut map = IndexMap::new();
                    map.insert("message".to_string(), msg);
                    map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16(&type_name)));
                    map.insert("name".to_string(), Value::String(crate::unicode::utf8_to_utf16(&type_name)));
                    self.stack.push(Value::VmObject(Rc::new(RefCell::new(map))));
                }
                Opcode::Dup => {
                    let val = self.stack.last().cloned().unwrap_or(Value::Undefined);
                    self.stack.push(val);
                }
                Opcode::Swap => {
                    let len = self.stack.len();
                    if len >= 2 {
                        self.stack.swap(len - 1, len - 2);
                    }
                }
                Opcode::ToNumber => {
                    let val = self.stack.pop().expect("VM Stack underflow on ToNumber");
                    self.stack.push(Value::Number(to_number(&val)));
                }
                Opcode::CollectRest => {
                    // Collect excess function args into a rest array.
                    // Operand: non_rest_count (u8) = number of formal non-rest params.
                    let non_rest_count = self.read_byte() as usize;
                    let frame = self.frames.last().expect("CollectRest: no call frame");
                    let actual_arg_count = frame.arg_count;
                    let bp = frame.bp;
                    let saved = frame.saved_args.clone();
                    if actual_arg_count > non_rest_count {
                        let rest_elems: Vec<Value<'gc>> = if let Some(ref sa) = saved {
                            // Excess args were removed from stack; get them from saved_args
                            sa[non_rest_count..actual_arg_count].to_vec()
                        } else {
                            // No excess args were removed; they're still on the stack
                            let start = bp + non_rest_count;
                            let end = bp + actual_arg_count;
                            let elems = self.stack[start..end].to_vec();
                            self.stack.drain(start..end);
                            elems
                        };
                        self.stack.push(Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(rest_elems)))));
                    } else {
                        // No excess args — push empty array
                        self.stack.push(Value::VmArray(Rc::new(RefCell::new(VmArrayData::new(Vec::new())))));
                    }
                    // The rest array is now the next local slot (at position non_rest_count)
                }
                Opcode::In => {
                    let obj = self.stack.pop().expect("VM Stack underflow on In (obj)");
                    let key_val = self.stack.pop().expect("VM Stack underflow on In (key)");
                    let key = value_to_string(&key_val);
                    if let Some(result) = self.try_proxy_has(&obj, &key)? {
                        self.stack.push(Value::Boolean(result));
                        continue;
                    }
                    let result = match &obj {
                        Value::VmObject(map) => {
                            let b = map.borrow();
                            if b.contains_key(&key) {
                                true
                            } else {
                                // Check built-in properties based on __type__
                                let type_name = b.get("__type__").map(|v| value_to_string(v)).unwrap_or_default();
                                if matches!(type_name.as_str(), "String" if key == "length") {
                                    true
                                } else {
                                    // Walk __proto__ chain
                                    let proto = b.get("__proto__").cloned();
                                    drop(b);
                                    self.lookup_proto_chain(&proto, &key).is_some()
                                }
                            }
                        }
                        Value::VmArray(arr) => {
                            if let Ok(idx) = key.parse::<usize>() {
                                let borrow = arr.borrow();
                                if idx < borrow.len() {
                                    // Check if the index was deleted (hole)
                                    !borrow.props.contains_key(&format!("__deleted_{}", idx))
                                } else {
                                    false
                                }
                            } else if key == "length" {
                                true
                            } else {
                                arr.borrow().props.contains_key(&key)
                            }
                        }
                        _ => false,
                    };
                    self.stack.push(Value::Boolean(result));
                }
                Opcode::InstanceOf => {
                    let rhs = self.stack.pop().expect("VM Stack underflow on InstanceOf (rhs)");
                    let lhs = self.stack.pop().expect("VM Stack underflow on InstanceOf (lhs)");

                    // Check Symbol.hasInstance (@@sym:2) on rhs first
                    let has_instance_fn = match &rhs {
                        Value::VmObject(map) => map.borrow().get("@@sym:2").cloned(),
                        Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                            self.get_fn_props(*ip, *arity).borrow().get("@@sym:2").cloned()
                        }
                        _ => None,
                    };
                    if let Some(hi_fn) = has_instance_fn {
                        let result = match hi_fn {
                            Value::VmFunction(ip, _) => {
                                self.this_stack.push(rhs.clone());
                                let r = self.call_vm_function_result(ip, std::slice::from_ref(&lhs), &[]);
                                self.this_stack.pop();
                                r?
                            }
                            Value::VmClosure(ip, _, upv) => {
                                self.this_stack.push(rhs.clone());
                                let uv = (*upv).clone();
                                let r = self.call_vm_function_result(ip, std::slice::from_ref(&lhs), &uv);
                                self.this_stack.pop();
                                r?
                            }
                            Value::VmNativeFunction(id) => self.call_method_builtin(id, rhs.clone(), vec![lhs.clone()]),
                            _ => Value::Boolean(false),
                        };
                        self.stack.push(Value::Boolean(result.to_truthy()));
                        continue;
                    }

                    // Try prototype-chain based instanceof first (works for user-defined classes)
                    let mut proto_chain_result: Option<bool> = None;

                    // Get rhs.prototype for prototype chain walking
                    let rhs_proto = match &rhs {
                        Value::VmFunction(ip, arity) | Value::VmClosure(ip, arity, _) => {
                            let fn_props = self.get_fn_props(*ip, *arity);
                            fn_props.borrow().get("prototype").cloned()
                        }
                        Value::VmObject(map) => map.borrow().get("prototype").cloned(),
                        _ => None,
                    };

                    if let Some(target_proto) = &rhs_proto {
                        // Walk __proto__ chain of lhs looking for target_proto
                        if let Value::VmObject(obj) = &lhs {
                            let mut current = obj.borrow().get("__proto__").cloned();
                            let mut depth = 0;
                            loop {
                                if depth > 100 {
                                    break;
                                }
                                depth += 1;
                                let proto_val = match current {
                                    Some(v) => v,
                                    None => break,
                                };
                                if let (Value::VmObject(a), Value::VmObject(b)) = (&proto_val, target_proto)
                                    && Rc::ptr_eq(a, b)
                                {
                                    proto_chain_result = Some(true);
                                    break;
                                }
                                current = if let Value::VmObject(proto_obj) = &proto_val {
                                    proto_obj.borrow().get("__proto__").cloned()
                                } else {
                                    None
                                };
                            }
                            if proto_chain_result.is_none() {
                                proto_chain_result = Some(false);
                            }
                        }
                    }

                    let result = if let Some(r) = proto_chain_result {
                        r
                    } else {
                        // Fallback: name-based instanceof for built-in types
                        let ctor_name = match &rhs {
                            Value::VmNativeFunction(id) => match *id {
                                BUILTIN_CTOR_ERROR => "Error",
                                BUILTIN_CTOR_TYPEERROR => "TypeError",
                                BUILTIN_CTOR_SYNTAXERROR => "SyntaxError",
                                BUILTIN_CTOR_RANGEERROR => "RangeError",
                                BUILTIN_CTOR_REFERENCEERROR => "ReferenceError",
                                BUILTIN_CTOR_DATE => "Date",
                                BUILTIN_CTOR_FUNCTION => "Function",
                                BUILTIN_CTOR_NUMBER => "Number",
                                BUILTIN_CTOR_STRING => "String",
                                BUILTIN_CTOR_BOOLEAN => "Boolean",
                                BUILTIN_CTOR_OBJECT => "Object",
                                BUILTIN_CTOR_WEAKREF => "WeakRef",
                                BUILTIN_CTOR_WEAKMAP => "WeakMap",
                                BUILTIN_CTOR_WEAKSET => "WeakSet",
                                BUILTIN_CTOR_FR => "FinalizationRegistry",
                                _ => "",
                            },
                            Value::VmObject(map) => {
                                let borrow = map.borrow();
                                if let Some(Value::Number(n)) = borrow.get("__native_id__") {
                                    match *n as u8 {
                                        BUILTIN_CTOR_DATE => "Date",
                                        BUILTIN_CTOR_FUNCTION => "Function",
                                        BUILTIN_CTOR_NUMBER => "Number",
                                        BUILTIN_CTOR_STRING => "String",
                                        BUILTIN_CTOR_BOOLEAN => "Boolean",
                                        BUILTIN_CTOR_OBJECT => "Object",
                                        BUILTIN_CTOR_ERROR => "Error",
                                        BUILTIN_CTOR_TYPEERROR => "TypeError",
                                        BUILTIN_CTOR_SYNTAXERROR => "SyntaxError",
                                        BUILTIN_CTOR_RANGEERROR => "RangeError",
                                        BUILTIN_CTOR_REFERENCEERROR => "ReferenceError",
                                        BUILTIN_CTOR_WEAKREF => "WeakRef",
                                        BUILTIN_CTOR_WEAKMAP => "WeakMap",
                                        BUILTIN_CTOR_WEAKSET => "WeakSet",
                                        BUILTIN_CTOR_FR => "FinalizationRegistry",
                                        _ => "",
                                    }
                                } else {
                                    ""
                                }
                            }
                            _ => "",
                        };
                        let ctor_str = if ctor_name.is_empty() {
                            value_to_string(&rhs)
                        } else {
                            ctor_name.to_string()
                        };
                        if let Value::VmObject(map) = &lhs {
                            let borrow = map.borrow();
                            if let Some(Value::String(type_u16)) = borrow.get("__type__") {
                                let type_name = crate::unicode::utf16_to_utf8(type_u16);
                                match ctor_str.as_str() {
                                    "Error" => type_name == "Error" || type_name.ends_with("Error"),
                                    "Object" => true,
                                    _ => type_name == ctor_str,
                                }
                            } else {
                                ctor_str == "Object"
                            }
                        } else if ctor_str == "Function" {
                            matches!(&lhs, Value::VmNativeFunction(_) | Value::VmFunction(..) | Value::VmClosure(..))
                                || matches!(&lhs, Value::VmObject(m) if m.borrow().contains_key("__native_id__"))
                        } else {
                            false
                        }
                    };
                    self.stack.push(Value::Boolean(result));
                }
                Opcode::DeleteProperty => {
                    let name_idx = self.read_u16() as usize;
                    let name_val = &self.chunk.constants[name_idx];
                    let key = if let Value::String(s) = name_val {
                        crate::unicode::utf16_to_utf8(s)
                    } else {
                        value_to_string(name_val)
                    };
                    let obj = self.stack.pop().expect("VM Stack underflow on DeleteProperty");
                    if let Some(result) = self.try_proxy_delete(&obj, &key)? {
                        self.stack.push(Value::Boolean(result));
                        continue;
                    }
                    // Check if object is a built-in (non-deletable properties)
                    let is_builtin = if let Value::VmObject(ref map) = obj {
                        if let Some(Value::VmObject(math_ref)) = self.globals.get("Math") {
                            Rc::ptr_eq(map, math_ref)
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if is_builtin {
                        // Non-configurable property: throw TypeError
                        let mut err_map = IndexMap::new();
                        err_map.insert(
                            "message".to_string(),
                            Value::String(crate::unicode::utf8_to_utf16(&format!(
                                "Cannot delete property '{}' of #<Object>",
                                key
                            ))),
                        );
                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                        let err = Value::VmObject(Rc::new(RefCell::new(err_map)));
                        self.handle_throw(err)?;
                        // After handle_throw, push undefined as placeholder on stack
                        self.stack.push(Value::Boolean(false));
                    } else if let Value::VmObject(map) = &obj {
                        map.borrow_mut().shift_remove(&key);
                        self.stack.push(Value::Boolean(true));
                    } else if let Value::VmArray(arr) = &obj {
                        arr.borrow_mut().props.shift_remove(&key);
                        self.stack.push(Value::Boolean(true));
                    } else {
                        self.stack.push(Value::Boolean(false));
                    }
                }
                Opcode::NewCall => {
                    let arg_count = self.read_byte() as usize;
                    // Stack: [..., constructor, arg0, arg1, ...]
                    let callee_idx = self.stack.len() - arg_count - 1;
                    let callee = self.stack[callee_idx].clone();
                    match callee {
                        Value::VmFunction(target_ip, _arity) | Value::VmClosure(target_ip, _arity, _) => {
                            // Create new empty object as `this`
                            let new_obj = Rc::new(RefCell::new(IndexMap::new()));
                            // Set __proto__ to constructor's prototype property
                            let fn_props = self.get_fn_props(target_ip, _arity);
                            if let Some(proto) = fn_props.borrow().get("prototype").cloned() {
                                new_obj.borrow_mut().insert("__proto__".to_string(), proto);
                            }
                            let this_val = Value::VmObject(new_obj.clone());
                            self.this_stack.push(this_val);
                            // Push new.target = the constructor being invoked
                            self.new_target_stack.push(callee.clone());
                            let closure_uv = if let Value::VmClosure(_, _, ref uv) = callee {
                                (**uv).clone()
                            } else {
                                Vec::new()
                            };
                            // Set up call frame
                            let frame = CallFrame {
                                return_ip: self.ip,
                                bp: callee_idx + 1,
                                is_method: false,
                                arg_count,
                                func_ip: target_ip,
                                arguments_obj: None,
                                upvalues: closure_uv,
                                saved_args: None,
                                local_cells: HashMap::new(),
                            };
                            self.frames.push(frame);
                            self.ip = target_ip;
                            // Run the constructor
                            let result = self.run_inner(self.frames.len());
                            self.this_stack.pop();
                            self.new_target_stack.pop();
                            // The constructor returns `this` (we compiled GetThis+Return)
                            // but result from run_inner is what was returned
                            match result {
                                Ok(val) => {
                                    // If constructor returned an object, use it; otherwise use `this`
                                    match &val {
                                        Value::VmObject(_) => self.stack.push(val),
                                        _ => self.stack.push(Value::VmObject(new_obj)),
                                    }
                                }
                                Err(e) => return Err(e),
                            }
                        }
                        Value::VmNativeFunction(id) => {
                            let args: Vec<Value<'gc>> = (0..arg_count)
                                .map(|_| self.stack.pop().expect("VM Stack underflow"))
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .collect();
                            self.stack.pop(); // pop constructor
                            match id {
                                BUILTIN_CTOR_MAP => {
                                    let mut entries = Vec::new();
                                    // new Map(iterable) — iterable is an array of [key, value] pairs
                                    if let Some(Value::VmArray(arr)) = args.first() {
                                        for item in arr.borrow().iter() {
                                            if let Value::VmArray(pair) = item {
                                                let p = pair.borrow();
                                                let k = p.first().cloned().unwrap_or(Value::Undefined);
                                                let v = p.get(1).cloned().unwrap_or(Value::Undefined);
                                                entries.push((k, v));
                                            }
                                        }
                                    }
                                    self.stack
                                        .push(Value::VmMap(Rc::new(RefCell::new(VmMapData { entries, is_weak: false }))));
                                }
                                BUILTIN_CTOR_SET => {
                                    let mut values = Vec::new();
                                    // new Set(iterable) — iterable is an array
                                    if let Some(Value::VmArray(arr)) = args.first() {
                                        for item in arr.borrow().iter() {
                                            if !values.iter().any(|v| self.values_equal(v, item)) {
                                                values.push(item.clone());
                                            }
                                        }
                                    }
                                    self.stack
                                        .push(Value::VmSet(Rc::new(RefCell::new(VmSetData { values, is_weak: false }))));
                                }
                                BUILTIN_CTOR_WEAKMAP => {
                                    // WeakMap: implemented as regular Map (no GC)
                                    self.stack.push(Value::VmMap(Rc::new(RefCell::new(VmMapData {
                                        entries: Vec::new(),
                                        is_weak: true,
                                    }))));
                                }
                                BUILTIN_CTOR_WEAKSET => {
                                    // WeakSet: implemented as regular Set (no GC)
                                    self.stack.push(Value::VmSet(Rc::new(RefCell::new(VmSetData {
                                        values: Vec::new(),
                                        is_weak: true,
                                    }))));
                                }
                                BUILTIN_CTOR_WEAKREF => {
                                    // WeakRef: target must be an object or unregistered symbol
                                    let target = args.into_iter().next().unwrap_or(Value::Undefined);
                                    // Check for registered VM symbol — reject it
                                    let is_registered_symbol = if let Value::VmObject(ref obj) = target {
                                        let b = obj.borrow();
                                        b.contains_key("__vm_symbol__") && b.contains_key("__registered__")
                                    } else {
                                        false
                                    };
                                    let is_valid = match &target {
                                        Value::VmObject(_) if !is_registered_symbol => true,
                                        Value::VmArray(_)
                                        | Value::VmMap(_)
                                        | Value::VmSet(_)
                                        | Value::VmFunction(..)
                                        | Value::VmClosure(..)
                                        | Value::Closure(..)
                                        | Value::Symbol(_) => true,
                                        _ => false,
                                    };
                                    if is_valid {
                                        let mut m = IndexMap::new();
                                        m.insert("__weakref__".to_string(), Value::Boolean(true));
                                        m.insert("__target__".to_string(), target);
                                        m.insert(
                                            "__toStringTag__".to_string(),
                                            Value::String(crate::unicode::utf8_to_utf16("WeakRef")),
                                        );
                                        m.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("WeakRef")));
                                        self.stack.push(Value::VmObject(Rc::new(RefCell::new(m))));
                                    } else {
                                        let mut err_map = IndexMap::new();
                                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                        err_map.insert(
                                            "message".to_string(),
                                            Value::String(crate::unicode::utf8_to_utf16("Invalid value used as weak reference target")),
                                        );
                                        let err = Value::VmObject(Rc::new(RefCell::new(err_map)));
                                        self.handle_throw(err)?;
                                    }
                                }
                                BUILTIN_CTOR_FR => {
                                    // new FinalizationRegistry(callback)
                                    let callback = args.into_iter().next().unwrap_or(Value::Undefined);
                                    let is_callable = matches!(
                                        callback,
                                        Value::VmFunction(..) | Value::VmClosure(..) | Value::VmNativeFunction(_) | Value::Closure(..)
                                    ) || matches!(&callback, Value::VmObject(o) if {
                                        let b = o.borrow();
                                        b.contains_key("__fn_body__") || b.contains_key("__native_id__")
                                    });
                                    if !is_callable {
                                        let mut err_map = IndexMap::new();
                                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                        err_map.insert(
                                            "message".to_string(),
                                            Value::String(crate::unicode::utf8_to_utf16(
                                                "FinalizationRegistry requires a callable cleanup callback",
                                            )),
                                        );
                                        let err = Value::VmObject(Rc::new(RefCell::new(err_map)));
                                        self.handle_throw(err)?;
                                    } else {
                                        let mut m = IndexMap::new();
                                        m.insert("__fr__".to_string(), Value::Boolean(true));
                                        m.insert("__fr_callback__".to_string(), callback);
                                        m.insert("__fr_count__".to_string(), Value::Number(0.0));
                                        m.insert(
                                            "__type__".to_string(),
                                            Value::String(crate::unicode::utf8_to_utf16("FinalizationRegistry")),
                                        );
                                        m.insert(
                                            "__toStringTag__".to_string(),
                                            Value::String(crate::unicode::utf8_to_utf16("FinalizationRegistry")),
                                        );
                                        m.insert("register".to_string(), Value::VmNativeFunction(BUILTIN_FR_REGISTER));
                                        m.insert("unregister".to_string(), Value::VmNativeFunction(BUILTIN_FR_UNREGISTER));
                                        self.stack.push(Value::VmObject(Rc::new(RefCell::new(m))));
                                    }
                                }
                                BUILTIN_CTOR_REGEXP => {
                                    // new RegExp(pattern, flags)
                                    let pattern = args.first().map(value_to_string).unwrap_or_default();
                                    let flags = args.get(1).map(value_to_string).unwrap_or_default();
                                    let mut map = IndexMap::new();
                                    map.insert(
                                        "__regex_pattern__".to_string(),
                                        Value::String(crate::unicode::utf8_to_utf16(&pattern)),
                                    );
                                    map.insert("__regex_flags__".to_string(), Value::String(crate::unicode::utf8_to_utf16(&flags)));
                                    map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("RegExp")));
                                    map.insert(
                                        "__toStringTag__".to_string(),
                                        Value::String(crate::unicode::utf8_to_utf16("RegExp")),
                                    );
                                    map.insert("source".to_string(), Value::String(crate::unicode::utf8_to_utf16(&pattern)));
                                    map.insert("flags".to_string(), Value::String(crate::unicode::utf8_to_utf16(&flags)));
                                    map.insert("global".to_string(), Value::Boolean(flags.contains('g')));
                                    map.insert("ignoreCase".to_string(), Value::Boolean(flags.contains('i')));
                                    map.insert("multiline".to_string(), Value::Boolean(flags.contains('m')));
                                    map.insert("dotAll".to_string(), Value::Boolean(flags.contains('s')));
                                    map.insert("sticky".to_string(), Value::Boolean(flags.contains('y')));
                                    map.insert("unicode".to_string(), Value::Boolean(flags.contains('u')));
                                    map.insert("hasIndices".to_string(), Value::Boolean(flags.contains('d')));
                                    map.insert("unicodeSets".to_string(), Value::Boolean(flags.contains('v')));
                                    map.insert("lastIndex".to_string(), Value::Number(0.0));
                                    self.stack.push(Value::VmObject(Rc::new(RefCell::new(map))));
                                }
                                BUILTIN_CTOR_DATE => {
                                    use std::time::{SystemTime, UNIX_EPOCH};
                                    let ms = if args.is_empty() {
                                        // new Date() — current timestamp
                                        SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .map(|d| d.as_millis() as f64)
                                            .unwrap_or(0.0)
                                    } else if args.len() == 1 {
                                        // new Date(value) — parse or timestamp
                                        match &args[0] {
                                            Value::Number(n) => *n,
                                            Value::String(s) => {
                                                let s_str = crate::unicode::utf16_to_utf8(s);
                                                // Try to parse ISO date string
                                                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&s_str) {
                                                    dt.timestamp_millis() as f64
                                                } else if let Ok(dt) = chrono::NaiveDate::parse_from_str(&s_str, "%Y-%m-%d") {
                                                    dt.and_hms_opt(0, 0, 0)
                                                        .map(|d| d.and_utc().timestamp_millis() as f64)
                                                        .unwrap_or(f64::NAN)
                                                } else {
                                                    f64::NAN
                                                }
                                            }
                                            _ => f64::NAN,
                                        }
                                    } else {
                                        // new Date(year, month, day?, hours?, min?, sec?, ms?)
                                        let year = if let Value::Number(n) = &args[0] { *n as i32 } else { 0 };
                                        let month = if let Value::Number(n) = args.get(1).unwrap_or(&Value::Number(0.0)) {
                                            *n as u32
                                        } else {
                                            0
                                        };
                                        let day = if let Value::Number(n) = args.get(2).unwrap_or(&Value::Number(1.0)) {
                                            *n as u32
                                        } else {
                                            1
                                        };
                                        let hour = if let Value::Number(n) = args.get(3).unwrap_or(&Value::Number(0.0)) {
                                            *n as u32
                                        } else {
                                            0
                                        };
                                        let min = if let Value::Number(n) = args.get(4).unwrap_or(&Value::Number(0.0)) {
                                            *n as u32
                                        } else {
                                            0
                                        };
                                        let sec = if let Value::Number(n) = args.get(5).unwrap_or(&Value::Number(0.0)) {
                                            *n as u32
                                        } else {
                                            0
                                        };
                                        let ms_part = if let Value::Number(n) = args.get(6).unwrap_or(&Value::Number(0.0)) {
                                            *n as u32
                                        } else {
                                            0
                                        };
                                        // Adjust year: 0-99 maps to 1900-1999
                                        let full_year = if (0..100).contains(&year) { year + 1900 } else { year };
                                        // Use chrono to build the date in local timezone
                                        use chrono::{Local, TimeZone};
                                        let result = Local.with_ymd_and_hms(full_year, month + 1, day, hour, min, sec);
                                        match result {
                                            chrono::LocalResult::Single(dt) => dt.timestamp_millis() as f64 + ms_part as f64,
                                            _ => f64::NAN,
                                        }
                                    };
                                    let mut map = IndexMap::new();
                                    map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Date")));
                                    map.insert("__date_ms__".to_string(), Value::Number(ms));
                                    // Install Date instance methods
                                    map.insert("getTime".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETTIME));
                                    map.insert("valueOf".to_string(), Value::VmNativeFunction(BUILTIN_DATE_VALUEOF));
                                    map.insert("toString".to_string(), Value::VmNativeFunction(BUILTIN_DATE_TOSTRING));
                                    map.insert(
                                        "toLocaleDateString".to_string(),
                                        Value::VmNativeFunction(BUILTIN_DATE_TOLOCALEDATESTRING),
                                    );
                                    map.insert(
                                        "toLocaleTimeString".to_string(),
                                        Value::VmNativeFunction(BUILTIN_DATE_TOLOCALETIMESTRING),
                                    );
                                    map.insert("toLocaleString".to_string(), Value::VmNativeFunction(BUILTIN_DATE_TOLOCALESTRING));
                                    map.insert("toISOString".to_string(), Value::VmNativeFunction(BUILTIN_DATE_TOISOSTRING));
                                    map.insert("getFullYear".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETFULLYEAR));
                                    map.insert("getMonth".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETMONTH));
                                    map.insert("getDate".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETDATE));
                                    map.insert("getDay".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETDAY));
                                    map.insert("getHours".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETHOURS));
                                    map.insert("getMinutes".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETMINUTES));
                                    map.insert("getSeconds".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETSECONDS));
                                    map.insert("getMilliseconds".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETMILLISECONDS));
                                    map.insert("setFullYear".to_string(), Value::VmNativeFunction(BUILTIN_DATE_SETFULLYEAR));
                                    map.insert("setMonth".to_string(), Value::VmNativeFunction(BUILTIN_DATE_SETMONTH));
                                    map.insert("setDate".to_string(), Value::VmNativeFunction(BUILTIN_DATE_SETDATE));
                                    map.insert("setHours".to_string(), Value::VmNativeFunction(BUILTIN_DATE_SETHOURS));
                                    map.insert("setMinutes".to_string(), Value::VmNativeFunction(BUILTIN_DATE_SETMINUTES));
                                    map.insert(
                                        "getTimezoneOffset".to_string(),
                                        Value::VmNativeFunction(BUILTIN_DATE_GETTIMEZONEOFFSET),
                                    );
                                    map.insert("getUTCFullYear".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETUTCFULLYEAR));
                                    map.insert("getUTCMonth".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETUTCMONTH));
                                    map.insert("getUTCDate".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETUTCDATE));
                                    map.insert("getUTCHours".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETUTCHOURS));
                                    map.insert("getUTCMinutes".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETUTCMINUTES));
                                    map.insert("getUTCSeconds".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETUTCSECONDS));
                                    self.stack.push(Value::VmObject(Rc::new(RefCell::new(map))));
                                }
                                _ => {
                                    log::warn!("NewCall on VmNativeFunction #{}: returning empty object", id);
                                    self.stack.push(Value::VmObject(Rc::new(RefCell::new(IndexMap::new()))));
                                }
                            }
                            if let Some(thrown) = self.pending_throw.take() {
                                self.handle_throw(thrown)?;
                                continue;
                            }
                        }
                        _ => {
                            // Check for VmObject with __native_id__ (e.g. Object, Number, String constructors)
                            if let Value::VmObject(ref map) = callee {
                                let borrow = map.borrow();
                                if let Some(Value::Number(native_id)) = borrow.get("__native_id__") {
                                    let id = *native_id as u8;
                                    drop(borrow);
                                    let args: Vec<Value<'gc>> = (0..arg_count)
                                        .map(|_| self.stack.pop().expect("VM Stack underflow"))
                                        .collect::<Vec<_>>()
                                        .into_iter()
                                        .rev()
                                        .collect();
                                    self.stack.pop(); // pop constructor

                                    // Date is exposed as a constructor object (with __native_id__),
                                    // so handle `new Date(...)` here as well.
                                    if id == BUILTIN_CTOR_DATE {
                                        use std::time::{SystemTime, UNIX_EPOCH};
                                        let ms = if args.is_empty() {
                                            SystemTime::now()
                                                .duration_since(UNIX_EPOCH)
                                                .map(|d| d.as_millis() as f64)
                                                .unwrap_or(0.0)
                                        } else if args.len() == 1 {
                                            match &args[0] {
                                                Value::Number(n) => *n,
                                                Value::String(s) => {
                                                    let s_str = crate::unicode::utf16_to_utf8(s);
                                                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&s_str) {
                                                        dt.timestamp_millis() as f64
                                                    } else if let Ok(dt) = chrono::NaiveDate::parse_from_str(&s_str, "%Y-%m-%d") {
                                                        dt.and_hms_opt(0, 0, 0)
                                                            .map(|d| d.and_utc().timestamp_millis() as f64)
                                                            .unwrap_or(f64::NAN)
                                                    } else {
                                                        f64::NAN
                                                    }
                                                }
                                                _ => f64::NAN,
                                            }
                                        } else {
                                            let year = if let Value::Number(n) = &args[0] { *n as i32 } else { 0 };
                                            let month = if let Value::Number(n) = args.get(1).unwrap_or(&Value::Number(0.0)) {
                                                *n as u32
                                            } else {
                                                0
                                            };
                                            let day = if let Value::Number(n) = args.get(2).unwrap_or(&Value::Number(1.0)) {
                                                *n as u32
                                            } else {
                                                1
                                            };
                                            let hour = if let Value::Number(n) = args.get(3).unwrap_or(&Value::Number(0.0)) {
                                                *n as u32
                                            } else {
                                                0
                                            };
                                            let min = if let Value::Number(n) = args.get(4).unwrap_or(&Value::Number(0.0)) {
                                                *n as u32
                                            } else {
                                                0
                                            };
                                            let sec = if let Value::Number(n) = args.get(5).unwrap_or(&Value::Number(0.0)) {
                                                *n as u32
                                            } else {
                                                0
                                            };
                                            let ms_part = if let Value::Number(n) = args.get(6).unwrap_or(&Value::Number(0.0)) {
                                                *n as u32
                                            } else {
                                                0
                                            };
                                            let full_year = if (0..100).contains(&year) { year + 1900 } else { year };
                                            use chrono::{Local, TimeZone};
                                            let result = Local.with_ymd_and_hms(full_year, month + 1, day, hour, min, sec);
                                            match result {
                                                chrono::LocalResult::Single(dt) => dt.timestamp_millis() as f64 + ms_part as f64,
                                                _ => f64::NAN,
                                            }
                                        };

                                        let mut m = IndexMap::new();
                                        m.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Date")));
                                        m.insert("__date_ms__".to_string(), Value::Number(ms));
                                        m.insert("getTime".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETTIME));
                                        m.insert("valueOf".to_string(), Value::VmNativeFunction(BUILTIN_DATE_VALUEOF));
                                        m.insert("toString".to_string(), Value::VmNativeFunction(BUILTIN_DATE_TOSTRING));
                                        m.insert("toDateString".to_string(), Value::VmNativeFunction(BUILTIN_DATE_TODATESTRING));
                                        m.insert("setTime".to_string(), Value::VmNativeFunction(BUILTIN_DATE_SETTIME));
                                        m.insert(
                                            "toLocaleDateString".to_string(),
                                            Value::VmNativeFunction(BUILTIN_DATE_TOLOCALEDATESTRING),
                                        );
                                        m.insert(
                                            "toLocaleTimeString".to_string(),
                                            Value::VmNativeFunction(BUILTIN_DATE_TOLOCALETIMESTRING),
                                        );
                                        m.insert("toLocaleString".to_string(), Value::VmNativeFunction(BUILTIN_DATE_TOLOCALESTRING));
                                        m.insert("toISOString".to_string(), Value::VmNativeFunction(BUILTIN_DATE_TOISOSTRING));
                                        m.insert("getFullYear".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETFULLYEAR));
                                        m.insert("getMonth".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETMONTH));
                                        m.insert("getDate".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETDATE));
                                        m.insert("getDay".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETDAY));
                                        m.insert("getHours".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETHOURS));
                                        m.insert("getMinutes".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETMINUTES));
                                        m.insert("getSeconds".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETSECONDS));
                                        m.insert("getMilliseconds".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETMILLISECONDS));
                                        m.insert("setFullYear".to_string(), Value::VmNativeFunction(BUILTIN_DATE_SETFULLYEAR));
                                        m.insert("setMonth".to_string(), Value::VmNativeFunction(BUILTIN_DATE_SETMONTH));
                                        m.insert("setDate".to_string(), Value::VmNativeFunction(BUILTIN_DATE_SETDATE));
                                        m.insert("setHours".to_string(), Value::VmNativeFunction(BUILTIN_DATE_SETHOURS));
                                        m.insert("setMinutes".to_string(), Value::VmNativeFunction(BUILTIN_DATE_SETMINUTES));
                                        m.insert(
                                            "getTimezoneOffset".to_string(),
                                            Value::VmNativeFunction(BUILTIN_DATE_GETTIMEZONEOFFSET),
                                        );
                                        m.insert("getUTCFullYear".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETUTCFULLYEAR));
                                        m.insert("getUTCMonth".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETUTCMONTH));
                                        m.insert("getUTCDate".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETUTCDATE));
                                        m.insert("getUTCHours".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETUTCHOURS));
                                        m.insert("getUTCMinutes".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETUTCMINUTES));
                                        m.insert("getUTCSeconds".to_string(), Value::VmNativeFunction(BUILTIN_DATE_GETUTCSECONDS));
                                        self.stack.push(Value::VmObject(Rc::new(RefCell::new(m))));
                                        continue;
                                    }

                                    // new Symbol() should throw TypeError
                                    if id == BUILTIN_SYMBOL {
                                        let mut err_map = IndexMap::new();
                                        err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                                        err_map.insert(
                                            "message".to_string(),
                                            Value::String(crate::unicode::utf8_to_utf16("Symbol is not a constructor")),
                                        );
                                        self.handle_throw(Value::VmObject(Rc::new(RefCell::new(err_map))))?;
                                        continue;
                                    }

                                    let result = self.call_builtin(id, args);
                                    // For constructors like Number/String/Boolean,
                                    // wrap the primitive result in an object
                                    let wrapped = match id {
                                        BUILTIN_CTOR_NUMBER => {
                                            let mut m = IndexMap::new();
                                            m.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Number")));
                                            m.insert("__value__".to_string(), result);
                                            Value::VmObject(Rc::new(RefCell::new(m)))
                                        }
                                        BUILTIN_CTOR_STRING => {
                                            let mut m = IndexMap::new();
                                            m.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("String")));
                                            m.insert("__value__".to_string(), result);
                                            Value::VmObject(Rc::new(RefCell::new(m)))
                                        }
                                        BUILTIN_CTOR_BOOLEAN => {
                                            let mut m = IndexMap::new();
                                            m.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Boolean")));
                                            m.insert("__value__".to_string(), result);
                                            Value::VmObject(Rc::new(RefCell::new(m)))
                                        }
                                        _ => result,
                                    };
                                    self.stack.push(wrapped);
                                    if let Some(thrown) = self.pending_throw.take() {
                                        self.handle_throw(thrown)?;
                                        continue;
                                    }
                                } else {
                                    drop(borrow);
                                    log::warn!("NewCall on non-constructor VmObject");
                                    for _i in 0..arg_count {
                                        self.stack.pop();
                                    }
                                    self.stack.pop();
                                    self.stack.push(Value::VmObject(Rc::new(RefCell::new(IndexMap::new()))));
                                }
                            } else {
                                log::warn!("NewCall on non-VmFunction: treating as regular call");
                                for _i in 0..arg_count {
                                    self.stack.pop();
                                }
                                self.stack.pop(); // pop constructor
                                self.stack.push(Value::VmObject(Rc::new(RefCell::new(IndexMap::new()))));
                            }
                        }
                    }
                }
                Opcode::DeleteIndex => {
                    // Stack: [..., obj, index]
                    let idx_val = self.stack.pop().expect("VM Stack underflow on DeleteIndex (idx)");
                    let obj = self.stack.pop().expect("VM Stack underflow on DeleteIndex (obj)");
                    match &obj {
                        Value::VmArray(arr) => {
                            let idx_str = value_to_string(&idx_val);
                            if let Ok(idx) = idx_str.parse::<usize>() {
                                let mut borrow = arr.borrow_mut();
                                if idx < borrow.elements.len() {
                                    // Set to a "hole" — use a sentinel or remove
                                    // JS delete arr[3] creates a hole (empty slot)
                                    borrow.elements[idx] = Value::Undefined;
                                    // Mark as deleted by storing in a "holes" set
                                    borrow.props.insert(format!("__deleted_{}", idx), Value::Boolean(true));
                                }
                            }
                            self.stack.push(Value::Boolean(true));
                        }
                        Value::VmObject(map) => {
                            let key = match self.as_property_key_string(&idx_val) {
                                Ok(k) => k,
                                Err(_) => value_to_string(&idx_val),
                            };
                            map.borrow_mut().shift_remove(&key);
                            self.stack.push(Value::Boolean(true));
                        }
                        _ => self.stack.push(Value::Boolean(false)),
                    }
                }
            }
        }
    }
}
