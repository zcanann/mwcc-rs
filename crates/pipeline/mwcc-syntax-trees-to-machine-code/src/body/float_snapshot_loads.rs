//! Reuse ordinary memory reads within a promoted floating snapshot.
//!
//! Each definition gets a distinct virtual identity before loads are shared.
//! A store invalidates all addresses: input and output parameters may alias.
//! Source facts are required positively, so untracked or volatile pointers
//! cannot acquire ordinary-memory semantics from the selected instructions.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{Expression, Function, Statement, Type};
use mwcc_vreg::{Class, RegisterRole};
use std::collections::HashMap;

impl Generator {
    pub(crate) fn reuse_float_snapshot_loads(&mut self, function: &Function) {
        if self.promoted_float_locals.is_empty()
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || function.return_expression.is_some()
            || !function
                .statements
                .iter()
                .all(|statement| snapshot_statement(statement, function))
            || function
                .locals
                .iter()
                .any(|local| local.initializer.is_some() || local.is_volatile)
        {
            return;
        }
        let ordinary_bases = self
            .nonvolatile_pointer_bindings
            .iter()
            .filter_map(|name| self.lookup_general(name))
            .collect::<Vec<_>>();
        let instructions = &self.output.instructions;
        let Some(start) = instructions.iter().position(is_float_instruction) else {
            return;
        };
        let Some(end) = instructions.iter().rposition(is_float_instruction) else {
            return;
        };
        let remaining_ids = u64::from(u32::MAX) + 1 - u64::from(mwcc_vreg::VIRTUAL_BASE);
        if u64::from(self.virtual_cursors.float) + (end - start + 1) as u64 > remaining_ids
            || !instructions[start..=end]
                .iter()
                .all(|instruction| match instruction {
                    Instruction::LoadFloatSingle { a, .. } => ordinary_bases.contains(a),
                    other => is_float_instruction(other),
                })
        {
            return;
        }
        // An assignment to an input pointer would change the meaning of its
        // original entry register. Snapshot declarations may only assign locals.
        if function.statements.iter().any(|statement| {
            matches!(statement,
            Statement::Assign { name, .. } if function.parameters.iter().any(|p| &p.name == name))
        }) {
            return;
        }
        let mut trial = self.clone();
        let mut values = HashMap::<u32, u32>::new();
        let mut loads = HashMap::<(u32, i16), u32>::new();
        let mut removed = Vec::new();
        for index in start..=end {
            let mut instruction = self.output.instructions[index].clone();
            if let Instruction::LoadFloatSingle { d, a, offset } = instruction {
                if let Some(&value) = loads.get(&(a, offset)) {
                    values.insert(d, value);
                    removed.push(index);
                    continue;
                }
            }
            let is_load = matches!(instruction, Instruction::LoadFloatSingle { .. });
            let mut definitions = Vec::new();
            mwcc_vreg::for_each_register(&mut instruction, |role, class, register| {
                if class != Class::Float {
                    return;
                }
                match role {
                    RegisterRole::Use => {
                        *register = values.get(register).copied().unwrap_or(*register);
                    }
                    RegisterRole::Define => {
                        let fresh = if is_load {
                            trial.fresh_virtual_float()
                        } else {
                            trial.fresh_virtual_float_preferring(0)
                        };
                        definitions.push((*register, fresh));
                        *register = fresh;
                    }
                }
            });
            values.extend(definitions);
            match instruction {
                Instruction::LoadFloatSingle { d, a, offset } => {
                    loads.insert((a, offset), d);
                }
                Instruction::StoreFloatSingle { .. } => loads.clear(),
                _ => {}
            }
            trial.output.instructions[index] = instruction;
        }
        if removed.is_empty() {
            return;
        }
        for index in removed.into_iter().rev() {
            crate::remove_instruction_retargeting_to_next(&mut trial, index);
        }
        *self = trial;
    }
}

fn is_float_instruction(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::LoadFloatSingle { .. }
            | Instruction::StoreFloatSingle { .. }
            | Instruction::FloatMultiplySingle { .. }
            | Instruction::FloatAddSingle { .. }
            | Instruction::FloatSubtractSingle { .. }
            | Instruction::FloatMove { .. }
            | Instruction::FloatMultiplyAddSingle { .. }
            | Instruction::FloatMultiplySubtractSingle { .. }
            | Instruction::FloatNegativeMultiplySubtractSingle { .. }
    )
}

fn snapshot_statement(statement: &Statement, function: &Function) -> bool {
    match statement {
        Statement::Store { target, value } => {
            snapshot_expression(target, function) && snapshot_expression(value, function)
        }
        Statement::Assign { value, .. } => snapshot_expression(value, function),
        Statement::Return(None) => true,
        Statement::Expression(Expression::Cast {
            target_type: Type::Void,
            operand,
        }) => {
            matches!(operand.as_ref(), Expression::IntegerLiteral(_))
        }
        _ => false,
    }
}

fn snapshot_expression(expression: &Expression, function: &Function) -> bool {
    match expression {
        Expression::Variable(name) => {
            function.parameters.iter().any(|p| &p.name == name)
                || function.locals.iter().any(|local| &local.name == name)
        }
        Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) => true,
        Expression::Binary { left, right, .. } => {
            snapshot_expression(left, function) && snapshot_expression(right, function)
        }
        Expression::Member { base, .. } => snapshot_expression(base, function),
        Expression::Index { base, index } => {
            snapshot_expression(base, function)
                && matches!(index.as_ref(), Expression::IntegerLiteral(_))
        }
        // Casts can introduce volatile pointees independently of the declaration.
        _ => false,
    }
}
