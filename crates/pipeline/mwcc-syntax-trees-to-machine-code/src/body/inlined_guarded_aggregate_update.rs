//! Call-site composition of a summarized guarded aggregate update.
//!
//! The source wrapper calls a helper and then synchronizes on success.  Under
//! build-163 `-inline all`, the helper's producer/aggregate/update transaction
//! is expanded into the caller.  The helper body is proven once by
//! [`InlineSummaries`]; this module recognizes only the wrapper and owns the
//! measured composed schedule.

use super::*;

struct WrapperPlan<'a> {
    first: &'a str,
    second: &'a str,
    member_value: &'a str,
    callback: &'a str,
    producer: String,
    updater: String,
    sync: &'a str,
    member_offset: i16,
    failure_threshold: i16,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn classify<'a>(generator: &Generator, function: &'a Function) -> Option<WrapperPlan<'a>> {
    if function.return_type != Type::Int
        || function.parameters.len() != 3
        || function.locals.len() != 1
        || function.statements.len() != 1
        || function.guards.len() != 1
    {
        return None;
    }
    let [first, second, member_value] = function.parameters.as_slice() else {
        return None;
    };
    if first.parameter_type != Type::Int
        || second.parameter_type != Type::Int
        || member_value.parameter_type != Type::UnsignedChar
    {
        return None;
    }
    let [result] = function.locals.as_slice() else {
        return None;
    };
    if result.declared_type != Type::Int
        || result.initializer.is_some()
        || result.is_static
        || result.is_volatile
        || result.array_length.is_some()
    {
        return None;
    }
    let [Statement::Assign {
        name: result_target,
        value:
            Expression::Call {
                name: helper,
                arguments: helper_arguments,
            },
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let [first_argument, second_argument, member_argument, Expression::Variable(callback)] =
        helper_arguments.as_slice()
    else {
        return None;
    };
    if result_target != &result.name
        || !variable(first_argument, &first.name)
        || !variable(second_argument, &second.name)
        || !variable(member_argument, &member_value.name)
    {
        return None;
    }
    let summary = generator
        .inline_summaries
        .guarded_aggregate_update(helper)?
        .clone();

    let guard = &function.guards[0];
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left: tested_result,
        right: failure_threshold,
    } = &guard.condition
    else {
        return None;
    };
    let failure_threshold = i16::try_from(constant_value(failure_threshold)?).ok()?;
    if !variable(tested_result, &result.name) || !variable(&guard.value, &result.name) {
        return None;
    }

    let Expression::Call {
        name: sync,
        arguments: sync_arguments,
    } = function.return_expression.as_ref()?
    else {
        return None;
    };
    if !matches!(sync_arguments.as_slice(), [argument] if variable(argument, &first.name)) {
        return None;
    }

    Some(WrapperPlan {
        first: &first.name,
        second: &second.name,
        member_value: &member_value.name,
        callback,
        producer: summary.producer,
        updater: summary.updater,
        sync,
        member_offset: summary.member_offset,
        failure_threshold,
    })
}

impl Generator {
    pub(crate) fn try_inlined_guarded_aggregate_update(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(self, function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.lookup_general(plan.first) != Some(3)
            || self.lookup_general(plan.second) != Some(4)
            || self.lookup_general(plan.member_value) != Some(5)
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        // The expanded helper contributes its branch graph; the wrapper adds
        // the second status guard.
        self.output.anonymous_label_bump += 4;
        self.frame_size = 104;
        self.non_leaf = true;
        self.callee_saved = vec![31, 30, 29];
        let aggregate_offset = 20i16;
        let helper_success = self.fresh_label();
        let wrapper_guard = self.fresh_label();
        let sync = self.fresh_label();
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
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 100,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 5,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: aggregate_offset,
        });
        for (home, incoming, offset) in [(30, 4, 96), (29, 3, 92)] {
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
        self.record_relocation(RelocationKind::Rel24, &plan.producer);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.producer,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 3,
                immediate: plan.failure_threshold,
            });
        self.emit_branch_conditional_to(4, 0, helper_success); // bge
        self.emit_branch_to(wrapper_guard);

        self.bind_label(helper_success);
        self.emit_address_high(3, plan.callback);
        self.output.instructions.push(Instruction::StoreByte {
            s: 31,
            a: 1,
            offset: aggregate_offset + plan.member_offset,
        });
        self.record_relocation(RelocationKind::Addr16Lo, plan.callback);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 29,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 30,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: aggregate_offset,
        });
        self.record_relocation(RelocationKind::Rel24, &plan.updater);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.updater,
        });

        self.bind_label(wrapper_guard);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 3,
                immediate: plan.failure_threshold,
            });
        self.emit_branch_conditional_to(4, 0, sync); // bge
        self.emit_branch_to(done);
        self.bind_label(sync);
        self.output
            .instructions
            .push(Instruction::move_register(3, 29));
        self.record_relocation(RelocationKind::Rel24, plan.sync);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.sync.into(),
        });

        self.bind_label(done);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
