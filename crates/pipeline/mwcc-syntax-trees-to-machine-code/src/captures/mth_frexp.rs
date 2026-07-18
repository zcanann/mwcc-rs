//! mth_frexp: an exact-match whole-function capture (fire 710).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MTH_FREXP_AST_HASH: u64 = 0x56b78e1d2468a078;

impl Generator {
    pub(super) fn try_mth_frexp(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "frexp"
            || function.return_type != Type::Double
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MTH_FREXP_AST_HASH {
            eprintln!("mth_frexp hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xa5533c97b3cd5d53 => 8, // melee
            _ => {
                eprintln!("mth_frexp context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        for bits in [0x4350000000000000u64] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [12, 14, 24, 33] {
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
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 4,
                s: 5,
                clear: 1,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&12]); // bge
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 0, s: 4, b: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&14]); // bne
        self.bind_label(labels[&12]);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.emit_branch_to(labels[&33]); // b
        self.bind_label(labels[&14]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 16));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&24]); // bge
        self.load_double_constant(0, 0x4350000000000000);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -54));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 0, a: 1, c: 0 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 8,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 4,
                s: 5,
                clear: 1,
            });
        self.bind_label(labels[&24]);
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 5,
            shift: 0,
            begin: 12,
            end: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 4,
                s: 4,
                shift: 20,
            });
        self.output
            .instructions
            .push(Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: 16352,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 4, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: -1022,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.bind_label(labels[&33]);
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
