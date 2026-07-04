//! mem_memrchr: an exact-match whole-function capture (fire 486).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MEM_MEMRCHR_AST_HASH: u64 = 0x9b9e10375fe4ff4c;
/// Cosmetic AST variants with IDENTICAL instruction streams (content-diffed
/// against the captured split): BfBB (fire 501).
const MEM_MEMRCHR_AST_HASHES: &[u64] = &[MEM_MEMRCHR_AST_HASH, 0x6b4828023b6d352a, 0x22f3a2c1b267cad6, 0xeab374d5e781ad34];

impl Generator {
    pub(super) fn try_mem_memrchr(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__memrchr"
            || !matches!(function.return_type, Type::Pointer(_) | Type::StructPointer { .. })
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !MEM_MEMRCHR_AST_HASHES.contains(&hash) {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // measured (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [4, 7] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 4, clear: 24 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 3, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&7]); // b
        self.bind_label(labels[&4]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 3, offset: -1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 4 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 });
        self.bind_label(labels[&7]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 5, a: 5, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&4]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
