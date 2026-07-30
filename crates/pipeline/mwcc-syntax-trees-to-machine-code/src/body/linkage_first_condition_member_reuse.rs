//! Reuse a loaded pointer member on a call-free conditional edge.
//!
//! A linkage-first branch may call on its true edge and immediately exit while
//! the false edge compares the same member again. MWCC keeps the first member
//! value in its destination on that false edge instead of retaining the base
//! pointer and reloading the member.

use super::forward_pointer_global_copy::{pointer_type, sda_target};
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    base_load: usize,
    reload: usize,
    member: u8,
    offset: i16,
}

fn recognize(
    output: &mwcc_machine_code::MachineFunction,
    globals: &std::collections::HashMap<String, Type>,
) -> Option<Plan> {
    output
        .instructions
        .windows(5)
        .enumerate()
        .find_map(|(base_load, window)| {
            let [Instruction::LoadWord {
                d: base,
                a: 0,
                offset: 0,
            }, Instruction::LoadWord {
                d: member,
                a: member_base,
                offset,
            }, Instruction::AddImmediateShifted {
                d: scratch,
                a: compared_member,
                ..
            }, comparison, Instruction::BranchConditionalForward { target: reload, .. }] = window
            else {
                return None;
            };
            if base != member_base
                || base == member
                || member != compared_member
                || *offset == 0
                || !matches!(
                    comparison,
                    Instruction::CompareLogicalWordImmediate { a, .. }
                        | Instruction::CompareWordImmediate { a, .. }
                        if a == scratch
                )
            {
                return None;
            }
            let Some(Instruction::LoadWord {
                d: repeated_member,
                a: repeated_base,
                offset: repeated_offset,
            }) = output.instructions.get(*reload)
            else {
                return None;
            };
            let pointer_global = sda_target(output, base_load)?;
            if member != repeated_member
                || base != repeated_base
                || offset != repeated_offset
                || !pointer_type(globals.get(pointer_global))
                || !output.instructions[base_load + 5..*reload]
                    .iter()
                    .any(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
                || !output.instructions[base_load + 5..*reload]
                    .iter()
                    .any(|instruction| {
                        matches!(instruction, Instruction::Branch { target } if *target > *reload)
                    })
            {
                return None;
            }
            Some(Plan {
                base_load,
                reload: *reload,
                member: *member,
                offset: *offset,
            })
        })
}

impl Generator {
    pub(crate) fn reuse_linkage_first_condition_member(&mut self) {
        if self.behavior.frame_convention != mwcc_versions::FrameConvention::LinkageFirst {
            return;
        }
        let Some(plan) = recognize(&self.output, &self.globals) else {
            return;
        };
        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[plan.base_load] else {
            unreachable!("the condition member plan owns its global load");
        };
        *d = plan.member;
        self.output.instructions[plan.base_load + 1] = Instruction::AddImmediate {
            d: plan.member,
            a: plan.member,
            immediate: plan.offset,
        };
        crate::insert_instruction_retargeting(
            self,
            plan.base_load + 2,
            Instruction::LoadWord {
                d: plan.member,
                a: plan.member,
                offset: 0,
            },
        );
        crate::remove_instruction_retargeting_to_next(self, plan.reload + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    #[test]
    fn recognizes_a_member_reloaded_only_on_the_call_free_edge() {
        let output = mwcc_machine_code::MachineFunction {
            instructions: vec![
                Instruction::LoadWord {
                    d: 5,
                    a: 0,
                    offset: 0,
                },
                Instruction::LoadWord {
                    d: 3,
                    a: 5,
                    offset: 32,
                },
                Instruction::AddImmediateShifted {
                    d: 0,
                    a: 3,
                    immediate: 1,
                },
                Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 },
                Instruction::BranchConditionalForward {
                    options: 4,
                    condition_bit: 2,
                    target: 8,
                },
                Instruction::BranchAndLink {
                    target: "report".into(),
                },
                Instruction::BranchAndLink {
                    target: "load".into(),
                },
                Instruction::Branch { target: 10 },
                Instruction::LoadWord {
                    d: 3,
                    a: 5,
                    offset: 32,
                },
                Instruction::CompareLogicalWordImmediate { a: 3, immediate: 3 },
                Instruction::BranchToLinkRegister,
            ],
            relocations: vec![Relocation {
                instruction_index: 0,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::External("boot".into()),
            }],
            ..Default::default()
        };
        let globals = [("boot".into(), Type::Pointer(Pointee::UnsignedInt))]
            .into_iter()
            .collect();

        assert_eq!(
            recognize(&output, &globals),
            Some(Plan {
                base_load: 0,
                reload: 8,
                member: 3,
                offset: 32,
            })
        );
    }
}
