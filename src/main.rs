use rust_quickjs::*;
use std::ffi::CString;

fn main() {
    unsafe {
        let rt = quickjs::JS_NewRuntime();
        if rt.is_null() {
            eprintln!("Failed to create runtime");
            return;
        }
        let ctx = quickjs::JS_NewContext(rt);
        if ctx.is_null() {
            eprintln!("Failed to create context");
            quickjs::JS_FreeRuntime(rt);
            return;
        }

        // Create an object
        let obj = quickjs::JS_NewObject(ctx);
        println!("Created object with tag: {}", obj.get_tag());

        // Define a property
        let key = CString::new("answer").unwrap();
        let atom = (*rt).js_new_atom_len(key.as_ptr() as *const u8, 6);
        let val = quickjs::JSValue::new_int32(42);
        let ret = quickjs::JS_DefinePropertyValue(ctx, obj, atom, val, 0);
        println!("Defined property: {}", ret);

        // Get the property
        let retrieved = quickjs::JS_GetProperty(ctx, obj, atom);
        println!(
            "Retrieved value: {} (tag: {})",
            retrieved.u.int32,
            retrieved.get_tag()
        );

        // Eval a simple script
        let script = b"3.14";
        let result = quickjs::JS_Eval(
            ctx,
            script.as_ptr() as *const i8,
            script.len(),
            std::ptr::null(),
            0,
        );
        println!(
            "Eval result: {} (tag: {})",
            result.u.float64,
            result.get_tag()
        );

        quickjs::JS_FreeContext(ctx);
        quickjs::JS_FreeRuntime(rt);
        println!("QuickJS Rust demo completed successfully!");
    }
}
