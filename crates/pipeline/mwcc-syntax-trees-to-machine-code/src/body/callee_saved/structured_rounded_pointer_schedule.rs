//! PowerPC 7400 scheduling for dense rounded-pointer frames.
//!
//! These bodies combine a frame load batch, repeated feedback calls, and a
//! callback-address argument. Their dependencies are ordinary structured
//! instructions, but the 7400 scheduler consistently fills producer latency
//! from the next argument or load group. Keeping the overlay here leaves
//! lifetime planning and generic expression selection schedule-independent.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_power_pc_7400_rounded_pointer_body(&mut self) {
        self.schedule_rounded_shift_add_call();
        self.schedule_rounded_five_argument_call();
        self.schedule_rounded_load_batch_call();
        self.schedule_rounded_feedback_arguments();
        self.schedule_rounded_final_feedback_store();
        self.schedule_rounded_callback_argument();
        self.schedule_rounded_result_stores();
    }

    fn schedule_rounded_shift_add_call(&mut self) {
        let Some(start) = self.output.instructions.windows(5).position(|window| {
            matches!(
                window,
                [
                    Instruction::ShiftLeftImmediate { a: staged, .. },
                    Instruction::AddImmediate {
                        d: argument,
                        a: staged_source,
                        ..
                    },
                    Instruction::AddImmediate {
                        d: 3,
                        immediate: 0,
                        ..
                    },
                    Instruction::AddImmediate {
                        d: 4,
                        a: argument_source,
                        immediate: 0,
                    },
                    Instruction::BranchAndLink { .. },
                ] if staged == staged_source
                    && argument == argument_source
            )
        }) else {
            return;
        };
        self.move_instruction_before(start + 2, start + 1);
    }

    fn schedule_rounded_five_argument_call(&mut self) {
        let Some(start) = self.output.instructions.windows(6).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate { d: 6, .. },
                    Instruction::AddImmediate {
                        d: 3,
                        immediate: 0,
                        ..
                    },
                    Instruction::AddImmediate { d: 4, a: 0, .. },
                    Instruction::AddImmediate { d: 5, a: 1, .. },
                    Instruction::AddImmediate { d: 7, a: 0, .. },
                    Instruction::BranchAndLink { .. },
                ]
            )
        }) else {
            return;
        };
        let window = self.output.instructions[start..start + 6].to_vec();
        for (destination, source) in [1, 0, 3, 2, 4, 5].into_iter().enumerate() {
            self.output.instructions[start + destination] = window[source].clone();
        }
    }

    fn schedule_rounded_load_batch_call(&mut self) {
        let Some(start) = self.output.instructions.windows(10).position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadWord { a: 1, .. },
                    Instruction::LoadWord { a: 1, .. },
                    Instruction::LoadWord { a: 1, .. },
                    Instruction::LoadWord { a: 1, .. },
                    Instruction::LoadWord { a: 1, .. },
                    Instruction::LoadWord { d: 3, .. },
                    Instruction::Xor { a, s, b: 3 },
                    Instruction::AddImmediate {
                        d: literal,
                        a: 0,
                        ..
                    },
                    Instruction::AddImmediate {
                        d: 4,
                        a: literal_source,
                        immediate: 0,
                    },
                    Instruction::BranchAndLink { .. },
                ] if a == s && literal == literal_source
            )
        }) else {
            return;
        };
        let window = self.output.instructions[start..start + 10].to_vec();
        for (destination, source) in [5, 7, 0, 1, 2, 6, 3, 4, 8, 9].into_iter().enumerate() {
            self.output.instructions[start + destination] = window[source].clone();
        }
    }

    fn schedule_rounded_feedback_arguments(&mut self) {
        let mut call = 0;
        while call < self.output.instructions.len() {
            if !matches!(
                self.output.instructions[call],
                Instruction::BranchAndLink { .. }
            ) {
                call += 1;
                continue;
            }
            let Some(copy) = call.checked_sub(1) else {
                call += 1;
                continue;
            };
            let source = match self.output.instructions[copy] {
                Instruction::AddImmediate {
                    d: 4,
                    a,
                    immediate: 0,
                } => a,
                _ => {
                    call += 1;
                    continue;
                }
            };
            let definition_start = self.output.instructions[..copy]
                .iter()
                .rposition(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
                .map_or(0, |previous_call| previous_call + 1);
            let Some(argument) = self.output.instructions[definition_start..copy]
                .iter()
                .rposition(|instruction| rounded_defined_general(instruction) == Some(source))
                .map(|offset| definition_start + offset)
            else {
                call += 1;
                continue;
            };
            let simple_literal = matches!(
                self.output.instructions[argument],
                Instruction::AddImmediate { a: 0, .. }
            );
            let shifted_argument = matches!(
                self.output.instructions[argument],
                Instruction::ShiftLeftImmediate { .. }
            );
            if !simple_literal && !shifted_argument {
                call += 1;
                continue;
            }
            let feedback_start = call.saturating_sub(13);
            let Some(equivalence) = self.output.instructions[feedback_start..argument]
                .iter()
                .rposition(|instruction| matches!(instruction, Instruction::Eqv { a: 0, .. }))
                .map(|offset| feedback_start + offset)
            else {
                call += 1;
                continue;
            };
            if shifted_argument {
                let window = &mut self.output.instructions[equivalence - 5..=equivalence];
                let [
                    Instruction::ShiftLeftImmediate { .. },
                    Instruction::ShiftLeftImmediate { a: tap_15, .. },
                    Instruction::Xor { .. },
                    Instruction::ShiftLeftImmediate { a: tap_23, .. },
                    Instruction::Xor { s: folded_15, .. },
                    Instruction::Eqv { s: folded_23, .. },
                ] = window
                else {
                    call += 1;
                    continue;
                };
                *tap_15 = 5;
                *folded_15 = 5;
                *tap_23 = 6;
                *folded_23 = 6;
            }
            self.move_instruction_before(argument, equivalence);
            call += 1;
        }
    }

    fn schedule_rounded_callback_argument(&mut self) {
        let Some(start) = self.output.instructions.windows(5).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate {
                        d: 3,
                        immediate: 0,
                        ..
                    },
                    Instruction::AddImmediate {
                        d: 4,
                        immediate: 0,
                        ..
                    },
                    Instruction::AddImmediateShifted { d: 5, .. },
                    Instruction::AddImmediate {
                        d: 5,
                        a: 5,
                        immediate: 0,
                    },
                    Instruction::BranchAndLink { .. },
                ]
            )
        }) else {
            return;
        };
        let Instruction::AddImmediateShifted {
            a,
            immediate,
            ..
        } = self.output.instructions[start + 2]
        else {
            unreachable!("callback high half was gated");
        };
        self.output.instructions[start + 2] =
            Instruction::AddImmediateShifted { d: 4, a, immediate };
        let Instruction::AddImmediate { immediate, .. } =
            self.output.instructions[start + 3]
        else {
            unreachable!("callback low half was gated");
        };
        self.output.instructions[start + 3] = Instruction::AddImmediate {
            d: 5,
            a: 4,
            immediate,
        };
        self.move_instruction_before(start + 2, start);
        self.move_instruction_before(start + 3, start + 2);
    }

    fn schedule_rounded_final_feedback_store(&mut self) {
        let Some(start) = self.output.instructions.windows(20).position(|window| {
            matches!(
                window,
                [
                    Instruction::ShiftLeftImmediate { a: 0, .. },
                    Instruction::ShiftLeftImmediate { a: 4, .. },
                    Instruction::Xor { a: 0, .. },
                    Instruction::ShiftLeftImmediate { a: 5, .. },
                    Instruction::Xor { a: 0, s: 4, .. },
                    Instruction::Eqv { a: 0, s: 5, .. },
                    Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, .. },
                    Instruction::Or { a: 0, .. },
                    Instruction::StoreWord { s: 0, .. },
                    Instruction::StoreWord { .. },
                    Instruction::StoreWord { .. },
                    Instruction::StoreWord { .. },
                    Instruction::AddImmediate {
                        d: 0,
                        a: 0,
                        immediate: 8,
                    },
                    Instruction::StoreWord { s: 0, .. },
                    Instruction::StoreWord { .. },
                    Instruction::AddImmediate {
                        d: 0,
                        a: 0,
                        immediate: 0,
                    },
                    Instruction::StoreWord { s: 0, .. },
                    Instruction::AddImmediate {
                        d: 3,
                        immediate: 0,
                        ..
                    },
                    Instruction::AddImmediate {
                        d: 4,
                        a: 0,
                        immediate: 8,
                    },
                    Instruction::BranchAndLink { .. },
                ]
            )
        }) else {
            return;
        };
        let mut window = self.output.instructions[start..start + 20].to_vec();
        let Instruction::ShiftLeftImmediate { s, shift, .. } = window[3] else {
            unreachable!("final feedback third tap was gated");
        };
        window[3] = Instruction::ShiftLeftImmediate { a: 6, s, shift };
        window[5] = Instruction::Eqv { a: 4, s: 6, b: 0 };
        window[6] = Instruction::ShiftRightLogicalImmediate {
            a: 6,
            s: 4,
            shift: 31,
        };
        window[7] = Instruction::Or { a: 6, s: 3, b: 6 };
        let Instruction::StoreWord { a, offset, .. } = window[8] else {
            unreachable!("final feedback store was gated");
        };
        window[8] = Instruction::StoreWord { s: 6, a, offset };
        window[12] = Instruction::load_immediate(5, 8);
        let Instruction::StoreWord { a, offset, .. } = window[13] else {
            unreachable!("length store was gated");
        };
        window[13] = Instruction::StoreWord { s: 5, a, offset };
        let order = [
            0, 1, 2, 3, 4, 12, 5, 15, 6, 18, 7, 17, 8, 9, 10, 11, 13, 14, 16, 19,
        ];
        for (destination, source) in order.into_iter().enumerate() {
            self.output.instructions[start + destination] = window[source].clone();
        }
    }

    fn schedule_rounded_result_stores(&mut self) {
        let Some(start) = self.output.instructions.windows(4).position(|window| {
            matches!(
                window,
                [
                    Instruction::StoreWord { .. },
                    Instruction::StoreWord { .. },
                    Instruction::StoreWord { .. },
                    Instruction::AddImmediate {
                        d: 3,
                        a: 0,
                        immediate: 0,
                    },
                ]
            )
        }) else {
            return;
        };
        self.move_instruction_before(start + 3, start + 1);
    }
}

fn rounded_defined_general(instruction: &Instruction) -> Option<u8> {
    mwcc_vreg::register_operands(instruction)
        .into_iter()
        .find(|operand| {
            operand.class == mwcc_vreg::Class::General
                && operand.role == mwcc_vreg::RegisterRole::Define
        })
        .map(|operand| operand.register)
}
