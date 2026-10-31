#![cfg(feature = "jit")]

use std::collections::HashSet;

const OPCODE_SOURCE: &str = include_str!("../src/core/opcode.rs");
const FUNCTION_ID_SOURCE: &str = include_str!("../src/core/function_id.rs");
const BASELINE_SOURCE: &str = include_str!("../src/core/vm/runner.rs");
const UNCOVERED_BASELINE_OPCODES: &[&str] = &["DisposeResources", "DisposeResourcesAsync"];
const BASELINE_SEMANTIC_TESTS: &[(&str, &[&str])] = &[
    ("jit_integer_remainder_matches_number_semantics_at_conversion_boundaries", &["Mod"]),
    (
        "jit_tail_calls_enter_native_functions_without_bypassing_the_callers_return",
        &["Call", "TailCall", "Return"],
    ),
    (
        "jit_native_numeric_loops_keep_locals_in_registers_across_backedges",
        &[
            "GetLocal",
            "SetLocal",
            "Add",
            "Sub",
            "Mul",
            "Div",
            "Mod",
            "Jump",
            "JumpIfFalse",
            "ReboxLocal",
        ],
    ),
    (
        "jit_lazy_iteration_cells_preserve_shared_mutations_and_old_bindings",
        &["ReboxLocal", "GetLocal", "SetLocal", "MakeClosure"],
    ),
    (
        "jit_numeric_regions_keep_intermediate_values_out_of_the_vm_stack",
        &["Constant", "GetLocal", "SetLocal", "Add", "Sub", "Mul", "Div"],
    ),
    (
        "jit_numeric_regions_preserve_all_numeric_comparison_results",
        &[
            "LessThan",
            "GreaterThan",
            "LessEqual",
            "GreaterEqual",
            "Equal",
            "StrictEqual",
            "NotEqual",
        ],
    ),
    (
        "jit_baseline_classes_preserve_super_receivers_and_descriptors",
        &[
            "ValidateClassHeritage",
            "GetThisSuper",
            "ClearThisTdz",
            "ValidateProtoValue",
            "EnterFieldInit",
            "LeaveFieldInit",
            "ResetPrototype",
            "GetSuperProperty",
            "GetSuperPropertyComputed",
            "SetSuperProperty",
            "SetSuperPropertyComputed",
            "DefineComputedMethod",
            "SetComputedClassGetter",
            "SetComputedClassSetter",
        ],
    ),
    ("jit_baseline_classes_preserve_heritage_errors_and_private_brands", &["AllocBrand"]),
    (
        "jit_baseline_generator_resume_throw_and_return_preserve_identity",
        &["Yield", "GeneratorParamInitDone", "CheckGeneratorReturn", "SetGeneratorReturn"],
    ),
    (
        "jit_baseline_generator_delegation_closes_iterators",
        &["YieldDirect", "ClearGeneratorReturn"],
    ),
    ("jit_baseline_async_resumption_preserves_exceptions_and_microtask_order", &["Await"]),
];
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
    "StrictEqual",
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
    "TailCall",
    "NewArray",
    "NewObject",
    "GetProperty",
    "SetProperty",
    "GetIndex",
    "SetIndex",
];
const DIRECT_NATIVE_BUILTINS: &[&str] = &[
    "BUILTIN_MATH_ABS",
    "BUILTIN_MATH_CEIL",
    "BUILTIN_MATH_FLOOR",
    "BUILTIN_MATH_ROUND",
    "BUILTIN_MATH_SIGN",
    "BUILTIN_MATH_SQRT",
    "BUILTIN_MATH_TRUNC",
];
const INFRASTRUCTURE_MATH_BUILTINS: &[&str] = &[
    "BUILTIN_MATH_MAX",
    "BUILTIN_MATH_MIN",
    "BUILTIN_MATH_SIN",
    "BUILTIN_MATH_COS",
    "BUILTIN_MATH_TAN",
    "BUILTIN_MATH_ASIN",
    "BUILTIN_MATH_ACOS",
    "BUILTIN_MATH_ATAN",
    "BUILTIN_MATH_ATAN2",
    "BUILTIN_MATH_SINH",
    "BUILTIN_MATH_COSH",
    "BUILTIN_MATH_TANH",
    "BUILTIN_MATH_ASINH",
    "BUILTIN_MATH_ACOSH",
    "BUILTIN_MATH_ATANH",
    "BUILTIN_MATH_EXP",
    "BUILTIN_MATH_EXPM1",
    "BUILTIN_MATH_LOG",
    "BUILTIN_MATH_LOG10",
    "BUILTIN_MATH_LOG1P",
    "BUILTIN_MATH_LOG2",
    "BUILTIN_MATH_FROUND",
    "BUILTIN_MATH_F16ROUND",
    "BUILTIN_MATH_CBRT",
    "BUILTIN_MATH_HYPOT",
    "BUILTIN_MATH_POW",
    "BUILTIN_MATH_RANDOM",
    "BUILTIN_MATH_CLZ32",
    "BUILTIN_MATH_IMUL",
    "BUILTIN_MATH_SUMPRECISE",
];

#[test]
fn report_opcode_and_builtin_jit_coverage_by_semantic_family() {
    let opcodes = parse_opcodes(OPCODE_SOURCE);
    let builtins = parse_builtins(FUNCTION_ID_SOURCE);
    let baseline_opcodes = parse_baseline_opcodes(BASELINE_SOURCE);

    assert!(!opcodes.is_empty(), "review the JIT coverage audit after changing the opcode set");
    assert!(!builtins.is_empty(), "review the JIT coverage audit after changing the builtin set");
    assert_eq!(
        builtins.len(),
        331,
        "builtin inventory changed: explicitly review coverage and add semantic tests before updating this gate"
    );
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
    for builtin in INFRASTRUCTURE_MATH_BUILTINS {
        assert!(
            builtins.iter().any(|(name, _)| name == builtin),
            "stale infrastructure builtin entry: {builtin}"
        );
    }
    assert!(!baseline_opcodes.is_empty(), "baseline helper audit must not be empty");
    for opcode in &baseline_opcodes {
        assert!(opcodes.iter().any(|name| name == opcode), "stale baseline opcode entry: {opcode}");
    }
    let uncovered = opcodes
        .iter()
        .map(String::as_str)
        .filter(|name| !baseline_opcodes.contains(name))
        .collect::<HashSet<_>>();
    assert_eq!(
        uncovered,
        UNCOVERED_BASELINE_OPCODES.iter().copied().collect(),
        "new opcodes need a baseline semantic bridge or an explicit uncovered decision"
    );
    let semantic_test_source = include_str!("jit_tests.rs");
    for (test, names) in BASELINE_SEMANTIC_TESTS {
        assert!(
            semantic_test_source.contains(&format!("fn {test}(")),
            "missing semantic regression: {test}"
        );
        for opcode in *names {
            assert!(
                baseline_opcodes.contains(opcode),
                "semantic regression {test} refers to an uncovered opcode: {opcode}"
            );
        }
    }

    println!("JIT coverage audit by semantic family");
    println!("Baseline helpers reuse VM semantics; they are not specialized native implementations.");
    for opcode in opcodes {
        let family = opcode_family(&opcode);
        let (status, detail) = opcode_status(&opcode, &baseline_opcodes);
        let baseline = if baseline_opcodes.contains(opcode.as_str()) {
            "helper-backed"
        } else {
            "uncovered"
        };
        let semantic_tests = BASELINE_SEMANTIC_TESTS
            .iter()
            .filter(|(_, names)| names.contains(&opcode.as_str()))
            .map(|(test, _)| *test)
            .collect::<Vec<_>>();
        println!(
            "opcode {opcode} [{family}]: {status} ({detail}; baseline={baseline}; regressions={})",
            semantic_tests.join(",")
        );
    }
    for (name, _) in builtins {
        let (status, detail) = builtin_status(&name);
        println!("builtin {name} [{}]: {status} ({detail})", builtin_family(&name));
    }
}

fn builtin_status(name: &str) -> (&'static str, &'static str) {
    if DIRECT_NATIVE_BUILTINS.contains(&name) {
        ("native", "numeric Math builtin inlining guarded by the runtime builtin identity")
    } else if INFRASTRUCTURE_MATH_BUILTINS.contains(&name) {
        if name == "BUILTIN_MATH_SUMPRECISE" {
            (
                "infrastructure",
                "precise summation for verified virtual numeric arrays; general iterables use VM helpers",
            )
        } else {
            (
                "infrastructure",
                "numeric Math builtin dispatched through the JIT host helper with a runtime identity guard",
            )
        }
    } else {
        (
            "uncovered",
            "no specialized native dispatch; baseline Call may use the runtime builtin",
        )
    }
}

fn opcode_status(name: &str, baseline_opcodes: &HashSet<&str>) -> (&'static str, &'static str) {
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
    } else if name == "Mod" {
        (
            "native",
            "exact signed integer remainder with 32/64-bit guards and signed zero; other Number inputs call fmod",
        )
    } else if DIRECT_NATIVE_OPCODES.contains(&name) {
        ("native", "restricted numeric/boolean bytecode specialization")
    } else if AST_NATIVE_OPCODES.contains(&name) {
        match name {
            "Call" | "TailCall" => (
                "native",
                "limited numeric inlining and guarded numeric/identity/closure ABI dispatch, including tail-position calls",
            ),
            _ => ("native", "AST-only virtual scalar array/object subset"),
        }
    } else if INFRASTRUCTURE_OPCODES.contains(&name) {
        match name {
            "Pow" => ("infrastructure", "calls the shared JS number-pow helper"),
            "BitwiseAnd" | "BitwiseOr" | "BitwiseXor" | "ShiftLeft" | "ShiftRight" | "UnsignedShiftRight" => {
                ("infrastructure", "calls the shared JS ToInt32/ToUint32 helper")
            }
            "BitwiseNot" => ("infrastructure", "calls the shared JS ToInt32 helper"),
            _ => unreachable!("all infrastructure opcodes have a detail"),
        }
    } else if baseline_opcodes.contains(name) {
        ("infrastructure", "compiled baseline block invokes a VM semantic helper")
    } else {
        ("uncovered", "no native translation or JIT-to-VM bridge")
    }
}

fn parse_baseline_opcodes(source: &str) -> HashSet<&str> {
    let table = source
        .split_once("baseline_helpers! {")
        .expect("baseline helper table must exist")
        .1
        .split_once("\n}")
        .expect("baseline helper table must be closed")
        .0;
    let names = table
        .lines()
        .filter_map(|line| line.trim().split_once(" => ").map(|(name, _)| name))
        .collect::<Vec<_>>();
    let unique = names.iter().copied().collect::<HashSet<_>>();
    assert_eq!(unique.len(), names.len(), "baseline opcode entries must be unique");
    unique
}

fn opcode_family(name: &str) -> &'static str {
    match name {
        "Constant" | "Pop" | "GetLocal" | "SetLocal" | "Negate" | "Not" | "TypeOf" | "Dup" | "Swap" | "GetThis" | "ToNumber"
        | "GetArguments" | "GetNewTarget" | "ToPropertyKey" | "ToNumeric" => "values-and-locals",
        "Add" | "Sub" | "Mul" | "Div" | "Mod" | "LessThan" | "GreaterThan" | "Equal" | "LessEqual" | "GreaterEqual" | "NotEqual"
        | "StrictEqual" | "Pow" | "BitwiseAnd" | "BitwiseOr" | "BitwiseXor" | "ShiftLeft" | "ShiftRight" | "UnsignedShiftRight"
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
