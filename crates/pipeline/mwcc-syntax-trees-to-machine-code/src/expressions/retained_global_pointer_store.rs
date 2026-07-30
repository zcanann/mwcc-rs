//! Store-local reuse of a file-scope structure pointer.
//!
//! MWCC loads the base once for expressions such as
//! `current->total = current->total + current->chunk - remaining`.  The
//! ordinary recursive evaluator cannot infer that the final store base and
//! multiple member loads denote the same nonvolatile value, so it otherwise
//! reloads the pointer for every access.

use super::*;
use crate::condition_global_cache::ConditionGlobalValue;

impl Generator {
    pub(crate) fn try_emit_retained_global_pointer_member_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Some((name, offset, member_type)) = retained_store_plan(target, value) else {
            return Ok(false);
        };
        if self.locations.contains_key(name)
            || self.volatile_globals.contains(name)
            || !matches!(self.globals.get(name), Some(Type::StructPointer { .. }))
            || self.condition_global_values.contains_key(name)
        {
            return Ok(false);
        }
        let pointee = match pointee_of_type(member_type) {
            Some(pointee) if !matches!(pointee, Pointee::Float | Pointee::Double) => pointee,
            _ => return Ok(false),
        };
        let displacement = match i16::try_from(offset) {
            Ok(displacement) => displacement,
            Err(_) => return Ok(false),
        };
        let accumulator = retained_accumulator_parts(value, name, offset, member_type);

        let name = name.to_owned();
        let base = accumulator
            .as_ref()
            .map(|_| self.fresh_virtual_general_preferring(6))
            .unwrap_or_else(|| self.fresh_virtual_general());
        self.emit_global_load_value(&name, base)?;
        self.condition_global_values
            .insert(name.clone(), ConditionGlobalValue::Register(base));
        let restore_reservation = self.reserved.insert(base);
        let source = match accumulator {
            Some(accumulator) => self.emit_retained_accumulator_value(accumulator),
            None => self.place_store_value(value, pointee),
        };
        if restore_reservation {
            self.reserved.remove(&base);
        }
        self.condition_global_values.remove(&name);
        let source = source?;

        self.output
            .instructions
            .push(displacement_store(pointee, source, base, displacement)?);
        Ok(true)
    }

    fn emit_retained_accumulator_value(
        &mut self,
        accumulator: RetainedAccumulator<'_>,
    ) -> Compilation<u8> {
        let decrement = self.fresh_virtual_general_preferring(4);
        self.evaluate_general(accumulator.decrement, decrement)?;
        let restore_decrement = self.reserved.insert(decrement);

        let increment = self.fresh_virtual_general_preferring(GENERAL_SCRATCH);
        let increment_result = self.evaluate_general(accumulator.increment, increment);
        if let Err(error) = increment_result {
            if restore_decrement {
                self.reserved.remove(&decrement);
            }
            return Err(error);
        }
        let restore_increment = self.reserved.insert(increment);

        let accumulated = self.fresh_virtual_general_preferring(5);
        let accumulated_result = self.evaluate_general(accumulator.accumulated, accumulated);
        if restore_increment {
            self.reserved.remove(&increment);
        }
        if restore_decrement {
            self.reserved.remove(&decrement);
        }
        accumulated_result?;

        self.output.instructions.push(Instruction::SubtractFrom {
            d: increment,
            a: decrement,
            b: increment,
        });
        self.output.instructions.push(Instruction::Add {
            d: increment,
            a: accumulated,
            b: increment,
        });
        Ok(increment)
    }
}

struct RetainedAccumulator<'a> {
    accumulated: &'a Expression,
    increment: &'a Expression,
    decrement: &'a Expression,
}

fn retained_store_plan<'a>(
    target: &'a Expression,
    value: &Expression,
) -> Option<(&'a str, u32, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = target
    else {
        return None;
    };
    let Expression::Variable(name) = base.as_ref() else {
        return None;
    };
    (pure_member_reads(value, name)? >= 2).then_some((name, *offset, *member_type))
}

fn retained_accumulator_parts<'a>(
    value: &'a Expression,
    name: &str,
    offset: u32,
    member_type: Type,
) -> Option<RetainedAccumulator<'a>> {
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: accumulated,
        right,
    } = value
    else {
        return None;
    };
    if !is_direct_member(accumulated, name, offset, member_type) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left: increment,
        right: decrement,
    } = right.as_ref()
    else {
        return None;
    };
    if !is_direct_member_of(increment, name) || pure_member_reads(decrement, name)? != 0 {
        return None;
    }
    Some(RetainedAccumulator {
        accumulated,
        increment,
        decrement,
    })
}

fn is_direct_member(expression: &Expression, name: &str, offset: u32, member_type: Type) -> bool {
    matches!(
        expression,
        Expression::Member {
            base,
            offset: member_offset,
            member_type: actual_type,
            index_stride: None,
        } if matches!(
            base.as_ref(),
            Expression::Variable(base_name) if base_name == name
        ) && *member_offset == offset && *actual_type == member_type
    )
}

fn is_direct_member_of(expression: &Expression, name: &str) -> bool {
    matches!(
        expression,
        Expression::Member {
            base,
            index_stride: None,
            ..
        } if matches!(
            base.as_ref(),
            Expression::Variable(base_name) if base_name == name
        )
    )
}

/// Count direct member reads through `name` while proving that evaluating the
/// expression cannot change the pointer value. Unsupported control-flow forms
/// conservatively decline this store-local schedule.
fn pure_member_reads(expression: &Expression, name: &str) -> Option<usize> {
    match expression {
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            let direct = usize::from(matches!(
                base.as_ref(),
                Expression::Variable(base_name) if base_name == name
            ));
            Some(direct + pure_member_reads(base, name)?)
        }
        Expression::Binary { left, right, .. }
        | Expression::Index {
            base: left,
            index: right,
        } => Some(pure_member_reads(left, name)? + pure_member_reads(right, name)?),
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::BitFieldRead {
            extracted: operand, ..
        } => pure_member_reads(operand, name),
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => Some(0),
        Expression::Call { .. }
        | Expression::CallThrough { .. }
        | Expression::VirtualCall { .. }
        | Expression::ConstructedNew { .. }
        | Expression::PostStep { .. }
        | Expression::Assign { .. }
        | Expression::Comma { .. }
        | Expression::Conditional { .. }
        | Expression::IndexedUpdateValue { .. }
        | Expression::AggregateLiteral(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(global: &str, offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(global.into())),
            offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        }
    }

    fn store_target(global: &str) -> Expression {
        member(global, 32)
    }

    #[test]
    fn recognizes_two_pure_rhs_member_reads() {
        let value = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(member("current", 32)),
            right: Box::new(member("current", 28)),
        };
        assert_eq!(
            retained_store_plan(&store_target("current"), &value),
            Some(("current", 32, Type::UnsignedInt))
        );
    }

    #[test]
    fn recognizes_retained_accumulator_shape() {
        let decrement = Expression::Variable("remaining".into());
        let value = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(member("current", 32)),
            right: Box::new(Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: Box::new(member("current", 28)),
                right: Box::new(decrement.clone()),
            }),
        };
        let plan = retained_accumulator_parts(&value, "current", 32, Type::UnsignedInt).unwrap();
        assert!(structurally_equal(plan.accumulated, &member("current", 32)));
        assert!(structurally_equal(plan.increment, &member("current", 28)));
        assert!(structurally_equal(plan.decrement, &decrement));
    }

    #[test]
    fn rejects_a_single_rhs_member_read() {
        assert_eq!(
            retained_store_plan(&store_target("current"), &member("current", 28)),
            None
        );
    }

    #[test]
    fn rejects_reuse_across_a_call() {
        let value = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(member("current", 32)),
            right: Box::new(Expression::Call {
                name: "advance".into(),
                arguments: vec![member("current", 28)],
            }),
        };
        assert_eq!(retained_store_plan(&store_target("current"), &value), None);
    }

    #[test]
    fn counts_only_the_target_pointer() {
        let value = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(member("current", 32)),
            right: Box::new(member("other", 28)),
        };
        assert_eq!(retained_store_plan(&store_target("current"), &value), None);
    }
}
