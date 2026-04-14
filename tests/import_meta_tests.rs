use javascript::*;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn import_meta_returns_stable_module_object() {
    let module_path = std::env::temp_dir().join(format!("import_meta_same_{}.mjs", std::process::id()));
    let result = evaluate_script(
        "typeof import.meta === 'object' && import.meta !== null && import.meta === import.meta",
        true,
        Some(&module_path),
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn import_meta_is_distinct_per_module() {
    let test_dir = std::env::temp_dir().join(format!("import_meta_distinct_{}", std::process::id()));
    std::fs::create_dir_all(&test_dir).unwrap();

    let dep_path = test_dir.join("dep.mjs");
    let main_path = test_dir.join("main.mjs");

    std::fs::write(
        &dep_path,
        "export const meta = import.meta;\nexport function getMeta() { return import.meta; }\n",
    )
    .unwrap();
    std::fs::write(
        &main_path,
        "import { meta as importedMeta, getMeta } from './dep.mjs';\nimport.meta !== importedMeta && importedMeta === getMeta() && import.meta === import.meta;\n",
    )
    .unwrap();

    let main_source = std::fs::read_to_string(&main_path).unwrap();
    let result = evaluate_script(&main_source, true, Some(&main_path)).unwrap();
    assert_eq!(result, "true");

    std::fs::remove_file(dep_path).ok();
    std::fs::remove_file(main_path).ok();
    std::fs::remove_dir(test_dir).ok();
}

#[test]
fn direct_eval_cannot_access_import_meta() {
    let module_path = std::env::temp_dir().join(format!("import_meta_eval_{}.mjs", std::process::id()));
    let result = evaluate_script(
        "try { eval('import.meta'); false; } catch (e) { e && e.name === 'SyntaxError'; }",
        true,
        Some(&module_path),
    )
    .unwrap();
    assert_eq!(result, "true");
}
