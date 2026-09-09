//! Argument placement as parallel assignments to the ABI registers.
//!
//! A destination may still be an input to another argument. Schedule that
//! consumer first; break cycles by saving a register-leaf argument value.
//! Expression evaluation remains with the ordinary shared evaluator.

use super::*;
use std::collections::HashSet;

#[derive(Debug)]
struct PlacementPlan {
    snapshots: Vec<usize>,
    order: Vec<usize>,
}

fn plan_placements(mut reads: Vec<HashSet<u32>>, leaves: &[Option<u32>]) -> Option<PlacementPlan> {
    let mut pending = vec![true; reads.len()];
    let mut plan = PlacementPlan {
        snapshots: Vec::new(),
        order: Vec::new(),
    };
    while plan.order.len() < reads.len() {
        let ready = (0..reads.len()).find(|&index| {
            pending[index] && {
                let target = 3 + index as u32;
                leaves[index] == Some(u32::from(target))
                    || (0..reads.len()).all(|other| {
                        other == index || !pending[other] || !reads[other].contains(&target)
                    })
            }
        });
        if let Some(index) = ready {
            pending[index] = false;
            plan.order.push(index);
            continue;
        }
        let index = (0..reads.len()).find(|&index| {
            pending[index] && leaves[index].is_some() && !plan.snapshots.contains(&index)
        })?;
        plan.snapshots.push(index);
        reads[index].clear();
    }
    Some(plan)
}

impl Generator {
    pub(super) fn try_emit_dependent_word_arguments(
        &mut self,
        arguments: &[Expression],
        callee: &str,
    ) -> Compilation<bool> {
        if !(2..=8).contains(&arguments.len())
            || arguments.iter().enumerate().any(|(index, arg)| {
                !self.call_input_is_word_argument(
                    arg,
                    super::call_argument_types::source_parameter_type(
                        self.call_parameter_types.get(callee).map(Vec::as_slice),
                        matches!(
                            self.call_return_types.get(callee),
                            Some(Type::Struct { .. })
                        ),
                        arguments.len(),
                        index,
                    ),
                )
            })
        {
            return Ok(false);
        }
        let reads: Vec<_> = arguments
            .iter()
            .map(|arg| self.registers_used_by(arg))
            .collect();
        let leaves: Vec<_> = arguments
            .iter()
            .map(|arg| {
                self.leaf_info(arg)
                    .ok()
                    .and_then(|(register, width, _)| (width == 32).then_some(register))
            })
            .collect();
        // Existing schedules own leaf-only shuffles and already safe packets.
        // This owner closes the generic guard's unresolved non-leaf dependency.
        if !(0..arguments.len()).any(|index| {
            leaves[index] != Some((3 + index as u32).into())
                && (index + 1..arguments.len()).any(|later| {
                    leaves[later].is_none() && reads[later].contains(&(3 + index as u32))
                })
        }) {
            return Ok(false);
        }
        let preserve_memory_order = self.behavior.optimization == mwcc_versions::Optimization::O0
            || !self.behavior.scheduler_enabled
            || (!self.behavior.reorder_volatile_call_inputs
                && arguments
                    .iter()
                    .any(|arg| self.call_input_may_read_volatile(arg)));
        if preserve_memory_order {
            return self.emit_ordered_word_arguments(arguments, callee, &reads);
        }
        let Some(plan) = plan_placements(reads.clone(), &leaves) else {
            return Ok(false);
        };
        let mut trial = self.clone();
        let mut snapshots = std::collections::HashMap::new();
        for &index in &plan.snapshots {
            let register = trial.fresh_virtual_general_preferring(0);
            trial
                .output
                .instructions
                .push(Instruction::move_register(register, leaves[index].unwrap()));
            snapshots.insert(index, register);
        }
        let mut completed = Vec::new();
        for &index in &plan.order {
            let target = 3 + index as u32;
            let mut protect: HashSet<_> = completed.iter().copied().collect();
            for other in 0..arguments.len() {
                if !completed.contains(&(3 + other as u32))
                    && other != index
                    && !snapshots.contains_key(&other)
                {
                    protect.extend(reads[other].iter().copied());
                }
            }
            protect.remove(&target);
            let inserted: Vec<_> = protect
                .into_iter()
                .filter(|reg| trial.reserved.insert((*reg).into()))
                .collect();
            let result = if let Some(&register) = snapshots.get(&index) {
                trial
                    .output
                    .instructions
                    .push(Instruction::move_register(target.into(), register));
                Ok(())
            } else {
                let ty = super::call_argument_types::source_parameter_type(
                    trial.call_parameter_types.get(callee).map(Vec::as_slice),
                    matches!(
                        trial.call_return_types.get(callee),
                        Some(Type::Struct { .. })
                    ),
                    arguments.len(),
                    index,
                )
                .expect("word-argument eligibility checked the ABI slot");
                trial.evaluate(&arguments[index], ty, target.into())
            };
            for register in inserted {
                trial.reserved.remove(&register);
            }
            result?;
            completed.push(target);
        }
        *self = trial;
        Ok(true)
    }

    fn emit_ordered_word_arguments(
        &mut self,
        arguments: &[Expression],
        callee: &str,
        reads: &[HashSet<u32>],
    ) -> Compilation<bool> {
        let mut trial = self.clone();
        let endangered: Vec<_> = (0..arguments.len())
            .filter_map(|index| {
                let target = 3 + index as u32;
                reads[index + 1..]
                    .iter()
                    .any(|set| set.contains(&target))
                    .then_some(target)
            })
            .collect();
        let mut restored = Vec::new();
        for source in endangered {
            let register = trial.fresh_virtual_general_preferring((3 + arguments.len() as u32).into());
            trial
                .output
                .instructions
                .push(Instruction::move_register(register, source.into()));
            for (name, location) in &mut trial.locations {
                if location.class == ValueClass::General && location.register == source.into() {
                    restored.push((name.clone(), source));
                    location.register = register;
                }
            }
        }
        for (index, argument) in arguments.iter().enumerate() {
            let target = 3 + index as u32;
            let inserted: Vec<_> = (3..target)
                .filter(|reg| trial.reserved.insert((*reg).into()))
                .collect();
            let ty = super::call_argument_types::source_parameter_type(
                trial.call_parameter_types.get(callee).map(Vec::as_slice),
                matches!(
                    trial.call_return_types.get(callee),
                    Some(Type::Struct { .. })
                ),
                arguments.len(),
                index,
            )
            .expect("word-argument eligibility checked the ABI slot");
            let result = trial.evaluate(argument, ty, target.into());
            for register in inserted {
                trial.reserved.remove(&register);
            }
            result?;
        }
        for (name, register) in restored {
            trial.locations.get_mut(&name).unwrap().register = u32::from(register);
        }
        *self = trial;
        Ok(true)
    }

    fn call_input_may_read_volatile(&self, expression: &Expression) -> bool {
        let ordinary = |pointer: &Expression| {
            matches!(pointer, Expression::Variable(name)
            if self.nonvolatile_pointer_bindings.contains(name))
        };
        match expression {
            Expression::Dereference { pointer } => !ordinary(pointer),
            Expression::Index { base, index } => {
                !ordinary(base) || self.call_input_may_read_volatile(index)
            }
            Expression::Member { base, .. } => !ordinary(base),
            Expression::Binary { left, right, .. } => {
                self.call_input_may_read_volatile(left) || self.call_input_may_read_volatile(right)
            }
            Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => {
                self.call_input_may_read_volatile(operand)
            }
            Expression::Variable(name) => self.globals.contains_key(name),
            _ => false,
        }
    }

    pub(crate) fn call_input_is_word_argument(
        &self,
        argument: &Expression,
        parameter_type: Option<Type>,
    ) -> bool {
        // A compact struct-pointer formal can represent a C++ reference.
        // Its dereferenced source needs address recovery in the generic owner.
        matches!(
            parameter_type,
            Some(Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. })
        ) && !(matches!(parameter_type, Some(Type::StructPointer { .. }))
            && matches!(argument, Expression::Dereference { .. }))
            && self.call_input_is_word_expression(argument)
    }

    pub(crate) fn call_input_is_word_expression(&self, expression: &Expression) -> bool {
        let word = |ty| {
            matches!(
                ty,
                Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
            )
        };
        match expression {
            Expression::IntegerLiteral(_) => true,
            Expression::Variable(name) => {
                self.locations
                    .get(name)
                    .is_some_and(|l| l.class == ValueClass::General && l.width == 32)
                    || self.globals.get(name).copied().is_some_and(word)
            }
            Expression::Dereference { pointer } => {
                self.pointee_of(pointer)
                    .is_ok_and(|p| !matches!(p, Pointee::Float | Pointee::Double) && p.size() <= 4)
                    && self.call_input_is_word_expression(pointer)
            }
            Expression::Index { base, index } => {
                self.pointee_of(base)
                    .is_ok_and(|p| !matches!(p, Pointee::Float | Pointee::Double) && p.size() <= 4)
                    && self.call_input_is_word_expression(base)
                    && self.call_input_is_word_expression(index)
            }
            Expression::Member {
                base, member_type, ..
            } => word(*member_type) && self.call_input_is_word_expression(base),
            Expression::Binary { left, right, .. } => {
                self.call_input_is_word_expression(left)
                    && self.call_input_is_word_expression(right)
            }
            Expression::Unary { operand, .. } => self.call_input_is_word_expression(operand),
            Expression::Cast {
                target_type,
                operand,
            } => word(*target_type) && self.call_input_is_word_expression(operand),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reads(values: &[&[u32]]) -> Vec<HashSet<u32>> {
        values.iter().map(|v| v.iter().copied().collect()).collect()
    }
    #[test]
    fn breaks_a_pointer_load_cycle_by_preserving_the_leaf_value() {
        let plan = plan_placements(reads(&[&[4], &[3]]), &[Some(4), None]).unwrap();
        assert_eq!(plan.snapshots, [0]);
        assert_eq!(plan.order, [1, 0]);
    }
    #[test]
    fn consumes_crossed_inputs_before_overwriting_their_homes() {
        let plan = plan_placements(reads(&[&[4], &[3], &[4]]), &[Some(4), None, None]).unwrap();
        assert_eq!(plan.snapshots, [0]);
        assert_eq!(plan.order, [2, 1, 0]);
        assert!(plan_placements(reads(&[&[4], &[3]]), &[None, None]).is_none());
    }
}
