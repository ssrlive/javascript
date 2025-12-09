use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_tmpfile_puts_tell() {
    // use evaluate_script to inspect Value-level results
    let src = r#"
        import * as std from "std";
        let f = std.tmpfile();
        f.puts("hello");
        f.puts("\n");
        f.puts("world");
        let s = f.readAsString();
        s
    "#;
    match evaluate_script(src, None::<&std::path::Path>) {
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
    let src = r#"
        import * as std from "std";
        let f = std.tmpfile();
        f.puts("a\n");
        f.puts("b\n");
        f.seek(0, std.SEEK_SET);
        let l1 = f.getline();
        l1
    "#;
    match evaluate_script(src, None::<&std::path::Path>) {
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
    match evaluate_script(src, None::<&std::path::Path>) {
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
    match evaluate_script(src, None::<&std::path::Path>) {
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
    match evaluate_script(src, None::<&std::path::Path>) {
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
    match evaluate_script(src, None::<&std::path::Path>) {
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
    match evaluate_script(src, None::<&std::path::Path>) {
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
    match evaluate_script(src, None::<&std::path::Path>) {
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
    match evaluate_script(src, None::<&std::path::Path>) {
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
