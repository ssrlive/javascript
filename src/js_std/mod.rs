pub(crate) mod sprintf;
pub(crate) mod tmpfile;

use crate::core::MutationContext;
use crate::core::{JSObjectDataPtr, Value, new_js_object_data, object_set_key_value};
use crate::error::JSError;

// local helper (currently unused but kept for future use)
#[allow(dead_code)]
fn utf8_to_utf16_local(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn initialize_std_module<'gc>(mc: &MutationContext<'gc>, global_obj: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let std_obj = make_std_object(mc)?;
    // Optionally expose it globally, or just rely on module system import
    object_set_key_value(mc, global_obj, "std", Value::Object(std_obj))?;
    Ok(())
}

pub fn make_std_object<'gc>(mc: &MutationContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let obj = new_js_object_data(mc);
    object_set_key_value(mc, &obj, "sprintf", Value::Function("std.sprintf".to_string()))?;
    object_set_key_value(mc, &obj, "tmpfile", Value::Function("std.tmpfile".to_string()))?;
    object_set_key_value(mc, &obj, "loadFile", Value::Function("std.loadFile".to_string()))?;
    object_set_key_value(mc, &obj, "open", Value::Function("std.open".to_string()))?;
    object_set_key_value(mc, &obj, "popen", Value::Function("std.popen".to_string()))?;
    object_set_key_value(mc, &obj, "fdopen", Value::Function("std.fdopen".to_string()))?;
    object_set_key_value(mc, &obj, "gc", Value::Function("std.gc".to_string()))?;
    object_set_key_value(mc, &obj, "SEEK_SET", Value::Number(0.0))?;
    object_set_key_value(mc, &obj, "SEEK_END", Value::Number(2.0))?;
    Ok(obj)
}
