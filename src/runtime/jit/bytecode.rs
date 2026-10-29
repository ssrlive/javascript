use crate::core::{Chunk, Opcode, OpcodeOperandLayout};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;

#[derive(Debug)]
pub(crate) struct DecodedInstruction {
    pub(crate) offset: usize,
    pub(crate) next_offset: usize,
    pub(crate) opcode: Opcode,
    pub(crate) operand: Range<usize>,
    branch_target: Option<usize>,
}

#[derive(Debug)]
pub(crate) struct BasicBlock {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) instruction_range: Range<usize>,
    pub(crate) successors: Vec<usize>,
}

#[derive(Debug)]
pub(crate) struct ExceptionHandler {
    pub(crate) setup_offset: usize,
    pub(crate) target: usize,
}

#[derive(Debug)]
pub(crate) struct ControlFlowGraph {
    pub(crate) instructions: Vec<DecodedInstruction>,
    pub(crate) blocks: Vec<BasicBlock>,
    pub(crate) exception_handlers: Vec<ExceptionHandler>,
}

impl ControlFlowGraph {
    pub(crate) fn build(chunk: &Chunk<'_>) -> Result<Self, String> {
        let code = &chunk.code;
        if code.is_empty() {
            return Ok(Self {
                instructions: Vec::new(),
                blocks: Vec::new(),
                exception_handlers: Vec::new(),
            });
        }

        let mut instructions = Vec::new();
        let mut handlers = Vec::new();
        let mut leaders = BTreeSet::from([0]);
        let mut offset = 0;
        while offset < code.len() {
            let opcode = Opcode::try_from(code[offset]).map_err(|error| format!("{error} at bytecode offset {offset}"))?;
            let operand_start = offset + 1;
            let operand_len = opcode
                .operand_len(code, operand_start)
                .map_err(|error| format!("{error} at bytecode offset {offset}"))?;
            let next_offset = operand_start + operand_len;
            let operand = operand_start..next_offset;
            let branch_target = match opcode.operand_layout() {
                OpcodeOperandLayout::JumpTarget => Some(read_u32(code, operand.start)?),
                OpcodeOperandLayout::SetupTry => {
                    let target = read_u32(code, operand.start)?;
                    handlers.push(ExceptionHandler {
                        setup_offset: offset,
                        target,
                    });
                    Some(target)
                }
                _ => None,
            };
            if let Some(target) = branch_target {
                if target >= code.len() {
                    return Err(format!("branch target {target} at {offset} is outside bytecode"));
                }
                leaders.insert(target);
            }
            if matches!(
                opcode,
                Opcode::Jump
                    | Opcode::JumpIfFalse
                    | Opcode::JumpIfTrue
                    | Opcode::Return
                    | Opcode::Throw
                    | Opcode::ThrowTypeError
                    | Opcode::TailCall
                    | Opcode::TailCallSpread
            ) && next_offset < code.len()
            {
                leaders.insert(next_offset);
            }
            instructions.push(DecodedInstruction {
                offset,
                next_offset,
                opcode,
                operand,
                branch_target,
            });
            offset = next_offset;
        }

        let instruction_offsets = instructions.iter().map(|instruction| instruction.offset).collect::<HashSet<_>>();
        for target in leaders.iter().copied().chain(chunk.fn_names.keys().copied()) {
            if target < code.len() && !instruction_offsets.contains(&target) {
                return Err(format!("control-flow target {target} is not an instruction boundary"));
            }
        }
        for entry in chunk.fn_names.keys() {
            if *entry >= code.len() || !instruction_offsets.contains(entry) {
                return Err(format!("function entry {entry} is not an instruction boundary"));
            }
            leaders.insert(*entry);
        }

        let block_starts = leaders.into_iter().collect::<Vec<_>>();
        let mut block_by_start = HashMap::new();
        let mut blocks = Vec::with_capacity(block_starts.len());
        for (index, start) in block_starts.iter().copied().enumerate() {
            let end = block_starts.get(index + 1).copied().unwrap_or(code.len());
            let instruction_start = instructions.partition_point(|instruction| instruction.offset < start);
            let instruction_end = instructions.partition_point(|instruction| instruction.offset < end);
            if instruction_start == instruction_end {
                return Err(format!("empty basic block at bytecode offset {start}"));
            }
            block_by_start.insert(start, index);
            blocks.push(BasicBlock {
                start,
                end,
                instruction_range: instruction_start..instruction_end,
                successors: Vec::new(),
            });
        }

        for block in &mut blocks {
            let last = &instructions[block.instruction_range.end - 1];
            if last.next_offset != block.end {
                return Err(format!("basic block at {} does not end on an instruction boundary", block.start));
            }
            match last.opcode {
                Opcode::Jump => block.successors.push(required_target(last)?),
                Opcode::JumpIfFalse | Opcode::JumpIfTrue => {
                    block.successors.push(required_target(last)?);
                    if last.next_offset < code.len() {
                        block.successors.push(last.next_offset);
                    }
                }
                Opcode::Return | Opcode::Throw | Opcode::ThrowTypeError | Opcode::TailCall | Opcode::TailCallSpread => {}
                _ if last.next_offset < code.len() => block.successors.push(last.next_offset),
                _ => {}
            }
            block.successors.sort_unstable();
            block.successors.dedup();
            if block.successors.iter().any(|target| !block_by_start.contains_key(target)) {
                return Err(format!("basic block at {} has a successor that is not a block leader", block.start));
            }
        }

        for handler in &handlers {
            if !block_by_start.contains_key(&handler.target) {
                return Err(format!(
                    "exception handler at {} targets non-block offset {}",
                    handler.setup_offset, handler.target
                ));
            }
        }

        Ok(Self {
            instructions,
            blocks,
            exception_handlers: handlers,
        })
    }
}

fn read_u32(code: &[u8], offset: usize) -> Result<usize, String> {
    let bytes = code
        .get(offset..offset + 4)
        .ok_or_else(|| format!("truncated u32 operand at bytecode offset {offset}"))?;
    let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    Ok(value as usize)
}

fn required_target(instruction: &DecodedInstruction) -> Result<usize, String> {
    instruction
        .branch_target
        .ok_or_else(|| format!("{:?} at {} has no branch target", instruction.opcode, instruction.offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(code: Vec<u8>) -> Chunk<'static> {
        Chunk { code, ..Chunk::default() }
    }

    #[test]
    fn builds_blocks_and_edges_for_conditional_jumps() {
        let graph = ControlFlowGraph::build(&chunk(vec![
            Opcode::JumpIfFalse as u8,
            8,
            0,
            0,
            0,
            Opcode::Constant as u8,
            0,
            0,
            Opcode::Return as u8,
        ]))
        .unwrap();

        assert_eq!(graph.blocks.len(), 3);
        assert_eq!(graph.blocks[0].successors, vec![5, 8]);
        assert_eq!(graph.blocks[1].successors, vec![8]);
        assert!(graph.blocks[2].successors.is_empty());
        assert_eq!(graph.instructions[0].operand, 1..5);
    }

    #[test]
    fn decodes_variable_length_call_and_closure_operands() {
        let graph = ControlFlowGraph::build(&chunk(vec![
            Opcode::Call as u8,
            0x3f,
            2,
            1,
            Opcode::MakeClosure as u8,
            0,
            0,
            1,
            1,
            3,
            Opcode::Return as u8,
        ]))
        .unwrap();

        assert_eq!(graph.instructions[0].operand, 1..4);
        assert_eq!(graph.instructions[1].operand, 5..10);
    }

    #[test]
    fn rejects_truncated_operands_and_non_boundary_jump_targets() {
        assert!(ControlFlowGraph::build(&chunk(vec![Opcode::Jump as u8, 0, 0])).is_err());
        assert!(ControlFlowGraph::build(&chunk(vec![Opcode::Jump as u8, 2, 0, 0, 0, Opcode::Return as u8])).is_err());
    }

    #[test]
    fn records_setup_try_handler_edges_separately() {
        let graph = ControlFlowGraph::build(&chunk(vec![
            Opcode::SetupTry as u8,
            8,
            0,
            0,
            0,
            0,
            0,
            0,
            Opcode::TeardownTry as u8,
            Opcode::Return as u8,
        ]))
        .unwrap();

        assert_eq!(graph.exception_handlers.len(), 1);
        assert_eq!(graph.exception_handlers[0].target, 8);
    }

    #[test]
    fn every_opcode_declares_a_decodable_operand_layout() {
        for byte in 0..=121 {
            let opcode = Opcode::try_from(byte).unwrap();
            let operands = match opcode.operand_layout() {
                OpcodeOperandLayout::None => Vec::new(),
                OpcodeOperandLayout::U8 => vec![0],
                OpcodeOperandLayout::ConstantIndex => vec![0, 0],
                OpcodeOperandLayout::JumpTarget => vec![0, 0, 0, 0],
                OpcodeOperandLayout::CallArguments => vec![0x3f, 0, 0],
                OpcodeOperandLayout::SetupTry => vec![0; 7],
                OpcodeOperandLayout::ClosureCaptures => vec![0, 0, 1, 0, 0],
            };
            assert_eq!(opcode.operand_len(&operands, 0).unwrap(), operands.len(), "{opcode:?}");
        }
    }

    #[test]
    fn merged_bytecode_adjustment_uses_the_shared_operand_metadata() {
        let mut code = vec![Opcode::Constant as u8, 1, 0, Opcode::Jump as u8, 8, 0, 0, 0, Opcode::Return as u8];

        Chunk::adjust_bytecode_offsets(&mut code, 10, 2);

        assert_eq!(&code[1..3], &[3, 0]);
        assert_eq!(&code[4..8], &[18, 0, 0, 0]);
    }
}
