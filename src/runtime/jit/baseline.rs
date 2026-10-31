use crate::core::vm::runner::{
    BaselineBlock, baseline_helper, baseline_numeric_commit, baseline_numeric_prepare, baseline_terminal_helper,
};
use crate::core::{Chunk, Opcode};
use cranelift_codegen::Context;
use cranelift_codegen::ir::{AbiParam, InstBuilder, StackSlotData, StackSlotKind, UserFuncName, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::JITModule;
use cranelift_module::{Linkage, Module};
use std::collections::HashMap;

const MAX_TEMPLATE_OPCODES: usize = 8;
pub(crate) const NATIVE_REGION_THRESHOLD: usize = 32;

pub(crate) mod loops;
pub(crate) mod numeric;

type NumericRegions = Vec<(usize, Box<numeric::Region>)>;
type NumericTemplateKey = (Vec<(Opcode, usize)>, u8);

struct CompiledBlock {
    code: Vec<u8>,
    function: BaselineBlock,
    visits: usize,
    tiered: bool,
}

/// Threaded baseline code: opcode selection happens at compile time, while
/// semantic helpers keep all live JS values in the VM's traced stack and cells.
pub(crate) struct BaselineJit {
    module: JITModule,
    blocks: HashMap<usize, CompiledBlock>,
    ordinary_loops: HashMap<usize, CompiledBlock>,
    templates: HashMap<Vec<(Opcode, usize)>, BaselineBlock>,
    numeric_templates: HashMap<NumericTemplateKey, BaselineBlock>,
    context: Context,
    builder_context: FunctionBuilderContext,
    // Machine code embeds these addresses; the allocations must not move.
    #[allow(clippy::vec_box)]
    numeric_regions: Vec<Box<numeric::Region>>,
    #[allow(clippy::vec_box)]
    numeric_loops: Vec<Box<loops::NumericLoop>>,
    next_id: u32,
}

impl BaselineJit {
    pub(crate) fn new() -> Result<Self, String> {
        let builder = super::native_builder()?;
        let module = JITModule::new(builder);
        let context = module.make_context();
        Ok(Self {
            module,
            blocks: HashMap::new(),
            ordinary_loops: HashMap::new(),
            templates: HashMap::new(),
            numeric_templates: HashMap::new(),
            context,
            builder_context: FunctionBuilderContext::new(),
            numeric_regions: Vec::new(),
            numeric_loops: Vec::new(),
            next_id: 0,
        })
    }

    pub(crate) fn block(&mut self, chunk: &Chunk<'_>, start: usize) -> Result<BaselineBlock, String> {
        let mut tier_up = false;
        if let Some(block) = self.blocks.get_mut(&start)
            && chunk.code.get(start..start + block.code.len()) == Some(block.code.as_slice())
        {
            if block.tiered {
                return Ok(block.function);
            }
            block.visits += 1;
            if block.visits < NATIVE_REGION_THRESHOLD {
                return Ok(block.function);
            }
            tier_up = true;
        }

        if tier_up && let Some(plan) = loops::NumericLoop::build(chunk, start) {
            log::debug!("JIT compiling numeric loop at {start}, locals={:?}", plan.locals);
            let end = start + plan.exit;
            let fallback = self.blocks[&start].function;
            let function = self.compile_loop(plan, Some(fallback))?;
            self.blocks.insert(
                start,
                CompiledBlock {
                    code: chunk.code[start..end].to_vec(),
                    function,
                    visits: 1,
                    tiered: true,
                },
            );
            return Ok(function);
        }

        let mut instructions = Vec::new();
        let mut shape = Vec::new();
        let mut offset = start;
        // Short shapes reduce cold compilation and increase template reuse.
        while offset < chunk.code.len() && instructions.len() < MAX_TEMPLATE_OPCODES {
            let opcode = Opcode::try_from(chunk.code[offset]).map_err(|error| error.to_string())?;
            let Some(helper) = baseline_helper(opcode) else {
                if instructions.is_empty() {
                    return Err(format!("unsupported baseline opcode {opcode:?} at {offset}"));
                }
                break;
            };
            let next = offset + 1 + opcode.operand_len(&chunk.code, offset + 1)?;
            shape.push((opcode, next - offset));
            instructions.push((offset - start, next - start, helper));
            offset = next;
            if matches!(
                opcode,
                Opcode::Return
                    | Opcode::Jump
                    | Opcode::JumpIfFalse
                    | Opcode::JumpIfTrue
                    | Opcode::Call
                    | Opcode::TailCall
                    | Opcode::CallSpread
                    | Opcode::TailCallSpread
                    | Opcode::NewCall
                    | Opcode::NewCallSpread
                    | Opcode::Throw
                    | Opcode::ThrowTypeError
                    | Opcode::Yield
                    | Opcode::YieldDirect
                    | Opcode::GeneratorParamInitDone
                    | Opcode::Await
            ) {
                break;
            }
        }
        let Some(&(last_opcode, _)) = shape.last() else {
            return Err(format!("no baseline instruction at {start}"));
        };

        let mut regions = Vec::new();
        if tier_up {
            let decoded = instructions
                .iter()
                .zip(&shape)
                .map(|(&(offset, next, _), &(opcode, _))| (opcode, offset, next))
                .collect::<Vec<_>>();
            let mut index = 0;
            while index < instructions.len() {
                if let Some(region) = numeric::Region::build(&decoded[index..])
                    && region.constants_are_numeric(chunk, start)
                {
                    let count = region.count;
                    regions.push((index, Box::new(region)));
                    index += count;
                } else {
                    index += 1;
                }
            }
        }
        let terminal_helper =
            baseline_terminal_helper(last_opcode).ok_or_else(|| format!("unsupported baseline terminal opcode {last_opcode:?}"))?;
        let function = if regions.is_empty() {
            if let Some(function) = self.templates.get(&shape) {
                *function
            } else {
                let function = self.compile_template(&instructions, terminal_helper, regions)?;
                self.templates.insert(shape, function);
                function
            }
        } else {
            let mask = regions.iter().fold(0u8, |mask, (index, _)| mask | (1 << index));
            let key = (shape, mask);
            if let Some(function) = self.numeric_templates.get(&key) {
                *function
            } else {
                let function = self.compile_template(&instructions, terminal_helper, regions)?;
                self.numeric_templates.insert(key, function);
                function
            }
        };
        self.blocks.insert(
            start,
            CompiledBlock {
                code: chunk.code[start..offset].to_vec(),
                function,
                visits: 1,
                tiered: tier_up,
            },
        );
        Ok(function)
    }

    pub(crate) fn ordinary_loop(&mut self, chunk: &Chunk<'_>, start: usize) -> Result<Option<BaselineBlock>, String> {
        if let Some(block) = self.ordinary_loops.get(&start)
            && chunk.code.get(start..start + block.code.len()) == Some(block.code.as_slice())
        {
            return Ok(Some(block.function));
        }
        let Some(plan) = loops::NumericLoop::build(chunk, start) else {
            return Ok(None);
        };
        let end = start + plan.exit;
        let function = self.compile_loop(plan, None)?;
        self.ordinary_loops.insert(
            start,
            CompiledBlock {
                code: chunk.code[start..end].to_vec(),
                function,
                visits: 1,
                tiered: true,
            },
        );
        Ok(Some(function))
    }

    fn compile_template(
        &mut self,
        instructions: &[(usize, usize, crate::core::vm::runner::BaselineHelper)],
        terminal_helper: crate::core::vm::runner::BaselineHelper,
        regions: NumericRegions,
    ) -> Result<BaselineBlock, String> {
        let pointer_type = self.module.target_config().pointer_type();
        let mut signature = self.module.make_signature();
        signature.params.extend([AbiParam::new(pointer_type); 2]);
        let name = format!("baseline_{}", self.next_id);
        self.next_id += 1;
        let id = self
            .module
            .declare_function(&name, Linkage::Local, &signature)
            .map_err(|error| error.to_string())?;
        self.module.clear_context(&mut self.context);
        self.context.func.name = UserFuncName::user(0, id.as_u32());
        self.context.func.signature = signature;
        let mut helper_signature = self.module.make_signature();
        helper_signature.params.extend([AbiParam::new(pointer_type); 3]);
        helper_signature.returns.push(AbiParam::new(types::I8));
        let mut numeric_signature = self.module.make_signature();
        numeric_signature.params.extend([AbiParam::new(pointer_type); 4]);
        numeric_signature.returns.push(AbiParam::new(types::I8));
        {
            let mut builder = FunctionBuilder::new(&mut self.context.func, &mut self.builder_context);
            let helper_signature = builder.import_signature(helper_signature);
            let numeric_signature = (!regions.is_empty()).then(|| builder.import_signature(numeric_signature));
            let entry = builder.create_block();
            let exit = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let state = builder.block_params(entry)[0];
            let base = builder.block_params(entry)[1];
            // Operands stay in the current chunk. Only helper selection and
            // relative instruction widths belong to the reusable native code.
            let mut index = 0;
            while index < instructions.len() {
                let region = regions.iter().find(|(start, _)| *start == index).map(|(_, region)| region);
                let count = region.map_or(1, |region| region.count);
                let continuation = builder.create_block();
                if let Some(region) = region {
                    let numeric_signature = numeric_signature.expect("numeric regions have a boundary signature");
                    let slot = builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, region.scratch_bytes(), 3));
                    let scratch = builder.ins().stack_addr(pointer_type, slot, 0);
                    let plan = builder
                        .ins()
                        .iconst(pointer_type, (&**region as *const numeric::Region) as usize as i64);
                    let prepare = builder
                        .ins()
                        .iconst(pointer_type, baseline_numeric_prepare as *const () as usize as i64);
                    let call = builder
                        .ins()
                        .call_indirect(numeric_signature, prepare, &[state, base, plan, scratch]);
                    let prepared = builder.inst_results(call)[0];
                    let native = builder.create_block();
                    let fallback = builder.create_block();
                    builder.ins().brif(prepared, native, &[], fallback, &[]);
                    builder.switch_to_block(native);
                    region.emit(&mut builder, scratch);
                    let commit = builder
                        .ins()
                        .iconst(pointer_type, baseline_numeric_commit as *const () as usize as i64);
                    let call = builder
                        .ins()
                        .call_indirect(numeric_signature, commit, &[state, base, plan, scratch]);
                    let keep_running = builder.inst_results(call)[0];
                    builder.ins().brif(keep_running, continuation, &[], exit, &[]);
                    builder.switch_to_block(fallback);
                }
                for (relative, &(offset, next, helper)) in instructions[index..index + count].iter().enumerate() {
                    let helper = if index + relative + 1 == instructions.len() {
                        terminal_helper
                    } else {
                        helper
                    };
                    let address = builder.ins().iconst(pointer_type, helper as usize as i64);
                    let offset = builder.ins().iadd_imm_u(base, offset as i64);
                    let next = builder.ins().iadd_imm_u(base, next as i64);
                    let call = builder.ins().call_indirect(helper_signature, address, &[state, offset, next]);
                    if index + relative + 1 < instructions.len() {
                        let keep_running = builder.inst_results(call)[0];
                        let next_helper = if relative + 1 == count {
                            continuation
                        } else {
                            builder.create_block()
                        };
                        builder.ins().brif(keep_running, next_helper, &[], exit, &[]);
                        if relative + 1 < count {
                            builder.switch_to_block(next_helper);
                        }
                    }
                }
                if index + count == instructions.len() {
                    if region.is_some() {
                        builder.ins().jump(continuation, &[]);
                        builder.switch_to_block(continuation);
                    }
                    break;
                }
                builder.switch_to_block(continuation);
                index += count;
            }
            builder.ins().jump(exit, &[]);
            builder.switch_to_block(exit);
            builder.ins().return_(&[]);
            builder.seal_all_blocks();
            builder.finalize(self.module.target_config());
        }
        self.module
            .define_function(id, &mut self.context)
            .map_err(|error| error.to_string())?;
        self.module.finalize_definitions().map_err(|error| error.to_string())?;
        self.numeric_regions.extend(regions.into_iter().map(|(_, region)| region));
        let address = self.module.get_finalized_function(id);
        // The module owns this executable allocation for the VM's lifetime.
        let function = unsafe { std::mem::transmute::<*const u8, BaselineBlock>(address) };
        Ok(function)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_region_emits_register_arithmetic_and_only_a_boundary_commit_call() {
        use cranelift_codegen::ir::Opcode as IrOpcode;

        let mut jit = BaselineJit::new().unwrap();
        let mut chunk = Chunk::new();
        chunk.constants.push(crate::core::Value::Number(1.0));
        for opcode in [
            Opcode::Constant,
            Opcode::Constant,
            Opcode::Mul,
            Opcode::Constant,
            Opcode::Add,
            Opcode::SetLocal,
            Opcode::Pop,
            Opcode::Return,
        ] {
            chunk.write_opcode(opcode);
            match opcode {
                Opcode::Constant => chunk.write_u16(0),
                Opcode::SetLocal => chunk.code.push(0),
                _ => {}
            }
        }
        for _ in 0..NATIVE_REGION_THRESHOLD - 1 {
            jit.block(&chunk, 0).unwrap();
        }
        assert!(jit.numeric_regions.is_empty(), "cold paths must not pay for a second compilation");
        assert_eq!(jit.next_id, 1);
        jit.block(&chunk, 0).unwrap();
        assert_eq!(jit.next_id, 2);
        assert_eq!(jit.numeric_regions.len(), 1);
        assert_eq!(jit.numeric_regions[0].count, 7);
        assert!(
            jit.numeric_regions[0].outputs.is_empty(),
            "assignment temporary must never reach the VM stack"
        );
        let function = &jit.context.func;
        let native = function
            .layout
            .blocks()
            .find(|&block| {
                function
                    .layout
                    .block_insts(block)
                    .any(|instruction| function.dfg.insts[instruction].opcode() == IrOpcode::Fmul)
            })
            .expect("numeric arithmetic must be emitted, not executed by an opcode helper");
        let opcodes = function
            .layout
            .block_insts(native)
            .map(|instruction| function.dfg.insts[instruction].opcode())
            .collect::<Vec<_>>();
        assert!(opcodes.contains(&IrOpcode::Fadd));
        assert_eq!(opcodes.iter().filter(|&&opcode| opcode == IrOpcode::CallIndirect).count(), 1);
        assert_eq!(opcodes.iter().filter(|&&opcode| opcode == IrOpcode::Store).count(), 1);
    }

    #[test]
    fn numeric_template_reuse_distinguishes_constant_eligibility() {
        let mut jit = BaselineJit::new().unwrap();
        let mut chunk = Chunk::new();
        chunk
            .constants
            .extend([crate::core::Value::Number(1.0), crate::core::Value::from("1")]);
        for first_constant in [0, 1] {
            let start = chunk.code.len();
            for index in 0..3 {
                chunk.write_opcode(Opcode::Constant);
                chunk.write_u16(if index == 0 { first_constant } else { 0 });
                if index == 1 {
                    chunk.write_opcode(Opcode::Mul);
                }
            }
            chunk.write_opcode(Opcode::Add);
            chunk.write_opcode(Opcode::SetLocal);
            chunk.code.push(0);
            chunk.write_opcode(Opcode::Pop);
            chunk.write_opcode(Opcode::Return);
            for _ in 0..NATIVE_REGION_THRESHOLD {
                jit.block(&chunk, start).unwrap();
            }
        }
        assert_eq!(jit.templates.len(), 1);
        assert_eq!(jit.numeric_templates.len(), 2);
        assert_eq!(jit.numeric_regions.len(), 2);
        assert_eq!(jit.next_id, 3);
    }

    #[test]
    fn templates_bound_cold_compilation_without_truncating_operands() {
        let mut jit = BaselineJit::new().unwrap();
        let mut chunk = Chunk::new();
        for index in 0..MAX_TEMPLATE_OPCODES + 1 {
            chunk.write_opcode(Opcode::Constant);
            chunk.write_u16(index as u16);
        }
        chunk.write_opcode(Opcode::Return);
        jit.block(&chunk, 0).unwrap();
        let end = MAX_TEMPLATE_OPCODES * 3;
        assert_eq!(jit.blocks[&0].code, chunk.code[..end]);
        jit.block(&chunk, end).unwrap();
        assert_eq!(jit.blocks[&end].code, chunk.code[end..]);
    }

    #[test]
    fn single_opcode_templates_do_not_branch_on_unused_helper_result() {
        let mut jit = BaselineJit::new().unwrap();
        let mut chunk = Chunk::new();
        chunk.write_opcode(Opcode::Return);
        jit.block(&chunk, 0).unwrap();
        assert!(
            !jit.context.func.layout.blocks().any(|block| {
                jit.context
                    .func
                    .layout
                    .block_insts(block)
                    .any(|instruction| jit.context.func.dfg.insts[instruction].opcode() == cranelift_codegen::ir::Opcode::Brif)
            }),
            "a final semantic helper always exits the block"
        );
    }

    #[test]
    fn templates_share_code_across_offsets_and_constant_operands() {
        let mut jit = BaselineJit::new().unwrap();
        let mut chunk = Chunk::new();
        for index in 0..400 {
            chunk.write_opcode(Opcode::Constant);
            chunk.write_u16(index);
            chunk.write_opcode(Opcode::Return);
        }
        for start in (0..chunk.code.len()).step_by(4) {
            jit.block(&chunk, start).unwrap();
        }
        assert_eq!(jit.blocks.len(), 400);
        assert_eq!(jit.templates.len(), 1);
        assert_eq!(jit.next_id, 1, "identical shapes must compile only once");
    }

    #[test]
    fn templates_distinguish_opcodes_and_variable_operand_widths() {
        let mut jit = BaselineJit::new().unwrap();
        let mut chunk = Chunk::new();
        chunk.code = vec![Opcode::Call as u8, 0];
        jit.block(&chunk, 0).unwrap();
        chunk.code = vec![Opcode::Call as u8, 0x3f, 64, 0];
        jit.block(&chunk, 0).unwrap();
        chunk.code = vec![Opcode::TailCall as u8, 0];
        jit.block(&chunk, 0).unwrap();
        assert_eq!(jit.templates.len(), 3);

        chunk.code = vec![Opcode::Call as u8, 1];
        jit.block(&chunk, 0).unwrap();
        assert_eq!(jit.templates.len(), 3, "new operands must reuse an existing shape");
        assert_eq!(jit.blocks[&0].code, chunk.code);
    }

    #[test]
    fn suspension_opcodes_end_baseline_blocks() {
        let mut jit = BaselineJit::new().unwrap();
        for opcode in [Opcode::Yield, Opcode::YieldDirect, Opcode::GeneratorParamInitDone, Opcode::Await] {
            let mut chunk = Chunk::new();
            chunk.code = vec![opcode as u8, Opcode::DisposeResources as u8, 0];
            jit.block(&chunk, 0).unwrap();
            assert_eq!(jit.blocks[&0].code, vec![opcode as u8]);
        }
    }

    #[test]
    fn reserved_disposal_opcodes_fail_instead_of_becoming_noops() {
        let mut jit = BaselineJit::new().unwrap();
        for opcode in [Opcode::DisposeResources, Opcode::DisposeResourcesAsync] {
            let mut chunk = Chunk::new();
            chunk.code = vec![opcode as u8, 0];
            let error = jit.block(&chunk, 0).unwrap_err();
            assert!(error.contains(&format!("unsupported baseline opcode {opcode:?}")), "{error}");
        }
    }

    #[test]
    fn truncated_operands_fail_before_native_compilation() {
        let mut jit = BaselineJit::new().unwrap();
        for opcode in [Opcode::GetSuperProperty, Opcode::SetSuperProperty, Opcode::MakeClosure] {
            let mut chunk = Chunk::new();
            chunk.code = vec![opcode as u8];
            let error = jit.block(&chunk, 0).unwrap_err();
            assert!(error.contains("truncated"), "{error}");
        }
    }
}
