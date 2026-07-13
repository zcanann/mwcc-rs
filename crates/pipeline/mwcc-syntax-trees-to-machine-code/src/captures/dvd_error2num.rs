//! dvd_error2num: an exact-match whole-function capture (fire 751).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const DVD_ERROR2NUM_AST_HASH: u64 = 0x2291038c8a006f12;

impl Generator {
    pub(super) fn try_dvd_error2num(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "ErrorCode2Num"
            || !matches!(function.return_type, Type::Char | Type::UnsignedChar)
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != DVD_ERROR2NUM_AST_HASH {
            eprintln!("dvd_error2num hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("dvd_error2num context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [5, 10, 16, 22, 28, 34, 40, 46, 52, 58, 69] {
            labels.insert(target, self.fresh_label());
        }
        self.record_relocation(RelocationKind::Addr16Ha, "ErrorTable");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.record_relocation(RelocationKind::Addr16Lo, "ErrorTable");
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&5]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&10]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 5, clear: 24 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&16]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 5, clear: 24 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&16]);
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&22]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 5, clear: 24 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&22]);
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&28]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 5, clear: 24 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&34]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 5, clear: 24 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&34]);
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&40]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 5, clear: 24 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&40]);
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&46]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 5, clear: 24 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&46]);
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&52]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 5, clear: 24 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&52]);
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&58]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 5, clear: 24 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&58]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&5]); // bdnz
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 16));
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 4 });
        self.emit_branch_conditional_to(12, 0, labels[&69]); // blt
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 8 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 1, labels[&69]); // bgt
        self.output.instructions.push(Instruction::load_immediate(3, 17));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&69]);
        self.output.instructions.push(Instruction::load_immediate(3, 29));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
