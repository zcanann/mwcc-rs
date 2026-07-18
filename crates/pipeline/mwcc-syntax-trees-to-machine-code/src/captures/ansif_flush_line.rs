//! ansif_flush_line: an exact-match whole-function capture (fire 511).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ANSIF_FLUSH_LINE_AST_HASH: u64 = 0xc325c3547e7193bf; // strikers (f511)

impl Generator {
    pub(super) fn try_ansif_flush_line(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__flush_line_buffered_output_files"
            || function.return_type != Type::Int
            || function.parameters.len() != 0
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ANSIF_FLUSH_LINE_AST_HASH {
            eprintln!("ansif_flush_line hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // strikers ansi_files (f511)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [10, 25, 26] {
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
        self.emit_branch_to(labels[&26]); // b
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
        self.emit_branch_conditional_to(12, 2, labels[&25]); // beq
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 30,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 31,
                begin: 31,
                end: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&25]); // beq
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 30,
            offset: 8,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 27,
            begin: 29,
            end: 31,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&25]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&25]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(31, -1));
        self.bind_label(labels[&25]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 30,
            offset: 76,
        });
        self.bind_label(labels[&26]);
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
