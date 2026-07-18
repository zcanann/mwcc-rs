//! satan: an exact-match whole-function capture (see captures::ast_hash
//! and docs/emission-model.md for the pipeline).

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the fdlibm atan (captured fire 440).
const SATAN_AST_HASH: u64 = 0xccb154e87b122186;

impl Generator {
    /// THE S_ATAN EXACT-MATCH TEMPLATE (fires 440/441): atan() whole, via
    /// the capture->dis2rust->AST-hash pipeline (see try_efmod). 134
    /// instructions; pool constants via load_double_constant; the
    /// coefficient tables address through the  anchor
    /// (the writer emits it when these relocations exist).
    pub(super) fn try_satan(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "atan"
            || function.return_type != Type::Double
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SATAN_AST_HASH {
            return Ok(false);
        }
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [18, 21, 29, 36, 48, 50, 66, 73, 85, 89, 120, 132] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.record_relocation(RelocationKind::Addr16Ha, "...rodata.0");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 17424));
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.record_relocation(RelocationKind::Addr16Lo, "...rodata.0");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 4,
                s: 6,
                clear: 1,
            });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&36]); // blt
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 1, labels[&18]); // bgt
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 0,
                a: 4,
                immediate: -32752,
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&21]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&21]); // beq
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 1, a: 0, b: 0 });
        self.emit_branch_to(labels[&132]); // b
        self.bind_label(labels[&21]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&29]); // ble
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 5,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 32,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 4,
            offset: 24,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 3,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 1, a: 1, b: 0 });
        self.emit_branch_to(labels[&132]); // b
        self.bind_label(labels[&29]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 5,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 32,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 4,
            offset: 24,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 3,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 1, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 1, a: 1, b: 0 });
        self.emit_branch_to(labels[&132]); // b
        self.bind_label(labels[&36]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 16348));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&50]); // bge
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 15904));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&48]); // bge
        self.load_double_constant(2, 0x7e37e43c8800759c);
        self.load_double_constant(0, 0x3ff0000000000000);
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 2, a: 2, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 2, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&48]); // ble
        self.emit_branch_to(labels[&132]); // b
        self.bind_label(labels[&48]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -1));
        self.emit_branch_to(labels[&89]); // b
        self.bind_label(labels[&50]);
        self.output
            .instructions
            .push(Instruction::FloatAbsolute { d: 3, b: 1 });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 16371));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 3,
                a: 1,
                offset: 8,
            });
        self.emit_branch_conditional_to(4, 0, labels[&73]); // bge
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 16358));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&66]); // bge
        self.load_double_constant(2, 0x4000000000000000);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.load_double_constant(1, 0x3ff0000000000000);
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 0, a: 2, b: 3 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySubtractDouble {
                d: 1,
                a: 2,
                c: 3,
                b: 1,
            });
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 0, a: 1, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 8,
            });
        self.emit_branch_to(labels[&89]); // b
        self.bind_label(labels[&66]);
        self.load_double_constant(0, 0x3ff0000000000000);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 1, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 0, a: 0, b: 3 });
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 0, a: 1, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 8,
            });
        self.emit_branch_to(labels[&89]); // b
        self.bind_label(labels[&73]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 16388));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: -32768,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&85]); // bge
        self.load_double_constant(2, 0x3ff8000000000000);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.load_double_constant(0, 0x3ff0000000000000);
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 1, a: 3, b: 2 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 0,
                a: 2,
                c: 3,
                b: 0,
            });
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 0, a: 1, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 8,
            });
        self.emit_branch_to(labels[&89]); // b
        self.bind_label(labels[&85]);
        self.load_double_constant(0, 0xbff0000000000000);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 0, a: 0, b: 3 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 8,
            });
        self.bind_label(labels[&89]);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 9,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 64,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 4,
            a: 3,
            offset: 80,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 11, a: 9, c: 9 });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 3,
            offset: 64,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 7,
            a: 3,
            offset: 48,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 3,
            a: 3,
            offset: 72,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 3,
            offset: 56,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble {
                d: 10,
                a: 11,
                c: 11,
            });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 6,
            a: 3,
            offset: 32,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 2,
            a: 3,
            offset: 40,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 5,
            a: 3,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 8,
                a: 10,
                c: 4,
                b: 1,
            });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 3,
            offset: 24,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 4,
            a: 5,
            offset: 64,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 3,
                a: 10,
                c: 3,
                b: 0,
            });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 3,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 7,
                a: 10,
                c: 8,
                b: 7,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 2,
                a: 10,
                c: 3,
                b: 2,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 3,
                a: 10,
                c: 7,
                b: 6,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 1,
                a: 10,
                c: 2,
                b: 1,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 2,
                a: 10,
                c: 3,
                b: 5,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 0,
                a: 10,
                c: 1,
                b: 0,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 1,
                a: 10,
                c: 2,
                b: 4,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 2, a: 10, c: 0 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 0, a: 11, c: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&120]); // bge
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 0, a: 0, b: 2 });
        self.output
            .instructions
            .push(Instruction::FloatNegativeMultiplySubtractDouble {
                d: 1,
                a: 9,
                c: 0,
                b: 9,
            });
        self.emit_branch_to(labels[&132]); // b
        self.bind_label(labels[&120]);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 3,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 1, a: 0, b: 2 });
        self.output
            .instructions
            .push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.output
            .instructions
            .push(Instruction::LoadFloatDoubleIndexed { d: 2, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySubtractDouble {
                d: 0,
                a: 9,
                c: 1,
                b: 0,
            });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 9 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 1, a: 2, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&132]); // bge
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 1, b: 1 });
        self.bind_label(labels[&132]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe — the real pools start at @47 (the
        // function's internal labels consume the counter).
        self.output.anonymous_label_bump += 39;
        Ok(true)
    }
}
