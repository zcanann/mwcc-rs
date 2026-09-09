//! Preserve the middle-generation signed-word subtraction quirk at its use.
use super::*;

impl Graph<'_> {
    pub(super) fn lower_word_subtrahends(&mut self, uses: &HashMap<usize, usize>) {
        if self.word_subtrahend_extension == mwcc_versions::WordSubtrahendExtension::FullWidth {
            return;
        }
        let operations = std::mem::take(&mut self.operations);
        self.operations = self.rewrite_subtrahends(operations, uses);
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
                    if self.first_shared_promotion(*right)
                        || self.single_word_promotion(*right, uses)
                    {
                        self.zero_extend_word_at_use(right, &mut rewritten);
                    }
                }
                _ => {}
            }
            rewritten.push(op);
        }
        rewritten
    }
}
