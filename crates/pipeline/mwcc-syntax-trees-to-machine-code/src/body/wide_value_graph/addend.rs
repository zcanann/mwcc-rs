//! GC/1.3's addend extensions depend on operand order and expression origin.
//! A word-first addition can lose its sign word, as can a word beside a pending
//! unsigned expression. Constants, named pairs, casts, and materialized word
//! computations distinguish these paths. Signed views retain the original
//! pair's origin; classify before address folding and word-demand pruning.
use super::*;
use std::collections::HashSet;

#[derive(Default)]
struct OperandFacts {
    unsigned_pairs: HashSet<usize>,
    materialized_words: HashSet<usize>,
    widened_words: HashSet<usize>,
    unsigned_casts: HashSet<usize>,
}

fn collect_operand_facts(
    operations: &[Operation],
    named: &HashSet<usize>,
    facts: &mut OperandFacts,
) {
    for op in operations {
        let result = match op {
            Operation::Parameter { result, .. }
            | Operation::Copy { result, .. }
            | Operation::Convert { result, .. }
            | Operation::Load { result, .. }
            | Operation::Binary { result, .. } => Some(*result),
            Operation::Call { result, .. } => *result,
            _ => None,
        };
        if let Some(Value {
            ty: Type::UnsignedLongLong,
            source: Source::Register(id),
        }) = result
        {
            facts.unsigned_pairs.insert(id);
        }
        match op {
            Operation::Call {
                result:
                    Some(Value {
                        ty,
                        source: Source::Register(id),
                    }),
                ..
            } if !wide(*ty) => {
                facts.materialized_words.insert(*id);
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
                facts.materialized_words.insert(*id);
            }
            Operation::Convert { result, value } if wide(result.ty) && !wide(value.ty) => {
                if let Source::Register(id) = result.source {
                    facts.widened_words.insert(id);
                }
            }
            Operation::Copy { result, value }
                if result.ty == Type::UnsignedLongLong && value.ty == Type::LongLong =>
            {
                if let Source::Register(id) = result.source {
                    facts.unsigned_casts.insert(id);
                }
            }
            Operation::Branch {
                then_body,
                else_body,
                ..
            } => {
                collect_operand_facts(then_body, named, facts);
                collect_operand_facts(else_body, named, facts);
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

// Wide constants are materialized before an adjacent word conversion. A
// nontrivial multiply remains an arithmetic expression; identities do not.
fn materializes_constant_pair(operator: BinaryOperator, left: Value, right: Value) -> bool {
    if !wide(left.ty) {
        return false;
    }
    let constant = match (left.source, right.source) {
        (Source::Constant(n), _) | (_, Source::Constant(n)) => n,
        _ => return false,
    };
    matches!(
        operator,
        BinaryOperator::Add
            | BinaryOperator::Subtract
            | BinaryOperator::BitAnd
            | BinaryOperator::BitOr
            | BinaryOperator::BitXor
    ) || operator == BinaryOperator::Multiply && constant <= 1
}

impl Graph<'_> {
    pub(super) fn lower_word_addends(&mut self, uses: &HashMap<usize, usize>) {
        let mut facts = OperandFacts::default();
        collect_operand_facts(&self.operations, &self.named_values, &mut facts);
        let operations = std::mem::take(&mut self.operations);
        self.operations = self.rewrite_addends(operations, uses, &facts);
    }

    fn rewrite_addends(
        &mut self,
        operations: Vec<Operation>,
        uses: &HashMap<usize, usize>,
        facts: &OperandFacts,
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
                    *then_body = self.rewrite_addends(std::mem::take(then_body), uses, facts);
                    *else_body = self.rewrite_addends(std::mem::take(else_body), uses, facts);
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
                            && !matches!(v.source, Source::Register(id) if matches!(self.promotion_shapes.get(&id), Some(nested::Shape::Cast(Type::LongLong, _))))
                            && !matches!(v.source, Source::Register(id) if self.signed_word_promotions.get(&id).is_some_and(|input| facts.materialized_words.contains(input)))
                    };
                    if *operator == BinaryOperator::Add && wide(result.ty) {
                        if result.ty == Type::UnsignedLongLong
                            && self.first_shared_promotion(*right)
                        {
                            self.zero_extend_word_at_use(right, &mut rewritten);
                        } else if result.ty == Type::UnsignedLongLong
                            && self.first_shared_promotion(*left)
                        {
                            self.zero_extend_word_at_use(left, &mut rewritten);
                        } else if single(*left)
                            && (result.ty == Type::UnsignedLongLong
                                || matches!(right.source, Source::Register(id) if facts.unsigned_pairs.contains(&id)))
                            && !matches!(right.source, Source::Register(id) if facts.widened_words.contains(&id)
                                || right.ty == Type::UnsignedLongLong && facts.unsigned_casts.contains(&id))
                        {
                            self.zero_extend_word_at_use(left, &mut rewritten);
                        } else if result.ty == Type::UnsignedLongLong
                            && matches!(left.source, Source::Constant(_))
                            && single(*right)
                        {
                            self.zero_extend_word_at_use(right, &mut rewritten);
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
                    } else if (!materializes_constant_pair(*operator, *left, *right)
                        || is_pending(*left)
                        || is_pending(*right))
                        && matches!(
                            operator,
                            BinaryOperator::Add
                                | BinaryOperator::Subtract
                                | BinaryOperator::Multiply
                                | BinaryOperator::BitAnd
                                | BinaryOperator::BitOr
                                | BinaryOperator::BitXor
                        )
                    {
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
