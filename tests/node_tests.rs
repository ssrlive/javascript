#![cfg(feature = "node")]

use javascript::{JsRuntime, JsValue, evaluate_script};

fn evaluate_module(source: &str) -> String {
    evaluate_script(source, true, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap()
}

#[test]
fn process_is_available_as_global_and_builtin_module() {
    let result = evaluate_module(
        r#"
            import processModule from "node:process";
            processModule === process && process.pid > 0 && process.platform.length > 0;
        "#,
    );
    assert_eq!(result, "true");
}

#[test]
fn os_exports_node_style_host_information() {
    let result = evaluate_module(
        r#"
            import * as os from "os";
            os.arch().length > 0 && os.platform().length > 0 && os.cpus().length > 0;
        "#,
    );
    assert_eq!(result, "true");
}

#[test]
fn os_reports_host_memory_uptime_and_load_average() {
    let result = evaluate_module(
        r#"
            import * as os from "os";
            os.totalmem() > 0 && os.freemem() > 0 && os.uptime() > 0 && os.loadavg().length === 3;
        "#,
    );
    assert_eq!(result, "true");
}

#[test]
fn fs_sync_read_write_and_stats_work() {
    let path = std::env::temp_dir().join(format!(
        "javascript-node-fs-{}-{}.txt",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let path = serde_json::to_string(path.to_str().unwrap()).unwrap();
    let result = evaluate_module(&format!(
        r#"
            import fs from "node:fs";
            fs.writeFileSync({path}, "host modules");
            let content = fs.readFileSync({path}, "utf8");
            let buffer = fs.readFileSync({path});
            let stats = fs.statSync({path});
            fs.unlinkSync({path});
            content === "host modules" && Buffer.isBuffer(buffer) && buffer.toString() === content &&
                stats.isFile() && stats.size === 12;
        "#
    ));
    assert_eq!(result, "true");
}

#[test]
fn fs_promises_are_available_as_builtin_module() {
    let path = std::env::temp_dir().join(format!("javascript-node-fs-promise-{}.txt", std::process::id()));
    let path = serde_json::to_string(path.to_str().unwrap()).unwrap();
    let result = evaluate_module(&format!(
        r#"
            import fs from "node:fs/promises";
            await fs.writeFile({path}, "promise api");
            let content = await fs.readFile({path}, "utf8");
            await fs.unlink({path});
            content;
        "#
    ));
    assert_eq!(result, r#""promise api""#);
}

#[test]
fn callback_apis_and_next_tick_schedule_callbacks() {
    let path = std::env::temp_dir().join(format!("javascript-node-fs-callback-{}.txt", std::process::id()));
    let path = serde_json::to_string(path.to_str().unwrap()).unwrap();
    let result = evaluate_module(&format!(
        r#"
            import fs from "fs";
            let results = [];
            fs.writeFile({path}, "callback api", error => {{
                if (error) throw error;
                fs.readFile({path}, "utf8", (readError, data) => {{
                    if (readError) throw readError;
                    results.push(data);
                    fs.unlinkSync({path});
                }});
            }});
            process.nextTick(value => results.push(value), "next tick");
            results;
        "#
    ));
    assert_eq!(result, r#"["next tick","callback api"]"#);
}

#[test]
fn process_exit_is_observable_without_exiting_the_host() {
    let mut runtime = JsRuntime::new();
    let result = runtime
        .eval("process.exitCode = 23; process.exit(); globalThis.afterExit = true;")
        .unwrap();
    assert_eq!(result, JsValue::Undefined);
    assert!(runtime.process_exit_requested());
    assert_eq!(runtime.process_exit_code(), Some(23));
}
