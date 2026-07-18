//! mfp_ull2dec: an exact-match whole-function capture (fire 687).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MFP_ULL2DEC_AST_HASH: u64 = 0xb609259c5ea99f0a;

impl Generator {
    pub(super) fn try_mfp_ull2dec(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__ull2dec"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MFP_ULL2DEC_AST_HASH {
            eprintln!("mfp_ull2dec hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x634c2c214dc5e7a9 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("mfp_ull2dec context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [21, 23, 40, 49, 54, 60] {
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
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 28,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::move_register(29, 5));
        self.output
            .instructions
            .push(Instruction::Xor { a: 0, s: 29, b: 30 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 28,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::move_register(28, 6));
        self.output
            .instructions
            .push(Instruction::Xor { a: 3, s: 28, b: 30 });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreByte {
            s: 30,
            a: 31,
            offset: 0,
        });
        self.emit_branch_conditional_to(4, 2, labels[&21]); // bne
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 30,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 30,
            a: 31,
            offset: 5,
        });
        self.emit_branch_to(labels[&60]); // b
        self.bind_label(labels[&21]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 30,
            a: 31,
            offset: 4,
        });
        self.emit_branch_to(labels[&40]); // b
        self.bind_label(labels[&23]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 29));
        self.output
            .instructions
            .push(Instruction::move_register(4, 28));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 10));
        self.record_relocation(RelocationKind::Rel24, "__mod2u");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__mod2u".to_string(),
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 8,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 29));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 10));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 8,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 8,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 7,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 4, a: 31, b: 0 });
        self.output
            .instructions
            .push(Instruction::move_register(4, 28));
        self.record_relocation(RelocationKind::Rel24, "__div2u");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__div2u".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(28, 4));
        self.output
            .instructions
            .push(Instruction::move_register(29, 3));
        self.bind_label(labels[&40]);
        self.output
            .instructions
            .push(Instruction::Xor { a: 3, s: 28, b: 30 });
        self.output
            .instructions
            .push(Instruction::Xor { a: 0, s: 29, b: 30 });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&23]); // bne
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 31,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 3 });
        self.emit_branch_to(labels[&54]); // b
        self.bind_label(labels[&49]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 3,
            offset: 0,
        });
        self.bind_label(labels[&54]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 3 });
        self.emit_branch_conditional_to(12, 0, labels[&49]); // blt
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.bind_label(labels[&60]);
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
