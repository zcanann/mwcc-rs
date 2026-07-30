//! Synchronous wait wrappers around an automatically inlined async starter.
//!
//! The starter publishes a command and callback, then forwards to a queue
//! routine. Its synchronous caller rejects a zero result and waits under an
//! interrupt token until one of three terminal states is observed. Build 163
//! colors the command block and interrupt token into two homes, then reuses the
//! block home for the returned member.

#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
struct StreamWaitPlan {
    command: i16,
    command_offset: i16,
    callback: String,
    callback_offset: i16,
    priority: i16,
    issue: String,
    disable: String,
    state_offset: i16,
    result_offset: i16,
    queue: String,
    sleep: String,
    restore: String,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn flatten_comma<'a>(expression: &'a Expression, output: &mut Vec<&'a Expression>) {
    match expression {
        Expression::Comma { left, right } => {
            flatten_comma(left, output);
            flatten_comma(right, output);
        }
        expression => output.push(expression),
    }
}

fn member<'a>(expression: &'a Expression, base: &str) -> Option<(i16, Type)> {
    let Expression::Member {
        base: found,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    variable(found, base).then_some((i16::try_from(*offset).ok()?, *member_type))
}

fn recognizes_terminal_state_test(expression: &Expression, state: &str) -> bool {
    fn equality(expression: &Expression, state: &str, values: &mut Vec<i64>) -> bool {
        match expression {
            Expression::Binary {
                operator: BinaryOperator::LogicalOr,
                left,
                right,
            } => equality(left, state, values) && equality(right, state, values),
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            } if variable(left, state) => {
                let Some(value) = constant_value(right) else {
                    return false;
                };
                values.push(value);
                true
            }
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            } if variable(right, state) => {
                let Some(value) = constant_value(left) else {
                    return false;
                };
                values.push(value);
                true
            }
            _ => false,
        }
    }

    let mut values = Vec::new();
    if equality(expression, state, &mut values) {
        values.sort_unstable();
        return values == [-1, 0, 10];
    }

    // The focused canary spells the range explicitly. The source-project form
    // reaches the same instruction tree through MWCC's equality coalescer.
    fn adjacent_range(expression: &Expression, state: &str) -> bool {
        matches!(expression, Expression::Binary {
            operator: BinaryOperator::LessEqual,
            left,
            right,
        } if constant_value(right) == Some(1)
            && matches!(left.as_ref(), Expression::Cast {
                target_type: Type::UnsignedInt,
                operand,
            } if matches!(operand.as_ref(), Expression::Binary {
                operator: BinaryOperator::Subtract,
                left,
                right,
            } if variable(left, state) && constant_value(right) == Some(-1))))
    }

    fn canceled_equality(expression: &Expression, state: &str) -> bool {
        matches!(expression, Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        } if (variable(left, state) && constant_value(right) == Some(10))
            || (variable(right, state) && constant_value(left) == Some(10)))
    }

    matches!(expression, Expression::Binary {
        operator: BinaryOperator::LogicalOr,
        left,
        right,
    } if (adjacent_range(left, state) && canceled_equality(right, state))
        || (canceled_equality(left, state) && adjacent_range(right, state)))
}

fn classify(function: &Function) -> Option<StreamWaitPlan> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || function.return_expression.is_none()
    {
        return None;
    }
    let [block] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(block.parameter_type, Type::StructPointer { .. }) {
        return None;
    }
    let [Statement::Assign {
        name: result_name,
        value: starter,
    }, Statement::If {
        condition: result_test,
        then_body: early_body,
        else_body: early_else,
    }, Statement::Assign {
        name: enabled_name,
        value:
            Expression::Call {
                name: disable,
                arguments: disable_arguments,
            },
    }, Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(loop_condition),
        step: None,
        body: loop_body,
    }, Statement::Expression(Expression::Call {
        name: restore,
        arguments: restore_arguments,
    })] = function.statements.as_slice()
    else {
        return None;
    };
    if constant_value(loop_condition) != Some(1)
        || !disable_arguments.is_empty()
        || !early_else.is_empty()
        || !matches!(early_body.as_slice(), [Statement::Return(Some(value))]
            if constant_value(value) == Some(-1))
        || !matches!(result_test, Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        } if variable(left, result_name) && constant_value(right) == Some(0))
        || !matches!(restore_arguments.as_slice(), [value] if variable(value, enabled_name))
    {
        return None;
    }

    let mut sequence = Vec::new();
    flatten_comma(starter, &mut sequence);
    let [callback_assignment, command_store, callback_store, issue_assignment, issue_result] =
        sequence.as_slice()
    else {
        return None;
    };
    let (callback_assignment, command_store, callback_store, issue_assignment, issue_result) = (
        *callback_assignment,
        *command_store,
        *callback_store,
        *issue_assignment,
        *issue_result,
    );
    let Expression::Assign {
        target: callback_local,
        value: callback_value,
    } = callback_assignment
    else {
        return None;
    };
    let Expression::Variable(callback_local) = callback_local.as_ref() else {
        return None;
    };
    let Expression::Variable(callback) = callback_value.as_ref() else {
        return None;
    };
    let Expression::Assign {
        target: command_target,
        value: command,
    } = command_store
    else {
        return None;
    };
    let (command_offset, _) = member(command_target, &block.name)?;
    let command = i16::try_from(constant_value(command)?).ok()?;
    let Expression::Assign {
        target: callback_target,
        value: callback_value,
    } = callback_store
    else {
        return None;
    };
    let (callback_offset, _) = member(callback_target, &block.name)?;
    if !variable(callback_value, callback_local) {
        return None;
    }
    let Expression::Assign {
        target: issue_local,
        value: issue_value,
    } = issue_assignment
    else {
        return None;
    };
    let Expression::Call {
        name: issue,
        arguments: issue_arguments,
    } = issue_value.as_ref()
    else {
        return None;
    };
    let Expression::Variable(issue_local) = issue_local.as_ref() else {
        return None;
    };
    let [priority, issue_block] = issue_arguments.as_slice() else {
        return None;
    };
    let priority = i16::try_from(constant_value(priority)?).ok()?;
    if !variable(issue_block, &block.name) || !variable(issue_result, issue_local) {
        return None;
    }

    let [Statement::Assign {
        name: state_name,
        value: state_member,
    }, Statement::If {
        condition: terminal_test,
        then_body: terminal_body,
        else_body: terminal_else,
    }, Statement::Expression(Expression::Call {
        name: sleep,
        arguments: sleep_arguments,
    })] = loop_body.as_slice()
    else {
        return None;
    };
    let (state_offset, state_type) = member(state_member, &block.name)?;
    if state_type != Type::Int
        || !terminal_else.is_empty()
        || !recognizes_terminal_state_test(terminal_test, state_name)
    {
        return None;
    }
    let [Statement::Assign {
        name: return_name,
        value: return_member,
    }, Statement::Break] = terminal_body.as_slice()
    else {
        return None;
    };
    let (result_offset, result_type) = member(return_member, &block.name)?;
    if !matches!(result_type, Type::Int | Type::UnsignedInt)
        || !matches!(function.return_expression.as_ref(), Some(value) if variable(value, return_name))
    {
        return None;
    }
    let [Expression::AddressOf { operand }] = sleep_arguments.as_slice() else {
        return None;
    };
    let Expression::Variable(queue) = operand.as_ref() else {
        return None;
    };

    Some(StreamWaitPlan {
        command,
        command_offset,
        callback: callback.clone(),
        callback_offset,
        priority,
        issue: issue.clone(),
        disable: disable.clone(),
        state_offset,
        result_offset,
        queue: queue.clone(),
        sleep: sleep.clone(),
        restore: restore.clone(),
    })
}

impl Generator {
    pub(crate) fn try_inlined_async_stream_wait(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        const INTERRUPT: u8 = 31;
        const BLOCK_OR_RESULT: u8 = 30;

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![INTERRUPT, BLOCK_OR_RESULT];
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::load_immediate(0, plan.command),
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::StoreWord {
                s: INTERRUPT,
                a: 1,
                offset: 28,
            },
            Instruction::StoreWord {
                s: BLOCK_OR_RESULT,
                a: 1,
                offset: 24,
            },
            Instruction::AddImmediate {
                d: BLOCK_OR_RESULT,
                a: 3,
                immediate: 0,
            },
        ]);
        self.record_relocation(RelocationKind::Addr16Ha, &plan.callback);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            });
        self.output.instructions.extend([Instruction::StoreWord {
            s: 0,
            a: BLOCK_OR_RESULT,
            offset: plan.command_offset,
        }]);
        self.record_relocation(RelocationKind::Addr16Lo, &plan.callback);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: BLOCK_OR_RESULT,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: BLOCK_OR_RESULT,
                offset: plan.callback_offset,
            },
            Instruction::load_immediate(3, plan.priority),
        ]);
        self.record_relocation(RelocationKind::Rel24, &plan.issue);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.issue.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        let accepted = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        let early_return = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });

        let accepted_target = self.output.instructions.len();
        self.record_relocation(RelocationKind::Rel24, &plan.disable);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.disable.clone(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(INTERRUPT, 3));
        let loop_head = self.output.instructions.len();
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: BLOCK_OR_RESULT,
                offset: plan.state_offset,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: 1,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 },
        ]);
        let adjacent_terminal = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 3,
                immediate: 10,
            });
        let nonterminal = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });
        let terminal = self.output.instructions.len();
        self.output.instructions.push(Instruction::LoadWord {
            d: BLOCK_OR_RESULT,
            a: BLOCK_OR_RESULT,
            offset: plan.result_offset,
        });
        let to_restore = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });
        let sleep_target = self.output.instructions.len();
        self.record_relocation(RelocationKind::EmbSda21, &plan.queue);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.record_relocation(RelocationKind::Rel24, &plan.sleep);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.sleep.clone(),
        });
        self.output
            .instructions
            .push(Instruction::Branch { target: loop_head });
        let restore_target = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::move_register(3, INTERRUPT));
        self.record_relocation(RelocationKind::Rel24, &plan.restore);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.restore,
        });
        self.output
            .instructions
            // `mr r3,r30`. This owner fixes the complete physical schedule;
            // keeping the encoded word prevents generic virtual self-move
            // coalescing from mistaking the reused result home for r3.
            .push(Instruction::VerbatimWord(0x7fc3_f378));
        let epilogue = self.output.instructions.len();
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            Instruction::LoadWord {
                d: INTERRUPT,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: BLOCK_OR_RESULT,
                a: 1,
                offset: 24,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);

        for (index, target) in [
            (accepted, accepted_target),
            (early_return, epilogue),
            (adjacent_terminal, terminal),
            (nonterminal, sleep_target),
            (to_restore, restore_target),
        ] {
            match &mut self.output.instructions[index] {
                Instruction::Branch { target: found }
                | Instruction::BranchConditionalForward { target: found, .. } => *found = target,
                _ => unreachable!("stream wait branch placeholder changed form"),
            }
        }
        Ok(true)
    }
}
