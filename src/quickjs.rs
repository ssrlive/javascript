#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use libc;
use std::ffi::c_void;

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

pub unsafe fn JS_NewString(ctx: *mut JSContext, s: &str) -> JSValue {
    let len = s.len();
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
    for (i, &byte) in s.as_bytes().iter().enumerate() {
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
        Err(_) => JS_UNDEFINED,
    }
}

fn evaluate_script(script: &str) -> Result<Value, ()> {
    let mut tokens = tokenize(script)?;
    let statements = parse_statements(&mut tokens)?;
    let mut env = std::collections::HashMap::new();
    evaluate_statements(&mut env, &statements)
}

pub fn parse_statements(tokens: &mut Vec<Token>) -> Result<Vec<Statement>, ()> {
    let mut statements = Vec::new();
    while !tokens.is_empty() {
        let stmt = parse_statement(tokens)?;
        statements.push(stmt);
        if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon) {
            tokens.remove(0);
        }
    }
    Ok(statements)
}

fn parse_statement(tokens: &mut Vec<Token>) -> Result<Statement, ()> {
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
    Ok(Statement::Expr(expr))
}

pub fn evaluate_statements(
    env: &mut std::collections::HashMap<String, Value>,
    statements: &[Statement],
) -> Result<Value, ()> {
    let mut last_value = Value::Number(0.0);
    for stmt in statements {
        match stmt {
            Statement::Let(name, expr) => {
                let val = evaluate_expr(env, expr)?;
                env.insert(name.clone(), val.clone());
                last_value = val;
            }
            Statement::Expr(expr) => {
                last_value = evaluate_expr(env, expr)?;
            }
        }
    }
    Ok(last_value)
}

fn evaluate_expr(env: &std::collections::HashMap<String, Value>, expr: &Expr) -> Result<Value, ()> {
    match expr {
        Expr::Number(n) => Ok(Value::Number(*n)),
        Expr::StringLit(s) => Ok(Value::String(s.clone())),
        Expr::Var(name) => env.get(name).cloned().ok_or(()),
        Expr::UnaryNeg(expr) => {
            let val = evaluate_expr(env, expr)?;
            match val {
                Value::Number(n) => Ok(Value::Number(-n)),
                _ => Err(()),
            }
        }
        Expr::Binary(left, op, right) => {
            let l = evaluate_expr(env, left)?;
            let r = evaluate_expr(env, right)?;
            match op {
                BinaryOp::Add => match (l, r) {
                    (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln + rn)),
                    (Value::String(ls), Value::String(rs)) => Ok(Value::String(ls + &rs)),
                    (Value::Number(ln), Value::String(rs)) => {
                        Ok(Value::String(ln.to_string() + &rs))
                    }
                    (Value::String(ls), Value::Number(rn)) => {
                        Ok(Value::String(ls + &rn.to_string()))
                    }
                },
                BinaryOp::Sub => match (l, r) {
                    (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln - rn)),
                    _ => Err(()),
                },
                BinaryOp::Mul => match (l, r) {
                    (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln * rn)),
                    _ => Err(()),
                },
                BinaryOp::Div => match (l, r) {
                    (Value::Number(ln), Value::Number(rn)) => {
                        if rn == 0.0 {
                            Err(())
                        } else {
                            Ok(Value::Number(ln / rn))
                        }
                    }
                    _ => Err(()),
                },
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::String(s), Value::Number(n)) => {
                    let idx = n as usize;
                    if idx < s.len() {
                        let ch = s.chars().nth(idx).unwrap();
                        Ok(Value::String(ch.to_string()))
                    } else {
                        Ok(Value::String(String::new())) // or return undefined, but use empty string here
                    }
                }
                _ => Err(()), // other types of indexing not supported yet
            }
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match (obj_val, prop.as_str()) {
                (Value::String(s), "length") => Ok(Value::Number(s.len() as f64)),
                _ => Err(()), // property not found or not supported
            }
        }
        Expr::Call(func_expr, args) => {
            // For now, we only support method calls on strings
            if let Expr::Property(obj_expr, method_name) = &**func_expr {
                let obj_val = evaluate_expr(env, obj_expr)?;
                if let Value::String(s) = obj_val {
                    match method_name.as_str() {
                        "substring" => {
                            if args.len() == 2 {
                                let start_val = evaluate_expr(env, &args[0])?;
                                let end_val = evaluate_expr(env, &args[1])?;
                                if let (Value::Number(start), Value::Number(end)) =
                                    (start_val, end_val)
                                {
                                    let start_idx = start as usize;
                                    let end_idx = end as usize;
                                    if start_idx <= end_idx && end_idx <= s.len() {
                                        Ok(Value::String(s[start_idx..end_idx].to_string()))
                                    } else {
                                        Err(())
                                    }
                                } else {
                                    Err(())
                                }
                            } else {
                                Err(())
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

                            let len = s.len() as isize;
                            let start = if start < 0 { len + start } else { start };
                            let end = if end < 0 { len + end } else { end };

                            let start = start.max(0).min(len) as usize;
                            let end = end.max(0).min(len) as usize;

                            if start <= end {
                                Ok(Value::String(s[start..end].to_string()))
                            } else {
                                Ok(Value::String("".to_string()))
                            }
                        }
                        "toUpperCase" => {
                            if args.is_empty() {
                                Ok(Value::String(s.to_uppercase()))
                            } else {
                                Err(())
                            }
                        }
                        "toLowerCase" => {
                            if args.is_empty() {
                                Ok(Value::String(s.to_lowercase()))
                            } else {
                                Err(())
                            }
                        }
                        "indexOf" => {
                            if args.len() == 1 {
                                let search_val = evaluate_expr(env, &args[0])?;
                                if let Value::String(search) = search_val {
                                    if let Some(pos) = s.find(&search) {
                                        Ok(Value::Number(pos as f64))
                                    } else {
                                        Ok(Value::Number(-1.0))
                                    }
                                } else {
                                    Err(())
                                }
                            } else {
                                Err(())
                            }
                        }
                        "lastIndexOf" => {
                            if args.len() == 1 {
                                let search_val = evaluate_expr(env, &args[0])?;
                                if let Value::String(search) = search_val {
                                    if let Some(pos) = s.rfind(&search) {
                                        Ok(Value::Number(pos as f64))
                                    } else {
                                        Ok(Value::Number(-1.0))
                                    }
                                } else {
                                    Err(())
                                }
                            } else {
                                Err(())
                            }
                        }
                        "replace" => {
                            if args.len() == 2 {
                                let search_val = evaluate_expr(env, &args[0])?;
                                let replace_val = evaluate_expr(env, &args[1])?;
                                if let (Value::String(search), Value::String(replace)) =
                                    (search_val, replace_val)
                                {
                                    Ok(Value::String(s.replacen(&search, &replace, 1)))
                                } else {
                                    Err(())
                                }
                            } else {
                                Err(())
                            }
                        }
                        "split" => {
                            if args.len() == 1 {
                                let sep_val = evaluate_expr(env, &args[0])?;
                                if let Value::String(sep) = sep_val {
                                    // For simplicity, return the first part only as a string
                                    // In real JS, split returns an array
                                    if let Some(pos) = s.find(&sep) {
                                        Ok(Value::String(s[..pos].to_string()))
                                    } else {
                                        Ok(Value::String(s.clone()))
                                    }
                                } else {
                                    Err(())
                                }
                            } else {
                                Err(())
                            }
                        }
                        _ => Err(()), // method not found
                    }
                } else {
                    Err(())
                }
            } else {
                Err(())
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Number(f64),
    String(String),
}

#[derive(Debug)]
pub enum Statement {
    Let(String, Expr),
    Expr(Expr),
}

#[derive(Debug)]
pub enum Expr {
    Number(f64),
    StringLit(String),
    Var(String),
    Binary(Box<Expr>, BinaryOp, Box<Expr>),
    UnaryNeg(Box<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Property(Box<Expr>, String),
    Call(Box<Expr>, Vec<Expr>),
}

#[derive(Debug)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

fn parse_string_literal(chars: &[char], start: &mut usize, end_char: char) -> Result<String, ()> {
    let mut result = String::new();
    while *start < chars.len() && chars[*start] != end_char {
        if chars[*start] == '\\' {
            *start += 1;
            if *start >= chars.len() {
                return Err(());
            }
            match chars[*start] {
                'n' => result.push('\n'),
                't' => result.push('\t'),
                'r' => result.push('\r'),
                '\\' => result.push('\\'),
                '"' => result.push('"'),
                '\'' => result.push('\''),
                '`' => result.push('`'),
                _ => return Err(()), // Invalid escape sequence
            }
        } else {
            result.push(chars[*start]);
        }
        *start += 1;
    }
    if *start >= chars.len() {
        return Err(());
    }
    Ok(result)
}

pub fn tokenize(expr: &str) -> Result<Vec<Token>, ()> {
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
            '.' => {
                tokens.push(Token::Dot);
                i += 1;
            }
            '0'..='9' => {
                let start = i;
                while i < chars.len() && (chars[i].is_digit(10) || chars[i] == '.') {
                    i += 1;
                }
                let num_str = &expr[start..i];
                let num = num_str.parse::<f64>().map_err(|_| ())?;
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
                            return Err(());
                        }
                        let expr_str = &expr[expr_start..i - 1];
                        // Tokenize the expression inside ${}
                        let expr_tokens = tokenize(expr_str)?;
                        parts.push(TemplatePart::Expr(expr_tokens));
                        current_start = i;
                    } else {
                        i += 1;
                    }
                }
                if i >= chars.len() {
                    return Err(());
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
                let ident = &expr[start..i];
                match ident {
                    "let" => tokens.push(Token::Let),
                    "var" => tokens.push(Token::Var),
                    _ => tokens.push(Token::Identifier(ident.to_string())),
                }
            }
            '=' => {
                tokens.push(Token::Assign);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            ';' => {
                tokens.push(Token::Semicolon);
                i += 1;
            }
            _ => return Err(()),
        }
    }
    Ok(tokens)
}

#[derive(Debug, Clone)]
pub enum TemplatePart {
    String(String),
    Expr(Vec<Token>),
}

#[derive(Debug, Clone)]
pub enum Token {
    Number(f64),
    StringLit(String),
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
    Dot,
    Comma,
    Let,
    Var,
    Assign,
    Semicolon,
}

fn parse_expression(tokens: &mut Vec<Token>) -> Result<Expr, ()> {
    parse_additive(tokens)
}

fn parse_additive(tokens: &mut Vec<Token>) -> Result<Expr, ()> {
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

fn parse_multiplicative(tokens: &mut Vec<Token>) -> Result<Expr, ()> {
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

fn parse_primary(tokens: &mut Vec<Token>) -> Result<Expr, ()> {
    if tokens.is_empty() {
        return Err(());
    }
    let mut expr = match tokens.remove(0) {
        Token::Number(n) => Expr::Number(n),
        Token::StringLit(s) => Expr::StringLit(s),
        Token::Minus => {
            let inner = parse_primary(tokens)?;
            Expr::UnaryNeg(Box::new(inner))
        }
        Token::TemplateString(parts) => {
            if parts.is_empty() {
                Expr::StringLit(String::new())
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
        Token::LParen => {
            let expr = parse_expression(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                return Err(());
            }
            tokens.remove(0);
            expr
        }
        _ => return Err(()),
    };

    // Handle postfix operators like index access
    while !tokens.is_empty() {
        match &tokens[0] {
            Token::LBracket => {
                tokens.remove(0); // consume '['
                let index_expr = parse_expression(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0], Token::RBracket) {
                    return Err(());
                }
                tokens.remove(0); // consume ']'
                expr = Expr::Index(Box::new(expr), Box::new(index_expr));
            }
            Token::Dot => {
                tokens.remove(0); // consume '.'
                if tokens.is_empty() || !matches!(tokens[0], Token::Identifier(_)) {
                    return Err(());
                }
                if let Token::Identifier(prop) = tokens.remove(0) {
                    expr = Expr::Property(Box::new(expr), prop);
                } else {
                    return Err(());
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
                            return Err(());
                        }
                        if matches!(tokens[0], Token::RParen) {
                            break;
                        }
                        if !matches!(tokens[0], Token::Comma) {
                            return Err(());
                        }
                        tokens.remove(0); // consume ','
                    }
                }
                if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                    return Err(());
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
