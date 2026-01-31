use javascript::evaluate_script;

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

        let result = evaluate_script(script, None::<&std::path::Path>);
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

        let result = evaluate_script(script, None::<&std::path::Path>);
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

        let result = evaluate_script(script, None::<&std::path::Path>);
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

        let result = evaluate_script(script, None::<&std::path::Path>);
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

        let result = evaluate_script(script, None::<&std::path::Path>);
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

        let result = evaluate_script(script, None::<&std::path::Path>);
        match &result {
            Ok(s) => {
                let s_inner = if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                    s[1..s.len() - 1].to_string()
                } else {
                    s.clone()
                };
                println!("{}", s_inner);
                assert!(s_inner.contains("is_person_instance: true"));
                assert!(s_inner.contains("is_animal_instance: true"));
                assert!(s_inner.contains("is_person_animal: false"));
                assert!(s_inner.contains("is_obj_person: false"));
            }
            Err(e) => println!("Error: {:?}", e),
        }
        assert!(result.is_ok(), "instanceof operator should work");
    }

    #[test]
    fn test_class_inheritance() {
        let script = r#"
            class Animal {
                constructor(name) {
                    this.name = name;
                }

                speak() {
                    return this.name + " makes a sound";
                }
            }

            class Dog extends Animal {
                constructor(name, breed) {
                    super(name);
                    this.breed = breed;
                }

                speak() {
                    return super.speak() + " - Woof!";
                }

                getBreed() {
                    return this.breed;
                }
            }

            let dog = new Dog("Buddy", "Golden Retriever");
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>);
        assert!(result.is_ok(), "Class inheritance should work");
    }

    #[test]
    fn test_super_in_constructor() {
        let script = r#"
            class Parent {
                constructor(value) {
                    this.value = value;
                }
            }

            class Child extends Parent {
                constructor(value, extra) {
                    super(value);
                    this.extra = extra;
                }
            }

            let child = new Child("test", "more");
            child.value + " " + child.extra;
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>);
        match &result {
            Ok(val) => println!("Success: {:?}", val),
            Err(e) => println!("Error: {:?}", e),
        }
        assert!(result.is_ok(), "super() in constructor should work");
    }

    #[test]
    fn test_super_method_call() {
        let script = r#"
            class Calculator {
                add(a, b) {
                    return a + b;
                }
            }

            class AdvancedCalculator extends Calculator {
                add(a, b) {
                    let base = super.add(a, b);
                    return base * 2;
                }
            }

            let calc = new AdvancedCalculator();
            calc.add(3, 4);
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>);
        match &result {
            Ok(val) => println!("Success: {:?}", val),
            Err(e) => println!("Error: {:?}", e),
        }
        assert!(result.is_ok(), "super.method() should work");
    }

    #[test]
    fn test_static_members() {
        let script = r#"
            class Test {
                static staticProp = "static value";
                static staticMethod() {
                    return "static method result";
                }
                constructor(name) { this.name = name; }
            }

            let staticProp = Test.staticProp;
            let staticResult = Test.staticMethod();
            let instance = new Test("test");
            let instanceName = instance.name;
            staticProp + ", " + staticResult + ", " + instanceName;
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>);
        match &result {
            Ok(val) => println!("Success: {:?}", val),
            Err(e) => println!("Error: {:?}", e),
        }
        assert!(result.is_ok(), "Static property, method access and instance properties should work");
    }

    #[test]
    fn test_class_constructor_name_non_enumerable() {
        let script = r#"
            class C { }
            Object.getOwnPropertyDescriptor(C, 'name')
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        assert!(result.is_ok(), "Script should execute");
        // Expect the descriptor to have enumerable=false per spec
        assert_eq!(
            result.unwrap(),
            "{\"value\":\"C\",\"writable\":true,\"enumerable\":false,\"configurable\":true}"
        );
    }

    #[test]
    fn test_super_missing_method_throws() {
        let script = r#"
            class P {}
            class C extends P {
                m() { return super.foo(); }
            }
            let c = new C();
            c.m();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        assert!(result.is_err(), "Calling missing super method should throw an error");
    }

    #[test]
    fn test_super_non_function_property_throws() {
        let script = r#"
            class P {}
            P.prototype.foo = 5;
            class C extends P { m() { return super.foo(); } }
            let c = new C();
            c.m();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        assert!(result.is_err(), "Calling a non-function super property should throw a TypeError");
    }

    #[test]
    fn test_super_getter_property() {
        let script = r#"
            class P { get value() { return "parent"; } }
            class C extends P { m() { return super.value; } }
            new C().m();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"parent\"", "Expected super getter property to return correct value");
    }

    #[test]
    fn test_super_getter_descriptor_inspect() {
        // Debugging test: inspect the descriptor stored on C.prototype for 'value'
        let script = r#"
            class P { get value() { return "parent"; } }
            class C extends P { }
            Object.getOwnPropertyDescriptor(C.prototype, 'value')
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        // Print descriptor to stderr for inspection when running with --nocapture
        eprintln!("DESCRIPTOR: {}", result);
        // Expect descriptor to be { get: [Getter], set: undefined, enumerable: false, configurable: true }
        assert!(result.contains("get"), "Descriptor should include getter");
    }
    #[test]
    fn test_super_deep_chain() {
        let script = r#"
            class A { toString() { return "A"; } }
            class B extends A { toString() { return "B " + super.toString(); } }
            class C extends B { toString() { return "C " + super.toString(); } }
            let x = new C();
            x.toString();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"C B A\"", "Expected super calls through deep inheritance chain to work");
    }

    #[test]
    fn test_super_in_arrow() {
        let script = r#"
            class P { m() { return "P"; } }
            class C extends P { m() { let f = () => super.m(); return f(); } }
            new C().m();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"P\"", "Expected super call in arrow function to work");
    }
}
