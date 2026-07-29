//! Physical linkage for a guarded mutating inline in a scratch-framed caller.
//!
//! Virtual scheduling establishes value ownership. After allocation, build
//! 163's saved-register packet still needs canonical descending slots and
//! restore order.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_guarded_mutating_inline_linkage(&mut self, function: &Function) {
        if !function.locals.iter().any(|local| {
            local.array_length.is_some()
                && !super::structured_locals::body_uses_local(&function.statements, &local.name)
        }) || !has_physical_guarded_mutating_inline(&self.output.instructions)
        {
            return;
        }
        let Some(frame_bytes) =
            self.output
                .instructions
                .iter()
                .find_map(|instruction| match instruction {
                    Instruction::StoreWordWithUpdate { s: 1, a: 1, offset } if *offset < 0 => {
                        Some(-*offset)
                    }
                    _ => None,
                })
        else {
            return;
        };
        let Some(first_call) = self
            .output
            .instructions
            .iter()
            .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        else {
            return;
        };
        let Some(last_call) = self
            .output
            .instructions
            .iter()
            .rposition(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        else {
            return;
        };

        let saved = saved_register_indices(&self.output.instructions[..first_call], true);
        let restored = saved_register_indices(&self.output.instructions[last_call + 1..], false)
            .into_iter()
            .map(|index| index + last_call + 1)
            .collect::<Vec<_>>();
        if saved.len() != 3
            || restored.len() != 3
            || !same_saved_register_set(&self.output.instructions, &saved, &restored)
        {
            return;
        }

        for &index in &saved {
            let Instruction::StoreWord { s, offset, .. } = &mut self.output.instructions[index]
            else {
                unreachable!("saved register index changed form")
            };
            *offset = canonical_saved_offset(frame_bytes, *s);
        }

        let mut loads = restored
            .iter()
            .map(|&index| self.output.instructions[index].clone())
            .collect::<Vec<_>>();
        loads.sort_by_key(|instruction| match instruction {
            Instruction::LoadWord { d, .. } => std::cmp::Reverse(*d),
            _ => unreachable!("restored register index changed form"),
        });
        for (&index, mut instruction) in restored.iter().zip(loads) {
            let Instruction::LoadWord { d, offset, .. } = &mut instruction else {
                unreachable!("restored register changed form")
            };
            *offset = canonical_saved_offset(frame_bytes, *d);
            self.output.instructions[index] = instruction;
        }
    }
}

fn has_physical_guarded_mutating_inline(instructions: &[Instruction]) -> bool {
    instructions.windows(17).any(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord { d: 4, a: 29, .. },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0,
                },
                Instruction::LoadWord { d: 31, a: 4, .. },
                Instruction::AddImmediate {
                    d: 3,
                    a: 4,
                    immediate: 0,
                },
                Instruction::StoreWord { s: 0, a: 4, .. },
                Instruction::StoreWord { s: 0, a: 4, .. },
                Instruction::StoreWord { s: 0, a: 4, .. },
                Instruction::StoreWord { s: 0, a: 4, .. },
                Instruction::BranchAndLink { .. },
                Instruction::LoadFloatSingle { d: 1, .. },
                Instruction::Or { a: 3, s: 29, b: 29 },
                Instruction::LoadFloatSingle { d: 2, a: 31, .. },
                Instruction::Or { a: 4, s: 30, b: 30 },
                Instruction::FloatMove { d: 3, b: 1 },
                Instruction::AddImmediate { d: 5, a: 0, .. },
                Instruction::AddImmediate { d: 6, a: 0, .. },
                Instruction::BranchAndLink { .. },
            ]
        )
    })
}

fn saved_register_indices(instructions: &[Instruction], stores: bool) -> Vec<usize> {
    instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| match instruction {
            Instruction::StoreWord { s, a: 1, .. } if stores && matches!(*s, 29..=31) => {
                Some(index)
            }
            Instruction::LoadWord { d, a: 1, .. } if !stores && matches!(*d, 29..=31) => {
                Some(index)
            }
            _ => None,
        })
        .collect()
}

fn same_saved_register_set(
    instructions: &[Instruction],
    saved: &[usize],
    restored: &[usize],
) -> bool {
    let mut saved_registers = saved
        .iter()
        .map(|&index| match instructions[index] {
            Instruction::StoreWord { s, .. } => s,
            _ => unreachable!("saved register index changed form"),
        })
        .collect::<Vec<_>>();
    let mut restored_registers = restored
        .iter()
        .map(|&index| match instructions[index] {
            Instruction::LoadWord { d, .. } => d,
            _ => unreachable!("restored register index changed form"),
        })
        .collect::<Vec<_>>();
    saved_registers.sort_unstable();
    restored_registers.sort_unstable();
    saved_registers == [29, 30, 31] && restored_registers == saved_registers
}

fn canonical_saved_offset(frame_bytes: i16, register: u8) -> i16 {
    frame_bytes - 4 * i16::from(32 - register)
}
