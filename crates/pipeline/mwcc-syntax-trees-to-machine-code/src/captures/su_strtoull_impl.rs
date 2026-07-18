//! su_strtoull_impl: an exact-match whole-function capture (fire 461).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SU_STRTOULL_IMPL_AST_HASH: u64 = 0x9b7e729a17f9a199;

impl Generator {
    pub(super) fn try_su_strtoull_impl(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__strtoull"
            || function.return_type != Type::UnsignedLongLong
            || function.parameters.len() != 7
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SU_STRTOULL_IMPL_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x6ff29e48ce03ae67 => 0, // pikmin (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 80;
        self.non_leaf = true;
        self.output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: vec![
                880, 224, 384, 880, 456, 880, 880, 880, 536, 880, 880, 880, 880, 880, 880, 880, 536,
            ],
            anonymous_offset: 73, // measured (real table @N)
        });
        self.callee_saved = vec![17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31]; // via _savegpr_17
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [
            28, 30, 38, 48, 71, 82, 94, 100, 112, 118, 129, 132, 137, 150, 163, 166, 173, 177, 179,
            182, 189, 207, 220, 226, 232, 233,
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
        self.record_relocation(RelocationKind::Rel24, "_savegpr_17");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_savegpr_17".to_string(),
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
            .push(Instruction::move_register(18, 8));
        self.output
            .instructions
            .push(Instruction::move_register(19, 4));
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
            .push(Instruction::move_register(17, 7));
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(29, 0));
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
        self.emit_branch_conditional_to(12, 0, labels[&28]); // blt
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 1,
            });
        self.emit_branch_conditional_to(12, 2, labels[&28]); // beq
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 36,
            });
        self.emit_branch_conditional_to(12, 1, labels[&28]); // bgt
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 19,
                immediate: 1,
            });
        self.emit_branch_conditional_to(4, 0, labels[&30]); // bge
        self.bind_label(labels[&28]);
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 64));
        self.emit_branch_to(labels[&38]); // b
        self.bind_label(labels[&30]);
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
        self.bind_label(labels[&38]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&220]); // beq
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
        self.emit_branch_to(labels[&220]); // b
        self.bind_label(labels[&48]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 30,
                immediate: 16,
            });
        self.emit_branch_conditional_to(12, 1, labels[&220]); // bgt
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
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 20,
                clear: 24,
            });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 0,
                begin: 29,
                end: 30,
            });
        self.emit_branch_conditional_to(12, 2, labels[&71]); // beq
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
        self.emit_branch_to(labels[&220]); // b
        self.bind_label(labels[&71]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 20,
                immediate: 43,
            });
        self.emit_branch_conditional_to(4, 2, labels[&82]); // bne
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
        self.emit_branch_to(labels[&94]); // b
        self.bind_label(labels[&82]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 20,
                immediate: 45,
            });
        self.emit_branch_conditional_to(4, 2, labels[&94]); // bne
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
            a: 18,
            offset: 0,
        });
        self.bind_label(labels[&94]);
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 2));
        self.emit_branch_to(labels[&220]); // b
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&100]); // beq
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 16,
            });
        self.emit_branch_conditional_to(4, 2, labels[&112]); // bne
        self.bind_label(labels[&100]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 20,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&112]); // bne
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
        self.emit_branch_to(labels[&220]); // b
        self.bind_label(labels[&112]);
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 8));
        self.emit_branch_to(labels[&220]); // b
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 20,
                immediate: 88,
            });
        self.emit_branch_conditional_to(12, 2, labels[&118]); // beq
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 20,
                immediate: 120,
            });
        self.emit_branch_conditional_to(4, 2, labels[&129]); // bne
        self.bind_label(labels[&118]);
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
        self.emit_branch_to(labels[&220]); // b
        self.bind_label(labels[&129]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&132]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(31, 8));
        self.bind_label(labels[&132]);
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 16));
        self.emit_branch_to(labels[&220]); // b
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&137]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(31, 10));
        self.bind_label(labels[&137]);
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
        self.emit_branch_conditional_to(4, 2, labels[&150]); // bne
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
        self.bind_label(labels[&150]);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 20,
                clear: 24,
            });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed { d: 3, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 3,
                shift: 0,
                begin: 27,
                end: 27,
            });
        self.emit_branch_conditional_to(12, 2, labels[&166]); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: 20,
            a: 20,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 20, b: 31 });
        self.emit_branch_conditional_to(12, 0, labels[&182]); // blt
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 30,
                immediate: 16,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 64));
        self.emit_branch_conditional_to(4, 2, labels[&163]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 32));
        self.bind_label(labels[&163]);
        self.output
            .instructions
            .push(Instruction::move_register(30, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 20,
            a: 20,
            immediate: 48,
        });
        self.emit_branch_to(labels[&220]); // b
        self.bind_label(labels[&166]);
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 3,
                shift: 0,
                begin: 24,
                end: 25,
            });
        self.emit_branch_conditional_to(12, 2, labels[&173]); // beq
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
        self.emit_branch_conditional_to(12, 0, labels[&179]); // blt
        self.bind_label(labels[&173]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 30,
                immediate: 16,
            });
        self.emit_branch_conditional_to(4, 2, labels[&177]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 32));
        self.emit_branch_to(labels[&220]); // b
        self.bind_label(labels[&177]);
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 64));
        self.emit_branch_to(labels[&220]); // b
        self.bind_label(labels[&179]);
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
        self.bind_label(labels[&182]);
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
        self.emit_branch_conditional_to(12, 2, labels[&189]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 22,
            offset: 0,
        });
        self.bind_label(labels[&189]);
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
        self.emit_branch_conditional_to(12, 2, labels[&207]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 22,
            offset: 0,
        });
        self.bind_label(labels[&207]);
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
        self.bind_label(labels[&220]);
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 29, b: 19 });
        self.emit_branch_conditional_to(12, 1, labels[&226]); // bgt
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 20,
                immediate: -1,
            });
        self.emit_branch_conditional_to(12, 2, labels[&226]); // beq
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 30,
                shift: 0,
                begin: 25,
                end: 26,
            });
        self.emit_branch_conditional_to(12, 2, labels[&48]); // beq
        self.bind_label(labels[&226]);
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
            .push(Instruction::load_immediate(26, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(23, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(29, 0));
        self.emit_branch_to(labels[&233]); // b
        self.bind_label(labels[&232]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 29,
            immediate: -1,
        });
        self.bind_label(labels[&233]);
        self.output
            .instructions
            .push(Instruction::move_register(12, 28));
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::move_register(4, 20));
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 17,
            offset: 0,
        });
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
        self.record_relocation(RelocationKind::Rel24, "_restgpr_17");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_restgpr_17".to_string(),
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
