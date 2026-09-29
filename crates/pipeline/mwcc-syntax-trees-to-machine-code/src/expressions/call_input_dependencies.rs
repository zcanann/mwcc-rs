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

/// ABI homes that source-order evaluation overwrites while a later argument
/// still reads the incoming value there. An argument already in its home
/// writes nothing.
fn source_order_endangered_homes(reads: &[HashSet<u32>], leaves: &[Option<u32>]) -> Vec<u32> {
    (0..reads.len())
        .filter_map(|index| {
            let target = 3 + index as u32;
            (leaves[index] != Some(target)
                && reads[index + 1..].iter().any(|set| set.contains(&target)))
            .then_some(target)
        })
        .collect()
}

/// Whether the generic marshaler's one-leaf pre-copy resolves `endangered`.
/// It copies the single later leaf reading the endangered home into that
/// leaf's own ABI home before anything else is written (`memset(d, 0, n)`:
/// `mr r5,r4; li r4,0`). That coalesced copy is sound only when no other
/// argument reads either register afterward.
fn generic_leaf_prematerialization_is_sound(
    endangered: &[u32],
    reads: &[HashSet<u32>],
    leaves: &[Option<u32>],
) -> bool {
    let [home] = endangered else {
        return false;
    };
    let readers: Vec<_> = (0..reads.len())
        .filter(|&index| 3 + index as u32 != *home && reads[index].contains(home))
        .collect();
    let [later] = readers[..] else {
        return false;
    };
    let later_home = 3 + later as u32;
    later_home > *home
        && leaves[later] == Some(*home)
        && (0..reads.len()).all(|other| other == later || !reads[other].contains(&later_home))
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
        let later_non_leaf_reads_earlier_home = (0..arguments.len()).any(|index| {
            leaves[index] != Some((3 + index as u32).into())
                && (index + 1..arguments.len()).any(|later| {
                    leaves[later].is_none() && reads[later].contains(&(3 + index as u32))
                })
        });
        // A later LEAF whose incoming register is overwritten by an earlier
        // argument (`k2(b - 1, a)`: arg 0 writes r3 while `a` still lives
        // there). The generic marshaler copies such a leaf straight into its
        // own ABI home first, which is only sound when nothing else still
        // reads that home. Every other shape is a parallel move with a cycle
        // through a computed argument; claim it here rather than let the
        // generic path overwrite a live input.
        let endangered = source_order_endangered_homes(&reads, &leaves);
        if !later_non_leaf_reads_earlier_home
            && (endangered.is_empty()
                || generic_leaf_prematerialization_is_sound(&endangered, &reads, &leaves))
        {
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
        // An acyclic dependency is a pure reordering: each consumer runs
        // before its input's home is overwritten, and no copy is needed. A
        // cycle needs a saved copy; mwcc saves the endangered incoming values
        // in source order (see `emit_snapshotted_word_arguments`).
        let plan = plan_placements(reads.clone(), &leaves)
            .filter(|plan| plan.snapshots.is_empty());
        let Some(plan) = plan else {
            return self.emit_snapshotted_word_arguments(arguments, callee, &reads);
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

    /// Marshal a pure word-leaf permutation (`k2(b, a)`, `k3(c, a, b)`) with
    /// the snapshot coloring used by scheduled non-linkage-first builds:
    /// `mr r0,r3; mr r3,r4; mr r4,r0`. Linkage-first builds and unscheduled
    /// modes keep their dedicated swap/permutation schedules.
    pub(crate) fn try_emit_snapshotted_word_permutation(
        &mut self,
        arguments: &[Expression],
        callee: &str,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention == mwcc_versions::FrameConvention::LinkageFirst
            || self.behavior.optimization == mwcc_versions::Optimization::O0
            || !self.behavior.scheduler_enabled
            || !(2..=8).contains(&arguments.len())
        {
            return Ok(false);
        }
        let return_struct = matches!(self.call_return_types.get(callee), Some(Type::Struct { .. }));
        let parameter_types = self.call_parameter_types.get(callee).map(Vec::as_slice);
        if !arguments.iter().enumerate().all(|(index, argument)| {
            let parameter_type = super::call_argument_types::source_parameter_type(
                parameter_types,
                return_struct,
                arguments.len(),
                index,
            );
            parameter_type.is_some_and(|ty| ty.width() == 32)
                && self.call_input_is_word_argument(argument, parameter_type)
                && self
                    .leaf_info(argument)
                    .is_ok_and(|(_, width, _)| width == 32)
        }) {
            return Ok(false);
        }
        let reads: Vec<_> = arguments
            .iter()
            .map(|arg| self.registers_used_by(arg))
            .collect();
        let leaves: Vec<_> = arguments
            .iter()
            .map(|arg| self.leaf_info(arg).ok().map(|(register, _, _)| register))
            .collect();
        let endangered = source_order_endangered_homes(&reads, &leaves);
        // Acyclic shuffles (`k3(d, a, b)`: `mr r5,r4; mr r4,r3; mr r3,r6`)
        // stay with the dependency-ordered owners.
        if endangered.is_empty()
            || plan_placements(reads.clone(), &leaves).is_some_and(|plan| plan.snapshots.is_empty())
        {
            return Ok(false);
        }
        // The dedicated swap/permutation owners remain the fallback.
        Ok(self
            .emit_snapshotted_word_arguments(arguments, callee, &reads)
            .unwrap_or(false))
    }

    /// Resolve a word-argument parallel move the way mwcc's coloring does.
    ///
    /// Arguments are evaluated in source order. Each incoming value whose
    /// register an earlier argument overwrites is first copied to its own
    /// virtual home, and every read in the argument list uses that copy
    /// (`k2(b + a, a)`: `mr r0,r3; add r3,r4,r0; mr r4,r0`). Homes are colored
    /// from the last endangered value back: the first one that is never a
    /// nonzero base (in this call or earlier in the body) takes r0, and the
    /// rest take the registers just past the argument list in turn
    /// (`k4(d, a, b, c)`: c -> r0, b -> r7, a -> r8).
    fn emit_snapshotted_word_arguments(
        &mut self,
        arguments: &[Expression],
        callee: &str,
        reads: &[HashSet<u32>],
    ) -> Compilation<bool> {
        let leaves: Vec<_> = arguments
            .iter()
            .map(|arg| {
                self.leaf_info(arg)
                    .ok()
                    .and_then(|(register, width, _)| (width == 32).then_some(register))
            })
            .collect();
        let endangered = source_order_endangered_homes(reads, &leaves);
        let mut trial = self.clone();
        let prior_end = trial.output.instructions.len();
        let mut snapshots = Vec::new();
        let mut restored = Vec::new();
        for source in endangered {
            let register = trial.fresh_virtual_general();
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
            snapshots.push((source, register));
        }
        let body_start = trial.output.instructions.len();
        let renamed_reads: Vec<_> = arguments
            .iter()
            .map(|arg| trial.registers_used_by(arg))
            .collect();
        // A word-to-word cast (`(int)p`) is still a plain register copy.
        fn peel_word_casts(mut expression: &Expression) -> &Expression {
            while let Expression::Cast {
                target_type:
                    Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. },
                operand,
            } = expression
            {
                expression = operand;
            }
            expression
        }
        let renamed_leaves: Vec<_> = arguments
            .iter()
            .map(|arg| {
                trial
                    .leaf_info(peel_word_casts(arg))
                    .ok()
                    .and_then(|(register, width, _)| (width == 32).then_some(register))
            })
            .collect();
        let mut result = Ok(());
        for (index, argument) in arguments.iter().enumerate() {
            let target = 3 + index as u32;
            // Completed homes and every register a later argument still reads.
            let protect: HashSet<u32> = (3..target)
                .chain(renamed_reads[index + 1..].iter().flatten().copied())
                .filter(|register| *register != target && *register < 32)
                .collect();
            let inserted: Vec<_> = protect
                .into_iter()
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
            result = trial.evaluate(argument, ty, target.into());
            for register in inserted {
                trial.reserved.remove(&register);
            }
            if result.is_err() {
                break;
            }
        }
        for (name, register) in restored {
            trial.locations.get_mut(&name).unwrap().register = u32::from(register);
        }
        result?;
        let body_bases: HashSet<u32> = trial.output.instructions[body_start..]
            .iter()
            .filter_map(mwcc_vreg::nonzero_base)
            .collect();
        let prior_bases: HashSet<u32> = trial.output.instructions[..prior_end]
            .iter()
            .filter_map(mwcc_vreg::nonzero_base)
            .collect();
        // A copy passed directly as a leaf argument whose ABI home holds no
        // live input coalesces into that home; the leaf's own move vanishes
        // (`k3(b, p[0], p)`: `mr r5,r3; mr r3,r4; lwz r4,0(r5)`).
        let sources: HashSet<u32> = snapshots.iter().map(|&(source, _)| source).collect();
        let mut coalesced = HashSet::new();
        let mut homes_taken = HashSet::new();
        for &(_, register) in &snapshots {
            let Some(home) = (0..arguments.len())
                .find(|&index| renamed_leaves[index] == Some(register))
                .map(|index| 3 + index as u32)
            else {
                continue;
            };
            if !sources.contains(&home)
                && homes_taken.insert(home)
                && renamed_reads.iter().all(|set| !set.contains(&home))
            {
                trial.prefer_virtual_general(register, home);
                coalesced.insert(register);
            }
        }
        let mut zero_taken = false;
        let mut next_home = 3 + arguments.len() as u32;
        for &(source, register) in snapshots.iter().rev() {
            if coalesced.contains(&register) {
                continue;
            }
            if !zero_taken && !body_bases.contains(&register) && !prior_bases.contains(&source) {
                trial.prefer_virtual_general(register, 0);
                zero_taken = true;
            } else {
                if next_home <= u32::from(Eabi::LAST_GENERAL_ARGUMENT) + 2 {
                    trial.prefer_virtual_general(register, next_home);
                }
                next_home += 1;
            }
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
    #[test]
    fn a_computed_argument_reading_a_later_leafs_home_is_not_a_coalescable_precopy() {
        // `k2(b - 1, a)`: copying a (r3) into r4 first would destroy b.
        let crossed = reads(&[&[4], &[3]]);
        let leaves = [None, Some(3)];
        let endangered = source_order_endangered_homes(&crossed, &leaves);
        assert_eq!(endangered, [3]);
        assert!(!generic_leaf_prematerialization_is_sound(&endangered, &crossed, &leaves));
        // `memset(d, 0, n)`-style: `k3(x, 0, a)` with a in r4 -> r5 is sound.
        let forward = reads(&[&[6], &[], &[4]]);
        let leaves = [Some(6), None, Some(4)];
        let endangered = source_order_endangered_homes(&forward, &leaves);
        assert_eq!(endangered, [4]);
        assert!(generic_leaf_prematerialization_is_sound(&endangered, &forward, &leaves));
    }
}
