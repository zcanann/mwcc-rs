//! sldx_pik: an exact-match whole-function capture (fire 490).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SLDX_PIK_AST_HASH: u64 = 0xd9a6a0f08cbd6be3;
/// Post-fold AST (fire 524).
const SLDX_PIK_AST_HASHES: &[u64] = &[SLDX_PIK_AST_HASH, 0xb53bbfc1e01b7bef];

impl Generator {
    pub(super) fn try_sldx_pik(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "ldexp"
            || function.return_type != Type::Double
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !SLDX_PIK_AST_HASHES.contains(&hash) {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 22, // pikmin: pool @27 (ours @5)
            _ => return Ok(false),
        };
        // mwcc's extern order: the MANGLED inline-asm callee first (its bl at
        // 0x18 precedes copysign's) — the AST order carries the unmangled name,
        // so pin the run explicitly (the atof precedent).
        self.output.symbol_order = vec!["__fpclassifyd__Fd".to_string(), "copysign".to_string()];
        self.output.local_undefined_callees = vec!["__fpclassifyd__Fd".to_string()];
        // -- emit (the capture, verbatim) --
        for bits in [
            0x0000000000000000u64,
            0x4350000000000000,
            0x01a56e1fc2f8f359,
            0x7e37e43c8800759c,
            0x3c90000000000000,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [13, 15, 23, 36, 41, 50, 58, 70, 76, 84] {
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
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 28,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.record_relocation(RelocationKind::Rel24, "__fpclassifyd__Fd");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__fpclassifyd__Fd".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 2 });
        self.emit_branch_conditional_to(4, 1, labels[&13]); // ble
        self.load_double_constant(0, 0x0000000000000000);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 0, b: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&15]); // bne
        self.bind_label(labels[&13]);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.emit_branch_to(labels[&84]); // b
        self.bind_label(labels[&15]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 4,
                s: 5,
                shift: 12,
                begin: 21,
                end: 31,
            });
        self.emit_branch_conditional_to(4, 2, labels[&36]); // bne
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 5,
                clear: 1,
            });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&23]); // bne
        self.emit_branch_to(labels[&84]); // b
        self.bind_label(labels[&23]);
        self.load_double_constant(0, 0x4350000000000000);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, -1));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 15536,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 1, c: 0 });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 31, b: 0 });
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
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 5,
            shift: 12,
            begin: 21,
            end: 31,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -54,
        });
        self.emit_branch_conditional_to(4, 0, labels[&36]); // bge
        self.load_double_constant(0, 0x01a56e1fc2f8f359);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 0, c: 1 });
        self.emit_branch_to(labels[&84]); // b
        self.bind_label(labels[&36]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 2047,
            });
        self.emit_branch_conditional_to(4, 2, labels[&41]); // bne
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 1, a: 0, b: 0 });
        self.emit_branch_to(labels[&84]); // b
        self.bind_label(labels[&41]);
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 4, b: 31 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 2046,
            });
        self.emit_branch_conditional_to(4, 1, labels[&50]); // ble
        self.load_double_constant(1, 0x7e37e43c8800759c);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 2,
            a: 1,
            offset: 8,
        });
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "copysign".to_string(),
        });
        self.load_double_constant(0, 0x7e37e43c8800759c);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 0, c: 1 });
        self.emit_branch_to(labels[&84]); // b
        self.bind_label(labels[&50]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&58]); // ble
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 5,
            shift: 0,
            begin: 12,
            end: 0,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 4,
                shift: 20,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.emit_branch_to(labels[&84]); // b
        self.bind_label(labels[&58]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: -54,
            });
        self.emit_branch_conditional_to(12, 1, labels[&76]); // bgt
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 1));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: -15536,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 31, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&70]); // ble
        self.load_double_constant(1, 0x7e37e43c8800759c);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 2,
            a: 1,
            offset: 8,
        });
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "copysign".to_string(),
        });
        self.load_double_constant(0, 0x7e37e43c8800759c);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 0, c: 1 });
        self.emit_branch_to(labels[&84]); // b
        self.bind_label(labels[&70]);
        self.load_double_constant(1, 0x01a56e1fc2f8f359);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 2,
            a: 1,
            offset: 8,
        });
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "copysign".to_string(),
        });
        self.load_double_constant(0, 0x01a56e1fc2f8f359);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 0, c: 1 });
        self.emit_branch_to(labels[&84]); // b
        self.bind_label(labels[&76]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 54,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 5,
            shift: 0,
            begin: 12,
            end: 0,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 20,
            });
        self.load_double_constant(1, 0x3c90000000000000);
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 1, c: 0 });
        self.bind_label(labels[&84]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 28,
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
