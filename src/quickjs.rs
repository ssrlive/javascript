#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use crate::error::JSError;
use crate::js_console;
use crate::js_math;
use crate::sprintf;
use crate::tmpfile;
use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;

/// Maximum number of loop iterations before triggering infinite loop detection
pub const MAX_LOOP_ITERATIONS: usize = 1000;

#[repr(C)]
#[derive(Copy, Clone)]
pub union JSValueUnion {
    pub int32: i32,
    pub float64: f64,
    pub ptr: *mut c_void,
    pub short_big_int: i64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct JSValue {
    pub u: JSValueUnion,
    pub tag: i64,
}

pub const JS_TAG_FIRST: i32 = -9;
pub const JS_TAG_BIG_INT: i32 = -9;
pub const JS_TAG_SYMBOL: i32 = -8;
pub const JS_TAG_STRING: i32 = -7;
pub const JS_TAG_STRING_ROPE: i32 = -6;
pub const JS_TAG_MODULE: i32 = -3;
pub const JS_TAG_FUNCTION_BYTECODE: i32 = -2;
pub const JS_TAG_OBJECT: i32 = -1;

pub const JS_TAG_INT: i32 = 0;
pub const JS_TAG_BOOL: i32 = 1;
pub const JS_TAG_NULL: i32 = 2;
pub const JS_TAG_UNDEFINED: i32 = 3;
pub const JS_TAG_UNINITIALIZED: i32 = 4;
pub const JS_TAG_CATCH_OFFSET: i32 = 5;
pub const JS_TAG_EXCEPTION: i32 = 6;
pub const JS_TAG_SHORT_BIG_INT: i32 = 7;
pub const JS_TAG_FLOAT64: i32 = 8;

pub const JS_FLOAT64_NAN: f64 = f64::NAN;

impl JSValue {
    pub fn new_int32(val: i32) -> JSValue {
        JSValue {
            u: JSValueUnion { int32: val },
            tag: JS_TAG_INT as i64,
        }
    }

    pub fn new_bool(val: bool) -> JSValue {
        JSValue {
            u: JSValueUnion {
                int32: if val { 1 } else { 0 },
            },
            tag: JS_TAG_BOOL as i64,
        }
    }

    pub fn new_float64(val: f64) -> JSValue {
        JSValue {
            u: JSValueUnion { float64: val },
            tag: JS_TAG_FLOAT64 as i64,
        }
    }

    pub fn new_ptr(tag: i32, ptr: *mut c_void) -> JSValue {
        JSValue {
            u: JSValueUnion { ptr },
            tag: tag as i64,
        }
    }

    pub fn has_ref_count(&self) -> bool {
        let t = self.tag as i32;
        (t >= JS_TAG_FIRST) && (t <= JS_TAG_OBJECT)
    }

    pub fn get_ptr(&self) -> *mut c_void {
        unsafe { self.u.ptr }
    }

    pub fn get_tag(&self) -> i32 {
        self.tag as i32
    }
}

pub const JS_NULL: JSValue = JSValue {
    u: JSValueUnion { int32: 0 },
    tag: JS_TAG_NULL as i64,
};

pub const JS_UNDEFINED: JSValue = JSValue {
    u: JSValueUnion { int32: 0 },
    tag: JS_TAG_UNDEFINED as i64,
};

pub const JS_FALSE: JSValue = JSValue {
    u: JSValueUnion { int32: 0 },
    tag: JS_TAG_BOOL as i64,
};

pub const JS_TRUE: JSValue = JSValue {
    u: JSValueUnion { int32: 1 },
    tag: JS_TAG_BOOL as i64,
};

pub const JS_EXCEPTION: JSValue = JSValue {
    u: JSValueUnion { int32: 0 },
    tag: JS_TAG_EXCEPTION as i64,
};

pub const JS_UNINITIALIZED: JSValue = JSValue {
    u: JSValueUnion { int32: 0 },
    tag: JS_TAG_UNINITIALIZED as i64,
};

#[repr(C)]
pub struct list_head {
    pub prev: *mut list_head,
    pub next: *mut list_head,
}

impl list_head {
    pub unsafe fn init(&mut self) {
        self.prev = self;
        self.next = self;
    }

    pub unsafe fn add_tail(&mut self, new_entry: *mut list_head) {
        let prev = self.prev;
        (*new_entry).next = self;
        (*new_entry).prev = prev;
        (*prev).next = new_entry;
        self.prev = new_entry;
    }

    pub unsafe fn del(&mut self) {
        let next = self.next;
        let prev = self.prev;
        (*next).prev = prev;
        (*prev).next = next;
        self.next = std::ptr::null_mut();
        self.prev = std::ptr::null_mut();
    }
}

#[repr(C)]
pub struct JSMallocState {
    pub malloc_count: usize,
    pub malloc_size: usize,
    pub malloc_limit: usize,
    pub opaque: *mut c_void,
}

#[repr(C)]
pub struct JSMallocFunctions {
    pub js_malloc: Option<unsafe extern "C" fn(*mut JSMallocState, usize) -> *mut c_void>,
    pub js_free: Option<unsafe extern "C" fn(*mut JSMallocState, *mut c_void)>,
    pub js_realloc: Option<unsafe extern "C" fn(*mut JSMallocState, *mut c_void, usize) -> *mut c_void>,
    pub js_malloc_usable_size: Option<unsafe extern "C" fn(*const c_void) -> usize>,
}

pub type JSAtom = u32;

#[repr(C)]
pub struct JSRefCountHeader {
    pub ref_count: i32,
}

#[repr(C)]
pub struct JSString {
    pub header: JSRefCountHeader,
    pub len: u32,  // len: 31, is_wide_char: 1 (packed manually)
    pub hash: u32, // hash: 30, atom_type: 2 (packed manually)
    pub hash_next: u32,
    // Variable length data follows
}

pub type JSAtomStruct = JSString;

#[repr(C)]
pub struct JSClass {
    pub class_id: u32,
    pub class_name: JSAtom,
    pub finalizer: *mut c_void, // JSClassFinalizer
    pub gc_mark: *mut c_void,   // JSClassGCMark
    pub call: *mut c_void,      // JSClassCall
    pub exotic: *mut c_void,    // JSClassExoticMethods
}

#[repr(C)]
pub struct JSRuntime {
    pub mf: JSMallocFunctions,
    pub malloc_state: JSMallocState,
    pub rt_info: *const i8,

    pub atom_hash_size: i32,
    pub atom_count: i32,
    pub atom_size: i32,
    pub atom_count_resize: i32,
    pub atom_hash: *mut u32,
    pub atom_array: *mut *mut JSAtomStruct,
    pub atom_free_index: i32,

    pub class_count: i32,
    pub class_array: *mut JSClass,

    pub context_list: list_head,
    pub gc_obj_list: list_head,
    pub gc_zero_ref_count_list: list_head,
    pub tmp_obj_list: list_head,
    pub gc_phase: u8,
    pub malloc_gc_threshold: usize,
    pub weakref_list: list_head,

    pub shape_hash_bits: i32,
    pub shape_hash_size: i32,
    pub shape_hash_count: i32,
    pub shape_hash: *mut *mut JSShape,
    pub user_opaque: *mut c_void,
}

#[repr(C)]
pub struct JSGCObjectHeader {
    pub ref_count: i32,
    pub gc_obj_type: u8, // 4 bits
    pub mark: u8,        // 1 bit
    pub dummy0: u8,      // 3 bits
    pub dummy1: u8,
    pub dummy2: u16,
    pub link: list_head,
}

#[repr(C)]
pub struct JSShape {
    pub header: JSGCObjectHeader,
    pub is_hashed: u8,
    pub has_small_array_index: u8,
    pub hash: u32,
    pub prop_hash_mask: u32,
    pub prop_size: i32,
    pub prop_count: i32,
    pub deleted_prop_count: i32,
    pub prop: *mut JSShapeProperty,
    pub prop_hash: *mut u32,
    pub proto: *mut JSObject,
}

#[repr(C)]
pub struct JSContext {
    pub header: JSGCObjectHeader,
    pub rt: *mut JSRuntime,
    pub link: list_head,

    pub binary_object_count: u16,
    pub binary_object_size: i32,
    pub std_array_prototype: u8,

    pub array_shape: *mut JSShape,
    pub arguments_shape: *mut JSShape,
    pub mapped_arguments_shape: *mut JSShape,
    pub regexp_shape: *mut JSShape,
    pub regexp_result_shape: *mut JSShape,

    pub class_proto: *mut JSValue,
    pub function_proto: JSValue,
    pub function_ctor: JSValue,
    pub array_ctor: JSValue,
    pub regexp_ctor: JSValue,
    pub promise_ctor: JSValue,
    pub native_error_proto: [JSValue; 8], // JS_NATIVE_ERROR_COUNT = 8 (usually)
    pub iterator_ctor: JSValue,
    pub async_iterator_proto: JSValue,
    pub array_proto_values: JSValue,
    pub throw_type_error: JSValue,
    pub eval_obj: JSValue,

    pub global_obj: JSValue,
    pub global_var_obj: JSValue,

    pub random_state: u64,
    pub interrupt_counter: i32,

    pub loaded_modules: list_head,

    pub compile_regexp: Option<unsafe extern "C" fn(*mut JSContext, JSValue, JSValue) -> JSValue>,
    pub eval_internal: Option<unsafe extern "C" fn(*mut JSContext, JSValue, *const i8, usize, *const i8, i32, i32) -> JSValue>,
    pub user_opaque: *mut c_void,
}

#[repr(C)]
pub struct JSFunctionBytecode {
    pub header: JSGCObjectHeader,
    pub js_mode: u8,
    pub flags: u16, // Packed bitfields
    pub byte_code_buf: *mut u8,
    pub byte_code_len: i32,
    pub func_name: JSAtom,
    pub vardefs: *mut c_void,     // JSBytecodeVarDef
    pub closure_var: *mut c_void, // JSClosureVar
    pub arg_count: u16,
    pub var_count: u16,
    pub defined_arg_count: u16,
    pub stack_size: u16,
    pub var_ref_count: u16,
    pub realm: *mut JSContext,
    pub cpool: *mut JSValue,
    pub cpool_count: i32,
    pub closure_var_count: i32,
    // debug info
    pub filename: JSAtom,
    pub source_len: i32,
    pub pc2line_len: i32,
    pub pc2line_buf: *mut u8,
    pub source: *mut i8,
}

#[repr(C)]
pub struct JSStackFrame {
    pub prev_frame: *mut JSStackFrame,
    pub cur_func: JSValue,
    pub arg_buf: *mut JSValue,
    pub var_buf: *mut JSValue,
    pub var_refs: *mut *mut c_void, // JSVarRef
    pub cur_pc: *const u8,
    pub arg_count: i32,
    pub js_mode: i32,
    pub cur_sp: *mut JSValue,
}

pub const JS_GC_OBJ_TYPE_JS_OBJECT: u8 = 1;
pub const JS_GC_OBJ_TYPE_FUNCTION_BYTECODE: u8 = 2;
pub const JS_GC_OBJ_TYPE_SHAPE: u8 = 3;
pub const JS_GC_OBJ_TYPE_VAR_REF: u8 = 4;
pub const JS_GC_OBJ_TYPE_ASYNC_FUNCTION: u8 = 5;
pub const JS_GC_OBJ_TYPE_JS_CONTEXT: u8 = 6;

#[repr(C)]
pub struct JSShapeProperty {
    pub hash_next: u32,
    pub flags: u8,
    pub atom: JSAtom,
}

#[repr(C)]
pub struct JSProperty {
    pub u: JSPropertyUnion,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union JSPropertyUnion {
    pub value: JSValue,
    pub next: *mut JSProperty, // simplified for now
}

#[repr(C)]
pub struct JSObject {
    pub header: JSGCObjectHeader,
    pub shape: *mut JSShape,
    pub prop: *mut JSProperty,
    pub first_weak_ref: *mut JSObject,
}

#[repr(C)]
pub struct JSClassDef {
    pub class_name: *const i8,
    pub finalizer: Option<unsafe extern "C" fn(*mut JSRuntime, JSValue)>,
    pub gc_mark: Option<unsafe extern "C" fn(*mut JSRuntime, JSValue, *mut c_void)>,
    pub call: Option<unsafe extern "C" fn(*mut JSContext, JSValue, JSValue, i32, *mut JSValue, i32) -> JSValue>,
    pub exotic: *mut c_void,
}

impl JSShape {
    pub unsafe fn find_own_property(&self, atom: JSAtom) -> Option<(i32, *mut JSShapeProperty)> {
        if self.is_hashed != 0 {
            let h = (atom as u32) & self.prop_hash_mask;
            let mut prop_idx = *self.prop_hash.offset(h as isize);
            while prop_idx != 0 {
                let idx = (prop_idx - 1) as i32;
                let pr = self.prop.offset(idx as isize);
                if (*pr).atom == atom {
                    return Some((idx, pr));
                }
                prop_idx = (*pr).hash_next;
            }
            None
        } else {
            for i in 0..self.prop_count {
                let pr = self.prop.offset(i as isize);
                if (*pr).atom == atom {
                    return Some((i, pr));
                }
            }
            None
        }
    }
}

impl JSRuntime {
    pub unsafe fn resize_shape(&mut self, sh: *mut JSShape, new_size: i32) -> i32 {
        let new_prop = self.js_realloc_rt(
            (*sh).prop as *mut c_void,
            new_size as usize * std::mem::size_of::<JSShapeProperty>(),
        ) as *mut JSShapeProperty;

        if new_prop.is_null() {
            return -1;
        }
        (*sh).prop = new_prop;
        (*sh).prop_size = new_size;
        0
    }

    pub unsafe fn add_property(&mut self, sh: *mut JSShape, atom: JSAtom, flags: u8) -> i32 {
        // Check if property already exists
        if let Some((idx, _)) = (*sh).find_own_property(atom) {
            // Already exists
            return idx;
        }

        if (*sh).prop_count >= (*sh).prop_size {
            let new_size = if (*sh).prop_size == 0 { 4 } else { (*sh).prop_size * 3 / 2 };
            if self.resize_shape(sh, new_size) < 0 {
                return -1;
            }
        }

        // Enable hash if needed
        if (*sh).prop_count >= 4 && (*sh).is_hashed == 0 {
            (*sh).is_hashed = 1;
            (*sh).prop_hash_mask = 15; // 16 - 1
            let hash_size = 16;
            (*sh).prop_hash = self.js_malloc_rt(hash_size * std::mem::size_of::<u32>()) as *mut u32;
            if (*sh).prop_hash.is_null() {
                return -1;
            }
            for i in 0..hash_size {
                *(*sh).prop_hash.offset(i as isize) = 0;
            }
            // Fill hash table with existing properties
            for i in 0..(*sh).prop_count {
                let pr = (*sh).prop.offset(i as isize);
                let h = ((*pr).atom as u32) & (*sh).prop_hash_mask;
                (*pr).hash_next = *(*sh).prop_hash.offset(h as isize);
                *(*sh).prop_hash.offset(h as isize) = (i + 1) as u32;
            }
        }

        let idx = (*sh).prop_count;
        let pr = (*sh).prop.offset(idx as isize);
        (*pr).atom = atom;
        (*pr).flags = flags;
        if (*sh).is_hashed != 0 {
            let h = (atom as u32) & (*sh).prop_hash_mask;
            (*pr).hash_next = *(*sh).prop_hash.offset(h as isize);
            *(*sh).prop_hash.offset(h as isize) = (idx + 1) as u32;
        } else {
            (*pr).hash_next = 0;
        }
        (*sh).prop_count += 1;

        idx
    }

    pub unsafe fn js_realloc_rt(&mut self, ptr: *mut c_void, size: usize) -> *mut c_void {
        if let Some(realloc_func) = self.mf.js_realloc {
            realloc_func(&mut self.malloc_state, ptr, size)
        } else {
            std::ptr::null_mut()
        }
    }

    pub unsafe fn js_malloc_rt(&mut self, size: usize) -> *mut c_void {
        if let Some(malloc_func) = self.mf.js_malloc {
            malloc_func(&mut self.malloc_state, size)
        } else {
            std::ptr::null_mut()
        }
    }

    pub unsafe fn js_free_rt(&mut self, ptr: *mut c_void) {
        if let Some(free_func) = self.mf.js_free {
            free_func(&mut self.malloc_state, ptr);
        }
    }

    pub unsafe fn init_atoms(&mut self) {
        self.atom_hash_size = 16;
        self.atom_count = 0;
        self.atom_size = 16;
        self.atom_count_resize = 8;
        self.atom_hash = self.js_malloc_rt((self.atom_hash_size as usize) * std::mem::size_of::<u32>()) as *mut u32;
        if self.atom_hash.is_null() {
            return;
        }
        for i in 0..self.atom_hash_size {
            *self.atom_hash.offset(i as isize) = 0;
        }
        self.atom_array = self.js_malloc_rt((self.atom_size as usize) * std::mem::size_of::<*mut JSAtomStruct>()) as *mut *mut JSAtomStruct;
        if self.atom_array.is_null() {
            self.js_free_rt(self.atom_hash as *mut c_void);
            self.atom_hash = std::ptr::null_mut();
            return;
        }
        for i in 0..self.atom_size {
            *self.atom_array.offset(i as isize) = std::ptr::null_mut();
        }
        self.atom_free_index = 0;
    }

    pub unsafe fn js_new_shape(&mut self, proto: *mut JSObject) -> *mut JSShape {
        let sh = self.js_malloc_rt(std::mem::size_of::<JSShape>()) as *mut JSShape;
        if sh.is_null() {
            return std::ptr::null_mut();
        }
        (*sh).header.ref_count = 1;
        (*sh).header.gc_obj_type = 0; // JS_GC_OBJ_TYPE_SHAPE
        (*sh).header.mark = 0;
        (*sh).header.dummy0 = 0;
        (*sh).header.dummy1 = 0;
        (*sh).header.dummy2 = 0;
        (*sh).header.link.init();
        (*sh).is_hashed = 0;
        (*sh).has_small_array_index = 0;
        (*sh).hash = 0;
        (*sh).prop_hash_mask = 0;
        (*sh).prop_size = 0;
        (*sh).prop_count = 0;
        (*sh).prop = std::ptr::null_mut();
        (*sh).prop_hash = std::ptr::null_mut();
        (*sh).proto = proto;
        sh
    }

    pub unsafe fn js_free_shape(&mut self, sh: *mut JSShape) {
        if !sh.is_null() {
            if !(*sh).prop.is_null() {
                self.js_free_rt((*sh).prop as *mut c_void);
            }
            if !(*sh).prop_hash.is_null() {
                self.js_free_rt((*sh).prop_hash as *mut c_void);
            }
            self.js_free_rt(sh as *mut c_void);
        }
    }
}

pub unsafe fn JS_DefinePropertyValue(ctx: *mut JSContext, this_obj: JSValue, prop: JSAtom, val: JSValue, flags: i32) -> i32 {
    if this_obj.tag != JS_TAG_OBJECT as i64 {
        return -1; // TypeError
    }
    let p = this_obj.u.ptr as *mut JSObject;
    let sh = (*p).shape;

    // Add property to shape
    // Note: In real QuickJS, we might need to clone shape if it is shared
    // For now, assume shape is unique to object or we modify it in place (dangerous if shared)

    let idx = (*(*ctx).rt).add_property(sh, prop, flags as u8);
    if idx < 0 {
        return -1;
    }

    // Resize object prop array if needed
    // JSObject prop array stores JSProperty (values)
    // JSShape prop array stores JSShapeProperty (names/flags)
    // They must match in size/index

    // TODO: Resize object prop array
    // For now, let's assume we have enough space or implement resize logic for object prop

    // Actually, we need to implement object prop resizing here
    // But JSObject definition: pub prop: *mut JSProperty
    // We don't store prop_size in JSObject?
    // QuickJS stores it in JSShape? No.
    // QuickJS: JSObject has no size field. It relies on Shape?
    // Ah, JSObject allocates prop array based on shape->prop_size?
    // Or maybe it reallocates when shape grows?

    // Let's look at QuickJS:
    // JS_DefinePropertyValue -> JS_DefineProperty -> add_property
    // add_property modifies shape.
    // If shape grows, we need to grow object's prop array too?
    // Yes, but how do we know the current size of object's prop array?
    // It seems we assume it matches shape's prop_count or prop_size?

    // Let's implement a simple resize for object prop
    let old_prop = (*p).prop;
    let new_prop = (*(*ctx).rt).js_realloc_rt(
        (*p).prop as *mut c_void,
        ((*sh).prop_size as usize) * std::mem::size_of::<JSProperty>(),
    ) as *mut JSProperty;

    if new_prop.is_null() {
        return -1;
    }
    (*p).prop = new_prop;
    // If the prop array was just created, zero-initialize it to avoid reading
    // uninitialized JSProperty values later.
    if old_prop.is_null() && !new_prop.is_null() {
        let size_bytes = ((*sh).prop_size as usize) * std::mem::size_of::<JSProperty>();
        std::ptr::write_bytes(new_prop as *mut u8, 0, size_bytes);
    }

    // Set value
    let pr = (*p).prop.offset(idx as isize);
    // If replacing an existing value, free it
    let old_val = (*pr).u.value;
    if old_val.has_ref_count() {
        JS_FreeValue((*ctx).rt, old_val);
    }
    // Duplicate incoming value if it's ref-counted
    if val.has_ref_count() {
        JS_DupValue((*ctx).rt, val);
    }
    (*pr).u.value = val;

    1
}

pub unsafe fn JS_NewRuntime() -> *mut JSRuntime {
    unsafe extern "C" fn my_malloc(_state: *mut JSMallocState, size: usize) -> *mut c_void {
        libc::malloc(size)
    }
    unsafe extern "C" fn my_free(_state: *mut JSMallocState, ptr: *mut c_void) {
        libc::free(ptr);
    }
    unsafe extern "C" fn my_realloc(_state: *mut JSMallocState, ptr: *mut c_void, size: usize) -> *mut c_void {
        libc::realloc(ptr, size)
    }

    let rt = libc::malloc(std::mem::size_of::<JSRuntime>()) as *mut JSRuntime;
    if rt.is_null() {
        return std::ptr::null_mut();
    }

    // Initialize malloc functions
    (*rt).mf.js_malloc = Some(my_malloc);
    (*rt).mf.js_free = Some(my_free);
    (*rt).mf.js_realloc = Some(my_realloc);
    (*rt).mf.js_malloc_usable_size = None;

    (*rt).malloc_state = JSMallocState {
        malloc_count: 0,
        malloc_size: 0,
        malloc_limit: 0,
        opaque: std::ptr::null_mut(),
    };

    (*rt).rt_info = std::ptr::null();

    // Initialize atoms
    (*rt).atom_hash_size = 0;
    (*rt).atom_count = 0;
    (*rt).atom_size = 0;
    (*rt).atom_count_resize = 0;
    (*rt).atom_hash = std::ptr::null_mut();
    (*rt).atom_array = std::ptr::null_mut();
    (*rt).atom_free_index = 0;

    (*rt).class_count = 0;
    (*rt).class_array = std::ptr::null_mut();

    (*rt).context_list.init();
    (*rt).gc_obj_list.init();
    (*rt).gc_zero_ref_count_list.init();
    (*rt).tmp_obj_list.init();
    (*rt).gc_phase = 0;
    (*rt).malloc_gc_threshold = 0;
    (*rt).weakref_list.init();

    (*rt).shape_hash_bits = 0;
    (*rt).shape_hash_size = 0;
    (*rt).shape_hash_count = 0;
    (*rt).shape_hash = std::ptr::null_mut();

    (*rt).user_opaque = std::ptr::null_mut();

    (*rt).init_atoms();

    rt
}

pub unsafe fn JS_FreeRuntime(rt: *mut JSRuntime) {
    if !rt.is_null() {
        // Free allocated resources
        // For now, just free the rt
        libc::free(rt as *mut c_void);
    }
}

pub unsafe fn JS_NewContext(rt: *mut JSRuntime) -> *mut JSContext {
    let ctx = (*rt).js_malloc_rt(std::mem::size_of::<JSContext>()) as *mut JSContext;
    if ctx.is_null() {
        return std::ptr::null_mut();
    }
    (*ctx).header.ref_count = 1;
    (*ctx).header.gc_obj_type = 0;
    (*ctx).header.mark = 0;
    (*ctx).header.dummy0 = 0;
    (*ctx).header.dummy1 = 0;
    (*ctx).header.dummy2 = 0;
    (*ctx).header.link.init();
    (*ctx).rt = rt;
    (*ctx).link.init();
    // Initialize other fields to zero/null
    (*ctx).binary_object_count = 0;
    (*ctx).binary_object_size = 0;
    (*ctx).std_array_prototype = 0;
    (*ctx).array_shape = std::ptr::null_mut();
    (*ctx).arguments_shape = std::ptr::null_mut();
    (*ctx).mapped_arguments_shape = std::ptr::null_mut();
    (*ctx).regexp_shape = std::ptr::null_mut();
    (*ctx).regexp_result_shape = std::ptr::null_mut();
    (*ctx).class_proto = std::ptr::null_mut();
    (*ctx).function_proto = JS_NULL;
    (*ctx).function_ctor = JS_NULL;
    (*ctx).array_ctor = JS_NULL;
    (*ctx).regexp_ctor = JS_NULL;
    (*ctx).promise_ctor = JS_NULL;
    for i in 0..8 {
        (*ctx).native_error_proto[i] = JS_NULL;
    }
    (*ctx).iterator_ctor = JS_NULL;
    (*ctx).async_iterator_proto = JS_NULL;
    (*ctx).array_proto_values = JS_NULL;
    (*ctx).throw_type_error = JS_NULL;
    (*ctx).eval_obj = JS_NULL;
    (*ctx).global_obj = JS_NULL;
    (*ctx).global_var_obj = JS_NULL;
    (*ctx).random_state = 0;
    (*ctx).interrupt_counter = 0;
    (*ctx).loaded_modules.init();
    (*ctx).compile_regexp = None;
    (*ctx).eval_internal = None;
    (*ctx).user_opaque = std::ptr::null_mut();
    ctx
}

pub unsafe fn JS_FreeContext(ctx: *mut JSContext) {
    if !ctx.is_null() {
        (*(*ctx).rt).js_free_rt(ctx as *mut c_void);
    }
}

pub unsafe fn JS_NewObject(ctx: *mut JSContext) -> JSValue {
    let obj = (*(*ctx).rt).js_malloc_rt(std::mem::size_of::<JSObject>()) as *mut JSObject;
    if obj.is_null() {
        return JS_EXCEPTION;
    }
    (*obj).header.ref_count = 1;
    (*obj).header.gc_obj_type = 0;
    (*obj).header.mark = 0;
    (*obj).header.dummy0 = 0;
    (*obj).header.dummy1 = 0;
    (*obj).header.dummy2 = 0;
    (*obj).header.link.init();
    (*obj).shape = (*(*ctx).rt).js_new_shape(std::ptr::null_mut());
    if (*obj).shape.is_null() {
        (*(*ctx).rt).js_free_rt(obj as *mut c_void);
        return JS_EXCEPTION;
    }
    (*obj).prop = std::ptr::null_mut();
    (*obj).first_weak_ref = std::ptr::null_mut();
    JSValue::new_ptr(JS_TAG_OBJECT, obj as *mut c_void)
}

pub unsafe fn JS_NewString(ctx: *mut JSContext, s: &[u16]) -> JSValue {
    let utf8_str = utf16_to_utf8(s);
    let len = utf8_str.len();
    if len == 0 {
        // Empty string
        return JSValue::new_ptr(JS_TAG_STRING, std::ptr::null_mut());
    }
    let str_size = std::mem::size_of::<JSString>() + len;
    let p = (*(*ctx).rt).js_malloc_rt(str_size) as *mut JSString;
    if p.is_null() {
        return JS_EXCEPTION;
    }
    (*p).header.ref_count = 1;
    (*p).len = len as u32;
    (*p).hash = 0; // TODO: compute hash
    (*p).hash_next = 0;
    // Copy string data
    let str_data = (p as *mut u8).offset(std::mem::size_of::<JSString>() as isize);
    for (i, &byte) in utf8_str.as_bytes().iter().enumerate() {
        *str_data.offset(i as isize) = byte;
    }
    JSValue::new_ptr(JS_TAG_STRING, p as *mut c_void)
}

pub unsafe fn JS_Eval(_ctx: *mut JSContext, input: *const i8, input_len: usize, _filename: *const i8, _eval_flags: i32) -> JSValue {
    if input_len == 0 {
        return JS_UNDEFINED;
    }
    let s = std::slice::from_raw_parts(input as *const u8, input_len);
    let script = std::str::from_utf8(s).unwrap_or("");

    // Evaluate statements
    match evaluate_script(script.trim()) {
        Ok(Value::Number(num)) => JSValue::new_float64(num),
        Ok(Value::String(s)) => JS_NewString(_ctx, &s),
        Ok(Value::Boolean(b)) => {
            if b {
                JS_TRUE
            } else {
                JS_FALSE
            }
        }
        Ok(Value::Undefined) => JS_UNDEFINED,
        Ok(Value::Object(_)) => JS_UNDEFINED,        // For now
        Ok(Value::Function(_)) => JS_UNDEFINED,      // For now
        Ok(Value::Closure(_, _, _)) => JS_UNDEFINED, // For now
        Err(_) => JS_UNDEFINED,
    }
}

pub fn evaluate_script(script: &str) -> Result<Value, JSError> {
    // Remove simple import lines that we've already handled via shim injection
    let mut filtered = String::new();
    for line in script.lines() {
        let l = line.trim();
        if l.starts_with("import * as") && l.contains("from") {
            // skip this import line (already injected into env)
            continue;
        }
        filtered.push_str(line);
        filtered.push('\n');
    }

    let mut tokens = tokenize(&filtered)?;
    let statements = parse_statements(&mut tokens)?;
    let mut env: JSObjectData = JSObjectData::new();

    // Inject simple host `std` / `os` shims when importing with the pattern:
    //   import * as NAME from "std";
    for line in script.lines() {
        let l = line.trim();
        if l.starts_with("import * as") && l.contains("from") {
            if let Some(as_idx) = l.find("as") {
                if let Some(from_idx) = l.find("from") {
                    let name_part = &l[as_idx + 2..from_idx].trim();
                    let name = name_part.trim();
                    if let Some(start_quote) = l[from_idx..].find(|c: char| c == '"' || c == '\'') {
                        let quote_char = l[from_idx + start_quote..].chars().next().unwrap();
                        let rest = &l[from_idx + start_quote + 1..];
                        if let Some(end_quote) = rest.find(quote_char) {
                            let module = &rest[..end_quote];
                            if module == "std" {
                                env.insert(
                                    name.to_string(),
                                    Rc::new(RefCell::new(Value::Object(crate::js_std::make_std_object()))),
                                );
                            } else if module == "os" {
                                env.insert(
                                    name.to_string(),
                                    Rc::new(RefCell::new(Value::Object(crate::js_os::make_os_object()))),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    evaluate_statements(&mut env, &statements)
}

pub fn parse_statements(tokens: &mut Vec<Token>) -> Result<Vec<Statement>, JSError> {
    let mut statements = Vec::new();
    while !tokens.is_empty() && !matches!(tokens[0], Token::RBrace) {
        let stmt = parse_statement(tokens)?;
        statements.push(stmt);
        if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon) {
            tokens.remove(0);
        }
    }
    Ok(statements)
}

fn parse_statement(tokens: &mut Vec<Token>) -> Result<Statement, JSError> {
    if tokens.len() >= 1 && matches!(tokens[0], Token::Function) {
        tokens.remove(0); // consume function
        if let Some(Token::Identifier(name)) = tokens.get(0).cloned() {
            tokens.remove(0);
            if tokens.len() >= 1 && matches!(tokens[0], Token::LParen) {
                tokens.remove(0); // consume (
                let mut params = Vec::new();
                if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
                    loop {
                        if let Some(Token::Identifier(param)) = tokens.get(0).cloned() {
                            tokens.remove(0);
                            params.push(param);
                            if tokens.is_empty() {
                                return Err(JSError::ParseError);
                            }
                            if matches!(tokens[0], Token::RParen) {
                                break;
                            }
                            if !matches!(tokens[0], Token::Comma) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume ,
                        } else {
                            return Err(JSError::ParseError);
                        }
                    }
                }
                if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume )
                if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume {
                let body = parse_statements(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume }
                return Ok(Statement::Let(name, Expr::Function(params, body)));
            }
        }
    }
    if tokens.len() >= 1 && matches!(tokens[0], Token::If) {
        tokens.remove(0); // consume if
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume (
        let condition = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume )
        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume {
        let then_body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume }

        let else_body = if !tokens.is_empty() && matches!(tokens[0], Token::Else) {
            tokens.remove(0); // consume else
            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume {
            let body = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume }
            Some(body)
        } else {
            None
        };

        return Ok(Statement::If(condition, then_body, else_body));
    }
    if tokens.len() >= 1 && matches!(tokens[0], Token::For) {
        tokens.remove(0); // consume for
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume (

        // Parse initialization
        let init = if tokens.len() >= 1 && (matches!(tokens[0], Token::Let) || matches!(tokens[0], Token::Var)) {
            Some(Box::new(parse_statement(tokens)?))
        } else if !matches!(tokens[0], Token::Semicolon) {
            Some(Box::new(Statement::Expr(parse_expression(tokens)?)))
        } else {
            None
        };

        if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume first ;

        // Parse condition
        let condition = if !matches!(tokens[0], Token::Semicolon) {
            Some(parse_expression(tokens)?)
        } else {
            None
        };

        if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume second ;

        // Parse increment
        let increment = if !matches!(tokens[0], Token::RParen) {
            Some(Box::new(Statement::Expr(parse_expression(tokens)?)))
        } else {
            None
        };

        if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume )

        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume {

        let body = parse_statements(tokens)?;

        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume }

        return Ok(Statement::For(init, condition, increment, body));
    }
    if tokens.len() >= 1 && matches!(tokens[0], Token::Return) {
        tokens.remove(0); // consume return
        if tokens.is_empty() || matches!(tokens[0], Token::Semicolon) {
            return Ok(Statement::Return(None));
        }
        let expr = parse_expression(tokens)?;
        return Ok(Statement::Return(Some(expr)));
    }
    if tokens.len() >= 1 && (matches!(tokens[0], Token::Let) || matches!(tokens[0], Token::Var)) {
        tokens.remove(0); // consume let/var
        if let Some(Token::Identifier(name)) = tokens.get(0).cloned() {
            tokens.remove(0);
            if tokens.len() >= 1 && matches!(tokens[0], Token::Assign) {
                tokens.remove(0);
                let expr = parse_expression(tokens)?;
                return Ok(Statement::Let(name, expr));
            }
        }
    }
    let expr = parse_expression(tokens)?;
    // Check if this is an assignment expression
    if let Expr::Assign(target, value) = &expr {
        if let Expr::Var(name) = target.as_ref() {
            return Ok(Statement::Assign(name.clone(), *value.clone()));
        }
    }
    Ok(Statement::Expr(expr))
}

pub fn evaluate_statements(env: &mut JSObjectData, statements: &[Statement]) -> Result<Value, JSError> {
    let mut last_value = Value::Number(0.0);
    for stmt in statements {
        match stmt {
            Statement::Let(name, expr) => {
                let val = evaluate_expr(env, expr)?;
                env_set(env, name.as_str(), val.clone());
                last_value = val;
            }
            Statement::Assign(name, expr) => {
                let val = evaluate_expr(env, expr)?;
                env_set(env, name.as_str(), val.clone());
                last_value = val;
            }
            Statement::Expr(expr) => {
                last_value = evaluate_expr(env, expr)?;
            }
            Statement::Return(expr_opt) => {
                return match expr_opt {
                    Some(expr) => evaluate_expr(env, expr),
                    None => Ok(Value::Undefined),
                };
            }
            Statement::If(condition, then_body, else_body) => {
                let cond_val = evaluate_expr(env, condition)?;
                if is_truthy(&cond_val) {
                    last_value = evaluate_statements(env, then_body)?;
                } else if let Some(else_stmts) = else_body {
                    last_value = evaluate_statements(env, else_stmts)?;
                }
            }
            Statement::For(init, condition, increment, body) => {
                // Execute initialization
                if let Some(init_stmt) = init {
                    match init_stmt.as_ref() {
                        Statement::Let(name, expr) => {
                            let val = evaluate_expr(env, expr)?;
                            env_set(env, name.as_str(), val);
                        }
                        Statement::Expr(expr) => {
                            evaluate_expr(env, expr)?;
                        }
                        _ => {
                            return Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        } // For now, only support let and expr in init
                    }
                }

                // For now, limit to MAX_LOOP_ITERATIONS iterations to prevent infinite loops
                let mut iterations = 0;
                loop {
                    if iterations >= MAX_LOOP_ITERATIONS {
                        return Err(JSError::InfiniteLoopError {
                            iterations: MAX_LOOP_ITERATIONS,
                        });
                    }

                    // Check condition
                    let should_continue = if let Some(cond_expr) = condition {
                        let cond_val = evaluate_expr(env, cond_expr)?;
                        is_truthy(&cond_val)
                    } else {
                        true // No condition means infinite loop
                    };

                    if !should_continue {
                        break;
                    }

                    // Execute body
                    let result = evaluate_statements(env, body);
                    match result {
                        Ok(val) => last_value = val,
                        Err(err) => return Err(err),
                    }

                    // Execute increment
                    if let Some(incr_stmt) = increment {
                        match incr_stmt.as_ref() {
                            Statement::Expr(expr) => match expr {
                                Expr::Assign(target, value) => {
                                    if let Expr::Var(name) = target.as_ref() {
                                        let val = evaluate_expr(env, value)?;
                                        env_set(env, name.as_str(), val);
                                    }
                                }
                                _ => {
                                    evaluate_expr(env, expr)?;
                                }
                            },
                            _ => {
                                return Err(JSError::EvaluationError {
                                    message: "error".to_string(),
                                })
                            } // For now, only support expr in increment
                        }
                    }

                    iterations += 1;
                }
            }
        }
    }
    Ok(last_value)
}

pub fn evaluate_expr(env: &JSObjectData, expr: &Expr) -> Result<Value, JSError> {
    match expr {
        Expr::Number(n) => evaluate_number(*n),
        Expr::StringLit(s) => evaluate_string_lit(s),
        Expr::Boolean(b) => evaluate_boolean(*b),
        Expr::Var(name) => evaluate_var(env, name),
        Expr::Assign(_target, value) => evaluate_assign(env, value),
        Expr::UnaryNeg(expr) => evaluate_unary_neg(env, expr),
        Expr::Binary(left, op, right) => evaluate_binary(env, left, op, right),
        Expr::Index(obj, idx) => evaluate_index(env, obj, idx),
        Expr::Property(obj, prop) => evaluate_property(env, obj, prop),
        Expr::Call(func_expr, args) => evaluate_call(env, func_expr, args),
        Expr::Function(params, body) => Ok(Value::Closure(params.clone(), body.clone(), env.clone())),
        Expr::Object(properties) => evaluate_object(env, properties),
        Expr::Array(elements) => evaluate_array(env, elements),
    }
}

fn evaluate_number(n: f64) -> Result<Value, JSError> {
    Ok(Value::Number(n))
}

fn evaluate_string_lit(s: &Vec<u16>) -> Result<Value, JSError> {
    Ok(Value::String(s.clone()))
}

fn evaluate_boolean(b: bool) -> Result<Value, JSError> {
    Ok(Value::Boolean(b))
}

fn evaluate_var(env: &JSObjectData, name: &str) -> Result<Value, JSError> {
    if let Some(val) = env_get(env, name) {
        Ok(val.borrow().clone())
    } else if name == "console" {
        Ok(Value::Object(js_console::make_console_object()))
    } else if name == "String" {
        Ok(Value::Function("String".to_string()))
    } else if name == "Math" {
        Ok(Value::Object(js_math::make_math_object()))
    } else if name == "JSON" {
        let mut json_obj = JSObjectData::new();
        obj_set_val(&mut json_obj, "parse", Value::Function("JSON.parse".to_string()));
        obj_set_val(&mut json_obj, "stringify", Value::Function("JSON.stringify".to_string()));
        Ok(Value::Object(json_obj))
    } else if name == "Object" {
        let mut object_obj = JSObjectData::new();
        obj_set_val(&mut object_obj, "keys", Value::Function("Object.keys".to_string()));
        obj_set_val(&mut object_obj, "values", Value::Function("Object.values".to_string()));
        Ok(Value::Object(object_obj))
    } else if name == "parseInt" {
        Ok(Value::Function("parseInt".to_string()))
    } else if name == "parseFloat" {
        Ok(Value::Function("parseFloat".to_string()))
    } else if name == "isNaN" {
        Ok(Value::Function("isNaN".to_string()))
    } else if name == "isFinite" {
        Ok(Value::Function("isFinite".to_string()))
    } else if name == "encodeURIComponent" {
        Ok(Value::Function("encodeURIComponent".to_string()))
    } else if name == "decodeURIComponent" {
        Ok(Value::Function("decodeURIComponent".to_string()))
    } else if name == "eval" {
        Ok(Value::Function("eval".to_string()))
    } else if name == "encodeURI" {
        Ok(Value::Function("encodeURI".to_string()))
    } else if name == "decodeURI" {
        Ok(Value::Function("decodeURI".to_string()))
    } else if name == "Array" {
        Ok(Value::Function("Array".to_string()))
    } else if name == "Number" {
        Ok(Value::Function("Number".to_string()))
    } else if name == "Boolean" {
        Ok(Value::Function("Boolean".to_string()))
    } else if name == "Date" {
        Ok(Value::Function("Date".to_string()))
    } else if name == "NaN" {
        Ok(Value::Number(f64::NAN))
    } else {
        Ok(Value::Undefined)
    }
}

fn evaluate_assign(env: &JSObjectData, value: &Expr) -> Result<Value, JSError> {
    // Assignment is handled at statement level, just evaluate the value
    evaluate_expr(env, value)
}

fn evaluate_unary_neg(env: &JSObjectData, expr: &Expr) -> Result<Value, JSError> {
    let val = evaluate_expr(env, expr)?;
    match val {
        Value::Number(n) => Ok(Value::Number(-n)),
        _ => Err(JSError::EvaluationError {
            message: "error".to_string(),
        }),
    }
}

fn evaluate_binary(env: &JSObjectData, left: &Expr, op: &BinaryOp, right: &Expr) -> Result<Value, JSError> {
    let l = evaluate_expr(env, left)?;
    let r = evaluate_expr(env, right)?;
    match op {
        BinaryOp::Add => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln + rn)),
            (Value::String(ls), Value::String(rs)) => {
                let mut result = ls.clone();
                result.extend_from_slice(&rs);
                Ok(Value::String(result))
            }
            (Value::Number(ln), Value::String(rs)) => {
                let mut result = utf8_to_utf16(&ln.to_string());
                result.extend_from_slice(&rs);
                Ok(Value::String(result))
            }
            (Value::String(ls), Value::Number(rn)) => {
                let mut result = ls.clone();
                result.extend_from_slice(&utf8_to_utf16(&rn.to_string()));
                Ok(Value::String(result))
            }
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        },
        BinaryOp::Sub => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln - rn)),
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        },
        BinaryOp::Mul => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln * rn)),
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        },
        BinaryOp::Div => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => {
                if rn == 0.0 {
                    Err(JSError::EvaluationError {
                        message: "error".to_string(),
                    })
                } else {
                    Ok(Value::Number(ln / rn))
                }
            }
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        },
        BinaryOp::Equal => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(if ln == rn { 1.0 } else { 0.0 })),
            (Value::String(ls), Value::String(rs)) => Ok(Value::Number(if ls == rs { 1.0 } else { 0.0 })),
            _ => Ok(Value::Number(0.0)), // Different types are not equal
        },
        BinaryOp::StrictEqual => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(if ln == rn { 1.0 } else { 0.0 })),
            (Value::String(ls), Value::String(rs)) => Ok(Value::Number(if ls == rs { 1.0 } else { 0.0 })),
            _ => Ok(Value::Number(0.0)), // Different types are not equal
        },
        BinaryOp::LessThan => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(if ln < rn { 1.0 } else { 0.0 })),
            (Value::String(ls), Value::String(rs)) => Ok(Value::Number(if ls < rs { 1.0 } else { 0.0 })),
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        },
        BinaryOp::GreaterThan => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(if ln > rn { 1.0 } else { 0.0 })),
            (Value::String(ls), Value::String(rs)) => Ok(Value::Number(if ls > rs { 1.0 } else { 0.0 })),
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        },
        BinaryOp::LessEqual => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(if ln <= rn { 1.0 } else { 0.0 })),
            (Value::String(ls), Value::String(rs)) => Ok(Value::Number(if ls <= rs { 1.0 } else { 0.0 })),
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        },
        BinaryOp::GreaterEqual => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(if ln >= rn { 1.0 } else { 0.0 })),
            (Value::String(ls), Value::String(rs)) => Ok(Value::Number(if ls >= rs { 1.0 } else { 0.0 })),
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        },
        BinaryOp::Mod => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => {
                if rn == 0.0 {
                    Err(JSError::EvaluationError {
                        message: "Division by zero".to_string(),
                    })
                } else {
                    Ok(Value::Number(ln % rn))
                }
            }
            _ => Err(JSError::EvaluationError {
                message: "Modulo operation only supported for numbers".to_string(),
            }),
        },
    }
}

fn evaluate_index(env: &JSObjectData, obj: &Expr, idx: &Expr) -> Result<Value, JSError> {
    let obj_val = evaluate_expr(env, obj)?;
    let idx_val = evaluate_expr(env, idx)?;
    match (obj_val, idx_val) {
        (Value::String(s), Value::Number(n)) => {
            let idx = n as usize;
            if let Some(ch) = utf16_char_at(&s, idx) {
                Ok(Value::String(vec![ch]))
            } else {
                Ok(Value::String(Vec::new())) // or return undefined, but use empty string here
            }
        }
        (Value::Object(obj_map), Value::Number(n)) => {
            // Array-like indexing
            let key = n.to_string();
            if let Some(val) = obj_get(&obj_map, &key) {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Object(obj_map), Value::String(s)) => {
            // Object property access with string key
            let key = String::from_utf16_lossy(&s);
            if let Some(val) = obj_get(&obj_map, &key) {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        _ => Err(JSError::EvaluationError {
            message: "error".to_string(),
        }), // other types of indexing not supported yet
    }
}

fn evaluate_property(env: &JSObjectData, obj: &Expr, prop: &str) -> Result<Value, JSError> {
    let obj_val = evaluate_expr(env, obj)?;
    println!("Property: obj_val={:?}, prop={}", obj_val, prop);
    match obj_val {
        Value::String(s) if prop == "length" => Ok(Value::Number(utf16_len(&s) as f64)),
        Value::Object(obj_map) => {
            if let Some(val) = obj_get(&obj_map, prop) {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        _ => {
            println!("Property not found");
            Err(JSError::EvaluationError {
                message: "error".to_string(),
            })
        }
    }
}

fn evaluate_call(env: &JSObjectData, func_expr: &Expr, args: &[Expr]) -> Result<Value, JSError> {
    // Check if it's a method call first
    if let Expr::Property(obj_expr, method_name) = func_expr {
        // Special case for Array static methods
        if let Expr::Var(var_name) = &**obj_expr {
            if var_name == "Array" {
                return crate::js_array::handle_array_static_method(method_name, args, env);
            }
        }

        let obj_val = evaluate_expr(env, &**obj_expr)?;
        match (obj_val, method_name.as_str()) {
            (Value::Object(obj_map), "log") if obj_map.contains_key("log") => {
                return js_console::handle_console_method(method_name, args, env);
            }
            (obj_val, "toString") => {
                // toString method for all values
                if args.is_empty() {
                    match obj_val {
                        Value::Number(n) => Ok(Value::String(utf8_to_utf16(&n.to_string()))),
                        Value::String(s) => Ok(Value::String(s.clone())),
                        Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
                        Value::Undefined => Ok(Value::String(utf8_to_utf16("undefined"))),
                        Value::Object(ref obj_map) => {
                            // If this object looks like an array (has a length), join elements with comma
                            if obj_map.contains_key("length") {
                                let length = obj_get(&obj_map, "length")
                                    .map(|v| v.borrow().clone())
                                    .unwrap_or(Value::Number(0.0));
                                let current_len = match length {
                                    Value::Number(n) => n as usize,
                                    _ => 0,
                                };
                                let mut parts = Vec::new();
                                for i in 0..current_len {
                                    if let Some(val_rc) = obj_get(&obj_map, &i.to_string()) {
                                        match &*val_rc.borrow() {
                                            Value::String(s) => parts.push(String::from_utf16_lossy(s)),
                                            Value::Number(n) => parts.push(n.to_string()),
                                            Value::Boolean(b) => parts.push(b.to_string()),
                                            _ => parts.push("[object Object]".to_string()),
                                        }
                                    } else {
                                        parts.push("".to_string())
                                    }
                                }
                                Ok(Value::String(utf8_to_utf16(&parts.join(","))))
                            } else {
                                Ok(Value::String(utf8_to_utf16("[object Object]")))
                            }
                        }
                        Value::Function(name) => Ok(Value::String(utf8_to_utf16(&format!("[Function: {}]", name)))),
                        Value::Closure(_, _, _) => Ok(Value::String(utf8_to_utf16("[Function]"))),
                    }
                } else {
                    Err(JSError::EvaluationError {
                        message: "error".to_string(),
                    })
                }
            }
            (Value::Object(mut obj_map), method) => {
                // If this object looks like the `std` module (we used 'sprintf' as marker)
                if obj_map.contains_key("sprintf") {
                    match method {
                        "sprintf" => {
                            return sprintf::handle_sprintf_call(env, args);
                        }
                        "tmpfile" => {
                            return tmpfile::create_tmpfile();
                        }
                        _ => {}
                    }
                }

                // If this object looks like the `os` module (we used 'open' as marker)
                if obj_map.contains_key("open") {
                    return crate::js_os::handle_os_method(&obj_map, method, args, env);
                }

                // If this object looks like the `os.path` module
                if obj_map.contains_key("join") {
                    return crate::js_os::handle_os_method(&obj_map, method, args, env);
                }

                // If this object is a file-like object (we use '__file_id' as marker)
                if obj_map.contains_key("__file_id") {
                    return tmpfile::handle_file_method(&obj_map, method, args, env);
                }
                // Check if this is the Math object
                if obj_map.contains_key("PI") && obj_map.contains_key("E") {
                    return js_math::handle_math_method(method, args, env);
                } else if obj_map.contains_key("parse") && obj_map.contains_key("stringify") {
                    // JSON methods
                    match method {
                        "parse" => {
                            if args.len() == 1 {
                                let arg_val = evaluate_expr(env, &args[0])?;
                                match arg_val {
                                    Value::String(s) => {
                                        // Simple JSON parsing - for now just return the string as-is
                                        // In a real implementation, this would parse JSON
                                        Ok(Value::String(s))
                                    }
                                    _ => Err(JSError::EvaluationError {
                                        message: "JSON.parse expects a string".to_string(),
                                    }),
                                }
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "JSON.parse expects exactly one argument".to_string(),
                                })
                            }
                        }
                        "stringify" => {
                            if args.len() == 1 {
                                let arg_val = evaluate_expr(env, &args[0])?;
                                match arg_val {
                                    Value::Number(n) => Ok(Value::String(utf8_to_utf16(&n.to_string()))),
                                    Value::String(s) => {
                                        // Simple JSON stringification - just return the string
                                        Ok(Value::String(s))
                                    }
                                    Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
                                    Value::Undefined => Ok(Value::String(utf8_to_utf16("null"))),
                                    Value::Object(_) => Ok(Value::String(utf8_to_utf16("{}"))), // Simple object representation
                                    _ => Ok(Value::String(utf8_to_utf16("null"))),
                                }
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "JSON.stringify expects exactly one argument".to_string(),
                                })
                            }
                        }
                        _ => Err(JSError::EvaluationError {
                            message: format!("JSON.{} is not implemented", method),
                        }),
                    }
                } else if obj_map.contains_key("keys") && obj_map.contains_key("values") {
                    // Object methods
                    match method {
                        "keys" => {
                            if args.len() == 1 {
                                let obj_val = evaluate_expr(env, &args[0])?;
                                if let Value::Object(obj) = obj_val {
                                    let mut keys = Vec::new();
                                    for key in obj.keys() {
                                        if key != "length" {
                                            // Skip array length property
                                            keys.push(Value::String(utf8_to_utf16(key)));
                                        }
                                    }
                                    // Create a simple array-like object for keys
                                    let mut result_obj = JSObjectData::new();
                                    for (i, key) in keys.into_iter().enumerate() {
                                        obj_set_val(&mut result_obj, &i.to_string(), key);
                                    }
                                    let len = Value::Number(result_obj.len() as f64);
                                    obj_set_val(&mut result_obj, "length", len);
                                    Ok(Value::Object(result_obj))
                                } else {
                                    Err(JSError::EvaluationError {
                                        message: "Object.keys expects an object".to_string(),
                                    })
                                }
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "Object.keys expects exactly one argument".to_string(),
                                })
                            }
                        }
                        "values" => {
                            if args.len() == 1 {
                                let obj_val = evaluate_expr(env, &args[0])?;
                                if let Value::Object(obj) = obj_val {
                                    let mut values = Vec::new();
                                    for (key, value) in obj.iter() {
                                        if key != "length" {
                                            // Skip array length property
                                            values.push(value.clone());
                                        }
                                    }
                                    // Create a simple array-like object for values
                                    let mut result_obj = JSObjectData::new();
                                    for (i, value) in values.into_iter().enumerate() {
                                        obj_set_val(&mut result_obj, &i.to_string(), value.borrow().clone());
                                    }
                                    let len = Value::Number(result_obj.len() as f64);
                                    obj_set_val(&mut result_obj, "length", len);
                                    Ok(Value::Object(result_obj))
                                } else {
                                    Err(JSError::EvaluationError {
                                        message: "Object.values expects an object".to_string(),
                                    })
                                }
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "Object.values expects exactly one argument".to_string(),
                                })
                            }
                        }
                        _ => Err(JSError::EvaluationError {
                            message: format!("Object.{} is not implemented", method),
                        }),
                    }
                } else if obj_map.contains_key("length") {
                    // Array instance methods
                    return crate::js_array::handle_array_instance_method(&mut obj_map, method, args, env, &**obj_expr);
                } else {
                    // Other object methods not implemented
                    return Err(JSError::EvaluationError {
                        message: format!("Method {} not implemented for this object type", method),
                    });
                }
            }
            (Value::String(s), method) => {
                // String method call
                match method {
                    "toString" => {
                        if args.is_empty() {
                            Ok(Value::String(s.clone()))
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "substring" => {
                        if args.len() == 2 {
                            let start_val = evaluate_expr(env, &args[0])?;
                            let end_val = evaluate_expr(env, &args[1])?;
                            if let (Value::Number(start), Value::Number(end)) = (start_val, end_val) {
                                let start_idx = start as usize;
                                let end_idx = end as usize;
                                if start_idx <= end_idx && end_idx <= utf16_len(&s) {
                                    Ok(Value::String(utf16_slice(&s, start_idx, end_idx)))
                                } else {
                                    Err(JSError::EvaluationError {
                                        message: "error".to_string(),
                                    })
                                }
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "error".to_string(),
                                })
                            }
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "slice" => {
                        let start = if args.len() >= 1 {
                            match evaluate_expr(env, &args[0])? {
                                Value::Number(n) => n as isize,
                                _ => 0isize,
                            }
                        } else {
                            0isize
                        };
                        let end = if args.len() >= 2 {
                            match evaluate_expr(env, &args[1])? {
                                Value::Number(n) => n as isize,
                                _ => s.len() as isize,
                            }
                        } else {
                            s.len() as isize
                        };

                        let len = utf16_len(&s) as isize;
                        let start = if start < 0 { len + start } else { start };
                        let end = if end < 0 { len + end } else { end };

                        let start = start.max(0).min(len) as usize;
                        let end = end.max(0).min(len) as usize;

                        if start <= end {
                            Ok(Value::String(utf16_slice(&s, start, end)))
                        } else {
                            Ok(Value::String(Vec::new()))
                        }
                    }
                    "toUpperCase" => {
                        if args.is_empty() {
                            Ok(Value::String(utf16_to_uppercase(&s)))
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "toLowerCase" => {
                        if args.is_empty() {
                            Ok(Value::String(utf16_to_lowercase(&s)))
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "indexOf" => {
                        if args.len() == 1 {
                            let search_val = evaluate_expr(env, &args[0])?;
                            if let Value::String(search) = search_val {
                                if let Some(pos) = utf16_find(&s, &search) {
                                    Ok(Value::Number(pos as f64))
                                } else {
                                    Ok(Value::Number(-1.0))
                                }
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "error".to_string(),
                                })
                            }
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "lastIndexOf" => {
                        if args.len() == 1 {
                            let search_val = evaluate_expr(env, &args[0])?;
                            if let Value::String(search) = search_val {
                                if let Some(pos) = utf16_rfind(&s, &search) {
                                    Ok(Value::Number(pos as f64))
                                } else {
                                    Ok(Value::Number(-1.0))
                                }
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "error".to_string(),
                                })
                            }
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "replace" => {
                        if args.len() == 2 {
                            let search_val = evaluate_expr(env, &args[0])?;
                            let replace_val = evaluate_expr(env, &args[1])?;
                            if let (Value::String(search), Value::String(replace)) = (search_val, replace_val) {
                                Ok(Value::String(utf16_replace(&s, &search, &replace)))
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "error".to_string(),
                                })
                            }
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "split" => {
                        if args.len() == 1 {
                            let sep_val = evaluate_expr(env, &args[0])?;
                            if let Value::String(sep) = sep_val {
                                // Implement split returning an array-like object
                                let mut parts: Vec<Vec<u16>> = Vec::new();
                                if sep.is_empty() {
                                    // split by empty separator => each UTF-16 code unit as string
                                    for i in 0..utf16_len(&s) {
                                        if let Some(ch) = utf16_char_at(&s, i) {
                                            parts.push(vec![ch]);
                                        }
                                    }
                                } else {
                                    let mut start = 0usize;
                                    while start <= utf16_len(&s) {
                                        if let Some(pos) = utf16_find(&s[start..].to_vec(), &sep) {
                                            let end = start + pos;
                                            parts.push(utf16_slice(&s, start, end));
                                            start = end + utf16_len(&sep);
                                        } else {
                                            // remainder
                                            parts.push(utf16_slice(&s, start, utf16_len(&s)));
                                            break;
                                        }
                                    }
                                }
                                let mut arr = JSObjectData::new();
                                for (i, part) in parts.into_iter().enumerate() {
                                    arr.insert(i.to_string(), Rc::new(RefCell::new(Value::String(part))));
                                }
                                arr.insert("length".to_string(), Rc::new(RefCell::new(Value::Number(arr.len() as f64))));
                                Ok(Value::Object(arr))
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "error".to_string(),
                                })
                            }
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "charAt" => {
                        if args.len() == 1 {
                            let idx_val = evaluate_expr(env, &args[0])?;
                            if let Value::Number(n) = idx_val {
                                let idx = n as isize;
                                // let len = utf16_len(&s) as isize;
                                let idx = if idx < 0 { 0 } else { idx } as usize;
                                if idx < utf16_len(&s) {
                                    if let Some(ch) = utf16_char_at(&s, idx) {
                                        Ok(Value::String(vec![ch]))
                                    } else {
                                        Ok(Value::String(Vec::new()))
                                    }
                                } else {
                                    Ok(Value::String(Vec::new()))
                                }
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "error".to_string(),
                                })
                            }
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "trim" => {
                        if args.is_empty() {
                            let str_val = String::from_utf16_lossy(&s);
                            let trimmed = str_val.trim();
                            Ok(Value::String(utf8_to_utf16(trimmed)))
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "startsWith" => {
                        if args.len() == 1 {
                            let search_val = evaluate_expr(env, &args[0])?;
                            if let Value::String(search) = search_val {
                                let starts = s.len() >= search.len() && s[..search.len()] == search[..];
                                Ok(Value::Boolean(starts))
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "error".to_string(),
                                })
                            }
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "endsWith" => {
                        if args.len() == 1 {
                            let search_val = evaluate_expr(env, &args[0])?;
                            if let Value::String(search) = search_val {
                                let ends = s.len() >= search.len() && s[s.len() - search.len()..] == search[..];
                                Ok(Value::Boolean(ends))
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "error".to_string(),
                                })
                            }
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "includes" => {
                        if args.len() == 1 {
                            let search_val = evaluate_expr(env, &args[0])?;
                            if let Value::String(search) = search_val {
                                let includes = utf16_find(&s, &search).is_some();
                                Ok(Value::Boolean(includes))
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "error".to_string(),
                                })
                            }
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "repeat" => {
                        if args.len() == 1 {
                            let count_val = evaluate_expr(env, &args[0])?;
                            if let Value::Number(n) = count_val {
                                let count = n as usize;
                                let mut repeated = Vec::new();
                                for _ in 0..count {
                                    repeated.extend_from_slice(&s);
                                }
                                Ok(Value::String(repeated))
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "error".to_string(),
                                })
                            }
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "concat" => {
                        let mut result = s.clone();
                        for arg in args {
                            let arg_val = evaluate_expr(env, arg)?;
                            if let Value::String(arg_str) = arg_val {
                                result.extend(arg_str);
                            } else {
                                // Convert to string
                                let str_val = match arg_val {
                                    Value::Number(n) => utf8_to_utf16(&n.to_string()),
                                    Value::Boolean(b) => utf8_to_utf16(&b.to_string()),
                                    Value::Undefined => utf8_to_utf16("undefined"),
                                    _ => utf8_to_utf16("[object Object]"),
                                };
                                result.extend(str_val);
                            }
                        }
                        Ok(Value::String(result))
                    }
                    "padStart" => {
                        if args.len() >= 1 {
                            let target_len_val = evaluate_expr(env, &args[0])?;
                            if let Value::Number(target_len) = target_len_val {
                                let target_len = target_len as usize;
                                let current_len = utf16_len(&s);
                                if current_len >= target_len {
                                    Ok(Value::String(s.clone()))
                                } else {
                                    let pad_char = if args.len() >= 2 {
                                        let pad_val = evaluate_expr(env, &args[1])?;
                                        if let Value::String(pad_str) = pad_val {
                                            if !pad_str.is_empty() {
                                                pad_str[0]
                                            } else {
                                                ' ' as u16
                                            }
                                        } else {
                                            ' ' as u16
                                        }
                                    } else {
                                        ' ' as u16
                                    };
                                    let pad_count = target_len - current_len;
                                    let mut padded = vec![pad_char; pad_count];
                                    padded.extend_from_slice(&s);
                                    Ok(Value::String(padded))
                                }
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "error".to_string(),
                                })
                            }
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    "padEnd" => {
                        if args.len() >= 1 {
                            let target_len_val = evaluate_expr(env, &args[0])?;
                            if let Value::Number(target_len) = target_len_val {
                                let target_len = target_len as usize;
                                let current_len = utf16_len(&s);
                                if current_len >= target_len {
                                    Ok(Value::String(s.clone()))
                                } else {
                                    let pad_char = if args.len() >= 2 {
                                        let pad_val = evaluate_expr(env, &args[1])?;
                                        if let Value::String(pad_str) = pad_val {
                                            if !pad_str.is_empty() {
                                                pad_str[0]
                                            } else {
                                                ' ' as u16
                                            }
                                        } else {
                                            ' ' as u16
                                        }
                                    } else {
                                        ' ' as u16
                                    };
                                    let pad_count = target_len - current_len;
                                    let mut padded = s.clone();
                                    padded.extend(vec![pad_char; pad_count]);
                                    Ok(Value::String(padded))
                                }
                            } else {
                                Err(JSError::EvaluationError {
                                    message: "error".to_string(),
                                })
                            }
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
                        }
                    }
                    _ => Err(JSError::EvaluationError {
                        message: "error".to_string(),
                    }), // method not found
                }
            }
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        }
    } else {
        // Regular function call
        let func_val = evaluate_expr(env, func_expr)?;
        match func_val {
            Value::Function(func_name) => match func_name.as_str() {
                "String" => {
                    // String() constructor
                    if args.len() == 1 {
                        let arg_val = evaluate_expr(env, &args[0])?;
                        match arg_val {
                            Value::Number(n) => Ok(Value::String(utf8_to_utf16(&n.to_string()))),
                            Value::String(s) => Ok(Value::String(s.clone())),
                            Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
                            Value::Undefined => Ok(Value::String(utf8_to_utf16("undefined"))),
                            Value::Object(_) => Ok(Value::String(utf8_to_utf16("[object Object]"))),
                            Value::Function(name) => Ok(Value::String(utf8_to_utf16(&format!("[Function: {}]", name)))),
                            Value::Closure(_, _, _) => Ok(Value::String(utf8_to_utf16("[Function]"))),
                        }
                    } else {
                        Ok(Value::String(Vec::new())) // String() with no args returns empty string
                    }
                }

                "parseInt" => {
                    if args.len() >= 1 {
                        let arg_val = evaluate_expr(env, &args[0])?;
                        match arg_val {
                            Value::String(s) => {
                                let str_val = String::from_utf16_lossy(&s);
                                // Parse integer from the beginning of the string
                                let trimmed = str_val.trim();
                                let mut end_pos = 0;
                                let mut chars = trimmed.chars();
                                if let Some(first_char) = chars.next() {
                                    if first_char == '-' || first_char == '+' || first_char.is_digit(10) {
                                        end_pos = 1;
                                        for ch in chars {
                                            if ch.is_digit(10) {
                                                end_pos += 1;
                                            } else {
                                                break;
                                            }
                                        }
                                    }
                                }
                                let num_str = &trimmed[0..end_pos];
                                match num_str.parse::<i32>() {
                                    Ok(n) => Ok(Value::Number(n as f64)),
                                    Err(_) => Ok(Value::Number(f64::NAN)),
                                }
                            }
                            Value::Number(n) => Ok(Value::Number(n.trunc())),
                            _ => Ok(Value::Number(f64::NAN)),
                        }
                    } else {
                        Ok(Value::Number(f64::NAN))
                    }
                }
                "parseFloat" => {
                    if args.len() >= 1 {
                        let arg_val = evaluate_expr(env, &args[0])?;
                        match arg_val {
                            Value::String(s) => {
                                let str_val = String::from_utf16_lossy(&s);
                                match str_val.trim().parse::<f64>() {
                                    Ok(n) => Ok(Value::Number(n)),
                                    Err(_) => Ok(Value::Number(f64::NAN)),
                                }
                            }
                            Value::Number(n) => Ok(Value::Number(n)),
                            _ => Ok(Value::Number(f64::NAN)),
                        }
                    } else {
                        Ok(Value::Number(f64::NAN))
                    }
                }
                "isNaN" => {
                    if args.len() >= 1 {
                        let arg_val = evaluate_expr(env, &args[0])?;
                        match arg_val {
                            Value::Number(n) => Ok(Value::Boolean(n.is_nan())),
                            _ => Ok(Value::Boolean(false)),
                        }
                    } else {
                        Ok(Value::Boolean(false))
                    }
                }
                "isFinite" => {
                    if args.len() >= 1 {
                        let arg_val = evaluate_expr(env, &args[0])?;
                        match arg_val {
                            Value::Number(n) => Ok(Value::Boolean(n.is_finite())),
                            _ => Ok(Value::Boolean(false)),
                        }
                    } else {
                        Ok(Value::Boolean(false))
                    }
                }
                "encodeURIComponent" => {
                    if args.len() >= 1 {
                        let arg_val = evaluate_expr(env, &args[0])?;
                        match arg_val {
                            Value::String(s) => {
                                let str_val = String::from_utf16_lossy(&s);
                                // Simple URI encoding - replace spaces with %20 and some special chars
                                let encoded = str_val
                                    .replace("%", "%25")
                                    .replace(" ", "%20")
                                    .replace("\"", "%22")
                                    .replace("'", "%27")
                                    .replace("<", "%3C")
                                    .replace(">", "%3E")
                                    .replace("&", "%26");
                                Ok(Value::String(utf8_to_utf16(&encoded)))
                            }
                            _ => {
                                // For non-string values, convert to string first
                                let str_val = match arg_val {
                                    Value::Number(n) => n.to_string(),
                                    Value::Boolean(b) => b.to_string(),
                                    _ => "[object Object]".to_string(),
                                };
                                Ok(Value::String(utf8_to_utf16(&str_val)))
                            }
                        }
                    } else {
                        Ok(Value::String(Vec::new()))
                    }
                }
                "decodeURIComponent" => {
                    if args.len() >= 1 {
                        let arg_val = evaluate_expr(env, &args[0])?;
                        match arg_val {
                            Value::String(s) => {
                                let str_val = String::from_utf16_lossy(&s);
                                // Simple URI decoding - replace %20 with spaces and some special chars
                                let decoded = str_val
                                    .replace("%20", " ")
                                    .replace("%22", "\"")
                                    .replace("%27", "'")
                                    .replace("%3C", "<")
                                    .replace("%3E", ">")
                                    .replace("%26", "&")
                                    .replace("%25", "%");
                                Ok(Value::String(utf8_to_utf16(&decoded)))
                            }
                            _ => {
                                // For non-string values, convert to string first
                                let str_val = match arg_val {
                                    Value::Number(n) => n.to_string(),
                                    Value::Boolean(b) => b.to_string(),
                                    _ => "[object Object]".to_string(),
                                };
                                Ok(Value::String(utf8_to_utf16(&str_val)))
                            }
                        }
                    } else {
                        Ok(Value::String(Vec::new()))
                    }
                }
                "Array" => {
                    return crate::js_array::handle_array_constructor(args, env);
                }
                "Number" => {
                    // Number constructor
                    if args.len() == 1 {
                        let arg_val = evaluate_expr(env, &args[0])?;
                        match arg_val {
                            Value::Number(n) => Ok(Value::Number(n)),
                            Value::String(s) => {
                                let str_val = String::from_utf16_lossy(&s);
                                match str_val.trim().parse::<f64>() {
                                    Ok(n) => Ok(Value::Number(n)),
                                    Err(_) => Ok(Value::Number(f64::NAN)),
                                }
                            }
                            Value::Boolean(b) => Ok(Value::Number(if b { 1.0 } else { 0.0 })),
                            _ => Ok(Value::Number(f64::NAN)),
                        }
                    } else {
                        Ok(Value::Number(0.0)) // Number() with no args returns 0
                    }
                }
                "Boolean" => {
                    // Boolean constructor
                    if args.len() == 1 {
                        let arg_val = evaluate_expr(env, &args[0])?;
                        let bool_val = match arg_val {
                            Value::Boolean(b) => b,
                            Value::Number(n) => n != 0.0 && !n.is_nan(),
                            Value::String(s) => !s.is_empty(),
                            Value::Object(_) => true,
                            Value::Undefined => false,
                            _ => false,
                        };
                        Ok(Value::Boolean(bool_val))
                    } else {
                        Ok(Value::Boolean(false)) // Boolean() with no args returns false
                    }
                }
                "Date" => {
                    // Date constructor - for now just return current timestamp
                    use std::time::{SystemTime, UNIX_EPOCH};
                    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                    let timestamp = duration.as_millis() as f64;
                    Ok(Value::String(utf8_to_utf16(&format!("Date: {}", timestamp))))
                }
                "eval" => {
                    // eval function - basic implementation
                    if args.len() >= 1 {
                        let arg_val = evaluate_expr(env, &args[0])?;
                        match arg_val {
                            Value::String(s) => {
                                // For now, just return the string as-is
                                // In a real implementation, this would parse and execute the code
                                Ok(Value::String(s))
                            }
                            _ => Ok(arg_val),
                        }
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                "encodeURI" => {
                    if args.len() >= 1 {
                        let arg_val = evaluate_expr(env, &args[0])?;
                        match arg_val {
                            Value::String(s) => {
                                let str_val = String::from_utf16_lossy(&s);
                                // Simple URI encoding - replace spaces with %20
                                let encoded = str_val.replace(" ", "%20");
                                Ok(Value::String(utf8_to_utf16(&encoded)))
                            }
                            _ => {
                                let str_val = match arg_val {
                                    Value::Number(n) => n.to_string(),
                                    Value::Boolean(b) => b.to_string(),
                                    _ => "[object Object]".to_string(),
                                };
                                Ok(Value::String(utf8_to_utf16(&str_val)))
                            }
                        }
                    } else {
                        Ok(Value::String(Vec::new()))
                    }
                }
                "decodeURI" => {
                    if args.len() >= 1 {
                        let arg_val = evaluate_expr(env, &args[0])?;
                        match arg_val {
                            Value::String(s) => {
                                let str_val = String::from_utf16_lossy(&s);
                                // Simple URI decoding - replace %20 with spaces
                                let decoded = str_val.replace("%20", " ");
                                Ok(Value::String(utf8_to_utf16(&decoded)))
                            }
                            _ => {
                                let str_val = match arg_val {
                                    Value::Number(n) => n.to_string(),
                                    Value::Boolean(b) => b.to_string(),
                                    _ => "[object Object]".to_string(),
                                };
                                Ok(Value::String(utf8_to_utf16(&str_val)))
                            }
                        }
                    } else {
                        Ok(Value::String(Vec::new()))
                    }
                }
                _ => Err(JSError::EvaluationError {
                    message: "error".to_string(),
                }),
            },
            Value::Closure(params, body, captured_env) => {
                // Function call
                if params.len() != args.len() {
                    return Err(JSError::ParseError);
                }
                // Create new environment starting with captured environment
                let mut func_env = captured_env.clone();
                // Add parameters
                for (param, arg) in params.iter().zip(args.iter()) {
                    let arg_val = evaluate_expr(env, arg)?;
                    env_set(&mut func_env, param.as_str(), arg_val);
                }
                // Execute function body
                evaluate_statements(&mut func_env, &body)
            }
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        }
    }
}

fn evaluate_object(env: &JSObjectData, properties: &Vec<(String, Expr)>) -> Result<Value, JSError> {
    let mut obj = JSObjectData::new();
    for (key, value_expr) in properties {
        let value = evaluate_expr(env, value_expr)?;
        obj_set_val(&mut obj, key.as_str(), value);
    }
    Ok(Value::Object(obj))
}

fn evaluate_array(env: &JSObjectData, elements: &Vec<Expr>) -> Result<Value, JSError> {
    let mut arr = JSObjectData::new();
    for (i, elem_expr) in elements.iter().enumerate() {
        let value = evaluate_expr(env, elem_expr)?;
        obj_set_val(&mut arr, &i.to_string(), value);
    }
    // Set length property
    obj_set_val(&mut arr, "length", Value::Number(elements.len() as f64));
    Ok(Value::Object(arr))
}

pub type JSObjectData = std::collections::HashMap<String, std::rc::Rc<std::cell::RefCell<Value>>>;

#[derive(Debug, Clone)]
pub enum Value {
    Number(f64),
    String(Vec<u16>), // UTF-16 code units
    Boolean(bool),
    Undefined,
    Object(JSObjectData),                               // Object with properties
    Function(String),                                   // Function name
    Closure(Vec<String>, Vec<Statement>, JSObjectData), // parameters, body, captured environment
}

// Helper functions for UTF-16 string operations
pub fn utf8_to_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn utf16_to_utf8(v: &[u16]) -> String {
    String::from_utf16_lossy(v)
}

fn utf16_len(v: &[u16]) -> usize {
    v.len()
}

fn utf16_slice(v: &[u16], start: usize, end: usize) -> Vec<u16> {
    if start >= v.len() {
        Vec::new()
    } else {
        let end = end.min(v.len());
        v[start..end].to_vec()
    }
}

fn utf16_char_at(v: &[u16], index: usize) -> Option<u16> {
    v.get(index).copied()
}

fn utf16_to_uppercase(v: &[u16]) -> Vec<u16> {
    let s = utf16_to_utf8(v);
    utf8_to_utf16(&s.to_uppercase())
}

fn utf16_to_lowercase(v: &[u16]) -> Vec<u16> {
    let s = utf16_to_utf8(v);
    utf8_to_utf16(&s.to_lowercase())
}

fn utf16_find(v: &[u16], pattern: &[u16]) -> Option<usize> {
    if pattern.is_empty() {
        return Some(0);
    }
    for i in 0..=v.len().saturating_sub(pattern.len()) {
        if v[i..i + pattern.len()] == *pattern {
            return Some(i);
        }
    }
    None
}

fn utf16_rfind(v: &[u16], pattern: &[u16]) -> Option<usize> {
    if pattern.is_empty() {
        return Some(v.len());
    }
    for i in (0..=v.len().saturating_sub(pattern.len())).rev() {
        if v[i..i + pattern.len()] == *pattern {
            return Some(i);
        }
    }
    None
}

fn utf16_replace(v: &[u16], search: &[u16], replace: &[u16]) -> Vec<u16> {
    if let Some(pos) = utf16_find(v, search) {
        let mut result = v[..pos].to_vec();
        result.extend_from_slice(replace);
        result.extend_from_slice(&v[pos + search.len()..]);
        result
    } else {
        v.to_vec()
    }
}

// Helper function to compare two values for equality
pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(na), Value::Number(nb)) => na == nb,
        (Value::String(sa), Value::String(sb)) => sa == sb,
        (Value::Boolean(ba), Value::Boolean(bb)) => ba == bb,
        (Value::Undefined, Value::Undefined) => true,
        (Value::Object(_), Value::Object(_)) => false, // Objects are not equal unless same reference
        _ => false,                                    // Different types are not equal
    }
}

// Helper function to convert value to string for sorting
pub fn value_to_sort_string(val: &Value) -> String {
    match val {
        Value::Number(n) => {
            if n.is_nan() {
                "NaN".to_string()
            } else if *n == f64::INFINITY {
                "Infinity".to_string()
            } else if *n == f64::NEG_INFINITY {
                "-Infinity".to_string()
            } else {
                n.to_string()
            }
        }
        Value::String(s) => String::from_utf16_lossy(s),
        Value::Boolean(b) => b.to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Object(_) => "[object Object]".to_string(),
        Value::Function(name) => format!("[function {}]", name),
        Value::Closure(_, _, _) => "[function]".to_string(),
    }
}

// Helper accessors for objects and environments
pub fn obj_get(map: &JSObjectData, key: &str) -> Option<Rc<RefCell<Value>>> {
    map.get(key).cloned()
}

pub fn obj_set_val(map: &mut JSObjectData, key: &str, val: Value) {
    map.insert(key.to_string(), Rc::new(RefCell::new(val)));
}

pub fn obj_set_rc(map: &mut JSObjectData, key: &str, val_rc: Rc<RefCell<Value>>) {
    map.insert(key.to_string(), val_rc);
}

pub fn env_get(env: &JSObjectData, key: &str) -> Option<Rc<RefCell<Value>>> {
    env.get(key).cloned()
}

pub fn env_set(env: &mut JSObjectData, key: &str, val: Value) {
    env.insert(key.to_string(), Rc::new(RefCell::new(val)));
}

pub fn env_set_rc(env: &mut JSObjectData, key: &str, val_rc: Rc<RefCell<Value>>) {
    env.insert(key.to_string(), val_rc);
}

// Higher-level property API that operates on expressions + environment.
// `get_prop_env` evaluates `obj_expr` in `env` and returns the property's Rc if present.
pub fn get_prop_env(env: &JSObjectData, obj_expr: &Expr, prop: &str) -> Result<Option<Rc<RefCell<Value>>>, JSError> {
    let obj_val = evaluate_expr(env, obj_expr)?;
    match obj_val {
        Value::Object(map) => Ok(obj_get(&map, prop)),
        _ => Ok(None),
    }
}

// `set_prop_env` attempts to set a property on the object referenced by `obj_expr`.
// Behavior:
// - If `obj_expr` is a variable name (Expr::Var) and that variable exists in `env`
//   and is an object, it mutates the stored object in-place and returns `Ok(None)`.
// - Otherwise it evaluates `obj_expr`, and if it yields an object, it inserts the
//   property into that object's map and returns `Ok(Some(Value::Object(map)))` so
//   the caller can decide what to do with the updated object value.
pub fn set_prop_env(env: &mut JSObjectData, obj_expr: &Expr, prop: &str, val: Value) -> Result<Option<Value>, JSError> {
    // Fast path: obj_expr is a variable that we can mutate in-place in env
    if let Expr::Var(varname) = obj_expr {
        if let Some(rc_val) = env_get(&*env, varname) {
            let mut borrowed = rc_val.borrow_mut();
            if let Value::Object(ref mut map) = *borrowed {
                map.insert(prop.to_string(), Rc::new(RefCell::new(val)));
                return Ok(None);
            }
        }
    }

    // Fall back: evaluate the object expression and return an updated object value
    let obj_val = evaluate_expr(&*env, obj_expr)?;
    match obj_val {
        Value::Object(mut map) => {
            obj_set_val(&mut map, prop, val);
            Ok(Some(Value::Object(map)))
        }
        _ => Err(JSError::EvaluationError {
            message: "not an object".to_string(),
        }),
    }
}

#[derive(Debug, Clone)]
pub enum Statement {
    Let(String, Expr),
    Assign(String, Expr), // variable assignment
    Expr(Expr),
    Return(Option<Expr>),
    If(Expr, Vec<Statement>, Option<Vec<Statement>>), // condition, then_body, else_body
    For(Option<Box<Statement>>, Option<Expr>, Option<Box<Statement>>, Vec<Statement>), // init, condition, increment, body
}

#[derive(Debug, Clone)]
pub enum Expr {
    Number(f64),
    StringLit(Vec<u16>),
    Boolean(bool),
    Var(String),
    Binary(Box<Expr>, BinaryOp, Box<Expr>),
    UnaryNeg(Box<Expr>),
    Assign(Box<Expr>, Box<Expr>), // target, value
    Index(Box<Expr>, Box<Expr>),
    Property(Box<Expr>, String),
    Call(Box<Expr>, Vec<Expr>),
    Function(Vec<String>, Vec<Statement>), // parameters, body
    Object(Vec<(String, Expr)>),           // object literal: key-value pairs
    Array(Vec<Expr>),                      // array literal: [elem1, elem2, ...]
}

#[derive(Debug, Clone)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Equal,
    StrictEqual,
    LessThan,
    GreaterThan,
    LessEqual,
    GreaterEqual,
}

fn parse_string_literal(chars: &[char], start: &mut usize, end_char: char) -> Result<Vec<u16>, JSError> {
    let mut result = Vec::new();
    while *start < chars.len() && chars[*start] != end_char {
        if chars[*start] == '\\' {
            *start += 1;
            if *start >= chars.len() {
                return Err(JSError::TokenizationError);
            }
            match chars[*start] {
                'n' => result.push('\n' as u16),
                't' => result.push('\t' as u16),
                'r' => result.push('\r' as u16),
                '\\' => result.push('\\' as u16),
                '"' => result.push('"' as u16),
                '\'' => result.push('\'' as u16),
                '`' => result.push('`' as u16),
                'u' => {
                    // Unicode escape sequence \uXXXX
                    *start += 1;
                    if *start + 4 > chars.len() {
                        return Err(JSError::TokenizationError);
                    }
                    let hex_str: String = chars[*start..*start + 4].iter().collect();
                    *start += 3; // will be incremented by 1 at the end
                    match u16::from_str_radix(&hex_str, 16) {
                        Ok(code) => {
                            result.push(code);
                        }
                        Err(_) => return Err(JSError::TokenizationError), // Invalid hex
                    }
                }
                _ => return Err(JSError::TokenizationError), // Invalid escape sequence
            }
        } else {
            result.push(chars[*start] as u16);
        }
        *start += 1;
    }
    if *start >= chars.len() {
        return Err(JSError::TokenizationError);
    }
    Ok(result)
}

pub fn tokenize(expr: &str) -> Result<Vec<Token>, JSError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\n' => i += 1,
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(Token::Multiply);
                i += 1;
            }
            '/' => {
                tokens.push(Token::Divide);
                i += 1;
            }
            '%' => {
                tokens.push(Token::Mod);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '[' => {
                tokens.push(Token::LBracket);
                i += 1;
            }
            ']' => {
                tokens.push(Token::RBracket);
                i += 1;
            }
            '{' => {
                tokens.push(Token::LBrace);
                i += 1;
            }
            '}' => {
                tokens.push(Token::RBrace);
                i += 1;
            }
            ':' => {
                tokens.push(Token::Colon);
                i += 1;
            }
            '.' => {
                tokens.push(Token::Dot);
                i += 1;
            }
            '=' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    if i + 2 < chars.len() && chars[i + 2] == '=' {
                        tokens.push(Token::StrictEqual);
                        i += 3;
                    } else {
                        tokens.push(Token::Equal);
                        i += 2;
                    }
                } else {
                    tokens.push(Token::Assign);
                    i += 1;
                }
            }
            '<' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::LessEqual);
                    i += 2;
                } else {
                    tokens.push(Token::LessThan);
                    i += 1;
                }
            }
            '>' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::GreaterEqual);
                    i += 2;
                } else {
                    tokens.push(Token::GreaterThan);
                    i += 1;
                }
            }
            '0'..='9' => {
                let start = i;
                while i < chars.len() && (chars[i].is_digit(10) || chars[i] == '.') {
                    i += 1;
                }
                let num_str: String = chars[start..i].iter().collect();
                let num = num_str.parse::<f64>().map_err(|_| JSError::TokenizationError)?;
                tokens.push(Token::Number(num));
            }
            '"' => {
                i += 1; // skip opening quote
                let mut start = i;
                let str_lit = parse_string_literal(&chars, &mut start, '"')?;
                tokens.push(Token::StringLit(str_lit));
                i = start + 1; // skip closing quote
            }
            '\'' => {
                i += 1; // skip opening quote
                let mut start = i;
                let str_lit = parse_string_literal(&chars, &mut start, '\'')?;
                tokens.push(Token::StringLit(str_lit));
                i = start + 1; // skip closing quote
            }
            '`' => {
                i += 1; // skip opening backtick
                let mut parts = Vec::new();
                let mut current_start = i;
                while i < chars.len() && chars[i] != '`' {
                    if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1] == '{' {
                        // Found ${, add string part before it
                        if current_start < i {
                            let mut start_idx = current_start;
                            let str_part = parse_string_literal(&chars, &mut start_idx, '$')?;
                            parts.push(TemplatePart::String(str_part));
                            i = start_idx; // Update i to after the parsed string
                        }
                        i += 2; // skip ${
                        let expr_start = i;
                        let mut brace_count = 1;
                        while i < chars.len() && brace_count > 0 {
                            if chars[i] == '{' {
                                brace_count += 1;
                            } else if chars[i] == '}' {
                                brace_count -= 1;
                            }
                            i += 1;
                        }
                        if brace_count != 0 {
                            return Err(JSError::TokenizationError);
                        }
                        let expr_str: String = chars[expr_start..i - 1].iter().collect();
                        // Tokenize the expression inside ${}
                        let expr_tokens = tokenize(&expr_str)?;
                        parts.push(TemplatePart::Expr(expr_tokens));
                        current_start = i;
                    } else {
                        i += 1;
                    }
                }
                if i >= chars.len() {
                    return Err(JSError::TokenizationError);
                }
                // Add remaining string part
                if current_start < i {
                    let mut start_idx = current_start;
                    let str_part = parse_string_literal(&chars, &mut start_idx, '`')?;
                    parts.push(TemplatePart::String(str_part));
                }
                tokens.push(Token::TemplateString(parts));
                i += 1; // skip closing backtick
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                match ident.as_str() {
                    "let" => tokens.push(Token::Let),
                    "var" => tokens.push(Token::Var),
                    "function" => tokens.push(Token::Function),
                    "return" => tokens.push(Token::Return),
                    "if" => tokens.push(Token::If),
                    "else" => tokens.push(Token::Else),
                    "for" => tokens.push(Token::For),
                    "true" => tokens.push(Token::True),
                    "false" => tokens.push(Token::False),
                    _ => tokens.push(Token::Identifier(ident)),
                }
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            ';' => {
                tokens.push(Token::Semicolon);
                i += 1;
            }
            _ => return Err(JSError::TokenizationError),
        }
    }
    Ok(tokens)
}

#[derive(Debug, Clone)]
pub enum TemplatePart {
    String(Vec<u16>),
    Expr(Vec<Token>),
}

#[derive(Debug, Clone)]
pub enum Token {
    Number(f64),
    StringLit(Vec<u16>),
    TemplateString(Vec<TemplatePart>),
    Identifier(String),
    Plus,
    Minus,
    Multiply,
    Divide,
    Mod,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Colon,
    Dot,
    Comma,
    Let,
    Var,
    Function,
    Return,
    If,
    Else,
    For,
    Assign,
    Semicolon,
    Equal,
    StrictEqual,
    LessThan,
    GreaterThan,
    LessEqual,
    GreaterEqual,
    True,
    False,
}

fn is_truthy(val: &Value) -> bool {
    match val {
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Boolean(b) => *b,
        Value::Undefined => false,
        Value::Object(_) => true,
        Value::Function(_) => true,
        Value::Closure(_, _, _) => true,
    }
}

fn parse_expression(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    parse_assignment(tokens)
}

fn parse_assignment(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let left = parse_comparison(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    if matches!(tokens[0], Token::Assign) {
        tokens.remove(0);
        let right = parse_assignment(tokens)?;
        Ok(Expr::Assign(Box::new(left), Box::new(right)))
    } else {
        Ok(left)
    }
}

fn parse_comparison(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let left = parse_additive(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    match &tokens[0] {
        Token::Equal => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::Equal, Box::new(right)))
        }
        Token::StrictEqual => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::StrictEqual, Box::new(right)))
        }
        Token::LessThan => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::LessThan, Box::new(right)))
        }
        Token::GreaterThan => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::GreaterThan, Box::new(right)))
        }
        Token::LessEqual => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::LessEqual, Box::new(right)))
        }
        Token::GreaterEqual => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::GreaterEqual, Box::new(right)))
        }
        _ => Ok(left),
    }
}

fn parse_additive(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let left = parse_multiplicative(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    match &tokens[0] {
        Token::Plus => {
            tokens.remove(0);
            let right = parse_additive(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::Add, Box::new(right)))
        }
        Token::Minus => {
            tokens.remove(0);
            let right = parse_additive(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::Sub, Box::new(right)))
        }
        _ => Ok(left),
    }
}

fn parse_multiplicative(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let left = parse_primary(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    match &tokens[0] {
        Token::Multiply => {
            tokens.remove(0);
            let right = parse_multiplicative(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::Mul, Box::new(right)))
        }
        Token::Divide => {
            tokens.remove(0);
            let right = parse_multiplicative(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::Div, Box::new(right)))
        }
        Token::Mod => {
            tokens.remove(0);
            let right = parse_multiplicative(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::Mod, Box::new(right)))
        }
        _ => Ok(left),
    }
}

fn parse_primary(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    if tokens.is_empty() {
        return Err(JSError::ParseError);
    }
    let mut expr = match tokens.remove(0) {
        Token::Number(n) => Expr::Number(n),
        Token::StringLit(s) => Expr::StringLit(s),
        Token::True => Expr::Boolean(true),
        Token::False => Expr::Boolean(false),
        Token::Minus => {
            let inner = parse_primary(tokens)?;
            Expr::UnaryNeg(Box::new(inner))
        }
        Token::TemplateString(parts) => {
            if parts.is_empty() {
                Expr::StringLit(Vec::new())
            } else if parts.len() == 1 {
                match &parts[0] {
                    TemplatePart::String(s) => Expr::StringLit(s.clone()),
                    TemplatePart::Expr(expr_tokens) => {
                        let mut expr_tokens = expr_tokens.clone();
                        parse_expression(&mut expr_tokens)?
                    }
                }
            } else {
                // Build binary addition chain
                let mut expr = match &parts[0] {
                    TemplatePart::String(s) => Expr::StringLit(s.clone()),
                    TemplatePart::Expr(expr_tokens) => {
                        let mut expr_tokens = expr_tokens.clone();
                        parse_expression(&mut expr_tokens)?
                    }
                };
                for part in &parts[1..] {
                    let right = match part {
                        TemplatePart::String(s) => Expr::StringLit(s.clone()),
                        TemplatePart::Expr(expr_tokens) => {
                            let mut expr_tokens = expr_tokens.clone();
                            parse_expression(&mut expr_tokens)?
                        }
                    };
                    expr = Expr::Binary(Box::new(expr), BinaryOp::Add, Box::new(right));
                }
                expr
            }
        }
        Token::Identifier(name) => Expr::Var(name),
        Token::LBrace => {
            // Parse object literal
            let mut properties = Vec::new();
            if !tokens.is_empty() && matches!(tokens[0], Token::RBrace) {
                // Empty object {}
                tokens.remove(0); // consume }
                return Ok(Expr::Object(properties));
            }
            loop {
                // Parse key
                let key = if let Some(Token::Identifier(name)) = tokens.get(0).cloned() {
                    tokens.remove(0);
                    name
                } else if let Some(Token::StringLit(s)) = tokens.get(0).cloned() {
                    tokens.remove(0);
                    String::from_utf16_lossy(&s)
                } else {
                    return Err(JSError::ParseError);
                };

                // Expect colon
                if tokens.is_empty() || !matches!(tokens[0], Token::Colon) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume :

                // Parse value
                let value = parse_expression(tokens)?;
                properties.push((key, value));

                // Check for comma or end
                if tokens.is_empty() {
                    return Err(JSError::ParseError);
                }
                if matches!(tokens[0], Token::RBrace) {
                    tokens.remove(0); // consume }
                    break;
                } else if matches!(tokens[0], Token::Comma) {
                    tokens.remove(0); // consume ,
                } else {
                    return Err(JSError::ParseError);
                }
            }
            Expr::Object(properties)
        }
        Token::LBracket => {
            // Parse array literal
            let mut elements = Vec::new();
            if !tokens.is_empty() && matches!(tokens[0], Token::RBracket) {
                // Empty array []
                tokens.remove(0); // consume ]
                return Ok(Expr::Array(elements));
            }
            loop {
                // Parse element
                let elem = parse_expression(tokens)?;
                elements.push(elem);

                // Check for comma or end
                if tokens.is_empty() {
                    return Err(JSError::ParseError);
                }
                if matches!(tokens[0], Token::RBracket) {
                    tokens.remove(0); // consume ]
                    break;
                } else if matches!(tokens[0], Token::Comma) {
                    tokens.remove(0); // consume ,
                } else {
                    return Err(JSError::ParseError);
                }
            }
            Expr::Array(elements)
        }
        Token::Function => {
            // Parse function expression
            if tokens.len() >= 1 && matches!(tokens[0], Token::LParen) {
                tokens.remove(0); // consume (
                let mut params = Vec::new();
                if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
                    loop {
                        if let Some(Token::Identifier(param)) = tokens.get(0).cloned() {
                            tokens.remove(0);
                            params.push(param);
                            if tokens.is_empty() {
                                return Err(JSError::ParseError);
                            }
                            if matches!(tokens[0], Token::RParen) {
                                break;
                            }
                            if !matches!(tokens[0], Token::Comma) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume ,
                        } else {
                            return Err(JSError::ParseError);
                        }
                    }
                }
                if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume )
                if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume {
                let body = parse_statements(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume }
                Expr::Function(params, body)
            } else {
                return Err(JSError::ParseError);
            }
        }
        Token::LParen => {
            let expr = parse_expression(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0);
            expr
        }
        _ => {
            return Err(JSError::EvaluationError {
                message: "error".to_string(),
            })
        }
    };

    // Handle postfix operators like index access
    while !tokens.is_empty() {
        match &tokens[0] {
            Token::LBracket => {
                tokens.remove(0); // consume '['
                let index_expr = parse_expression(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0], Token::RBracket) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume ']'
                expr = Expr::Index(Box::new(expr), Box::new(index_expr));
            }
            Token::Dot => {
                tokens.remove(0); // consume '.'
                if tokens.is_empty() || !matches!(tokens[0], Token::Identifier(_)) {
                    return Err(JSError::ParseError);
                }
                if let Token::Identifier(prop) = tokens.remove(0) {
                    expr = Expr::Property(Box::new(expr), prop);
                } else {
                    return Err(JSError::ParseError);
                }
            }
            Token::LParen => {
                tokens.remove(0); // consume '('
                let mut args = Vec::new();
                if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
                    loop {
                        let arg = parse_expression(tokens)?;
                        args.push(arg);
                        if tokens.is_empty() {
                            return Err(JSError::ParseError);
                        }
                        if matches!(tokens[0], Token::RParen) {
                            break;
                        }
                        if !matches!(tokens[0], Token::Comma) {
                            return Err(JSError::ParseError);
                        }
                        tokens.remove(0); // consume ','
                    }
                }
                if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume ')'
                expr = Expr::Call(Box::new(expr), args);
            }
            _ => break,
        }
    }

    Ok(expr)
}

pub unsafe fn JS_GetProperty(_ctx: *mut JSContext, this_obj: JSValue, prop: JSAtom) -> JSValue {
    if this_obj.tag != JS_TAG_OBJECT as i64 {
        return JS_UNDEFINED;
    }
    let p = this_obj.u.ptr as *mut JSObject;
    let sh = (*p).shape;
    if let Some((idx, _)) = (*sh).find_own_property(prop) {
        let prop_val = (*(*p).prop.offset(idx as isize)).u.value;
        // Duplicate returned value when it's ref-counted so caller owns a reference
        if prop_val.has_ref_count() {
            JS_DupValue((*_ctx).rt, prop_val);
        }
        prop_val
    } else {
        JS_UNDEFINED
    }
}

// Reference-count helpers: basic dup/free on objects/strings that store a ref_count
// NOTE: This is a minimal implementation. Proper finalizers and nested frees
// are not implemented here and should be added per object type.
pub unsafe fn JS_DupValue(_rt: *mut JSRuntime, v: JSValue) {
    if v.has_ref_count() {
        let p = v.get_ptr();
        if !p.is_null() {
            let header = p as *mut JSRefCountHeader;
            (*header).ref_count += 1;
        }
    }
}

pub unsafe fn JS_FreeValue(rt: *mut JSRuntime, v: JSValue) {
    if v.has_ref_count() {
        let p = v.get_ptr();
        if p.is_null() {
            return;
        }
        let header = p as *mut JSRefCountHeader;
        (*header).ref_count -= 1;
        if (*header).ref_count > 0 {
            return;
        }
        // ref_count reached zero: dispatch based on tag to proper finalizer
        match v.get_tag() {
            x if x == JS_TAG_STRING => {
                js_free_string(rt, v);
            }
            x if x == JS_TAG_OBJECT => {
                js_free_object(rt, v);
            }
            x if x == JS_TAG_FUNCTION_BYTECODE => {
                js_free_function_bytecode(rt, v);
            }
            x if x == JS_TAG_SYMBOL => {
                js_free_symbol(rt, v);
            }
            x if x == JS_TAG_BIG_INT => {
                js_free_bigint(rt, v);
            }
            x if x == JS_TAG_MODULE => {
                js_free_module(rt, v);
            }
            // For other heap types, do a default free of the pointer
            _ => {
                (*rt).js_free_rt(p as *mut c_void);
            }
        }
    }
}

unsafe fn js_free_string(rt: *mut JSRuntime, v: JSValue) {
    let p = v.get_ptr() as *mut JSString;
    if p.is_null() {
        return;
    }
    // The whole JSString allocation was allocated via js_malloc_rt
    (*rt).js_free_rt(p as *mut c_void);
}

unsafe fn js_free_object(rt: *mut JSRuntime, v: JSValue) {
    let p = v.get_ptr() as *mut JSObject;
    if p.is_null() {
        return;
    }
    // Free property array
    if !(*p).prop.is_null() {
        (*rt).js_free_rt((*p).prop as *mut c_void);
        (*p).prop = std::ptr::null_mut();
    }
    // Free shape
    if !(*p).shape.is_null() {
        (*rt).js_free_shape((*p).shape);
        (*p).shape = std::ptr::null_mut();
    }
    // Free object struct
    (*rt).js_free_rt(p as *mut c_void);
}

unsafe fn js_free_function_bytecode(rt: *mut JSRuntime, v: JSValue) {
    let p = v.get_ptr() as *mut JSFunctionBytecode;
    if p.is_null() {
        return;
    }
    // Free bytecode buffer
    if !(*p).byte_code_buf.is_null() {
        (*rt).js_free_rt((*p).byte_code_buf as *mut c_void);
        (*p).byte_code_buf = std::ptr::null_mut();
    }
    // Free pc2line buffer
    if !(*p).pc2line_buf.is_null() {
        (*rt).js_free_rt((*p).pc2line_buf as *mut c_void);
        (*p).pc2line_buf = std::ptr::null_mut();
    }
    // Free source
    if !(*p).source.is_null() {
        (*rt).js_free_rt((*p).source as *mut c_void);
        (*p).source = std::ptr::null_mut();
    }
    // Free cpool values
    if !(*p).cpool.is_null() && (*p).cpool_count > 0 {
        for i in 0..(*p).cpool_count as isize {
            let val = *(*p).cpool.offset(i);
            if val.has_ref_count() {
                JS_FreeValue(rt, val);
            }
        }
        (*rt).js_free_rt((*p).cpool as *mut c_void);
        (*p).cpool = std::ptr::null_mut();
    }
    // Finally free the struct
    (*rt).js_free_rt(p as *mut c_void);
}

unsafe fn js_free_symbol(rt: *mut JSRuntime, v: JSValue) {
    let p = v.get_ptr();
    if p.is_null() {
        return;
    }
    // Symbols typically store their name as a JSString or internal struct
    // For now, free the pointer directly. Add type-aware finalizer later.
    (*rt).js_free_rt(p as *mut c_void);
}

unsafe fn js_free_bigint(rt: *mut JSRuntime, v: JSValue) {
    let p = v.get_ptr();
    if p.is_null() {
        return;
    }
    // BigInt representation may be inline or heap-allocated. Here we free pointer.
    (*rt).js_free_rt(p as *mut c_void);
}

unsafe fn js_free_module(rt: *mut JSRuntime, v: JSValue) {
    let p = v.get_ptr();
    if p.is_null() {
        return;
    }
    // Module structure not modelled here; free pointer for now.
    (*rt).js_free_rt(p as *mut c_void);
}

pub unsafe fn JS_SetProperty(ctx: *mut JSContext, this_obj: JSValue, prop: JSAtom, val: JSValue) -> i32 {
    JS_DefinePropertyValue(ctx, this_obj, prop, val, 0)
}

impl JSRuntime {
    pub unsafe fn js_new_atom_len(&mut self, name: *const u8, len: usize) -> JSAtom {
        if len == 0 {
            return 0; // invalid
        }
        // Compute hash
        let mut h = 0u32;
        for i in 0..len {
            h = h.wrapping_mul(31).wrapping_add(*name.offset(i as isize) as u32);
        }
        // Find in hash table
        let hash_index = (h % self.atom_hash_size as u32) as i32;
        let mut atom = *self.atom_hash.offset(hash_index as isize);
        while atom != 0 {
            let p = *self.atom_array.offset((atom - 1) as isize);
            if (*p).len == len as u32 && (*p).hash == h {
                // Check string
                let str_data = (p as *mut u8).offset(std::mem::size_of::<JSString>() as isize);
                let mut equal = true;
                for i in 0..len {
                    if *str_data.offset(i as isize) != *name.offset(i as isize) {
                        equal = false;
                        break;
                    }
                }
                if equal {
                    return atom;
                }
            }
            atom = (*p).hash_next;
        }
        // Not found, create new
        if self.atom_count >= self.atom_size {
            let new_size = self.atom_size * 2;
            let new_array = self.js_realloc_rt(
                self.atom_array as *mut c_void,
                (new_size as usize) * std::mem::size_of::<*mut JSAtomStruct>(),
            ) as *mut *mut JSAtomStruct;
            if new_array.is_null() {
                return 0;
            }
            self.atom_array = new_array;
            self.atom_size = new_size;
            for i in self.atom_count..new_size {
                *self.atom_array.offset(i as isize) = std::ptr::null_mut();
            }
        }
        // Allocate JSString
        let str_size = std::mem::size_of::<JSString>() + len;
        let p = self.js_malloc_rt(str_size) as *mut JSString;
        if p.is_null() {
            return 0;
        }
        (*p).header.ref_count = 1;
        (*p).len = len as u32;
        (*p).hash = h;
        (*p).hash_next = *self.atom_hash.offset(hash_index as isize);
        // Copy string
        let str_data = (p as *mut u8).offset(std::mem::size_of::<JSString>() as isize);
        for i in 0..len {
            *str_data.offset(i as isize) = *name.offset(i as isize);
        }
        let new_atom = (self.atom_count + 1) as u32;
        *self.atom_array.offset(self.atom_count as isize) = p;
        *self.atom_hash.offset(hash_index as isize) = new_atom;
        self.atom_count += 1;
        new_atom
    }
}

// Helper functions for array flattening
fn flatten_array(obj_map: &JSObjectData, result: &mut Vec<Value>, depth: usize) {
    let length = obj_get(obj_map, "length").map(|v| v.borrow().clone()).unwrap_or(Value::Number(0.0));
    let current_len = match length {
        Value::Number(n) => n as usize,
        _ => 0,
    };

    for i in 0..current_len {
        if let Some(val) = obj_get(obj_map, &i.to_string()) {
            let value = val.borrow().clone();
            flatten_single_value(value, result, depth);
        }
    }
}

fn flatten_single_value(value: Value, result: &mut Vec<Value>, depth: usize) {
    if depth == 0 {
        result.push(value);
        return;
    }

    match value {
        Value::Object(obj) => {
            // Check if it's an array-like object
            if obj.contains_key("length") {
                flatten_array(&obj, result, depth - 1);
            } else {
                result.push(Value::Object(obj));
            }
        }
        _ => {
            result.push(value);
        }
    }
}
