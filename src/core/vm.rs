use crate::core::opcode::{Chunk, Opcode};
use crate::core::value::{VmArrayData, VmMapData, VmSetData, value_to_string};
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

#[derive(Debug, Clone)]
pub struct CallFrame {
    pub return_ip: usize,
    pub bp: usize,        // Base pointer
    pub is_method: bool,  // Pop this_stack on return
    pub arg_count: usize, // Actual number of arguments passed
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
    // Property storage for VmFunction values, keyed by function IP
    fn_props: HashMap<usize, Rc<RefCell<IndexMap<String, Value<'gc>>>>>,
    // Global this object — top-level `this` refers to this; SetProperty on it writes to globals
    global_this: Rc<RefCell<IndexMap<String, Value<'gc>>>>,
}

impl<'gc> VM<'gc> {
    pub fn new(chunk: Chunk<'gc>) -> Self {
        let global_this = Rc::new(RefCell::new(IndexMap::new()));
        let mut vm = Self {
            chunk,
            ip: 0,
            stack: Vec::with_capacity(256),
            globals: HashMap::new(),
            frames: Vec::new(),
            try_stack: Vec::new(),
            this_stack: vec![Value::VmObject(global_this.clone())],
            output: Vec::new(),
            fn_props: HashMap::new(),
            global_this,
        };
        vm.register_builtins();
        vm
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
        // Set function name if known
        if let Some(name) = self.chunk.fn_names.get(&ip) {
            props.insert("name".to_string(), Value::String(crate::unicode::utf8_to_utf16(name)));
        }
        let props_rc = Rc::new(RefCell::new(props));
        self.fn_props.insert(ip, props_rc.clone());
        props_rc
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
        // Minimal Symbol object for iterator key
        let mut sym_obj = IndexMap::new();
        sym_obj.insert("iterator".to_string(), Value::String(crate::unicode::utf8_to_utf16("iterator")));
        self.globals
            .insert("Symbol".to_string(), Value::VmObject(Rc::new(RefCell::new(sym_obj))));
        self.globals
            .insert("parseInt".to_string(), Value::VmNativeFunction(BUILTIN_PARSEINT));
        self.globals
            .insert("parseFloat".to_string(), Value::VmNativeFunction(BUILTIN_PARSEFLOAT));
        self.globals.insert("eval".to_string(), Value::VmNativeFunction(BUILTIN_EVAL));

        // JSON object
        let mut json_map = IndexMap::new();
        json_map.insert("stringify".to_string(), Value::VmNativeFunction(BUILTIN_JSON_STRINGIFY));
        json_map.insert("parse".to_string(), Value::VmNativeFunction(BUILTIN_JSON_PARSE));
        self.globals
            .insert("JSON".to_string(), Value::VmObject(Rc::new(RefCell::new(json_map))));

        // Array.isArray and prototype
        let mut array_obj = IndexMap::new();
        array_obj.insert("isArray".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_ISARRAY));
        // Create Array.prototype with iterator method
        let mut arr_proto = IndexMap::new();
        arr_proto.insert("iterator".to_string(), Value::VmNativeFunction(BUILTIN_ARRAY_ITERATOR));
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
        self.globals.insert("Date".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_DATE));
        self.globals
            .insert("Boolean".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_BOOLEAN));
        // Object constructor with static methods
        let mut object_map = IndexMap::new();
        let object_proto = Rc::new(RefCell::new(IndexMap::new()));
        object_proto
            .borrow_mut()
            .insert("hasOwnProperty".to_string(), Value::VmNativeFunction(BUILTIN_OBJ_HASOWNPROPERTY));
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
        let object_val = Value::VmObject(Rc::new(RefCell::new(object_map)));
        // Set Object.prototype.constructor = Object (circular reference)
        object_proto.borrow_mut().insert("constructor".to_string(), object_val.clone());
        self.globals.insert("Object".to_string(), object_val);

        // Number object with constants and static methods
        let mut number_map = IndexMap::new();
        number_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_NUMBER as f64));
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
        self.globals
            .insert("String".to_string(), Value::VmObject(Rc::new(RefCell::new(string_map))));

        // Global constants
        self.globals.insert("Infinity".to_string(), Value::Number(f64::INFINITY));
        self.globals.insert("NaN".to_string(), Value::Number(f64::NAN));
        self.globals.insert("undefined".to_string(), Value::Undefined);

        // Map / Set constructors
        self.globals.insert("Map".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_MAP));
        self.globals.insert("Set".to_string(), Value::VmNativeFunction(BUILTIN_CTOR_SET));

        // globalThis — refers to the global this object
        self.globals
            .insert("globalThis".to_string(), Value::VmObject(self.global_this.clone()));

        // Function constructor with prototype (call, apply, bind)
        let mut fn_proto = IndexMap::new();
        fn_proto.insert("call".to_string(), Value::VmNativeFunction(BUILTIN_FN_CALL));
        fn_proto.insert("apply".to_string(), Value::VmNativeFunction(BUILTIN_FN_APPLY));
        fn_proto.insert("bind".to_string(), Value::VmNativeFunction(BUILTIN_FN_BIND));
        let fn_proto_val = Value::VmObject(Rc::new(RefCell::new(fn_proto)));
        let mut function_map = IndexMap::new();
        function_map.insert("__native_id__".to_string(), Value::Number(BUILTIN_CTOR_FUNCTION as f64));
        function_map.insert("prototype".to_string(), fn_proto_val);
        self.globals
            .insert("Function".to_string(), Value::VmObject(Rc::new(RefCell::new(function_map))));
    }

    /// Convert a value to string, calling toString() on VmObjects if available
    fn vm_to_string(&mut self, val: &Value<'gc>) -> String {
        if let Value::VmObject(map) = val {
            let ts = map.borrow().get("toString").cloned();
            if let Some(Value::VmFunction(ip, _arity)) = ts {
                let result = self.call_vm_function(ip, &[]);
                return value_to_string(&result);
            }
            // Check __value__ for wrapper objects (e.g. new String("abc"))
            let inner = map.borrow().get("__value__").cloned();
            if let Some(v) = inner {
                return value_to_string(&v);
            }
        }
        value_to_string(val)
    }

    /// Execute a native/built-in function
    fn call_builtin(&mut self, id: u8, args: Vec<Value<'gc>>) -> Value<'gc> {
        match id {
            BUILTIN_CONSOLE_LOG | BUILTIN_CONSOLE_WARN | BUILTIN_CONSOLE_ERROR => {
                let parts: Vec<String> = args.iter().map(|v| self.vm_to_string(v)).collect();
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
                match crate::core::compile_and_run_vm_snippet(&code) {
                    Ok(v) => crate::core::static_to_gc(v),
                    Err(_e) => Value::Undefined,
                }
            }
            BUILTIN_NEW_FUNCTION => {
                // new Function(body): return a callable wrapper with __fn_body__
                let body = args.first().map(value_to_string).unwrap_or_default();
                let mut map = IndexMap::new();
                map.insert("__fn_body__".to_string(), Value::String(crate::unicode::utf8_to_utf16(&body)));
                map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("Function")));
                Value::VmObject(Rc::new(RefCell::new(map)))
            }
            // Number() as function: convert argument to number
            BUILTIN_CTOR_NUMBER => {
                let n = args.first().map(|v| to_number(v)).unwrap_or(0.0);
                Value::Number(n)
            }
            // String() as function: convert argument to string
            BUILTIN_CTOR_STRING => {
                let s = match args.first() {
                    Some(v) => self.vm_to_string(v),
                    None => String::new(),
                };
                Value::String(crate::unicode::utf8_to_utf16(&s))
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
            // Error constructors called as functions (without `new`) — still create error objects
            BUILTIN_CTOR_ERROR
            | BUILTIN_CTOR_TYPEERROR
            | BUILTIN_CTOR_SYNTAXERROR
            | BUILTIN_CTOR_RANGEERROR
            | BUILTIN_CTOR_REFERENCEERROR => {
                let type_name = match id {
                    BUILTIN_CTOR_TYPEERROR => "TypeError",
                    BUILTIN_CTOR_SYNTAXERROR => "SyntaxError",
                    BUILTIN_CTOR_RANGEERROR => "RangeError",
                    BUILTIN_CTOR_REFERENCEERROR => "ReferenceError",
                    _ => "Error",
                };
                let msg = args.first().map(|v| self.vm_to_string(v)).unwrap_or_default();
                let mut map = IndexMap::new();
                map.insert("message".to_string(), Value::String(crate::unicode::utf8_to_utf16(&msg)));
                map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16(type_name)));
                Value::VmObject(Rc::new(RefCell::new(map)))
            }
            // Object.keys(obj) → array of own enumerable string keys
            BUILTIN_OBJECT_KEYS => {
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
            // Object.values(obj) → array of own enumerable values
            BUILTIN_OBJECT_VALUES => {
                if let Some(Value::VmObject(obj)) = args.first() {
                    let vals: Vec<Value<'gc>> = obj
                        .borrow()
                        .iter()
                        .filter(|(k, _)| !k.starts_with("__"))
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
                                .filter(|(k, _)| !k.starts_with("__"))
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
                        if let Some(val) = desc_borrow.get("value") {
                            obj.borrow_mut().insert(key.clone(), val.clone());
                        }
                        if let Some(Value::VmFunction(ip, _)) = desc_borrow.get("get") {
                            let getter_key = format!("__get_{}", key);
                            obj.borrow_mut().insert(getter_key, Value::VmFunction(*ip, 0));
                        }
                        if let Some(Value::VmFunction(ip, arity)) = desc_borrow.get("set") {
                            let setter_key = format!("__set_{}", key);
                            obj.borrow_mut().insert(setter_key, Value::VmFunction(*ip, *arity));
                        }
                        // Handle writable — for accessor descriptors (get/set), skip writable
                        let is_accessor = desc_borrow.contains_key("get") || desc_borrow.contains_key("set");
                        if !is_accessor && !matches!(desc_borrow.get("writable"), Some(Value::Boolean(true))) {
                            let ro_key = format!("__readonly_{}__", key);
                            obj.borrow_mut().insert(ro_key, Value::Boolean(true));
                        }
                    }
                    Value::VmObject(obj.clone())
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
                        if let Some(val) = borrow.get(&key) {
                            make_desc(val.clone(), !is_readonly, true, true)
                        } else {
                            // Check for getter
                            let getter_key = format!("__get_{}", key);
                            if let Some(getter) = borrow.get(&getter_key) {
                                let mut desc = IndexMap::new();
                                desc.insert("get".to_string(), getter.clone());
                                desc.insert("set".to_string(), Value::Undefined);
                                desc.insert("enumerable".to_string(), Value::Boolean(true));
                                desc.insert("configurable".to_string(), Value::Boolean(true));
                                Value::VmObject(Rc::new(RefCell::new(desc)))
                            } else {
                                Value::Undefined
                            }
                        }
                    }
                    Some(Value::VmFunction(ip, arity)) => {
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
                            if let Some(val) = desc_borrow.get("value") {
                                obj.borrow_mut().insert(key.clone(), val.clone());
                            }
                            if let Some(Value::VmFunction(ip, _)) = desc_borrow.get("get") {
                                let getter_key = format!("__get_{}", key);
                                obj.borrow_mut().insert(getter_key, Value::VmFunction(*ip, 0));
                            }
                            if let Some(Value::VmFunction(ip, arity)) = desc_borrow.get("set") {
                                let setter_key = format!("__set_{}", key);
                                obj.borrow_mut().insert(setter_key, Value::VmFunction(*ip, *arity));
                            }
                            if desc_borrow.contains_key("value") && !matches!(desc_borrow.get("writable"), Some(Value::Boolean(true))) {
                                let ro_key = format!("__readonly_{}__", key);
                                obj.borrow_mut().insert(ro_key, Value::Boolean(true));
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
                    && let Value::VmFunction(ip, _arity) = &callback
                {
                    let items: Vec<Value<'gc>> = arr.borrow().iter().cloned().collect();
                    for item in &items {
                        let key_val = self.call_vm_function(*ip, std::slice::from_ref(item));
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
            | BUILTIN_OBJECT_GETOWNPROPERTYNAMES => {
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
                BUILTIN_ARRAY_ITERATOR => {
                    let items = arr.borrow().elements.clone();
                    return self.make_iterator(items);
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
            if let Some(n) = num {
                match id {
                    BUILTIN_NUM_TOFIXED => {
                        let digits = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                        return Value::String(crate::unicode::utf8_to_utf16(&format!("{:.prec$}", n, prec = digits)));
                    }
                    BUILTIN_NUM_TOEXPONENTIAL => {
                        let digits = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                        return Value::String(crate::unicode::utf8_to_utf16(&format!("{:.prec$e}", n, prec = digits)));
                    }
                    BUILTIN_NUM_TOPRECISION => {
                        let prec = args.first().map(|v| to_number(v) as usize).unwrap_or(1);
                        // JS toPrecision: up to `prec` significant digits
                        let s = format!("{:.prec$e}", n, prec = prec.saturating_sub(1));
                        // Parse back and format without unnecessary trailing zeros
                        if let Ok(val) = s.parse::<f64>() {
                            let result = format!("{}", val);
                            // Pad with trailing zeros if needed
                            if !result.contains('.') && prec > result.len() {
                                return Value::String(crate::unicode::utf8_to_utf16(&format!(
                                    "{}.{}",
                                    result,
                                    "0".repeat(prec - result.len())
                                )));
                            }
                            return Value::String(crate::unicode::utf8_to_utf16(&result));
                        }
                        return Value::String(crate::unicode::utf8_to_utf16(&s));
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
        }

        // Map methods
        if let Value::VmMap(ref m) = receiver {
            match id {
                BUILTIN_MAP_SET => {
                    let key = args.first().cloned().unwrap_or(Value::Undefined);
                    let val = args.get(1).cloned().unwrap_or(Value::Undefined);
                    let mut borrow = m.borrow_mut();
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
                    if let Some(Value::VmFunction(ip, arity)) = args.first() {
                        let entries: Vec<(Value<'gc>, Value<'gc>)> = m.borrow().entries.clone();
                        let map_ref = receiver.clone();
                        for (k, v) in &entries {
                            if *arity >= 3 {
                                self.call_vm_function(*ip, &[v.clone(), k.clone(), map_ref.clone()]);
                            } else {
                                self.call_vm_function(*ip, &[v.clone(), k.clone()]);
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
                    if let Some(Value::VmFunction(ip, arity)) = args.first() {
                        let vals: Vec<Value<'gc>> = s.borrow().values.clone();
                        let set_ref = receiver.clone();
                        for v in &vals {
                            if *arity >= 3 {
                                self.call_vm_function(*ip, &[v.clone(), v.clone(), set_ref.clone()]);
                            } else {
                                self.call_vm_function(*ip, &[v.clone(), v.clone()]);
                            }
                        }
                    }
                    return Value::Undefined;
                }
                _ => {}
            }
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

        // Object.* static methods: delegate to call_builtin
        if (BUILTIN_OBJECT_KEYS..=BUILTIN_OBJECT_DEFINEPROP).contains(&id) {
            return self.call_builtin(id, args);
        }

        // Object.prototype.hasOwnProperty(key)
        if id == BUILTIN_OBJ_HASOWNPROPERTY {
            let key = args.first().map(value_to_string).unwrap_or_default();
            return match &receiver {
                Value::VmObject(map) => {
                    let has = map.borrow().contains_key(&key) && !key.starts_with("__proto__") && !key.starts_with("__type__");
                    Value::Boolean(has)
                }
                _ => Value::Boolean(false),
            };
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
                Value::VmFunction(ip, _arity) => {
                    self.this_stack.push(this_arg);
                    let result = self.call_vm_function(*ip, &call_args);
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
                Value::VmFunction(ip, _arity) => {
                    self.this_stack.push(this_arg);
                    let result = self.call_vm_function(*ip, &call_args);
                    self.this_stack.pop();
                    return result;
                }
                _ => return Value::Undefined,
            }
        }

        log::warn!("Unknown method builtin ID {} on {}", id, value_to_string(&receiver));
        Value::Undefined
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

    /// JS loose equality (==) with type coercion
    fn loose_equal(&self, a: &Value<'gc>, b: &Value<'gc>) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => x == y,
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
            is_method: false,
            arg_count: args.len(),
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
            let msg = match &thrown {
                Value::VmObject(map) => {
                    let b = map.borrow();
                    let type_name = b.get("__type__").map(|v| value_to_string(v)).unwrap_or_default();
                    let message = b.get("message").map(|v| value_to_string(v)).unwrap_or_default();
                    if !type_name.is_empty() && !message.is_empty() {
                        format!("{}: {}", type_name, message)
                    } else if !message.is_empty() {
                        message
                    } else {
                        value_to_string(&thrown)
                    }
                }
                _ => value_to_string(&thrown),
            };
            Err(crate::raise_syntax_error!(format!("Uncaught: {}", msg)))
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
                            // Pad missing args with Undefined
                            if (arg_count as u8) < arity {
                                for _ in 0..(arity as usize - arg_count) {
                                    self.stack.push(Value::Undefined);
                                }
                            }
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
                                };
                                self.frames.push(frame);
                            } else {
                                let frame = CallFrame {
                                    return_ip: self.ip,
                                    bp: callee_idx + 1,
                                    is_method: false,
                                    arg_count,
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
                            // Check if it's a Function wrapper (VmObject with __fn_body__ or __native_id__)
                            if let Value::VmObject(ref map) = callee {
                                let borrow = map.borrow();
                                if let Some(Value::Number(native_id)) = borrow.get("__native_id__") {
                                    let id = *native_id as u8;
                                    drop(borrow);
                                    let args_collected: Vec<Value<'gc>> = self.stack.drain(callee_idx + 1..).collect();
                                    self.stack.pop(); // pop callee
                                    if is_method {
                                        self.stack.pop(); // pop receiver
                                    }
                                    let result = self.call_builtin(id, args_collected);
                                    self.stack.push(result);
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
                                    let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                                    self.stack.truncate(base);
                                    self.stack.push(Value::Undefined);
                                }
                            } else {
                                log::warn!("Attempted to call non-function: {}", value_to_string(&callee));
                                let base = if is_method { callee_idx.saturating_sub(1) } else { callee_idx };
                                self.stack.truncate(base);
                                self.stack.push(Value::Undefined);
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
                    let is_a_str = matches!(&a, Value::String(_));
                    let is_b_str = matches!(&b, Value::String(_));
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
                    let a_num = to_number(&a);
                    let b_num = to_number(&b);
                    self.stack.push(Value::Number(a_num - b_num));
                }
                Opcode::Mul => {
                    let b = self.stack.pop().expect("VM Stack underflow on Mul (b)");
                    let a = self.stack.pop().expect("VM Stack underflow on Mul (a)");
                    self.stack.push(Value::Number(to_number(&a) * to_number(&b)));
                }
                Opcode::Div => {
                    let b = self.stack.pop().expect("VM Stack underflow on Div (b)");
                    let a = self.stack.pop().expect("VM Stack underflow on Div (a)");
                    self.stack.push(Value::Number(to_number(&a) / to_number(&b)));
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
                        (Value::VmNativeFunction(a_id), Value::VmNativeFunction(b_id)) => {
                            self.stack.push(Value::Boolean(a_id != b_id));
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
                    self.stack.push(Value::Number(to_number(&a) % to_number(&b)));
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
                        Value::Symbol(_) => "symbol",
                        Value::VmFunction(..) | Value::Closure(..) | Value::Function(..) | Value::VmNativeFunction(_) => "function",
                        Value::VmObject(map) => {
                            let b = map.borrow();
                            if b.contains_key("__fn_body__") || b.contains_key("__native_id__") {
                                "function"
                            } else {
                                "object"
                            }
                        }
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
                            let borrow = map.borrow();
                            // Check for getter first
                            let getter_key = format!("__get_{}", key);
                            if let Some(Value::VmFunction(ip, _)) = borrow.get(&getter_key) {
                                let ip = *ip;
                                drop(borrow);
                                // Push the object as `this` for the getter
                                self.this_stack.push(obj.clone());
                                let result = self.call_vm_function(ip, &[]);
                                self.this_stack.pop();
                                self.stack.push(result);
                            } else {
                                let val = borrow.get(&key).cloned();
                                if let Some(v) = val {
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
                                        let found = self.lookup_proto_chain(&effective_proto, &key);
                                        self.stack.push(found.unwrap_or(Value::Undefined));
                                    }
                                }
                            }
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
                            "iterator" => {
                                // lookup on Array.prototype so deletion propagates
                                if let Some(Value::VmObject(arr_ctor)) = self.globals.get("Array") {
                                    if let Some(Value::VmObject(proto)) = arr_ctor.borrow().get("prototype").cloned() {
                                        if let Some(v) = proto.borrow().get("iterator").cloned() {
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
                        Value::VmFunction(ip, arity) => {
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
                        let borrow = map.borrow();
                        let is_frozen = matches!(borrow.get("__frozen__"), Some(Value::Boolean(true)));
                        let is_non_ext = matches!(borrow.get("__non_extensible__"), Some(Value::Boolean(true)));
                        let key_exists = borrow.contains_key(&key);
                        let readonly_key = format!("__readonly_{}__", key);
                        let is_readonly = matches!(borrow.get(&readonly_key), Some(Value::Boolean(true)));
                        // Check prototype chain for setter
                        let setter_key = format!("__set_{}", key);
                        let setter = borrow.get(&setter_key).cloned().or_else(|| {
                            borrow
                                .get("__proto__")
                                .cloned()
                                .and_then(|proto| self.lookup_proto_chain(&Some(proto), &setter_key))
                        });
                        // Check prototype chain for non-writable inherited property
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
                        if let Some(Value::VmFunction(setter_ip, _)) = setter {
                            // Invoke setter with obj as this
                            self.this_stack.push(obj.clone());
                            let result = self.call_vm_function(setter_ip, std::slice::from_ref(&val));
                            self.this_stack.pop();
                            let _ = result;
                            self.stack.push(val);
                        } else if is_frozen || (is_non_ext && !key_exists) || is_readonly || proto_readonly {
                            let msg = if is_frozen {
                                format!("Cannot assign to read only property '{}' of object", key)
                            } else {
                                format!("Cannot add property {}, object is not extensible", key)
                            };
                            let mut err_map = IndexMap::new();
                            err_map.insert("message".to_string(), Value::String(crate::unicode::utf8_to_utf16(&msg)));
                            err_map.insert("__type__".to_string(), Value::String(crate::unicode::utf8_to_utf16("TypeError")));
                            let err = Value::VmObject(Rc::new(RefCell::new(err_map)));
                            self.handle_throw(err)?;
                            self.stack.push(val);
                        } else {
                            map.borrow_mut().insert(key.clone(), val.clone());
                            // If setting on globalThis, also write to globals
                            if Rc::ptr_eq(map, &self.global_this) {
                                self.globals.insert(key, val.clone());
                            }
                            self.stack.push(val);
                        }
                    } else if let Value::VmArray(arr) = &obj {
                        arr.borrow_mut().props.insert(key, val.clone());
                        self.stack.push(val);
                    } else if let Value::VmFunction(ip, arity) = &obj {
                        let props = self.get_fn_props(*ip, *arity);
                        props.borrow_mut().insert(key, val.clone());
                        self.stack.push(val);
                    } else {
                        log::warn!("SetProperty on non-object: {}", value_to_string(&obj));
                        self.stack.push(val);
                    }
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
                        Value::VmObject(map) => {
                            let borrow = map.borrow();
                            if let Some(v) = borrow.get(&key).cloned() {
                                v
                            } else {
                                // Check typed wrapper methods first
                                let type_name = borrow.get("__type__").map(|v| value_to_string(v));
                                let proto = borrow.get("__proto__").cloned();
                                drop(borrow);
                                let typed_result = match type_name.as_deref() {
                                    Some("Number") => match key.as_str() {
                                        "toFixed" => Some(Value::VmNativeFunction(BUILTIN_NUM_TOFIXED)),
                                        "toExponential" => Some(Value::VmNativeFunction(BUILTIN_NUM_TOEXPONENTIAL)),
                                        "toPrecision" => Some(Value::VmNativeFunction(BUILTIN_NUM_TOPRECISION)),
                                        "toString" => Some(Value::VmNativeFunction(BUILTIN_NUM_TOSTRING)),
                                        "valueOf" => Some(Value::VmNativeFunction(BUILTIN_NUM_VALUEOF)),
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
                            _ => Value::Undefined,
                        },
                        Value::VmFunction(..) | Value::VmNativeFunction(_) => match key.as_str() {
                            "call" => Value::VmNativeFunction(BUILTIN_FN_CALL),
                            "apply" => Value::VmNativeFunction(BUILTIN_FN_APPLY),
                            "bind" => Value::VmNativeFunction(BUILTIN_FN_BIND),
                            "name" => {
                                if let Value::VmFunction(ip, arity) = obj {
                                    let props = self.get_fn_props(*ip, *arity);
                                    props.borrow().get("name").cloned().unwrap_or(Value::Undefined)
                                } else {
                                    Value::Undefined
                                }
                            }
                            "prototype" => {
                                if let Value::VmFunction(ip, arity) = obj {
                                    let props = self.get_fn_props(*ip, *arity);
                                    props.borrow().get("prototype").cloned().unwrap_or(Value::Undefined)
                                } else {
                                    Value::Undefined
                                }
                            }
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
                    log::warn!(
                        "CollectRest: non_rest={}, actual_args={}, bp={}, stack_len={}",
                        non_rest_count,
                        actual_arg_count,
                        bp,
                        self.stack.len()
                    );
                    if actual_arg_count > non_rest_count {
                        // Collect excess args from stack positions bp+non_rest_count..bp+actual_arg_count
                        let start = bp + non_rest_count;
                        let end = bp + actual_arg_count;
                        log::warn!("CollectRest: collecting stack[{}..{}]", start, end);
                        let rest_elems: Vec<Value<'gc>> = self.stack[start..end].to_vec();
                        // Remove the excess args from the stack
                        self.stack.drain(start..end);
                        // Push the rest array
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
                    // Map rhs constructor sentinel to name
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
                            _ => "",
                        },
                        Value::VmObject(map) => {
                            let b = map.borrow();
                            if let Some(Value::Number(id)) = b.get("__native_id__") {
                                match *id as u8 {
                                    BUILTIN_CTOR_NUMBER => "Number",
                                    BUILTIN_CTOR_STRING => "String",
                                    BUILTIN_CTOR_BOOLEAN => "Boolean",
                                    BUILTIN_CTOR_OBJECT => "Object",
                                    BUILTIN_CTOR_DATE => "Date",
                                    BUILTIN_CTOR_FUNCTION => "Function",
                                    _ => "",
                                }
                            } else {
                                ""
                            }
                        }
                        Value::String(s) => {
                            // Fallback for string sentinels
                            let name = crate::unicode::utf16_to_utf8(s);
                            // Leak is avoided; use a match approach instead
                            match name.as_str() {
                                "Error" | "TypeError" | "SyntaxError" | "RangeError" | "ReferenceError" | "Date" | "Function"
                                | "Number" | "String" | "Boolean" | "Object" => {
                                    // handled below via value_to_string
                                    ""
                                }
                                _ => "",
                            }
                        }
                        _ => "",
                    };
                    // For VmNativeFunction-based check
                    let ctor_str = if ctor_name.is_empty() {
                        value_to_string(&rhs)
                    } else {
                        ctor_name.to_string()
                    };
                    let result = if let Value::VmObject(map) = &lhs {
                        let borrow = map.borrow();
                        if let Some(Value::String(type_u16)) = borrow.get("__type__") {
                            let type_name = crate::unicode::utf16_to_utf8(type_u16);
                            match ctor_str.as_str() {
                                "Error" => type_name == "Error" || type_name.ends_with("Error"),
                                "Object" => true, // all VmObjects are instances of Object
                                _ => type_name == ctor_str,
                            }
                        } else {
                            // Object without __type__: instanceof Object = true
                            ctor_str == "Object"
                        }
                    } else if ctor_str == "Function" {
                        // VmNativeFunction / VmFunction are instances of Function
                        matches!(&lhs, Value::VmNativeFunction(_) | Value::VmFunction(..))
                            || matches!(&lhs, Value::VmObject(m) if m.borrow().contains_key("__native_id__"))
                    } else {
                        false
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
                        Value::VmFunction(target_ip, _arity) => {
                            // Create new empty object as `this`
                            let new_obj = Rc::new(RefCell::new(IndexMap::new()));
                            // Set __proto__ to constructor's prototype property
                            let fn_props = self.get_fn_props(target_ip, _arity);
                            if let Some(proto) = fn_props.borrow().get("prototype").cloned() {
                                new_obj.borrow_mut().insert("__proto__".to_string(), proto);
                            }
                            let this_val = Value::VmObject(new_obj.clone());
                            self.this_stack.push(this_val);
                            // Set up call frame
                            let frame = CallFrame {
                                return_ip: self.ip,
                                bp: callee_idx + 1,
                                is_method: false,
                                arg_count,
                            };
                            self.frames.push(frame);
                            self.ip = target_ip;
                            // Run the constructor
                            let result = self.run_inner(self.frames.len());
                            self.this_stack.pop();
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
                                    self.stack.push(Value::VmMap(Rc::new(RefCell::new(VmMapData { entries }))));
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
                                    self.stack.push(Value::VmSet(Rc::new(RefCell::new(VmSetData { values }))));
                                }
                                _ => {
                                    log::warn!("NewCall on VmNativeFunction #{}: returning empty object", id);
                                    self.stack.push(Value::VmObject(Rc::new(RefCell::new(IndexMap::new()))));
                                }
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
                            let key = value_to_string(&idx_val);
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
