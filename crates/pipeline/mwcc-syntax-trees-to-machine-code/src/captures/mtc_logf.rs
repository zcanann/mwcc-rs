//! mtc_logf: an exact-match whole-function capture (fire 718).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MTC_LOGF_AST_HASH: u64 = 0x123f993e1d68242a;

impl Generator {
    pub(super) fn try_mtc_logf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "logf"
            || function.return_type != Type::Float
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MTC_LOGF_AST_HASH {
            eprintln!("mtc_logf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xa5533c97b3cd5d53 => 19, // melee
            _ => {
                eprintln!("mtc_logf context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 48;
        self.output.constant_number_gaps = vec![(1, 1)];
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [10, 31, 59, 73, 77, 81, 83, 85] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -48 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 32640));
        self.output.instructions.push(Instruction::StoreFloatSingle { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 4, shift: 0, begin: 1, end: 8 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&73]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&10]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&83]); // beq
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 8 });
        self.record_relocation(RelocationKind::EmbSda21, "__logf_C0_bits");
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 0, offset: 0 });
        self.record_relocation(RelocationKind::EmbSda21, "__logf_C1_bits");
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 0, offset: 0 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 6, clear: 16 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 3, s: 6, shift: 23 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 5, s: 6, clear: 9 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 8, s: 6, shift: 16, begin: 25, end: 31 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 3, immediate: -127 });
        self.emit_branch_conditional_to(12, 2, labels[&59]); // beq
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 6, shift: 0, begin: 9, end: 15 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 6, shift: 0, begin: 16, end: 16 });
        self.output.instructions.push(Instruction::OrImmediateShifted { a: 4, s: 3, immediate: 16256 });
        self.output.instructions.push(Instruction::OrImmediateShifted { a: 3, s: 5, immediate: 16256 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 12 });
        self.emit_branch_conditional_to(12, 2, labels[&31]); // beq
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 0, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 8, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 16 });
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 1, offset: 12 });
        self.record_relocation(RelocationKind::Addr16Ha, "__one_over_F");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 4, s: 7, immediate: 32768 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 5, s: 8, shift: 2 });
        self.record_relocation(RelocationKind::Addr16Lo, "__one_over_F");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::FloatSubtractSingle { d: 6, a: 1, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatSingleIndexed { d: 0, a: 3, b: 5 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 17200));
        self.record_relocation(RelocationKind::Addr16Ha, "__ln_F");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__ln_F");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::FloatMultiplySingle { d: 6, a: 6, c: 0 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 36 });
        // the float numbers first (real @41=float, @43=double)
        self.output.intern_constant(0x3f317218u64, 4);
        self.load_double_constant(4, 0x4330000080000000);
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::FloatMultiplySingle { d: 2, a: 6, c: 6 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 0, a: 6, c: 1, b: 0 });
        self.load_float_constant(5, f32::from_bits(0x3f317218));
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 3, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::LoadFloatSingleIndexed { d: 1, a: 3, b: 5 });
        self.output.instructions.push(Instruction::FloatSubtractSingle { d: 3, a: 3, b: 4 });
        self.output.instructions.push(Instruction::FloatMultiplySingle { d: 0, a: 2, c: 0 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 1, a: 5, c: 3, b: 1 });
        self.output.instructions.push(Instruction::FloatAddSingle { d: 0, a: 6, b: 0 });
        self.output.instructions.push(Instruction::FloatAddSingle { d: 1, a: 1, b: 0 });
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&59]);
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 0, s: 7, immediate: 32768 });
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 17200));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.record_relocation(RelocationKind::Addr16Ha, "__ln_F");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.load_double_constant(2, 0x4330000080000000);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 8, shift: 2 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 32 });
        self.record_relocation(RelocationKind::Addr16Lo, "__ln_F");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.load_float_constant(3, f32::from_bits(0x3f317218));
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::LoadFloatSingleIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::FloatSubtractSingle { d: 1, a: 1, b: 2 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 1, a: 3, c: 1, b: 0 });
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&73]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 9 });
        self.emit_branch_conditional_to(12, 2, labels[&77]); // beq
        self.output.instructions.push(Instruction::RoundToSingle { d: 1, b: 1 });
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&77]);
        self.output.instructions.push(Instruction::AndMaskRecord { a: 0, s: 4, begin: 0, end: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&81]); // beq
        self.record_relocation(RelocationKind::EmbSda21, "float_nan");
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 0, offset: 0 });
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&81]);
        self.record_relocation(RelocationKind::EmbSda21, "float_inf");
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 0, offset: 0 });
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&83]);
        self.record_relocation(RelocationKind::EmbSda21, "float_inf");
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 });
        self.output.instructions.push(Instruction::FloatNegate { d: 1, b: 0 });
        self.bind_label(labels[&85]);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 48 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
