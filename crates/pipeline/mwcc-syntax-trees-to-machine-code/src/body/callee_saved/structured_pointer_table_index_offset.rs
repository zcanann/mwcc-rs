//! Shared byte offsets for repeated file-scope pointer-table accesses.
//!
//! When one derived source index addresses multiple pointer tables across calls,
//! MWCC scales it once and keeps the four-byte offset live. Exposing that value
//! before liveness planning prevents each access from inventing its own shift.

use super::*;
use super::structured_expression_visit::visit_statement;

pub(super) fn prescale_repeated_pointer_table_index(
    function: &Function,
    globals: &std::collections::HashMap<String, Type>,
) -> Option<Function> {
    if !function.guards.is_empty() {
        return None;
    }
    for (position, statement) in function.statements.iter().enumerate() {
        let Statement::Assign { name: index, .. } = statement else {
            continue;
        };
        if !function.locals.iter().any(|local| {
            local.name == *index && local.declared_type.width() == 32
        }) || function
            .return_expression
            .as_ref()
            .is_some_and(|value| crate::analysis::expression_reads_name(value, index))
        {
            continue;
        }

        let following = &function.statements[position + 1..];
        if following.iter().any(|statement| {
            matches!(statement, Statement::Assign { name, .. } if name == index)
        }) {
            continue;
        }
        let mut reads = 0usize;
        let mut pointer_indices = 0usize;
        for statement in following {
            visit_statement(statement, &mut |expression| match expression {
                Expression::Variable(name) if name == index => reads += 1,
                Expression::Index { base, index: used } => {
                    let (Expression::Variable(global), Expression::Variable(used)) =
                        (base.as_ref(), used.as_ref())
                    else {
                        return;
                    };
                    if used == index
                        && matches!(
                            globals.get(global),
                            Some(Type::Pointer(Pointee::Pointer | Pointee::WordPointer))
                        )
                    {
                        pointer_indices += 1;
                    }
                }
                _ => {}
            });
        }
        if pointer_indices < 2 || pointer_indices != reads {
            continue;
        }
        let first_use = following
            .iter()
            .position(|statement| pointer_table_index_count(statement, index, globals) != 0)
            .expect("the repeated pointer-table index count was nonzero")
            + position
            + 1;
        let insertion_position = function.statements[position + 1..first_use]
            .iter()
            .rposition(crate::analysis::statement_has_call)
            .map_or(position + 1, |call| position + call + 2);

        let mut used: std::collections::HashSet<String> = function
            .parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .chain(function.locals.iter().map(|local| local.name.clone()))
            .collect();
        let offset = (0usize..)
            .map(|ordinal| {
                format!(
                    "{}{}",
                    crate::analysis::PRESCALED_POINTER_TABLE_INDEX_PREFIX,
                    ordinal
                )
            })
            .find(|candidate| used.insert(candidate.clone()))
            .expect("the synthetic pointer-table offset namespace is unbounded");

        let mut reduced = function.clone();
        let local_position = reduced
            .locals
            .iter()
            .position(|local| local.name == *index)
            .expect("the repeated pointer-table index local was checked");
        reduced.locals.insert(
            local_position,
            LocalDeclaration {
                declared_type: Type::UnsignedInt,
                name: offset.clone(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            },
        );
        reduced.statements.insert(
            insertion_position,
            Statement::Assign {
                name: offset.clone(),
                value: Expression::Binary {
                    operator: BinaryOperator::ShiftLeft,
                    left: Box::new(Expression::Variable(index.clone())),
                    right: Box::new(Expression::IntegerLiteral(2)),
                },
            },
        );
        let values = std::collections::HashMap::from([(
            index.clone(),
            Expression::Variable(offset.clone()),
        )]);
        for statement in &mut reduced.statements[insertion_position + 1..] {
            *statement = super::structured_pointer_table_index_cursor::substitute_statement(
                statement,
                &values,
            );
        }
        reuse_expired_index_home(
            &mut reduced,
            insertion_position + 1,
            index,
            &offset,
        );
        return Some(reduced);
    }
    None
}

fn reuse_expired_index_home(
    function: &mut Function,
    assignment_position: usize,
    index: &str,
    offset: &str,
) {
    let Some(Statement::Assign { name: reused, value }) =
        function.statements.get(assignment_position)
    else {
        return;
    };
    if crate::analysis::expression_reads_name(value, index)
        || crate::analysis::expression_reads_name(value, offset)
        || function
            .return_expression
            .as_ref()
            .is_some_and(|value| crate::analysis::expression_reads_name(value, reused))
        || !function.locals.iter().any(|local| {
            local.name == *reused
                && local.declared_type.width() == 32
                && local.initializer.is_none()
        })
        || function
            .statements
            .iter()
            .filter(|statement| {
                matches!(statement, Statement::Assign { name, .. } if name == reused)
            })
            .count()
            != 1
    {
        return;
    }
    let reused = reused.clone();
    let value = value.clone();
    function.statements[assignment_position] = Statement::Assign {
        name: index.to_owned(),
        value,
    };
    let replacements = std::collections::HashMap::from([(
        reused.clone(),
        Expression::Variable(index.to_owned()),
    )]);
    for statement in &mut function.statements[assignment_position + 1..] {
        *statement = super::structured_pointer_table_index_cursor::substitute_statement(
            statement,
            &replacements,
        );
    }
    function.locals.retain(|local| local.name != reused);
}

fn pointer_table_index_count(
    statement: &Statement,
    index: &str,
    globals: &std::collections::HashMap<String, Type>,
) -> usize {
    let mut count = 0usize;
    visit_statement(statement, &mut |expression| {
        let Expression::Index { base, index: used } = expression else {
            return;
        };
        let (Expression::Variable(global), Expression::Variable(used)) =
            (base.as_ref(), used.as_ref())
        else {
            return;
        };
        if used == index
            && matches!(
                globals.get(global),
                Some(Type::Pointer(Pointee::Pointer | Pointee::WordPointer))
            )
        {
            count += 1;
        }
    });
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_index(table: &str) -> Statement {
        Statement::Expression(Expression::Index {
            base: Box::new(Expression::Variable(table.into())),
            index: Box::new(Expression::Variable("i".into())),
        })
    }

    #[test]
    fn prescales_a_repeated_index_after_the_last_preceding_call() {
        let function = Function {
            return_type: Type::Void,
            name: "lookup".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![LocalDeclaration {
                declared_type: Type::UnsignedInt,
                name: "i".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            }],
            statements: vec![
                Statement::Assign {
                    name: "i".into(),
                    value: Expression::IntegerLiteral(3),
                },
                Statement::Expression(Expression::Call {
                    name: "prepare".into(),
                    arguments: Vec::new(),
                }),
                table_index("first"),
                table_index("second"),
            ],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let globals = std::collections::HashMap::from([
            ("first".into(), Type::Pointer(Pointee::Pointer)),
            ("second".into(), Type::Pointer(Pointee::Pointer)),
        ]);

        let reduced = prescale_repeated_pointer_table_index(&function, &globals)
            .expect("repeated pointer-table indices should be prescaled");
        let offset = &reduced.locals[0].name;
        assert!(offset.starts_with(
            crate::analysis::PRESCALED_POINTER_TABLE_INDEX_PREFIX
        ));
        assert!(matches!(
            &reduced.statements[2],
            Statement::Assign { name, .. } if name == offset
        ));
        assert!(matches!(
            &reduced.statements[3],
            Statement::Expression(Expression::Index { index, .. })
                if matches!(index.as_ref(), Expression::Variable(name) if name == offset)
        ));
    }
}
