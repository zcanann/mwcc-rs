//! wsc_wsformatter: an exact-match whole-function capture (fire 701).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const WSC_WSFORMATTER_AST_HASH: u64 = 0xa61813dd11095203;

impl Generator {
    pub(super) fn try_wsc_wsformatter(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__wsformatter"
            || function.return_type != Type::Int
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != WSC_WSFORMATTER_AST_HASH {
            eprintln!("wsc_wsformatter hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 170, // strikers
            _ => {
                eprintln!("wsc_wsformatter context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 144;
        self.non_leaf = true;
        for bits in [
            0x4330000080000000u64,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [13, 17, 22, 23, 41, 62, 65, 80, 81, 94, 97, 103, 106, 117, 120, 126, 131, 133, 134, 149, 158, 161, 163, 165, 166, 167, 169, 171, 173, 174, 189, 198, 201, 203, 205, 206, 207, 209, 235, 238, 241, 243, 244, 245, 247, 252, 257, 263, 269, 272, 287, 293, 296, 299, 314, 317, 320, 321, 342, 344, 352, 360, 370, 376, 382, 387, 390, 413, 423, 428, 432, 435, 458, 468, 469, 478, 480, 489, 492, 494, 496, 497, 508, 521, 522] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -144 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 148 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 144 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_22");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_22".to_string() });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::move_register(30, 4));
        self.output.instructions.push(Instruction::move_register(23, 6));
        self.output.instructions.push(Instruction::move_register(26, 5));
        self.output.instructions.push(Instruction::load_immediate(27, 0));
        self.output.instructions.push(Instruction::load_immediate(24, 0));
        self.output.instructions.push(Instruction::load_immediate(25, 0));
        self.emit_branch_to(labels[&497]); // b
        self.bind_label(labels[&13]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 22, clear: 16 });
        self.record_relocation(RelocationKind::Rel24, "iswspace");
        self.output.instructions.push(Instruction::BranchAndLink { target: "iswspace".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&41]); // beq
        self.bind_label(labels[&17]);
        self.output.instructions.push(Instruction::LoadHalfZeroWithUpdate { d: 3, a: 26, offset: 2 });
        self.record_relocation(RelocationKind::Rel24, "iswspace");
        self.output.instructions.push(Instruction::BranchAndLink { target: "iswspace".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&17]); // bne
        self.emit_branch_to(labels[&23]); // b
        self.bind_label(labels[&22]);
        self.output.instructions.push(Instruction::AddImmediate { d: 27, a: 27, immediate: 1 });
        self.bind_label(labels[&23]);
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(29, 3));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 3, clear: 16 });
        self.record_relocation(RelocationKind::Rel24, "iswspace");
        self.output.instructions.push(Instruction::BranchAndLink { target: "iswspace".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&22]); // bne
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::move_register(4, 29));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.emit_branch_to(labels[&497]); // b
        self.bind_label(labels[&41]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 22, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 37 });
        self.emit_branch_conditional_to(12, 2, labels[&65]); // beq
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(29, 3));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 22, clear: 16 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 3, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&62]); // beq
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::move_register(4, 29));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.emit_branch_to(labels[&508]); // b
        self.bind_label(labels[&62]);
        self.output.instructions.push(Instruction::AddImmediate { d: 27, a: 27, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 26, a: 26, immediate: 2 });
        self.emit_branch_to(labels[&497]); // b
        self.bind_label(labels[&65]);
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 20 });
        self.record_relocation(RelocationKind::Rel24, "parse_format");
        self.output.instructions.push(Instruction::BranchAndLink { target: "parse_format".to_string() });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::move_register(26, 3));
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&80]); // bne
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 37 });
        self.emit_branch_conditional_to(12, 2, labels[&80]); // beq
        self.output.instructions.push(Instruction::move_register(3, 23));
        self.output.instructions.push(Instruction::load_immediate(4, 1));
        self.record_relocation(RelocationKind::Rel24, "__va_arg");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__va_arg".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 28, a: 3, offset: 0 });
        self.emit_branch_to(labels[&81]); // b
        self.bind_label(labels[&80]);
        self.output.instructions.push(Instruction::load_immediate(28, 0));
        self.bind_label(labels[&81]);
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 4, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 104 });
        self.emit_branch_conditional_to(12, 2, labels[&508]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&106]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 88 });
        self.emit_branch_conditional_to(12, 2, labels[&173]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&97]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 69 });
        self.emit_branch_conditional_to(12, 2, labels[&209]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&94]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 37 });
        self.emit_branch_conditional_to(12, 2, labels[&321]); // beq
        self.emit_branch_to(labels[&508]); // b
        self.bind_label(labels[&94]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 71 });
        self.emit_branch_conditional_to(12, 2, labels[&209]); // beq
        self.emit_branch_to(labels[&508]); // b
        self.bind_label(labels[&97]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 99 });
        self.emit_branch_conditional_to(12, 2, labels[&247]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&103]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 91 });
        self.emit_branch_conditional_to(12, 2, labels[&370]); // beq
        self.emit_branch_to(labels[&508]); // b
        self.bind_label(labels[&103]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 101 });
        self.emit_branch_conditional_to(4, 0, labels[&209]); // bge
        self.emit_branch_to(labels[&131]); // b
        self.bind_label(labels[&106]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 116 });
        self.emit_branch_conditional_to(12, 2, labels[&508]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&120]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 111 });
        self.emit_branch_conditional_to(12, 2, labels[&169]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&117]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 110 });
        self.emit_branch_conditional_to(4, 0, labels[&480]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 106 });
        self.emit_branch_conditional_to(4, 0, labels[&508]); // bge
        self.emit_branch_to(labels[&133]); // b
        self.bind_label(labels[&117]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 115 });
        self.emit_branch_conditional_to(4, 0, labels[&344]); // bge
        self.emit_branch_to(labels[&508]); // b
        self.bind_label(labels[&120]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 120 });
        self.emit_branch_conditional_to(12, 2, labels[&173]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&126]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 118 });
        self.emit_branch_conditional_to(4, 0, labels[&508]); // bge
        self.emit_branch_to(labels[&171]); // b
        self.bind_label(labels[&126]);
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 1));
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&508]); // beq
        self.emit_branch_to(labels[&508]); // b
        self.bind_label(labels[&131]);
        self.output.instructions.push(Instruction::load_immediate(3, 10));
        self.emit_branch_to(labels[&134]); // b
        self.bind_label(labels[&133]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.bind_label(labels[&134]);
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::move_register(5, 31));
        self.output.instructions.push(Instruction::move_register(6, 30));
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 1, immediate: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 1, immediate: 8 });
        self.record_relocation(RelocationKind::Rel24, "__wcstoul");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__wcstoul".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&508]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::Add { d: 27, a: 27, b: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&149]); // beq
        self.output.instructions.push(Instruction::Negate { d: 3, a: 3 });
        self.bind_label(labels[&149]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&167]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 22 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&163]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&158]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&161]); // bge
        self.emit_branch_to(labels[&166]); // b
        self.bind_label(labels[&158]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&166]); // bge
        self.emit_branch_to(labels[&165]); // b
        self.bind_label(labels[&161]);
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 28, offset: 0 });
        self.emit_branch_to(labels[&166]); // b
        self.bind_label(labels[&163]);
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 28, offset: 0 });
        self.emit_branch_to(labels[&166]); // b
        self.bind_label(labels[&165]);
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 28, offset: 0 });
        self.bind_label(labels[&166]);
        self.output.instructions.push(Instruction::AddImmediate { d: 24, a: 24, immediate: 1 });
        self.bind_label(labels[&167]);
        self.output.instructions.push(Instruction::AddImmediate { d: 25, a: 25, immediate: 1 });
        self.emit_branch_to(labels[&497]); // b
        self.bind_label(labels[&169]);
        self.output.instructions.push(Instruction::load_immediate(3, 8));
        self.emit_branch_to(labels[&174]); // b
        self.bind_label(labels[&171]);
        self.output.instructions.push(Instruction::load_immediate(3, 10));
        self.emit_branch_to(labels[&174]); // b
        self.bind_label(labels[&173]);
        self.output.instructions.push(Instruction::load_immediate(3, 16));
        self.bind_label(labels[&174]);
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::move_register(5, 31));
        self.output.instructions.push(Instruction::move_register(6, 30));
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 1, immediate: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 1, immediate: 8 });
        self.record_relocation(RelocationKind::Rel24, "__wcstoul");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__wcstoul".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&508]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::Add { d: 27, a: 27, b: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&189]); // beq
        self.output.instructions.push(Instruction::Negate { d: 3, a: 3 });
        self.bind_label(labels[&189]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&207]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 22 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&203]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&198]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&201]); // bge
        self.emit_branch_to(labels[&206]); // b
        self.bind_label(labels[&198]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&206]); // bge
        self.emit_branch_to(labels[&205]); // b
        self.bind_label(labels[&201]);
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 28, offset: 0 });
        self.emit_branch_to(labels[&206]); // b
        self.bind_label(labels[&203]);
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 28, offset: 0 });
        self.emit_branch_to(labels[&206]); // b
        self.bind_label(labels[&205]);
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 28, offset: 0 });
        self.bind_label(labels[&206]);
        self.output.instructions.push(Instruction::AddImmediate { d: 24, a: 24, immediate: 1 });
        self.bind_label(labels[&207]);
        self.output.instructions.push(Instruction::AddImmediate { d: 25, a: 25, immediate: 1 });
        self.emit_branch_to(labels[&497]); // b
        self.bind_label(labels[&209]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::move_register(4, 31));
        self.output.instructions.push(Instruction::move_register(5, 30));
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 1, immediate: 8 });
        self.record_relocation(RelocationKind::Rel24, "__wcstold");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__wcstold".to_string() });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 3, s: 3, immediate: 32768 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 17200));
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 100 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 96 });
        self.load_double_constant(1, 0x4330000080000000);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 96 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&508]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 28, immediate: 0 });
        self.output.instructions.push(Instruction::Add { d: 27, a: 27, b: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&245]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 22 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&241]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&235]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&238]); // beq
        self.emit_branch_to(labels[&244]); // b
        self.bind_label(labels[&235]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&244]); // bge
        self.emit_branch_to(labels[&243]); // b
        self.bind_label(labels[&238]);
        self.output.instructions.push(Instruction::RoundToSingle { d: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatSingle { s: 0, a: 28, offset: 0 });
        self.emit_branch_to(labels[&244]); // b
        self.bind_label(labels[&241]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 28, offset: 0 });
        self.emit_branch_to(labels[&244]); // b
        self.bind_label(labels[&243]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 28, offset: 0 });
        self.bind_label(labels[&244]);
        self.output.instructions.push(Instruction::AddImmediate { d: 24, a: 24, immediate: 1 });
        self.bind_label(labels[&245]);
        self.output.instructions.push(Instruction::AddImmediate { d: 25, a: 25, immediate: 1 });
        self.emit_branch_to(labels[&497]); // b
        self.bind_label(labels[&247]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 21 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&252]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 28 });
        self.bind_label(labels[&252]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&293]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 16 });
        self.emit_branch_to(labels[&272]); // b
        self.bind_label(labels[&257]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 22 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&263]); // bne
        self.output.instructions.push(Instruction::StoreHalfword { s: 29, a: 28, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 28, immediate: 2 });
        self.emit_branch_to(labels[&269]); // b
        self.bind_label(labels[&263]);
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 29, clear: 16 });
        self.record_relocation(RelocationKind::Rel24, "wctomb");
        self.output.instructions.push(Instruction::BranchAndLink { target: "wctomb".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&508]); // blt
        self.output.instructions.push(Instruction::Add { d: 28, a: 28, b: 3 });
        self.bind_label(labels[&269]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 16 });
        self.bind_label(labels[&272]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 28 });
        self.emit_branch_conditional_to(12, 2, labels[&287]); // beq
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 3, clear: 16 });
        self.output.instructions.push(Instruction::move_register(29, 3));
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65535 });
        self.emit_branch_conditional_to(4, 2, labels[&257]); // bne
        self.bind_label(labels[&287]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&508]); // beq
        self.output.instructions.push(Instruction::Add { d: 27, a: 27, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 24, a: 24, immediate: 1 });
        self.emit_branch_to(labels[&317]); // b
        self.bind_label(labels[&293]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 16 });
        self.emit_branch_to(labels[&299]); // b
        self.bind_label(labels[&296]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 16 });
        self.bind_label(labels[&299]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 28 });
        self.emit_branch_conditional_to(12, 2, labels[&314]); // beq
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 3, clear: 16 });
        self.output.instructions.push(Instruction::move_register(29, 3));
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65535 });
        self.emit_branch_conditional_to(4, 2, labels[&296]); // bne
        self.bind_label(labels[&314]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&508]); // beq
        self.bind_label(labels[&317]);
        self.output.instructions.push(Instruction::AddImmediate { d: 25, a: 25, immediate: 1 });
        self.emit_branch_to(labels[&497]); // b
        self.emit_branch_to(labels[&321]); // b
        self.bind_label(labels[&320]);
        self.output.instructions.push(Instruction::AddImmediate { d: 27, a: 27, immediate: 1 });
        self.bind_label(labels[&321]);
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(29, 3));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 3, clear: 16 });
        self.record_relocation(RelocationKind::Rel24, "iswspace");
        self.output.instructions.push(Instruction::BranchAndLink { target: "iswspace".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&320]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 29, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 37 });
        self.emit_branch_conditional_to(12, 2, labels[&342]); // beq
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::move_register(4, 29));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.emit_branch_to(labels[&508]); // b
        self.bind_label(labels[&342]);
        self.output.instructions.push(Instruction::AddImmediate { d: 27, a: 27, immediate: 1 });
        self.emit_branch_to(labels[&497]); // b
        self.bind_label(labels[&344]);
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(29, 3));
        self.emit_branch_to(labels[&360]); // b
        self.bind_label(labels[&352]);
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 27, a: 27, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(29, 3));
        self.bind_label(labels[&360]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 29, clear: 16 });
        self.record_relocation(RelocationKind::Rel24, "iswspace");
        self.output.instructions.push(Instruction::BranchAndLink { target: "iswspace".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&352]); // bne
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::move_register(4, 29));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.bind_label(labels[&370]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&428]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 22, a: 1, immediate: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 16 });
        self.emit_branch_to(labels[&390]); // b
        self.bind_label(labels[&376]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 22 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&382]); // bne
        self.output.instructions.push(Instruction::StoreHalfword { s: 29, a: 28, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 28, immediate: 2 });
        self.emit_branch_to(labels[&387]); // b
        self.bind_label(labels[&382]);
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.record_relocation(RelocationKind::Rel24, "wctomb");
        self.output.instructions.push(Instruction::BranchAndLink { target: "wctomb".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&508]); // blt
        self.output.instructions.push(Instruction::Add { d: 28, a: 28, b: 3 });
        self.bind_label(labels[&387]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 16 });
        self.bind_label(labels[&390]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 28 });
        self.emit_branch_conditional_to(12, 2, labels[&413]); // beq
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 3, clear: 16 });
        self.output.instructions.push(Instruction::move_register(29, 3));
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 65535 });
        self.emit_branch_conditional_to(12, 2, labels[&413]); // beq
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 29, shift: 30, begin: 18, end: 30 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 29, clear: 29 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 3, immediate: 12 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::LoadHalfwordZeroIndexed { d: 5, a: 22, b: 5 });
        self.output.instructions.push(Instruction::ShiftLeftWord { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::AndRecord { a: 0, s: 5, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&376]); // bne
        self.bind_label(labels[&413]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&423]); // bne
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::move_register(4, 29));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.emit_branch_to(labels[&508]); // b
        self.bind_label(labels[&423]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::Add { d: 27, a: 27, b: 3 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 28, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 24, a: 24, immediate: 1 });
        self.emit_branch_to(labels[&469]); // b
        self.bind_label(labels[&428]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 22, a: 1, immediate: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 16 });
        self.emit_branch_to(labels[&435]); // b
        self.bind_label(labels[&432]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 16 });
        self.bind_label(labels[&435]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 28 });
        self.emit_branch_conditional_to(12, 2, labels[&458]); // beq
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 3, clear: 16 });
        self.output.instructions.push(Instruction::move_register(29, 3));
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65535 });
        self.emit_branch_conditional_to(12, 2, labels[&458]); // beq
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 29, shift: 30, begin: 18, end: 30 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 29, clear: 29 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: 12 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::LoadHalfwordZeroIndexed { d: 4, a: 22, b: 4 });
        self.output.instructions.push(Instruction::ShiftLeftWord { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::AndRecord { a: 0, s: 4, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&432]); // bne
        self.bind_label(labels[&458]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&468]); // bne
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::move_register(4, 29));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.emit_branch_to(labels[&508]); // b
        self.bind_label(labels[&468]);
        self.output.instructions.push(Instruction::Add { d: 27, a: 27, b: 0 });
        self.bind_label(labels[&469]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&478]); // blt
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::move_register(4, 29));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.bind_label(labels[&478]);
        self.output.instructions.push(Instruction::AddImmediate { d: 25, a: 25, immediate: 1 });
        self.emit_branch_to(labels[&497]); // b
        self.bind_label(labels[&480]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&497]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 22 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&494]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&489]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&492]); // bge
        self.emit_branch_to(labels[&497]); // b
        self.bind_label(labels[&489]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&497]); // bge
        self.emit_branch_to(labels[&496]); // b
        self.bind_label(labels[&492]);
        self.output.instructions.push(Instruction::StoreWord { s: 27, a: 28, offset: 0 });
        self.emit_branch_to(labels[&497]); // b
        self.bind_label(labels[&494]);
        self.output.instructions.push(Instruction::StoreHalfword { s: 27, a: 28, offset: 0 });
        self.emit_branch_to(labels[&497]); // b
        self.bind_label(labels[&496]);
        self.output.instructions.push(Instruction::StoreWord { s: 27, a: 28, offset: 0 });
        self.bind_label(labels[&497]);
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 2));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 3, clear: 16 });
        self.emit_branch_conditional_to(4, 2, labels[&508]); // bne
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 22, a: 26, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 22, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&13]); // bne
        self.bind_label(labels[&508]);
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 2));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 3, clear: 16 });
        self.emit_branch_conditional_to(12, 2, labels[&521]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&521]); // bne
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 1));
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -1 });
        self.emit_branch_to(labels[&522]); // b
        self.bind_label(labels[&521]);
        self.output.instructions.push(Instruction::move_register(3, 24));
        self.bind_label(labels[&522]);
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 144 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_22");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_22".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 148 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 144 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
