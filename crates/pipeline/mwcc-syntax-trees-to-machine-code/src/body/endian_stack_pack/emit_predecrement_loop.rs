//! GC/1.3 composition of a single-use endian word pack inside a counted loop.

#[allow(unused_imports)]
use super::super::*;
use super::recognize_loop::PackLoop;

pub(super) fn emit(
    generator: &mut Generator,
    _loop_plan: &PackLoop<'_>,
    flag: &str,
    callee: &str,
) {
    const BUFFER: u8 = 27;
    const COUNT: u8 = 28;
    const INDEX: u8 = 29;
    const DATA: u8 = 30;
    const FLAG: u8 = 31;
    let body = generator.fresh_label();
    let swapped = generator.fresh_label();
    let selected = generator.fresh_label();
    let condition = generator.fresh_label();
    let epilogue = generator.fresh_label();

    generator.non_leaf = true;
    generator.frame_size = 48;
    generator.callee_saved = (27..=31).collect();
    generator.output.pre_scheduled = true;
    generator.owns_link_register_schedule = true;
    generator.output.instructions.extend([
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -48 },
        Instruction::MoveFromLinkRegister { d: 0 },
    ]);
    generator.record_relocation(RelocationKind::Addr16Ha, flag);
    generator.output.instructions.extend([
        Instruction::AddImmediateShifted { d: 6, a: 0, immediate: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 52 },
        Instruction::StoreMultipleWord { s: 27, a: 1, offset: 28 },
        Instruction::move_register(BUFFER, 3),
        Instruction::move_register(COUNT, 5),
        Instruction::move_register(DATA, 4),
    ]);
    generator.record_relocation(RelocationKind::Addr16Lo, flag);
    generator.output.instructions.extend([
        Instruction::AddImmediate { d: FLAG, a: 6, immediate: 0 },
        Instruction::load_immediate(INDEX, 0),
        Instruction::load_immediate(3, 0),
    ]);
    generator.emit_branch_to(condition);

    generator.bind_label(body);
    generator.output.instructions.extend([
        Instruction::LoadWord { d: 0, a: FLAG, offset: 0 },
        Instruction::LoadWord { d: 3, a: DATA, offset: 0 },
        Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        Instruction::StoreWord { s: 3, a: 1, offset: 8 },
    ]);
    generator.emit_branch_conditional_to(12, 2, swapped);
    generator.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 8 });
    generator.emit_branch_to(selected);

    generator.bind_label(swapped);
    for (destination, register) in [6, 5, 3, 0].into_iter().enumerate() {
        generator.output.instructions.push(Instruction::LoadByteZero {
            d: register,
            a: 1,
            offset: 11 - destination as i16,
        });
        if destination == 0 {
            generator.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 12 });
        }
    }
    for (destination, register) in [6, 5, 3, 0].into_iter().enumerate() {
        generator.output.instructions.push(Instruction::StoreByte {
            s: register,
            a: 1,
            offset: 12 + destination as i16,
        });
    }

    generator.bind_label(selected);
    generator.output.instructions.extend([
        Instruction::move_register(3, BUFFER),
        Instruction::load_immediate(5, 4),
    ]);
    generator.record_relocation(RelocationKind::Rel24, callee);
    generator.output.instructions.extend([
        Instruction::BranchAndLink { target: callee.to_owned() },
        Instruction::AddImmediate { d: DATA, a: DATA, immediate: 4 },
        Instruction::AddImmediate { d: INDEX, a: INDEX, immediate: 1 },
    ]);

    generator.bind_label(condition);
    generator.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
    generator.emit_branch_conditional_to(4, 2, epilogue);
    generator.output.instructions.push(Instruction::CompareWord { a: INDEX, b: COUNT });
    generator.emit_branch_conditional_to(12, 0, body);

    generator.bind_label(epilogue);
    generator.output.instructions.extend([
        Instruction::LoadMultipleWord { d: 27, a: 1, offset: 28 },
        Instruction::LoadWord { d: 0, a: 1, offset: 52 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 48 },
        Instruction::BranchToLinkRegister,
    ]);
}
