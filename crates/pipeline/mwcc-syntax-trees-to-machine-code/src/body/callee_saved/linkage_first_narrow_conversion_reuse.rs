//! Reuse of a widened narrow value across linkage-first calls.
//!
//! Build 163 can widen a saved narrow parameter into a dead saved GPR, consume
//! that value in an integer-to-float image, and compare the same widened value
//! later. Keeping the widened value live frees r0 for the image high word and
//! avoids a second mask at the condition.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NarrowConversionPacket {
    start: usize,
    condition: usize,
    source: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PrecomputedBooleanPacket {
    start: usize,
    conversion: usize,
    condition: usize,
    source: u8,
    reusable: u8,
}

impl Generator {
    pub(crate) fn reuse_linkage_first_narrow_conversion_value(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.behavior.legacy_float_cast_schedule
        {
            return;
        }
        if let Some(packet) = find_precomputed_boolean_packet(&self.output.instructions) {
            self.rewrite_precomputed_boolean_packet(packet);
            return;
        }
        let Some(packet) = find_narrow_conversion_packet(&self.output.instructions) else {
            return;
        };
        let saved_registers =
            physical_saved_registers_before(&self.output.instructions, packet.start);
        let Some(reusable) = reusable_saved_register(
            &self.output.instructions,
            packet.start,
            packet.condition,
            packet.source,
            &saved_registers,
        ) else {
            return;
        };
        if self.output.instructions[packet.start..=packet.condition]
            .iter()
            .flat_map(mwcc_vreg::register_operands)
            .any(|operand| {
                operand.class == mwcc_vreg::Class::Float && operand.register == 2
            })
        {
            return;
        }

        let start = packet.start;
        let Instruction::ClearLeftImmediate { a, .. } =
            &mut self.output.instructions[start + 2]
        else {
            unreachable!("the packet recognizer selected the widening mask")
        };
        *a = reusable;
        let Instruction::AddImmediateShifted { d, .. } =
            &mut self.output.instructions[start + 3]
        else {
            unreachable!("the packet recognizer selected the image high word")
        };
        *d = 0;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[start + 4] else {
            unreachable!("the packet recognizer selected the widened-value store")
        };
        *s = reusable;
        let Instruction::LoadFloatDouble { d, .. } =
            &mut self.output.instructions[start + 5]
        else {
            unreachable!("the packet recognizer selected the bias load")
        };
        *d = 2;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[start + 6] else {
            unreachable!("the packet recognizer selected the image-high store")
        };
        *s = 0;
        let Instruction::FloatSubtractSingle { b, .. } =
            &mut self.output.instructions[start + 8]
        else {
            unreachable!("the packet recognizer selected the conversion subtract")
        };
        *b = 2;
        self.output.instructions[packet.condition] =
            Instruction::CompareLogicalWordImmediate {
                a: reusable,
                immediate: 0,
            };

        self.move_instruction_before(start + 2, start);
        self.move_instruction_before(start + 5, start + 1);
        self.move_instruction_before(start + 5, start + 2);
        self.move_instruction_before(start + 5, start + 3);
    }

    fn rewrite_precomputed_boolean_packet(&mut self, packet: PrecomputedBooleanPacket) {
        let start = packet.start;
        self.output.instructions[start + 1] = Instruction::Negate {
            d: 4,
            a: 0,
        };
        self.output.instructions[start + 2] = Instruction::AddImmediateCarrying {
            d: 0,
            a: 4,
            immediate: -1,
        };
        self.output.instructions[start + 3] = Instruction::SubtractFromExtended {
            d: 0,
            a: 0,
            b: 4,
        };
        self.insert_narrow_conversion_instruction(
            start + 4,
            Instruction::ClearLeftImmediate {
                a: packet.reusable,
                s: 0,
                clear: 24,
            },
        );
        self.move_instruction_before(start + 5, start + 1);

        let conversion = packet.conversion + 1;
        let Instruction::LoadFloatDouble { d, .. } =
            &mut self.output.instructions[conversion + 4]
        else {
            unreachable!("the packet recognizer selected the conversion bias load")
        };
        *d = 2;
        let Instruction::FloatSubtractSingle { b, .. } =
            &mut self.output.instructions[conversion + 7]
        else {
            unreachable!("the packet recognizer selected the conversion subtract")
        };
        *b = 2;
        self.move_instruction_before(conversion + 3, conversion);
        self.move_instruction_before(conversion + 3, conversion + 1);
        self.move_instruction_before(conversion + 4, conversion + 2);
        self.move_instruction_before(conversion + 5, conversion + 4);

        self.output.instructions[packet.condition + 1] =
            Instruction::CompareLogicalWordImmediate {
                a: packet.reusable,
                immediate: 0,
            };
    }

    fn insert_narrow_conversion_instruction(
        &mut self,
        position: usize,
        instruction: Instruction,
    ) {
        self.output.instructions.insert(position, instruction);
        self.labels.inserted(position, 1);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index >= position {
                relocation.instruction_index += 1;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                    if *target >= position =>
                {
                    *target += 1;
                }
                _ => {}
            }
        }
    }
}

fn find_precomputed_boolean_packet(
    instructions: &[Instruction],
) -> Option<PrecomputedBooleanPacket> {
    for (start, window) in instructions.windows(6).enumerate() {
        let (
            Instruction::ClearLeftImmediate {
                a: 0,
                s: source,
                clear: 24,
            },
            Instruction::Negate {
                d: reusable,
                a: 0,
            },
            Instruction::AddImmediateCarrying {
                d: 0,
                a: carrying,
                immediate: -1,
            },
            Instruction::SubtractFromExtended {
                d: result,
                a: 0,
                b: extended,
            },
            Instruction::LoadWord { d: 3, a: 1, .. },
            Instruction::BranchAndLink { .. },
        ) = (
            &window[0], &window[1], &window[2], &window[3], &window[4], &window[5],
        )
        else {
            continue;
        };
        if reusable != carrying
            || reusable != result
            || reusable != extended
            || !(14..=31).contains(reusable)
            || !(14..=31).contains(source)
        {
            continue;
        }
        let conversion = start + 6;
        let Some(conversion_window) = instructions.get(conversion..conversion + 9) else {
            continue;
        };
        if !matches!(
            conversion_window,
            [
                Instruction::FloatMove { d: 14..=31, b: 1 },
                Instruction::LoadWord { d: 3, a: 1, .. },
                Instruction::AddImmediateShifted {
                    d: 0,
                    a: 0,
                    immediate: 17200,
                },
                Instruction::StoreWord {
                    s,
                    a: 1,
                    ..
                },
                Instruction::LoadFloatDouble { d: 1, a: 0, offset: 0 },
                Instruction::StoreWord { s: 0, a: 1, .. },
                Instruction::LoadFloatDouble { d: 0, a: 1, .. },
                Instruction::FloatSubtractSingle { d: 1, a: 0, b: 1 },
                Instruction::BranchAndLink { .. },
            ] if s == reusable
        ) {
            continue;
        }
        let Some(condition) = instructions[conversion + 9..]
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::ClearLeftImmediateRecord {
                        a: 0,
                        s,
                        clear: 24,
                    } if s == reusable
                )
            })
            .map(|offset| conversion + 9 + offset)
        else {
            continue;
        };
        if matches!(
            instructions.get(condition + 1),
            Some(Instruction::BranchConditionalForward { .. })
        ) {
            return Some(PrecomputedBooleanPacket {
                start,
                conversion,
                condition,
                source: *source,
                reusable: *reusable,
            });
        }
    }
    None
}

fn find_narrow_conversion_packet(instructions: &[Instruction]) -> Option<NarrowConversionPacket> {
    for (start, window) in instructions.windows(10).enumerate() {
        let (
            Instruction::FloatMove {
                d: saved_float,
                b: 1,
            },
            Instruction::LoadWord { d: 3, a: 1, .. },
            Instruction::ClearLeftImmediate {
                a: 0,
                s: source,
                clear,
            },
            Instruction::AddImmediateShifted {
                d: high_register,
                a: 0,
                immediate: 17200,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: value_offset,
            },
            Instruction::LoadFloatDouble {
                d: 1,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: stored_high,
                a: 1,
                offset: high_offset,
            },
            Instruction::LoadFloatDouble {
                d: 0,
                a: 1,
                offset: loaded_offset,
            },
            Instruction::FloatSubtractSingle {
                d: 1,
                a: 0,
                b: 1,
            },
            Instruction::BranchAndLink { .. },
        ) = (
            &window[0], &window[1], &window[2], &window[3], &window[4],
            &window[5], &window[6], &window[7], &window[8], &window[9],
        )
        else {
            continue;
        };
        if *saved_float < 14
            || *source < 14
            || !matches!(*clear, 16 | 24)
            || stored_high != high_register
            || high_offset != loaded_offset
            || *value_offset != *high_offset + 4
        {
            continue;
        }
        let Some(condition) = instructions[start + 10..]
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::ClearLeftImmediateRecord {
                        a: 0,
                        s,
                        clear: later_clear
                    } if s == source && later_clear == clear
                )
            })
            .map(|offset| start + 10 + offset)
        else {
            continue;
        };
        if matches!(
            instructions.get(condition + 1),
            Some(Instruction::BranchConditionalForward { .. })
        ) {
            return Some(NarrowConversionPacket {
                start,
                condition,
                source: *source,
            });
        }
    }
    None
}

fn physical_saved_registers_before(instructions: &[Instruction], before: usize) -> Vec<u8> {
    instructions[..before]
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::StoreWord { s, a: 1, .. } if (14..=31).contains(s) => Some(*s),
            _ => None,
        })
        .collect()
}

fn reusable_saved_register(
    instructions: &[Instruction],
    start: usize,
    condition: usize,
    source: u8,
    callee_saved: &[u8],
) -> Option<u8> {
    callee_saved.iter().copied().find(|candidate| {
        *candidate != source
            && !instructions[start..=condition]
                .iter()
                .flat_map(mwcc_vreg::register_operands)
                .any(|operand| {
                    operand.class == mwcc_vreg::Class::General
                        && operand.register == *candidate
                })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_boolean_computed_before_the_conversion_call() {
        let instructions = vec![
            Instruction::ClearLeftImmediate {
                a: 0,
                s: 29,
                clear: 24,
            },
            Instruction::Negate { d: 30, a: 0 },
            Instruction::AddImmediateCarrying {
                d: 0,
                a: 30,
                immediate: -1,
            },
            Instruction::SubtractFromExtended {
                d: 30,
                a: 0,
                b: 30,
            },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: 24,
            },
            Instruction::BranchAndLink {
                target: "prepare".into(),
            },
            Instruction::FloatMove { d: 31, b: 1 },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: 24,
            },
            Instruction::AddImmediateShifted {
                d: 0,
                a: 0,
                immediate: 17200,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 36,
            },
            Instruction::LoadFloatDouble {
                d: 1,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 32,
            },
            Instruction::LoadFloatDouble {
                d: 0,
                a: 1,
                offset: 32,
            },
            Instruction::FloatSubtractSingle { d: 1, a: 0, b: 1 },
            Instruction::BranchAndLink {
                target: "animate".into(),
            },
            Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 30,
                clear: 24,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 16,
            },
        ];

        assert_eq!(
            find_precomputed_boolean_packet(&instructions),
            Some(PrecomputedBooleanPacket {
                start: 0,
                conversion: 6,
                condition: 15,
                source: 29,
                reusable: 30,
            })
        );
    }

    #[test]
    fn recognizes_conversion_and_later_repeated_narrow_condition() {
        let instructions = vec![
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 44,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 40,
            },
            Instruction::FloatMove { d: 30, b: 1 },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: 24,
            },
            Instruction::ClearLeftImmediate {
                a: 0,
                s: 30,
                clear: 24,
            },
            Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 17200,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 36,
            },
            Instruction::LoadFloatDouble {
                d: 1,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 4,
                a: 1,
                offset: 32,
            },
            Instruction::LoadFloatDouble {
                d: 0,
                a: 1,
                offset: 32,
            },
            Instruction::FloatSubtractSingle { d: 1, a: 0, b: 1 },
            Instruction::BranchAndLink {
                target: "animate".into(),
            },
            Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 30,
                clear: 24,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 12,
            },
        ];

        let packet = find_narrow_conversion_packet(&instructions).unwrap();
        assert_eq!(packet.start, 2);
        assert_eq!(packet.condition, 12);
        let saved = physical_saved_registers_before(&instructions, packet.start);
        assert_eq!(
            reusable_saved_register(
                &instructions,
                packet.start,
                packet.condition,
                30,
                &saved
            ),
            Some(31)
        );
    }
}
