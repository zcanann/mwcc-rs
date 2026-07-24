//! Body scheduling for a structured loop with retained assertion strings.
//!
//! The schedule has two independent latency fills: a member-float load moves
//! directly behind the preceding call, and a canonical boolean copy moves ahead
//! of an unrelated member load while preparing the loop's virtual call.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_loop_assertion_body(&mut self) {
        if self.loop_assertion_string_highs.len() != 2 {
            return;
        }
        let canonical_boolean_homes: Vec<u8> = self
            .canonical_boolean_locals
            .iter()
            .filter_map(|name| self.lookup_general(name))
            .collect();
        if let Some(start) =
            post_call_float_schedule_start(&self.output.instructions, &canonical_boolean_homes)
        {
            let Instruction::LoadFloatSingle { d, .. } =
                &mut self.output.instructions[start + 3]
            else {
                unreachable!("float schedule recognition guarantees a load");
            };
            *d = FLOAT_SCRATCH;
            let Instruction::StoreFloatSingle { s, .. } =
                &mut self.output.instructions[start + 4]
            else {
                unreachable!("float schedule recognition guarantees a store");
            };
            *s = FLOAT_SCRATCH;
            self.move_instruction_before(start + 3, start + 1);
        }

        if let Some(start) =
            canonical_argument_schedule_start(&self.output.instructions, &canonical_boolean_homes)
        {
            self.move_instruction_before(start + 2, start + 1);
        }
    }
}

fn post_call_float_schedule_start(
    instructions: &[Instruction],
    canonical_boolean_homes: &[u8],
) -> Option<usize> {
    instructions.windows(5).position(|window| {
        matches!(
            window,
            [
                Instruction::BranchToCountRegisterAndLink,
                Instruction::AddImmediate {
                    d: boolean,
                    a: 0,
                    immediate: 1,
                },
                Instruction::LoadWord {
                    d: pointer,
                    a: pointer_base,
                    ..
                },
                Instruction::LoadFloatSingle {
                    d: loaded,
                    a: value_base,
                    ..
                },
                Instruction::StoreFloatSingle {
                    s: stored,
                    a: store_base,
                    ..
                },
            ] if canonical_boolean_homes.contains(boolean)
                && pointer_base == value_base
                && loaded == stored
                && pointer == store_base
        )
    })
}

fn canonical_argument_schedule_start(
    instructions: &[Instruction],
    canonical_boolean_homes: &[u8],
) -> Option<usize> {
    instructions.windows(4).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord { d: 3, .. },
                Instruction::LoadWord { d: 4, .. },
                Instruction::Or {
                    a: 5,
                    s: boolean,
                    b: boolean_copy,
                },
                Instruction::AddImmediate { d: 6, a: 0, .. },
            ] if boolean == boolean_copy && canonical_boolean_homes.contains(boolean)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_post_call_float_latency_region() {
        let instructions = vec![
            Instruction::BranchToCountRegisterAndLink,
            Instruction::load_immediate(27, 1),
            Instruction::LoadWord {
                d: 3,
                a: 26,
                offset: 36,
            },
            Instruction::LoadFloatSingle {
                d: 1,
                a: 26,
                offset: 12,
            },
            Instruction::StoreFloatSingle {
                s: 1,
                a: 3,
                offset: 16,
            },
        ];

        assert_eq!(
            post_call_float_schedule_start(&instructions, &[27]),
            Some(0)
        );
        assert_eq!(post_call_float_schedule_start(&instructions, &[28]), None);
    }

    #[test]
    fn recognizes_the_canonical_boolean_argument_copy() {
        let instructions = vec![
            Instruction::LoadWord {
                d: 3,
                a: 31,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 4,
                a: 26,
                offset: 36,
            },
            Instruction::move_register(5, 27),
            Instruction::load_immediate(6, 0),
        ];

        assert_eq!(
            canonical_argument_schedule_start(&instructions, &[27]),
            Some(0)
        );
        assert_eq!(
            canonical_argument_schedule_start(&instructions, &[28]),
            None
        );
    }
}
