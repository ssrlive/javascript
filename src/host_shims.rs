use crate::quickjs::{obj_set_val, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

fn utf8_to_utf16_local(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn make_std_object() -> HashMap<String, Rc<RefCell<Value>>> {
    let mut obj = HashMap::new();
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

pub fn make_os_object() -> HashMap<String, Rc<RefCell<Value>>> {
    let mut obj = HashMap::new();
    obj_set_val(&mut obj, "remove", Value::Function("os.remove".to_string()));
    obj_set_val(&mut obj, "mkdir", Value::Function("os.mkdir".to_string()));
    obj_set_val(&mut obj, "open", Value::Function("os.open".to_string()));
    obj_set_val(&mut obj, "write", Value::Function("os.write".to_string()));
    obj_set_val(&mut obj, "read", Value::Function("os.read".to_string()));
    obj_set_val(&mut obj, "seek", Value::Function("os.seek".to_string()));
    obj_set_val(&mut obj, "close", Value::Function("os.close".to_string()));
    obj_set_val(&mut obj, "readdir", Value::Function("os.readdir".to_string()));
    obj_set_val(&mut obj, "utimes", Value::Function("os.utimes".to_string()));
    obj_set_val(&mut obj, "stat", Value::Function("os.stat".to_string()));
    obj_set_val(&mut obj, "lstat", Value::Function("os.lstat".to_string()));
    obj_set_val(&mut obj, "symlink", Value::Function("os.symlink".to_string()));
    obj_set_val(&mut obj, "readlink", Value::Function("os.readlink".to_string()));
    obj_set_val(&mut obj, "getcwd", Value::Function("os.getcwd".to_string()));
    obj_set_val(&mut obj, "realpath", Value::Function("os.realpath".to_string()));
    obj_set_val(&mut obj, "exec", Value::Function("os.exec".to_string()));
    obj_set_val(&mut obj, "pipe", Value::Function("os.pipe".to_string()));
    obj_set_val(&mut obj, "waitpid", Value::Function("os.waitpid".to_string()));
    obj_set_val(&mut obj, "kill", Value::Function("os.kill".to_string()));
    obj_set_val(&mut obj, "isatty", Value::Function("os.isatty".to_string()));
    obj_set_val(&mut obj, "O_RDWR", Value::Number(2.0));
    obj_set_val(&mut obj, "O_CREAT", Value::Number(64.0));
    obj_set_val(&mut obj, "O_TRUNC", Value::Number(512.0));
    obj_set_val(&mut obj, "O_RDONLY", Value::Number(0.0));
    obj_set_val(&mut obj, "S_IFMT", Value::Number(0o170000 as f64));
    obj_set_val(&mut obj, "S_IFREG", Value::Number(0o100000 as f64));
    obj_set_val(&mut obj, "S_IFLNK", Value::Number(0o120000 as f64));
    obj_set_val(&mut obj, "SIGTERM", Value::Number(15.0));
    obj
}
