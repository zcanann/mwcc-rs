//! Deferred bounded-read composition for endian-selecting scalar wrappers.

#[allow(unused_imports)]
use super::super::*;
use super::recognize::{EndianStackUnpack, InlineRead};

struct Registers {
    buffer: u8,
    selected: u8,
    length: u8,
    error: u8,
    output: u8,
}

pub(super) fn emit(
    generator: &mut Generator,
    plan: &EndianStackUnpack<'_>,
    read: &InlineRead,
) {
    let registers = match plan.width {
        4 => Registers {
            buffer: 27,
            selected: 28,
            length: 29,
            error: 30,
            output: 31,
        },
        8 => Registers {
            buffer: 27,
            selected: 31,
            length: 28,
            error: 29,
            output: 30,
        },
        _ => unreachable!(),
    };
    let temporary = generator.fresh_label();
    let selected = generator.fresh_label();
    let enough_bytes = generator.fresh_label();
    let epilogue = generator.fresh_label();

    generator.non_leaf = true;
    generator.frame_size = 48;
    generator.callee_saved = (27..=31).collect();
    generator.output.pre_scheduled = true;
    generator.owns_link_register_schedule = true;
    generator.output.instructions.extend([
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -48,
        },
        Instruction::MoveFromLinkRegister { d: 0 },
    ]);
    if generator.behavior.global_addressing == GlobalAddressing::Absolute {
        generator.record_relocation(RelocationKind::Addr16Ha, plan.flag);
        generator
            .output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: 0,
            });
    }
    generator.output.instructions.extend([
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 52,
        },
        Instruction::StoreMultipleWord {
            s: 27,
            a: 1,
            offset: 28,
        },
        Instruction::move_register(registers.buffer, 3),
        Instruction::move_register(registers.output, 4),
    ]);
    emit_global_load(generator, plan.flag, 5);
    generator
        .output
        .instructions
        .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
    generator.emit_branch_conditional_to(12, 2, temporary);
    generator.output.instructions.push(Instruction::move_register(
        registers.selected,
        registers.output,
    ));
    generator.emit_branch_to(selected);
    generator.bind_label(temporary);
    generator.output.instructions.push(Instruction::AddImmediate {
        d: registers.selected,
        a: 1,
        immediate: 8,
    });
    generator.bind_label(selected);

    generator.output.instructions.extend([
        Instruction::LoadWord {
            d: 3,
            a: registers.buffer,
            offset: read.position_offset,
        },
        Instruction::load_immediate(registers.length, i16::from(plan.width)),
        Instruction::LoadWord {
            d: 0,
            a: registers.buffer,
            offset: read.length_offset,
        },
        Instruction::load_immediate(registers.error, 0),
        Instruction::SubtractFrom { d: 0, a: 3, b: 0 },
        Instruction::CompareLogicalWord {
            a: registers.length,
            b: 0,
        },
    ]);
    generator.emit_branch_conditional_to(4, 1, enough_bytes);
    generator.output.instructions.extend([
        Instruction::load_immediate(registers.error, read.error_code),
        Instruction::move_register(registers.length, 0),
    ]);
    generator.bind_label(enough_bytes);
    generator.output.instructions.extend([
        Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: read.data_offset,
        },
        Instruction::move_register(3, registers.selected),
        Instruction::move_register(5, registers.length),
        Instruction::Add {
            d: 4,
            a: registers.buffer,
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
            a: registers.buffer,
            offset: read.position_offset,
        },
    ]);
    if generator.behavior.global_addressing == GlobalAddressing::Absolute {
        generator.record_relocation(RelocationKind::Addr16Ha, plan.flag);
        generator
            .output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            });
    }
    generator.output.instructions.extend([
        Instruction::Add {
            d: 0,
            a: 0,
            b: registers.length,
        },
        Instruction::StoreWord {
            s: 0,
            a: registers.buffer,
            offset: read.position_offset,
        },
    ]);
    emit_global_load(generator, plan.flag, 3);
    generator
        .output
        .instructions
        .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
    generator.emit_branch_conditional_to(4, 2, epilogue);
    generator
        .output
        .instructions
        .push(Instruction::CompareWordImmediate {
            a: registers.error,
            immediate: 0,
        });
    generator.emit_branch_conditional_to(4, 2, epilogue);
    for destination in 0..plan.width {
        generator.output.instructions.extend([
            Instruction::LoadByteZero {
                d: 0,
                a: registers.selected,
                offset: i16::from(plan.width - 1 - destination),
            },
            Instruction::StoreByte {
                s: 0,
                a: registers.output,
                offset: i16::from(destination),
            },
        ]);
    }
    generator.bind_label(epilogue);
    generator.output.instructions.extend([
        Instruction::move_register(3, registers.error),
        Instruction::LoadMultipleWord {
            d: 27,
            a: 1,
            offset: 28,
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 52,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 48,
        },
        Instruction::BranchToLinkRegister,
    ]);
}

fn emit_global_load(generator: &mut Generator, global: &str, absolute_base: u8) {
    match generator.behavior.global_addressing {
        GlobalAddressing::SmallData => {
            generator.record_relocation(RelocationKind::EmbSda21, global);
            generator.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 0,
                offset: 0,
            });
        }
        GlobalAddressing::Absolute => {
            generator.record_relocation(RelocationKind::Addr16Lo, global);
            generator.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: absolute_base,
                offset: 0,
            });
        }
    }
}
