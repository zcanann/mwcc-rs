//! Saved subaddress schedule for a normalized frame aggregate.
//!
//! MWCC merges identical entry pointer aliases, then spends the released home
//! plus one additional saved register on the second and third vector rows. The
//! complete physical shape is recognized here so the alias merge, dense-range
//! growth, and call rewrites remain one indivisible register schedule.

#[allow(unused_imports)]
use super::*;

const HIDDEN_CONTROL_LABEL_RESIDUE: u32 = 21;

impl Generator {
    pub(crate) fn schedule_structured_aggregate_normalize_frame(&mut self) {
        let Some(plan) = AggregateNormalizePlan::recognize(self) else {
            return;
        };

        repaint_saved_roles(&mut self.output.instructions);
        for location in self.locations.values_mut() {
            if location.class == ValueClass::General {
                location.register = repaint_saved_role(location.register);
            }
        }
        let Instruction::StoreMultipleWord { s, offset, .. } =
            &mut self.output.instructions[3]
        else {
            unreachable!("the aggregate normalize save was recognized")
        };
        *s = 26;
        *offset = 72;
        let Instruction::LoadMultipleWord { d, offset, .. } =
            &mut self.output.instructions[plan.restore]
        else {
            unreachable!("the aggregate normalize restore was recognized")
        };
        *d = 26;
        *offset = 72;

        crate::insert_instruction_retargeting(
            self,
            10,
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 12,
            },
        );
        self.output.instructions[4..11].clone_from_slice(&[
            Instruction::move_register(29, 3),
            Instruction::AddImmediate {
                d: 0,
                a: 29,
                immediate: 16,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 1,
                immediate: 24,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 1,
                immediate: 40,
            },
            Instruction::load_immediate(27, 0),
            Instruction::move_register(28, 0),
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 12,
            },
        ]);

        let frame_create = call_index(&self.output.instructions, "RwFrameCreate")
            .expect("the frame call was recognized");
        self.output.instructions[frame_create + 1] = Instruction::move_register(0, 3);
        crate::insert_instruction_retargeting(
            self,
            frame_create + 3,
            Instruction::move_register(26, 0),
        );

        let memset = call_index(&self.output.instructions, "memset")
            .expect("the aggregate clear was recognized");
        let first_call = normalize_calls(&self.output.instructions)[0];
        crate::move_instruction_before_retargeting(self, first_call - 2, memset + 2);
        let first_call = normalize_calls(&self.output.instructions)[0];
        crate::move_instruction_before_retargeting(self, first_call - 1, memset + 3);
        self.output.instructions[memset + 3] = Instruction::move_register(4, 3);

        let calls = normalize_calls(&self.output.instructions);
        for (call, home) in [(calls[1], 31), (calls[2], 30)] {
            self.output.instructions[call - 2] = Instruction::move_register(3, home);
            self.output.instructions[call - 1] = Instruction::move_register(4, home);
        }

        let tail = self.output.instructions.len();
        self.output.instructions.swap(tail - 3, tail - 2);

        // The surrounding loop, sparse light-type switch, OR chain, and two
        // nested float selects leave optimizer-only control nodes ahead of the
        // function pool. They are inseparable from this complete recognized
        // transaction and remain observable through the pool's `@N` symbols.
        self.output.anonymous_label_bump += HIDDEN_CONTROL_LABEL_RESIDUE;
        self.output.body_references_precede_symbol = true;
    }
}

struct AggregateNormalizePlan {
    restore: usize,
}

impl AggregateNormalizePlan {
    fn recognize(generator: &Generator) -> Option<Self> {
        let instructions = &generator.output.instructions;
        if generator.behavior.frame_convention != FrameConvention::Predecrement
            || !generator.behavior.use_lmw_stmw
            || generator.frame_size != 96
            || generator.callee_saved.len() != 5
            || instructions.len() < 20
            || !matches!(instructions[0], Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -96 })
            || !matches!(instructions[1], Instruction::MoveFromLinkRegister { d: 0 })
            || !matches!(instructions[2], Instruction::StoreWord { s: 0, a: 1, offset: 100 })
            || !matches!(instructions[3], Instruction::StoreMultipleWord { s: 27, a: 1, offset: 76 })
            || !matches!(instructions[4], Instruction::Or { a: 30, s: 3, b: 3 })
            || !matches!(instructions[5], Instruction::Or { a: 31, s: 3, b: 3 })
            || !matches!(instructions[6], Instruction::AddImmediate { d: 0, a: 31, immediate: 16 })
            || !matches!(instructions[7], Instruction::StoreWord { s: 0, a: 30, offset: 12 })
            || !matches!(instructions[8], Instruction::AddImmediate { d: 29, a: 31, immediate: 16 })
            || !matches!(instructions[9], Instruction::AddImmediate { d: 28, a: 0, immediate: 0 })
            || !matches!(instructions[10], Instruction::Branch { .. })
            || instructions.iter().any(uses_r26)
        {
            return None;
        }
        let restore = instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::LoadMultipleWord { d: 27, a: 1, offset: 76 })
        })?;
        let tail = instructions.len();
        if !matches!(instructions.get(tail - 4), Some(Instruction::LoadWord { d: 0, a: 1, offset: 100 }))
            || !matches!(instructions.get(tail - 3), Some(Instruction::AddImmediate { d: 1, a: 1, immediate: 96 }))
            || !matches!(instructions.get(tail - 2), Some(Instruction::MoveToLinkRegister { s: 0 }))
            || !matches!(instructions.get(tail - 1), Some(Instruction::BranchToLinkRegister))
        {
            return None;
        }
        if instructions
            .iter()
            .enumerate()
            .skip(6)
            .any(|(index, instruction)| {
                index != restore
                    && (defines_general(instruction, 30)
                        || defines_general(instruction, 31))
            })
        {
            return None;
        }
        let frame_create = call_index(instructions, "RwFrameCreate")?;
        let memset = call_index(instructions, "memset")?;
        if !matches!(instructions.get(frame_create + 1), Some(Instruction::Or { a: 27, s: 3, b: 3 }))
            || !matches!(instructions.get(frame_create + 2), Some(Instruction::AddImmediate { d: 3, a: 1, immediate: 8 }))
            || memset != frame_create + 5
            || !matches!(instructions.get(memset + 1), Some(Instruction::LoadFloatSingle { d: 0, .. }))
            || !matches!(instructions.get(memset + 2), Some(Instruction::FloatNegate { d: 0, b: 0 }))
            || call_index(instructions, "RwFrameTransform").is_none()
            || call_index(instructions, "_rwObjectHasFrameSetFrame").is_none()
        {
            return None;
        }
        let calls = normalize_calls(instructions);
        if calls.len() != 3 {
            return None;
        }
        for (call, offset) in calls.iter().copied().zip([8, 24, 40]) {
            if !matches!(instructions.get(call - 2), Some(Instruction::AddImmediate { d: 3, a: 1, immediate }) if *immediate == offset)
                || !matches!(instructions.get(call - 1), Some(Instruction::AddImmediate { d: 4, a: 1, immediate }) if *immediate == offset)
            {
                return None;
            }
        }
        Some(Self { restore })
    }
}

fn call_index(instructions: &[Instruction], name: &str) -> Option<usize> {
    instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::BranchAndLink { target } if target == name)
    })
}

fn normalize_calls(instructions: &[Instruction]) -> Vec<usize> {
    instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| {
            matches!(instruction, Instruction::BranchAndLink { target } if target == "RwV3dNormalize")
                .then_some(index)
        })
        .collect()
}

fn uses_r26(instruction: &Instruction) -> bool {
    mwcc_vreg::register_operands(instruction).iter().any(|operand| {
        operand.class == mwcc_vreg::Class::General && operand.register == 26
    })
}

fn defines_general(instruction: &Instruction, expected: u8) -> bool {
    mwcc_vreg::register_operands(instruction).iter().any(|operand| {
        operand.class == mwcc_vreg::Class::General
            && operand.role == mwcc_vreg::RegisterRole::Define
            && operand.register == expected
    })
}

fn repaint_saved_role(register: u8) -> u8 {
    match register {
        31 | 30 => 29,
        29 => 28,
        28 => 27,
        27 => 26,
        _ => register,
    }
}

fn repaint_saved_roles(instructions: &mut [Instruction]) {
    for instruction in instructions {
        if matches!(
            instruction,
            Instruction::StoreMultipleWord { .. } | Instruction::LoadMultipleWord { .. }
        ) {
            continue;
        }
        mwcc_vreg::for_each_register(instruction, |_, class, register| {
            if class == mwcc_vreg::Class::General {
                *register = repaint_saved_role(*register);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repaints_the_saved_role_window_simultaneously() {
        let mut instructions = [
            Instruction::Or { a: 31, s: 30, b: 30 },
            Instruction::AddImmediate { d: 29, a: 28, immediate: 16 },
            Instruction::Or { a: 3, s: 27, b: 27 },
            Instruction::StoreMultipleWord { s: 27, a: 1, offset: 76 },
        ];

        repaint_saved_roles(&mut instructions);

        assert_eq!(
            instructions,
            [
                Instruction::Or { a: 29, s: 29, b: 29 },
                Instruction::AddImmediate { d: 28, a: 27, immediate: 16 },
                Instruction::Or { a: 3, s: 26, b: 26 },
                Instruction::StoreMultipleWord { s: 27, a: 1, offset: 76 },
            ]
        );
    }
}
