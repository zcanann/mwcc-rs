//! Saved-GPR roles for linkage-first functions with several variadic loop phases.
//!
//! Optimized MWCC colors the shared loop index, typed member-array cursors, an
//! incoming context, and later state values by their role in the phase graph.
//! Definition order alone gives a legal coloring but rotates those roles through
//! different saved registers. This owner recognizes the complete phase shape and
//! supplies only home preferences; ordinary liveness still proves every reuse.

use super::structured_locals::DeferredSavedHomePlan;
use super::structured_member_array_call_cursor::CURSOR_PREFIX;
use super::structured_parameter_home_reuse::StructuredParameterHomeReuse;
#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::{ArmBody, Parameter, Pointee};
use mwcc_versions::FrameConvention;

pub(super) struct StructuredMultiPhaseVariadicHomeLayout {
    preference_by_home: [u8; 6],
}

impl StructuredMultiPhaseVariadicHomeLayout {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn plan(
        function: &Function,
        frame_convention: FrameConvention,
        with_frame_array: bool,
        has_standalone_data_anchor: bool,
        variadic_callees: &std::collections::HashSet<String>,
        eager_locals: &[&LocalDeclaration],
        saved_parameters: &[&Parameter],
        deferred_locals: &[&LocalDeclaration],
        deferred: &DeferredSavedHomePlan,
        parameter_reuse: &StructuredParameterHomeReuse,
        home_count: usize,
    ) -> Option<Self> {
        let [context] = saved_parameters else {
            return None;
        };
        if frame_convention != FrameConvention::LinkageFirst
            || !with_frame_array
            || !has_standalone_data_anchor
            || !matches!(context.parameter_type, Type::StructPointer { .. })
            || !eager_locals.is_empty()
            || deferred_locals.len() != 6
            || deferred.group_count != 5
            || parameter_reuse.fresh_group_count != 5
            || home_count != 6
        {
            return None;
        }

        let word_cursor = unique_cursor(deferred_locals, Pointee::UnsignedInt)?;
        let double_cursor = unique_cursor(deferred_locals, Pointee::Double)?;
        let index_candidates = deferred_locals
            .iter()
            .filter(|local| !local.name.starts_with(CURSOR_PREFIX))
            .filter(|local| matches!(local.declared_type, Type::Int | Type::UnsignedInt))
            .filter(|local| {
                variadic_loop_count(function, &local.name, variadic_callees, None) >= 4
            })
            .copied()
            .collect::<Vec<_>>();
        let [index] = index_candidates.as_slice() else {
            return None;
        };
        if variadic_loop_count(
            function,
            &index.name,
            variadic_callees,
            Some(&word_cursor.name),
        ) < 2
            || variadic_loop_count(
                function,
                &index.name,
                variadic_callees,
                Some(&double_cursor.name),
            ) < 2
        {
            return None;
        }

        let index_group = deferred.group_if_present(&index.name)?;
        let word_cursor_group = deferred.group_if_present(&word_cursor.name)?;
        let double_cursor_group = deferred.group_if_present(&double_cursor.name)?;
        let role_groups = [index_group, word_cursor_group, double_cursor_group];
        if index_group == word_cursor_group
            || index_group == double_cursor_group
            || word_cursor_group == double_cursor_group
        {
            return None;
        }
        let mut state_groups = (0..deferred.group_count)
            .filter(|group| !role_groups.contains(group))
            .collect::<Vec<_>>();
        if state_groups.len() != 2 {
            return None;
        }
        state_groups.sort_by_key(|group| deferred.first_assignment(*group));

        let mut preference_by_home = [0; 6];
        let mut occupied = [false; 6];
        let mut set = |home: usize, preference: u8| {
            if home >= preference_by_home.len() || occupied[home] {
                return false;
            }
            occupied[home] = true;
            preference_by_home[home] = preference;
            true
        };
        let home = |group| parameter_reuse.home_index(group);
        if !set(0, 28)
            || !set(home(index_group), 25)
            || !set(home(word_cursor_group), 27)
            || !set(home(double_cursor_group), 26)
            || !set(home(state_groups[0]), 29)
            || !set(home(state_groups[1]), 30)
            || occupied.iter().any(|occupied| !occupied)
        {
            return None;
        }
        Some(Self { preference_by_home })
    }

    pub(super) fn preference(&self, home_index: usize) -> Option<u8> {
        self.preference_by_home.get(home_index).copied()
    }
}

fn unique_cursor<'a>(
    locals: &'a [&LocalDeclaration],
    pointee: Pointee,
) -> Option<&'a LocalDeclaration> {
    let candidates = locals
        .iter()
        .filter(|local| {
            local.name.starts_with(CURSOR_PREFIX)
                && local.declared_type == Type::Pointer(pointee)
        })
        .copied()
        .collect::<Vec<_>>();
    let [cursor] = candidates.as_slice() else {
        return None;
    };
    Some(cursor)
}

fn variadic_loop_count(
    function: &Function,
    index: &str,
    variadic_callees: &std::collections::HashSet<String>,
    required_value: Option<&str>,
) -> usize {
    fn visit(
        statements: &[Statement],
        index: &str,
        variadic_callees: &std::collections::HashSet<String>,
        required_value: Option<&str>,
    ) -> usize {
        statements
            .iter()
            .map(|statement| match statement {
                Statement::Loop {
                    initializer: Some(initializer),
                    condition: Some(condition),
                    step: Some(step),
                    body,
                    ..
                } => {
                    let direct_variadic_call = match body.as_slice() {
                        [Statement::Expression(Expression::Call { name, arguments })]
                            if variadic_callees.contains(name)
                                && required_value.is_none_or(|required| {
                                    arguments.iter().any(|argument| {
                                        crate::analysis::expression_reads_name(argument, required)
                                    })
                                }) => true,
                        _ => false,
                    };
                    usize::from(
                        direct_variadic_call
                            && expression_assigns_name(initializer, index)
                            && crate::analysis::expression_reads_name(condition, index)
                            && expression_assigns_name(step, index),
                    ) + visit(body, index, variadic_callees, required_value)
                }
                Statement::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    visit(then_body, index, variadic_callees, required_value)
                        + visit(else_body, index, variadic_callees, required_value)
                }
                Statement::Switch { arms, default, .. } => {
                    arms.iter()
                        .map(|arm| match &arm.body {
                            ArmBody::Statements(body) => {
                                visit(body, index, variadic_callees, required_value)
                            }
                            ArmBody::Return(_) => 0,
                        })
                        .sum::<usize>()
                        + default.as_ref().map_or(0, |body| match body {
                            ArmBody::Statements(body) => {
                                visit(body, index, variadic_callees, required_value)
                            }
                            ArmBody::Return(_) => 0,
                        })
                }
                _ => 0,
            })
            .sum()
    }
    visit(
        &function.statements,
        index,
        variadic_callees,
        required_value,
    )
}

fn expression_assigns_name(expression: &Expression, name: &str) -> bool {
    match expression {
        Expression::Assign { target, value } => {
            matches!(target.as_ref(), Expression::Variable(target) if target == name)
                || expression_assigns_name(value, name)
        }
        Expression::Comma { left, right } => {
            expression_assigns_name(left, name) || expression_assigns_name(right, name)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::callee_saved::structured_eager_home_reuse::StructuredEagerHomeReuse;
    use crate::body::callee_saved::structured_locals::plan_deferred_saved_homes;
    use mwcc_syntax_trees::{BinaryOperator, LoopKind};

    #[test]
    fn assigns_saved_homes_by_variadic_phase_role() {
        let function = multi_phase_function();
        let deferred_locals = function.locals.iter().collect::<Vec<_>>();
        let deferred = plan_deferred_saved_homes(&function, &deferred_locals)
            .expect("deferred saved homes");
        assert_eq!(deferred.group_count, 5);
        let eager_reuse = StructuredEagerHomeReuse::plan(&function, &[], &deferred);
        let saved_parameters = vec![&function.parameters[0]];
        let parameter_reuse = StructuredParameterHomeReuse::plan(
            &function,
            0,
            &saved_parameters,
            &deferred,
            &eager_reuse,
        );
        let variadic_callees = std::collections::HashSet::from(["report".to_string()]);
        let layout = StructuredMultiPhaseVariadicHomeLayout::plan(
            &function,
            FrameConvention::LinkageFirst,
            true,
            true,
            &variadic_callees,
            &[],
            &saved_parameters,
            &deferred_locals,
            &deferred,
            &parameter_reuse,
            6,
        )
        .expect("multi-phase variadic layout");

        let preference = |name: &str| {
            layout.preference(parameter_reuse.home_index(deferred.group(name)))
        };
        assert_eq!(layout.preference(0), Some(28));
        assert_eq!(preference("i"), Some(25));
        assert_eq!(preference("__mwcc_member_array_cursor_0"), Some(27));
        assert_eq!(preference("__mwcc_member_array_cursor_1"), Some(26));
        assert_eq!(preference("enabled"), Some(29));
        assert_eq!(preference("current"), Some(30));
    }

    #[test]
    fn rejects_the_phase_shape_without_a_standalone_data_anchor() {
        let function = multi_phase_function();
        let deferred_locals = function.locals.iter().collect::<Vec<_>>();
        let deferred = plan_deferred_saved_homes(&function, &deferred_locals)
            .expect("deferred saved homes");
        let eager_reuse = StructuredEagerHomeReuse::plan(&function, &[], &deferred);
        let saved_parameters = vec![&function.parameters[0]];
        let parameter_reuse = StructuredParameterHomeReuse::plan(
            &function,
            0,
            &saved_parameters,
            &deferred,
            &eager_reuse,
        );
        let variadic_callees = std::collections::HashSet::from(["report".to_string()]);
        assert!(StructuredMultiPhaseVariadicHomeLayout::plan(
            &function,
            FrameConvention::LinkageFirst,
            true,
            false,
            &variadic_callees,
            &[],
            &saved_parameters,
            &deferred_locals,
            &deferred,
            &parameter_reuse,
            6,
        )
        .is_none());
    }

    fn multi_phase_function() -> Function {
        let word_cursor = "__mwcc_member_array_cursor_0";
        let double_cursor = "__mwcc_member_array_cursor_1";
        let mut statements = vec![
            variadic_loop(word_cursor),
            variadic_loop(word_cursor),
            variadic_loop(double_cursor),
            variadic_loop(double_cursor),
            Statement::Assign {
                name: "enabled".to_string(),
                value: Expression::Call {
                    name: "disable".to_string(),
                    arguments: Vec::new(),
                },
            },
            Statement::Assign {
                name: "current".to_string(),
                value: Expression::Variable("context".to_string()),
            },
            Statement::Expression(Expression::Call {
                name: "consume_state".to_string(),
                arguments: vec![
                    Expression::Variable("enabled".to_string()),
                    Expression::Variable("current".to_string()),
                ],
            }),
            Statement::Assign {
                name: "p".to_string(),
                value: Expression::Variable("context".to_string()),
            },
        ];
        statements.push(variadic_loop("p"));
        Function {
            return_type: Type::Void,
            name: "multi_phase".to_string(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::StructPointer { element_size: 64 },
                name: "context".to_string(),
            }],
            locals: vec![
                local("i", Type::UnsignedInt),
                local("p", Type::Pointer(Pointee::UnsignedInt)),
                local("current", Type::StructPointer { element_size: 64 }),
                local("enabled", Type::Int),
                local(word_cursor, Type::Pointer(Pointee::UnsignedInt)),
                local(double_cursor, Type::Pointer(Pointee::Double)),
            ],
            statements,
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: true,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    fn local(name: &str, declared_type: Type) -> LocalDeclaration {
        LocalDeclaration {
            declared_type,
            name: name.to_string(),
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

    fn variadic_loop(value: &str) -> Statement {
        Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Comma {
                left: Box::new(assign("i", Expression::IntegerLiteral(0))),
                right: Box::new(assign(
                    value,
                    Expression::Variable("context".to_string()),
                )),
            }),
            condition: Some(Expression::Binary {
                operator: BinaryOperator::Less,
                left: Box::new(Expression::Variable("i".to_string())),
                right: Box::new(Expression::IntegerLiteral(4)),
            }),
            step: Some(Expression::Comma {
                left: Box::new(assign(
                    "i",
                    Expression::Binary {
                        operator: BinaryOperator::Add,
                        left: Box::new(Expression::Variable("i".to_string())),
                        right: Box::new(Expression::IntegerLiteral(1)),
                    },
                )),
                right: Box::new(assign(
                    value,
                    Expression::Variable(value.to_string()),
                )),
            }),
            body: vec![Statement::Expression(Expression::Call {
                name: "report".to_string(),
                arguments: vec![
                    Expression::StringLiteral(vec![b'%', b'd']),
                    Expression::Variable("i".to_string()),
                    Expression::Variable(value.to_string()),
                ],
            })],
        }
    }

    fn assign(name: &str, value: Expression) -> Expression {
        Expression::Assign {
            target: Box::new(Expression::Variable(name.to_string())),
            value: Box::new(value),
        }
    }
}
