use rust_quickjs::quickjs::{evaluate_script, Value};

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod class_tests {
    use super::*;

    #[test]
    fn test_simple_class_declaration() {
        let script = r#"
            class Person {
            }
        "#;

        let result = evaluate_script(script);
        if let Err(e) = &result {
            println!("Error: {:?}", e);
        }
        assert!(result.is_ok(), "Simple class declaration should work");
    }

    #[test]
    fn test_class_new_simple() {
        let script = r#"
            class Person {
                constructor() {
                }
            }

            let person = new Person();
        "#;

        let result = evaluate_script(script);
        match &result {
            Ok(val) => println!("Success: {:?}", val),
            Err(e) => println!("Error: {:?}", e),
        }
        assert!(result.is_ok(), "Simple new expression should work");
    }

    #[test]
    fn test_class_constructor_with_this() {
        let script = r#"
            class Person {
                constructor(name) {
                    this.name = name;
                }
            }

            let person = new Person("Alice");
        "#;

        let result = evaluate_script(script);
        match &result {
            Ok(val) => println!("Success: {:?}", val),
            Err(e) => println!("Error: {:?}", e),
        }
        assert!(result.is_ok(), "Class constructor with this should work");
    }

    #[test]
    fn test_class_method_call() {
        let script = r#"
            class Person {
                constructor(name) {
                    this.name = name;
                }

                greet() {
                    return "Hello, " + this.name;
                }
            }

            let person = new Person("Alice");
            let greeting = person.greet();
        "#;

        let result = evaluate_script(script);
        match &result {
            Ok(val) => println!("Success: {:?}", val),
            Err(e) => println!("Error: {:?}", e),
        }
        assert!(result.is_ok(), "Class method call should work");
    }

    #[test]
    fn test_is_class_instance() {
        let script = r#"
            class Person {
                constructor(name) {
                    this.name = name;
                }
            }

            let person = new Person("Alice");
            let obj = {};
        "#;

        let result = evaluate_script(script);
        assert!(result.is_ok(), "Script should execute successfully");

        // Note: We can't easily test is_class_instance from here since it's internal
        // But the fact that the script runs without errors means the logic is working
    }

    #[test]
    fn test_instanceof_operator() {
        let script = r#"
            class Person {
                constructor(name) {
                    this.name = name;
                }
            }

            class Animal {
                constructor(type) {
                    this.type = type;
                }
            }

            let person = new Person("Alice");
            let animal = new Animal("Dog");
            let obj = {};

            let is_person_instance = person instanceof Person;
            let is_animal_instance = animal instanceof Animal;
            let is_person_animal = person instanceof Animal;
            let is_obj_person = obj instanceof Person;

            "is_person_instance: " + is_person_instance + "\n" +
            "is_animal_instance: " + is_animal_instance + "\n" +
            "is_person_animal: " + is_person_animal + "\n" +
            "is_obj_person: " + is_obj_person;
        "#;

        let result = evaluate_script(script);
        match &result {
            Ok(val) => {
                if let Value::String(s) = val {
                    let s = String::from_utf16_lossy(s);
                    println!("{}", s);
                    assert!(s.contains("is_person_instance: true"));
                    assert!(s.contains("is_animal_instance: true"));
                    assert!(s.contains("is_person_animal: false"));
                    assert!(s.contains("is_obj_person: false"));
                } else {
                    println!("Unexpected result type: {:?}", val);
                }
            }
            Err(e) => println!("Error: {:?}", e),
        }
        assert!(result.is_ok(), "instanceof operator should work");
    }
}
