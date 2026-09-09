//! Preserve the middle-generation signed-word subtraction quirk at its use.
//! A shared promotion materializes a full pair and keeps its sign high word.
//! Group repeated promotions of one scalar before deciding which are shared.
use super::*;

fn visit_condition(condition: &Condition, visit: &mut impl FnMut(Value)) {
    match *condition {
        Condition::Value(value) => visit(value),
        Condition::Compare { left, right, .. } => {
            visit(left);
            visit(right);
        }
    }
}

fn visit_uses(operations: &[Operation], visit: &mut impl FnMut(Value)) {
    for op in operations {
        match op {
            Operation::BranchIf { condition, .. } => visit_condition(condition, visit),
            Operation::Branch {
                condition,
                then_body,
                else_body,
            } => {
                visit_condition(condition, visit);
                visit_uses(then_body, visit);
                visit_uses(else_body, visit);
            }
            Operation::Return(Some(value)) => visit(*value),
            Operation::Copy { result, value } | Operation::Convert { result, value }
                if wide(result.ty) =>
            {
                visit(*value)
            }
            Operation::Binary { left, right, .. } => {
                visit(*left);
                visit(*right);
            }
            Operation::Store { value, .. } => visit(*value),
            Operation::Call { arguments, .. } => {
                for (_, value) in arguments {
                    visit(*value);
                }
            }
            _ => {}
        }
    }
}

impl Graph<'_> {
    pub(super) fn retain_named_promotion(&mut self, value: Value) {
        if self.optimization < mwcc_versions::Optimization::O3 && wide(value.ty) {
            if let Source::Register(id) = value.source {
                self.materialized_word_promotions.insert(id);
            }
        }
    }

    pub(super) fn lower_word_subtrahends(&mut self) {
        if self.word_subtrahend_extension == mwcc_versions::WordSubtrahendExtension::FullWidth {
            return;
        }
        let mut uses = HashMap::<usize, usize>::new();
        let mut count = |value: Value| {
            if let Source::Register(id) = value.source {
                if wide(value.ty) {
                    if let Some(input) = self.signed_word_promotions.get(&id) {
                        *uses
                            .entry(if self.optimization >= mwcc_versions::Optimization::O2 {
                                *input
                            } else {
                                id
                            })
                            .or_default() += 1;
                    }
                }
            }
        };
        visit_uses(&self.operations, &mut count);
        if let Some(value) = self.returned {
            count(value);
        }
        let operations = std::mem::take(&mut self.operations);
        self.operations = self.rewrite_subtrahends(operations, &uses);
    }

    fn rewrite_subtrahends(
        &mut self,
        operations: Vec<Operation>,
        uses: &HashMap<usize, usize>,
    ) -> Vec<Operation> {
        let mut rewritten = Vec::new();
        for mut op in operations {
            match &mut op {
                Operation::Branch {
                    then_body,
                    else_body,
                    ..
                } => {
                    *then_body = self.rewrite_subtrahends(std::mem::take(then_body), uses);
                    *else_body = self.rewrite_subtrahends(std::mem::take(else_body), uses);
                }
                Operation::Binary {
                    operator: BinaryOperator::Subtract,
                    left,
                    right,
                    ..
                } if wide(left.ty)
                    && (left.ty == Type::LongLong
                        || self.word_subtrahend_extension
                            == mwcc_versions::WordSubtrahendExtension::ZeroExtendAll) =>
                {
                    let single = match right.source {
                        Source::Register(id) => {
                            !self.materialized_word_promotions.contains(&id)
                                && self.signed_word_promotions.get(&id).is_some_and(|input| {
                                    uses.get(
                                        if self.optimization >= mwcc_versions::Optimization::O2 {
                                            input
                                        } else {
                                            &id
                                        },
                                    ) == Some(&1)
                                })
                        }
                        _ => false,
                    };
                    if single {
                        let word = self.fresh(Type::UnsignedInt);
                        let pair = self.fresh(right.ty);
                        rewritten.push(Operation::Convert {
                            result: word,
                            value: *right,
                        });
                        rewritten.push(Operation::Convert {
                            result: pair,
                            value: word,
                        });
                        *right = pair;
                    }
                }
                _ => {}
            }
            rewritten.push(op);
        }
        rewritten
    }
}
