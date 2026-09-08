//! Source value homes for GC/1.1p1 O0 loops with aliased parameter images.
//!
//! Values with multiple source uses occupy descending saved registers. Single-use parameters share
//! SP+8, including overlaps with saved registers; unused parameters have no
//! image. Keep the source plan separate from the shared loop/statement emitter.

use super::structured_expression_visit::visit_expression;
use super::*;
use crate::generator::FrameSlot;
use std::collections::HashMap;

#[derive(Default)]
struct Uses {
    counts: HashMap<String, usize>,
    definitions: Vec<String>,
    has_loop: bool,
    unsupported: bool,
}

impl Uses {
    fn define(&mut self, name: &str) {
        if !self.definitions.iter().any(|n| n == name) {
            self.definitions.push(name.into());
        }
    }

    fn expression(&mut self, value: &Expression) {
        visit_expression(value, &mut |value| match value {
            Expression::Variable(name) => *self.counts.entry(name.clone()).or_default() += 1,
            Expression::Assign { target, .. } => {
                if let Expression::Variable(name) = target.as_ref() {
                    self.define(name);
                }
            }
            Expression::AddressOf { .. } => self.unsupported = true,
            _ => {}
        });
    }

    fn statements(&mut self, statements: &[Statement]) {
        for statement in statements {
            match statement {
                Statement::Assign { name, value } => {
                    self.define(name);
                    self.expression(value);
                }
                Statement::Store { target, value } => {
                    self.expression(target);
                    self.expression(value);
                }
                Statement::Expression(value) => self.expression(value),
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } => {
                    self.expression(condition);
                    self.statements(then_body);
                    self.statements(else_body);
                }
                Statement::Loop {
                    initializer,
                    condition,
                    step,
                    body,
                    ..
                } => {
                    self.has_loop = true;
                    if let Some(value) = initializer {
                        self.expression(value);
                    }
                    for value in [condition, step].into_iter().flatten() {
                        self.expression(value);
                    }
                    self.statements(body);
                }
                _ => self.unsupported = true,
            }
        }
    }
}

fn word(ty: Type) -> bool {
    matches!(
        ty,
        Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
    )
}

impl Generator {
    pub(crate) fn try_shared_spill_loops(&mut self, function: &Function) -> Compilation<bool> {
        if !self.behavior.unoptimized_shared_parameter_spills
            || function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || self.data_section_anchor.is_some()
            || !function_makes_call(function)
            || !function.inline_asm_blocks.is_empty()
            || function.asm_body.is_some()
            || function.parameters.len() > 8
            || function.parameters.iter().any(|p| !word(p.parameter_type))
            || function.locals.iter().any(|l| {
                !word(l.declared_type) || l.is_static || l.is_volatile || l.array_length.is_some()
            })
            || function_calls_any(function, &self.skipped_inline_names)
        {
            return Ok(false);
        }
        let mut uses = Uses::default();
        for local in &function.locals {
            if let Some(value) = &local.initializer {
                uses.define(&local.name);
                uses.expression(value);
            }
        }
        uses.statements(&function.statements);
        if uses.unsupported || !uses.has_loop {
            return Ok(false);
        }
        let source_counts = self
            .source_variable_reference_counts
            .as_ref()
            .filter(|counts| {
                counts.len() == function.locals.len() + function.parameters.len()
                    && function.locals.iter().all(|l| counts.contains_key(&l.name))
                    && function
                        .parameters
                        .iter()
                        .all(|p| counts.contains_key(&p.name))
            });
        let counts = source_counts.unwrap_or(&uses.counts);
        let count = |name: &str| counts.get(name).copied().unwrap_or(0);
        let locals: Vec<_> = uses
            .definitions
            .iter()
            .filter_map(|name| {
                function
                    .locals
                    .iter()
                    .find(|l| l.name == *name && count(name) != 0)
            })
            .collect();
        if locals.iter().any(|l| count(&l.name) < 2) {
            return Ok(false);
        }
        let retained: Vec<_> = function
            .parameters
            .iter()
            .filter(|p| count(&p.name) > 1)
            .collect();
        let spilled: Vec<_> = function
            .parameters
            .iter()
            .filter(|p| count(&p.name) == 1)
            .collect();
        let homes = locals.len() + retained.len();
        if spilled.is_empty() || homes == 0 || homes > 18 {
            return Ok(false);
        }
        let Some(lowered) = super::structured_loop_lowering::lower_unoptimized_structured_loops(
            function,
            &self.global_array_sizes,
        ) else {
            return Ok(false);
        };
        let saved: Vec<_> = (0..homes).map(|i| 31 - i as u8).collect();
        let mut ranked_names: Vec<_> = locals.iter().map(|l| l.name.as_str()).collect();
        if source_counts.is_some() {
            // Stable ties retain local first-definition order, followed by
            // parameters in reverse declaration order. All compete by source
            // frequency, including parameters with more uses than the locals.
            ranked_names.extend(retained.iter().rev().map(|p| p.name.as_str()));
            ranked_names.sort_by_key(|name| std::cmp::Reverse(count(name)));
        } else {
            ranked_names.extend(retained.iter().map(|p| p.name.as_str()));
        }
        self.structured_loop_carried_names
            .extend(ranked_names.iter().map(|n| (*n).to_owned()));
        let home_of = |name: &str| saved[ranked_names.iter().position(|n| *n == name).unwrap()];
        let first = *saved.last().unwrap();
        let helpers = homes >= 3 && !self.behavior.use_lmw_stmw;
        let multiple = homes >= 2 && self.behavior.use_lmw_stmw;
        if helpers {
            let frame = (self.outgoing_general_parameter_end + 4 * homes as i16 + 7) & !7;
            self.emit_savegpr_frame_prologue_with_convention(
                first,
                frame,
                FrameConvention::LinkageFirst,
            );
        } else {
            self.emit_linkage_first_nonleaf_prologue(&saved);
            if multiple {
                self.output.instructions.truncate(3);
                self.output
                    .instructions
                    .push(Instruction::StoreMultipleWord {
                        s: first,
                        a: 1,
                        offset: self.frame_size - 4 * homes as i16,
                    });
            }
        }
        for local in &locals {
            let ty = local.declared_type;
            self.locations.insert(
                local.name.clone(),
                Location {
                    class: ValueClass::General,
                    register: home_of(&local.name),
                    signed: self.signed_of(ty),
                    width: ty.width(),
                    pointee: match ty {
                        Type::Pointer(p) => Some(p),
                        _ => None,
                    },
                    stride: pointer_stride(ty),
                },
            );
        }
        for parameter in &function.parameters {
            let incoming = self
                .locations
                .get(&parameter.name)
                .expect("incoming parameter")
                .register;
            if retained.iter().any(|p| p.name == parameter.name) {
                let home = home_of(&parameter.name);
                self.output
                    .instructions
                    .push(Instruction::move_register(home, incoming));
                self.locations.get_mut(&parameter.name).unwrap().register = home;
            } else if spilled.iter().any(|p| p.name == parameter.name) {
                self.output.instructions.push(Instruction::StoreWord {
                    s: incoming,
                    a: 1,
                    offset: 8,
                });
                self.locations.remove(&parameter.name);
                self.frame_slots.insert(
                    parameter.name.clone(),
                    FrameSlot {
                        offset: 8,
                        class: ValueClass::General,
                        size: 4,
                        value_type: parameter.parameter_type,
                        parameter_register: Some(incoming),
                        is_array: false,
                    },
                );
            }
        }
        for local in &function.locals {
            if let Some(value) = &local.initializer {
                let Some(home) = self.lookup_general(&local.name) else {
                    return Ok(false);
                };
                self.evaluate(value, local.declared_type, home)?;
            }
        }
        let mut returns = Vec::new();
        let mut labels = HashMap::new();
        let mut gotos = Vec::new();
        self.emit_structured_statements(
            &lowered.statements,
            &lowered,
            &[],
            false,
            &mut returns,
            &mut labels,
            &mut gotos,
            &mut None,
        )?;
        for (at, label) in gotos {
            let target = *labels
                .get(&label)
                .ok_or_else(|| Diagnostic::error("shared spill loop label is missing"))?;
            if let Instruction::Branch {
                target: destination,
            } = &mut self.output.instructions[at]
            {
                *destination = target;
            }
        }
        self.schedule_shared_spill_store_literals();
        if helpers {
            self.emit_restgpr_frame_epilogue_with_convention(first, FrameConvention::LinkageFirst);
        } else {
            let end = self.output.instructions.len();
            self.emit_epilogue_and_return();
            if multiple {
                self.output.instructions[end] = Instruction::LoadMultipleWord {
                    d: first,
                    a: 1,
                    offset: self.frame_size - 4 * homes as i16,
                };
                for _ in 1..homes {
                    crate::remove_instruction_retargeting_to_next(self, end + 1);
                }
            }
        }
        Ok(true)
    }
    fn schedule_shared_spill_store_literals(&mut self) {
        for at in 0..self.output.instructions.len().saturating_sub(2) {
            if !matches!(&self.output.instructions[at..at + 3], [
                Instruction::LoadWord { d: address, a: 1, offset: 8 },
                Instruction::AddImmediate { d: 0, a: 0, .. },
                Instruction::StoreWord { s: 0, a: base, .. }
                    | Instruction::StoreHalfword { s: 0, a: base, .. }
                    | Instruction::StoreByte { s: 0, a: base, .. },
            ] if *address != 0 && address == base)
                || self.output.relocations.iter().any(|r| r.instruction_index == at + 1)
                || self.output.deferred_displacements.iter().any(|r| r.instruction_index == at + 1)
                || self.output.instructions.iter().any(|i| matches!(i,
                    Instruction::Branch { target } | Instruction::BranchConditionalForward { target, .. }
                        if (at + 1..at + 3).contains(target)))
            { continue; }
            crate::move_instruction_before_retargeting(self, at + 1, at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_execution_frequency_does_not_promote_a_single_source_use() {
        let mut uses = Uses::default();
        uses.statements(&[Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::Variable("running".into())),
            step: None,
            body: vec![Statement::Expression(Expression::Variable(
                "pointer".into(),
            ))],
        }]);
        assert_eq!(uses.counts["pointer"], 1);
        assert_eq!(uses.counts["running"], 1);
        uses.expression(&Expression::Variable("pointer".into()));
        assert_eq!(uses.counts["pointer"], 2);
    }
}
