use crate::core::vm::runner::{BaselineBlock, baseline_helper};
use crate::core::{Chunk, Opcode};
use cranelift_codegen::ir::{AbiParam, InstBuilder, UserFuncName, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::JITModule;
use cranelift_module::{Linkage, Module};
use std::collections::HashMap;

struct CompiledBlock {
    code: Vec<u8>,
    function: BaselineBlock,
}

/// Threaded baseline code: opcode selection happens at compile time, while
/// semantic helpers keep all live JS values in the VM's traced stack and cells.
pub(crate) struct BaselineJit {
    module: JITModule,
    blocks: HashMap<usize, CompiledBlock>,
    next_id: u32,
}

impl BaselineJit {
    pub(crate) fn new() -> Result<Self, String> {
        let builder = super::native_builder()?;
        Ok(Self {
            module: JITModule::new(builder),
            blocks: HashMap::new(),
            next_id: 0,
        })
    }

    pub(crate) fn block(&mut self, chunk: &Chunk<'_>, start: usize) -> Result<BaselineBlock, String> {
        if let Some(block) = self.blocks.get(&start)
            && chunk.code.get(start..start + block.code.len()) == Some(block.code.as_slice())
        {
            return Ok(block.function);
        }

        let mut instructions = Vec::new();
        let mut offset = start;
        // Bound compilation latency, including when a GC slice resumes midway
        // through a previously compiled block.
        while offset < chunk.code.len() && instructions.len() < 32 {
            let opcode = Opcode::try_from(chunk.code[offset]).map_err(|error| error.to_string())?;
            let Some(helper) = baseline_helper(opcode) else {
                if instructions.is_empty() {
                    return Err(format!("unsupported baseline opcode {opcode:?} at {offset}"));
                }
                break;
            };
            let next = offset + 1 + opcode.operand_len(&chunk.code, offset + 1)?;
            instructions.push((offset, next, helper));
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
        if instructions.is_empty() {
            return Err(format!("no baseline instruction at {start}"));
        }

        let pointer_type = self.module.target_config().pointer_type();
        let mut signature = self.module.make_signature();
        signature.params.push(AbiParam::new(pointer_type));
        let name = format!("baseline_{}", self.next_id);
        self.next_id += 1;
        let id = self
            .module
            .declare_function(&name, Linkage::Local, &signature)
            .map_err(|error| error.to_string())?;
        let mut context = self.module.make_context();
        context.func.name = UserFuncName::user(0, id.as_u32());
        context.func.signature = signature;
        let mut helper_signature = self.module.make_signature();
        helper_signature.params.extend([AbiParam::new(pointer_type); 3]);
        helper_signature.returns.push(AbiParam::new(types::I8));
        let mut builder_context = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
            let helper_signature = builder.import_signature(helper_signature);
            let entry = builder.create_block();
            let exit = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let state = builder.block_params(entry)[0];
            for (offset, next, helper) in instructions {
                let address = builder.ins().iconst(pointer_type, helper as usize as i64);
                let offset = builder.ins().iconst(pointer_type, offset as i64);
                let next = builder.ins().iconst(pointer_type, next as i64);
                let call = builder.ins().call_indirect(helper_signature, address, &[state, offset, next]);
                let keep_running = builder.inst_results(call)[0];
                let continuation = builder.create_block();
                builder.ins().brif(keep_running, continuation, &[], exit, &[]);
                builder.switch_to_block(continuation);
            }
            builder.ins().jump(exit, &[]);
            builder.switch_to_block(exit);
            builder.ins().return_(&[]);
            builder.seal_all_blocks();
            builder.finalize(self.module.target_config());
        }
        self.module.define_function(id, &mut context).map_err(|error| error.to_string())?;
        self.module.finalize_definitions().map_err(|error| error.to_string())?;
        let address = self.module.get_finalized_function(id);
        // The module owns this executable allocation for the VM's lifetime.
        let function = unsafe { std::mem::transmute::<*const u8, BaselineBlock>(address) };
        self.blocks.insert(
            start,
            CompiledBlock {
                code: chunk.code[start..offset].to_vec(),
                function,
            },
        );
        Ok(function)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
