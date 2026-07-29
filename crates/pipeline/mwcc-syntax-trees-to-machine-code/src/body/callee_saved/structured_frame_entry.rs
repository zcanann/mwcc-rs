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
    let assignment = function
        .statements
        .iter()
        .position(|statement| matches!(statement, Statement::Assign { .. }))?;
    function.statements[..assignment]
        .iter()
        .all(|statement| matches!(statement, Statement::Expression(_)))
        .then_some(assignment)
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
    /// address-low instruction; every instruction-index owner follows the move.
    pub(super) fn schedule_structured_prefixed_frame_entry(&mut self) {
        let Some((copy, insertion)) = prefixed_frame_entry_move(
            &self.output.instructions,
            self.behavior.frame_convention,
        ) else {
            return;
        };
        let old_len = self.output.instructions.len();
        let instruction = self.output.instructions.remove(copy);
        self.output.instructions.insert(insertion, instruction);
        self.labels.moved_before(copy, insertion);
        let permutation: Vec<usize> = (0..old_len)
            .map(|old| {
                if old == copy {
                    insertion
                } else if (insertion..copy).contains(&old) {
                    old + 1
                } else {
                    old
                }
            })
            .collect();
        crate::remap_instruction_indices(self, &permutation);
    }
}

fn prefixed_frame_entry_move(
    instructions: &[Instruction],
    convention: FrameConvention,
) -> Option<(usize, usize)> {
    let add = instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::Add { d, .. } if *d >= mwcc_vreg::VIRTUAL_BASE)
    })?;
    // This schedule belongs to the first call fed by the computed entry. An
    // unbounded search can capture an unrelated r3 copy from a later call and
    // hoist it across calls and control-flow joins.
    let call = instructions[add + 1..]
        .iter()
        .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        .map(|offset| add + 1 + offset)?;
    let copy = instructions[add + 1..call]
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                Instruction::AddImmediate {
                    d: 3,
                    immediate: 0,
                    ..
                } | Instruction::Or { a: 3, .. }
            )
        })
        .map(|offset| add + 1 + offset)?;
    let insertion = match convention {
        FrameConvention::LinkageFirst => add,
        FrameConvention::Predecrement => add.saturating_sub(1),
    };
    (insertion < copy).then_some((copy, insertion))
}

#[cfg(test)]
mod tests {
    use super::{prefixed_frame_entry_move, structured_dense_frame_entry_index};
    use mwcc_machine_code::Instruction;
    use mwcc_syntax_trees::{Expression, Function, Statement, Type};
    use mwcc_versions::FrameConvention;

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "caller".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements,
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    fn assignment() -> Statement {
        Statement::Assign {
            name: "entry".into(),
            value: Expression::IntegerLiteral(0),
        }
    }

    #[test]
    fn dense_entry_prefix_accepts_only_expression_statements() {
        let expression_prefix = function(vec![
            Statement::Expression(Expression::IntegerLiteral(1)),
            assignment(),
        ]);
        assert_eq!(
            structured_dense_frame_entry_index(&expression_prefix),
            Some(1)
        );

        let control_flow_prefix = function(vec![
            Statement::If {
                condition: Expression::IntegerLiteral(1),
                then_body: Vec::new(),
                else_body: Vec::new(),
            },
            assignment(),
        ]);
        assert_eq!(structured_dense_frame_entry_index(&control_flow_prefix), None);
    }

    #[test]
    fn prefixed_entry_copy_stays_with_the_first_call() {
        let instructions = [
            Instruction::Add {
                d: mwcc_vreg::VIRTUAL_BASE,
                a: 4,
                b: 5,
            },
            Instruction::BranchAndLink {
                target: "first".into(),
            },
            Instruction::move_register(3, mwcc_vreg::VIRTUAL_BASE),
            Instruction::BranchAndLink {
                target: "second".into(),
            },
        ];

        assert_eq!(
            prefixed_frame_entry_move(&instructions, FrameConvention::Predecrement),
            None
        );
    }

    #[test]
    fn prefixed_entry_recognizes_a_copy_before_the_first_call() {
        let instructions = [
            Instruction::load_immediate(0, 0),
            Instruction::Add {
                d: mwcc_vreg::VIRTUAL_BASE,
                a: 4,
                b: 5,
            },
            Instruction::CompareWordImmediate {
                a: 6,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 5,
            },
            Instruction::move_register(3, mwcc_vreg::VIRTUAL_BASE),
            Instruction::BranchAndLink {
                target: "first".into(),
            },
        ];

        assert_eq!(
            prefixed_frame_entry_move(&instructions, FrameConvention::Predecrement),
            Some((4, 0))
        );
    }
}
