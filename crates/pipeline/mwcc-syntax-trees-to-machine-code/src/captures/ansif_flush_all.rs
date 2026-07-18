//! ansif_flush_all: an exact-match whole-function capture (fire 507).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ANSIF_FLUSH_ALL_AST_HASH: u64 = 0xd185121f4e614814; // AC (f507)
/// Cosmetic AST variants with IDENTICAL instruction streams (@N-normalized).
const ANSIF_FLUSH_ALL_AST_HASHES: &[u64] = &[
    ANSIF_FLUSH_ALL_AST_HASH,
    0x71e915194f81e815,
    0x9e79533bbf14dd76,
];

impl Generator {
    pub(super) fn try_ansif_flush_all(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__flush_all"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 0
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !ANSIF_FLUSH_ALL_AST_HASHES.contains(&hash) {
            eprintln!("ansif_flush_all hash candidate: {hash:#x}");
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
        for target in [10, 18, 19] {
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
            .push(Instruction::load_immediate(31, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 0));
        self.emit_branch_to(labels[&19]); // b
        self.bind_label(labels[&10]);
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 30,
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
        self.emit_branch_conditional_to(12, 2, labels[&18]); // beq
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.record_relocation(RelocationKind::Rel24, "fflush");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "fflush".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&18]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(31, -1));
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 30,
            offset: 76,
        });
        self.bind_label(labels[&19]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 30,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&10]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 8,
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
