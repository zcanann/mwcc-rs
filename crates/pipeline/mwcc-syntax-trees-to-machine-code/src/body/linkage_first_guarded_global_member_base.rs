//! Reuse an absolute global-member address on a guarded zero-store edge.
//!
//! Build 163 completes the member address before testing its value, then keeps
//! that address live on the taken edge for a store back to the same member.
//! Independent expression lowering instead rematerializes the global in the
//! arm.  This physical pass joins those two selections after allocation has
//! exposed their common base register and relocation target.

use super::*;
use mwcc_machine_code::RelocationTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    load: usize,
    repeated_high: usize,
    repeated_low: usize,
    store: usize,
    base: u8,
    offset: i16,
}

fn external_target(
    output: &mwcc_machine_code::MachineFunction,
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&str> {
    output.relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != instruction_index || relocation.kind != kind {
            return None;
        }
        match &relocation.target {
            RelocationTarget::External(target) => Some(target.as_str()),
            _ => None,
        }
    })
}

fn recognize(
    output: &mwcc_machine_code::MachineFunction,
    globals: &std::collections::HashMap<String, Type>,
) -> Option<Plan> {
    for load in 0..output.instructions.len().saturating_sub(2) {
        let [
            Instruction::LoadWord {
                d: condition,
                a: base,
                offset,
            },
            Instruction::CompareWordImmediate {
                a: compared,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                target: false_edge,
                ..
            },
        ] = &output.instructions[load..load + 3]
        else {
            continue;
        };
        if condition != compared || *offset == 0 || *false_edge <= load + 3 {
            continue;
        }

        let Some(first_low) = (0..load).rev().find(|index| {
            matches!(
                output.instructions[*index],
                Instruction::AddImmediate {
                    d,
                    a,
                    ..
                } if d == *base && a == *base
            ) && external_target(output, *index, RelocationKind::Addr16Lo).is_some()
        }) else {
            continue;
        };
        let Some(target) = external_target(output, first_low, RelocationKind::Addr16Lo) else {
            continue;
        };
        let Some(first_high) = (0..first_low).rev().find(|index| {
            matches!(
                output.instructions[*index],
                Instruction::AddImmediateShifted { d, a: 0, .. } if d == *base
            ) && external_target(output, *index, RelocationKind::Addr16Ha) == Some(target)
        }) else {
            continue;
        };
        if output.instructions[first_high + 1..first_low]
            .iter()
            .any(|instruction| {
                mwcc_vreg::register_operands(instruction)
                    .into_iter()
                    .any(|operand| {
                        operand.role == mwcc_vreg::RegisterRole::Define
                            && operand.register == *base
                    })
            })
        {
            continue;
        }
        if !matches!(globals.get(target), Some(Type::Struct { .. })) {
            continue;
        }

        let true_edge = load + 3;
        if *false_edge != true_edge + 5 {
            continue;
        }
        let [
            Instruction::AddImmediateShifted {
                d: repeated_base,
                a: 0,
                ..
            },
            Instruction::AddImmediate {
                d: repeated_low_base,
                a: repeated_low_source,
                ..
            },
            Instruction::AddImmediate {
                d: zero,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: stored,
                a: store_base,
                offset: store_offset,
            },
            Instruction::Branch { target: join },
        ] = &output.instructions[true_edge..*false_edge]
        else {
            continue;
        };
        if repeated_base != repeated_low_base
            || repeated_base != repeated_low_source
            || zero != stored
            || store_base != repeated_base
            || store_offset != offset
            || *join <= *false_edge
            || external_target(output, true_edge, RelocationKind::Addr16Ha) != Some(target)
            || external_target(output, true_edge + 1, RelocationKind::Addr16Lo) != Some(target)
        {
            continue;
        }
        return Some(Plan {
            load,
            repeated_high: true_edge,
            repeated_low: true_edge + 1,
            store: true_edge + 3,
            base: *base,
            offset: *offset,
        });
    }
    None
}

impl Generator {
    pub(crate) fn reuse_linkage_first_guarded_global_member_base(&mut self) {
        if self.behavior.frame_convention != mwcc_versions::FrameConvention::LinkageFirst {
            return;
        }
        let Some(plan) = recognize(&self.output, &self.addressable_globals) else {
            return;
        };
        let Instruction::LoadWord { offset, .. } = &mut self.output.instructions[plan.load]
        else {
            unreachable!("the guarded member plan owns its condition load")
        };
        *offset = 0;
        let Instruction::StoreWord { a, offset, .. } =
            &mut self.output.instructions[plan.store]
        else {
            unreachable!("the guarded member plan owns its zero store")
        };
        *a = plan.base;
        *offset = 0;

        crate::remove_instruction_retargeting_to_next(self, plan.repeated_low);
        crate::remove_instruction_retargeting_to_next(self, plan.repeated_high);
        crate::insert_instruction_retargeting(
            self,
            plan.load,
            Instruction::AddImmediate {
                d: plan.base,
                a: plan.base,
                immediate: plan.offset,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn relocation(index: usize, kind: RelocationKind, target: &str) -> Relocation {
        Relocation {
            instruction_index: index,
            kind,
            target: RelocationTarget::External(target.into()),
        }
    }

    fn guarded_member(target: &str) -> mwcc_machine_code::MachineFunction {
        mwcc_machine_code::MachineFunction {
            instructions: vec![
                Instruction::AddImmediateShifted {
                    d: 3,
                    a: 0,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: 0,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 3,
                    offset: 156,
                },
                Instruction::CompareWordImmediate { a: 0, immediate: 0 },
                Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: 10,
                },
                Instruction::AddImmediateShifted {
                    d: 3,
                    a: 0,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 3,
                    offset: 156,
                },
                Instruction::Branch { target: 11 },
                Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
                Instruction::BranchToLinkRegister,
            ],
            relocations: vec![
                relocation(0, RelocationKind::Addr16Ha, target),
                relocation(1, RelocationKind::Addr16Lo, target),
                relocation(5, RelocationKind::Addr16Ha, target),
                relocation(6, RelocationKind::Addr16Lo, target),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn recognizes_a_guarded_zero_store_to_the_tested_member() {
        let globals = std::collections::HashMap::from([(
            "state".into(),
            Type::Struct {
                size: 160,
                align: 4,
            },
        )]);

        assert_eq!(
            recognize(&guarded_member("state"), &globals),
            Some(Plan {
                load: 2,
                repeated_high: 5,
                repeated_low: 6,
                store: 8,
                base: 3,
                offset: 156,
            })
        );
    }

    #[test]
    fn rejects_a_reload_of_a_different_global() {
        let mut output = guarded_member("state");
        output.relocations[2].target = RelocationTarget::External("other".into());
        let globals = std::collections::HashMap::from([(
            "state".into(),
            Type::Struct {
                size: 160,
                align: 4,
            },
        )]);

        assert_eq!(recognize(&output, &globals), None);
    }
}
