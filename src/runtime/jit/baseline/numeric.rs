use crate::core::{Chunk, Opcode, Value as JsValue};
use cranelift_codegen::ir::{
    InstBuilder, MemFlagsData, Value,
    condcodes::{FloatCC, IntCC},
    types,
};
use cranelift_frontend::FunctionBuilder;

pub(super) fn to_uint32(builder: &mut FunctionBuilder<'_>, value: Value) -> Value {
    // Extract the truncated integer modulo 2^32, including huge and nonfinite
    // inputs, without trapping conversions or a floating-point remainder call.
    let bits = builder.ins().bitcast(types::I64, MemFlagsData::new(), value);
    let exponent = builder.ins().ushr_imm_u(bits, 52);
    let exponent = builder.ins().band_imm_u(exponent, 2047);
    let fraction = builder.ins().band_imm_u(bits, 0x000f_ffff_ffff_ffff);
    let mantissa = builder.ins().bor_imm_u(fraction, 1i64 << 52);
    let midpoint = builder.ins().iconst(types::I64, 1075);
    let right_shift = builder.ins().isub(midpoint, exponent);
    let right = builder.ins().ushr(mantissa, right_shift);
    let left_shift = builder.ins().iadd_imm_s(exponent, -1075);
    let left = builder.ins().ishl(mantissa, left_shift);
    let small = builder.ins().icmp_imm_u(IntCC::UnsignedLessThanOrEqual, exponent, 1075);
    let integer = builder.ins().select(small, right, left);
    let lower = builder.ins().icmp_imm_u(IntCC::UnsignedGreaterThanOrEqual, exponent, 1023);
    let upper = builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, exponent, 1107);
    let valid = builder.ins().band(lower, upper);
    let zero = builder.ins().iconst(types::I64, 0);
    let integer = builder.ins().select(valid, integer, zero);
    let negative = builder.ins().icmp_imm_s(IntCC::SignedLessThan, bits, 0);
    let negated = builder.ins().ineg(integer);
    let integer = builder.ins().select(negative, negated, integer);
    builder.ins().ireduce(types::I32, integer)
}

pub(crate) enum Input {
    Stack(usize),
    Constant(usize),
    Local(usize),
}

enum Operation {
    Input(usize),
    Binary(Opcode, usize, usize),
    Negate(usize),
}

pub(crate) struct Region {
    pub(crate) inputs: Vec<Input>,
    operations: Vec<Operation>,
    pub(crate) outputs: Vec<usize>,
    pub(crate) store: Option<(usize, usize)>,
    pub(crate) consumed: usize,
    pub(crate) count: usize,
    pub(crate) next: usize,
    pub(crate) last: usize,
    booleans: Vec<bool>,
}

impl Region {
    pub(crate) fn constants_are_numeric(&self, chunk: &Chunk<'_>, base: usize) -> bool {
        self.inputs.iter().all(|input| {
            let Input::Constant(operand) = *input else {
                return true;
            };
            let operand = base + operand;
            let index = u16::from_le_bytes([chunk.code[operand], chunk.code[operand + 1]]) as usize;
            matches!(chunk.constants.get(index), Some(JsValue::Number(_)))
        })
    }

    pub(crate) fn build(instructions: &[(Opcode, usize, usize)]) -> Option<Self> {
        let mut region = Self {
            inputs: Vec::new(),
            operations: Vec::new(),
            outputs: Vec::new(),
            store: None,
            consumed: 0,
            count: 0,
            next: 0,
            last: 0,
            booleans: Vec::new(),
        };
        let mut arithmetic = false;
        for &(opcode, offset, next) in instructions {
            if region.store.is_some() && opcode != Opcode::Pop {
                break;
            }
            match opcode {
                Opcode::Constant | Opcode::GetLocal => {
                    let input = if opcode == Opcode::Constant {
                        Input::Constant(offset + 1)
                    } else {
                        Input::Local(offset + 1)
                    };
                    let value = region.input(input);
                    region.outputs.push(value);
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
                    let right = region.pop();
                    let left = region.pop();
                    let value = region.operations.len();
                    region.operations.push(Operation::Binary(opcode, left, right));
                    region
                        .booleans
                        .push(!matches!(opcode, Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div));
                    region.outputs.push(value);
                    arithmetic = true;
                }
                Opcode::Negate => {
                    let operand = region.pop();
                    let value = region.operations.len();
                    region.operations.push(Operation::Negate(operand));
                    region.booleans.push(false);
                    region.outputs.push(value);
                    arithmetic = true;
                }
                Opcode::Dup => {
                    let value = region.pop();
                    region.outputs.extend([value, value]);
                }
                Opcode::SetLocal if !region.outputs.is_empty() => {
                    region.store = Some((offset + 1, *region.outputs.last()?));
                }
                Opcode::Pop if region.store.is_some() => {
                    region.outputs.pop();
                }
                _ => break,
            }
            region.count += 1;
            region.next = next;
            region.last = offset;
            if region.booleans.last() == Some(&true) || opcode == Opcode::Pop {
                break;
            }
        }

        (arithmetic && region.count >= 4).then_some(region)
    }

    fn input(&mut self, input: Input) -> usize {
        let value = self.operations.len();
        self.operations.push(Operation::Input(self.inputs.len()));
        self.booleans.push(false);
        self.inputs.push(input);
        value
    }

    fn pop(&mut self) -> usize {
        self.outputs.pop().unwrap_or_else(|| {
            let depth = self.consumed;
            self.consumed += 1;
            self.input(Input::Stack(depth))
        })
    }

    pub(crate) fn is_boolean(&self, value: usize) -> bool {
        self.booleans[value]
    }

    pub(crate) fn scratch_bytes(&self) -> u32 {
        ((self.inputs.len() + self.outputs.len() + usize::from(self.store.is_some())) * 8) as u32
    }

    pub(crate) fn emit(&self, builder: &mut FunctionBuilder<'_>, scratch: Value) {
        let mut values = Vec::with_capacity(self.operations.len());
        for operation in &self.operations {
            let value = match *operation {
                Operation::Input(index) => builder.ins().load(types::F64, MemFlagsData::trusted(), scratch, (index * 8) as i32),
                Operation::Negate(operand) => builder.ins().fneg(values[operand]),
                Operation::Binary(opcode, left, right) => {
                    let (left, right) = (values[left], values[right]);
                    match opcode {
                        Opcode::Add => builder.ins().fadd(left, right),
                        Opcode::Sub => builder.ins().fsub(left, right),
                        Opcode::Mul => builder.ins().fmul(left, right),
                        Opcode::Div => builder.ins().fdiv(left, right),
                        _ => {
                            let condition = match opcode {
                                Opcode::LessThan => FloatCC::LessThan,
                                Opcode::GreaterThan => FloatCC::GreaterThan,
                                Opcode::LessEqual => FloatCC::LessThanOrEqual,
                                Opcode::GreaterEqual => FloatCC::GreaterThanOrEqual,
                                Opcode::Equal | Opcode::StrictEqual => FloatCC::Equal,
                                Opcode::NotEqual => FloatCC::NotEqual,
                                _ => unreachable!("region builder only accepts numeric comparisons"),
                            };
                            let comparison = builder.ins().fcmp(condition, left, right);
                            builder.ins().fcvt_from_uint(types::F64, comparison)
                        }
                    }
                }
            };
            values.push(value);
        }
        for (index, &value) in self.outputs.iter().chain(self.store.as_ref().map(|(_, value)| value)).enumerate() {
            builder.ins().store(
                MemFlagsData::trusted(),
                values[value],
                scratch,
                ((self.inputs.len() + index) * 8) as i32,
            );
        }
    }
}

#[cfg(test)]
mod conversion_tests {
    use super::*;
    use cranelift_codegen::ir::AbiParam;
    use cranelift_frontend::FunctionBuilderContext;
    use cranelift_jit::JITModule;
    use cranelift_module::{Linkage, Module};

    #[test]
    fn native_uint32_conversion_matches_ieee_boundaries_and_random_bits() {
        let mut module = JITModule::new(crate::runtime::jit::native_builder().unwrap());
        let mut context = module.make_context();
        context.func.signature.params.push(AbiParam::new(types::F64));
        context.func.signature.returns.push(AbiParam::new(types::I32));
        let id = module
            .declare_function("uint32_conversion", Linkage::Local, &context.func.signature)
            .unwrap();
        let mut builder_context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let block = builder.create_block();
        builder.append_block_params_for_function_params(block);
        builder.switch_to_block(block);
        let input = builder.block_params(block)[0];
        let result = to_uint32(&mut builder, input);
        builder.ins().return_(&[result]);
        builder.seal_all_blocks();
        builder.finalize(module.target_config());
        module.define_function(id, &mut context).unwrap();
        module.finalize_definitions().unwrap();
        let function: unsafe extern "C" fn(f64) -> u32 = unsafe { std::mem::transmute(module.get_finalized_function(id)) };
        for exponent in 0..=2047u64 {
            for fraction in [0, 1, 0x0008_0000_0000_0000, 0x000f_ffff_ffff_ffff] {
                for sign in [0, 1u64 << 63] {
                    let value = f64::from_bits(sign | (exponent << 52) | fraction);
                    assert_eq!(unsafe { function(value) }, crate::core::vm::to_uint32(value), "{value:?}");
                }
            }
        }
        let mut bits = 17u64;
        for _ in 0..100000 {
            bits ^= bits << 13;
            bits ^= bits >> 7;
            bits ^= bits << 17;
            let value = f64::from_bits(bits);
            assert_eq!(unsafe { function(value) }, crate::core::vm::to_uint32(value), "{value:?}");
        }
    }
}
