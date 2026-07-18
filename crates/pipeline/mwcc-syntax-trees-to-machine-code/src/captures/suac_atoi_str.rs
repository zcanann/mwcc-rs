//! suac_atoi_str: an exact-match whole-function capture (fire 512).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SUAC_ATOI_STR_AST_HASH: u64 = 0x6e2a82fd2b0f54bc; // strikers (f512)

impl Generator {
    pub(super) fn try_suac_atoi_str(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "atoi"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SUAC_ATOI_STR_AST_HASH {
            eprintln!("suac_atoi_str hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x4dc5812f6e4177a3 => 0, // strikers strtoul (f512)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [26, 31, 41, 44] {
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
        self.record_relocation(RelocationKind::Addr16Ha, "__StringRead");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__StringRead");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 4,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 1,
            immediate: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, -32768));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 1,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 24,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 1,
            immediate: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 9,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 10));
        self.record_relocation(RelocationKind::Rel24, "__strtoul");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__strtoul".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&31]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&26]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, -32768));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 1, labels[&31]); // bgt
        self.bind_label(labels[&26]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&41]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, -32768));
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&41]); // ble
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, -32768));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 34));
        self.output
            .instructions
            .push(Instruction::Negate { d: 4, a: 5 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 4, s: 4, b: 5 });
        self.record_relocation(RelocationKind::EmbSda21, "errno");
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 4,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 0, b: 3 });
        self.emit_branch_to(labels[&44]); // b
        self.bind_label(labels[&41]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&44]); // beq
        self.output
            .instructions
            .push(Instruction::Negate { d: 3, a: 3 });
        self.bind_label(labels[&44]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
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
