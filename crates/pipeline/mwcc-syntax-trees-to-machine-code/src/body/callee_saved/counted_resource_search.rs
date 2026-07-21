//! Counted searches whose acquired resource survives a guarded call sequence.
//!
//! The loop status, counter, acquired resource, and two output pointers all
//! cross calls. Legacy mwcc colors those values as one dense callee-saved
//! region and, with `-use_lmw_stmw on`, saves/restores the region inline.

#[allow(unused_imports)]
use super::*;

struct CountedResourceSearch<'a> {
    id_output: &'a str,
    resource_output: &'a str,
    initial_status: i16,
    bound: i16,
    used_offset: i16,
    get: &'a str,
    acquire: &'a str,
    reset: &'a str,
    reset_argument: i16,
    mark_used: &'a str,
    mark_argument: i16,
    release: &'a str,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn assigned_variable(expression: &Expression, expected: &str, value: i64) -> bool {
    matches!(expression,
        Expression::Assign { target, value: assigned }
            if variable(target, expected) && constant_value(assigned) == Some(value))
}

fn dereference_of(expression: &Expression, expected: &str) -> bool {
    matches!(expression,
        Expression::Dereference { pointer } if variable(pointer, expected))
}

fn is_null_pointer_constant(expression: &Expression) -> bool {
    match expression {
        Expression::Cast { operand, .. } => is_null_pointer_constant(operand),
        _ => constant_value(expression) == Some(0),
    }
}

fn call_with_variable<'a>(statement: &'a Statement, expected: &str) -> Option<&'a str> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    matches!(arguments.as_slice(), [argument] if variable(argument, expected))
        .then_some(name.as_str())
}

fn call_with_variable_and_constant<'a>(
    statement: &'a Statement,
    expected: &str,
) -> Option<(&'a str, i16)> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    let [first, second] = arguments.as_slice() else {
        return None;
    };
    let constant = i16::try_from(constant_value(second)?).ok()?;
    variable(first, expected).then_some((name.as_str(), constant))
}

fn classify(function: &Function) -> Option<CountedResourceSearch<'_>> {
    if !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        || !function.guards.is_empty()
        || function.parameters.len() != 2
        || function.locals.len() != 3
    {
        return None;
    }
    let [id_output, resource_output] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(id_output.parameter_type, Type::Pointer(_))
        || !matches!(resource_output.parameter_type, Type::Pointer(_))
    {
        return None;
    }
    let [status, counter, resource] = function.locals.as_slice() else {
        return None;
    };
    let initial_status = i16::try_from(constant_value(status.initializer.as_ref()?)?).ok()?;
    if !matches!(status.declared_type, Type::Int | Type::UnsignedInt)
        || counter.declared_type != Type::Int
        || counter.initializer.is_some()
        || !matches!(
            resource.declared_type,
            Type::Pointer(_) | Type::StructPointer { .. }
        )
        || resource.initializer.is_some()
        || [status, counter, resource]
            .iter()
            .any(|local| local.array_length.is_some() || local.is_static || local.is_volatile)
        || !matches!(function.return_expression.as_ref(), Some(result) if variable(result, &status.name))
    {
        return None;
    }

    let [Statement::Store {
        target: initial_output,
        value: null_value,
    }, Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !dereference_of(initial_output, &resource_output.name)
        || !is_null_pointer_constant(null_value)
        || !assigned_variable(initializer, &counter.name, 0)
    {
        return None;
    }
    let bound = match condition {
        Expression::Binary {
            operator: BinaryOperator::Less,
            left,
            right,
        } if variable(left, &counter.name) => i16::try_from(constant_value(right)?).ok()?,
        _ => return None,
    };
    if bound <= 0
        || !matches!(step,
            Expression::Assign { target, value }
                if variable(target, &counter.name)
                    && matches!(value.as_ref(), Expression::Binary {
                        operator: BinaryOperator::Add,
                        left,
                        right,
                    } if variable(left, &counter.name) && constant_value(right) == Some(1)))
    {
        return None;
    }

    let [Statement::Assign {
        name: assigned_resource,
        value:
            Expression::Call {
                name: get,
                arguments: get_arguments,
            },
    }, acquire_statement, Statement::If {
        condition: used_condition,
        then_body,
        else_body,
    }, release_statement] = body.as_slice()
    else {
        return None;
    };
    if assigned_resource != &resource.name
        || !matches!(get_arguments.as_slice(), [argument] if variable(argument, &counter.name))
        || !else_body.is_empty()
    {
        return None;
    }
    let acquire = call_with_variable(acquire_statement, &resource.name)?;
    let release = call_with_variable(release_statement, &resource.name)?;
    let used_offset = match used_condition {
        Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand,
        } => match operand.as_ref() {
            Expression::Member {
                base,
                offset,
                member_type,
                index_stride: None,
            } if variable(base, &resource.name)
                && matches!(member_type, Type::Int | Type::UnsignedInt) =>
            {
                i16::try_from(*offset).ok()?
            }
            _ => return None,
        },
        _ => return None,
    };

    let [reset_statement, mark_statement, Statement::Assign {
        name: assigned_status,
        value: success_status,
    }, Statement::Store {
        target: selected_output,
        value: selected_resource,
    }, Statement::Store {
        target: selected_id,
        value: selected_counter,
    }, Statement::Assign {
        name: forced_counter,
        value: forced_bound,
    }] = then_body.as_slice()
    else {
        return None;
    };
    let (reset, reset_argument) = call_with_variable_and_constant(reset_statement, &resource.name)?;
    let (mark_used, mark_argument) =
        call_with_variable_and_constant(mark_statement, &resource.name)?;
    if assigned_status != &status.name
        || constant_value(success_status) != Some(0)
        || !dereference_of(selected_output, &resource_output.name)
        || !variable(selected_resource, &resource.name)
        || !dereference_of(selected_id, &id_output.name)
        || !variable(selected_counter, &counter.name)
        || forced_counter != &counter.name
        || constant_value(forced_bound) != Some(i64::from(bound))
    {
        return None;
    }

    Some(CountedResourceSearch {
        id_output: &id_output.name,
        resource_output: &resource_output.name,
        initial_status,
        bound,
        used_offset,
        get,
        acquire,
        reset,
        reset_argument,
        mark_used,
        mark_argument,
        release,
    })
}

impl Generator {
    /// Lower a counted acquire/test/reset/release search. The virtual homes make
    /// the five independent survivor lifetimes explicit; measured preferences
    /// select mwcc's dense descending region while the save instruction derives
    /// its physical first register solely from the region width.
    pub(crate) fn try_counted_resource_search(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.plain_linkage_epilogue_style
                != PlainLinkageEpilogueStyle::StackRestoreBeforeReload
            || !self.behavior.use_lmw_stmw
            || !self.frame_slots.is_empty()
            || self
                .locations
                .get(shape.id_output)
                .is_none_or(|location| location.register != 3)
            || self
                .locations
                .get(shape.resource_output)
                .is_none_or(|location| location.register != 4)
        {
            return Ok(false);
        }

        let status = self.fresh_virtual_general_preferring(31);
        let counter = self.fresh_virtual_general_preferring(30);
        let resource = self.fresh_virtual_general_preferring(29);
        let resource_output = self.fresh_virtual_general_preferring(28);
        let id_output = self.fresh_virtual_general_preferring(27);
        let saved_count = 5u8;
        let first_saved = 32 - saved_count;

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![status, counter, resource, resource_output, id_output];
        self.output.pre_scheduled = true;
        self.output.anonymous_label_bump = 6;

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
            .push(Instruction::load_immediate(0, 0));
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        self.output
            .instructions
            .push(Instruction::StoreMultipleWord {
                s: first_saved,
                a: 1,
                offset: 12,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: resource_output,
            a: 4,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: id_output,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(status, shape.initial_status));
        self.output
            .instructions
            .push(Instruction::load_immediate(counter, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 0,
        });

        let loop_body = self.fresh_label();
        let loop_test = self.fresh_label();
        let release = self.fresh_label();
        self.emit_branch_to(loop_test);
        self.bind_label(loop_body);

        self.output
            .instructions
            .push(Instruction::move_register(3, counter));
        self.record_relocation(RelocationKind::Rel24, shape.get);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.get.to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(resource, 3));
        self.record_relocation(RelocationKind::Rel24, shape.acquire);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.acquire.to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: resource,
            offset: shape.used_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, release);

        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: resource,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, shape.reset_argument));
        self.record_relocation(RelocationKind::Rel24, shape.reset);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.reset.to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: resource,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, shape.mark_argument));
        self.record_relocation(RelocationKind::Rel24, shape.mark_used);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.mark_used.to_string(),
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: resource,
            a: resource_output,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(status, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: counter,
            a: id_output,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(counter, shape.bound));

        self.bind_label(release);
        self.output
            .instructions
            .push(Instruction::move_register(3, resource));
        self.record_relocation(RelocationKind::Rel24, shape.release);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.release.to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: counter,
            a: counter,
            immediate: 1,
        });
        self.bind_label(loop_test);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: counter,
                immediate: shape.bound,
            });
        self.emit_branch_conditional_to(12, 0, loop_body);

        self.output
            .instructions
            .push(Instruction::move_register(3, status));
        self.output
            .instructions
            .push(Instruction::LoadMultipleWord {
                d: first_saved,
                a: 1,
                offset: 12,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
