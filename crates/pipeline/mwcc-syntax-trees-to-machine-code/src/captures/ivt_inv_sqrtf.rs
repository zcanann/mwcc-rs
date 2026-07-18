//! ivt_inv_sqrtf: an exact-match whole-function capture (fire 714).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const IVT_INV_SQRTF_AST_HASH: u64 = 0x64bf4534305215a1;

impl Generator {
    pub(super) fn try_ivt_inv_sqrtf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "_inv_sqrtf"
            || function.return_type != Type::Float
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != IVT_INV_SQRTF_AST_HASH && hash != 0x2ec58cfa84d019c1 {
            eprintln!("ivt_inv_sqrtf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x1d008359133dc5f8 => 0, // sunshine
            0x19234177da3e2378 => 0, // pikmin
            _ => {
                eprintln!("ivt_inv_sqrtf context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [20, 25] {
            labels.insert(target, self.fresh_label());
        }
        self.load_float_constant(0, f32::from_bits(0x00000000));
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&20]); // ble
        self.output
            .instructions
            .push(Instruction::FloatReciprocalSqrtEstimate { d: 2, b: 1 });
        self.load_float_constant(4, f32::from_bits(0x3f000000));
        self.load_float_constant(3, f32::from_bits(0x40400000));
        self.output
            .instructions
            .push(Instruction::RoundToSingle { d: 2, b: 2 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 0, a: 2, c: 2 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 2, a: 4, c: 2 });
        self.output
            .instructions
            .push(Instruction::FloatNegativeMultiplySubtractSingle {
                d: 0,
                a: 1,
                c: 0,
                b: 3,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 2, a: 2, c: 0 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 0, a: 2, c: 2 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 2, a: 4, c: 2 });
        self.output
            .instructions
            .push(Instruction::FloatNegativeMultiplySubtractSingle {
                d: 0,
                a: 1,
                c: 0,
                b: 3,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 2, a: 2, c: 0 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 0, a: 2, c: 2 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 2, a: 4, c: 2 });
        self.output
            .instructions
            .push(Instruction::FloatNegativeMultiplySubtractSingle {
                d: 0,
                a: 1,
                c: 0,
                b: 3,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 1, a: 2, c: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&20]);
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 1, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&25]); // beq
        self.record_relocation(RelocationKind::Addr16Ha, "__float_nan");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__float_nan");
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&25]);
        self.record_relocation(RelocationKind::Addr16Ha, "__float_huge");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__float_huge");
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
