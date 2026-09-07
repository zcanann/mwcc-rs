//! The GC/1.1p1 O0 source-parameter spill bug, before register allocation.
//!
//! These are aliased source images, not independently allocated frame locals.
//! Their declaration-order writes and reloads deliberately preserve overlaps
//! with each other, the saved result home, and the caller's linkage area.

use super::structured_expression_visit::{rewrite_expression, visit_expression};
use super::*;
use crate::expressions::displacement_load;

impl Generator {
    pub(crate) fn try_shared_parameter_spill_call(
        &mut self,
        source: &Function,
    ) -> Compilation<bool> {
        if !self.behavior.unoptimized_shared_parameter_spills
            || source.return_type != Type::UnsignedInt
            || !source.guards.is_empty()
            || !self.frame_slots.is_empty()
            || source.parameters.is_empty()
        {
            return Ok(false);
        }
        let lowered = self.expose_structured_runtime_conversions(source);
        let function = lowered.as_ref().unwrap_or(source);
        let Some(tail) = &function.return_expression else {
            return Ok(false);
        };
        let (result_name, call_value) = match function.locals.as_slice() {
            [] if function.statements.is_empty() => (None, None),
            [local]
                if local.declared_type == Type::UnsignedInt
                    && !local.is_static
                    && !local.is_volatile
                    && local.array_length.is_none() =>
            {
                let value = match (local.initializer.as_ref(), function.statements.as_slice()) {
                    (Some(value), []) => value,
                    (None, [Statement::Assign { name, value }]) if name == &local.name => value,
                    _ => return Ok(false),
                };
                (Some(local.name.as_str()), Some(value))
            }
            _ => return Ok(false),
        };
        if result_name.is_some_and(|name| !expression_reads_name(tail, name)) {
            return Ok(false);
        }
        let mut calls = Vec::new();
        visit_expression(call_value.unwrap_or(tail), &mut |value| {
            if let Expression::Call { name, arguments } = value {
                calls.push((name.clone(), arguments.clone()));
            }
        });
        let [(callee, arguments)] = calls.as_slice() else {
            return Ok(false);
        };
        let [Expression::Variable(argument)] = arguments.as_slice() else {
            return Ok(false);
        };
        if self.skipped_inline_names.contains(callee)
            || self.call_return_types.get(callee) != Some(&Type::UnsignedInt)
            || call_value.is_some_and(|value| !matches!(value, Expression::Call { .. }))
        {
            return Ok(false);
        }
        let Some(argument_parameter) = function.parameters.iter().find(|p| p.name == *argument)
        else {
            return Ok(false);
        };
        let argument_type = argument_parameter.parameter_type;
        if callee == crate::runtime_conversions::FLOAT_TO_UNSIGNED {
            if !matches!(argument_type, Type::Float | Type::Double) {
                return Ok(false);
            }
        } else if !matches!(argument_type, Type::Int | Type::UnsignedInt)
            || !matches!(
                self.call_parameter_types.get(callee).map(Vec::as_slice),
                Some([Type::Int | Type::UnsignedInt])
            )
        {
            return Ok(false);
        }
        let mut valid = true;
        let mut tail_calls = 0;
        visit_expression(tail, &mut |value| match value {
            Expression::Call { .. } => tail_calls += 1,
            Expression::Variable(name) if Some(name.as_str()) == result_name => {}
            Expression::Variable(name) => {
                valid &= function.parameters.iter().any(|p| p.name == *name);
            }
            Expression::Binary {
                operator:
                    BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::BitXor
                    | BinaryOperator::BitOr
                    | BinaryOperator::BitAnd,
                ..
            }
            | Expression::Cast {
                target_type: Type::Int | Type::UnsignedInt,
                ..
            } => {}
            _ => valid = false,
        });
        if !valid || tail_calls != usize::from(result_name.is_none()) {
            return Ok(false);
        }
        let mut general = 3;
        let mut float = 1;
        let mut spills = Vec::new();
        for parameter in &function.parameters {
            let ty = parameter.parameter_type;
            let Some(pointee) = pointee_of_type(ty) else {
                return Ok(false);
            };
            if matches!(ty, Type::LongLong | Type::UnsignedLongLong) {
                return Ok(false);
            }
            let class = class_of(ty)?;
            let register = if class == ValueClass::Float {
                let r = float;
                float += 1;
                r
            } else {
                let r = general;
                general += 1;
                r
            };
            if general > 11 || float > 9 {
                return Ok(false);
            }
            if !expression_reads_name(tail, &parameter.name) && parameter.name != *argument {
                return Ok(false);
            }
            spills.push((parameter.name.clone(), ty, class, register, pointee));
        }
        self.emit_linkage_first_nonleaf_prologue(if result_name.is_some() { &[31] } else { &[] });
        for (name, _, _, register, pointee) in &spills {
            self.output
                .instructions
                .push(displacement_store(*pointee, *register, 1, 8)?);
            self.locations.remove(name);
        }
        self.output.instructions.push(displacement_load(
            pointee_of_type(argument_type).expect("admitted scalar"),
            if matches!(argument_type, Type::Float | Type::Double) {
                1
            } else {
                3
            },
            1,
            8,
        )?);
        self.emit_call(callee, &[], Some(3), false)?;
        let home = if result_name.is_some() { 31 } else { 3 };
        let mut suffix = 0;
        let (result, stack) = loop {
            let result = format!("__mwcc_shared_spill_result_{suffix}");
            let stack = format!("__mwcc_shared_spill_stack_{suffix}");
            if function
                .parameters
                .iter()
                .all(|p| p.name != result && p.name != stack)
                && function
                    .locals
                    .iter()
                    .all(|p| p.name != result && p.name != stack)
            {
                break (result, stack);
            }
            suffix += 1;
        };
        let result = result_name.unwrap_or(&result);
        if result_name.is_some() {
            self.output
                .instructions
                .push(Instruction::move_register(31, 3));
        }
        self.locations.insert(
            result.into(),
            Location {
                class: ValueClass::General,
                register: home,
                signed: false,
                width: 32,
                pointee: None,
                stride: None,
            },
        );
        self.locations.insert(
            stack.clone(),
            Location {
                class: ValueClass::General,
                register: 1,
                signed: false,
                width: 32,
                pointee: None,
                stride: Some(1),
            },
        );
        let tail = rewrite_expression(tail, &mut |value| match value {
            Expression::Call { .. } => Some(Expression::Variable(result.into())),
            Expression::Variable(name) if Some(name.as_str()) != result_name => spills
                .iter()
                .find(|(parameter, ..)| parameter == name)
                .map(|(_, ty, ..)| Expression::Member {
                    base: Box::new(Expression::Variable(stack.clone())),
                    offset: 8,
                    member_type: *ty,
                    index_stride: None,
                }),
            _ => None,
        });
        // The patch's unoptimized sum scheduler combines the two spill loads
        // before adding the retained call-result home.
        let grouped_sum = result_name.is_some()
            && matches!(&tail,
            Expression::Binary { operator: BinaryOperator::Add, left, right }
            if matches!(right.as_ref(), Expression::Member { member_type: Type::Int | Type::UnsignedInt, .. })
                && matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::Add, left, right }
                    if matches!(left.as_ref(), Expression::Variable(name) if name == result)
                        && matches!(right.as_ref(), Expression::Member { member_type: Type::Int | Type::UnsignedInt, .. })));
        if grouped_sum {
            self.output.instructions.extend([
                Instruction::LoadWord {
                    d: 3,
                    a: 1,
                    offset: 8,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: 8,
                },
                Instruction::Add { d: 3, a: 0, b: 3 },
                Instruction::Add { d: 3, a: 31, b: 3 },
            ]);
        } else {
            self.evaluate_general(&tail, 3)?;
        }
        self.locations.remove(&stack);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
