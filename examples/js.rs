use javascript::*;
use std::process;

#[derive(clap::Parser)]
#[command(name = "js", version, about = "JavaScript Rust Interpreter")]
struct Cli {
    /// Execute script
    #[arg(short, long)]
    eval: Option<String>,

    /// JavaScript file to execute
    file: Option<std::path::PathBuf>,
}

unsafe fn get_js_string(val: &JSValue) -> String {
    unsafe {
        if val.get_tag() != JS_TAG_STRING {
            return String::new();
        }
        let p = val.get_ptr() as *mut JSString;
        if p.is_null() {
            return String::new();
        }
        let len = (*p).len as usize;
        let str_data = (p as *mut u8).add(std::mem::size_of::<JSString>());
        let bytes = std::slice::from_raw_parts(str_data, len);
        String::from_utf8_lossy(bytes).to_string()
    }
}

fn main() {
    let cli = <Cli as clap::Parser>::parse();

    // Initialize logger (controlled by RUST_LOG)
    env_logger::init();

    let script_content: String;
    let mut filename = "<eval>".to_string();

    if let Some(script) = cli.eval {
        script_content = script;
    } else if let Some(file) = cli.file {
        filename = file.to_string_lossy().to_string();
        match std::fs::read_to_string(&file) {
            Ok(content) => script_content = content,
            Err(e) => {
                eprintln!("Error reading file {}: {}", file.display(), e);
                process::exit(1);
            }
        }
    } else {
        eprintln!("Error: Must provide either --eval or a file");
        process::exit(1);
    }

    unsafe {
        let rt = JS_NewRuntime();
        if rt.is_null() {
            eprintln!("Failed to create runtime");
            process::exit(1);
        }
        let ctx = JS_NewContext(rt);
        if ctx.is_null() {
            eprintln!("Failed to create context");
            JS_FreeRuntime(rt);
            process::exit(1);
        }

        let script_c = std::ffi::CString::new(script_content.clone()).unwrap();
        let result = JS_Eval(
            ctx,
            script_c.as_ptr(),
            script_content.len(),
            std::ffi::CString::new(filename).unwrap().as_ptr(),
            0,
        );

        // Print result
        match result.get_tag() {
            JS_TAG_FLOAT64 => println!("{}", result.u.float64),
            JS_TAG_INT => println!("{}", result.u.int32),
            JS_TAG_BOOL => println!("{}", if result.u.int32 != 0 { "true" } else { "false" }),
            JS_TAG_NULL => println!("null"),
            JS_TAG_UNDEFINED => println!("undefined"),
            JS_TAG_STRING => {
                let s = get_js_string(&result);
                println!("{}", s);
            }
            _ => println!("[unknown]"),
        }

        JS_FreeContext(ctx);
        JS_FreeRuntime(rt);
    }
}
