//! GC/1.1p1 linkage-first stack-pack emission.

#[allow(unused_imports)]
use super::super::*;
use super::recognize::EndianStackPack;

pub(super) fn emit(generator: &mut Generator, plan: &EndianStackPack<'_>) {
    let frame_size = if plan.width == 8 { 24 } else { 16 };
    let global_base = if plan.width == 8 { 4 } else { 5 };
    let swap_offset = if plan.width == 8 { 16 } else { 12 };
    let swapped = generator.fresh_label();
    let selected = generator.fresh_label();
    generator.non_leaf = true;
    generator.frame_size = frame_size;
    generator.output.pre_scheduled = true;
    generator.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
    match generator.behavior.global_addressing {
        GlobalAddressing::Absolute => {
            generator.record_relocation(RelocationKind::Addr16Ha, plan.flag);
            generator.output.instructions.push(Instruction::AddImmediateShifted {
                d: global_base,
                a: 0,
                immediate: 0,
            });
            generator.output.instructions.extend([
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -frame_size },
            ]);
            emit_spill(generator, plan.width);
            generator.record_relocation(RelocationKind::Addr16Lo, plan.flag);
            generator.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: global_base,
                offset: 0,
            });
            generator.output.instructions.push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 0,
            });
        }
        GlobalAddressing::SmallData => {
            generator.output.instructions.extend([
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -frame_size },
            ]);
            generator.record_relocation(RelocationKind::EmbSda21, plan.flag);
            generator.output.instructions.push(Instruction::LoadWord { d: 0, a: 0, offset: 0 });
            match plan.width {
                2 => generator.output.instructions.extend([
                    Instruction::StoreHalfword { s: 4, a: 1, offset: 8 },
                    Instruction::CompareWordImmediate { a: 0, immediate: 0 },
                ]),
                4 => generator.output.instructions.extend([
                    Instruction::StoreWord { s: 4, a: 1, offset: 8 },
                    Instruction::CompareWordImmediate { a: 0, immediate: 0 },
                ]),
                8 => generator.output.instructions.extend([
                    Instruction::StoreWord { s: 5, a: 1, offset: 8 },
                    Instruction::CompareWordImmediate { a: 0, immediate: 0 },
                    Instruction::StoreWord { s: 6, a: 1, offset: 12 },
                ]),
                _ => unreachable!(),
            }
        }
    }
    generator.emit_branch_conditional_to(12, 2, swapped);
    generator.output.instructions.push(Instruction::AddImmediate {
        d: 4,
        a: 1,
        immediate: 8,
    });
    generator.emit_branch_to(selected);
    generator.bind_label(swapped);
    for destination in 0..plan.width {
        generator.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 8 + i16::from(plan.width - 1 - destination),
        });
        if destination == 0 {
            generator.output.instructions.push(Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: swap_offset,
            });
        }
        generator.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: swap_offset + i16::from(destination),
        });
    }
    generator.bind_label(selected);
    generator.output.instructions.push(Instruction::load_immediate(5, i16::from(plan.width)));
    generator.record_relocation(RelocationKind::Rel24, plan.callee);
    generator.output.instructions.push(Instruction::BranchAndLink {
        target: plan.callee.to_owned(),
    });
    generator.output.instructions.extend([
        Instruction::AddImmediate { d: 1, a: 1, immediate: frame_size },
        Instruction::LoadWord { d: 0, a: 1, offset: 4 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ]);
}

fn emit_spill(generator: &mut Generator, width: u8) {
    match width {
        2 => generator.output.instructions.push(Instruction::StoreHalfword {
            s: 4,
            a: 1,
            offset: 8,
        }),
        4 => generator.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 8,
        }),
        8 => generator.output.instructions.extend([
            Instruction::StoreWord { s: 5, a: 1, offset: 8 },
            Instruction::StoreWord { s: 6, a: 1, offset: 12 },
        ]),
        _ => unreachable!(),
    }
}
