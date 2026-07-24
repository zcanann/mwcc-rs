//! Entry scheduling for a retained receiver plus a member-derived local.
//!
//! In `local = receiver->member; local->field = 0; first(receiver); ...`, the
//! entry r3 remains valid through the initializer and store. Build 163 saves
//! both long-lived values before materializing either one, and lets the first
//! call consume that unchanged entry value without a redundant remarshal.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_structured_entry_zero_store(&mut self, function: &Function) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        if self.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Branch { .. } | Instruction::BranchConditionalForward { .. }
            )
        }) {
            return;
        }
        let Some((receiver, local)) = recognize(function) else {
            return;
        };
        let (Some(receiver_home), Some(local_home)) =
            (self.lookup_general(receiver), self.lookup_general(local))
        else {
            return;
        };
        let Some(first_call) = self
            .output
            .instructions
            .iter()
            .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        else {
            return;
        };
        let Some(alias_save) = self.output.instructions[..first_call]
            .iter()
            .position(|instruction| {
                matches!(instruction, Instruction::StoreWord { s, a: 1, .. } if *s == local_home)
            })
        else {
            return;
        };
        let Some(alias_load) = self.output.instructions[..first_call]
            .iter()
            .position(|instruction| {
                matches!(instruction, Instruction::LoadWord { d, a: 3, .. } if *d == local_home)
            })
        else {
            return;
        };
        let Some(receiver_save) = self.output.instructions[..first_call]
            .iter()
            .position(|instruction| {
                matches!(instruction, Instruction::StoreWord { s, a: 1, .. } if *s == receiver_home)
            })
        else {
            return;
        };
        let Some(receiver_copy) = self.output.instructions[..first_call]
            .iter()
            .position(|instruction| {
                matches!(instruction,
                    Instruction::Or { a, s: 3, b: 3 } if *a == receiver_home)
                    || matches!(instruction,
                        Instruction::AddImmediate { d, a: 3, immediate: 0 } if *d == receiver_home)
            })
        else {
            return;
        };
        let Some(first_argument_copy) = first_call.checked_sub(1) else {
            return;
        };
        if !(alias_save < alias_load
            && alias_load < receiver_save
            && receiver_save < receiver_copy
            && receiver_copy < first_argument_copy)
            || !matches!(
                self.output.instructions[first_argument_copy],
                Instruction::Or { a: 3, s, b } if s == receiver_home && b == receiver_home
            )
        {
            return;
        }

        // Move the local load after both saved-home stores and the receiver
        // copy. The insertion index is the old copy index: removing the earlier
        // load shifts the copy left, so insertion there lands immediately after.
        let moved = self.output.instructions.remove(alias_load);
        self.output.instructions.insert(receiver_copy, moved);
        self.labels.removed(alias_load, 1);
        self.labels.inserted(receiver_copy, 1);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = if relocation.instruction_index == alias_load {
                receiver_copy
            } else if (alias_load + 1..=receiver_copy).contains(&relocation.instruction_index) {
                relocation.instruction_index - 1
            } else {
                relocation.instruction_index
            };
        }

        // The move was before the first-call remarshal, so its index is stable.
        self.output.instructions.remove(first_argument_copy);
        self.labels.removed(first_argument_copy, 1);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != first_argument_copy);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > first_argument_copy {
                relocation.instruction_index -= 1;
            }
        }
    }
}

fn recognize(function: &Function) -> Option<(&str, &str)> {
    if function.return_type != Type::Void || function.return_expression.is_some() {
        return None;
    }
    let receiver = function.parameters.first()?.name.as_str();
    let local = function.locals.iter().find(|local| {
        matches!(&local.initializer,
            Some(Expression::Member { base, .. })
                if matches!(base.as_ref(), Expression::Variable(name) if name == receiver))
    })?;
    let [
        Statement::Store {
            target: Expression::Member { base: store_base, .. },
            value: Expression::IntegerLiteral(0),
        },
        Statement::Expression(Expression::Call { arguments, .. }),
        ..
    ] = function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(store_base.as_ref(), Expression::Variable(name) if name == &local.name)
        || !matches!(arguments.first(), Some(Expression::Variable(name)) if name == receiver)
    {
        return None;
    }
    Some((receiver, local.name.as_str()))
}
