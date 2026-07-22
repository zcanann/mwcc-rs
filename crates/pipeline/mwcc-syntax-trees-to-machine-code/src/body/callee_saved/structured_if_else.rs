//! Two-arm control flow for the structured virtual-register path.
//!
//! The parent owns liveness, frame construction, and the shared exit. This
//! module owns only the diamond: condition exits target the else arm and a
//! fallthrough then arm skips to the common continuation.

use super::structured::logical_and_terms;
use super::structured_entry_alias::{fold_entry_alias_zero_test, EntryParameterAlias};
#[allow(unused_imports)]
use super::*;

impl Generator {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_structured_if_else(
        &mut self,
        condition: &Expression,
        then_body: &[Statement],
        else_body: &[Statement],
        statement_index: usize,
        function: &Function,
        ephemeral_locals: &[&LocalDeclaration],
        return_branches: &mut Vec<usize>,
        label_positions: &mut std::collections::HashMap<String, usize>,
        pending_gotos: &mut Vec<(usize, String)>,
        entry_alias: &mut Option<EntryParameterAlias>,
    ) -> Compilation<()> {
        debug_assert!(!else_body.is_empty());
        let previous_cache = self.begin_condition_global_cache(condition);
        let previous_float_cache = self.begin_condition_float_cache(condition);
        let branches = (|| {
            self.preload_condition_global_cache(condition)?;
            let terms = logical_and_terms(condition);
            let mut branches = Vec::with_capacity(terms.len());
            for (term_index, term) in terms.into_iter().enumerate() {
                let (options, condition_bit) = self.emit_condition_test(term).map_err(
                    |mut diagnostic| {
                        diagnostic.message.push_str(&format!(
                            " (in structured if/else condition {statement_index})"
                        ));
                        diagnostic
                    },
                )?;
                if statement_index == 0 && term_index == 0 {
                    if let Some(alias) = entry_alias.as_ref() {
                        fold_entry_alias_zero_test(&mut self.output.instructions, alias);
                    }
                }
                branches.push(self.output.instructions.len());
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalForward {
                        options,
                        condition_bit,
                        target: 0,
                    });
                if statement_index == 0 && term_index == 0 {
                    if let Some(alias) = entry_alias.take() {
                        self.locations
                            .get_mut(&alias.name)
                            .expect("planned saved parameter")
                            .register = alias.home;
                    }
                }
            }
            Ok(branches)
        })();
        self.restore_condition_global_cache(previous_cache);
        self.restore_condition_float_cache(previous_float_cache);
        let branches = branches?;
        if let [branch] = branches.as_slice() {
            self.schedule_frame_store_before_if_branch(*branch);
        }
        self.commit_structured_float_handoff();

        if !self.try_emit_structured_frame_bitfield_stores(then_body)? {
            self.emit_structured_statements(
                then_body,
                function,
                ephemeral_locals,
                false,
                return_branches,
                label_positions,
                pending_gotos,
                entry_alias,
            )
            .map_err(|mut diagnostic| {
                diagnostic.message.push_str(&format!(
                    " (inside structured then arm {statement_index})"
                ));
                diagnostic
            })?;
        }
        let skip_else = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });

        let else_start = self.output.instructions.len();
        for branch in branches {
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[branch]
            {
                *target = else_start;
            }
        }
        if !self.try_emit_structured_frame_bitfield_stores(else_body)? {
            self.emit_structured_statements(
                else_body,
                function,
                ephemeral_locals,
                false,
                return_branches,
                label_positions,
                pending_gotos,
                entry_alias,
            )
            .map_err(|mut diagnostic| {
                diagnostic.message.push_str(&format!(
                    " (inside structured else arm {statement_index})"
                ));
                diagnostic
            })?;
        }
        let join = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[skip_else] {
            *target = join;
        }
        Ok(())
    }
}
