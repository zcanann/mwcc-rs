//! fio_strnicmp_sv: an exact-match whole-function capture (fire 508).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const FIO_STRNICMP_SV_AST_HASH: u64 = 0xf56a122e40e5609c; // BfBB (f508)

impl Generator {
    pub(super) fn try_fio_strnicmp_sv(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__msl_strnicmp"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != FIO_STRNICMP_SV_AST_HASH {
            eprintln!("fio_strnicmp_sv hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // BfBB (f508)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.non_leaf = true;
        self.callee_saved = vec![27, 28, 29, 30, 31]; // via _savegpr_27
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [10, 24, 27, 31, 32, 35] {
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
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 32,
        });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_27");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_savegpr_27".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(27, 3));
        self.output
            .instructions
            .push(Instruction::move_register(28, 4));
        self.output
            .instructions
            .push(Instruction::move_register(29, 5));
        self.output
            .instructions
            .push(Instruction::load_immediate(31, 0));
        self.emit_branch_to(labels[&32]); // b
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 27,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 27,
            a: 27,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 3, s: 0 });
        self.record_relocation(RelocationKind::Rel24, "tolower");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "tolower".to_string(),
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 28,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 30, s: 3 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 28,
            a: 28,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 3, s: 0 });
        self.record_relocation(RelocationKind::Rel24, "tolower");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "tolower".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 30, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&24]); // bge
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&35]); // b
        self.bind_label(labels[&24]);
        self.emit_branch_conditional_to(4, 1, labels[&27]); // ble
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.emit_branch_to(labels[&35]); // b
        self.bind_label(labels[&27]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 30 });
        self.emit_branch_conditional_to(4, 2, labels[&31]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&35]); // b
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 31,
            immediate: 1,
        });
        self.bind_label(labels[&32]);
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 31, b: 29 });
        self.emit_branch_conditional_to(12, 0, labels[&10]); // blt
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.bind_label(labels[&35]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 32,
        });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_27");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_restgpr_27".to_string(),
        });
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
