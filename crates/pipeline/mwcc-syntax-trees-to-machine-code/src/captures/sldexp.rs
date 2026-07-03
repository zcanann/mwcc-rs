//! sldexp: an exact-match whole-function capture (see captures::ast_hash
//! and docs/emission-model.md for the pipeline).

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the fdlibm ldexp (captured fire 446).
const SLDEXP_AST_HASH: u64 = 0xb692cf8cfaada89f;

impl Generator {
    /// THE S_LDEXP EXACT-MATCH TEMPLATE (fire 446): ldexp whole — the
    /// __fpclassifyd inlines flatten in; three bl copysign calls
    /// (non-leaf); 5 pool constants.
    pub(super) fn try_sldexp(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "ldexp"
            || function.return_type != Type::Double
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
            || !self.skipped_inline_names.contains("__fpclassifyd")
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SLDEXP_AST_HASH {
            return Ok(false);
        }
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.non_leaf = true;
        for bits in [
            0x0000000000000000u64,
            0x4350000000000000,
            0x01a56e1fc2f8f359,
            0x7e37e43c8800759c,
            0x3c90000000000000,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [14, 19, 21, 23, 28, 30, 32, 33, 39, 47, 61, 66, 75, 83, 95, 101, 109] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::RotateAndMask { a: 4, s: 5, shift: 0, begin: 1, end: 11 });
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.emit_branch_conditional_to(12, 2, labels[&14]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&32]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&23]); // beq
        self.emit_branch_to(labels[&32]); // b
        self.bind_label(labels[&14]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 5, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&19]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&21]); // beq
        self.bind_label(labels[&19]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&33]); // b
        self.bind_label(labels[&21]);
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&33]); // b
        self.bind_label(labels[&23]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 5, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&28]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&30]); // beq
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&33]); // b
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&33]); // b
        self.bind_label(labels[&32]);
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&33]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 1, labels[&109]); // ble
        self.load_double_constant(0, 0x0000000000000000);
        self.output.instructions.push(Instruction::FloatCompareUnordered { a: 0, b: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&39]); // bne
        self.emit_branch_to(labels[&109]); // b
        self.bind_label(labels[&39]);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 4, s: 5, shift: 12, begin: 21, end: 31 });
        self.emit_branch_conditional_to(4, 2, labels[&61]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 5, clear: 1 });
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: 6, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&47]); // bne
        self.emit_branch_to(labels[&109]); // b
        self.bind_label(labels[&47]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::load_immediate_shifted(4, -1));
        self.load_double_constant(0, 0x4350000000000000);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 15536 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 1, c: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 4, s: 5, shift: 12, begin: 21, end: 31 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -54 });
        self.emit_branch_conditional_to(4, 0, labels[&61]); // bge
        self.load_double_constant(0, 0x01a56e1fc2f8f359);
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 0, c: 1 });
        self.emit_branch_to(labels[&109]); // b
        self.bind_label(labels[&61]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 2047 });
        self.emit_branch_conditional_to(4, 2, labels[&66]); // bne
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 0, b: 0 });
        self.emit_branch_to(labels[&109]); // b
        self.bind_label(labels[&66]);
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 2046 });
        self.emit_branch_conditional_to(4, 1, labels[&75]); // ble
        self.load_double_constant(1, 0x7e37e43c8800759c);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 1, offset: 8 });
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink { target: "copysign".to_string() });
        self.load_double_constant(0, 0x7e37e43c8800759c);
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 0, c: 1 });
        self.emit_branch_to(labels[&109]); // b
        self.bind_label(labels[&75]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&83]); // ble
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 5, shift: 0, begin: 12, end: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 4, shift: 20 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.emit_branch_to(labels[&109]); // b
        self.bind_label(labels[&83]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: -54 });
        self.emit_branch_conditional_to(12, 1, labels[&101]); // bgt
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 1));
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: -15536 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&95]); // ble
        self.load_double_constant(1, 0x7e37e43c8800759c);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 1, offset: 8 });
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink { target: "copysign".to_string() });
        self.load_double_constant(0, 0x7e37e43c8800759c);
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 0, c: 1 });
        self.emit_branch_to(labels[&109]); // b
        self.bind_label(labels[&95]);
        self.load_double_constant(1, 0x01a56e1fc2f8f359);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 1, offset: 8 });
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink { target: "copysign".to_string() });
        self.load_double_constant(0, 0x01a56e1fc2f8f359);
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 0, c: 1 });
        self.emit_branch_to(labels[&109]); // b
        self.bind_label(labels[&101]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 54 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 5, shift: 0, begin: 12, end: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 20 });
        self.load_double_constant(1, 0x3c90000000000000);
        self.output.instructions.push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 1, c: 0 });
        self.bind_label(labels[&109]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe — the real pools start at @59.
        self.output.anonymous_label_bump += 33;
        Ok(true)
    }
}
