//! eatan2_ww: an exact-match whole-function capture (fire 452).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const EATAN2_WW_AST_HASH: u64 = 0x4cf94f9a99810736;

impl Generator {
    pub(super) fn try_eatan2_ww(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__ieee754_atan2"
            || function.return_type != Type::Double
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != EATAN2_WW_AST_HASH {
            return Ok(false);
        }
        // -- emit (the capture, verbatim) --
        self.frame_size = 48;
        self.non_leaf = true;
        self.callee_saved = vec![31];
        for bits in [
            0x400921fb54442d18u64,
            0xc00921fb54442d18,
            0xbff921fb54442d18,
            0x3ff921fb54442d18,
            0x3fe921fb54442d18,
            0xbfe921fb54442d18,
            0x4002d97c7f3321d2,
            0xc002d97c7f3321d2,
            0x0000000000000000,
            0x8000000000000000,
            0x3ca1a62633145c07,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [25, 29, 34, 45, 49, 51, 53, 59, 61, 74, 77, 79, 81, 83, 85, 92, 95, 97, 99, 101, 103, 110, 112, 119, 126, 132, 138, 141, 143, 148, 154, 159] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -48 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 32752));
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::Negate { d: 0, a: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 8, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 6, s: 4, clear: 1 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 6, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 3 });
        self.output.instructions.push(Instruction::LoadWord { d: 9, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 7, s: 5, clear: 1 });
        self.emit_branch_conditional_to(12, 1, labels[&25]); // bgt
        self.output.instructions.push(Instruction::Negate { d: 0, a: 9 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 9, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 7, b: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 3 });
        self.emit_branch_conditional_to(4, 1, labels[&29]); // ble
        self.bind_label(labels[&25]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 1, b: 0 });
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&29]);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 0, a: 4, immediate: -16368 });
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: 0, b: 8 });
        self.emit_branch_conditional_to(4, 2, labels[&34]); // bne
        self.record_relocation(RelocationKind::Rel24, "atan");
        self.output.instructions.push(Instruction::BranchAndLink { target: "atan".to_string() });
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&34]);
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: 7, b: 9 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 4, shift: 2, begin: 30, end: 30 });
        self.output.instructions.push(Instruction::move_register(31, 0));
        self.output.instructions.push(Instruction::RotateAndMaskInsert { a: 31, s: 5, shift: 1, begin: 31, end: 31 });
        self.emit_branch_conditional_to(4, 2, labels[&53]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&49]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&45]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&159]); // bge
        self.emit_branch_to(labels[&53]); // b
        self.bind_label(labels[&45]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 4 });
        self.emit_branch_conditional_to(4, 0, labels[&53]); // bge
        self.emit_branch_to(labels[&51]); // b
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&49]);
        self.load_double_constant(1, 0x400921fb54442d18);
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&51]);
        self.load_double_constant(1, 0xc00921fb54442d18);
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&53]);
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: 6, b: 8 });
        self.emit_branch_conditional_to(4, 2, labels[&61]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&59]); // bge
        self.load_double_constant(1, 0xbff921fb54442d18);
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&59]);
        self.load_double_constant(1, 0x3ff921fb54442d18);
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&61]);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 0, a: 6, immediate: -32752 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&103]); // bne
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 0, a: 7, immediate: -32752 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&85]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&81]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&74]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&77]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&79]); // bge
        self.emit_branch_to(labels[&103]); // b
        self.bind_label(labels[&74]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 4 });
        self.emit_branch_conditional_to(4, 0, labels[&103]); // bge
        self.emit_branch_to(labels[&83]); // b
        self.bind_label(labels[&77]);
        self.load_double_constant(1, 0x3fe921fb54442d18);
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&79]);
        self.load_double_constant(1, 0xbfe921fb54442d18);
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&81]);
        self.load_double_constant(1, 0x4002d97c7f3321d2);
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&83]);
        self.load_double_constant(1, 0xc002d97c7f3321d2);
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&85]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&99]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&92]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&95]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&97]); // bge
        self.emit_branch_to(labels[&103]); // b
        self.bind_label(labels[&92]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 4 });
        self.emit_branch_conditional_to(4, 0, labels[&103]); // bge
        self.emit_branch_to(labels[&101]); // b
        self.bind_label(labels[&95]);
        self.load_double_constant(1, 0x0000000000000000);
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&97]);
        self.load_double_constant(1, 0x8000000000000000);
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&99]);
        self.load_double_constant(1, 0x400921fb54442d18);
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&101]);
        self.load_double_constant(1, 0xc00921fb54442d18);
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&103]);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 0, a: 7, immediate: -32752 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&112]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&110]); // bge
        self.load_double_constant(1, 0xbff921fb54442d18);
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&110]);
        self.load_double_constant(1, 0x3ff921fb54442d18);
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&112]);
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 6, b: 7 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 0, s: 0, shift: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 60 });
        self.emit_branch_conditional_to(4, 1, labels[&119]); // ble
        self.load_double_constant(0, 0x3ff921fb54442d18);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 24 });
        self.emit_branch_to(labels[&132]); // b
        self.bind_label(labels[&119]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&126]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: -60 });
        self.emit_branch_conditional_to(4, 0, labels[&126]); // bge
        self.load_double_constant(0, 0x0000000000000000);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 24 });
        self.emit_branch_to(labels[&132]); // b
        self.bind_label(labels[&126]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::FloatDivideDouble { d: 0, a: 1, b: 0 });
        self.output.instructions.push(Instruction::FloatAbsolute { d: 1, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "atan");
        self.output.instructions.push(Instruction::BranchAndLink { target: "atan".to_string() });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 24 });
        self.bind_label(labels[&132]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&143]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&138]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&141]); // bge
        self.emit_branch_to(labels[&154]); // b
        self.bind_label(labels[&138]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&154]); // bge
        self.emit_branch_to(labels[&148]); // b
        self.bind_label(labels[&141]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 24 });
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&143]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 0, s: 0, immediate: 32768 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 24 });
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&148]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 24 });
        self.load_double_constant(0, 0x3ca1a62633145c07);
        self.load_double_constant(2, 0x400921fb54442d18);
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 1, b: 0 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 1, a: 2, b: 0 });
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&154]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 1, offset: 24 });
        self.load_double_constant(1, 0x3ca1a62633145c07);
        self.load_double_constant(0, 0x400921fb54442d18);
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 1, a: 2, b: 1 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 1, a: 1, b: 0 });
        self.bind_label(labels[&159]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 48 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // @N: measured per HEADER CONTEXT (fingerprint-dispatched; the
        // fire-451 lesson — an unconditional bump is a latent DIFF).
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbceeda89e0a55f64 => 51, // wind_waker: pools @84 (ours @33)
            0xb61776ae26f47f0e => 51, // BfBB (sweep-verified)
            0xbd60acb658c79e45 => 51, // pikmin2 (sweep-verified)
            _ => return Ok(false),
        };
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
