//! Deferred bounded-read composition for the direct one-byte wrapper.

#[allow(unused_imports)]
use super::super::*;
use super::recognize::{DirectStackUnpack, InlineRead};

pub(super) fn emit(
    generator: &mut Generator,
    plan: &DirectStackUnpack<'_>,
    read: &InlineRead,
) {
    const BUFFER: u8 = 29;
    const LENGTH: u8 = 30;
    const ERROR: u8 = 31;
    let enough_bytes = generator.fresh_label();

    generator.non_leaf = true;
    generator.frame_size = 32;
    generator.callee_saved = vec![BUFFER, LENGTH, ERROR];
    generator.output.pre_scheduled = true;
    generator.owns_link_register_schedule = true;
    generator.output.instructions.extend([
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -32,
        },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        },
        Instruction::StoreWord {
            s: ERROR,
            a: 1,
            offset: 28,
        },
        Instruction::load_immediate(ERROR, 0),
        Instruction::StoreWord {
            s: LENGTH,
            a: 1,
            offset: 24,
        },
        Instruction::load_immediate(LENGTH, i16::from(plan.width)),
        Instruction::StoreWord {
            s: BUFFER,
            a: 1,
            offset: 20,
        },
        Instruction::move_register(BUFFER, 3),
        Instruction::LoadWord {
            d: 5,
            a: 3,
            offset: read.position_offset,
        },
        Instruction::move_register(3, 4),
        Instruction::LoadWord {
            d: 0,
            a: BUFFER,
            offset: read.length_offset,
        },
        Instruction::SubtractFrom { d: 0, a: 5, b: 0 },
        Instruction::CompareLogicalWord {
            a: LENGTH,
            b: 0,
        },
    ]);
    generator.emit_branch_conditional_to(4, 1, enough_bytes);
    generator.output.instructions.extend([
        Instruction::load_immediate(ERROR, read.error_code),
        Instruction::move_register(LENGTH, 0),
    ]);
    generator.bind_label(enough_bytes);
    generator.output.instructions.extend([
        Instruction::AddImmediate {
            d: 4,
            a: 5,
            immediate: read.data_offset,
        },
        Instruction::move_register(5, LENGTH),
        Instruction::Add {
            d: 4,
            a: BUFFER,
            b: 4,
        },
    ]);
    generator.record_relocation(RelocationKind::Rel24, &read.copy_callee);
    generator.output.instructions.push(Instruction::BranchAndLink {
        target: read.copy_callee.clone(),
    });
    generator.output.instructions.extend([
        Instruction::LoadWord {
            d: 0,
            a: BUFFER,
            offset: read.position_offset,
        },
        Instruction::move_register(3, ERROR),
        Instruction::Add {
            d: 0,
            a: 0,
            b: LENGTH,
        },
        Instruction::StoreWord {
            s: 0,
            a: BUFFER,
            offset: read.position_offset,
        },
        Instruction::LoadWord {
            d: ERROR,
            a: 1,
            offset: 28,
        },
        Instruction::LoadWord {
            d: LENGTH,
            a: 1,
            offset: 24,
        },
        Instruction::LoadWord {
            d: BUFFER,
            a: 1,
            offset: 20,
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        },
        Instruction::BranchToLinkRegister,
    ]);
}
