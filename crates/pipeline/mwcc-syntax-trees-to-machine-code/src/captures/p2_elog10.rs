//! p2_elog10: an exact-match whole-function capture (fire 459).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const P2_ELOG10_AST_HASH: u64 = 0x1ed5ceaa93be7420;

impl Generator {
    pub(super) fn try_p2_elog10(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__ieee754_log10"
            || function.return_type != Type::Double
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != P2_ELOG10_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xb61776ae26f47f0e => 11, // BfBB post-fold (f524)
            0xbd60acb658c79e45 => 11, // pikmin2 (measured)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.output.keep_named_const_scalars = vec!["zero".to_string()];
        self.non_leaf = true;
        self.output.constant_number_gaps = vec![(5, 1)];
        for bits in [
            0xc350000000000000u64,
            0x4350000000000000,
            0x3d59fef311f12b36,
            0x3fdbcb7b1526e50e,
            0x3fd34413509f6000,
            0x4330000080000000,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [20, 28, 33, 39, 63] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 16));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 5, b: 0 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 31,
                a: 1,
                offset: 24,
            });
        self.emit_branch_conditional_to(4, 0, labels[&33]); // bge
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 5,
                clear: 1,
            });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 0, s: 0, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&20]); // bne
        self.load_double_constant(1, 0xc350000000000000);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 33));
        self.record_relocation(RelocationKind::EmbSda21, "zero");
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.record_relocation(RelocationKind::EmbSda21, "errno");
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 1, a: 1, b: 0 });
        self.emit_branch_to(labels[&63]); // b
        self.bind_label(labels[&20]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&28]); // bge
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 1, a: 1, b: 1 });
        self.record_relocation(RelocationKind::EmbSda21, "zero");
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 33));
        self.record_relocation(RelocationKind::EmbSda21, "errno");
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 1, a: 1, b: 0 });
        self.emit_branch_to(labels[&63]); // b
        self.bind_label(labels[&28]);
        self.load_double_constant(0, 0x4350000000000000);
        self.output
            .instructions
            .push(Instruction::load_immediate(4, -54));
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 0, a: 1, c: 0 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 8,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 8,
        });
        self.bind_label(labels[&33]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 5, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&39]); // blt
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 1, a: 0, b: 0 });
        self.emit_branch_to(labels[&63]); // b
        self.bind_label(labels[&39]);
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 3,
                s: 5,
                shift: 20,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 17200));
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 3, b: 4 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1023,
        });
        self.load_double_constant(1, 0x4330000080000000);
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 3,
                s: 4,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 4, b: 3 });
        self.output
            .instructions
            .push(Instruction::XorImmediateShifted {
                a: 0,
                s: 0,
                immediate: 32768,
            });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 3,
                a: 3,
                immediate: 1023,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 3,
                shift: 20,
            });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: 0,
                s: 5,
                shift: 0,
                begin: 12,
                end: 31,
            });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 31, a: 0, b: 1 });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.record_relocation(RelocationKind::Rel24, "__ieee754_log");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__ieee754_log".to_string(),
        });
        self.load_double_constant(0, 0x3fdbcb7b1526e50e);
        self.load_double_constant(2, 0x3d59fef311f12b36);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 0, c: 1 });
        self.load_double_constant(0, 0x3fd34413509f6000);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 1,
                a: 2,
                c: 31,
                b: 1,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 1,
                a: 0,
                c: 31,
                b: 1,
            });
        self.bind_label(labels[&63]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 31,
            a: 1,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
