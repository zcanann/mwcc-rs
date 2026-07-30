//! Nested interrupt-protected status query after retained-inline expansion.
//!
//! An outer critical section handles global fatal/paused/idle states. Its final
//! arm expands a small helper that enters another critical section and maps one
//! object status value. Build 163 keeps the outer token in one saved home and
//! reuses the selected-object home for every returned status.

#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
struct NestedStatusPlan {
    disable: String,
    restore: String,
    fatal_global: String,
    pause_global: String,
    object_global: String,
    dummy_global: String,
    fatal_result: i16,
    pause_result: i16,
    empty_result: i16,
    mapped_state: i16,
    mapped_result: i16,
    state_offset: i16,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn zero(expression: &Expression) -> bool {
    constant_value(expression) == Some(0)
}

fn flatten_comma<'a>(expression: &'a Expression, output: &mut Vec<&'a Expression>) {
    match expression {
        Expression::Comma { left, right } => {
            flatten_comma(left, output);
            flatten_comma(right, output);
        }
        Expression::Cast {
            target_type: Type::Void,
            operand,
        } if constant_value(operand).is_some() => {}
        expression => output.push(expression),
    }
}

fn global_zero_test(expression: &Expression, operator: BinaryOperator) -> Option<String> {
    let Expression::Binary {
        operator: found,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if *found != operator {
        return None;
    }
    if zero(right) {
        variable(left).map(str::to_owned)
    } else if zero(left) {
        variable(right).map(str::to_owned)
    } else {
        None
    }
}

fn assigned_constant(statements: &[Statement], expected: &str) -> Option<i16> {
    let [Statement::Assign { name, value }] = statements else {
        return None;
    };
    (name == expected)
        .then(|| constant_value(value).and_then(|value| i16::try_from(value).ok()))
        .flatten()
}

fn assigned_value<'a>(expression: &'a Expression) -> Option<(&'a str, &'a Expression)> {
    let Expression::Assign { target, value } = expression else {
        return None;
    };
    Some((variable(target)?, value))
}

fn member(expression: &Expression, base_name: &str) -> Option<(i16, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    (variable(base)? == base_name)
        .then(|| Some((i16::try_from(*offset).ok()?, *member_type)))
        .flatten()
}

fn assignment_from_comma<'a>(
    expression: &'a Expression,
    expected_name: &str,
) -> Option<&'a Expression> {
    let Expression::Comma { left, right } = expression else {
        return None;
    };
    if !zero(right) {
        return None;
    }
    let (name, value) = assigned_value(left)?;
    (name == expected_name).then_some(value)
}

fn classify(function: &Function) -> Option<NestedStatusPlan> {
    if function.return_type != Type::Int
        || !function.parameters.is_empty()
        || !function.guards.is_empty()
    {
        return None;
    }
    let [outer_if, Statement::Expression(Expression::Call {
        name: outer_restore,
        arguments: outer_restore_arguments,
    })] = function.statements.as_slice()
    else {
        return None;
    };
    let Some(Expression::Variable(return_name)) = function.return_expression.as_ref() else {
        return None;
    };
    let outer_enabled = function.locals.iter().find_map(|local| {
        let Expression::Call { name, arguments } = local.initializer.as_ref()? else {
            return None;
        };
        arguments
            .is_empty()
            .then(|| (local.name.as_str(), name.as_str()))
    })?;
    if !matches!(outer_restore_arguments.as_slice(), [argument]
        if variable(argument) == Some(outer_enabled.0))
    {
        return None;
    }

    let Statement::If {
        condition: fatal_test,
        then_body: fatal_body,
        else_body: fatal_else,
    } = outer_if
    else {
        return None;
    };
    let fatal_global = global_zero_test(fatal_test, BinaryOperator::NotEqual)?;
    let fatal_result = assigned_constant(fatal_body, return_name)?;
    let [Statement::If {
        condition: pause_test,
        then_body: pause_body,
        else_body: pause_else,
    }] = fatal_else.as_slice()
    else {
        return None;
    };
    let pause_global = global_zero_test(pause_test, BinaryOperator::NotEqual)?;
    let pause_result = assigned_constant(pause_body, return_name)?;
    let [Statement::If {
        condition: empty_test,
        then_body: empty_body,
        else_body: empty_else,
    }] = pause_else.as_slice()
    else {
        return None;
    };
    let object_global = global_zero_test(empty_test, BinaryOperator::Equal)?;
    let empty_result = assigned_constant(empty_body, return_name)?;
    let [Statement::If {
        condition: dummy_test,
        then_body: dummy_body,
        else_body: query_else,
    }] = empty_else.as_slice()
    else {
        return None;
    };
    if assigned_constant(dummy_body, return_name)? != empty_result {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left: dummy_left,
        right: dummy_right,
    } = dummy_test
    else {
        return None;
    };
    let dummy_global = match (variable(dummy_left), dummy_right.as_ref()) {
        (Some(found), Expression::AddressOf { operand }) if found == object_global => {
            variable(operand)?.to_owned()
        }
        _ => match (dummy_left.as_ref(), variable(dummy_right)) {
            (Expression::AddressOf { operand }, Some(found)) if found == object_global => {
                variable(operand)?.to_owned()
            }
            _ => return None,
        },
    };
    let [Statement::Assign {
        name: query_result,
        value: query,
    }] = query_else.as_slice()
    else {
        return None;
    };
    if query_result != return_name {
        return None;
    }

    let mut sequence = Vec::new();
    flatten_comma(query, &mut sequence);
    let [block_assignment, enabled_assignment, status_selection, inner_restore, inner_result] =
        sequence.as_slice()
    else {
        return None;
    };
    let (block_name, block_value) = assigned_value(block_assignment)?;
    let Expression::Cast { operand, .. } = block_value else {
        return None;
    };
    if variable(operand) != Some(&object_global) {
        return None;
    }
    let (inner_enabled, disable_value) = assigned_value(enabled_assignment)?;
    let Expression::Call {
        name: inner_disable,
        arguments: inner_disable_arguments,
    } = disable_value
    else {
        return None;
    };
    if !inner_disable_arguments.is_empty() || inner_disable != outer_enabled.1 {
        return None;
    }
    let Expression::Conditional {
        condition: state_test,
        when_true,
        when_false,
        origin: mwcc_syntax_trees::ConditionalOrigin::IfAssignments,
    } = status_selection
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left: state_member,
        right: mapped_state,
    } = state_test.as_ref()
    else {
        return None;
    };
    let mapped_state = i16::try_from(constant_value(mapped_state)?).ok()?;
    let (state_offset, state_type) = member(state_member, block_name)?;
    if state_type != Type::Int {
        return None;
    }
    let (inner_result_name, mapped_value) = match when_true.as_ref() {
        Expression::Comma { left, .. } => assigned_value(left)?,
        _ => return None,
    };
    let mapped_result = i16::try_from(constant_value(mapped_value)?).ok()?;
    let false_value = assignment_from_comma(when_false, inner_result_name)?;
    if member(false_value, block_name)? != (state_offset, Type::Int) {
        return None;
    }
    let Expression::Call {
        name: inner_restore,
        arguments: inner_restore_arguments,
    } = inner_restore
    else {
        return None;
    };
    if inner_restore != outer_restore
        || !matches!(inner_restore_arguments.as_slice(), [argument]
            if variable(argument) == Some(inner_enabled))
        || variable(inner_result) != Some(inner_result_name)
    {
        return None;
    }

    Some(NestedStatusPlan {
        disable: outer_enabled.1.to_owned(),
        restore: outer_restore.clone(),
        fatal_global,
        pause_global,
        object_global,
        dummy_global,
        fatal_result,
        pause_result,
        empty_result,
        mapped_state,
        mapped_result,
        state_offset,
    })
}

fn patch_branch(instructions: &mut [Instruction], index: usize, target: usize) {
    match &mut instructions[index] {
        Instruction::Branch { target: found }
        | Instruction::BranchConditionalForward { target: found, .. } => *found = target,
        _ => unreachable!("nested status branch placeholder changed form"),
    }
}

impl Generator {
    pub(crate) fn try_inlined_nested_status_query(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        const RESULT: u8 = 31;
        const OUTER_TOKEN: u8 = 30;

        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved = vec![RESULT, OUTER_TOKEN];
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            },
            Instruction::StoreWord {
                s: RESULT,
                a: 1,
                offset: 20,
            },
            Instruction::StoreWord {
                s: OUTER_TOKEN,
                a: 1,
                offset: 16,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &plan.disable);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.disable.clone(),
        });
        self.record_relocation(RelocationKind::EmbSda21, &plan.fatal_global);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 0,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: OUTER_TOKEN,
                a: 3,
                immediate: 0,
            },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        ]);
        let no_fatal = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(RESULT, plan.fatal_result));
        let fatal_done = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });

        let pause_test = self.output.instructions.len();
        self.record_relocation(RelocationKind::EmbSda21, &plan.pause_global);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 0,
                offset: 0,
            },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        ]);
        let no_pause = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(RESULT, plan.pause_result));
        let pause_done = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });

        let object_test = self.output.instructions.len();
        self.record_relocation(RelocationKind::EmbSda21, &plan.object_global);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: RESULT,
                a: 0,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate {
                a: RESULT,
                immediate: 0,
            },
        ]);
        let has_object = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(RESULT, plan.empty_result));
        let empty_done = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });

        let dummy_test = self.output.instructions.len();
        self.record_relocation(RelocationKind::Addr16Ha, &plan.dummy_global);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            });
        self.record_relocation(RelocationKind::Addr16Lo, &plan.dummy_global);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: 0,
            },
            Instruction::CompareLogicalWord { a: RESULT, b: 0 },
        ]);
        let real_object = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(RESULT, plan.empty_result));
        let dummy_done = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });

        let query = self.output.instructions.len();
        self.record_relocation(RelocationKind::Rel24, &plan.disable);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.disable,
        });
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: RESULT,
                a: RESULT,
                offset: plan.state_offset,
            },
            Instruction::CompareWordImmediate {
                a: RESULT,
                immediate: plan.mapped_state,
            },
        ]);
        let state_unmapped = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(RESULT, plan.mapped_result));
        let inner_restore = self.output.instructions.len();
        self.record_relocation(RelocationKind::Rel24, &plan.restore);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.restore.clone(),
        });

        let outer_restore = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::move_register(3, OUTER_TOKEN));
        self.record_relocation(RelocationKind::Rel24, &plan.restore);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.restore,
        });
        self.output
            .instructions
            // Keep the result move ahead of the saved-LR reload. Generic
            // physical scheduling otherwise treats it as an epilogue move and
            // hoists the reload across this already measured schedule.
            .push(Instruction::VerbatimWord(0x7fe3_fb78));
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: RESULT,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: OUTER_TOKEN,
                a: 1,
                offset: 16,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 24,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);

        for (index, target) in [
            (no_fatal, pause_test),
            (fatal_done, outer_restore),
            (no_pause, object_test),
            (pause_done, outer_restore),
            (has_object, dummy_test),
            (empty_done, outer_restore),
            (real_object, query),
            (dummy_done, outer_restore),
            (state_unmapped, inner_restore),
        ] {
            patch_branch(&mut self.output.instructions, index, target);
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_commuted_global_zero_tests() {
        let direct = Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left: Box::new(Expression::Variable("fatal".into())),
            right: Box::new(Expression::IntegerLiteral(0)),
        };
        let commuted = Expression::Binary {
            operator: BinaryOperator::Equal,
            left: Box::new(Expression::IntegerLiteral(0)),
            right: Box::new(Expression::Variable("object".into())),
        };

        assert_eq!(
            global_zero_test(&direct, BinaryOperator::NotEqual).as_deref(),
            Some("fatal")
        );
        assert_eq!(
            global_zero_test(&commuted, BinaryOperator::Equal).as_deref(),
            Some("object")
        );
    }

    #[test]
    fn extracts_the_assignment_value_from_inline_conditional_arms() {
        let value = Expression::Member {
            base: Box::new(Expression::Variable("block".into())),
            offset: 12,
            member_type: Type::Int,
            index_stride: None,
        };
        let arm = Expression::Comma {
            left: Box::new(Expression::Assign {
                target: Box::new(Expression::Variable("result".into())),
                value: Box::new(value),
            }),
            right: Box::new(Expression::IntegerLiteral(0)),
        };

        assert_eq!(
            member(
                assignment_from_comma(&arm, "result").expect("assignment arm"),
                "block",
            ),
            Some((12, Type::Int))
        );
        assert!(assignment_from_comma(&arm, "other").is_none());
    }
}
