//! Coalesce call-free runs of macro-expanded display-list packet writes.
//!
//! Graphics macros commonly expand each eight-byte packet into a postincrement
//! alias followed by two word stores. MWCC keeps one cursor base for a call-free
//! run, addresses every packet by displacement, then advances the cursor once.
//! Rewriting that source pattern exposes the same store run to the generic
//! structured emitter without tying it to a particular command encoding.

use super::*;

struct Packet<'a> {
    alias: &'a str,
    cursor: &'a str,
    first_value: &'a Expression,
    second_value: &'a Expression,
}

fn packet<'a>(statements: &'a [Statement]) -> Option<Packet<'a>> {
    let [Statement::Assign { name: alias, value }, Statement::Store {
        target: first_target,
        value: first_value,
    }, Statement::Store {
        target: second_target,
        value: second_value,
    }, ..] = statements
    else {
        return None;
    };
    let poststep = match value {
        Expression::Cast { operand, .. } => operand.as_ref(),
        expression => expression,
    };
    let Expression::PostStep {
        target,
        operator: BinaryOperator::Add,
        pointer_link: None,
    } = poststep
    else {
        return None;
    };
    let Expression::Variable(cursor) = target.as_ref() else {
        return None;
    };
    if cursor == alias
        || expression_has_call(first_value)
        || expression_has_call(second_value)
        || expression_reads_name(first_value, cursor)
        || expression_reads_name(second_value, cursor)
        || expression_reads_name(first_value, alias)
        || expression_reads_name(second_value, alias)
    {
        return None;
    }
    let member = |target: &'a Expression, expected_offset| {
        let Expression::Member {
            base,
            offset,
            member_type,
            index_stride: None,
        } = target
        else {
            return false;
        };
        matches!(base.as_ref(), Expression::Variable(name) if name == alias)
            && *offset == expected_offset
            && matches!(member_type, Type::Int | Type::UnsignedInt)
    };
    if !member(first_target, 0) || !member(second_target, 4) {
        return None;
    }
    Some(Packet {
        alias,
        cursor,
        first_value,
        second_value,
    })
}

fn cursor_is_replaced_by_call(statements: &[Statement], cursor: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign { name, value } => name == cursor && expression_has_call(value),
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            cursor_is_replaced_by_call(then_body, cursor)
                || cursor_is_replaced_by_call(else_body, cursor)
        }
        Statement::Loop { body, .. } => cursor_is_replaced_by_call(body, cursor),
        Statement::Switch { arms, default, .. } => {
            arms.iter().any(|arm| {
                matches!(
                    &arm.body,
                    mwcc_syntax_trees::ArmBody::Statements(body)
                        if cursor_is_replaced_by_call(body, cursor)
                )
            }) || matches!(
                default,
                Some(mwcc_syntax_trees::ArmBody::Statements(body))
                    if cursor_is_replaced_by_call(body, cursor)
            )
        }
        _ => false,
    })
}

fn coalesce_statements(
    statements: &[Statement],
    function: &Function,
    changed: &mut bool,
) -> Vec<Statement> {
    let mut output = Vec::with_capacity(statements.len());
    let mut index = 0;
    while index < statements.len() {
        let Some(first) = packet(&statements[index..]) else {
            let mut statement = statements[index].clone();
            match &mut statement {
                Statement::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    *then_body = coalesce_statements(then_body, function, changed);
                    *else_body = coalesce_statements(else_body, function, changed);
                }
                Statement::Loop { body, .. } => {
                    *body = coalesce_statements(body, function, changed);
                }
                Statement::Switch { arms, default, .. } => {
                    for arm in arms {
                        if let mwcc_syntax_trees::ArmBody::Statements(body) = &mut arm.body {
                            *body = coalesce_statements(body, function, changed);
                        }
                    }
                    if let Some(mwcc_syntax_trees::ArmBody::Statements(body)) = default {
                        *body = coalesce_statements(body, function, changed);
                    }
                }
                _ => {}
            }
            output.push(statement);
            index += 1;
            continue;
        };
        let cursor_eligible = function.locals.iter().any(|local| {
            local.name == first.cursor
                && !local.is_volatile
                && matches!(local.declared_type, Type::StructPointer { element_size: 8 })
        }) && !cursor_is_replaced_by_call(&function.statements, first.cursor);
        let alias_eligible = |alias: &str| {
            function.locals.iter().any(|local| {
                local.name == alias
                    && !local.is_volatile
                    && matches!(local.declared_type, Type::StructPointer { element_size: 8 })
            })
        };
        if !cursor_eligible || !alias_eligible(first.alias) {
            output.push(statements[index].clone());
            index += 1;
            continue;
        }

        let mut packets = Vec::new();
        let mut cursor = index;
        while let Some(next) = packet(&statements[cursor..]) {
            if next.cursor != first.cursor || !alias_eligible(next.alias) {
                break;
            }
            packets.push(next);
            cursor += 3;
        }
        if packets.len() < 2 {
            output.push(statements[index].clone());
            index += 1;
            continue;
        }

        for (packet_index, packet) in packets.iter().enumerate() {
            let offset = u32::try_from(packet_index * 8).expect("packet run offset");
            output.push(Statement::Store {
                target: Expression::Member {
                    base: Box::new(Expression::Variable(first.cursor.into())),
                    offset,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
                value: packet.first_value.clone(),
            });
            output.push(Statement::Store {
                target: Expression::Member {
                    base: Box::new(Expression::Variable(first.cursor.into())),
                    offset: offset + 4,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
                value: packet.second_value.clone(),
            });
        }
        output.push(Statement::Assign {
            name: first.cursor.into(),
            value: Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(Expression::Variable(first.cursor.into())),
                right: Box::new(Expression::IntegerLiteral(
                    i64::try_from(packets.len()).expect("packet count"),
                )),
            },
        });
        *changed = true;
        index = cursor;
    }
    output
}

pub(crate) fn coalesce_display_list_packet_runs(function: &Function) -> Option<Function> {
    let mut changed = false;
    let statements = coalesce_statements(&function.statements, function, &mut changed);
    changed.then(|| Function {
        statements,
        ..function.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::StructPointer { element_size: 8 },
            name: name.into(),
            initializer: None,
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

    fn packet(alias: &str, cursor: &str, first: i64, second: i64) -> Vec<Statement> {
        vec![
            Statement::Assign {
                name: alias.into(),
                value: Expression::Cast {
                    target_type: Type::StructPointer { element_size: 8 },
                    operand: Box::new(Expression::PostStep {
                        target: Box::new(Expression::Variable(cursor.into())),
                        operator: BinaryOperator::Add,
                        pointer_link: None,
                    }),
                },
            },
            Statement::Store {
                target: Expression::Member {
                    base: Box::new(Expression::Variable(alias.into())),
                    offset: 0,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
                value: Expression::IntegerLiteral(first),
            },
            Statement::Store {
                target: Expression::Member {
                    base: Box::new(Expression::Variable(alias.into())),
                    offset: 4,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
                value: Expression::IntegerLiteral(second),
            },
        ]
    }

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "packets".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![local("cursor"), local("packet0"), local("packet1")],
            statements,
            guards: Vec::new(),
            return_expression: None,
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
    fn coalesces_a_nested_packet_run_onto_one_cursor() {
        let mut body = packet("packet0", "cursor", 1, 2);
        body.extend(packet("packet1", "cursor", 3, 4));
        let function = function(vec![Statement::If {
            condition: Expression::IntegerLiteral(1),
            then_body: body,
            else_body: Vec::new(),
        }]);

        let rewritten = coalesce_display_list_packet_runs(&function).expect("coalesced packet run");
        let Statement::If { then_body, .. } = &rewritten.statements[0] else {
            unreachable!()
        };
        assert_eq!(then_body.len(), 5);
        for (index, statement) in then_body[..4].iter().enumerate() {
            assert!(matches!(
                statement,
                Statement::Store {
                    target: Expression::Member { base, offset, .. },
                    ..
                } if matches!(base.as_ref(), Expression::Variable(name) if name == "cursor")
                    && *offset == u32::try_from(index * 4).unwrap()
            ));
        }
        assert!(matches!(
            &then_body[4],
            Statement::Assign {
                name,
                value: Expression::Binary {
                    operator: BinaryOperator::Add,
                    right,
                    ..
                },
            } if name == "cursor" && constant_value(right) == Some(2)
        ));
    }

    #[test]
    fn leaves_a_cursor_replaced_by_a_call_for_the_call_scheduler() {
        let mut statements = packet("packet0", "cursor", 1, 2);
        statements.extend(packet("packet1", "cursor", 3, 4));
        statements.push(Statement::Assign {
            name: "cursor".into(),
            value: Expression::Call {
                name: "replace_cursor".into(),
                arguments: vec![Expression::Variable("cursor".into())],
            },
        });

        assert!(coalesce_display_list_packet_runs(&function(statements)).is_none());
    }
}
