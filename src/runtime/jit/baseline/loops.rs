use super::{BaselineBlock, BaselineJit};
use crate::core::vm::runner::{baseline_loop_commit, baseline_loop_prepare};
use crate::core::{Chunk, Opcode, Value as JsValue};
use cranelift_codegen::ir::{
    AbiParam, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, UserFuncName, Value,
    condcodes::{FloatCC, IntCC},
    types,
};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::{Linkage, Module};
use std::collections::BTreeSet;

struct Instruction {
    opcode: Opcode,
    operand: usize,
}

pub(crate) struct NumericLoop {
    instructions: Vec<Instruction>,
    pub(crate) locals: Vec<usize>,
    pub(crate) written: BTreeSet<usize>,
    pub(crate) reboxed: BTreeSet<usize>,
    pub(crate) constants: Vec<usize>,
    pub(crate) header_count: usize,
    pub(crate) exit: usize,
    pub(crate) condition: usize,
    pub(crate) backedge: usize,
}

impl NumericLoop {
    pub(crate) fn build(chunk: &Chunk<'_>, start: usize) -> Option<Self> {
        let mut plan = Self {
            instructions: Vec::new(),
            locals: Vec::new(),
            written: BTreeSet::new(),
            reboxed: BTreeSet::new(),
            constants: Vec::new(),
            header_count: 0,
            exit: 0,
            condition: 0,
            backedge: 0,
        };
        let mut locals = BTreeSet::new();
        let mut stack = Vec::<bool>::new();
        let mut offset = start;
        // Only balanced, single-backedge loops without calls or internal branches.
        for _ in 0..96 {
            let opcode = Opcode::try_from(*chunk.code.get(offset)?).ok()?;
            let next = offset + 1 + opcode.operand_len(&chunk.code, offset + 1).ok()?;
            let mut operand = 0;
            match opcode {
                Opcode::Constant => {
                    let index = u16::from_le_bytes(chunk.code[offset + 1..next].try_into().ok()?) as usize;
                    if !matches!(chunk.constants.get(index), Some(JsValue::Number(_))) {
                        return None;
                    }
                    operand = plan.constants.len();
                    plan.constants.push(offset + 1 - start);
                    stack.push(false);
                }
                Opcode::GetLocal | Opcode::SetLocal | Opcode::ReboxLocal => {
                    operand = chunk.code[offset + 1] as usize;
                    locals.insert(operand);
                    if opcode == Opcode::GetLocal {
                        stack.push(false);
                    } else if opcode == Opcode::SetLocal {
                        if *stack.last()? {
                            return None;
                        }
                        plan.written.insert(operand);
                    } else {
                        plan.reboxed.insert(operand);
                    }
                }
                Opcode::Add
                | Opcode::Sub
                | Opcode::Mul
                | Opcode::Div
                | Opcode::LessThan
                | Opcode::GreaterThan
                | Opcode::LessEqual
                | Opcode::GreaterEqual
                | Opcode::Equal
                | Opcode::StrictEqual
                | Opcode::NotEqual => {
                    if stack.pop()? || stack.pop()? {
                        return None;
                    }
                    stack.push(!matches!(opcode, Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div));
                }
                Opcode::Negate | Opcode::Increment | Opcode::Decrement | Opcode::ToNumber | Opcode::ToNumeric => {
                    if *stack.last()? {
                        return None;
                    }
                }
                Opcode::Dup => stack.push(*stack.last()?),
                Opcode::Swap => {
                    let len = stack.len();
                    if len < 2 {
                        return None;
                    }
                    stack.swap(len - 1, len - 2);
                }
                Opcode::Pop => {
                    stack.pop()?;
                }
                Opcode::JumpIfFalse if plan.header_count == 0 => {
                    stack.pop()?;
                    if !stack.is_empty() || !plan.written.is_empty() {
                        return None;
                    }
                    let target = u32::from_le_bytes(chunk.code[offset + 1..next].try_into().ok()?) as usize;
                    if target <= next || target > chunk.code.len() {
                        return None;
                    }
                    plan.exit = target - start;
                    plan.condition = offset - start;
                    plan.header_count = plan.instructions.len() + 1;
                }
                Opcode::Jump if plan.header_count != 0 && stack.is_empty() => {
                    let target = u32::from_le_bytes(chunk.code[offset + 1..next].try_into().ok()?) as usize;
                    if target != start || next - start != plan.exit || plan.written.is_empty() {
                        return None;
                    }
                    plan.backedge = offset - start;
                    plan.instructions.push(Instruction { opcode, operand });
                    plan.locals = locals.into_iter().collect();
                    return Some(plan);
                }
                _ => return None,
            }
            plan.instructions.push(Instruction { opcode, operand });
            offset = next;
        }
        None
    }

    pub(crate) fn count(&self) -> usize {
        self.instructions.len()
    }

    pub(crate) fn metadata(&self) -> usize {
        self.locals.len() + self.constants.len()
    }

    fn emit(&self, builder: &mut FunctionBuilder<'_>, scratch: Value) {
        let pointer_type = builder.func.dfg.value_type(scratch);
        let mut locals = Vec::new();
        for (index, _) in self.locals.iter().enumerate() {
            let variable = builder.declare_var(types::F64);
            let value = builder.ins().load(types::F64, MemFlagsData::trusted(), scratch, (index * 8) as i32);
            builder.def_var(variable, value);
            locals.push(variable);
        }
        let constants = self
            .constants
            .iter()
            .enumerate()
            .map(|(index, _)| {
                builder.ins().load(
                    types::F64,
                    MemFlagsData::trusted(),
                    scratch,
                    ((self.locals.len() + index) * 8) as i32,
                )
            })
            .collect::<Vec<_>>();
        let remaining = builder.declare_var(pointer_type);
        let quota = builder
            .ins()
            .load(pointer_type, MemFlagsData::trusted(), scratch, (self.metadata() * 8) as i32);
        builder.def_var(remaining, quota);
        let executed = builder.declare_var(pointer_type);
        let zero = builder.ins().iconst(pointer_type, 0);
        builder.def_var(executed, zero);
        let header = builder.create_block();
        let test = builder.create_block();
        let body = builder.create_block();
        let exhausted = builder.create_block();
        let complete = builder.create_block();
        let finish = builder.create_block();
        builder.append_block_param(finish, pointer_type);
        builder.ins().jump(header, &[]);
        builder.switch_to_block(header);
        let quota = builder.use_var(remaining);
        let available = builder.ins().icmp_imm_u(IntCC::NotEqual, quota, 0);
        builder.ins().brif(available, test, &[], exhausted, &[]);
        builder.switch_to_block(test);
        let mut stack = Vec::<(Value, bool)>::new();
        for instruction in &self.instructions {
            let local = |slot: usize| -> Variable { locals[self.locals.binary_search(&slot).expect("decoded local is registered")] };
            match instruction.opcode {
                Opcode::Constant => stack.push((constants[instruction.operand], false)),
                Opcode::GetLocal => stack.push((builder.use_var(local(instruction.operand)), false)),
                Opcode::SetLocal => builder.def_var(local(instruction.operand), stack.last().expect("verified stack").0),
                Opcode::ReboxLocal | Opcode::ToNumber | Opcode::ToNumeric => {}
                Opcode::Pop => {
                    stack.pop();
                }
                Opcode::Dup => stack.push(*stack.last().expect("verified stack")),
                Opcode::Swap => {
                    let len = stack.len();
                    stack.swap(len - 1, len - 2);
                }
                Opcode::Negate | Opcode::Increment | Opcode::Decrement => {
                    let (value, _) = stack.pop().expect("verified operand");
                    let result = if instruction.opcode == Opcode::Negate {
                        builder.ins().fneg(value)
                    } else {
                        let one = builder.ins().f64const(1.0);
                        if instruction.opcode == Opcode::Increment {
                            builder.ins().fadd(value, one)
                        } else {
                            builder.ins().fsub(value, one)
                        }
                    };
                    stack.push((result, false));
                }
                Opcode::JumpIfFalse => {
                    let (value, boolean) = stack.pop().expect("verified condition");
                    let condition = if boolean {
                        value
                    } else {
                        let zero = builder.ins().f64const(0.0);
                        builder.ins().fcmp(FloatCC::OrderedNotEqual, value, zero)
                    };
                    builder.ins().brif(condition, body, &[], complete, &[]);
                    builder.switch_to_block(body);
                }
                Opcode::Jump => {
                    let count = builder.use_var(executed);
                    let count = builder.ins().iadd_imm_u(count, self.count() as i64);
                    builder.def_var(executed, count);
                    let quota = builder.use_var(remaining);
                    let quota = builder.ins().iadd_imm_u(quota, -1);
                    builder.def_var(remaining, quota);
                    builder.ins().jump(header, &[]);
                }
                opcode => {
                    let right = stack.pop().expect("verified rhs").0;
                    let left = stack.pop().expect("verified lhs").0;
                    let result = match opcode {
                        Opcode::Add => (builder.ins().fadd(left, right), false),
                        Opcode::Sub => (builder.ins().fsub(left, right), false),
                        Opcode::Mul => (builder.ins().fmul(left, right), false),
                        Opcode::Div => (builder.ins().fdiv(left, right), false),
                        _ => {
                            let condition = match opcode {
                                Opcode::LessThan => FloatCC::LessThan,
                                Opcode::GreaterThan => FloatCC::GreaterThan,
                                Opcode::LessEqual => FloatCC::LessThanOrEqual,
                                Opcode::GreaterEqual => FloatCC::GreaterThanOrEqual,
                                Opcode::Equal | Opcode::StrictEqual => FloatCC::Equal,
                                Opcode::NotEqual => FloatCC::NotEqual,
                                _ => unreachable!("validated numeric loop opcode"),
                            };
                            (builder.ins().fcmp(condition, left, right), true)
                        }
                    };
                    stack.push(result);
                }
            }
        }
        builder.switch_to_block(exhausted);
        builder.ins().jump(finish, &[zero.into()]);
        builder.switch_to_block(complete);
        let count = builder.use_var(executed);
        let count = builder.ins().iadd_imm_u(count, self.header_count as i64);
        builder.def_var(executed, count);
        let one = builder.ins().iconst(pointer_type, 1);
        builder.ins().jump(finish, &[one.into()]);
        builder.switch_to_block(finish);
        let complete = builder.block_params(finish)[0];
        for (index, slot) in self.locals.iter().enumerate() {
            if self.written.contains(slot) {
                let value = builder.use_var(locals[index]);
                builder.ins().store(MemFlagsData::trusted(), value, scratch, (index * 8) as i32);
            }
        }
        let count = builder.use_var(executed);
        builder
            .ins()
            .store(MemFlagsData::trusted(), count, scratch, (self.metadata() * 8) as i32);
        builder
            .ins()
            .store(MemFlagsData::trusted(), complete, scratch, ((self.metadata() + 1) * 8) as i32);
    }
}

impl BaselineJit {
    pub(super) fn compile_loop(&mut self, plan: NumericLoop, fallback: Option<BaselineBlock>) -> Result<BaselineBlock, String> {
        let plan = Box::new(plan);
        let pointer_type = self.module.target_config().pointer_type();
        let mut signature = self.module.make_signature();
        signature.params.extend([AbiParam::new(pointer_type); 2]);
        let id = self
            .module
            .declare_function(&format!("baseline_loop_{}", self.next_id), Linkage::Local, &signature)
            .map_err(|error| error.to_string())?;
        self.next_id += 1;
        self.module.clear_context(&mut self.context);
        self.context.func.name = UserFuncName::user(0, id.as_u32());
        self.context.func.signature = signature.clone();
        let mut boundary_signature = self.module.make_signature();
        boundary_signature.params.extend([AbiParam::new(pointer_type); 4]);
        boundary_signature.returns.push(AbiParam::new(types::I8));
        {
            let mut builder = FunctionBuilder::new(&mut self.context.func, &mut self.builder_context);
            let boundary_signature = builder.import_signature(boundary_signature);
            let fallback_signature = builder.import_signature(signature);
            let entry = builder.create_block();
            let native = builder.create_block();
            let slow = builder.create_block();
            let finish = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let state = builder.block_params(entry)[0];
            let base = builder.block_params(entry)[1];
            let slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                ((plan.metadata() + 2) * 8) as u32,
                3,
            ));
            let scratch = builder.ins().stack_addr(pointer_type, slot, 0);
            let address = builder.ins().iconst(pointer_type, (&*plan as *const NumericLoop) as usize as i64);
            let helper = builder
                .ins()
                .iconst(pointer_type, baseline_loop_prepare as *const () as usize as i64);
            let call = builder
                .ins()
                .call_indirect(boundary_signature, helper, &[state, base, address, scratch]);
            let prepared = builder.inst_results(call)[0];
            builder.ins().brif(prepared, native, &[], slow, &[]);
            builder.switch_to_block(native);
            plan.emit(&mut builder, scratch);
            let helper = builder
                .ins()
                .iconst(pointer_type, baseline_loop_commit as *const () as usize as i64);
            builder
                .ins()
                .call_indirect(boundary_signature, helper, &[state, base, address, scratch]);
            builder.ins().jump(finish, &[]);
            builder.switch_to_block(slow);
            if let Some(fallback) = fallback {
                let fallback = builder.ins().iconst(pointer_type, fallback as usize as i64);
                builder.ins().call_indirect(fallback_signature, fallback, &[state, base]);
            }
            builder.ins().jump(finish, &[]);
            builder.switch_to_block(finish);
            builder.ins().return_(&[]);
            builder.seal_all_blocks();
            builder.finalize(self.module.target_config());
        }

        self.module
            .define_function(id, &mut self.context)
            .map_err(|error| error.to_string())?;
        self.module.finalize_definitions().map_err(|error| error.to_string())?;
        self.numeric_loops.push(plan);
        let address = self.module.get_finalized_function(id);
        // Executable code and its stable plan allocation are owned by this JIT.
        Ok(unsafe { std::mem::transmute::<*const u8, BaselineBlock>(address) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_generated_numeric_function_loop_is_recognized() {
        let mut runtime = crate::runtime::JsRuntime::new();
        runtime
            .start_eval(
                "function compute() { Symbol(); let total = 0; \
             for (let i = 0; i < 1500; i++) total = total + i * 2 - i / 2; \
             return total; } compute();",
            )
            .unwrap();
        runtime.mutate_root(|_, vm| {
            let chunk = &vm.chunk;
            let mut offset = 0;
            while offset < chunk.code.len() {
                let opcode = Opcode::try_from(chunk.code[offset]).unwrap();
                let next = offset + 1 + opcode.operand_len(&chunk.code, offset + 1).unwrap();
                if opcode == Opcode::Jump {
                    let target = u32::from_le_bytes(chunk.code[offset + 1..next].try_into().unwrap()) as usize;
                    if target < offset {
                        let mut decoded = Vec::new();
                        let mut ip = target;
                        while ip <= offset {
                            let opcode = Opcode::try_from(chunk.code[ip]).unwrap();
                            decoded.push(opcode);
                            ip += 1 + opcode.operand_len(&chunk.code, ip + 1).unwrap();
                        }
                        assert!(NumericLoop::build(chunk, target).is_some(), "{decoded:?}");
                        let mut jit = BaselineJit::new().unwrap();
                        for _ in 0..super::super::NATIVE_REGION_THRESHOLD {
                            jit.block(chunk, target).unwrap();
                        }
                        assert_eq!(jit.numeric_loops.len(), 1);
                        use cranelift_codegen::ir::Opcode as IrOpcode;
                        let function = &jit.context.func;
                        let body = function
                            .layout
                            .blocks()
                            .find(|&block| {
                                function
                                    .layout
                                    .block_insts(block)
                                    .any(|instruction| function.dfg.insts[instruction].opcode() == IrOpcode::Fmul)
                            })
                            .expect("loop body must contain native arithmetic");
                        assert!(
                            function.layout.block_insts(body).all(|instruction| {
                                !matches!(
                                    function.dfg.insts[instruction].opcode(),
                                    IrOpcode::Call | IrOpcode::CallIndirect | IrOpcode::Load | IrOpcode::Store
                                )
                            }),
                            "loop body must have no helper calls or VM/scratch memory traffic"
                        );
                        assert!(
                            function.layout.blocks().any(|block| function.dfg.block_params(block).len() >= 3),
                            "loop-carried locals must use SSA block parameters"
                        );
                        return;
                    }
                }
                offset = next;
            }
            panic!("expected numeric loop backedge");
        });
    }
}
