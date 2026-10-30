use crate::core::function_id::*;
use crate::core::{BinaryOp, Chunk, DestructuringElement, Expr, Opcode, Statement, StatementKind};
use crate::host_api::JitDependency;
use crate::host_api::{JitValue as NativeJitValue, JitValueTag, NativeJitFrame, NativeJitFunction, NativeJitRegistration, NativeJitStatus};
use cranelift_codegen::ir::{
    AbiParam, BlockArg, FuncRef, Function as ClifFunction, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, UserFuncName,
    condcodes::{FloatCC, IntCC},
    types,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module, default_libcall_names};
use std::collections::{HashMap, HashSet, VecDeque};

mod bytecode;

pub(crate) struct JitEngine {
    module: JITModule,
    fmod_function: FuncId,
    pow_function: FuncId,
    bitwise_function: FuncId,
    bitwise_not_function: FuncId,
    math_builtin_function: FuncId,
    next_function_id: u32,
}

fn is_local_slot_initializer(
    instructions: &[bytecode::DecodedInstruction],
    instruction_index: usize,
    chunk: &Chunk<'_>,
    parameter_count: usize,
    local_count: usize,
    is_function_entry: bool,
) -> bool {
    let instruction = &instructions[instruction_index];
    if instruction.opcode != Opcode::Constant {
        return false;
    }
    let constant_index = u16::from_le_bytes([chunk.code[instruction.operand.start], chunk.code[instruction.operand.start + 1]]) as usize;
    if !matches!(
        chunk.constants.get(constant_index),
        Some(crate::core::Value::Undefined | crate::core::Value::Uninitialized)
    ) {
        return false;
    }
    let boxed_local_initializer = instructions
        .get(instruction_index + 1)
        .is_some_and(|next| next.opcode == Opcode::BoxLocal && chunk.code[next.operand.start] as usize >= parameter_count);
    if boxed_local_initializer {
        return true;
    }
    if !is_function_entry || local_count <= parameter_count {
        return false;
    }

    let mut next_local = parameter_count;
    let mut next_instruction = 0;
    while next_local < local_count {
        let Some(initializer) = instructions.get(next_instruction) else {
            break;
        };
        if initializer.opcode != Opcode::Constant {
            break;
        }
        let initializer_index =
            u16::from_le_bytes([chunk.code[initializer.operand.start], chunk.code[initializer.operand.start + 1]]) as usize;
        if !matches!(
            chunk.constants.get(initializer_index),
            Some(crate::core::Value::Undefined | crate::core::Value::Uninitialized)
        ) {
            break;
        }
        if next_instruction == instruction_index {
            return true;
        }
        next_instruction += 1;
        next_local += 1;
        if instructions
            .get(next_instruction)
            .is_some_and(|next| next.opcode == Opcode::BoxLocal && chunk.code[next.operand.start] as usize == next_local - 1)
        {
            next_instruction += 1;
        }
    }
    false
}

#[derive(Clone)]
struct FunctionCandidate<'a> {
    ip: usize,
    name: String,
    params: Vec<String>,
    body: &'a [Statement],
    dependencies: Vec<JitDependency>,
}

#[derive(Clone)]
struct InlineFunction {
    ip: usize,
    params: Vec<String>,
    expression: Expr,
}

enum JitValue {
    Number(cranelift_codegen::ir::Value),
    Boolean(cranelift_codegen::ir::Value),
    Array(Vec<Variable>),
    Object(HashMap<String, Variable>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BytecodeStackKind {
    Number,
    Boolean,
    Undefined,
    Null,
}

fn bytecode_stack_type(kind: BytecodeStackKind) -> cranelift_codegen::ir::Type {
    match kind {
        BytecodeStackKind::Number => types::F64,
        BytecodeStackKind::Boolean | BytecodeStackKind::Undefined | BytecodeStackKind::Null => types::I8,
    }
}

fn bytecode_jump_target(chunk: &Chunk<'_>, instruction: &bytecode::DecodedInstruction) -> Option<usize> {
    let bytes: [u8; 4] = chunk.code.get(instruction.operand.clone())?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes) as usize)
}

#[derive(Clone)]
enum JitLocal {
    Number(Variable),
    Array(Vec<Variable>),
    Object(HashMap<String, Variable>),
}

enum JitLValue {
    Variable(Variable),
    ArrayElement {
        elements: Vec<Variable>,
        index_matches: Vec<cranelift_codegen::ir::Value>,
    },
}

unsafe extern "C" fn js_fmod(left: f64, right: f64) -> f64 {
    left % right
}

unsafe extern "C" fn js_pow(base: f64, exponent: f64) -> f64 {
    crate::core::vm::number_pow(base, exponent)
}

unsafe extern "C" fn js_bitwise_binary(left: f64, right: f64, operation: i32) -> f64 {
    let left_i32 = crate::core::vm::to_int32(left);
    let right_i32 = crate::core::vm::to_int32(right);
    let shift = crate::core::vm::to_uint32(right) & 0x1f;
    match operation {
        0 => (left_i32 & right_i32) as f64,
        1 => (left_i32 | right_i32) as f64,
        2 => (left_i32 ^ right_i32) as f64,
        3 => (left_i32 << shift) as f64,
        4 => (left_i32 >> shift) as f64,
        5 => (crate::core::vm::to_uint32(left) >> shift) as f64,
        _ => f64::NAN,
    }
}

unsafe extern "C" fn js_bitwise_not(value: f64) -> f64 {
    (!crate::core::vm::to_int32(value)) as f64
}

unsafe extern "C" fn js_math_builtin(arguments: *const f64, length: usize, id: usize) -> f64 {
    let arguments = if length == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(arguments, length) }
    };
    let first = arguments.first().copied().unwrap_or(f64::NAN);
    match id {
        BUILTIN_MATH_FLOOR => first.floor(),
        BUILTIN_MATH_CEIL => first.ceil(),
        BUILTIN_MATH_ROUND => {
            if first.is_nan() || first.is_infinite() || first == 0.0 {
                first
            } else if first > 0.0 && first < 0.5 {
                0.0
            } else if (-0.5..0.0).contains(&first) {
                -0.0
            } else if first - first.trunc() == 0.0 {
                first
            } else {
                (first + 0.5).floor()
            }
        }
        BUILTIN_MATH_ABS => first.abs(),
        BUILTIN_MATH_SQRT => first.sqrt(),
        BUILTIN_MATH_SIN => first.sin(),
        BUILTIN_MATH_COS => first.cos(),
        BUILTIN_MATH_TAN => first.tan(),
        BUILTIN_MATH_ASIN => first.asin(),
        BUILTIN_MATH_ACOS => first.acos(),
        BUILTIN_MATH_ATAN => first.atan(),
        BUILTIN_MATH_ATAN2 => first.atan2(arguments.get(1).copied().unwrap_or(f64::NAN)),
        BUILTIN_MATH_SINH => first.sinh(),
        BUILTIN_MATH_COSH => first.cosh(),
        BUILTIN_MATH_TANH => first.tanh(),
        BUILTIN_MATH_ASINH => first.asinh(),
        BUILTIN_MATH_ACOSH => first.acosh(),
        BUILTIN_MATH_ATANH => first.atanh(),
        BUILTIN_MATH_EXP => first.exp(),
        BUILTIN_MATH_EXPM1 => first.exp_m1(),
        BUILTIN_MATH_LOG => first.ln(),
        BUILTIN_MATH_LOG10 => first.log10(),
        BUILTIN_MATH_LOG1P => first.ln_1p(),
        BUILTIN_MATH_LOG2 => first.log2(),
        BUILTIN_MATH_FROUND => (first as f32) as f64,
        BUILTIN_MATH_F16ROUND => crate::core::vm::f16round(first),
        BUILTIN_MATH_CBRT => first.cbrt(),
        BUILTIN_MATH_TRUNC => first.trunc(),
        BUILTIN_MATH_SIGN => {
            if first.is_nan() || first == 0.0 {
                first
            } else if first > 0.0 {
                1.0
            } else {
                -1.0
            }
        }
        BUILTIN_MATH_MAX => {
            let mut result = f64::NEG_INFINITY;
            for &value in arguments {
                if value.is_nan() {
                    return f64::NAN;
                }
                if value > result || (value == 0.0 && result == 0.0 && result.is_sign_negative() && !value.is_sign_negative()) {
                    result = value;
                }
            }
            result
        }
        BUILTIN_MATH_MIN => {
            let mut result = f64::INFINITY;
            for &value in arguments {
                if value.is_nan() {
                    return f64::NAN;
                }
                if value < result || (value == 0.0 && result == 0.0 && !result.is_sign_negative() && value.is_sign_negative()) {
                    result = value;
                }
            }
            result
        }
        BUILTIN_MATH_HYPOT => {
            if arguments.iter().any(|value| value.is_infinite()) {
                return f64::INFINITY;
            }
            if arguments.iter().any(|value| value.is_nan()) {
                return f64::NAN;
            }
            arguments.iter().map(|value| value * value).sum::<f64>().sqrt()
        }
        BUILTIN_MATH_POW => {
            let exponent = arguments.get(1).copied().unwrap_or(f64::NAN);
            if exponent == 0.0 {
                1.0
            } else if first.is_nan() || exponent.is_nan() || (first.abs() == 1.0 && exponent.is_infinite()) {
                f64::NAN
            } else {
                first.powf(exponent)
            }
        }
        BUILTIN_MATH_CLZ32 => arguments
            .first()
            .map(|value| crate::core::vm::to_uint32(*value).leading_zeros() as f64)
            .unwrap_or(32.0),
        BUILTIN_MATH_IMUL => {
            let left = arguments.first().map(|value| crate::core::vm::to_int32(*value)).unwrap_or(0);
            let right = arguments.get(1).map(|value| crate::core::vm::to_int32(*value)).unwrap_or(0);
            left.wrapping_mul(right) as f64
        }
        BUILTIN_MATH_SUMPRECISE => crate::core::vm::math_sum_precise_numbers(arguments.to_vec()),
        BUILTIN_MATH_RANDOM => {
            use std::time::SystemTime;
            let seed = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|duration| duration.as_nanos() as u64)
                .unwrap_or(0);
            let mixed = seed ^ (seed >> 17) ^ (seed << 13);
            let bits53 = mixed & ((1u64 << 53) - 1);
            (bits53 as f64) / ((1u64 << 53) as f64)
        }
        _ => f64::NAN,
    }
}

impl JitEngine {
    pub(crate) fn new() -> Result<Self, String> {
        if !cfg!(all(
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )) {
            return Err("JIT is currently supported only on 64-bit x86 and ARM hosts".to_string());
        }

        let mut flag_builder = settings::builder();
        flag_builder
            .set("use_colocated_libcalls", "false")
            .map_err(|error| error.to_string())?;
        flag_builder
            .set("is_pic", if cfg!(target_arch = "x86_64") { "true" } else { "false" })
            .map_err(|error| error.to_string())?;

        let isa_builder = cranelift_native::builder().map_err(|error| error.to_string())?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|error| error.to_string())?;
        let mut builder = JITBuilder::with_isa(isa, default_libcall_names());
        builder.symbol("javascript_jit_fmod", js_fmod as *const u8);
        builder.symbol("javascript_jit_pow", js_pow as *const u8);
        builder.symbol("javascript_jit_bitwise", js_bitwise_binary as *const u8);
        builder.symbol("javascript_jit_bitwise_not", js_bitwise_not as *const u8);
        builder.symbol("javascript_jit_math_builtin", js_math_builtin as *const u8);
        let mut module = JITModule::new(builder);
        let pointer_type = module.target_config().pointer_type();
        let mut numeric_binary_signature = module.make_signature();
        numeric_binary_signature.params.push(AbiParam::new(types::F64));
        numeric_binary_signature.params.push(AbiParam::new(types::F64));
        numeric_binary_signature.returns.push(AbiParam::new(types::F64));
        let fmod_function = module
            .declare_function("javascript_jit_fmod", Linkage::Import, &numeric_binary_signature)
            .map_err(|error| error.to_string())?;
        let pow_function = module
            .declare_function("javascript_jit_pow", Linkage::Import, &numeric_binary_signature)
            .map_err(|error| error.to_string())?;
        let mut bitwise_signature = module.make_signature();
        bitwise_signature.params.push(AbiParam::new(types::F64));
        bitwise_signature.params.push(AbiParam::new(types::F64));
        bitwise_signature.params.push(AbiParam::new(types::I32));
        bitwise_signature.returns.push(AbiParam::new(types::F64));
        let bitwise_function = module
            .declare_function("javascript_jit_bitwise", Linkage::Import, &bitwise_signature)
            .map_err(|error| error.to_string())?;
        let mut bitwise_not_signature = module.make_signature();
        bitwise_not_signature.params.push(AbiParam::new(types::F64));
        bitwise_not_signature.returns.push(AbiParam::new(types::F64));
        let bitwise_not_function = module
            .declare_function("javascript_jit_bitwise_not", Linkage::Import, &bitwise_not_signature)
            .map_err(|error| error.to_string())?;
        let mut math_builtin_signature = module.make_signature();
        math_builtin_signature.params.push(AbiParam::new(pointer_type));
        math_builtin_signature.params.push(AbiParam::new(pointer_type));
        math_builtin_signature.params.push(AbiParam::new(types::I32));
        math_builtin_signature.returns.push(AbiParam::new(types::F64));
        let math_builtin_function = module
            .declare_function("javascript_jit_math_builtin", Linkage::Import, &math_builtin_signature)
            .map_err(|error| error.to_string())?;

        Ok(Self {
            module,
            fmod_function,
            pow_function,
            bitwise_function,
            bitwise_not_function,
            math_builtin_function,
            next_function_id: 0,
        })
    }

    pub(crate) fn compile_functions(&mut self, statements: &[Statement], chunk: &Chunk<'_>) -> Vec<NativeJitRegistration> {
        let cfg = match bytecode::ControlFlowGraph::build(chunk) {
            Ok(cfg) => cfg,
            Err(error) => {
                log::warn!("JIT skipped script because bytecode CFG construction failed: {error}");
                return Vec::new();
            }
        };
        log::debug!(
            "JIT decoded {} bytecode instructions ({} operand bytes) into {} basic blocks and {} exception handlers",
            cfg.instructions.len(),
            cfg.instructions.iter().map(|instruction| instruction.operand.len()).sum::<usize>(),
            cfg.blocks.len(),
            cfg.exception_handlers.len()
        );
        let function_bodies = collect_function_bodies(statements);
        let mut candidates = Vec::new();
        let mut name_counts = HashMap::<&str, usize>::new();
        for name in chunk.fn_names.values() {
            *name_counts.entry(name).or_default() += 1;
        }
        let inline_functions = collect_inline_functions(&function_bodies, chunk, &name_counts);

        for (ip, name) in &chunk.fn_names {
            if name_counts.get(name.as_str()) != Some(&1) {
                continue;
            }
            let Some((params, body)) = function_bodies.get(name.as_str()) else {
                continue;
            };
            let Some(params) = simple_parameters(params) else {
                continue;
            };
            if params.len() > u8::MAX as usize {
                log::debug!("JIT skipped function {}: too many parameters", name);
                continue;
            }
            let mut dependency_names = HashSet::new();
            for statement in *body {
                collect_inline_dependencies_from_statement(statement, &inline_functions, &mut dependency_names);
            }
            let mut pending_dependencies = dependency_names.iter().cloned().collect::<Vec<_>>();
            let mut builtin_dependencies = HashSet::<(String, usize)>::new();
            for statement in *body {
                collect_builtin_dependencies_from_statement(statement, &mut builtin_dependencies);
            }
            while let Some(dependency_name) = pending_dependencies.pop() {
                let Some(inline) = inline_functions.get(&dependency_name) else {
                    continue;
                };
                collect_builtin_dependencies_from_expression(&inline.expression, &mut builtin_dependencies);
                let mut nested_dependencies = HashSet::new();
                collect_inline_dependencies_from_expression(&inline.expression, &inline_functions, &mut nested_dependencies);
                for nested_dependency in nested_dependencies {
                    if dependency_names.insert(nested_dependency.clone()) {
                        pending_dependencies.push(nested_dependency);
                    }
                }
            }
            let mut dependencies = dependency_names
                .into_iter()
                .filter_map(|dependency_name| {
                    inline_functions.get(&dependency_name).map(|inline| JitDependency::Function {
                        name: dependency_name,
                        ip: inline.ip,
                    })
                })
                .collect::<Vec<_>>();
            for (property, function_id) in builtin_dependencies {
                dependencies.push(JitDependency::BuiltinProperty {
                    object: "Math".to_string(),
                    property,
                    function_id,
                });
                if function_id == BUILTIN_MATH_SUMPRECISE {
                    dependencies.push(JitDependency::BuiltinHostFunctionProperty {
                        object: "Array.prototype".to_string(),
                        property: "@@sym:1".to_string(),
                        function_name: "array.values".to_string(),
                    });
                    dependencies.push(JitDependency::BuiltinHostFunctionProperty {
                        object: "__ArrayIteratorPrototype__".to_string(),
                        property: "next".to_string(),
                        function_name: "iterator.next".to_string(),
                    });
                }
            }
            candidates.push(FunctionCandidate {
                ip: *ip,
                name: name.clone(),
                params,
                body,
                dependencies,
            });
        }

        let mut compiled = Vec::new();
        for candidate in candidates {
            let generic_value = is_identity_function(&candidate.params, candidate.body);
            let compilation = if generic_value {
                self.compile_identity_function(&candidate).map(|function| (function, true))
            } else {
                let bytecode_result = self.compile_bytecode_function(&candidate, &cfg, chunk);
                match bytecode_result {
                    Ok(Some(function)) => Ok((function, true)),
                    Ok(None) if is_supported_function(&candidate.params, candidate.body, &inline_functions) => self
                        .compile_function(&candidate, &inline_functions)
                        .map(|(ip, arity, function)| ((ip, arity, 0, function), false)),
                    Ok(None) => {
                        log::debug!("JIT skipped function {}: unsupported bytecode semantics", candidate.name);
                        continue;
                    }
                    Err(error) => Err(error),
                }
            };
            match compilation {
                Ok((function, from_bytecode)) => {
                    let source = if from_bytecode { "bytecode" } else { "AST" };
                    log::debug!(
                        "JIT compiled function {} from {source} at bytecode offset {}",
                        candidate.name,
                        candidate.ip
                    );
                    compiled.push((function, candidate.dependencies, generic_value));
                }
                Err(error) => log::warn!("JIT skipped function {}: {error}", candidate.name),
            }
        }

        if let Err(error) = self.module.finalize_definitions() {
            log::warn!("JIT finalization failed; using the interpreter: {error}");
            return Vec::new();
        }

        compiled
            .into_iter()
            .map(|((ip, arity, upvalue_count, function_id), dependencies, generic_value)| {
                let address = self.module.get_finalized_function(function_id);
                let function = unsafe { std::mem::transmute::<*const u8, NativeJitFunction>(address) };
                NativeJitRegistration {
                    ip,
                    arity,
                    upvalue_count,
                    function,
                    generic_value,
                    dependencies,
                }
            })
            .collect()
    }

    fn compile_identity_function(&mut self, candidate: &FunctionCandidate<'_>) -> Result<(usize, usize, usize, FuncId), String> {
        let function_name = format!("js_{}_{}", self.next_function_id, candidate.name);
        self.next_function_id += 1;
        let pointer_type = self.module.target_config().pointer_type();
        let mut signature = self.module.make_signature();
        signature.params.push(AbiParam::new(pointer_type));
        signature.returns.push(AbiParam::new(types::I32));
        let function_id = self
            .module
            .declare_function(&function_name, Linkage::Local, &signature)
            .map_err(|error| error.to_string())?;
        let mut context = self.module.make_context();
        context.func = ClifFunction::with_name_signature(UserFuncName::user(0, function_id.as_u32()), signature);
        let mut builder_context = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
            let entry = builder.create_block();
            builder.switch_to_block(entry);
            builder.append_block_params_for_function_params(entry);
            let frame_pointer = builder.block_params(entry)[0];
            let arguments_pointer = builder.ins().load(
                pointer_type,
                MemFlagsData::new(),
                frame_pointer,
                std::mem::offset_of!(NativeJitFrame, arguments) as i32,
            );
            let result_offset = std::mem::offset_of!(NativeJitFrame, result) as i32;
            for (ty, value_offset) in [
                (types::I32, std::mem::offset_of!(NativeJitValue, tag)),
                (types::I32, std::mem::offset_of!(NativeJitValue, flags)),
                (types::I64, std::mem::offset_of!(NativeJitValue, payload)),
            ] {
                let value = builder.ins().load(ty, MemFlagsData::new(), arguments_pointer, value_offset as i32);
                builder
                    .ins()
                    .store(MemFlagsData::new(), value, frame_pointer, result_offset + value_offset as i32);
            }
            let returned = builder.ins().iconst(types::I32, NativeJitStatus::Returned as i64);
            builder.ins().return_(&[returned]);
            builder.seal_all_blocks();
            builder.finalize(self.module.target_config());
        }
        self.module
            .define_function(function_id, &mut context)
            .map_err(|error| error.to_string())?;
        self.module.clear_context(&mut context);
        Ok((candidate.ip, candidate.params.len(), 0, function_id))
    }

    fn compile_bytecode_function(
        &mut self,
        candidate: &FunctionCandidate<'_>,
        cfg: &bytecode::ControlFlowGraph,
        chunk: &Chunk<'_>,
    ) -> Result<Option<(usize, usize, usize, FuncId)>, String> {
        let Some(entry_index) = cfg.blocks.iter().position(|block| block.start == candidate.ip) else {
            return Ok(None);
        };

        let block_indices = cfg
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (block.start, index))
            .collect::<HashMap<_, _>>();
        let mut candidate_reachable_blocks = HashSet::new();
        let mut reachability_worklist = VecDeque::from([entry_index]);
        while let Some(block_index) = reachability_worklist.pop_front() {
            if !candidate_reachable_blocks.insert(block_index) {
                continue;
            }
            reachability_worklist.extend(
                cfg.blocks[block_index]
                    .successors
                    .iter()
                    .filter_map(|target| block_indices.get(target).copied()),
            );
            reachability_worklist.extend(
                cfg.blocks[block_index]
                    .exceptional_successors
                    .iter()
                    .filter_map(|target| block_indices.get(target).copied()),
            );
        }
        let candidate_reachable_offsets = candidate_reachable_blocks
            .iter()
            .flat_map(|index| {
                cfg.instructions[cfg.blocks[*index].instruction_range.clone()]
                    .iter()
                    .map(|instruction| instruction.offset)
            })
            .collect::<HashSet<_>>();
        let other_function_entries = chunk
            .fn_names
            .keys()
            .copied()
            .filter(|entry| *entry != candidate.ip)
            .collect::<HashSet<_>>();
        let mut incoming_stacks = vec![None::<Vec<BytecodeStackKind>>; cfg.blocks.len()];
        let function_end = chunk
            .fn_names
            .keys()
            .copied()
            .filter(|entry| *entry > candidate.ip)
            .min()
            .unwrap_or(chunk.code.len());
        let mut catch_binding_locals = HashMap::<usize, (usize, usize)>::new();
        for instruction in cfg
            .instructions
            .iter()
            .filter(|instruction| candidate_reachable_offsets.contains(&instruction.offset) && instruction.opcode == Opcode::SetupTry)
        {
            let binding = u16::from_le_bytes([chunk.code[instruction.operand.start + 4], chunk.code[instruction.operand.start + 5]]);
            let flags = chunk.code[instruction.operand.start + 6];
            if binding == u16::MAX || flags & 1 != 0 {
                continue;
            }
            let Some(crate::core::Value::String(binding_name)) = chunk.constants.get(binding as usize) else {
                return Ok(None);
            };
            let binding_name = crate::unicode::utf16_to_utf8(binding_name);
            let Some(target_bytes) = chunk.code.get(instruction.operand.start..instruction.operand.start + 4) else {
                return Ok(None);
            };
            let target = u32::from_le_bytes(target_bytes.try_into().expect("four-byte handler target")) as usize;
            let Some(target_index) = cfg.instructions.iter().position(|instruction| instruction.offset == target) else {
                return Ok(None);
            };
            let get_global = &cfg.instructions[target_index];
            if get_global.opcode != Opcode::GetGlobal {
                return Ok(None);
            }
            let global_name_index =
                u16::from_le_bytes([chunk.code[get_global.operand.start], chunk.code[get_global.operand.start + 1]]) as usize;
            if !matches!(
                chunk.constants.get(global_name_index),
                Some(crate::core::Value::String(name)) if crate::unicode::utf16_to_utf8(name) == binding_name
            ) {
                return Ok(None);
            }
            let Some(get_local) = cfg
                .instructions
                .get(target_index + 1)
                .filter(|next| next.offset == get_global.next_offset)
            else {
                return Ok(None);
            };
            if get_local.opcode != Opcode::GetLocal {
                return Ok(None);
            }
            let local_index = chunk.code[get_local.operand.start] as usize;
            let catch_slot = chunk
                .fn_local_names
                .get(&candidate.ip)
                .map_or(candidate.params.len(), Vec::len)
                .max(candidate.params.len());
            if local_index != catch_slot
                || cfg.instructions.iter().any(|prior| {
                    candidate.ip <= prior.offset
                        && prior.offset < instruction.offset
                        && matches!(prior.opcode, Opcode::SetLocal | Opcode::BoxLocal)
                        && chunk.code[prior.operand.start] as usize == local_index
                })
            {
                return Ok(None);
            }
            catch_binding_locals.insert(get_global.offset, (target, local_index));
        }
        let bytecode_local_count = cfg
            .instructions
            .iter()
            .filter(|instruction| candidate.ip <= instruction.offset && instruction.offset < function_end)
            .filter(|instruction| matches!(instruction.opcode, Opcode::GetLocal | Opcode::SetLocal | Opcode::BoxLocal))
            .map(|instruction| chunk.code[instruction.operand.start] as usize + 1)
            .max()
            .unwrap_or(candidate.params.len());
        let local_count = chunk
            .fn_local_names
            .get(&candidate.ip)
            .map_or(candidate.params.len(), Vec::len)
            .max(bytecode_local_count);
        if local_count < candidate.params.len() || local_count > u8::MAX as usize + 1 {
            return Ok(None);
        }
        let mut initial_locals = vec![None; local_count];
        initial_locals[..candidate.params.len()].fill(Some(BytecodeStackKind::Number));
        let mut local_types = initial_locals.clone();
        let mut local_type_conflicts = vec![false; local_count];
        let mut local_reads = vec![false; local_count];
        let mut catch_binding_kinds = HashMap::<usize, BytecodeStackKind>::new();
        let mut upvalue_count = 0;
        let mut incoming_locals = vec![None::<Vec<Option<BytecodeStackKind>>>; cfg.blocks.len()];
        let mut worklist = VecDeque::from([entry_index]);
        let mut reachable_blocks = Vec::new();
        let mut has_return = false;
        incoming_stacks[entry_index] = Some(Vec::new());
        incoming_locals[entry_index] = Some(initial_locals);

        while let Some(block_index) = worklist.pop_front() {
            let block = &cfg.blocks[block_index];
            reachable_blocks.push(block_index);
            let mut stack = incoming_stacks[block_index]
                .clone()
                .expect("reachable blocks have an incoming stack");
            let mut locals = incoming_locals[block_index].clone().expect("reachable blocks have incoming locals");
            let instructions = &cfg.instructions[block.instruction_range.clone()];
            for (instruction_index, instruction) in instructions.iter().enumerate() {
                match instruction.opcode {
                    Opcode::Constant => {
                        let constant_index =
                            u16::from_le_bytes([chunk.code[instruction.operand.start], chunk.code[instruction.operand.start + 1]]) as usize;
                        if is_local_slot_initializer(
                            instructions,
                            instruction_index,
                            chunk,
                            candidate.params.len(),
                            local_count,
                            block_index == entry_index,
                        ) {
                            continue;
                        }
                        match chunk.constants.get(constant_index) {
                            Some(crate::core::Value::Number(_)) => stack.push(BytecodeStackKind::Number),
                            Some(crate::core::Value::Boolean(_)) => stack.push(BytecodeStackKind::Boolean),
                            Some(crate::core::Value::Undefined) => {
                                stack.push(BytecodeStackKind::Undefined);
                            }
                            Some(crate::core::Value::Null) => stack.push(BytecodeStackKind::Null),
                            Some(crate::core::Value::Uninitialized) => return Ok(None),
                            _ => return Ok(None),
                        }
                    }
                    Opcode::GetLocal => {
                        let local_index = chunk.code[instruction.operand.start] as usize;
                        if let Some(read) = local_reads.get_mut(local_index) {
                            *read = true;
                        } else {
                            return Ok(None);
                        }
                        let Some(Some(
                            local_kind @ (BytecodeStackKind::Number
                            | BytecodeStackKind::Boolean
                            | BytecodeStackKind::Undefined
                            | BytecodeStackKind::Null),
                        )) = locals.get(local_index)
                        else {
                            return Ok(None);
                        };
                        stack.push(*local_kind);
                    }
                    Opcode::GetGlobal => {
                        let Some((target, local_index)) = catch_binding_locals.get(&instruction.offset).copied() else {
                            return Ok(None);
                        };
                        let Some(value_kind) = catch_binding_kinds.get(&target).copied() else {
                            return Ok(None);
                        };
                        if local_index >= locals.len() {
                            return Ok(None);
                        }
                        if local_type_conflicts[local_index] {
                            local_types[local_index] = None;
                        } else if local_types[local_index].is_some_and(|kind| kind != value_kind) {
                            local_type_conflicts[local_index] = true;
                            local_types[local_index] = None;
                        } else {
                            local_types[local_index] = Some(value_kind);
                        }
                        locals[local_index] = Some(value_kind);
                    }
                    Opcode::GetUpvalue => {
                        let upvalue_index = chunk.code[instruction.operand.start] as usize;
                        upvalue_count = upvalue_count.max(upvalue_index + 1);
                        stack.push(BytecodeStackKind::Number);
                    }
                    Opcode::SetUpvalue => {
                        let upvalue_index = chunk.code[instruction.operand.start] as usize;
                        if !matches!(stack.last(), Some(BytecodeStackKind::Number)) {
                            return Ok(None);
                        }
                        upvalue_count = upvalue_count.max(upvalue_index + 1);
                    }
                    Opcode::SetLocal => {
                        let local_index = chunk.code[instruction.operand.start] as usize;
                        let Some(value_kind) = stack.last().copied() else {
                            return Ok(None);
                        };
                        if local_index >= locals.len() {
                            return Ok(None);
                        }
                        if local_type_conflicts[local_index] {
                            local_types[local_index] = None;
                        } else if local_types[local_index].is_some_and(|kind| kind != value_kind) {
                            local_type_conflicts[local_index] = true;
                            local_types[local_index] = None;
                        } else {
                            local_types[local_index] = Some(value_kind);
                        }
                        locals[local_index] = Some(value_kind);
                    }
                    Opcode::BoxLocal => {
                        let local_index = chunk.code[instruction.operand.start] as usize;
                        if local_index >= locals.len() {
                            return Ok(None);
                        }
                    }
                    Opcode::Dup => {
                        let Some(value) = stack.last().copied() else {
                            return Ok(None);
                        };
                        stack.push(value);
                    }
                    Opcode::ToNumeric => {
                        if stack.last() != Some(&BytecodeStackKind::Number) {
                            return Ok(None);
                        }
                    }
                    Opcode::ToNumber => {
                        if stack.last() != Some(&BytecodeStackKind::Number) {
                            return Ok(None);
                        }
                    }
                    Opcode::Swap => {
                        if stack.len() < 2 {
                            return Ok(None);
                        }
                        let top = stack.len() - 1;
                        stack.swap(top, top - 1);
                    }
                    Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Mod | Opcode::Pow => {
                        if stack.pop() != Some(BytecodeStackKind::Number) || stack.pop() != Some(BytecodeStackKind::Number) {
                            return Ok(None);
                        }
                        stack.push(BytecodeStackKind::Number);
                    }
                    Opcode::BitwiseAnd
                    | Opcode::BitwiseOr
                    | Opcode::BitwiseXor
                    | Opcode::ShiftLeft
                    | Opcode::ShiftRight
                    | Opcode::UnsignedShiftRight => {
                        if stack.pop() != Some(BytecodeStackKind::Number) || stack.pop() != Some(BytecodeStackKind::Number) {
                            return Ok(None);
                        }
                        stack.push(BytecodeStackKind::Number);
                    }
                    Opcode::LessThan
                    | Opcode::GreaterThan
                    | Opcode::LessEqual
                    | Opcode::GreaterEqual
                    | Opcode::Equal
                    | Opcode::NotEqual
                    | Opcode::StrictEqual => {
                        if stack.pop() != Some(BytecodeStackKind::Number) || stack.pop() != Some(BytecodeStackKind::Number) {
                            return Ok(None);
                        }
                        stack.push(BytecodeStackKind::Boolean);
                    }
                    Opcode::Negate | Opcode::Increment | Opcode::Decrement => {
                        if stack.last() != Some(&BytecodeStackKind::Number) {
                            return Ok(None);
                        }
                    }
                    Opcode::BitwiseNot => {
                        if stack.pop() != Some(BytecodeStackKind::Number) {
                            return Ok(None);
                        }
                        stack.push(BytecodeStackKind::Number);
                    }
                    Opcode::Not => match stack.pop() {
                        Some(
                            BytecodeStackKind::Number | BytecodeStackKind::Boolean | BytecodeStackKind::Undefined | BytecodeStackKind::Null,
                        ) => {
                            stack.push(BytecodeStackKind::Boolean);
                        }
                        _ => {
                            return Ok(None);
                        }
                    },
                    Opcode::Pop => {
                        if stack.pop().is_none() {
                            return Ok(None);
                        }
                    }
                    Opcode::JumpIfFalse | Opcode::JumpIfTrue => {
                        if !matches!(
                            stack.pop(),
                            Some(
                                BytecodeStackKind::Number
                                    | BytecodeStackKind::Boolean
                                    | BytecodeStackKind::Undefined
                                    | BytecodeStackKind::Null
                            )
                        ) {
                            return Ok(None);
                        }
                    }
                    Opcode::Jump => {}
                    Opcode::Return => {
                        if stack.len() != 1 || stack.pop() != Some(BytecodeStackKind::Number) {
                            return Ok(None);
                        }
                        has_return = true;
                    }
                    Opcode::Throw => {
                        let Some(
                            value_kind @ (BytecodeStackKind::Number
                            | BytecodeStackKind::Boolean
                            | BytecodeStackKind::Undefined
                            | BytecodeStackKind::Null),
                        ) = stack.pop()
                        else {
                            return Ok(None);
                        };
                        if !instruction.exceptional_targets.is_empty() && (instruction.exceptional_targets.len() != 1 || !stack.is_empty())
                        {
                            return Ok(None);
                        }
                        if let Some(&target) = instruction.exceptional_targets.first()
                            && catch_binding_locals.values().any(|(binding_target, _)| *binding_target == target)
                            && catch_binding_kinds
                                .insert(target, value_kind)
                                .is_some_and(|existing| existing != value_kind)
                        {
                            return Ok(None);
                        }
                    }
                    Opcode::SetupTry => {
                        let binding =
                            u16::from_le_bytes([chunk.code[instruction.operand.start + 4], chunk.code[instruction.operand.start + 5]]);
                        let flags = chunk.code[instruction.operand.start + 6];
                        if binding != u16::MAX && !matches!(chunk.constants.get(binding as usize), Some(crate::core::Value::String(_)))
                            || flags & 1 != 0
                        {
                            return Ok(None);
                        }
                    }
                    Opcode::TeardownTry => {}
                    _ => return Ok(None),
                }
            }

            let Some(last) = instructions.last() else {
                return Ok(None);
            };
            if last.opcode == Opcode::Return {
                if !block.successors.is_empty() {
                    return Ok(None);
                }
                continue;
            }
            if last.opcode == Opcode::Throw {
                if !block.successors.is_empty() {
                    return Ok(None);
                }
                if let Some(&target) = last.exceptional_targets.first() {
                    if other_function_entries.contains(&target) {
                        return Ok(None);
                    }
                    let Some(&target_index) = block_indices.get(&target) else {
                        return Ok(None);
                    };
                    let stack_is_new = incoming_stacks[target_index].is_none();
                    if incoming_stacks[target_index].as_ref().is_some_and(|existing| existing != &stack) {
                        return Ok(None);
                    }
                    let locals_are_new = incoming_locals[target_index].is_none();
                    if incoming_locals[target_index].as_ref().is_some_and(|existing| existing != &locals) {
                        return Ok(None);
                    }
                    if stack_is_new {
                        incoming_stacks[target_index] = Some(stack);
                    }
                    if locals_are_new {
                        incoming_locals[target_index] = Some(locals);
                    }
                    if stack_is_new || locals_are_new {
                        worklist.push_back(target_index);
                    }
                }
                continue;
            }
            if block.successors.is_empty() {
                return Ok(None);
            }
            if matches!(last.opcode, Opcode::JumpIfFalse | Opcode::JumpIfTrue) {
                let Some(target) = bytecode_jump_target(chunk, last) else {
                    return Ok(None);
                };
                if !block.successors.contains(&target) || !block.successors.contains(&last.next_offset) {
                    return Ok(None);
                }
            } else if block.successors.len() != 1 {
                return Ok(None);
            }

            for target in &block.successors {
                if other_function_entries.contains(target) {
                    return Ok(None);
                }
                let Some(&target_index) = block_indices.get(target) else {
                    return Ok(None);
                };
                let stack_is_new = incoming_stacks[target_index].is_none();
                if incoming_stacks[target_index].as_ref().is_some_and(|existing| existing != &stack) {
                    return Ok(None);
                }
                let locals_are_new = incoming_locals[target_index].is_none();
                if incoming_locals[target_index].as_ref().is_some_and(|existing| existing != &locals) {
                    return Ok(None);
                }
                if stack_is_new {
                    incoming_stacks[target_index] = Some(stack.clone());
                }
                if locals_are_new {
                    incoming_locals[target_index] = Some(locals.clone());
                }
                if stack_is_new || locals_are_new {
                    worklist.push_back(target_index);
                }
            }
        }

        if !has_return {
            return Ok(None);
        }
        if local_type_conflicts
            .iter()
            .zip(local_reads.iter())
            .any(|(conflict, read)| *conflict && *read)
        {
            return Ok(None);
        }

        let function_name = format!("js_{}_{}", self.next_function_id, candidate.name);
        self.next_function_id += 1;
        let pointer_type = self.module.target_config().pointer_type();
        let mut signature = self.module.make_signature();
        signature.params.push(AbiParam::new(pointer_type));
        signature.returns.push(AbiParam::new(types::I32));
        let function_id = self
            .module
            .declare_function(&function_name, Linkage::Local, &signature)
            .map_err(|error| error.to_string())?;
        let mut context = self.module.make_context();
        context.func = ClifFunction::with_name_signature(UserFuncName::user(0, function_id.as_u32()), signature);
        let fmod_function = self.module.declare_func_in_func(self.fmod_function, &mut context.func);
        let pow_function = self.module.declare_func_in_func(self.pow_function, &mut context.func);
        let bitwise_function = self.module.declare_func_in_func(self.bitwise_function, &mut context.func);
        let bitwise_not_function = self.module.declare_func_in_func(self.bitwise_not_function, &mut context.func);
        let mut builder_context = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
            let native_blocks = reachable_blocks
                .iter()
                .map(|index| (cfg.blocks[*index].start, builder.create_block()))
                .collect::<HashMap<_, _>>();
            let entry = native_blocks[&candidate.ip];
            builder.switch_to_block(entry);
            builder.append_block_params_for_function_params(entry);
            for kind in incoming_stacks[entry_index].as_ref().expect("entry stack was validated") {
                builder.append_block_param(entry, bytecode_stack_type(*kind));
            }
            for block_index in reachable_blocks.iter().copied().filter(|index| *index != entry_index) {
                let block = native_blocks[&cfg.blocks[block_index].start];
                for kind in incoming_stacks[block_index].as_ref().expect("reachable stack was validated") {
                    builder.append_block_param(block, bytecode_stack_type(*kind));
                }
            }

            let frame_pointer = builder.block_params(entry)[0];
            let arguments_pointer = builder.ins().load(
                pointer_type,
                MemFlagsData::new(),
                frame_pointer,
                std::mem::offset_of!(NativeJitFrame, arguments) as i32,
            );
            let mut locals = Vec::with_capacity(local_count);
            for (index, local_kind) in local_types.iter().copied().enumerate() {
                let local_type = local_kind.map_or(types::F64, bytecode_stack_type);
                let local = builder.declare_var(local_type);
                if index < candidate.params.len() {
                    let argument = builder.ins().load(
                        types::F64,
                        MemFlagsData::new(),
                        arguments_pointer,
                        (index * std::mem::size_of::<NativeJitValue>() + std::mem::offset_of!(NativeJitValue, payload)) as i32,
                    );
                    builder.def_var(local, argument);
                }
                locals.push(local);
            }

            for (position, block_index) in reachable_blocks.iter().copied().enumerate() {
                let bytecode_block = &cfg.blocks[block_index];
                let native_block = native_blocks[&bytecode_block.start];
                if position != 0 {
                    builder.switch_to_block(native_block);
                }
                let parameter_offset = usize::from(block_index == entry_index);
                let mut stack = builder.block_params(native_block)[parameter_offset..].to_vec();
                let mut stack_kinds = incoming_stacks[block_index].clone().expect("reachable stack was validated");
                let instructions = &cfg.instructions[bytecode_block.instruction_range.clone()];
                for (instruction_index, instruction) in instructions.iter().enumerate() {
                    match instruction.opcode {
                        Opcode::Constant => {
                            let constant_index =
                                u16::from_le_bytes([chunk.code[instruction.operand.start], chunk.code[instruction.operand.start + 1]])
                                    as usize;
                            if is_local_slot_initializer(
                                instructions,
                                instruction_index,
                                chunk,
                                candidate.params.len(),
                                local_count,
                                block_index == entry_index,
                            ) {
                                continue;
                            }
                            match chunk.constants[constant_index] {
                                crate::core::Value::Number(number) => {
                                    stack.push(builder.ins().f64const(number));
                                    stack_kinds.push(BytecodeStackKind::Number);
                                }
                                crate::core::Value::Boolean(value) => {
                                    stack.push(builder.ins().iconst(types::I8, i64::from(value)));
                                    stack_kinds.push(BytecodeStackKind::Boolean);
                                }
                                crate::core::Value::Undefined | crate::core::Value::Uninitialized => {
                                    stack.push(builder.ins().iconst(types::I8, 0));
                                    stack_kinds.push(BytecodeStackKind::Undefined);
                                }
                                crate::core::Value::Null => {
                                    stack.push(builder.ins().iconst(types::I8, 0));
                                    stack_kinds.push(BytecodeStackKind::Null);
                                }
                                _ => unreachable!("bytecode constant types were validated"),
                            }
                        }
                        Opcode::GetLocal => {
                            let local_index = chunk.code[instruction.operand.start] as usize;
                            stack.push(builder.use_var(locals[local_index]));
                            stack_kinds.push(local_types[local_index].expect("bytecode local kinds were validated"));
                        }
                        Opcode::GetGlobal => {
                            let (target, local_index) = catch_binding_locals[&instruction.offset];
                            let value_kind = catch_binding_kinds[&target];
                            let exception_offset = std::mem::offset_of!(NativeJitFrame, exception) as i32;
                            let payload_offset = exception_offset + std::mem::offset_of!(NativeJitValue, payload) as i32;
                            let value =
                                builder
                                    .ins()
                                    .load(bytecode_stack_type(value_kind), MemFlagsData::new(), frame_pointer, payload_offset);
                            builder.def_var(locals[local_index], value);
                        }
                        Opcode::GetUpvalue => {
                            let upvalue_index = chunk.code[instruction.operand.start] as usize;
                            let argument_index = candidate.params.len() + upvalue_index;
                            let value = builder.ins().load(
                                types::F64,
                                MemFlagsData::new(),
                                arguments_pointer,
                                (argument_index * std::mem::size_of::<NativeJitValue>() + std::mem::offset_of!(NativeJitValue, payload))
                                    as i32,
                            );
                            stack.push(value);
                            stack_kinds.push(BytecodeStackKind::Number);
                        }
                        Opcode::SetUpvalue => {
                            let upvalue_index = chunk.code[instruction.operand.start] as usize;
                            let argument_index = candidate.params.len() + upvalue_index;
                            let value = *stack.last().expect("bytecode stack was validated");
                            builder.ins().store(
                                MemFlagsData::new(),
                                value,
                                arguments_pointer,
                                (argument_index * std::mem::size_of::<NativeJitValue>() + std::mem::offset_of!(NativeJitValue, payload))
                                    as i32,
                            );
                        }
                        Opcode::SetLocal => {
                            let local_index = chunk.code[instruction.operand.start] as usize;
                            if !local_type_conflicts[local_index] {
                                builder.def_var(locals[local_index], *stack.last().expect("bytecode stack was validated"));
                            }
                        }
                        Opcode::BoxLocal => {
                            // Closure bytecodes are excluded, so this scalar local needs no heap cell.
                        }
                        Opcode::Dup => {
                            let value = *stack.last().expect("bytecode stack was validated");
                            let kind = *stack_kinds.last().expect("bytecode stack kinds were validated");
                            stack.push(value);
                            stack_kinds.push(kind);
                        }
                        Opcode::ToNumeric => {}
                        Opcode::ToNumber => {}
                        Opcode::Swap => {
                            let top = stack.len() - 1;
                            stack.swap(top, top - 1);
                            stack_kinds.swap(top, top - 1);
                        }
                        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Mod | Opcode::Pow => {
                            let right = stack.pop().expect("bytecode stack was validated");
                            let left = stack.pop().expect("bytecode stack was validated");
                            let _ = stack_kinds.pop().expect("bytecode stack kinds were validated");
                            let _ = stack_kinds.pop().expect("bytecode stack kinds were validated");
                            let result = match instruction.opcode {
                                Opcode::Add => builder.ins().fadd(left, right),
                                Opcode::Sub => builder.ins().fsub(left, right),
                                Opcode::Mul => builder.ins().fmul(left, right),
                                Opcode::Div => builder.ins().fdiv(left, right),
                                Opcode::Mod => {
                                    let call = builder.ins().call(fmod_function, &[left, right]);
                                    builder.inst_results(call)[0]
                                }
                                Opcode::Pow => {
                                    let call = builder.ins().call(pow_function, &[left, right]);
                                    builder.inst_results(call)[0]
                                }
                                _ => unreachable!("matched numeric binary opcode"),
                            };
                            stack.push(result);
                            stack_kinds.push(BytecodeStackKind::Number);
                        }
                        Opcode::BitwiseAnd
                        | Opcode::BitwiseOr
                        | Opcode::BitwiseXor
                        | Opcode::ShiftLeft
                        | Opcode::ShiftRight
                        | Opcode::UnsignedShiftRight => {
                            let right = stack.pop().expect("bytecode stack was validated");
                            let left = stack.pop().expect("bytecode stack was validated");
                            let _ = stack_kinds.pop().expect("bytecode stack kinds were validated");
                            let _ = stack_kinds.pop().expect("bytecode stack kinds were validated");
                            let operation = match instruction.opcode {
                                Opcode::BitwiseAnd => 0,
                                Opcode::BitwiseOr => 1,
                                Opcode::BitwiseXor => 2,
                                Opcode::ShiftLeft => 3,
                                Opcode::ShiftRight => 4,
                                Opcode::UnsignedShiftRight => 5,
                                _ => unreachable!("matched numeric bitwise opcode"),
                            };
                            let operation = builder.ins().iconst(types::I32, operation);
                            let call = builder.ins().call(bitwise_function, &[left, right, operation]);
                            stack.push(builder.inst_results(call)[0]);
                            stack_kinds.push(BytecodeStackKind::Number);
                        }
                        Opcode::LessThan
                        | Opcode::GreaterThan
                        | Opcode::LessEqual
                        | Opcode::GreaterEqual
                        | Opcode::Equal
                        | Opcode::NotEqual
                        | Opcode::StrictEqual => {
                            let right = stack.pop().expect("bytecode stack was validated");
                            let left = stack.pop().expect("bytecode stack was validated");
                            let condition = match instruction.opcode {
                                Opcode::LessThan => FloatCC::LessThan,
                                Opcode::GreaterThan => FloatCC::GreaterThan,
                                Opcode::LessEqual => FloatCC::LessThanOrEqual,
                                Opcode::GreaterEqual => FloatCC::GreaterThanOrEqual,
                                Opcode::Equal => FloatCC::Equal,
                                Opcode::NotEqual => FloatCC::NotEqual,
                                Opcode::StrictEqual => FloatCC::Equal,
                                _ => unreachable!("matched numeric comparison opcode"),
                            };
                            let condition = builder.ins().fcmp(condition, left, right);
                            let one = builder.ins().iconst(types::I8, 1);
                            let zero = builder.ins().iconst(types::I8, 0);
                            stack.push(builder.ins().select(condition, one, zero));
                            let _ = stack_kinds.pop().expect("bytecode stack kinds were validated");
                            let _ = stack_kinds.pop().expect("bytecode stack kinds were validated");
                            stack_kinds.push(BytecodeStackKind::Boolean);
                        }
                        Opcode::Negate | Opcode::Increment | Opcode::Decrement => {
                            let value = stack.pop().expect("bytecode stack was validated");
                            let result = match instruction.opcode {
                                Opcode::Negate => builder.ins().fneg(value),
                                Opcode::Increment => {
                                    let one = builder.ins().f64const(1.0);
                                    builder.ins().fadd(value, one)
                                }
                                Opcode::Decrement => {
                                    let one = builder.ins().f64const(1.0);
                                    builder.ins().fsub(value, one)
                                }
                                _ => unreachable!("matched numeric unary opcode"),
                            };
                            stack.push(result);
                            let _ = stack_kinds.pop().expect("bytecode stack kinds were validated");
                            stack_kinds.push(BytecodeStackKind::Number);
                        }
                        Opcode::BitwiseNot => {
                            let value = stack.pop().expect("bytecode stack was validated");
                            let _ = stack_kinds.pop().expect("bytecode stack kinds were validated");
                            let call = builder.ins().call(bitwise_not_function, &[value]);
                            stack.push(builder.inst_results(call)[0]);
                            stack_kinds.push(BytecodeStackKind::Number);
                        }
                        Opcode::Not => {
                            let value = stack.pop().expect("bytecode stack was validated");
                            let value_kind = stack_kinds.pop().expect("bytecode stack kinds were validated");
                            let is_falsy = match value_kind {
                                BytecodeStackKind::Number => {
                                    let zero = builder.ins().f64const(0.0);
                                    let truthy = builder.ins().fcmp(FloatCC::OrderedNotEqual, value, zero);
                                    builder.ins().icmp_imm_s(IntCC::Equal, truthy, 0)
                                }
                                BytecodeStackKind::Boolean => builder.ins().icmp_imm_s(IntCC::Equal, value, 0),
                                BytecodeStackKind::Undefined | BytecodeStackKind::Null => builder.ins().iconst(types::I8, 1),
                            };
                            let one = builder.ins().iconst(types::I8, 1);
                            let zero = builder.ins().iconst(types::I8, 0);
                            stack.push(builder.ins().select(is_falsy, one, zero));
                            stack_kinds.push(BytecodeStackKind::Boolean);
                        }
                        Opcode::Pop => {
                            stack.pop().expect("bytecode stack was validated");
                            let _ = stack_kinds.pop().expect("bytecode stack kinds were validated");
                        }
                        Opcode::Jump => {
                            let target = bytecode_jump_target(chunk, instruction).expect("jump target was validated");
                            let arguments = stack.iter().copied().map(BlockArg::Value).collect::<Vec<_>>();
                            builder.ins().jump(native_blocks[&target], &arguments);
                        }
                        Opcode::JumpIfFalse | Opcode::JumpIfTrue => {
                            let condition = stack.pop().expect("bytecode stack was validated");
                            let condition_kind = stack_kinds.pop().expect("bytecode stack kinds were validated");
                            let condition = match condition_kind {
                                BytecodeStackKind::Number => {
                                    let zero = builder.ins().f64const(0.0);
                                    builder.ins().fcmp(FloatCC::OrderedNotEqual, condition, zero)
                                }
                                BytecodeStackKind::Boolean => builder.ins().icmp_imm_s(IntCC::NotEqual, condition, 0),
                                BytecodeStackKind::Undefined | BytecodeStackKind::Null => builder.ins().iconst(types::I8, 0),
                            };
                            let target = bytecode_jump_target(chunk, instruction).expect("jump target was validated");
                            let fallthrough = native_blocks[&instruction.next_offset];
                            let target = native_blocks[&target];
                            let arguments = stack.iter().copied().map(BlockArg::Value).collect::<Vec<_>>();
                            if instruction.opcode == Opcode::JumpIfFalse {
                                builder.ins().brif(condition, fallthrough, &arguments, target, &arguments);
                            } else {
                                builder.ins().brif(condition, target, &arguments, fallthrough, &arguments);
                            }
                        }
                        Opcode::Return => {
                            let value = stack.pop().expect("bytecode stack was validated");
                            let _ = stack_kinds.pop().expect("bytecode stack kinds were validated");
                            let result_offset = std::mem::offset_of!(NativeJitFrame, result) as i32;
                            let tag = builder.ins().iconst(types::I32, JitValueTag::Number as i64);
                            builder.ins().store(
                                MemFlagsData::new(),
                                tag,
                                frame_pointer,
                                result_offset + std::mem::offset_of!(NativeJitValue, tag) as i32,
                            );
                            builder.ins().store(
                                MemFlagsData::new(),
                                value,
                                frame_pointer,
                                result_offset + std::mem::offset_of!(NativeJitValue, payload) as i32,
                            );
                            let returned = builder.ins().iconst(types::I32, NativeJitStatus::Returned as i64);
                            builder.ins().return_(&[returned]);
                        }
                        Opcode::Throw => {
                            let value = stack.pop().expect("bytecode stack was validated");
                            let kind = stack_kinds.pop().expect("bytecode stack kinds were validated");
                            let exception_offset = std::mem::offset_of!(NativeJitFrame, exception) as i32;
                            let tag = match kind {
                                BytecodeStackKind::Number => JitValueTag::Number,
                                BytecodeStackKind::Boolean => JitValueTag::Boolean,
                                BytecodeStackKind::Undefined => JitValueTag::Undefined,
                                BytecodeStackKind::Null => JitValueTag::Null,
                            };
                            let tag = builder.ins().iconst(types::I32, tag as i64);
                            builder.ins().store(
                                MemFlagsData::new(),
                                tag,
                                frame_pointer,
                                exception_offset + std::mem::offset_of!(NativeJitValue, tag) as i32,
                            );
                            let payload_offset = exception_offset + std::mem::offset_of!(NativeJitValue, payload) as i32;
                            match kind {
                                BytecodeStackKind::Number => {
                                    builder.ins().store(MemFlagsData::new(), value, frame_pointer, payload_offset);
                                }
                                BytecodeStackKind::Boolean => {
                                    let payload = builder.ins().uextend(types::I64, value);
                                    builder.ins().store(MemFlagsData::new(), payload, frame_pointer, payload_offset);
                                }
                                BytecodeStackKind::Undefined | BytecodeStackKind::Null => {
                                    let payload = builder.ins().iconst(types::I64, 0);
                                    builder.ins().store(MemFlagsData::new(), payload, frame_pointer, payload_offset);
                                }
                            }
                            if let Some(target) = instruction.exceptional_targets.first() {
                                let arguments = stack.iter().copied().map(BlockArg::Value).collect::<Vec<_>>();
                                builder.ins().jump(native_blocks[target], &arguments);
                            } else {
                                let threw = builder.ins().iconst(types::I32, NativeJitStatus::Threw as i64);
                                builder.ins().return_(&[threw]);
                            }
                        }
                        Opcode::SetupTry | Opcode::TeardownTry => {}
                        _ => unreachable!("bytecode instruction set was validated"),
                    }
                }
                let last = instructions.last().expect("basic blocks contain instructions");
                if !matches!(
                    last.opcode,
                    Opcode::Jump | Opcode::JumpIfFalse | Opcode::JumpIfTrue | Opcode::Return | Opcode::Throw
                ) {
                    let target = bytecode_block.successors[0];
                    let arguments = stack.iter().copied().map(BlockArg::Value).collect::<Vec<_>>();
                    builder.ins().jump(native_blocks[&target], &arguments);
                }
            }

            builder.seal_all_blocks();
            builder.finalize(self.module.target_config());
        }
        self.module
            .define_function(function_id, &mut context)
            .map_err(|error| error.to_string())?;
        self.module.clear_context(&mut context);

        Ok(Some((candidate.ip, candidate.params.len(), upvalue_count, function_id)))
    }

    fn compile_function(
        &mut self,
        candidate: &FunctionCandidate<'_>,
        inline_functions: &HashMap<String, InlineFunction>,
    ) -> Result<(usize, usize, FuncId), String> {
        let function_name = format!("js_{}_{}", self.next_function_id, candidate.name);
        self.next_function_id += 1;

        let pointer_type = self.module.target_config().pointer_type();
        let mut signature = self.module.make_signature();
        signature.params.push(AbiParam::new(pointer_type));
        signature.returns.push(AbiParam::new(types::I32));
        let function_id = self
            .module
            .declare_function(&function_name, Linkage::Local, &signature)
            .map_err(|error| error.to_string())?;

        let mut context = self.module.make_context();
        context.func = ClifFunction::with_name_signature(UserFuncName::user(0, function_id.as_u32()), signature);
        let fmod_function = self.module.declare_func_in_func(self.fmod_function, &mut context.func);
        let pow_function = self.module.declare_func_in_func(self.pow_function, &mut context.func);
        let bitwise_function = self.module.declare_func_in_func(self.bitwise_function, &mut context.func);
        let bitwise_not_function = self.module.declare_func_in_func(self.bitwise_not_function, &mut context.func);
        let math_builtin_function = self.module.declare_func_in_func(self.math_builtin_function, &mut context.func);
        let mut builder_context = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
            let entry = builder.create_block();
            builder.switch_to_block(entry);
            builder.append_block_params_for_function_params(entry);
            let frame_pointer = builder.block_params(entry)[0];
            let arguments_pointer = builder.ins().load(
                pointer_type,
                MemFlagsData::new(),
                frame_pointer,
                std::mem::offset_of!(NativeJitFrame, arguments) as i32,
            );

            let mut function_compiler = FunctionCompiler {
                builder: &mut builder,
                locals: HashMap::new(),
                fmod_function,
                pow_function,
                bitwise_function,
                bitwise_not_function,
                math_builtin_function,
                pointer_type,
                inline_functions: inline_functions.clone(),
            };
            for (index, name) in candidate.params.iter().enumerate() {
                let value = function_compiler.builder.ins().load(
                    types::F64,
                    MemFlagsData::new(),
                    arguments_pointer,
                    (index * std::mem::size_of::<NativeJitValue>() + std::mem::offset_of!(NativeJitValue, payload)) as i32,
                );
                function_compiler.declare_local(name, value)?;
            }

            for statement in candidate.body {
                if matches!(&*statement.kind, StatementKind::Return(_)) {
                    let StatementKind::Return(Some(expression)) = &*statement.kind else {
                        return Err("only numeric return expressions are supported".to_string());
                    };
                    let JitValue::Number(value) = function_compiler.compile_expression(expression)? else {
                        return Err("only numeric return expressions are supported".to_string());
                    };
                    let result_offset = std::mem::offset_of!(NativeJitFrame, result) as i32;
                    let tag = function_compiler.builder.ins().iconst(types::I32, JitValueTag::Number as i64);
                    function_compiler.builder.ins().store(
                        MemFlagsData::new(),
                        tag,
                        frame_pointer,
                        result_offset + std::mem::offset_of!(NativeJitValue, tag) as i32,
                    );
                    function_compiler.builder.ins().store(
                        MemFlagsData::new(),
                        value,
                        frame_pointer,
                        result_offset + std::mem::offset_of!(NativeJitValue, payload) as i32,
                    );
                    let returned = function_compiler.builder.ins().iconst(types::I32, NativeJitStatus::Returned as i64);
                    function_compiler.builder.ins().return_(&[returned]);
                } else {
                    function_compiler.compile_statement(statement)?;
                }
            }

            builder.seal_all_blocks();
            builder.finalize(self.module.target_config());
        }
        self.module
            .define_function(function_id, &mut context)
            .map_err(|error| error.to_string())?;
        self.module.clear_context(&mut context);

        Ok((candidate.ip, candidate.params.len(), function_id))
    }
}

fn is_identity_function(params: &[String], body: &[Statement]) -> bool {
    let [parameter] = params else {
        return false;
    };
    let [statement] = body else {
        return false;
    };
    matches!(
        &*statement.kind,
        StatementKind::Return(Some(Expr::Var(name, ..))) if name == parameter
    )
}

fn collect_function_bodies(statements: &[Statement]) -> HashMap<&str, (&[DestructuringElement], &[Statement])> {
    let mut declarations = HashMap::new();
    fn collect<'a>(statements: &'a [Statement], declarations: &mut HashMap<&'a str, (&'a [DestructuringElement], &'a [Statement])>) {
        for statement in statements {
            match &*statement.kind {
                StatementKind::FunctionDeclaration(name, params, body, false, false) => {
                    declarations.insert(name.as_str(), (params.as_slice(), body.as_slice()));
                    collect(body, declarations);
                }
                StatementKind::Export(_, Some(inner), _) => collect(std::slice::from_ref(inner), declarations),
                StatementKind::Const(bindings) => {
                    for (name, initializer) in bindings {
                        let (params, body) = match initializer {
                            Expr::Function(_, params, body, _) | Expr::ArrowFunction(params, body) => (params, body),
                            _ => continue,
                        };
                        declarations.insert(name.as_str(), (params.as_slice(), body.as_slice()));
                        collect(body, declarations);
                    }
                }
                StatementKind::Let(bindings) | StatementKind::Var(bindings) => {
                    for (name, initializer) in bindings {
                        let Some(initializer) = initializer else {
                            continue;
                        };
                        let (params, body) = match initializer {
                            Expr::Function(_, params, body, _) | Expr::ArrowFunction(params, body) => (params, body),
                            _ => continue,
                        };
                        declarations.insert(name.as_str(), (params.as_slice(), body.as_slice()));
                        collect(body, declarations);
                    }
                }
                StatementKind::Block(body) | StatementKind::While(_, body) | StatementKind::DoWhile(body, _) => {
                    collect(body, declarations);
                }
                StatementKind::If(branch) => {
                    collect(&branch.then_body, declarations);
                    if let Some(else_body) = &branch.else_body {
                        collect(else_body, declarations);
                    }
                }
                StatementKind::TryCatch(try_catch) => {
                    collect(&try_catch.try_body, declarations);
                    if let Some(catch_body) = &try_catch.catch_body {
                        collect(catch_body, declarations);
                    }
                    if let Some(finally_body) = &try_catch.finally_body {
                        collect(finally_body, declarations);
                    }
                }
                StatementKind::For(for_statement) => {
                    if let Some(initializer) = &for_statement.init {
                        collect(std::slice::from_ref(initializer), declarations);
                    }
                    collect(&for_statement.body, declarations);
                }
                StatementKind::ForOf(_, _, _, body)
                | StatementKind::ForOfExpr(_, _, body)
                | StatementKind::ForAwaitOf(_, _, _, body)
                | StatementKind::ForAwaitOfExpr(_, _, body)
                | StatementKind::ForIn(_, _, _, body)
                | StatementKind::ForInExpr(_, _, body)
                | StatementKind::ForInDestructuringObject(_, _, _, body)
                | StatementKind::ForInDestructuringArray(_, _, _, body)
                | StatementKind::ForOfDestructuringObject(_, _, _, body)
                | StatementKind::ForOfDestructuringArray(_, _, _, body)
                | StatementKind::ForAwaitOfDestructuringObject(_, _, _, body)
                | StatementKind::ForAwaitOfDestructuringArray(_, _, _, body)
                | StatementKind::With(_, body) => collect(body, declarations),
                StatementKind::Label(_, body) => collect(std::slice::from_ref(body), declarations),
                _ => {}
            }
        }
    }
    collect(statements, &mut declarations);
    declarations
}

fn collect_inline_functions(
    declarations: &HashMap<&str, (&[DestructuringElement], &[Statement])>,
    chunk: &Chunk<'_>,
    name_counts: &HashMap<&str, usize>,
) -> HashMap<String, InlineFunction> {
    let mut inline_functions = HashMap::new();
    for (ip, name) in &chunk.fn_names {
        if name_counts.get(name.as_str()) != Some(&1) {
            continue;
        }
        let Some((params, body)) = declarations.get(name.as_str()) else {
            continue;
        };
        let Some(params) = simple_parameters(params) else {
            continue;
        };
        let [statement] = *body else {
            continue;
        };
        let StatementKind::Return(Some(expression)) = &*statement.kind else {
            continue;
        };
        let analysis = JitAnalysis {
            locals: params.iter().cloned().collect(),
            ..JitAnalysis::default()
        };
        if supports_inline_numeric_expression(expression, &analysis) {
            inline_functions.insert(
                name.clone(),
                InlineFunction {
                    ip: *ip,
                    params,
                    expression: expression.clone(),
                },
            );
        }
    }
    inline_functions
}

fn supports_inline_numeric_expression(expression: &Expr, analysis: &JitAnalysis) -> bool {
    match expression {
        Expr::Number(_) => true,
        Expr::Var(name, ..) => analysis.is_number(name),
        Expr::Call(callee, arguments) if let Some((_, id)) = math_numeric_builtin_call(callee) => {
            id != BUILTIN_MATH_SUMPRECISE
                && !analysis.locals.contains("Math")
                && math_builtin_arity_supported(id, arguments.len())
                && arguments
                    .iter()
                    .all(|argument| supports_inline_numeric_expression(argument, analysis))
        }
        Expr::Binary(left, op, right) => {
            matches!(
                op,
                BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::Pow
                    | BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::LeftShift
                    | BinaryOp::RightShift
                    | BinaryOp::UnsignedRightShift
            ) && supports_inline_numeric_expression(left, analysis)
                && supports_inline_numeric_expression(right, analysis)
        }
        Expr::Mod(left, right) | Expr::Pow(left, right) => {
            supports_inline_numeric_expression(left, analysis) && supports_inline_numeric_expression(right, analysis)
        }
        Expr::UnaryNeg(inner) | Expr::UnaryPlus(inner) | Expr::BitNot(inner) => supports_inline_numeric_expression(inner, analysis),
        _ => false,
    }
}

fn collect_inline_dependencies_from_statement(
    statement: &Statement,
    inline_functions: &HashMap<String, InlineFunction>,
    dependencies: &mut HashSet<String>,
) {
    match &*statement.kind {
        StatementKind::Let(declarations) | StatementKind::Var(declarations) => {
            for (_, initializer) in declarations {
                if let Some(initializer) = initializer {
                    collect_inline_dependencies_from_expression(initializer, inline_functions, dependencies);
                }
            }
        }
        StatementKind::Const(declarations) => {
            for (_, initializer) in declarations {
                collect_inline_dependencies_from_expression(initializer, inline_functions, dependencies);
            }
        }
        StatementKind::Assign(_, expression) | StatementKind::Expr(expression) => {
            collect_inline_dependencies_from_expression(expression, inline_functions, dependencies);
        }
        StatementKind::If(if_statement) => {
            collect_inline_dependencies_from_expression(&if_statement.condition, inline_functions, dependencies);
            for statement in &if_statement.then_body {
                collect_inline_dependencies_from_statement(statement, inline_functions, dependencies);
            }
            if let Some(else_body) = &if_statement.else_body {
                for statement in else_body {
                    collect_inline_dependencies_from_statement(statement, inline_functions, dependencies);
                }
            }
        }
        StatementKind::For(for_statement) => {
            if let Some(init) = &for_statement.init {
                collect_inline_dependencies_from_statement(init, inline_functions, dependencies);
            }
            if let Some(test) = &for_statement.test {
                collect_inline_dependencies_from_expression(test, inline_functions, dependencies);
            }
            if let Some(update) = &for_statement.update {
                collect_inline_dependencies_from_statement(update, inline_functions, dependencies);
            }
            for statement in &for_statement.body {
                collect_inline_dependencies_from_statement(statement, inline_functions, dependencies);
            }
        }
        StatementKind::Block(statements) => {
            for statement in statements {
                collect_inline_dependencies_from_statement(statement, inline_functions, dependencies);
            }
        }
        _ => {}
    }
}

fn collect_builtin_dependencies_from_statement(statement: &Statement, dependencies: &mut HashSet<(String, usize)>) {
    match &*statement.kind {
        StatementKind::Let(declarations) | StatementKind::Var(declarations) => {
            for (_, initializer) in declarations {
                if let Some(initializer) = initializer {
                    collect_builtin_dependencies_from_expression(initializer, dependencies);
                }
            }
        }
        StatementKind::Const(declarations) => {
            for (_, initializer) in declarations {
                collect_builtin_dependencies_from_expression(initializer, dependencies);
            }
        }
        StatementKind::Assign(_, expression) | StatementKind::Expr(expression) | StatementKind::Throw(expression) => {
            collect_builtin_dependencies_from_expression(expression, dependencies)
        }
        StatementKind::Return(Some(expression)) => collect_builtin_dependencies_from_expression(expression, dependencies),
        StatementKind::If(if_statement) => {
            collect_builtin_dependencies_from_expression(&if_statement.condition, dependencies);
            for statement in &if_statement.then_body {
                collect_builtin_dependencies_from_statement(statement, dependencies);
            }
            if let Some(else_body) = &if_statement.else_body {
                for statement in else_body {
                    collect_builtin_dependencies_from_statement(statement, dependencies);
                }
            }
        }
        StatementKind::For(for_statement) => {
            if let Some(init) = &for_statement.init {
                collect_builtin_dependencies_from_statement(init, dependencies);
            }
            if let Some(test) = &for_statement.test {
                collect_builtin_dependencies_from_expression(test, dependencies);
            }
            if let Some(update) = &for_statement.update {
                collect_builtin_dependencies_from_statement(update, dependencies);
            }
            for statement in &for_statement.body {
                collect_builtin_dependencies_from_statement(statement, dependencies);
            }
        }
        StatementKind::Block(statements) => {
            for statement in statements {
                collect_builtin_dependencies_from_statement(statement, dependencies);
            }
        }
        _ => {}
    }
}

fn collect_builtin_dependencies_from_expression(expression: &Expr, dependencies: &mut HashSet<(String, usize)>) {
    match expression {
        Expr::Call(callee, arguments) => {
            if let Some((property, function_id)) = math_numeric_builtin_call(callee) {
                dependencies.insert((property.to_string(), function_id));
            }
            collect_builtin_dependencies_from_expression(callee, dependencies);
            for argument in arguments {
                collect_builtin_dependencies_from_expression(argument, dependencies);
            }
        }
        Expr::Assign(left, right)
        | Expr::Binary(left, _, right)
        | Expr::LogicalAnd(left, right)
        | Expr::LogicalOr(left, right)
        | Expr::NullishCoalescing(left, right)
        | Expr::Mod(left, right)
        | Expr::Pow(left, right)
        | Expr::AddAssign(left, right)
        | Expr::SubAssign(left, right)
        | Expr::PowAssign(left, right)
        | Expr::MulAssign(left, right)
        | Expr::DivAssign(left, right)
        | Expr::ModAssign(left, right)
        | Expr::BitXorAssign(left, right)
        | Expr::BitAndAssign(left, right)
        | Expr::BitOrAssign(left, right)
        | Expr::LeftShiftAssign(left, right)
        | Expr::RightShiftAssign(left, right)
        | Expr::UnsignedRightShiftAssign(left, right)
        | Expr::Index(left, right)
        | Expr::OptionalIndex(left, right) => {
            collect_builtin_dependencies_from_expression(left, dependencies);
            collect_builtin_dependencies_from_expression(right, dependencies);
        }
        Expr::Conditional(condition, then_expression, else_expression) => {
            collect_builtin_dependencies_from_expression(condition, dependencies);
            collect_builtin_dependencies_from_expression(then_expression, dependencies);
            collect_builtin_dependencies_from_expression(else_expression, dependencies);
        }
        Expr::UnaryNeg(inner)
        | Expr::UnaryPlus(inner)
        | Expr::BitNot(inner)
        | Expr::Increment(inner)
        | Expr::Decrement(inner)
        | Expr::PostIncrement(inner)
        | Expr::PostDecrement(inner)
        | Expr::Property(inner, _)
        | Expr::OptionalProperty(inner, _)
        | Expr::OptionalPrivateMember(inner, _)
        | Expr::PrivateMember(inner, _)
        | Expr::TypeOf(inner)
        | Expr::Delete(inner)
        | Expr::Void(inner)
        | Expr::Await(inner)
        | Expr::YieldStar(inner)
        | Expr::LogicalNot(inner)
        | Expr::New(inner, _)
        | Expr::Spread(inner) => collect_builtin_dependencies_from_expression(inner, dependencies),
        Expr::OptionalCall(callee, arguments) => {
            collect_builtin_dependencies_from_expression(callee, dependencies);
            for argument in arguments {
                collect_builtin_dependencies_from_expression(argument, dependencies);
            }
        }
        Expr::Yield(Some(inner)) => collect_builtin_dependencies_from_expression(inner, dependencies),
        Expr::Object(entries) => {
            for (key, value, _, _) in entries {
                collect_builtin_dependencies_from_expression(key, dependencies);
                collect_builtin_dependencies_from_expression(value, dependencies);
            }
        }
        Expr::Array(elements) => {
            for element in elements.iter().flatten() {
                collect_builtin_dependencies_from_expression(element, dependencies);
            }
        }
        _ => {}
    }
}

fn collect_inline_dependencies_from_expression(
    expression: &Expr,
    inline_functions: &HashMap<String, InlineFunction>,
    dependencies: &mut HashSet<String>,
) {
    match expression {
        Expr::Call(callee, arguments) => {
            if let Expr::Var(name, ..) = &**callee
                && inline_functions.contains_key(name)
            {
                dependencies.insert(name.clone());
            }
            for argument in arguments {
                collect_inline_dependencies_from_expression(argument, inline_functions, dependencies);
            }
        }
        Expr::Assign(left, right)
        | Expr::Binary(left, _, right)
        | Expr::Mod(left, right)
        | Expr::Pow(left, right)
        | Expr::AddAssign(left, right)
        | Expr::SubAssign(left, right)
        | Expr::MulAssign(left, right)
        | Expr::DivAssign(left, right)
        | Expr::ModAssign(left, right)
        | Expr::Index(left, right) => {
            collect_inline_dependencies_from_expression(left, inline_functions, dependencies);
            collect_inline_dependencies_from_expression(right, inline_functions, dependencies);
        }
        Expr::UnaryNeg(inner)
        | Expr::UnaryPlus(inner)
        | Expr::Increment(inner)
        | Expr::Decrement(inner)
        | Expr::PostIncrement(inner)
        | Expr::PostDecrement(inner)
        | Expr::Property(inner, _) => collect_inline_dependencies_from_expression(inner, inline_functions, dependencies),
        Expr::Array(elements) => {
            for element in elements.iter().flatten() {
                collect_inline_dependencies_from_expression(element, inline_functions, dependencies);
            }
        }
        Expr::Object(entries) => {
            for (key, value, _, _) in entries {
                collect_inline_dependencies_from_expression(key, inline_functions, dependencies);
                collect_inline_dependencies_from_expression(value, inline_functions, dependencies);
            }
        }
        _ => {}
    }
}

fn math_numeric_builtin_call(callee: &Expr) -> Option<(&'static str, usize)> {
    let Expr::Property(base, property) = callee else {
        return None;
    };
    if !matches!(&**base, Expr::Var(name, ..) if name == "Math") {
        return None;
    }
    match property.as_str() {
        "abs" => Some(("abs", BUILTIN_MATH_ABS)),
        "ceil" => Some(("ceil", BUILTIN_MATH_CEIL)),
        "floor" => Some(("floor", BUILTIN_MATH_FLOOR)),
        "max" => Some(("max", BUILTIN_MATH_MAX)),
        "min" => Some(("min", BUILTIN_MATH_MIN)),
        "sin" => Some(("sin", BUILTIN_MATH_SIN)),
        "cos" => Some(("cos", BUILTIN_MATH_COS)),
        "tan" => Some(("tan", BUILTIN_MATH_TAN)),
        "asin" => Some(("asin", BUILTIN_MATH_ASIN)),
        "acos" => Some(("acos", BUILTIN_MATH_ACOS)),
        "atan" => Some(("atan", BUILTIN_MATH_ATAN)),
        "atan2" => Some(("atan2", BUILTIN_MATH_ATAN2)),
        "sinh" => Some(("sinh", BUILTIN_MATH_SINH)),
        "cosh" => Some(("cosh", BUILTIN_MATH_COSH)),
        "tanh" => Some(("tanh", BUILTIN_MATH_TANH)),
        "asinh" => Some(("asinh", BUILTIN_MATH_ASINH)),
        "acosh" => Some(("acosh", BUILTIN_MATH_ACOSH)),
        "atanh" => Some(("atanh", BUILTIN_MATH_ATANH)),
        "exp" => Some(("exp", BUILTIN_MATH_EXP)),
        "expm1" => Some(("expm1", BUILTIN_MATH_EXPM1)),
        "log" => Some(("log", BUILTIN_MATH_LOG)),
        "log10" => Some(("log10", BUILTIN_MATH_LOG10)),
        "log1p" => Some(("log1p", BUILTIN_MATH_LOG1P)),
        "log2" => Some(("log2", BUILTIN_MATH_LOG2)),
        "fround" => Some(("fround", BUILTIN_MATH_FROUND)),
        "f16round" => Some(("f16round", BUILTIN_MATH_F16ROUND)),
        "round" => Some(("round", BUILTIN_MATH_ROUND)),
        "sign" => Some(("sign", BUILTIN_MATH_SIGN)),
        "sqrt" => Some(("sqrt", BUILTIN_MATH_SQRT)),
        "trunc" => Some(("trunc", BUILTIN_MATH_TRUNC)),
        "cbrt" => Some(("cbrt", BUILTIN_MATH_CBRT)),
        "hypot" => Some(("hypot", BUILTIN_MATH_HYPOT)),
        "pow" => Some(("pow", BUILTIN_MATH_POW)),
        "random" => Some(("random", BUILTIN_MATH_RANDOM)),
        "clz32" => Some(("clz32", BUILTIN_MATH_CLZ32)),
        "imul" => Some(("imul", BUILTIN_MATH_IMUL)),
        "sumPrecise" => Some(("sumPrecise", BUILTIN_MATH_SUMPRECISE)),
        _ => None,
    }
}

fn math_builtin_arity_supported(id: usize, argument_count: usize) -> bool {
    match id {
        BUILTIN_MATH_MAX | BUILTIN_MATH_MIN | BUILTIN_MATH_HYPOT => true,
        BUILTIN_MATH_ATAN2 | BUILTIN_MATH_POW | BUILTIN_MATH_IMUL => argument_count <= 2,
        BUILTIN_MATH_RANDOM => argument_count == 0,
        BUILTIN_MATH_SUMPRECISE => argument_count == 1,
        _ => argument_count <= 1,
    }
}

fn math_builtin_arguments_supported(id: usize, arguments: &[Expr], analysis: &JitAnalysis) -> bool {
    if !math_builtin_arity_supported(id, arguments.len()) || analysis.locals.contains("Math") {
        return false;
    }
    if id == BUILTIN_MATH_SUMPRECISE {
        return matches!(
            arguments.first(),
            Some(Expr::Array(elements))
                if elements.iter().all(|element| element.as_ref().is_some_and(|value| supports_expression(value, analysis)))
        ) || matches!(
            arguments.first(),
            Some(Expr::Var(name, ..)) if analysis.arrays.contains_key(name)
        );
    }
    arguments.iter().all(|argument| supports_expression(argument, analysis))
}

fn simple_parameters(params: &[DestructuringElement]) -> Option<Vec<String>> {
    params
        .iter()
        .map(|param| match param {
            DestructuringElement::Variable(name, None) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

#[derive(Clone, Default)]
struct JitAnalysis {
    locals: HashSet<String>,
    arrays: HashMap<String, usize>,
    objects: HashMap<String, HashSet<String>>,
    induction_vars: HashSet<String>,
    safe_indices: HashMap<String, (String, usize)>,
    inline_functions: HashMap<String, InlineFunction>,
}

impl JitAnalysis {
    fn is_number(&self, name: &str) -> bool {
        self.locals.contains(name) && !self.arrays.contains_key(name) && !self.objects.contains_key(name)
    }
}

fn is_supported_function(params: &[String], body: &[Statement], inline_functions: &HashMap<String, InlineFunction>) -> bool {
    let mut analysis = JitAnalysis {
        locals: params.iter().cloned().collect(),
        inline_functions: inline_functions.clone(),
        ..JitAnalysis::default()
    };
    let Some((last, prefix)) = body.split_last() else {
        return false;
    };
    if !matches!(&*last.kind, StatementKind::Return(Some(_))) {
        return false;
    }
    for statement in prefix {
        if !supports_statement(statement, &mut analysis) {
            return false;
        }
    }
    let StatementKind::Return(Some(expression)) = &*last.kind else {
        return false;
    };
    let supported = matches!(
        expression,
        Expr::Number(_)
            | Expr::Var(..)
            | Expr::Binary(..)
            | Expr::Mod(..)
            | Expr::UnaryNeg(..)
            | Expr::UnaryPlus(..)
            | Expr::Call(..)
            | Expr::Property(..)
            | Expr::Index(..)
    ) && supports_expression(expression, &analysis);
    if !supported {
        log::debug!("JIT rejected return expression: {expression:?}");
    }
    supported
}

fn supports_statement(statement: &Statement, analysis: &mut JitAnalysis) -> bool {
    match &*statement.kind {
        StatementKind::Let(declarations) | StatementKind::Var(declarations) => {
            for (name, initializer) in declarations {
                let Some(initializer) = initializer else {
                    return false;
                };
                if !supports_expression(initializer, analysis) || !analysis.locals.insert(name.clone()) {
                    return false;
                }
                record_local_shape(name, initializer, analysis);
            }
            true
        }
        StatementKind::Const(declarations) => {
            for (name, initializer) in declarations {
                if !supports_expression(initializer, analysis) || !analysis.locals.insert(name.clone()) {
                    return false;
                }
                record_local_shape(name, initializer, analysis);
            }
            true
        }
        StatementKind::Assign(name, expression) => {
            analysis.is_number(name)
                && !analysis.induction_vars.contains(name)
                && !analysis.safe_indices.contains_key(name)
                && supports_expression(expression, analysis)
        }
        StatementKind::Expr(expression) => supports_expression(expression, analysis),
        StatementKind::If(if_statement) => {
            if !supports_expression(&if_statement.condition, analysis) {
                return false;
            }
            let mut then_analysis = analysis.clone();
            if !if_statement
                .then_body
                .iter()
                .all(|statement| supports_branch_statement(statement, &mut then_analysis))
            {
                return false;
            }
            let mut else_analysis = analysis.clone();
            if let Some(else_body) = &if_statement.else_body
                && !else_body
                    .iter()
                    .all(|statement| supports_branch_statement(statement, &mut else_analysis))
            {
                return false;
            }
            true
        }
        StatementKind::For(for_statement) => {
            let mut loop_analysis = analysis.clone();
            if let Some(init) = &for_statement.init
                && !supports_statement(init, &mut loop_analysis)
            {
                return false;
            }
            let induction_var = simple_induction_variable(for_statement);
            if !for_statement
                .test
                .as_ref()
                .is_some_and(|test| supports_expression(test, &loop_analysis))
            {
                return false;
            }
            if let Some(update) = &for_statement.update
                && !supports_statement(update, &mut loop_analysis)
            {
                return false;
            }
            if let Some(induction_var) = induction_var {
                loop_analysis.induction_vars.insert(induction_var);
            }
            for body_statement in &for_statement.body {
                if !supports_statement(body_statement, &mut loop_analysis) {
                    return false;
                }
            }
            true
        }
        StatementKind::Block(statements) => {
            let mut block_analysis = analysis.clone();
            statements
                .iter()
                .all(|statement| supports_statement(statement, &mut block_analysis))
        }
        StatementKind::Return(_) => false,
        _ => false,
    }
}

fn supports_branch_statement(statement: &Statement, analysis: &mut JitAnalysis) -> bool {
    match &*statement.kind {
        StatementKind::Assign(..) | StatementKind::Expr(..) => supports_statement(statement, analysis),
        StatementKind::Block(statements) => statements.iter().all(|statement| supports_branch_statement(statement, analysis)),
        StatementKind::If(if_statement) => {
            if !supports_expression(&if_statement.condition, analysis) {
                return false;
            }
            let mut then_analysis = analysis.clone();
            if !if_statement
                .then_body
                .iter()
                .all(|statement| supports_branch_statement(statement, &mut then_analysis))
            {
                return false;
            }
            let mut else_analysis = analysis.clone();
            if let Some(else_body) = &if_statement.else_body
                && !else_body
                    .iter()
                    .all(|statement| supports_branch_statement(statement, &mut else_analysis))
            {
                return false;
            }
            true
        }
        _ => false,
    }
}

fn record_local_shape(name: &str, initializer: &Expr, analysis: &mut JitAnalysis) {
    match initializer {
        Expr::Array(elements) => {
            analysis.arrays.insert(name.to_string(), elements.len());
        }
        Expr::Object(entries) => {
            let fields = entries
                .iter()
                .filter_map(|(key, _, computed, has_value)| (!computed && *has_value).then(|| object_property_name(key)).flatten())
                .collect();
            analysis.objects.insert(name.to_string(), fields);
        }
        expression => {
            if let Some((induction_var, array_name)) = modulo_array_length(expression)
                && analysis.induction_vars.contains(induction_var)
                && let Some(&length) = analysis.arrays.get(array_name)
            {
                analysis.safe_indices.insert(name.to_string(), (array_name.to_string(), length));
            }
        }
    }
}

fn object_property_name(key: &Expr) -> Option<String> {
    match key {
        Expr::StringLit(units) => String::from_utf16(units).ok(),
        _ => None,
    }
}

fn modulo_array_length(expression: &Expr) -> Option<(&str, &str)> {
    let (left, right) = match expression {
        Expr::Binary(left, BinaryOp::Mod, right) | Expr::Mod(left, right) => (&**left, &**right),
        _ => return None,
    };
    let Expr::Var(induction_var, ..) = left else {
        return None;
    };
    let Expr::Property(array, key) = right else {
        return None;
    };
    if key != "length" {
        return None;
    }
    let Expr::Var(array_name, ..) = &**array else {
        return None;
    };
    Some((induction_var, array_name))
}

fn simple_induction_variable(for_statement: &crate::core::ForStatement) -> Option<String> {
    let init_statement = for_statement.init.as_ref()?;
    let (init_name, initializer) = match &*init_statement.kind {
        StatementKind::Let(declarations) | StatementKind::Var(declarations) if declarations.len() == 1 => {
            let (name, initializer) = &declarations[0];
            (name, initializer.as_ref()?)
        }
        StatementKind::Const(declarations) if declarations.len() == 1 => {
            let (name, initializer) = &declarations[0];
            (name, initializer)
        }
        _ => return None,
    };
    if !matches!(initializer, Expr::Number(number) if *number == 0.0) {
        return None;
    }
    let test_matches = matches!(
        for_statement.test.as_ref(),
        Some(Expr::Binary(left, BinaryOp::LessThan | BinaryOp::LessEqual, _))
            if matches!(&**left, Expr::Var(name, ..) if name == init_name)
    );
    let update_matches = matches!(
        for_statement.update.as_ref().map(|statement| &*statement.kind),
        Some(StatementKind::Expr(Expr::PostIncrement(target) | Expr::Increment(target)))
            if matches!(&**target, Expr::Var(name, ..) if name == init_name)
    );
    (test_matches && update_matches).then(|| init_name.clone())
}

fn supports_lvalue(expression: &Expr, analysis: &JitAnalysis) -> bool {
    match expression {
        Expr::Var(name, ..) => {
            analysis.is_number(name) && !analysis.induction_vars.contains(name) && !analysis.safe_indices.contains_key(name)
        }
        Expr::Property(base, key) => {
            matches!(&**base, Expr::Var(name, ..) if analysis.objects.get(name).is_some_and(|fields| fields.contains(key)))
        }
        Expr::Index(base, index) => {
            if let Expr::Var(name, ..) = &**base {
                analysis
                    .arrays
                    .get(name)
                    .is_some_and(|&length| supports_expression(index, analysis) && safe_array_index(index, name, length, analysis))
            } else {
                false
            }
        }
        _ => false,
    }
}

fn safe_array_index(expression: &Expr, array_name: &str, length: usize, analysis: &JitAnalysis) -> bool {
    match expression {
        Expr::Number(index) => *index >= 0.0 && *index < length as f64 && index.fract() == 0.0,
        Expr::Var(name, ..) => analysis
            .safe_indices
            .get(name)
            .is_some_and(|(bounded_array, bound)| bounded_array == array_name && *bound <= length),
        Expr::Binary(left, BinaryOp::Sub, right) => {
            matches!(&**left, Expr::Property(base, key) if key == "length" && matches!(&**base, Expr::Var(name, ..) if name == array_name))
                && matches!(&**right, Expr::Number(number) if *number == 1.0)
                && length > 0
        }
        _ => modulo_array_length(expression).is_some_and(|(induction_var, bounded_array)| {
            analysis.induction_vars.contains(induction_var)
                && bounded_array == array_name
                && analysis.arrays.get(bounded_array) == Some(&length)
        }),
    }
}

fn supports_expression(expression: &Expr, analysis: &JitAnalysis) -> bool {
    match expression {
        Expr::Number(_) => true,
        Expr::Var(name, ..) => analysis.is_number(name),
        Expr::Binary(left, op, right) => {
            matches!(
                op,
                BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::Pow
                    | BinaryOp::LessThan
                    | BinaryOp::GreaterThan
                    | BinaryOp::LessEqual
                    | BinaryOp::GreaterEqual
                    | BinaryOp::Equal
                    | BinaryOp::StrictEqual
                    | BinaryOp::NotEqual
                    | BinaryOp::StrictNotEqual
                    | BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::LeftShift
                    | BinaryOp::RightShift
                    | BinaryOp::UnsignedRightShift
            ) && supports_expression(left, analysis)
                && supports_expression(right, analysis)
        }
        Expr::Mod(left, right) | Expr::Pow(left, right) => supports_expression(left, analysis) && supports_expression(right, analysis),
        Expr::UnaryNeg(inner) | Expr::UnaryPlus(inner) | Expr::BitNot(inner) => supports_expression(inner, analysis),
        Expr::Assign(left, right) => supports_lvalue(left, analysis) && supports_expression(right, analysis),
        Expr::AddAssign(left, right)
        | Expr::SubAssign(left, right)
        | Expr::MulAssign(left, right)
        | Expr::DivAssign(left, right)
        | Expr::ModAssign(left, right) => supports_lvalue(left, analysis) && supports_expression(right, analysis),
        Expr::Increment(inner) | Expr::Decrement(inner) | Expr::PostIncrement(inner) | Expr::PostDecrement(inner) => {
            matches!(&**inner, Expr::Var(name, ..) if analysis.is_number(name) && !analysis.induction_vars.contains(name) && !analysis.safe_indices.contains_key(name))
        }
        Expr::Array(elements) => elements
            .iter()
            .all(|element| element.as_ref().is_some_and(|value| supports_expression(value, analysis))),
        Expr::Object(entries) => entries.iter().all(|(key, value, computed, has_value)| {
            !computed && *has_value && object_property_name(key).is_some() && supports_expression(value, analysis)
        }),
        Expr::Property(base, key) => {
            if let Expr::Var(name, ..) = &**base {
                analysis.objects.get(name).is_some_and(|fields| fields.contains(key))
                    || (key == "length" && analysis.arrays.contains_key(name))
            } else {
                false
            }
        }
        Expr::Index(base, index) => {
            if let Expr::Var(name, ..) = &**base {
                analysis
                    .arrays
                    .get(name)
                    .is_some_and(|&length| supports_expression(index, analysis) && safe_array_index(index, name, length, analysis))
            } else {
                false
            }
        }
        Expr::Call(callee, arguments) => {
            if let Some((_, id)) = math_numeric_builtin_call(callee) {
                return math_builtin_arguments_supported(id, arguments, analysis);
            }
            let Expr::Var(name, ..) = &**callee else {
                return false;
            };
            if analysis.locals.contains(name) {
                return false;
            }
            let Some(inline) = analysis.inline_functions.get(name) else {
                return false;
            };
            arguments.len() == inline.params.len() && arguments.iter().all(|arg| supports_expression(arg, analysis))
        }
        _ => false,
    }
}

struct FunctionCompiler<'a, 'b> {
    builder: &'a mut FunctionBuilder<'b>,
    locals: HashMap<String, JitLocal>,
    fmod_function: FuncRef,
    pow_function: FuncRef,
    bitwise_function: FuncRef,
    bitwise_not_function: FuncRef,
    math_builtin_function: FuncRef,
    pointer_type: types::Type,
    inline_functions: HashMap<String, InlineFunction>,
}

impl FunctionCompiler<'_, '_> {
    fn declare_local(&mut self, name: &str, value: cranelift_codegen::ir::Value) -> Result<(), String> {
        if self.locals.contains_key(name) {
            return Err(format!("duplicate local {name}"));
        }
        let variable = self.builder.declare_var(types::F64);
        self.builder.def_var(variable, value);
        self.locals.insert(name.to_string(), JitLocal::Number(variable));
        Ok(())
    }

    fn local(&self, name: &str) -> Result<Variable, String> {
        match self.locals.get(name) {
            Some(JitLocal::Number(variable)) => Ok(*variable),
            Some(_) => Err(format!("{name} is not a numeric local")),
            None => Err(format!("unknown local {name}")),
        }
    }

    fn compile_math_builtin_helper(&mut self, arguments: &[Expr], function_id: usize) -> Result<JitValue, String> {
        let mut values = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let JitValue::Number(value) = self.compile_expression(argument)? else {
                return Err("Math builtin arguments must be numeric".to_string());
            };
            values.push(value);
        }
        self.compile_math_builtin_values(values, function_id)
    }

    fn compile_math_builtin_values(&mut self, values: Vec<cranelift_codegen::ir::Value>, function_id: usize) -> Result<JitValue, String> {
        let arguments_pointer = if values.is_empty() {
            self.builder.ins().iconst(self.pointer_type, 0)
        } else {
            let bytes = values
                .len()
                .checked_mul(std::mem::size_of::<f64>())
                .and_then(|bytes| u32::try_from(bytes).ok())
                .ok_or_else(|| "too many Math builtin arguments".to_string())?;
            let stack_slot = self
                .builder
                .create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, bytes, 3));
            let pointer = self.builder.ins().stack_addr(self.pointer_type, stack_slot, 0);
            for (index, value) in values.iter().copied().enumerate() {
                let offset = index
                    .checked_mul(std::mem::size_of::<f64>())
                    .and_then(|offset| i32::try_from(offset).ok())
                    .ok_or_else(|| "too many Math builtin arguments".to_string())?;
                self.builder.ins().store(MemFlagsData::new(), value, pointer, offset);
            }
            pointer
        };
        let length = self.builder.ins().iconst(self.pointer_type, values.len() as i64);
        let function_id = self.builder.ins().iconst(types::I32, function_id as i64);
        let call = self
            .builder
            .ins()
            .call(self.math_builtin_function, &[arguments_pointer, length, function_id]);
        Ok(JitValue::Number(self.builder.inst_results(call)[0]))
    }

    fn compile_lvalue(&mut self, expression: &Expr) -> Result<JitLValue, String> {
        match expression {
            Expr::Var(name, ..) => Ok(JitLValue::Variable(self.local(name)?)),
            Expr::Property(base, key) => {
                let Expr::Var(name, ..) = &**base else {
                    return Err("only local object properties are supported".to_string());
                };
                match self.locals.get(name) {
                    Some(JitLocal::Object(fields)) => fields
                        .get(key)
                        .copied()
                        .map(JitLValue::Variable)
                        .ok_or_else(|| format!("unknown local object property {key}")),
                    _ => Err("only local object properties are supported".to_string()),
                }
            }
            Expr::Index(base, index) => {
                let Expr::Var(name, ..) = &**base else {
                    return Err("only local array indices are supported".to_string());
                };
                let JitLocal::Array(elements) = self.locals.get(name).ok_or_else(|| format!("unknown local {name}"))? else {
                    return Err("only local array indices are supported".to_string());
                };
                let elements = elements.clone();
                let JitValue::Number(index) = self.compile_expression(index)? else {
                    return Err("array indices must be numeric".to_string());
                };
                let index_matches = elements
                    .iter()
                    .enumerate()
                    .map(|(position, _)| {
                        let position = self.builder.ins().f64const(position as f64);
                        self.builder.ins().fcmp(FloatCC::Equal, index, position)
                    })
                    .collect();
                Ok(JitLValue::ArrayElement { elements, index_matches })
            }
            _ => Err("unsupported assignment target".to_string()),
        }
    }

    fn read_lvalue(&mut self, target: &JitLValue) -> cranelift_codegen::ir::Value {
        match target {
            JitLValue::Variable(variable) => self.builder.use_var(*variable),
            JitLValue::ArrayElement { elements, index_matches } => {
                let mut value = self.builder.use_var(elements[0]);
                for (element, condition) in elements.iter().zip(index_matches).skip(1) {
                    let element_value = self.builder.use_var(*element);
                    value = self.builder.ins().select(*condition, element_value, value);
                }
                value
            }
        }
    }

    fn write_lvalue(&mut self, target: &JitLValue, value: cranelift_codegen::ir::Value) {
        match target {
            JitLValue::Variable(variable) => self.builder.def_var(*variable, value),
            JitLValue::ArrayElement { elements, index_matches } => {
                for (element, condition) in elements.iter().zip(index_matches) {
                    let old_value = self.builder.use_var(*element);
                    let next_value = self.builder.ins().select(*condition, value, old_value);
                    self.builder.def_var(*element, next_value);
                }
            }
        }
    }

    fn declare_value(&mut self, name: &str, value: JitValue) -> Result<(), String> {
        if self.locals.contains_key(name) {
            return Err(format!("duplicate local {name}"));
        }
        let local = match value {
            JitValue::Number(value) => {
                let variable = self.builder.declare_var(types::F64);
                self.builder.def_var(variable, value);
                JitLocal::Number(variable)
            }
            JitValue::Array(elements) => JitLocal::Array(elements),
            JitValue::Object(fields) => JitLocal::Object(fields),
            JitValue::Boolean(_) => return Err("boolean locals are not supported".to_string()),
        };
        self.locals.insert(name.to_string(), local);
        Ok(())
    }

    fn compile_statement(&mut self, statement: &Statement) -> Result<(), String> {
        match &*statement.kind {
            StatementKind::Let(declarations) | StatementKind::Var(declarations) => {
                for (name, initializer) in declarations {
                    let Some(initializer) = initializer else {
                        return Err("uninitialized locals are not supported".to_string());
                    };
                    let value = self.compile_expression(initializer)?;
                    self.declare_value(name, value)?;
                }
            }
            StatementKind::Const(declarations) => {
                for (name, initializer) in declarations {
                    let value = self.compile_expression(initializer)?;
                    self.declare_value(name, value)?;
                }
            }
            StatementKind::Assign(name, expression) => {
                let variable = self.local(name)?;
                let JitValue::Number(value) = self.compile_expression(expression)? else {
                    return Err("local assignments must be numeric".to_string());
                };
                self.builder.def_var(variable, value);
            }
            StatementKind::Expr(expression) => {
                let _ = self.compile_expression(expression)?;
            }
            StatementKind::For(for_statement) => {
                let outer_locals = self.locals.clone();
                if let Some(init) = &for_statement.init {
                    self.compile_statement(init)?;
                }
                let header = self.builder.create_block();
                let body = self.builder.create_block();
                let exit = self.builder.create_block();
                self.builder.ins().jump(header, &[]);

                self.builder.switch_to_block(header);
                let condition = self.compile_expression(for_statement.test.as_ref().expect("validated for-test"))?;
                let condition = match condition {
                    JitValue::Boolean(value) => value,
                    JitValue::Number(value) => {
                        let zero = self.builder.ins().f64const(0.0);
                        self.builder.ins().fcmp(FloatCC::OrderedNotEqual, value, zero)
                    }
                    JitValue::Array(_) | JitValue::Object(_) => return Err("loop tests must be scalar".to_string()),
                };
                self.builder.ins().brif(condition, body, &[], exit, &[]);

                self.builder.switch_to_block(body);
                for statement in &for_statement.body {
                    self.compile_statement(statement)?;
                }
                if let Some(update) = &for_statement.update {
                    self.compile_statement(update)?;
                }
                self.builder.ins().jump(header, &[]);
                self.builder.seal_block(header);
                self.builder.seal_block(body);
                self.builder.switch_to_block(exit);
                self.builder.seal_block(exit);
                self.locals = outer_locals;
            }
            StatementKind::Block(statements) => {
                let outer_locals = self.locals.clone();
                for statement in statements {
                    self.compile_statement(statement)?;
                }
                self.locals = outer_locals;
            }
            _ => return Err("unsupported statement".to_string()),
        }
        Ok(())
    }

    fn compile_expression(&mut self, expression: &Expr) -> Result<JitValue, String> {
        match expression {
            Expr::Number(number) => Ok(JitValue::Number(self.builder.ins().f64const(*number))),
            Expr::Var(name, ..) => Ok(JitValue::Number(self.builder.use_var(self.local(name)?))),
            Expr::Array(elements) => {
                let mut values = Vec::with_capacity(elements.len());
                for element in elements {
                    let Some(element) = element else {
                        return Err("array holes are not supported".to_string());
                    };
                    let JitValue::Number(value) = self.compile_expression(element)? else {
                        return Err("array elements must be numeric".to_string());
                    };
                    let variable = self.builder.declare_var(types::F64);
                    self.builder.def_var(variable, value);
                    values.push(variable);
                }
                Ok(JitValue::Array(values))
            }
            Expr::Object(entries) => {
                let mut fields = HashMap::with_capacity(entries.len());
                for (key, expression, computed, has_value) in entries {
                    if *computed || !has_value {
                        return Err("only plain data properties are supported".to_string());
                    }
                    let Some(name) = object_property_name(key) else {
                        return Err("object property names must be static".to_string());
                    };
                    if name == "__proto__" {
                        return Err("object prototype properties are not supported".to_string());
                    }
                    let JitValue::Number(value) = self.compile_expression(expression)? else {
                        return Err("object property values must be numeric".to_string());
                    };
                    let variable = self.builder.declare_var(types::F64);
                    self.builder.def_var(variable, value);
                    fields.insert(name, variable);
                }
                Ok(JitValue::Object(fields))
            }
            Expr::Property(base, key) => {
                let Expr::Var(name, ..) = &**base else {
                    return Err("only local data properties are supported".to_string());
                };
                match self.locals.get(name) {
                    Some(JitLocal::Object(fields)) => {
                        let variable = fields.get(key).copied().ok_or_else(|| format!("unknown object property {key}"))?;
                        Ok(JitValue::Number(self.builder.use_var(variable)))
                    }
                    Some(JitLocal::Array(elements)) if key == "length" => {
                        Ok(JitValue::Number(self.builder.ins().f64const(elements.len() as f64)))
                    }
                    _ => Err("only local data properties are supported".to_string()),
                }
            }
            Expr::Index(..) => {
                let target = self.compile_lvalue(expression)?;
                Ok(JitValue::Number(self.read_lvalue(&target)))
            }
            Expr::Call(callee, arguments) => {
                if let Some((property, function_id)) = math_numeric_builtin_call(callee) {
                    if self.locals.contains_key("Math") || !math_builtin_arity_supported(function_id, arguments.len()) {
                        return Err(format!(
                            "Math.{property} requires the unshadowed Math global and a supported number of numeric arguments"
                        ));
                    }
                    if function_id == BUILTIN_MATH_SUMPRECISE {
                        let [argument] = arguments.as_slice() else {
                            return Err("Math.sumPrecise requires one virtual numeric array".to_string());
                        };
                        let values = match argument {
                            Expr::Array(_) => match self.compile_expression(argument)? {
                                JitValue::Array(elements) => elements.into_iter().map(|variable| self.builder.use_var(variable)).collect(),
                                _ => return Err("Math.sumPrecise requires a virtual numeric array".to_string()),
                            },
                            Expr::Var(name, ..) => match self.locals.get(name) {
                                Some(JitLocal::Array(elements)) => {
                                    elements.iter().map(|variable| self.builder.use_var(*variable)).collect()
                                }
                                _ => return Err("Math.sumPrecise requires a virtual numeric array".to_string()),
                            },
                            _ => return Err("Math.sumPrecise requires a virtual numeric array".to_string()),
                        };
                        return self.compile_math_builtin_values(values, function_id);
                    }
                    if arguments.len() != 1 || !matches!(property, "abs" | "ceil" | "floor" | "round" | "sign" | "sqrt" | "trunc") {
                        return self.compile_math_builtin_helper(arguments, function_id);
                    }
                    let JitValue::Number(argument) = self.compile_expression(&arguments[0])? else {
                        return Err(format!("Math.{property} requires numeric arguments"));
                    };
                    let result = match property {
                        "abs" => self.builder.ins().fabs(argument),
                        "ceil" => self.builder.ins().ceil(argument),
                        "floor" => self.builder.ins().floor(argument),
                        "sqrt" => self.builder.ins().sqrt(argument),
                        "sign" => {
                            let zero = self.builder.ins().f64const(0.0);
                            let negative_one = self.builder.ins().f64const(-1.0);
                            let positive_one = self.builder.ins().f64const(1.0);
                            let is_negative = self.builder.ins().fcmp(FloatCC::LessThan, argument, zero);
                            let is_zero = self.builder.ins().fcmp(FloatCC::Equal, argument, zero);
                            let is_nan = self.builder.ins().fcmp(FloatCC::Unordered, argument, argument);
                            let sign = self.builder.ins().select(is_negative, negative_one, positive_one);
                            let sign_or_nan = self.builder.ins().select(is_nan, argument, sign);
                            self.builder.ins().select(is_zero, argument, sign_or_nan)
                        }
                        "trunc" => {
                            let zero = self.builder.ins().f64const(0.0);
                            let is_negative = self.builder.ins().fcmp(FloatCC::LessThan, argument, zero);
                            let rounded_up = self.builder.ins().ceil(argument);
                            let rounded_down = self.builder.ins().floor(argument);
                            self.builder.ins().select(is_negative, rounded_up, rounded_down)
                        }
                        "round" => {
                            let half = self.builder.ins().f64const(0.5);
                            let shifted = self.builder.ins().fadd(argument, half);
                            let rounded = self.builder.ins().floor(shifted);
                            let zero = self.builder.ins().f64const(0.0);
                            let negative_half = self.builder.ins().f64const(-0.5);
                            let negative_zero = self.builder.ins().f64const(-0.0);
                            let is_negative = self.builder.ins().fcmp(FloatCC::LessThan, argument, zero);
                            let at_least_negative_half = self.builder.ins().fcmp(FloatCC::GreaterThanOrEqual, argument, negative_half);
                            let negative_tie = self.builder.ins().select(is_negative, negative_zero, rounded);
                            let rounded = self.builder.ins().select(at_least_negative_half, negative_tie, rounded);
                            let is_zero = self.builder.ins().fcmp(FloatCC::Equal, argument, zero);
                            self.builder.ins().select(is_zero, argument, rounded)
                        }
                        _ => unreachable!("recognized Math builtin"),
                    };
                    return Ok(JitValue::Number(result));
                }
                let Expr::Var(name, ..) = &**callee else {
                    return Err("only statically known numeric calls can be inlined".to_string());
                };
                let inline = self
                    .inline_functions
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("function {name} is not eligible for inlining"))?;
                if arguments.len() != inline.params.len() {
                    return Err("inlined calls must use the exact parameter count".to_string());
                }
                let mut values = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    let JitValue::Number(value) = self.compile_expression(argument)? else {
                        return Err("inlined call arguments must be numeric".to_string());
                    };
                    values.push(value);
                }
                let outer_locals = std::mem::take(&mut self.locals);
                let result = {
                    for (name, value) in inline.params.iter().zip(values) {
                        let variable = self.builder.declare_var(types::F64);
                        self.builder.def_var(variable, value);
                        self.locals.insert(name.clone(), JitLocal::Number(variable));
                    }
                    self.compile_expression(&inline.expression)
                };
                self.locals = outer_locals;
                result
            }
            Expr::Binary(left, op, right) => {
                let JitValue::Number(left) = self.compile_expression(left)? else {
                    return Err("numeric operands required".to_string());
                };
                let JitValue::Number(right) = self.compile_expression(right)? else {
                    return Err("numeric operands required".to_string());
                };
                let value = match op {
                    BinaryOp::Add => JitValue::Number(self.builder.ins().fadd(left, right)),
                    BinaryOp::Sub => JitValue::Number(self.builder.ins().fsub(left, right)),
                    BinaryOp::Mul => JitValue::Number(self.builder.ins().fmul(left, right)),
                    BinaryOp::Div => JitValue::Number(self.builder.ins().fdiv(left, right)),
                    BinaryOp::Mod => {
                        let call = self.builder.ins().call(self.fmod_function, &[left, right]);
                        JitValue::Number(self.builder.inst_results(call)[0])
                    }
                    BinaryOp::Pow => {
                        let call = self.builder.ins().call(self.pow_function, &[left, right]);
                        JitValue::Number(self.builder.inst_results(call)[0])
                    }
                    BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::LeftShift
                    | BinaryOp::RightShift
                    | BinaryOp::UnsignedRightShift => {
                        let operation = match op {
                            BinaryOp::BitAnd => 0,
                            BinaryOp::BitOr => 1,
                            BinaryOp::BitXor => 2,
                            BinaryOp::LeftShift => 3,
                            BinaryOp::RightShift => 4,
                            BinaryOp::UnsignedRightShift => 5,
                            _ => unreachable!("matched bitwise binary operator"),
                        };
                        let operation = self.builder.ins().iconst(types::I32, operation);
                        let call = self.builder.ins().call(self.bitwise_function, &[left, right, operation]);
                        JitValue::Number(self.builder.inst_results(call)[0])
                    }
                    BinaryOp::LessThan => JitValue::Boolean(self.builder.ins().fcmp(FloatCC::LessThan, left, right)),
                    BinaryOp::GreaterThan => JitValue::Boolean(self.builder.ins().fcmp(FloatCC::GreaterThan, left, right)),
                    BinaryOp::LessEqual => JitValue::Boolean(self.builder.ins().fcmp(FloatCC::LessThanOrEqual, left, right)),
                    BinaryOp::GreaterEqual => JitValue::Boolean(self.builder.ins().fcmp(FloatCC::GreaterThanOrEqual, left, right)),
                    BinaryOp::Equal | BinaryOp::StrictEqual => JitValue::Boolean(self.builder.ins().fcmp(FloatCC::Equal, left, right)),
                    BinaryOp::NotEqual | BinaryOp::StrictNotEqual => {
                        JitValue::Boolean(self.builder.ins().fcmp(FloatCC::NotEqual, left, right))
                    }
                    _ => return Err("unsupported numeric operator".to_string()),
                };
                Ok(value)
            }
            Expr::Mod(left, right) => {
                let JitValue::Number(left) = self.compile_expression(left)? else {
                    return Err("numeric operands required".to_string());
                };
                let JitValue::Number(right) = self.compile_expression(right)? else {
                    return Err("numeric operands required".to_string());
                };
                let call = self.builder.ins().call(self.fmod_function, &[left, right]);
                Ok(JitValue::Number(self.builder.inst_results(call)[0]))
            }
            Expr::Pow(left, right) => {
                let JitValue::Number(left) = self.compile_expression(left)? else {
                    return Err("numeric operands required".to_string());
                };
                let JitValue::Number(right) = self.compile_expression(right)? else {
                    return Err("numeric operands required".to_string());
                };
                let call = self.builder.ins().call(self.pow_function, &[left, right]);
                Ok(JitValue::Number(self.builder.inst_results(call)[0]))
            }
            Expr::UnaryNeg(inner) => {
                let JitValue::Number(value) = self.compile_expression(inner)? else {
                    return Err("numeric operand required".to_string());
                };
                Ok(JitValue::Number(self.builder.ins().fneg(value)))
            }
            Expr::UnaryPlus(inner) => self.compile_expression(inner),
            Expr::BitNot(inner) => {
                let JitValue::Number(value) = self.compile_expression(inner)? else {
                    return Err("numeric operand required".to_string());
                };
                let call = self.builder.ins().call(self.bitwise_not_function, &[value]);
                Ok(JitValue::Number(self.builder.inst_results(call)[0]))
            }
            Expr::Assign(left, right) => {
                let target = self.compile_lvalue(left)?;
                let JitValue::Number(value) = self.compile_expression(right)? else {
                    return Err("local assignments must be numeric".to_string());
                };
                self.write_lvalue(&target, value);
                Ok(JitValue::Number(value))
            }
            Expr::AddAssign(left, right)
            | Expr::SubAssign(left, right)
            | Expr::MulAssign(left, right)
            | Expr::DivAssign(left, right)
            | Expr::ModAssign(left, right) => {
                let target = self.compile_lvalue(left)?;
                let current = self.read_lvalue(&target);
                let JitValue::Number(right) = self.compile_expression(right)? else {
                    return Err("compound assignments require numeric operands".to_string());
                };
                let value = match expression {
                    Expr::AddAssign(..) => self.builder.ins().fadd(current, right),
                    Expr::SubAssign(..) => self.builder.ins().fsub(current, right),
                    Expr::MulAssign(..) => self.builder.ins().fmul(current, right),
                    Expr::DivAssign(..) => self.builder.ins().fdiv(current, right),
                    Expr::ModAssign(..) => {
                        let call = self.builder.ins().call(self.fmod_function, &[current, right]);
                        self.builder.inst_results(call)[0]
                    }
                    _ => unreachable!(),
                };
                self.write_lvalue(&target, value);
                Ok(JitValue::Number(value))
            }
            Expr::Increment(inner) | Expr::Decrement(inner) | Expr::PostIncrement(inner) | Expr::PostDecrement(inner) => {
                let Expr::Var(name, ..) = &**inner else {
                    return Err("only local increments are supported".to_string());
                };
                let variable = self.local(name)?;
                let current = self.builder.use_var(variable);
                let one = self.builder.ins().f64const(1.0);
                let is_decrement = matches!(expression, Expr::Decrement(_) | Expr::PostDecrement(_));
                let next = if is_decrement {
                    self.builder.ins().fsub(current, one)
                } else {
                    self.builder.ins().fadd(current, one)
                };
                self.builder.def_var(variable, next);
                if matches!(expression, Expr::PostIncrement(_) | Expr::PostDecrement(_)) {
                    Ok(JitValue::Number(current))
                } else {
                    Ok(JitValue::Number(next))
                }
            }
            _ => Err("unsupported expression".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytecode_lowerer_compiles_and_executes_numeric_addition() {
        let chunk = Chunk {
            code: vec![
                Opcode::GetLocal as u8,
                0,
                Opcode::GetLocal as u8,
                1,
                Opcode::Add as u8,
                Opcode::Return as u8,
            ],
            fn_names: HashMap::from([(0, "sum".to_string())]),
            ..Chunk::default()
        };
        let cfg = bytecode::ControlFlowGraph::build(&chunk).unwrap();
        let candidate = FunctionCandidate {
            ip: 0,
            name: "sum".to_string(),
            params: vec!["left".to_string(), "right".to_string()],
            body: &[],
            dependencies: Vec::new(),
        };
        let mut engine = JitEngine::new().unwrap();
        let (ip, arity, upvalue_count, function_id) = engine.compile_bytecode_function(&candidate, &cfg, &chunk).unwrap().unwrap();
        assert_eq!(ip, 0);
        assert_eq!(arity, 2);
        assert_eq!(upvalue_count, 0);

        engine.module.finalize_definitions().unwrap();
        let address = engine.module.get_finalized_function(function_id);
        let function = unsafe { std::mem::transmute::<*const u8, NativeJitFunction>(address) };
        let mut arguments = [NativeJitValue::number(19.0), NativeJitValue::number(23.0)];
        let (status, result, _) = crate::host_api::invoke_native_jit(function, &mut arguments, 0).unwrap();

        assert_eq!(status, NativeJitStatus::Returned);
        assert_eq!(result.as_number(), Some(42.0));
    }

    #[test]
    fn bytecode_lowerer_compiles_branches_and_merges_stack_values() {
        let chunk = Chunk {
            code: vec![
                Opcode::GetLocal as u8,
                0,
                Opcode::JumpIfFalse as u8,
                15,
                0,
                0,
                0,
                Opcode::Constant as u8,
                0,
                0,
                Opcode::Jump as u8,
                18,
                0,
                0,
                0,
                Opcode::Constant as u8,
                1,
                0,
                Opcode::Return as u8,
            ],
            constants: vec![crate::core::Value::Number(42.0), crate::core::Value::Number(-1.0)],
            fn_names: HashMap::from([(0, "choose".to_string())]),
            ..Chunk::default()
        };
        let cfg = bytecode::ControlFlowGraph::build(&chunk).unwrap();
        let candidate = FunctionCandidate {
            ip: 0,
            name: "choose".to_string(),
            params: vec!["condition".to_string()],
            body: &[],
            dependencies: Vec::new(),
        };
        let mut engine = JitEngine::new().unwrap();
        let (ip, arity, upvalue_count, function_id) = engine.compile_bytecode_function(&candidate, &cfg, &chunk).unwrap().unwrap();
        assert_eq!(ip, 0);
        assert_eq!(arity, 1);
        assert_eq!(upvalue_count, 0);

        engine.module.finalize_definitions().unwrap();
        let address = engine.module.get_finalized_function(function_id);
        let function = unsafe { std::mem::transmute::<*const u8, NativeJitFunction>(address) };
        let cases: [(f64, f64); 3] = [(7.0, 42.0), (0.0, -1.0), (f64::NAN, -1.0)];
        for (argument, expected) in cases {
            let mut arguments = [NativeJitValue::number(argument)];
            let (status, result, _) = crate::host_api::invoke_native_jit(function, &mut arguments, 0).unwrap();
            assert_eq!(status, NativeJitStatus::Returned);
            let actual = result.as_number().unwrap();
            if expected.is_nan() {
                assert!(actual.is_nan());
            } else {
                assert_eq!(actual, expected);
            }
        }
    }
}
