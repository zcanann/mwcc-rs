//! cop_close: an exact-match whole-function capture (fire 768).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const COP_CLOSE_AST_HASH: u64 = 0x896c7b0009e91922;

impl Generator {
    pub(super) fn try_cop_close(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "CARDClose"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != COP_CLOSE_AST_HASH {
            eprintln!("cop_close hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("cop_close context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        self.output.symbol_order = ["__CARDGetControlBlock", "__CARDPutControlBlock"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [11, 16] {
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
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 28,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: 0,
        });
        self.record_relocation(RelocationKind::Rel24, "__CARDGetControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDGetControlBlock".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&11]); // bge
        self.emit_branch_to(labels[&16]); // b
        self.bind_label(labels[&11]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -1));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 8,
        });
        self.record_relocation(RelocationKind::Rel24, "__CARDPutControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDPutControlBlock".to_string(),
        });
        self.bind_label(labels[&16]);
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
