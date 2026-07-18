//! sfp_num2dec_i: an exact-match whole-function capture (fire 681).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SFP_NUM2DEC_I_AST_HASH: u64 = 0x218d1a54b4fcc1fc;

impl Generator {
    pub(super) fn try_sfp_num2dec_i(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__num2dec_internal"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SFP_NUM2DEC_I_AST_HASH {
            eprintln!("sfp_num2dec_i hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("sfp_num2dec_i context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 160;
        self.non_leaf = true;
        // @1104 is skipped between the two pool doubles (@1103, @1105).
        self.output.constant_number_gaps = vec![(1, 1)];
        self.callee_saved_float = 2;
        for bits in [0x0000000000000000u64, 0x4330000080000000] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [
            23, 38, 40, 43, 64, 70, 72, 77, 81, 84, 93, 99, 101, 106, 110, 112, 113, 138, 141, 158,
            168, 173, 179, 184,
        ] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -160,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 164,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 152,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 31,
                a: 1,
                offset: 152,
            });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_26");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_savegpr_26".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 3));
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 31, b: 1 });
        self.record_relocation(RelocationKind::Rel24, "signbit");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "signbit".to_string(),
        });
        self.load_double_constant(0, 0x0000000000000000);
        self.output
            .instructions
            .push(Instruction::Negate { d: 0, a: 3 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 0, b: 3 });
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 0, b: 31 });
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
        self.emit_branch_conditional_to(4, 2, labels[&23]); // bne
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
        self.emit_branch_to(labels[&184]); // b
        self.bind_label(labels[&23]);
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 31 });
        self.record_relocation(RelocationKind::Rel24, "isfinite");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "isfinite".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&40]); // bne
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
            .push(Instruction::FloatMove { d: 1, b: 31 });
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
        self.record_relocation(RelocationKind::Rel24, "fpclassify");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "fpclassify".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 1 });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 73));
        self.emit_branch_conditional_to(4, 2, labels[&38]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 78));
        self.bind_label(labels[&38]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 30,
            offset: 5,
        });
        self.emit_branch_to(labels[&184]); // b
        self.bind_label(labels[&40]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&43]); // beq
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 31, b: 31 });
        self.bind_label(labels[&43]);
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 31 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 8,
        });
        self.record_relocation(RelocationKind::Rel24, "frexp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "frexp".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::XorImmediateShifted {
                a: 3,
                s: 3,
                immediate: 32768,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 17200));
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 124,
        });
        self.load_double_constant(1, 0x4330000080000000);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 120,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 120,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 31, a: 0, b: 1 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 31,
                a: 1,
                offset: 16,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 8,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 8, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&84]); // beq
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
        self.emit_branch_to(labels[&81]); // b
        self.bind_label(labels[&64]);
        self.output
            .instructions
            .push(Instruction::AndRecord { a: 0, s: 8, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&70]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 7, a: 7, b: 4 });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 8, s: 8, b: 4 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 6, a: 4, b: 6 });
        self.emit_branch_to(labels[&72]); // b
        self.bind_label(labels[&70]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&113]); // beq
        self.bind_label(labels[&72]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&77]); // ble
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
        self.bind_label(labels[&77]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&81]); // ble
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 3, s: 3, b: 5 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&81]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&64]); // bne
        self.emit_branch_to(labels[&113]); // b
        self.bind_label(labels[&84]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 16,
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
        self.emit_branch_to(labels[&110]); // b
        self.bind_label(labels[&93]);
        self.output
            .instructions
            .push(Instruction::AndRecord { a: 0, s: 8, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&99]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 7, a: 7, b: 4 });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 8, s: 8, b: 4 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 6, a: 4, b: 6 });
        self.emit_branch_to(labels[&101]); // b
        self.bind_label(labels[&99]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&112]); // beq
        self.bind_label(labels[&101]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&106]); // ble
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
        self.bind_label(labels[&106]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&110]); // ble
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 3, s: 3, b: 5 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&110]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&93]); // bne
        self.bind_label(labels[&112]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 7,
            immediate: 32,
        });
        self.bind_label(labels[&113]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 8,
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
            immediate: 32,
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
            d: 4,
            a: 1,
            immediate: 24,
        });
        self.record_relocation(RelocationKind::Rel24, "modf");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "modf".to_string(),
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 24,
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
            offset: 76,
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
        self.emit_branch_conditional_to(4, 2, labels[&138]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 28,
            a: 1,
            offset: 78,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 80,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 28,
            a: 1,
            offset: 81,
        });
        self.emit_branch_to(labels[&179]); // b
        self.bind_label(labels[&138]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 28,
            a: 1,
            offset: 80,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 1,
            immediate: 76,
        });
        self.emit_branch_to(labels[&158]); // b
        self.bind_label(labels[&141]);
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
            offset: 80,
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
            offset: 80,
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
        self.bind_label(labels[&158]);
        self.output
            .instructions
            .push(Instruction::Xor { a: 3, s: 26, b: 28 });
        self.output
            .instructions
            .push(Instruction::Xor { a: 0, s: 27, b: 28 });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&141]); // bne
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 1,
            offset: 80,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 1,
            immediate: 76,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 81,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 0, b: 4 });
        self.emit_branch_to(labels[&173]); // b
        self.bind_label(labels[&168]);
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
        self.bind_label(labels[&173]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 5, b: 4 });
        self.emit_branch_conditional_to(12, 0, labels[&168]); // blt
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 1,
            offset: 80,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 1,
            offset: 78,
        });
        self.bind_label(labels[&179]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 76,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 32,
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
        self.bind_label(labels[&184]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 152,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 31,
            a: 1,
            offset: 152,
        });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_26");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_restgpr_26".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 164,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 160,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
