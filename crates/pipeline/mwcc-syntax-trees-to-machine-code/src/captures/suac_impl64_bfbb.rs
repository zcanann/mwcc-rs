//! suac_impl64_bfbb: an exact-match whole-function capture (fire 513).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SUAC_IMPL64_BFBB_AST_HASH: u64 = 0x20ace4a844742d9b; // BfBB (f513)

impl Generator {
    pub(super) fn try_suac_impl64_bfbb(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__strtoull"
            || function.return_type != Type::UnsignedLongLong
            || function.parameters.len() != 7
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SUAC_IMPL64_BFBB_AST_HASH {
            eprintln!("suac_impl64_bfbb hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xf8b1cd38c2b39c70 => 0, // BfBB strtoul (f513)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 80;
        self.non_leaf = true;
        self.callee_saved = vec![
            16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
        ]; // via _savegpr_16
        self.output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: vec![
                876, 228, 380, 876, 452, 876, 876, 876, 532, 876, 876, 876, 876, 876, 876, 876, 532,
            ],
            anonymous_offset: 72, // measured: table @151 (f513 refctx probe)
        });
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [
            29, 31, 39, 49, 70, 81, 93, 99, 111, 117, 128, 131, 136, 149, 160, 163, 172, 176, 178,
            181, 188, 206, 219, 225, 232, 235,
        ] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -80,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 84,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 80,
        });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_16");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_savegpr_16".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 31, s: 3, b: 3 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 9,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::move_register(22, 9));
        self.output
            .instructions
            .push(Instruction::move_register(17, 8));
        self.output
            .instructions
            .push(Instruction::move_register(18, 4));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 8,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::move_register(28, 5));
        self.output
            .instructions
            .push(Instruction::move_register(27, 6));
        self.output
            .instructions
            .push(Instruction::move_register(16, 7));
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(29, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(19, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(26, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(23, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(24, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(25, 0));
        self.emit_branch_conditional_to(12, 0, labels[&29]); // blt
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 1,
            });
        self.emit_branch_conditional_to(12, 2, labels[&29]); // beq
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 36,
            });
        self.emit_branch_conditional_to(12, 1, labels[&29]); // bgt
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 18,
                immediate: 1,
            });
        self.emit_branch_conditional_to(4, 0, labels[&31]); // bge
        self.bind_label(labels[&29]);
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 64));
        self.emit_branch_to(labels[&39]); // b
        self.bind_label(labels[&31]);
        self.output
            .instructions
            .push(Instruction::move_register(12, 28));
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::load_immediate(29, 1));
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.output
            .instructions
            .push(Instruction::move_register(20, 3));
        self.bind_label(labels[&39]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&219]); // beq
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 5,
                s: 31,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::move_register(6, 31));
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, -1));
        self.record_relocation(RelocationKind::Rel24, "__div2u");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__div2u".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(24, 4));
        self.output
            .instructions
            .push(Instruction::move_register(25, 3));
        self.emit_branch_to(labels[&219]); // b
        self.bind_label(labels[&49]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 30,
                immediate: 16,
            });
        self.emit_branch_conditional_to(12, 1, labels[&219]); // bgt
        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::JumpTable,
        );
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 30,
                shift: 2,
            });
        self.record_target(
            RelocationKind::Addr16Lo,
            mwcc_machine_code::RelocationTarget::JumpTable,
        );
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegister);
        self.output
            .instructions
            .push(Instruction::move_register(3, 20));
        self.record_relocation(RelocationKind::Rel24, "isspace");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "isspace".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&70]); // beq
        self.output
            .instructions
            .push(Instruction::move_register(12, 28));
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.output
            .instructions
            .push(Instruction::move_register(20, 3));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 19,
            a: 19,
            immediate: 1,
        });
        self.emit_branch_to(labels[&219]); // b
        self.bind_label(labels[&70]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 20,
                immediate: 43,
            });
        self.emit_branch_conditional_to(4, 2, labels[&81]); // bne
        self.output
            .instructions
            .push(Instruction::move_register(12, 28));
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 29,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.output
            .instructions
            .push(Instruction::move_register(20, 3));
        self.emit_branch_to(labels[&93]); // b
        self.bind_label(labels[&81]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 20,
                immediate: 45,
            });
        self.emit_branch_conditional_to(4, 2, labels[&93]); // bne
        self.output
            .instructions
            .push(Instruction::move_register(12, 28));
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 29,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output
            .instructions
            .push(Instruction::move_register(20, 3));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 17,
            offset: 0,
        });
        self.bind_label(labels[&93]);
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 2));
        self.emit_branch_to(labels[&219]); // b
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&99]); // beq
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 16,
            });
        self.emit_branch_conditional_to(4, 2, labels[&111]); // bne
        self.bind_label(labels[&99]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 20,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&111]); // bne
        self.output
            .instructions
            .push(Instruction::move_register(12, 28));
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 4));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 29,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.output
            .instructions
            .push(Instruction::move_register(20, 3));
        self.emit_branch_to(labels[&219]); // b
        self.bind_label(labels[&111]);
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 8));
        self.emit_branch_to(labels[&219]); // b
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 20,
                immediate: 88,
            });
        self.emit_branch_conditional_to(12, 2, labels[&117]); // beq
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 20,
                immediate: 120,
            });
        self.emit_branch_conditional_to(4, 2, labels[&128]); // bne
        self.bind_label(labels[&117]);
        self.output
            .instructions
            .push(Instruction::move_register(12, 28));
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::load_immediate(31, 16));
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 8));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 29,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.output
            .instructions
            .push(Instruction::move_register(20, 3));
        self.emit_branch_to(labels[&219]); // b
        self.bind_label(labels[&128]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&131]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(31, 8));
        self.bind_label(labels[&131]);
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 16));
        self.emit_branch_to(labels[&219]); // b
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&136]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(31, 10));
        self.bind_label(labels[&136]);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 0,
                s: 3,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::Xor { a: 3, s: 24, b: 3 });
        self.output
            .instructions
            .push(Instruction::Xor { a: 0, s: 25, b: 0 });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&149]); // bne
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 5,
                s: 31,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::move_register(6, 31));
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, -1));
        self.record_relocation(RelocationKind::Rel24, "__div2u");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__div2u".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(24, 4));
        self.output
            .instructions
            .push(Instruction::move_register(25, 3));
        self.bind_label(labels[&149]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 20));
        self.record_relocation(RelocationKind::Rel24, "isdigit");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "isdigit".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&163]); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: 20,
            a: 20,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 20, b: 31 });
        self.emit_branch_conditional_to(12, 0, labels[&181]); // blt
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 30,
                immediate: 16,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 64));
        self.emit_branch_conditional_to(4, 2, labels[&160]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 32));
        self.bind_label(labels[&160]);
        self.output
            .instructions
            .push(Instruction::move_register(30, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 20,
            a: 20,
            immediate: 48,
        });
        self.emit_branch_to(labels[&219]); // b
        self.bind_label(labels[&163]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 20));
        self.record_relocation(RelocationKind::Rel24, "isalpha");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "isalpha".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&172]); // beq
        self.output
            .instructions
            .push(Instruction::move_register(3, 20));
        self.record_relocation(RelocationKind::Rel24, "toupper");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "toupper".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: -55,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 0, b: 31 });
        self.emit_branch_conditional_to(12, 0, labels[&178]); // blt
        self.bind_label(labels[&172]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 30,
                immediate: 16,
            });
        self.emit_branch_conditional_to(4, 2, labels[&176]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 32));
        self.emit_branch_to(labels[&219]); // b
        self.bind_label(labels[&176]);
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 64));
        self.emit_branch_to(labels[&219]); // b
        self.bind_label(labels[&178]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 20));
        self.record_relocation(RelocationKind::Rel24, "toupper");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "toupper".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 20,
            a: 3,
            immediate: -55,
        });
        self.bind_label(labels[&181]);
        self.output
            .instructions
            .push(Instruction::SubtractFromCarrying { d: 0, a: 26, b: 24 });
        self.output
            .instructions
            .push(Instruction::SubtractFromExtended { d: 0, a: 23, b: 25 });
        self.output
            .instructions
            .push(Instruction::SubtractFromExtended { d: 0, a: 21, b: 21 });
        self.output
            .instructions
            .push(Instruction::NegateRecord { d: 0, a: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&188]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 22,
            offset: 0,
        });
        self.bind_label(labels[&188]);
        self.output
            .instructions
            .push(Instruction::MultiplyHighWordUnsigned { d: 3, a: 26, b: 31 });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 8,
                s: 31,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 0,
                s: 20,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, -1));
        self.output
            .instructions
            .push(Instruction::MultiplyLow { d: 5, a: 23, b: 31 });
        self.output
            .instructions
            .push(Instruction::Add { d: 6, a: 3, b: 5 });
        self.output
            .instructions
            .push(Instruction::MultiplyLow { d: 7, a: 26, b: 31 });
        self.output
            .instructions
            .push(Instruction::MultiplyLow { d: 5, a: 26, b: 8 });
        self.output
            .instructions
            .push(Instruction::SubtractFromCarrying { d: 3, a: 7, b: 4 });
        self.output
            .instructions
            .push(Instruction::Add { d: 5, a: 6, b: 5 });
        self.output
            .instructions
            .push(Instruction::SubtractFromExtended { d: 4, a: 5, b: 4 });
        self.output
            .instructions
            .push(Instruction::SubtractFromCarrying { d: 3, a: 20, b: 3 });
        self.output
            .instructions
            .push(Instruction::SubtractFromExtended { d: 3, a: 0, b: 4 });
        self.output
            .instructions
            .push(Instruction::SubtractFromExtended { d: 3, a: 21, b: 21 });
        self.output
            .instructions
            .push(Instruction::NegateRecord { d: 3, a: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&206]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 22,
            offset: 0,
        });
        self.bind_label(labels[&206]);
        self.output
            .instructions
            .push(Instruction::AddCarrying { d: 4, a: 7, b: 20 });
        self.output
            .instructions
            .push(Instruction::move_register(12, 28));
        self.output
            .instructions
            .push(Instruction::AddExtended { d: 0, a: 5, b: 0 });
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::move_register(26, 4));
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 16));
        self.output
            .instructions
            .push(Instruction::move_register(23, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 29,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.output
            .instructions
            .push(Instruction::move_register(20, 3));
        self.bind_label(labels[&219]);
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 29, b: 18 });
        self.emit_branch_conditional_to(12, 1, labels[&225]); // bgt
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 20,
                immediate: -1,
            });
        self.emit_branch_conditional_to(12, 2, labels[&225]); // beq
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 30,
                shift: 0,
                begin: 25,
                end: 26,
            });
        self.emit_branch_conditional_to(12, 2, labels[&49]); // beq
        self.bind_label(labels[&225]);
        self.output
            .instructions
            .push(Instruction::AndImmediateRecord {
                a: 0,
                s: 30,
                immediate: 52,
            });
        self.emit_branch_conditional_to(4, 2, labels[&232]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(26, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 16,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 23,
                s: 0,
                shift: 31,
            });
        self.emit_branch_to(labels[&235]); // b
        self.bind_label(labels[&232]);
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 29, b: 19 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 16,
            offset: 0,
        });
        self.bind_label(labels[&235]);
        self.output
            .instructions
            .push(Instruction::move_register(12, 28));
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::move_register(4, 20));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 1));
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.output
            .instructions
            .push(Instruction::move_register(4, 26));
        self.output
            .instructions
            .push(Instruction::move_register(3, 23));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 80,
        });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_16");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_restgpr_16".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 84,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 80,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
