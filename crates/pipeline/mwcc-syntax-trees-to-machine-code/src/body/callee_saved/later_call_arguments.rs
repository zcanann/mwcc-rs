//! Scheduling for entry parameters that survive one call and feed a later call.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// With two entry parameters saved as r31/r30, mwcc fills the latency slot
    /// between the two save/copy pairs with the first call's independent r4
    /// setup. The caller verifies the narrow straight-line family before
    /// emission; this helper owns only the instruction/relocation permutation.
    pub(super) fn schedule_later_call_argument_prologue(
        &mut self,
        body_start: usize,
    ) -> Compilation<()> {
        let Some(first_call) = self.output.instructions[body_start..]
            .iter()
            .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
            .map(|offset| body_start + offset)
        else {
            return Err(Diagnostic::error(
                "internal: later-call survivor schedule has no first call",
            ));
        };
        let Some(setup) = (body_start..first_call).find(|&index| {
            matches!(
                self.output.instructions[index],
                Instruction::AddImmediate { d: 4, a: 0, .. }
                    | Instruction::AddImmediateShifted { d: 4, a: 0, .. }
            )
        }) else {
            return Err(Diagnostic::error(
                "internal: later-call survivor schedule has no independent r4 setup",
            ));
        };
        let insertion = body_start.checked_sub(2).ok_or_else(|| {
            Diagnostic::error("internal: later-call survivor prologue is incomplete")
        })?;
        move_instruction_before(&mut self.output, setup, insertion);
        Ok(())
    }
}

fn move_instruction_before(
    output: &mut mwcc_machine_code::MachineFunction,
    from: usize,
    to: usize,
) {
    debug_assert!(to < from);
    let instruction = output.instructions.remove(from);
    output.instructions.insert(to, instruction);
    for relocation in &mut output.relocations {
        relocation.instruction_index = if relocation.instruction_index == from {
            to
        } else if (to..from).contains(&relocation.instruction_index) {
            relocation.instruction_index + 1
        } else {
            relocation.instruction_index
        };
    }
}
