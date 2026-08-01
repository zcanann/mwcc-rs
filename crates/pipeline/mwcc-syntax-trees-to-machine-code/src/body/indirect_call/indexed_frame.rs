//! Indirect calls through a global table indexed by a guarded frame byte.

use super::*;

impl Generator {
    /// Reuse the frame byte loaded for the immediately dominating bounds check
    /// when indexing a global callback table in the taken arm.
    pub(super) fn try_emit_frame_indexed_global_indirect_call(
        &mut self,
        target: &Expression,
        arguments: &[Expression],
    ) -> Compilation<bool> {
        let Expression::Member {
            base,
            offset: 0,
            index_stride: Some(stride),
            ..
        } = target
        else {
            return Ok(false);
        };
        let Expression::Index { base, index } = base.as_ref() else {
            return Ok(false);
        };
        let (Expression::Variable(table), Expression::Variable(index_name)) =
            (base.as_ref(), index.as_ref())
        else {
            return Ok(false);
        };
        if !self.globals.contains_key(table)
            || !self.frame_slots.contains_key(index_name)
            || *stride == 0
            || !stride.is_power_of_two()
        {
            return Ok(false);
        }

        let instructions = &self.output.instructions;
        let Some((branch, prefix)) = instructions.split_last() else {
            return Ok(false);
        };
        if !matches!(branch, Instruction::BranchConditionalForward { .. }) {
            return Ok(false);
        }
        let Some((compare, prefix)) = prefix.split_last() else {
            return Ok(false);
        };
        let index_register = match compare {
            Instruction::CompareWord { a, b: GENERAL_SCRATCH }
            | Instruction::CompareLogicalWord { a, b: GENERAL_SCRATCH } => *a,
            _ => return Ok(false),
        };
        let Some(slot) = self.frame_slots.get(index_name) else {
            return Ok(false);
        };
        if !prefix.iter().rev().take(3).any(|instruction| {
            matches!(instruction,
                Instruction::LoadByteZero { d, a: 1, offset }
                    if *d == index_register && *offset == slot.offset)
        }) {
            return Ok(false);
        }

        let placements = self.indirect_argument_placements(arguments)?;
        let table_base = self.fresh_virtual_general_preferring(3);
        self.emit_address_high(table_base, table);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: index_register,
                s: index_register,
                shift: stride.trailing_zeros() as u8,
            });
        self.record_relocation(RelocationKind::Addr16Lo, table);
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: table_base,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::Add {
            d: table_base,
            a: GENERAL_SCRATCH,
            b: index_register,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: table_base,
            offset: 0,
        });
        self.emit_indirect_arguments(arguments, &placements)?;
        self.emit_indirect_branch_and_link(12);
        Ok(true)
    }
}
