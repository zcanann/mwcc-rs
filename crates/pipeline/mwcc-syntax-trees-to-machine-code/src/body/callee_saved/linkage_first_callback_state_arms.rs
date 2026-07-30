//! Linkage-first scheduling for callback state-publication arms.
//!
//! A switch arm loads a global state object, publishes an immediate member,
//! then passes a function address to a direct call. Build 163 keeps the object
//! only through the store, placing it in an argument lane the callback does not
//! occupy, and reuses r3 for the callback address high half.

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Arm {
    start: usize,
    object: u8,
    scratch: u8,
    callback_high: usize,
    callback_low: usize,
    callback_high_base: u8,
    callback_argument: u8,
}

fn external_target(relocation: &mwcc_machine_code::Relocation) -> Option<&str> {
    match &relocation.target {
        mwcc_machine_code::RelocationTarget::External(target) => Some(target),
        _ => None,
    }
}

fn recognize_at(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    start: usize,
) -> Option<Arm> {
    let [Instruction::LoadWord {
        d: object, a: 0, ..
    }, Instruction::AddImmediate {
        d: scratch, a: 0, ..
    }, Instruction::StoreWord {
        s, a: store_object, ..
    }] = instructions.get(start..start + 3)?
    else {
        return None;
    };
    if scratch != s
        || object != store_object
        || !relocations.iter().any(|relocation| {
            relocation.instruction_index == start && relocation.kind == RelocationKind::EmbSda21
        })
    {
        return None;
    }

    let call =
        instructions[start + 3..]
            .iter()
            .enumerate()
            .find_map(|(relative, instruction)| match instruction {
                Instruction::BranchAndLink { .. } => Some(Some(start + 3 + relative)),
                Instruction::Branch { .. } | Instruction::BranchConditionalForward { .. } => {
                    Some(None)
                }
                _ => None,
            })??;
    let low_relocation = relocations
        .iter()
        .filter(|relocation| {
            (start + 3..call).contains(&relocation.instruction_index)
                && relocation.kind == RelocationKind::Addr16Lo
        })
        .last()?;
    let Instruction::AddImmediate {
        d: callback_argument,
        a: callback_high_base,
        ..
    } = instructions.get(low_relocation.instruction_index)?
    else {
        return None;
    };
    let low_target = external_target(low_relocation)?;
    let high_relocation = relocations.iter().find(|relocation| {
        (start + 3..low_relocation.instruction_index).contains(&relocation.instruction_index)
            && relocation.kind == RelocationKind::Addr16Ha
            && external_target(relocation) == Some(low_target)
    })?;
    if !matches!(
        instructions.get(high_relocation.instruction_index),
        Some(Instruction::AddImmediateShifted { d, a: 0, .. })
            if d == callback_high_base
    ) || instructions[high_relocation.instruction_index + 1..low_relocation.instruction_index]
        .iter()
        .any(|instruction| {
            mwcc_vreg::register_operands(instruction)
                .into_iter()
                .any(|operand| {
                    operand.class == mwcc_vreg::Class::General
                        && operand.register == *callback_high_base
                })
        })
    {
        return None;
    }

    Some(Arm {
        start,
        object: *object,
        scratch: *scratch,
        callback_high: high_relocation.instruction_index,
        callback_low: low_relocation.instruction_index,
        callback_high_base: *callback_high_base,
        callback_argument: *callback_argument,
    })
}

fn rewrite_registers(
    instructions: &mut [Instruction],
    arm: Arm,
    object: u8,
    callback_high_base: u8,
) {
    let Instruction::LoadWord { d, .. } = &mut instructions[arm.start] else {
        unreachable!("the recognized state-object load remains present");
    };
    *d = object;
    let Instruction::StoreWord { a, .. } = &mut instructions[arm.start + 2] else {
        unreachable!("the recognized state publication remains present");
    };
    *a = object;
    let Instruction::AddImmediateShifted { d, .. } = &mut instructions[arm.callback_high] else {
        unreachable!("the recognized callback high remains a lis");
    };
    *d = callback_high_base;
    let Instruction::AddImmediate { a, .. } = &mut instructions[arm.callback_low] else {
        unreachable!("the recognized callback low remains an addi");
    };
    *a = callback_high_base;
}

impl Generator {
    pub(crate) fn schedule_linkage_first_callback_state_arms(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }

        let mut start = 0;
        while start + 3 <= self.output.instructions.len() {
            let Some(arm) =
                recognize_at(&self.output.instructions, &self.output.relocations, start)
            else {
                start += 1;
                continue;
            };
            let object_preference = if arm.callback_argument == 4 { 5 } else { 4 };
            let object = self.fresh_virtual_general_preferring(object_preference);
            let callback_high_base = self.fresh_virtual_general_preferring(3);
            rewrite_registers(
                &mut self.output.instructions,
                arm,
                object,
                callback_high_base,
            );

            crate::move_instruction_before_retargeting(self, arm.callback_high, start + 2);
            if arm.callback_low > start + 4 {
                crate::move_instruction_before_retargeting(self, arm.callback_low, start + 4);
            }
            start += 3;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn relocation(instruction_index: usize, kind: RelocationKind, target: &str) -> Relocation {
        Relocation {
            instruction_index,
            kind,
            target: RelocationTarget::External(target.to_string()),
        }
    }

    #[test]
    fn recognizes_a_state_publication_before_a_callback_call() {
        let instructions = vec![
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate(0, 1),
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 28,
            },
            Instruction::load_immediate(3, 0),
            Instruction::load_immediate_shifted(6, 0),
            Instruction::AddImmediate {
                d: 6,
                a: 6,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "start".to_string(),
            },
        ];
        let relocations = vec![
            relocation(0, RelocationKind::EmbSda21, "executing"),
            relocation(4, RelocationKind::Addr16Ha, "callback"),
            relocation(5, RelocationKind::Addr16Lo, "callback"),
        ];

        assert_eq!(
            recognize_at(&instructions, &relocations, 0),
            Some(Arm {
                start: 0,
                object: 3,
                scratch: 0,
                callback_high: 4,
                callback_low: 5,
                callback_high_base: 6,
                callback_argument: 6,
            })
        );
    }

    #[test]
    fn rejects_a_publication_separated_from_its_call_by_control_flow() {
        let mut instructions = vec![
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate(0, 1),
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 28,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 7,
            },
        ];
        instructions.extend([
            Instruction::load_immediate_shifted(4, 0),
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "start".to_string(),
            },
        ]);
        let relocations = vec![
            relocation(0, RelocationKind::EmbSda21, "executing"),
            relocation(4, RelocationKind::Addr16Ha, "callback"),
            relocation(5, RelocationKind::Addr16Lo, "callback"),
        ];

        assert_eq!(recognize_at(&instructions, &relocations, 0), None);
    }
}
