//! Shared promotion identities and use counts for version-specific pair quirks.
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
    pub(super) fn retain_named_value(&mut self, value: Value) {
        if let Source::Register(id) = value.source {
            self.named_values.insert(id);
            if wide(value.ty) && self.optimization < mwcc_versions::Optimization::O3 {
                self.materialized_word_promotions.insert(id);
            }
        }
    }

    pub(super) fn lower_word_promotions(&mut self) {
        if !self.computed_unsigned_addend_zero_extends
            && self.word_subtrahend_extension == mwcc_versions::WordSubtrahendExtension::FullWidth
        {
            return;
        }
        self.shared_promotions = self.find_shared_word_promotions();
        self.find_nested_word_promotions();
        // Count the original graph once: rewriting one use must not make a
        // promotion shared by addition and subtraction appear single-use.
        let mut uses = HashMap::<usize, usize>::new();
        let mut count = |value: Value| {
            if let Source::Register(id) = value.source {
                if wide(value.ty) {
                    if let Some(key) = self.word_promotion_key(id) {
                        *uses.entry(key).or_default() += 1;
                    }
                }
            }
        };
        visit_uses(&self.operations, &mut count);
        if let Some(value) = self.returned {
            count(value);
        }
        if self.computed_unsigned_addend_zero_extends {
            self.lower_word_addends(&uses);
        }
        self.lower_word_subtrahends(&uses);
    }

    fn word_promotion_key(&self, id: usize) -> Option<usize> {
        let input = *self.signed_word_promotions.get(&id)?;
        Some(
            if self.optimization >= mwcc_versions::Optimization::O2
                && !(self.computed_unsigned_addend_zero_extends
                    && self.promotion_sites.explicit_word_cast(id))
            {
                input
            } else {
                id
            },
        )
    }

    pub(super) fn single_word_promotion(&self, value: Value, uses: &HashMap<usize, usize>) -> bool {
        match value.source {
            Source::Register(id) => {
                !self.shared_promotions.full.contains(&id)
                    && !self.materialized_word_promotions.contains(&id)
                    && self
                        .word_promotion_key(id)
                        .is_some_and(|key| uses.get(&key) == Some(&1))
            }
            _ => false,
        }
    }

    pub(super) fn first_shared_promotion(&self, value: Value) -> bool {
        matches!(value.source, Source::Register(id) if self.shared_promotions.zero.contains(&id))
    }

    pub(super) fn zero_extend_word_at_use(
        &mut self,
        value: &mut Value,
        operations: &mut Vec<Operation>,
    ) {
        let word = self.fresh(Type::UnsignedInt);
        let pair = self.fresh(value.ty);
        operations.push(Operation::Convert {
            result: word,
            value: *value,
        });
        operations.push(Operation::Convert {
            result: pair,
            value: word,
        });
        *value = pair;
    }
}
