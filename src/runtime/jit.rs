use crate::core::{BinaryOp, Chunk, DestructuringElement, Expr, Opcode, Statement, StatementKind};
use crate::host_api::{JitValue as NativeJitValue, JitValueTag, NativeJitFrame, NativeJitFunction, NativeJitRegistration, NativeJitStatus};
use cranelift_codegen::ir::{
    AbiParam, BlockArg, FuncRef, Function as ClifFunction, InstBuilder, MemFlagsData, UserFuncName,
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
    next_function_id: u32,
}

#[derive(Clone)]
struct FunctionCandidate<'a> {
    ip: usize,
    name: String,
    params: Vec<String>,
    body: &'a [Statement],
    dependencies: Vec<(String, usize)>,
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
}

fn bytecode_stack_type(kind: BytecodeStackKind) -> cranelift_codegen::ir::Type {
    match kind {
        BytecodeStackKind::Number => types::F64,
        BytecodeStackKind::Boolean => types::I8,
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
        let mut module = JITModule::new(builder);
        let mut fmod_signature = module.make_signature();
        fmod_signature.params.push(AbiParam::new(types::F64));
        fmod_signature.params.push(AbiParam::new(types::F64));
        fmod_signature.returns.push(AbiParam::new(types::F64));
        let fmod_function = module
            .declare_function("javascript_jit_fmod", Linkage::Import, &fmod_signature)
            .map_err(|error| error.to_string())?;

        Ok(Self {
            module,
            fmod_function,
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

        let declarations = collect_function_declarations(statements);
        let mut candidates = Vec::new();
        let mut name_counts = HashMap::<&str, usize>::new();
        for name in chunk.fn_names.values() {
            *name_counts.entry(name).or_default() += 1;
        }
        let inline_functions = collect_inline_functions(&declarations, chunk, &name_counts);

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
            if params.len() > u8::MAX as usize {
                log::debug!("JIT skipped function {}: too many parameters", name);
                continue;
            }
            if !is_supported_function(&params, body, &inline_functions) {
                log::debug!("JIT skipped function {}: unsupported syntax", name);
                continue;
            }
            let mut dependency_names = HashSet::new();
            for statement in *body {
                collect_inline_dependencies_from_statement(statement, &inline_functions, &mut dependency_names);
            }
            let dependencies = dependency_names
                .into_iter()
                .filter_map(|dependency_name| inline_functions.get(&dependency_name).map(|inline| (dependency_name, inline.ip)))
                .collect();
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
            let bytecode_result = self.compile_bytecode_function(&candidate, &cfg, chunk);
            let compilation = match bytecode_result {
                Ok(Some(function)) => Ok((function, true)),
                Ok(None) => self
                    .compile_function(&candidate, &inline_functions)
                    .map(|function| (function, false)),
                Err(error) => Err(error),
            };
            match compilation {
                Ok((function, from_bytecode)) => {
                    let source = if from_bytecode { "bytecode" } else { "AST" };
                    log::debug!(
                        "JIT compiled function {} from {source} at bytecode offset {}",
                        candidate.name,
                        candidate.ip
                    );
                    compiled.push((function, candidate.dependencies));
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
            .map(|((ip, arity, function_id), dependencies)| {
                let address = self.module.get_finalized_function(function_id);
                let function = unsafe { std::mem::transmute::<*const u8, NativeJitFunction>(address) };
                NativeJitRegistration {
                    ip,
                    arity,
                    function,
                    dependencies,
                }
            })
            .collect()
    }

    fn compile_bytecode_function(
        &mut self,
        candidate: &FunctionCandidate<'_>,
        cfg: &bytecode::ControlFlowGraph,
        chunk: &Chunk<'_>,
    ) -> Result<Option<(usize, usize, FuncId)>, String> {
        let Some(entry_index) = cfg.blocks.iter().position(|block| block.start == candidate.ip) else {
            return Ok(None);
        };

        let block_indices = cfg
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (block.start, index))
            .collect::<HashMap<_, _>>();
        let other_function_entries = chunk
            .fn_names
            .keys()
            .copied()
            .filter(|entry| *entry != candidate.ip)
            .collect::<HashSet<_>>();
        let mut incoming_stacks = vec![None::<Vec<BytecodeStackKind>>; cfg.blocks.len()];
        let mut worklist = VecDeque::from([entry_index]);
        let mut reachable_blocks = Vec::new();
        let mut has_return = false;
        incoming_stacks[entry_index] = Some(Vec::new());

        while let Some(block_index) = worklist.pop_front() {
            let block = &cfg.blocks[block_index];
            if !block.exceptional_successors.is_empty() {
                return Ok(None);
            }
            reachable_blocks.push(block_index);
            let mut stack = incoming_stacks[block_index]
                .clone()
                .expect("reachable blocks have an incoming stack");
            let instructions = &cfg.instructions[block.instruction_range.clone()];
            for instruction in instructions {
                if !instruction.exceptional_targets.is_empty() {
                    return Ok(None);
                }
                match instruction.opcode {
                    Opcode::Constant => {
                        let constant_index =
                            u16::from_le_bytes([chunk.code[instruction.operand.start], chunk.code[instruction.operand.start + 1]]) as usize;
                        if !matches!(chunk.constants.get(constant_index), Some(crate::core::Value::Number(_))) {
                            return Ok(None);
                        }
                        stack.push(BytecodeStackKind::Number);
                    }
                    Opcode::GetLocal => {
                        let local_index = chunk.code[instruction.operand.start] as usize;
                        if local_index >= candidate.params.len() {
                            return Ok(None);
                        }
                        stack.push(BytecodeStackKind::Number);
                    }
                    Opcode::SetLocal => {
                        let local_index = chunk.code[instruction.operand.start] as usize;
                        if local_index >= candidate.params.len() || stack.last() != Some(&BytecodeStackKind::Number) {
                            return Ok(None);
                        }
                    }
                    Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Mod => {
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
                    | Opcode::StrictNotEqual => {
                        if stack.pop() != Some(BytecodeStackKind::Number) || stack.pop() != Some(BytecodeStackKind::Number) {
                            return Ok(None);
                        }
                        stack.push(BytecodeStackKind::Boolean);
                    }
                    Opcode::Negate => {
                        if stack.last() != Some(&BytecodeStackKind::Number) {
                            return Ok(None);
                        }
                    }
                    Opcode::Pop => {
                        if stack.pop().is_none() {
                            return Ok(None);
                        }
                    }
                    Opcode::JumpIfFalse | Opcode::JumpIfTrue => {
                        if stack.pop().is_none() {
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
                match &incoming_stacks[target_index] {
                    Some(existing) if existing != &stack => return Ok(None),
                    Some(_) => {}
                    None => {
                        incoming_stacks[target_index] = Some(stack.clone());
                        worklist.push_back(target_index);
                    }
                }
            }
        }

        if !has_return {
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
            let mut locals = Vec::with_capacity(candidate.params.len());
            for index in 0..candidate.params.len() {
                let argument = builder.ins().load(
                    types::F64,
                    MemFlagsData::new(),
                    arguments_pointer,
                    (index * std::mem::size_of::<NativeJitValue>() + std::mem::offset_of!(NativeJitValue, payload)) as i32,
                );
                let local = builder.declare_var(types::F64);
                builder.def_var(local, argument);
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
                for instruction in instructions {
                    match instruction.opcode {
                        Opcode::Constant => {
                            let constant_index =
                                u16::from_le_bytes([chunk.code[instruction.operand.start], chunk.code[instruction.operand.start + 1]])
                                    as usize;
                            let crate::core::Value::Number(number) = chunk.constants[constant_index] else {
                                unreachable!("bytecode numeric constants were validated");
                            };
                            stack.push(builder.ins().f64const(number));
                            stack_kinds.push(BytecodeStackKind::Number);
                        }
                        Opcode::GetLocal => {
                            let local_index = chunk.code[instruction.operand.start] as usize;
                            stack.push(builder.use_var(locals[local_index]));
                            stack_kinds.push(BytecodeStackKind::Number);
                        }
                        Opcode::SetLocal => {
                            let local_index = chunk.code[instruction.operand.start] as usize;
                            builder.def_var(locals[local_index], *stack.last().expect("bytecode stack was validated"));
                        }
                        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Mod => {
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
                                _ => unreachable!("matched numeric binary opcode"),
                            };
                            stack.push(result);
                            stack_kinds.push(BytecodeStackKind::Number);
                        }
                        Opcode::LessThan
                        | Opcode::GreaterThan
                        | Opcode::LessEqual
                        | Opcode::GreaterEqual
                        | Opcode::Equal
                        | Opcode::NotEqual
                        | Opcode::StrictNotEqual => {
                            let right = stack.pop().expect("bytecode stack was validated");
                            let left = stack.pop().expect("bytecode stack was validated");
                            let condition = match instruction.opcode {
                                Opcode::LessThan => FloatCC::LessThan,
                                Opcode::GreaterThan => FloatCC::GreaterThan,
                                Opcode::LessEqual => FloatCC::LessThanOrEqual,
                                Opcode::GreaterEqual => FloatCC::GreaterThanOrEqual,
                                Opcode::Equal => FloatCC::Equal,
                                Opcode::NotEqual | Opcode::StrictNotEqual => FloatCC::NotEqual,
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
                        Opcode::Negate => {
                            let value = stack.pop().expect("bytecode stack was validated");
                            stack.push(builder.ins().fneg(value));
                            let _ = stack_kinds.pop().expect("bytecode stack kinds were validated");
                            stack_kinds.push(BytecodeStackKind::Number);
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
                        _ => unreachable!("bytecode instruction set was validated"),
                    }
                }
                let last = instructions.last().expect("basic blocks contain instructions");
                if !matches!(
                    last.opcode,
                    Opcode::Jump | Opcode::JumpIfFalse | Opcode::JumpIfTrue | Opcode::Return
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

        Ok(Some((candidate.ip, candidate.params.len(), function_id)))
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

fn collect_function_declarations(statements: &[Statement]) -> HashMap<&str, (&[DestructuringElement], &[Statement])> {
    let mut declarations = HashMap::new();
    for statement in statements {
        let declaration = match &*statement.kind {
            StatementKind::FunctionDeclaration(name, params, body, false, false) => Some((name, params, body)),
            StatementKind::Export(_, Some(inner), _) => match &*inner.kind {
                StatementKind::FunctionDeclaration(name, params, body, false, false) => Some((name, params, body)),
                _ => None,
            },
            _ => None,
        };
        if let Some((name, params, body)) = declaration {
            declarations.insert(name.as_str(), (params.as_slice(), body.as_slice()));
        }
    }
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
        Expr::Binary(left, op, right) => {
            matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod)
                && supports_inline_numeric_expression(left, analysis)
                && supports_inline_numeric_expression(right, analysis)
        }
        Expr::Mod(left, right) => supports_inline_numeric_expression(left, analysis) && supports_inline_numeric_expression(right, analysis),
        Expr::UnaryNeg(inner) | Expr::UnaryPlus(inner) => supports_inline_numeric_expression(inner, analysis),
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
                    | BinaryOp::LessThan
                    | BinaryOp::GreaterThan
                    | BinaryOp::LessEqual
                    | BinaryOp::GreaterEqual
                    | BinaryOp::Equal
                    | BinaryOp::StrictEqual
                    | BinaryOp::NotEqual
                    | BinaryOp::StrictNotEqual
            ) && supports_expression(left, analysis)
                && supports_expression(right, analysis)
        }
        Expr::Mod(left, right) => supports_expression(left, analysis) && supports_expression(right, analysis),
        Expr::UnaryNeg(inner) | Expr::UnaryPlus(inner) => supports_expression(inner, analysis),
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
            Expr::UnaryNeg(inner) => {
                let JitValue::Number(value) = self.compile_expression(inner)? else {
                    return Err("numeric operand required".to_string());
                };
                Ok(JitValue::Number(self.builder.ins().fneg(value)))
            }
            Expr::UnaryPlus(inner) => self.compile_expression(inner),
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
        let (ip, arity, function_id) = engine.compile_bytecode_function(&candidate, &cfg, &chunk).unwrap().unwrap();
        assert_eq!(ip, 0);
        assert_eq!(arity, 2);

        engine.module.finalize_definitions().unwrap();
        let address = engine.module.get_finalized_function(function_id);
        let function = unsafe { std::mem::transmute::<*const u8, NativeJitFunction>(address) };
        let arguments = [NativeJitValue::number(19.0), NativeJitValue::number(23.0)];
        let (status, result, _) = crate::host_api::invoke_native_jit(function, &arguments, 0).unwrap();

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
        let (ip, arity, function_id) = engine.compile_bytecode_function(&candidate, &cfg, &chunk).unwrap().unwrap();
        assert_eq!(ip, 0);
        assert_eq!(arity, 1);

        engine.module.finalize_definitions().unwrap();
        let address = engine.module.get_finalized_function(function_id);
        let function = unsafe { std::mem::transmute::<*const u8, NativeJitFunction>(address) };
        let cases: [(f64, f64); 3] = [(7.0, 42.0), (0.0, -1.0), (f64::NAN, -1.0)];
        for (argument, expected) in cases {
            let arguments = [NativeJitValue::number(argument)];
            let (status, result, _) = crate::host_api::invoke_native_jit(function, &arguments, 0).unwrap();
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
