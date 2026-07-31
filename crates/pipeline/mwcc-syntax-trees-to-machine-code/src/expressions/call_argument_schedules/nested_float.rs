//! Mixed-ABI argument schedules with a nested call in the first FPR slot.

use super::*;

fn call_bearing_first_float(arguments: &[Expression], parameter_types: &[Type]) -> Option<usize> {
    if arguments.len() != parameter_types.len() {
        return None;
    }
    let first_float = parameter_types
        .iter()
        .position(|ty| matches!(ty, Type::Float | Type::Double))?;
    if first_float == 0
        || parameter_types[..first_float]
            .iter()
            .any(|ty| matches!(ty, Type::Float | Type::Double))
        || parameter_types[first_float..]
            .iter()
            .any(|ty| !matches!(ty, Type::Float | Type::Double))
        || !expression_has_call(&arguments[first_float])
        || arguments
            .iter()
            .enumerate()
            .any(|(index, argument)| index != first_float && expression_has_call(argument))
    {
        return None;
    }
    Some(first_float)
}

impl Generator {
    /// Marshal a general prefix, then a float-only suffix whose first value
    /// contains the sole nested call.
    ///
    /// The nested call is evaluated first into f1. Every prefix and remaining
    /// suffix expression is reloadable from constants, memory, or nonvolatile
    /// homes, so the general arguments can be reconstructed in r3.. and the
    /// later floating arguments formed in f2.. afterward.
    pub(crate) fn try_emit_call_bearing_first_float_with_reloadable_suffix_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<bool> {
        let direct_call = !self.globals.contains_key(name)
            && !self.locations.contains_key(name)
            && !self.known_locals.contains(name);
        let Some(parameter_types) = self.call_parameter_types.get(name).cloned() else {
            return Ok(false);
        };
        let Some(first_float) = call_bearing_first_float(arguments, &parameter_types) else {
            return Ok(false);
        };
        let reloadable = |generator: &Self, expression: &Expression| {
            generator
                .registers_used_by(expression)
                .into_iter()
                .all(|register| !matches!(register, 0 | 3..=12))
        };
        if !direct_call
            || !arguments[..first_float]
                .iter()
                .all(|argument| reloadable(self, argument))
            || !arguments[first_float + 1..]
                .iter()
                .all(|argument| reloadable(self, argument) && self.is_float_value(argument))
        {
            return Ok(false);
        }

        let call_bearing = &arguments[first_float];
        let call_bearing_type = parameter_types[first_float];
        let call_bearing_result = Eabi::FIRST_FLOAT_ARGUMENT;
        let call_bearing_evaluation =
            if call_bearing_type == Type::Float && self.is_double_value(call_bearing) {
                self.evaluate_float(call_bearing, FLOAT_SCRATCH).map(|()| {
                    self.output.instructions.push(Instruction::RoundToSingle {
                        d: call_bearing_result,
                        b: FLOAT_SCRATCH,
                    });
                })
            } else {
                self.evaluate(call_bearing, call_bearing_type, call_bearing_result)
            };
        call_bearing_evaluation.map_err(|mut diagnostic| {
            diagnostic.message.push_str(&format!(
                " (while scheduling the call-bearing first float argument to '{name}')"
            ));
            diagnostic
        })?;

        for (index, argument) in arguments[..first_float].iter().enumerate() {
            self.evaluate(
                argument,
                parameter_types[index],
                Eabi::FIRST_GENERAL_ARGUMENT + index as u8,
            )?;
        }
        for (index, argument) in arguments[first_float + 1..].iter().enumerate() {
            let source_index = first_float + 1 + index;
            self.evaluate(
                argument,
                parameter_types[source_index],
                Eabi::FIRST_FLOAT_ARGUMENT + 1 + index as u8,
            )?;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::call_bearing_first_float;
    use mwcc_syntax_trees::{Expression, Type};

    #[test]
    fn finds_the_only_nested_call_at_the_float_suffix_boundary() {
        let arguments = vec![
            Expression::Variable("model".into()),
            Expression::Call {
                name: "sin".into(),
                arguments: vec![Expression::Variable("angle".into())],
            },
            Expression::Variable("scale".into()),
        ];
        assert_eq!(
            call_bearing_first_float(&arguments, &[Type::Int, Type::Float, Type::Float]),
            Some(1)
        );
        assert_eq!(
            call_bearing_first_float(&arguments, &[Type::Float, Type::Float, Type::Float]),
            None
        );
    }
}
