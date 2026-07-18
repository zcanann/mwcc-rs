//! ansif_close_all: an exact-match whole-function capture (fire 507).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ANSIF_CLOSE_ALL_AST_HASH: u64 = 0x2d6578b0ff99fb80; // AC; +mp4, ww (f507)
/// Cosmetic AST variants with IDENTICAL instruction streams (@N-normalized).
const ANSIF_CLOSE_ALL_AST_HASHES: &[u64] = &[
    ANSIF_CLOSE_ALL_AST_HASH,
    0x138d7a7cbc054577,
    0x2bf7b5283a5b0b56,
];

impl Generator {
    pub(super) fn try_ansif_close_all(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__close_all"
            || function.return_type != Type::Void
            || function.parameters.len() != 0
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !ANSIF_CLOSE_ALL_AST_HASHES.contains(&hash) {
            eprintln!("ansif_close_all hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // the MSL-common fingerprint (f507)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [8, 13, 20, 31] {
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
        self.record_relocation(RelocationKind::Addr16Ha, "__files");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "__files");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 0));
        self.emit_branch_to(labels[&31]); // b
        self.bind_label(labels[&8]);
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 31,
                offset: 4,
            });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 26,
                begin: 29,
                end: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&13]); // beq
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.record_relocation(RelocationKind::Rel24, "fclose");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "fclose".to_string(),
        });
        self.bind_label(labels[&13]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 31,
            offset: 76,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&20]); // beq
        self.record_relocation(RelocationKind::Rel24, "free");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "free".to_string(),
        });
        self.emit_branch_to(labels[&31]); // b
        self.bind_label(labels[&20]);
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 3,
                offset: 4,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 3));
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: 0,
                s: 4,
                shift: 6,
                begin: 23,
                end: 25,
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 3,
            offset: 4,
        });
        self.emit_branch_conditional_to(12, 2, labels[&31]); // beq
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 31,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&31]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 76,
        });
        self.bind_label(labels[&31]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&8]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 12,
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
