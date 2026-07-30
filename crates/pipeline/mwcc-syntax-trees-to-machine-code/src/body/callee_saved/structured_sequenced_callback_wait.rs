//! Frame layout and final scheduling for an inlined asynchronous transaction
//! followed by a synchronous interrupt-guarded wait.

use super::*;
use mwcc_syntax_trees::{LocalDeclaration, Parameter};

struct SequencedCallbackWait<'a> {
    receiver: &'a str,
    identifier: &'a str,
    starter: &'a str,
}

fn sequenced_callback_wait(function: &Function) -> Option<SequencedCallbackWait<'_>> {
    let [receiver, identifier] = function.parameters.as_slice() else {
        return None;
    };
    let Statement::Assign {
        name: result,
        value: sequence @ Expression::Comma { .. },
    } = function.statements.first()?
    else {
        return None;
    };
    let (starter, arguments) = terminal_call(sequence)?;
    if arguments.last().and_then(variable_name) != Some(receiver.name.as_str())
        || !sequence_contains_conditional(sequence)
        || !function
            .statements
            .get(1)
            .is_some_and(|statement| rejects_zero_result(statement, result))
    {
        return None;
    }
    Some(SequencedCallbackWait {
        receiver: &receiver.name,
        identifier: &identifier.name,
        starter,
    })
}

pub(super) fn sequenced_callback_wait_starter(function: &Function) -> Option<&str> {
    sequenced_callback_wait(function).map(|transaction| transaction.starter)
}

/// Lay out the retained receiver, identifier, and wait state in ascending
/// saved-register order. The identifier dies after publication, so the later
/// return value may reuse its middle home while the interrupt token occupies
/// the high home.
pub(super) fn sequenced_callback_wait_home_preference(
    function: &Function,
    saved_parameters: &[&Parameter],
    deferred_saved_locals: &[&LocalDeclaration],
    first_saved: usize,
    home_index: usize,
) -> Option<u8> {
    let transaction = is_sequenced_callback_wait_layout(
        function,
        saved_parameters,
        deferred_saved_locals,
        first_saved,
    )
    .then(|| sequenced_callback_wait(function))
    .flatten()?;
    let preferred = saved_parameters
        .get(home_index)
        .map(|parameter| {
            if parameter.name == transaction.receiver {
                first_saved
            } else {
                first_saved + 1
            }
        })
        .unwrap_or(first_saved + 2);
    u8::try_from(preferred).ok()
}

pub(super) fn is_sequenced_callback_wait_layout(
    function: &Function,
    saved_parameters: &[&Parameter],
    deferred_saved_locals: &[&LocalDeclaration],
    first_saved: usize,
) -> bool {
    sequenced_callback_wait(function).is_some_and(|transaction| {
        saved_parameters.len() == 2
            && deferred_saved_locals.len() == 2
            && first_saved == 29
            && saved_parameters.iter().all(|parameter| {
                parameter.name == transaction.receiver || parameter.name == transaction.identifier
            })
    })
}

/// Source-home indices in physical frame-slot order. The deferred interrupt
/// state owns the top slot, followed by the identifier and receiver homes.
pub(super) const fn sequenced_callback_wait_save_order() -> [usize; 3] {
    [2, 0, 1]
}

pub(super) fn sequenced_callback_wait_frame_slot(home_index: usize) -> Option<usize> {
    sequenced_callback_wait_save_order()
        .iter()
        .position(|candidate| *candidate == home_index)
}

impl Generator {
    /// Apply MWCC's final physical schedule only after both the semantic
    /// transaction and every rewritten instruction region have been verified.
    pub(crate) fn schedule_sequenced_callback_wait(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let Some(starter) = self.structured_sequenced_callback_wait_starter.as_deref() else {
            return;
        };
        let Some(plan) =
            physical_schedule(&self.output.instructions, &self.output.relocations, starter)
        else {
            return;
        };

        self.output.instructions[plan.identifier_copy] =
            Instruction::move_register(30, Eabi::FIRST_GENERAL_ARGUMENT + 1);
        let Instruction::LoadByteZero { a, .. } =
            &mut self.output.instructions[plan.identifier_load]
        else {
            unreachable!()
        };
        *a = Eabi::FIRST_GENERAL_ARGUMENT + 1;
        self.output.instructions[plan.issue_receiver] = Instruction::AddImmediate {
            d: Eabi::FIRST_GENERAL_ARGUMENT + 1,
            a: 29,
            immediate: 0,
        };
        self.output.instructions[plan.interrupt_copy] =
            Instruction::move_register(31, Eabi::general_result().number);
        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[plan.state_load] else {
            unreachable!()
        };
        *d = GENERAL_SCRATCH;
        for comparison in plan.state_comparisons {
            let Instruction::CompareWordImmediate { a, .. } =
                &mut self.output.instructions[comparison]
            else {
                unreachable!()
            };
            *a = GENERAL_SCRATCH;
        }

        crate::move_instruction_before_retargeting(
            self,
            plan.callback_address_high,
            plan.identifier_store,
        );
        crate::move_instruction_before_retargeting(
            self,
            plan.callback_address_low,
            plan.identifier_store + 1,
        );
    }
}

#[derive(Debug, PartialEq, Eq)]
struct PhysicalSchedule {
    identifier_copy: usize,
    identifier_load: usize,
    identifier_store: usize,
    callback_address_high: usize,
    callback_address_low: usize,
    issue_receiver: usize,
    interrupt_copy: usize,
    state_load: usize,
    state_comparisons: [usize; 3],
}

fn physical_schedule(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    starter: &str,
) -> Option<PhysicalSchedule> {
    if !matches!(
        instructions.get(0..9),
        Some([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -40,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 36,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 32,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 4,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 29,
                a: 1,
                offset: 28,
            },
            Instruction::AddImmediate {
                d: 29,
                a: 3,
                immediate: 0,
            },
            Instruction::LoadByteZero {
                d: 0,
                a: 30,
                offset: 4,
            },
        ])
    ) {
        return None;
    }

    let identifier_store = instructions.windows(4).position(|window| {
        matches!(
            window,
            [
                Instruction::StoreWord {
                    s: 30,
                    a: 29,
                    offset: 36,
                },
                Instruction::AddImmediateShifted { d: 3, a: 0, .. },
                Instruction::AddImmediate { d: 0, a: 3, .. },
                Instruction::StoreWord {
                    s: 0,
                    a: 29,
                    offset: 40,
                },
            ]
        )
    })?;
    let callback_address_high = identifier_store + 1;
    let callback_address_low = identifier_store + 2;
    if !matching_address_relocations(relocations, callback_address_high, callback_address_low) {
        return None;
    }

    let issue_receiver = instructions.windows(2).position(|window| {
        matches!(
            window,
            [
                Instruction::Or { a: 4, s: 29, b: 29 },
                Instruction::BranchAndLink { target },
            ] if target == starter
        )
    })?;
    let interrupt_call = instructions.iter().position(|instruction| {
        matches!(
            instruction,
            Instruction::BranchAndLink { target } if target == "OSDisableInterrupts"
        )
    })?;
    if !matches!(
        instructions.get(interrupt_call + 1..interrupt_call + 12),
        Some([
            Instruction::AddImmediate {
                d: 31,
                a: 3,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 29,
                offset: 12,
            },
            Instruction::CompareWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::AddImmediate {
                d: 30,
                a: 0,
                immediate: 0,
            },
            Instruction::Branch { .. },
            Instruction::CompareWordImmediate {
                a: 3,
                immediate: -1,
            },
            Instruction::BranchConditionalForward { .. },
            Instruction::AddImmediate {
                d: 30,
                a: 0,
                immediate: -1,
            },
            Instruction::Branch { .. },
            Instruction::CompareWordImmediate {
                a: 3,
                immediate: 10,
            },
        ])
    ) {
        return None;
    }

    Some(PhysicalSchedule {
        identifier_copy: 5,
        identifier_load: 8,
        identifier_store,
        callback_address_high,
        callback_address_low,
        issue_receiver,
        interrupt_copy: interrupt_call + 1,
        state_load: interrupt_call + 2,
        state_comparisons: [interrupt_call + 3, interrupt_call + 7, interrupt_call + 11],
    })
}

fn matching_address_relocations(
    relocations: &[mwcc_machine_code::Relocation],
    high: usize,
    low: usize,
) -> bool {
    use mwcc_machine_code::{RelocationKind, RelocationTarget};

    let target = |instruction_index, kind| {
        relocations.iter().find_map(|relocation| {
            (relocation.instruction_index == instruction_index && relocation.kind == kind)
                .then_some(&relocation.target)
        })
    };
    matches!(
        (
            target(high, RelocationKind::Addr16Ha),
            target(low, RelocationKind::Addr16Lo),
        ),
        (
            Some(RelocationTarget::External(high_target)),
            Some(RelocationTarget::External(low_target)),
        ) if high_target == low_target
    )
}

fn terminal_call(expression: &Expression) -> Option<(&str, &[Expression])> {
    match expression {
        Expression::Call { name, arguments } => Some((name, arguments)),
        Expression::Comma { right, .. } => terminal_call(right),
        _ => None,
    }
}

fn variable_name(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable_name(operand),
        _ => None,
    }
}

fn sequence_contains_conditional(expression: &Expression) -> bool {
    match expression {
        Expression::Conditional { .. } => true,
        Expression::Comma { left, right } => {
            sequence_contains_conditional(left) || sequence_contains_conditional(right)
        }
        _ => false,
    }
}

fn rejects_zero_result(statement: &Statement, result: &str) -> bool {
    matches!(statement, Statement::If {
        condition: Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        },
        then_body,
        else_body,
    } if else_body.is_empty()
        && matches!(left.as_ref(), Expression::Variable(name) if name == result)
        && constant_value(right) == Some(0)
        && matches!(then_body.as_slice(), [Statement::Return(Some(value))]
            if constant_value(value) == Some(-1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_order_and_slots_are_inverses() {
        for (slot, home_index) in sequenced_callback_wait_save_order().into_iter().enumerate() {
            assert_eq!(sequenced_callback_wait_frame_slot(home_index), Some(slot),);
        }
        assert_eq!(sequenced_callback_wait_frame_slot(3), None);
    }
}
