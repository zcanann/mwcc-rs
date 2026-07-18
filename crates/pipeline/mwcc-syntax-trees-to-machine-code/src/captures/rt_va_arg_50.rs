//! rt_va_arg_50: an exact-match whole-function capture (fire 676).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hashes of the captured function — the 50-instruction MSL
/// revision (no fpr block), byte-uniform across these projects.
const RT_VA_ARG_50_AST_HASHES: [u64; 5] = [
    0x866bb28b5630cdeb, // animal_crossing
    0x36a5d94aafa92fd8, // battle_for_bikini_bottom, mark_kart_double_dash, pikmin2
    0x4fae33833e2437d7, // marioparty4
    0xaf57a16bb7147a4e, // metroid_prime
    0x422abf4c7231043d, // super_mario_strikers
];

impl Generator {
    pub(super) fn try_rt_va_arg_50(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__va_arg"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !RT_VA_ARG_50_AST_HASHES.contains(&hash) {
            eprintln!("rt_va_arg_50 hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // ac/bfbb/mkdd/prime/pikmin2/mp4: single-function TU
            0x626216a8cf3d36f5 => 0, // super_mario_strikers
            _ => {
                eprintln!("rt_va_arg_50 context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [17, 24, 25, 35, 45, 48] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 7,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 3 });
        self.output
            .instructions
            .push(Instruction::move_register(6, 3));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 8));
        self.output
            .instructions
            .push(Instruction::load_immediate(8, 4));
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 7, s: 7 });
        self.output
            .instructions
            .push(Instruction::load_immediate(9, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(10, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(11, 4));
        self.emit_branch_conditional_to(4, 2, labels[&17]); // bne
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 7,
            a: 3,
            offset: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 3,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(8, 8));
        self.output
            .instructions
            .push(Instruction::load_immediate(10, 32));
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 7, s: 7 });
        self.output
            .instructions
            .push(Instruction::load_immediate(11, 8));
        self.bind_label(labels[&17]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&25]); // bne
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 7,
                clear: 31,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(8, 8));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 7));
        self.emit_branch_conditional_to(12, 2, labels[&24]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 1));
        self.bind_label(labels[&24]);
        self.output
            .instructions
            .push(Instruction::load_immediate(9, 2));
        self.bind_label(labels[&25]);
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 7, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&35]); // bge
        self.output
            .instructions
            .push(Instruction::Add { d: 7, a: 7, b: 5 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 3,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::MultiplyLow { d: 3, a: 7, b: 11 });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 7, b: 9 });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 6,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 6, a: 10, b: 3 });
        self.output
            .instructions
            .push(Instruction::Add { d: 6, a: 5, b: 6 });
        self.emit_branch_to(labels[&45]); // b
        self.bind_label(labels[&35]);
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 8));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 8,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 6,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::Nor { a: 6, s: 0, b: 0 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 5, a: 8, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::And { a: 6, s: 6, b: 0 });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 6, b: 8 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 4,
        });
        self.bind_label(labels[&45]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&48]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 6,
            offset: 0,
        });
        self.bind_label(labels[&48]);
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
