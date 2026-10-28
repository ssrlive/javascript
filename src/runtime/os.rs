use super::node_common::{make_module, node_arch, node_platform};
use crate::core::{ArrayData, GcContext, VM, Value, new_gc_cell_ptr};
use indexmap::IndexMap;
#[cfg(target_os = "linux")]
use std::fs;

pub(crate) fn install<'gc>(vm: &mut VM<'gc>, ctx: &GcContext<'gc>) {
    let os = make_module(
        ctx,
        "os",
        &[
            ("arch", "os.arch", 0.0),
            ("platform", "os.platform", 0.0),
            ("type", "os.type", 0.0),
            ("release", "os.release", 0.0),
            ("hostname", "os.hostname", 0.0),
            ("endianness", "os.endianness", 0.0),
            ("tmpdir", "os.tmpdir", 0.0),
            ("homedir", "os.homedir", 0.0),
            ("cpus", "os.cpus", 0.0),
            ("totalmem", "os.totalmem", 0.0),
            ("freemem", "os.freemem", 0.0),
            ("uptime", "os.uptime", 0.0),
            ("loadavg", "os.loadavg", 0.0),
        ],
    );
    if let Value::Object(os_obj) = &os {
        let mut props = os_obj.borrow_mut(ctx);
        props.insert("EOL".to_string(), Value::from(if cfg!(windows) { "\r\n" } else { "\n" }));
        props.insert("constants".to_string(), Value::Object(new_gc_cell_ptr(ctx, IndexMap::new())));
    }
    vm.register_host_module("__node_os", os);
    vm.register_host_function_handler(call_named);
}

pub(crate) fn builtin_global(specifier: &str) -> Option<&'static str> {
    matches!(specifier, "os" | "node:os").then_some("__node_os")
}

pub(crate) fn call_named<'gc>(
    _vm: &mut VM<'gc>,
    ctx: &GcContext<'gc>,
    name: &str,
    _receiver: Option<&Value<'gc>>,
    _args: &[Value<'gc>],
) -> Option<Value<'gc>> {
    match name {
        "node.os.arch" => Some(Value::from(node_arch())),
        "node.os.platform" => Some(Value::from(node_platform())),
        "node.os.type" => Some(Value::from(node_os_type())),
        "node.os.release" => Some(Value::from(node_release())),
        "node.os.hostname" => Some(Value::from(
            std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .unwrap_or_default()
                .as_str(),
        )),
        "node.os.endianness" => Some(Value::from(if cfg!(target_endian = "little") { "LE" } else { "BE" })),
        "node.os.tmpdir" => Some(Value::from(std::env::temp_dir().to_string_lossy().as_ref())),
        "node.os.homedir" => Some(Value::from(
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_default()
                .as_str(),
        )),
        "node.os.cpus" => Some(os_cpus(ctx)),
        "node.os.totalmem" => Some(Value::Number(system_memory().0 as f64)),
        "node.os.freemem" => Some(Value::Number(system_memory().1 as f64)),
        "node.os.uptime" => Some(Value::Number(system_uptime())),
        "node.os.loadavg" => Some(Value::Array(new_gc_cell_ptr(
            ctx,
            ArrayData::new(system_loadavg().into_iter().map(Value::Number).collect()),
        ))),
        _ => None,
    }
}

fn os_cpus<'gc>(ctx: &GcContext<'gc>) -> Value<'gc> {
    let count = std::thread::available_parallelism().map(usize::from).unwrap_or(1);
    let cpus = (0..count)
        .map(|_| {
            Value::Object(new_gc_cell_ptr(
                ctx,
                IndexMap::from([
                    ("model".to_string(), Value::from("")),
                    ("speed".to_string(), Value::Number(0.0)),
                    (
                        "times".to_string(),
                        Value::Object(new_gc_cell_ptr(
                            ctx,
                            ["user", "nice", "sys", "idle", "irq"]
                                .into_iter()
                                .map(|name| (name.to_string(), Value::Number(0.0)))
                                .collect(),
                        )),
                    ),
                ]),
            ))
        })
        .collect();
    Value::Array(new_gc_cell_ptr(ctx, ArrayData::new(cpus)))
}

fn node_os_type() -> &'static str {
    if cfg!(windows) {
        "Windows_NT"
    } else if cfg!(target_os = "macos") {
        "Darwin"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        std::env::consts::OS
    }
}

fn node_release() -> String {
    #[cfg(target_os = "linux")]
    if let Ok(release) = fs::read_to_string("/proc/sys/kernel/osrelease") {
        return release.trim().to_string();
    }
    #[cfg(target_os = "macos")]
    if let Some(release) = mac_sysctl_string(b"kern.osrelease\0") {
        return release;
    }
    #[cfg(windows)]
    if let Some(release) = windows_os_release() {
        return release;
    }
    String::new()
}

#[cfg(target_os = "linux")]
fn system_memory() -> (u64, u64) {
    let Ok(info) = fs::read_to_string("/proc/meminfo") else {
        return (0, 0);
    };
    let read_kib = |key: &str| {
        info.lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                (name == key)
                    .then(|| value.split_whitespace().next()?.parse::<u64>().ok())
                    .flatten()
            })
            .unwrap_or(0)
    };
    (read_kib("MemTotal") * 1024, read_kib("MemAvailable") * 1024)
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn system_memory() -> (u64, u64) {
    let Some(total) = mac_sysctl::<u64>(b"hw.memsize\0") else {
        return (0, 0);
    };
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return (total, 0);
    }

    let mut stats = std::mem::MaybeUninit::<libc::vm_statistics64_data_t>::zeroed();
    let mut count = libc::HOST_VM_INFO64_COUNT;
    let result = unsafe { libc::host_statistics64(libc::mach_host_self(), libc::HOST_VM_INFO64, stats.as_mut_ptr().cast(), &mut count) };
    if result != libc::KERN_SUCCESS {
        return (total, 0);
    }
    let stats = unsafe { stats.assume_init() };
    let free_pages = u64::from(stats.free_count).saturating_add(u64::from(stats.speculative_count));
    (total, free_pages.saturating_mul(page_size as u64))
}

#[cfg(windows)]
fn system_memory() -> (u64, u64) {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return (0, 0);
    }
    (status.ullTotalPhys, status.ullAvailPhys)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn system_memory() -> (u64, u64) {
    (0, 0)
}

#[cfg(target_os = "linux")]
fn system_uptime() -> f64 {
    fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|line| line.split_whitespace().next()?.parse().ok())
        .unwrap_or(0.0)
}

#[cfg(target_os = "macos")]
fn system_uptime() -> f64 {
    let Some(boot_time) = mac_sysctl::<libc::timeval>(b"kern.boottime\0") else {
        return 0.0;
    };
    let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return 0.0;
    };
    (now.as_secs_f64() - boot_time.tv_sec as f64 - boot_time.tv_usec as f64 / 1_000_000.0).max(0.0)
}

#[cfg(windows)]
fn system_uptime() -> f64 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;

    unsafe { GetTickCount64() as f64 / 1_000.0 }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn system_uptime() -> f64 {
    0.0
}

#[cfg(target_os = "linux")]
fn system_loadavg() -> [f64; 3] {
    let Ok(contents) = fs::read_to_string("/proc/loadavg") else {
        return [0.0; 3];
    };
    let mut values = [0.0; 3];
    for (index, token) in contents.split_whitespace().take(3).enumerate() {
        values[index] = token.parse().unwrap_or(0.0);
    }
    values
}

#[cfg(target_os = "macos")]
fn system_loadavg() -> [f64; 3] {
    let mut values = [0.0; 3];
    if unsafe { libc::getloadavg(values.as_mut_ptr(), values.len() as i32) } == values.len() as i32 {
        values
    } else {
        [0.0; 3]
    }
}

#[cfg(windows)]
fn system_loadavg() -> [f64; 3] {
    [0.0; 3]
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn system_loadavg() -> [f64; 3] {
    [0.0; 3]
}

#[cfg(target_os = "macos")]
fn mac_sysctl<T: Copy>(name: &[u8]) -> Option<T> {
    let mut value = std::mem::MaybeUninit::<T>::uninit();
    let mut size = std::mem::size_of::<T>();
    let result = unsafe { libc::sysctlbyname(name.as_ptr().cast(), value.as_mut_ptr().cast(), &mut size, std::ptr::null_mut(), 0) };
    (result == 0 && size == std::mem::size_of::<T>()).then(|| unsafe { value.assume_init() })
}

#[cfg(target_os = "macos")]
fn mac_sysctl_string(name: &[u8]) -> Option<String> {
    let mut buffer = [0_u8; 128];
    let mut size = buffer.len();
    let result = unsafe { libc::sysctlbyname(name.as_ptr().cast(), buffer.as_mut_ptr().cast(), &mut size, std::ptr::null_mut(), 0) };
    if result != 0 {
        return None;
    }
    let length = buffer[..size.min(buffer.len())]
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(size.min(buffer.len()));
    Some(String::from_utf8_lossy(&buffer[..length]).into_owned())
}

#[cfg(windows)]
fn windows_os_release() -> Option<String> {
    #[repr(C)]
    struct OsVersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform_id: u32,
        service_pack: [u16; 128],
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(info: *mut OsVersionInfo) -> i32;
    }

    let mut info = OsVersionInfo {
        size: std::mem::size_of::<OsVersionInfo>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform_id: 0,
        service_pack: [0; 128],
    };
    (unsafe { RtlGetVersion(&mut info) } == 0).then(|| format!("{}.{}.{}", info.major, info.minor, info.build))
}
