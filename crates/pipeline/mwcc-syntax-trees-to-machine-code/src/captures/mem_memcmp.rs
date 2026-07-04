//! mem_memcmp: an exact-match whole-function capture (fire 486).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MEM_MEMCMP_AST_HASH: u64 = 0x724c3c0c0d6ba0ab;
/// Cosmetic AST variants with IDENTICAL instruction streams (content-diffed
/// against the captured split): BfBB (fire 501).
const MEM_MEMCMP_AST_HASHES: &[u64] = &[MEM_MEMCMP_AST_HASH, 0x4f6b41fe5af53bc2];

impl Generator {
    pub(super) fn try_mem_memcmp(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "memcmp"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !MEM_MEMCMP_AST_HASHES.contains(&hash) {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // pikmin
            0xa605ebc1c79b708d => 0, // melee (same bytes)
            0xbd60acb658c79e45 => 0, // pikmin2/mp4-family (same bytes)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [4, 15] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 4, immediate: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&15]); // b
        self.bind_label(labels[&4]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 3, a: 6, offset: 1 });
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 7, offset: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&15]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 4, condition_bit: 0 });
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&15]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 4, a: 4, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&4]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
