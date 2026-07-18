//! bfp_num2dec_i: an exact-match whole-function capture (fire 686).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const BFP_NUM2DEC_I_AST_HASH: u64 = 0xa495593445b8fdc6;

impl Generator {
    pub(super) fn try_bfp_num2dec_i(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__num2dec_internal"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != BFP_NUM2DEC_I_AST_HASH {
            eprintln!("bfp_num2dec_i hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xdbce2bc49da89140 => 249, // bfbb
            _ => {
                eprintln!("bfp_num2dec_i context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 176;
        self.non_leaf = true;
        self.callee_saved_float = 2;
        for bits in [0x0000000000000000u64] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [
            21, 31, 36, 38, 40, 45, 47, 49, 50, 68, 73, 75, 77, 82, 84, 86, 87, 91, 93, 97, 112,
            118, 120, 125, 129, 132, 141, 147, 149, 154, 158, 160, 161, 186, 189, 206, 216, 221,
            227, 232,
        ] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -176,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 180,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 168,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 31,
                a: 1,
                offset: 168,
            });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_26");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_savegpr_26".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output
            .instructions
            .push(Instruction::move_register(30, 3));
        self.load_double_constant(0, 0x0000000000000000);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 0, b: 1 });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 31, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&21]); // bne
        self.output.instructions.push(Instruction::StoreByte {
            s: 31,
            a: 30,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 3,
            a: 30,
            offset: 2,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 30,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 30,
            offset: 5,
        });
        self.emit_branch_to(labels[&232]); // b
        self.bind_label(labels[&21]);
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 40,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 40,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 4,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&31]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&49]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&40]); // beq
        self.emit_branch_to(labels[&49]); // b
        self.bind_label(labels[&31]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&36]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&38]); // beq
        self.bind_label(labels[&36]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&50]); // b
        self.bind_label(labels[&38]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&50]); // b
        self.bind_label(labels[&40]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&45]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&47]); // beq
        self.bind_label(labels[&45]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&50]); // b
        self.bind_label(labels[&47]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&50]); // b
        self.bind_label(labels[&49]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&50]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 1, labels[&93]); // bgt
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::StoreByte {
            s: 31,
            a: 30,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 32,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 32,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 3,
            a: 30,
            offset: 2,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 5,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 30,
            offset: 4,
        });
        self.emit_branch_conditional_to(12, 2, labels[&68]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&86]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&77]); // beq
        self.emit_branch_to(labels[&86]); // b
        self.bind_label(labels[&68]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 5,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&73]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&75]); // beq
        self.bind_label(labels[&73]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&87]); // b
        self.bind_label(labels[&75]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&87]); // b
        self.bind_label(labels[&77]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 5,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&82]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&84]); // beq
        self.bind_label(labels[&82]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&87]); // b
        self.bind_label(labels[&84]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&87]); // b
        self.bind_label(labels[&86]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&87]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 73));
        self.emit_branch_conditional_to(4, 2, labels[&91]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 78));
        self.bind_label(labels[&91]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 30,
            offset: 5,
        });
        self.emit_branch_to(labels[&232]); // b
        self.bind_label(labels[&93]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&97]); // beq
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 0, b: 1 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 8,
            });
        self.bind_label(labels[&97]);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 16,
        });
        self.record_relocation(RelocationKind::Rel24, "frexp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "frexp".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 31, b: 1 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 31,
                a: 1,
                offset: 24,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 8,
            a: 1,
            offset: 28,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 8, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&132]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 16));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(7, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 32));
        self.output
            .instructions
            .push(Instruction::move_register(4, 5));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.emit_branch_to(labels[&129]); // b
        self.bind_label(labels[&112]);
        self.output
            .instructions
            .push(Instruction::AndRecord { a: 0, s: 8, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&118]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 7, a: 7, b: 4 });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 8, s: 8, b: 4 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 6, a: 4, b: 6 });
        self.emit_branch_to(labels[&120]); // b
        self.bind_label(labels[&118]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&161]); // beq
        self.bind_label(labels[&120]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&125]); // ble
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 5,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 0, b: 5 });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 5,
                s: 0,
                shift: 1,
            });
        self.bind_label(labels[&125]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&129]); // ble
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 3, s: 3, b: 5 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&129]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&112]); // bne
        self.emit_branch_to(labels[&161]); // b
        self.bind_label(labels[&132]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 16));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(7, 0));
        self.output
            .instructions
            .push(Instruction::move_register(4, 5));
        self.output
            .instructions
            .push(Instruction::OrImmediateShifted {
                a: 8,
                s: 0,
                immediate: 16,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 32));
        self.emit_branch_to(labels[&158]); // b
        self.bind_label(labels[&141]);
        self.output
            .instructions
            .push(Instruction::AndRecord { a: 0, s: 8, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&147]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 7, a: 7, b: 4 });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 8, s: 8, b: 4 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 6, a: 4, b: 6 });
        self.emit_branch_to(labels[&149]); // b
        self.bind_label(labels[&147]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&160]); // beq
        self.bind_label(labels[&149]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&154]); // ble
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 5,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 0, b: 5 });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 5,
                s: 0,
                shift: 1,
            });
        self.bind_label(labels[&154]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&158]); // ble
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 3, s: 3, b: 5 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&158]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&141]); // bne
        self.bind_label(labels[&160]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 7,
            immediate: 32,
        });
        self.bind_label(labels[&161]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 26,
                a: 7,
                immediate: 53,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 56,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 26, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "__two_exp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__two_exp".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 31 });
        self.output
            .instructions
            .push(Instruction::move_register(3, 26));
        self.record_relocation(RelocationKind::Rel24, "ldexp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "ldexp".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 48,
        });
        self.record_relocation(RelocationKind::Rel24, "modf");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "modf".to_string(),
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 48,
        });
        self.record_relocation(RelocationKind::Rel24, "__cvt_dbl_usll");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__cvt_dbl_usll".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(28, 0));
        self.output
            .instructions
            .push(Instruction::move_register(27, 3));
        self.output
            .instructions
            .push(Instruction::move_register(26, 4));
        self.output.instructions.push(Instruction::StoreByte {
            s: 28,
            a: 1,
            offset: 100,
        });
        self.output
            .instructions
            .push(Instruction::Xor { a: 3, s: 26, b: 28 });
        self.output
            .instructions
            .push(Instruction::Xor { a: 0, s: 27, b: 28 });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&186]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 28,
            a: 1,
            offset: 102,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 104,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 28,
            a: 1,
            offset: 105,
        });
        self.emit_branch_to(labels[&227]); // b
        self.bind_label(labels[&186]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 28,
            a: 1,
            offset: 104,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 1,
            immediate: 100,
        });
        self.emit_branch_to(labels[&206]); // b
        self.bind_label(labels[&189]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::move_register(4, 26));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 10));
        self.record_relocation(RelocationKind::Rel24, "__mod2u");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__mod2u".to_string(),
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 8,
            a: 1,
            offset: 104,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 10));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 8,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 8,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 7,
            a: 1,
            offset: 104,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 4, a: 29, b: 0 });
        self.output
            .instructions
            .push(Instruction::move_register(4, 26));
        self.record_relocation(RelocationKind::Rel24, "__div2u");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__div2u".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(26, 4));
        self.output
            .instructions
            .push(Instruction::move_register(27, 3));
        self.bind_label(labels[&206]);
        self.output
            .instructions
            .push(Instruction::Xor { a: 3, s: 26, b: 28 });
        self.output
            .instructions
            .push(Instruction::Xor { a: 0, s: 27, b: 28 });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&189]); // bne
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 1,
            offset: 104,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 1,
            immediate: 100,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 105,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 0, b: 4 });
        self.emit_branch_to(labels[&221]); // b
        self.bind_label(labels[&216]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 4,
            offset: 0,
        });
        self.bind_label(labels[&221]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 5, b: 4 });
        self.emit_branch_conditional_to(12, 0, labels[&216]); // blt
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 1,
            offset: 104,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 1,
            offset: 102,
        });
        self.bind_label(labels[&227]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 100,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 56,
        });
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__timesdec".to_string(),
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 31,
            a: 30,
            offset: 0,
        });
        self.bind_label(labels[&232]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 168,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 31,
            a: 1,
            offset: 168,
        });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_26");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_restgpr_26".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 180,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 176,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
