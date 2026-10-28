use crate::core::{ArrayData, GcContext, ObjectHandle, VM, Value, new_gc_cell_ptr, value_to_string};
use indexmap::IndexMap;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn install<'gc>(vm: &mut VM<'gc>, ctx: &GcContext<'gc>) {
    let promises = make_fs_module(
        ctx,
        "fs/promises",
        &[
            ("readFile", "fs.promises.readFile", 2.0),
            ("writeFile", "fs.promises.writeFile", 3.0),
            ("stat", "fs.promises.stat", 1.0),
            ("lstat", "fs.promises.lstat", 1.0),
            ("access", "fs.promises.access", 2.0),
            ("readdir", "fs.promises.readdir", 2.0),
            ("mkdir", "fs.promises.mkdir", 2.0),
            ("rm", "fs.promises.rm", 2.0),
            ("unlink", "fs.promises.unlink", 1.0),
            ("rename", "fs.promises.rename", 2.0),
        ],
    );
    let fs_module = make_fs_module(
        ctx,
        "fs",
        &[
            ("readFile", "fs.readFile", 3.0),
            ("readFileSync", "fs.readFileSync", 2.0),
            ("writeFile", "fs.writeFile", 4.0),
            ("writeFileSync", "fs.writeFileSync", 3.0),
            ("stat", "fs.stat", 2.0),
            ("statSync", "fs.statSync", 1.0),
            ("lstat", "fs.lstat", 2.0),
            ("lstatSync", "fs.lstatSync", 1.0),
            ("access", "fs.access", 3.0),
            ("accessSync", "fs.accessSync", 2.0),
            ("exists", "fs.exists", 2.0),
            ("existsSync", "fs.existsSync", 1.0),
            ("readdir", "fs.readdir", 3.0),
            ("readdirSync", "fs.readdirSync", 2.0),
            ("mkdir", "fs.mkdir", 3.0),
            ("mkdirSync", "fs.mkdirSync", 2.0),
            ("rm", "fs.rm", 3.0),
            ("rmSync", "fs.rmSync", 2.0),
            ("unlink", "fs.unlink", 2.0),
            ("unlinkSync", "fs.unlinkSync", 1.0),
            ("rename", "fs.rename", 3.0),
            ("renameSync", "fs.renameSync", 2.0),
        ],
    );
    if let Value::Object(fs_obj) = &fs_module {
        fs_obj.borrow_mut(ctx).insert("promises".to_string(), promises.clone());
    }
    vm.register_host_module("__node_fs", fs_module);
    vm.register_host_module("__node_fs_promises", promises);

    let mut buffer_properties = IndexMap::new();
    buffer_properties.insert(
        "isBuffer".to_string(),
        VM::make_host_fn_with_name_len(ctx, "node.buffer.isBuffer", "isBuffer", 1.0, false),
    );
    vm.register_host_global(ctx, "Buffer", Value::Object(new_gc_cell_ptr(ctx, buffer_properties)));
    vm.register_host_function_handler(call_named);
}

pub(crate) fn builtin_global(specifier: &str) -> Option<&'static str> {
    match specifier {
        "fs" | "node:fs" => Some("__node_fs"),
        "fs/promises" | "node:fs/promises" => Some("__node_fs_promises"),
        _ => None,
    }
}

fn make_fs_module<'gc>(ctx: &GcContext<'gc>, module: &str, functions: &[(&str, &str, f64)]) -> Value<'gc> {
    super::node_common::make_module(ctx, module, functions)
}

pub(crate) fn call_named<'gc>(
    vm: &mut VM<'gc>,
    ctx: &GcContext<'gc>,
    name: &str,
    receiver: Option<&Value<'gc>>,
    args: &[Value<'gc>],
) -> Option<Value<'gc>> {
    match name {
        "node.buffer.isBuffer" => Some(Value::Boolean(matches!(
            args.first(),
            Some(Value::Array(array))
                if matches!(array.borrow().props.get("__typedarray_name__"), Some(Value::String(name)) if crate::unicode::utf16_to_utf8(name) == "Buffer")
        ))),
        "node.fs.stats.file" | "node.fs.stats.directory" | "node.fs.stats.symlink" => {
            let expected = name.rsplit('.').next().unwrap_or_default();
            Some(Value::Boolean(matches!(
                receiver,
                Some(Value::Object(stats))
                    if matches!(stats.borrow().get("__node_file_type__"), Some(Value::String(kind)) if crate::unicode::utf16_to_utf8(kind) == expected)
            )))
        }
        "node.fs.buffer.toString" => Some(buffer_to_string(vm, ctx, receiver, args)),
        _ if name.starts_with("node.fs.") => Some(fs_call(vm, ctx, name, args)),
        _ => None,
    }
}

fn buffer_to_string<'gc>(vm: &mut VM<'gc>, ctx: &GcContext<'gc>, receiver: Option<&Value<'gc>>, _args: &[Value<'gc>]) -> Value<'gc> {
    let bytes = match receiver {
        Some(Value::Array(array)) => array
            .borrow()
            .elements
            .iter()
            .map(|value| value_to_i32(value).unwrap_or_default() as u8)
            .collect::<Vec<_>>(),
        _ => {
            vm.throw_host_type_error(ctx, "Buffer.prototype.toString called on an incompatible receiver");
            return Value::Undefined;
        }
    };
    match String::from_utf8(bytes) {
        Ok(text) => Value::from(text.as_str()),
        Err(error) => Value::from(String::from_utf8_lossy(error.as_bytes()).as_ref()),
    }
}

fn fs_call<'gc>(vm: &mut VM<'gc>, ctx: &GcContext<'gc>, name: &str, args: &[Value<'gc>]) -> Value<'gc> {
    let promise_api = name.starts_with("node.fs.promises.");
    let operation = name.rsplit('.').next().unwrap_or_default();
    let is_sync = operation.ends_with("Sync");
    let operation = operation.trim_end_matches("Sync");
    let callback = if !promise_api && !is_sync {
        args.last().filter(|value| vm.is_host_callable(value)).cloned()
    } else {
        None
    };
    if !promise_api && !is_sync && callback.is_none() {
        vm.throw_host_type_error(ctx, "The \"callback\" argument must be of type function");
        return Value::Undefined;
    }
    let result = match args.first().and_then(value_to_path) {
        Some(path) => fs_execute(ctx, operation, path, args),
        None => Err(NodeFsError::invalid_path()),
    };
    if promise_api {
        return vm.settle_host_promise(ctx, result.map_err(|error| Value::Object(error.to_value(ctx))));
    }
    if let Some(callback) = callback {
        let callback_args = match result {
            Ok(Value::Boolean(found)) if operation == "exists" => vec![Value::Boolean(found)],
            Ok(value) => vec![Value::Null, value],
            Err(_error) if operation == "exists" => vec![Value::Boolean(false)],
            Err(error) => vec![Value::Object(error.to_value(ctx))],
        };
        vm.queue_host_callback(ctx, callback, callback_args, "The \"callback\" argument must be of type function");
        return Value::Undefined;
    }
    match result {
        Ok(value) => value,
        Err(error) => {
            vm.throw_host_value(Value::Object(error.to_value(ctx)));
            Value::Undefined
        }
    }
}

fn fs_execute<'gc>(ctx: &GcContext<'gc>, operation: &str, path: PathBuf, args: &[Value<'gc>]) -> Result<Value<'gc>, NodeFsError> {
    match operation {
        "readFile" => {
            let bytes = fs::read(&path).map_err(|error| NodeFsError::from_io(error, &path))?;
            if let Some(encoding) = fs_encoding(args) {
                let text = String::from_utf8(bytes).map_err(|error| NodeFsError::encoding(error.to_string(), &path))?;
                if encoding.eq_ignore_ascii_case("utf8") || encoding.eq_ignore_ascii_case("utf-8") {
                    Ok(Value::from(text.as_str()))
                } else {
                    Err(NodeFsError::unsupported_encoding(&encoding, &path))
                }
            } else {
                Ok(make_buffer(ctx, bytes))
            }
        }
        "writeFile" => {
            let data = args.get(1).map(fs_data_bytes).unwrap_or_default();
            fs::write(&path, data).map_err(|error| NodeFsError::from_io(error, &path))?;
            Ok(Value::Undefined)
        }
        "stat" | "lstat" => {
            let metadata = if operation == "lstat" {
                fs::symlink_metadata(&path)
            } else {
                fs::metadata(&path)
            }
            .map_err(|error| NodeFsError::from_io(error, &path))?;
            Ok(stats_object(ctx, metadata))
        }
        "access" => fs::metadata(&path)
            .map(|_| Value::Undefined)
            .map_err(|error| NodeFsError::from_io(error, &path)),
        "exists" => Ok(Value::Boolean(path.exists())),
        "readdir" => {
            let entries = fs::read_dir(&path).map_err(|error| NodeFsError::from_io(error, &path))?;
            let mut names = Vec::new();
            for entry in entries {
                let entry = entry.map_err(|error| NodeFsError::from_io(error, &path))?;
                names.push(Value::from(entry.file_name().to_string_lossy().as_ref()));
            }
            Ok(Value::Array(new_gc_cell_ptr(ctx, ArrayData::new(names))))
        }
        "mkdir" => {
            if fs_recursive(args) {
                fs::create_dir_all(&path)
            } else {
                fs::create_dir(&path)
            }
            .map_err(|error| NodeFsError::from_io(error, &path))?;
            Ok(Value::Undefined)
        }
        "rm" => {
            if !path.exists() && fs_force(args) {
                return Ok(Value::Undefined);
            }
            let metadata = fs::symlink_metadata(&path).map_err(|error| NodeFsError::from_io(error, &path))?;
            if metadata.is_dir() {
                if fs_recursive(args) {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_dir(&path)
                }
            } else {
                fs::remove_file(&path)
            }
            .map_err(|error| NodeFsError::from_io(error, &path))?;
            Ok(Value::Undefined)
        }
        "unlink" => {
            fs::remove_file(&path).map_err(|error| NodeFsError::from_io(error, &path))?;
            Ok(Value::Undefined)
        }
        "rename" => {
            let destination = args.get(1).and_then(value_to_path).ok_or_else(NodeFsError::invalid_path)?;
            fs::rename(&path, &destination).map_err(|error| NodeFsError::from_io(error, &path))?;
            Ok(Value::Undefined)
        }
        _ => Err(NodeFsError::unsupported(operation)),
    }
}

fn stats_object<'gc>(ctx: &GcContext<'gc>, metadata: fs::Metadata) -> Value<'gc> {
    let mut props = IndexMap::new();
    props.insert("size".to_string(), Value::Number(metadata.len() as f64));
    props.insert("mode".to_string(), Value::Number(node_file_mode(&metadata) as f64));
    props.insert(
        "mtimeMs".to_string(),
        Value::Number(
            metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as f64)
                .unwrap_or(0.0),
        ),
    );
    props.insert(
        "__node_file_type__".to_string(),
        Value::from(if metadata.file_type().is_symlink() {
            "symlink"
        } else if metadata.is_file() {
            "file"
        } else if metadata.is_dir() {
            "directory"
        } else {
            "other"
        }),
    );
    for (method, kind) in [("isFile", "file"), ("isDirectory", "directory"), ("isSymbolicLink", "symlink")] {
        props.insert(
            method.to_string(),
            VM::make_host_fn_with_name_len(ctx, &format!("node.fs.stats.{kind}"), method, 0.0, false),
        );
    }
    Value::Object(new_gc_cell_ptr(ctx, props))
}

struct NodeFsError {
    code: String,
    message: String,
    path: Option<String>,
}

impl NodeFsError {
    fn invalid_path() -> Self {
        Self {
            code: "ERR_INVALID_ARG_TYPE".into(),
            message: "The \"path\" argument must be a string".into(),
            path: None,
        }
    }

    fn unsupported(operation: &str) -> Self {
        Self {
            code: "ERR_METHOD_NOT_IMPLEMENTED".into(),
            message: format!("fs.{operation} is not implemented"),
            path: None,
        }
    }

    fn unsupported_encoding(encoding: &str, path: &Path) -> Self {
        Self {
            code: "ERR_UNKNOWN_ENCODING".into(),
            message: format!("Unknown encoding: {encoding}"),
            path: Some(path.to_string_lossy().into_owned()),
        }
    }

    fn encoding(message: String, path: &Path) -> Self {
        Self {
            code: "ERR_ENCODING_INVALID_ENCODED_DATA".into(),
            message,
            path: Some(path.to_string_lossy().into_owned()),
        }
    }

    fn from_io(error: std::io::Error, path: &Path) -> Self {
        let code = match error.kind() {
            std::io::ErrorKind::NotFound => "ENOENT",
            std::io::ErrorKind::PermissionDenied => "EACCES",
            std::io::ErrorKind::AlreadyExists => "EEXIST",
            std::io::ErrorKind::NotADirectory => "ENOTDIR",
            std::io::ErrorKind::IsADirectory => "EISDIR",
            _ => "EIO",
        };
        Self {
            code: code.into(),
            message: error.to_string(),
            path: Some(path.to_string_lossy().into_owned()),
        }
    }

    fn to_value<'gc>(&self, ctx: &GcContext<'gc>) -> ObjectHandle<'gc> {
        let mut props = IndexMap::new();
        props.insert("__type__".to_string(), Value::from("Error"));
        props.insert("name".to_string(), Value::from("Error"));
        props.insert("message".to_string(), Value::from(self.message.as_str()));
        props.insert("code".to_string(), Value::from(self.code.as_str()));
        if let Some(path) = &self.path {
            props.insert("path".to_string(), Value::from(path.as_str()));
        }
        new_gc_cell_ptr(ctx, props)
    }
}

fn make_buffer<'gc>(ctx: &GcContext<'gc>, bytes: Vec<u8>) -> Value<'gc> {
    let elements = bytes.iter().map(|byte| Value::Number(*byte as f64)).collect();
    let mut props = IndexMap::new();
    props.insert("__typedarray_name__".to_string(), Value::from("Buffer"));
    props.insert("__bytes_per_element__".to_string(), Value::Number(1.0));
    props.insert("__buffer_byte_length__".to_string(), Value::Number(bytes.len() as f64));
    props.insert("length".to_string(), Value::Number(bytes.len() as f64));
    props.insert(
        "toString".to_string(),
        VM::make_host_fn_with_name_len(ctx, "node.fs.buffer.toString", "toString", 0.0, false),
    );
    Value::Array(new_gc_cell_ptr(ctx, ArrayData { elements, props }))
}

fn fs_encoding<'gc>(args: &[Value<'gc>]) -> Option<String> {
    for arg in args.iter().skip(1) {
        if let Value::String(value) = arg {
            return Some(crate::unicode::utf16_to_utf8(value));
        }
        if let Value::Object(options) = arg
            && let Some(Value::String(value)) = options.borrow().get("encoding")
        {
            return Some(crate::unicode::utf16_to_utf8(value));
        }
    }
    None
}

fn fs_recursive<'gc>(args: &[Value<'gc>]) -> bool {
    args.iter()
        .skip(1)
        .any(|arg| matches!(arg, Value::Object(options) if matches!(options.borrow().get("recursive"), Some(Value::Boolean(true)))))
}

fn fs_force<'gc>(args: &[Value<'gc>]) -> bool {
    args.iter()
        .skip(1)
        .any(|arg| matches!(arg, Value::Object(options) if matches!(options.borrow().get("force"), Some(Value::Boolean(true)))))
}

fn fs_data_bytes<'gc>(value: &Value<'gc>) -> Vec<u8> {
    match value {
        Value::String(text) => crate::unicode::utf16_to_utf8(text).into_bytes(),
        Value::Array(array) => array
            .borrow()
            .elements
            .iter()
            .map(|value| value_to_i32(value).unwrap_or(0) as u8)
            .collect(),
        _ => value_to_string(value).into_bytes(),
    }
}

fn value_to_path<'gc>(value: &Value<'gc>) -> Option<PathBuf> {
    match value {
        Value::String(path) => Some(PathBuf::from(crate::unicode::utf16_to_utf8(path))),
        _ => None,
    }
}

fn value_to_i32<'gc>(value: &Value<'gc>) -> Option<i32> {
    match value {
        Value::Number(number) => Some(*number as i32),
        _ => None,
    }
}

#[cfg(unix)]
fn node_file_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn node_file_mode(metadata: &fs::Metadata) -> u32 {
    if metadata.permissions().readonly() { 0o444 } else { 0o666 }
}
