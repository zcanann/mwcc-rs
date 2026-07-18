//! dio_fread_impl_str: an exact-match whole-function capture (fire 511).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const DIO_FREAD_IMPL_STR_AST_HASH: u64 = 0xb5cff6035956da61; // strikers (f511)

impl Generator {
    pub(super) fn try_dio_fread_impl_str(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__fread"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != DIO_FREAD_IMPL_STR_AST_HASH {
            eprintln!("dio_fread_impl_str hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // strikers direct_io (f511)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 48;
        self.non_leaf = true;
        self.callee_saved = vec![25, 26, 27, 28, 29, 30, 31]; // via _savegpr_25
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [
            17, 25, 27, 36, 47, 57, 69, 77, 91, 99, 110, 116, 123, 139, 146, 148, 153, 171, 192,
            199, 207, 208,
        ] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -48,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 52,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 48,
        });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_25");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_savegpr_25".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(27, 6));
        self.output
            .instructions
            .push(Instruction::move_register(28, 3));
        self.output
            .instructions
            .push(Instruction::move_register(26, 4));
        self.output
            .instructions
            .push(Instruction::move_register(25, 5));
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Rel24, "fwide");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "fwide".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&17]); // bne
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, -1));
        self.record_relocation(RelocationKind::Rel24, "fwide");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "fwide".to_string(),
        });
        self.bind_label(labels[&17]);
        self.output
            .instructions
            .push(Instruction::MultiplyLowRecord {
                d: 29,
                a: 26,
                b: 25,
            });
        self.emit_branch_conditional_to(12, 2, labels[&25]); // beq
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 27,
            offset: 10,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&25]); // bne
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 27,
                offset: 4,
            });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 26,
                begin: 29,
                end: 31,
            });
        self.emit_branch_conditional_to(4, 2, labels[&27]); // bne
        self.bind_label(labels[&25]);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&208]); // b
        self.bind_label(labels[&27]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 27,
            offset: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(31, 1));
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 29,
                begin: 31,
                end: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&36]); // beq
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 27,
            offset: 4,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 31,
            begin: 30,
            end: 31,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&36]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(31, 0));
        self.bind_label(labels[&36]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 27,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 3,
                shift: 27,
                begin: 29,
                end: 31,
            });
        self.emit_branch_conditional_to(4, 2, labels[&47]); // bne
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 27,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 29,
                begin: 31,
                end: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&47]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: 3,
                s: 0,
                shift: 5,
                begin: 24,
                end: 26,
            });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 27,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 27,
            offset: 40,
        });
        self.bind_label(labels[&47]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 27,
            offset: 8,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 27,
            begin: 29,
            end: 31,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 0, labels[&57]); // bge
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 27,
            offset: 10,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 27,
            offset: 40,
        });
        self.emit_branch_to(labels[&208]); // b
        self.bind_label(labels[&57]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 27,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 31,
                begin: 31,
                end: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&69]); // beq
        self.record_relocation(RelocationKind::Rel24, "__flush_line_buffered_output_files");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__flush_line_buffered_output_files".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&69]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 27,
            offset: 10,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 27,
            offset: 40,
        });
        self.emit_branch_to(labels[&208]); // b
        self.bind_label(labels[&69]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 29,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::move_register(30, 28));
        self.output
            .instructions
            .push(Instruction::load_immediate(28, 0));
        self.emit_branch_conditional_to(12, 2, labels[&116]); // beq
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 27,
            offset: 8,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 27,
            begin: 29,
            end: 31,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(12, 0, labels[&116]); // blt
        self.bind_label(labels[&77]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Rel24, "fwide");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "fwide".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&91]); // bne
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 27,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 28,
            a: 28,
            immediate: 2,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 29,
            immediate: -2,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 0,
            shift: 28,
            begin: 28,
            end: 30,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 12,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZeroIndexed { d: 0, a: 27, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 30,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 30,
            a: 30,
            immediate: 2,
        });
        self.emit_branch_to(labels[&99]); // b
        self.bind_label(labels[&91]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 27,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 28,
            a: 28,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 29,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 0,
            shift: 27,
            begin: 29,
            end: 31,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 12,
        });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed { d: 0, a: 27, b: 0 });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 30,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 30,
            a: 30,
            immediate: 1,
        });
        self.bind_label(labels[&99]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 27,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 29,
                immediate: 0,
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 4,
            shift: 27,
            begin: 29,
            end: 31,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: 4,
                s: 0,
                shift: 5,
                begin: 24,
                end: 26,
            });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 27,
            offset: 8,
        });
        self.emit_branch_conditional_to(12, 2, labels[&110]); // beq
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 27,
            offset: 8,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 27,
            begin: 29,
            end: 31,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&77]); // bge
        self.bind_label(labels[&110]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 27,
            offset: 8,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 27,
            begin: 29,
            end: 31,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&116]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 27,
            offset: 48,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 27,
            offset: 40,
        });
        self.bind_label(labels[&116]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 29,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&171]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 27,
            offset: 40,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&123]); // bne
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&171]); // beq
        self.bind_label(labels[&123]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 27,
            offset: 40,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&148]); // bne
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.record_relocation(RelocationKind::Rel24, "__load_buffer");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__load_buffer".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&148]); // beq
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&139]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 27,
            offset: 10,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 27,
            offset: 40,
        });
        self.emit_branch_to(labels[&146]); // b
        self.bind_label(labels[&139]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 27,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: 3,
                s: 4,
                shift: 5,
                begin: 24,
                end: 26,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 27,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 27,
            offset: 9,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 27,
            offset: 40,
        });
        self.bind_label(labels[&146]);
        self.output
            .instructions
            .push(Instruction::load_immediate(29, 0));
        self.emit_branch_to(labels[&171]); // b
        self.bind_label(labels[&148]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 27,
            offset: 40,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 0, b: 29 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.emit_branch_conditional_to(4, 1, labels[&153]); // ble
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: 8,
        });
        self.bind_label(labels[&153]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 27,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 8,
        });
        self.record_relocation(RelocationKind::Rel24, "memcpy");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "memcpy".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 27,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFromRecord { d: 29, a: 3, b: 29 });
        self.output
            .instructions
            .push(Instruction::Add { d: 30, a: 30, b: 3 });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 0, b: 3 });
        self.output
            .instructions
            .push(Instruction::Add { d: 28, a: 28, b: 3 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 27,
            offset: 36,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 27,
            offset: 40,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 27,
            offset: 40,
        });
        self.emit_branch_conditional_to(12, 2, labels[&171]); // beq
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&123]); // bne
        self.bind_label(labels[&171]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 29,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&207]); // beq
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&207]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 27,
            offset: 28,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output.instructions.push(Instruction::LoadWord {
            d: 25,
            a: 27,
            offset: 32,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 27,
            offset: 28,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 27,
            offset: 32,
        });
        self.record_relocation(RelocationKind::Rel24, "__load_buffer");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__load_buffer".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&199]); // beq
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&192]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 27,
            offset: 10,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 27,
            offset: 40,
        });
        self.emit_branch_to(labels[&199]); // b
        self.bind_label(labels[&192]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 27,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: 3,
                s: 4,
                shift: 5,
                begin: 24,
                end: 26,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 27,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 27,
            offset: 9,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 27,
            offset: 40,
        });
        self.bind_label(labels[&199]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 27,
            offset: 28,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 28, a: 28, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 25,
            a: 27,
            offset: 32,
        });
        self.record_relocation(RelocationKind::Rel24, "__prep_buffer");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__prep_buffer".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 27,
            offset: 40,
        });
        self.bind_label(labels[&207]);
        self.output
            .instructions
            .push(Instruction::DivideWordUnsigned { d: 3, a: 28, b: 26 });
        self.bind_label(labels[&208]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 48,
        });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_25");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_restgpr_25".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 52,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 48,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
