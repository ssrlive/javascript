#![cfg(feature = "jit")]

use std::collections::HashSet;

const OPCODE_SOURCE: &str = include_str!("../src/core/opcode.rs");
const FUNCTION_ID_SOURCE: &str = include_str!("../src/core/function_id.rs");

#[test]
fn report_opcode_and_builtin_jit_coverage() {
    let opcodes = parse_opcodes(OPCODE_SOURCE);
    let builtins = parse_builtins(FUNCTION_ID_SOURCE);

    assert_eq!(opcodes.len(), 122, "review the JIT coverage audit after changing the opcode set");
    assert_eq!(builtins.len(), 331, "review the JIT coverage audit after changing the builtin set");
    assert_eq!(
        opcodes.iter().collect::<HashSet<_>>().len(),
        opcodes.len(),
        "opcode names must be unique"
    );
    assert_eq!(
        builtins.iter().map(|(_, id)| *id).collect::<HashSet<_>>().len(),
        builtins.len(),
        "builtin IDs must be unique"
    );
    assert_eq!(
        builtins.iter().map(|(name, _)| name).collect::<HashSet<_>>().len(),
        builtins.len(),
        "builtin names must be unique"
    );

    println!("JIT coverage audit");
    println!("Current JIT compiles a restricted AST subset, not bytecode opcodes.");
    for opcode in opcodes {
        println!("opcode {opcode}: unsupported (no bytecode JIT translation)");
    }
    for (name, _) in builtins {
        println!("builtin {name}: unsupported (runtime implementation only)");
    }
}

fn parse_opcodes(source: &str) -> Vec<String> {
    let enum_body = source
        .split_once("pub enum Opcode {")
        .expect("Opcode enum must exist")
        .1
        .split_once("\n}")
        .expect("Opcode enum must be closed")
        .0;
    enum_body
        .lines()
        .filter_map(|line| {
            let variant = line.split("//").next()?.trim();
            let (name, _) = variant.split_once('=')?;
            Some(name.trim().to_string())
        })
        .collect()
}

fn parse_builtins(source: &str) -> Vec<(String, usize)> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let declaration = line.strip_prefix("pub(crate) const ")?;
            let (name, value) = declaration.split_once(": FunctionID = ")?;
            let value = value.strip_suffix(';')?.parse().ok()?;
            Some((name.to_string(), value))
        })
        .collect()
}
