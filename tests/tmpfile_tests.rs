use rust_quickjs::quickjs::*;
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
        let result = JS_Eval(ctx, script.as_ptr() as *const i8, script.len(), std::ptr::null(), 0);
        assert_eq!(result.get_tag(), JS_TAG_FLOAT64);
        assert_eq!(result.u.float64, 42.5);

        JS_FreeContext(ctx);
        JS_FreeRuntime(rt);
    }
}

#[test]
fn test_tmpfile_puts_tell() {
    // use evaluate_script to inspect Value-level results
    let src = "import * as std from \"std\";\nlet f = std.tmpfile();\nf.puts(\"hello\");\nf.puts(\"\\n\");\nf.puts(\"world\");\nlet s = f.readAsString();\ns";
    match rust_quickjs::quickjs::evaluate_script(src) {
        Ok(val) => {
            if let Value::String(vec) = val {
                let s = String::from_utf16_lossy(&vec);
                assert_eq!(s, "hello\nworld");
            } else {
                panic!("expected string from evaluate_script, got {:?}", val);
            }
        }
        Err(e) => panic!("evaluate_script error: {:?}", e),
    }
}

#[test]
fn test_tmpfile_getline() {
    let src = "import * as std from \"std\";\nlet f = std.tmpfile();\nf.puts(\"a\\n\");\nf.puts(\"b\\n\");\nf.seek(0, std.SEEK_SET);\nlet l1 = f.getline();\nl1";
    match rust_quickjs::quickjs::evaluate_script(src) {
        Ok(val) => {
            if let Value::String(vec) = val {
                let s = String::from_utf16_lossy(&vec);
                assert_eq!(s, "a");
            } else {
                panic!("expected string from evaluate_script, got {:?}", val);
            }
        }
        Err(e) => panic!("evaluate_script error: {:?}", e),
    }
}

#[test]
fn test_sprintf_basic() {
    let src = "import * as std from \"std\";\nstd.sprintf(\"a=%d s=%s\", 123, \"abc\")";
    match rust_quickjs::quickjs::evaluate_script(src) {
        Ok(val) => {
            if let Value::String(vec) = val {
                let s = String::from_utf16_lossy(&vec);
                assert_eq!(s, "a=123 s=abc");
            } else {
                panic!("expected string from evaluate_script, got {:?}", val);
            }
        }
        Err(e) => panic!("evaluate_script error: {:?}", e),
    }
}

#[test]
fn test_sprintf_zero_pad() {
    let src = "import * as std from \"std\";\nstd.sprintf(\"%010d\", 123)";
    match rust_quickjs::quickjs::evaluate_script(src) {
        Ok(val) => {
            if let Value::String(vec) = val {
                let s = String::from_utf16_lossy(&vec);
                assert_eq!(s, "0000000123");
            } else {
                panic!("expected string from evaluate_script, got {:?}", val);
            }
        }
        Err(e) => panic!("evaluate_script error: {:?}", e),
    }
}

#[test]
fn test_sprintf_hex() {
    let src = "import * as std from \"std\";\nstd.sprintf(\"%x\", -2)";
    match rust_quickjs::quickjs::evaluate_script(src) {
        Ok(val) => {
            if let Value::String(vec) = val {
                let s = String::from_utf16_lossy(&vec);
                assert_eq!(s, "fffffffe");
            } else {
                panic!("expected string from evaluate_script, got {:?}", val);
            }
        }
        Err(e) => panic!("evaluate_script error: {:?}", e),
    }
}

#[test]
fn test_sprintf_float() {
    let src = "import * as std from \"std\";\nstd.sprintf(\"%10.1f\", 2.1)";
    match rust_quickjs::quickjs::evaluate_script(src) {
        Ok(val) => {
            if let Value::String(vec) = val {
                let s = String::from_utf16_lossy(&vec);
                assert_eq!(s, "       2.1");
            } else {
                panic!("expected string from evaluate_script, got {:?}", val);
            }
        }
        Err(e) => panic!("evaluate_script error: {:?}", e),
    }
}

#[test]
fn test_sprintf_dynamic_width() {
    let src = "import * as std from \"std\";\nstd.sprintf(\"%*.*f\", 10, 2, -2.13)";
    match rust_quickjs::quickjs::evaluate_script(src) {
        Ok(val) => {
            if let Value::String(vec) = val {
                let s = String::from_utf16_lossy(&vec);
                assert_eq!(s, "     -2.13");
            } else {
                panic!("expected string from evaluate_script, got {:?}", val);
            }
        }
        Err(e) => panic!("evaluate_script error: {:?}", e),
    }
}

#[test]
fn test_sprintf_long_hex() {
    let src = "import * as std from \"std\";\nstd.sprintf(\"%lx\", -2)";
    match rust_quickjs::quickjs::evaluate_script(src) {
        Ok(val) => {
            if let Value::String(vec) = val {
                let s = String::from_utf16_lossy(&vec);
                assert_eq!(s, "fffffffffffffffe");
            } else {
                panic!("expected string from evaluate_script, got {:?}", val);
            }
        }
        Err(e) => panic!("evaluate_script error: {:?}", e),
    }
}

#[test]
fn test_sprintf_hex_with_prefix() {
    let src = "import * as std from \"std\";\nstd.sprintf(\"%#lx\", 123)";
    match rust_quickjs::quickjs::evaluate_script(src) {
        Ok(val) => {
            if let Value::String(vec) = val {
                let s = String::from_utf16_lossy(&vec);
                assert_eq!(s, "0x7b");
            } else {
                panic!("expected string from evaluate_script, got {:?}", val);
            }
        }
        Err(e) => panic!("evaluate_script error: {:?}", e),
    }
}
