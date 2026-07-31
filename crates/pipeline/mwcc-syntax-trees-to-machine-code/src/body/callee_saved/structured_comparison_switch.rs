//! Shared-base comparison-tree dispatch inside allocator-backed functions.
//!
//! Small switches and dense jump-table switches are distinct MWCC policies.
//! This owner retains a small source switch whose case values share one
//! materializable high half, emits the proven balanced tree, and then delegates
//! each arm back to the ordinary structured statement emitter.

use super::structured_dense_switch::{statements_fall_through, switch_bodies_use_name};
use super::structured_entry_alias::EntryParameterAlias;
use super::structured_switch_lowering::shared_base_comparison_switch;
#[allow(unused_imports)]
use super::*;
use crate::switch::Target;
use mwcc_syntax_trees::ArmBody;

impl Generator {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_structured_comparison_switch(
        &mut self,
        scrutinee: &Expression,
        arms: &[mwcc_syntax_trees::SwitchArm],
        default: Option<&ArmBody>,
        function: &Function,
        ephemeral_locals: &[&LocalDeclaration],
        return_branches: &mut Vec<usize>,
        label_positions: &mut std::collections::HashMap<String, usize>,
        pending_gotos: &mut Vec<(usize, String)>,
        entry_alias: &mut Option<EntryParameterAlias>,
    ) -> Compilation<()> {
        let base = shared_base_comparison_switch(arms).ok_or_else(|| {
            Diagnostic::error("structured comparison switch has no shared-base dispatch plan")
        })?;
        let mut sorted = arms
            .iter()
            .enumerate()
            .map(|(source_index, arm)| (arm.value, source_index))
            .collect::<Vec<_>>();
        sorted.sort_by_key(|&(value, _)| value);
        let values = sorted.iter().map(|&(value, _)| value).collect::<Vec<_>>();

        let source_register = match scrutinee {
            Expression::Variable(name) if self.locations.contains_key(name) => {
                let location = &self.locations[name];
                if location.class != ValueClass::General {
                    return Err(Diagnostic::error(
                        "structured switch scrutinee is not an integer",
                    ));
                }
                location.register
            }
            _ => {
                self.evaluate_general(scrutinee, Eabi::general_result().number)?;
                Eabi::general_result().number
            }
        };

        // The dispatch owns r3 for the compared value and r4 for the shared
        // high-half base. Preserve any allocator homes that the case bodies
        // still read before claiming those physical scratch registers.
        let preserved = self
            .locations
            .iter()
            .filter(|(name, location)| {
                location.class == ValueClass::General
                    && matches!(location.register, 3 | 4)
                    && switch_bodies_use_name(arms, default, name)
            })
            .map(|(name, location)| (name.clone(), location.register))
            .collect::<Vec<_>>();
        for (offset, (name, source)) in preserved.into_iter().enumerate() {
            let retained = self.fresh_virtual_general_preferring(7u8.saturating_sub(offset as u8));
            self.output
                .instructions
                .push(Instruction::move_register(retained, source));
            self.locations
                .get_mut(&name)
                .expect("preserved dispatch value came from a known location")
                .register = retained;
        }
        if source_register != Eabi::general_result().number {
            self.output.instructions.push(Instruction::move_register(
                Eabi::general_result().number,
                source_register,
            ));
        }

        let mut patches = Vec::new();
        self.lower_shared_base_switch_range(
            Eabi::general_result().number,
            &values,
            4,
            base,
            &mut patches,
        );

        let mut body_start = vec![0usize; arms.len()];
        let mut join_branches = Vec::new();
        for (source_index, arm) in arms.iter().enumerate() {
            let sorted_index = sorted
                .iter()
                .position(|&(_, index)| index == source_index)
                .expect("each source arm has one sorted dispatch slot");
            body_start[sorted_index] = self.output.instructions.len();
            self.reset_structured_switch_edge_caches();
            let falls_through_body = match &arm.body {
                ArmBody::Statements(statements) => {
                    self.emit_structured_arm_with_global_pointer_cache(
                        statements,
                        function,
                        ephemeral_locals,
                        return_branches,
                        label_positions,
                        pending_gotos,
                        entry_alias,
                    )?;
                    statements_fall_through(statements)
                }
                ArmBody::Return(value) => {
                    let result = match function.return_type {
                        Type::Float | Type::Double => Eabi::float_result().number,
                        _ => Eabi::general_result().number,
                    };
                    self.evaluate(value, function.return_type, result)?;
                    return_branches.push(self.output.instructions.len());
                    self.output.instructions.push(Instruction::Branch {
                        target: super::structured_early_return_schedule::
                            STRUCTURED_EPILOGUE_PLACEHOLDER,
                    });
                    false
                }
            };
            if falls_through_body
                && !arm.falls_through
                && (source_index + 1 != arms.len() || default.is_some())
            {
                join_branches.push(self.output.instructions.len());
                self.output
                    .instructions
                    .push(Instruction::Branch { target: 0 });
            }
        }

        let default_start = self.output.instructions.len();
        if let Some(default) = default {
            self.reset_structured_switch_edge_caches();
            match default {
                ArmBody::Statements(statements) => {
                    self.emit_structured_arm_with_global_pointer_cache(
                        statements,
                        function,
                        ephemeral_locals,
                        return_branches,
                        label_positions,
                        pending_gotos,
                        entry_alias,
                    )?;
                }
                ArmBody::Return(value) => {
                    let result = match function.return_type {
                        Type::Float | Type::Double => Eabi::float_result().number,
                        _ => Eabi::general_result().number,
                    };
                    self.evaluate(value, function.return_type, result)?;
                    return_branches.push(self.output.instructions.len());
                    self.output.instructions.push(Instruction::Branch {
                        target: super::structured_early_return_schedule::
                            STRUCTURED_EPILOGUE_PLACEHOLDER,
                    });
                }
            }
        }
        let join = self.output.instructions.len();
        self.reset_structured_switch_edge_caches();

        for branch in join_branches {
            if let Instruction::Branch { target } = &mut self.output.instructions[branch] {
                *target = join;
            }
        }
        for (index, target) in patches {
            let destination = match target {
                Target::Body(body) => body_start[body],
                Target::Default => default_start,
            };
            match &mut self.output.instructions[index] {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target } => *target = destination,
                _ => unreachable!("structured switch patch points at a non-branch instruction"),
            }
        }
        Ok(())
    }
}
