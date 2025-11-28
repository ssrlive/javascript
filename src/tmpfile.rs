use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::rc::Rc;
use std::sync::{LazyLock, Mutex};

use crate::error::JSError;
use crate::quickjs::{evaluate_expr, obj_set_value, utf16_to_utf8, utf8_to_utf16, Expr, JSObjectData, JSObjectDataPtr, Value};

static FILE_STORE: LazyLock<Mutex<HashMap<u64, File>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_FILE_ID: LazyLock<Mutex<u64>> = LazyLock::new(|| Mutex::new(1));

fn get_next_file_id() -> u64 {
    let mut id = NEXT_FILE_ID.lock().unwrap();
    let current = *id;
    *id += 1;
    current
}

/// Create a temporary file object
pub(crate) fn create_tmpfile() -> Result<Value, JSError> {
    // Create a real temporary file
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let temp_dir = std::env::temp_dir();
    let filename = temp_dir.join(format!("quickjs_tmp_{}.tmp", timestamp));
    match std::fs::OpenOptions::new().read(true).write(true).create(true).open(&filename) {
        Ok(file) => {
            let file_id = get_next_file_id();
            FILE_STORE.lock().unwrap().insert(file_id, file);

            let mut tmp = Rc::new(RefCell::new(JSObjectData::new()));
            obj_set_value(&mut tmp, "__file_id", Value::Number(file_id as f64))?;
            obj_set_value(&mut tmp, "__eof", Value::Boolean(false))?;
            // methods
            obj_set_value(&mut tmp, "puts", Value::Function("tmp.puts".to_string()))?;
            obj_set_value(&mut tmp, "readAsString", Value::Function("tmp.readAsString".to_string()))?;
            obj_set_value(&mut tmp, "seek", Value::Function("tmp.seek".to_string()))?;
            obj_set_value(&mut tmp, "tell", Value::Function("tmp.tell".to_string()))?;
            obj_set_value(&mut tmp, "putByte", Value::Function("tmp.putByte".to_string()))?;
            obj_set_value(&mut tmp, "getByte", Value::Function("tmp.getByte".to_string()))?;
            obj_set_value(&mut tmp, "getline", Value::Function("tmp.getline".to_string()))?;
            obj_set_value(&mut tmp, "eof", Value::Function("tmp.eof".to_string()))?;
            obj_set_value(&mut tmp, "close", Value::Function("tmp.close".to_string()))?;
            Ok(Value::Object(tmp))
        }
        Err(e) => Err(JSError::EvaluationError {
            message: format!("Failed to create temporary file: {}", e),
        }),
    }
}

/// Handle file object method calls
pub(crate) fn handle_file_method(obj_map: &JSObjectDataPtr, method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // If this object is a file-like object (we use '__file_id' as marker)
    if obj_map.borrow().contains_key("__file_id") {
        let file_id_val = obj_map.borrow().get("__file_id").unwrap().borrow().clone();
        let file_id = match file_id_val {
            Value::Number(n) => n as u64,
            _ => {
                return Err(JSError::EvaluationError {
                    message: "Invalid file object".to_string(),
                })
            }
        };

        let mut file_store = FILE_STORE.lock().unwrap();
        let file = match file_store.get_mut(&file_id) {
            Some(f) => f,
            None => {
                return Err(JSError::EvaluationError {
                    message: "File not found".to_string(),
                })
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
                for a in args {
                    let av = evaluate_expr(env, a)?;
                    match av {
                        Value::String(sv) => to_write.push_str(&utf16_to_utf8(&sv)),
                        Value::Number(n) => to_write.push_str(&n.to_string()),
                        Value::Boolean(b) => to_write.push_str(&b.to_string()),
                        _ => {}
                    }
                }
                // write to file
                if let Err(_) = file.write_all(to_write.as_bytes()) {
                    return Ok(Value::Number(-1.0));
                }
                if let Err(_) = file.flush() {
                    return Ok(Value::Number(-1.0));
                }
                return Ok(Value::Undefined);
            }
            "readAsString" => {
                // flush any pending writes and seek to beginning and read entire file
                if let Err(_) = file.flush() {
                    return Ok(Value::String(utf8_to_utf16("")));
                }
                if let Err(_) = file.seek(SeekFrom::Start(0)) {
                    return Ok(Value::String(utf8_to_utf16("")));
                }
                let mut contents = String::new();
                if let Err(_) = file.read_to_string(&mut contents) {
                    return Ok(Value::String(utf8_to_utf16("")));
                }
                return Ok(Value::String(utf8_to_utf16(&contents)));
            }
            "seek" => {
                // seek(offset, whence)
                if args.len() >= 2 {
                    let offv = evaluate_expr(env, &args[0])?;
                    let whv = evaluate_expr(env, &args[1])?;
                    let offset = match offv {
                        Value::Number(n) => n as i64,
                        _ => 0,
                    };
                    let whence = match whv {
                        Value::Number(n) => n as i32,
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
                if args.len() >= 1 {
                    let bv = evaluate_expr(env, &args[0])?;
                    let byte = match bv {
                        Value::Number(n) => n as u8,
                        _ => 0,
                    };
                    // write byte to file
                    if let Err(_) = file.write_all(&[byte]) {
                        return Ok(Value::Number(-1.0));
                    }
                    if let Err(_) = file.flush() {
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
                        let _ = file.seek(SeekFrom::Current(-1));
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

    Err(JSError::EvaluationError {
        message: format!("File method {method} not implemented"),
    })
}
