use crate::quickjs::JSObjectData;
use crate::quickjs::{obj_set_val, Value};

// local helper (currently unused but kept for future use)
#[allow(dead_code)]
fn utf8_to_utf16_local(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn make_std_object() -> JSObjectData {
    let mut obj = JSObjectData::new();
    obj_set_val(&mut obj, "sprintf", Value::Function("std.sprintf".to_string()));
    obj_set_val(&mut obj, "tmpfile", Value::Function("std.tmpfile".to_string()));
    obj_set_val(&mut obj, "loadFile", Value::Function("std.loadFile".to_string()));
    obj_set_val(&mut obj, "open", Value::Function("std.open".to_string()));
    obj_set_val(&mut obj, "popen", Value::Function("std.popen".to_string()));
    obj_set_val(&mut obj, "fdopen", Value::Function("std.fdopen".to_string()));
    obj_set_val(&mut obj, "gc", Value::Function("std.gc".to_string()));
    obj_set_val(&mut obj, "SEEK_SET", Value::Number(0.0));
    obj_set_val(&mut obj, "SEEK_END", Value::Number(2.0));
    obj
}
