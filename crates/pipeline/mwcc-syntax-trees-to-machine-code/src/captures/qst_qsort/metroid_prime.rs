//! Metroid Prime's GC/1.3 `qsort` schedule.
//!
//! This source uses inline `stmw`/`lmw` saves and the older multiply-by-two
//! heap-index sequence. Keeping it separate from the Mario Party 4 capture
//! makes the measured version policy explicit.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;

pub(super) fn emit(generator: &mut Generator) -> Compilation<bool> {
    generator.frame_size = 64;
    generator.non_leaf = true;
    let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
        std::collections::HashMap::new();
    for target in [18, 23, 27, 34, 40, 45, 63, 74, 81, 83, 87] {
        labels.insert(target, generator.fresh_label());
    }

    generator
        .output
        .instructions
        .push(Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -64,
        });
    generator
        .output
        .instructions
        .push(Instruction::MoveFromLinkRegister { d: 0 });
    generator
        .output
        .instructions
        .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 2 });
    generator.output.instructions.push(Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 68,
    });
    generator
        .output
        .instructions
        .push(Instruction::StoreMultipleWord {
            s: 21,
            a: 1,
            offset: 20,
        });
    generator
        .output
        .instructions
        .push(Instruction::move_register(29, 3));
    generator
        .output
        .instructions
        .push(Instruction::move_register(30, 5));
    generator
        .output
        .instructions
        .push(Instruction::move_register(31, 6));
    generator.emit_branch_conditional_to(12, 0, labels[&87]);
    generator
        .output
        .instructions
        .push(Instruction::ShiftRightLogicalImmediate {
            a: 3,
            s: 4,
            shift: 1,
        });
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: -1,
        });
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 28,
            a: 3,
            immediate: 1,
        });
    generator
        .output
        .instructions
        .push(Instruction::move_register(27, 4));
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 3,
            a: 28,
            immediate: -1,
        });
    generator
        .output
        .instructions
        .push(Instruction::MultiplyLow { d: 3, a: 30, b: 3 });
    generator
        .output
        .instructions
        .push(Instruction::MultiplyLow { d: 0, a: 30, b: 0 });
    generator
        .output
        .instructions
        .push(Instruction::Add { d: 25, a: 29, b: 3 });
    generator
        .output
        .instructions
        .push(Instruction::Add { d: 24, a: 29, b: 0 });

    generator.bind_label(labels[&18]);
    generator
        .output
        .instructions
        .push(Instruction::CompareLogicalWordImmediate {
            a: 28,
            immediate: 1,
        });
    generator.emit_branch_conditional_to(4, 1, labels[&23]);
    generator
        .output
        .instructions
        .push(Instruction::SubtractFrom {
            d: 25,
            a: 30,
            b: 25,
        });
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 28,
            a: 28,
            immediate: -1,
        });
    generator.emit_branch_to(labels[&40]);

    generator.bind_label(labels[&23]);
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 3,
            a: 24,
            immediate: -1,
        });
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 4,
            a: 25,
            immediate: -1,
        });
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 5,
            a: 30,
            immediate: 1,
        });
    generator.emit_branch_to(labels[&34]);

    generator.bind_label(labels[&27]);
    generator
        .output
        .instructions
        .push(Instruction::LoadByteZero {
            d: 6,
            a: 4,
            offset: 1,
        });
    generator
        .output
        .instructions
        .push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 1,
        });
    generator
        .output
        .instructions
        .push(Instruction::ExtendSignByte { a: 6, s: 6 });
    generator.output.instructions.push(Instruction::StoreByte {
        s: 0,
        a: 4,
        offset: 1,
    });
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
    generator.output.instructions.push(Instruction::StoreByte {
        s: 6,
        a: 3,
        offset: 1,
    });
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });

    generator.bind_label(labels[&34]);
    generator
        .output
        .instructions
        .push(Instruction::AddImmediateCarryingRecord {
            d: 5,
            a: 5,
            immediate: -1,
        });
    generator.emit_branch_conditional_to(4, 2, labels[&27]);
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 27,
            a: 27,
            immediate: -1,
        });
    generator
        .output
        .instructions
        .push(Instruction::CompareLogicalWordImmediate {
            a: 27,
            immediate: 1,
        });
    generator.emit_branch_conditional_to(12, 2, labels[&87]);
    generator
        .output
        .instructions
        .push(Instruction::SubtractFrom {
            d: 24,
            a: 30,
            b: 24,
        });

    generator.bind_label(labels[&40]);
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 0,
            a: 28,
            immediate: -1,
        });
    generator
        .output
        .instructions
        .push(Instruction::move_register(26, 28));
    generator
        .output
        .instructions
        .push(Instruction::MultiplyLow { d: 0, a: 30, b: 0 });
    generator
        .output
        .instructions
        .push(Instruction::Add { d: 22, a: 29, b: 0 });
    generator.emit_branch_to(labels[&83]);

    generator.bind_label(labels[&45]);
    generator
        .output
        .instructions
        .push(Instruction::load_immediate(0, 2));
    generator
        .output
        .instructions
        .push(Instruction::move_register(23, 22));
    generator
        .output
        .instructions
        .push(Instruction::MultiplyLow { d: 26, a: 26, b: 0 });
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 0,
            a: 26,
            immediate: -1,
        });
    generator
        .output
        .instructions
        .push(Instruction::MultiplyLow { d: 0, a: 30, b: 0 });
    generator
        .output
        .instructions
        .push(Instruction::CompareLogicalWord { a: 26, b: 27 });
    generator
        .output
        .instructions
        .push(Instruction::Add { d: 22, a: 29, b: 0 });
    generator.emit_branch_conditional_to(4, 0, labels[&63]);
    generator.output.instructions.push(Instruction::Add {
        d: 21,
        a: 22,
        b: 30,
    });
    generator
        .output
        .instructions
        .push(Instruction::move_register(12, 31));
    generator
        .output
        .instructions
        .push(Instruction::move_register(3, 22));
    generator
        .output
        .instructions
        .push(Instruction::move_register(4, 21));
    generator
        .output
        .instructions
        .push(Instruction::MoveToCountRegister { s: 12 });
    generator
        .output
        .instructions
        .push(Instruction::BranchToCountRegisterAndLink);
    generator
        .output
        .instructions
        .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
    generator.emit_branch_conditional_to(4, 0, labels[&63]);
    generator
        .output
        .instructions
        .push(Instruction::move_register(22, 21));
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 26,
            a: 26,
            immediate: 1,
        });

    generator.bind_label(labels[&63]);
    generator
        .output
        .instructions
        .push(Instruction::move_register(12, 31));
    generator
        .output
        .instructions
        .push(Instruction::move_register(3, 23));
    generator
        .output
        .instructions
        .push(Instruction::move_register(4, 22));
    generator
        .output
        .instructions
        .push(Instruction::MoveToCountRegister { s: 12 });
    generator
        .output
        .instructions
        .push(Instruction::BranchToCountRegisterAndLink);
    generator
        .output
        .instructions
        .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
    generator.emit_branch_conditional_to(4, 0, labels[&18]);
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 3,
            a: 22,
            immediate: -1,
        });
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 4,
            a: 23,
            immediate: -1,
        });
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 5,
            a: 30,
            immediate: 1,
        });
    generator.emit_branch_to(labels[&81]);

    generator.bind_label(labels[&74]);
    generator
        .output
        .instructions
        .push(Instruction::LoadByteZero {
            d: 6,
            a: 4,
            offset: 1,
        });
    generator
        .output
        .instructions
        .push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 1,
        });
    generator
        .output
        .instructions
        .push(Instruction::ExtendSignByte { a: 6, s: 6 });
    generator.output.instructions.push(Instruction::StoreByte {
        s: 0,
        a: 4,
        offset: 1,
    });
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
    generator.output.instructions.push(Instruction::StoreByte {
        s: 6,
        a: 3,
        offset: 1,
    });
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });

    generator.bind_label(labels[&81]);
    generator
        .output
        .instructions
        .push(Instruction::AddImmediateCarryingRecord {
            d: 5,
            a: 5,
            immediate: -1,
        });
    generator.emit_branch_conditional_to(4, 2, labels[&74]);

    generator.bind_label(labels[&83]);
    generator
        .output
        .instructions
        .push(Instruction::ShiftLeftImmediate {
            a: 0,
            s: 26,
            shift: 1,
        });
    generator
        .output
        .instructions
        .push(Instruction::CompareLogicalWord { a: 0, b: 27 });
    generator.emit_branch_conditional_to(4, 1, labels[&45]);
    generator.emit_branch_to(labels[&18]);

    generator.bind_label(labels[&87]);
    generator
        .output
        .instructions
        .push(Instruction::LoadMultipleWord {
            d: 21,
            a: 1,
            offset: 20,
        });
    generator.output.instructions.push(Instruction::LoadWord {
        d: 0,
        a: 1,
        offset: 68,
    });
    generator
        .output
        .instructions
        .push(Instruction::MoveToLinkRegister { s: 0 });
    generator
        .output
        .instructions
        .push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 64,
        });
    generator
        .output
        .instructions
        .push(Instruction::BranchToLinkRegister);
    Ok(true)
}
