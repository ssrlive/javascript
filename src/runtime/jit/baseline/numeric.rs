use crate::core::{Chunk, Opcode, Value as JsValue};
use cranelift_codegen::ir::{InstBuilder, MemFlagsData, Value, condcodes::FloatCC, types};
use cranelift_frontend::FunctionBuilder;

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
