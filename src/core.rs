#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use crate::error::JSError;
use crate::js_array::{get_array_length, is_array, set_array_length};
use crate::js_class::{
    ClassDefinition, ClassMember, call_class_method, call_static_method, create_class_object, evaluate_new, evaluate_super,
    evaluate_super_call, evaluate_super_method, evaluate_super_property, evaluate_this, is_class_instance, is_instance_of,
};
use crate::js_console;
use crate::js_math;
use crate::js_number;
use crate::js_promise::{JSPromise, PromiseState, handle_promise_method, run_event_loop};
use crate::sprintf;
use crate::tmpfile;
use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;

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
        (JS_TAG_FIRST..=JS_TAG_OBJECT).contains(&t)
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
    /// # Safety
    /// The caller must ensure that the list_head is properly initialized and not concurrently accessed.
    pub unsafe fn init(&mut self) {
        self.prev = self;
        self.next = self;
    }

    /// # Safety
    /// The caller must ensure that `new_entry` is a valid pointer to an uninitialized list_head,
    /// and that the list is not concurrently modified.
    pub unsafe fn add_tail(&mut self, new_entry: *mut list_head) {
        unsafe {
            let prev = self.prev;
            (*new_entry).next = self;
            (*new_entry).prev = prev;
            (*prev).next = new_entry;
            self.prev = new_entry;
        }
    }

    /// # Safety
    /// The caller must ensure that the list_head is part of a valid linked list and not concurrently accessed.
    pub unsafe fn del(&mut self) {
        unsafe {
            let next = self.next;
            let prev = self.prev;
            (*next).prev = prev;
            (*prev).next = next;
            self.next = std::ptr::null_mut();
            self.prev = std::ptr::null_mut();
        }
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
    /// # Safety
    /// The caller must ensure that the JSShape and its property arrays are valid and not concurrently modified.
    pub unsafe fn find_own_property(&self, atom: JSAtom) -> Option<(i32, *mut JSShapeProperty)> {
        unsafe {
            if self.is_hashed != 0 {
                let h = atom & self.prop_hash_mask;
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
}

impl JSRuntime {
    /// # Safety
    /// The caller must ensure that `sh` is a valid pointer to a JSShape that is not concurrently accessed,
    /// and that the runtime's memory allocation functions are properly set up.
    pub unsafe fn resize_shape(&mut self, sh: *mut JSShape, new_size: i32) -> i32 {
        unsafe {
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
    }

    /// # Safety
    /// The caller must ensure that `sh` is a valid pointer to a JSShape that is not concurrently accessed,
    /// and that the runtime's memory allocation functions are properly set up.
    pub unsafe fn add_property(&mut self, sh: *mut JSShape, atom: JSAtom, flags: u8) -> i32 {
        unsafe {
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
                    *(*sh).prop_hash.add(i) = 0;
                }
                // Fill hash table with existing properties
                for i in 0..(*sh).prop_count {
                    let pr = (*sh).prop.add(i as usize);
                    let h = ((*pr).atom) & (*sh).prop_hash_mask;
                    (*pr).hash_next = *(*sh).prop_hash.add(h as usize);
                    *(*sh).prop_hash.add(h as usize) = (i + 1) as u32;
                }
            }

            let idx = (*sh).prop_count;
            let pr = (*sh).prop.add(idx as usize);
            (*pr).atom = atom;
            (*pr).flags = flags;
            if (*sh).is_hashed != 0 {
                let h = (atom) & (*sh).prop_hash_mask;
                (*pr).hash_next = *(*sh).prop_hash.add(h as usize);
                *(*sh).prop_hash.add(h as usize) = (idx + 1) as u32;
            } else {
                (*pr).hash_next = 0;
            }
            (*sh).prop_count += 1;

            idx
        }
    }

    /// # Safety
    /// The caller must ensure that the runtime's memory allocation functions are properly set up,
    /// and that `ptr` is either null or a valid pointer previously returned by the allocator.
    pub unsafe fn js_realloc_rt(&mut self, ptr: *mut c_void, size: usize) -> *mut c_void {
        unsafe {
            if let Some(realloc_func) = self.mf.js_realloc {
                realloc_func(&mut self.malloc_state, ptr, size)
            } else {
                std::ptr::null_mut()
            }
        }
    }

    /// # Safety
    /// The caller must ensure that the runtime's memory allocation functions are properly set up.
    pub unsafe fn js_malloc_rt(&mut self, size: usize) -> *mut c_void {
        unsafe {
            if let Some(malloc_func) = self.mf.js_malloc {
                malloc_func(&mut self.malloc_state, size)
            } else {
                std::ptr::null_mut()
            }
        }
    }

    /// # Safety
    /// The caller must ensure that `ptr` is either null or a valid pointer previously returned by the allocator,
    /// and that the runtime's memory allocation functions are properly set up.
    pub unsafe fn js_free_rt(&mut self, ptr: *mut c_void) {
        unsafe {
            if let Some(free_func) = self.mf.js_free {
                free_func(&mut self.malloc_state, ptr);
            }
        }
    }

    /// # Safety
    /// The caller must ensure that the runtime's memory allocation functions are properly set up
    /// and that the runtime is not concurrently accessed.
    pub unsafe fn init_atoms(&mut self) {
        unsafe {
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
            self.atom_array =
                self.js_malloc_rt((self.atom_size as usize) * std::mem::size_of::<*mut JSAtomStruct>()) as *mut *mut JSAtomStruct;
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
    }

    /// # Safety
    /// The caller must ensure that `proto` is either null or a valid pointer to a JSObject,
    /// and that the runtime's memory allocation functions are properly set up.
    pub unsafe fn js_new_shape(&mut self, proto: *mut JSObject) -> *mut JSShape {
        unsafe {
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
            (*sh).deleted_prop_count = 0;
            (*sh).prop = std::ptr::null_mut();
            (*sh).prop_hash = std::ptr::null_mut();
            (*sh).proto = proto;
            sh
        }
    }

    /// # Safety
    /// The caller must ensure that `sh` is either null or a valid pointer to a JSShape previously allocated by this runtime,
    /// and that the runtime's memory allocation functions are properly set up.
    pub unsafe fn js_free_shape(&mut self, sh: *mut JSShape) {
        unsafe {
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
}

/// # Safety
/// The caller must ensure that `ctx` and `this_obj` are valid pointers, and that the runtime is properly initialized.
pub unsafe fn JS_DefinePropertyValue(ctx: *mut JSContext, this_obj: JSValue, prop: JSAtom, val: JSValue, flags: i32) -> i32 {
    if this_obj.tag != JS_TAG_OBJECT as i64 {
        return -1; // TypeError
    }
    let p = unsafe { this_obj.u.ptr } as *mut JSObject;
    let sh = unsafe { (*p).shape };

    // Add property to shape
    // Note: In real QuickJS, we might need to clone shape if it is shared
    // For now, assume shape is unique to object or we modify it in place (dangerous if shared)

    let idx = unsafe { (*(*ctx).rt).add_property(sh, prop, flags as u8) };
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
    let old_prop = unsafe { (*p).prop };
    let new_prop = unsafe {
        (*(*ctx).rt).js_realloc_rt(
            (*p).prop as *mut c_void,
            ((*sh).prop_size as usize) * std::mem::size_of::<JSProperty>(),
        ) as *mut JSProperty
    };

    if new_prop.is_null() {
        return -1;
    }
    unsafe { (*p).prop = new_prop };
    // If the prop array was just created, zero-initialize it to avoid reading
    // uninitialized JSProperty values later.
    if old_prop.is_null() && !new_prop.is_null() {
        let size_bytes = unsafe { ((*sh).prop_size as usize) * std::mem::size_of::<JSProperty>() };
        unsafe { std::ptr::write_bytes(new_prop as *mut u8, 0, size_bytes) };
    }

    // Set value
    let pr = unsafe { (*p).prop.offset(idx as isize) };
    // If replacing an existing value, free it
    let old_val = unsafe { (*pr).u.value };
    if old_val.has_ref_count() {
        unsafe { JS_FreeValue((*ctx).rt, old_val) };
    }
    // Duplicate incoming value if it's ref-counted
    if val.has_ref_count() {
        unsafe { JS_DupValue((*ctx).rt, val) };
    }
    unsafe { (*pr).u.value = val };

    1
}

/// # Safety
/// This function initializes a new JavaScript runtime with default memory allocation functions.
/// The caller must ensure that the returned runtime is properly freed with JS_FreeRuntime.
pub unsafe fn JS_NewRuntime() -> *mut JSRuntime {
    unsafe extern "C" fn my_malloc(_state: *mut JSMallocState, size: usize) -> *mut c_void {
        unsafe { libc::malloc(size) }
    }
    unsafe extern "C" fn my_free(_state: *mut JSMallocState, ptr: *mut c_void) {
        unsafe { libc::free(ptr) };
    }
    unsafe extern "C" fn my_realloc(_state: *mut JSMallocState, ptr: *mut c_void, size: usize) -> *mut c_void {
        unsafe { libc::realloc(ptr, size) }
    }

    unsafe {
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
}

/// # Safety
/// The caller must ensure that `rt` is either null or a valid pointer to a JSRuntime previously created by JS_NewRuntime,
/// and that no contexts or objects from this runtime are still in use.
pub unsafe fn JS_FreeRuntime(rt: *mut JSRuntime) {
    if !rt.is_null() {
        // Free allocated resources
        // For now, just free the rt
        unsafe { libc::free(rt as *mut c_void) };
    }
}

/// # Safety
/// The caller must ensure that `rt` is a valid pointer to a JSRuntime, and that the context is properly freed with JS_FreeContext.
pub unsafe fn JS_NewContext(rt: *mut JSRuntime) -> *mut JSContext {
    unsafe {
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
}

/// # Safety
/// The caller must ensure that `ctx` is either null or a valid pointer to a JSContext previously created by JS_NewContext,
/// and that no objects from this context are still in use.
pub unsafe fn JS_FreeContext(ctx: *mut JSContext) {
    if !ctx.is_null() {
        unsafe { (*(*ctx).rt).js_free_rt(ctx as *mut c_void) };
    }
}

/// # Safety
/// The caller must ensure that `ctx` is a valid pointer to a JSContext.
pub unsafe fn JS_NewObject(ctx: *mut JSContext) -> JSValue {
    unsafe {
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
}

/// # Safety
/// The caller must ensure that `ctx` is a valid pointer to a JSContext.
pub unsafe fn JS_NewString(ctx: *mut JSContext, s: &[u16]) -> JSValue {
    unsafe {
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
        let str_data = (p as *mut u8).add(std::mem::size_of::<JSString>());
        for (i, &byte) in utf8_str.as_bytes().iter().enumerate() {
            *str_data.add(i) = byte;
        }
        JSValue::new_ptr(JS_TAG_STRING, p as *mut c_void)
    }
}

/// # Safety
/// The caller must ensure that `ctx` is a valid pointer to a JSContext, and that `input` points to valid UTF-8 data of length `input_len`.
pub unsafe fn JS_Eval(_ctx: *mut JSContext, input: *const i8, input_len: usize, _filename: *const i8, _eval_flags: i32) -> JSValue {
    unsafe {
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
            Ok(Value::Object(_)) => JS_UNDEFINED,          // For now
            Ok(Value::Function(_)) => JS_UNDEFINED,        // For now
            Ok(Value::Closure(_, _, _)) => JS_UNDEFINED,   // For now
            Ok(Value::ClassDefinition(_)) => JS_UNDEFINED, // For now
            Ok(Value::Getter(_, _)) => JS_UNDEFINED,       // For now
            Ok(Value::Setter(_, _, _)) => JS_UNDEFINED,    // For now
            Ok(Value::Property { .. }) => JS_UNDEFINED,    // For now
            Ok(Value::Promise(_)) => JS_UNDEFINED,         // For now
            Err(_) => JS_UNDEFINED,
        }
    }
}

pub fn evaluate_script<T: AsRef<str>>(script: T) -> Result<Value, JSError> {
    let script = script.as_ref();
    log::debug!("evaluate_script async called with script len {}", script.len());
    let filtered = filter_input_script(script);
    log::trace!("filtered script:\n{}", filtered);
    let mut tokens = match tokenize(&filtered) {
        Ok(t) => t,
        Err(e) => {
            log::debug!("tokenize error: {e:?}");
            return Err(e);
        }
    };
    let statements = match parse_statements(&mut tokens) {
        Ok(s) => s,
        Err(e) => {
            log::debug!("parse_statements error: {e:?}");
            return Err(e);
        }
    };
    log::debug!("parsed {} statements", statements.len());
    for (i, stmt) in statements.iter().enumerate() {
        log::trace!("stmt[{i}] = {stmt:?}");
    }
    let env: JSObjectDataPtr = Rc::new(RefCell::new(JSObjectData::new()));

    // Inject simple host `std` / `os` shims when importing with the pattern:
    //   import * as NAME from "std";
    for line in script.lines() {
        let l = line.trim();
        if l.starts_with("import * as")
            && l.contains("from")
            && let (Some(as_idx), Some(from_idx)) = (l.find("as"), l.find("from"))
        {
            let name_part = &l[as_idx + 2..from_idx].trim();
            let name = name_part.trim();
            if let Some(start_quote) = l[from_idx..].find(|c: char| ['"', '\''].contains(&c)) {
                let quote_char = l[from_idx + start_quote..].chars().next().unwrap();
                let rest = &l[from_idx + start_quote + 1..];
                if let Some(end_quote) = rest.find(quote_char) {
                    let module = &rest[..end_quote];
                    if module == "std" {
                        obj_set_value(&env, name, Value::Object(crate::js_std::make_std_object()?))?;
                    } else if module == "os" {
                        obj_set_value(&env, name, Value::Object(crate::js_os::make_os_object()?))?;
                    }
                }
            }
        }
    }

    // Initialize global built-in constructors
    initialize_global_constructors(&env);

    match evaluate_statements(&env, &statements) {
        Ok(v) => {
            // If the result is a Promise object (wrapped in Object with __promise property), wait for it to resolve
            if let Value::Object(obj) = &v
                && let Some(promise_val_rc) = obj_get_value(obj, "__promise")?
                && let Value::Promise(promise) = &*promise_val_rc.borrow()
            {
                // Run the event loop until the promise is resolved
                loop {
                    run_event_loop()?;
                    let promise_borrow = promise.borrow();
                    match &promise_borrow.state {
                        PromiseState::Fulfilled(val) => return Ok(val.clone()),
                        PromiseState::Rejected(reason) => {
                            return Err(JSError::EvaluationError {
                                message: format!("Promise rejected: {}", value_to_string(reason)),
                            });
                        }
                        PromiseState::Pending => {
                            // Continue running the event loop
                        }
                    }
                }
            }
            // Run the event loop to process any queued asynchronous tasks
            run_event_loop()?;
            Ok(v)
        }
        Err(e) => {
            log::debug!("evaluate_statements error: {e:?}");
            Err(e)
        }
    }
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
    if !tokens.is_empty() && matches!(tokens[0], Token::Break) {
        tokens.remove(0); // consume break
        if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume ;
        return Ok(Statement::Break);
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Continue) {
        tokens.remove(0); // consume continue
        if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume ;
        return Ok(Statement::Continue);
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::While) {
        tokens.remove(0); // consume while
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
        let body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume }
        return Ok(Statement::While(condition, body));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Do) {
        tokens.remove(0); // consume do
        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume {
        let body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume }
        if tokens.is_empty() || !matches!(tokens[0], Token::While) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume while
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume (
        let condition = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume )
        if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume ;
        return Ok(Statement::DoWhile(body, condition));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Switch) {
        tokens.remove(0); // consume switch
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume (
        let expr = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume )
        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume {
        let mut cases = Vec::new();
        while !tokens.is_empty() && !matches!(tokens[0], Token::RBrace) {
            if matches!(tokens[0], Token::Case) {
                tokens.remove(0); // consume case
                let case_value = parse_expression(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0], Token::Colon) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume :
                let mut case_stmts = Vec::new();
                while !tokens.is_empty()
                    && !matches!(tokens[0], Token::Case)
                    && !matches!(tokens[0], Token::Default)
                    && !matches!(tokens[0], Token::RBrace)
                {
                    let stmt = parse_statement(tokens)?;
                    case_stmts.push(stmt);
                    if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon) {
                        tokens.remove(0);
                    }
                }
                cases.push(SwitchCase::Case(case_value, case_stmts));
            } else if matches!(tokens[0], Token::Default) {
                tokens.remove(0); // consume default
                if tokens.is_empty() || !matches!(tokens[0], Token::Colon) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume :
                let mut default_stmts = Vec::new();
                while !tokens.is_empty() && !matches!(tokens[0], Token::RBrace) {
                    let stmt = parse_statement(tokens)?;
                    default_stmts.push(stmt);
                    if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon) {
                        tokens.remove(0);
                    }
                }
                cases.push(SwitchCase::Default(default_stmts));
            } else {
                return Err(JSError::ParseError);
            }
        }
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume }
        return Ok(Statement::Switch(expr, cases));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Throw) {
        tokens.remove(0); // consume throw
        let expr = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume ;
        return Ok(Statement::Throw(expr));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Async) {
        tokens.remove(0); // consume async
        if !tokens.is_empty() && matches!(tokens[0], Token::Function) {
            tokens.remove(0); // consume function
            if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                tokens.remove(0);
                if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
                    tokens.remove(0); // consume (
                    let mut params = Vec::new();
                    if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
                        loop {
                            if let Some(Token::Identifier(param)) = tokens.first().cloned() {
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
                    return Ok(Statement::Let(name, Some(Expr::AsyncFunction(params, body))));
                }
            }
        }
        return Err(JSError::ParseError);
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Function) {
        tokens.remove(0); // consume function
        if let Some(Token::Identifier(name)) = tokens.first().cloned() {
            tokens.remove(0);
            if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
                tokens.remove(0); // consume (
                let mut params = Vec::new();
                if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
                    loop {
                        if let Some(Token::Identifier(param)) = tokens.first().cloned() {
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
                return Ok(Statement::Let(name, Some(Expr::Function(params, body))));
            }
        }
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::If) {
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
    if !tokens.is_empty() && matches!(tokens[0], Token::Try) {
        tokens.remove(0); // consume try
        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume {
        let try_body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume }

        // Parse optional catch
        let mut catch_param = String::new();
        let mut catch_body: Vec<Statement> = Vec::new();
        let mut finally_body: Option<Vec<Statement>> = None;

        if !tokens.is_empty() && matches!(tokens[0], Token::Catch) {
            tokens.remove(0); // consume catch
            if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume (
            if tokens.is_empty() {
                return Err(JSError::ParseError);
            }
            if let Token::Identifier(name) = tokens.remove(0) {
                catch_param = name;
            } else {
                return Err(JSError::ParseError);
            }
            if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume )
            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume {
            catch_body = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume }
        }

        // Optional finally
        if !tokens.is_empty() && matches!(tokens[0], Token::Finally) {
            tokens.remove(0); // consume finally
            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume {
            let fb = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume }
            finally_body = Some(fb);
        }

        return Ok(Statement::TryCatch(try_body, catch_param, catch_body, finally_body));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::For) {
        tokens.remove(0); // consume for
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume (

        // Check if this is a for-of loop
        if !tokens.is_empty() && (matches!(tokens[0], Token::Let) || matches!(tokens[0], Token::Var) || matches!(tokens[0], Token::Const)) {
            let saved_declaration_token = tokens[0].clone();
            tokens.remove(0); // consume let/var/const
            if let Some(Token::Identifier(var_name)) = tokens.first().cloned() {
                let saved_identifier_token = tokens[0].clone();
                tokens.remove(0);
                if !tokens.is_empty() && matches!(tokens[0], Token::Identifier(ref s) if s == "of") {
                    // This is a for-of loop
                    tokens.remove(0); // consume of
                    let iterable = parse_expression(tokens)?;
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
                    return Ok(Statement::ForOf(var_name, iterable, body));
                } else {
                    // This is a regular for loop with variable declaration, put tokens back
                    tokens.insert(0, saved_identifier_token);
                    tokens.insert(0, saved_declaration_token);
                }
            } else {
                // Not an identifier, put back the declaration token
                tokens.insert(0, saved_declaration_token);
            }
        }

        // Parse initialization (regular for loop)
        let init = if !tokens.is_empty() && (matches!(tokens[0], Token::Let) || matches!(tokens[0], Token::Var)) {
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
    if !tokens.is_empty() && matches!(tokens[0], Token::Return) {
        tokens.remove(0); // consume return
        if tokens.is_empty() || matches!(tokens[0], Token::Semicolon) {
            return Ok(Statement::Return(None));
        }
        let expr = parse_expression(tokens)?;
        return Ok(Statement::Return(Some(expr)));
    }
    if !tokens.is_empty() && (matches!(tokens[0], Token::Let) || matches!(tokens[0], Token::Var) || matches!(tokens[0], Token::Const)) {
        let is_const = matches!(tokens[0], Token::Const);
        tokens.remove(0); // consume let/var/const

        // Check for destructuring
        if !tokens.is_empty() && matches!(tokens[0], Token::LBracket) {
            // Array destructuring
            let pattern = parse_array_destructuring_pattern(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::Assign) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume =
            let expr = parse_expression(tokens)?;
            if is_const {
                return Ok(Statement::ConstDestructuringArray(pattern, expr));
            } else {
                return Ok(Statement::LetDestructuringArray(pattern, expr));
            }
        } else if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
            // Object destructuring
            let pattern = parse_object_destructuring_pattern(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::Assign) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume =
            let expr = parse_expression(tokens)?;
            if is_const {
                return Ok(Statement::ConstDestructuringObject(pattern, expr));
            } else {
                return Ok(Statement::LetDestructuringObject(pattern, expr));
            }
        } else {
            // Regular variable declaration
            if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                tokens.remove(0);
                if !tokens.is_empty() && matches!(tokens[0], Token::Assign) {
                    tokens.remove(0);
                    let expr = parse_expression(tokens)?;
                    if is_const {
                        return Ok(Statement::Const(name, expr));
                    } else {
                        return Ok(Statement::Let(name, Some(expr)));
                    }
                } else if !is_const {
                    return Ok(Statement::Let(name, None));
                }
            }
        }
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Class) {
        tokens.remove(0); // consume class
        if let Some(Token::Identifier(name)) = tokens.first().cloned() {
            tokens.remove(0);
            let extends = if !tokens.is_empty() && matches!(tokens[0], Token::Extends) {
                tokens.remove(0); // consume extends
                if let Some(Token::Identifier(parent_name)) = tokens.first().cloned() {
                    tokens.remove(0);
                    Some(parent_name)
                } else {
                    return Err(JSError::ParseError);
                }
            } else {
                None
            };

            // Parse class body
            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume {

            let mut members = Vec::new();
            while !tokens.is_empty() && !matches!(tokens[0], Token::RBrace) {
                let is_static = if !tokens.is_empty() && matches!(tokens[0], Token::Static) {
                    tokens.remove(0);
                    true
                } else {
                    false
                };

                if let Some(Token::Identifier(method_name)) = tokens.first() {
                    let method_name = method_name.clone();
                    if method_name == "constructor" {
                        tokens.remove(0);
                        // Parse constructor
                        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
                            return Err(JSError::ParseError);
                        }
                        tokens.remove(0); // consume (
                        let params = parse_parameters(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                            return Err(JSError::ParseError);
                        }
                        tokens.remove(0); // consume {
                        let body = parse_statement_block(tokens)?;
                        members.push(ClassMember::Constructor(params, body));
                    } else {
                        tokens.remove(0);
                        if tokens.is_empty() {
                            return Err(JSError::ParseError);
                        }
                        // Check for getter/setter
                        let is_getter = matches!(tokens[0], Token::Identifier(ref id) if id == "get");
                        let is_setter = matches!(tokens[0], Token::Identifier(ref id) if id == "set");
                        if is_getter || is_setter {
                            tokens.remove(0); // consume get/set
                            if tokens.is_empty() || !matches!(tokens[0], Token::Identifier(_)) {
                                return Err(JSError::ParseError);
                            }
                            let prop_name = if let Token::Identifier(name) = tokens.remove(0) {
                                name
                            } else {
                                return Err(JSError::ParseError);
                            };
                            if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume (
                            let params = parse_parameters(tokens)?;
                            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume {
                            let body = parse_statement_block(tokens)?;
                            if is_getter {
                                if !params.is_empty() {
                                    return Err(JSError::ParseError); // getters should have no parameters
                                }
                                if is_static {
                                    members.push(ClassMember::StaticGetter(prop_name, body));
                                } else {
                                    members.push(ClassMember::Getter(prop_name, body));
                                }
                            } else {
                                // setter
                                if params.len() != 1 {
                                    return Err(JSError::ParseError); // setters should have exactly one parameter
                                }
                                if is_static {
                                    members.push(ClassMember::StaticSetter(prop_name, params, body));
                                } else {
                                    members.push(ClassMember::Setter(prop_name, params, body));
                                }
                            }
                        } else if matches!(tokens[0], Token::LParen) {
                            // This is a method
                            tokens.remove(0); // consume (
                            let params = parse_parameters(tokens)?;
                            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume {
                            let body = parse_statement_block(tokens)?;
                            if is_static {
                                members.push(ClassMember::StaticMethod(method_name, params, body));
                            } else {
                                members.push(ClassMember::Method(method_name, params, body));
                            }
                        } else if matches!(tokens[0], Token::Assign) {
                            // This is a property
                            tokens.remove(0); // consume =
                            let value = parse_expression(tokens)?;
                            if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume ;
                            if is_static {
                                members.push(ClassMember::StaticProperty(method_name, value));
                            } else {
                                members.push(ClassMember::Property(method_name, value));
                            }
                        } else {
                            return Err(JSError::ParseError);
                        }
                    }
                } else {
                    return Err(JSError::ParseError);
                }
            }

            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume }

            return Ok(Statement::Class(name, extends, members));
        }
    }
    let expr = parse_expression(tokens)?;
    // Check if this is an assignment expression
    if let Expr::Assign(target, value) = &expr
        && let Expr::Var(name) = target.as_ref()
    {
        return Ok(Statement::Assign(name.clone(), *value.clone()));
    }
    Ok(Statement::Expr(expr))
}

#[derive(Clone, Debug)]
pub enum ControlFlow {
    Normal(Value),
    Break,
    Continue,
    Return(Value),
}

pub fn evaluate_statements(env: &JSObjectDataPtr, statements: &[Statement]) -> Result<Value, JSError> {
    match evaluate_statements_with_context(env, statements)? {
        ControlFlow::Normal(val) => Ok(val),
        ControlFlow::Break => Err(JSError::EvaluationError {
            message: "break statement not in loop or switch".to_string(),
        }),
        ControlFlow::Continue => Err(JSError::EvaluationError {
            message: "continue statement not in loop".to_string(),
        }),
        ControlFlow::Return(val) => Ok(val),
    }
}

fn evaluate_statements_with_context(env: &JSObjectDataPtr, statements: &[Statement]) -> Result<ControlFlow, JSError> {
    let mut last_value = Value::Number(0.0);
    for (i, stmt) in statements.iter().enumerate() {
        log::trace!("Evaluating statement {i}: {stmt:?}");
        match stmt {
            Statement::Let(name, expr_opt) => {
                let val = expr_opt.clone().map_or(Ok(Value::Undefined), |expr| evaluate_expr(env, &expr))?;
                env_set(env, name.as_str(), val.clone())?;
                last_value = val;
            }
            Statement::Const(name, expr) => {
                let val = evaluate_expr(env, expr)?;
                env_set_const(env, name.as_str(), val.clone());
                last_value = val;
            }
            Statement::Class(name, extends, members) => {
                let class_obj = create_class_object(name, extends, members, env)?;
                env_set(env, name.as_str(), class_obj)?;
                last_value = Value::Undefined;
            }
            Statement::Assign(name, expr) => {
                let val = evaluate_expr(env, expr)?;
                env_set(env, name.as_str(), val.clone())?;
                last_value = val;
            }
            Statement::Expr(expr) => {
                // Special-case assignment expressions so we can mutate `env` or
                // object properties. `parse_statement` only turns simple
                // variable assignments into `Statement::Assign`, so here we
                // handle expression-level assignments such as `obj.prop = val`
                // and `arr[0] = val`.
                if let Expr::Assign(target, value_expr) = expr {
                    match target.as_ref() {
                        Expr::Var(name) => {
                            let v = evaluate_expr(env, value_expr)?;
                            env_set(env, name.as_str(), v.clone())?;
                            last_value = v;
                        }
                        Expr::Property(obj_expr, prop_name) => {
                            let v = evaluate_expr(env, value_expr)?;
                            // set_prop_env will attempt to mutate the env-held
                            // object when possible, otherwise it will update
                            // the evaluated object and return it.
                            match set_prop_env(env, obj_expr, prop_name.as_str(), v.clone())? {
                                Some(updated_obj) => last_value = updated_obj,
                                None => last_value = v,
                            }
                        }
                        Expr::Index(obj_expr, idx_expr) => {
                            // Evaluate index to a string key
                            let idx_val = evaluate_expr(env, idx_expr)?;
                            let key = match idx_val {
                                Value::Number(n) => n.to_string(),
                                Value::String(s) => String::from_utf16_lossy(&s),
                                _ => {
                                    return Err(JSError::EvaluationError {
                                        message: "Invalid index type".to_string(),
                                    });
                                }
                            };
                            let v = evaluate_expr(env, value_expr)?;
                            match set_prop_env(env, obj_expr, &key, v.clone())? {
                                Some(updated_obj) => last_value = updated_obj,
                                None => last_value = v,
                            }
                        }
                        _ => {
                            // Fallback: evaluate the expression normally
                            last_value = evaluate_expr(env, expr)?;
                        }
                    }
                } else if let Expr::LogicalAndAssign(target, value_expr) = expr {
                    // Handle logical AND assignment: a &&= b
                    let left_val = evaluate_expr(env, target)?;
                    if is_truthy(&left_val) {
                        match target.as_ref() {
                            Expr::Var(name) => {
                                let v = evaluate_expr(env, value_expr)?;
                                env_set(env, name.as_str(), v.clone())?;
                                last_value = v;
                            }
                            Expr::Property(obj_expr, prop_name) => {
                                let v = evaluate_expr(env, value_expr)?;
                                match set_prop_env(env, obj_expr, prop_name.as_str(), v.clone())? {
                                    Some(updated_obj) => last_value = updated_obj,
                                    None => last_value = v,
                                }
                            }
                            Expr::Index(obj_expr, idx_expr) => {
                                let idx_val = evaluate_expr(env, idx_expr)?;
                                let key = match idx_val {
                                    Value::Number(n) => n.to_string(),
                                    Value::String(s) => String::from_utf16_lossy(&s),
                                    _ => {
                                        return Err(JSError::EvaluationError {
                                            message: "Invalid index type".to_string(),
                                        });
                                    }
                                };
                                let v = evaluate_expr(env, value_expr)?;
                                match set_prop_env(env, obj_expr, &key, v.clone())? {
                                    Some(updated_obj) => last_value = updated_obj,
                                    None => last_value = v,
                                }
                            }
                            _ => {
                                last_value = evaluate_expr(env, expr)?;
                            }
                        }
                    } else {
                        last_value = left_val;
                    }
                } else if let Expr::LogicalOrAssign(target, value_expr) = expr {
                    // Handle logical OR assignment: a ||= b
                    let left_val = evaluate_expr(env, target)?;
                    if !is_truthy(&left_val) {
                        match target.as_ref() {
                            Expr::Var(name) => {
                                let v = evaluate_expr(env, value_expr)?;
                                env_set(env, name.as_str(), v.clone())?;
                                last_value = v;
                            }
                            Expr::Property(obj_expr, prop_name) => {
                                let v = evaluate_expr(env, value_expr)?;
                                match set_prop_env(env, obj_expr, prop_name.as_str(), v.clone())? {
                                    Some(updated_obj) => last_value = updated_obj,
                                    None => last_value = v,
                                }
                            }
                            Expr::Index(obj_expr, idx_expr) => {
                                let idx_val = evaluate_expr(env, idx_expr)?;
                                let key = match idx_val {
                                    Value::Number(n) => n.to_string(),
                                    Value::String(s) => String::from_utf16_lossy(&s),
                                    _ => {
                                        return Err(JSError::EvaluationError {
                                            message: "Invalid index type".to_string(),
                                        });
                                    }
                                };
                                let v = evaluate_expr(env, value_expr)?;
                                match set_prop_env(env, obj_expr, &key, v.clone())? {
                                    Some(updated_obj) => last_value = updated_obj,
                                    None => last_value = v,
                                }
                            }
                            _ => {
                                last_value = evaluate_expr(env, expr)?;
                            }
                        }
                    } else {
                        last_value = left_val;
                    }
                } else if let Expr::NullishAssign(target, value_expr) = expr {
                    // Handle nullish coalescing assignment: a ??= b
                    let left_val = evaluate_expr(env, target)?;
                    match left_val {
                        Value::Undefined => match target.as_ref() {
                            Expr::Var(name) => {
                                let v = evaluate_expr(env, value_expr)?;
                                env_set(env, name.as_str(), v.clone())?;
                                last_value = v;
                            }
                            Expr::Property(obj_expr, prop_name) => {
                                let v = evaluate_expr(env, value_expr)?;
                                match set_prop_env(env, obj_expr, prop_name.as_str(), v.clone())? {
                                    Some(updated_obj) => last_value = updated_obj,
                                    None => last_value = v,
                                }
                            }
                            Expr::Index(obj_expr, idx_expr) => {
                                let idx_val = evaluate_expr(env, idx_expr)?;
                                let key = match idx_val {
                                    Value::Number(n) => n.to_string(),
                                    Value::String(s) => String::from_utf16_lossy(&s),
                                    _ => {
                                        return Err(JSError::EvaluationError {
                                            message: "Invalid index type".to_string(),
                                        });
                                    }
                                };
                                let v = evaluate_expr(env, value_expr)?;
                                match set_prop_env(env, obj_expr, &key, v.clone())? {
                                    Some(updated_obj) => last_value = updated_obj,
                                    None => last_value = v,
                                }
                            }
                            _ => {
                                last_value = evaluate_expr(env, expr)?;
                            }
                        },
                        _ => {
                            last_value = left_val;
                        }
                    }
                } else {
                    last_value = evaluate_expr(env, expr)?;
                }
            }
            Statement::Return(expr_opt) => {
                let return_val = match expr_opt {
                    Some(expr) => evaluate_expr(env, expr)?,
                    None => Value::Undefined,
                };
                return Ok(ControlFlow::Return(return_val));
            }
            Statement::Throw(expr) => {
                let throw_val = evaluate_expr(env, expr)?;
                return Err(JSError::Throw { value: throw_val });
            }
            Statement::If(condition, then_body, else_body) => {
                let cond_val = evaluate_expr(env, condition)?;
                if is_truthy(&cond_val) {
                    match evaluate_statements_with_context(env, then_body)? {
                        ControlFlow::Normal(val) => last_value = val,
                        cf => return Ok(cf),
                    }
                } else if let Some(else_stmts) = else_body {
                    match evaluate_statements_with_context(env, else_stmts)? {
                        ControlFlow::Normal(val) => last_value = val,
                        cf => return Ok(cf),
                    }
                }
            }
            Statement::TryCatch(try_body, catch_param, catch_body, finally_body_opt) => {
                // Execute try block and handle catch/finally semantics
                match evaluate_statements_with_context(env, try_body) {
                    Ok(ControlFlow::Normal(v)) => last_value = v,
                    Ok(cf) => {
                        // Handle control flow in try block
                        match cf {
                            ControlFlow::Return(val) => return Ok(ControlFlow::Return(val)),
                            ControlFlow::Break => return Ok(ControlFlow::Break),
                            ControlFlow::Continue => return Ok(ControlFlow::Continue),
                            _ => unreachable!(),
                        }
                    }
                    Err(err) => {
                        if catch_param.is_empty() {
                            // No catch: run finally if present then propagate error
                            if let Some(finally_body) = finally_body_opt {
                                evaluate_statements_with_context(env, finally_body)?;
                            }
                            return Err(err);
                        } else {
                            let catch_env = env.clone();
                            let catch_value = match &err {
                                JSError::Throw { value } => value.clone(),
                                _ => Value::String(utf8_to_utf16(&err.to_string())),
                            };
                            env_set(&catch_env, catch_param.as_str(), catch_value)?;
                            match evaluate_statements_with_context(&catch_env, catch_body)? {
                                ControlFlow::Normal(val) => last_value = val,
                                cf => {
                                    // Finally block executes after try/catch
                                    if let Some(finally_body) = finally_body_opt {
                                        evaluate_statements_with_context(env, finally_body)?;
                                    }
                                    return Ok(cf);
                                }
                            }
                        }
                    }
                }
                // Finally block executes after try/catch
                if let Some(finally_body) = finally_body_opt {
                    match evaluate_statements_with_context(env, finally_body)? {
                        ControlFlow::Normal(val) => last_value = val,
                        cf => return Ok(cf),
                    }
                }
            }
            Statement::For(init, condition, increment, body) => {
                // Execute initialization
                if let Some(init_stmt) = init {
                    match init_stmt.as_ref() {
                        Statement::Let(name, expr_opt) => {
                            let val = expr_opt.clone().map_or(Ok(Value::Undefined), |expr| evaluate_expr(env, &expr))?;
                            env_set(env, name.as_str(), val)?;
                        }
                        Statement::Expr(expr) => {
                            evaluate_expr(env, expr)?;
                        }
                        _ => {
                            return Err(JSError::EvaluationError {
                                message: "error".to_string(),
                            });
                        } // For now, only support let and expr in init
                    }
                }

                loop {
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
                    match evaluate_statements_with_context(env, body)? {
                        ControlFlow::Normal(val) => last_value = val,
                        ControlFlow::Break => break,
                        ControlFlow::Continue => {}
                        ControlFlow::Return(val) => return Ok(ControlFlow::Return(val)),
                    }

                    // Execute increment
                    if let Some(incr_stmt) = increment {
                        match incr_stmt.as_ref() {
                            Statement::Expr(expr) => match expr {
                                Expr::Assign(target, value) => {
                                    if let Expr::Var(name) = target.as_ref() {
                                        let val = evaluate_expr(env, value)?;
                                        env_set(env, name.as_str(), val)?;
                                    }
                                }
                                _ => {
                                    evaluate_expr(env, expr)?;
                                }
                            },
                            _ => {
                                return Err(JSError::EvaluationError {
                                    message: "error".to_string(),
                                });
                            } // For now, only support expr in increment
                        }
                    }
                }
            }
            Statement::ForOf(var, iterable, body) => {
                let iterable_val = evaluate_expr(env, iterable)?;
                match iterable_val {
                    Value::Object(obj_map) => {
                        if is_array(&obj_map) {
                            let len = get_array_length(&obj_map).unwrap_or(0);
                            for i in 0..len {
                                let key = i.to_string();
                                if let Some(element_rc) = obj_get_value(&obj_map, &key)? {
                                    let element = element_rc.borrow().clone();
                                    env_set(env, var.as_str(), element)?;
                                    match evaluate_statements_with_context(env, body)? {
                                        ControlFlow::Normal(val) => last_value = val,
                                        ControlFlow::Break => break,
                                        ControlFlow::Continue => {}
                                        ControlFlow::Return(val) => return Ok(ControlFlow::Return(val)),
                                    }
                                }
                            }
                        } else {
                            return Err(JSError::EvaluationError {
                                message: "for-of loop requires an iterable".to_string(),
                            });
                        }
                    }
                    _ => {
                        return Err(JSError::EvaluationError {
                            message: "for-of loop requires an iterable".to_string(),
                        });
                    }
                }
            }
            Statement::While(condition, body) => {
                loop {
                    // Check condition
                    let cond_val = evaluate_expr(env, condition)?;
                    if !is_truthy(&cond_val) {
                        break;
                    }

                    // Execute body
                    match evaluate_statements_with_context(env, body)? {
                        ControlFlow::Normal(val) => last_value = val,
                        ControlFlow::Break => break,
                        ControlFlow::Continue => {}
                        ControlFlow::Return(val) => return Ok(ControlFlow::Return(val)),
                    }
                }
            }
            Statement::DoWhile(body, condition) => {
                loop {
                    // Execute body first
                    match evaluate_statements_with_context(env, body)? {
                        ControlFlow::Normal(val) => last_value = val,
                        ControlFlow::Break => break,
                        ControlFlow::Continue => {}
                        ControlFlow::Return(val) => return Ok(ControlFlow::Return(val)),
                    }

                    // Check condition
                    let cond_val = evaluate_expr(env, condition)?;
                    if !is_truthy(&cond_val) {
                        break;
                    }
                }
            }
            Statement::Switch(expr, cases) => {
                let switch_val = evaluate_expr(env, expr)?;
                let mut found_match = false;
                let mut executed_default = false;

                for case in cases {
                    match case {
                        SwitchCase::Case(case_expr, case_stmts) => {
                            if !found_match {
                                let case_val = evaluate_expr(env, case_expr)?;
                                // Simple equality check for switch cases
                                if values_equal(&switch_val, &case_val) {
                                    found_match = true;
                                }
                            }
                            if found_match {
                                match evaluate_statements_with_context(env, case_stmts)? {
                                    ControlFlow::Normal(val) => last_value = val,
                                    ControlFlow::Break => break,
                                    cf => return Ok(cf),
                                }
                            }
                        }
                        SwitchCase::Default(default_stmts) => {
                            if !found_match && !executed_default {
                                executed_default = true;
                                match evaluate_statements_with_context(env, default_stmts)? {
                                    ControlFlow::Normal(val) => last_value = val,
                                    ControlFlow::Break => break,
                                    cf => return Ok(cf),
                                }
                            } else if found_match {
                                // Default case also falls through if a match was found before it
                                match evaluate_statements_with_context(env, default_stmts)? {
                                    ControlFlow::Normal(val) => last_value = val,
                                    ControlFlow::Break => break,
                                    cf => return Ok(cf),
                                }
                            }
                        }
                    }
                }
            }
            Statement::Break => {
                return Ok(ControlFlow::Break);
            }
            Statement::Continue => {
                return Ok(ControlFlow::Continue);
            }
            Statement::LetDestructuringArray(pattern, expr) => {
                let val = evaluate_expr(env, expr)?;
                perform_array_destructuring(env, pattern, &val, false)?;
                last_value = val;
            }
            Statement::ConstDestructuringArray(pattern, expr) => {
                let val = evaluate_expr(env, expr)?;
                perform_array_destructuring(env, pattern, &val, true)?;
                last_value = val;
            }
            Statement::LetDestructuringObject(pattern, expr) => {
                let val = evaluate_expr(env, expr)?;
                perform_object_destructuring(env, pattern, &val, false)?;
                last_value = val;
            }
            Statement::ConstDestructuringObject(pattern, expr) => {
                let val = evaluate_expr(env, expr)?;
                perform_object_destructuring(env, pattern, &val, true)?;
                last_value = val;
            }
        }
    }
    Ok(ControlFlow::Normal(last_value))
}

fn perform_array_destructuring(
    env: &JSObjectDataPtr,
    pattern: &Vec<DestructuringElement>,
    value: &Value,
    is_const: bool,
) -> Result<(), JSError> {
    match value {
        Value::Object(arr) if is_array(arr) => {
            let mut index = 0;
            let mut rest_index = None;
            let mut rest_var = None;

            for element in pattern {
                match element {
                    DestructuringElement::Variable(var) => {
                        let key = index.to_string();
                        let val = if let Some(val_rc) = obj_get_value(arr, &key)? {
                            val_rc.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        if is_const {
                            env_set_const(env, var, val);
                        } else {
                            env_set(env, var, val)?;
                        }
                        index += 1;
                    }
                    DestructuringElement::NestedArray(nested_pattern) => {
                        let key = index.to_string();
                        let val = if let Some(val_rc) = obj_get_value(arr, &key)? {
                            val_rc.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        perform_array_destructuring(env, nested_pattern, &val, is_const)?;
                        index += 1;
                    }
                    DestructuringElement::NestedObject(nested_pattern) => {
                        let key = index.to_string();
                        let val = if let Some(val_rc) = obj_get_value(arr, &key)? {
                            val_rc.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        perform_object_destructuring(env, nested_pattern, &val, is_const)?;
                        index += 1;
                    }
                    DestructuringElement::Rest(var) => {
                        rest_index = Some(index);
                        rest_var = Some(var.clone());
                        break;
                    }
                    DestructuringElement::Empty => {
                        index += 1;
                    }
                }
            }

            // Handle rest element
            if let (Some(rest_start), Some(var)) = (rest_index, rest_var) {
                let mut rest_elements: Vec<Value> = Vec::new();
                let len = get_array_length(arr).unwrap_or(0);
                for i in rest_start..len {
                    let key = i.to_string();
                    if let Some(val_rc) = obj_get_value(arr, &key)? {
                        rest_elements.push(val_rc.borrow().clone());
                    }
                }
                let rest_obj = Rc::new(RefCell::new(JSObjectData::new()));
                let mut rest_index = 0;
                for elem in rest_elements {
                    obj_set_value(&rest_obj, rest_index.to_string(), elem)?;
                    rest_index += 1;
                }
                set_array_length(&rest_obj, rest_index)?;
                let rest_value = Value::Object(rest_obj);
                if is_const {
                    env_set_const(env, &var, rest_value);
                } else {
                    env_set(env, &var, rest_value)?;
                }
            }
        }
        _ => {
            return Err(JSError::EvaluationError {
                message: "Cannot destructure non-array value".to_string(),
            });
        }
    }
    Ok(())
}

fn perform_object_destructuring(
    env: &JSObjectDataPtr,
    pattern: &Vec<ObjectDestructuringElement>,
    value: &Value,
    is_const: bool,
) -> Result<(), JSError> {
    match value {
        Value::Object(obj) => {
            for element in pattern {
                match element {
                    ObjectDestructuringElement::Property { key, value: dest } => {
                        let prop_val = if let Some(val_rc) = obj_get_value(obj, key)? {
                            val_rc.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        match dest {
                            DestructuringElement::Variable(var) => {
                                if is_const {
                                    env_set_const(env, var, prop_val);
                                } else {
                                    env_set(env, var, prop_val)?;
                                }
                            }
                            DestructuringElement::NestedArray(nested_pattern) => {
                                perform_array_destructuring(env, nested_pattern, &prop_val, is_const)?;
                            }
                            DestructuringElement::NestedObject(nested_pattern) => {
                                perform_object_destructuring(env, nested_pattern, &prop_val, is_const)?;
                            }
                            _ => {
                                // Rest in property value not supported in object destructuring
                                return Err(JSError::EvaluationError {
                                    message: "Invalid destructuring pattern".to_string(),
                                });
                            }
                        }
                    }
                    ObjectDestructuringElement::Rest(var) => {
                        // Collect remaining properties
                        let rest_obj = Rc::new(RefCell::new(JSObjectData::new()));
                        let mut assigned_keys = std::collections::HashSet::new();

                        // Collect keys that were already assigned
                        for element in pattern {
                            if let ObjectDestructuringElement::Property { key, .. } = element {
                                assigned_keys.insert(key.clone());
                            }
                        }

                        // Add remaining properties to rest object
                        for (key, val_rc) in obj.borrow().properties.iter() {
                            if !assigned_keys.contains(key) {
                                rest_obj.borrow_mut().insert(key.clone(), val_rc.clone());
                            }
                        }

                        let rest_value = Value::Object(rest_obj);
                        if is_const {
                            env_set_const(env, var, rest_value);
                        } else {
                            env_set(env, var, rest_value)?;
                        }
                    }
                }
            }
        }
        _ => {
            return Err(JSError::EvaluationError {
                message: "Cannot destructure non-object value".to_string(),
            });
        }
    }
    Ok(())
}

pub fn evaluate_expr(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    match expr {
        Expr::Number(n) => evaluate_number(*n),
        Expr::StringLit(s) => evaluate_string_lit(s),
        Expr::Boolean(b) => evaluate_boolean(*b),
        Expr::Var(name) => evaluate_var(env, name),
        Expr::Assign(_target, value) => evaluate_assign(env, value),
        Expr::LogicalAndAssign(target, value) => evaluate_logical_and_assign(env, target, value),
        Expr::LogicalOrAssign(target, value) => evaluate_logical_or_assign(env, target, value),
        Expr::NullishAssign(target, value) => evaluate_nullish_assign(env, target, value),
        Expr::AddAssign(target, value) => evaluate_add_assign(env, target, value),
        Expr::SubAssign(target, value) => evaluate_sub_assign(env, target, value),
        Expr::MulAssign(target, value) => evaluate_mul_assign(env, target, value),
        Expr::DivAssign(target, value) => evaluate_div_assign(env, target, value),
        Expr::ModAssign(target, value) => evaluate_mod_assign(env, target, value),
        Expr::Increment(expr) => evaluate_increment(env, expr),
        Expr::Decrement(expr) => evaluate_decrement(env, expr),
        Expr::PostIncrement(expr) => evaluate_post_increment(env, expr),
        Expr::PostDecrement(expr) => evaluate_post_decrement(env, expr),
        Expr::UnaryNeg(expr) => evaluate_unary_neg(env, expr),
        Expr::TypeOf(expr) => evaluate_typeof(env, expr),
        Expr::Delete(expr) => evaluate_delete(env, expr),
        Expr::Void(expr) => evaluate_void(env, expr),
        Expr::Binary(left, op, right) => evaluate_binary(env, left, op, right),
        Expr::Index(obj, idx) => evaluate_index(env, obj, idx),
        Expr::Property(obj, prop) => evaluate_property(env, obj, prop),
        Expr::Call(func_expr, args) => evaluate_call(env, func_expr, args),
        Expr::Function(params, body) => Ok(Value::Closure(params.clone(), body.clone(), env.clone())),
        Expr::ArrowFunction(params, body) => Ok(Value::Closure(params.clone(), body.clone(), env.clone())),
        Expr::Object(properties) => evaluate_object(env, properties),
        Expr::Array(elements) => evaluate_array(env, elements),
        Expr::Getter(func_expr) => evaluate_expr(env, func_expr),
        Expr::Setter(func_expr) => evaluate_expr(env, func_expr),
        Expr::Spread(_expr) => Err(JSError::EvaluationError {
            message: "Spread operator must be used in array, object, or function call context".to_string(),
        }),
        Expr::OptionalProperty(obj, prop) => evaluate_optional_property(env, obj, prop),
        Expr::OptionalCall(func_expr, args) => evaluate_optional_call(env, func_expr, args),
        Expr::This => evaluate_this(env),
        Expr::New(constructor, args) => evaluate_new(env, constructor, args),
        Expr::Super => evaluate_super(env),
        Expr::SuperCall(args) => evaluate_super_call(env, args),
        Expr::SuperProperty(prop) => evaluate_super_property(env, prop),
        Expr::SuperMethod(method, args) => evaluate_super_method(env, method, args),
        Expr::ArrayDestructuring(pattern) => evaluate_array_destructuring(env, pattern),
        Expr::ObjectDestructuring(pattern) => evaluate_object_destructuring(env, pattern),
        Expr::AsyncFunction(params, body) => Ok(Value::Closure(params.clone(), body.clone(), env.clone())),
        Expr::Await(expr) => {
            let promise_val = evaluate_expr(env, expr)?;
            match promise_val {
                Value::Promise(promise) => {
                    // Wait for the promise to resolve by running the event loop
                    loop {
                        run_event_loop()?;
                        let promise_borrow = promise.borrow();
                        match &promise_borrow.state {
                            PromiseState::Fulfilled(val) => return Ok(val.clone()),
                            PromiseState::Rejected(reason) => {
                                return Err(JSError::EvaluationError {
                                    message: format!("Promise rejected: {}", value_to_string(reason)),
                                });
                            }
                            PromiseState::Pending => {
                                // Continue running the event loop
                            }
                        }
                    }
                }
                Value::Object(obj) => {
                    // Check if this is a Promise object with __promise property
                    if let Some(promise_rc) = obj_get_value(&obj, "__promise")?
                        && let Value::Promise(promise) = promise_rc.borrow().clone()
                    {
                        // Wait for the promise to resolve by running the event loop
                        loop {
                            run_event_loop()?;
                            let promise_borrow = promise.borrow();
                            match &promise_borrow.state {
                                PromiseState::Fulfilled(val) => return Ok(val.clone()),
                                PromiseState::Rejected(reason) => {
                                    return Err(JSError::EvaluationError {
                                        message: format!("Promise rejected: {}", value_to_string(reason)),
                                    });
                                }
                                PromiseState::Pending => {
                                    // Continue running the event loop
                                }
                            }
                        }
                    }
                    Err(JSError::EvaluationError {
                        message: "await can only be used with promises".to_string(),
                    })
                }
                _ => Err(JSError::EvaluationError {
                    message: "await can only be used with promises".to_string(),
                }),
            }
        }
        Expr::Value(value) => Ok(value.clone()),
    }
}

fn evaluate_number(n: f64) -> Result<Value, JSError> {
    Ok(Value::Number(n))
}

fn evaluate_string_lit(s: &[u16]) -> Result<Value, JSError> {
    Ok(Value::String(s.to_vec()))
}

fn evaluate_boolean(b: bool) -> Result<Value, JSError> {
    Ok(Value::Boolean(b))
}

fn evaluate_var(env: &JSObjectDataPtr, name: &str) -> Result<Value, JSError> {
    if let Some(val) = env_get(env, name) {
        Ok(val.borrow().clone())
    } else if name == "console" {
        Ok(Value::Object(js_console::make_console_object()?))
    } else if name == "String" {
        Ok(Value::Function("String".to_string()))
    } else if name == "Math" {
        Ok(Value::Object(js_math::make_math_object()?))
    } else if name == "JSON" {
        let json_obj = Rc::new(RefCell::new(JSObjectData::new()));
        obj_set_value(&json_obj, "parse", Value::Function("JSON.parse".to_string()))?;
        obj_set_value(&json_obj, "stringify", Value::Function("JSON.stringify".to_string()))?;
        Ok(Value::Object(json_obj))
    } else if name == "Object" {
        // Return Object constructor function, not an object with methods
        Ok(Value::Function("Object".to_string()))
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
        Ok(Value::Object(js_number::make_number_object()?))
    } else if name == "Boolean" {
        Ok(Value::Function("Boolean".to_string()))
    } else if name == "Date" {
        Ok(Value::Function("Date".to_string()))
    } else if name == "RegExp" {
        Ok(Value::Function("RegExp".to_string()))
    } else if name == "Promise" {
        Ok(Value::Function("Promise".to_string()))
    } else if name == "new" {
        Ok(Value::Function("new".to_string()))
    } else if name == "__internal_resolve_promise" {
        Ok(Value::Function("__internal_resolve_promise".to_string()))
    } else if name == "__internal_reject_promise" {
        Ok(Value::Function("__internal_reject_promise".to_string()))
    } else if name == "__internal_promise_allsettled_resolve" {
        Ok(Value::Function("__internal_promise_allsettled_resolve".to_string()))
    } else if name == "__internal_promise_allsettled_reject" {
        Ok(Value::Function("__internal_promise_allsettled_reject".to_string()))
    } else if name == "NaN" {
        Ok(Value::Number(f64::NAN))
    } else if name == "Infinity" {
        Ok(Value::Number(f64::INFINITY))
    } else {
        Ok(Value::Undefined)
    }
}

fn evaluate_assign(env: &JSObjectDataPtr, value: &Expr) -> Result<Value, JSError> {
    // Assignment is handled at statement level, just evaluate the value
    evaluate_expr(env, value)
}

fn evaluate_logical_and_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a &&= b is equivalent to a && (a = b)
    let left_val = evaluate_expr(env, target)?;
    if is_truthy(&left_val) {
        // Evaluate the assignment
        evaluate_assignment_expr(env, target, value)
    } else {
        // Return the left value without assignment
        Ok(left_val)
    }
}

fn evaluate_logical_or_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a ||= b is equivalent to a || (a = b)
    let left_val = evaluate_expr(env, target)?;
    if !is_truthy(&left_val) {
        // Evaluate the assignment
        evaluate_assignment_expr(env, target, value)
    } else {
        // Return the left value without assignment
        Ok(left_val)
    }
}

fn evaluate_nullish_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a ??= b is equivalent to a ?? (a = b)
    let left_val = evaluate_expr(env, target)?;
    match left_val {
        Value::Undefined => {
            // Evaluate the assignment
            evaluate_assignment_expr(env, target, value)
        }
        _ => {
            // Return the left value without assignment
            Ok(left_val)
        }
    }
}

fn evaluate_add_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a += b is equivalent to a = a + b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => Value::Number(ln + rn),
        (Value::String(ls), Value::String(rs)) => {
            let mut result = ls.clone();
            result.extend_from_slice(&rs);
            Value::String(result)
        }
        (Value::Number(ln), Value::String(rs)) => {
            let mut result = utf8_to_utf16(&ln.to_string());
            result.extend_from_slice(&rs);
            Value::String(result)
        }
        (Value::String(ls), Value::Number(rn)) => {
            let mut result = ls.clone();
            result.extend_from_slice(&utf8_to_utf16(&rn.to_string()));
            Value::String(result)
        }
        _ => {
            return Err(JSError::EvaluationError {
                message: "Invalid operands for +=".to_string(),
            });
        }
    };
    let assignment_expr = match &result {
        Value::Number(n) => Expr::Number(*n),
        Value::String(s) => Expr::StringLit(s.clone()),
        _ => unreachable!(),
    };
    evaluate_assignment_expr(env, target, &assignment_expr)?;
    Ok(result)
}

fn evaluate_sub_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a -= b is equivalent to a = a - b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => Value::Number(ln - rn),
        _ => {
            return Err(JSError::EvaluationError {
                message: "Invalid operands for -=".to_string(),
            });
        }
    };
    let Value::Number(n) = result else { unreachable!() };
    evaluate_assignment_expr(env, target, &Expr::Number(n))?;
    Ok(result)
}

fn evaluate_mul_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a *= b is equivalent to a = a * b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => Value::Number(ln * rn),
        _ => {
            return Err(JSError::EvaluationError {
                message: "Invalid operands for *=".to_string(),
            });
        }
    };
    let Value::Number(n) = result else { unreachable!() };
    evaluate_assignment_expr(env, target, &Expr::Number(n))?;
    Ok(result)
}

fn evaluate_div_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a /= b is equivalent to a = a / b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => {
            if rn == 0.0 {
                return Err(JSError::EvaluationError {
                    message: "Division by zero".to_string(),
                });
            }
            Value::Number(ln / rn)
        }
        _ => {
            return Err(JSError::EvaluationError {
                message: "Invalid operands for /=".to_string(),
            });
        }
    };
    let Value::Number(n) = result else { unreachable!() };
    evaluate_assignment_expr(env, target, &Expr::Number(n))?;
    Ok(result)
}

fn evaluate_mod_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a %= b is equivalent to a = a % b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => {
            if rn == 0.0 {
                return Err(JSError::EvaluationError {
                    message: "Division by zero".to_string(),
                });
            }
            Value::Number(ln % rn)
        }
        _ => {
            return Err(JSError::EvaluationError {
                message: "Invalid operands for %=".to_string(),
            });
        }
    };
    let Value::Number(n) = result else { unreachable!() };
    evaluate_assignment_expr(env, target, &Expr::Number(n))?;
    Ok(result)
}

fn evaluate_assignment_expr(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    let val = evaluate_expr(env, value)?;
    match target {
        Expr::Var(name) => {
            env_set(env, name, val.clone())?;
            Ok(val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(obj_map) => {
                    obj_set_value(&obj_map, prop, val.clone())?;
                    Ok(val)
                }
                _ => Err(JSError::EvaluationError {
                    message: "Cannot assign to property of non-object".to_string(),
                }),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(obj_map), Value::String(s)) => {
                    let key = String::from_utf16_lossy(&s);
                    obj_set_value(&obj_map, &key, val.clone())?;
                    Ok(val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = n.to_string();
                    obj_set_value(&obj_map, &key, val.clone())?;
                    Ok(val)
                }
                _ => Err(JSError::EvaluationError {
                    message: "Invalid index assignment".to_string(),
                }),
            }
        }
        _ => Err(JSError::EvaluationError {
            message: "Invalid assignment target".to_string(),
        }),
    }
}

fn evaluate_increment(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    // Prefix increment: ++expr
    let current_val = evaluate_expr(env, expr)?;
    let new_val = match current_val {
        Value::Number(n) => Value::Number(n + 1.0),
        _ => {
            return Err(JSError::EvaluationError {
                message: "Increment operand must be a number".to_string(),
            });
        }
    };
    // Assign back
    match expr {
        Expr::Var(name) => {
            env_set(env, name, new_val.clone())?;
            Ok(new_val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(obj_map) => {
                    obj_set_value(&obj_map, prop, new_val.clone())?;
                    Ok(new_val)
                }
                _ => Err(JSError::EvaluationError {
                    message: "Cannot increment property of non-object".to_string(),
                }),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(obj_map), Value::String(s)) => {
                    let key = String::from_utf16_lossy(&s);
                    obj_set_value(&obj_map, &key, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = n.to_string();
                    obj_set_value(&obj_map, &key, new_val.clone())?;
                    Ok(new_val)
                }
                _ => Err(JSError::EvaluationError {
                    message: "Invalid index increment".to_string(),
                }),
            }
        }
        _ => Err(JSError::EvaluationError {
            message: "Invalid increment target".to_string(),
        }),
    }
}

fn evaluate_decrement(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    // Prefix decrement: --expr
    let current_val = evaluate_expr(env, expr)?;
    let new_val = match current_val {
        Value::Number(n) => Value::Number(n - 1.0),
        _ => {
            return Err(JSError::EvaluationError {
                message: "Decrement operand must be a number".to_string(),
            });
        }
    };
    // Assign back
    match expr {
        Expr::Var(name) => {
            env_set(env, name, new_val.clone())?;
            Ok(new_val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(obj_map) => {
                    obj_set_value(&obj_map, prop, new_val.clone())?;
                    Ok(new_val)
                }
                _ => Err(JSError::EvaluationError {
                    message: "Cannot decrement property of non-object".to_string(),
                }),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(obj_map), Value::String(s)) => {
                    let key = String::from_utf16_lossy(&s);
                    obj_set_value(&obj_map, &key, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = n.to_string();
                    obj_set_value(&obj_map, &key, new_val.clone())?;
                    Ok(new_val)
                }
                _ => Err(JSError::EvaluationError {
                    message: "Invalid index decrement".to_string(),
                }),
            }
        }
        _ => Err(JSError::EvaluationError {
            message: "Invalid decrement target".to_string(),
        }),
    }
}

fn evaluate_post_increment(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    // Postfix increment: expr++
    let current_val = evaluate_expr(env, expr)?;
    let old_val = current_val.clone();
    let new_val = match current_val {
        Value::Number(n) => Value::Number(n + 1.0),
        _ => {
            return Err(JSError::EvaluationError {
                message: "Increment operand must be a number".to_string(),
            });
        }
    };
    // Assign back
    match expr {
        Expr::Var(name) => {
            env_set(env, name, new_val)?;
            Ok(old_val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(obj_map) => {
                    obj_set_value(&obj_map, prop, new_val)?;
                    Ok(old_val)
                }
                _ => Err(JSError::EvaluationError {
                    message: "Cannot increment property of non-object".to_string(),
                }),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(obj_map), Value::String(s)) => {
                    let key = String::from_utf16_lossy(&s);
                    obj_set_value(&obj_map, &key, new_val)?;
                    Ok(old_val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = n.to_string();
                    obj_set_value(&obj_map, &key, new_val)?;
                    Ok(old_val)
                }
                _ => Err(JSError::EvaluationError {
                    message: "Invalid index increment".to_string(),
                }),
            }
        }
        _ => Err(JSError::EvaluationError {
            message: "Invalid increment target".to_string(),
        }),
    }
}

fn evaluate_post_decrement(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    // Postfix decrement: expr--
    let current_val = evaluate_expr(env, expr)?;
    let old_val = current_val.clone();
    let new_val = match current_val {
        Value::Number(n) => Value::Number(n - 1.0),
        _ => {
            return Err(JSError::EvaluationError {
                message: "Decrement operand must be a number".to_string(),
            });
        }
    };
    // Assign back
    match expr {
        Expr::Var(name) => {
            env_set(env, name, new_val)?;
            Ok(old_val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(obj_map) => {
                    obj_set_value(&obj_map, prop, new_val)?;
                    Ok(old_val)
                }
                _ => Err(JSError::EvaluationError {
                    message: "Cannot decrement property of non-object".to_string(),
                }),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(obj_map), Value::String(s)) => {
                    let key = String::from_utf16_lossy(&s);
                    obj_set_value(&obj_map, &key, new_val)?;
                    Ok(old_val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = n.to_string();
                    obj_set_value(&obj_map, &key, new_val)?;
                    Ok(old_val)
                }
                _ => Err(JSError::EvaluationError {
                    message: "Invalid index decrement".to_string(),
                }),
            }
        }
        _ => Err(JSError::EvaluationError {
            message: "Invalid decrement target".to_string(),
        }),
    }
}

fn evaluate_unary_neg(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    let val = evaluate_expr(env, expr)?;
    match val {
        Value::Number(n) => Ok(Value::Number(-n)),
        _ => Err(JSError::EvaluationError {
            message: "error".to_string(),
        }),
    }
}

fn evaluate_typeof(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    let val = evaluate_expr(env, expr)?;
    let type_str = match val {
        Value::Undefined => "undefined",
        Value::Boolean(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Object(_) => "object",
        Value::Function(_) => "function",
        Value::Closure(_, _, _) => "function",
        Value::ClassDefinition(_) => "function",
        Value::Getter(_, _) => "function",
        Value::Setter(_, _, _) => "function",
        Value::Property { .. } => "undefined",
        Value::Promise(_) => "object",
    };
    Ok(Value::String(utf8_to_utf16(type_str)))
}

fn evaluate_delete(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    match expr {
        Expr::Var(_) => {
            // Cannot delete local variables
            Ok(Value::Boolean(false))
        }
        Expr::Property(obj, prop) => {
            // Delete property from object
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(obj_map) => {
                    let deleted = obj_delete(&obj_map, prop);
                    Ok(Value::Boolean(deleted))
                }
                _ => Ok(Value::Boolean(false)),
            }
        }
        Expr::Index(obj, idx) => {
            // Delete indexed property
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(obj_map), Value::String(s)) => {
                    let key = String::from_utf16_lossy(&s);
                    let deleted = obj_delete(&obj_map, &key);
                    Ok(Value::Boolean(deleted))
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = n.to_string();
                    let deleted = obj_delete(&obj_map, &key);
                    Ok(Value::Boolean(deleted))
                }
                _ => Ok(Value::Boolean(false)),
            }
        }
        _ => {
            // Cannot delete other types of expressions
            Ok(Value::Boolean(false))
        }
    }
}

fn evaluate_void(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    // Evaluate the expression but always return undefined
    evaluate_expr(env, expr)?;
    Ok(Value::Undefined)
}

fn evaluate_binary(env: &JSObjectDataPtr, left: &Expr, op: &BinaryOp, right: &Expr) -> Result<Value, JSError> {
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
            (Value::Boolean(lb), Value::String(rs)) => {
                let mut result = utf8_to_utf16(&lb.to_string());
                result.extend_from_slice(&rs);
                Ok(Value::String(result))
            }
            (Value::String(ls), Value::Boolean(rb)) => {
                let mut result = ls.clone();
                result.extend_from_slice(&utf8_to_utf16(&rb.to_string()));
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
        BinaryOp::InstanceOf => {
            // Check if left is an instance of right (constructor)
            match (l, r) {
                (Value::Object(obj), Value::Object(constructor)) => Ok(Value::Boolean(is_instance_of(&obj, &constructor)?)),
                _ => Ok(Value::Boolean(false)),
            }
        }
        BinaryOp::In => {
            // Check if property exists in object
            match (l, r) {
                (Value::String(prop), Value::Object(obj)) => {
                    let prop_str = String::from_utf16_lossy(&prop);
                    Ok(Value::Boolean(obj_get_value(&obj, &prop_str)?.is_some()))
                }
                _ => Ok(Value::Boolean(false)),
            }
        }
        BinaryOp::NullishCoalescing => {
            // Nullish coalescing: return right if left is null or undefined, otherwise left
            match l {
                Value::Undefined => Ok(r),
                _ => Ok(l),
            }
        }
    }
}

fn evaluate_index(env: &JSObjectDataPtr, obj: &Expr, idx: &Expr) -> Result<Value, JSError> {
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
            if let Some(val) = obj_get_value(&obj_map, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Object(obj_map), Value::String(s)) => {
            // Object property access with string key
            let key = String::from_utf16_lossy(&s);
            if let Some(val) = obj_get_value(&obj_map, &key)? {
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

fn evaluate_property(env: &JSObjectDataPtr, obj: &Expr, prop: &str) -> Result<Value, JSError> {
    let obj_val = evaluate_expr(env, obj)?;
    log::trace!("Property: obj_val={obj_val:?}, prop={prop}");
    match obj_val {
        Value::String(s) if prop == "length" => Ok(Value::Number(utf16_len(&s) as f64)),
        Value::Object(obj_map) => {
            if let Some(val) = obj_get_value(&obj_map, prop)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        _ => Err(JSError::EvaluationError {
            message: format!("Property not found for obj_val={obj_val:?}, prop={prop}"),
        }),
    }
}

fn evaluate_optional_property(env: &JSObjectDataPtr, obj: &Expr, prop: &str) -> Result<Value, JSError> {
    let obj_val = evaluate_expr(env, obj)?;
    log::trace!("OptionalProperty: obj_val={obj_val:?}, prop={prop}");
    match obj_val {
        Value::Undefined => Ok(Value::Undefined),
        Value::Object(obj_map) => {
            if let Some(val) = obj_get_value(&obj_map, prop)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        Value::String(s) if prop == "length" => Ok(Value::Number(utf16_len(&s) as f64)),
        _ => Err(JSError::EvaluationError {
            message: format!("Property not found for obj_val={obj_val:?}, prop={prop}"),
        }),
    }
}

fn evaluate_call(env: &JSObjectDataPtr, func_expr: &Expr, args: &[Expr]) -> Result<Value, JSError> {
    log::trace!("evaluate_call entry: args_len={} func_expr=...", args.len());
    // Check if it's a method call first
    if let Expr::Property(obj_expr, method_name) = func_expr {
        // Special case for Array static methods
        if let Expr::Var(var_name) = &**obj_expr
            && var_name == "Array"
        {
            return crate::js_array::handle_array_static_method(method_name, args, env);
        }

        let obj_val = evaluate_expr(env, obj_expr)?;
        log::trace!("evaluate_call - object eval result: {obj_val:?}");
        match (obj_val, method_name.as_str()) {
            (Value::Object(obj_map), "log") if obj_map.borrow().contains_key("log") => {
                js_console::handle_console_method(method_name, args, env)
            }
            (obj_val, "toString") => crate::js_object::handle_to_string_method(&obj_val, args),
            (obj_val, "valueOf") => crate::js_object::handle_value_of_method(&obj_val, args),
            (Value::Object(obj_map), method) => {
                // If this object looks like the `std` module (we used 'sprintf' as marker)
                if obj_map.borrow().contains_key("sprintf") {
                    match method {
                        "sprintf" => {
                            log::trace!("js dispatch calling sprintf with {} args", args.len());
                            return sprintf::handle_sprintf_call(env, args);
                        }
                        "tmpfile" => {
                            return tmpfile::create_tmpfile();
                        }
                        _ => {}
                    }
                }

                // If this object looks like the `os` module (we used 'open' as marker)
                if obj_map.borrow().contains_key("open") {
                    return crate::js_os::handle_os_method(&obj_map, method, args, env);
                }

                // If this object looks like the `os.path` module
                if obj_map.borrow().contains_key("join") {
                    return crate::js_os::handle_os_method(&obj_map, method, args, env);
                }

                // If this object is a file-like object (we use '__file_id' as marker)
                if obj_map.borrow().contains_key("__file_id") {
                    return tmpfile::handle_file_method(&obj_map, method, args, env);
                }
                // Check if this is the Math object
                if obj_map.borrow().contains_key("PI") && obj_map.borrow().contains_key("E") {
                    js_math::handle_math_method(method, args, env)
                } else if obj_map.borrow().contains_key("parse") && obj_map.borrow().contains_key("stringify") {
                    crate::js_json::handle_json_method(method, args, env)
                } else if obj_map.borrow().contains_key("keys") && obj_map.borrow().contains_key("values") {
                    crate::js_object::handle_object_method(method, args, env)
                } else if obj_map.borrow().contains_key("MAX_VALUE") && obj_map.borrow().contains_key("MIN_VALUE") {
                    crate::js_number::handle_number_method(method, args, env)
                } else if obj_map.borrow().contains_key("__timestamp") {
                    // Date instance methods
                    crate::js_date::handle_date_method(&obj_map, method, args)
                } else if obj_map.borrow().contains_key("__regex") {
                    // RegExp instance methods
                    crate::js_regexp::handle_regexp_method(&obj_map, method, args, env)
                } else if is_array(&obj_map) {
                    // Array instance methods
                    crate::js_array::handle_array_instance_method(&obj_map, method, args, env, obj_expr)
                } else if obj_map.borrow().contains_key("__promise") {
                    // Promise instance methods
                    handle_promise_method(&obj_map, method, args, env)
                } else if obj_map.borrow().contains_key("__class_def__") {
                    // Class static methods
                    call_static_method(&obj_map, method, args, env)
                } else if is_class_instance(&obj_map)? {
                    call_class_method(&obj_map, method, args, env)
                } else {
                    // Check for user-defined method
                    if let Some(prop_val) = obj_get_value(&obj_map, method)? {
                        match prop_val.borrow().clone() {
                            Value::Closure(params, body, captured_env) => {
                                // Function call
                                // Collect all arguments, expanding spreads
                                let mut evaluated_args = Vec::new();
                                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                                if params.len() != evaluated_args.len() {
                                    return Err(JSError::ParseError);
                                }
                                // Create new environment starting with captured environment
                                let func_env = captured_env.clone();
                                // Add parameters
                                for (param, arg_val) in params.iter().zip(evaluated_args.iter()) {
                                    env_set(&func_env, param.as_str(), arg_val.clone())?;
                                }
                                // Execute function body
                                evaluate_statements(&func_env, &body)
                            }
                            Value::Function(func_name) => crate::js_function::handle_global_function(&func_name, args, env),
                            _ => Err(JSError::EvaluationError {
                                message: format!("Property '{}' is not a function", method),
                            }),
                        }
                    } else {
                        Err(JSError::EvaluationError {
                            message: format!("Method {method} not found on object"),
                        })
                    }
                }
            }
            (Value::Function(func_name), method) => {
                // Handle constructor static methods
                match func_name.as_str() {
                    "Object" => crate::js_object::handle_object_method(method, args, env),
                    "Array" => crate::js_array::handle_array_static_method(method, args, env),
                    "Promise" => crate::js_promise::handle_promise_static_method(method, args, env),
                    _ => Err(JSError::EvaluationError {
                        message: format!("{} has no static method '{}'", func_name, method),
                    }),
                }
            }
            (Value::String(s), method) => crate::js_string::handle_string_method(&s, method, args, env),
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        }
    } else if let Expr::OptionalProperty(obj_expr, method_name) = func_expr {
        // Optional method call
        let obj_val = evaluate_expr(env, obj_expr)?;
        match obj_val {
            Value::Undefined => Ok(Value::Undefined),
            Value::Object(obj_map) => handle_optional_method_call(&obj_map, method_name, args, env, obj_expr),
            Value::Function(func_name) => {
                // Handle constructor static methods
                match func_name.as_str() {
                    "Object" => crate::js_object::handle_object_method(method_name, args, env),
                    "Array" => crate::js_array::handle_array_static_method(method_name, args, env),
                    "Promise" => crate::js_promise::handle_promise_static_method(method_name, args, env),
                    _ => Err(JSError::EvaluationError {
                        message: format!("{} has no static method '{}'", func_name, method_name),
                    }),
                }
            }
            Value::String(s) => crate::js_string::handle_string_method(&s, method_name, args, env),
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        }
    } else {
        // Regular function call
        let func_val = evaluate_expr(env, func_expr)?;
        match func_val {
            Value::Function(func_name) => crate::js_function::handle_global_function(&func_name, args, env),
            Value::Closure(params, body, captured_env) => {
                // Function call
                // Collect all arguments, expanding spreads
                let mut evaluated_args = Vec::new();
                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                if params.len() != evaluated_args.len() {
                    return Err(JSError::ParseError);
                }
                // Create new environment starting with captured environment
                let func_env = captured_env.clone();
                // Add parameters
                for (param, arg_val) in params.iter().zip(evaluated_args.iter()) {
                    env_set(&func_env, param.as_str(), arg_val.clone())?;
                }
                // Execute function body
                evaluate_statements(&func_env, &body)
            }
            Value::Object(obj_map) => {
                // Check if this is a built-in constructor object
                if obj_map.borrow().contains_key("MAX_VALUE") && obj_map.borrow().contains_key("MIN_VALUE") {
                    // Number constructor call
                    crate::js_function::handle_global_function("Number", args, env)
                } else {
                    Err(JSError::EvaluationError {
                        message: "error".to_string(),
                    })
                }
            }
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        }
    }
}

fn evaluate_optional_call(env: &JSObjectDataPtr, func_expr: &Expr, args: &[Expr]) -> Result<Value, JSError> {
    log::trace!("evaluate_optional_call entry: args_len={} func_expr=...", args.len());
    // Check if it's a method call first
    if let Expr::Property(obj_expr, method_name) = func_expr {
        // Special case for Array static methods
        if let Expr::Var(var_name) = &**obj_expr
            && var_name == "Array"
        {
            return crate::js_array::handle_array_static_method(method_name, args, env);
        }

        let obj_val = evaluate_expr(env, obj_expr)?;
        log::trace!("evaluate_optional_call - object eval result: {obj_val:?}");
        match obj_val {
            Value::Undefined => Ok(Value::Undefined),
            Value::Object(obj_map) => {
                // If this object looks like the `std` module (we used 'sprintf' as marker)
                if obj_map.borrow().contains_key("sprintf") {
                    match method_name.as_str() {
                        "sprintf" => {
                            log::trace!("js dispatch calling sprintf with {} args", args.len());
                            return sprintf::handle_sprintf_call(env, args);
                        }
                        "tmpfile" => {
                            return tmpfile::create_tmpfile();
                        }
                        _ => {}
                    }
                }

                // If this object looks like the `os` module (we used 'open' as marker)
                if obj_map.borrow().contains_key("open") {
                    return crate::js_os::handle_os_method(&obj_map, method_name, args, env);
                }

                // If this object looks like the `os.path` module
                if obj_map.borrow().contains_key("join") {
                    return crate::js_os::handle_os_method(&obj_map, method_name, args, env);
                }

                // If this object is a file-like object (we use '__file_id' as marker)
                if obj_map.borrow().contains_key("__file_id") {
                    return tmpfile::handle_file_method(&obj_map, method_name, args, env);
                }
                // Check if this is the Math object
                if obj_map.borrow().contains_key("PI") && obj_map.borrow().contains_key("E") {
                    js_math::handle_math_method(method_name, args, env)
                } else if obj_map.borrow().contains_key("parse") && obj_map.borrow().contains_key("stringify") {
                    crate::js_json::handle_json_method(method_name, args, env)
                } else if obj_map.borrow().contains_key("keys") && obj_map.borrow().contains_key("values") {
                    crate::js_object::handle_object_method(method_name, args, env)
                } else if obj_map.borrow().contains_key("MAX_VALUE") && obj_map.borrow().contains_key("MIN_VALUE") {
                    crate::js_number::handle_number_method(method_name, args, env)
                } else if obj_map.borrow().contains_key("__timestamp") {
                    // Date instance methods
                    crate::js_date::handle_date_method(&obj_map, method_name, args)
                } else if obj_map.borrow().contains_key("__regex") {
                    // RegExp instance methods
                    crate::js_regexp::handle_regexp_method(&obj_map, method_name, args, env)
                } else if is_array(&obj_map) {
                    // Array instance methods
                    crate::js_array::handle_array_instance_method(&obj_map, method_name, args, env, obj_expr)
                } else if obj_map.borrow().contains_key("__promise") {
                    // Promise instance methods
                    handle_promise_method(&obj_map, method_name, args, env)
                } else if obj_map.borrow().contains_key("__class_def__") {
                    // Class static methods
                    call_static_method(&obj_map, method_name, args, env)
                } else if is_class_instance(&obj_map)? {
                    call_class_method(&obj_map, method_name, args, env)
                } else {
                    Err(JSError::EvaluationError {
                        message: format!("Method {method_name} not found on object"),
                    })
                }
            }
            Value::Function(func_name) => {
                // Handle constructor static methods
                match func_name.as_str() {
                    "Object" => crate::js_object::handle_object_method(method_name, args, env),
                    "Array" => crate::js_array::handle_array_static_method(method_name, args, env),
                    _ => Err(JSError::EvaluationError {
                        message: format!("{} has no static method '{}'", func_name, method_name),
                    }),
                }
            }
            Value::String(s) => crate::js_string::handle_string_method(&s, method_name, args, env),
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        }
    } else {
        // Regular function call - check if base is null/undefined
        let func_val = evaluate_expr(env, func_expr)?;
        match func_val {
            Value::Undefined => Ok(Value::Undefined),
            Value::Function(func_name) => crate::js_function::handle_global_function(&func_name, args, env),
            Value::Closure(params, body, captured_env) => {
                // Function call
                // Collect all arguments, expanding spreads
                let mut evaluated_args = Vec::new();
                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                if params.len() != evaluated_args.len() {
                    return Err(JSError::ParseError);
                }
                // Create new environment starting with captured environment
                let func_env = captured_env.clone();
                // Add parameters
                for (param, arg_val) in params.iter().zip(evaluated_args.iter()) {
                    env_set(&func_env, param.as_str(), arg_val.clone())?;
                }
                // Execute function body
                evaluate_statements(&func_env, &body)
            }
            _ => Err(JSError::EvaluationError {
                message: "error".to_string(),
            }),
        }
    }
}

fn evaluate_object(env: &JSObjectDataPtr, properties: &Vec<(String, Expr)>) -> Result<Value, JSError> {
    let obj = Rc::new(RefCell::new(JSObjectData::new()));
    for (key, value_expr) in properties {
        if key.is_empty() && matches!(value_expr, Expr::Spread(_)) {
            // Spread operator: evaluate the expression and spread its properties
            if let Expr::Spread(expr) = value_expr {
                let spread_val = evaluate_expr(env, expr)?;
                if let Value::Object(spread_obj) = spread_val {
                    // Copy all properties from spread_obj to obj
                    for (prop_key, prop_val) in spread_obj.borrow().properties.iter() {
                        obj.borrow_mut().insert(prop_key.clone(), prop_val.clone());
                    }
                } else {
                    return Err(JSError::EvaluationError {
                        message: "Spread operator can only be applied to objects".to_string(),
                    });
                }
            }
        } else {
            match value_expr {
                Expr::Getter(func_expr) => {
                    if let Expr::Function(_params, body) = func_expr.as_ref() {
                        // Check if property already exists
                        let existing_opt = obj.borrow().get(key);
                        if let Some(existing) = existing_opt {
                            let mut val = existing.borrow().clone();
                            if let Value::Property {
                                value: _,
                                getter,
                                setter: _,
                            } = &mut val
                            {
                                // Update getter
                                getter.replace((body.clone(), env.clone()));
                                obj.borrow_mut().insert(key.to_string(), Rc::new(RefCell::new(val)));
                            } else {
                                // Create new property descriptor
                                let prop = Value::Property {
                                    value: Some(existing.clone()),
                                    getter: Some((body.clone(), env.clone())),
                                    setter: None,
                                };
                                obj.borrow_mut().insert(key.to_string(), Rc::new(RefCell::new(prop)));
                            }
                        } else {
                            // Create new property descriptor with getter
                            let prop = Value::Property {
                                value: None,
                                getter: Some((body.clone(), env.clone())),
                                setter: None,
                            };
                            obj.borrow_mut().insert(key.to_string(), Rc::new(RefCell::new(prop)));
                        }
                    } else {
                        return Err(JSError::EvaluationError {
                            message: "Getter must be a function".to_string(),
                        });
                    }
                }
                Expr::Setter(func_expr) => {
                    if let Expr::Function(params, body) = func_expr.as_ref() {
                        // Check if property already exists
                        let existing_opt = obj.borrow().get(key);
                        if let Some(existing) = existing_opt {
                            let mut val = existing.borrow().clone();
                            if let Value::Property {
                                value: _,
                                getter: _,
                                setter,
                            } = &mut val
                            {
                                // Update setter
                                setter.replace((params.clone(), body.clone(), env.clone()));
                                obj.borrow_mut().insert(key.to_string(), Rc::new(RefCell::new(val)));
                            } else {
                                // Create new property descriptor
                                let prop = Value::Property {
                                    value: Some(existing.clone()),
                                    getter: None,
                                    setter: Some((params.clone(), body.clone(), env.clone())),
                                };
                                obj.borrow_mut().insert(key.to_string(), Rc::new(RefCell::new(prop)));
                            }
                        } else {
                            // Create new property descriptor with setter
                            let prop = Value::Property {
                                value: None,
                                getter: None,
                                setter: Some((params.clone(), body.clone(), env.clone())),
                            };
                            obj.borrow_mut().insert(key.to_string(), Rc::new(RefCell::new(prop)));
                        }
                    } else {
                        return Err(JSError::EvaluationError {
                            message: "Setter must be a function".to_string(),
                        });
                    }
                }
                _ => {
                    let value = evaluate_expr(env, value_expr)?;
                    // Check if property already exists
                    let existing_rc = obj.borrow().get(key);
                    if let Some(existing) = existing_rc {
                        let mut existing_val = existing.borrow().clone();
                        if let Value::Property {
                            value: prop_value,
                            getter: _,
                            setter: _,
                        } = &mut existing_val
                        {
                            // Update value
                            prop_value.replace(Rc::new(RefCell::new(value)));
                            obj.borrow_mut().insert(key.to_string(), Rc::new(RefCell::new(existing_val)));
                        } else {
                            // Create new property descriptor
                            let prop = Value::Property {
                                value: Some(Rc::new(RefCell::new(value))),
                                getter: None,
                                setter: None,
                            };
                            obj.borrow_mut().insert(key.to_string(), Rc::new(RefCell::new(prop)));
                        }
                    } else {
                        obj_set_value(&obj, key.as_str(), value)?;
                    }
                }
            }
        }
    }
    Ok(Value::Object(obj))
}

fn evaluate_array(env: &JSObjectDataPtr, elements: &Vec<Expr>) -> Result<Value, JSError> {
    let arr = Rc::new(RefCell::new(JSObjectData::new()));
    let mut index = 0;
    for elem_expr in elements {
        if let Expr::Spread(spread_expr) = elem_expr {
            // Spread operator: evaluate the expression and spread its elements
            let spread_val = evaluate_expr(env, spread_expr)?;
            if let Value::Object(spread_obj) = spread_val {
                // Assume it's an array-like object
                let mut i = 0;
                loop {
                    let key = i.to_string();
                    if let Some(val) = obj_get_value(&spread_obj, &key)? {
                        obj_set_value(&arr, index.to_string(), val.borrow().clone())?;
                        index += 1;
                        i += 1;
                    } else {
                        break;
                    }
                }
            } else {
                return Err(JSError::EvaluationError {
                    message: "Spread operator can only be applied to arrays".to_string(),
                });
            }
        } else {
            let value = evaluate_expr(env, elem_expr)?;
            obj_set_value(&arr, index.to_string(), value)?;
            index += 1;
        }
    }
    // Set length property
    set_array_length(&arr, index)?;
    Ok(Value::Object(arr))
}

fn evaluate_array_destructuring(_env: &JSObjectDataPtr, _pattern: &Vec<DestructuringElement>) -> Result<Value, JSError> {
    // Array destructuring is handled at the statement level, not as an expression
    Err(JSError::EvaluationError {
        message: "Array destructuring should not be evaluated as an expression".to_string(),
    })
}

fn evaluate_object_destructuring(_env: &JSObjectDataPtr, _pattern: &Vec<ObjectDestructuringElement>) -> Result<Value, JSError> {
    // Object destructuring is handled at the statement level, not as an expression
    Err(JSError::EvaluationError {
        message: "Object destructuring should not be evaluated as an expression".to_string(),
    })
}

pub type JSObjectDataPtr = Rc<RefCell<JSObjectData>>;

#[derive(Clone, Default, Debug)]
pub struct JSObjectData {
    pub properties: std::collections::HashMap<String, Rc<RefCell<Value>>>,
    pub constants: std::collections::HashSet<String>,
    pub prototype: Option<Rc<RefCell<JSObjectData>>>,
}

impl JSObjectData {
    pub fn new() -> Self {
        JSObjectData::default()
    }

    pub fn insert(&mut self, key: String, val: Rc<RefCell<Value>>) {
        self.properties.insert(key, val);
    }

    pub fn get(&self, key: &str) -> Option<Rc<RefCell<Value>>> {
        self.properties.get(key).cloned()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.properties.contains_key(key)
    }

    pub fn remove(&mut self, key: &str) -> Option<Rc<RefCell<Value>>> {
        self.properties.remove(key)
    }

    pub fn keys(&self) -> std::collections::hash_map::Keys<'_, String, Rc<RefCell<Value>>> {
        self.properties.keys()
    }

    pub fn is_const(&self, key: &str) -> bool {
        self.constants.contains(key)
    }

    pub fn set_const(&mut self, key: String) {
        self.constants.insert(key);
    }
}

#[derive(Clone, Debug)]
pub enum Value {
    Number(f64),
    String(Vec<u16>), // UTF-16 code units
    Boolean(bool),
    Undefined,
    Object(JSObjectDataPtr),                               // Object with properties
    Function(String),                                      // Function name
    Closure(Vec<String>, Vec<Statement>, JSObjectDataPtr), // parameters, body, captured environment
    ClassDefinition(Rc<ClassDefinition>),                  // Class definition
    Getter(Vec<Statement>, JSObjectDataPtr),               // getter body, captured environment
    Setter(Vec<String>, Vec<Statement>, JSObjectDataPtr),  // setter parameter, body, captured environment
    Property {
        // Property descriptor with getter/setter/value
        value: Option<Rc<RefCell<Value>>>,
        getter: Option<(Vec<Statement>, JSObjectDataPtr)>,
        setter: Option<(Vec<String>, Vec<Statement>, JSObjectDataPtr)>,
    },
    Promise(Rc<RefCell<JSPromise>>), // Promise object
}

// Helper functions for UTF-16 string operations
pub fn utf8_to_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn utf16_to_utf8(v: &[u16]) -> String {
    String::from_utf16_lossy(v)
}

pub fn utf16_len(v: &[u16]) -> usize {
    v.len()
}

pub fn utf16_slice(v: &[u16], start: usize, end: usize) -> Vec<u16> {
    if start >= v.len() {
        Vec::new()
    } else {
        let end = end.min(v.len());
        v[start..end].to_vec()
    }
}

pub fn utf16_char_at(v: &[u16], index: usize) -> Option<u16> {
    v.get(index).copied()
}

pub fn utf16_to_uppercase(v: &[u16]) -> Vec<u16> {
    let s = utf16_to_utf8(v);
    utf8_to_utf16(&s.to_uppercase())
}

pub fn utf16_to_lowercase(v: &[u16]) -> Vec<u16> {
    let s = utf16_to_utf8(v);
    utf8_to_utf16(&s.to_lowercase())
}

pub fn utf16_find(v: &[u16], pattern: &[u16]) -> Option<usize> {
    if pattern.is_empty() {
        return Some(0);
    }
    (0..=v.len().saturating_sub(pattern.len())).find(|&i| v[i..i + pattern.len()] == *pattern)
}

pub fn utf16_rfind(v: &[u16], pattern: &[u16]) -> Option<usize> {
    if pattern.is_empty() {
        return Some(v.len());
    }
    (0..=v.len().saturating_sub(pattern.len()))
        .rev()
        .find(|&i| v[i..i + pattern.len()] == *pattern)
}

pub fn utf16_replace(v: &[u16], search: &[u16], replace: &[u16]) -> Vec<u16> {
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

// Helper function to convert value to string for display
pub fn value_to_string(val: &Value) -> String {
    match val {
        Value::Number(n) => n.to_string(),
        Value::String(s) => String::from_utf16_lossy(s),
        Value::Boolean(b) => b.to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Object(_) => "[object Object]".to_string(),
        Value::Function(name) => format!("function {}", name),
        Value::Closure(_, _, _) => "function".to_string(),
        Value::ClassDefinition(_) => "class".to_string(),
        Value::Getter(_, _) => "getter".to_string(),
        Value::Setter(_, _, _) => "setter".to_string(),
        Value::Property { .. } => "[property]".to_string(),
        Value::Promise(_) => "[object Promise]".to_string(),
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
        Value::ClassDefinition(_) => "[class]".to_string(),
        Value::Getter(_, _) => "[getter]".to_string(),
        Value::Setter(_, _, _) => "[setter]".to_string(),
        Value::Property { .. } => "[property]".to_string(),
        Value::Promise(_) => "[object Promise]".to_string(),
    }
}

// Helper accessors for objects and environments
pub fn obj_get_value<T: AsRef<str>>(js_obj: &JSObjectDataPtr, key: T) -> Result<Option<Rc<RefCell<Value>>>, JSError> {
    let obj = js_obj.borrow().get(key.as_ref());
    if let Some(val) = obj {
        // Check if this is a property descriptor
        let val_clone = val.borrow().clone();
        match val_clone {
            Value::Property { value, getter, .. } => {
                if let Some((body, env)) = getter {
                    // Create a new environment with this bound to the object
                    let getter_env = Rc::new(RefCell::new(JSObjectData::new()));
                    getter_env.borrow_mut().prototype = Some(env);
                    env_set(&getter_env, "this", Value::Object(js_obj.clone()))?;
                    let result = evaluate_statements(&getter_env, &body)?;
                    Ok(Some(Rc::new(RefCell::new(result))))
                } else if let Some(val_rc) = value {
                    Ok(Some(val_rc))
                } else {
                    Ok(Some(Rc::new(RefCell::new(Value::Undefined))))
                }
            }
            Value::Getter(body, env) => {
                // Create a new environment with this bound to the object
                let getter_env = Rc::new(RefCell::new(JSObjectData::new()));
                getter_env.borrow_mut().prototype = Some(env);
                env_set(&getter_env, "this", Value::Object(js_obj.clone()))?;
                let result = evaluate_statements(&getter_env, &body)?;
                Ok(Some(Rc::new(RefCell::new(result))))
            }
            _ => Ok(Some(val.clone())),
        }
    } else if let Some(ref proto) = js_obj.borrow().prototype {
        obj_get_value(proto, key)
    } else {
        Ok(None)
    }
}

pub fn obj_set_value<T: AsRef<str>>(js_obj: &JSObjectDataPtr, key: T, val: Value) -> Result<(), JSError> {
    let key = key.as_ref().to_string();
    // Check if there's a setter for this property
    let existing_opt = js_obj.borrow().get(&key);
    if let Some(existing) = existing_opt {
        match existing.borrow().clone() {
            Value::Property { value: _, getter, setter } => {
                if let Some((param, body, env)) = setter {
                    // Create a new environment with this bound to the object and the parameter
                    let setter_env = Rc::new(RefCell::new(JSObjectData::new()));
                    setter_env.borrow_mut().prototype = Some(env);
                    env_set(&setter_env, "this", Value::Object(js_obj.clone()))?;
                    env_set(&setter_env, &param[0], val)?;
                    let _v = evaluate_statements(&setter_env, &body)?;
                } else {
                    // No setter, update value
                    let value = Some(Rc::new(RefCell::new(val)));
                    let new_prop = Value::Property { value, getter, setter };
                    js_obj.borrow_mut().insert(key, Rc::new(RefCell::new(new_prop)));
                }
                return Ok(());
            }
            Value::Setter(param, body, env) => {
                // Create a new environment with this bound to the object and the parameter
                let setter_env = Rc::new(RefCell::new(JSObjectData::new()));
                setter_env.borrow_mut().prototype = Some(env);
                env_set(&setter_env, "this", Value::Object(js_obj.clone()))?;
                env_set(&setter_env, &param[0], val)?;
                evaluate_statements(&setter_env, &body)?;
                return Ok(());
            }
            _ => {}
        }
    }
    // No setter, just set the value normally
    js_obj.borrow_mut().insert(key, Rc::new(RefCell::new(val)));
    Ok(())
}

pub fn obj_set_rc(map: &JSObjectDataPtr, key: &str, val_rc: Rc<RefCell<Value>>) {
    map.borrow_mut().insert(key.to_string(), val_rc);
}

pub fn obj_delete(map: &JSObjectDataPtr, key: &str) -> bool {
    map.borrow_mut().remove(key);
    true // In JavaScript, delete always returns true
}

pub fn env_get<T: AsRef<str>>(env: &JSObjectDataPtr, key: T) -> Option<Rc<RefCell<Value>>> {
    env.borrow().get(key.as_ref())
}

pub fn env_set<T: AsRef<str>>(env: &JSObjectDataPtr, key: T, val: Value) -> Result<(), JSError> {
    let key = key.as_ref();
    if env.borrow().is_const(key) {
        return Err(JSError::TypeError {
            message: format!("Assignment to constant variable '{key}'"),
        });
    }
    env.borrow_mut().insert(key.to_string(), Rc::new(RefCell::new(val)));
    Ok(())
}

pub fn env_set_const(env: &JSObjectDataPtr, key: &str, val: Value) {
    let mut env_mut = env.borrow_mut();
    env_mut.insert(key.to_string(), Rc::new(RefCell::new(val)));
    env_mut.set_const(key.to_string());
}

// Higher-level property API that operates on expressions + environment.
// `get_prop_env` evaluates `obj_expr` in `env` and returns the property's Rc if present.
pub fn get_prop_env(env: &JSObjectDataPtr, obj_expr: &Expr, prop: &str) -> Result<Option<Rc<RefCell<Value>>>, JSError> {
    let obj_val = evaluate_expr(env, obj_expr)?;
    match obj_val {
        Value::Object(map) => obj_get_value(&map, prop),
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
pub fn set_prop_env(env: &JSObjectDataPtr, obj_expr: &Expr, prop: &str, val: Value) -> Result<Option<Value>, JSError> {
    // Fast path: obj_expr is a variable that we can mutate in-place in env
    if let Expr::Var(varname) = obj_expr
        && let Some(rc_val) = env_get(env, varname)
    {
        let mut borrowed = rc_val.borrow_mut();
        if let Value::Object(ref mut map) = *borrowed {
            // Special-case `__proto__` assignment: set the prototype
            if prop == "__proto__" {
                if let Value::Object(proto_map) = val {
                    map.borrow_mut().prototype = Some(proto_map);
                    return Ok(None);
                } else {
                    // Non-object assigned to __proto__: ignore or set to None
                    map.borrow_mut().prototype = None;
                    return Ok(None);
                }
            }

            obj_set_value(map, prop, val)?;
            return Ok(None);
        }
    }

    // Fall back: evaluate the object expression and return an updated object value
    let obj_val = evaluate_expr(env, obj_expr)?;
    match obj_val {
        Value::Object(obj) => {
            // Special-case `__proto__` assignment: set the object's prototype
            if prop == "__proto__" {
                if let Value::Object(proto_map) = val {
                    obj.borrow_mut().prototype = Some(proto_map);
                    return Ok(Some(Value::Object(obj)));
                } else {
                    obj.borrow_mut().prototype = None;
                    return Ok(Some(Value::Object(obj)));
                }
            }

            obj_set_value(&obj, prop, val)?;
            Ok(Some(Value::Object(obj)))
        }
        _ => Err(JSError::EvaluationError {
            message: "not an object".to_string(),
        }),
    }
}

#[derive(Clone, Debug)]
pub enum SwitchCase {
    Case(Expr, Vec<Statement>), // case value, statements
    Default(Vec<Statement>),    // default statements
}

#[derive(Clone)]
pub enum Statement {
    Let(String, Option<Expr>),
    Const(String, Expr),
    LetDestructuringArray(Vec<DestructuringElement>, Expr), // array destructuring: let [a, b] = [1, 2];
    ConstDestructuringArray(Vec<DestructuringElement>, Expr), // const [a, b] = [1, 2];
    LetDestructuringObject(Vec<ObjectDestructuringElement>, Expr), // object destructuring: let {a, b} = {a: 1, b: 2};
    ConstDestructuringObject(Vec<ObjectDestructuringElement>, Expr), // const {a, b} = {a: 1, b: 2};
    Class(String, Option<String>, Vec<ClassMember>),        // name, extends, members
    Assign(String, Expr),                                   // variable assignment
    Expr(Expr),
    Return(Option<Expr>),
    If(Expr, Vec<Statement>, Option<Vec<Statement>>), // condition, then_body, else_body
    For(Option<Box<Statement>>, Option<Expr>, Option<Box<Statement>>, Vec<Statement>), // init, condition, increment, body
    ForOf(String, Expr, Vec<Statement>),              // variable, iterable, body
    While(Expr, Vec<Statement>),                      // condition, body
    DoWhile(Vec<Statement>, Expr),                    // body, condition
    Switch(Expr, Vec<SwitchCase>),                    // expression, cases
    Break,
    Continue,
    TryCatch(Vec<Statement>, String, Vec<Statement>, Option<Vec<Statement>>), // try_body, catch_param, catch_body, finally_body
    Throw(Expr),                                                              // throw expression
}

impl std::fmt::Debug for Statement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Statement::Let(var, expr) => write!(f, "Let({}, {:?})", var, expr),
            Statement::Const(var, expr) => write!(f, "Const({}, {:?})", var, expr),
            Statement::LetDestructuringArray(pattern, expr) => write!(f, "LetDestructuringArray({:?}, {:?})", pattern, expr),
            Statement::ConstDestructuringArray(pattern, expr) => write!(f, "ConstDestructuringArray({:?}, {:?})", pattern, expr),
            Statement::LetDestructuringObject(pattern, expr) => write!(f, "LetDestructuringObject({:?}, {:?})", pattern, expr),
            Statement::ConstDestructuringObject(pattern, expr) => write!(f, "ConstDestructuringObject({:?}, {:?})", pattern, expr),
            Statement::Class(name, extends, members) => write!(f, "Class({name}, {extends:?}, {members:?})"),
            Statement::Assign(var, expr) => write!(f, "Assign({}, {:?})", var, expr),
            Statement::Expr(expr) => write!(f, "Expr({:?})", expr),
            Statement::Return(Some(expr)) => write!(f, "Return({:?})", expr),
            Statement::Return(None) => write!(f, "Return(None)"),
            Statement::If(cond, then_body, else_body) => {
                write!(f, "If({:?}, {:?}, {:?})", cond, then_body, else_body)
            }
            Statement::For(init, cond, incr, body) => {
                write!(f, "For({:?}, {:?}, {:?}, {:?})", init, cond, incr, body)
            }
            Statement::ForOf(var, iterable, body) => {
                write!(f, "ForOf({}, {:?}, {:?})", var, iterable, body)
            }
            Statement::While(cond, body) => {
                write!(f, "While({:?}, {:?})", cond, body)
            }
            Statement::DoWhile(body, cond) => {
                write!(f, "DoWhile({:?}, {:?})", body, cond)
            }
            Statement::Switch(expr, cases) => {
                write!(f, "Switch({:?}, {:?})", expr, cases)
            }
            Statement::Break => write!(f, "Break"),
            Statement::Continue => write!(f, "Continue"),
            Statement::TryCatch(try_body, catch_param, catch_body, finally_body) => {
                write!(f, "TryCatch({:?}, {}, {:?}, {:?})", try_body, catch_param, catch_body, finally_body)
            }
            Statement::Throw(expr) => {
                write!(f, "Throw({:?})", expr)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum Expr {
    Number(f64),
    StringLit(Vec<u16>),
    Boolean(bool),
    Var(String),
    Binary(Box<Expr>, BinaryOp, Box<Expr>),
    UnaryNeg(Box<Expr>),
    TypeOf(Box<Expr>),
    Delete(Box<Expr>),
    Void(Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),           // target, value
    LogicalAndAssign(Box<Expr>, Box<Expr>), // target, value
    LogicalOrAssign(Box<Expr>, Box<Expr>),  // target, value
    NullishAssign(Box<Expr>, Box<Expr>),    // target, value
    AddAssign(Box<Expr>, Box<Expr>),        // target, value
    SubAssign(Box<Expr>, Box<Expr>),        // target, value
    MulAssign(Box<Expr>, Box<Expr>),        // target, value
    DivAssign(Box<Expr>, Box<Expr>),        // target, value
    ModAssign(Box<Expr>, Box<Expr>),        // target, value
    Increment(Box<Expr>),
    Decrement(Box<Expr>),
    PostIncrement(Box<Expr>),
    PostDecrement(Box<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Property(Box<Expr>, String),
    Call(Box<Expr>, Vec<Expr>),
    Function(Vec<String>, Vec<Statement>),                // parameters, body
    AsyncFunction(Vec<String>, Vec<Statement>),           // parameters, body for async functions
    ArrowFunction(Vec<String>, Vec<Statement>),           // parameters, body
    Object(Vec<(String, Expr)>),                          // object literal: key-value pairs
    Array(Vec<Expr>),                                     // array literal: [elem1, elem2, ...]
    Getter(Box<Expr>),                                    // getter function
    Setter(Box<Expr>),                                    // setter function
    Spread(Box<Expr>),                                    // spread operator: ...expr
    OptionalProperty(Box<Expr>, String),                  // optional property access: obj?.prop
    OptionalCall(Box<Expr>, Vec<Expr>),                   // optional call: obj?.method(args)
    Await(Box<Expr>),                                     // await expression
    This,                                                 // this keyword
    New(Box<Expr>, Vec<Expr>),                            // new expression: new Constructor(args)
    Super,                                                // super keyword
    SuperCall(Vec<Expr>),                                 // super() call in constructor
    SuperProperty(String),                                // super.property access
    SuperMethod(String, Vec<Expr>),                       // super.method() call
    ArrayDestructuring(Vec<DestructuringElement>),        // array destructuring: [a, b, ...rest]
    ObjectDestructuring(Vec<ObjectDestructuringElement>), // object destructuring: {a, b: c, ...rest}
    Value(Value),                                         // literal value
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
    InstanceOf,
    In,
    NullishCoalescing,
}

#[derive(Debug, Clone)]
pub enum DestructuringElement {
    Variable(String),                              // a
    NestedArray(Vec<DestructuringElement>),        // [a, b]
    NestedObject(Vec<ObjectDestructuringElement>), // {a, b}
    Rest(String),                                  // ...rest
    Empty,                                         // for skipped elements: [, b] = [1, 2]
}

#[derive(Debug, Clone)]
pub enum ObjectDestructuringElement {
    Property { key: String, value: DestructuringElement }, // a: b or a
    Rest(String),                                          // ...rest
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
                'x' => {
                    // Hex escape sequence \xHH
                    *start += 1;
                    if *start + 2 > chars.len() {
                        return Err(JSError::TokenizationError);
                    }
                    let hex_str: String = chars[*start..*start + 2].iter().collect();
                    *start += 1; // will be incremented by 1 at the end
                    match u8::from_str_radix(&hex_str, 16) {
                        Ok(code) => {
                            result.push(code as u16);
                        }
                        Err(_) => return Err(JSError::TokenizationError),
                    }
                }
                // For other escapes (regex escapes like \., \s, \], etc.) keep the backslash
                // so the regex engine receives the escape sequence. Push '\' then the char.
                other => {
                    result.push('\\' as u16);
                    result.push(other as u16);
                }
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
                if i + 1 < chars.len() && chars[i + 1] == '+' {
                    tokens.push(Token::Increment);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::AddAssign);
                    i += 2;
                } else {
                    tokens.push(Token::Plus);
                    i += 1;
                }
            }
            '-' => {
                if i + 1 < chars.len() && chars[i + 1] == '-' {
                    tokens.push(Token::Decrement);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::SubAssign);
                    i += 2;
                } else {
                    tokens.push(Token::Minus);
                    i += 1;
                }
            }
            '*' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::MulAssign);
                    i += 2;
                } else {
                    tokens.push(Token::Multiply);
                    i += 1;
                }
            }
            '/' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::DivAssign);
                    i += 2;
                } else {
                    tokens.push(Token::Divide);
                    i += 1;
                }
            }
            '%' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::ModAssign);
                    i += 2;
                } else {
                    tokens.push(Token::Mod);
                    i += 1;
                }
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
                if i + 2 < chars.len() && chars[i + 1] == '.' && chars[i + 2] == '.' {
                    tokens.push(Token::Spread);
                    i += 3;
                } else {
                    tokens.push(Token::Dot);
                    i += 1;
                }
            }
            '?' => {
                // Recognize '??=' (nullish coalescing assignment), '??' (nullish coalescing), and '?.' (optional chaining)
                if i + 2 < chars.len() && chars[i + 1] == '?' && chars[i + 2] == '=' {
                    tokens.push(Token::NullishAssign);
                    i += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '?' {
                    tokens.push(Token::NullishCoalescing);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '.' {
                    tokens.push(Token::OptionalChain);
                    i += 2;
                } else {
                    return Err(JSError::TokenizationError);
                }
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
                } else if i + 1 < chars.len() && chars[i + 1] == '>' {
                    tokens.push(Token::Arrow);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '+' {
                    tokens.push(Token::AddAssign);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '-' {
                    tokens.push(Token::SubAssign);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '*' {
                    tokens.push(Token::MulAssign);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '/' {
                    tokens.push(Token::DivAssign);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '%' {
                    tokens.push(Token::ModAssign);
                    i += 2;
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
            '&' => {
                // Recognize '&&=' (logical AND assignment) and '&&' (logical AND)
                if i + 2 < chars.len() && chars[i + 1] == '&' && chars[i + 2] == '=' {
                    tokens.push(Token::LogicalAndAssign);
                    i += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '&' {
                    tokens.push(Token::LogicalAnd);
                    i += 2;
                } else {
                    return Err(JSError::TokenizationError);
                }
            }
            '|' => {
                // Recognize '||=' (logical OR assignment) and '||' (logical OR)
                if i + 2 < chars.len() && chars[i + 1] == '|' && chars[i + 2] == '=' {
                    tokens.push(Token::LogicalOrAssign);
                    i += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '|' {
                    tokens.push(Token::LogicalOr);
                    i += 2;
                } else {
                    return Err(JSError::TokenizationError);
                }
            }
            '0'..='9' => {
                let start = i;
                // integer and fractional part
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                // optional exponent part
                if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                    let mut j = i + 1;
                    // optional sign after e/E
                    if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                        j += 1;
                    }
                    // require at least one digit in exponent
                    if j >= chars.len() || !chars[j].is_ascii_digit() {
                        return Err(JSError::TokenizationError);
                    }
                    // consume exponent digits
                    while j < chars.len() && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                    i = j;
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
                    "const" => tokens.push(Token::Const),
                    "class" => tokens.push(Token::Class),
                    "extends" => tokens.push(Token::Extends),
                    "super" => tokens.push(Token::Super),
                    "this" => tokens.push(Token::This),
                    "static" => tokens.push(Token::Static),
                    "new" => tokens.push(Token::New),
                    "instanceof" => tokens.push(Token::InstanceOf),
                    "typeof" => tokens.push(Token::TypeOf),
                    "delete" => tokens.push(Token::Delete),
                    "void" => tokens.push(Token::Void),
                    "in" => tokens.push(Token::In),
                    "try" => tokens.push(Token::Try),
                    "catch" => tokens.push(Token::Catch),
                    "finally" => tokens.push(Token::Finally),
                    "throw" => tokens.push(Token::Throw),
                    "function" => tokens.push(Token::Function),
                    "return" => tokens.push(Token::Return),
                    "if" => tokens.push(Token::If),
                    "else" => tokens.push(Token::Else),
                    "for" => tokens.push(Token::For),
                    "while" => tokens.push(Token::While),
                    "do" => tokens.push(Token::Do),
                    "switch" => tokens.push(Token::Switch),
                    "case" => tokens.push(Token::Case),
                    "default" => tokens.push(Token::Default),
                    "break" => tokens.push(Token::Break),
                    "continue" => tokens.push(Token::Continue),
                    "true" => tokens.push(Token::True),
                    "false" => tokens.push(Token::False),
                    "async" => tokens.push(Token::Async),
                    "await" => tokens.push(Token::Await),
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
    Const,
    Class,
    Extends,
    Super,
    This,
    Static,
    New,
    InstanceOf,
    TypeOf,
    In,
    Delete,
    Void,
    Function,
    Return,
    If,
    Else,
    For,
    While,
    Do,
    Switch,
    Case,
    Default,
    Break,
    Continue,
    Try,
    Catch,
    Finally,
    Throw,
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
    Arrow,
    Spread,
    OptionalChain,
    NullishCoalescing,
    LogicalAnd,
    LogicalOr,
    LogicalAndAssign,
    LogicalOrAssign,
    NullishAssign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    Increment,
    Decrement,
    Async,
    Await,
}

impl Token {
    /// Get the string representation of a token that can be used as an identifier/property name
    pub fn as_identifier_string(&self) -> Option<String> {
        match self {
            Token::Identifier(s) => Some(s.clone()),
            Token::Let => Some("let".to_string()),
            Token::Var => Some("var".to_string()),
            Token::Const => Some("const".to_string()),
            Token::Class => Some("class".to_string()),
            Token::Extends => Some("extends".to_string()),
            Token::Super => Some("super".to_string()),
            Token::This => Some("this".to_string()),
            Token::Static => Some("static".to_string()),
            Token::New => Some("new".to_string()),
            Token::InstanceOf => Some("instanceof".to_string()),
            Token::TypeOf => Some("typeof".to_string()),
            Token::In => Some("in".to_string()),
            Token::Delete => Some("delete".to_string()),
            Token::Void => Some("void".to_string()),
            Token::Function => Some("function".to_string()),
            Token::Return => Some("return".to_string()),
            Token::If => Some("if".to_string()),
            Token::Else => Some("else".to_string()),
            Token::For => Some("for".to_string()),
            Token::While => Some("while".to_string()),
            Token::Do => Some("do".to_string()),
            Token::Switch => Some("switch".to_string()),
            Token::Case => Some("case".to_string()),
            Token::Default => Some("default".to_string()),
            Token::Break => Some("break".to_string()),
            Token::Continue => Some("continue".to_string()),
            Token::Try => Some("try".to_string()),
            Token::Catch => Some("catch".to_string()),
            Token::Finally => Some("finally".to_string()),
            Token::Throw => Some("throw".to_string()),
            Token::True => Some("true".to_string()),
            Token::False => Some("false".to_string()),
            Token::Async => Some("async".to_string()),
            Token::Await => Some("await".to_string()),
            _ => None,
        }
    }
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
        Value::ClassDefinition(_) => true,
        Value::Getter(_, _) => true,
        Value::Setter(_, _, _) => true,
        Value::Property { .. } => true,
        Value::Promise(_) => true,
    }
}

fn parse_parameters(tokens: &mut Vec<Token>) -> Result<Vec<String>, JSError> {
    let mut params = Vec::new();
    if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
        loop {
            if let Some(Token::Identifier(param)) = tokens.first().cloned() {
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
    Ok(params)
}

fn parse_statement_block(tokens: &mut Vec<Token>) -> Result<Vec<Statement>, JSError> {
    let body = parse_statements(tokens)?;
    if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
        return Err(JSError::ParseError);
    }
    tokens.remove(0); // consume }
    Ok(body)
}

fn parse_expression(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    parse_assignment(tokens)
}

fn parse_assignment(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let left = parse_nullish(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    match &tokens[0] {
        Token::Assign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::Assign(Box::new(left), Box::new(right)))
        }
        Token::LogicalAndAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::LogicalAndAssign(Box::new(left), Box::new(right)))
        }
        Token::LogicalOrAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::LogicalOrAssign(Box::new(left), Box::new(right)))
        }
        Token::NullishAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::NullishAssign(Box::new(left), Box::new(right)))
        }
        Token::AddAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::AddAssign(Box::new(left), Box::new(right)))
        }
        Token::SubAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::SubAssign(Box::new(left), Box::new(right)))
        }
        Token::MulAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::MulAssign(Box::new(left), Box::new(right)))
        }
        Token::DivAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::DivAssign(Box::new(left), Box::new(right)))
        }
        Token::ModAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::ModAssign(Box::new(left), Box::new(right)))
        }
        _ => Ok(left),
    }
}

fn parse_nullish(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let left = parse_comparison(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    if matches!(tokens[0], Token::NullishCoalescing) {
        tokens.remove(0);
        let right = parse_nullish(tokens)?;
        Ok(Expr::Binary(Box::new(left), BinaryOp::NullishCoalescing, Box::new(right)))
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
        Token::InstanceOf => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::InstanceOf, Box::new(right)))
        }
        Token::In => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::In, Box::new(right)))
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
        Token::TypeOf => {
            let inner = parse_primary(tokens)?;
            Expr::TypeOf(Box::new(inner))
        }
        Token::Delete => {
            let inner = parse_primary(tokens)?;
            Expr::Delete(Box::new(inner))
        }
        Token::Void => {
            let inner = parse_primary(tokens)?;
            Expr::Void(Box::new(inner))
        }
        Token::Await => {
            let inner = parse_primary(tokens)?;
            Expr::Await(Box::new(inner))
        }
        Token::New => {
            // Constructor should be a simple identifier or property access, not a full expression
            let constructor = if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                tokens.remove(0);
                Expr::Var(name)
            } else {
                return Err(JSError::ParseError);
            };
            let args = if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
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
                args
            } else {
                Vec::new()
            };
            Expr::New(Box::new(constructor), args)
        }
        Token::Minus => {
            let inner = parse_primary(tokens)?;
            Expr::UnaryNeg(Box::new(inner))
        }
        Token::Increment => {
            let inner = parse_primary(tokens)?;
            Expr::Increment(Box::new(inner))
        }
        Token::Decrement => {
            let inner = parse_primary(tokens)?;
            Expr::Decrement(Box::new(inner))
        }
        Token::Spread => {
            let inner = parse_primary(tokens)?;
            Expr::Spread(Box::new(inner))
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
        Token::Identifier(name) => {
            let mut expr = Expr::Var(name.clone());
            if !tokens.is_empty() && matches!(tokens[0], Token::Arrow) {
                tokens.remove(0);
                let body = parse_arrow_body(tokens)?;
                expr = Expr::ArrowFunction(vec![name], body);
            }
            expr
        }
        Token::This => Expr::This,
        Token::Super => {
            // Check if followed by ( for super() call
            if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
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
                Expr::SuperCall(args)
            } else if !tokens.is_empty() && matches!(tokens[0], Token::Dot) {
                tokens.remove(0); // consume '.'
                if tokens.is_empty() || !matches!(tokens[0], Token::Identifier(_)) {
                    return Err(JSError::ParseError);
                }
                let prop = if let Token::Identifier(name) = tokens.remove(0) {
                    name
                } else {
                    return Err(JSError::ParseError);
                };
                // Check if followed by ( for method call
                if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
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
                    Expr::SuperMethod(prop, args)
                } else {
                    Expr::SuperProperty(prop)
                }
            } else {
                Expr::Super
            }
        }
        Token::LBrace => {
            // Parse object literal
            let mut properties = Vec::new();
            if !tokens.is_empty() && matches!(tokens[0], Token::RBrace) {
                // Empty object {}
                tokens.remove(0); // consume }
                return Ok(Expr::Object(properties));
            }
            loop {
                // Check for spread
                if !tokens.is_empty() && matches!(tokens[0], Token::Spread) {
                    tokens.remove(0); // consume ...
                    let expr = parse_expression(tokens)?;
                    properties.push(("".to_string(), Expr::Spread(Box::new(expr))));
                } else {
                    // Check for getter/setter
                    let is_getter = !tokens.is_empty() && matches!(tokens[0], Token::Identifier(ref id) if id == "get");
                    let is_setter = !tokens.is_empty() && matches!(tokens[0], Token::Identifier(ref id) if id == "set");

                    if is_getter || is_setter {
                        tokens.remove(0); // consume get/set
                    }

                    // Parse key
                    let key = if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                        tokens.remove(0);
                        name
                    } else if let Some(Token::StringLit(s)) = tokens.first().cloned() {
                        tokens.remove(0);
                        String::from_utf16_lossy(&s)
                    } else {
                        return Err(JSError::ParseError);
                    };

                    // Expect colon or parentheses for getter/setter
                    if is_getter || is_setter {
                        // Parse function for getter/setter
                        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
                            return Err(JSError::ParseError);
                        }
                        tokens.remove(0); // consume (

                        let mut params = Vec::new();
                        if is_setter {
                            // Setter should have exactly one parameter
                            if let Some(Token::Identifier(param)) = tokens.first().cloned() {
                                tokens.remove(0);
                                params.push(param);
                            } else {
                                return Err(JSError::ParseError);
                            }
                        } else if is_getter {
                            // Getter should have no parameters
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

                        if is_getter {
                            properties.push((key, Expr::Getter(Box::new(Expr::Function(params, body)))));
                        } else {
                            properties.push((key, Expr::Setter(Box::new(Expr::Function(params, body)))));
                        }
                    } else {
                        // Regular property
                        if tokens.is_empty() || !matches!(tokens[0], Token::Colon) {
                            return Err(JSError::ParseError);
                        }
                        tokens.remove(0); // consume :

                        // Parse value
                        let value = parse_expression(tokens)?;
                        properties.push((key, value));
                    }
                }

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
            if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
                tokens.remove(0); // consume (
                let mut params = Vec::new();
                if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
                    loop {
                        if let Some(Token::Identifier(param)) = tokens.first().cloned() {
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
        Token::Async => {
            // Check if followed by function or arrow function parameters
            if !tokens.is_empty() && matches!(tokens[0], Token::Function) {
                tokens.remove(0); // consume function
                if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
                    tokens.remove(0); // consume (
                    let mut params = Vec::new();
                    if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
                        loop {
                            if let Some(Token::Identifier(param)) = tokens.first().cloned() {
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
                    Expr::AsyncFunction(params, body)
                } else {
                    return Err(JSError::ParseError);
                }
            } else if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
                // Async arrow function
                tokens.remove(0); // consume (
                let mut params = Vec::new();
                let mut is_arrow = false;
                if matches!(tokens.first(), Some(&Token::RParen)) {
                    tokens.remove(0);
                    if !tokens.is_empty() && matches!(tokens[0], Token::Arrow) {
                        tokens.remove(0);
                        is_arrow = true;
                    } else {
                        return Err(JSError::ParseError);
                    }
                } else {
                    // Try to parse params
                    let mut param_names = Vec::new();
                    let mut local_consumed = Vec::new();
                    let mut valid = true;
                    loop {
                        if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                            tokens.remove(0);
                            local_consumed.push(Token::Identifier(name.clone()));
                            param_names.push(name);
                            if tokens.is_empty() {
                                valid = false;
                                break;
                            }
                            if matches!(tokens[0], Token::RParen) {
                                tokens.remove(0);
                                local_consumed.push(Token::RParen);
                                if !tokens.is_empty() && matches!(tokens[0], Token::Arrow) {
                                    tokens.remove(0);
                                    is_arrow = true;
                                } else {
                                    valid = false;
                                }
                                break;
                            } else if matches!(tokens[0], Token::Comma) {
                                tokens.remove(0);
                                local_consumed.push(Token::Comma);
                            } else {
                                valid = false;
                                break;
                            }
                        } else {
                            valid = false;
                            break;
                        }
                    }
                    if !valid || !is_arrow {
                        // Put back local_consumed
                        for t in local_consumed.into_iter().rev() {
                            tokens.insert(0, t);
                        }
                        return Err(JSError::ParseError);
                    }
                    params = param_names;
                }
                if is_arrow {
                    // For async arrow functions, we need to create a special async closure
                    // For now, we'll treat them as regular arrow functions but mark them as async
                    // This will need to be handled in evaluation
                    Expr::ArrowFunction(params, parse_arrow_body(tokens)?)
                } else {
                    return Err(JSError::ParseError);
                }
            } else {
                return Err(JSError::ParseError);
            }
        }
        Token::LParen => {
            // Check if it's arrow function
            let mut params = Vec::new();
            let mut is_arrow = false;
            let mut result_expr = None;
            if matches!(tokens.first(), Some(&Token::RParen)) {
                tokens.remove(0);
                if !tokens.is_empty() && matches!(tokens[0], Token::Arrow) {
                    tokens.remove(0);
                    is_arrow = true;
                } else {
                    return Err(JSError::ParseError);
                }
            } else {
                // Try to parse params
                let mut param_names = Vec::new();
                let mut local_consumed = Vec::new();
                let mut valid = true;
                loop {
                    if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                        tokens.remove(0);
                        local_consumed.push(Token::Identifier(name.clone()));
                        param_names.push(name);
                        if tokens.is_empty() {
                            valid = false;
                            break;
                        }
                        if matches!(tokens[0], Token::RParen) {
                            tokens.remove(0);
                            local_consumed.push(Token::RParen);
                            if !tokens.is_empty() && matches!(tokens[0], Token::Arrow) {
                                tokens.remove(0);
                                is_arrow = true;
                            } else {
                                valid = false;
                            }
                            break;
                        } else if matches!(tokens[0], Token::Comma) {
                            tokens.remove(0);
                            local_consumed.push(Token::Comma);
                        } else {
                            valid = false;
                            break;
                        }
                    } else {
                        valid = false;
                        break;
                    }
                }
                if valid && is_arrow {
                    params = param_names;
                } else {
                    // Put back local_consumed
                    for t in local_consumed.into_iter().rev() {
                        tokens.insert(0, t);
                    }
                    // Parse as expression
                    let expr_inner = parse_expression(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                        return Err(JSError::ParseError);
                    }
                    tokens.remove(0);
                    result_expr = Some(expr_inner);
                }
            }
            if is_arrow {
                Expr::ArrowFunction(params, parse_arrow_body(tokens)?)
            } else {
                result_expr.unwrap()
            }
        }
        _ => {
            return Err(JSError::EvaluationError {
                message: "error".to_string(),
            });
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
                if tokens.is_empty() {
                    return Err(JSError::ParseError);
                }
                if let Some(prop) = tokens[0].as_identifier_string() {
                    tokens.remove(0);
                    expr = Expr::Property(Box::new(expr), prop);
                } else {
                    return Err(JSError::ParseError);
                }
            }
            Token::OptionalChain => {
                tokens.remove(0); // consume '?.'
                if tokens.is_empty() {
                    return Err(JSError::ParseError);
                }
                if matches!(tokens[0], Token::LParen) {
                    // Optional call: obj?.method(args)
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
                    expr = Expr::OptionalCall(Box::new(expr), args);
                } else if matches!(tokens[0], Token::Identifier(_)) {
                    // Optional property access: obj?.prop
                    if let Some(prop) = tokens[0].as_identifier_string() {
                        tokens.remove(0);
                        expr = Expr::OptionalProperty(Box::new(expr), prop);
                    } else {
                        return Err(JSError::ParseError);
                    }
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
            Token::Increment => {
                tokens.remove(0);
                expr = Expr::PostIncrement(Box::new(expr));
            }
            Token::Decrement => {
                tokens.remove(0);
                expr = Expr::PostDecrement(Box::new(expr));
            }
            _ => break,
        }
    }

    Ok(expr)
}

fn parse_arrow_body(tokens: &mut Vec<Token>) -> Result<Vec<Statement>, JSError> {
    if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
        tokens.remove(0);
        let body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0);
        Ok(body)
    } else {
        let expr = parse_expression(tokens)?;
        Ok(vec![Statement::Return(Some(expr))])
    }
}

fn parse_array_destructuring_pattern(tokens: &mut Vec<Token>) -> Result<Vec<DestructuringElement>, JSError> {
    if tokens.is_empty() || !matches!(tokens[0], Token::LBracket) {
        return Err(JSError::ParseError);
    }
    tokens.remove(0); // consume [

    let mut pattern = Vec::new();
    if !tokens.is_empty() && matches!(tokens[0], Token::RBracket) {
        tokens.remove(0); // consume ]
        return Ok(pattern);
    }

    loop {
        if !tokens.is_empty() && matches!(tokens[0], Token::Spread) {
            tokens.remove(0); // consume ...
            if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                tokens.remove(0);
                pattern.push(DestructuringElement::Rest(name));
            } else {
                return Err(JSError::ParseError);
            }
            // Rest must be the last element
            if tokens.is_empty() || !matches!(tokens[0], Token::RBracket) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume ]
            break;
        } else if !tokens.is_empty() && matches!(tokens[0], Token::Comma) {
            tokens.remove(0); // consume ,
            pattern.push(DestructuringElement::Empty);
        } else if !tokens.is_empty() && matches!(tokens[0], Token::LBracket) {
            // Nested array destructuring
            let nested_pattern = parse_array_destructuring_pattern(tokens)?;
            pattern.push(DestructuringElement::NestedArray(nested_pattern));
        } else if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
            // Nested object destructuring
            let nested_pattern = parse_object_destructuring_pattern(tokens)?;
            pattern.push(DestructuringElement::NestedObject(nested_pattern));
        } else if let Some(Token::Identifier(name)) = tokens.first().cloned() {
            tokens.remove(0);
            pattern.push(DestructuringElement::Variable(name));
        } else {
            return Err(JSError::ParseError);
        }

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

    Ok(pattern)
}

fn parse_object_destructuring_pattern(tokens: &mut Vec<Token>) -> Result<Vec<ObjectDestructuringElement>, JSError> {
    if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
        return Err(JSError::ParseError);
    }
    tokens.remove(0); // consume {

    let mut pattern = Vec::new();
    if !tokens.is_empty() && matches!(tokens[0], Token::RBrace) {
        tokens.remove(0); // consume }
        return Ok(pattern);
    }

    loop {
        if !tokens.is_empty() && matches!(tokens[0], Token::Spread) {
            tokens.remove(0); // consume ...
            if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                tokens.remove(0);
                pattern.push(ObjectDestructuringElement::Rest(name));
            } else {
                return Err(JSError::ParseError);
            }
            // Rest must be the last element
            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume }
            break;
        } else {
            // Parse property
            let key = if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                tokens.remove(0);
                name
            } else {
                return Err(JSError::ParseError);
            };

            let value = if !tokens.is_empty() && matches!(tokens[0], Token::Colon) {
                tokens.remove(0); // consume :
                // Parse the value pattern
                if !tokens.is_empty() && matches!(tokens[0], Token::LBracket) {
                    DestructuringElement::NestedArray(parse_array_destructuring_pattern(tokens)?)
                } else if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
                    DestructuringElement::NestedObject(parse_object_destructuring_pattern(tokens)?)
                } else if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                    tokens.remove(0);
                    DestructuringElement::Variable(name)
                } else {
                    return Err(JSError::ParseError);
                }
            } else {
                // Shorthand: key is the same as variable name
                DestructuringElement::Variable(key.clone())
            };

            pattern.push(ObjectDestructuringElement::Property { key, value });
        }

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

    Ok(pattern)
}

/// # Safety
/// The caller must ensure that `_ctx` is a valid pointer to a JSContext and `this_obj` is a valid JSValue.
pub unsafe fn JS_GetProperty(_ctx: *mut JSContext, this_obj: JSValue, prop: JSAtom) -> JSValue {
    unsafe {
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
}

// Reference-count helpers: basic dup/free on objects/strings that store a ref_count
// NOTE: This is a minimal implementation. Proper finalizers and nested frees
// are not implemented here and should be added per object type.
/// # Safety
/// The caller must ensure that `v` is a valid JSValue and `_rt` is a valid JSRuntime pointer.
pub unsafe fn JS_DupValue(_rt: *mut JSRuntime, v: JSValue) {
    unsafe {
        if v.has_ref_count() {
            let p = v.get_ptr();
            if !p.is_null() {
                let header = p as *mut JSRefCountHeader;
                (*header).ref_count += 1;
            }
        }
    }
}

/// # Safety
/// The caller must ensure that `rt` is a valid JSRuntime pointer and `v` is a valid JSValue.
pub unsafe fn JS_FreeValue(rt: *mut JSRuntime, v: JSValue) {
    unsafe {
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
                    (*rt).js_free_rt(p);
                }
            }
        }
    }
}

unsafe fn js_free_string(rt: *mut JSRuntime, v: JSValue) {
    unsafe {
        let p = v.get_ptr() as *mut JSString;
        if p.is_null() {
            return;
        }
        // The whole JSString allocation was allocated via js_malloc_rt
        (*rt).js_free_rt(p as *mut c_void);
    }
}

unsafe fn js_free_object(rt: *mut JSRuntime, v: JSValue) {
    unsafe {
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
}

unsafe fn js_free_function_bytecode(rt: *mut JSRuntime, v: JSValue) {
    unsafe {
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
}

unsafe fn js_free_symbol(rt: *mut JSRuntime, v: JSValue) {
    let p = v.get_ptr();
    if p.is_null() {
        return;
    }
    // Symbols typically store their name as a JSString or internal struct
    // For now, free the pointer directly. Add type-aware finalizer later.
    unsafe { (*rt).js_free_rt(p) };
}

unsafe fn js_free_bigint(rt: *mut JSRuntime, v: JSValue) {
    let p = v.get_ptr();
    if p.is_null() {
        return;
    }
    // BigInt representation may be inline or heap-allocated. Here we free pointer.
    unsafe { (*rt).js_free_rt(p) };
}

unsafe fn js_free_module(rt: *mut JSRuntime, v: JSValue) {
    let p = v.get_ptr();
    if p.is_null() {
        return;
    }
    // Module structure not modelled here; free pointer for now.
    unsafe { (*rt).js_free_rt(p) };
}

/// # Safety
/// The caller must ensure that `ctx` is a valid JSContext pointer, `this_obj` is a valid JSValue, and `prop` is a valid JSAtom.
pub unsafe fn JS_SetProperty(ctx: *mut JSContext, this_obj: JSValue, prop: JSAtom, val: JSValue) -> i32 {
    unsafe { JS_DefinePropertyValue(ctx, this_obj, prop, val, 0) }
}

impl JSRuntime {
    /// # Safety
    /// The caller must ensure that `name` points to a valid buffer of at least `len` bytes.
    pub unsafe fn js_new_atom_len(&mut self, name: *const u8, len: usize) -> JSAtom {
        if len == 0 {
            return 0; // invalid
        }
        // Compute hash
        let mut h = 0u32;
        for i in 0..len {
            h = h.wrapping_mul(31).wrapping_add(unsafe { *name.add(i) } as u32);
        }
        // Find in hash table
        let hash_index = (h % self.atom_hash_size as u32) as i32;
        let mut atom = unsafe { *self.atom_hash.offset(hash_index as isize) };
        while atom != 0 {
            let p = unsafe { *self.atom_array.offset((atom - 1) as isize) };
            if unsafe { (*p).len == len as u32 && (*p).hash == h } {
                // Check string
                let str_data = unsafe { (p as *mut u8).add(std::mem::size_of::<JSString>()) };
                let mut equal = true;
                for i in 0..len {
                    if unsafe { *str_data.add(i) != *name.add(i) } {
                        equal = false;
                        break;
                    }
                }
                if equal {
                    return atom;
                }
            }
            atom = unsafe { (*p).hash_next };
        }
        // Not found, create new
        if self.atom_count >= self.atom_size {
            let new_size = self.atom_size * 2;
            let new_array = unsafe {
                self.js_realloc_rt(
                    self.atom_array as *mut c_void,
                    (new_size as usize) * std::mem::size_of::<*mut JSAtomStruct>(),
                )
            } as *mut *mut JSAtomStruct;
            if new_array.is_null() {
                return 0;
            }
            self.atom_array = new_array;
            self.atom_size = new_size;
            for i in self.atom_count..new_size {
                unsafe { *self.atom_array.offset(i as isize) = std::ptr::null_mut() };
            }
        }
        // Allocate JSString
        let str_size = std::mem::size_of::<JSString>() + len;
        let p = unsafe { self.js_malloc_rt(str_size) } as *mut JSString;
        if p.is_null() {
            return 0;
        }
        unsafe { (*p).header.ref_count = 1 };
        unsafe { (*p).len = len as u32 };
        unsafe { (*p).hash = h };
        unsafe { (*p).hash_next = *self.atom_hash.offset(hash_index as isize) };
        // Copy string
        let str_data = unsafe { (p as *mut u8).add(std::mem::size_of::<JSString>()) };
        for i in 0..len {
            unsafe { *str_data.add(i) = *name.add(i) };
        }
        let new_atom = (self.atom_count + 1) as u32;
        unsafe { *self.atom_array.offset(self.atom_count as isize) = p };
        unsafe { *self.atom_hash.offset(hash_index as isize) = new_atom };
        self.atom_count += 1;
        new_atom
    }
}

fn filter_input_script(script: &str) -> String {
    // Remove comments and simple import lines that we've already handled via shim injection
    let mut filtered = String::new();
    let chars: Vec<char> = script.chars().collect();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    let mut escape = false;

    while i < chars.len() {
        let ch = chars[i];

        // Handle escape sequences
        if escape {
            filtered.push(ch);
            escape = false;
            i += 1;
            continue;
        }
        if ch == '\\' {
            escape = true;
            filtered.push(ch);
            i += 1;
            continue;
        }

        // Handle quote states
        match ch {
            '\'' if !in_double && !in_backtick => {
                in_single = !in_single;
                filtered.push(ch);
                i += 1;
                continue;
            }
            '"' if !in_single && !in_backtick => {
                in_double = !in_double;
                filtered.push(ch);
                i += 1;
                continue;
            }
            '`' if !in_single && !in_double => {
                in_backtick = !in_backtick;
                filtered.push(ch);
                i += 1;
                continue;
            }
            _ => {}
        }

        // Only process comments when not inside quotes
        if !in_single && !in_double && !in_backtick {
            // Handle single-line comments: //
            if i + 1 < chars.len() && ch == '/' && chars[i + 1] == '/' {
                // Skip to end of line
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                // Don't add the newline yet, continue to next iteration
                continue;
            }

            // Handle multi-line comments: /* */
            if i + 1 < chars.len() && ch == '/' && chars[i + 1] == '*' {
                i += 2; // Skip /*
                while i + 1 < chars.len() {
                    if chars[i] == '*' && chars[i + 1] == '/' {
                        i += 2; // Skip */
                        break;
                    }
                    i += 1;
                }
                continue;
            }
        }

        // Handle regular characters and newlines
        filtered.push(ch);
        i += 1;
    }

    // Now process the filtered script line by line for import statements
    let mut final_filtered = String::new();
    for (i, line) in filtered.lines().enumerate() {
        // Split line on semicolons only when not inside quotes/backticks
        let mut current = String::new();
        let mut in_single = false;
        let mut in_double = false;
        let mut in_backtick = false;
        let mut escape = false;
        // track parts along with whether they were followed by a semicolon
        let mut parts: Vec<(String, bool)> = Vec::new();
        for ch in line.chars() {
            if escape {
                current.push(ch);
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                current.push(ch);
                continue;
            }
            match ch {
                '\'' if !in_double && !in_backtick => {
                    in_single = !in_single;
                    current.push(ch);
                    continue;
                }
                '"' if !in_single && !in_backtick => {
                    in_double = !in_double;
                    current.push(ch);
                    continue;
                }
                '`' if !in_single && !in_double => {
                    in_backtick = !in_backtick;
                    current.push(ch);
                    continue;
                }
                _ => {}
            }
            if ch == ';' && !in_single && !in_double && !in_backtick {
                parts.push((current.clone(), true));
                current.clear();
                continue;
            }
            current.push(ch);
        }
        // If there is a trailing part (possibly no trailing semicolon), add it
        if !current.is_empty() {
            parts.push((current, false));
        }

        for (part, had_semicolon) in parts.iter() {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            log::trace!("script part[{i}]='{p}'");
            if p.starts_with("import * as") && p.contains("from") {
                log::debug!("skipping import part[{i}]: \"{p}\"");
                continue;
            }
            final_filtered.push_str(p);
            // Re-add semicolon if the original part was followed by a semicolon
            if *had_semicolon {
                final_filtered.push(';');
            }
        }
        final_filtered.push('\n');
    }

    // Remove any trailing newline(s) added during filtering to avoid an extra
    // empty statement at the end when tokenizing/parsing.
    final_filtered.trim_end_matches('\n').to_string()
}

/// Initialize global built-in constructors in the environment
fn initialize_global_constructors(env: &JSObjectDataPtr) {
    let mut env_borrow = env.borrow_mut();

    // Object constructor
    env_borrow.insert("Object".to_string(), Rc::new(RefCell::new(Value::Function("Object".to_string()))));

    // Number constructor - handled by evaluate_var
    // env_borrow.insert("Number".to_string(), Rc::new(RefCell::new(Value::Function("Number".to_string()))));

    // Boolean constructor
    env_borrow.insert("Boolean".to_string(), Rc::new(RefCell::new(Value::Function("Boolean".to_string()))));

    // String constructor
    env_borrow.insert("String".to_string(), Rc::new(RefCell::new(Value::Function("String".to_string()))));

    // Array constructor (already handled by js_array module)
    env_borrow.insert("Array".to_string(), Rc::new(RefCell::new(Value::Function("Array".to_string()))));

    // Date constructor (already handled by js_date module)
    env_borrow.insert("Date".to_string(), Rc::new(RefCell::new(Value::Function("Date".to_string()))));

    // RegExp constructor (already handled by js_regexp module)
    env_borrow.insert("RegExp".to_string(), Rc::new(RefCell::new(Value::Function("RegExp".to_string()))));

    // Internal promise resolution functions
    env_borrow.insert(
        "__internal_resolve_promise".to_string(),
        Rc::new(RefCell::new(Value::Function("__internal_resolve_promise".to_string()))),
    );
    env_borrow.insert(
        "__internal_reject_promise".to_string(),
        Rc::new(RefCell::new(Value::Function("__internal_reject_promise".to_string()))),
    );
    env_borrow.insert(
        "__internal_allsettled_state_record_fulfilled".to_string(),
        Rc::new(RefCell::new(Value::Function(
            "__internal_allsettled_state_record_fulfilled".to_string(),
        ))),
    );
    env_borrow.insert(
        "__internal_allsettled_state_record_rejected".to_string(),
        Rc::new(RefCell::new(Value::Function(
            "__internal_allsettled_state_record_rejected".to_string(),
        ))),
    );
}

/// Expand spread operator in function call arguments
fn expand_spread_in_call_args(env: &JSObjectDataPtr, args: &[Expr], evaluated_args: &mut Vec<Value>) -> Result<(), JSError> {
    for arg_expr in args {
        if let Expr::Spread(spread_expr) = arg_expr {
            let spread_val = evaluate_expr(env, spread_expr)?;
            if let Value::Object(spread_obj) = spread_val {
                // Assume it's an array-like object
                let mut i = 0;
                loop {
                    let key = i.to_string();
                    if let Some(val) = obj_get_value(&spread_obj, &key)? {
                        evaluated_args.push(val.borrow().clone());
                        i += 1;
                    } else {
                        break;
                    }
                }
            } else {
                return Err(JSError::EvaluationError {
                    message: "Spread operator can only be applied to arrays in function calls".to_string(),
                });
            }
        } else {
            let arg_val = evaluate_expr(env, arg_expr)?;
            evaluated_args.push(arg_val);
        }
    }
    Ok(())
}

/// Handle optional method call on an object, Similar logic to regular method call but for optional
fn handle_optional_method_call(
    obj_map: &JSObjectDataPtr,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr,
    obj_expr: &Expr,
) -> Result<Value, JSError> {
    match method {
        "log" if obj_map.borrow().contains_key("log") => js_console::handle_console_method(method, args, env),
        "toString" => crate::js_object::handle_to_string_method(&Value::Object(obj_map.clone()), args),
        "valueOf" => crate::js_object::handle_value_of_method(&Value::Object(obj_map.clone()), args),
        method => {
            // If this object looks like the `std` module (we used 'sprintf' as marker)
            if obj_map.borrow().contains_key("sprintf") {
                match method {
                    "sprintf" => {
                        log::trace!("js dispatch calling sprintf with {} args", args.len());
                        sprintf::handle_sprintf_call(env, args)
                    }
                    "tmpfile" => tmpfile::create_tmpfile(),
                    _ => Ok(Value::Undefined),
                }
            } else if obj_map.borrow().contains_key("open") {
                // If this object looks like the `os` module (we used 'open' as marker)
                crate::js_os::handle_os_method(obj_map, method, args, env)
            } else if obj_map.borrow().contains_key("join") {
                // If this object looks like the `os.path` module
                crate::js_os::handle_os_method(obj_map, method, args, env)
            } else if obj_map.borrow().contains_key("__file_id") {
                // If this object is a file-like object (we use '__file_id' as marker)
                tmpfile::handle_file_method(obj_map, method, args, env)
            } else if obj_map.borrow().contains_key("PI") && obj_map.borrow().contains_key("E") {
                // Check if this is the Math object
                js_math::handle_math_method(method, args, env)
            } else if obj_map.borrow().contains_key("parse") && obj_map.borrow().contains_key("stringify") {
                crate::js_json::handle_json_method(method, args, env)
            } else if obj_map.borrow().contains_key("keys") && obj_map.borrow().contains_key("values") {
                crate::js_object::handle_object_method(method, args, env)
            } else if obj_map.borrow().contains_key("__timestamp") {
                // Date instance methods
                crate::js_date::handle_date_method(obj_map, method, args)
            } else if obj_map.borrow().contains_key("__regex") {
                // RegExp instance methods
                crate::js_regexp::handle_regexp_method(obj_map, method, args, env)
            } else if is_array(obj_map) {
                // Array instance methods
                crate::js_array::handle_array_instance_method(obj_map, method, args, env, obj_expr)
            } else if obj_map.borrow().contains_key("__class_def__") {
                // Class static methods
                call_static_method(obj_map, method, args, env)
            } else if is_class_instance(obj_map)? {
                call_class_method(obj_map, method, args, env)
            } else {
                // Check for user-defined method
                if let Some(prop_val) = obj_get_value(obj_map, method)? {
                    match prop_val.borrow().clone() {
                        Value::Closure(params, body, captured_env) => {
                            // Function call
                            // Collect all arguments, expanding spreads
                            let mut evaluated_args = Vec::new();
                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                            if params.len() != evaluated_args.len() {
                                return Err(JSError::ParseError);
                            }
                            // Create new environment starting with captured environment
                            let func_env = captured_env.clone();
                            // Add parameters
                            for (param, arg_val) in params.iter().zip(evaluated_args.iter()) {
                                env_set(&func_env, param.as_str(), arg_val.clone())?;
                            }
                            // Execute function body
                            evaluate_statements(&func_env, &body)
                        }
                        Value::Function(func_name) => crate::js_function::handle_global_function(&func_name, args, env),
                        _ => Err(JSError::EvaluationError {
                            message: format!("Property '{}' is not a function", method),
                        }),
                    }
                } else {
                    Ok(Value::Undefined)
                }
            }
        }
    }
}
