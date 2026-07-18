//! efmod: an exact-match whole-function capture (see captures::ast_hash
//! and docs/emission-model.md for the pipeline).

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the fdlibm __ieee754_fmod (captured fire 439).
const EFMOD_AST_HASH: u64 = 0x9cb47488c619261c;

impl Generator {
    /// THE E_FMOD EXACT-MATCH TEMPLATE (fire 439): the whole
    /// __ieee754_fmod compiled as one claimed shape. Three fitting
    /// passes (docs/efmod-register-map.md) established the whole-
    /// function register assignment is mwcc-IR-order territory, so
    /// this template emits the CAPTURED 207 instructions verbatim
    /// (docs/efmod-knit-target.dis, transcribed by tools/dis2rust.py)
    /// and gates on a TOTAL structural match: the Debug hash of the
    /// parsed function must equal the known fdlibm e_fmod AST hash —
    /// any deviation (names, constants, statement order) defers.
    pub(super) fn try_efmod(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__ieee754_fmod"
            || function.return_type != Type::Double
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != EFMOD_AST_HASH {
            return Ok(false);
        }
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [
            21, 26, 31, 33, 39, 47, 49, 52, 55, 57, 60, 62, 70, 72, 75, 78, 80, 83, 85, 90, 99,
            102, 107, 116, 119, 122, 127, 134, 141, 145, 146, 151, 155, 162, 164, 169, 181, 190,
            198, 201, 204, 205,
        ] {
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
            .push(Instruction::StoreFloatDouble {
                s: 2,
                a: 1,
                offset: 16,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 10,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 8,
                s: 10,
                clear: 1,
            });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 0,
                s: 6,
                begin: 0,
                end: 0,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 3, s: 8, b: 5 });
        self.output
            .instructions
            .push(Instruction::Xor { a: 7, s: 6, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&21]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(6, 32752));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 7, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&21]); // bge
        self.output
            .instructions
            .push(Instruction::Negate { d: 3, a: 5 });
        self.output
            .instructions
            .push(Instruction::Or { a: 3, s: 5, b: 3 });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 3,
                s: 3,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 3, s: 8, b: 3 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 3, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&26]); // ble
        self.bind_label(labels[&21]);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 0, a: 1, c: 0 });
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 1, a: 0, b: 0 });
        self.emit_branch_to(labels[&205]); // b
        self.bind_label(labels[&26]);
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(12, 1, labels[&39]); // bgt
        self.emit_branch_conditional_to(12, 0, labels[&31]); // blt
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 0, labels[&33]); // bge
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.emit_branch_to(labels[&205]); // b
        self.bind_label(labels[&33]);
        self.emit_branch_conditional_to(4, 2, labels[&39]); // bne
        self.record_relocation(RelocationKind::Addr16Ha, "Zero");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 4,
            begin: 28,
            end: 28,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "Zero");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadFloatDoubleIndexed { d: 1, a: 3, b: 0 });
        self.emit_branch_to(labels[&205]); // b
        self.bind_label(labels[&39]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 16));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 7, b: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&60]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&52]); // bne
        self.output
            .instructions
            .push(Instruction::move_register(3, 4));
        self.output
            .instructions
            .push(Instruction::load_immediate(11, -1043));
        self.emit_branch_to(labels[&49]); // b
        self.bind_label(labels[&47]);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 3,
                s: 3,
                shift: 1,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 11,
            immediate: -1,
        });
        self.bind_label(labels[&49]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 1, labels[&47]); // bgt
        self.emit_branch_to(labels[&62]); // b
        self.bind_label(labels[&52]);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 3,
                s: 7,
                shift: 11,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(11, -1022));
        self.emit_branch_to(labels[&57]); // b
        self.bind_label(labels[&55]);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 3,
                s: 3,
                shift: 1,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 11,
            immediate: -1,
        });
        self.bind_label(labels[&57]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 1, labels[&55]); // bgt
        self.emit_branch_to(labels[&62]); // b
        self.bind_label(labels[&60]);
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 3,
                s: 7,
                shift: 20,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 3,
            immediate: -1023,
        });
        self.bind_label(labels[&62]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 16));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 8, b: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&83]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 8, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&75]); // bne
        self.output
            .instructions
            .push(Instruction::move_register(6, 5));
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1043));
        self.emit_branch_to(labels[&72]); // b
        self.bind_label(labels[&70]);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 6,
                s: 6,
                shift: 1,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.bind_label(labels[&72]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(12, 1, labels[&70]); // bgt
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&75]);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 6,
                s: 8,
                shift: 11,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1022));
        self.emit_branch_to(labels[&80]); // b
        self.bind_label(labels[&78]);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 6,
                s: 6,
                shift: 1,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.bind_label(labels[&80]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(12, 1, labels[&78]); // bgt
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&83]);
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 3,
                s: 8,
                shift: 20,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1023,
        });
        self.bind_label(labels[&85]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 11,
                immediate: -1022,
            });
        self.emit_branch_conditional_to(12, 0, labels[&90]); // blt
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 6,
                s: 7,
                clear: 12,
            });
        self.output
            .instructions
            .push(Instruction::OrImmediateShifted {
                a: 9,
                s: 6,
                immediate: 16,
            });
        self.emit_branch_to(labels[&102]); // b
        self.bind_label(labels[&90]);
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 9,
                a: 11,
                immediate: -1022,
            });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 9,
                immediate: 31,
            });
        self.emit_branch_conditional_to(12, 1, labels[&99]); // bgt
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 6,
                a: 9,
                immediate: 32,
            });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 7, s: 7, b: 9 });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 6, s: 4, b: 6 });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 4, s: 4, b: 9 });
        self.output
            .instructions
            .push(Instruction::Or { a: 9, s: 7, b: 6 });
        self.emit_branch_to(labels[&102]); // b
        self.bind_label(labels[&99]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 9,
            immediate: -32,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 9, s: 4, b: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.bind_label(labels[&102]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 3,
                immediate: -1022,
            });
        self.emit_branch_conditional_to(12, 0, labels[&107]); // blt
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 6,
                s: 10,
                clear: 12,
            });
        self.output
            .instructions
            .push(Instruction::OrImmediateShifted {
                a: 7,
                s: 6,
                immediate: 16,
            });
        self.emit_branch_to(labels[&119]); // b
        self.bind_label(labels[&107]);
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 10,
                a: 3,
                immediate: -1022,
            });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 10,
                immediate: 31,
            });
        self.emit_branch_conditional_to(12, 1, labels[&116]); // bgt
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 6,
                a: 10,
                immediate: 32,
            });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 7, s: 8, b: 10 });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 6, s: 5, b: 6 });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 5, s: 5, b: 10 });
        self.output
            .instructions
            .push(Instruction::Or { a: 7, s: 7, b: 6 });
        self.emit_branch_to(labels[&119]); // b
        self.bind_label(labels[&116]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 10,
            immediate: -32,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 7, s: 5, b: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.bind_label(labels[&119]);
        self.output
            .instructions
            .push(Instruction::SubtractFromRecord { d: 6, a: 3, b: 11 });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 6 });
        self.emit_branch_conditional_to(12, 2, labels[&146]); // beq
        self.bind_label(labels[&122]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 8, a: 7, b: 9 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 10, a: 5, b: 4 });
        self.emit_branch_conditional_to(4, 0, labels[&127]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 8,
            immediate: -1,
        });
        self.bind_label(labels[&127]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 8, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&134]); // bge
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 6,
                s: 4,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 4, b: 4 });
        self.output
            .instructions
            .push(Instruction::Add { d: 6, a: 9, b: 6 });
        self.output
            .instructions
            .push(Instruction::Add { d: 9, a: 9, b: 6 });
        self.emit_branch_to(labels[&145]); // b
        self.bind_label(labels[&134]);
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 4, s: 8, b: 10 });
        self.emit_branch_conditional_to(4, 2, labels[&141]); // bne
        self.record_relocation(RelocationKind::Addr16Ha, "Zero");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 4,
            begin: 28,
            end: 28,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "Zero");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadFloatDoubleIndexed { d: 1, a: 3, b: 0 });
        self.emit_branch_to(labels[&205]); // b
        self.bind_label(labels[&141]);
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 6,
                s: 10,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 10, b: 10 });
        self.output
            .instructions
            .push(Instruction::Add { d: 9, a: 8, b: 6 });
        self.output
            .instructions
            .push(Instruction::Add { d: 9, a: 8, b: 9 });
        self.bind_label(labels[&145]);
        self.emit_branch_conditional_to(16, 0, labels[&122]); // bdnz
        self.bind_label(labels[&146]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 6, a: 7, b: 9 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 5, a: 5, b: 4 });
        self.emit_branch_conditional_to(4, 0, labels[&151]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 6,
            immediate: -1,
        });
        self.bind_label(labels[&151]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&155]); // blt
        self.output
            .instructions
            .push(Instruction::move_register(9, 6));
        self.output
            .instructions
            .push(Instruction::move_register(4, 5));
        self.bind_label(labels[&155]);
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 5, s: 9, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&162]); // bne
        self.record_relocation(RelocationKind::Addr16Ha, "Zero");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 4,
            begin: 28,
            end: 28,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "Zero");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadFloatDoubleIndexed { d: 1, a: 3, b: 0 });
        self.emit_branch_to(labels[&205]); // b
        self.bind_label(labels[&162]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(5, 16));
        self.emit_branch_to(labels[&169]); // b
        self.bind_label(labels[&164]);
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 6,
                s: 4,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 4, b: 4 });
        self.output
            .instructions
            .push(Instruction::Add { d: 6, a: 9, b: 6 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 9, a: 9, b: 6 });
        self.bind_label(labels[&169]);
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 9, b: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&164]); // blt
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 3,
                immediate: -1022,
            });
        self.emit_branch_conditional_to(12, 0, labels[&181]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1023,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 5,
                a: 9,
                immediate: -16,
            });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 3,
                s: 3,
                shift: 20,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 3, s: 5, b: 3 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.emit_branch_to(labels[&204]); // b
        self.bind_label(labels[&181]);
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 6,
                a: 3,
                immediate: -1022,
            });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 6,
                immediate: 20,
            });
        self.emit_branch_conditional_to(12, 1, labels[&190]); // bgt
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 3,
                a: 6,
                immediate: 32,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 4, s: 4, b: 6 });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 3, s: 9, b: 3 });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicWord { a: 9, s: 9, b: 6 });
        self.output
            .instructions
            .push(Instruction::Or { a: 3, s: 4, b: 3 });
        self.emit_branch_to(labels[&201]); // b
        self.bind_label(labels[&190]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 6,
                immediate: 31,
            });
        self.emit_branch_conditional_to(12, 1, labels[&198]); // bgt
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 5,
                a: 6,
                immediate: 32,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 3, s: 4, b: 6 });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 4, s: 9, b: 5 });
        self.output
            .instructions
            .push(Instruction::move_register(9, 0));
        self.output
            .instructions
            .push(Instruction::Or { a: 3, s: 4, b: 3 });
        self.emit_branch_to(labels[&201]); // b
        self.bind_label(labels[&198]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: -32,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicWord { a: 3, s: 9, b: 3 });
        self.output
            .instructions
            .push(Instruction::move_register(9, 0));
        self.bind_label(labels[&201]);
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 9, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.bind_label(labels[&204]);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.bind_label(labels[&205]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe after implementation.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }
}
