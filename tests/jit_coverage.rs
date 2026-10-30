#![cfg(feature = "jit")]

use std::collections::HashSet;

const OPCODE_SOURCE: &str = include_str!("../src/core/opcode.rs");
const FUNCTION_ID_SOURCE: &str = include_str!("../src/core/function_id.rs");
const DIRECT_NATIVE_OPCODES: &[&str] = &[
    "Return",
    "Constant",
    "Add",
    "Sub",
    "Mul",
    "Div",
    "Pop",
    "GetLocal",
    "GetGlobal",
    "GetUpvalue",
    "SetLocal",
    "SetUpvalue",
    "Negate",
    "Not",
    "LessThan",
    "GreaterThan",
    "LessEqual",
    "GreaterEqual",
    "Equal",
    "NotEqual",
    "StrictNotEqual",
    "Jump",
    "JumpIfFalse",
    "JumpIfTrue",
    "BoxLocal",
    "Dup",
    "ToNumeric",
    "ToNumber",
    "Swap",
    "Increment",
    "Decrement",
    "Throw",
    "SetupTry",
    "TeardownTry",
];
const INFRASTRUCTURE_OPCODES: &[&str] = &[
    "Mod",
    "Pow",
    "BitwiseAnd",
    "BitwiseOr",
    "BitwiseXor",
    "ShiftLeft",
    "ShiftRight",
    "UnsignedShiftRight",
    "BitwiseNot",
];
const AST_NATIVE_OPCODES: &[&str] = &[
    "Call",
    "NewArray",
    "NewObject",
    "GetProperty",
    "SetProperty",
    "GetIndex",
    "SetIndex",
];
const DIRECT_NATIVE_BUILTINS: &[&str] = &["BUILTIN_MATH_ABS"];

#[test]
fn report_opcode_and_builtin_jit_coverage_by_semantic_family() {
    let opcodes = parse_opcodes(OPCODE_SOURCE);
    let builtins = parse_builtins(FUNCTION_ID_SOURCE);

    assert!(!opcodes.is_empty(), "review the JIT coverage audit after changing the opcode set");
    assert!(!builtins.is_empty(), "review the JIT coverage audit after changing the builtin set");
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

    for opcode in DIRECT_NATIVE_OPCODES {
        assert!(opcodes.iter().any(|name| name == opcode), "stale native opcode entry: {opcode}");
    }
    for opcode in INFRASTRUCTURE_OPCODES {
        assert!(
            opcodes.iter().any(|name| name == opcode),
            "stale infrastructure opcode entry: {opcode}"
        );
    }
    for opcode in AST_NATIVE_OPCODES {
        assert!(opcodes.iter().any(|name| name == opcode), "stale AST-native opcode entry: {opcode}");
    }
    for builtin in DIRECT_NATIVE_BUILTINS {
        assert!(
            builtins.iter().any(|(name, _)| name == builtin),
            "stale native builtin entry: {builtin}"
        );
    }

    println!("JIT coverage audit by semantic family");
    for opcode in opcodes {
        let family = opcode_family(&opcode);
        let (status, detail) = opcode_status(&opcode);
        println!("opcode {opcode} [{family}]: {status} ({detail})");
    }
    for (name, _) in builtins {
        let (status, detail) = builtin_status(&name);
        println!("builtin {name} [{}]: {status} ({detail})", builtin_family(&name));
    }
}

fn builtin_status(name: &str) -> (&'static str, &'static str) {
    if DIRECT_NATIVE_BUILTINS.contains(&name) {
        (
            "native",
            "numeric Math.abs inlining guarded by the runtime Math.abs builtin identity",
        )
    } else {
        ("uncovered", "not dispatched from native JIT")
    }
}

fn opcode_status(name: &str) -> (&'static str, &'static str) {
    if name == "BoxLocal" {
        ("native", "elided for verified closure-free scalar locals")
    } else if name == "Throw" {
        (
            "native",
            "Number/Boolean/Undefined/Null throws propagate to VM handlers or supported local catch handlers",
        )
    } else if matches!(name, "SetupTry" | "TeardownTry") {
        (
            "native",
            "no-finally local handlers; catch identifiers require a consistent primitive type and verifiable next-local slot",
        )
    } else if name == "Return" {
        ("native", "numeric results plus opaque tagged identity values")
    } else if name == "GetLocal" {
        ("native", "numeric/boolean locals plus tagged identity passthrough")
    } else if name == "GetGlobal" {
        (
            "native",
            "catch-parameter initialization for consistent primitive values in a verified local slot",
        )
    } else if name == "GetUpvalue" {
        ("native", "numeric captures loaded through the traced closure-cell ABI")
    } else if name == "SetUpvalue" {
        ("native", "numeric captures written through the traced closure-cell ABI")
    } else if DIRECT_NATIVE_OPCODES.contains(&name) {
        ("native", "restricted numeric/boolean bytecode specialization")
    } else if AST_NATIVE_OPCODES.contains(&name) {
        match name {
            "Call" => (
                "native",
                "limited numeric inlining, generic identity ABI dispatch, and numeric-capture closure calls",
            ),
            _ => ("native", "AST-only virtual scalar array/object subset"),
        }
    } else if INFRASTRUCTURE_OPCODES.contains(&name) {
        match name {
            "Mod" => ("infrastructure", "calls the registered fmod host helper"),
            "Pow" => ("infrastructure", "calls the shared JS number-pow helper"),
            "BitwiseAnd" | "BitwiseOr" | "BitwiseXor" | "ShiftLeft" | "ShiftRight" | "UnsignedShiftRight" => {
                ("infrastructure", "calls the shared JS ToInt32/ToUint32 helper")
            }
            "BitwiseNot" => ("infrastructure", "calls the shared JS ToInt32 helper"),
            _ => unreachable!("all infrastructure opcodes have a detail"),
        }
    } else {
        ("uncovered", "no native translation or JIT-to-VM bridge")
    }
}

fn opcode_family(name: &str) -> &'static str {
    match name {
        "Constant" | "Pop" | "GetLocal" | "SetLocal" | "Negate" | "Not" | "TypeOf" | "Dup" | "Swap" | "GetThis" | "ToNumber"
        | "GetArguments" | "GetNewTarget" | "ToPropertyKey" | "ToNumeric" => "values-and-locals",
        "Add" | "Sub" | "Mul" | "Div" | "Mod" | "LessThan" | "GreaterThan" | "Equal" | "LessEqual" | "GreaterEqual" | "NotEqual"
        | "StrictNotEqual" | "Pow" | "BitwiseAnd" | "BitwiseOr" | "BitwiseXor" | "ShiftLeft" | "ShiftRight" | "UnsignedShiftRight"
        | "BitwiseNot" | "Increment" | "Decrement" => "operators",
        "Return" | "Jump" | "JumpIfFalse" | "JumpIfTrue" => "control-flow",
        "Call" | "NewCall" | "CollectRest" | "CallSpread" | "NewCallSpread" | "GetUpvalue" | "SetUpvalue" | "MakeClosure" | "BoxLocal"
        | "InitNamedFnSelf" | "ReboxLocal" | "ClearLocalCells" | "SetArguments" | "TailCall" | "TailCallSpread" => "functions-and-closures",
        "NewArray"
        | "NewObject"
        | "GetProperty"
        | "SetProperty"
        | "GetIndex"
        | "SetIndex"
        | "GetKeys"
        | "GetMethod"
        | "In"
        | "InstanceOf"
        | "DeleteProperty"
        | "DeleteIndex"
        | "SetSuperProperty"
        | "GetSuperProperty"
        | "ArrayPush"
        | "ArraySpread"
        | "ObjectSpread"
        | "ArrayHole"
        | "SetComputedGetter"
        | "SetComputedSetter"
        | "InitProperty"
        | "InitIndex"
        | "ObjectSpreadExcluding"
        | "GetSuperPropertyComputed"
        | "SetSuperPropertyComputed"
        | "DefineComputedMethod"
        | "FreezeTemplate"
        | "MarkPropertyNonEnumerable"
        | "MarkPropertyReadonly" => "objects-and-properties",
        "Throw"
        | "SetupTry"
        | "TeardownTry"
        | "NewError"
        | "ThrowTypeError"
        | "ThrowIfNullish"
        | "ThrowIfNotConstructor"
        | "AssertGlobalDefined" => "exceptions",
        "Yield"
        | "GeneratorParamInitDone"
        | "Await"
        | "YieldDirect"
        | "CheckGeneratorReturn"
        | "SetGeneratorReturn"
        | "ClearGeneratorReturn" => "generators-and-async",
        "ValidateClassHeritage"
        | "GetThisSuper"
        | "ClearThisTdz"
        | "ValidateProtoValue"
        | "EnterFieldInit"
        | "LeaveFieldInit"
        | "AllocBrand"
        | "ResetPrototype"
        | "SetComputedClassGetter"
        | "SetComputedClassSetter" => "classes-and-prototypes",
        "IteratorClose" | "AssertIterResult" | "IteratorCloseAbrupt" => "collections-and-iterators",
        "DefineGlobal" | "GetGlobal" | "SetGlobal" | "TypeOfGlobal" | "DeleteGlobal" | "DefineGlobalConst" | "DefineGlobalSoft" => {
            "globals-and-modules"
        }
        "DisposeResources" | "DisposeResourcesAsync" => "resource-management",
        _ => panic!("opcode {name} has no semantic-family classification"),
    }
}

fn builtin_family(name: &str) -> String {
    let Some(suffix) = name.strip_prefix("BUILTIN_") else {
        panic!("builtin name {name} does not use the BUILTIN_ prefix");
    };
    match suffix.split('_').next().unwrap_or_default() {
        "CTOR" => "constructors".to_string(),
        "" => panic!("builtin name {name} has no family"),
        family => family.to_ascii_lowercase(),
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
