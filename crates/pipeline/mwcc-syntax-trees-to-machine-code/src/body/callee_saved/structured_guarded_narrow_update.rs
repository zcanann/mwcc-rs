//! Reuse a narrow comparison value in its guarded read-modify-write tail.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Carry a halfword member loaded for a bound check into the taken arm's
    /// increment. Extending that value across the early-return edge both avoids
    /// a reload and gives the two pointer bases MWCC's measured allocation
    /// order.
    pub(super) fn reuse_guarded_narrow_member_update(&mut self) {
        let Some(plan) = guarded_narrow_member_update(&self.output.instructions) else {
            return;
        };
        self.prefer_virtual_general(plan.owner, 5);
        self.prefer_virtual_general(plan.attributes, 4);
        self.prefer_virtual_general(plan.value, Eabi::FIRST_GENERAL_ARGUMENT);
        match &mut self.output.instructions[plan.reload + 1] {
            Instruction::AddImmediate { a, .. } => *a = plan.value,
            _ => unreachable!("guarded narrow update shape was checked"),
        }
        self.remove_structured_condition_instruction(plan.reload);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GuardedNarrowUpdate {
    owner: u8,
    attributes: u8,
    value: u8,
    reload: usize,
}

fn guarded_narrow_member_update(instructions: &[Instruction]) -> Option<GuardedNarrowUpdate> {
    instructions
        .windows(13)
        .enumerate()
        .find_map(|(start, window)| match window {
            [Instruction::LoadWord { d: owner, a: 3, .. }, Instruction::LoadWord {
                d: attributes,
                a: attributes_base,
                ..
            }, Instruction::LoadHalfwordZero {
                d: value,
                a: value_base,
                offset: value_offset,
            }, Instruction::LoadWord {
                d: bound,
                a: bound_base,
                ..
            }, Instruction::CompareWord {
                a: compared,
                b: compared_bound,
            }, Instruction::BranchConditionalForward { .. }, Instruction::LoadHalfwordZero {
                d: reloaded,
                a: reload_base,
                offset: reload_offset,
            }, Instruction::AddImmediate {
                a: increment_source,
                immediate: 1,
                ..
            }, Instruction::StoreHalfword {
                a: store_base,
                offset: store_offset,
                ..
            }, Instruction::LoadFloatSingle {
                a: first_float_base,
                ..
            }, Instruction::LoadFloatSingle {
                a: second_float_base,
                ..
            }, Instruction::FloatMultiplySingle { .. }, Instruction::StoreFloatSingle {
                a: float_store_base,
                ..
            }] if owner == attributes_base
                && owner == value_base
                && owner == reload_base
                && owner == store_base
                && owner == first_float_base
                && owner == float_store_base
                && attributes == bound_base
                && attributes == second_float_base
                && value == compared
                && bound == compared_bound
                && reloaded == increment_source
                && value_offset == reload_offset
                && value_offset == store_offset =>
            {
                Some(GuardedNarrowUpdate {
                    owner: *owner,
                    attributes: *attributes,
                    value: *value,
                    reload: start + 6,
                })
            }
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_compared_halfword_reloaded_for_a_guarded_increment() {
        let owner = mwcc_vreg::Reg::general(0).to_field();
        let attributes = mwcc_vreg::Reg::general(1).to_field();
        let value = mwcc_vreg::Reg::general(2).to_field();
        let bound = mwcc_vreg::Reg::general(3).to_field();
        let reloaded = mwcc_vreg::Reg::general(4).to_field();
        let instructions = [
            Instruction::LoadWord {
                d: owner,
                a: 3,
                offset: 44,
            },
            Instruction::LoadWord {
                d: attributes,
                a: owner,
                offset: 724,
            },
            Instruction::LoadHalfwordZero {
                d: value,
                a: owner,
                offset: 9024,
            },
            Instruction::LoadWord {
                d: bound,
                a: attributes,
                offset: 120,
            },
            Instruction::CompareWord { a: value, b: bound },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 1,
                target: 13,
            },
            Instruction::LoadHalfwordZero {
                d: reloaded,
                a: owner,
                offset: 9024,
            },
            Instruction::AddImmediate {
                d: 0,
                a: reloaded,
                immediate: 1,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: owner,
                offset: 9024,
            },
            Instruction::LoadFloatSingle {
                d: 1,
                a: owner,
                offset: 9028,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: attributes,
                offset: 116,
            },
            Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 },
            Instruction::StoreFloatSingle {
                s: 0,
                a: owner,
                offset: 9028,
            },
        ];

        assert_eq!(
            guarded_narrow_member_update(&instructions),
            Some(GuardedNarrowUpdate {
                owner,
                attributes,
                value,
                reload: 6,
            })
        );
    }
}
