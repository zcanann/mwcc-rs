//! Schedule a build-163 pointer publication around callback installation.
//!
//! The pointer value, saved zero, callback address, and interrupt argument are
//! independent until their stores and call. MWCC interleaves them to fill the
//! latency slots while retaining the pointer for its mirrored global.

use super::forward_pointer_global_copy::{pointer_type, sda_target};
use super::*;
use mwcc_machine_code::RelocationTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    start: usize,
}

fn relocation_target(
    output: &mwcc_machine_code::MachineFunction,
    index: usize,
    kind: RelocationKind,
) -> Option<&str> {
    output.relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != index || relocation.kind != kind {
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
    output
        .instructions
        .windows(9)
        .enumerate()
        .find_map(|(start, window)| {
            let [Instruction::AddImmediate {
                d: zero,
                a: 0,
                immediate: 0,
            }, Instruction::StoreWord {
                s: zero_source,
                a: 0,
                offset: 0,
            }, Instruction::AddImmediateShifted {
                d: pointer, a: 0, ..
            }, Instruction::StoreWord {
                s: published,
                a: 0,
                offset: 0,
            }, Instruction::StoreWord {
                s: copied,
                a: 0,
                offset: 0,
            }, Instruction::AddImmediate {
                d: argument, a: 0, ..
            }, Instruction::AddImmediateShifted {
                d: callback_high,
                a: 0,
                ..
            }, Instruction::AddImmediate {
                d: callback,
                a: callback_source,
                ..
            }, Instruction::BranchAndLink { .. }] = window
            else {
                return None;
            };
            if !(14..=31).contains(zero)
                || zero != zero_source
                || *pointer != GENERAL_SCRATCH
                || pointer != published
                || pointer != copied
                || *argument != Eabi::FIRST_GENERAL_ARGUMENT
                || callback_high != callback
                || callback_high != callback_source
            {
                return None;
            }
            let zero_global = sda_target(output, start + 1)?;
            let pointer_global = sda_target(output, start + 3)?;
            let copy_global = sda_target(output, start + 4)?;
            let callback_high_target =
                relocation_target(output, start + 6, RelocationKind::Addr16Ha)?;
            let callback_low_target =
                relocation_target(output, start + 7, RelocationKind::Addr16Lo)?;
            if zero_global == pointer_global
                || zero_global == copy_global
                || pointer_global == copy_global
                || pointer_type(globals.get(zero_global))
                || !pointer_type(globals.get(pointer_global))
                || !pointer_type(globals.get(copy_global))
                || callback_high_target != callback_low_target
                || output.instructions.iter().any(|instruction| {
                    matches!(
                        instruction,
                        Instruction::BranchConditionalForward { target, .. }
                            | Instruction::Branch { target }
                            if (start..start + 9).contains(target)
                    )
                })
            {
                return None;
            }
            Some(Plan { start })
        })
}

impl Generator {
    pub(crate) fn schedule_linkage_first_pointer_publication(&mut self) {
        if self.behavior.frame_convention != mwcc_versions::FrameConvention::LinkageFirst {
            return;
        }
        let Some(plan) = recognize(&self.output, &self.globals) else {
            return;
        };
        let start = plan.start;
        crate::move_instruction_before_retargeting(self, start + 2, start);
        crate::move_instruction_before_retargeting(self, start + 3, start + 2);
        crate::move_instruction_before_retargeting(self, start + 6, start + 3);
        crate::move_instruction_before_retargeting(self, start + 7, start + 4);
        crate::move_instruction_before_retargeting(self, start + 7, start + 6);
        let Instruction::AddImmediateShifted { immediate, .. } =
            self.output.instructions[start + 3]
        else {
            unreachable!("the pointer publication callback high half was recognized");
        };
        self.output.instructions[start + 3] = Instruction::AddImmediateShifted {
            d: Eabi::FIRST_GENERAL_ARGUMENT,
            a: 0,
            immediate,
        };
        let Instruction::AddImmediate { immediate, .. } = self.output.instructions[start + 4]
        else {
            unreachable!("the pointer publication callback low half was recognized");
        };
        self.output.instructions[start + 4] = Instruction::AddImmediate {
            d: Eabi::FIRST_GENERAL_ARGUMENT + 1,
            a: Eabi::FIRST_GENERAL_ARGUMENT,
            immediate,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::Relocation;

    fn relocation(instruction_index: usize, kind: RelocationKind, target: &str) -> Relocation {
        Relocation {
            instruction_index,
            kind,
            target: RelocationTarget::External(target.into()),
        }
    }

    fn scheduled_publication() -> mwcc_machine_code::MachineFunction {
        mwcc_machine_code::MachineFunction {
            instructions: vec![
                Instruction::load_immediate(31, 0),
                Instruction::StoreWord {
                    s: 31,
                    a: 0,
                    offset: 0,
                },
                Instruction::load_immediate_shifted(0, -32768),
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::load_immediate(3, 21),
                Instruction::load_immediate_shifted(4, 0),
                Instruction::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: 0,
                },
                Instruction::BranchAndLink {
                    target: "install".into(),
                },
            ],
            relocations: vec![
                relocation(1, RelocationKind::EmbSda21, "state"),
                relocation(3, RelocationKind::EmbSda21, "source"),
                relocation(4, RelocationKind::EmbSda21, "destination"),
                relocation(6, RelocationKind::Addr16Ha, "callback"),
                relocation(7, RelocationKind::Addr16Lo, "callback"),
                relocation(8, RelocationKind::Rel24, "install"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn recognizes_the_pointer_publication_schedule() {
        let globals = [
            ("state".into(), Type::UnsignedInt),
            ("source".into(), Type::Pointer(Pointee::UnsignedInt)),
            ("destination".into(), Type::Pointer(Pointee::UnsignedInt)),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            recognize(&scheduled_publication(), &globals),
            Some(Plan { start: 0 })
        );
    }
}
