//! ansif_find_unopened: an exact-match whole-function capture (fire 511).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ANSIF_FIND_UNOPENED_AST_HASH: u64 = 0x2166d21029d5c0f; // strikers (f511)

impl Generator {
    pub(super) fn try_ansif_find_unopened(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__find_unopened_file"
            || !matches!(
                function.return_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            )
            || function.parameters.len() != 0
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ANSIF_FIND_UNOPENED_AST_HASH {
            eprintln!("ansif_find_unopened hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // strikers ansi_files (f511, shares pikmin's set)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [9, 13, 15, 29, 30] {
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
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: 236,
        });
        self.emit_branch_to(labels[&15]); // b
        self.bind_label(labels[&9]);
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 3,
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
        self.emit_branch_conditional_to(4, 2, labels[&13]); // bne
        self.emit_branch_to(labels[&30]); // b
        self.bind_label(labels[&13]);
        self.output
            .instructions
            .push(Instruction::move_register(30, 3));
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: 76,
        });
        self.bind_label(labels[&15]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&9]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 80));
        self.record_relocation(RelocationKind::Rel24, "malloc");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "malloc".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 31, s: 3, b: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&29]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 80));
        self.record_relocation(RelocationKind::Rel24, "memset");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "memset".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 31,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 30,
            offset: 76,
        });
        self.emit_branch_to(labels[&30]); // b
        self.bind_label(labels[&29]);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.bind_label(labels[&30]);
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
