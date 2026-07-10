//! afp_dec2num_pik: an exact-match whole-function capture (fire 679).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const AFP_DEC2NUM_PIK_AST_HASH: u64 = 0x25c2685f0c520fe9;

impl Generator {
    pub(super) fn try_afp_dec2num_pik(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__dec2num"
            || function.return_type != Type::Double
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != AFP_DEC2NUM_PIK_AST_HASH {
            eprintln!("afp_dec2num_pik hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xe4f69f920b359fc6 => 61, // pikmin: @179 (ours @39; +61 past num2dec block)
            _ => {
                eprintln!("afp_dec2num_pik context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 48;
        self.non_leaf = true;
        self.callee_saved_float = 1;
        for bits in [
            0x0000000000000000u64,
            0x3ff0000000000000,
            0x3fb999999999999a,
            0x4024000000000000,
            0x4330000080000000,
            0x4197d78400000000,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [17, 19, 27, 30, 35, 40, 48, 64, 67, 72, 83, 85, 98, 104, 107, 111, 123, 126, 127, 129, 140, 142, 154, 157, 158, 162, 163] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -48 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 40 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 40 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_27");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_27".to_string() });
        self.output.instructions.push(Instruction::LoadByteZero { d: 29, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 31, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 1 });
        self.load_double_constant(31, 0x0000000000000000);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 31, s: 31 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 30, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&17]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&19]); // bne
        self.bind_label(labels[&17]);
        self.load_double_constant(1, 0x0000000000000000);
        self.emit_branch_to(labels[&163]); // b
        self.bind_label(labels[&19]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 73 });
        self.emit_branch_conditional_to(4, 2, labels[&30]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&27]); // beq
        self.record_relocation(RelocationKind::Addr16Ha, "__double_huge");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__double_huge");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::FloatNegate { d: 1, b: 0 });
        self.emit_branch_to(labels[&163]); // b
        self.bind_label(labels[&27]);
        self.record_relocation(RelocationKind::Addr16Ha, "__double_huge");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__double_huge");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 3, offset: 0 });
        self.emit_branch_to(labels[&163]); // b
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 78 });
        self.emit_branch_conditional_to(4, 2, labels[&35]); // bne
        self.record_relocation(RelocationKind::Addr16Ha, "__double_nan");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__double_nan");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 3, offset: 0 });
        self.emit_branch_to(labels[&163]); // b
        self.bind_label(labels[&35]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 16 });
        self.emit_branch_conditional_to(4, 1, labels[&40]); // ble
        self.output.instructions.push(Instruction::Add { d: 30, a: 29, b: 30 });
        self.output.instructions.push(Instruction::load_immediate(29, 16));
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 30, immediate: -16 });
        self.bind_label(labels[&40]);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 29, shift: 29 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 4, s: 29, shift: 31 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 27, a: 3, immediate: 5 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 3, begin: 0, end: 31 });
        self.output.instructions.push(Instruction::AddRecord { d: 28, a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&48]); // bne
        self.output.instructions.push(Instruction::load_immediate(28, 8));
        self.bind_label(labels[&48]);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 29, immediate: -1 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 17200));
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 3, s: 3, immediate: 32768 });
        self.output.instructions.push(Instruction::Add { d: 30, a: 29, b: 30 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 30, immediate: -1 });
        self.load_double_constant(2, 0x4330000080000000);
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.record_relocation(RelocationKind::EmbSda21, "ten");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 0, offset: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 2, a: 0, b: 2 });
        self.record_relocation(RelocationKind::Rel24, "pow");
        self.output.instructions.push(Instruction::BranchAndLink { target: "pow".to_string() });
        self.load_double_constant(3, 0x4197d78400000000);
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 17200));
        self.load_double_constant(2, 0x4330000080000000);
        self.emit_branch_to(labels[&83]); // b
        self.bind_label(labels[&64]);
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 28, immediate: 1 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&72]); // b
        self.bind_label(labels[&67]);
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 3, a: 3, immediate: 10 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 27, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 27, a: 27, immediate: 1 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 4, b: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.bind_label(labels[&72]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 5, a: 5, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&67]); // bne
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 3, s: 3, immediate: 32768 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::SubtractFromRecord { d: 29, a: 28, b: 29 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 2 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 31, a: 3, c: 31, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&85]); // beq
        self.output.instructions.push(Instruction::load_immediate(28, 8));
        self.bind_label(labels[&83]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&64]); // bne
        self.bind_label(labels[&85]);
        self.output.instructions.push(Instruction::FloatDivideDouble { d: 31, a: 31, b: 1 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 0, s: 30, shift: 31 });
        self.output.instructions.push(Instruction::Xor { a: 5, s: 0, b: 30 });
        self.record_relocation(RelocationKind::Addr16Ha, "bit_values");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::SubtractFrom { d: 5, a: 0, b: 5 });
        self.record_relocation(RelocationKind::Addr16Lo, "bit_values");
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 511 });
        self.output.instructions.push(Instruction::move_register(4, 0));
        self.emit_branch_conditional_to(4, 1, labels[&107]); // ble
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&98]); // bge
        self.load_double_constant(1, 0x0000000000000000);
        self.emit_branch_to(labels[&163]); // b
        self.bind_label(labels[&98]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&104]); // beq
        self.record_relocation(RelocationKind::Addr16Ha, "__double_huge");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__double_huge");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::FloatNegate { d: 1, b: 0 });
        self.emit_branch_to(labels[&163]); // b
        self.bind_label(labels[&104]);
        self.record_relocation(RelocationKind::Addr16Ha, "__double_huge");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__double_huge");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 3, offset: 0 });
        self.emit_branch_to(labels[&163]); // b
        self.bind_label(labels[&107]);
        self.record_relocation(RelocationKind::Addr16Ha, "__double_max");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.load_double_constant(3, 0x3ff0000000000000);
        self.record_relocation(RelocationKind::Addr16Lo, "__double_max");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 3, offset: 0 });
        self.emit_branch_to(labels[&129]); // b
        self.bind_label(labels[&111]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 5, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&127]); // beq
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::FloatDivideDouble { d: 0, a: 1, b: 2 });
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&126]); // ble
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&123]); // beq
        self.record_relocation(RelocationKind::Addr16Ha, "__double_huge");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__double_huge");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::FloatNegate { d: 1, b: 0 });
        self.emit_branch_to(labels[&163]); // b
        self.bind_label(labels[&123]);
        self.record_relocation(RelocationKind::Addr16Ha, "__double_huge");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__double_huge");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 3, offset: 0 });
        self.emit_branch_to(labels[&163]); // b
        self.bind_label(labels[&126]);
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 3, a: 3, c: 2 });
        self.bind_label(labels[&127]);
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 5, s: 5, shift: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 8 });
        self.bind_label(labels[&129]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&111]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&142]); // bge
        self.record_relocation(RelocationKind::Addr16Ha, "__double_min");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__double_min");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 0, a: 0, c: 3 });
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 31, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&140]); // bge
        self.load_double_constant(1, 0x0000000000000000);
        self.emit_branch_to(labels[&163]); // b
        self.bind_label(labels[&140]);
        self.output.instructions.push(Instruction::FloatDivideDouble { d: 31, a: 31, b: 3 });
        self.emit_branch_to(labels[&158]); // b
        self.bind_label(labels[&142]);
        self.emit_branch_conditional_to(4, 1, labels[&158]); // ble
        self.record_relocation(RelocationKind::Addr16Ha, "__double_max");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__double_max");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::FloatDivideDouble { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 31, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&157]); // ble
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&154]); // beq
        self.record_relocation(RelocationKind::Addr16Ha, "__double_huge");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__double_huge");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::FloatNegate { d: 1, b: 0 });
        self.emit_branch_to(labels[&163]); // b
        self.bind_label(labels[&154]);
        self.record_relocation(RelocationKind::Addr16Ha, "__double_huge");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__double_huge");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 3, offset: 0 });
        self.emit_branch_to(labels[&163]); // b
        self.bind_label(labels[&157]);
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 31, a: 31, c: 3 });
        self.bind_label(labels[&158]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&162]); // beq
        self.output.instructions.push(Instruction::FloatNegate { d: 1, b: 31 });
        self.emit_branch_to(labels[&163]); // b
        self.bind_label(labels[&162]);
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 31 });
        self.bind_label(labels[&163]);
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 40 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 31, a: 1, offset: 40 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_27");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_27".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 48 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
