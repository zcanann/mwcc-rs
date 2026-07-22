//! Entry scheduling for dense structured frames.
//!
//! Once many incoming values must survive the first call, MWCC interleaves
//! their saved-home copies with an independent computed-address definition.
//! Keeping that schedule here lets the structured CFG owner remain concerned
//! with liveness and statement emission rather than one prologue permutation.

use super::guarded_computed_survivor::emit_scaled_index;
use super::super::assertion_expression::dense_frame_assertion_parameter;
#[allow(unused_imports)]
use super::*;

pub(super) fn structured_dense_frame_entry_index(function: &Function) -> Option<usize> {
    function
        .statements
        .iter()
        .position(|statement| matches!(statement, Statement::Assign { .. }))
}

impl Generator {
    pub(super) fn emit_structured_dense_frame_entry(
        &mut self,
        function: &Function,
        saved_parameters: &[(String, u8, u8)],
    ) -> Compilation<Option<usize>> {
        let Some(assignment_index) = structured_dense_frame_entry_index(function) else {
            return Ok(None);
        };
        let Statement::Assign { name, value } = &function.statements[assignment_index]
        else {
            unreachable!("entry index identifies an assignment")
        };
        if function.statements[..assignment_index]
            .iter()
            .any(|statement| !matches!(statement, Statement::Expression(_)))
        {
            return Ok(None);
        }
        let Some(local) = function.locals.iter().find(|local| &local.name == name) else {
            return Ok(None);
        };
        let Type::StructPointer { element_size } = local.declared_type else {
            return Ok(None);
        };
        let Expression::AddressOf { operand } = value else {
            return Ok(None);
        };
        let Expression::Index { base, index } = operand.as_ref() else {
            return Ok(None);
        };
        let (Expression::Variable(global), Expression::Variable(index_name)) =
            (base.as_ref(), index.as_ref())
        else {
            return Ok(None);
        };
        if !self.global_array_sizes.contains_key(global) {
            return Ok(None);
        }
        let Some(&(.., index_home, index_incoming)) = saved_parameters
            .iter()
            .find(|(parameter, _, _)| parameter == index_name)
        else {
            return Ok(None);
        };
        if index_incoming != Eabi::FIRST_GENERAL_ARGUMENT {
            return Ok(None);
        }
        let Some(destination) = self.locations.get(name).map(|location| location.register) else {
            return Ok(None);
        };

        if assignment_index != 0 {
            // A leading inlined assertion observes the preserved parameter set
            // before the computed local begins its lifetime. Save every entry
            // value, switch name lookup to those homes, and lower that prefix
            // before materializing the array address.
            let assertion_parameter = function.statements[..assignment_index]
                .iter()
                .find_map(|statement| match statement {
                    Statement::Expression(expression) => {
                        dense_frame_assertion_parameter(expression)
                    }
                    _ => None,
                });
            for parameter in &function.parameters {
                let Some((name, home, incoming)) = saved_parameters
                    .iter()
                    .find(|(name, _, _)| name == &parameter.name)
                else {
                    continue;
        };
                if assertion_parameter.as_deref() == Some(parameter.name.as_str()) {
                    self.output.instructions.push(Instruction::OrRecord {
                        a: *home,
                        s: *incoming,
                        b: *incoming,
                    });
                } else {
                    self.emit_callee_saved_home_copy(*home, *incoming);
                }
                self.locations
                    .get_mut(name)
                    .expect("saved parameter was eligibility checked")
                    .register = *home;
            }
            for statement in &function.statements[..assignment_index] {
                let emitted = match statement {
                    Statement::Expression(expression) => self
                        .try_emit_dense_frame_assertion(
                            expression,
                            index_home,
                            self.behavior.frame_convention == FrameConvention::LinkageFirst,
                        )?,
                    _ => false,
                };
                if emitted {
                    continue;
                }
                self.emit_statement(statement).map_err(|mut diagnostic| {
                    diagnostic
                        .message
                        .push_str(" (in dense structured entry prefix)");
                    diagnostic
                })?;
            }

            let (high_preference, scaled_preference) = match self.behavior.frame_convention {
                FrameConvention::LinkageFirst => (3, 4),
                FrameConvention::Predecrement => (4, 5),
            };
            let high = self.fresh_virtual_general_preferring(high_preference);
            let scaled = self.fresh_virtual_general_preferring(scaled_preference);
            emit_scaled_index(
                &mut self.output.instructions,
                scaled,
                index_home,
                element_size,
            )?;
            self.emit_address_high(high, global);
            self.record_relocation(RelocationKind::Addr16Lo, global);
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: high,
                immediate: 0,
            });
            self.output.instructions.push(Instruction::Add {
                d: destination,
                a: GENERAL_SCRATCH,
                b: scaled,
            });
            return Ok(Some(assignment_index + 1));
        }

        self.emit_callee_saved_home_copy(index_home, index_incoming);
        let (high_preference, scaled_preference) = match self.behavior.frame_convention {
            FrameConvention::LinkageFirst => (3, 4),
            FrameConvention::Predecrement => (8, 9),
        };
        let high = self.fresh_virtual_general_preferring(high_preference);
        let scaled = self.fresh_virtual_general_preferring(scaled_preference);
        self.emit_address_high(high, global);
        let remaining: Vec<(u8, u8)> = function
            .parameters
            .iter()
            .filter_map(|parameter| {
                saved_parameters
                    .iter()
                    .find(|(name, _, _)| name == &parameter.name && name != index_name)
                    .map(|(_, home, incoming)| (*home, *incoming))
            })
            .collect();
        if self.behavior.frame_convention == FrameConvention::Predecrement {
            emit_scaled_index(
                &mut self.output.instructions,
                scaled,
                index_incoming,
                element_size,
            )?;
        }
        if let Some(&(home, incoming)) = remaining.first() {
            self.emit_callee_saved_home_copy(home, incoming);
        }
        self.record_relocation(RelocationKind::Addr16Lo, global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: high,
            immediate: 0,
        });
        if self.behavior.frame_convention == FrameConvention::LinkageFirst {
            emit_scaled_index(
                &mut self.output.instructions,
                scaled,
                index_home,
                element_size,
            )?;
        }
        for (remaining_index, &(home, incoming)) in remaining.iter().enumerate().skip(1) {
            self.emit_callee_saved_home_copy(home, incoming);
            if self.behavior.frame_convention == FrameConvention::LinkageFirst
                && remaining_index == 1
            {
                self.output.instructions.push(Instruction::Add {
                    d: destination,
                    a: GENERAL_SCRATCH,
                    b: scaled,
                });
            }
        }
        if self.behavior.frame_convention == FrameConvention::Predecrement {
            self.output.instructions.push(Instruction::Add {
                d: destination,
                a: GENERAL_SCRATCH,
                b: scaled,
            });
        }
        Ok(Some(assignment_index + 1))
    }

    /// Move the first guarded call's channel argument into the computed-entry
    /// latency gap. The two frame generations choose opposite sides of the
    /// address-low instruction; relocation indices follow the move.
    pub(super) fn schedule_structured_prefixed_frame_entry(&mut self) {
        let Some(add) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::Add { d, .. } if *d >= mwcc_vreg::VIRTUAL_BASE)
        }) else {
            return;
        };
        let Some(copy) = self.output.instructions[add + 1..]
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::AddImmediate { d: 3, immediate: 0, .. }
                        | Instruction::Or { a: 3, .. }
                )
            })
            .map(|offset| add + 1 + offset)
        else {
            return;
        };
        let insertion = match self.behavior.frame_convention {
            FrameConvention::LinkageFirst => add,
            FrameConvention::Predecrement => add.saturating_sub(1),
        };
        if insertion >= copy {
            return;
        }
        let instruction = self.output.instructions.remove(copy);
        self.output.instructions.insert(insertion, instruction);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = match relocation.instruction_index {
                index if index == copy => insertion,
                index if (insertion..copy).contains(&index) => index + 1,
                index => index,
            };
        }
    }
}
