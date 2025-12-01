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
    obj_set_value(&obj, &"sprintf".into(), Value::Function("std.sprintf".to_string()))?;
    obj_set_value(&obj, &"tmpfile".into(), Value::Function("std.tmpfile".to_string()))?;
    obj_set_value(&obj, &"loadFile".into(), Value::Function("std.loadFile".to_string()))?;
    obj_set_value(&obj, &"open".into(), Value::Function("std.open".to_string()))?;
    obj_set_value(&obj, &"popen".into(), Value::Function("std.popen".to_string()))?;
    obj_set_value(&obj, &"fdopen".into(), Value::Function("std.fdopen".to_string()))?;
    obj_set_value(&obj, &"gc".into(), Value::Function("std.gc".to_string()))?;
    obj_set_value(&obj, &"SEEK_SET".into(), Value::Number(0.0))?;
    obj_set_value(&obj, &"SEEK_END".into(), Value::Number(2.0))?;
    Ok(obj)
}
