#![allow(unused)]

use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
#[cfg(feature = "std")]
fn test_tmpfile_puts_tell() {
    // use evaluate_module to inspect Value-level results
    let src = r#"
        import * as std from "std";
        let f = std.tmpfile();
        f.puts("hello");
        f.puts("\n");
        f.puts("world");
        let s = f.readAsString();
        s
    "#;
    let result = evaluate_module(src, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"hello\\nworld\"");
}

#[test]
#[cfg(feature = "std")]
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
    let result = evaluate_module(src, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"a\"");
}

#[test]
#[cfg(feature = "std")]
fn test_sprintf_basic() {
    let src = "import * as std from \"std\";\nstd.sprintf(\"a=%d s=%s\", 123, \"abc\")";
    let result = evaluate_module(src, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"a=123 s=abc\"");
}

#[test]
#[cfg(feature = "std")]
fn test_sprintf_zero_pad() {
    let src = "import * as std from \"std\";\nstd.sprintf(\"%010d\", 123)";
    let result = evaluate_module(src, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"0000000123\"");
}

#[test]
#[cfg(feature = "std")]
fn test_sprintf_hex() {
    let src = "import * as std from \"std\";\nstd.sprintf(\"%x\", -2)";
    let result = evaluate_module(src, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"fffffffe\"");
}

#[test]
#[cfg(feature = "std")]
fn test_sprintf_float() {
    let src = "import * as std from \"std\";\nstd.sprintf(\"%10.1f\", 2.1)";
    let result = evaluate_module(src, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"       2.1\"");
}

#[test]
#[cfg(feature = "std")]
fn test_sprintf_dynamic_width() {
    let src = "import * as std from \"std\";\nstd.sprintf(\"%*.*f\", 10, 2, -2.13)";
    let result = evaluate_module(src, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"     -2.13\"");
}

#[test]
#[cfg(feature = "std")]
fn test_sprintf_long_hex() {
    let src = "import * as std from \"std\";\nstd.sprintf(\"%lx\", -2)";
    let result = evaluate_module(src, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"fffffffffffffffe\"");
}

#[test]
#[cfg(feature = "std")]
fn test_sprintf_hex_with_prefix() {
    let src = "import * as std from \"std\";\nstd.sprintf(\"%#lx\", 123)";
    let result = evaluate_module(src, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"0x7b\"");
}
