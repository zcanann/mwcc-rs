//! Predecrement stack-pack emission with a verified bounded append composed in.

#[allow(unused_imports)]
use super::super::*;
use super::recognize::{EndianStackPack, InlineAppend};

pub(super) fn emit(
    generator: &mut Generator,
    plan: &EndianStackPack<'_>,
    append: &InlineAppend,
) {
    let swapped = generator.fresh_label();
    let selected = generator.fresh_label();
    let enough_space = generator.fresh_label();
    let block_copy = generator.fresh_label();
    let copy_done = generator.fresh_label();

    generator.non_leaf = true;
    generator.frame_size = 48;
    generator.callee_saved = vec![31, 30, 29];
    generator.output.pre_scheduled = true;
    generator.owns_link_register_schedule = true;
    generator.output.instructions.extend([
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -48 },
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
    generator.output.instructions.extend([
        Instruction::StoreWord { s: 0, a: 1, offset: 52 },
        Instruction::StoreWord { s: 31, a: 1, offset: 44 },
        Instruction::move_register(31, 3),
        Instruction::StoreWord { s: 30, a: 1, offset: 40 },
        Instruction::StoreWord { s: 29, a: 1, offset: 36 },
    ]);
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
    let registers = [10, 9, 8, 7, 6, 5, 3, 0];
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
    generator.output.instructions.extend([
        Instruction::LoadWord { d: 3, a: 31, offset: append.count_offset },
        Instruction::load_immediate(29, 8),
        Instruction::load_immediate(30, 0),
        Instruction::SubtractFromImmediate {
            d: 0,
            a: 3,
            immediate: append.capacity,
        },
        Instruction::CompareLogicalWordImmediate { a: 0, immediate: 8 },
    ]);
    generator.emit_branch_conditional_to(4, 0, enough_space);
    generator.output.instructions.extend([
        Instruction::load_immediate(30, append.error_code),
        Instruction::move_register(29, 0),
    ]);
    generator.bind_label(enough_space);
    generator.output.instructions.push(Instruction::CompareLogicalWordImmediate {
        a: 29,
        immediate: 1,
    });
    generator.emit_branch_conditional_to(4, 2, block_copy);
    generator.output.instructions.extend([
        Instruction::LoadByteZero { d: 0, a: 4, offset: 0 },
        Instruction::Add { d: 3, a: 31, b: 3 },
        Instruction::StoreByte { s: 0, a: 3, offset: append.data_offset },
    ]);
    generator.emit_branch_to(copy_done);
    generator.bind_label(block_copy);
    generator.output.instructions.extend([
        Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: append.data_offset,
        },
        Instruction::move_register(5, 29),
        Instruction::Add { d: 3, a: 31, b: 3 },
    ]);
    generator.record_relocation(RelocationKind::Rel24, &append.copy_callee);
    generator.output.instructions.push(Instruction::BranchAndLink {
        target: append.copy_callee.clone(),
    });
    generator.bind_label(copy_done);
    generator.output.instructions.extend([
        Instruction::LoadWord { d: 0, a: 31, offset: append.count_offset },
        Instruction::move_register(3, 30),
        Instruction::Add { d: 0, a: 0, b: 29 },
        Instruction::StoreWord { s: 0, a: 31, offset: append.count_offset },
        Instruction::LoadWord { d: 0, a: 31, offset: append.count_offset },
        Instruction::StoreWord { s: 0, a: 31, offset: append.mirror_offset },
        Instruction::LoadWord { d: 31, a: 1, offset: 44 },
        Instruction::LoadWord { d: 30, a: 1, offset: 40 },
        Instruction::LoadWord { d: 29, a: 1, offset: 36 },
        Instruction::LoadWord { d: 0, a: 1, offset: 52 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 48 },
        Instruction::BranchToLinkRegister,
    ]);
}
