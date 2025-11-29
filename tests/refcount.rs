use javascript::*;
use std::ffi::CString;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_dup_free_string() {
    unsafe {
        let rt = JS_NewRuntime();
        assert!(!rt.is_null());
        let ctx = JS_NewContext(rt);
        assert!(!ctx.is_null());

        let txt = "hello".encode_utf16().collect::<Vec<u16>>();
        let s = JS_NewString(ctx, &txt);
        assert_eq!(s.get_tag(), JS_TAG_STRING);
        let p = s.get_ptr() as *mut JSString;
        assert!(!p.is_null());
        assert_eq!((*p).header.ref_count, 1);

        // dup and free
        JS_DupValue(rt, s);
        assert_eq!((*p).header.ref_count, 2);
        JS_FreeValue(rt, s);
        assert_eq!((*p).header.ref_count, 1);

        // final free (can't inspect after this)
        JS_FreeValue(rt, s);

        JS_FreeContext(ctx);
        JS_FreeRuntime(rt);
    }
}

#[test]
fn test_define_property_refcount() {
    unsafe {
        let rt = JS_NewRuntime();
        assert!(!rt.is_null());
        let ctx = JS_NewContext(rt);
        assert!(!ctx.is_null());

        // create object and string
        let obj = JS_NewObject(ctx);
        assert_eq!(obj.get_tag(), JS_TAG_OBJECT);
        let txt = "world".encode_utf16().collect::<Vec<u16>>();
        let s = JS_NewString(ctx, &txt);
        assert_eq!(s.get_tag(), JS_TAG_STRING);
        let p = s.get_ptr() as *mut JSString;
        assert_eq!((*p).header.ref_count, 1);

        // atom for property name 'a'
        let key = CString::new("a").unwrap();
        let atom = (*rt).js_new_atom_len(key.as_ptr() as *const u8, 1);
        assert!(atom != 0);

        // define property -> property slot should duplicate value
        let ret = JS_DefinePropertyValue(ctx, obj, atom, s, 0);
        assert_eq!(ret, 1);
        // now refcount should be 2 (owner + property)
        assert_eq!((*p).header.ref_count, 2);

        // free caller's ref -> still 1 (kept by property)
        JS_FreeValue(rt, s);
        assert_eq!((*p).header.ref_count, 1);

        // overwrite property with integer -> old string should be freed
        let intval = JSValue::new_int32(42);
        let ret2 = JS_DefinePropertyValue(ctx, obj, atom, intval, 0);
        assert_eq!(ret2, 1);

        // get property -> should be int 42
        let got = JS_GetProperty(ctx, obj, atom);
        assert_eq!(got.get_tag(), JS_TAG_INT);
        assert_eq!(got.u.int32, 42);

        JS_FreeContext(ctx);
        JS_FreeRuntime(rt);
    }
}
