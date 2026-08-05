//! Linkage-first endian stack-unpack emission.

#[allow(unused_imports)]
use super::super::*;
use super::recognize::EndianStackUnpack;

pub(super) fn emit(generator: &mut Generator, plan: &EndianStackUnpack<'_>) {
    const SELECTED: u8 = 31;
    const OUTPUT: u8 = 30;
    let temporary = generator.fresh_label();
    let selected = generator.fresh_label();
    let epilogue = generator.fresh_label();
    generator.non_leaf = true;
    generator.frame_size = 24;
    generator.callee_saved = vec![SELECTED, OUTPUT];
    generator.output.pre_scheduled = true;
    generator
        .output
        .instructions
        .push(Instruction::MoveFromLinkRegister { d: 0 });
    generator.record_relocation(RelocationKind::Addr16Ha, plan.flag);
    generator
        .output
        .instructions
        .push(Instruction::AddImmediateShifted {
            d: 5,
            a: 0,
            immediate: 0,
        });
    generator.output.instructions.extend([
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        },
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -24,
        },
        Instruction::StoreWord {
            s: SELECTED,
            a: 1,
            offset: 20,
        },
        Instruction::StoreWord {
            s: OUTPUT,
            a: 1,
            offset: 16,
        },
        Instruction::AddImmediate {
            d: OUTPUT,
            a: 4,
            immediate: 0,
        },
    ]);
    generator.record_relocation(RelocationKind::Addr16Lo, plan.flag);
    generator.output.instructions.push(Instruction::LoadWord {
        d: 0,
        a: 5,
        offset: 0,
    });
    generator
        .output
        .instructions
        .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
    generator.emit_branch_conditional_to(12, 2, temporary);
    generator
        .output
        .instructions
        .push(Instruction::move_register(SELECTED, OUTPUT));
    generator.emit_branch_to(selected);
    generator.bind_label(temporary);
    generator.output.instructions.push(Instruction::AddImmediate {
        d: SELECTED,
        a: 1,
        immediate: 8,
    });
    generator.bind_label(selected);
    generator.output.instructions.extend([
        Instruction::AddImmediate {
            d: 4,
            a: SELECTED,
            immediate: 0,
        },
        Instruction::load_immediate(5, i16::from(plan.width)),
    ]);
    generator.record_relocation(RelocationKind::Rel24, plan.callee);
    generator.output.instructions.push(Instruction::BranchAndLink {
        target: plan.callee.to_string(),
    });
    generator.record_relocation(RelocationKind::Addr16Ha, plan.flag);
    generator
        .output
        .instructions
        .push(Instruction::AddImmediateShifted {
            d: 4,
            a: 0,
            immediate: 0,
        });
    generator.record_relocation(RelocationKind::Addr16Lo, plan.flag);
    generator.output.instructions.push(Instruction::LoadWord {
        d: 0,
        a: 4,
        offset: 0,
    });
    generator
        .output
        .instructions
        .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
    generator.emit_branch_conditional_to(4, 2, epilogue);
    generator
        .output
        .instructions
        .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
    generator.emit_branch_conditional_to(4, 2, epilogue);
    for destination in 0..plan.width {
        generator.output.instructions.extend([
            Instruction::LoadByteZero {
                d: 0,
                a: SELECTED,
                offset: i16::from(plan.width - 1 - destination),
            },
            Instruction::StoreByte {
                s: 0,
                a: OUTPUT,
                offset: i16::from(destination),
            },
        ]);
    }
    generator.bind_label(epilogue);
    generator.output.instructions.extend([
        Instruction::LoadWord {
            d: SELECTED,
            a: 1,
            offset: 20,
        },
        Instruction::LoadWord {
            d: OUTPUT,
            a: 1,
            offset: 16,
        },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 24,
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 4,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ]);
}
