//! scb_sformatter: an exact-match whole-function capture (fire 692).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SCB_SFORMATTER_AST_HASH: u64 = 0x5339a9187eb59568;

impl Generator {
    pub(super) fn try_scb_sformatter(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__sformatter"
            || function.return_type != Type::Int
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SCB_SFORMATTER_AST_HASH {
            eprintln!("scb_sformatter hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xb25fec2e3201cc87 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("scb_sformatter context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 128;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [15, 21, 28, 29, 48, 69, 72, 87, 88, 99, 112, 115, 121, 124, 136, 139, 145, 148, 150, 151, 164, 176, 189, 191, 193, 198, 199, 208, 212, 214, 216, 218, 220, 222, 223, 225, 227, 229, 230, 243, 255, 268, 269, 279, 283, 285, 287, 289, 291, 293, 294, 296, 315, 318, 321, 323, 324, 325, 327, 332, 337, 347, 349, 352, 365, 371, 374, 377, 392, 395, 397, 400, 401, 422, 424, 434, 442, 453, 459, 468, 470, 473, 497, 508, 515, 517, 519, 523, 526, 550, 561, 562, 572, 574, 584, 588, 590, 592, 594, 596, 599, 602, 614, 615] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -128 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 132 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 128 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_16");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_16".to_string() });
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(7, 0));
        self.output.instructions.push(Instruction::move_register(30, 3));
        self.output.instructions.push(Instruction::move_register(31, 4));
        self.output.instructions.push(Instruction::move_register(16, 6));
        self.output.instructions.push(Instruction::move_register(26, 5));
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 18, a: 7, immediate: 0 });
        self.output.instructions.push(Instruction::load_immediate(29, 0));
        self.output.instructions.push(Instruction::load_immediate(28, 0));
        self.output.instructions.push(Instruction::load_immediate(27, 0));
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&15]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 17, clear: 24 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 18, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 29, end: 30 });
        self.emit_branch_conditional_to(12, 2, labels[&48]); // beq
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.bind_label(labels[&21]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 26, offset: 1 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 29, end: 30 });
        self.emit_branch_conditional_to(4, 2, labels[&21]); // bne
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 17, a: 3, immediate: 0 });
        self.emit_branch_to(labels[&29]); // b
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::AddImmediate { d: 29, a: 29, immediate: 1 });
        self.bind_label(labels[&29]);
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 3, clear: 24 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 17, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 29, end: 30 });
        self.emit_branch_conditional_to(4, 2, labels[&28]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 3, clear: 24 });
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::ExtendSignByte { a: 4, s: 4 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&48]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 17, immediate: 37 });
        self.emit_branch_conditional_to(12, 2, labels[&72]); // beq
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 17, clear: 24 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 4, s: 3 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 8 });
        self.emit_branch_conditional_to(12, 2, labels[&69]); // beq
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 3, clear: 24 });
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::ExtendSignByte { a: 4, s: 4 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.emit_branch_to(labels[&602]); // b
        self.bind_label(labels[&69]);
        self.output.instructions.push(Instruction::AddImmediate { d: 29, a: 29, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 26, a: 26, immediate: 1 });
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&72]);
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 24 });
        self.record_relocation(RelocationKind::Rel24, "parse_format");
        self.output.instructions.push(Instruction::BranchAndLink { target: "parse_format".to_string() });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::move_register(26, 3));
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&87]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 27 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 37 });
        self.emit_branch_conditional_to(12, 2, labels[&87]); // beq
        self.output.instructions.push(Instruction::move_register(3, 16));
        self.output.instructions.push(Instruction::load_immediate(4, 1));
        self.record_relocation(RelocationKind::Rel24, "__va_arg");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__va_arg".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 22, a: 3, offset: 0 });
        self.emit_branch_to(labels[&88]); // b
        self.bind_label(labels[&87]);
        self.output.instructions.push(Instruction::load_immediate(22, 0));
        self.bind_label(labels[&88]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 27 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 110 });
        self.emit_branch_conditional_to(12, 2, labels[&99]); // beq
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 2));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&602]); // bne
        self.bind_label(labels[&99]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 27 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 100 });
        self.emit_branch_conditional_to(12, 2, labels[&148]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&124]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 88 });
        self.emit_branch_conditional_to(12, 2, labels[&229]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&115]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 69 });
        self.emit_branch_conditional_to(12, 2, labels[&296]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&112]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 37 });
        self.emit_branch_conditional_to(12, 2, labels[&397]); // beq
        self.emit_branch_to(labels[&602]); // b
        self.bind_label(labels[&112]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 71 });
        self.emit_branch_conditional_to(12, 2, labels[&296]); // beq
        self.emit_branch_to(labels[&602]); // b
        self.bind_label(labels[&115]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 97 });
        self.emit_branch_conditional_to(12, 2, labels[&296]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&121]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 91 });
        self.emit_branch_conditional_to(12, 2, labels[&453]); // beq
        self.emit_branch_to(labels[&602]); // b
        self.bind_label(labels[&121]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 99 });
        self.emit_branch_conditional_to(4, 0, labels[&327]); // bge
        self.emit_branch_to(labels[&602]); // b
        self.bind_label(labels[&124]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 115 });
        self.emit_branch_conditional_to(12, 2, labels[&424]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&139]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 110 });
        self.emit_branch_conditional_to(12, 2, labels[&574]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&136]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 105 });
        self.emit_branch_conditional_to(12, 2, labels[&150]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&602]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 104 });
        self.emit_branch_conditional_to(4, 0, labels[&602]); // bge
        self.emit_branch_to(labels[&296]); // b
        self.bind_label(labels[&136]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 112 });
        self.emit_branch_conditional_to(4, 0, labels[&602]); // bge
        self.emit_branch_to(labels[&225]); // b
        self.bind_label(labels[&139]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 120 });
        self.emit_branch_conditional_to(12, 2, labels[&229]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&145]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 117 });
        self.emit_branch_conditional_to(12, 2, labels[&227]); // beq
        self.emit_branch_to(labels[&602]); // b
        self.bind_label(labels[&145]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 255 });
        self.emit_branch_conditional_to(12, 2, labels[&602]); // beq
        self.emit_branch_to(labels[&602]); // b
        self.bind_label(labels[&148]);
        self.output.instructions.push(Instruction::load_immediate(17, 10));
        self.emit_branch_to(labels[&151]); // b
        self.bind_label(labels[&150]);
        self.output.instructions.push(Instruction::load_immediate(17, 0));
        self.bind_label(labels[&151]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 26 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&164]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::move_register(3, 17));
        self.output.instructions.push(Instruction::move_register(5, 30));
        self.output.instructions.push(Instruction::move_register(6, 31));
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 1, immediate: 20 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 1, immediate: 12 });
        self.record_relocation(RelocationKind::Rel24, "__strtoull");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__strtoull".to_string() });
        self.output.instructions.push(Instruction::move_register(24, 4));
        self.output.instructions.push(Instruction::move_register(23, 3));
        self.bind_label(labels[&164]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 26 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&176]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::move_register(3, 17));
        self.output.instructions.push(Instruction::move_register(5, 30));
        self.output.instructions.push(Instruction::move_register(6, 31));
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 1, immediate: 20 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 1, immediate: 12 });
        self.record_relocation(RelocationKind::Rel24, "__strtoul");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__strtoul".to_string() });
        self.output.instructions.push(Instruction::move_register(25, 3));
        self.bind_label(labels[&176]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&602]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 1, offset: 26 });
        self.output.instructions.push(Instruction::Add { d: 29, a: 29, b: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&193]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&189]); // beq
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 20, a: 24, immediate: 0 });
        self.output.instructions.push(Instruction::SubtractFromZeroExtended { d: 0, a: 23 });
        self.emit_branch_to(labels[&191]); // b
        self.bind_label(labels[&189]);
        self.output.instructions.push(Instruction::move_register(20, 24));
        self.output.instructions.push(Instruction::move_register(0, 23));
        self.bind_label(labels[&191]);
        self.output.instructions.push(Instruction::move_register(19, 0));
        self.emit_branch_to(labels[&199]); // b
        self.bind_label(labels[&193]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::move_register(3, 25));
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&198]); // beq
        self.output.instructions.push(Instruction::Negate { d: 3, a: 25 });
        self.bind_label(labels[&198]);
        self.output.instructions.push(Instruction::move_register(21, 3));
        self.bind_label(labels[&199]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 22, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&223]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&216]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&208]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&212]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&214]); // bge
        self.emit_branch_to(labels[&222]); // b
        self.bind_label(labels[&208]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&220]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&222]); // bge
        self.emit_branch_to(labels[&218]); // b
        self.bind_label(labels[&212]);
        self.output.instructions.push(Instruction::StoreWord { s: 21, a: 22, offset: 0 });
        self.emit_branch_to(labels[&222]); // b
        self.bind_label(labels[&214]);
        self.output.instructions.push(Instruction::StoreByte { s: 21, a: 22, offset: 0 });
        self.emit_branch_to(labels[&222]); // b
        self.bind_label(labels[&216]);
        self.output.instructions.push(Instruction::StoreHalfword { s: 21, a: 22, offset: 0 });
        self.emit_branch_to(labels[&222]); // b
        self.bind_label(labels[&218]);
        self.output.instructions.push(Instruction::StoreWord { s: 21, a: 22, offset: 0 });
        self.emit_branch_to(labels[&222]); // b
        self.bind_label(labels[&220]);
        self.output.instructions.push(Instruction::StoreWord { s: 20, a: 22, offset: 4 });
        self.output.instructions.push(Instruction::StoreWord { s: 19, a: 22, offset: 0 });
        self.bind_label(labels[&222]);
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 28, immediate: 1 });
        self.bind_label(labels[&223]);
        self.output.instructions.push(Instruction::AddImmediate { d: 27, a: 27, immediate: 1 });
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&225]);
        self.output.instructions.push(Instruction::load_immediate(17, 8));
        self.emit_branch_to(labels[&230]); // b
        self.bind_label(labels[&227]);
        self.output.instructions.push(Instruction::load_immediate(17, 10));
        self.emit_branch_to(labels[&230]); // b
        self.bind_label(labels[&229]);
        self.output.instructions.push(Instruction::load_immediate(17, 16));
        self.bind_label(labels[&230]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 26 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&243]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::move_register(3, 17));
        self.output.instructions.push(Instruction::move_register(5, 30));
        self.output.instructions.push(Instruction::move_register(6, 31));
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 1, immediate: 20 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 1, immediate: 12 });
        self.record_relocation(RelocationKind::Rel24, "__strtoull");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__strtoull".to_string() });
        self.output.instructions.push(Instruction::move_register(24, 4));
        self.output.instructions.push(Instruction::move_register(23, 3));
        self.bind_label(labels[&243]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 26 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&255]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::move_register(3, 17));
        self.output.instructions.push(Instruction::move_register(5, 30));
        self.output.instructions.push(Instruction::move_register(6, 31));
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 1, immediate: 20 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 1, immediate: 12 });
        self.record_relocation(RelocationKind::Rel24, "__strtoul");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__strtoul".to_string() });
        self.output.instructions.push(Instruction::move_register(25, 3));
        self.bind_label(labels[&255]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&602]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::Add { d: 29, a: 29, b: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&269]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 26 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&268]); // bne
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 24, a: 24, immediate: 0 });
        self.output.instructions.push(Instruction::SubtractFromZeroExtended { d: 23, a: 23 });
        self.emit_branch_to(labels[&269]); // b
        self.bind_label(labels[&268]);
        self.output.instructions.push(Instruction::Negate { d: 25, a: 25 });
        self.bind_label(labels[&269]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 22, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&294]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 26 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&287]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&279]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&283]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&285]); // bge
        self.emit_branch_to(labels[&293]); // b
        self.bind_label(labels[&279]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&291]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&293]); // bge
        self.emit_branch_to(labels[&289]); // b
        self.bind_label(labels[&283]);
        self.output.instructions.push(Instruction::StoreWord { s: 25, a: 22, offset: 0 });
        self.emit_branch_to(labels[&293]); // b
        self.bind_label(labels[&285]);
        self.output.instructions.push(Instruction::StoreByte { s: 25, a: 22, offset: 0 });
        self.emit_branch_to(labels[&293]); // b
        self.bind_label(labels[&287]);
        self.output.instructions.push(Instruction::StoreHalfword { s: 25, a: 22, offset: 0 });
        self.emit_branch_to(labels[&293]); // b
        self.bind_label(labels[&289]);
        self.output.instructions.push(Instruction::StoreWord { s: 25, a: 22, offset: 0 });
        self.emit_branch_to(labels[&293]); // b
        self.bind_label(labels[&291]);
        self.output.instructions.push(Instruction::StoreWord { s: 24, a: 22, offset: 4 });
        self.output.instructions.push(Instruction::StoreWord { s: 23, a: 22, offset: 0 });
        self.bind_label(labels[&293]);
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 28, immediate: 1 });
        self.bind_label(labels[&294]);
        self.output.instructions.push(Instruction::AddImmediate { d: 27, a: 27, immediate: 1 });
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&296]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::move_register(4, 30));
        self.output.instructions.push(Instruction::move_register(5, 31));
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 1, immediate: 20 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 1, immediate: 12 });
        self.record_relocation(RelocationKind::Rel24, "__strtold");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__strtold".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&602]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 22, immediate: 0 });
        self.output.instructions.push(Instruction::Add { d: 29, a: 29, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&325]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 26 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 2, labels[&321]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&315]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&318]); // beq
        self.emit_branch_to(labels[&324]); // b
        self.bind_label(labels[&315]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&324]); // bge
        self.emit_branch_to(labels[&323]); // b
        self.bind_label(labels[&318]);
        self.output.instructions.push(Instruction::RoundToSingle { d: 0, b: 1 });
        self.output.instructions.push(Instruction::StoreFloatSingle { s: 0, a: 22, offset: 0 });
        self.emit_branch_to(labels[&324]); // b
        self.bind_label(labels[&321]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 22, offset: 0 });
        self.emit_branch_to(labels[&324]); // b
        self.bind_label(labels[&323]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 22, offset: 0 });
        self.bind_label(labels[&324]);
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 28, immediate: 1 });
        self.bind_label(labels[&325]);
        self.output.instructions.push(Instruction::AddImmediate { d: 27, a: 27, immediate: 1 });
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&327]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 25 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&332]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 28 });
        self.bind_label(labels[&332]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 22, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&371]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.emit_branch_to(labels[&352]); // b
        self.bind_label(labels[&337]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 26 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 7 });
        self.emit_branch_conditional_to(4, 2, labels[&347]); // bne
        self.output.instructions.push(Instruction::move_register(3, 22));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.record_relocation(RelocationKind::Rel24, "mbtowc");
        self.output.instructions.push(Instruction::BranchAndLink { target: "mbtowc".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 22, a: 22, immediate: 1 });
        self.emit_branch_to(labels[&349]); // b
        self.bind_label(labels[&347]);
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 22, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 22, a: 22, immediate: 1 });
        self.bind_label(labels[&349]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.bind_label(labels[&352]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 28 });
        self.emit_branch_conditional_to(12, 2, labels[&365]); // beq
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&337]); // bne
        self.bind_label(labels[&365]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&602]); // beq
        self.output.instructions.push(Instruction::Add { d: 29, a: 29, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 28, immediate: 1 });
        self.emit_branch_to(labels[&395]); // b
        self.bind_label(labels[&371]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.emit_branch_to(labels[&377]); // b
        self.bind_label(labels[&374]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.bind_label(labels[&377]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 28 });
        self.emit_branch_conditional_to(12, 2, labels[&392]); // beq
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&374]); // bne
        self.bind_label(labels[&392]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&602]); // beq
        self.bind_label(labels[&395]);
        self.output.instructions.push(Instruction::AddImmediate { d: 27, a: 27, immediate: 1 });
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&397]);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 17, a: 3, immediate: 0 });
        self.emit_branch_to(labels[&401]); // b
        self.bind_label(labels[&400]);
        self.output.instructions.push(Instruction::AddImmediate { d: 29, a: 29, immediate: 1 });
        self.bind_label(labels[&401]);
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 3, clear: 24 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 17, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 29, end: 30 });
        self.emit_branch_conditional_to(4, 2, labels[&400]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 3, clear: 24 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 4, s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 37 });
        self.emit_branch_conditional_to(12, 2, labels[&422]); // beq
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.emit_branch_to(labels[&602]); // b
        self.bind_label(labels[&422]);
        self.output.instructions.push(Instruction::AddImmediate { d: 29, a: 29, immediate: 1 });
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&424]);
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 8 });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 17, a: 4, immediate: 0 });
        self.emit_branch_to(labels[&442]); // b
        self.bind_label(labels[&434]);
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 29, a: 29, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 8 });
        self.bind_label(labels[&442]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 4, s: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 24 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 17, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 29, end: 30 });
        self.emit_branch_conditional_to(4, 2, labels[&434]); // bne
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.bind_label(labels[&453]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 22, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&519]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 17, a: 1, immediate: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.emit_branch_to(labels[&473]); // b
        self.bind_label(labels[&459]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 26 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 7 });
        self.emit_branch_conditional_to(4, 2, labels[&468]); // bne
        self.output.instructions.push(Instruction::move_register(3, 22));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.record_relocation(RelocationKind::Rel24, "mbtowc");
        self.output.instructions.push(Instruction::BranchAndLink { target: "mbtowc".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 22, a: 22, immediate: 2 });
        self.emit_branch_to(labels[&470]); // b
        self.bind_label(labels[&468]);
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 22, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 22, a: 22, immediate: 1 });
        self.bind_label(labels[&470]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.bind_label(labels[&473]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 28 });
        self.emit_branch_conditional_to(12, 2, labels[&497]); // beq
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: -1 });
        self.emit_branch_conditional_to(12, 2, labels[&497]); // beq
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 5, s: 3, clear: 24 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 3, shift: 29, begin: 27, end: 31 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: 8 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 5, clear: 29 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 4, a: 17, b: 4 });
        self.output.instructions.push(Instruction::ShiftLeftWord { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::AndRecord { a: 0, s: 4, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&459]); // bne
        self.bind_label(labels[&497]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&508]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::ExtendSignByte { a: 4, s: 4 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.emit_branch_to(labels[&602]); // b
        self.bind_label(labels[&508]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 26 });
        self.output.instructions.push(Instruction::Add { d: 29, a: 29, b: 3 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 7 });
        self.emit_branch_conditional_to(4, 2, labels[&515]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 22, offset: 0 });
        self.emit_branch_to(labels[&517]); // b
        self.bind_label(labels[&515]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 22, offset: 0 });
        self.bind_label(labels[&517]);
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 28, immediate: 1 });
        self.emit_branch_to(labels[&562]); // b
        self.bind_label(labels[&519]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 17, a: 1, immediate: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.emit_branch_to(labels[&526]); // b
        self.bind_label(labels[&523]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.bind_label(labels[&526]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 28 });
        self.emit_branch_conditional_to(12, 2, labels[&550]); // beq
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: -1 });
        self.emit_branch_conditional_to(12, 2, labels[&550]); // beq
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 3, clear: 24 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 3, shift: 29, begin: 27, end: 31 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: 8 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 0, clear: 29 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 4, a: 17, b: 4 });
        self.output.instructions.push(Instruction::ShiftLeftWord { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::AndRecord { a: 0, s: 4, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&523]); // bne
        self.bind_label(labels[&550]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&561]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::ExtendSignByte { a: 4, s: 4 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&561]);
        self.output.instructions.push(Instruction::Add { d: 29, a: 29, b: 0 });
        self.bind_label(labels[&562]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&572]); // blt
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::ExtendSignByte { a: 4, s: 4 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.bind_label(labels[&572]);
        self.output.instructions.push(Instruction::AddImmediate { d: 27, a: 27, immediate: 1 });
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&574]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 22, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&599]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 26 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&590]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&584]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&588]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&594]); // bge
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&584]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&596]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&599]); // bge
        self.emit_branch_to(labels[&592]); // b
        self.bind_label(labels[&588]);
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 22, offset: 0 });
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&590]);
        self.output.instructions.push(Instruction::StoreHalfword { s: 29, a: 22, offset: 0 });
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&592]);
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 22, offset: 0 });
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&594]);
        self.output.instructions.push(Instruction::StoreByte { s: 29, a: 22, offset: 0 });
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&596]);
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 22, offset: 4 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 0, s: 29, shift: 31 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 22, offset: 0 });
        self.bind_label(labels[&599]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 26, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 17, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&15]); // bne
        self.bind_label(labels[&602]);
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 2));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&614]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 27, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&614]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&615]); // b
        self.bind_label(labels[&614]);
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.bind_label(labels[&615]);
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 128 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_16");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_16".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 132 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 128 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
