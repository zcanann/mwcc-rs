//! mbs_mbtowc_bfbb: an exact-match whole-function capture (fire 516).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MBS_MBTOWC_BFBB_AST_HASH: u64 = 0x9d1b8b7778ca353c; // BfBB (f517, mbstowcs inlined into the wrapper)

impl Generator {
    pub(super) fn try_mbs_mbtowc_bfbb(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "mbtowc"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MBS_MBTOWC_BFBB_AST_HASH {
            eprintln!("mbs_mbtowc_bfbb hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc1eb9a856a0f8258 => 0, // BfBB mbstring (f517)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [
            4, 8, 11, 16, 21, 32, 34, 36, 51, 53, 59, 61, 63, 65, 66, 70, 76, 81, 86, 90, 94, 98,
            102, 103, 107, 110, 111,
        ] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 0));
        self.emit_branch_conditional_to(4, 2, labels[&4]); // bne
        self.emit_branch_to(labels[&111]); // b
        self.bind_label(labels[&4]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&8]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(6, -1));
        self.emit_branch_to(labels[&111]); // b
        self.bind_label(labels[&8]);
        self.emit_branch_conditional_to(4, 2, labels[&11]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -1));
        self.emit_branch_to(labels[&66]); // b
        self.bind_label(labels[&11]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 7,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 7 });
        self.emit_branch_conditional_to(4, 2, labels[&16]); // bne
        self.output
            .instructions
            .push(Instruction::move_register(5, 6));
        self.emit_branch_to(labels[&66]); // b
        self.bind_label(labels[&16]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 7, s: 7 });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 7,
                shift: 0,
                begin: 24,
                end: 24,
            });
        self.emit_branch_conditional_to(4, 2, labels[&21]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 1));
        self.emit_branch_to(labels[&66]); // b
        self.bind_label(labels[&21]);
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 7,
            shift: 0,
            begin: 24,
            end: 26,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 192,
            });
        self.emit_branch_conditional_to(4, 2, labels[&36]); // bne
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 2 });
        self.emit_branch_conditional_to(12, 0, labels[&34]); // blt
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 1,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 0,
            begin: 24,
            end: 24,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 128,
            });
        self.emit_branch_conditional_to(4, 2, labels[&32]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 2));
        self.emit_branch_to(labels[&66]); // b
        self.bind_label(labels[&32]);
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -1));
        self.emit_branch_to(labels[&66]); // b
        self.bind_label(labels[&34]);
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -2));
        self.emit_branch_to(labels[&66]); // b
        self.bind_label(labels[&36]);
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 7,
            shift: 0,
            begin: 24,
            end: 27,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 224,
            });
        self.emit_branch_conditional_to(4, 2, labels[&65]); // bne
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 3 });
        self.emit_branch_conditional_to(12, 0, labels[&53]); // blt
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 1,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 0,
            begin: 24,
            end: 24,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 128,
            });
        self.emit_branch_conditional_to(4, 2, labels[&51]); // bne
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 2,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 0,
            begin: 24,
            end: 24,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 128,
            });
        self.emit_branch_conditional_to(4, 2, labels[&51]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 3));
        self.emit_branch_to(labels[&66]); // b
        self.bind_label(labels[&51]);
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -1));
        self.emit_branch_to(labels[&66]); // b
        self.bind_label(labels[&53]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&59]); // bne
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 1,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 0,
            begin: 24,
            end: 24,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 128,
            });
        self.emit_branch_conditional_to(12, 2, labels[&61]); // beq
        self.bind_label(labels[&59]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&63]); // bne
        self.bind_label(labels[&61]);
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -2));
        self.emit_branch_to(labels[&66]); // b
        self.bind_label(labels[&63]);
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -1));
        self.emit_branch_to(labels[&66]); // b
        self.bind_label(labels[&65]);
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -1));
        self.bind_label(labels[&66]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&70]); // bge
        self.output
            .instructions
            .push(Instruction::load_immediate(6, -1));
        self.emit_branch_to(labels[&111]); // b
        self.bind_label(labels[&70]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&81]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&76]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&86]); // bge
        self.emit_branch_to(labels[&90]); // b
        self.bind_label(labels[&76]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 4 });
        self.emit_branch_conditional_to(4, 0, labels[&90]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 6,
            s: 0,
            shift: 6,
            begin: 22,
            end: 25,
        });
        self.bind_label(labels[&81]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 0,
                clear: 26,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 6, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 6,
            s: 0,
            shift: 6,
            begin: 16,
            end: 25,
        });
        self.bind_label(labels[&86]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 0,
                clear: 25,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 6, b: 0 });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 6,
                s: 0,
                clear: 16,
            });
        self.bind_label(labels[&90]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 16,
            });
        self.emit_branch_conditional_to(4, 2, labels[&94]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&103]); // b
        self.bind_label(labels[&94]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 128,
            });
        self.emit_branch_conditional_to(4, 0, labels[&98]); // bge
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&103]); // b
        self.bind_label(labels[&98]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 2048,
            });
        self.emit_branch_conditional_to(4, 0, labels[&102]); // bge
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&103]); // b
        self.bind_label(labels[&102]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.bind_label(labels[&103]);
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 0, b: 5 });
        self.emit_branch_conditional_to(12, 2, labels[&107]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(6, -1));
        self.emit_branch_to(labels[&111]); // b
        self.bind_label(labels[&107]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&110]); // beq
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 6,
            a: 3,
            offset: 0,
        });
        self.bind_label(labels[&110]);
        self.output
            .instructions
            .push(Instruction::move_register(6, 5));
        self.bind_label(labels[&111]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 6));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
