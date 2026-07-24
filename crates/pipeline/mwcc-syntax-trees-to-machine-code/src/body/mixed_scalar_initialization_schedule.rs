//! Shared constants across mixed scalar member initialization.
//!
//! MWCC keeps a small integer and a pooled float live across heterogeneous
//! member stores, and starts the next integer constant before the float load.
//! Per-statement lowering cannot see that complete lifetime, so this final
//! physical-stream owner claims only the proven straight-line region.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_mixed_scalar_initialization(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(18)
            .position(is_mixed_scalar_initialization)
        else {
            return;
        };
        if [start + 4, start + 6, start + 8]
            .into_iter()
            .any(|reload| {
                !schedule_relocations::same_relocated_value(
                    &self.output.relocations,
                    &self.output.constants,
                    start + 2,
                    reload,
                )
            })
            || self.output.instructions.iter().any(|instruction| {
                matches!(instruction,
                    Instruction::BranchConditionalForward { target, .. }
                        | Instruction::Branch { target }
                        if (start..start + 18).contains(target))
            })
        {
            return;
        }

        match &mut self.output.instructions[start] {
            Instruction::AddImmediate { d, .. } => *d = 4,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 1] {
            Instruction::StoreWord { s, .. } => *s = 4,
            _ => unreachable!(),
        }

        self.move_mixed_initialization_instruction(start + 12, start + 2);
        for at in [start + 9, start + 7, start + 5] {
            self.remove_mixed_initialization_instruction(at);
        }
        self.remove_mixed_initialization_instruction(start + 8);
        match &mut self.output.instructions[start + 8] {
            Instruction::StoreByte { s, .. } => *s = 4,
            _ => unreachable!(),
        }
    }

    fn move_mixed_initialization_instruction(&mut self, from: usize, to: usize) {
        debug_assert!(self
            .output
            .relocations
            .iter()
            .all(|relocation| relocation.instruction_index != from));
        let instruction = self.output.instructions.remove(from);
        self.labels.moved_before(from, to);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > from {
                relocation.instruction_index -= 1;
            }
        }
        self.output.instructions.insert(to, instruction);
        self.labels.inserted(to, 1);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index >= to {
                relocation.instruction_index += 1;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target } => {
                    *target = if *target == from {
                        to
                    } else if (to..from).contains(&*target) {
                        *target + 1
                    } else {
                        *target
                    };
                }
                _ => {}
            }
        }
    }

    fn remove_mixed_initialization_instruction(&mut self, at: usize) {
        self.output.instructions.remove(at);
        self.labels.removed(at, 1);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != at);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > at {
                relocation.instruction_index -= 1;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                    if *target > at =>
                {
                    *target -= 1;
                }
                _ => {}
            }
        }
    }
}

fn is_mixed_scalar_initialization(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::AddImmediate { d: first_value, a: 0, immediate: first_immediate },
        Instruction::StoreWord { s: first_store, a: base, .. },
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::StoreFloatSingle { s: 0, a: float_base_1, .. },
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::StoreFloatSingle { s: 0, a: float_base_2, .. },
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::StoreFloatSingle { s: 0, a: float_base_3, .. },
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::StoreFloatSingle { s: 0, a: float_base_4, .. },
        Instruction::AddImmediate { d: repeated_value, a: 0, immediate: repeated_immediate },
        Instruction::StoreByte { s: repeated_store, a: byte_base, .. },
        Instruction::AddImmediate { d: next_value, a: 0, immediate: next_immediate },
        Instruction::StoreWord { s: next_store, a: next_base, .. },
        Instruction::LoadWord { d: rmw, a: rmw_base, offset: rmw_offset },
        Instruction::OrImmediate { a: rmw_result, s: rmw_source, .. },
        Instruction::StoreWord { s: rmw_store, a: rmw_store_base, offset: rmw_store_offset },
        Instruction::BranchToLinkRegister,
    ] if first_immediate == repeated_immediate
        && first_immediate != next_immediate
        && first_value == first_store
        && repeated_value == repeated_store
        && next_value == next_store
        && base == float_base_1
        && base == float_base_2
        && base == float_base_3
        && base == float_base_4
        && base == byte_base
        && base == next_base
        && base == rmw_base
        && base == rmw_store_base
        && rmw == rmw_result
        && rmw == rmw_source
        && rmw == rmw_store
        && rmw_offset == rmw_store_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_mixed_integer_float_and_rmw_initialization() {
        let instructions = [
            Instruction::AddImmediate { d: 0, a: 0, immediate: 1 },
            Instruction::StoreWord { s: 0, a: 3, offset: 224 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: 236 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: 160 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: 184 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: 120 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 1 },
            Instruction::StoreByte { s: 0, a: 3, offset: 6504 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 10 },
            Instruction::StoreWord { s: 0, a: 3, offset: 2188 },
            Instruction::LoadWord { d: 0, a: 3, offset: 2080 },
            Instruction::OrImmediate { a: 0, s: 0, immediate: 16 },
            Instruction::StoreWord { s: 0, a: 3, offset: 2080 },
            Instruction::BranchToLinkRegister,
        ];
        assert!(is_mixed_scalar_initialization(&instructions));
    }
}
