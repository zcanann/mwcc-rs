//! Labels over a linear instruction stream — the substrate for multi-block
//! emission (task: block-structured codegen).
//!
//! Selection has so far patched branch targets by hand: remember the index of a
//! `BranchConditionalForward`, emit the fall-through, write the target back.
//! That bookkeeping does not scale to functions with shared cold blocks, merge
//! points, or backward branches (loops). A [`Label`] names a position that may
//! not exist yet; branches record a use, [`Labels::bind`] pins the position, and
//! one [`Labels::resolve`] pass writes every target. Both forward and backward
//! references work — `encode_text` computes signed offsets from indices, and the
//! BD/LI field masks carry negative displacements correctly.
//!
//! Targets are INSTRUCTION INDICES into the finished stream, so resolution must
//! happen when emission is complete and before any pass that inserts or removes
//! instructions (the schedulers refuse to reorder across branches, but
//! `coalesce_self_moves` shortens the stream — a function combining self-moves
//! with branches would need the permutation applied to targets, which no shape
//! produces today).

use mwcc_machine_code::Instruction;

/// A named position in the instruction stream, created before it is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Label(usize);

/// An opaque snapshot used to discard labels created by speculative emission.
///
/// Instruction selection occasionally tries one lowering and truncates its
/// instruction stream when that lowering declines.  Label state is part of
/// that stream and must be restored with it; retaining a speculative branch
/// use can otherwise patch an unrelated instruction which later reuses the
/// same index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LabelCheckpoint {
    bound_len: usize,
    pending_len: usize,
}

/// The label table for one function: bindings and recorded branch uses.
#[derive(Debug, Default)]
pub struct Labels {
    /// `bound[label]` is the instruction index the label pins, once bound.
    bound: Vec<Option<usize>>,
    /// `(branch instruction index, label)` pairs awaiting resolution.
    pending: Vec<(usize, Label)>,
}

impl Labels {
    /// Snapshot the append-only portions of this function's label table.
    pub fn checkpoint(&self) -> LabelCheckpoint {
        LabelCheckpoint {
            bound_len: self.bound.len(),
            pending_len: self.pending.len(),
        }
    }

    /// Discard labels and branch uses created after `checkpoint`.
    pub fn rollback(&mut self, checkpoint: LabelCheckpoint) {
        self.bound.truncate(checkpoint.bound_len);
        self.pending.truncate(checkpoint.pending_len);
    }

    /// Account for instructions inserted into the stream during emission.
    /// Both branch owners and bound destinations are instruction indices, so
    /// an insertion at either position moves them together.
    pub fn inserted(&mut self, at: usize, count: usize) {
        for binding in self.bound.iter_mut().flatten() {
            if *binding >= at {
                *binding += count;
            }
        }
        for (instruction_index, _) in &mut self.pending {
            if *instruction_index >= at {
                *instruction_index += count;
            }
        }
    }

    /// Account for instructions removed from the stream. Callers must prove no
    /// label is bound to, and no branch use originates in, the removed range.
    pub fn removed(&mut self, at: usize, count: usize) {
        let end = at + count;
        for binding in self.bound.iter_mut().flatten() {
            debug_assert!(!(*binding >= at && *binding < end));
            if *binding >= end {
                *binding -= count;
            }
        }
        for (instruction_index, _) in &mut self.pending {
            debug_assert!(!(*instruction_index >= at && *instruction_index < end));
            if *instruction_index >= end {
                *instruction_index -= count;
            }
        }
    }

    /// Account for moving one instruction from `from` to an earlier `to` slot.
    pub fn moved_before(&mut self, from: usize, to: usize) {
        debug_assert!(to < from);
        let move_index = |index: &mut usize| {
            *index = if *index == from {
                to
            } else if (to..from).contains(index) {
                *index + 1
            } else {
                *index
            };
        };
        for binding in self.bound.iter_mut().flatten() {
            move_index(binding);
        }
        for (instruction_index, _) in &mut self.pending {
            move_index(instruction_index);
        }
    }

    /// A new, unbound label.
    pub fn fresh(&mut self) -> Label {
        self.bound.push(None);
        Label(self.bound.len() - 1)
    }

    /// Pin `label` to instruction index `at` (the next instruction to be
    /// emitted). Binding twice is a logic error.
    pub fn bind(&mut self, label: Label, at: usize) {
        debug_assert!(self.bound[label.0].is_none(), "label bound twice");
        self.bound[label.0] = Some(at);
    }

    /// Record that the branch at `instruction_index` targets `label`.
    pub fn use_at(&mut self, instruction_index: usize, label: Label) {
        self.pending.push((instruction_index, label));
    }

    /// Write every recorded use's target. Errs with the offending label if one
    /// was used but never bound.
    pub fn resolve(&self, instructions: &mut [Instruction]) -> Result<(), Label> {
        for &(index, label) in &self.pending {
            let resolved = self.bound[label.0].ok_or(label)?;
            if !matches!(
                instructions[index],
                Instruction::BranchConditionalForward { .. } | Instruction::Branch { .. }
            ) {
                unreachable!("label use at instruction {index} recorded on a non-branch: {:?}", instructions[index]);
            }
            match &mut instructions[index] {
                Instruction::BranchConditionalForward { target, .. } | Instruction::Branch { target } => *target = resolved,
                _ => unreachable!("checked above"),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_forward_conditional_resolves_to_the_bound_index() {
        let mut labels = Labels::default();
        let skip = labels.fresh();
        let mut stream = vec![
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 1 },
        ];
        labels.use_at(0, skip);
        labels.bind(skip, 2);
        labels.resolve(&mut stream).unwrap();
        assert_eq!(stream[0], Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 2 });
    }

    #[test]
    fn removal_and_earlier_move_keep_label_indices_attached() {
        let mut labels = Labels::default();
        let target = labels.fresh();
        labels.use_at(7, target);
        labels.bind(target, 12);

        labels.removed(5, 1);
        assert_eq!(labels.pending[0].0, 6);
        assert_eq!(labels.bound[target.0], Some(11));

        labels.moved_before(6, 2);
        assert_eq!(labels.pending[0].0, 2);
        assert_eq!(labels.bound[target.0], Some(11));
    }

    #[test]
    fn two_branches_share_one_epilogue_label() {
        let mut labels = Labels::default();
        let epilogue = labels.fresh();
        let mut stream = vec![
            Instruction::Branch { target: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 1 },
            Instruction::Branch { target: 0 },
            Instruction::BranchToLinkRegister,
        ];
        labels.use_at(0, epilogue);
        labels.use_at(2, epilogue);
        labels.bind(epilogue, 3);
        labels.resolve(&mut stream).unwrap();
        assert_eq!(stream[0], Instruction::Branch { target: 3 });
        assert_eq!(stream[2], Instruction::Branch { target: 3 });
    }

    #[test]
    fn a_backward_branch_resolves_to_an_earlier_index() {
        // The loop shape: bind the head first, branch back to it from below.
        let mut labels = Labels::default();
        let head = labels.fresh();
        labels.bind(head, 1);
        let mut stream = vec![
            Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: -1 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 1, target: 0 },
        ];
        labels.use_at(2, head);
        labels.resolve(&mut stream).unwrap();
        assert_eq!(stream[2], Instruction::BranchConditionalForward { options: 12, condition_bit: 1, target: 1 });
    }

    #[test]
    fn an_unbound_label_is_an_error_naming_it() {
        let mut labels = Labels::default();
        let never = labels.fresh();
        let mut stream = vec![Instruction::Branch { target: 0 }];
        labels.use_at(0, never);
        assert_eq!(labels.resolve(&mut stream), Err(never));
    }

    #[test]
    fn rollback_discards_speculative_branch_uses() {
        let mut labels = Labels::default();
        let checkpoint = labels.checkpoint();
        let speculative = labels.fresh();
        labels.use_at(0, speculative);
        labels.bind(speculative, 1);

        labels.rollback(checkpoint);
        let mut replacement_stream = vec![Instruction::AddImmediate {
            d: 3,
            a: 0,
            immediate: 0,
        }];
        labels.resolve(&mut replacement_stream).unwrap();
    }

    #[test]
    fn insertion_moves_branch_uses_and_bound_destinations() {
        let mut labels = Labels::default();
        let join = labels.fresh();
        labels.use_at(0, join);
        labels.bind(join, 1);
        labels.inserted(0, 1);

        let mut stream = vec![
            Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::Branch { target: 0 },
        ];
        labels.resolve(&mut stream).unwrap();
        assert_eq!(stream[1], Instruction::Branch { target: 2 });
    }
}
