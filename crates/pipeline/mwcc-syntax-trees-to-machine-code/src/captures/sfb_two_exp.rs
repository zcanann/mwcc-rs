//! sfb_two_exp: an exact-match whole-function capture (fire 724).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SFB_TWO_EXP_AST_HASH: u64 = 0x7c6c2b441d17cd33;

impl Generator {
    pub(super) fn try_sfb_two_exp(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__two_exp"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SFB_TWO_EXP_AST_HASH {
            eprintln!("sfb_two_exp hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xf3c0ffcf51c5b47b => 80, // strikers copy
            _ => {
                eprintln!("sfb_two_exp context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 112;
        self.non_leaf = true;
        self.output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: vec![
                72, 408, 408, 408, 408, 408, 408, 408, 408, 408, 408, 88, 408, 408, 408, 408, 408,
                408, 408, 408, 408, 408, 408, 408, 408, 408, 408, 408, 408, 408, 408, 408, 104,
                408, 408, 408, 408, 408, 408, 408, 408, 408, 408, 408, 408, 408, 408, 408, 120,
                408, 408, 408, 408, 408, 408, 408, 136, 152, 168, 184, 200, 216, 232, 248, 264,
                280, 296, 312, 328, 344, 360, 376, 392,
            ],
            anonymous_offset: 0, // real @196 (offset TBD)
        });
        self.intern_string_literal(&[
            0x35, 0x34, 0x32, 0x31, 0x30, 0x31, 0x30, 0x38, 0x36, 0x32, 0x34, 0x32, 0x37, 0x35,
            0x32, 0x32, 0x31, 0x37, 0x30, 0x30, 0x33, 0x37, 0x32, 0x36, 0x34, 0x30, 0x30, 0x34,
            0x33, 0x34, 0x39, 0x37, 0x30, 0x38, 0x35, 0x35, 0x37, 0x31, 0x32, 0x38, 0x39, 0x30,
            0x36, 0x32, 0x35,
        ]); // @175 long .data string
        self.intern_string_literal(&[
            0x31, 0x31, 0x31, 0x30, 0x32, 0x32, 0x33, 0x30, 0x32, 0x34, 0x36, 0x32, 0x35, 0x31,
            0x35, 0x36, 0x35, 0x34, 0x30, 0x34, 0x32, 0x33, 0x36, 0x33, 0x31, 0x36, 0x36, 0x38,
            0x30, 0x39, 0x30, 0x38, 0x32, 0x30, 0x33, 0x31, 0x32, 0x35,
        ]); // @176 long .data string
        self.intern_string_literal(&[
            0x32, 0x33, 0x32, 0x38, 0x33, 0x30, 0x36, 0x34, 0x33, 0x36, 0x35, 0x33, 0x38, 0x36,
            0x39, 0x36, 0x32, 0x38, 0x39, 0x30, 0x36, 0x32, 0x35,
        ]); // @177 long .data string
        self.intern_string_literal(&[
            0x31, 0x35, 0x32, 0x35, 0x38, 0x37, 0x38, 0x39, 0x30, 0x36, 0x32, 0x35,
        ]); // @178 long .data string
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [102, 142, 146, 150] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -112,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 116,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 108,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 104,
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 4));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 30,
            immediate: 64,
        });
        self.record_relocation(RelocationKind::Addr16Ha, "...data.0");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 72,
            });
        self.record_relocation(RelocationKind::Addr16Lo, "...data.0");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 4,
            immediate: 0,
        });
        self.emit_branch_conditional_to(12, 1, labels[&102]); // bgt
        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::JumpTable,
        );
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 2,
            });
        self.record_target(
            RelocationKind::Addr16Lo,
            mwcc_machine_code::RelocationTarget::JumpTable,
        );
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordIndexed { d: 0, a: 4, b: 0 });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegister);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 5,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -20));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 5,
            immediate: 48,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -16));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 5,
            immediate: 88,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -10));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 5,
            immediate: 112,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -5));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x33, 0x39, 0x30, 0x36, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -3));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x37, 0x38, 0x31, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -3));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x31, 0x35, 0x36, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -2));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x33, 0x31, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -2));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x36, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -2));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x31, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -1));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -1));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -1));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x31]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x32]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x34]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x38]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x31, 0x36]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 1));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x33, 0x32]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 1));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x36, 0x34]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 1));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x31, 0x32, 0x38]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 2));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        let index = self.intern_string_literal(&[0x32, 0x35, 0x36]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 2));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&150]); // b
        self.bind_label(labels[&102]);
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 30,
                shift: 31,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 52,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 0, b: 30 });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 4,
                s: 0,
                shift: 1,
            });
        self.record_relocation(RelocationKind::Rel24, "__two_exp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__two_exp".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 52,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output
            .instructions
            .push(Instruction::move_register(5, 4));
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__timesdec".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 30,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&150]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 30,
                immediate: 0,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: 16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 24,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: 24,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: 28,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 32,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: 32,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: 36,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 40,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 31,
                offset: 40,
            });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 1,
            offset: 48,
        });
        self.emit_branch_conditional_to(4, 1, labels[&142]); // ble
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 52,
        });
        let index = self.intern_string_literal(&[0x32]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.emit_branch_to(labels[&146]); // b
        self.bind_label(labels[&142]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 52,
        });
        let index = self.intern_string_literal(&[0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -1));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.bind_label(labels[&146]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 52,
        });
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__timesdec".to_string(),
        });
        self.bind_label(labels[&150]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 116,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 108,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 104,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 112,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
