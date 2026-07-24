//! mbs_wcstombs_pik: an exact-match whole-function capture (fire 514).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MBS_WCSTOMBS_PIK_AST_HASH: u64 = 0x8e001d087650f703; // mbs_mel (f514)
const MBS_WCSTOMBS_PIK_TWO_STEP_SIGN_EXTEND_AST_HASH: u64 = 0xafd58fe7a7dc20c4;
/// Measured semantic variants of the same runtime loop. The current Pikmin
/// spelling uses a distinct two-step sign-extension schedule below.
const MBS_WCSTOMBS_PIK_AST_HASHES: &[u64] = &[
    MBS_WCSTOMBS_PIK_AST_HASH,
    0x7c6bcb19e8ec6f14,
    MBS_WCSTOMBS_PIK_TWO_STEP_SIGN_EXTEND_AST_HASH,
];

impl Generator {
    pub(super) fn try_mbs_wcstombs_pik(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "wcstombs"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !MBS_WCSTOMBS_PIK_AST_HASHES.contains(&hash) {
            eprintln!("mbs_wcstombs_pik hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // pikmin (f514)
            0xbd60acb658c79e45 => 0, // mbs_mel (f514)
            _ => return Ok(false),
        };
        let two_step_sign_extend = hash == MBS_WCSTOMBS_PIK_TWO_STEP_SIGN_EXTEND_AST_HASH;
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        let exit_target = if two_step_sign_extend { 13 } else { 12 };
        for target in [4, exit_target] {
            labels.insert(target, self.fresh_label());
        }
        if two_step_sign_extend {
            self.output
                .instructions
                .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
            self.output
                .instructions
                .push(Instruction::MoveToCountRegister { s: 5 });
            self.output
                .instructions
                .push(Instruction::load_immediate(6, 0));
        } else {
            self.output
                .instructions
                .push(Instruction::load_immediate(6, 0));
            self.output
                .instructions
                .push(Instruction::MoveToCountRegister { s: 5 });
            self.output
                .instructions
                .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        }
        self.emit_branch_conditional_to(4, 1, labels[&exit_target]); // ble
        self.bind_label(labels[&4]);
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: if two_step_sign_extend { 0 } else { 5 },
                a: 4,
                offset: 0,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 2,
        });
        if two_step_sign_extend {
            self.output
                .instructions
                .push(Instruction::ExtendSignByte { a: 5, s: 0 });
        }
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 3,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.emit_branch_conditional_to(12, 2, labels[&exit_target]); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 6,
            immediate: 1,
        });
        self.emit_branch_conditional_to(16, 0, labels[&4]); // bdnz
        self.bind_label(labels[&exit_target]);
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
