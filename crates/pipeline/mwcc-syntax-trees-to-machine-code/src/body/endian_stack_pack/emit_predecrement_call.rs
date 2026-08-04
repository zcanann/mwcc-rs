//! Predecrement stack-pack emission with an out-of-line append call.

#[allow(unused_imports)]
use super::super::*;
use super::recognize::EndianStackPack;

pub(super) fn emit(generator: &mut Generator, plan: &EndianStackPack<'_>) {
    let swapped = generator.fresh_label();
    let selected = generator.fresh_label();
    generator.non_leaf = true;
    generator.frame_size = 32;
    generator.output.pre_scheduled = true;
    generator.owns_link_register_schedule = true;
    generator.output.instructions.extend([
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
        Instruction::MoveFromLinkRegister { d: 0 },
    ]);
    if generator.behavior.global_addressing == GlobalAddressing::Absolute {
        generator.record_relocation(RelocationKind::Addr16Ha, plan.flag);
        generator.output.instructions.push(Instruction::AddImmediateShifted {
            d: 4,
            a: 0,
            immediate: 0,
        });
    }
    generator.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
    match generator.behavior.global_addressing {
        GlobalAddressing::SmallData => {
            generator.record_relocation(RelocationKind::EmbSda21, plan.flag);
            generator.output.instructions.push(Instruction::LoadWord { d: 0, a: 0, offset: 0 });
        }
        GlobalAddressing::Absolute => {
            generator.record_relocation(RelocationKind::Addr16Lo, plan.flag);
            generator.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 0 });
        }
    }
    generator.output.instructions.extend([
        Instruction::StoreWord { s: 5, a: 1, offset: 8 },
        Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        Instruction::StoreWord { s: 6, a: 1, offset: 12 },
    ]);
    generator.emit_branch_conditional_to(12, 2, swapped);
    generator.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 8 });
    generator.emit_branch_to(selected);
    generator.bind_label(swapped);
    let registers = [11, 10, 9, 8, 7, 6, 5, 0];
    for (destination, register) in registers.into_iter().enumerate() {
        generator.output.instructions.push(Instruction::LoadByteZero {
            d: register,
            a: 1,
            offset: 15 - destination as i16,
        });
        if destination == 0 {
            generator.output.instructions.push(Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: 16,
            });
        }
    }
    for (destination, register) in registers.into_iter().enumerate() {
        generator.output.instructions.push(Instruction::StoreByte {
            s: register,
            a: 1,
            offset: 16 + destination as i16,
        });
    }
    generator.bind_label(selected);
    generator.output.instructions.push(Instruction::load_immediate(5, 8));
    generator.record_relocation(RelocationKind::Rel24, plan.callee);
    generator.output.instructions.push(Instruction::BranchAndLink {
        target: plan.callee.to_owned(),
    });
    generator.output.instructions.extend([
        Instruction::LoadWord { d: 0, a: 1, offset: 36 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
        Instruction::BranchToLinkRegister,
    ]);
}
