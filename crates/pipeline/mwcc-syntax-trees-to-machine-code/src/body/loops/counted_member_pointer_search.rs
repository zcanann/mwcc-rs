//! Counted searches strength-reduced from an indexed member pointer.
//!
//! MWCC recognizes that an explicitly advanced pointer local and
//! `object->table[index]` describe the same induction stream. It retains the
//! pointer, advances it by one element, and moves the decreasing bound into
//! CTR instead of reloading the member and rescaling the index each iteration.

#[allow(unused_imports)]
use super::*;

struct CountedMemberPointerSearch<'a> {
    object: &'a str,
    needle: &'a str,
    count_offset: i16,
    pointer_offset: i16,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn assigned_value<'a>(expression: &'a Expression, expected: &str) -> Option<&'a Expression> {
    match expression {
        Expression::Assign { target, value } if variable(target) == Some(expected) => Some(value),
        _ => None,
    }
}

fn increments(expression: &Expression, expected: &str) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } if variable(left) == Some(expected) && constant_value(right) == Some(1)
    )
}

fn member<'a>(
    expression: &'a Expression,
    expected_base: &str,
    expected_type: Type,
) -> Option<u32> {
    match expression {
        Expression::Member {
            base,
            offset,
            member_type,
            index_stride: None,
        } if variable(base) == Some(expected_base) && *member_type == expected_type => Some(*offset),
        _ => None,
    }
}

fn recognize(function: &Function) -> Option<CountedMemberPointerSearch<'_>> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || function_makes_call(function)
        || constant_value(function.return_expression.as_ref()?) != Some(-1)
    {
        return None;
    }
    let [object, needle] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(object.parameter_type, Type::StructPointer { .. })
        || needle.parameter_type != Type::UnsignedInt
    {
        return None;
    }
    let [index, cursor, count] = function.locals.as_slice() else {
        return None;
    };
    if index.declared_type != Type::Int
        || index.initializer.as_ref().and_then(constant_value) != Some(0)
        || cursor.declared_type != Type::Pointer(Pointee::UnsignedInt)
        || count.declared_type != Type::Int
        || count.initializer.is_some()
        || function.locals.iter().any(|local| {
            local.is_volatile || local.array_length.is_some() || local.is_static
        })
    {
        return None;
    }
    let pointer_offset = member(
        cursor.initializer.as_ref()?,
        &object.name,
        Type::Pointer(Pointee::UnsignedInt),
    )?;

    let [Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let count_offset = member(
        assigned_value(initializer, &count.name)?,
        &object.name,
        Type::Int,
    )?;
    if !matches!(
        condition,
        Expression::Binary {
            operator: BinaryOperator::Greater,
            left,
            right,
        } if variable(left) == Some(&count.name) && constant_value(right) == Some(0)
    ) || !matches!(
        assigned_value(step, &count.name),
        Some(Expression::Binary {
            operator: BinaryOperator::Subtract,
            left,
            right,
        }) if variable(left) == Some(&count.name) && constant_value(right) == Some(1)
    ) {
        return None;
    }

    let [Statement::If {
        condition: found,
        then_body,
        else_body,
    }, Statement::Assign {
        name: advanced_cursor,
        value: cursor_step,
    }, Statement::Assign {
        name: advanced_index,
        value: index_step,
    }] = body.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty()
        || !matches!(then_body.as_slice(), [Statement::Return(Some(value))] if variable(value) == Some(&index.name))
        || advanced_cursor != &cursor.name
        || !increments(cursor_step, &cursor.name)
        || advanced_index != &index.name
        || !increments(index_step, &index.name)
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = found
    else {
        return None;
    };
    let indexed_member = |expression: &Expression| {
        let Expression::Index { base, index: subscript } = expression else {
            return false;
        };
        variable(subscript) == Some(&index.name)
            && member(base, &object.name, Type::Pointer(Pointee::UnsignedInt))
                == Some(pointer_offset)
    };
    if !((variable(left) == Some(&needle.name) && indexed_member(right))
        || (variable(right) == Some(&needle.name) && indexed_member(left)))
    {
        return None;
    }

    Some(CountedMemberPointerSearch {
        object: &object.name,
        needle: &needle.name,
        count_offset: i16::try_from(count_offset).ok()?,
        pointer_offset: i16::try_from(pointer_offset).ok()?,
    })
}

impl Generator {
    pub(crate) fn try_counted_member_pointer_search(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(search) = recognize(function) else {
            return Ok(false);
        };
        if !self.frame_slots.is_empty() || !self.output.instructions.is_empty() {
            return Ok(false);
        }
        let object = self.general_register_of(search.object)?;
        let needle = self.general_register_of(search.needle)?;
        if (object, needle) != (3, 4) {
            return Ok(false);
        }
        let cursor = 5;
        let index = 6;

        self.output.pre_scheduled = true;
        self.output.anonymous_label_bump += 7;
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: GENERAL_SCRATCH,
                a: object,
                offset: search.count_offset,
            },
            Instruction::load_immediate(index, 0),
            Instruction::LoadWord {
                d: cursor,
                a: object,
                offset: search.pointer_offset,
            },
            Instruction::MoveToCountRegister { s: GENERAL_SCRATCH },
            Instruction::CompareWordImmediate {
                a: GENERAL_SCRATCH,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 14,
            },
            Instruction::LoadWord {
                d: GENERAL_SCRATCH,
                a: cursor,
                offset: 0,
            },
            Instruction::CompareLogicalWord {
                a: needle,
                b: GENERAL_SCRATCH,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 11,
            },
            Instruction::move_register(Eabi::general_result().number, index),
            Instruction::BranchToLinkRegister,
            Instruction::AddImmediate {
                d: cursor,
                a: cursor,
                immediate: i16::from(Pointee::UnsignedInt.size()),
            },
            Instruction::AddImmediate {
                d: index,
                a: index,
                immediate: 1,
            },
            Instruction::BranchConditionalForward {
                options: 16,
                condition_bit: 0,
                target: 6,
            },
            Instruction::load_immediate(Eabi::general_result().number, -1),
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{LocalDeclaration, Parameter};

    fn name(value: &str) -> Expression {
        Expression::Variable(value.into())
    }

    fn binary(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn assign(target: &str, value: Expression) -> Expression {
        Expression::Assign {
            target: Box::new(name(target)),
            value: Box::new(value),
        }
    }

    fn object_member(offset: u32, member_type: Type) -> Expression {
        Expression::Member {
            base: Box::new(name("object")),
            offset,
            member_type,
            index_stride: None,
        }
    }

    fn local(name: &str, declared_type: Type, initializer: Option<Expression>) -> LocalDeclaration {
        LocalDeclaration {
            declared_type,
            name: name.into(),
            initializer,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    fn function() -> Function {
        Function {
            return_type: Type::Int,
            name: "member_search".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 52 },
                    name: "object".into(),
                },
                Parameter {
                    parameter_type: Type::UnsignedInt,
                    name: "needle".into(),
                },
            ],
            locals: vec![
                local("index", Type::Int, Some(Expression::IntegerLiteral(0))),
                local(
                    "cursor",
                    Type::Pointer(Pointee::UnsignedInt),
                    Some(object_member(12, Type::Pointer(Pointee::UnsignedInt))),
                ),
                local("count", Type::Int, None),
            ],
            statements: vec![Statement::Loop {
                kind: LoopKind::For,
                initializer: Some(assign("count", object_member(8, Type::Int))),
                condition: Some(binary(
                    BinaryOperator::Greater,
                    name("count"),
                    Expression::IntegerLiteral(0),
                )),
                step: Some(assign(
                    "count",
                    binary(
                        BinaryOperator::Subtract,
                        name("count"),
                        Expression::IntegerLiteral(1),
                    ),
                )),
                body: vec![
                    Statement::If {
                        condition: binary(
                            BinaryOperator::Equal,
                            name("needle"),
                            Expression::Index {
                                base: Box::new(object_member(
                                    12,
                                    Type::Pointer(Pointee::UnsignedInt),
                                )),
                                index: Box::new(name("index")),
                            },
                        ),
                        then_body: vec![Statement::Return(Some(name("index")))],
                        else_body: Vec::new(),
                    },
                    Statement::Assign {
                        name: "cursor".into(),
                        value: binary(
                            BinaryOperator::Add,
                            name("cursor"),
                            Expression::IntegerLiteral(1),
                        ),
                    },
                    Statement::Assign {
                        name: "index".into(),
                        value: binary(
                            BinaryOperator::Add,
                            name("index"),
                            Expression::IntegerLiteral(1),
                        ),
                    },
                ],
            }],
            guards: Vec::new(),
            return_expression: Some(Expression::IntegerLiteral(-1)),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn recognizes_a_counted_member_pointer_induction_search() {
        let function = function();
        let search = recognize(&function).expect("counted member-pointer search");
        assert_eq!(search.object, "object");
        assert_eq!(search.needle, "needle");
        assert_eq!((search.count_offset, search.pointer_offset), (8, 12));
    }

    #[test]
    fn rejects_a_mismatched_indexed_member() {
        let mut function = function();
        let Statement::Loop { body, .. } = &mut function.statements[0] else {
            unreachable!()
        };
        let Statement::If { condition, .. } = &mut body[0] else {
            unreachable!()
        };
        let Expression::Binary { right, .. } = condition else {
            unreachable!()
        };
        let Expression::Index { base, .. } = right.as_mut() else {
            unreachable!()
        };
        *base = Box::new(object_member(16, Type::Pointer(Pointee::UnsignedInt)));
        assert!(recognize(&function).is_none());
    }
}
