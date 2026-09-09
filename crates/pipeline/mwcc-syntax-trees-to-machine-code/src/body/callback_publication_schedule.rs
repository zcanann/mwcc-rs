//! Scheduling a callback publication immediately before a call.
//!
//! When a callback address is stored globally and the same callback is then
//! called with one loaded argument, MWCC borrows r4 for the address chain. That
//! frees r3 for the argument load and overlaps the callback high/low latency.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_callback_publication_call(&mut self, function: &Function) {
        self.schedule_callback_publication_registration(function);
        let Some(start) = (0..self.output.instructions.len().saturating_sub(4)).find(|&start| {
            let [Instruction::AddImmediateShifted { d: high, a: 0, .. }, Instruction::AddImmediate {
                d: published,
                a: low_base,
                ..
            }, Instruction::LoadWord {
                d: argument, a: 0, ..
            }, Instruction::StoreWord {
                s: stored, a: 0, ..
            }, Instruction::BranchAndLink { target }] = &self.output.instructions[start..start + 5]
            else {
                return false;
            };
            if *high != Eabi::FIRST_GENERAL_ARGUMENT.into()
                || *low_base != *high
                || *argument != *high
                || published != stored
                || *published == (Eabi::FIRST_GENERAL_ARGUMENT + 1).into()
                || self
                    .call_parameter_types
                    .get(target)
                    .is_none_or(|parameters| {
                        parameters.len() != 1 || matches!(parameters[0], Type::Float | Type::Double)
                    })
            {
                return false;
            }
            let Some(high_target) =
                relocation_target(&self.output.relocations, start, RelocationKind::Addr16Ha)
            else {
                return false;
            };
            let Some(low_target) = relocation_target(
                &self.output.relocations,
                start + 1,
                RelocationKind::Addr16Lo,
            ) else {
                return false;
            };
            high_target == low_target
                && high_target == target
                && has_relocation(
                    &self.output.relocations,
                    start + 2,
                    RelocationKind::EmbSda21,
                )
                && has_relocation(
                    &self.output.relocations,
                    start + 3,
                    RelocationKind::EmbSda21,
                )
        }) else {
            return;
        };

        let borrowed: u32 = (Eabi::FIRST_GENERAL_ARGUMENT + 1) as u32;
        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[start]
        else {
            unreachable!()
        };
        *d = u32::from(borrowed);
        let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[start + 1] else {
            unreachable!()
        };
        *a = u32::from(borrowed);
        self.output.instructions.swap(start + 1, start + 2);
        swap_relocation_indices(&mut self.output.relocations, start + 1, start + 2);
    }

    /// Overlap publication with a separate handler address. The argument
    /// marshaler has already selected the high-half register and narrowed the
    /// integer argument; this pass only orders independent instructions.
    fn schedule_callback_publication_registration(&mut self, function: &Function) {
        if !self.behavior.schedule_latency_slots
            || !function.guards.is_empty()
            // Retained inline bodies may introduce control flow absent from
            // the caller's source statements. Keep this owner within a linear
            // body, including the compact fall-through epilogue below.
            || self.output.instructions.iter().any(|instruction| matches!(instruction,
                Instruction::Branch { .. } | Instruction::BranchConditionalForward { .. }
                    | Instruction::BranchToCountRegister))
            || !function.statements.iter().all(|statement| {
                matches!(
                    statement,
                    Statement::Store { .. } | Statement::Expression(Expression::Call { .. })
                )
            })
        {
            return;
        }
        for call in 6..self.output.instructions.len() {
            if !matches!(
                self.output.instructions[call],
                Instruction::BranchAndLink { .. }
            ) {
                continue;
            }
            for size in 6..=8 {
                let Some(start) = call.checked_sub(size) else {
                    continue;
                };
                let Some(schedule) = registration_order(
                    &self.output,
                    start,
                    size,
                    self.behavior.frame_convention,
                    &|name| self.is_direct_function_symbol(name),
                ) else {
                    continue;
                };
                // These symbols are discovered when their independent address
                // chains start. Preserve any earlier discovery in this body.
                if self.behavior.symbol_traversal_style
                    != mwcc_versions::SymbolTraversalStyle::GroupedByKind
                    && !self.output.relocations.iter().any(|relocation| {
                        relocation.instruction_index < start
                            && matches!(&relocation.target,
                                mwcc_machine_code::RelocationTarget::External(name)
                                | mwcc_machine_code::RelocationTarget::ExternalWithAddend(name, _)
                            if name == &schedule.global || name == &schedule.handler)
                    })
                {
                    if let (Some(global), Some(handler)) = (
                        self.output
                            .symbol_order
                            .iter()
                            .position(|name| name == &schedule.global),
                        self.output
                            .symbol_order
                            .iter()
                            .position(|name| name == &schedule.handler),
                    ) {
                        if global < handler {
                            let handler = self.output.symbol_order.remove(handler);
                            self.output.symbol_order.insert(global, handler);
                        }
                    }
                }
                crate::permute_machine_function_region(&mut self.output, start, &schedule.order);
                // A legacy fall-through result needs no work in the LR reload
                // slot; registration exits fill it with the stack restore.
                let end = self.output.instructions.len();
                if self.behavior.frame_convention == FrameConvention::LinkageFirst
                    && function.return_expression.is_none()
                    && matches!(
                        &self.output.instructions[end.saturating_sub(4)..],
                        [
                            Instruction::LoadWord {
                                d: 0,
                                a: 1,
                                offset: 12
                            },
                            Instruction::MoveToLinkRegister { s: 0 },
                            Instruction::AddImmediate {
                                d: 1,
                                a: 1,
                                immediate: 8
                            },
                            Instruction::BranchToLinkRegister,
                        ]
                    )
                {
                    crate::permute_machine_function_region(&mut self.output, end - 3, &[1, 0, 2]);
                }
                break;
            }
        }
    }
}

struct RegistrationSchedule {
    order: Vec<usize>,
    global: String,
    handler: String,
}

/// Decode the six operations of a publication/registration packet, allowing
/// only the compact entry's LR save and stack update between them. Relocation
/// identity distinguishes function addresses from unrelated integer constants.
fn registration_order(
    output: &mwcc_machine_code::MachineFunction,
    start: usize,
    size: usize,
    convention: FrameConvention,
    is_function: &impl Fn(&str) -> bool,
) -> Option<RegistrationSchedule> {
    let instructions = output.instructions.get(start..start + size)?;
    let linkage_first = convention == FrameConvention::LinkageFirst;
    let entry = if linkage_first {
        start == 1
            && matches!(
                output.instructions.first(),
                Some(Instruction::MoveFromLinkRegister { d: 0 })
            )
    } else {
        start == 2
            && matches!(
                &output.instructions[..2],
                [
                    Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -16
                    },
                    Instruction::MoveFromLinkRegister { d: 0 },
                ]
            )
    };
    let mut operations = Vec::new();
    let mut save = None;
    let mut stack = None;
    for (index, instruction) in instructions.iter().enumerate() {
        match instruction {
            Instruction::StoreWord { s: 0, a: 1, offset }
                if entry && *offset == if linkage_first { 4 } else { 20 } =>
            {
                if save.replace(index).is_some() {
                    return None;
                }
            }
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -8,
            } if entry && linkage_first => {
                if stack.replace(index).is_some() {
                    return None;
                }
            }
            _ => operations.push(index),
        }
    }
    let [published_high, published_low, store, handler_high, arg_a, arg_b] =
        <[usize; 6]>::try_from(operations).ok()?;
    let Instruction::AddImmediateShifted { d: 3, a: 0, .. } = instructions[published_high] else {
        return None;
    };
    let Instruction::AddImmediate {
        d: published @ (0 | 3),
        a: 3,
        ..
    } = instructions[published_low]
    else {
        return None;
    };
    if !matches!(instructions[store], Instruction::StoreWord { s, a: 0, offset: 0 } if s == published)
    {
        return None;
    }
    let high = if linkage_first { 3 } else { 4 };
    if !matches!(instructions[handler_high], Instruction::AddImmediateShifted { d, a: 0, .. } if d == high)
    {
        return None;
    }
    let (handler_low, constant) = if linkage_first {
        (arg_a, arg_b)
    } else {
        (arg_b, arg_a)
    };
    if !matches!(instructions[handler_low], Instruction::AddImmediate { d: 4, a, .. } if a == high)
        || !matches!(
            instructions[constant],
            Instruction::AddImmediate { d: 3, a: 0, .. }
        )
        || (linkage_first && published != 0)
    {
        return None;
    }
    for (high, low) in [(published_high, published_low), (handler_high, handler_low)] {
        let target =
            relocation_target(&output.relocations, start + high, RelocationKind::Addr16Ha)?;
        if !is_function(target)
            || relocation_target(&output.relocations, start + low, RelocationKind::Addr16Lo)
                != Some(target)
        {
            return None;
        }
    }
    if !has_relocation(&output.relocations, start + store, RelocationKind::EmbSda21)
        || output
            .relocations
            .iter()
            .any(|relocation| relocation.instruction_index == start + constant)
    {
        return None;
    }
    let order = match (linkage_first, entry, save, stack) {
        (true, false, None, None) => Some(vec![
            published_high,
            published_low,
            handler_high,
            store,
            handler_low,
            constant,
        ]),
        (true, true, Some(save), Some(stack)) => Some(vec![
            published_high,
            save,
            published_low,
            handler_high,
            stack,
            handler_low,
            constant,
            store,
        ]),
        (false, false, None, None) if published == 0 => Some(vec![
            published_high,
            handler_high,
            published_low,
            store,
            constant,
            handler_low,
        ]),
        (false, false, None, None) => Some(vec![
            published_high,
            handler_high,
            published_low,
            store,
            handler_low,
            constant,
        ]),
        (false, true, Some(save), None) if published == 0 => Some(vec![
            handler_high,
            published_high,
            save,
            published_low,
            constant,
            handler_low,
            store,
        ]),
        (false, true, Some(save), None) => Some(vec![
            published_high,
            handler_high,
            save,
            published_low,
            handler_low,
            store,
            constant,
        ]),
        _ => None,
    }?;
    Some(RegistrationSchedule {
        order,
        global: relocation_target(&output.relocations, start + store, RelocationKind::EmbSda21)?
            .to_owned(),
        handler: relocation_target(
            &output.relocations,
            start + handler_high,
            RelocationKind::Addr16Ha,
        )?
        .to_owned(),
    })
}

fn relocation_target(
    relocations: &[mwcc_machine_code::Relocation],
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&str> {
    relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != instruction_index || relocation.kind != kind {
            return None;
        }
        let mwcc_machine_code::RelocationTarget::External(target) = &relocation.target else {
            return None;
        };
        Some(target.as_str())
    })
}

fn has_relocation(
    relocations: &[mwcc_machine_code::Relocation],
    instruction_index: usize,
    kind: RelocationKind,
) -> bool {
    relocations.iter().any(|relocation| {
        relocation.instruction_index == instruction_index && relocation.kind == kind
    })
}

fn swap_relocation_indices(
    relocations: &mut [mwcc_machine_code::Relocation],
    first: usize,
    second: usize,
) {
    for relocation in relocations {
        relocation.instruction_index = match relocation.instruction_index {
            index if index == first => second,
            index if index == second => first,
            index => index,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    #[test]
    fn finds_only_the_requested_external_relocation_kind() {
        let relocations = [
            Relocation {
                instruction_index: 2,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External("callback".into()),
            },
            Relocation {
                instruction_index: 2,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("other".into()),
            },
        ];

        assert_eq!(
            relocation_target(&relocations, 2, RelocationKind::Addr16Ha),
            Some("callback")
        );
    }
}
