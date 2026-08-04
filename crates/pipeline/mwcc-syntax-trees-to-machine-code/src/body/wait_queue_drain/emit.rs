use super::*;

pub(super) fn emit(generator: &mut Generator, plan: &WaitQueueDrain<'_>) {
    const OBJECT: u8 = 29;
    const TABLE: u8 = 30;
    const STOP_AFTER_ONE: u8 = 31;
    generator.non_leaf = true;
    generator.frame_size = 40;
    generator.callee_saved = vec![STOP_AFTER_ONE, TABLE, OBJECT];
    generator.output.pre_scheduled = true;
    generator.owns_link_register_schedule = true;

    generator.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
    generator.emit_address_high(4, plan.table);
    generator.output.instructions.extend([
        Instruction::StoreWord { s: 0, a: 1, offset: 4 },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -40 },
        Instruction::StoreMultipleWord { s: OBJECT, a: 1, offset: 28 },
        Instruction::ClearLeftImmediate {
            a: STOP_AFTER_ONE,
            s: 3,
            clear: 24,
        },
    ]);
    generator.record_relocation(RelocationKind::Addr16Lo, plan.table);
    generator.output.instructions.extend([
        Instruction::AddImmediate { d: TABLE, a: 4, immediate: 0 },
        Instruction::Branch { target: 55 },
    ]);

    emit_sda(
        generator,
        plan.index,
        Instruction::LoadWord { d: 4, a: 0, offset: 0 },
    );
    generator.output.instructions.extend([
        Instruction::ShiftLeftImmediate { a: 0, s: 4, shift: 2 },
        Instruction::Add { d: 3, a: TABLE, b: 0 },
        Instruction::LoadWord { d: 0, a: 3, offset: 0 },
        Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        Instruction::move_register(OBJECT, 0),
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 45,
        },
        Instruction::load_immediate(3, 0),
        Instruction::AddImmediate { d: 4, a: OBJECT, immediate: 0 },
    ]);
    emit_call(generator, plan.allocate);
    generator.output.instructions.extend([
        Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 58,
        },
        Instruction::StoreWord {
            s: 3,
            a: OBJECT,
            offset: plan.object_result_offset,
        },
        Instruction::move_register(3, OBJECT),
    ]);
    emit_call(generator, plan.play);
    generator.output.instructions.push(Instruction::move_register(3, OBJECT));
    emit_call(generator, plan.cut);
    generator.output.instructions.extend([
        Instruction::CompareWordImmediate { a: 3, immediate: -1 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 31,
        },
        Instruction::LoadWord {
            d: 3,
            a: OBJECT,
            offset: plan.object_manager_offset,
        },
        Instruction::AddImmediate { d: 4, a: OBJECT, immediate: 0 },
        Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: plan.manager_list_offset,
        },
    ]);
    emit_call(generator, plan.append);

    emit_sda(
        generator,
        plan.index,
        Instruction::LoadWord { d: 3, a: 0, offset: 0 },
    );
    generator.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
    emit_sda(
        generator,
        plan.index,
        Instruction::StoreWord { s: 0, a: 0, offset: 0 },
    );
    emit_sda(
        generator,
        plan.index,
        Instruction::LoadWord { d: 0, a: 0, offset: 0 },
    );
    generator.output.instructions.extend([
        Instruction::CompareLogicalWordImmediate { a: 0, immediate: plan.bound },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: 39,
        },
        Instruction::load_immediate(0, 0),
    ]);
    emit_sda(
        generator,
        plan.index,
        Instruction::StoreWord { s: 0, a: 0, offset: 0 },
    );
    emit_sda(
        generator,
        plan.count,
        Instruction::LoadWord { d: 3, a: 0, offset: 0 },
    );
    generator.output.instructions.extend([
        Instruction::CompareLogicalWordImmediate {
            a: STOP_AFTER_ONE,
            immediate: 1,
        },
        Instruction::AddImmediate { d: 0, a: 3, immediate: -1 },
    ]);
    emit_sda(
        generator,
        plan.count,
        Instruction::StoreWord { s: 0, a: 0, offset: 0 },
    );
    generator.output.instructions.extend([
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 58,
        },
        Instruction::Branch { target: 55 },
        Instruction::AddImmediate { d: 0, a: 4, immediate: 1 },
    ]);
    emit_sda(
        generator,
        plan.index,
        Instruction::StoreWord { s: 0, a: 0, offset: 0 },
    );
    emit_sda(
        generator,
        plan.index,
        Instruction::LoadWord { d: 0, a: 0, offset: 0 },
    );
    generator.output.instructions.extend([
        Instruction::CompareLogicalWordImmediate { a: 0, immediate: plan.bound },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: 52,
        },
        Instruction::load_immediate(0, 0),
    ]);
    emit_sda(
        generator,
        plan.index,
        Instruction::StoreWord { s: 0, a: 0, offset: 0 },
    );
    emit_sda(
        generator,
        plan.count,
        Instruction::LoadWord { d: 3, a: 0, offset: 0 },
    );
    generator.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
    emit_sda(
        generator,
        plan.count,
        Instruction::StoreWord { s: 0, a: 0, offset: 0 },
    );
    emit_sda(
        generator,
        plan.count,
        Instruction::LoadWord { d: 0, a: 0, offset: 0 },
    );
    generator.output.instructions.extend([
        Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: 8,
        },
        Instruction::LoadMultipleWord { d: OBJECT, a: 1, offset: 28 },
        Instruction::LoadWord { d: 0, a: 1, offset: 44 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 40 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ]);
}

fn emit_sda(generator: &mut Generator, symbol: &str, instruction: Instruction) {
    generator.record_relocation(RelocationKind::EmbSda21, symbol);
    generator.output.instructions.push(instruction);
}

fn emit_call(generator: &mut Generator, target: &str) {
    generator.record_relocation(RelocationKind::Rel24, target);
    generator.output.instructions.push(Instruction::BranchAndLink {
        target: target.to_owned(),
    });
}
