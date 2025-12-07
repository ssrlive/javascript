use super::{Value, evaluate_script, utf16_to_utf8};
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
    // Linked list of objects which currently use this shape. Each object's
    // `next_in_shape` pointer links to the next object in the list.
    pub first_object: *mut JSObject,
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
    // Pointer to next object in the shape's object list
    pub next_in_shape: *mut JSObject,
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
    pub unsafe fn add_property(&mut self, sh: *mut JSShape, atom: JSAtom, flags: u8) -> (i32, i32) {
        unsafe {
            // Remember previous prop_size (before any possible resize)
            let prev_prop_size = (*sh).prop_size;

            // Check if property already exists
            if let Some((idx, _)) = (*sh).find_own_property(atom) {
                // Already exists
                return (idx, prev_prop_size);
            }

            if (*sh).prop_count >= (*sh).prop_size {
                let new_size = if (*sh).prop_size == 0 { 4 } else { (*sh).prop_size * 3 / 2 };
                // Resize the shape's property descriptor array
                if self.resize_shape(sh, new_size) < 0 {
                    return (-1, prev_prop_size);
                }

                // For all objects that currently use this shape, reallocate their
                // JSProperty arrays to match the new shape size so every object
                // has space for the newly added property slots.
                let mut obj_ptr = (*sh).first_object;
                while !obj_ptr.is_null() {
                    // keep next pointer in case reallocation moves memory
                    let next_obj = (*obj_ptr).next_in_shape;

                    let old_prop = (*obj_ptr).prop;
                    let new_prop_obj = self.js_realloc_rt(old_prop as *mut c_void, (new_size as usize) * std::mem::size_of::<JSProperty>())
                        as *mut JSProperty;

                    if new_prop_obj.is_null() {
                        return (-1, prev_prop_size);
                    }

                    // If we just allocated the array, zero-initialize all slots.
                    if old_prop.is_null() {
                        // Initialize all slots to JS_UNDEFINED (correct default), avoid raw zeros
                        for i in 0..(new_size as isize) {
                            (*new_prop_obj.offset(i)).u.value = JS_UNDEFINED;
                        }
                    } else if new_size > prev_prop_size {
                        // Zero only the newly-added slots
                        let start_index = prev_prop_size as usize;
                        let new_slots = (new_size - prev_prop_size) as usize;
                        if new_slots > 0 {
                            for i in start_index..(start_index + new_slots) {
                                (*new_prop_obj.add(i)).u.value = JS_UNDEFINED;
                            }
                        }
                    }

                    (*obj_ptr).prop = new_prop_obj;
                    obj_ptr = next_obj;
                }
            }

            // Enable hash if needed
            if (*sh).prop_count >= 4 && (*sh).is_hashed == 0 {
                (*sh).is_hashed = 1;
                (*sh).prop_hash_mask = 15; // 16 - 1
                let hash_size = 16;
                (*sh).prop_hash = self.js_malloc_rt(hash_size * std::mem::size_of::<u32>()) as *mut u32;
                if (*sh).prop_hash.is_null() {
                    return (-1, prev_prop_size);
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

            // return index and previous prop_size so callers can resize object prop arrays
            (idx, prev_prop_size)
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
            (*sh).first_object = std::ptr::null_mut();
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

    let (idx, _prev_prop_size) = unsafe { (*(*ctx).rt).add_property(sh, prop, flags as u8) };
    if idx < 0 {
        return -1;
    }

    // After add_property we should have ensured the shape has enough capacity and
    // that objects using this shape have their prop arrays resized. However it's
    // still possible this particular object has no prop array (for example a
    // newly created object that was not present when the shape grew). In that
    // case allocate a prop array sized to the current shape.
    let old_prop = unsafe { (*p).prop };
    if old_prop.is_null() && unsafe { (*sh).prop_size } > 0 {
        let size = unsafe { (*sh).prop_size as usize } * std::mem::size_of::<JSProperty>();
        let new_prop = unsafe { (*(*ctx).rt).js_realloc_rt(std::ptr::null_mut(), size) as *mut JSProperty };
        if new_prop.is_null() {
            return -1;
        }
        unsafe {
            (*p).prop = new_prop;
            // initialize slots to JS_UNDEFINED
            let n = (*sh).prop_size as isize;
            for i in 0..n {
                (*new_prop.offset(i)).u.value = JS_UNDEFINED;
            }
        }
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
        (*obj).next_in_shape = std::ptr::null_mut();

        // Link object into its shape's object list so that when the shape changes
        // (for example when its prop_size grows) we can update all objects using
        // the same shape.
        if !(*obj).shape.is_null() {
            let sh = (*obj).shape;
            // prepend to shape's list
            (*obj).next_in_shape = (*sh).first_object;
            (*sh).first_object = obj;
        }
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
            Ok(Value::Object(_)) => JS_UNDEFINED,             // For now
            Ok(Value::Function(_)) => JS_UNDEFINED,           // For now
            Ok(Value::Closure(_, _, _)) => JS_UNDEFINED,      // For now
            Ok(Value::AsyncClosure(_, _, _)) => JS_UNDEFINED, // For now
            Ok(Value::ClassDefinition(_)) => JS_UNDEFINED,    // For now
            Ok(Value::Getter(_, _)) => JS_UNDEFINED,          // For now
            Ok(Value::Setter(_, _, _)) => JS_UNDEFINED,       // For now
            Ok(Value::Property { .. }) => JS_UNDEFINED,       // For now
            Ok(Value::Promise(_)) => JS_UNDEFINED,            // For now
            Ok(Value::Symbol(_)) => JS_UNDEFINED,             // For now
            Ok(Value::BigInt(_)) => JS_UNDEFINED,
            Ok(Value::Map(_)) => JS_UNDEFINED,                     // For now
            Ok(Value::Set(_)) => JS_UNDEFINED,                     // For now
            Ok(Value::WeakMap(_)) => JS_UNDEFINED,                 // For now
            Ok(Value::WeakSet(_)) => JS_UNDEFINED,                 // For now
            Ok(Value::GeneratorFunction(_, _, _)) => JS_UNDEFINED, // For now
            Ok(Value::Generator(_)) => JS_UNDEFINED,               // For now
            Ok(Value::Proxy(_)) => JS_UNDEFINED,                   // For now
            Ok(Value::ArrayBuffer(_)) => JS_UNDEFINED,             // For now
            Ok(Value::DataView(_)) => JS_UNDEFINED,                // For now
            Ok(Value::TypedArray(_)) => JS_UNDEFINED,              // For now
            Err(_) => JS_UNDEFINED,
        }
    }
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
        // Unlink from shape's object list and free shape only if no objects remain
        if !(*p).shape.is_null() {
            let sh = (*p).shape;
            // unlink p from sh->first_object list
            if (*sh).first_object == p {
                (*sh).first_object = (*p).next_in_shape;
            } else {
                let mut prev = (*sh).first_object;
                while !prev.is_null() {
                    if (*prev).next_in_shape == p {
                        (*prev).next_in_shape = (*p).next_in_shape;
                        break;
                    }
                    prev = (*prev).next_in_shape;
                }
            }
            // clear link for p
            (*p).next_in_shape = std::ptr::null_mut();

            // if no objects remain using this shape, free it
            if (*sh).first_object.is_null() {
                (*rt).js_free_shape(sh);
            }
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
