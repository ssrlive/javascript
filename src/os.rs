use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::{LazyLock, Mutex};

use crate::error::JSError;
use crate::quickjs::{evaluate_expr, obj_set_val, utf16_to_utf8, utf8_to_utf16, Expr, JSObjectData, Value};

static OS_FILE_STORE: LazyLock<Mutex<HashMap<u64, File>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_OS_FILE_ID: LazyLock<Mutex<u64>> = LazyLock::new(|| Mutex::new(1));

fn get_next_os_file_id() -> u64 {
    let mut id = NEXT_OS_FILE_ID.lock().unwrap();
    let current = *id;
    *id += 1;
    current
}

#[cfg(windows)]
fn get_parent_pid_windows() -> u32 {
    use windows_sys::Win32::Foundation::{CloseHandle, FALSE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32First, Process32Next, PROCESSENTRY32, TH32CS_SNAPPROCESS,
    };

    let current_pid = std::process::id();
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };

    if snapshot == INVALID_HANDLE_VALUE {
        return 0;
    }

    let mut pe: PROCESSENTRY32 = unsafe { std::mem::zeroed() };
    pe.dwSize = std::mem::size_of::<PROCESSENTRY32>() as u32;

    if unsafe { Process32First(snapshot, &mut pe) } == FALSE {
        unsafe { CloseHandle(snapshot) };
        return 0;
    }

    let mut ppid = 0;
    loop {
        if pe.th32ProcessID == current_pid {
            ppid = pe.th32ParentProcessID;
            break;
        }
        if unsafe { Process32Next(snapshot, &mut pe) } == FALSE {
            break;
        }
    }

    unsafe { CloseHandle(snapshot) };
    ppid
}

/// Handle OS module method calls
pub(crate) fn handle_os_method(obj_map: &JSObjectData, method: &str, args: &[Expr], env: &JSObjectData) -> Result<Value, JSError> {
    // If this object looks like the `os` module (we used 'open' as marker)
    if obj_map.contains_key("open") {
        match method {
            "open" => {
                if args.len() >= 1 {
                    let filename_val = evaluate_expr(env, &args[0])?;
                    let filename = match filename_val {
                        Value::String(s) => utf16_to_utf8(&s),
                        _ => {
                            return Err(JSError::EvaluationError {
                                message: "os.open filename must be a string".to_string(),
                            })
                        }
                    };
                    let flags = if args.len() >= 2 {
                        match evaluate_expr(env, &args[1])? {
                            Value::Number(n) => n as i32,
                            _ => 0,
                        }
                    } else {
                        0
                    };
                    // For simplicity, treat flags as: 0=read, 1=write, 2=read+write
                    let mut options = std::fs::OpenOptions::new();
                    if flags & 2 != 0 {
                        // O_RDWR
                        options.read(true).write(true);
                    } else if flags & 1 != 0 {
                        // O_WRONLY
                        options.write(true);
                    } else {
                        options.read(true);
                    }
                    if flags & 64 != 0 {
                        // O_CREAT
                        options.create(true);
                    }
                    if flags & 512 != 0 {
                        // O_TRUNC
                        options.truncate(true);
                    }
                    match options.open(&filename) {
                        Ok(file) => {
                            let fd = get_next_os_file_id();
                            OS_FILE_STORE.lock().unwrap().insert(fd, file);
                            return Ok(Value::Number(fd as f64));
                        }
                        Err(e) => {
                            return Err(JSError::EvaluationError {
                                message: format!("Failed to open file: {}", e),
                            })
                        }
                    }
                }
                return Ok(Value::Number(-1.0));
            }
            "close" => {
                if args.len() >= 1 {
                    let fd_val = evaluate_expr(env, &args[0])?;
                    let fd = match fd_val {
                        Value::Number(n) => n as u64,
                        _ => {
                            return Err(JSError::EvaluationError {
                                message: "os.close fd must be a number".to_string(),
                            })
                        }
                    };
                    let mut store = OS_FILE_STORE.lock().unwrap();
                    if store.remove(&fd).is_some() {
                        return Ok(Value::Number(0.0));
                    } else {
                        return Ok(Value::Number(-1.0));
                    }
                }
                return Ok(Value::Number(-1.0));
            }
            "read" => {
                if args.len() >= 2 {
                    let fd_val = evaluate_expr(env, &args[0])?;
                    let size_val = evaluate_expr(env, &args[1])?;
                    let fd = match fd_val {
                        Value::Number(n) => n as u64,
                        _ => {
                            return Err(JSError::EvaluationError {
                                message: "os.read fd must be a number".to_string(),
                            })
                        }
                    };
                    let size = match size_val {
                        Value::Number(n) => n as usize,
                        _ => 0,
                    };
                    let mut store = OS_FILE_STORE.lock().unwrap();
                    if let Some(file) = store.get_mut(&fd) {
                        let mut buf = vec![0u8; size];
                        match file.read(&mut buf) {
                            Ok(n) => {
                                buf.truncate(n);
                                return Ok(Value::String(utf8_to_utf16(&String::from_utf8_lossy(&buf))));
                            }
                            Err(_) => return Ok(Value::String(utf8_to_utf16(""))),
                        }
                    }
                }
                return Ok(Value::String(utf8_to_utf16("")));
            }
            "write" => {
                if args.len() >= 2 {
                    let fd_val = evaluate_expr(env, &args[0])?;
                    let data_val = evaluate_expr(env, &args[1])?;
                    let fd = match fd_val {
                        Value::Number(n) => n as u64,
                        _ => {
                            return Err(JSError::EvaluationError {
                                message: "os.write fd must be a number".to_string(),
                            })
                        }
                    };
                    let data = match data_val {
                        Value::String(s) => utf16_to_utf8(&s),
                        _ => "".to_string(),
                    };
                    let mut store = OS_FILE_STORE.lock().unwrap();
                    if let Some(file) = store.get_mut(&fd) {
                        match file.write_all(data.as_bytes()) {
                            Ok(_) => {
                                let _ = file.flush();
                                return Ok(Value::Number(data.len() as f64));
                            }
                            Err(_) => return Ok(Value::Number(-1.0)),
                        }
                    }
                }
                return Ok(Value::Number(-1.0));
            }
            "seek" => {
                if args.len() >= 3 {
                    let fd_val = evaluate_expr(env, &args[0])?;
                    let offset_val = evaluate_expr(env, &args[1])?;
                    let whence_val = evaluate_expr(env, &args[2])?;
                    let fd = match fd_val {
                        Value::Number(n) => n as u64,
                        _ => {
                            return Err(JSError::EvaluationError {
                                message: "os.seek fd must be a number".to_string(),
                            })
                        }
                    };
                    let offset = match offset_val {
                        Value::Number(n) => n as i64,
                        _ => 0,
                    };
                    let whence = match whence_val {
                        Value::Number(n) => n as i32,
                        _ => 0,
                    };
                    let seek_from = match whence {
                        0 => SeekFrom::Start(offset as u64), // SEEK_SET
                        1 => SeekFrom::Current(offset),      // SEEK_CUR
                        2 => SeekFrom::End(offset),          // SEEK_END
                        _ => SeekFrom::Start(0),
                    };
                    let mut store = OS_FILE_STORE.lock().unwrap();
                    if let Some(file) = store.get_mut(&fd) {
                        match file.seek(seek_from) {
                            Ok(pos) => return Ok(Value::Number(pos as f64)),
                            Err(_) => return Ok(Value::Number(-1.0)),
                        }
                    }
                }
                return Ok(Value::Number(-1.0));
            }
            "remove" => {
                if args.len() >= 1 {
                    let filename_val = evaluate_expr(env, &args[0])?;
                    let filename = match filename_val {
                        Value::String(s) => utf16_to_utf8(&s),
                        _ => {
                            return Err(JSError::EvaluationError {
                                message: "os.remove filename must be a string".to_string(),
                            })
                        }
                    };
                    match std::fs::remove_file(&filename) {
                        Ok(_) => return Ok(Value::Number(0.0)),
                        Err(_) => return Ok(Value::Number(-1.0)),
                    }
                }
                return Ok(Value::Number(-1.0));
            }
            "mkdir" => {
                if args.len() >= 1 {
                    let dirname_val = evaluate_expr(env, &args[0])?;
                    let dirname = match dirname_val {
                        Value::String(s) => utf16_to_utf8(&s),
                        _ => {
                            return Err(JSError::EvaluationError {
                                message: "os.mkdir dirname must be a string".to_string(),
                            })
                        }
                    };
                    match std::fs::create_dir(&dirname) {
                        Ok(_) => return Ok(Value::Number(0.0)),
                        Err(_) => return Ok(Value::Number(-1.0)),
                    }
                }
                return Ok(Value::Number(-1.0));
            }
            "readdir" => {
                if args.len() >= 1 {
                    let dirname_val = evaluate_expr(env, &args[0])?;
                    let dirname = match dirname_val {
                        Value::String(s) => utf16_to_utf8(&s),
                        _ => {
                            return Err(JSError::EvaluationError {
                                message: "os.readdir dirname must be a string".to_string(),
                            })
                        }
                    };
                    match std::fs::read_dir(&dirname) {
                        Ok(entries) => {
                            let mut obj = JSObjectData::new();
                            let mut i = 0;
                            for entry in entries {
                                if let Ok(entry) = entry {
                                    if let Some(name) = entry.file_name().to_str() {
                                        obj_set_val(&mut obj, &i.to_string(), Value::String(utf8_to_utf16(name)));
                                        i += 1;
                                    }
                                }
                            }
                            obj_set_val(&mut obj, "length", Value::Number(i as f64));
                            return Ok(Value::Object(obj));
                        }
                        Err(_) => {
                            let mut obj = JSObjectData::new();
                            obj_set_val(&mut obj, "length", Value::Number(0.0));
                            return Ok(Value::Object(obj));
                        }
                    }
                }
                let mut obj = JSObjectData::new();
                obj_set_val(&mut obj, "length", Value::Number(0.0));
                return Ok(Value::Object(obj));
            }
            "getcwd" => {
                match std::env::current_dir() {
                    Ok(path) => {
                        if let Some(path_str) = path.to_str() {
                            return Ok(Value::String(utf8_to_utf16(path_str)));
                        }
                    }
                    _ => {}
                }
                return Ok(Value::String(utf8_to_utf16("")));
            }
            "getpid" => {
                return Ok(Value::Number(std::process::id() as f64));
            }
            "getppid" => {
                #[cfg(unix)]
                {
                    let ppid = unsafe { libc::getppid() };
                    return Ok(Value::Number(ppid as f64));
                }
                #[cfg(windows)]
                {
                    let ppid = get_parent_pid_windows();
                    return Ok(Value::Number(ppid as f64));
                }
                #[cfg(not(any(unix, windows)))]
                {
                    return Ok(Value::Number(0.0));
                }
            }
            _ => {}
        }
    }

    // If this object looks like the `os.path` module
    if obj_map.contains_key("join") {
        match method {
            "join" => {
                let mut result = String::new();
                for (i, arg) in args.iter().enumerate() {
                    let val = evaluate_expr(env, arg)?;
                    let part = match val {
                        Value::String(s) => utf16_to_utf8(&s),
                        _ => "".to_string(),
                    };
                    if i > 0 {
                        result.push('\\'); // Windows path separator
                    }
                    result.push_str(&part);
                }
                return Ok(Value::String(utf8_to_utf16(&result)));
            }
            "dirname" => {
                if args.len() >= 1 {
                    let val = evaluate_expr(env, &args[0])?;
                    let path = match val {
                        Value::String(s) => utf16_to_utf8(&s),
                        _ => "".to_string(),
                    };
                    let path_obj = std::path::Path::new(&path);
                    if let Some(parent) = path_obj.parent() {
                        if let Some(parent_str) = parent.to_str() {
                            return Ok(Value::String(utf8_to_utf16(parent_str)));
                        }
                    }
                    return Ok(Value::String(utf8_to_utf16(".")));
                }
                return Ok(Value::String(utf8_to_utf16(".")));
            }
            "basename" => {
                if args.len() >= 1 {
                    let val = evaluate_expr(env, &args[0])?;
                    let path = match val {
                        Value::String(s) => utf16_to_utf8(&s),
                        _ => "".to_string(),
                    };
                    let path_obj = std::path::Path::new(&path);
                    if let Some(filename) = path_obj.file_name() {
                        if let Some(filename_str) = filename.to_str() {
                            return Ok(Value::String(utf8_to_utf16(filename_str)));
                        }
                    }
                    return Ok(Value::String(utf8_to_utf16("")));
                }
                return Ok(Value::String(utf8_to_utf16("")));
            }
            "extname" => {
                if args.len() >= 1 {
                    let val = evaluate_expr(env, &args[0])?;
                    let path = match val {
                        Value::String(s) => utf16_to_utf8(&s),
                        _ => "".to_string(),
                    };
                    let path_obj = std::path::Path::new(&path);
                    if let Some(extension) = path_obj.extension() {
                        if let Some(ext_str) = extension.to_str() {
                            return Ok(Value::String(utf8_to_utf16(&format!(".{}", ext_str))));
                        }
                    }
                    return Ok(Value::String(utf8_to_utf16("")));
                }
                return Ok(Value::String(utf8_to_utf16("")));
            }
            "resolve" => {
                if args.len() >= 1 {
                    let val = evaluate_expr(env, &args[0])?;
                    let path = match val {
                        Value::String(s) => utf16_to_utf8(&s),
                        _ => "".to_string(),
                    };
                    match std::fs::canonicalize(&path) {
                        Ok(canonical) => {
                            if let Some(canonical_str) = canonical.to_str() {
                                return Ok(Value::String(utf8_to_utf16(canonical_str)));
                            }
                        }
                        _ => {}
                    }
                    return Ok(Value::String(utf8_to_utf16(&path)));
                }
                return Ok(Value::String(utf8_to_utf16("")));
            }
            "normalize" => {
                if args.len() >= 1 {
                    let val = evaluate_expr(env, &args[0])?;
                    let path = match val {
                        Value::String(s) => utf16_to_utf8(&s),
                        _ => "".to_string(),
                    };
                    let normalized = std::path::Path::new(&path).to_string_lossy().to_string();
                    return Ok(Value::String(utf8_to_utf16(&normalized)));
                }
                return Ok(Value::String(utf8_to_utf16("")));
            }
            "isAbsolute" => {
                if args.len() >= 1 {
                    let val = evaluate_expr(env, &args[0])?;
                    let path = match val {
                        Value::String(s) => utf16_to_utf8(&s),
                        _ => "".to_string(),
                    };
                    let is_absolute = std::path::Path::new(&path).is_absolute();
                    return Ok(Value::Boolean(is_absolute));
                }
                return Ok(Value::Boolean(false));
            }
            _ => {}
        }
    }

    Err(JSError::EvaluationError {
        message: format!("OS method {method} not implemented"),
    })
}
