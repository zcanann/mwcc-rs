//! Unoptimized general-register homes recovered from decompilation local names.

use super::*;

pub(super) struct StructuredRecoveredGeneralHomes {
    names: Vec<String>,
    preferences: Vec<u8>,
    parameter_count: usize,
    save_order: Option<Vec<usize>>,
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
    pub(super) fn plan(function: &Function) -> Option<Self> {
        if !function.guards.is_empty()
            || !function_makes_call(function)
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
                    && (function.statements.iter().any(|statement| {
                        matches!(statement, Statement::Assign { name, .. } if name == &local.name)
                    }) || (recovered_register(&local.name).is_some()
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
    ) -> Option<u8> {
        (eager_count == 0
            && parameter_count == self.parameter_count
            && total_count == self.preferences.len())
            .then(|| self.preferences.get(home_index).copied())
            .flatten()
    }

    pub(super) fn save_order(&self) -> Option<&[usize]> {
        self.save_order.as_deref()
    }

    pub(super) fn frame_slot(&self, home_index: usize) -> Option<usize> {
        self.save_order()?
            .iter()
            .position(|candidate| *candidate == home_index)
    }
}

fn single_missing_register(registers: &[u8]) -> Option<u8> {
    let first = *registers.iter().min()?;
    let last = *registers.iter().max()?;
    let mut missing = (first..=last).filter(|register| !registers.contains(register));
    let register = missing.next()?;
    missing.next().is_none().then_some(register)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
