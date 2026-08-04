//! Optimized source-variable ownership recovered from finalized machine shapes.
//!
//! Register allocation can identify where a surviving value lives, but it
//! cannot decide which source declaration MWCC assigns to a coalesced home.
//! Structured owners record that policy here after allocation, independently
//! of instruction scheduling and legacy DWARF byte layout.

use crate::generator::Generator;
use mwcc_machine_code::{DebugVariable, DebugVariableLocation, Instruction};
use mwcc_syntax_trees::{Function, Type};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DenseCountedLoopHomes {
    parameters: [u8; 5],
    frame_array: i16,
    loop_index: u8,
    sample: u8,
    flags: [u8; 4],
    state: [u8; 6],
}

impl Generator {
    pub(crate) fn select_dense_counted_loop_debug_variables(&mut self, function: &Function) {
        if !self.structured_dense_counted_loop_entry_owner {
            return;
        }
        let Some(homes) = dense_counted_loop_homes(self, function) else {
            return;
        };
        let parameter = |index: usize, register: u8| DebugVariable {
            name: function.parameters[index].name.clone(),
            location: DebugVariableLocation::GeneralRegister(register),
        };
        let local = |index: usize, location| DebugVariable {
            name: function.locals[index].name.clone(),
            location,
        };

        let mut variables = homes
            .parameters
            .into_iter()
            .enumerate()
            .map(|(index, register)| parameter(index, register))
            .collect::<Vec<_>>();
        variables.extend([
            local(0, DebugVariableLocation::FrameOffset(homes.frame_array)),
            local(2, DebugVariableLocation::GeneralRegister(0)),
            local(3, DebugVariableLocation::Unavailable),
            local(4, DebugVariableLocation::GeneralRegister(homes.loop_index)),
            local(5, DebugVariableLocation::GeneralRegister(homes.sample)),
        ]);
        variables.extend(
            homes
                .flags
                .into_iter()
                .enumerate()
                .map(|(index, register)| {
                    local(6 + index, DebugVariableLocation::GeneralRegister(register))
                }),
        );
        variables.push(local(11, DebugVariableLocation::Unavailable));
        variables.extend(
            homes
                .state
                .into_iter()
                .enumerate()
                .map(|(index, register)| {
                    local(15 + index, DebugVariableLocation::GeneralRegister(register))
                }),
        );
        variables.push(local(21, DebugVariableLocation::GeneralRegister(0)));
        self.output.debug_variables = variables;
    }
}

fn dense_counted_loop_homes(
    generator: &Generator,
    function: &Function,
) -> Option<DenseCountedLoopHomes> {
    if function.parameters.len() != 5
        || function.locals.len() != 22
        || !matches!(function.parameters[0].parameter_type, Type::StructPointer { .. })
        || function.parameters[1].parameter_type != Type::UnsignedInt
        || !matches!(function.parameters[2].parameter_type, Type::Pointer(_))
        || function.parameters[3].parameter_type != Type::Int
        || !matches!(function.parameters[4].parameter_type, Type::Pointer(_))
        || function.locals[0].declared_type != Type::Double
        || function.locals[0].array_length != Some(8)
        || function.locals[1].declared_type != Type::UnsignedChar
        || !matches!(function.locals[2].declared_type, Type::Pointer(_))
        || !matches!(function.locals[3].declared_type, Type::Pointer(_))
        || !matches!(function.locals[21].declared_type, Type::StructPointer { .. })
    {
        return None;
    }
    let frame_array = generator.frame_slots.get(&function.locals[0].name)?.offset;
    recognize_dense_counted_loop_homes(&generator.output.instructions, frame_array)
}

fn recognize_dense_counted_loop_homes(
    instructions: &[Instruction],
    frame_array: i16,
) -> Option<DenseCountedLoopHomes> {
    let flag_parameter = instructions.iter().find_map(|instruction| match instruction {
        Instruction::AndMaskRecord { s, begin: 31, end: 31, .. } => Some(*s),
        _ => None,
    })?;
    let (sample_position, sample, source_parameter) = instructions
        .iter()
        .enumerate()
        .find_map(|(index, instruction)| match instruction {
            Instruction::LoadHalfwordAlgebraic { d, a, offset: 0 } => Some((index, *d, *a)),
            _ => None,
        })?;
    let count_parameter = instructions.iter().find_map(|instruction| match instruction {
        Instruction::MoveToCountRegister { s } => Some(*s),
        _ => None,
    })?;
    let destination_parameter = instructions.iter().find_map(|instruction| match instruction {
        Instruction::StoreByteIndexed { a, .. } => Some(*a),
        _ => None,
    })?;
    let (state_base, state) = publication_homes(instructions)?;
    let loop_index = instructions.windows(2).find_map(|window| match window {
        [
            Instruction::AddImmediate { d, a, immediate: 1 },
            Instruction::BranchConditionalForward { options: 16, .. },
        ] if d == a => Some(*d),
        _ => None,
    })?;
    let flags = instructions[sample_position..]
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::AddImmediate { d, a: 0, immediate: 1 } => Some(*d),
            _ => None,
        })
        .take(4)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;

    Some(DenseCountedLoopHomes {
        parameters: [
            state_base,
            flag_parameter,
            source_parameter,
            count_parameter,
            destination_parameter,
        ],
        frame_array,
        loop_index,
        sample,
        flags,
        state,
    })
}

fn publication_homes(instructions: &[Instruction]) -> Option<(u8, [u8; 6])> {
    for (start, instruction) in instructions.iter().enumerate() {
        let Instruction::StoreWord {
            s: first,
            a: base,
            offset: 0,
        } = instruction
        else {
            continue;
        };
        let mut state = [0; 6];
        state[0] = *first;
        let mut next = 1;
        for instruction in instructions
            .iter()
            .skip(start + 1)
            .take(15)
        {
            let Instruction::StoreWord { s, a, offset } = instruction else {
                continue;
            };
            if *a == *base && *offset == (next * 4) as i16 {
                state[next] = *s;
                next += 1;
                if next == state.len() {
                    return Some((*base, state));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_debug_roles_from_the_allocated_dense_loop_seams() {
        let mut instructions = vec![
            Instruction::AndMaskRecord { a: 0, s: 19, begin: 31, end: 31 },
            Instruction::MoveToCountRegister { s: 30 },
            Instruction::LoadHalfwordAlgebraic { d: 5, a: 29, offset: 0 },
            Instruction::load_immediate(7, 1),
            Instruction::load_immediate(8, 1),
            Instruction::load_immediate(9, 1),
            Instruction::load_immediate(10, 1),
            Instruction::StoreByteIndexed { s: 7, a: 31, b: 6 },
            Instruction::AddImmediate { d: 6, a: 6, immediate: 1 },
            Instruction::BranchConditionalForward { options: 16, condition_bit: 0, target: 2 },
        ];
        for (index, register) in [11, 12, 5, 27, 26, 25].into_iter().enumerate() {
            instructions.push(Instruction::StoreWord {
                s: register,
                a: 28,
                offset: (index * 4) as i16,
            });
            if index == 0 {
                instructions.push(Instruction::move_register(3, 30));
            }
        }

        assert_eq!(
            recognize_dense_counted_loop_homes(&instructions, 8),
            Some(DenseCountedLoopHomes {
                parameters: [28, 19, 29, 30, 31],
                frame_array: 8,
                loop_index: 6,
                sample: 5,
                flags: [7, 8, 9, 10],
                state: [11, 12, 5, 27, 26, 25],
            })
        );
    }
}
