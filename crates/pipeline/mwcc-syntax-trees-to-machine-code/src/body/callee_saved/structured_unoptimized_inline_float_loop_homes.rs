//! O0 source-image FPR homes retained through an inlined loop expression.
//!
//! Automatic inline expansion represents a value-returning helper as a comma
//! chain of hygienic assignments. At O0, MWCC keeps those source bindings in a
//! descending saved-FPR window even when ordinary call liveness would permit
//! volatile registers. The returned binding owns the top of the new window;
//! argument bindings follow below it in source order.

use mwcc_syntax_trees::{Expression, Function, LocalDeclaration, Statement, Type};

pub(super) struct StructuredUnoptimizedInlineFloatLoopHomes {
    arguments: Vec<String>,
    result: String,
}

impl StructuredUnoptimizedInlineFloatLoopHomes {
    pub(super) fn plan(
        function: &Function,
        ephemeral_locals: &[&LocalDeclaration],
    ) -> Option<Self> {
        let inline_float_locals: std::collections::HashSet<&str> = ephemeral_locals
            .iter()
            .filter(|local| {
                local.declared_type == Type::Float
                    && local.initializer.is_none()
                    && local.name.starts_with("__mwcc_inline_")
            })
            .map(|local| local.name.as_str())
            .collect();
        if inline_float_locals.len() < 2 {
            return None;
        }

        let mut plans = function.statements.iter().filter_map(|statement| {
            loop_store_assignment_sequence(statement, &inline_float_locals)
        });
        let plan = plans.next()?;
        if plans.next().is_some() {
            return None;
        }
        Some(plan)
    }

    pub(super) fn preference(&self, name: &str, existing_saved_count: u8) -> Option<u8> {
        let top = 31u8.checked_sub(existing_saved_count)?;
        if name == self.result {
            return Some(top);
        }
        self.arguments
            .iter()
            .position(|candidate| candidate == name)
            .and_then(|index| top.checked_sub(u8::try_from(index + 1).ok()?))
    }
}

fn loop_store_assignment_sequence(
    statement: &Statement,
    inline_float_locals: &std::collections::HashSet<&str>,
) -> Option<StructuredUnoptimizedInlineFloatLoopHomes> {
    let Statement::Loop { body, .. } = statement else {
        return None;
    };
    let [Statement::Store { value, .. }] = body.as_slice() else {
        return None;
    };

    let mut terms = Vec::new();
    flatten_comma(value, &mut terms);
    let Expression::Variable(returned) = terms.pop()? else {
        return None;
    };
    let assignments: Vec<_> = terms
        .into_iter()
        .map(assigned_variable)
        .collect::<Option<_>>()?;
    let (result, arguments) = assignments.split_last()?;
    if result != returned
        || arguments.is_empty()
        || assignments
            .iter()
            .any(|name| !inline_float_locals.contains(name.as_str()))
    {
        return None;
    }

    Some(StructuredUnoptimizedInlineFloatLoopHomes {
        arguments: arguments.to_vec(),
        result: result.clone(),
    })
}

fn flatten_comma<'a>(expression: &'a Expression, output: &mut Vec<&'a Expression>) {
    if let Expression::Comma { left, right } = expression {
        output.push(left);
        flatten_comma(right, output);
    } else {
        output.push(expression);
    }
}

fn assigned_variable(expression: &Expression) -> Option<String> {
    let Expression::Assign { target, .. } = expression else {
        return None;
    };
    let Expression::Variable(name) = target.as_ref() else {
        return None;
    };
    Some(name.clone())
}

#[cfg(test)]
mod tests {
    use super::StructuredUnoptimizedInlineFloatLoopHomes;
    use mwcc_syntax_trees::{Expression, Function, LocalDeclaration, LoopKind, Statement, Type};

    fn local(name: &str) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Float,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }
    }

    fn assign(name: &str, value: Expression) -> Expression {
        Expression::Assign {
            target: Box::new(Expression::Variable(name.into())),
            value: Box::new(value),
        }
    }

    fn comma(left: Expression, right: Expression) -> Expression {
        Expression::Comma {
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn places_the_inline_result_above_its_argument_bindings() {
        let names = [
            "__mwcc_inline_map_0_arg0",
            "__mwcc_inline_map_1_arg1",
            "__mwcc_inline_map_2_result",
        ];
        let locals: Vec<_> = names.iter().map(|name| local(name)).collect();
        let function = Function {
            return_type: Type::Void,
            name: "map".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: locals.clone(),
            statements: vec![Statement::Loop {
                kind: LoopKind::For,
                initializer: None,
                condition: Some(Expression::IntegerLiteral(1)),
                step: None,
                body: vec![Statement::Store {
                    target: Expression::Variable("output".into()),
                    value: comma(
                        assign(names[0], Expression::FloatLiteral(1.0)),
                        comma(
                            assign(names[1], Expression::FloatLiteral(2.0)),
                            comma(
                                assign(names[2], Expression::FloatLiteral(3.0)),
                                Expression::Variable(names[2].into()),
                            ),
                        ),
                    ),
                }],
            }],
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
        let ephemeral: Vec<_> = locals.iter().collect();

        let plan = StructuredUnoptimizedInlineFloatLoopHomes::plan(&function, &ephemeral)
            .expect("inlined comma loop should retain source-image homes");

        assert_eq!(plan.preference(names[2], 4), Some(27));
        assert_eq!(plan.preference(names[0], 4), Some(26));
        assert_eq!(plan.preference(names[1], 4), Some(25));
    }
}
