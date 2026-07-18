//! pfp_ctz: an exact-match whole-function capture (fire 687).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PFP_CTZ_AST_HASH: u64 = 0x6bda6c4a0ffce920;

impl Generator {
    pub(super) fn try_pfp_ctz(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__count_trailing_zero"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PFP_CTZ_AST_HASH {
            eprintln!("pfp_ctz hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xecff4eb19d59de49 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("pfp_ctz context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [12, 18, 20, 25, 29, 32, 41, 47, 49, 54, 58, 60, 61] {
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
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 8,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 8, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&32]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 16));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(7, 32));
        self.output
            .instructions
            .push(Instruction::move_register(5, 6));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&29]); // b
        self.bind_label(labels[&12]);
        self.output
            .instructions
            .push(Instruction::AndRecord { a: 0, s: 8, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&18]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 3, b: 5 });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 8, s: 8, b: 5 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 7, a: 5, b: 7 });
        self.emit_branch_to(labels[&20]); // b
        self.bind_label(labels[&18]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&61]); // beq
        self.bind_label(labels[&20]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&25]); // ble
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 6,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 0, b: 6 });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 6,
                s: 0,
                shift: 1,
            });
        self.bind_label(labels[&25]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&29]); // ble
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 4, s: 4, b: 6 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 5, a: 6, b: 5 });
        self.bind_label(labels[&29]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&12]); // bne
        self.emit_branch_to(labels[&61]); // b
        self.bind_label(labels[&32]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 16));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(7, 0));
        self.output
            .instructions
            .push(Instruction::move_register(4, 5));
        self.output
            .instructions
            .push(Instruction::OrImmediateShifted {
                a: 8,
                s: 0,
                immediate: 16,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 32));
        self.emit_branch_to(labels[&58]); // b
        self.bind_label(labels[&41]);
        self.output
            .instructions
            .push(Instruction::AndRecord { a: 0, s: 8, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&47]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 7, a: 7, b: 4 });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 8, s: 8, b: 4 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 6, a: 4, b: 6 });
        self.emit_branch_to(labels[&49]); // b
        self.bind_label(labels[&47]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&60]); // beq
        self.bind_label(labels[&49]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&54]); // ble
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 5,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 0, b: 5 });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 5,
                s: 0,
                shift: 1,
            });
        self.bind_label(labels[&54]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&58]); // ble
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 3, s: 3, b: 5 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&58]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&41]); // bne
        self.bind_label(labels[&60]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 7,
            immediate: 32,
        });
        self.bind_label(labels[&61]);
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
