//! Dependency ordering for pure general-register argument transactions.

use super::*;

fn pure_general_argument(expression: &Expression) -> bool {
    match expression {
        Expression::Variable(_) | Expression::IntegerLiteral(_) => true,
        Expression::Cast { operand, .. } | Expression::AddressOf { operand } => {
            pure_general_argument(operand)
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            pure_general_argument(base)
        }
        Expression::Binary { left, right, .. } => {
            pure_general_argument(left) && pure_general_argument(right)
        }
        _ => false,
    }
}

fn dependency_order(
    uses: &[std::collections::HashSet<u8>],
    destinations: &[u8],
    passthrough: &[bool],
) -> Option<Vec<usize>> {
    let source_order_is_unsafe = destinations.iter().enumerate().any(|(index, destination)| {
        !passthrough[index]
            && uses[index + 1..]
                .iter()
                .any(|later| later.contains(destination))
    });
    if !source_order_is_unsafe {
        return None;
    }

    let mut remaining: Vec<usize> = (0..destinations.len()).collect();
    let mut order = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let ready = remaining.iter().rposition(|&candidate| {
            passthrough[candidate]
                || remaining.iter().all(|&other| {
                    other == candidate || !uses[other].contains(&destinations[candidate])
                })
        })?;
        order.push(remaining.remove(ready));
    }
    Some(order)
}

impl Generator {
    /// Marshal a two-register permutation through the first free ABI argument
    /// register.
    ///
    /// A topological argument order cannot resolve `callee(r4, r3)`: writing
    /// either destination destroys the other source. Build 163 preserves the
    /// second source with `mr r5,r3`, then treats both legs of the swap as
    /// ordinary value materializations. Keeping this beside the acyclic
    /// dependency scheduler makes the distinction explicit instead of
    /// extending the generic one-way endangered-argument shortcut.
    pub(crate) fn try_emit_two_leaf_general_argument_swap(
        &mut self,
        arguments: &[Expression],
        name: &str,
        direct_call: bool,
    ) -> Compilation<bool> {
        let [first, second] = arguments else {
            return Ok(false);
        };
        if !direct_call
            || !self.call_parameter_types.get(name).is_some_and(|types| {
                types.len() >= 2
                    && types[..2].iter().all(|ty| {
                        ty.width() == 32
                            && !matches!(ty, Type::Float | Type::Double | Type::Struct { .. })
                    })
            })
        {
            return Ok(false);
        }

        let Ok((first_source, first_width, _)) = self.leaf_info(first) else {
            return Ok(false);
        };
        let Ok((second_source, second_width, _)) = self.leaf_info(second) else {
            return Ok(false);
        };
        let first_argument = Eabi::FIRST_GENERAL_ARGUMENT;
        let second_argument = first_argument + 1;
        if first_width != 32
            || second_width != 32
            || first_source != second_argument
            || second_source != first_argument
        {
            return Ok(false);
        }

        let scratch = second_argument + 1;
        self.output
            .instructions
            .push(Instruction::move_register(scratch, second_source));
        self.emit_integer_materialization_copy(first_argument, first_source);
        self.emit_integer_materialization_copy(second_argument, scratch);
        Ok(true)
    }

    /// Topologically marshal a pure, word-sized general argument list.
    ///
    /// Each expression may overwrite only its own ABI destination and r0. When
    /// a later expression still reads that destination, emit the later value
    /// first and reserve every completed ABI slot while its predecessors are
    /// evaluated. Cycles and richer argument classes remain with their focused
    /// schedulers.
    pub(crate) fn try_emit_dependency_ordered_general_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
        direct_call: bool,
    ) -> Compilation<bool> {
        if !direct_call
            || arguments.len() < 2
            || arguments.len()
                > usize::from(Eabi::LAST_GENERAL_ARGUMENT - Eabi::FIRST_GENERAL_ARGUMENT + 1)
            || !arguments.iter().all(pure_general_argument)
            || !self.call_parameter_types.get(name).is_some_and(|types| {
                types.len() >= arguments.len()
                    && types[..arguments.len()].iter().all(|ty| {
                        ty.width() == 32
                            && !matches!(ty, Type::Float | Type::Double | Type::Struct { .. })
                    })
            })
        {
            return Ok(false);
        }

        let destinations: Vec<u8> = (0..arguments.len())
            .map(|index| Eabi::FIRST_GENERAL_ARGUMENT + index as u8)
            .collect();
        let uses: Vec<_> = arguments
            .iter()
            .map(|argument| self.registers_used_by(argument))
            .collect();
        let passthrough: Vec<bool> = arguments
            .iter()
            .zip(&destinations)
            .map(|(argument, destination)| {
                self.leaf_info(argument)
                    .is_ok_and(|(source, width, _)| source == *destination && width == 32)
            })
            .collect();
        let Some(order) = dependency_order(&uses, &destinations, &passthrough) else {
            return Ok(false);
        };

        let mut completed = Vec::with_capacity(arguments.len());
        for index in order {
            let newly_reserved: Vec<u8> = completed
                .iter()
                .copied()
                .filter(|register| self.reserved.insert(*register))
                .collect();
            let result = self.evaluate_general(&arguments[index], destinations[index]);
            for register in newly_reserved {
                self.reserved.remove(&register);
            }
            result?;
            completed.push(destinations[index]);
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_a_shared_first_argument_base_from_right_to_left() {
        let uses = vec![
            std::collections::HashSet::from([3]),
            std::collections::HashSet::from([3]),
            std::collections::HashSet::from([3]),
            std::collections::HashSet::new(),
        ];

        assert_eq!(
            dependency_order(&uses, &[3, 4, 5, 6], &[false; 4]),
            Some(vec![3, 2, 1, 0])
        );
    }

    #[test]
    fn leaves_an_already_safe_source_order_to_the_generic_marshaler() {
        let uses = vec![
            std::collections::HashSet::from([3]),
            std::collections::HashSet::from([4]),
        ];

        assert_eq!(dependency_order(&uses, &[3, 4], &[true, true]), None);
    }
}
