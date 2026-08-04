use super::*;

pub(super) fn emit(generator: &mut Generator, plan: &ReleaseListBankToGlobal<'_>) {
    const OBJECT: u8 = 27;
    const PRIMARY_LIST: u8 = 28;
    const LANE: u8 = 29;
    const GLOBAL: u8 = 30;
    const SOURCE: u8 = 31;
    generator.non_leaf = true;
    generator.frame_size = 40;
    generator.callee_saved = vec![SOURCE, GLOBAL, LANE, PRIMARY_LIST, OBJECT];
    generator.output.pre_scheduled = true;
    generator.owns_link_register_schedule = true;

    let anchor_symbol = generator
        .data_section_anchor
        .as_ref()
        .filter(|anchor| anchor.symbols.contains(plan.global))
        .map(|anchor| anchor.anchor_symbol.clone())
        .unwrap_or_else(|| "...bss.0".to_owned());
    generator.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
    generator.emit_address_high(4, &anchor_symbol);
    generator.output.instructions.extend([
        Instruction::StoreWord { s: 0, a: 1, offset: 4 },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -40 },
        Instruction::StoreMultipleWord { s: OBJECT, a: 1, offset: 20 },
    ]);
    generator.record_relocation(RelocationKind::Addr16Lo, &anchor_symbol);
    generator.record_data_section_symbol_displacement(plan.global);
    generator.output.instructions.extend([
        Instruction::AddImmediate { d: GLOBAL, a: 4, immediate: 0 },
        Instruction::AddImmediate { d: SOURCE, a: 3, immediate: 0 },
        Instruction::AddImmediate {
            d: PRIMARY_LIST,
            a: GLOBAL,
            immediate: plan.destination_offsets[0],
        },
    ]);

    emit_regular_lane(generator, plan, 0, PRIMARY_LIST, 8, 17);
    generator.output.instructions.push(Instruction::AddImmediate {
        d: LANE,
        a: GLOBAL,
        immediate: plan.destination_offsets[1],
    });
    emit_regular_lane(generator, plan, 1, LANE, 18, 27);
    generator.output.instructions.push(Instruction::AddImmediate {
        d: LANE,
        a: GLOBAL,
        immediate: plan.destination_offsets[2],
    });
    emit_regular_lane(generator, plan, 2, LANE, 28, 37);

    generator.output.instructions.push(Instruction::AddImmediate {
        d: 3,
        a: SOURCE,
        immediate: plan.source_offsets[3],
    });
    emit_call(generator, plan.take);
    generator.output.instructions.extend([
        Instruction::OrRecord { a: LANE, s: 3, b: 3 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 48,
        },
        Instruction::move_register(3, LANE),
    ]);
    emit_call(generator, plan.cancel);
    generator.output.instructions.extend([
        Instruction::AddImmediate { d: 3, a: PRIMARY_LIST, immediate: 0 },
        Instruction::AddImmediate { d: 4, a: LANE, immediate: 0 },
    ]);
    emit_call(generator, plan.append);
    generator.output.instructions.extend([
        Instruction::StoreWord {
            s: GLOBAL,
            a: LANE,
            offset: plan.object_owner_offset,
        },
        Instruction::Branch { target: 37 },
        Instruction::LoadWord { d: 5, a: GLOBAL, offset: plan.count_offset },
        Instruction::load_immediate(0, 0),
        Instruction::LoadWord { d: 4, a: SOURCE, offset: plan.count_offset },
        Instruction::load_immediate(3, 0),
        Instruction::Add { d: 4, a: 5, b: 4 },
        Instruction::StoreWord { s: 4, a: GLOBAL, offset: plan.count_offset },
        Instruction::StoreWord { s: 0, a: SOURCE, offset: plan.count_offset },
        Instruction::LoadWord { d: 0, a: 1, offset: 44 },
        Instruction::LoadMultipleWord { d: OBJECT, a: 1, offset: 20 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 40 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ]);
}

fn emit_regular_lane(
    generator: &mut Generator,
    plan: &ReleaseListBankToGlobal<'_>,
    lane: usize,
    destination: u8,
    start: usize,
    next: usize,
) {
    generator.output.instructions.push(Instruction::AddImmediate {
        d: 3,
        a: 31,
        immediate: plan.source_offsets[lane],
    });
    emit_call(generator, plan.take);
    generator.output.instructions.extend([
        Instruction::OrRecord { a: 27, s: 3, b: 3 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: next,
        },
        Instruction::AddImmediate { d: 3, a: destination, immediate: 0 },
        Instruction::AddImmediate { d: 4, a: 27, immediate: 0 },
    ]);
    emit_call(generator, plan.append);
    generator.output.instructions.extend([
        Instruction::StoreWord { s: 30, a: 27, offset: plan.object_owner_offset },
        Instruction::Branch { target: start },
    ]);
}

fn emit_call(generator: &mut Generator, target: &str) {
    generator.record_relocation(RelocationKind::Rel24, target);
    generator.output.instructions.push(Instruction::BranchAndLink {
        target: target.to_owned(),
    });
}
