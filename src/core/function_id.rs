use crate::core::{GcContext, Value, VmObjectHandle};

pub type FunctionID = usize;

#[inline]
pub fn get_function_id<'gc>(obj: VmObjectHandle<'gc>) -> Option<FunctionID> {
    obj.borrow().get("__native_id__").and_then(|v| match v {
        Value::Number(n) => Some(*n as FunctionID),
        _ => None,
    })
}

#[inline]
#[allow(dead_code)]
pub fn set_function_id<'gc>(ctx: &GcContext<'gc>, obj: VmObjectHandle<'gc>, id: FunctionID) {
    obj.borrow_mut(ctx).insert("__native_id__".to_string(), Value::Number(id as f64));
}

// Builtin function IDs
// ── Console (0–9) ───────────────────────────────────────────────────
pub(crate) const BUILTIN_CONSOLE_LOG: FunctionID = 0;
pub(crate) const BUILTIN_CONSOLE_WARN: FunctionID = 1;
pub(crate) const BUILTIN_CONSOLE_ERROR: FunctionID = 2;
// ── Math (10–49) ────────────────────────────────────────────────────
pub(crate) const BUILTIN_MATH_FLOOR: FunctionID = 10;
pub(crate) const BUILTIN_MATH_CEIL: FunctionID = 11;
pub(crate) const BUILTIN_MATH_ROUND: FunctionID = 12;
pub(crate) const BUILTIN_MATH_ABS: FunctionID = 13;
pub(crate) const BUILTIN_MATH_SQRT: FunctionID = 14;
pub(crate) const BUILTIN_MATH_MAX: FunctionID = 15;
pub(crate) const BUILTIN_MATH_MIN: FunctionID = 16;
pub(crate) const BUILTIN_MATH_SIN: FunctionID = 17;
pub(crate) const BUILTIN_MATH_COS: FunctionID = 18;
pub(crate) const BUILTIN_MATH_TAN: FunctionID = 19;
pub(crate) const BUILTIN_MATH_ASIN: FunctionID = 20;
pub(crate) const BUILTIN_MATH_ACOS: FunctionID = 21;
pub(crate) const BUILTIN_MATH_ATAN: FunctionID = 22;
pub(crate) const BUILTIN_MATH_ATAN2: FunctionID = 23;
pub(crate) const BUILTIN_MATH_SINH: FunctionID = 24;
pub(crate) const BUILTIN_MATH_COSH: FunctionID = 25;
pub(crate) const BUILTIN_MATH_TANH: FunctionID = 26;
pub(crate) const BUILTIN_MATH_ASINH: FunctionID = 27;
pub(crate) const BUILTIN_MATH_ACOSH: FunctionID = 28;
pub(crate) const BUILTIN_MATH_ATANH: FunctionID = 29;
pub(crate) const BUILTIN_MATH_EXP: FunctionID = 30;
pub(crate) const BUILTIN_MATH_EXPM1: FunctionID = 31;
pub(crate) const BUILTIN_MATH_LOG: FunctionID = 32;
pub(crate) const BUILTIN_MATH_LOG10: FunctionID = 33;
pub(crate) const BUILTIN_MATH_LOG1P: FunctionID = 34;
pub(crate) const BUILTIN_MATH_LOG2: FunctionID = 35;
pub(crate) const BUILTIN_MATH_FROUND: FunctionID = 36;
pub(crate) const BUILTIN_MATH_TRUNC: FunctionID = 37;
pub(crate) const BUILTIN_MATH_CBRT: FunctionID = 38;
pub(crate) const BUILTIN_MATH_HYPOT: FunctionID = 39;
pub(crate) const BUILTIN_MATH_SIGN: FunctionID = 40;
pub(crate) const BUILTIN_MATH_POW: FunctionID = 41;
pub(crate) const BUILTIN_MATH_RANDOM: FunctionID = 42;
pub(crate) const BUILTIN_MATH_CLZ32: FunctionID = 43;
pub(crate) const BUILTIN_MATH_IMUL: FunctionID = 44;
pub(crate) const BUILTIN_MATH_SUMPRECISE: FunctionID = 45;
// ── Global functions (50–59) ────────────────────────────────────────
pub(crate) const BUILTIN_ISNAN: FunctionID = 50;
pub(crate) const BUILTIN_PARSEINT: FunctionID = 51;
pub(crate) const BUILTIN_PARSEFLOAT: FunctionID = 52;
pub(crate) const BUILTIN_EVAL: FunctionID = 53;
pub(crate) const BUILTIN_NEW_FUNCTION: FunctionID = 54;
// ── Array (60–109) ──────────────────────────────────────────────────
pub(crate) const BUILTIN_ARRAY_PUSH: FunctionID = 60;
pub(crate) const BUILTIN_ARRAY_POP: FunctionID = 61;
pub(crate) const BUILTIN_ARRAY_JOIN: FunctionID = 62;
pub(crate) const BUILTIN_ARRAY_INDEXOF: FunctionID = 63;
pub(crate) const BUILTIN_ARRAY_SLICE: FunctionID = 64;
pub(crate) const BUILTIN_ARRAY_CONCAT: FunctionID = 65;
pub(crate) const BUILTIN_ARRAY_MAP: FunctionID = 66;
pub(crate) const BUILTIN_ARRAY_FILTER: FunctionID = 67;
pub(crate) const BUILTIN_ARRAY_FOREACH: FunctionID = 68;
pub(crate) const BUILTIN_ARRAY_ISARRAY: FunctionID = 69;
pub(crate) const BUILTIN_ARRAY_REDUCE: FunctionID = 70;
pub(crate) const BUILTIN_ARRAY_REDUCERIGHT: FunctionID = 71;
pub(crate) const BUILTIN_CTOR_ARRAY: FunctionID = 72;
pub(crate) const BUILTIN_ARRAY_OF: FunctionID = 73;
pub(crate) const BUILTIN_ARRAY_FROM: FunctionID = 74;
pub(crate) const BUILTIN_ARRAY_SHIFT: FunctionID = 75;
pub(crate) const BUILTIN_ARRAY_UNSHIFT: FunctionID = 76;
pub(crate) const BUILTIN_ARRAY_SPLICE: FunctionID = 77;
pub(crate) const BUILTIN_ARRAY_REVERSE: FunctionID = 78;
pub(crate) const BUILTIN_ARRAY_SORT: FunctionID = 79;
pub(crate) const BUILTIN_ARRAY_FIND: FunctionID = 80;
pub(crate) const BUILTIN_ARRAY_FINDINDEX: FunctionID = 81;
pub(crate) const BUILTIN_ARRAY_INCLUDES: FunctionID = 82;
pub(crate) const BUILTIN_ARRAY_FLAT: FunctionID = 83;
pub(crate) const BUILTIN_ARRAY_FLATMAP: FunctionID = 84;
pub(crate) const BUILTIN_ARRAY_AT: FunctionID = 85;
pub(crate) const BUILTIN_ARRAY_EVERY: FunctionID = 86;
pub(crate) const BUILTIN_ARRAY_SOME: FunctionID = 87;
pub(crate) const BUILTIN_ARRAY_FILL: FunctionID = 88;
pub(crate) const BUILTIN_ARRAY_LASTINDEXOF: FunctionID = 89;
pub(crate) const BUILTIN_ARRAY_FINDLAST: FunctionID = 90;
pub(crate) const BUILTIN_ARRAY_FINDLASTINDEX: FunctionID = 91;
pub(crate) const BUILTIN_ARRAY_ITERATOR: FunctionID = 92;
pub(crate) const BUILTIN_ARRAY_FROMASYNC: FunctionID = 93;
// ── String (110–159) ────────────────────────────────────────────────
pub(crate) const BUILTIN_STRING_SPLIT: FunctionID = 110;
pub(crate) const BUILTIN_STRING_INDEXOF: FunctionID = 111;
pub(crate) const BUILTIN_STRING_SLICE: FunctionID = 112;
pub(crate) const BUILTIN_STRING_TOUPPERCASE: FunctionID = 113;
pub(crate) const BUILTIN_STRING_TOLOWERCASE: FunctionID = 114;
pub(crate) const BUILTIN_STRING_TRIM: FunctionID = 115;
pub(crate) const BUILTIN_STRING_CHARAT: FunctionID = 116;
pub(crate) const BUILTIN_STRING_INCLUDES: FunctionID = 117;
pub(crate) const BUILTIN_STRING_REPLACE: FunctionID = 118;
pub(crate) const BUILTIN_STRING_STARTSWITH: FunctionID = 119;
pub(crate) const BUILTIN_STRING_ENDSWITH: FunctionID = 120;
pub(crate) const BUILTIN_STRING_SUBSTRING: FunctionID = 121;
pub(crate) const BUILTIN_STRING_PADSTART: FunctionID = 122;
pub(crate) const BUILTIN_STRING_PADEND: FunctionID = 123;
pub(crate) const BUILTIN_STRING_REPEAT: FunctionID = 124;
pub(crate) const BUILTIN_STRING_CHARCODEAT: FunctionID = 125;
pub(crate) const BUILTIN_STRING_FROMCHARCODE: FunctionID = 126;
pub(crate) const BUILTIN_STRING_TRIMSTART: FunctionID = 127;
pub(crate) const BUILTIN_STRING_TRIMEND: FunctionID = 128;
pub(crate) const BUILTIN_STRING_LASTINDEXOF: FunctionID = 129;
pub(crate) const BUILTIN_STRING_MATCH: FunctionID = 130;
pub(crate) const BUILTIN_STRING_REPLACEALL: FunctionID = 131;
pub(crate) const BUILTIN_STRING_SEARCH: FunctionID = 132;
pub(crate) const BUILTIN_STRING_TOSTRING: FunctionID = 133;
pub(crate) const BUILTIN_STRING_VALUEOF: FunctionID = 134;
pub(crate) const BUILTIN_CTOR_STRING: FunctionID = 135;
pub(crate) const BUILTIN_STRING_RAW: FunctionID = 136;
pub(crate) const BUILTIN_STRING_CODEPOINTAT: FunctionID = 137;
pub(crate) const BUILTIN_STRING_NORMALIZE: FunctionID = 138;
pub(crate) const BUILTIN_STRING_MATCHALL: FunctionID = 139;
pub(crate) const BUILTIN_STRING_FROMCODEPOINT: FunctionID = 140;
pub(crate) const BUILTIN_STRING_AT: FunctionID = 141;
pub(crate) const BUILTIN_STRING_ISWELLFORMED: FunctionID = 142;
pub(crate) const BUILTIN_STRING_TOWELLFORMED: FunctionID = 143;
// ── Number (160–179) ────────────────────────────────────────────────
pub(crate) const BUILTIN_CTOR_NUMBER: FunctionID = 160;
pub(crate) const BUILTIN_NUMBER_ISNAN: FunctionID = 161;
pub(crate) const BUILTIN_NUMBER_ISFINITE: FunctionID = 162;
pub(crate) const BUILTIN_NUMBER_ISINTEGER: FunctionID = 163;
pub(crate) const BUILTIN_NUMBER_ISSAFEINTEGER: FunctionID = 164;
pub(crate) const BUILTIN_NUM_TOFIXED: FunctionID = 165;
pub(crate) const BUILTIN_NUM_TOEXPONENTIAL: FunctionID = 166;
pub(crate) const BUILTIN_NUM_TOPRECISION: FunctionID = 167;
pub(crate) const BUILTIN_NUM_TOSTRING: FunctionID = 168;
pub(crate) const BUILTIN_NUM_VALUEOF: FunctionID = 169;
pub(crate) const BUILTIN_NUM_TOLOCALESTRING: FunctionID = 170;
// ── BigInt (180–189) ────────────────────────────────────────────────
pub(crate) const BUILTIN_BIGINT: FunctionID = 180;
pub(crate) const BUILTIN_BIGINT_ASUINTN: FunctionID = 181;
pub(crate) const BUILTIN_BIGINT_ASINTN: FunctionID = 182;
pub(crate) const BUILTIN_BIGINT_TOSTRING: FunctionID = 183;
pub(crate) const BUILTIN_BIGINT_VALUEOF: FunctionID = 184;
pub(crate) const BUILTIN_BIGINT_TOLOCALESTRING: FunctionID = 185;
// ── Boolean (190–194) ───────────────────────────────────────────────
pub(crate) const BUILTIN_CTOR_BOOLEAN: FunctionID = 190;
// ── Object (200–229) ────────────────────────────────────────────────
pub(crate) const BUILTIN_CTOR_OBJECT: FunctionID = 200;
pub(crate) const BUILTIN_OBJECT_KEYS: FunctionID = 201;
pub(crate) const BUILTIN_OBJECT_VALUES: FunctionID = 202;
pub(crate) const BUILTIN_OBJECT_ENTRIES: FunctionID = 203;
pub(crate) const BUILTIN_OBJECT_ASSIGN: FunctionID = 204;
pub(crate) const BUILTIN_OBJECT_FREEZE: FunctionID = 205;
pub(crate) const BUILTIN_OBJECT_HASOWN: FunctionID = 206;
pub(crate) const BUILTIN_OBJECT_CREATE: FunctionID = 207;
pub(crate) const BUILTIN_OBJECT_GETPROTOTYPEOF: FunctionID = 208;
pub(crate) const BUILTIN_OBJECT_DEFINEPROPS: FunctionID = 209;
pub(crate) const BUILTIN_OBJECT_PREVENTEXT: FunctionID = 210;
pub(crate) const BUILTIN_OBJECT_GROUPBY: FunctionID = 211;
pub(crate) const BUILTIN_OBJECT_DEFINEPROP: FunctionID = 212;
pub(crate) const BUILTIN_OBJ_HASOWNPROPERTY: FunctionID = 213;
pub(crate) const BUILTIN_OBJECT_GETOWNPROPDESC: FunctionID = 214;
pub(crate) const BUILTIN_OBJECT_SETPROTOTYPEOF: FunctionID = 215;
pub(crate) const BUILTIN_OBJECT_GETOWNPROPERTYNAMES: FunctionID = 216;
pub(crate) const BUILTIN_OBJ_TOSTRING: FunctionID = 217;
pub(crate) const BUILTIN_OBJECT_FROMENTRIES: FunctionID = 218;
// ── Function (230–239) ──────────────────────────────────────────────
pub(crate) const BUILTIN_CTOR_FUNCTION: FunctionID = 230;
pub(crate) const BUILTIN_FN_CALL: FunctionID = 231;
pub(crate) const BUILTIN_FN_BIND: FunctionID = 232;
pub(crate) const BUILTIN_FN_APPLY: FunctionID = 233;
pub(crate) const BUILTIN_FN_TOSTRING: FunctionID = 234;
// ── JSON (240–249) ──────────────────────────────────────────────────
pub(crate) const BUILTIN_JSON_STRINGIFY: FunctionID = 240;
pub(crate) const BUILTIN_JSON_PARSE: FunctionID = 241;
pub(crate) const BUILTIN_JSON_RAWJSON: FunctionID = 242;
pub(crate) const BUILTIN_JSON_ISRAWJSON: FunctionID = 243;
// ── RegExp (250–259) ────────────────────────────────────────────────
pub(crate) const BUILTIN_CTOR_REGEXP: FunctionID = 250;
pub(crate) const BUILTIN_REGEX_EXEC: FunctionID = 251;
pub(crate) const BUILTIN_REGEX_TEST: FunctionID = 252;
pub(crate) const BUILTIN_REGEXP_ESCAPE: FunctionID = 253;
// ── Error constructors (260–269) ────────────────────────────────────
pub(crate) const BUILTIN_CTOR_ERROR: FunctionID = 260;
pub(crate) const BUILTIN_CTOR_TYPEERROR: FunctionID = 261;
pub(crate) const BUILTIN_CTOR_SYNTAXERROR: FunctionID = 262;
pub(crate) const BUILTIN_CTOR_RANGEERROR: FunctionID = 263;
pub(crate) const BUILTIN_CTOR_REFERENCEERROR: FunctionID = 264;
pub(crate) const BUILTIN_CTOR_EVALERROR: FunctionID = 265;
pub(crate) const BUILTIN_CTOR_URIERROR: FunctionID = 266;
pub(crate) const BUILTIN_ERROR_ISERROR: FunctionID = 267;
// ── Date (270–319) ──────────────────────────────────────────────────
pub(crate) const BUILTIN_CTOR_DATE: FunctionID = 270;
pub(crate) const BUILTIN_DATE_NOW: FunctionID = 271;
pub(crate) const BUILTIN_DATE_GETTIME: FunctionID = 272;
pub(crate) const BUILTIN_DATE_TOSTRING: FunctionID = 273;
pub(crate) const BUILTIN_DATE_TOLOCALEDATESTRING: FunctionID = 274;
pub(crate) const BUILTIN_DATE_GETFULLYEAR: FunctionID = 275;
pub(crate) const BUILTIN_DATE_GETMONTH: FunctionID = 276;
pub(crate) const BUILTIN_DATE_GETDATE: FunctionID = 277;
pub(crate) const BUILTIN_DATE_GETDAY: FunctionID = 278;
pub(crate) const BUILTIN_DATE_GETHOURS: FunctionID = 279;
pub(crate) const BUILTIN_DATE_GETMINUTES: FunctionID = 280;
pub(crate) const BUILTIN_DATE_GETSECONDS: FunctionID = 281;
pub(crate) const BUILTIN_DATE_GETMILLISECONDS: FunctionID = 282;
pub(crate) const BUILTIN_DATE_VALUEOF: FunctionID = 283;
pub(crate) const BUILTIN_DATE_SETFULLYEAR: FunctionID = 284;
pub(crate) const BUILTIN_DATE_SETMONTH: FunctionID = 285;
pub(crate) const BUILTIN_DATE_SETDATE: FunctionID = 286;
pub(crate) const BUILTIN_DATE_SETHOURS: FunctionID = 287;
pub(crate) const BUILTIN_DATE_SETMINUTES: FunctionID = 288;
pub(crate) const BUILTIN_DATE_TOLOCALETIMESTRING: FunctionID = 289;
pub(crate) const BUILTIN_DATE_TOLOCALESTRING: FunctionID = 290;
pub(crate) const BUILTIN_DATE_TOISOSTRING: FunctionID = 291;
pub(crate) const BUILTIN_DATE_GETUTCFULLYEAR: FunctionID = 292;
pub(crate) const BUILTIN_DATE_GETUTCMONTH: FunctionID = 293;
pub(crate) const BUILTIN_DATE_GETUTCDATE: FunctionID = 294;
pub(crate) const BUILTIN_DATE_GETUTCHOURS: FunctionID = 295;
pub(crate) const BUILTIN_DATE_GETUTCMINUTES: FunctionID = 296;
pub(crate) const BUILTIN_DATE_GETUTCSECONDS: FunctionID = 297;
pub(crate) const BUILTIN_DATE_GETTIMEZONEOFFSET: FunctionID = 298;
pub(crate) const BUILTIN_DATE_PARSE: FunctionID = 299;
pub(crate) const BUILTIN_DATE_SETTIME: FunctionID = 300;
pub(crate) const BUILTIN_DATE_TODATESTRING: FunctionID = 301;
pub(crate) const BUILTIN_DATE_GETUTCDAY: FunctionID = 302;
pub(crate) const BUILTIN_DATE_GETUTCMILLISECONDS: FunctionID = 303;
pub(crate) const BUILTIN_DATE_TOUTCSTRING: FunctionID = 304;
pub(crate) const BUILTIN_DATE_GETYEAR: FunctionID = 305;
pub(crate) const BUILTIN_DATE_SETYEAR: FunctionID = 306;
// ── Timers (320–329) ────────────────────────────────────────────────
pub(crate) const BUILTIN_SETTIMEOUT: FunctionID = 320;
pub(crate) const BUILTIN_CLEARTIMEOUT: FunctionID = 321;
pub(crate) const BUILTIN_SETINTERVAL: FunctionID = 322;
pub(crate) const BUILTIN_CLEARINTERVAL: FunctionID = 323;
// ── Map (330–349) ───────────────────────────────────────────────────
pub(crate) const BUILTIN_CTOR_MAP: FunctionID = 330;
pub(crate) const BUILTIN_MAP_SET: FunctionID = 331;
pub(crate) const BUILTIN_MAP_GET: FunctionID = 332;
pub(crate) const BUILTIN_MAP_HAS: FunctionID = 333;
pub(crate) const BUILTIN_MAP_DELETE: FunctionID = 334;
pub(crate) const BUILTIN_MAP_KEYS: FunctionID = 335;
pub(crate) const BUILTIN_MAP_VALUES: FunctionID = 336;
pub(crate) const BUILTIN_MAP_ENTRIES: FunctionID = 337;
pub(crate) const BUILTIN_MAP_FOREACH: FunctionID = 338;
pub(crate) const BUILTIN_MAP_CLEAR: FunctionID = 339;
pub(crate) const BUILTIN_MAP_GROUPBY: FunctionID = 340;
// ── Set (350–369) ───────────────────────────────────────────────────
pub(crate) const BUILTIN_CTOR_SET: FunctionID = 350;
pub(crate) const BUILTIN_SET_ADD: FunctionID = 351;
pub(crate) const BUILTIN_SET_HAS: FunctionID = 352;
pub(crate) const BUILTIN_SET_DELETE: FunctionID = 353;
pub(crate) const BUILTIN_SET_VALUES: FunctionID = 354;
pub(crate) const BUILTIN_SET_ENTRIES: FunctionID = 355;
pub(crate) const BUILTIN_SET_FOREACH: FunctionID = 356;
pub(crate) const BUILTIN_SET_CLEAR: FunctionID = 357;
pub(crate) const BUILTIN_SET_UNION: FunctionID = 358;
pub(crate) const BUILTIN_SET_INTERSECTION: FunctionID = 359;
pub(crate) const BUILTIN_SET_DIFFERENCE: FunctionID = 360;
pub(crate) const BUILTIN_SET_SYMMETRIC_DIFFERENCE: FunctionID = 361;
pub(crate) const BUILTIN_SET_IS_SUBSET_OF: FunctionID = 362;
pub(crate) const BUILTIN_SET_IS_SUPERSET_OF: FunctionID = 363;
pub(crate) const BUILTIN_SET_IS_DISJOINT_FROM: FunctionID = 364;
// ── Iterator (370–374) ──────────────────────────────────────────────
pub(crate) const BUILTIN_ITERATOR_NEXT: FunctionID = 370;
// ── Generator (375–379) ─────────────────────────────────────────────
pub(crate) const BUILTIN_GEN_NEXT: FunctionID = 375;
pub(crate) const BUILTIN_GEN_THROW: FunctionID = 376;
pub(crate) const BUILTIN_GEN_RETURN: FunctionID = 377;
// ── Async generator (380–389) ───────────────────────────────────────
pub(crate) const BUILTIN_ASYNCGEN_NEXT: FunctionID = 380;
pub(crate) const BUILTIN_ASYNCGEN_THROW: FunctionID = 381;
pub(crate) const BUILTIN_ASYNCGEN_RETURN: FunctionID = 382;
pub(crate) const BUILTIN_WEAKMAP_SET: FunctionID = 383;
pub(crate) const BUILTIN_WEAKMAP_GET: FunctionID = 384;
pub(crate) const BUILTIN_WEAKMAP_HAS: FunctionID = 385;
pub(crate) const BUILTIN_WEAKMAP_DELETE: FunctionID = 386;
// ── Weak collections / WeakRef (390–399) ────────────────────────────
pub(crate) const BUILTIN_CTOR_WEAKMAP: FunctionID = 390;
pub(crate) const BUILTIN_CTOR_WEAKSET: FunctionID = 391;
pub(crate) const BUILTIN_CTOR_WEAKREF: FunctionID = 392;
pub(crate) const BUILTIN_WEAKREF_DEREF: FunctionID = 393;
pub(crate) const BUILTIN_WEAKSET_ADD: FunctionID = 394;
pub(crate) const BUILTIN_WEAKSET_HAS: FunctionID = 395;
pub(crate) const BUILTIN_WEAKSET_DELETE: FunctionID = 396;
// ── Symbol (400–409) ────────────────────────────────────────────────
pub(crate) const BUILTIN_SYMBOL: FunctionID = 400;
pub(crate) const BUILTIN_SYMBOL_FOR: FunctionID = 401;
pub(crate) const BUILTIN_SYMBOL_KEYFOR: FunctionID = 402;
// ── FinalizationRegistry (410–419) ──────────────────────────────────
pub(crate) const BUILTIN_CTOR_FR: FunctionID = 410;
pub(crate) const BUILTIN_FR_REGISTER: FunctionID = 411;
pub(crate) const BUILTIN_FR_UNREGISTER: FunctionID = 412;
// ── Promise (420–429) ───────────────────────────────────────────────
pub(crate) const BUILTIN_CTOR_PROMISE: FunctionID = 420;
pub(crate) const BUILTIN_PROMISE_RESOLVE: FunctionID = 421;
pub(crate) const BUILTIN_PROMISE_ALL: FunctionID = 422;
pub(crate) const BUILTIN_PROMISE_THEN: FunctionID = 423;
pub(crate) const BUILTIN_PROMISE_NOOP: FunctionID = 424;
pub(crate) const BUILTIN_PROMISE_WITHRESOLVERS: FunctionID = 425;
pub(crate) const BUILTIN_PROMISE_TRY: FunctionID = 426;
// ── Proxy (430–434) ─────────────────────────────────────────────────
pub(crate) const BUILTIN_CTOR_PROXY: FunctionID = 430;
// ── Reflect (435–439) ───────────────────────────────────────────────
pub(crate) const BUILTIN_REFLECT_APPLY: FunctionID = 435;
// ── ArrayBuffer (440–449) ───────────────────────────────────────────
pub(crate) const BUILTIN_CTOR_ARRAYBUFFER: FunctionID = 440;
pub(crate) const BUILTIN_ARRAYBUFFER_RESIZE: FunctionID = 441;
// ── DataView (450–459) ──────────────────────────────────────────────
pub(crate) const BUILTIN_CTOR_DATAVIEW: FunctionID = 450;
// ── TypedArray constructors (460–479) ───────────────────────────────
pub(crate) const BUILTIN_CTOR_INT8ARRAY: FunctionID = 460;
pub(crate) const BUILTIN_CTOR_UINT8ARRAY: FunctionID = 461;
pub(crate) const BUILTIN_CTOR_UINT8CLAMPEDARRAY: FunctionID = 462;
pub(crate) const BUILTIN_CTOR_INT16ARRAY: FunctionID = 463;
pub(crate) const BUILTIN_CTOR_UINT16ARRAY: FunctionID = 464;
pub(crate) const BUILTIN_CTOR_INT32ARRAY: FunctionID = 465;
pub(crate) const BUILTIN_CTOR_UINT32ARRAY: FunctionID = 466;
pub(crate) const BUILTIN_CTOR_FLOAT32ARRAY: FunctionID = 467;
pub(crate) const BUILTIN_CTOR_FLOAT64ARRAY: FunctionID = 468;
pub(crate) const BUILTIN_CTOR_BIGINT64ARRAY: FunctionID = 469;
pub(crate) const BUILTIN_CTOR_BIGUINT64ARRAY: FunctionID = 470;
// ── SharedArrayBuffer (480–489) ─────────────────────────────────────
pub(crate) const BUILTIN_CTOR_SHAREDARRAYBUFFER: FunctionID = 480;
pub(crate) const BUILTIN_SHAREDARRAYBUFFER_GROW: FunctionID = 481;
// ── Atomics (490–509) ───────────────────────────────────────────────
pub(crate) const BUILTIN_ATOMICS_ISLOCKFREE: FunctionID = 490;
pub(crate) const BUILTIN_ATOMICS_LOAD: FunctionID = 491;
pub(crate) const BUILTIN_ATOMICS_STORE: FunctionID = 492;
pub(crate) const BUILTIN_ATOMICS_COMPAREEXCHANGE: FunctionID = 493;
pub(crate) const BUILTIN_ATOMICS_ADD: FunctionID = 494;
pub(crate) const BUILTIN_ATOMICS_EXCHANGE: FunctionID = 495;
pub(crate) const BUILTIN_ATOMICS_WAIT: FunctionID = 496;
pub(crate) const BUILTIN_ATOMICS_NOTIFY: FunctionID = 497;
pub(crate) const BUILTIN_ATOMICS_WAITASYNC: FunctionID = 498;
pub(crate) const BUILTIN_ATOMICS_PAUSE: FunctionID = 499;
// ── AbstractModuleSource (510–514) ──────────────────────────────────
pub(crate) const BUILTIN_CTOR_ABSTRACT_MODULE_SOURCE: FunctionID = 510;
pub(crate) const BUILTIN_ABSTRACT_MODULE_SOURCE_TOSTRINGTAG_GET: FunctionID = 511;
// ── DisposableStack (520–529) ───────────────────────────────────────
pub(crate) const BUILTIN_CTOR_DISPOSABLESTACK: FunctionID = 520;
pub(crate) const BUILTIN_DISPOSABLESTACK_DISPOSE: FunctionID = 521;
pub(crate) const BUILTIN_DISPOSABLESTACK_USE: FunctionID = 522;
pub(crate) const BUILTIN_DISPOSABLESTACK_ADOPT: FunctionID = 523;
pub(crate) const BUILTIN_DISPOSABLESTACK_DEFER: FunctionID = 524;
pub(crate) const BUILTIN_DISPOSABLESTACK_DISPOSED_GET: FunctionID = 525;
pub(crate) const BUILTIN_DISPOSABLESTACK_MOVE: FunctionID = 526;
// ── AsyncDisposableStack (530–539) ──────────────────────────────────
pub(crate) const BUILTIN_CTOR_ASYNCDISPOSABLESTACK: FunctionID = 530;
pub(crate) const BUILTIN_ASYNCDISPOSABLESTACK_DISPOSEASYNC: FunctionID = 531;
pub(crate) const BUILTIN_ASYNCDISPOSABLESTACK_USE: FunctionID = 532;
pub(crate) const BUILTIN_ASYNCDISPOSABLESTACK_ADOPT: FunctionID = 533;
pub(crate) const BUILTIN_ASYNCDISPOSABLESTACK_DEFER: FunctionID = 534;
pub(crate) const BUILTIN_ASYNCDISPOSABLESTACK_DISPOSED_GET: FunctionID = 535;
pub(crate) const BUILTIN_ASYNCDISPOSABLESTACK_MOVE: FunctionID = 536;
// ── Iterator dispose (540) ──────────────────────────────────────────
pub(crate) const BUILTIN_ITERATOR_PROTOTYPE_DISPOSE: FunctionID = 540;
// ── change-array-by-copy (550–553) ──────────────────────────────────
pub(crate) const BUILTIN_ARRAY_TOREVERSED: FunctionID = 550;
pub(crate) const BUILTIN_ARRAY_TOSORTED: FunctionID = 551;
pub(crate) const BUILTIN_ARRAY_TOSPLICED: FunctionID = 552;
pub(crate) const BUILTIN_ARRAY_WITH: FunctionID = 553;
// ── Map upsert (560–561) ────────────────────────────────────────────
pub(crate) const BUILTIN_MAP_GETORINSERT: FunctionID = 560;
pub(crate) const BUILTIN_MAP_GETORINSERTCOMPUTED: FunctionID = 561;
// ── ArrayBuffer detached / transfer (570–572) ───────────────────────
pub(crate) const BUILTIN_ARRAYBUFFER_DETACHED_GET: FunctionID = 570;
pub(crate) const BUILTIN_ARRAYBUFFER_TRANSFER: FunctionID = 571;
pub(crate) const BUILTIN_ARRAYBUFFER_TRANSFER_TO_FIXED: FunctionID = 572;
// ── WeakMap upsert (580–581) ─────────────────────────────────────────
pub(crate) const BUILTIN_WEAKMAP_GETORINSERT: FunctionID = 580;
pub(crate) const BUILTIN_WEAKMAP_GETORINSERTCOMPUTED: FunctionID = 581;
// ── TypedArray change-array-by-copy (590–593) ───────────────────────
pub(crate) const BUILTIN_TYPEDARRAY_TOREVERSED: FunctionID = 590;
pub(crate) const BUILTIN_TYPEDARRAY_TOSORTED: FunctionID = 591;
pub(crate) const BUILTIN_TYPEDARRAY_TOSPLICED: FunctionID = 592;
pub(crate) const BUILTIN_TYPEDARRAY_WITH: FunctionID = 593;
// ── Function.prototype[@@hasInstance] (600) ──────────────────────────
pub(crate) const BUILTIN_FN_HASINSTANCE: FunctionID = 600;
// Next available group: 610
