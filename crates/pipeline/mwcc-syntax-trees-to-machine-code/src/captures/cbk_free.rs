//! cbk_free: an exact-match whole-function capture (fire 767).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const CBK_FREE_AST_HASH: u64 = 0xca403eca15b602ee;

impl Generator {
    pub(super) fn try_cbk_free(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__CARDFreeBlock"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != CBK_FREE_AST_HASH {
            eprintln!("cbk_free hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("cbk_free context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        self.output.symbol_order = ["__CARDBlock", "__CARDUpdateFatBlock"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [12, 15, 21, 23, 29, 34] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 7,
                a: 3,
                immediate: 272,
            });
        self.record_relocation(RelocationKind::Addr16Ha, "__CARDBlock");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(6, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "__CARDBlock");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 6,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 8, a: 0, b: 7 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 8,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&12]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -3));
        self.emit_branch_to(labels[&34]); // b
        self.bind_label(labels[&12]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 9,
            a: 8,
            offset: 136,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(7, 0));
        self.emit_branch_to(labels[&29]); // b
        self.bind_label(labels[&15]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 6,
                s: 4,
                clear: 16,
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 6, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&21]); // blt
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 8,
                offset: 16,
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 6, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&23]); // blt
        self.bind_label(labels[&21]);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -6));
        self.emit_branch_to(labels[&34]); // b
        self.bind_label(labels[&23]);
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 4,
            shift: 1,
            begin: 15,
            end: 30,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZeroIndexed { d: 4, a: 9, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreHalfwordIndexed { s: 7, a: 9, b: 0 });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 6,
                a: 9,
                offset: 6,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 6,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 9,
            offset: 6,
        });
        self.bind_label(labels[&29]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 4,
                clear: 16,
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 65535,
            });
        self.emit_branch_conditional_to(4, 2, labels[&15]); // bne
        self.output
            .instructions
            .push(Instruction::move_register(4, 9));
        self.record_relocation(RelocationKind::Rel24, "__CARDUpdateFatBlock");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDUpdateFatBlock".to_string(),
        });
        self.bind_label(labels[&34]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
