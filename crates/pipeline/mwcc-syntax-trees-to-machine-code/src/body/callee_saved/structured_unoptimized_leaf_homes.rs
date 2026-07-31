//! Source-home retention for unoptimized straight-line leaf value chains.

use super::*;

pub(super) struct StructuredUnoptimizedLeafHomes {
    general_names: Vec<String>,
    float_names: Vec<String>,
}

impl StructuredUnoptimizedLeafHomes {
    pub(super) fn plan(function: &Function) -> Option<Self> {
        let [parameter] = function.parameters.as_slice() else {
            return None;
        };
        if function_makes_call(function)
            || !function.guards.is_empty()
            || !matches!(
                parameter.parameter_type,
                Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort
            )
            || function.locals.len() < 2
            || function.locals.len() != function.statements.len()
            || function.locals.len() > 18
        {
            return None;
        }
        for (index, (local, statement)) in
            function.locals.iter().zip(&function.statements).enumerate()
        {
            if local.is_static
                || local.initializer.is_some()
                || local.array_length.is_some()
                || !matches!(
                    class_of(local.declared_type),
                    Ok(ValueClass::General | ValueClass::Float)
                )
                || !matches!(statement, Statement::Assign { name, .. } if name == &local.name)
            {
                return None;
            }
            let Statement::Assign { value, .. } = statement else {
                unreachable!("the assignment was matched above")
            };
            let dependency = if index == 0 {
                &parameter.name
            } else {
                &function.locals[index - 1].name
            };
            if !expression_reads_name(value, dependency) {
                return None;
            }
        }
        let returned = function.return_expression.as_ref()?;
        if !matches!(returned, Expression::Variable(name) if name == &function.locals.last()?.name)
        {
            return None;
        }
        let general_names: Vec<_> = function
            .locals
            .iter()
            .filter(|local| class_of(local.declared_type).ok() == Some(ValueClass::General))
            .map(|local| local.name.clone())
            .collect();
        let float_names: Vec<_> = function
            .locals
            .iter()
            .filter(|local| class_of(local.declared_type).ok() == Some(ValueClass::Float))
            .map(|local| local.name.clone())
            .collect();
        if general_names.is_empty() || float_names.is_empty() {
            return None;
        }
        Some(Self {
            general_names,
            float_names,
        })
    }

    pub(super) fn names(&self) -> impl Iterator<Item = &str> {
        self.general_names
            .iter()
            .chain(&self.float_names)
            .map(String::as_str)
    }

    pub(super) fn general_preference(
        &self,
        home_index: usize,
        eager_count: usize,
        parameter_count: usize,
        total_count: usize,
    ) -> Option<u8> {
        (eager_count == 0
            && parameter_count == 0
            && total_count == self.general_names.len()
            && home_index < self.general_names.len())
        .then(|| 31u8.saturating_sub(home_index as u8))
    }

    pub(super) fn float_preference(&self, name: &str) -> Option<u8> {
        self.float_names
            .iter()
            .position(|candidate| candidate == name)
            .map(|index| 31u8.saturating_sub(index as u8))
    }
}
