//! Reuse a shared-global base across adjacent saved-float assignments.
//!
//! Conditional initialization often selects two fields from one global
//! configuration object. The generic statement emitter reloads the global
//! pointer for each assignment; build 163 keeps it in the argument register
//! across both independent float loads.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_shared_global_float_pairs(&mut self) {
        let mut search_from = 0;
        loop {
            let Some(relative) = self.output.instructions[search_from..]
                .windows(4)
                .position(is_reloaded_shared_global_float_pair)
            else {
                return;
            };
            let start = search_from + relative;
            let reload = start + 2;
            let reload_is_target = self.output.instructions.iter().any(|instruction| {
                matches!(instruction,
                    Instruction::BranchConditionalForward { target, .. }
                        | Instruction::Branch { target }
                        if *target == reload)
            });
            if !reload_is_target
                && schedule_relocations::same_relocated_value(
                    &self.output.relocations,
                    &self.output.constants,
                    start,
                    reload,
                )
            {
                self.remove_structured_condition_instruction(reload);
                search_from = start + 2;
            } else {
                search_from = start + 1;
            }
        }
    }
}

fn is_reloaded_shared_global_float_pair(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadWord { d: first_global, a: 0, .. },
        Instruction::LoadFloatSingle { a: first_base, .. },
        Instruction::LoadWord { d: second_global, a: 0, .. },
        Instruction::LoadFloatSingle { a: second_base, .. },
    ] if first_global == second_global
        && first_base == first_global
        && second_base == second_global)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_adjacent_float_fields_from_one_reloaded_global() {
        let instructions = [
            Instruction::LoadWord { d: 3, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 30, a: 3, offset: 1108 },
            Instruction::LoadWord { d: 3, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 31, a: 3, offset: 1112 },
        ];
        assert!(is_reloaded_shared_global_float_pair(&instructions));
    }
}
