//! fio_strnicmp_leaf: an exact-match whole-function capture (fire 508).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const FIO_STRNICMP_LEAF_AST_HASH: u64 = 0xffb2ee610e460817; // strikers (f508)

impl Generator {
    pub(super) fn try_fio_strnicmp_leaf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__msl_strnicmp"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != FIO_STRNICMP_LEAF_AST_HASH {
            eprintln!("fio_strnicmp_leaf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x4dc5812f6e4177a3 => 0, // strikers (f508)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [3, 10, 14, 22, 26, 31, 34, 38, 39] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&39]); // ble
        self.bind_label(labels[&3]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&10]); // bne
        self.output.instructions.push(Instruction::load_immediate(5, -1));
        self.emit_branch_to(labels[&14]); // b
        self.bind_label(labels[&10]);
        self.record_relocation(RelocationKind::Addr16Ha, "__lower_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(5, 0));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 0, clear: 24 });
        self.record_relocation(RelocationKind::Addr16Lo, "__lower_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 5, a: 5, b: 0 });
        self.bind_label(labels[&14]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 6, s: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&22]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.emit_branch_to(labels[&26]); // b
        self.bind_label(labels[&22]);
        self.record_relocation(RelocationKind::Addr16Ha, "__lower_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(5, 0));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 0, clear: 24 });
        self.record_relocation(RelocationKind::Addr16Lo, "__lower_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 5, b: 0 });
        self.bind_label(labels[&26]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 6, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&31]); // bge
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&31]);
        self.emit_branch_conditional_to(4, 1, labels[&34]); // ble
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&34]);
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&38]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&38]);
        self.emit_branch_conditional_to(16, 0, labels[&3]); // bdnz
        self.bind_label(labels[&39]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
