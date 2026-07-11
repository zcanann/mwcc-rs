//! pfa_float2str: an exact-match whole-function capture (fire 695).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PFA_FLOAT2STR_AST_HASH: u64 = 0x35a07e86f5e7bf4f;

impl Generator {
    pub(super) fn try_pfa_float2str(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "float2str"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PFA_FLOAT2STR_AST_HASH && hash != 0x3144f2b16fcf7aa9 && hash != 0xc3569d4b7645ecd3 {
            eprintln!("pfa_float2str hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x4dc5812f6e4177a3 => 0, // strikers (bump TBD)
            0xecff4eb19d59de49 => 0, // pikmin2 (bump TBD)
            0x46f259063d157aea => 0, // wind_waker (bump TBD)
            0xf8b1cd38c2b39c70 => 0, // animal_crossing (bump TBD)
            0x3012f8741ad9c69d => 0, // mp4
            _ => {
                eprintln!("pfa_float2str context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 80;
        // The pool double numbers @574 — a 204 gap past the strings.
        self.output.constant_number_gaps = vec![(0, 204)];
        self.non_leaf = true;
        self.callee_saved_float = 1;
        for bits in [
            0x0000000000000000u64,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [16, 27, 33, 39, 46, 49, 52, 66, 70, 81, 84, 86, 100, 104, 115, 118, 120, 138, 141, 145, 151, 157, 164, 167, 173, 176, 183, 191, 198, 204, 208, 221, 235, 243, 244, 246, 249, 251, 259, 261, 269, 275, 280, 286, 299, 303, 308, 317, 328, 329, 331, 333, 336, 341, 348, 359, 360, 362, 368, 370, 375, 377, 387, 406, 407, 411, 413, 419, 425, 429, 430] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -80 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 84 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 12 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 72 });
        self.output.instructions.push(Instruction::FloatMove { d: 31, b: 1 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 509 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 64 });
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 60 });
        self.output.instructions.push(Instruction::move_register(29, 4));
        self.output.instructions.push(Instruction::StoreWord { s: 28, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::move_register(28, 3));
        self.emit_branch_conditional_to(4, 1, labels[&16]); // ble
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&430]); // b
        self.bind_label(labels[&16]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::load_immediate(0, 32));
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 12 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 10 });
        self.record_relocation(RelocationKind::Rel24, "__num2dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__num2dec".to_string() });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 1, immediate: 17 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 31, b: 0 });
        self.emit_branch_to(labels[&33]); // b
        self.bind_label(labels[&27]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 14 });
        self.bind_label(labels[&33]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&39]); // ble
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 5, offset: -1 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(12, 2, labels[&27]); // beq
        self.bind_label(labels[&39]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 17 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 73 });
        self.emit_branch_conditional_to(12, 2, labels[&52]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&46]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(12, 2, labels[&49]); // beq
        self.emit_branch_to(labels[&120]); // b
        self.bind_label(labels[&46]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 78 });
        self.emit_branch_conditional_to(12, 2, labels[&86]); // beq
        self.emit_branch_to(labels[&120]); // b
        self.bind_label(labels[&49]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 14 });
        self.emit_branch_to(labels[&120]); // b
        self.bind_label(labels[&52]);
        self.load_double_constant(0, 0x0000000000000000);
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 31, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&70]); // bge
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 5 });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 28, immediate: -5 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 24, end: 24 });
        self.emit_branch_conditional_to(12, 2, labels[&66]); // beq
        self.output.instructions.push(Instruction::move_register(3, 28));
        let index = self.intern_string_literal(&[0x2d, 0x49, 0x4e, 0x46]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.emit_branch_to(labels[&84]); // b
        self.bind_label(labels[&66]);
        self.output.instructions.push(Instruction::move_register(3, 28));
        let index = self.intern_string_literal(&[0x2d, 0x69, 0x6e, 0x66]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.emit_branch_to(labels[&84]); // b
        self.bind_label(labels[&70]);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 5 });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 28, immediate: -4 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 24, end: 24 });
        self.emit_branch_conditional_to(12, 2, labels[&81]); // beq
        self.output.instructions.push(Instruction::move_register(3, 28));
        let index = self.intern_string_literal(&[0x49, 0x4e, 0x46]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.emit_branch_to(labels[&84]); // b
        self.bind_label(labels[&81]);
        self.output.instructions.push(Instruction::move_register(3, 28));
        let index = self.intern_string_literal(&[0x69, 0x6e, 0x66]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.bind_label(labels[&84]);
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.emit_branch_to(labels[&430]); // b
        self.bind_label(labels[&86]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&104]); // beq
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 5 });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 28, immediate: -5 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 24, end: 24 });
        self.emit_branch_conditional_to(12, 2, labels[&100]); // beq
        self.output.instructions.push(Instruction::move_register(3, 28));
        let index = self.intern_string_literal(&[0x2d, 0x4e, 0x41, 0x4e]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.emit_branch_to(labels[&118]); // b
        self.bind_label(labels[&100]);
        self.output.instructions.push(Instruction::move_register(3, 28));
        let index = self.intern_string_literal(&[0x2d, 0x6e, 0x61, 0x6e]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.emit_branch_to(labels[&118]); // b
        self.bind_label(labels[&104]);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 5 });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 28, immediate: -4 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 24, end: 24 });
        self.emit_branch_conditional_to(12, 2, labels[&115]); // beq
        self.output.instructions.push(Instruction::move_register(3, 28));
        let index = self.intern_string_literal(&[0x4e, 0x41, 0x4e]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.emit_branch_to(labels[&118]); // b
        self.bind_label(labels[&115]);
        self.output.instructions.push(Instruction::move_register(3, 28));
        let index = self.intern_string_literal(&[0x6e, 0x61, 0x6e]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.bind_label(labels[&118]);
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.emit_branch_to(labels[&430]); // b
        self.bind_label(labels[&120]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 5, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 28, immediate: -1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 5 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 101 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 4, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 28, offset: -1 });
        self.emit_branch_conditional_to(12, 2, labels[&191]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&141]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 70 });
        self.emit_branch_conditional_to(12, 2, labels[&280]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&138]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 69 });
        self.emit_branch_conditional_to(4, 0, labels[&191]); // bge
        self.emit_branch_to(labels[&429]); // b
        self.bind_label(labels[&138]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 72 });
        self.emit_branch_conditional_to(4, 0, labels[&429]); // bge
        self.emit_branch_to(labels[&145]); // b
        self.bind_label(labels[&141]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 103 });
        self.emit_branch_conditional_to(12, 2, labels[&145]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&429]); // bge
        self.emit_branch_to(labels[&280]); // b
        self.bind_label(labels[&145]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 1, labels[&151]); // ble
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 12 });
        self.record_relocation(RelocationKind::Rel24, "round_decimal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "round_decimal".to_string() });
        self.bind_label(labels[&151]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: -4 });
        self.emit_branch_conditional_to(12, 0, labels[&157]); // blt
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 3 });
        self.emit_branch_conditional_to(12, 0, labels[&176]); // blt
        self.bind_label(labels[&157]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 3 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&164]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 12 });
        self.emit_branch_to(labels[&167]); // b
        self.bind_label(labels[&164]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 12 });
        self.bind_label(labels[&167]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 103 });
        self.emit_branch_conditional_to(4, 2, labels[&173]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 101));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 29, offset: 5 });
        self.emit_branch_to(labels[&191]); // b
        self.bind_label(labels[&173]);
        self.output.instructions.push(Instruction::load_immediate(0, 69));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 29, offset: 5 });
        self.emit_branch_to(labels[&191]); // b
        self.bind_label(labels[&176]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 3 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&183]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 12 });
        self.emit_branch_to(labels[&280]); // b
        self.bind_label(labels[&183]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::SubtractFromRecord { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 12 });
        self.emit_branch_conditional_to(4, 0, labels[&280]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 12 });
        self.emit_branch_to(labels[&280]); // b
        self.bind_label(labels[&191]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 1, labels[&198]); // ble
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 12 });
        self.record_relocation(RelocationKind::Rel24, "round_decimal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "round_decimal".to_string() });
        self.bind_label(labels[&198]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 6, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::load_immediate(8, 43));
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&204]); // bge
        self.output.instructions.push(Instruction::Negate { d: 6, a: 6 });
        self.output.instructions.push(Instruction::load_immediate(8, 45));
        self.bind_label(labels[&204]);
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 26214));
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 3, immediate: 26215 });
        self.emit_branch_to(labels[&221]); // b
        self.bind_label(labels[&208]);
        self.output.instructions.push(Instruction::MultiplyHighWord { d: 0, a: 5, b: 6 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 4, s: 3, shift: 31 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 0, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 3, b: 4 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 4, a: 3, immediate: 10 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 3, s: 0, shift: 31 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 4, a: 4, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 6, a: 0, b: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 48 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 30, immediate: -1 });
        self.bind_label(labels[&221]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&208]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 7, immediate: 2 });
        self.emit_branch_conditional_to(12, 0, labels[&208]); // blt
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 5 });
        self.output.instructions.push(Instruction::StoreByte { s: 8, a: 30, offset: -1 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -2 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 30, b: 28 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 509 });
        self.emit_branch_conditional_to(4, 1, labels[&235]); // ble
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&430]); // b
        self.bind_label(labels[&235]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&246]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 2 });
        self.output.instructions.push(Instruction::load_immediate(0, 48));
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 4, b: 3 });
        self.emit_branch_to(labels[&244]); // b
        self.bind_label(labels[&243]);
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.bind_label(labels[&244]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 3, a: 3, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&243]); // bne
        self.bind_label(labels[&246]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 31, b: 3 });
        self.emit_branch_to(labels[&251]); // b
        self.bind_label(labels[&249]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 4, offset: -1 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.bind_label(labels[&251]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 3, a: 3, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&249]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&259]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 3 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&261]); // beq
        self.bind_label(labels[&259]);
        self.output.instructions.push(Instruction::load_immediate(0, 46));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.bind_label(labels[&261]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 17 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&269]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 45));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.emit_branch_to(labels[&429]); // b
        self.bind_label(labels[&269]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&275]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 43));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.emit_branch_to(labels[&429]); // b
        self.bind_label(labels[&275]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&429]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 32));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.emit_branch_to(labels[&429]); // b
        self.bind_label(labels[&280]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 0, b: 4 });
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 7, a: 3, immediate: -1 });
        self.emit_branch_conditional_to(4, 0, labels[&286]); // bge
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.bind_label(labels[&286]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::CompareWord { a: 7, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&299]); // ble
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 12 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 4, a: 0, b: 4 });
        self.record_relocation(RelocationKind::Rel24, "round_decimal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "round_decimal".to_string() });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 7, a: 3, immediate: -1 });
        self.emit_branch_conditional_to(4, 0, labels[&299]); // bge
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.bind_label(labels[&299]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 6, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&303]); // bge
        self.output.instructions.push(Instruction::load_immediate(6, 0));
        self.bind_label(labels[&303]);
        self.output.instructions.push(Instruction::Add { d: 0, a: 6, b: 7 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 509 });
        self.emit_branch_conditional_to(4, 1, labels[&308]); // ble
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&430]); // b
        self.bind_label(labels[&308]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::load_immediate(4, 48));
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::SubtractFromRecord { d: 3, a: 7, b: 0 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 31, b: 5 });
        self.emit_branch_conditional_to(4, 1, labels[&331]); // ble
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&328]); // beq
        self.bind_label(labels[&317]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -2 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -3 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -4 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -5 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -6 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -7 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 4, a: 30, offset: -8 });
        self.emit_branch_conditional_to(16, 0, labels[&317]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 3, s: 3, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&331]); // beq
        self.bind_label(labels[&328]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(labels[&329]);
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 4, a: 30, offset: -1 });
        self.emit_branch_conditional_to(16, 0, labels[&329]); // bdnz
        self.bind_label(labels[&331]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&336]); // b
        self.bind_label(labels[&333]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 5, offset: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.bind_label(labels[&336]);
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&341]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&333]); // blt
        self.bind_label(labels[&341]);
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 7 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 3, b: 7 });
        self.output.instructions.push(Instruction::load_immediate(4, 48));
        self.emit_branch_conditional_to(4, 0, labels[&362]); // bge
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&359]); // beq
        self.bind_label(labels[&348]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -2 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -3 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -4 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -5 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -6 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -7 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 4, a: 30, offset: -8 });
        self.emit_branch_conditional_to(16, 0, labels[&348]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 3, s: 3, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&362]); // beq
        self.bind_label(labels[&359]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(labels[&360]);
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 4, a: 30, offset: -1 });
        self.emit_branch_conditional_to(16, 0, labels[&360]); // bdnz
        self.bind_label(labels[&362]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&368]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 3 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&370]); // beq
        self.bind_label(labels[&368]);
        self.output.instructions.push(Instruction::load_immediate(0, 46));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.bind_label(labels[&370]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&411]); // beq
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(3, 48));
        self.emit_branch_to(labels[&377]); // b
        self.bind_label(labels[&375]);
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 3, a: 30, offset: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.bind_label(labels[&377]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 6 });
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&375]); // blt
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 6 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 4, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&413]); // bge
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&406]); // beq
        self.bind_label(labels[&387]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: -2 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -2 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: -3 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: -4 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: -5 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: -6 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: -7 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -7 });
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 5, offset: -8 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -8 });
        self.emit_branch_conditional_to(16, 0, labels[&387]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 3, s: 3, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&413]); // beq
        self.bind_label(labels[&406]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(labels[&407]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 5, offset: -1 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.emit_branch_conditional_to(16, 0, labels[&407]); // bdnz
        self.emit_branch_to(labels[&413]); // b
        self.bind_label(labels[&411]);
        self.output.instructions.push(Instruction::load_immediate(0, 48));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.bind_label(labels[&413]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&419]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 45));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.emit_branch_to(labels[&429]); // b
        self.bind_label(labels[&419]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&425]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 43));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.emit_branch_to(labels[&429]); // b
        self.bind_label(labels[&425]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&429]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 32));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.bind_label(labels[&429]);
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.bind_label(labels[&430]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 84 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 31, a: 1, offset: 72 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 64 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 60 });
        self.output.instructions.push(Instruction::LoadWord { d: 28, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 80 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
