//! GC/1.3 drops a single promoted addend's sign word beside an unsigned
//! arithmetic or call expression. Dynamic address or two-word computation
//! materializes the pending expression; `(u+v)+a[i]` differs from `a[i]+(u+v)`.
//! Named wide values, shifts, signed computations and word-valued calls keep
//! their full pair. Track these boundaries before address folding and demand.
use super::*;
use std::collections::HashSet;

fn materialized_word_results(
    operations: &[Operation],
    named: &HashSet<usize>,
    values: &mut HashSet<usize>,
) {
    for op in operations {
        match op {
            Operation::Call {
                result:
                    Some(Value {
                        ty,
                        source: Source::Register(id),
                    }),
                ..
            } if !wide(*ty) => {
                values.insert(*id);
            }
            Operation::Binary {
                operator: BinaryOperator::Multiply,
                result:
                    Value {
                        ty,
                        source: Source::Register(id),
                    },
                left,
                right,
                ..
            } if !wide(*ty)
                && !matches!(left.source, Source::Constant(_))
                && !matches!(right.source, Source::Constant(_))
                && !named.contains(id) =>
            {
                values.insert(*id);
            }
            Operation::Branch {
                then_body,
                else_body,
                ..
            } => {
                materialized_word_results(then_body, named, values);
                materialized_word_results(else_body, named, values);
            }
            _ => {}
        }
    }
}

// These remain expression operands until promotion, even when their selected
// instructions need multiple registers. A source multiply is different from
// the equivalent shift or negation; only multiplication by one disappears.
fn transparent_word_operation(
    operator: BinaryOperator,
    result: Value,
    left: Value,
    right: Value,
) -> bool {
    if !matches!(result.ty, Type::Int | Type::UnsignedInt) {
        return false;
    }
    if operator == BinaryOperator::Multiply {
        return matches!(left.source, Source::Constant(1))
            || matches!(right.source, Source::Constant(1));
    }
    matches!(
        operator,
        BinaryOperator::Add
            | BinaryOperator::Subtract
            | BinaryOperator::Divide
            | BinaryOperator::Modulo
            | BinaryOperator::BitAnd
            | BinaryOperator::BitOr
            | BinaryOperator::BitXor
            | BinaryOperator::ShiftLeft
            | BinaryOperator::ShiftRight
    ) && (matches!(left.source, Source::Constant(_)) || matches!(right.source, Source::Constant(_)))
}

impl Graph<'_> {
    pub(super) fn lower_word_addends(&mut self, uses: &HashMap<usize, usize>) {
        let mut materialized = HashSet::new();
        materialized_word_results(&self.operations, &self.named_values, &mut materialized);
        let operations = std::mem::take(&mut self.operations);
        self.operations = self.rewrite_addends(operations, uses, &materialized);
    }

    fn rewrite_addends(
        &mut self,
        operations: Vec<Operation>,
        uses: &HashMap<usize, usize>,
        materialized: &HashSet<usize>,
    ) -> Vec<Operation> {
        let mut rewritten = Vec::new();
        let mut pending = None;
        for mut op in operations {
            match &mut op {
                Operation::Branch {
                    then_body,
                    else_body,
                    ..
                } => {
                    *then_body =
                        self.rewrite_addends(std::mem::take(then_body), uses, materialized);
                    *else_body =
                        self.rewrite_addends(std::mem::take(else_body), uses, materialized);
                    pending = None;
                }
                Operation::Binary {
                    operator,
                    result,
                    left,
                    right,
                    ..
                } => {
                    let is_pending =
                        |v: Value| matches!(v.source, Source::Register(id) if Some(id) == pending);
                    let single = |v: Value| {
                        self.single_word_promotion(v, uses)
                            && !matches!(v.source, Source::Register(id) if self.signed_word_promotions.get(&id).is_some_and(|input| materialized.contains(input)))
                    };
                    if *operator == BinaryOperator::Add && result.ty == Type::UnsignedLongLong {
                        if self.first_shared_promotion(*right) {
                            self.zero_extend_word_at_use(right, &mut rewritten);
                        } else if self.first_shared_promotion(*left) {
                            self.zero_extend_word_at_use(left, &mut rewritten);
                        } else if is_pending(*left) && single(*right) {
                            self.zero_extend_word_at_use(right, &mut rewritten);
                        } else if is_pending(*right) && single(*left) {
                            self.zero_extend_word_at_use(left, &mut rewritten);
                        }
                    }
                    // A constant address displacement remains part of a load;
                    // a dynamic index requires its own evaluation instead.
                    let displacement =
                        matches!(result.ty, Type::Pointer(_) | Type::StructPointer { .. })
                            && matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
                            && matches!(right.source, Source::Constant(_));
                    pending = if displacement
                        || transparent_word_operation(*operator, *result, *left, *right)
                    {
                        pending
                    } else if matches!(
                        operator,
                        BinaryOperator::Add
                            | BinaryOperator::Subtract
                            | BinaryOperator::Multiply
                            | BinaryOperator::BitAnd
                            | BinaryOperator::BitOr
                            | BinaryOperator::BitXor
                    ) {
                        self.pending_unsigned_expression(*result)
                    } else {
                        None
                    };
                }
                Operation::Call { result, .. } => {
                    pending = result.and_then(|v| self.pending_unsigned_expression(v));
                }
                Operation::Label(_)
                | Operation::Jump(_)
                | Operation::BranchIf { .. }
                | Operation::Return(_) => pending = None,
                _ => {}
            }
            rewritten.push(op);
        }
        rewritten
    }

    fn pending_unsigned_expression(&self, value: Value) -> Option<usize> {
        match value {
            Value {
                ty: Type::UnsignedLongLong,
                source: Source::Register(id),
            } if !self.named_values.contains(&id) => Some(id),
            _ => None,
        }
    }
}
