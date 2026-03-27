pub(crate) mod sprintf;
pub(crate) mod tmpfile;

use crate::core::GcContext;
use crate::core::{JSObjectDataPtr, Value, new_js_object_data, object_set_key_value};
use crate::error::JSError;

// local helper (currently unused but kept for future use)
#[allow(dead_code)]
fn utf8_to_utf16_local(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn initialize_std_module<'gc>(ctx: &GcContext<'gc>, global_obj: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let std_obj = make_std_object(ctx)?;
    // Optionally expose it globally, or just rely on module system import
    object_set_key_value(ctx, global_obj, "std", &Value::Object(std_obj))?;
    Ok(())
}

pub fn make_std_object<'gc>(ctx: &GcContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let obj = new_js_object_data(ctx);
    object_set_key_value(ctx, &obj, "sprintf", &Value::Function("std.sprintf".to_string()))?;
    object_set_key_value(ctx, &obj, "tmpfile", &Value::Function("std.tmpfile".to_string()))?;
    object_set_key_value(ctx, &obj, "loadFile", &Value::Function("std.loadFile".to_string()))?;
    object_set_key_value(ctx, &obj, "open", &Value::Function("std.open".to_string()))?;
    object_set_key_value(ctx, &obj, "popen", &Value::Function("std.popen".to_string()))?;
    object_set_key_value(ctx, &obj, "fdopen", &Value::Function("std.fdopen".to_string()))?;
    object_set_key_value(ctx, &obj, "gc", &Value::Function("std.gc".to_string()))?;
    object_set_key_value(ctx, &obj, "SEEK_SET", &Value::Number(0.0))?;
    object_set_key_value(ctx, &obj, "SEEK_END", &Value::Number(2.0))?;
    Ok(obj)
}
