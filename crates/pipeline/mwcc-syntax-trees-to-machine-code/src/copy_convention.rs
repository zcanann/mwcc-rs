//! Generation-specific encodings for semantically neutral integer copies.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;
use mwcc_versions::MaterializationCopyStyle;

impl Generator {
    /// Schedule a saved-base call argument before an independent derived alias.
    /// Linkage-first MWCC uses its materialization-copy spelling for the ABI
    /// argument and fills that copy's issue slot with the alias computation.
    pub(crate) fn schedule_saved_base_call_argument(&mut self) {
        if self.behavior.materialization_copy_style
            != MaterializationCopyStyle::AddImmediateZero
        {
            return;
        }
        if self.callee_saved.len() == 2 && self.callee_saved_float == 2 {
            normalize_saved_frame_call_arguments(&mut self.output.instructions);
        }
        let Some(start) = self.output.instructions.windows(3).position(|window| {
            matches!(window, [
                Instruction::AddImmediate { d: alias, a: base, immediate },
                Instruction::Or { a: 3, s: argument, b: duplicate },
                Instruction::BranchAndLink { .. },
            ] if *immediate != 0 && alias != base && base == argument && argument == duplicate)
        }) else {
            return;
        };
        let (base, alias, immediate) = match self.output.instructions[start] {
            Instruction::AddImmediate { d, a, immediate } => (a, d, immediate),
            _ => unreachable!(),
        };
        self.output.instructions[start] = Instruction::AddImmediate {
            d: 3,
            a: base,
            immediate: 0,
        };
        self.output.instructions[start + 1] = Instruction::AddImmediate {
            d: alias,
            a: base,
            immediate,
        };
    }

    /// Normalize physical, straight-line r0 snapshots after allocation. `addi`
    /// cannot read r0 as a register (rA=0 means literal zero), so self/zero-source
    /// moves retain their logical encoding. A move immediately inside a
    /// conditional arm also retains `mr`: build 163's phi staging uses the
    /// logical copy even though arithmetic snapshots use add-immediate-zero.
    pub(crate) fn normalize_scratch_copy_convention(&mut self) {
        if self.behavior.materialization_copy_style != MaterializationCopyStyle::AddImmediateZero {
            return;
        }
        for index in 0..self.output.instructions.len() {
            let begins_conditional_arm = index > 0
                && matches!(
                    self.output.instructions[index - 1],
                    Instruction::BranchConditionalForward { .. }
                );
            if begins_conditional_arm {
                continue;
            }
            let source = match self.output.instructions[index] {
                Instruction::Or { a: 0, s, b } if s == b && s != 0 => s,
                _ => continue,
            };
            self.output.instructions[index] = Instruction::AddImmediate {
                d: 0,
                a: source,
                immediate: 0,
            };
        }
    }

    /// Emit a semantic integer-value materialization. Build 163 uses `addi
    /// d,s,0` for these copies (including scalar-to-wide conversion and wide
    /// ABI-result forwarding); later generations use the canonical `mr` alias.
    /// Address preservation and control-flow merges are separate copy purposes
    /// and deliberately do not call this helper.
    pub(crate) fn emit_integer_materialization_copy(&mut self, destination: u8, source: u8) {
        let instruction = if self.behavior.materialization_copy_style
            == MaterializationCopyStyle::AddImmediateZero
            && source != 0
        {
            Instruction::AddImmediate {
                d: destination,
                a: source,
                immediate: 0,
            }
        } else {
            Instruction::move_register(destination, source)
        };
        self.output.instructions.push(instruction);
    }
}

/// A retained object forwarded beside an address-taken frame aggregate is a
/// value materialization, not an address-preservation copy. Build 163 spells
/// each such first argument as `addi r3,saved,0`.
fn normalize_saved_frame_call_arguments(instructions: &mut [Instruction]) {
    for index in 0..instructions.len().saturating_sub(2) {
        let source = match (
            &instructions[index],
            &instructions[index + 1],
            &instructions[index + 2],
        ) {
            (
                Instruction::Or { a: 3, s, b },
                Instruction::AddImmediate { d: 4, a: 1, .. },
                Instruction::BranchAndLink { .. },
            ) if s == b && (14..=31).contains(s) => *s,
            _ => continue,
        };
        instructions[index] = Instruction::AddImmediate {
            d: 3,
            a: source,
            immediate: 0,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materializes_a_saved_object_beside_a_frame_argument() {
        let mut instructions = [
            Instruction::move_register(3, 31),
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: 16,
            },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
        ];

        normalize_saved_frame_call_arguments(&mut instructions);

        assert!(matches!(
            instructions[0],
            Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 0
            }
        ));
    }
}
