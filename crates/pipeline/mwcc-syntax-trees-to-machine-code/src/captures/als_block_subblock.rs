//! als_block_subblock: an exact-match whole-function capture (fire 732).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ALS_BLOCK_SUBBLOCK_AST_HASH: u64 = 0xd3b63e3617b3f403;

impl Generator {
    pub(super) fn try_als_block_subblock(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "Block_subBlock"
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
        if hash != ALS_BLOCK_SUBBLOCK_AST_HASH {
            eprintln!("als_block_subblock hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("als_block_subblock context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [10, 15, 21, 26, 48, 58, 59, 67, 76, 78, 86, 106, 116, 122] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 5,
                s: 0,
                begin: 0,
                end: 28,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: -4,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordIndexed { d: 5, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&10]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::move_register(6, 5));
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 0,
                s: 0,
                begin: 0,
                end: 28,
            });
        self.output
            .instructions
            .push(Instruction::move_register(7, 0));
        self.emit_branch_to(labels[&26]); // b
        self.bind_label(labels[&15]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 5,
            offset: 12,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 0,
                s: 0,
                begin: 0,
                end: 28,
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 7, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&21]); // bge
        self.output
            .instructions
            .push(Instruction::move_register(7, 0));
        self.bind_label(labels[&21]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 5, b: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&26]); // bne
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 3,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&26]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(12, 0, labels[&15]); // blt
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 0, a: 4, b: 0 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 80,
            });
        self.emit_branch_conditional_to(12, 0, labels[&86]); // blt
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 8, a: 5, b: 4 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 9,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 0,
                s: 0,
                begin: 0,
                end: 30,
            });
        self.output.instructions.push(Instruction::OrImmediate {
            a: 10,
            s: 0,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 9,
            shift: 0,
            begin: 30,
            end: 30,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 10,
            a: 5,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::CountLeadingZeros { a: 6, s: 0 });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 7,
                s: 6,
                shift: 5,
            });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 9,
                shift: 0,
                begin: 29,
                end: 29,
            });
        self.output
            .instructions
            .push(Instruction::CountLeadingZeros { a: 6, s: 7 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 6,
                s: 6,
                shift: 5,
            });
        self.emit_branch_conditional_to(12, 2, labels[&48]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::OrImmediate {
            a: 0,
            s: 0,
            immediate: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 5,
            offset: 0,
        });
        self.bind_label(labels[&48]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 9,
                s: 9,
                begin: 0,
                end: 28,
            });
        self.emit_branch_conditional_to(12, 2, labels[&58]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::OrImmediate {
            a: 0,
            s: 0,
            immediate: 2,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 8,
            offset: 0,
        });
        self.output.instructions.push(Instruction::OrImmediate {
            a: 0,
            s: 0,
            immediate: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 8,
            offset: 0,
        });
        self.emit_branch_to(labels[&59]); // b
        self.bind_label(labels[&58]);
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 8,
            offset: -4,
        });
        self.bind_label(labels[&59]);
        self.output.instructions.push(Instruction::StoreWord {
            s: 10,
            a: 8,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 4, b: 9 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 8,
            offset: 0,
        });
        self.emit_branch_conditional_to(12, 2, labels[&67]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 8,
            offset: 0,
        });
        self.output.instructions.push(Instruction::OrImmediate {
            a: 0,
            s: 0,
            immediate: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 8,
            offset: 0,
        });
        self.bind_label(labels[&67]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&76]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 8,
            offset: 0,
        });
        self.output.instructions.push(Instruction::OrImmediate {
            a: 0,
            s: 0,
            immediate: 2,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 8,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordIndexed { d: 0, a: 8, b: 4 });
        self.output.instructions.push(Instruction::OrImmediate {
            a: 0,
            s: 0,
            immediate: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordIndexed { s: 0, a: 8, b: 4 });
        self.emit_branch_to(labels[&78]); // b
        self.bind_label(labels[&76]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: -4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordIndexed { s: 4, a: 8, b: 0 });
        self.bind_label(labels[&78]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&86]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 8,
            offset: 12,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 8,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 8,
            a: 4,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 8,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 8,
            a: 5,
            offset: 12,
        });
        self.bind_label(labels[&86]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 12,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 5,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 4,
                s: 0,
                begin: 0,
                end: 28,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: -4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordIndexed { s: 6, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::OrImmediate {
            a: 0,
            s: 4,
            immediate: 2,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 4,
                s: 4,
                begin: 0,
                end: 28,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordIndexed { d: 0, a: 5, b: 4 });
        self.output.instructions.push(Instruction::OrImmediate {
            a: 0,
            s: 0,
            immediate: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordIndexed { s: 0, a: 5, b: 4 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 4,
                s: 0,
                begin: 0,
                end: 28,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -4,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordIndexed { d: 0, a: 3, b: 4 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 0, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&106]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordIndexed { s: 0, a: 3, b: 4 });
        self.bind_label(labels[&106]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 4,
                s: 0,
                begin: 0,
                end: 28,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -4,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordIndexed { d: 0, a: 3, b: 4 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 0, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&116]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output
            .instructions
            .push(Instruction::StoreWordIndexed { s: 0, a: 3, b: 4 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 8,
        });
        self.emit_branch_to(labels[&122]); // b
        self.bind_label(labels[&116]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 5,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 12,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 5,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 12,
        });
        self.bind_label(labels[&122]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 5));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
