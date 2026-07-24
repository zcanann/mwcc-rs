//! Guarded indirect calls through indexed global callback tables.
//!
//! The table entry is both the null-tested condition value and the indirect
//! callee. Lowering the `if` and call independently reloads it; this owner
//! recognizes the complete transaction and keeps the entry in r12 across the
//! branch. An optional leading member-null guard covers wrappers that only
//! dispatch while an associated object is present.

#[allow(unused_imports)]
use super::*;

struct Shape<'a> {
    object: &'a str,
    second_argument: Option<&'a str>,
    object_alias_offset: i16,
    selector_offset: i16,
    callback_table: &'a str,
    entry_guard: EntryGuard,
}

#[derive(Clone, Copy)]
enum EntryGuard {
    None,
    ReturnIfNull(i16),
    EnterIfEitherNull(i16, i16),
}

pub(super) fn variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(candidate) if candidate == name)
}

fn zero(expression: &Expression) -> bool {
    matches!(expression, Expression::IntegerLiteral(0))
}

pub(super) fn member_offset(expression: &Expression, base: &str) -> Option<i16> {
    let Expression::Member {
        base: member_base,
        offset,
        ..
    } = expression
    else {
        return None;
    };
    variable(member_base, base)
        .then(|| i16::try_from(*offset).ok())
        .flatten()
}

fn callback_entry<'a>(expression: &'a Expression, alias: &str) -> Option<(&'a str, i16)> {
    let Expression::Index { base, index } = expression else {
        return None;
    };
    let Expression::Variable(table) = base.as_ref() else {
        return None;
    };
    Some((table, member_offset(index, alias)?))
}

pub(super) fn callback_statement<'a>(
    statement: &'a Statement,
    alias: &str,
    object: &str,
    second_argument: Option<&str>,
) -> Option<(&'a str, i16)> {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if !zero(right) || !else_body.is_empty() {
        return None;
    }
    let (table, selector_offset) = callback_entry(left, alias)?;
    let [Statement::Expression(Expression::CallThrough { target, arguments })] =
        then_body.as_slice()
    else {
        return None;
    };
    if callback_entry(target, alias) != Some((table, selector_offset)) {
        return None;
    }
    let arguments_match = match (second_argument, arguments.as_slice()) {
        (None, [first]) => variable(first, object),
        (Some(second), [first, second_use]) => {
            variable(first, object) && variable(second_use, second)
        }
        _ => false,
    };
    arguments_match.then_some((table, selector_offset))
}

fn early_member_guard(statement: &Statement, alias: &str) -> Option<i16> {
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            },
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    (zero(right)
        && matches!(then_body.as_slice(), [Statement::Return(None)])
        && else_body.is_empty())
    .then(|| member_offset(left, alias))
    .flatten()
}

pub(super) fn either_null_callback<'a>(
    statement: &'a Statement,
    alias: &str,
) -> Option<(i16, i16, &'a Statement)> {
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalOr,
                left,
                right,
            },
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    let null_member = |expression: &Expression| {
        let Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        } = expression
        else {
            return None;
        };
        zero(right).then(|| member_offset(left, alias)).flatten()
    };
    let [callback] = then_body.as_slice() else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    Some((null_member(left)?, null_member(right)?, callback))
}

fn callback_parameter(parameter_type: Type) -> bool {
    matches!(
        parameter_type,
        Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
    )
}

fn classify(function: &Function) -> Option<Shape<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
    {
        return None;
    }
    let (object, second_argument) = match function.parameters.as_slice() {
        [object] => (object, None),
        [object, second] if callback_parameter(second.parameter_type) => (object, Some(second)),
        _ => return None,
    };
    if !matches!(
        object.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) {
        return None;
    }
    let [alias] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(
        alias.declared_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) {
        return None;
    }
    let object_alias_offset = member_offset(alias.initializer.as_ref()?, &object.name)?;
    let (entry_guard, callback) = match function.statements.as_slice() {
        [statement] => {
            if let Some((first, second, callback)) =
                either_null_callback(statement, &alias.name)
            {
                (EntryGuard::EnterIfEitherNull(first, second), callback)
            } else {
                (EntryGuard::None, statement)
            }
        }
        [guard, callback] => (
            EntryGuard::ReturnIfNull(early_member_guard(guard, &alias.name)?),
            callback,
        ),
        _ => return None,
    };
    let (callback_table, selector_offset) = callback_statement(
        callback,
        &alias.name,
        &object.name,
        second_argument.map(|parameter| parameter.name.as_str()),
    )?;
    Some(Shape {
        object: &object.name,
        second_argument: second_argument.map(|parameter| parameter.name.as_str()),
        object_alias_offset,
        selector_offset,
        callback_table,
        entry_guard,
    })
}

impl Generator {
    pub(crate) fn try_guarded_global_callback(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if self.general_register_of(shape.object)? != 3
            || shape
                .second_argument
                .is_some_and(|name| self.general_register_of(name).ok() != Some(4))
            // Function-pointer tables declared through typedefs currently arrive as
            // pointer-typed globals rather than sized arrays.  The indexed AST is the
            // stronger semantic proof here; merely requiring a file-scope symbol keeps
            // local pointer subscripts out of this global-address schedule.
            || !self.globals.contains_key(shape.callback_table)
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.non_leaf = true;
        self.frame_size = 8;

        let done = self.fresh_label();
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });

        if let EntryGuard::ReturnIfNull(member_offset) = shape.entry_guard {
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
                    offset: -8,
                });
            self.output.instructions.push(Instruction::LoadWord {
                d: 4,
                a: 3,
                offset: shape.object_alias_offset,
            });
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: member_offset,
            });
            self.output
                .instructions
                .push(Instruction::CompareLogicalWordImmediate {
                    a: 0,
                    immediate: 0,
                });
            self.emit_branch_conditional_to(12, 2, done);
            self.output.instructions.push(Instruction::LoadWord {
                d: 5,
                a: 4,
                offset: shape.selector_offset,
            });
            self.emit_address_high(4, shape.callback_table);
            self.record_relocation(RelocationKind::Addr16Lo, shape.callback_table);
            self.output.instructions.push(Instruction::AddImmediate {
                d: 0,
                a: 4,
                immediate: 0,
            });
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: 4,
                    s: 5,
                    shift: 2,
                });
            self.output
                .instructions
                .push(Instruction::Add { d: 4, a: 0, b: 4 });
            self.emit_guarded_callback_tail(4, done);
        } else if let EntryGuard::EnterIfEitherNull(first_offset, second_offset) = shape.entry_guard {
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
                    offset: -8,
                });
            self.output.instructions.push(Instruction::LoadWord {
                d: 4,
                a: 3,
                offset: shape.object_alias_offset,
            });
            self.emit_either_null_guarded_callback(
                4,
                first_offset,
                second_offset,
                shape.selector_offset,
                shape.callback_table,
                done,
            );
        } else {
            let table_register = if shape.second_argument.is_some() { 5 } else { 4 };
            let alias_register = table_register + 1;
            self.emit_address_high(table_register, shape.callback_table);
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            });
            self.record_relocation(RelocationKind::Addr16Lo, shape.callback_table);
            self.output.instructions.push(Instruction::AddImmediate {
                d: 0,
                a: table_register,
                immediate: 0,
            });
            self.output
                .instructions
                .push(Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -8,
                });
            self.output.instructions.push(Instruction::LoadWord {
                d: alias_register,
                a: 3,
                offset: shape.object_alias_offset,
            });
            self.output.instructions.push(Instruction::LoadWord {
                d: table_register,
                a: alias_register,
                offset: shape.selector_offset,
            });
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: table_register,
                    s: table_register,
                    shift: 2,
                });
            self.output.instructions.push(Instruction::Add {
                d: table_register,
                a: 0,
                b: table_register,
            });
            self.emit_guarded_callback_tail(table_register, done);
        }

        self.bind_label(done);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 8,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }

    fn emit_guarded_callback_tail(&mut self, entry_address: u8, done: mwcc_vreg::Label) {
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: entry_address,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, done);
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegisterAndLink);
    }

    pub(super) fn emit_either_null_guarded_callback(
        &mut self,
        alias: u8,
        first_offset: i16,
        second_offset: i16,
        selector_offset: i16,
        callback_table: &str,
        done: mwcc_vreg::Label,
    ) {
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: alias,
            offset: first_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 0,
            });
        let dispatch = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, dispatch);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: alias,
            offset: second_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, done);
        self.bind_label(dispatch);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: alias,
            offset: selector_offset,
        });
        self.emit_address_high(alias, callback_table);
        self.record_relocation(RelocationKind::Addr16Lo, callback_table);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: alias,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: alias,
                s: 5,
                shift: 2,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: alias, a: 0, b: alias });
        self.emit_guarded_callback_tail(alias, done);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_condition_that_calls_a_different_table_entry() {
        let condition_entry = Expression::Index {
            base: Box::new(Expression::Variable("callbacks".into())),
            index: Box::new(Expression::Member {
                base: Box::new(Expression::Variable("state".into())),
                offset: 4,
                member_type: Type::Int,
                index_stride: None,
            }),
        };
        let call_entry = Expression::Index {
            base: Box::new(Expression::Variable("callbacks".into())),
            index: Box::new(Expression::Member {
                base: Box::new(Expression::Variable("state".into())),
                offset: 8,
                member_type: Type::Int,
                index_stride: None,
            }),
        };
        let statement = Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left: Box::new(condition_entry),
                right: Box::new(Expression::IntegerLiteral(0)),
            },
            then_body: vec![Statement::Expression(Expression::CallThrough {
                target: Box::new(call_entry),
                arguments: vec![Expression::Variable("object".into())],
            })],
            else_body: vec![],
        };
        assert!(callback_statement(&statement, "state", "object", None).is_none());
    }
}
