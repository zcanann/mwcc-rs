//! Build-163 issue order for a guarded deferred-local publication.
//!
//! The structured owner has already established the local's saved lifetime and
//! retained optimizer lane. This final pass recognizes the independent global
//! address, volatile publication stores, and saved-local load as one scheduling
//! region, then applies the measured latency-filling order.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::RelocationTarget;

impl Generator {
    pub(crate) fn schedule_retained_guarded_local_publication(&mut self) {
        if self.legacy_callee_saved_frame_layout
            != LegacyCalleeSavedFrameLayout::RetainGuardedLocalLane
        {
            return;
        }
        let Some(plan) = retained_guarded_local_publication(&self.output) else {
            return;
        };

        crate::move_instruction_before_retargeting(self, plan.start + 4, plan.start);
        crate::move_instruction_before_retargeting(self, plan.start + 5, plan.start + 4);
        crate::move_instruction_before_retargeting(self, plan.start + 7, plan.start + 6);

        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[plan.start]
        else {
            unreachable!("the guarded publication address high was matched")
        };
        *d = 4;
        let Instruction::AddImmediate {
            d: low_destination,
            a: low_base,
            ..
        } = &mut self.output.instructions[plan.start + 4]
        else {
            unreachable!("the guarded publication address low was matched")
        };
        *low_destination = 3;
        *low_base = 4;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[plan.start + 7] else {
            unreachable!("the guarded publication pointer store was matched")
        };
        *s = 3;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RetainedGuardedLocalPublication {
    start: usize,
}

fn retained_guarded_local_publication(
    output: &mwcc_machine_code::MachineFunction,
) -> Option<RetainedGuardedLocalPublication> {
    let start = output.instructions.windows(9).position(|window| {
        matches!(
            window,
            [
                Instruction::StoreWord {
                    s: 3,
                    a: 0,
                    offset: 0,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0,
                },
                Instruction::LoadWord {
                    d: saved,
                    a: 0,
                    offset: 0,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::AddImmediateShifted {
                    d: high,
                    a: 0,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: low,
                    a: low_base,
                    immediate: 0,
                },
                Instruction::StoreWord {
                    s: stored,
                    a: 0,
                    offset: 0,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 10,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: state_base,
                    ..
                },
            ] if *saved >= 14
                && saved == state_base
                && high == low_base
                && low == stored
        )
    })?;

    let address = external_relocation_target(output, start + 4, RelocationKind::Addr16Ha)?;
    if external_relocation_target(output, start + 5, RelocationKind::Addr16Lo)? != address {
        return None;
    }
    let saved_source = external_relocation_target(output, start + 2, RelocationKind::EmbSda21)?;
    if external_relocation_target(output, start + 6, RelocationKind::EmbSda21)? != saved_source {
        return None;
    }
    for index in [start, start + 3] {
        external_relocation_target(output, index, RelocationKind::EmbSda21)?;
    }

    Some(RetainedGuardedLocalPublication { start })
}

fn external_relocation_target(
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

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::Relocation;

    #[test]
    fn recognizes_the_guarded_saved_local_publication_region() {
        let mut output = mwcc_machine_code::MachineFunction::new("publish");
        output.instructions = vec![
            Instruction::StoreWord {
                s: 3,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate(0, 0),
            Instruction::LoadWord {
                d: 31,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate(0, 10),
            Instruction::StoreWord {
                s: 0,
                a: 31,
                offset: 12,
            },
        ];
        for (instruction_index, kind, target) in [
            (0, RelocationKind::EmbSda21, "resume"),
            (2, RelocationKind::EmbSda21, "executing"),
            (3, RelocationKind::EmbSda21, "canceling"),
            (4, RelocationKind::Addr16Ha, "dummy"),
            (5, RelocationKind::Addr16Lo, "dummy"),
            (6, RelocationKind::EmbSda21, "executing"),
        ] {
            output.relocations.push(Relocation {
                instruction_index,
                kind,
                target: RelocationTarget::External(target.into()),
            });
        }

        assert_eq!(
            retained_guarded_local_publication(&output),
            Some(RetainedGuardedLocalPublication { start: 0 })
        );
    }
}
