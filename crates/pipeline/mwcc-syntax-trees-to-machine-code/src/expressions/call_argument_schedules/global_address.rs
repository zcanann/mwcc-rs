//! Schedules that mix scalar globals with file-scope object addresses.

use super::*;

fn global_pointer_address_constant(
    arguments: &[Expression],
) -> Option<(usize, &str, usize, &str, i16)> {
    let (pointer_index, pointer, address_index, addressed, value) = match arguments {
        [Expression::Variable(pointer), Expression::AddressOf { operand }, Expression::IntegerLiteral(value)] => {
            (0, pointer, 1, operand.as_ref(), value)
        }
        [Expression::AddressOf { operand }, Expression::Variable(pointer), Expression::IntegerLiteral(value)] => {
            (1, pointer, 0, operand.as_ref(), value)
        }
        _ => return None,
    };
    let Expression::Variable(addressed) = addressed else {
        return None;
    };
    let value = i16::try_from(*value).ok()?;
    Some((pointer_index, pointer, address_index, addressed, value))
}

impl Generator {
    /// Marshal a scalar global pointer, a file-scope object address, and an i16.
    ///
    /// Dolphin's disk-ID copy and compare calls establish the stable order:
    /// load the scalar pointer first, form the independent object address next,
    /// then materialize the size. The pointer load leads even when it belongs to
    /// the second argument slot.
    pub(crate) fn try_emit_global_pointer_address_constant_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
        direct_call: bool,
    ) -> Compilation<bool> {
        let Some((pointer_index, pointer, address_index, addressed, value)) =
            global_pointer_address_constant(arguments)
        else {
            return Ok(false);
        };
        let pointer_is_scalar = matches!(
            self.globals.get(pointer),
            Some(Type::Pointer(_) | Type::StructPointer { .. })
        );
        let all_general = self.call_parameter_types.get(name).is_none_or(|types| {
            types.len() >= 3
                && types[..3]
                    .iter()
                    .all(|ty| !matches!(ty, Type::Float | Type::Double))
        });
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || !pointer_is_scalar
            || !self.addressable_globals.contains_key(addressed)
            || !all_general
        {
            return Ok(false);
        }

        self.evaluate_general(
            &arguments[pointer_index],
            Eabi::FIRST_GENERAL_ARGUMENT + pointer_index as u8,
        )?;
        self.evaluate_general(
            &arguments[address_index],
            Eabi::FIRST_GENERAL_ARGUMENT + address_index as u8,
        )?;
        self.evaluate_general(
            &Expression::IntegerLiteral(i64::from(value)),
            Eabi::FIRST_GENERAL_ARGUMENT + 2,
        )?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_pointer_first_and_address_first_forms() {
        let pointer_first = vec![
            Expression::Variable("destination".into()),
            Expression::AddressOf {
                operand: Box::new(Expression::Variable("object".into())),
            },
            Expression::IntegerLiteral(32),
        ];
        let address_first = vec![
            pointer_first[1].clone(),
            pointer_first[0].clone(),
            pointer_first[2].clone(),
        ];

        assert_eq!(
            global_pointer_address_constant(&pointer_first),
            Some((0, "destination", 1, "object", 32))
        );
        assert_eq!(
            global_pointer_address_constant(&address_first),
            Some((1, "destination", 0, "object", 32))
        );
    }

    #[test]
    fn rejects_an_out_of_range_literal() {
        let arguments = vec![
            Expression::Variable("destination".into()),
            Expression::AddressOf {
                operand: Box::new(Expression::Variable("object".into())),
            },
            Expression::IntegerLiteral(i64::from(i16::MAX) + 1),
        ];

        assert_eq!(global_pointer_address_constant(&arguments), None);
    }
}
