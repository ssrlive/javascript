use crate::core::{Chunk, Opcode, OpcodeOperandLayout};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::ops::Range;

#[derive(Debug)]
pub(crate) struct DecodedInstruction {
    pub(crate) offset: usize,
    pub(crate) next_offset: usize,
    pub(crate) opcode: Opcode,
    pub(crate) operand: Range<usize>,
    branch_target: Option<usize>,
    // Conservative handler edges; opcode-specific throwability is not classified yet.
    pub(crate) exceptional_targets: Vec<usize>,
}

#[derive(Debug)]
pub(crate) struct BasicBlock {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) instruction_range: Range<usize>,
    pub(crate) successors: Vec<usize>,
    pub(crate) exceptional_successors: Vec<usize>,
}

#[derive(Debug)]
pub(crate) struct ExceptionHandler {
    pub(crate) setup_offset: usize,
    pub(crate) target: usize,
    for_finally: bool,
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
                        for_finally: code[operand.start + 6] & 1 != 0,
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
                exceptional_targets: Vec::new(),
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
                exceptional_successors: Vec::new(),
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

        build_exception_edges(
            &mut instructions,
            &mut blocks,
            &handlers,
            &block_by_start,
            chunk.fn_names.keys().copied(),
        )?;

        Ok(Self {
            instructions,
            blocks,
            exception_handlers: handlers,
        })
    }
}

fn build_exception_edges(
    instructions: &mut [DecodedInstruction],
    blocks: &mut [BasicBlock],
    handlers: &[ExceptionHandler],
    block_by_start: &HashMap<usize, usize>,
    function_entries: impl IntoIterator<Item = usize>,
) -> Result<(), String> {
    let handler_by_setup = handlers
        .iter()
        .enumerate()
        .map(|(index, handler)| (handler.setup_offset, index))
        .collect::<HashMap<_, _>>();
    let mut entry_handlers = vec![None::<Vec<usize>>; blocks.len()];
    let mut worklist = VecDeque::new();

    seed_handler_state(block_by_start[&0], Vec::new(), &mut entry_handlers, &mut worklist, blocks)?;
    for entry in function_entries {
        let block = *block_by_start
            .get(&entry)
            .ok_or_else(|| format!("function entry {entry} is not a basic block"))?;
        seed_handler_state(block, Vec::new(), &mut entry_handlers, &mut worklist, blocks)?;
    }

    while let Some(block_index) = worklist.pop_front() {
        let mut active_handlers = entry_handlers[block_index]
            .clone()
            .expect("queued blocks have an incoming exception-handler state");
        let instruction_range = blocks[block_index].instruction_range.clone();

        for instruction_index in instruction_range {
            let opcode = instructions[instruction_index].opcode;
            if !matches!(opcode, Opcode::SetupTry | Opcode::TeardownTry)
                && let Some(&top_handler) = active_handlers.last()
            {
                let mut handler_positions = vec![active_handlers.len() - 1];
                if !handlers[top_handler].for_finally
                    && let Some(outer_finally) = active_handlers[..active_handlers.len() - 1]
                        .iter()
                        .rposition(|&handler| handlers[handler].for_finally)
                {
                    handler_positions.push(outer_finally);
                }

                for handler_position in handler_positions {
                    let handler = &handlers[active_handlers[handler_position]];
                    instructions[instruction_index].exceptional_targets.push(handler.target);
                    let target_block = block_by_start[&handler.target];
                    seed_handler_state(
                        target_block,
                        active_handlers[..handler_position].to_vec(),
                        &mut entry_handlers,
                        &mut worklist,
                        blocks,
                    )?;
                }
                instructions[instruction_index].exceptional_targets.sort_unstable();
                instructions[instruction_index].exceptional_targets.dedup();
                blocks[block_index]
                    .exceptional_successors
                    .extend(instructions[instruction_index].exceptional_targets.iter().copied());
            }

            match opcode {
                Opcode::SetupTry => {
                    let handler = *handler_by_setup
                        .get(&instructions[instruction_index].offset)
                        .ok_or_else(|| format!("SetupTry at {} has no handler record", instructions[instruction_index].offset))?;
                    active_handlers.push(handler);
                }
                Opcode::TeardownTry => match active_handlers.pop() {
                    Some(_) => {}
                    None => {
                        return Err(format!(
                            "TeardownTry at {} has no active SetupTry",
                            instructions[instruction_index].offset
                        ));
                    }
                },
                _ => {}
            }
        }

        let successors = blocks[block_index].successors.clone();
        for successor in successors {
            let target_block = block_by_start[&successor];
            seed_handler_state(target_block, active_handlers.clone(), &mut entry_handlers, &mut worklist, blocks)?;
        }
        blocks[block_index].exceptional_successors.sort_unstable();
        blocks[block_index].exceptional_successors.dedup();
    }

    Ok(())
}

fn seed_handler_state(
    block_index: usize,
    handlers: Vec<usize>,
    entry_handlers: &mut [Option<Vec<usize>>],
    worklist: &mut VecDeque<usize>,
    blocks: &[BasicBlock],
) -> Result<(), String> {
    match &entry_handlers[block_index] {
        Some(existing) if *existing != handlers => Err(format!(
            "inconsistent active exception handlers at control-flow merge into bytecode offset {}",
            blocks[block_index].start
        )),
        Some(_) => Ok(()),
        None => {
            entry_handlers[block_index] = Some(handlers);
            worklist.push_back(block_index);
            Ok(())
        }
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
    fn connects_throwing_instructions_to_active_handlers() {
        let graph = ControlFlowGraph::build(&chunk(vec![
            Opcode::SetupTry as u8,
            12,
            0,
            0,
            0,
            0xff,
            0xff,
            0,
            Opcode::Constant as u8,
            0,
            0,
            Opcode::TeardownTry as u8,
            Opcode::Return as u8,
        ]))
        .unwrap();

        assert_eq!(graph.instructions[1].exceptional_targets, vec![12]);
        assert_eq!(graph.blocks[0].exceptional_successors, vec![12]);
    }

    #[test]
    fn includes_enclosing_finally_handlers_for_generator_return_paths() {
        let graph = ControlFlowGraph::build(&chunk(vec![
            Opcode::SetupTry as u8,
            22,
            0,
            0,
            0,
            0xff,
            0xff,
            1,
            Opcode::SetupTry as u8,
            24,
            0,
            0,
            0,
            0xff,
            0xff,
            0,
            Opcode::Constant as u8,
            0,
            0,
            Opcode::TeardownTry as u8,
            Opcode::TeardownTry as u8,
            Opcode::Return as u8,
            Opcode::Return as u8,
            Opcode::Return as u8,
            Opcode::TeardownTry as u8,
            Opcode::Return as u8,
        ]))
        .unwrap();

        assert_eq!(graph.instructions[2].exceptional_targets, vec![22, 24]);
    }

    #[test]
    fn rejects_inconsistent_handler_state_at_control_flow_merges() {
        let graph = ControlFlowGraph::build(&chunk(vec![
            Opcode::SetupTry as u8,
            16,
            0,
            0,
            0,
            0xff,
            0xff,
            0,
            Opcode::JumpIfFalse as u8,
            14,
            0,
            0,
            0,
            Opcode::TeardownTry as u8,
            Opcode::Return as u8,
            Opcode::Pop as u8,
            Opcode::Return as u8,
        ]));

        assert!(graph.unwrap_err().contains("inconsistent active exception handlers"));
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
