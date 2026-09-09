//! Source-equivalent word operands in an adjacent pair of wide operations.
//! GC/1.3 O1 drops both sign words; O2+ shares their full conversion instead.
//! Keep this separate from cross-statement sharing and from value-number CSE:
//! a word cast changes source equivalence even when it emits no instruction.
use super::*;

#[derive(Clone, PartialEq)]
pub(super) enum Shape {
    Register(usize),
    Constant(i64),
    Cast(Type, Box<Shape>),
    Unary(UnaryOperator, Box<Shape>),
    Binary(BinaryOperator, Box<Shape>, Box<Shape>),
}

impl Graph<'_> {
    pub(super) fn promotion_shape(&self, expression: &Expression) -> Option<Shape> {
        if !self.computed_unsigned_addend_zero_extends {
            return None;
        }
        Some(match expression {
            Expression::Variable(name) => {
                let value = self.bindings.get(name)?;
                if wide(value.ty) {
                    return None;
                }
                match value.source {
                    Source::Register(id) => Shape::Register(id),
                    Source::Constant(n) => Shape::Constant(n as i64),
                }
            }
            Expression::IntegerLiteral(n) => Shape::Constant(*n),
            Expression::Cast {
                target_type: Type::UnsignedLongLong,
                operand,
            } => return self.promotion_shape(operand),
            Expression::Cast {
                target_type,
                operand,
            } => Shape::Cast(*target_type, Box::new(self.promotion_shape(operand)?)),
            Expression::Unary { operator, operand } => {
                Shape::Unary(*operator, Box::new(self.promotion_shape(operand)?))
            }
            Expression::Binary {
                operator,
                left,
                right,
            } => {
                let left = self.promotion_shape(left)?;
                let right = self.promotion_shape(right)?;
                match (operator, &left, &right) {
                    (BinaryOperator::Add | BinaryOperator::Subtract, _, Shape::Constant(0)) => left,
                    (BinaryOperator::Add, Shape::Constant(0), _) => right,
                    _ => Shape::Binary(*operator, Box::new(left), Box::new(right)),
                }
            }
            _ => return None,
        })
    }

    pub(super) fn record_promotion_shape(&mut self, value: Value, shape: Option<Shape>) {
        if let (Source::Register(id), Some(shape)) = (value.source, shape) {
            if self.signed_word_promotions.contains_key(&id) {
                self.promotion_shapes.insert(id, shape);
            }
        }
    }

    pub(super) fn find_nested_word_promotions(&mut self) {
        if !self.computed_unsigned_addend_zero_extends
            || self.optimization == mwcc_versions::Optimization::O0
        {
            return;
        }
        let mut pairs = Vec::new();
        self.nested_pairs(&self.operations, &mut HashMap::new(), &mut pairs);
        for (a, b) in pairs {
            for id in [a, b] {
                if self.optimization == mwcc_versions::Optimization::O1
                    && !matches!(
                        self.promotion_shapes.get(&id),
                        Some(Shape::Cast(Type::LongLong, _))
                    )
                {
                    self.shared_promotions.full.remove(&id);
                    self.shared_promotions.zero.insert(id);
                } else {
                    self.shared_promotions.zero.remove(&id);
                    self.shared_promotions.full.insert(id);
                }
            }
        }
    }

    fn nested_pairs(
        &self,
        operations: &[Operation],
        additions: &mut HashMap<usize, (Value, Value)>,
        pairs: &mut Vec<(usize, usize)>,
    ) {
        for op in operations {
            match op {
                Operation::Binary {
                    result,
                    operator,
                    left,
                    right,
                    ..
                } if result.ty == Type::UnsignedLongLong
                    && matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) =>
                {
                    for (side, (base, word)) in
                        [(*left, *right), (*right, *left)].into_iter().enumerate()
                    {
                        // The O1 rewrite recognizes an add/subtract update of
                        // the prior sum. `word - sum` is a different operation.
                        if *operator == BinaryOperator::Subtract
                            && side == 1
                            && self.optimization == mwcc_versions::Optimization::O1
                        {
                            continue;
                        }
                        let (Source::Register(base), Source::Register(word)) =
                            (base.source, word.source)
                        else {
                            continue;
                        };
                        if self.named_values.contains(&base) {
                            continue;
                        }
                        let Some(shape) = self.promotion_shapes.get(&word) else {
                            continue;
                        };
                        if let Some((a, b)) = additions.get(&base) {
                            for v in [a, b] {
                                if let Source::Register(id) = v.source {
                                    if self.promotion_shapes.get(&id) == Some(shape) {
                                        pairs.push((id, word));
                                    }
                                }
                            }
                        }
                    }
                    if *operator == BinaryOperator::Add {
                        if let Source::Register(id) = result.source {
                            additions.insert(id, (*left, *right));
                        }
                    }
                }
                Operation::Branch {
                    then_body,
                    else_body,
                    ..
                } => {
                    self.nested_pairs(then_body, &mut HashMap::new(), pairs);
                    self.nested_pairs(else_body, &mut HashMap::new(), pairs);
                    additions.clear();
                }
                Operation::Label(_)
                | Operation::Jump(_)
                | Operation::BranchIf { .. }
                | Operation::Return(_)
                | Operation::Store { .. }
                | Operation::Call { .. } => additions.clear(),
                _ => {}
            }
        }
    }
}
