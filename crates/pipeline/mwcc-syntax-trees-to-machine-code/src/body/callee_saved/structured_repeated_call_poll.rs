//! Recognition for repeated direct-send / empty-call-poll transactions.
//!
//! MWCC treats a long transaction of this form as one structured CFG region:
//! pre-test entry branches that land on their physical successor disappear,
//! while the standalone-loop eight-byte alignment policy is not applied.

use mwcc_syntax_trees::{BinaryOperator, Expression, Function, LoopKind, Statement};

use crate::Generator;
use mwcc_machine_code::Instruction;

const MINIMUM_POLL_PAIRS: usize = 6;

pub(super) fn is_repeated_call_poll_transaction(function: &Function) -> bool {
    let mut plan = PollTransaction::default();
    if !plan.visit_block(&function.statements) || plan.pair_count < MINIMUM_POLL_PAIRS {
        return false;
    }
    plan.sender.is_some() && plan.poller.is_some()
}

#[derive(Default)]
struct PollTransaction<'a> {
    sender: Option<&'a str>,
    poller: Option<&'a str>,
    pair_count: usize,
}

impl<'a> PollTransaction<'a> {
    fn visit_block(&mut self, statements: &'a [Statement]) -> bool {
        for (index, statement) in statements.iter().enumerate() {
            match statement {
                Statement::Loop { .. } => {
                    let Some(sender) = index
                        .checked_sub(1)
                        .and_then(|previous| direct_call(&statements[previous]))
                    else {
                        return false;
                    };
                    let Some(poller) = empty_call_poll(statement) else {
                        return false;
                    };
                    if !same_or_record(&mut self.sender, sender)
                        || !same_or_record(&mut self.poller, poller)
                    {
                        return false;
                    }
                    self.pair_count += 1;
                }
                Statement::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    if !self.visit_block(then_body) || !self.visit_block(else_body) {
                        return false;
                    }
                }
                Statement::Switch { arms, default, .. } => {
                    for arm in arms {
                        if let mwcc_syntax_trees::ArmBody::Statements(body) = &arm.body {
                            if !self.visit_block(body) {
                                return false;
                            }
                        }
                    }
                    if let Some(mwcc_syntax_trees::ArmBody::Statements(body)) = default {
                        if !self.visit_block(body) {
                            return false;
                        }
                    }
                }
                _ => {}
            }
        }
        true
    }
}

fn same_or_record<'a>(slot: &mut Option<&'a str>, candidate: &'a str) -> bool {
    match slot {
        Some(expected) => *expected == candidate,
        None => {
            *slot = Some(candidate);
            true
        }
    }
}

fn direct_call(statement: &Statement) -> Option<&str> {
    let Statement::Expression(Expression::Call { name, .. }) = statement else {
        return None;
    };
    Some(name)
}

fn empty_call_poll(statement: &Statement) -> Option<&str> {
    let Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition:
            Some(Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            }),
        step: None,
        body,
    } = statement
    else {
        return None;
    };
    if !body.is_empty() || !matches!(right.as_ref(), Expression::IntegerLiteral(0)) {
        return None;
    }
    let Expression::Call { name, arguments } = left.as_ref() else {
        return None;
    };
    arguments.is_empty().then_some(name)
}

impl Generator {
    /// Apply the final instruction-selection and frame schedule only after the
    /// source transaction and the complete emitted skeleton both agree.
    pub(crate) fn schedule_structured_repeated_call_poll_transaction(&mut self) {
        if !self.structured_repeated_call_poll_owner {
            return;
        }
        let instructions = &self.output.instructions;
        let Some(epilogue) = instructions.len().checked_sub(6) else {
            return;
        };
        if !matches!(
            instructions.first(),
            Some(Instruction::StoreWordWithUpdate { s: 1, a: 1, .. })
        ) || !matches!(
            instructions.get(1),
            Some(Instruction::MoveFromLinkRegister { d: 0 })
        ) || !matches!(
            instructions.get(2),
            Some(Instruction::StoreWord { s: 0, a: 1, .. })
        ) || !matches!(
            instructions.get(3),
            Some(Instruction::StoreWord { s: 31, a: 1, .. })
        ) || !matches!(
            instructions.get(4),
            Some(Instruction::Or { a: 31, s: 4, b: 4 })
        ) || !matches!(
            instructions.get(5),
            Some(Instruction::StoreWord { s: 30, a: 1, .. })
        ) || !matches!(
            instructions.get(6),
            Some(Instruction::Or { a: 30, s: 3, b: 3 })
        ) || !matches!(
            instructions.get(7),
            Some(Instruction::CompareLogicalWordImmediate {
                a: 30,
                immediate: 0
            })
        ) || !matches!(
            instructions.get(9),
            Some(Instruction::LoadWord { d: 3, a: 30, .. })
        ) || !matches!(
            instructions.get(epilogue),
            Some(Instruction::LoadWord { d: 31, a: 1, .. })
        ) || !matches!(
            instructions.get(epilogue + 1),
            Some(Instruction::LoadWord { d: 0, a: 1, .. })
        ) || !matches!(
            instructions.get(epilogue + 2),
            Some(Instruction::LoadWord { d: 30, a: 1, .. })
        ) || !matches!(
            instructions.get(epilogue + 3),
            Some(Instruction::MoveToLinkRegister { s: 0 })
        ) || !matches!(
            instructions.get(epilogue + 4),
            Some(Instruction::AddImmediate { d: 1, a: 1, .. })
        ) || !matches!(
            instructions.get(epilogue + 5),
            Some(Instruction::BranchToLinkRegister)
        ) {
            return;
        }
        let poll_comparisons = (1..instructions.len().saturating_sub(1))
            .filter(|&index| {
                matches!(instructions[index - 1], Instruction::BranchAndLink { .. })
                    && matches!(instructions[index], Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 })
                    && matches!(instructions[index + 1], Instruction::BranchConditionalForward { target, .. } if target == index - 1)
            })
            .count();
        if poll_comparisons < MINIMUM_POLL_PAIRS {
            return;
        }

        let entry_compare = self.output.instructions.remove(7);
        debug_assert!(matches!(
            entry_compare,
            Instruction::CompareLogicalWordImmediate {
                a: 30,
                immediate: 0
            }
        ));
        self.output
            .instructions
            .insert(2, Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        let Instruction::LoadWord { a, .. } = &mut self.output.instructions[9] else {
            unreachable!("the repeated call-poll prefix was validated above")
        };
        *a = 3;
        self.output.instructions.swap(epilogue, epilogue + 1);
        for instruction in &mut self.output.instructions {
            let Instruction::CompareLogicalWordImmediate { a, immediate: 0 } = *instruction else {
                continue;
            };
            *instruction = Instruction::CompareWordImmediate { a, immediate: 0 };
        }
    }
}
