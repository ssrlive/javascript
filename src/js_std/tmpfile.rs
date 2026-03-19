#![cfg(feature = "std")]

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::sync::{LazyLock, Mutex};

use crate::core::MutationContext;
use crate::core::{InternalSlot, JSObjectDataPtr, Value, new_js_object_data, object_set_key_value, slot_get, slot_set};
use crate::error::JSError;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};

static FILE_STORE: LazyLock<Mutex<HashMap<u64, File>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_FILE_ID: LazyLock<Mutex<u64>> = LazyLock::new(|| Mutex::new(1));

fn get_next_file_id() -> u64 {
    let mut id = NEXT_FILE_ID.lock().unwrap();
    let current = *id;
    *id += 1;
    current
}

/// Create a temporary file object
pub(crate) fn create_tmpfile<'gc>(mc: &MutationContext<'gc>) -> Result<Value<'gc>, JSError> {
    // Create a real temporary file with a more random suffix
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let process_id = std::process::id();
    let thread_id = format!("{:?}", std::thread::current().id());
    let thread_hash = thread_id.chars().map(|c| c as u64).sum::<u64>();
    let temp_dir = std::env::temp_dir();
    let filename = temp_dir.join(format!("rust_js_tmp_{}_{}_{}.tmp", timestamp, process_id, thread_hash));
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&filename)
    {
        Ok(file) => {
            let file_id = get_next_file_id();
            FILE_STORE.lock().unwrap().insert(file_id, file);

            let tmp = new_js_object_data(mc);
            slot_set(mc, &tmp, InternalSlot::FileId, &Value::Number(file_id as f64));
            slot_set(mc, &tmp, InternalSlot::Eof, &Value::Boolean(false));
            // methods
            object_set_key_value(mc, &tmp, "puts", &Value::Function("tmp.puts".to_string()))?;
            object_set_key_value(mc, &tmp, "readAsString", &Value::Function("tmp.readAsString".to_string()))?;
            object_set_key_value(mc, &tmp, "seek", &Value::Function("tmp.seek".to_string()))?;
            object_set_key_value(mc, &tmp, "tell", &Value::Function("tmp.tell".to_string()))?;
            object_set_key_value(mc, &tmp, "putByte", &Value::Function("tmp.putByte".to_string()))?;
            object_set_key_value(mc, &tmp, "getByte", &Value::Function("tmp.getByte".to_string()))?;
            object_set_key_value(mc, &tmp, "getline", &Value::Function("tmp.getline".to_string()))?;
            object_set_key_value(mc, &tmp, "eof", &Value::Function("tmp.eof".to_string()))?;
            object_set_key_value(mc, &tmp, "close", &Value::Function("tmp.close".to_string()))?;
            Ok(Value::Object(tmp))
        }
        Err(e) => Err(raise_eval_error!(format!("Failed to create temporary file: {e}"))),
    }
}

/// Handle file object method calls
pub(crate) fn handle_file_method<'gc>(object: &JSObjectDataPtr<'gc>, method: &str, args: &[Value<'gc>]) -> Result<Value<'gc>, JSError> {
    // If this object is a file-like object (we use '__file_id' as marker)
    if let Some(file_id_val) = slot_get(object, &InternalSlot::FileId) {
        let file_id = match *file_id_val.borrow() {
            Value::Number(n) => n as u64,
            _ => return Err(raise_eval_error!("Invalid file object")),
        };

        let mut file_store = FILE_STORE.lock().unwrap();
        let file = match file_store.get_mut(&file_id) {
            Some(f) => f,
            None => {
                return Err(raise_eval_error!("File not found"));
            }
        };

        match method {
            "puts" => {
                // write string arguments to file
                if args.is_empty() {
                    return Ok(Value::Undefined);
                }
                // build string to write
                let mut to_write = String::new();
                for av in args {
                    match av {
                        Value::String(sv) => to_write.push_str(&utf16_to_utf8(sv)),
                        Value::Number(n) => to_write.push_str(&n.to_string()),
                        Value::Boolean(b) => to_write.push_str(&b.to_string()),
                        _ => {}
                    }
                }
                // write to file
                if file.write_all(to_write.as_bytes()).is_err() {
                    return Ok(Value::Number(-1.0));
                }
                if file.flush().is_err() {
                    return Ok(Value::Number(-1.0));
                }
                return Ok(Value::Undefined);
            }
            "readAsString" => {
                // flush any pending writes and seek to beginning and read entire file
                if file.flush().is_err() {
                    return Ok(Value::String(utf8_to_utf16("")));
                }
                if file.seek(SeekFrom::Start(0)).is_err() {
                    return Ok(Value::String(utf8_to_utf16("")));
                }
                let mut contents = String::new();
                if file.read_to_string(&mut contents).is_err() {
                    return Ok(Value::String(utf8_to_utf16("")));
                }
                return Ok(Value::String(utf8_to_utf16(&contents)));
            }
            "seek" => {
                // seek(offset, whence)
                if args.len() >= 2 {
                    let offv = &args[0];
                    let whv = &args[1];
                    let offset = match offv {
                        Value::Number(n) => *n as i64,
                        _ => 0,
                    };
                    let whence = match whv {
                        Value::Number(n) => *n as i32,
                        _ => 0,
                    };
                    let seek_from = match whence {
                        0 => SeekFrom::Start(offset as u64), // SEEK_SET
                        1 => SeekFrom::Current(offset),      // SEEK_CUR
                        2 => SeekFrom::End(offset),          // SEEK_END
                        _ => SeekFrom::Start(0),
                    };
                    match file.seek(seek_from) {
                        Ok(pos) => return Ok(Value::Number(pos as f64)),
                        Err(_) => return Ok(Value::Number(-1.0)),
                    }
                }
                return Ok(Value::Number(-1.0));
            }
            "tell" => match file.stream_position() {
                Ok(pos) => return Ok(Value::Number(pos as f64)),
                Err(_) => return Ok(Value::Number(-1.0)),
            },
            "putByte" => {
                if !args.is_empty() {
                    let bv = &args[0];
                    let byte = match bv {
                        Value::Number(n) => *n as u8,
                        _ => 0,
                    };
                    // write byte to file
                    if file.write_all(&[byte]).is_err() {
                        return Ok(Value::Number(-1.0));
                    }
                    if file.flush().is_err() {
                        return Ok(Value::Number(-1.0));
                    }
                    return Ok(Value::Undefined);
                }
                return Ok(Value::Undefined);
            }
            "getByte" => {
                // read one byte from current position
                let mut buf = [0u8; 1];
                match file.read(&mut buf) {
                    Ok(1) => return Ok(Value::Number(buf[0] as f64)),
                    _ => return Ok(Value::Number(-1.0)),
                }
            }
            "getline" => {
                // read line from current position
                let mut reader = BufReader::new(&mut *file);
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => return Ok(Value::Undefined), // EOF
                    Ok(_) => {
                        // remove trailing newline if present
                        if line.ends_with('\n') {
                            line.pop();
                            if line.ends_with('\r') {
                                line.pop();
                            }
                        }
                        return Ok(Value::String(utf8_to_utf16(&line)));
                    }
                    Err(_) => return Ok(Value::Undefined),
                }
            }
            "eof" => {
                // check if we're at EOF
                let mut buf = [0u8; 1];
                match file.read(&mut buf) {
                    Ok(0) => return Ok(Value::Boolean(true)), // EOF
                    Ok(_) => {
                        // unread the byte by seeking back
                        file.seek(SeekFrom::Current(-1))?;
                        return Ok(Value::Boolean(false));
                    }
                    Err(_) => return Ok(Value::Boolean(true)),
                }
            }
            "close" => {
                // remove file from store (file will be closed when dropped)
                drop(file_store.remove(&file_id));
                return Ok(Value::Undefined);
            }
            _ => {}
        }
    }

    Err(raise_eval_error!(format!("File method {method} not implemented")))
}

// ── VM-facing helpers (no GC lifetime needed) ───────────────────────────

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
pub fn vm_create_tmpfile<'gc>() -> Value<'gc> {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use indexmap::IndexMap;

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
            // Each method is a small VmObject with __host_fn__ key
            fn make_host_fn<'a>(name: &str) -> Value<'a> {
                let mut map = IndexMap::new();
                map.insert("__host_fn__".to_string(), Value::String(crate::unicode::utf8_to_utf16(name)));
                Value::VmObject(Rc::new(RefCell::new(map)))
            }
            obj.insert("puts".to_string(), make_host_fn("tmp.puts"));
            obj.insert("readAsString".to_string(), make_host_fn("tmp.readAsString"));
            obj.insert("seek".to_string(), make_host_fn("tmp.seek"));
            obj.insert("close".to_string(), make_host_fn("tmp.close"));
            Value::VmObject(Rc::new(RefCell::new(obj)))
        }
        Err(_) => Value::Undefined,
    }
}

/// Dispatch a VM file method call by host-fn name.
pub fn vm_dispatch_file_method<'gc>(name: &str, receiver: Option<Value<'gc>>, args: Vec<Value<'gc>>) -> Value<'gc> {
    let file_id = match &receiver {
        Some(Value::VmObject(obj)) => match obj.borrow().get("__file_id__") {
            Some(Value::Number(n)) => *n as u64,
            _ => return Value::Undefined,
        },
        _ => return Value::Undefined,
    };
    match name {
        "tmp.puts" => {
            vm_file_puts(file_id, &args);
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
        "tmp.close" => {
            vm_file_close(file_id);
            Value::Undefined
        }
        _ => Value::Undefined,
    }
}
