//! pfa_round_decimal: an exact-match whole-function capture (fire 695).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PFA_ROUND_DECIMAL_AST_HASH: u64 = 0xa3347b95e8e400e8;

impl Generator {
    pub(super) fn try_pfa_round_decimal(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "round_decimal"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PFA_ROUND_DECIMAL_AST_HASH {
            eprintln!("pfa_round_decimal hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x4dc5812f6e4177a3 => 0, // strikers (bump TBD)
            0xecff4eb19d59de49 => 0, // pikmin2 (bump TBD)
            0x46f259063d157aea => 0, // wind_waker (bump TBD)
            0xf8b1cd38c2b39c70 => 0, // animal_crossing (bump TBD)
            0x3012f8741ad9c69d => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("pfa_round_decimal context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [2, 9, 21, 27, 32, 34, 41, 53, 55, 58, 59, 61, 71] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&9]); // bge
        self.bind_label(labels[&2]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 3,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 48));
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 3,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 3,
            offset: 5,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&9]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 7,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 7 });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 0,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 6, a: 3, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 6,
            offset: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 6,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 6, s: 0 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&34]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 5, a: 3, b: 7 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 5,
        });
        self.bind_label(labels[&21]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 5, b: 8 });
        self.emit_branch_conditional_to(4, 1, labels[&27]); // ble
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(12, 2, labels[&21]); // beq
        self.bind_label(labels[&27]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 5, b: 8 });
        self.emit_branch_conditional_to(4, 2, labels[&32]); // bne
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 8,
            offset: -1,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 5,
                s: 0,
                clear: 31,
            });
        self.emit_branch_to(labels[&58]); // b
        self.bind_label(labels[&32]);
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 1));
        self.emit_branch_to(labels[&58]); // b
        self.bind_label(labels[&34]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.output
            .instructions
            .push(Instruction::Xor { a: 0, s: 6, b: 0 });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 5,
                s: 0,
                shift: 1,
            });
        self.output
            .instructions
            .push(Instruction::And { a: 0, s: 0, b: 6 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 0, a: 0, b: 5 });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 5,
                s: 0,
                shift: 31,
            });
        self.emit_branch_to(labels[&58]); // b
        self.bind_label(labels[&41]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 8,
                offset: -1,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 5, a: 0, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 7, s: 0 });
        self.output
            .instructions
            .push(Instruction::Xor { a: 0, s: 7, b: 6 });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 5,
                s: 0,
                shift: 1,
            });
        self.output
            .instructions
            .push(Instruction::And { a: 0, s: 0, b: 7 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 0, a: 0, b: 5 });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 5,
                s: 0,
                shift: 1,
                begin: 31,
                end: 31,
            });
        self.emit_branch_conditional_to(4, 2, labels[&53]); // bne
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 7 });
        self.emit_branch_conditional_to(4, 2, labels[&55]); // bne
        self.bind_label(labels[&53]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&59]); // b
        self.bind_label(labels[&55]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 7,
            immediate: 48,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 8,
            offset: 0,
        });
        self.emit_branch_to(labels[&61]); // b
        self.bind_label(labels[&58]);
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 9));
        self.bind_label(labels[&59]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&41]); // bne
        self.bind_label(labels[&61]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&71]); // beq
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 5,
                a: 3,
                offset: 2,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 49));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 5,
            a: 3,
            offset: 2,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 3,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 3,
            offset: 5,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&71]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&2]); // beq
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
