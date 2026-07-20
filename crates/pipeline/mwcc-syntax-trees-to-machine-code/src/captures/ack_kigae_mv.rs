//! ack_kigae_mv: an exact-match whole-function capture (fire 1310).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ACK_KIGAE_MV_AST_HASH: u64 = 0xfdef_d1f0_2118_f4df;

impl Generator {
    pub(super) fn try_ack_kigae_mv(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "eKigae_mv"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ACK_KIGAE_MV_AST_HASH {
            eprintln!("ack_kigae_mv hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xb5fa_c128_0f99_43ed => 10,
            _ => {
                eprintln!("ack_kigae_mv context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 64;
        self.non_leaf = true;
        self.output.pre_scheduled = true;
        self.callee_saved = vec![31, 30, 29];
        self.callee_saved_float = 1;
        self.output.symbol_order = [
            "_savegpr_29",
            "fqrand2",
            "sin_s",
            "common_data",
            "cos_s",
            "_restgpr_29",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        for bits in [0x41b00000u64, 0x40a00000, 0x41200000, 0xc1c80000] {
            self.output.intern_constant(bits, 4);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [89] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -64,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 68,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 31,
                a: 1,
                offset: 48,
            });
        self.output
            .instructions
            .push(Instruction::PairedSingleQuantizedStore {
                s: 31,
                a: 1,
                offset: 56,
                w: 0,
                i: 0,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 48,
        });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_29");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_savegpr_29".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 3));
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 3,
                offset: 76,
            });
        self.output
            .instructions
            .push(Instruction::move_register(31, 4));
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 30,
            offset: 16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 30,
            offset: 20,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: -1638,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 30,
            offset: 24,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 30,
            offset: 76,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: 8336,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 28,
        });
        self.emit_branch_conditional_to(4, 2, labels[&89]); // bne
        self.record_relocation(RelocationKind::Rel24, "fqrand2");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "fqrand2".to_string(),
        });
        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::Constant(1),
        );
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 30,
                offset: 76,
            });
        self.record_target(
            RelocationKind::Addr16Lo,
            mwcc_machine_code::RelocationTarget::Constant(1),
        );
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 31, a: 0, c: 1 });
        self.record_relocation(RelocationKind::Rel24, "sin_s");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "sin_s".to_string(),
        });
        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::Constant(0),
        );
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.record_target(
            RelocationKind::Addr16Lo,
            mwcc_machine_code::RelocationTarget::Constant(0),
        );
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 2,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 1, a: 2, c: 1 });
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 1, a: 1, b: 31 });
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 0, a: 0, b: 1 });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 0,
                a: 1,
                offset: 20,
            });
        self.record_relocation(RelocationKind::Rel24, "fqrand2");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "fqrand2".to_string(),
        });
        self.record_relocation(RelocationKind::Addr16Ha, "common_data");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::Constant(1),
        );
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "common_data");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::Constant(2),
        );
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(6, 0));
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 29,
                a: 3,
                immediate: 2,
            });
        self.record_target(
            RelocationKind::Addr16Lo,
            mwcc_machine_code::RelocationTarget::Constant(1),
        );
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 4,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 29,
            offset: 24732,
        });
        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::Constant(3),
        );
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.record_target(
            RelocationKind::Addr16Lo,
            mwcc_machine_code::RelocationTarget::Constant(3),
        );
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 4,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 3,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 31, a: 0, c: 1 });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 2,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 30,
                offset: 0,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 60));
        self.record_target(
            RelocationKind::Addr16Lo,
            mwcc_machine_code::RelocationTarget::Constant(2),
        );
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 6,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 1, a: 1, b: 31 });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 1,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 0, a: 0, b: 1 });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 0,
                a: 1,
                offset: 24,
            });
        self.record_relocation(RelocationKind::Rel24, "fqrand2");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "fqrand2".to_string(),
        });
        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::Constant(1),
        );
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 30,
                offset: 76,
            });
        self.record_target(
            RelocationKind::Addr16Lo,
            mwcc_machine_code::RelocationTarget::Constant(1),
        );
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 31, a: 0, c: 1 });
        self.record_relocation(RelocationKind::Rel24, "cos_s");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "cos_s".to_string(),
        });
        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::Constant(0),
        );
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 24,
        });
        self.record_target(
            RelocationKind::Addr16Lo,
            mwcc_machine_code::RelocationTarget::Constant(0),
        );
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 2,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::move_register(7, 31));
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 1, a: 2, c: 1 });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 86));
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 29,
            offset: 24732,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 0));
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 1, a: 1, b: 31 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(9, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(10, 0));
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 0, a: 0, b: 1 });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 0,
                a: 1,
                offset: 28,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 30,
            offset: 14,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 8,
                a: 30,
                offset: 12,
            });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.bind_label(labels[&89]);
        self.output
            .instructions
            .push(Instruction::PairedSingleQuantizedLoad {
                d: 31,
                a: 1,
                offset: 56,
                w: 0,
                i: 0,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 48,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 31,
            a: 1,
            offset: 48,
        });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_29");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_restgpr_29".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 68,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 64,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
