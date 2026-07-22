//! Lifetime-safe reuse of dead incoming-parameter homes by deferred locals.
//!
//! Structured frames initially reserve one saved home per cross-call parameter
//! and one per colored deferred-local group. MWCC colors both value classes in
//! one graph: after a parameter's final read, a later local definition may use
//! the same physical home. This plan composes those two independently proven
//! interval sets without coupling statement emission to source names.

use super::structured_locals::{structured_name_last_read, DeferredSavedHomePlan};
#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::Parameter;

pub(super) struct StructuredParameterHomeReuse {
    home_index_by_group: Vec<usize>,
    pub(super) fresh_group_count: usize,
}

impl StructuredParameterHomeReuse {
    pub(super) fn plan(
        function: &Function,
        eager_count: usize,
        saved_parameters: &[&Parameter],
        deferred: &DeferredSavedHomePlan,
        enabled: bool,
    ) -> Self {
        let mut reused_parameter_by_group = vec![None; deferred.group_count];
        if enabled {
            let mut parameters: Vec<_> = saved_parameters
                .iter()
                .enumerate()
                .filter(|(_, parameter)| {
                    function
                        .return_expression
                        .as_ref()
                        .is_none_or(|expression| {
                            !expression_reads_name(expression, &parameter.name)
                        })
                })
                .filter_map(|(index, parameter)| {
                    structured_name_last_read(function, &parameter.name)
                        .map(|last_read| (index, last_read))
                })
                .collect();
            parameters.sort_by_key(|(_, last_read)| std::cmp::Reverse(*last_read));

            for (parameter, last_read) in parameters {
                let reusable = (0..deferred.group_count)
                    .filter(|group| reused_parameter_by_group[*group].is_none())
                    .filter(|group| deferred.first_assignment(*group) > last_read)
                    .max_by_key(|group| deferred.first_assignment(*group));
                if let Some(group) = reusable {
                    reused_parameter_by_group[group] = Some(parameter);
                }
            }
        }

        let mut fresh_group_count = 0;
        let home_index_by_group = reused_parameter_by_group
            .into_iter()
            .map(|parameter| {
                if let Some(parameter) = parameter {
                    eager_count + parameter
                } else {
                    let home = eager_count + saved_parameters.len() + fresh_group_count;
                    fresh_group_count += 1;
                    home
                }
            })
            .collect();
        Self {
            home_index_by_group,
            fresh_group_count,
        }
    }

    pub(super) fn home_index(&self, group: usize) -> usize {
        self.home_index_by_group[group]
    }

    pub(super) fn reuses_parameter_home(&self, eager_count: usize, parameter_count: usize) -> bool {
        let fresh_home_base = eager_count + parameter_count;
        self.home_index_by_group
            .iter()
            .any(|home| *home < fresh_home_base)
    }
}

#[cfg(test)]
mod tests {
    use super::super::structured_locals::plan_deferred_saved_homes;
    use super::*;

    fn function(return_reads_parameter: bool) -> Function {
        Function {
            return_type: Type::Int,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Int,
                name: "incoming".into(),
            }],
            locals: vec![LocalDeclaration {
                declared_type: Type::Int,
                name: "late".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            }],
            statements: vec![
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::Variable("incoming".into())],
                }),
                Statement::Assign {
                    name: "late".into(),
                    value: Expression::IntegerLiteral(1),
                },
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::Variable("late".into())],
                }),
            ],
            guards: Vec::new(),
            return_expression: return_reads_parameter
                .then(|| Expression::Variable("incoming".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn reuses_a_parameter_home_after_its_final_read() {
        let function = function(false);
        let deferred = plan_deferred_saved_homes(&function, &[&function.locals[0]]).unwrap();
        let reuse = StructuredParameterHomeReuse::plan(
            &function,
            0,
            &[&function.parameters[0]],
            &deferred,
            true,
        );

        assert_eq!(reuse.fresh_group_count, 0);
        assert_eq!(reuse.home_index(deferred.group("late")), 0);
    }

    #[test]
    fn keeps_a_parameter_home_live_when_the_return_reads_it() {
        let function = function(true);
        let deferred = plan_deferred_saved_homes(&function, &[&function.locals[0]]).unwrap();
        let reuse = StructuredParameterHomeReuse::plan(
            &function,
            0,
            &[&function.parameters[0]],
            &deferred,
            true,
        );

        assert_eq!(reuse.fresh_group_count, 1);
        assert_eq!(reuse.home_index(deferred.group("late")), 1);
    }
}
