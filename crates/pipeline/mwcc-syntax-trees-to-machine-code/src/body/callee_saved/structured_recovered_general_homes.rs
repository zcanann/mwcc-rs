//! Unoptimized general-register homes recovered from decompilation local names.

use super::*;

pub(super) struct StructuredRecoveredGeneralHomes {
    names: Vec<String>,
    preferences: Vec<u8>,
    parameter_count: usize,
    save_order: Option<Vec<usize>>,
    preferences_follow_groups: bool,
    direct_paired_single_restores: bool,
}

fn recovered_register(name: &str) -> Option<u8> {
    let (_, suffix) = name.rsplit_once("_r")?;
    let register = suffix.parse::<u8>().ok()?;
    (14..=31).contains(&register).then_some(register)
}

impl StructuredRecoveredGeneralHomes {
    /// Decompilation from an unoptimized object retains one source home per
    /// assigned local even where lifetime analysis could coalesce or eliminate
    /// those homes. Activate only when a recovered `*_rN` name confirms the
    /// declaration-order descending window; unnamed neighbors occupy the
    /// preceding homes.
    pub(super) fn plan(
        function: &Function,
        inline_global_result_homes: &[String],
    ) -> Option<Self> {
        if !function.guards.is_empty()
            || function.statements.is_empty()
        {
            return None;
        }
        let names: Vec<_> = function
            .locals
            .iter()
            .filter(|local| {
                !local.is_static
                    && local.initializer.is_none()
                    && local.array_length.is_none()
                    && class_of(local.declared_type).ok() == Some(ValueClass::General)
                    && (body_assigns_local(&function.statements, &local.name)
                        || (recovered_register(&local.name).is_some()
                        && super::structured_locals::body_uses_local(
                            &function.statements,
                            &local.name,
                        )))
            })
            .map(|local| local.name.clone())
            .collect();
        if names.len() < 2 || names.len() > 18 {
            return None;
        }
        let recovered: Vec<_> = names
            .iter()
            .enumerate()
            .filter_map(|(index, name)| recovered_register(name).map(|register| (index, register)))
            .collect();
        if recovered.is_empty() {
            return None;
        }
        if !function_makes_call(function)
            && recovered_global_transaction_loop(
                function,
                &names,
                &recovered,
                inline_global_result_homes,
            )
        {
            let preferences = names
                .iter()
                .map(|name| {
                    recovered
                        .iter()
                        .find_map(|(index, register)| {
                            (&names[*index] == name).then_some(*register)
                        })
                        .or_else(|| {
                            inline_global_result_homes
                                .iter()
                                .position(|candidate| candidate == name)
                                .map(|index| 28u8.saturating_sub(index as u8))
                        })
                })
                .collect::<Option<Vec<_>>>()?;
            return Some(Self {
                names,
                preferences,
                parameter_count: 0,
                save_order: None,
                preferences_follow_groups: true,
                direct_paired_single_restores: false,
            });
        }
        if !function_makes_call(function) {
            return None;
        }
        let straight_assignments = function.statements.iter().all(|statement| {
            matches!(statement, Statement::Assign { name, .. } if names.contains(name))
        }) && function.return_expression.as_ref().is_some_and(|returned| {
            names
                .iter()
                .any(|name| expression_reads_name(returned, name))
        });
        if straight_assignments
            && recovered
                .iter()
                .all(|(index, register)| *register == 31u8.saturating_sub(*index as u8))
        {
            let preferences = (0..names.len())
                .map(|index| 31u8.saturating_sub(index as u8))
                .collect();
            return Some(Self {
                names,
                preferences,
                parameter_count: 0,
                save_order: None,
                preferences_follow_groups: false,
                direct_paired_single_restores: false,
            });
        }

        let has_loop = function
            .statements
            .iter()
            .any(|statement| matches!(statement, Statement::Loop { .. }));
        let calling_statements = function
            .statements
            .iter()
            .filter(|statement| statement_has_call(statement))
            .count();
        let terminal_call = function
            .statements
            .last()
            .is_some_and(|statement| statement_has_call(statement));
        let all_recovered = recovered.len() == names.len();
        let used_parameters: Vec<_> = function
            .parameters
            .iter()
            .filter(|parameter| {
                class_of(parameter.parameter_type).ok() == Some(ValueClass::General)
                    && super::structured_locals::body_uses_local(
                        &function.statements,
                        &parameter.name,
                    )
            })
            .collect();
        let recovered_registers: Vec<_> = recovered.iter().map(|(_, register)| *register).collect();
        let missing_window = sparse_window_parameter_homes(
            &recovered_registers,
            used_parameters.len(),
        );
        let nested_void_recovered_loop = function.return_type == Type::Void
            && !has_loop
            && names.iter().any(|name| {
                super::structured_locals::structured_name_occurs_in_loop(function, name)
            });
        let sparse_recovered_loop = ((function.return_type != Type::Void && has_loop)
            || nested_void_recovered_loop)
            && all_recovered
            && !used_parameters.is_empty()
            && missing_window.is_some()
            && names.iter().all(|name| {
                super::structured_locals::body_uses_local(&function.statements, name)
            });
        if sparse_recovered_loop {
            let missing_window = missing_window.expect("sparse loop window was checked");
            let parameter_homes: std::collections::HashMap<_, _> = used_parameters
                .iter()
                .zip(missing_window.iter().rev())
                .map(|(parameter, register)| (parameter.name.as_str(), *register))
                .collect();
            let mut deferred_names = names.clone();
            deferred_names.sort_by_key(|name| {
                super::structured_locals::structured_name_first_assignment(function, name)
                    .unwrap_or(usize::MAX)
            });
            if deferred_names.iter().any(|name| {
                super::structured_locals::structured_name_first_assignment(function, name)
                    .is_none()
            }) {
                return None;
            }
            let mut home_names: Vec<_> = used_parameters
                .iter()
                .rev()
                .map(|parameter| parameter.name.clone())
                .collect();
            home_names.extend(deferred_names.iter().cloned());
            let preferences = home_names
                .iter()
                .map(|name| {
                    parameter_homes.get(name.as_str()).copied().or_else(|| {
                        recovered
                            .iter()
                            .find_map(|(index, register)| {
                                (names[*index] == *name).then_some(*register)
                            })
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            return Some(Self {
                names: home_names,
                preferences,
                parameter_count: used_parameters.len(),
                save_order: None,
                preferences_follow_groups: false,
                direct_paired_single_restores: nested_void_recovered_loop,
            });
        }
        // Three or more recovered local names can describe an entire saved-GPR
        // window with one hole. When exactly one general parameter is also live,
        // that hole is direct evidence for its home even in a large mixed-loop
        // body: `var_r28`, `var_r30`, `var_r31` leaves r29 for the parameter.
        // Keep home indices in parameter/deferred-group order, while saving the
        // resolved physical window from high to low as MWCC does.
        if names.len() >= 3
            && all_recovered
            && used_parameters.len() == 1
            && names.iter().all(|name| {
                super::structured_locals::body_uses_local(&function.statements, name)
            })
        {
            if let Some(parameter_homes) =
                sparse_window_parameter_homes(&recovered_registers, 1)
            {
                let mut home_names = vec![used_parameters[0].name.clone()];
                home_names.extend(names.iter().cloned());
                let mut preferences = parameter_homes;
                preferences.extend(recovered_registers.iter().copied());
                let save_order = recovered_registers
                    .windows(2)
                    .all(|pair| pair[0] < pair[1])
                    .then(|| {
                        let mut order: Vec<_> = (0..preferences.len()).collect();
                        order.sort_by_key(|index| std::cmp::Reverse(preferences[*index]));
                        order
                    });
                return Some(Self {
                    names: home_names,
                    preferences,
                    parameter_count: 1,
                    save_order,
                    preferences_follow_groups: true,
                    direct_paired_single_restores: false,
                });
            }
        }
        let missing = single_missing_register(&recovered_registers);
        if function.return_type != Type::Void
            || !has_loop
            || calling_statements != 1
            || !terminal_call
            || !all_recovered
            || used_parameters.len() != 1
            || missing.is_none()
            || !names.iter().all(|name| {
                super::structured_locals::body_uses_local(&function.statements, name)
            })
        {
            return None;
        }
        let parameter = used_parameters[0];
        let mut survivor_names = names.clone();
        survivor_names.push(parameter.name.clone());
        let mut preferences = vec![missing.expect("checked above")];
        preferences.extend(recovered_registers.into_iter().rev());
        Some(Self {
            names: survivor_names,
            preferences,
            parameter_count: 1,
            save_order: Some(vec![1, 0, 2]),
            preferences_follow_groups: false,
            direct_paired_single_restores: false,
        })
    }

    pub(super) fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }

    pub(super) fn preference(
        &self,
        home_index: usize,
        eager_count: usize,
        parameter_count: usize,
        total_count: usize,
        deferred: &super::structured_locals::DeferredSavedHomePlan,
    ) -> Option<u8> {
        if eager_count != 0
            || parameter_count != self.parameter_count
            || total_count != self.preferences.len()
        {
            return None;
        }
        if self.preferences_follow_groups {
            if home_index < parameter_count {
                return self.preferences.get(home_index).copied();
            }
            let group = home_index.checked_sub(parameter_count)?;
            return deferred.members(group).find_map(|member| {
                self.names
                    .iter()
                    .position(|name| name == member)
                    .and_then(|index| self.preferences.get(index).copied())
            });
        }
        self.preferences.get(home_index).copied()
    }

    pub(super) fn save_order(&self) -> Option<&[usize]> {
        self.save_order.as_deref()
    }

    pub(super) fn source_order_parameter_copies(&self) -> bool {
        self.parameter_count >= 2
    }

    pub(super) fn direct_paired_single_restores(&self) -> bool {
        self.direct_paired_single_restores
    }

    pub(super) fn frame_slot(&self, home_index: usize) -> Option<usize> {
        self.save_order()?
            .iter()
            .position(|candidate| *candidate == home_index)
    }
}

impl Generator {
    /// Allocation uses the recovered physical homes as its tie-breakers. Once
    /// those homes are fixed, issue independent incoming copies in ABI source
    /// order without feeding that scheduling choice back into coloring.
    pub(crate) fn schedule_allocated_recovered_parameter_copies(&mut self) {
        if !self.structured_recovered_parameter_copies {
            return;
        }
        let Some(range) = recovered_parameter_copy_run(&self.output.instructions) else {
            return;
        };
        let old = self.output.instructions[range.clone()].to_vec();
        let mut order: Vec<_> = (range.clone()).collect();
        order.sort_by_key(|&index| {
            recovered_parameter_copy(&self.output.instructions[index])
                .expect("the recovered copy run was filtered as register moves")
                .1
        });
        let mut permutation: Vec<usize> = (0..self.output.instructions.len()).collect();
        for (new_index, old_index) in (range.clone()).zip(order) {
            self.output.instructions[new_index] = old[old_index - range.start].clone();
            permutation[old_index] = new_index;
        }
        crate::remap_instruction_indices(self, &permutation);
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }
}

fn recovered_parameter_copy_run(instructions: &[Instruction]) -> Option<std::ops::Range<usize>> {
    let mut start = None;
    for (index, instruction) in instructions.iter().enumerate() {
        if recovered_parameter_copy(instruction).is_some() {
            start.get_or_insert(index);
            continue;
        }
        if let Some(first) = start.take() {
            if index - first >= 2 {
                return Some(first..index);
            }
        }
    }
    start.and_then(|first| (instructions.len() - first >= 2).then_some(first..instructions.len()))
}

fn recovered_parameter_copy(instruction: &Instruction) -> Option<(u8, u8)> {
    match instruction {
        Instruction::Or { a, s, b }
            if a != s && s == b && (14..=31).contains(a) && (3..=10).contains(s) =>
        {
            Some((*a, *s))
        }
        _ => None,
    }
}

fn body_assigns_local(statements: &[Statement], local: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign { name, .. } => name == local,
        Statement::If {
            then_body,
            else_body,
            ..
        } => body_assigns_local(then_body, local) || body_assigns_local(else_body, local),
        Statement::Loop { body, .. } => body_assigns_local(body, local),
        Statement::Switch { arms, default, .. } => {
            arms.iter().any(|arm| match &arm.body {
                mwcc_syntax_trees::ArmBody::Statements(statements) => {
                    body_assigns_local(statements, local)
                }
                mwcc_syntax_trees::ArmBody::Return(_) => false,
            }) || default.as_ref().is_some_and(|body| match body {
                mwcc_syntax_trees::ArmBody::Statements(statements) => {
                    body_assigns_local(statements, local)
                }
                mwcc_syntax_trees::ArmBody::Return(_) => false,
            })
        }
        _ => false,
    })
}

fn recovered_global_transaction_loop(
    function: &Function,
    names: &[String],
    recovered: &[(usize, u8)],
    inline_global_result_homes: &[String],
) -> bool {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || recovered.len() + inline_global_result_homes.len() != names.len()
        || inline_global_result_homes.len() != 2
        || function
            .statements
            .iter()
            .filter(|statement| matches!(statement, Statement::Loop { .. }))
            .count()
            < 2
        || !names.iter().all(|name| {
            super::structured_locals::body_uses_local(&function.statements, name)
        })
    {
        return false;
    }
    let mut registers = recovered
        .iter()
        .map(|(_, register)| *register)
        .collect::<Vec<_>>();
    registers.sort_unstable();
    registers.dedup();
    if registers.len() != recovered.len()
        || registers.last().copied() != Some(31)
        || registers.first().copied() != Some(29)
        || registers.windows(2).any(|pair| pair[1] != pair[0] + 1)
    {
        return false;
    }
    let mut stored_globals = Vec::new();
    collect_scalar_global_stores(&function.statements, names, &mut stored_globals);
    matches!(stored_globals.as_slice(), [first, second, ..] if first == second)
}

fn collect_scalar_global_stores(
    statements: &[Statement],
    locals: &[String],
    output: &mut Vec<String>,
) {
    for statement in statements {
        match statement {
            Statement::Store {
                target: Expression::Variable(name),
                ..
            } if !locals.contains(name) => output.push(name.clone()),
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                collect_scalar_global_stores(then_body, locals, output);
                collect_scalar_global_stores(else_body, locals, output);
            }
            Statement::Loop { body, .. } => {
                collect_scalar_global_stores(body, locals, output);
            }
            _ => {}
        }
    }
}

fn single_missing_register(registers: &[u8]) -> Option<u8> {
    let first = *registers.iter().min()?;
    let last = *registers.iter().max()?;
    let mut missing = (first..=last).filter(|register| !registers.contains(register));
    let register = missing.next()?;
    missing.next().is_none().then_some(register)
}

fn sparse_window_parameter_homes(
    recovered_registers: &[u8],
    parameter_count: usize,
) -> Option<Vec<u8>> {
    let window_size = recovered_registers.len().checked_add(parameter_count)?;
    let first = 32u8.checked_sub(u8::try_from(window_size).ok()?)?;
    let window: Vec<_> = (first..=31).collect();
    if recovered_registers
        .iter()
        .any(|register| !window.contains(register))
    {
        return None;
    }
    let missing: Vec<_> = window
        .into_iter()
        .filter(|register| !recovered_registers.contains(register))
        .collect();
    (missing.len() == parameter_count).then_some(missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Int,
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

    #[test]
    fn parses_only_saved_general_register_suffixes() {
        assert_eq!(recovered_register("var_r30"), Some(30));
        assert_eq!(recovered_register("temp_r14"), Some(14));
        assert_eq!(recovered_register("data"), None);
        assert_eq!(recovered_register("var_r3"), None);
    }

    #[test]
    fn finds_one_parameter_home_between_recovered_locals() {
        assert_eq!(single_missing_register(&[29, 31]), Some(30));
        assert_eq!(single_missing_register(&[28, 31]), None);
        assert_eq!(single_missing_register(&[29, 30]), None);
    }

    #[test]
    fn finds_all_parameter_holes_in_a_sparse_saved_window() {
        assert_eq!(
            sparse_window_parameter_homes(&[27, 29, 31], 2),
            Some(vec![28, 30])
        );
        assert_eq!(
            sparse_window_parameter_homes(&[28, 27, 29, 30, 31], 1),
            Some(vec![26])
        );
        assert_eq!(sparse_window_parameter_homes(&[27, 31], 2), None);
        assert_eq!(sparse_window_parameter_homes(&[26, 29, 31], 2), None);
    }

    #[test]
    fn recovers_the_parameter_hole_in_a_four_home_window() {
        let function = Function {
            return_type: Type::Void,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![mwcc_syntax_trees::Parameter {
                parameter_type: Type::Pointer(mwcc_syntax_trees::Pointee::Int),
                name: "object".into(),
            }],
            locals: vec![local("var_r28"), local("var_r30"), local("var_r31")],
            statements: vec![
                Statement::Assign {
                    name: "var_r28".into(),
                    value: Expression::Call {
                        name: "create".into(),
                        arguments: Vec::new(),
                    },
                },
                Statement::Assign {
                    name: "var_r30".into(),
                    value: Expression::IntegerLiteral(0),
                },
                Statement::Assign {
                    name: "var_r31".into(),
                    value: Expression::IntegerLiteral(0),
                },
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![
                        Expression::Variable("object".into()),
                        Expression::Variable("var_r28".into()),
                        Expression::Variable("var_r30".into()),
                        Expression::Variable("var_r31".into()),
                    ],
                }),
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

        let plan = StructuredRecoveredGeneralHomes::plan(&function, &[])
            .expect("the recovered window should resolve the parameter hole");
        assert_eq!(plan.names, ["object", "var_r28", "var_r30", "var_r31"]);
        assert_eq!(plan.preferences, [29, 28, 30, 31]);
        assert_eq!(plan.parameter_count, 1);
        assert_eq!(plan.save_order(), Some([3, 2, 0, 1].as_slice()));
    }

    #[test]
    fn finds_the_recovered_entry_copy_packet() {
        let instructions = [
            Instruction::BranchAndLink {
                target: "callee".into(),
            },
            Instruction::move_register(28, 4),
            Instruction::move_register(30, 3),
            Instruction::AddImmediate {
                d: 27,
                a: 0,
                immediate: -1,
            },
        ];
        assert_eq!(recovered_parameter_copy_run(&instructions), Some(1..3));
    }
}
