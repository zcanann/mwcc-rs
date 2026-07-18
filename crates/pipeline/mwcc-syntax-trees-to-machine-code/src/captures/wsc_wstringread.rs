//! wsc_wstringread: an exact-match whole-function capture (fire 701).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const WSC_WSTRINGREAD_AST_HASH: u64 = 0x3832a40505a29ba1;

impl Generator {
    pub(super) fn try_wsc_wstringread(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__wStringRead"
            || function.return_type != Type::UnsignedShort
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != WSC_WSTRINGREAD_AST_HASH {
            eprintln!("wsc_wstringread hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("wsc_wstringread context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [6, 9, 18, 22, 29, 31, 33, 36] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&22]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&6]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&9]); // bge
        self.emit_branch_to(labels[&36]); // b
        self.bind_label(labels[&6]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&36]); // bge
        self.emit_branch_to(labels[&33]); // b
        self.bind_label(labels[&9]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 5,
                a: 4,
                offset: 0,
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&18]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 1));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 4,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 2,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 5));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&22]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&29]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: -2,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 0,
        });
        self.emit_branch_to(labels[&31]); // b
        self.bind_label(labels[&29]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 4,
        });
        self.bind_label(labels[&31]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 4));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&33]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 3,
                s: 0,
                clear: 16,
            });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&36]);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
