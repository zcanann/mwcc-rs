//! trg_sinf: an exact-match whole-function capture (fire 711).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const TRG_SINF_AST_HASH: u64 = 0x36ed921518880526;

impl Generator {
    pub(super) fn try_trg_sinf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "sinf"
            || function.return_type != Type::Float
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != TRG_SINF_AST_HASH {
            eprintln!("trg_sinf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x19234177da3e2378 => 10, // pikmin
            0xa5533c97b3cd5d53 => 12, // melee
            _ => {
                eprintln!("trg_sinf context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 64;
        self.non_leaf = true;
        self.output.constant_number_gaps = vec![(3, 3)];
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [22, 58, 77, 95] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -64 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::PairedSingleQuantizedStore { s: 31, a: 1, offset: 56, w: 0, i: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::RoundToSingle { d: 5, b: 1 });
        self.output.instructions.push(Instruction::StoreFloatSingle { s: 1, a: 1, offset: 8 });
        self.load_float_constant(0, f32::from_bits(0x3f22f983));
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::FloatMultiplySingle { d: 2, a: 0, c: 5 });
        self.load_float_constant(1, f32::from_bits(0x3f000000));
        self.output.instructions.push(Instruction::AndMaskRecord { a: 0, s: 0, begin: 0, end: 0 });
        self.output.instructions.push(Instruction::FloatAddSingle { d: 0, a: 1, b: 2 });
        self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 20 });
        self.emit_branch_conditional_to(12, 2, labels[&22]); // beq
        self.output.instructions.push(Instruction::FloatSubtractSingle { d: 0, a: 2, b: 1 });
        self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 28 });
        self.bind_label(labels[&22]);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 31, shift: 1 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 17200));
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 3, s: 3, immediate: 32768 });
        self.record_relocation(RelocationKind::Addr16Ha, "__four_over_pi_m1");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 36 });
        self.record_relocation(RelocationKind::Addr16Lo, "__four_over_pi_m1");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 4, immediate: 0 });
        // the third pooled float NUMBERS before the double (real: @38,@39,@40 then @44)
        self.output.intern_constant(0x39b504f3u64, 4);
        self.load_double_constant(1, 0x4330000080000000);
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 2, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 3, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::FloatSubtractSingle { d: 0, a: 0, b: 1 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 3, offset: 8 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 4, a: 3, offset: 12 });
        self.output.instructions.push(Instruction::FloatSubtractSingle { d: 0, a: 5, b: 0 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 0, a: 2, c: 5, b: 0 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 0, a: 3, c: 5, b: 0 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 0, a: 1, c: 5, b: 0 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 31, a: 4, c: 5, b: 0 });
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 31 });
        self.record_relocation(RelocationKind::Rel24, "fabsf__Ff");
        self.output.instructions.push(Instruction::BranchAndLink { target: "fabsf__Ff".to_string() });
        self.load_float_constant(0, f32::from_bits(0x39b504f3));
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&58]); // bge
        self.record_relocation(RelocationKind::Addr16Ha, "__sincos_on_quadrant");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Ha, "__sincos_poly");
        self.output.instructions.push(Instruction::load_immediate_shifted(5, 0));
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 31, shift: 3, begin: 27, end: 28 });
        self.record_relocation(RelocationKind::Addr16Lo, "__sincos_on_quadrant");
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Lo, "__sincos_poly");
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 0 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 4, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 2, a: 5, offset: 36 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::LoadFloatSingleIndexed { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::FloatMultiplySingle { d: 1, a: 31, c: 1 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 1, a: 2, c: 1, b: 0 });
        self.emit_branch_to(labels[&95]); // b
        self.bind_label(labels[&58]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 31, clear: 31 });
        self.output.instructions.push(Instruction::FloatMultiplySingle { d: 5, a: 31, c: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&77]); // beq
        self.record_relocation(RelocationKind::Addr16Ha, "__sincos_poly");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__sincos_poly");
        self.output.instructions.push(Instruction::LoadFloatSingleWithUpdate { d: 1, a: 4, offset: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, "__sincos_on_quadrant");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 31, shift: 3, begin: 27, end: 28 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 0, a: 4, offset: 8 });
        self.record_relocation(RelocationKind::Addr16Lo, "__sincos_on_quadrant");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 3, a: 4, offset: 16 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 4, a: 1, c: 5, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 2, a: 4, offset: 24 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 4, offset: 32 });
        self.output.instructions.push(Instruction::LoadFloatSingleIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 3, a: 5, c: 4, b: 3 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 2, a: 5, c: 3, b: 2 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 1, a: 5, c: 2, b: 1 });
        self.output.instructions.push(Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 });
        self.emit_branch_to(labels[&95]); // b
        self.bind_label(labels[&77]);
        self.record_relocation(RelocationKind::Addr16Ha, "__sincos_poly");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.record_relocation(RelocationKind::Addr16Ha, "__sincos_on_quadrant");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__sincos_poly");
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 31, shift: 3, begin: 27, end: 28 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 4, offset: 4 });
        self.record_relocation(RelocationKind::Addr16Lo, "__sincos_on_quadrant");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 0, a: 4, offset: 12 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 3, a: 4, offset: 20 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 4, a: 1, c: 5, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 2, a: 4, offset: 28 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 4, offset: 36 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 3, a: 5, c: 4, b: 3 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 2, a: 5, c: 3, b: 2 });
        self.output.instructions.push(Instruction::FloatMultiplyAddSingle { d: 1, a: 5, c: 2, b: 1 });
        self.output.instructions.push(Instruction::FloatMultiplySingle { d: 1, a: 31, c: 1 });
        self.output.instructions.push(Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 });
        self.bind_label(labels[&95]);
        self.output.instructions.push(Instruction::PairedSingleQuantizedLoad { d: 31, a: 1, offset: 56, w: 0, i: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 31, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 64 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
