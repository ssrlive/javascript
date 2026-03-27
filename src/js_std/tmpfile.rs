#![cfg(feature = "std")]
use crate::core::GcContext;
use crate::core::Value;
use crate::core::new_gc_cell_ptr;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::sync::{LazyLock, Mutex};
static FILE_STORE: LazyLock<Mutex<HashMap<u64, File>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_FILE_ID: LazyLock<Mutex<u64>> = LazyLock::new(|| Mutex::new(1));
fn get_next_file_id() -> u64 {
    let mut id = NEXT_FILE_ID.lock().unwrap();
    let current = *id;
    *id += 1;
    current
}
/// Store a file and return its id (used by VM's create_vm_tmpfile).
pub fn vm_store_file(file: File) -> u64 {
    let id = get_next_file_id();
    FILE_STORE.lock().unwrap().insert(id, file);
    id
}
/// Write string args to the file identified by `file_id`.
pub fn vm_file_puts(file_id: u64, args: &[crate::core::Value<'_>]) {
    use std::io::Write;
    let mut store = FILE_STORE.lock().unwrap();
    if let Some(file) = store.get_mut(&file_id) {
        let mut buf = String::new();
        for a in args {
            match a {
                crate::core::Value::String(s) => buf.push_str(&utf16_to_utf8(s)),
                crate::core::Value::Number(n) => buf.push_str(&n.to_string()),
                crate::core::Value::Boolean(b) => buf.push_str(&b.to_string()),
                _ => {}
            }
        }
        let _ = file.write_all(buf.as_bytes());
        let _ = file.flush();
    }
}
/// Read entire contents of file as a String.
pub fn vm_file_read_as_string(file_id: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut store = FILE_STORE.lock().unwrap();
    if let Some(file) = store.get_mut(&file_id) {
        let _ = file.flush();
        let _ = file.seek(SeekFrom::Start(0));
        let mut contents = String::new();
        if file.read_to_string(&mut contents).is_ok() {
            return Some(contents);
        }
    }
    None
}
/// Seek within a file. Returns new position or -1.
pub fn vm_file_seek(file_id: u64, offset: i64, whence: i32) -> i64 {
    use std::io::{Seek, SeekFrom};
    let mut store = FILE_STORE.lock().unwrap();
    if let Some(file) = store.get_mut(&file_id) {
        let seek_from = match whence {
            0 => SeekFrom::Start(offset as u64),
            1 => SeekFrom::Current(offset),
            2 => SeekFrom::End(offset),
            _ => SeekFrom::Start(0),
        };
        match file.seek(seek_from) {
            Ok(pos) => return pos as i64,
            Err(_) => return -1,
        }
    }
    -1
}
/// Close (remove) a file from the store.
pub fn vm_file_close(file_id: u64) {
    FILE_STORE.lock().unwrap().remove(&file_id);
}
/// Create a VM tmpfile object with host-fn method slots.
pub fn vm_create_tmpfile<'gc>(ctx: &GcContext<'gc>) -> Value<'gc> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let pid = std::process::id();
    let temp_dir = std::env::temp_dir();
    let filename = temp_dir.join(format!("rust_js_vm_tmp_{}_{}.tmp", timestamp, pid));
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&filename)
    {
        Ok(file) => {
            let file_id = vm_store_file(file);
            let mut obj = IndexMap::new();
            obj.insert("__file_id__".to_string(), Value::Number(file_id as f64));
            fn make_host_fn<'a>(ctx: &GcContext<'a>, name: &str) -> Value<'a> {
                let mut map = IndexMap::new();
                map.insert("__host_fn__".to_string(), Value::from(name));
                Value::VmObject(new_gc_cell_ptr(ctx, map))
            }
            obj.insert("puts".to_string(), make_host_fn(ctx, "tmp.puts"));
            obj.insert("readAsString".to_string(), make_host_fn(ctx, "tmp.readAsString"));
            obj.insert("getline".to_string(), make_host_fn(ctx, "tmp.getline"));
            obj.insert("seek".to_string(), make_host_fn(ctx, "tmp.seek"));
            obj.insert("close".to_string(), make_host_fn(ctx, "tmp.close"));
            Value::VmObject(new_gc_cell_ptr(ctx, obj))
        }
        Err(_) => Value::Undefined,
    }
}
/// Dispatch a VM file method call by host-fn name.
pub fn vm_dispatch_file_method<'gc>(name: &str, receiver: Option<&Value<'gc>>, args: &[Value<'gc>]) -> Value<'gc> {
    let file_id = match &receiver {
        Some(Value::VmObject(obj)) => match obj.borrow().get("__file_id__") {
            Some(Value::Number(n)) => *n as u64,
            _ => return Value::Undefined,
        },
        _ => return Value::Undefined,
    };
    match name {
        "tmp.puts" => {
            vm_file_puts(file_id, args);
            Value::Undefined
        }
        "tmp.readAsString" => match vm_file_read_as_string(file_id) {
            Some(s) => Value::String(utf8_to_utf16(&s)),
            None => Value::String(utf8_to_utf16("")),
        },
        "tmp.seek" => {
            let offset = match args.first() {
                Some(Value::Number(n)) => *n as i64,
                _ => 0,
            };
            let whence = match args.get(1) {
                Some(Value::Number(n)) => *n as i32,
                _ => 0,
            };
            Value::Number(vm_file_seek(file_id, offset, whence) as f64)
        }
        "tmp.getline" => {
            use std::io::Read;
            let mut store = FILE_STORE.lock().unwrap();
            if let Some(file) = store.get_mut(&file_id) {
                let mut line = Vec::new();
                let mut buf = [0u8; 1];
                let mut read_any = false;
                loop {
                    match file.read(&mut buf) {
                        Ok(0) => break,
                        Ok(_) => {
                            read_any = true;
                            if buf[0] == b'\n' {
                                break;
                            }
                            if buf[0] != b'\r' {
                                line.push(buf[0]);
                            }
                        }
                        Err(_) => break,
                    }
                }
                if !read_any {
                    Value::Undefined
                } else {
                    Value::String(utf8_to_utf16(&String::from_utf8_lossy(&line)))
                }
            } else {
                Value::Undefined
            }
        }
        "tmp.close" => {
            vm_file_close(file_id);
            Value::Undefined
        }
        _ => Value::Undefined,
    }
}
