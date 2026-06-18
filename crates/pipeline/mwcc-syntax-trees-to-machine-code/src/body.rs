//! Function-level emission: parameters, body, guards, and the return tail.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, GuardedReturn, LocalDeclaration, Statement, Type};
use mwcc_target::Eabi;
use crate::analysis::*;
use crate::generator::*;

impl Generator {

    pub(crate) fn assign_parameters(&mut self, function: &Function) -> Compilation<()> {
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut next_float = Eabi::FIRST_FLOAT_ARGUMENT;
        for parameter in &function.parameters {
            let class = class_of(parameter.parameter_type)?;
            let register = match class {
                ValueClass::General => {
                    let register = next_general;
                    next_general += 1;
                    register
                }
                ValueClass::Float => {
                    let register = next_float;
                    next_float += 1;
                    register
                }
            };
            let signed = self.signed_of(parameter.parameter_type);
            let pointee = match parameter.parameter_type {
                Type::Pointer(pointee) => Some(pointee),
                _ => None,
            };
            self.locations.insert(
                parameter.name.clone(),
                Location { class, register, signed, width: parameter.parameter_type.width(), pointee },
            );
        }
        Ok(())
    }

    /// Emit the whole function body, including its `blr`(s).
    pub(crate) fn evaluate_body(&mut self, function: &Function) -> Compilation<()> {
        // Value-tracked locals (reassignment, multiple locals) are inlined into the
        // return expression and compiled there; this takes over the whole body when
        // it applies, leaving the straight-line paths below byte-identical.
        if self.try_value_tracking(function)? {
            return Ok(());
        }
        // A function whose body is a single `switch` lowers to the dispatch tree:
        // the comparisons, then the case bodies, then the default (the `default:`
        // arm if present, else the function's trailing `return`). The cases and
        // default each end in their own `blr`, so this owns the whole body.
        if let [Statement::Switch { scrutinee, arms, default }] = function.statements.as_slice() {
            if function.guards.is_empty() && function.locals.is_empty() && !function_makes_call(function) {
                let default_expression = default
                    .as_ref()
                    .or(function.return_expression.as_ref())
                    .ok_or_else(|| Diagnostic::error("a switch with no default needs a trailing return"))?;
                let result = match function.return_type {
                    Type::Float | Type::Double => return Err(Diagnostic::error("a floating-point switch result is not supported yet (roadmap)")),
                    Type::Void => return Err(Diagnostic::error("a void switch is not supported yet (roadmap)")),
                    _ => Eabi::general_result().number,
                };
                return self.emit_switch(scrutinee, arms, default_expression, default.is_some(), function.return_type, result);
            }
        }
        // A function that calls is non-leaf: save the link register around a 16-byte
        // frame before doing anything else.
        if function_makes_call(function) {
            if !function.guards.is_empty() {
                return Err(Diagnostic::error("calls combined with guards not yet supported"));
            }
            self.non_leaf = true;
            self.frame_size = 16;
            self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
            self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
            self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        }

        // Body statements (stores, calls) run first.
        for statement in &function.statements {
            self.emit_statement(statement)?;
        }

        // A `void` function ends after its statements.
        if function.return_type == Type::Void {
            self.emit_epilogue_and_return();
            return Ok(());
        }

        let result = match function.return_type {
            Type::Float | Type::Double => Eabi::float_result().number,
            _ => Eabi::general_result().number,
        };
        let return_expression = function
            .return_expression
            .as_ref()
            .ok_or_else(|| Diagnostic::error("a non-void function needs a return value"))?;

        if !function.guards.is_empty() {
            if !function.locals.is_empty() {
                return Err(Diagnostic::error("locals combined with guards not yet supported"));
            }
            // mwcc lowers a single guard as a select (working-register form) but a
            // chain of guards as separate return blocks.
            if let [guard] = function.guards.as_slice() {
                let select = Expression::Conditional {
                    condition: Box::new(guard.condition.clone()),
                    when_true: Box::new(guard.value.clone()),
                    when_false: Box::new(return_expression.clone()),
                };
                self.evaluate_tail(&select, function.return_type, result)?;
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                return Ok(());
            }
            return self.emit_guard_sequence(&function.guards, return_expression, function.return_type, result);
        }

        match function.locals.as_slice() {
            [] => self.evaluate_tail(return_expression, function.return_type, result)?,
            [local] => self.evaluate_single_local(local, return_expression, function.return_type, result)?,
            _ => return Err(Diagnostic::error("multiple locals need the full register allocator (roadmap M1)")),
        }
        self.emit_epilogue_and_return();
        Ok(())
    }

    /// Tear down the stack frame (if one was allocated) and return. A non-leaf
    /// function restores the link register from `frame_size + 4` first.
    pub(crate) fn emit_epilogue_and_return(&mut self) {
        if self.non_leaf {
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
            self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        }
        if self.frame_size != 0 {
            self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
    }

    /// Emit a body statement.
    pub(crate) fn emit_statement(&mut self, statement: &Statement) -> Compilation<()> {
        match statement {
            Statement::Store { target, value } => self.emit_store(target, value),
            Statement::Expression(Expression::Call { name, arguments }) => {
                self.emit_call(name, arguments, None, false)
            }
            Statement::Expression(_) => Err(Diagnostic::error("only a call may be a bare statement (roadmap)")),
            // Reassignment is handled by value tracking; reaching here means it was
            // mixed with stores/calls, which that path defers.
            Statement::Assign { .. } => Err(Diagnostic::error("local reassignment mixed with stores/calls is not supported yet (roadmap)")),
            // The binary-search dispatch codegen is the next piece; switches parse
            // but defer for now (never miscompile).
            Statement::Switch { .. } => Err(Diagnostic::error("switch dispatch codegen is not implemented yet (roadmap)")),
        }
    }

    /// Emit a sequence of `if (c) return v;` guards followed by the final return.
    /// Each guard is its own block ending in `blr`; the last guard collapses the
    /// final return into a conditional return when the final value already sits in
    /// the result register.
    pub(crate) fn emit_guard_sequence(
        &mut self,
        guards: &[GuardedReturn],
        final_return: &Expression,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        let final_in_result = match final_return {
            Expression::Variable(name) => self.locations.get(name).map(|location| location.register) == Some(result),
            _ => false,
        };

        for (index, guard) in guards.iter().enumerate() {
            let (options, condition_bit) = self.emit_condition_test(&guard.condition)?;
            let value_register = self.general_register_of_leaf(&guard.value)?;
            let is_last = index + 1 == guards.len();

            if is_last && final_in_result {
                // false path returns the final value already in the result register
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
                if result != value_register {
                    self.output.instructions.push(Instruction::move_register(result, value_register));
                }
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                return Ok(());
            }

            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            if result != value_register {
                self.output.instructions.push(Instruction::move_register(result, value_register));
            }
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            let next = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = next;
            }
        }

        // Final fall-through return.
        self.evaluate_tail(final_return, return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(())
    }

    /// Evaluate the function result. A conditional in this tail position can use a
    /// conditional return when one of its values already sits in the result register.
    pub(crate) fn evaluate_tail(&mut self, expression: &Expression, value_type: Type, result: u8) -> Compilation<()> {
        match expression {
            Expression::Conditional { condition, when_true, when_false } => match value_type {
                Type::Float | Type::Double => self.emit_float_conditional(condition, when_true, when_false, result, true),
                _ => self.emit_conditional(condition, when_true, when_false, result, true),
            },
            Expression::Binary { operator: operator @ (BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr), left, right } => {
                self.emit_short_circuit(*operator, left, right, result)
            }
            // A narrow return type truncates the returned value. A `(type)` cast
            // expression already yields the narrow type, so it falls through to the
            // normal path; everything else is coerced here.
            other if is_narrow_int(value_type) && !matches!(other, Expression::Cast { .. }) => {
                self.evaluate_narrow_return(other, value_type, result)
            }
            other => self.evaluate(other, value_type, result),
        }
    }

    pub(crate) fn evaluate_single_local(
        &mut self,
        local: &LocalDeclaration,
        return_expression: &Expression,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        let class = class_of(local.declared_type)?;
        // The single-local straight-line path needs the local's initializer; an
        // uninitialized local (its value comes from an assignment) is value-tracked.
        let initializer = local
            .initializer
            .as_ref()
            .ok_or_else(|| Diagnostic::error("an uninitialized single local is not supported here (roadmap)"))?;

        // `return x;` — the local is the result, so compute its initializer
        // straight into the result register.
        if matches!(return_expression, Expression::Variable(name) if *name == local.name) {
            return self.evaluate(initializer, local.declared_type, result);
        }

        // Otherwise the local lives in the scratch register and is used as a leaf.
        // That only works if the result expression does not itself need the scratch.
        if needs_scratch(return_expression) {
            return Err(Diagnostic::error("local reused inside a scratch-needing expression (roadmap M1)"));
        }
        let scratch = match class {
            ValueClass::General => GENERAL_SCRATCH,
            ValueClass::Float => FLOAT_SCRATCH,
        };
        self.evaluate(initializer, local.declared_type, scratch)?;
        let signed = self.signed_of(local.declared_type);
        let pointee = match local.declared_type {
            Type::Pointer(pointee) => Some(pointee),
            _ => None,
        };
        self.locations.insert(local.name.clone(), Location { class, register: scratch, signed, width: local.declared_type.width(), pointee });
        self.evaluate(return_expression, return_type, result)
    }

    pub(crate) fn evaluate(&mut self, expression: &Expression, value_type: Type, destination: u8) -> Compilation<()> {
        match value_type {
            Type::Float => self.evaluate_float(expression, destination),
            // A `double` value shares the FPR file with `float`; the float path
            // picks the double-precision instructions via is_double_value.
            Type::Double => self.evaluate_float(expression, destination),
            Type::Void => Err(Diagnostic::error("cannot evaluate a void expression")),
            _ => self.evaluate_general(expression, destination),
        }
    }
}
