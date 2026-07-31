//! Unoptimized general-register homes recovered from decompilation local names.

use super::*;

pub(super) struct StructuredRecoveredGeneralHomes {
    names: Vec<String>,
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
                    && function.statements.iter().any(|statement| {
                        matches!(statement, Statement::Assign { name, .. } if name == &local.name)
                    })
            })
            .map(|local| local.name.clone())
            .collect();
        if names.len() < 2
            || names.len() > 18
            || !function.statements.iter().all(|statement| {
                matches!(statement, Statement::Assign { name, .. } if names.contains(name))
            })
            || function.return_expression.as_ref().is_none_or(|returned| {
                !names
                    .iter()
                    .any(|name| expression_reads_name(returned, name))
            })
        {
            return None;
        }
        let recovered: Vec<_> = names
            .iter()
            .enumerate()
            .filter_map(|(index, name)| recovered_register(name).map(|register| (index, register)))
            .collect();
        if recovered.is_empty()
            || recovered
                .iter()
                .any(|(index, register)| *register != 31u8.saturating_sub(*index as u8))
        {
            return None;
        }
        Some(Self { names })
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
            && parameter_count == 0
            && total_count == self.names.len()
            && home_index < self.names.len())
        .then(|| 31u8.saturating_sub(home_index as u8))
    }
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
}
