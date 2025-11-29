use crate::core::JSObjectData;
use crate::core::{JSObjectDataPtr, Value, obj_set_value};
use crate::error::JSError;
use std::cell::RefCell;
use std::rc::Rc;

// local helper (currently unused but kept for future use)
#[allow(dead_code)]
fn utf8_to_utf16_local(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn make_std_object() -> Result<JSObjectDataPtr, JSError> {
    let obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(&obj, "sprintf", Value::Function("std.sprintf".to_string()))?;
    obj_set_value(&obj, "tmpfile", Value::Function("std.tmpfile".to_string()))?;
    obj_set_value(&obj, "loadFile", Value::Function("std.loadFile".to_string()))?;
    obj_set_value(&obj, "open", Value::Function("std.open".to_string()))?;
    obj_set_value(&obj, "popen", Value::Function("std.popen".to_string()))?;
    obj_set_value(&obj, "fdopen", Value::Function("std.fdopen".to_string()))?;
    obj_set_value(&obj, "gc", Value::Function("std.gc".to_string()))?;
    obj_set_value(&obj, "SEEK_SET", Value::Number(0.0))?;
    obj_set_value(&obj, "SEEK_END", Value::Number(2.0))?;
    Ok(obj)
}
