use crate::quickjs::JSObjectData;
use crate::quickjs::{obj_set_val, JSObjectDataPtr, Value};
use std::cell::RefCell;
use std::rc::Rc;

// local helper (currently unused but kept for future use)
#[allow(dead_code)]
fn utf8_to_utf16_local(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn make_std_object() -> JSObjectDataPtr {
    let obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_val(&obj, "sprintf", Value::Function("std.sprintf".to_string()));
    obj_set_val(&obj, "tmpfile", Value::Function("std.tmpfile".to_string()));
    obj_set_val(&obj, "loadFile", Value::Function("std.loadFile".to_string()));
    obj_set_val(&obj, "open", Value::Function("std.open".to_string()));
    obj_set_val(&obj, "popen", Value::Function("std.popen".to_string()));
    obj_set_val(&obj, "fdopen", Value::Function("std.fdopen".to_string()));
    obj_set_val(&obj, "gc", Value::Function("std.gc".to_string()));
    obj_set_val(&obj, "SEEK_SET", Value::Number(0.0));
    obj_set_val(&obj, "SEEK_END", Value::Number(2.0));
    obj
}
