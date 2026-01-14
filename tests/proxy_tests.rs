use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
#[ignore = "Proxy not yet implemented"]
fn test_proxy_basic() {
    // Test basic Proxy creation
    let result = evaluate_script(
        r#"
        var target = { foo: 42 };
        var handler = {
            get: function(target, prop) {
                if (prop === "foo") {
                    return target[prop] * 2;
                }
                return target[prop];
            }
        };
        var proxy = new Proxy(target, handler);
        proxy.foo
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "84");
}

#[test]
#[ignore = "Proxy not yet implemented"]
fn test_proxy_revocable() {
    // Test Proxy.revocable
    let result = evaluate_script(
        r#"
        var target = { foo: 42 };
        var handler = {
            get: function(target, prop) {
                return target[prop];
            }
        };
        var revocable = Proxy.revocable(target, handler);
        var proxy = revocable.proxy;
        var result = proxy.foo;
        revocable.revoke();
        result
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "42");
}
