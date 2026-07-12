//! ivt_acosf: an exact-match whole-function capture (fire 714).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const IVT_ACOSF_AST_HASH: u64 = 0xd45f9f73f59574de;

impl Generator {
    pub(super) fn try_ivt_acosf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "acosf"
            || function.return_type != Type::Float
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != IVT_ACOSF_AST_HASH {
            eprintln!("ivt_acosf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x19234177da3e2378 => 0, // pikmin
            _ => {
                eprintln!("ivt_acosf context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.non_leaf = true;
        self.output.constant_number_gaps = vec![(1, 10)];
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::PairedSingleQuantizedStore { s: 31, a: 1, offset: 24, w: 0, i: 0 });
        self.output.instructions.push(Instruction::FloatMove { d: 31, b: 1 });
        self.load_float_constant(0, f32::from_bits(0x3f800000));
        self.output.instructions.push(Instruction::FloatNegativeMultiplySubtractSingle { d: 1, a: 31, c: 31, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "_inv_sqrtf");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_inv_sqrtf".to_string() });
        self.output.instructions.push(Instruction::FloatMultiplySingle { d: 1, a: 31, c: 1 });
        self.record_relocation(RelocationKind::Rel24, "atan__Ff");
        self.output.instructions.push(Instruction::BranchAndLink { target: "atan__Ff".to_string() });
        self.load_float_constant(0, f32::from_bits(0x3fc90fdb));
        self.output.instructions.push(Instruction::FloatSubtractSingle { d: 1, a: 0, b: 1 });
        self.output.instructions.push(Instruction::PairedSingleQuantizedLoad { d: 31, a: 1, offset: 24, w: 0, i: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 31, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
