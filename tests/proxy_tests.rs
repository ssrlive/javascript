use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor(unsafe)]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_proxy_basic() {
    // Test basic Proxy creation
    let script = r#"
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
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "84");
}

#[test]
fn test_proxy_revocable() {
    // Test Proxy.revocable
    let script = r#"
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
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "42");
}
