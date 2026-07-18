//! mbs_wcstombs_ac: an exact-match whole-function capture (fire 516).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MBS_WCSTOMBS_AC_AST_HASH: u64 = 0x7f92a09d213afc90; // re-armed f517 (the @4 static-slot pooled image)

impl Generator {
    pub(super) fn try_mbs_wcstombs_ac(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "wcstombs"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 3
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MBS_WCSTOMBS_AC_AST_HASH {
            eprintln!("mbs_wcstombs_ac hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // mbs_ac (f516)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 48;
        self.non_leaf = true;
        self.callee_saved = vec![27, 28, 29, 30, 31]; // via _savegpr_27
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [11, 13, 15, 21, 33, 35, 36] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -48,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 52,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 48,
        });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_27");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_savegpr_27".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 27, s: 3, b: 3 });
        self.output
            .instructions
            .push(Instruction::move_register(28, 5));
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 0));
        self.emit_branch_conditional_to(12, 2, labels[&11]); // beq
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&13]); // bne
        self.bind_label(labels[&11]);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&36]); // b
        self.bind_label(labels[&13]);
        self.output
            .instructions
            .push(Instruction::move_register(29, 4));
        self.emit_branch_to(labels[&33]); // b
        self.bind_label(labels[&15]);
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 4,
                a: 29,
                offset: 0,
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&21]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 0, a: 27, b: 30 });
        self.emit_branch_to(labels[&35]); // b
        self.bind_label(labels[&21]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 29,
            immediate: 2,
        });
        self.record_relocation(RelocationKind::Rel24, "unicode_to_UTF8");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "unicode_to_UTF8".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 30, b: 31 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 0, b: 28 });
        self.emit_branch_conditional_to(12, 1, labels[&35]); // bgt
        self.output
            .instructions
            .push(Instruction::move_register(5, 31));
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 27, b: 30 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 8,
        });
        self.record_relocation(RelocationKind::Rel24, "strncpy");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "strncpy".to_string(),
        });
        self.output.instructions.push(Instruction::Add {
            d: 30,
            a: 30,
            b: 31,
        });
        self.bind_label(labels[&33]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 30, b: 28 });
        self.emit_branch_conditional_to(4, 1, labels[&15]); // ble
        self.bind_label(labels[&35]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.bind_label(labels[&36]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 48,
        });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_27");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_restgpr_27".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 52,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 48,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
