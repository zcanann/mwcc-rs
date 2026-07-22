//! Guarded updates to a call-filled aggregate followed by a second call.
//!
//! Four incoming values survive the producer call.  Failure returns its status;
//! success updates one narrow aggregate member and forwards the aggregate plus
//! the preserved values to a second call.  This module owns the measured legacy
//! r31..r28 homes and aggregate-frame layout.

use super::*;

struct UpdatePlan<'a> {
    first: &'a str,
    second: &'a str,
    member_value: &'a str,
    callback: &'a str,
    aggregate: &'a str,
    result: &'a str,
    producer: &'a str,
    updater: &'a str,
    member_offset: i16,
    failure_threshold: i16,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn ordinary(local: &LocalDeclaration) -> bool {
    local.initializer.is_none()
        && !local.is_volatile
        && !local.is_static
        && local.array_length.is_none()
}

fn classify(function: &Function) -> Option<UpdatePlan<'_>> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || function.parameters.len() != 4
        || function.locals.len() != 2
    {
        return None;
    }
    let [first, second, member_value, callback] = function.parameters.as_slice() else {
        return None;
    };
    if first.parameter_type != Type::Int
        || second.parameter_type != Type::Int
        || member_value.parameter_type != Type::UnsignedChar
        || callback.parameter_type != Type::Pointer(Pointee::Int)
    {
        return None;
    }
    let [aggregate, result] = function.locals.as_slice() else {
        return None;
    };
    if aggregate.declared_type != (Type::Struct { size: 64, align: 4 })
        || result.declared_type != Type::Int
        || !ordinary(aggregate)
        || !ordinary(result)
    {
        return None;
    }

    let [Statement::Assign {
        name: result_target,
        value:
            Expression::Call {
                name: producer,
                arguments: producer_arguments,
            },
    }, Statement::If {
        condition,
        then_body,
        else_body,
    }, Statement::Store {
        target:
            Expression::Member {
                base: stored_aggregate,
                offset: member_offset,
                member_type: Type::UnsignedChar,
                index_stride: None,
            },
        value: stored_value,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if result_target != &result.name
        || !matches!(producer_arguments.as_slice(), [a, b, Expression::AddressOf { operand }]
            if variable(a, &first.name)
                && variable(b, &second.name)
                && variable(operand, &aggregate.name))
        || !else_body.is_empty()
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left: tested_result,
        right: failure_threshold,
    } = condition
    else {
        return None;
    };
    let [Statement::Return(Some(failure_result))] = then_body.as_slice() else {
        return None;
    };
    let failure_threshold = i16::try_from(constant_value(failure_threshold)?).ok()?;
    let member_offset = i16::try_from(*member_offset).ok()?;
    if !variable(tested_result, &result.name)
        || !variable(failure_result, &result.name)
        || !variable(stored_aggregate, &aggregate.name)
        || !variable(stored_value, &member_value.name)
        || !(0..64).contains(&member_offset)
    {
        return None;
    }

    let Expression::Call {
        name: updater,
        arguments: updater_arguments,
    } = function.return_expression.as_ref()?
    else {
        return None;
    };
    if !matches!(updater_arguments.as_slice(), [a, b, Expression::AddressOf { operand }, d]
        if variable(a, &first.name)
            && variable(b, &second.name)
            && variable(operand, &aggregate.name)
            && variable(d, &callback.name))
    {
        return None;
    }

    Some(UpdatePlan {
        first: &first.name,
        second: &second.name,
        member_value: &member_value.name,
        callback: &callback.name,
        aggregate: &aggregate.name,
        result: &result.name,
        producer,
        updater,
        member_offset,
        failure_threshold,
    })
}

impl Generator {
    pub(crate) fn try_guarded_aggregate_update(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.lookup_general(plan.first) != Some(3)
            || self.lookup_general(plan.second) != Some(4)
            || self.lookup_general(plan.member_value) != Some(5)
            || self.lookup_general(plan.callback) != Some(6)
        {
            return Ok(false);
        }
        let _ = (plan.aggregate, plan.result);

        self.output.pre_scheduled = true;
        self.output.anonymous_label_bump += 2;
        self.frame_size = 104;
        self.non_leaf = true;
        self.callee_saved = vec![31, 30, 29, 28];
        let aggregate_offset = 24i16;
        let success = self.fresh_label();
        let done = self.fresh_label();

        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -104,
            });
        for (home, incoming, offset) in [(31, 6, 100), (30, 5, 96)] {
            self.output.instructions.push(Instruction::StoreWord {
                s: home,
                a: 1,
                offset,
            });
            self.output.instructions.push(Instruction::AddImmediate {
                d: home,
                a: incoming,
                immediate: 0,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: aggregate_offset,
        });
        for (home, incoming, offset) in [(29, 4, 92), (28, 3, 88)] {
            self.output.instructions.push(Instruction::StoreWord {
                s: home,
                a: 1,
                offset,
            });
            self.output.instructions.push(Instruction::AddImmediate {
                d: home,
                a: incoming,
                immediate: 0,
            });
        }
        self.record_relocation(RelocationKind::Rel24, plan.producer);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.producer.into(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 3,
                immediate: plan.failure_threshold,
            });
        self.emit_branch_conditional_to(4, 0, success); // bge
        self.emit_branch_to(done);

        self.bind_label(success);
        self.output.instructions.push(Instruction::StoreByte {
            s: 30,
            a: 1,
            offset: aggregate_offset + plan.member_offset,
        });
        for (destination, source) in [(3, 28), (4, 29), (6, 31)] {
            self.output.instructions.push(Instruction::AddImmediate {
                d: destination,
                a: source,
                immediate: 0,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: aggregate_offset,
        });
        self.record_relocation(RelocationKind::Rel24, plan.updater);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.updater.into(),
        });

        self.bind_label(done);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
