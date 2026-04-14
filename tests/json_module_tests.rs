use javascript::*;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn json_module_static_default_import_works() {
    let test_dir = std::env::temp_dir().join(format!("json_module_static_{}", std::process::id()));
    std::fs::create_dir_all(&test_dir).unwrap();

    let json_path = test_dir.join("value.json");
    let main_path = test_dir.join("main.mjs");
    std::fs::write(&json_path, "{\"ok\":true,\"n\":262}").unwrap();
    std::fs::write(
        &main_path,
        "import value from './value.json' with { type: 'json' };\nvalue.ok === true && value.n === 262;\n",
    )
    .unwrap();

    let main_source = std::fs::read_to_string(&main_path).unwrap();
    let result = evaluate_script(&main_source, true, Some(&main_path)).unwrap();
    assert_eq!(result, "true");

    std::fs::remove_file(json_path).ok();
    std::fs::remove_file(main_path).ok();
    std::fs::remove_dir(test_dir).ok();
}

#[test]
fn json_module_dynamic_import_returns_namespace() {
    let test_dir = std::env::temp_dir().join(format!("json_module_dynamic_{}", std::process::id()));
    std::fs::create_dir_all(&test_dir).unwrap();

    let json_path = test_dir.join("value.json");
    let main_path = test_dir.join("main.mjs");
    std::fs::write(&json_path, "{\"ok\":true}").unwrap();
    std::fs::write(
        &main_path,
        "let ns = await import('./value.json', { with: { type: 'json' } });\nns.default && ns.default.ok === true;\n",
    )
    .unwrap();

    let main_source = std::fs::read_to_string(&main_path).unwrap();
    let result = evaluate_script(&main_source, true, Some(&main_path)).unwrap();
    assert_eq!(result, "true");

    std::fs::remove_file(json_path).ok();
    std::fs::remove_file(main_path).ok();
    std::fs::remove_dir(test_dir).ok();
}
