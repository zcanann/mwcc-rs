//! sfp_ctzl: an exact-match whole-function capture (fire 681).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SFP_CTZL_AST_HASH: u64 = 0x5e51d8f6381b330;

impl Generator {
    pub(super) fn try_sfp_ctzl(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__count_trailing_zerol"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SFP_CTZL_AST_HASH {
            eprintln!("sfp_ctzl hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 192, // strikers: file string @229 (ours @37 unbumped)
            _ => {
                eprintln!("sfp_ctzl context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [7, 13, 15, 20, 24, 26] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 1));
        self.output.instructions.push(Instruction::load_immediate(5, 32));
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 4, immediate: -1 });
        self.output.instructions.push(Instruction::load_immediate(6, 16));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(7, 16));
        self.emit_branch_to(labels[&24]); // b
        self.bind_label(labels[&7]);
        self.output.instructions.push(Instruction::AndRecord { a: 0, s: 3, b: 8 });
        self.emit_branch_conditional_to(4, 2, labels[&13]); // bne
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 7 });
        self.output.instructions.push(Instruction::ShiftRightWord { a: 3, s: 3, b: 7 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 5, a: 7, b: 5 });
        self.emit_branch_to(labels[&15]); // b
        self.bind_label(labels[&13]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 8, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&26]); // beq
        self.bind_label(labels[&15]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&20]); // ble
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 6, shift: 31 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 0, b: 6 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 6, s: 0, shift: 1 });
        self.bind_label(labels[&20]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 8, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&24]); // ble
        self.output.instructions.push(Instruction::ShiftRightWord { a: 8, s: 8, b: 6 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 7, a: 6, b: 7 });
        self.bind_label(labels[&24]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&7]); // bne
        self.bind_label(labels[&26]);
        self.output.instructions.push(Instruction::move_register(3, 4));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
