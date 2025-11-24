#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use crate::error::JSError;
use libc;
use std::ffi::c_void;

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
        (self.tag as i32) >= JS_TAG_FIRST
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
#[derive(Copy, Clone)]
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
#[derive(Copy, Clone)]
pub struct JSMallocState {
    pub malloc_count: usize,
    pub malloc_size: usize,
    pub malloc_limit: usize,
    pub opaque: *mut c_void,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct JSMallocFunctions {
    pub js_malloc: Option<unsafe extern "C" fn(*mut JSMallocState, usize) -> *mut c_void>,
    pub js_free: Option<unsafe extern "C" fn(*mut JSMallocState, *mut c_void)>,
    pub js_realloc:
        Option<unsafe extern "C" fn(*mut JSMallocState, *mut c_void, usize) -> *mut c_void>,
    pub js_malloc_usable_size: Option<unsafe extern "C" fn(*const c_void) -> usize>,
}

pub type JSAtom = u32;

#[repr(C)]
#[derive(Copy, Clone)]
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
#[derive(Copy, Clone)]
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
#[derive(Copy, Clone)]
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
#[derive(Copy, Clone)]
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
    pub eval_internal: Option<
        unsafe extern "C" fn(
            *mut JSContext,
            JSValue,
            *const i8,
            usize,
            *const i8,
            i32,
            i32,
        ) -> JSValue,
    >,
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
    pub call: Option<
        unsafe extern "C" fn(*mut JSContext, JSValue, JSValue, i32, *mut JSValue, i32) -> JSValue,
    >,
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
        if let Some(_) = (*sh).find_own_property(atom) {
            // Already exists
            let (idx, _) = (*sh).find_own_property(atom).unwrap();
            return idx;
        }

        if (*sh).prop_count >= (*sh).prop_size {
            let new_size = if (*sh).prop_size == 0 {
                4
            } else {
                (*sh).prop_size * 3 / 2
            };
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
        self.atom_hash = self
            .js_malloc_rt((self.atom_hash_size as usize) * std::mem::size_of::<u32>())
            as *mut u32;
        if self.atom_hash.is_null() {
            return;
        }
        for i in 0..self.atom_hash_size {
            *self.atom_hash.offset(i as isize) = 0;
        }
        self.atom_array = self
            .js_malloc_rt((self.atom_size as usize) * std::mem::size_of::<*mut JSAtomStruct>())
            as *mut *mut JSAtomStruct;
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

pub unsafe fn JS_DefinePropertyValue(
    ctx: *mut JSContext,
    this_obj: JSValue,
    prop: JSAtom,
    val: JSValue,
    flags: i32,
) -> i32 {
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
    let new_prop = (*(*ctx).rt).js_realloc_rt(
        (*p).prop as *mut c_void,
        ((*sh).prop_size as usize) * std::mem::size_of::<JSProperty>(),
    ) as *mut JSProperty;

    if new_prop.is_null() {
        return -1;
    }
    (*p).prop = new_prop;

    // Set value
    let pr = (*p).prop.offset(idx as isize);
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
    unsafe extern "C" fn my_realloc(
        _state: *mut JSMallocState,
        ptr: *mut c_void,
        size: usize,
    ) -> *mut c_void {
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

pub unsafe fn JS_Eval(
    _ctx: *mut JSContext,
    input: *const i8,
    input_len: usize,
    _filename: *const i8,
    _eval_flags: i32,
) -> JSValue {
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
    let mut tokens = tokenize(script)?;
    let statements = parse_statements(&mut tokens)?;
    let mut env = std::collections::HashMap::new();
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
        let init = if tokens.len() >= 1
            && (matches!(tokens[0], Token::Let) || matches!(tokens[0], Token::Var))
        {
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

pub fn evaluate_statements(
    env: &mut std::collections::HashMap<String, Value>,
    statements: &[Statement],
) -> Result<Value, JSError> {
    let mut last_value = Value::Number(0.0);
    for stmt in statements {
        match stmt {
            Statement::Let(name, expr) => {
                let val = evaluate_expr(env, expr)?;
                env.insert(name.clone(), val.clone());
                last_value = val;
            }
            Statement::Assign(name, expr) => {
                let val = evaluate_expr(env, expr)?;
                env.insert(name.clone(), val.clone());
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
                            env.insert(name.clone(), val);
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
                                        env.insert(name.clone(), val);
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

fn evaluate_expr(
    env: &std::collections::HashMap<String, Value>,
    expr: &Expr,
) -> Result<Value, JSError> {
    match expr {
        Expr::Number(n) => Ok(Value::Number(*n)),
        Expr::StringLit(s) => Ok(Value::String(s.clone())),
        Expr::Boolean(b) => Ok(Value::Boolean(*b)),
        Expr::Var(name) => {
            if let Some(val) = env.get(name) {
                Ok(val.clone())
            } else if name == "console" {
                let mut console_obj = std::collections::HashMap::new();
                console_obj.insert(
                    "log".to_string(),
                    Value::Function("console.log".to_string()),
                );
                Ok(Value::Object(console_obj))
            } else if name == "String" {
                Ok(Value::Function("String".to_string()))
            } else {
                Ok(Value::Undefined)
            }
        }
        Expr::Assign(_target, value) => {
            // Assignment is handled at statement level, just evaluate the value
            evaluate_expr(env, value)
        }
        Expr::UnaryNeg(expr) => {
            let val = evaluate_expr(env, expr)?;
            match val {
                Value::Number(n) => Ok(Value::Number(-n)),
                _ => Err(JSError::EvaluationError {
                    message: "error".to_string(),
                }),
            }
        }
        Expr::Binary(left, op, right) => {
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
                    (Value::Number(ln), Value::Number(rn)) => {
                        Ok(Value::Number(if ln == rn { 1.0 } else { 0.0 }))
                    }
                    (Value::String(ls), Value::String(rs)) => {
                        Ok(Value::Number(if ls == rs { 1.0 } else { 0.0 }))
                    }
                    _ => Ok(Value::Number(0.0)), // Different types are not equal
                },
                BinaryOp::StrictEqual => match (l, r) {
                    (Value::Number(ln), Value::Number(rn)) => {
                        Ok(Value::Number(if ln == rn { 1.0 } else { 0.0 }))
                    }
                    (Value::String(ls), Value::String(rs)) => {
                        Ok(Value::Number(if ls == rs { 1.0 } else { 0.0 }))
                    }
                    _ => Ok(Value::Number(0.0)), // Different types are not equal
                },
                BinaryOp::LessThan => match (l, r) {
                    (Value::Number(ln), Value::Number(rn)) => {
                        Ok(Value::Number(if ln < rn { 1.0 } else { 0.0 }))
                    }
                    (Value::String(ls), Value::String(rs)) => {
                        Ok(Value::Number(if ls < rs { 1.0 } else { 0.0 }))
                    }
                    _ => Err(JSError::EvaluationError {
                        message: "error".to_string(),
                    }),
                },
                BinaryOp::GreaterThan => match (l, r) {
                    (Value::Number(ln), Value::Number(rn)) => {
                        Ok(Value::Number(if ln > rn { 1.0 } else { 0.0 }))
                    }
                    (Value::String(ls), Value::String(rs)) => {
                        Ok(Value::Number(if ls > rs { 1.0 } else { 0.0 }))
                    }
                    _ => Err(JSError::EvaluationError {
                        message: "error".to_string(),
                    }),
                },
                BinaryOp::LessEqual => match (l, r) {
                    (Value::Number(ln), Value::Number(rn)) => {
                        Ok(Value::Number(if ln <= rn { 1.0 } else { 0.0 }))
                    }
                    (Value::String(ls), Value::String(rs)) => {
                        Ok(Value::Number(if ls <= rs { 1.0 } else { 0.0 }))
                    }
                    _ => Err(JSError::EvaluationError {
                        message: "error".to_string(),
                    }),
                },
                BinaryOp::GreaterEqual => match (l, r) {
                    (Value::Number(ln), Value::Number(rn)) => {
                        Ok(Value::Number(if ln >= rn { 1.0 } else { 0.0 }))
                    }
                    (Value::String(ls), Value::String(rs)) => {
                        Ok(Value::Number(if ls >= rs { 1.0 } else { 0.0 }))
                    }
                    _ => Err(JSError::EvaluationError {
                        message: "error".to_string(),
                    }),
                },
            }
        }
        Expr::Index(obj, idx) => {
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
                _ => Err(JSError::EvaluationError {
                    message: "error".to_string(),
                }), // other types of indexing not supported yet
            }
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            println!("Property: obj_val={:?}, prop={}", obj_val, prop);
            match obj_val {
                Value::String(s) if prop == "length" => Ok(Value::Number(utf16_len(&s) as f64)),
                Value::Object(obj_map) => {
                    if let Some(val) = obj_map.get(prop.as_str()) {
                        Ok(val.clone())
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
        Expr::Call(func_expr, args) => {
            // Check if it's a method call first
            if let Expr::Property(obj_expr, method_name) = &**func_expr {
                let obj_val = evaluate_expr(env, obj_expr)?;
                match (obj_val, method_name.as_str()) {
                    (Value::Object(obj_map), "log") if obj_map.contains_key("log") => {
                        // console.log call
                        for arg in args {
                            let arg_val = evaluate_expr(env, arg)?;
                            match arg_val {
                                Value::Number(n) => print!("{}", n),
                                Value::String(s) => {
                                    print!("{}", String::from_utf16_lossy(&s))
                                }
                                Value::Boolean(b) => print!("{}", b),
                                Value::Undefined => print!("undefined"),
                                Value::Object(_) => print!("[object Object]"),
                                Value::Function(name) => print!("[Function: {}]", name),
                                Value::Closure(_, _, _) => print!("[Function]"),
                            }
                        }
                        println!();
                        Ok(Value::Undefined)
                    }
                    (obj_val, "toString") => {
                        // toString method for all values
                        if args.is_empty() {
                            match obj_val {
                                Value::Number(n) => {
                                    Ok(Value::String(utf8_to_utf16(&n.to_string())))
                                }
                                Value::String(s) => Ok(Value::String(s.clone())),
                                Value::Boolean(b) => {
                                    Ok(Value::String(utf8_to_utf16(&b.to_string())))
                                }
                                Value::Undefined => Ok(Value::String(utf8_to_utf16("undefined"))),
                                Value::Object(_) => {
                                    Ok(Value::String(utf8_to_utf16("[object Object]")))
                                }
                                Value::Function(name) => Ok(Value::String(utf8_to_utf16(
                                    &format!("[Function: {}]", name),
                                ))),
                                Value::Closure(_, _, _) => {
                                    Ok(Value::String(utf8_to_utf16("[Function]")))
                                }
                            }
                        } else {
                            Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            })
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
                                    if let (Value::Number(start), Value::Number(end)) =
                                        (start_val, end_val)
                                    {
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
                                    if let (Value::String(search), Value::String(replace)) =
                                        (search_val, replace_val)
                                    {
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
                                        // For simplicity, return the first part only as a string
                                        // In real JS, split returns an array
                                        if let Some(pos) = utf16_find(&s, &sep) {
                                            Ok(Value::String(utf16_slice(&s, 0, pos)))
                                        } else {
                                            Ok(Value::String(s.clone()))
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
                                    Value::Number(n) => {
                                        Ok(Value::String(utf8_to_utf16(&n.to_string())))
                                    }
                                    Value::String(s) => Ok(Value::String(s.clone())),
                                    Value::Boolean(b) => {
                                        Ok(Value::String(utf8_to_utf16(&b.to_string())))
                                    }
                                    Value::Undefined => {
                                        Ok(Value::String(utf8_to_utf16("undefined")))
                                    }
                                    Value::Object(_) => {
                                        Ok(Value::String(utf8_to_utf16("[object Object]")))
                                    }
                                    Value::Function(name) => Ok(Value::String(utf8_to_utf16(
                                        &format!("[Function: {}]", name),
                                    ))),
                                    Value::Closure(_, _, _) => {
                                        Ok(Value::String(utf8_to_utf16("[Function]")))
                                    }
                                }
                            } else {
                                Ok(Value::String(Vec::new())) // String() with no args returns empty string
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
                            func_env.insert(param.clone(), arg_val);
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
        Expr::Function(params, body) => {
            Ok(Value::Closure(params.clone(), body.clone(), env.clone()))
        }
        Expr::Object(properties) => {
            let mut obj = std::collections::HashMap::new();
            for (key, value_expr) in properties {
                let value = evaluate_expr(env, value_expr)?;
                obj.insert(key.clone(), value);
            }
            Ok(Value::Object(obj))
        }
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Number(f64),
    String(Vec<u16>), // UTF-16 code units
    Boolean(bool),
    Undefined,
    Object(std::collections::HashMap<String, Value>), // Object with properties
    Function(String),                                 // Function name
    Closure(
        Vec<String>,
        Vec<Statement>,
        std::collections::HashMap<String, Value>,
    ), // parameters, body, captured environment
}

// Helper functions for UTF-16 string operations
fn utf8_to_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn utf16_to_utf8(v: &[u16]) -> String {
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

#[derive(Debug, Clone)]
pub enum Statement {
    Let(String, Expr),
    Assign(String, Expr), // variable assignment
    Expr(Expr),
    Return(Option<Expr>),
    If(Expr, Vec<Statement>, Option<Vec<Statement>>), // condition, then_body, else_body
    For(
        Option<Box<Statement>>,
        Option<Expr>,
        Option<Box<Statement>>,
        Vec<Statement>,
    ), // init, condition, increment, body
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
}

#[derive(Debug, Clone)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Equal,
    StrictEqual,
    LessThan,
    GreaterThan,
    LessEqual,
    GreaterEqual,
}

fn parse_string_literal(
    chars: &[char],
    start: &mut usize,
    end_char: char,
) -> Result<Vec<u16>, JSError> {
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
                let num = num_str
                    .parse::<f64>()
                    .map_err(|_| JSError::TokenizationError)?;
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
            Ok(Expr::Binary(
                Box::new(left),
                BinaryOp::Equal,
                Box::new(right),
            ))
        }
        Token::StrictEqual => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(
                Box::new(left),
                BinaryOp::StrictEqual,
                Box::new(right),
            ))
        }
        Token::LessThan => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(
                Box::new(left),
                BinaryOp::LessThan,
                Box::new(right),
            ))
        }
        Token::GreaterThan => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(
                Box::new(left),
                BinaryOp::GreaterThan,
                Box::new(right),
            ))
        }
        Token::LessEqual => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(
                Box::new(left),
                BinaryOp::LessEqual,
                Box::new(right),
            ))
        }
        Token::GreaterEqual => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(
                Box::new(left),
                BinaryOp::GreaterEqual,
                Box::new(right),
            ))
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
        prop_val
    } else {
        JS_UNDEFINED
    }
}

pub unsafe fn JS_SetProperty(
    ctx: *mut JSContext,
    this_obj: JSValue,
    prop: JSAtom,
    val: JSValue,
) -> i32 {
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
            h = h
                .wrapping_mul(31)
                .wrapping_add(*name.offset(i as isize) as u32);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_object_property() {
        unsafe {
            let rt = JS_NewRuntime();
            assert!(!rt.is_null());
            let ctx = JS_NewContext(rt);
            assert!(!ctx.is_null());

            // create object
            let obj = JS_NewObject(ctx);
            assert_eq!(obj.get_tag(), JS_TAG_OBJECT);
            let obj_ptr = obj.get_ptr() as *mut JSObject;
            assert!(!obj_ptr.is_null());

            // create property name atom
            let key = CString::new("a").unwrap();
            let atom = (*rt).js_new_atom_len(key.as_ptr() as *const u8, 1);
            assert!(atom != 0);

            // set property value
            let val = JSValue::new_int32(42);
            let ret = JS_DefinePropertyValue(ctx, obj, atom, val, 0);
            assert_eq!(ret, 1);

            // find property
            let shape = (*obj_ptr).shape;
            let (idx, _) = (*shape).find_own_property(atom).unwrap();
            let prop_val = (*(*obj_ptr).prop.offset(idx as isize)).u.value;
            assert_eq!(prop_val.get_tag(), JS_TAG_INT);
            assert_eq!(prop_val.u.int32, 42);

            JS_FreeContext(ctx);
            JS_FreeRuntime(rt);
        }
    }

    #[test]
    fn test_eval_numeric() {
        unsafe {
            let rt = JS_NewRuntime();
            assert!(!rt.is_null());
            let ctx = JS_NewContext(rt);
            assert!(!ctx.is_null());

            let script = b"42.5";
            let result = JS_Eval(
                ctx,
                script.as_ptr() as *const i8,
                script.len(),
                std::ptr::null(),
                0,
            );
            assert_eq!(result.get_tag(), JS_TAG_FLOAT64);
            assert_eq!(result.u.float64, 42.5);

            JS_FreeContext(ctx);
            JS_FreeRuntime(rt);
        }
    }
}
