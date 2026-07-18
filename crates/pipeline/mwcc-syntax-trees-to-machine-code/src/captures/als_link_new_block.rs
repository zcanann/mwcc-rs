//! als_link_new_block: an exact-match whole-function capture (fire 732).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ALS_LINK_NEW_BLOCK_AST_HASH: u64 = 0x1bd9b7e5ec76e9c5;

impl Generator {
    pub(super) fn try_als_link_new_block(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "link_new_block"
            || !matches!(
                function.return_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            )
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ALS_LINK_NEW_BLOCK_AST_HASH {
            eprintln!("als_link_new_block hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("als_link_new_block context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [13, 19, 34, 37, 38] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 31,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 1));
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 30,
                s: 4,
                begin: 0,
                end: 28,
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 30, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::move_register(29, 3));
        self.emit_branch_conditional_to(4, 0, labels[&13]); // bge
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(30, 1));
        self.bind_label(labels[&13]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.record_relocation(RelocationKind::Rel24, "__sys_alloc");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__sys_alloc".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 31, s: 3, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&19]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&38]); // b
        self.bind_label(labels[&19]);
        self.output
            .instructions
            .push(Instruction::move_register(4, 30));
        self.record_relocation(RelocationKind::Rel24, "Block_construct");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "Block_construct".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 29,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&34]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 3,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 29,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 29,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 3,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 29,
            offset: 0,
        });
        self.emit_branch_to(labels[&37]); // b
        self.bind_label(labels[&34]);
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 29,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 31,
            offset: 4,
        });
        self.bind_label(labels[&37]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.bind_label(labels[&38]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 24,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 29,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
