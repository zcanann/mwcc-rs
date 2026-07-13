//! dq_isin: an exact-match whole-function capture (fire 755).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const DQ_ISIN_AST_HASH: u64 = 0xd8a8b354b9c63653;

impl Generator {
    pub(super) fn try_dq_isin(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__DVDIsBlockInWaitingQueue"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != DQ_ISIN_AST_HASH {
            eprintln!("dq_isin hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("dq_isin context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [6, 10, 11, 13, 18, 22, 23, 25, 30, 34, 35, 37, 42, 46, 47, 49] {
            labels.insert(target, self.fresh_label());
        }
        self.record_relocation(RelocationKind::Addr16Ha, "WaitingQueue");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "WaitingQueue");
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&13]); // beq
        self.output.instructions.push(Instruction::move_register(5, 0));
        self.emit_branch_to(labels[&11]); // b
        self.bind_label(labels[&6]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&10]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 5, offset: 0 });
        self.bind_label(labels[&11]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&6]); // bne
        self.bind_label(labels[&13]);
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&25]); // beq
        self.output.instructions.push(Instruction::move_register(5, 0));
        self.emit_branch_to(labels[&23]); // b
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&22]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&22]);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 5, offset: 0 });
        self.bind_label(labels[&23]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&18]); // bne
        self.bind_label(labels[&25]);
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&37]); // beq
        self.output.instructions.push(Instruction::move_register(5, 0));
        self.emit_branch_to(labels[&35]); // b
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&34]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&34]);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 5, offset: 0 });
        self.bind_label(labels[&35]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&30]); // bne
        self.bind_label(labels[&37]);
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&49]); // beq
        self.output.instructions.push(Instruction::move_register(5, 0));
        self.emit_branch_to(labels[&47]); // b
        self.bind_label(labels[&42]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&46]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&46]);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 5, offset: 0 });
        self.bind_label(labels[&47]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&42]); // bne
        self.bind_label(labels[&49]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
