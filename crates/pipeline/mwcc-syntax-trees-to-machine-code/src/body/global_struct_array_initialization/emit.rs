use super::*;

pub(super) fn emit(generator: &mut Generator, plan: &GlobalStructArrayInitialization<'_>) {
    const INDEX: u8 = 27;
    const OWNER: u8 = 28;
    const ELEMENT: u8 = 29;
    const ARRAY: u8 = 30;
    const BYTE_OFFSET: u8 = 31;

    generator.non_leaf = true;
    generator.frame_size = 32;
    generator.callee_saved = vec![BYTE_OFFSET, ARRAY, ELEMENT, OWNER, INDEX];
    generator.output.pre_scheduled = true;
    generator.owns_link_register_schedule = true;

    generator.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
    generator.emit_address_high(3, plan.owner_global);
    generator.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 4 });
    generator.record_relocation(RelocationKind::Addr16Lo, plan.owner_global);
    generator.output.instructions.extend([
        Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
        Instruction::StoreMultipleWord { s: INDEX, a: 1, offset: 12 },
        Instruction::move_register(OWNER, 0),
        Instruction::AddImmediate { d: 3, a: OWNER, immediate: 0 },
    ]);
    emit_call(generator, plan.owner_init);
    generator.emit_address_high(3, plan.array_global);
    generator.output.instructions.push(Instruction::load_immediate(INDEX, 0));
    generator.record_relocation(RelocationKind::Addr16Lo, plan.array_global);
    generator.output.instructions.extend([
        Instruction::AddImmediate { d: ARRAY, a: 3, immediate: 0 },
        Instruction::load_immediate(BYTE_OFFSET, 0),
        Instruction::Add { d: ELEMENT, a: ARRAY, b: BYTE_OFFSET },
        Instruction::AddImmediate { d: 3, a: ELEMENT, immediate: 0 },
    ]);
    emit_call(generator, plan.element_init);
    generator.output.instructions.extend([
        Instruction::AddImmediate { d: 3, a: OWNER, immediate: plan.list_offset },
        Instruction::AddImmediate { d: 4, a: ELEMENT, immediate: 0 },
    ]);
    emit_call(generator, plan.append);
    generator.output.instructions.extend([
        Instruction::AddImmediate { d: INDEX, a: INDEX, immediate: 1 },
        Instruction::StoreWord { s: OWNER, a: ELEMENT, offset: plan.owner_offset },
        Instruction::CompareWordImmediate { a: INDEX, immediate: plan.count },
        Instruction::AddImmediate { d: BYTE_OFFSET, a: BYTE_OFFSET, immediate: plan.stride },
        Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 13 },
        Instruction::load_immediate(0, plan.count),
        Instruction::StoreWord { s: 0, a: OWNER, offset: plan.count_offset },
        Instruction::LoadMultipleWord { d: INDEX, a: 1, offset: 12 },
        Instruction::LoadWord { d: 0, a: 1, offset: 36 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ]);
}

fn emit_call(generator: &mut Generator, target: &str) {
    generator.record_relocation(RelocationKind::Rel24, target);
    generator.output.instructions.push(Instruction::BranchAndLink {
        target: target.to_owned(),
    });
}
