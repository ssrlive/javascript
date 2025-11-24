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
    // Opaque for now
    _unused: [u8; 0],
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

impl JSRuntime {
    pub unsafe fn js_malloc_rt(&mut self, size: usize) -> *mut c_void {
        if let Some(malloc_func) = self.mf.js_malloc {
            malloc_func(&mut self.malloc_state, size)
        } else {
            std::ptr::null_mut()
        }
    }

    pub unsafe fn js_free_rt(&mut self, ptr: *mut c_void) {
        if let Some(free_func) = self.mf.js_free {
            free_func(&mut self.malloc_state, ptr)
        }
    }

    pub unsafe fn js_realloc_rt(&mut self, ptr: *mut c_void, size: usize) -> *mut c_void {
        if let Some(realloc_func) = self.mf.js_realloc {
            realloc_func(&mut self.malloc_state, ptr, size)
        } else {
            std::ptr::null_mut()
        }
    }

    pub unsafe fn js_alloc_string(&mut self, max_len: usize, is_wide_char: bool) -> *mut JSString {
        let size =
            std::mem::size_of::<JSString>() + (max_len << (if is_wide_char { 1 } else { 0 })) + 1
                - (if is_wide_char { 1 } else { 0 });
        let ptr = self.js_malloc_rt(size) as *mut JSString;
        if !ptr.is_null() {
            (*ptr).header.ref_count = 1;
            (*ptr).len = (max_len as u32) | (if is_wide_char { 1 << 31 } else { 0 });
            (*ptr).hash = 0; // atom_type = 0
            (*ptr).hash_next = 0;
        }
        ptr
    }

    pub unsafe fn js_free_atom_struct(&mut self, p: *mut JSAtomStruct) {
        // TODO: handle different atom types
        self.js_free_rt(p as *mut c_void);
    }

    fn hash_string(str: &[u8]) -> u32 {
        let mut h: u32 = 0;
        for &c in str {
            h = h.wrapping_mul(263).wrapping_add(c as u32);
        }
        h
    }

    pub unsafe fn js_new_atom_len(&mut self, str: *const u8, len: usize) -> JSAtom {
        if self.atom_hash_size == 0 {
            if self.init_atoms() < 0 {
                return 0;
            }
        }

        let str_slice = std::slice::from_raw_parts(str, len);
        let h = Self::hash_string(str_slice);
        let h_masked = h & ((self.atom_hash_size as u32) - 1);

        let mut i = *self.atom_hash.offset(h_masked as isize);
        while i != 0 {
            let p = *self.atom_array.offset(i as isize);
            // Check if match
            // Assuming 8-bit string for now
            let p_len = ((*p).len & 0x7FFFFFFF) as usize;
            if p_len == len {
                let p_str = (p as *mut u8).offset(std::mem::size_of::<JSString>() as isize);
                let p_slice = std::slice::from_raw_parts(p_str, len);
                if p_slice == str_slice {
                    return i;
                }
            }
            i = (*p).hash_next;
        }

        if self.atom_free_index == 0 {
            if self.atom_count >= self.atom_size {
                let new_size = if self.atom_size == 0 {
                    128
                } else {
                    self.atom_size * 2
                };
                let new_array = self.js_realloc_rt(
                    self.atom_array as *mut c_void,
                    new_size as usize * std::mem::size_of::<*mut JSAtomStruct>(),
                ) as *mut *mut JSAtomStruct;
                if new_array.is_null() {
                    return 0;
                }
                self.atom_array = new_array;
                self.atom_size = new_size;
            }
            i = self.atom_count as u32;
            self.atom_count += 1;
        } else {
            i = self.atom_free_index as u32;
            self.atom_free_index = *self.atom_array.offset(i as isize) as i32;
        }

        let p = self.js_alloc_string(len, false);
        if p.is_null() {
            return 0;
        }

        let p_str = (p as *mut u8).offset(std::mem::size_of::<JSString>() as isize);
        std::ptr::copy_nonoverlapping(str, p_str, len);
        *p_str.offset(len as isize) = 0; // Null terminate

        (*p).hash = h;
        (*p).hash_next = *self.atom_hash.offset(h_masked as isize);
        *self.atom_hash.offset(h_masked as isize) = i;

        *self.atom_array.offset(i as isize) = p;

        i
    }

    pub unsafe fn init_atoms(&mut self) -> i32 {
        self.atom_hash_size = 256;
        self.atom_count = 1;
        self.atom_size = 256;

        self.atom_hash = self
            .js_malloc_rt(self.atom_hash_size as usize * std::mem::size_of::<u32>())
            as *mut u32;
        if self.atom_hash.is_null() {
            return -1;
        }
        std::ptr::write_bytes(self.atom_hash, 0, self.atom_hash_size as usize);

        self.atom_array = self
            .js_malloc_rt(self.atom_size as usize * std::mem::size_of::<*mut JSAtomStruct>())
            as *mut *mut JSAtomStruct;
        if self.atom_array.is_null() {
            self.js_free_rt(self.atom_hash as *mut c_void);
            self.atom_hash = std::ptr::null_mut();
            return -1;
        }

        0
    }
}

impl JSContext {
    pub unsafe fn js_malloc(&mut self, size: usize) -> *mut c_void {
        (*self.rt).js_malloc_rt(size)
    }

    pub unsafe fn js_free(&mut self, ptr: *mut c_void) {
        (*self.rt).js_free_rt(ptr)
    }

    pub unsafe fn js_realloc(&mut self, ptr: *mut c_void, size: usize) -> *mut c_void {
        (*self.rt).js_realloc_rt(ptr, size)
    }

    pub unsafe fn js_free_value(&mut self, v: JSValue) {
        if v.has_ref_count() {
            let p = v.get_ptr() as *mut JSRefCountHeader;
            (*p).ref_count -= 1;
            if (*p).ref_count <= 0 {
                self.js_free_value_rt(v);
            }
        }
    }

    pub unsafe fn js_free_value_rt(&mut self, v: JSValue) {
        (*self.rt).js_free_value_rt(v);
    }

    pub unsafe fn js_call(
        &mut self,
        func_obj: JSValue,
        this_obj: JSValue,
        args: &[JSValue],
    ) -> JSValue {
        self.js_call_internal(func_obj, this_obj, this_obj, args, 0)
    }

    pub unsafe fn js_call_internal(
        &mut self,
        _func_obj: JSValue,
        _this_obj: JSValue,
        _new_target: JSValue,
        _args: &[JSValue],
        _flags: i32,
    ) -> JSValue {
        // TODO: Implement bytecode interpreter loop
        JS_UNDEFINED
    }
}

impl JSRuntime {
    pub unsafe fn js_free_value_rt(&mut self, v: JSValue) {
        let tag = v.get_tag();
        match tag {
            JS_TAG_STRING => {
                let p = v.get_ptr() as *mut JSString;
                // Check atom_type (hash field)
                // hash: 30, atom_type: 2
                let atom_type = ((*p).hash >> 30) & 3;
                if atom_type != 0 {
                    self.js_free_atom_struct(p);
                } else {
                    self.js_free_rt(p as *mut c_void);
                }
            }
            JS_TAG_OBJECT | JS_TAG_FUNCTION_BYTECODE => {
                let p = v.get_ptr() as *mut JSGCObjectHeader;
                // GC logic
                // For now just free it directly to avoid complex GC logic in this step
                // But real implementation puts it in gc_zero_ref_count_list

                // Simplified:
                self.js_free_rt(p as *mut c_void);
            }
            _ => {}
        }
    }
}

pub unsafe fn JS_NewRuntime() -> *mut JSRuntime {
    let rt_ptr = libc::malloc(std::mem::size_of::<JSRuntime>()) as *mut JSRuntime;
    if rt_ptr.is_null() {
        return std::ptr::null_mut();
    }
    let rt = &mut *rt_ptr;
    std::ptr::write_bytes(rt_ptr, 0, 1);

    rt.mf.js_malloc = Some(js_def_malloc);
    rt.mf.js_free = Some(js_def_free);
    rt.mf.js_realloc = Some(js_def_realloc);
    rt.mf.js_malloc_usable_size = Some(js_def_malloc_usable_size);
    rt.malloc_state.malloc_limit = usize::MAX;

    rt.context_list.init();
    rt.gc_obj_list.init();
    rt.gc_zero_ref_count_list.init();
    rt.tmp_obj_list.init();
    rt.weakref_list.init();

    if rt.init_atoms() < 0 {
        JS_FreeRuntime(rt_ptr);
        return std::ptr::null_mut();
    }

    rt_ptr
}

pub unsafe fn JS_FreeRuntime(rt: *mut JSRuntime) {
    let rt_ref = &mut *rt;
    if !rt_ref.atom_hash.is_null() {
        rt_ref.js_free_rt(rt_ref.atom_hash as *mut c_void);
    }
    if !rt_ref.atom_array.is_null() {
        rt_ref.js_free_rt(rt_ref.atom_array as *mut c_void);
    }
    libc::free(rt as *mut c_void);
}

pub unsafe extern "C" fn js_def_malloc(s: *mut JSMallocState, size: usize) -> *mut c_void {
    let s = &mut *s;
    if s.malloc_size + size > s.malloc_limit {
        return std::ptr::null_mut();
    }
    let ptr = libc::malloc(size);
    if ptr.is_null() {
        return std::ptr::null_mut();
    }
    s.malloc_count += 1;
    s.malloc_size += js_def_malloc_usable_size(ptr);
    ptr
}

pub unsafe extern "C" fn js_def_free(s: *mut JSMallocState, ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    let s = &mut *s;
    s.malloc_count -= 1;
    s.malloc_size -= js_def_malloc_usable_size(ptr);
    libc::free(ptr);
}

pub unsafe extern "C" fn js_def_realloc(
    s: *mut JSMallocState,
    ptr: *mut c_void,
    size: usize,
) -> *mut c_void {
    let s = &mut *s;
    if ptr.is_null() {
        return js_def_malloc(s, size);
    }
    if size == 0 {
        js_def_free(s, ptr);
        return std::ptr::null_mut();
    }

    let old_size = js_def_malloc_usable_size(ptr);
    if s.malloc_size + size - old_size > s.malloc_limit {
        return std::ptr::null_mut();
    }

    let new_ptr = libc::realloc(ptr, size);
    if new_ptr.is_null() {
        return std::ptr::null_mut();
    }

    s.malloc_size -= old_size;
    s.malloc_size += js_def_malloc_usable_size(new_ptr);
    new_ptr
}

pub unsafe extern "C" fn js_def_malloc_usable_size(ptr: *const c_void) -> usize {
    // Windows/MSVC doesn't have malloc_usable_size easily accessible via libc crate usually,
    // or it might be _msize.
    // For now, let's assume we can't track exact size perfectly without platform specific calls.
    // But wait, quickjs.c uses malloc_usable_size on Linux and _msize on Windows.

    #[cfg(target_os = "linux")]
    return libc::malloc_usable_size(ptr as *mut c_void);

    #[cfg(target_os = "windows")]
    {
        // libc crate might expose _msize for windows-msvc
        // Let's check if we can use it.
        // If not, we might need to declare it.
        extern "C" {
            fn _msize(memblock: *mut c_void) -> usize;
        }
        return _msize(ptr as *mut c_void);
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    return 0; // TODO: support other platforms
}

pub unsafe fn JS_NewContext(rt: *mut JSRuntime) -> *mut JSContext {
    let ctx_ptr = (*rt).js_malloc_rt(std::mem::size_of::<JSContext>()) as *mut JSContext;
    if ctx_ptr.is_null() {
        return std::ptr::null_mut();
    }
    let ctx = &mut *ctx_ptr;
    std::ptr::write_bytes(ctx_ptr, 0, 1);

    ctx.header.ref_count = 1;
    ctx.rt = rt;
    ctx.link.init();

    (*rt).context_list.add_tail(&mut ctx.link);

    // TODO: Initialize built-in objects (Global, Object, Array, etc.)
    // This requires JS_NewObject and other helpers which are not implemented yet.

    ctx_ptr
}

pub unsafe fn JS_FreeContext(ctx: *mut JSContext) {
    let rt = (*ctx).rt;
    (*ctx).link.del();

    // TODO: Free built-in objects

    (*rt).js_free_rt(ctx as *mut c_void);
}
