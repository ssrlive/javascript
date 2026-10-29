use super::node_common::{make_module, node_arch, node_platform, value_to_i32};
use crate::core::{GcContext, VM, Value};
use indexmap::IndexMap;

pub(crate) fn install<'gc>(vm: &mut VM<'gc>, ctx: &GcContext<'gc>) {
    let process = make_module(
        ctx,
        "process",
        &[
            ("cwd", "process.cwd", 0.0),
            ("nextTick", "process.nextTick", 1.0),
            ("exit", "process.exit", 1.0),
        ],
    );
    if let Value::Object(process_obj) = &process {
        let mut props = process_obj.borrow_mut(ctx);
        props.insert("pid".to_string(), Value::Number(std::process::id() as f64));
        props.insert("platform".to_string(), Value::from(node_platform()));
        props.insert("arch".to_string(), Value::from(node_arch()));
        props.insert(
            "argv".to_string(),
            Value::Array(crate::core::new_gc_cell_ptr(
                ctx,
                crate::core::ArrayData::new(std::env::args().map(|arg| Value::from(arg.as_str())).collect()),
            )),
        );
        props.insert(
            "env".to_string(),
            Value::Object(crate::core::new_gc_cell_ptr(
                ctx,
                std::env::vars()
                    .map(|(key, value)| (key, Value::from(value.as_str())))
                    .collect::<IndexMap<_, _>>(),
            )),
        );
        let mut versions = IndexMap::new();
        versions.insert("node".to_string(), Value::from("22.0.0"));
        versions.insert("javascript".to_string(), Value::from(env!("CARGO_PKG_VERSION")));
        props.insert("versions".to_string(), Value::Object(crate::core::new_gc_cell_ptr(ctx, versions)));
        props.insert(
            "version".to_string(),
            Value::from(format!("v{}", env!("CARGO_PKG_VERSION")).as_str()),
        );
        props.insert("exitCode".to_string(), Value::Undefined);
    }
    vm.register_host_global(ctx, "process", process);
    vm.register_host_module_alias("process", "process");
    vm.register_host_module_alias("node:process", "process");
    vm.register_host_function_handler(call_named);
}

pub(crate) fn call_named<'gc>(
    vm: &mut VM<'gc>,
    ctx: &GcContext<'gc>,
    name: &str,
    _receiver: Option<&Value<'gc>>,
    args: &[Value<'gc>],
) -> Option<Value<'gc>> {
    match name {
        "node.process.cwd" => Some(
            std::env::current_dir()
                .map(|path| Value::from(path.to_string_lossy().as_ref()))
                .unwrap_or(Value::Undefined),
        ),
        "node.process.nextTick" => {
            let callback = args.first().cloned().unwrap_or(Value::Undefined);
            vm.queue_host_callback(
                ctx,
                callback,
                args.iter().skip(1).cloned().collect(),
                "The \"callback\" argument must be of type function",
            );
            Some(Value::Undefined)
        }
        "node.process.exit" => {
            let code = args
                .first()
                .and_then(value_to_i32)
                .or_else(|| {
                    let Some(Value::Object(process)) = vm.host_global("process") else {
                        return None;
                    };
                    match process.borrow().get("exitCode") {
                        Some(Value::Number(code)) => Some(*code as i32),
                        _ => None,
                    }
                })
                .unwrap_or(0);
            if let Some(Value::Object(process)) = vm.host_global("process") {
                process.borrow_mut(ctx).insert("exitCode".to_string(), Value::Number(code as f64));
            }
            vm.request_stop();
            Some(Value::Undefined)
        }
        _ => None,
    }
}
