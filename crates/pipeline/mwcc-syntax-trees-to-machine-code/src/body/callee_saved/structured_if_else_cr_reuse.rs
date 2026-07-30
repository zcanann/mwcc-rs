//! CR reuse across a nested three-way integer classification.
//!
//! A source `if (a == b) 0; else { 1; if (a < b) -1; }` needs only one
//! comparison. The equality branch consumes CR0 first; the less-than branch in
//! the else arm can consume the same ordered comparison after a non-recording
//! `li`.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn try_emit_structured_if_else_cr_reuse(
        &mut self,
        condition: &Expression,
        then_body: &[Statement],
        else_body: &[Statement],
    ) -> Compilation<bool> {
        let Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        } = condition
        else {
            return Ok(false);
        };
        if as_member(left).is_none() || as_member(right).is_none() {
            return Ok(false);
        }
        let [Statement::Assign {
            name: then_name,
            value: then_value,
        }] = then_body
        else {
            return Ok(false);
        };
        let [Statement::Assign {
            name: else_name,
            value: else_value,
        }, Statement::If {
            condition: nested_condition,
            then_body: nested_then,
            else_body: nested_else,
        }] = else_body
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: nested_operator,
            left: nested_left,
            right: nested_right,
        } = nested_condition
        else {
            return Ok(false);
        };
        let [Statement::Assign {
            name: nested_name,
            value: nested_value,
        }] = nested_then.as_slice()
        else {
            return Ok(false);
        };
        if !nested_else.is_empty()
            || then_name != else_name
            || then_name != nested_name
            || constant_value(then_value) != Some(0)
            || constant_value(else_value) != Some(1)
            || constant_value(nested_value) != Some(-1)
            || !matches!(
                nested_operator,
                BinaryOperator::Less
                    | BinaryOperator::LessEqual
                    | BinaryOperator::Greater
                    | BinaryOperator::GreaterEqual
            )
            || !structurally_equal(left, nested_left)
            || !structurally_equal(right, nested_right)
        {
            return Ok(false);
        }
        let Some(destination) = self.lookup_general(then_name) else {
            return Ok(false);
        };

        let (outer_options, outer_condition_bit) = self.emit_condition_test(condition)?;
        let enter_else = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: outer_options,
                condition_bit: outer_condition_bit,
                target: 0,
            });
        self.load_integer_constant(destination, 0);
        let skip_else = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });

        self.patch_forward(enter_else, self.output.instructions.len());
        self.load_integer_constant(destination, 1);
        let (nested_options, nested_condition_bit) =
            false_branch_bo_bi(*nested_operator).expect("the nested operator is a comparison");
        let skip_nested = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: nested_options,
                condition_bit: nested_condition_bit,
                target: 0,
            });
        self.load_integer_constant(destination, -1);

        let join = self.output.instructions.len();
        self.patch_forward(skip_nested, join);
        if let Instruction::Branch { target } = &mut self.output.instructions[skip_else] {
            *target = join;
        }
        Ok(true)
    }
}
