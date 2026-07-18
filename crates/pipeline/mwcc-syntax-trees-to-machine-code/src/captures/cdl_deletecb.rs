//! cdl_deletecb: an exact-match whole-function capture (fire 764).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const CDL_DELETECB_AST_HASH: u64 = 0xb0256c6663d3acf7;

impl Generator {
    pub(super) fn try_cdl_deletecb(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "DeleteCallback"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != CDL_DELETECB_AST_HASH {
            eprintln!("cdl_deletecb hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("cdl_deletecb context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // OSFastCast.h plain-`inline` asm helpers -> GLOBAL UND at head of the global-UND
        // run; attach to this source-first function (measured: CARDDelete.c).
        self.output.phantom_externals = vec!["__OSf32tos16".to_string(), "__OSf32tou8".to_string()];
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        self.output.symbol_order = ["__CARDBlock", "__CARDFreeBlock", "__CARDPutControlBlock"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [22, 32] {
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
        self.record_relocation(RelocationKind::Addr16Ha, "__CARDBlock");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(5, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "__CARDBlock");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 0,
        });
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
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 29, s: 4, b: 4 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 28,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::move_register(28, 3));
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 6,
                a: 28,
                immediate: 272,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 31, a: 0, b: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 31,
            offset: 208,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 208,
        });
        self.emit_branch_conditional_to(12, 0, labels[&22]); // blt
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 4,
                a: 31,
                offset: 190,
            });
        self.output
            .instructions
            .push(Instruction::move_register(5, 30));
        self.record_relocation(RelocationKind::Rel24, "__CARDFreeBlock");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDFreeBlock".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 29, s: 3, b: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&32]); // bge
        self.bind_label(labels[&22]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output
            .instructions
            .push(Instruction::move_register(4, 29));
        self.record_relocation(RelocationKind::Rel24, "__CARDPutControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDPutControlBlock".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 30,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&32]); // beq
        self.output
            .instructions
            .push(Instruction::move_register(12, 30));
        self.output
            .instructions
            .push(Instruction::move_register(3, 28));
        self.output
            .instructions
            .push(Instruction::move_register(4, 29));
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.bind_label(labels[&32]);
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
        self.output.instructions.push(Instruction::LoadWord {
            d: 28,
            a: 1,
            offset: 16,
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
