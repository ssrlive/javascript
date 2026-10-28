use crate::core::{GcContext, VM, Value, new_gc_cell_ptr};
use indexmap::IndexMap;

pub(crate) fn make_module<'gc>(ctx: &GcContext<'gc>, module: &str, functions: &[(&str, &str, f64)]) -> Value<'gc> {
    let mut properties = IndexMap::new();
    for &(name, host_name, length) in functions {
        properties.insert(
            name.to_string(),
            VM::make_host_fn_with_name_len(ctx, &format!("node.{host_name}"), name, length, false),
        );
    }
    properties.insert("__node_module__".to_string(), Value::from(module));
    Value::Object(new_gc_cell_ptr(ctx, properties))
}

#[cfg(feature = "process")]
pub(crate) fn value_to_i32<'gc>(value: &Value<'gc>) -> Option<i32> {
    match value {
        Value::Number(number) => Some(*number as i32),
        _ => None,
    }
}

#[cfg(any(feature = "process", feature = "os"))]
pub(crate) fn node_platform() -> &'static str {
    if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        std::env::consts::OS
    }
}

#[cfg(any(feature = "process", feature = "os"))]
pub(crate) fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "x86" => "ia32",
        "aarch64" => "arm64",
        other => other,
    }
}
