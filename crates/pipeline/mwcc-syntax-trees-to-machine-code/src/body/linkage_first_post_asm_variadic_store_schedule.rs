//! Final issue order for a post-asm fixed-address store before a variadic call.
//!
//! Build 163 keeps the linkage packet strict after an earlier assembly body.
//! After the first call, it issues the variadic CR setup before starting a
//! fixed-address zero store, then fills that store-address latency slot with
//! the independent string-address high half. Selection keeps the store and
//! call arguments in source order; this pass recognizes their complete
//! physical packet and applies the measured permutation.

use super::*;
use mwcc_machine_code::RelocationTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    start: usize,
}

impl Generator {
    pub(crate) fn schedule_linkage_first_post_asm_variadic_store(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.preceded_by_asm
        {
            return;
        }
        let Some(plan) = recognize(&self.output, &self.variadic_callees) else {
            return;
        };

        let start = plan.start;
        crate::move_instruction_before_retargeting(self, start + 5, start + 1);
        crate::move_instruction_before_retargeting(self, start + 4, start + 3);
    }
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
    variadic_callees: &std::collections::HashSet<String>,
) -> Option<Plan> {
    output
        .instructions
        .windows(7)
        .enumerate()
        .find_map(|(start, window)| {
            let [
                Instruction::AddImmediate {
                    d: zero,
                    a: 0,
                    immediate: 0,
                },
                Instruction::AddImmediateShifted {
                    d: fixed_address,
                    a: 0,
                    ..
                },
                Instruction::StoreWord {
                    s: stored_zero,
                    a: store_address,
                    ..
                },
                Instruction::AddImmediateShifted {
                    d: 3,
                    a: 0,
                    ..
                },
                Instruction::AddImmediate { d: 3, a: 3, .. },
                Instruction::ConditionRegisterClear { d: 6 },
                Instruction::BranchAndLink { target },
            ] = window
            else {
                return None;
            };
            if zero != stored_zero
                || fixed_address != store_address
                || zero == fixed_address
                || !variadic_callees.contains(target)
                || output
                    .relocations
                    .iter()
                    .any(|relocation| relocation.instruction_index == start + 1)
            {
                return None;
            }
            let string = external_target(output, start + 3, RelocationKind::Addr16Ha)?;
            if external_target(output, start + 4, RelocationKind::Addr16Lo) != Some(string)
                || output.instructions.iter().any(|instruction| {
                    matches!(
                        instruction,
                        Instruction::BranchConditionalForward { target, .. }
                            | Instruction::Branch { target }
                            if (start..start + 7).contains(target)
                    )
                })
            {
                return None;
            }
            Some(Plan { start })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn relocation(instruction_index: usize, kind: RelocationKind, target: &str) -> Relocation {
        Relocation {
            instruction_index,
            kind,
            target: RelocationTarget::External(target.into()),
        }
    }

    fn packet() -> mwcc_machine_code::MachineFunction {
        mwcc_machine_code::MachineFunction {
            instructions: vec![
                Instruction::load_immediate(0, 0),
                Instruction::load_immediate_shifted(4, -32768),
                Instruction::StoreWord {
                    s: 0,
                    a: 4,
                    offset: 216,
                },
                Instruction::load_immediate_shifted(3, 0),
                Instruction::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: 0,
                },
                Instruction::ConditionRegisterClear { d: 6 },
                Instruction::BranchAndLink {
                    target: "DBPrintf".into(),
                },
            ],
            relocations: vec![
                relocation(3, RelocationKind::Addr16Ha, "@@str0"),
                relocation(4, RelocationKind::Addr16Lo, "@@str0"),
                relocation(6, RelocationKind::Rel24, "DBPrintf"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn recognizes_the_fixed_store_variadic_packet() {
        let variadic = std::collections::HashSet::from(["DBPrintf".into()]);
        assert_eq!(recognize(&packet(), &variadic), Some(Plan { start: 0 }));
    }

    #[test]
    fn rejects_a_relocatable_store_address() {
        let mut output = packet();
        output.relocations.push(relocation(
            1,
            RelocationKind::Addr16Ha,
            "ordinary_global",
        ));
        let variadic = std::collections::HashSet::from(["DBPrintf".into()]);
        assert_eq!(recognize(&output, &variadic), None);
    }

    #[test]
    fn rejects_a_control_flow_entry_into_the_packet() {
        let mut output = packet();
        output.instructions.insert(0, Instruction::Branch { target: 3 });
        for relocation in &mut output.relocations {
            relocation.instruction_index += 1;
        }
        let variadic = std::collections::HashSet::from(["DBPrintf".into()]);
        assert_eq!(recognize(&output, &variadic), None);
    }
}
