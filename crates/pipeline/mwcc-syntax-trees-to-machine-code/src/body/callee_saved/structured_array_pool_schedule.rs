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
}
