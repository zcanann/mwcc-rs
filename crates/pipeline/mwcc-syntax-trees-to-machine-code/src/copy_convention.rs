//! Generation-specific encodings for semantically neutral integer copies.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;
use mwcc_versions::MaterializationCopyStyle;

impl Generator {
    /// Patched build 159 forwards the first pointer argument as an address copy
    /// and lets the adjacent pointer-difference argument consume the second
    /// source directly. Build 163's general materialization policy otherwise
    /// leaves a redundant r0 snapshot in this four-instruction call packet.
    pub(crate) fn normalize_patched_build159_pointer_difference_call(&mut self) {
        if self.behavior.frame_convention != mwcc_versions::FrameConvention::LinkageFirst
            || self.behavior.plain_linkage_epilogue_style
                != mwcc_versions::PlainLinkageEpilogueStyle::StackRestoreBeforeReload
        {
            return;
        }
        let relocated: std::collections::HashSet<usize> = self
            .output
            .relocations
            .iter()
            .map(|relocation| relocation.instruction_index)
            .collect();
        let Some((start, start_source, end_source)) = self
            .output
            .instructions
            .windows(4)
            .enumerate()
            .find_map(|(index, window)| {
                (!relocated.contains(&index)
                    && !relocated.contains(&(index + 1))
                    && !relocated.contains(&(index + 2)))
                    .then(|| patched_pointer_difference_call(window).map(|pair| (index, pair.0, pair.1)))
                    .flatten()
            })
        else {
            return;
        };
        self.output.instructions[start] = Instruction::move_register(3, start_source);
        self.output.instructions[start + 2] = Instruction::SubtractFrom {
            d: 4,
            a: start_source,
            b: end_source,
        };
        crate::remove_instruction_retargeting_to_next(self, start + 1);
    }

    /// Nonreturning linkage-first functions keep register copies in their `mr`
    /// form. Some ordered structured-entry paths initially use `addi d,s,0`
    /// before tail reachability is known; normalize those once the function
    /// proves to have no return edge. Relocated zero-offset additions are
    /// symbol address materializations and must retain `addi`.
    pub(crate) fn normalize_nonreturning_materialization_copies(&mut self) {
        if self.behavior.frame_convention
            != mwcc_versions::FrameConvention::LinkageFirst
        {
            return;
        }
        let relocated = self
            .output
            .relocations
            .iter()
            .map(|relocation| relocation.instruction_index)
            .collect::<std::collections::HashSet<_>>();
        normalize_nonreturning_copies(&mut self.output.instructions, &relocated);
    }

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
        normalize_saved_two_literal_call_arguments(&mut self.output.instructions);
        normalize_saved_literal_call_arguments(&mut self.output.instructions);
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

fn patched_pointer_difference_call(window: &[Instruction]) -> Option<(u8, u8)> {
    let [
        Instruction::AddImmediate {
            d: 3,
            a: start,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 0,
            a: end,
            immediate: 0,
        },
        Instruction::SubtractFrom { d: 4, a, b: 0 },
        Instruction::BranchAndLink { .. },
    ] = window
    else {
        return None;
    };
    (*start != 0 && *end != 0 && start != end && a == start).then_some((*start, *end))
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

/// A retained value forwarded beside two literal arguments is a value
/// materialization. This is the common three-argument command/ack packet;
/// recognizing the complete packet avoids changing address-preservation moves.
fn normalize_saved_two_literal_call_arguments(instructions: &mut [Instruction]) {
    for index in 0..instructions.len().saturating_sub(3) {
        let source = match &instructions[index..index + 4] {
            [
                Instruction::Or {
                    a: 3,
                    s: source,
                    b: duplicate,
                },
                Instruction::AddImmediate { d: 4, a: 0, .. },
                Instruction::AddImmediate { d: 5, a: 0, .. },
                Instruction::BranchAndLink { .. },
            ] if source == duplicate && (14..=31).contains(source) => *source,
            _ => continue,
        };
        instructions[index] = Instruction::AddImmediate {
            d: 3,
            a: source,
            immediate: 0,
        };
    }
}

/// A retained value forwarded beside three literal arguments is likewise a
/// materialization. The complete ABI argument packet distinguishes it from an
/// address-preservation copy or a control-flow merge.
fn normalize_saved_literal_call_arguments(instructions: &mut [Instruction]) {
    for index in 0..instructions.len().saturating_sub(4) {
        let source = match &instructions[index..index + 5] {
            [
                Instruction::Or {
                    a: 3,
                    s: source,
                    b: duplicate,
                },
                Instruction::AddImmediate { d: 4, a: 0, .. },
                Instruction::AddImmediate { d: 5, a: 0, .. },
                Instruction::AddImmediate { d: 6, a: 0, .. },
                Instruction::BranchAndLink { .. },
            ] if source == duplicate && (14..=31).contains(source) => *source,
            _ => continue,
        };
        instructions[index] = Instruction::AddImmediate {
            d: 3,
            a: source,
            immediate: 0,
        };
    }
}

fn normalize_nonreturning_copies(
    instructions: &mut [Instruction],
    relocated: &std::collections::HashSet<usize>,
) {
    for (index, instruction) in instructions.iter_mut().enumerate() {
        let (destination, source) = match *instruction {
            Instruction::AddImmediate {
                d,
                a,
                immediate: 0,
            } if a != 0 && !relocated.contains(&index) => (d, a),
            _ => continue,
        };
        *instruction = Instruction::move_register(destination, source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_forwarded_pointer_difference_call_packet() {
        let instructions = [
            Instruction::AddImmediate {
                d: 3,
                a: 4,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 5,
                immediate: 0,
            },
            Instruction::SubtractFrom { d: 4, a: 4, b: 0 },
            Instruction::BranchAndLink {
                target: "flush".into(),
            },
        ];

        assert_eq!(patched_pointer_difference_call(&instructions), Some((4, 5)));
    }

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

    #[test]
    fn materializes_a_saved_object_beside_three_literals() {
        let mut instructions = [
            Instruction::move_register(3, 31),
            Instruction::AddImmediate {
                d: 4,
                a: 0,
                immediate: 279,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 127,
            },
            Instruction::AddImmediate {
                d: 6,
                a: 0,
                immediate: 64,
            },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
        ];

        normalize_saved_literal_call_arguments(&mut instructions);

        assert!(matches!(
            instructions[0],
            Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 0
            }
        ));
    }

    #[test]
    fn materializes_a_saved_object_beside_two_literals() {
        let mut instructions = [
            Instruction::move_register(3, 31),
            Instruction::AddImmediate {
                d: 4,
                a: 0,
                immediate: 128,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 22,
            },
            Instruction::BranchAndLink {
                target: "ack".into(),
            },
        ];

        normalize_saved_two_literal_call_arguments(&mut instructions);

        assert!(matches!(
            instructions[0],
            Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 0
            }
        ));
    }

    #[test]
    fn nonreturning_copies_preserve_relocated_address_additions() {
        let mut instructions = [
            Instruction::AddImmediate {
                d: 31,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 30,
                immediate: 0,
            },
        ];
        let relocated = std::collections::HashSet::from([1]);

        normalize_nonreturning_copies(&mut instructions, &relocated);

        assert_eq!(instructions[0], Instruction::move_register(31, 3));
        assert!(matches!(
            instructions[1],
            Instruction::AddImmediate {
                d: 30,
                a: 30,
                immediate: 0
            }
        ));
    }
}
