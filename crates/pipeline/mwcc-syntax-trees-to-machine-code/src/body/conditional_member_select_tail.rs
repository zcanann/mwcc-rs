//! Nested null-select feeding a primary-base sibling call.
//!
//! GC 3.0's file IPA keeps a small member wrapper leaf: an outer null guard
//! assigns a local, a redundant ternary repeats the same test, and the local
//! occupies the final argument register of an inherited direct call.  The
//! repeated test intentionally reuses CR0; preserving that source-level shape
//! is observable in the two consecutive `beq` instructions.

use super::*;

struct Plan {
    actor: String,
    selected: String,
    member_offset: i16,
    fallback: i64,
    callee: String,
    arguments: Vec<Expression>,
}

fn null_tested_name(expression: &Expression) -> Option<&str> {
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left,
        right,
    } = expression
    else {
        return None;
    };
    match (left.as_ref(), right.as_ref()) {
        (Expression::Variable(name), Expression::IntegerLiteral(0))
        | (Expression::IntegerLiteral(0), Expression::Variable(name)) => Some(name),
        _ => None,
    }
}

fn variable_through_casts(mut expression: &Expression) -> Option<&str> {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    let Expression::Variable(name) = expression else {
        return None;
    };
    Some(name)
}

fn is_constant_noop(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Expression(Expression::Cast {
            target_type: Type::Void,
            operand,
        }) if matches!(operand.as_ref(), Expression::IntegerLiteral(_))
    )
}

fn recognize(function: &Function) -> Option<Plan> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
    {
        return None;
    }
    let [local] = function.locals.as_slice() else {
        return None;
    };
    if local.initializer.is_some()
        || local.is_volatile
        || local.is_static
        || local.array_length.is_some()
        || !matches!(local.declared_type, Type::Int | Type::UnsignedInt)
    {
        return None;
    }

    let substantive: Vec<&Statement> = function
        .statements
        .iter()
        .filter(|statement| !is_constant_noop(statement))
        .collect();
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }, Statement::Expression(Expression::Call {
        name: callee,
        arguments,
    })] = substantive.as_slice()
    else {
        return None;
    };
    let actor = null_tested_name(condition)?;
    let [Statement::Assign {
        name: then_name,
        value:
            Expression::Conditional {
                condition: inner_condition,
                when_true,
                when_false,
                ..
            },
    }] = then_body.as_slice()
    else {
        return None;
    };
    let [Statement::Assign {
        name: else_name,
        value: Expression::IntegerLiteral(else_value),
    }] = else_body.as_slice()
    else {
        return None;
    };
    if then_name != &local.name
        || else_name != &local.name
        || null_tested_name(inner_condition)? != actor
    {
        return None;
    }
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = when_true.as_ref()
    else {
        return None;
    };
    let Expression::IntegerLiteral(inner_value) = when_false.as_ref() else {
        return None;
    };
    if variable_through_casts(base)? != actor
        || !matches!(member_type, Type::Int | Type::UnsignedInt)
        || inner_value != else_value
        || *else_value != 0xffff_ffff
    {
        return None;
    }
    let [Expression::Variable(this), Expression::Variable(first), Expression::Variable(second), Expression::Variable(selected)] =
        arguments.as_slice()
    else {
        return None;
    };
    if this != "this" || selected != &local.name || first == second {
        return None;
    }
    Some(Plan {
        actor: actor.to_string(),
        selected: local.name.clone(),
        member_offset: i16::try_from(*offset).ok()?,
        fallback: *else_value,
        callee: callee.clone(),
        arguments: arguments.clone(),
    })
}

impl Generator {
    pub(crate) fn try_conditional_member_select_tail(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.behavior.tail_call_optimization {
            return Ok(false);
        }
        let Some(plan) = recognize(function) else {
            return Ok(false);
        };

        let actor_register = self
            .locations
            .get(plan.actor.as_str())
            .map(|location| location.register);
        let passthrough = plan.arguments[..3]
            .iter()
            .enumerate()
            .all(|(index, argument)| {
                self.leaf_info(argument)
                    .map(|(register, _, _)| register == Eabi::FIRST_GENERAL_ARGUMENT + index as u8)
                    .unwrap_or(false)
            });
        let selected_register = Eabi::FIRST_GENERAL_ARGUMENT + 3;
        if actor_register != Some(selected_register)
            || !passthrough
            || self.locations.contains_key(plan.selected.as_str())
            || self.call_return_types.get(plan.callee.as_str()) != Some(&Type::Void)
            || self.locations.contains_key(plan.callee.as_str())
            || self.globals.contains_key(plan.callee.as_str())
            || self.variadic_callees.contains(plan.callee.as_str())
        {
            return Ok(false);
        }

        self.locations.insert(
            plan.selected.clone(),
            Location {
                class: ValueClass::General,
                register: selected_register,
                signed: false,
                width: 32,
                pointee: None,
                stride: None,
            },
        );

        let outer_false = self.fresh_label();
        let inner_false = self.fresh_label();
        let join = self.fresh_label();
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: selected_register,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, outer_false); // beq
        self.emit_branch_conditional_to(12, 2, inner_false); // redundant ternary reuses CR0
        self.output.instructions.push(Instruction::LoadWord {
            d: selected_register,
            a: selected_register,
            offset: plan.member_offset,
        });
        self.emit_branch_to(join);
        self.bind_label(inner_false);
        self.load_integer_constant(selected_register, plan.fallback);
        self.emit_branch_to(join);
        self.bind_label(outer_false);
        self.load_integer_constant(selected_register, plan.fallback);
        self.bind_label(join);

        self.emit_arguments(&plan.arguments, &plan.callee)?;
        self.record_relocation(RelocationKind::Rel24, &plan.callee);
        self.output.instructions.push(Instruction::BranchExternal {
            target: plan.callee,
        });
        self.output.anonymous_label_bump += 4;
        Ok(true)
    }
}
