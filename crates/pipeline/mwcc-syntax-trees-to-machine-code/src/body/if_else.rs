//! Leaf if/else DIAMOND codegen — both arms store, the function returns a value.
//!
//! Split out of the `evaluate_body` dispatch (fire 623) so the general-control-flow (#21)
//! if/else slices have a cohesive home to grow in. The non-leaf if/else join and the
//! if-no-else prologue still live inline in driver.rs for now (their prologue scheduling
//! is entangled with the surrounding dispatch).

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// A LEAF if/else diamond with a return continuation: both arms are pure stores, no
    /// call/locals/guards, an int/unsigned return. Two byte-exact forms:
    ///
    /// - **JOIN** (a MATERIALIZED return — a small constant `li`, a parameter not already in
    ///   r3 `mr`, or a `param ± const` `addi`): the arms merge and the return materializes
    ///   once at the join — `<cond>; b<!c> else; <then>; b join; else: <else>; join:
    ///   <return into r3>; blr`. The else + join labels advance mwcc's `@N` counter by 2.
    /// - **TWO-EXIT** (the return value is ALREADY in r3, `return <cond var>`, and the
    ///   store arms leave it intact): each arm stores then returns directly, no shared join —
    ///   `<cond>; b<!c> else; <then>; blr; else: <else>; blr`. Local branch labels do not
    ///   advance `@N` (like the void two-store diamond).
    pub(crate) fn try_leaf_ifelse_diamond(&mut self, function: &Function) -> Compilation<bool> {
        let [Statement::If {
            condition,
            then_body,
            else_body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if function_makes_call(function)
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
            || then_body.is_empty()
            || else_body.is_empty()
            || !then_body
                .iter()
                .chain(else_body)
                .all(|statement| matches!(statement, Statement::Store { .. }))
        {
            return Ok(false);
        }
        let result = mwcc_target::Eabi::general_result().number;
        let return_expression = function.return_expression.as_ref();
        let already_in_result = matches!(return_expression, Some(Expression::Variable(name)) if self.lookup_general(name) == Some(result));
        // TWO-EXIT form: the return value is ALREADY in r3 (`return <cond var>`) and the
        // store arms (materializing through r0) leave it intact — so each arm stores then
        // returns directly, no shared join.
        if already_in_result
            && then_body.iter().chain(else_body).all(|statement| {
                matches!(
                    statement,
                    Statement::Store {
                        value: Expression::IntegerLiteral(_),
                        ..
                    } | Statement::Store {
                        value: Expression::Variable(_),
                        ..
                    }
                )
            })
        {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            let branch_to_else = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    target: 0,
                });
            for statement in then_body {
                self.emit_statement(statement)?;
            }
            self.emit_epilogue_and_return();
            let else_label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[branch_to_else]
            {
                *target = else_label;
            }
            for statement in else_body {
                self.emit_statement(statement)?;
            }
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        // JOIN form: a materialized return (not already in r3) is emitted once at the merge.
        let materialized_join = !already_in_result
            && return_expression.is_some_and(|expression| {
                constant_value(expression).is_some_and(|value| i16::try_from(value).is_ok())
                    || matches!(expression, Expression::Variable(name) if self.lookup_general(name).is_some())
                    || matches!(expression, Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right }
                        if matches!(left.as_ref(), Expression::Variable(_)) && matches!(right.as_ref(), Expression::IntegerLiteral(_)))
            });
        if materialized_join {
            // The else branch and the join both advance mwcc's anonymous-`@N` counter.
            self.output.anonymous_label_bump = 2;
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            let branch_to_else = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    target: 0,
                });
            for statement in then_body {
                self.emit_statement(statement)?;
            }
            let branch_to_join = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::Branch { target: 0 });
            let else_label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[branch_to_else]
            {
                *target = else_label;
            }
            for statement in else_body {
                self.emit_statement(statement)?;
            }
            let join_label = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[branch_to_join] {
                *target = join_label;
            }
            self.evaluate_tail(
                return_expression.expect("materialized_join implies Some"),
                function.return_type,
                result,
            )?;
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        Ok(false)
    }
}
