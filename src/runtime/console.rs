use crate::core::{GcContext, VM, Value, new_gc_cell_ptr, value_to_string};
use indexmap::IndexMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleLevel {
    Log,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleMessage {
    pub level: ConsoleLevel,
    pub text: String,
}

pub(crate) struct ConsoleState {
    output: Vec<ConsoleMessage>,
    writes_stdout: bool,
}

impl Default for ConsoleState {
    fn default() -> Self {
        Self {
            output: Vec::new(),
            writes_stdout: true,
        }
    }
}

impl ConsoleState {
    pub(crate) fn set_stdout(&mut self, enabled: bool) {
        self.writes_stdout = enabled;
    }

    pub(crate) fn take_output(&mut self) -> Vec<ConsoleMessage> {
        std::mem::take(&mut self.output)
    }
}

pub(crate) fn new_console_object<'gc>(ctx: &GcContext<'gc>) -> Value<'gc> {
    let mut properties = IndexMap::new();
    for (name, host_name) in [("log", "console.log"), ("warn", "console.warn"), ("error", "console.error")] {
        properties.insert(name.to_string(), VM::make_host_fn_with_name_len(ctx, host_name, name, 0.0, false));
    }
    Value::Object(new_gc_cell_ptr(ctx, properties))
}

pub(crate) fn install<'gc>(vm: &mut VM<'gc>, ctx: &GcContext<'gc>) {
    vm.register_host_global(ctx, "console", new_console_object(ctx));
    vm.register_host_function_handler(dispatch);
}

fn dispatch<'gc>(
    vm: &mut VM<'gc>,
    ctx: &GcContext<'gc>,
    name: &str,
    _receiver: Option<&Value<'gc>>,
    args: &[Value<'gc>],
) -> Option<Value<'gc>> {
    call_named(vm, ctx, name, args)
}

pub(crate) fn call_named<'gc>(vm: &mut VM<'gc>, ctx: &GcContext<'gc>, name: &str, args: &[Value<'gc>]) -> Option<Value<'gc>> {
    match name {
        "console.log" => Some(call(vm, ctx, ConsoleLevel::Log, args)),
        "console.warn" => Some(call(vm, ctx, ConsoleLevel::Warn, args)),
        "console.error" => Some(call(vm, ctx, ConsoleLevel::Error, args)),
        _ => None,
    }
}

fn call<'gc>(vm: &mut VM<'gc>, ctx: &GcContext<'gc>, level: ConsoleLevel, args: &[Value<'gc>]) -> Value<'gc> {
    let text = args.iter().map(|value| display_value(vm, ctx, value)).collect::<Vec<_>>().join(" ");
    vm.console.output.push(ConsoleMessage { level, text: text.clone() });
    if vm.console.writes_stdout {
        println!("{text}");
    }
    Value::Undefined
}

fn display_value<'gc>(vm: &mut VM<'gc>, ctx: &GcContext<'gc>, value: &Value<'gc>) -> String {
    if let Some(symbol_string) = vm.symbol_display_string(value) {
        return symbol_string;
    }

    if let Value::Object(map) = value {
        let borrow = map.borrow();
        if let Some(Value::String(type_name)) = borrow.get("__type__") {
            let error_name = crate::unicode::utf16_to_utf8(type_name);
            if VM::is_error_type_name(&error_name) {
                let message = borrow.get("message").map(value_to_string).unwrap_or_default();
                return VM::format_error_name_message(&error_name, &message);
            }
        }
        if borrow.get("__type__").map(value_to_string).as_deref() == Some("RegExp") {
            let pattern = match borrow.get("__regex_pattern__") {
                Some(Value::String(pattern)) => pattern.clone(),
                Some(value) => crate::unicode::utf8_to_utf16(&value_to_string(value)),
                None => Vec::new(),
            };
            let flags = borrow.get("__regex_flags__").map(value_to_string).unwrap_or_default();
            return format!("/{}/{}", crate::unicode::utf16_to_utf8(&pattern), flags);
        }
        drop(borrow);

        if let Some(Value::Function(ip, _) | Value::Closure(ip, _, _)) = map.borrow().get("toString").cloned() {
            let result = vm.call_vm_function_result(ctx, ip, &[], None, &[]).unwrap_or(Value::Undefined);
            return value_to_string(&result);
        }
        if let Some(Value::NativeFunction(_)) = map.borrow().get("toString") {
            return value_to_string(value);
        }
        if let Some(inner) = map.borrow().get("__value__") {
            return value_to_string(inner);
        }
    }

    value_to_string(value)
}
