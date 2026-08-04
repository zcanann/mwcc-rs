//! Branch-preserving selects whose value arms own inline-composed effects.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Emit a value diamond when either arm begins with comma-sequenced setup.
    ///
    /// Retained inline helpers use these prefixes for hygienic local
    /// initialization and branch-local calculation. They must remain inside
    /// the selected edge, so ordinary leaf/computed phi selection cannot peel
    /// or speculate them. This form is limited to non-tail value positions;
    /// tail-return scheduling has distinct MWCC layouts.
    pub(super) fn try_emit_composed_effect_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
    ) -> Compilation<bool> {
        if tail
            || (!matches!(when_true, Expression::Comma { .. })
                && !matches!(when_false, Expression::Comma { .. }))
        {
            return Ok(false);
        }

        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let false_arm = self.fresh_label();
        let join = self.fresh_label();
        self.emit_branch_conditional_to(options, condition_bit, false_arm);
        self.emit_composed_effect_arm(when_true, destination)?;
        self.emit_branch_to(join);
        self.bind_label(false_arm);
        self.emit_composed_effect_arm(when_false, destination)?;
        self.bind_label(join);
        Ok(true)
    }

    fn emit_composed_effect_arm(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        let mut value = expression;
        while let Expression::Comma { left, right } = value {
            self.emit_comma_side_effect(left)?;
            value = right;
        }
        self.evaluate_general(value, destination)
    }
}
