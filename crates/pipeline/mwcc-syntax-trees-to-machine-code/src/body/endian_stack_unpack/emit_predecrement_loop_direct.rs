//! Deferred bounded-read composition inside a direct byte loop.

#[allow(unused_imports)]
use super::super::*;
use super::recognize::InlineRead;
use super::recognize_loop::ReadLoop;

pub(super) fn emit(generator: &mut Generator, plan: &ReadLoop<'_>, read: &InlineRead) {
    const BUFFER: u8 = 26;
    const OUTPUT: u8 = 27;
    const COUNT: u8 = 28;
    const INDEX: u8 = 29;
    const LENGTH: u8 = 30;
    const ERROR: u8 = 31;
    let loop_body = generator.fresh_label();
    let enough_bytes = generator.fresh_label();
    let loop_check = generator.fresh_label();
    let epilogue = generator.fresh_label();

    generator.non_leaf = true;
    generator.frame_size = 32;
    generator.callee_saved = (BUFFER..=ERROR).collect();
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
        Instruction::StoreMultipleWord {
            s: BUFFER,
            a: 1,
            offset: 8,
        },
        Instruction::move_register(BUFFER, 3),
        Instruction::move_register(OUTPUT, 4),
        Instruction::move_register(COUNT, 5),
        Instruction::load_immediate(INDEX, 0),
        Instruction::load_immediate(3, 0),
    ]);
    generator.emit_branch_to(loop_check);
    generator.bind_label(loop_body);
    generator.output.instructions.extend([
        Instruction::LoadWord {
            d: 3,
            a: BUFFER,
            offset: read.position_offset,
        },
        Instruction::load_immediate(LENGTH, i16::from(plan.width)),
        Instruction::LoadWord {
            d: 0,
            a: BUFFER,
            offset: read.length_offset,
        },
        Instruction::load_immediate(ERROR, 0),
        Instruction::SubtractFrom { d: 0, a: 3, b: 0 },
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
            a: 3,
            immediate: read.data_offset,
        },
        Instruction::move_register(5, LENGTH),
        Instruction::Add {
            d: 3,
            a: OUTPUT,
            b: INDEX,
        },
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
        Instruction::AddImmediate {
            d: INDEX,
            a: INDEX,
            immediate: 1,
        },
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
    ]);
    generator.bind_label(loop_check);
    generator
        .output
        .instructions
        .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
    generator.emit_branch_conditional_to(4, 2, epilogue);
    generator.output.instructions.push(Instruction::CompareWord {
        a: INDEX,
        b: COUNT,
    });
    generator.emit_branch_conditional_to(12, 0, loop_body);
    generator.bind_label(epilogue);
    generator.output.instructions.extend([
        Instruction::LoadMultipleWord {
            d: BUFFER,
            a: 1,
            offset: 8,
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
