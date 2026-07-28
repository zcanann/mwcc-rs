//! Entry and exit issue order for structured frames backed by array pools.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// A pooled frame saves a dense physical GPR range with `stmw`, then copies
    /// retained incoming values into their homes in incoming-register order.
    /// Home planning is lifetime-ranked and therefore stores parameters in the
    /// opposite order; keep that concern separate from entry scheduling.
    pub(super) fn emit_structured_array_pool_parameter_copies(
        &mut self,
        saved_parameter_homes: &[(String, u8, u8)],
    ) {
        for (_, home, incoming) in saved_parameter_homes.iter().rev() {
            self.output
                .instructions
                .push(Instruction::move_register(*home, *incoming));
        }
    }

    /// Pooled dense frames use the ordinary MWCC teardown issue order even
    /// though non-pooled dense frames restore the stack pointer first.
    pub(super) fn schedule_structured_array_pool_epilogue(&mut self) {
        let end = self.output.instructions.len();
        if end < 5 {
            return;
        }
        if matches!(
            &self.output.instructions[end - 5..],
            [
                Instruction::LoadMultipleWord { a: 1, .. },
                Instruction::LoadWord { d: 0, a: 1, .. },
                Instruction::AddImmediate { d: 1, a: 1, .. },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::BranchToLinkRegister,
            ]
        ) {
            self.output.instructions.swap(end - 3, end - 2);
        }
    }

    /// Physical allocation uses entry-move order to break otherwise-equal home
    /// preferences, so changing the virtual stream to obtain MWCC's issue order
    /// can also swap the homes themselves. Reorder the already-allocated
    /// parameter-copy run instead: incoming r3, r4, ... order is then purely a
    /// schedule decision and cannot perturb allocation.
    pub(crate) fn schedule_allocated_structured_array_pool_parameter_copies(&mut self) {
        if self.output.anonymous_rodata.len() < 2
            || !self
                .output
                .anonymous_rodata
                .iter()
                .any(|blob| blob.static_slot_prefix_bump.is_some())
        {
            return;
        }

        let Some(store_index) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::StoreMultipleWord { s: 14, .. })
        }) else {
            return;
        };
        let start = store_index + 1;
        let count = self.output.instructions[start..]
            .iter()
            .take_while(|instruction| {
                matches!(
                    instruction,
                    Instruction::Or { a, s, b }
                        if a != s && s == b && (14..=31).contains(a) && (3..=10).contains(s)
                )
            })
            .count();
        if count < 2 {
            return;
        }
        self.output.instructions[start..start + count].sort_by_key(|instruction| {
            let Instruction::Or { s, .. } = instruction else {
                unreachable!("the parameter-copy run was filtered as register moves")
            };
            *s
        });
    }
}
