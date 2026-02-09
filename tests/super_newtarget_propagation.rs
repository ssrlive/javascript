use javascript::*;

// Ensure logger initialization for tests
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_super_newtarget_propagation() {
    let src = r#"
        var baseNewTarget, parentNewTarget;

        class Base {
          constructor() {
            baseNewTarget = new.target;
          }
        }

        class Parent extends Base {
          constructor() {
            parentNewTarget = new.target;
            super();
          }
        }

        class Child extends Parent {
          constructor() {
            super();
          }
        }

        new Child();

        if (parentNewTarget !== Child) throw new Error('parentNewTarget not propagated');
        if (baseNewTarget !== Child) throw new Error('baseNewTarget not propagated');
    "#;

    let res = evaluate_script(src, None::<&std::path::Path>);
    assert!(res.is_ok(), "Expected script to run without errors");
}
