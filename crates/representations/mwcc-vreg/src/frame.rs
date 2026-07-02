//! The frame builder: derive a function's prologue and epilogue from a plan,
//! instead of hand-emitting them in every selection arm.
//!
//! mwcc's EABI frame has one canonical shape — `stwu r1,-N(r1); mflr r0;
//! stw r0,N+4(r1); [stw rS,N-4k(r1) per save]` and the mirrored epilogue
//! `lwz r0,N+4(r1); [lwz rS per save]; mtlr r0; addi r1,r1,N; blr` — with a small
//! set of captured VARIATIONS (a load interleaved into the prologue's latency
//! slots, an LR-first epilogue, a result move riding between reloads). This
//! module produces the CANONICAL schedule; the variations remain the arms'
//! business (or later, options here) — an arm that needs one keeps hand emission.
//!
//! Save registers may be VIRTUAL field values: the builder threads them through
//! untouched, and `apply` renames the saves/restores together with every other
//! use once the allocator has chosen homes.

use mwcc_machine_code::Instruction;

/// What the frame must provide, independent of any block's contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramePlan {
    /// Total frame bytes: 8-byte linkage + one word per save, rounded up to 16.
    pub frame_size: i16,
    /// Registers saved across the body (virtual field values allowed), highest
    /// logical slot first — slot k stores at `frame_size - 4*(k+1)`.
    pub saved: Vec<u8>,
}

impl FramePlan {
    /// A plan for `count` saved registers: the standard 16-byte-aligned size.
    pub fn sized_for(saved: Vec<u8>) -> FramePlan {
        let frame_size = ((8 + 4 * saved.len() as i16 + 15) / 16) * 16;
        FramePlan { frame_size, saved }
    }

    /// The canonical non-leaf prologue: `stwu; mflr; stw r0; stw rS…` (saves in
    /// slot order, no interleaves).
    pub fn prologue(&self) -> Vec<Instruction> {
        let mut instructions = vec![
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -self.frame_size },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: self.frame_size + 4 },
        ];
        for (slot, &register) in self.saved.iter().enumerate() {
            instructions.push(Instruction::StoreWord { s: register, a: 1, offset: self.frame_size - 4 * (slot as i16 + 1) });
        }
        instructions
    }

    /// The INTERLEAVED-MOVE prologue: each save immediately followed by the move
    /// that parks its incoming value — `stwu; mflr; stw r0; stw rS,fs-4; mr rS,rX;
    /// stw rS',fs-8; mr rS',rY; …` — the captured schedule for parameters saved
    /// across calls. `incoming[k]` pairs with `saved[k]`.
    pub fn prologue_interleaved(&self, incoming: &[u8]) -> Vec<Instruction> {
        let mut instructions = vec![
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -self.frame_size },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: self.frame_size + 4 },
        ];
        for (slot, (&register, &source)) in self.saved.iter().zip(incoming).enumerate() {
            instructions.push(Instruction::StoreWord { s: register, a: 1, offset: self.frame_size - 4 * (slot as i16 + 1) });
            instructions.push(Instruction::Or { a: register, s: source, b: source });
        }
        instructions
    }

    /// The canonical epilogue: `lwz r0; lwz rS…; mtlr; addi; blr` (restores in
    /// slot order after the LR reload).
    pub fn epilogue(&self) -> Vec<Instruction> {
        let mut instructions = vec![Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 }];
        for (slot, &register) in self.saved.iter().enumerate() {
            instructions.push(Instruction::LoadWord { d: register, a: 1, offset: self.frame_size - 4 * (slot as i16 + 1) });
        }
        instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
        instructions.push(Instruction::BranchToLinkRegister);
        instructions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saveless_plan_is_the_plain_non_leaf_frame() {
        let plan = FramePlan::sized_for(vec![]);
        assert_eq!(plan.frame_size, 16);
        assert_eq!(
            plan.prologue(),
            vec![
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 20 },
            ]
        );
        assert_eq!(
            plan.epilogue(),
            vec![
                Instruction::LoadWord { d: 0, a: 1, offset: 20 },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::AddImmediate { d: 1, a: 1, immediate: 16 },
                Instruction::BranchToLinkRegister,
            ]
        );
    }

    #[test]
    fn the_interleaved_prologue_pairs_each_save_with_its_move() {
        let plan = FramePlan::sized_for(vec![31, 30]);
        let prologue = plan.prologue_interleaved(&[4, 3]);
        assert_eq!(prologue[3], Instruction::StoreWord { s: 31, a: 1, offset: 12 });
        assert_eq!(prologue[4], Instruction::Or { a: 31, s: 4, b: 4 });
        assert_eq!(prologue[5], Instruction::StoreWord { s: 30, a: 1, offset: 8 });
        assert_eq!(prologue[6], Instruction::Or { a: 30, s: 3, b: 3 });
    }

    #[test]
    fn saves_take_descending_slots_and_the_size_rounds_to_sixteen() {
        let plan = FramePlan::sized_for(vec![31, 30, 29]);
        assert_eq!(plan.frame_size, 32); // 8 + 12 -> 32
        assert_eq!(plan.prologue()[3], Instruction::StoreWord { s: 31, a: 1, offset: 28 });
        assert_eq!(plan.prologue()[4], Instruction::StoreWord { s: 30, a: 1, offset: 24 });
        assert_eq!(plan.prologue()[5], Instruction::StoreWord { s: 29, a: 1, offset: 20 });
        assert_eq!(plan.epilogue()[1], Instruction::LoadWord { d: 31, a: 1, offset: 28 });
    }
}
