use super::*;

pub(super) fn emit(generator: &mut Generator, plan: &DspChannelParameterUpdate<'_>) {
    const OBJECT: u8 = 28;
    const INDEX: u8 = 29;
    const CHANNEL_ID: u8 = 30;
    const BYTE_OFFSET: u8 = 31;
    generator.non_leaf = true;
    generator.frame_size = 32;
    generator.callee_saved = vec![BYTE_OFFSET, CHANNEL_ID, INDEX, OBJECT];
    generator.output.pre_scheduled = true;
    generator.owns_link_register_schedule = true;

    generator.output.instructions.extend([
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 4 },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
        Instruction::StoreMultipleWord { s: OBJECT, a: 1, offset: 16 },
    ]);
    if let Some(leading) = &plan.leading {
        generator.output.instructions.extend([
            Instruction::move_register(OBJECT, 3),
            Instruction::LoadWord { d: 4, a: 3, offset: plan.channel_pointer_offset },
            Instruction::LoadWord { d: 3, a: 3, offset: plan.manager_offset },
            Instruction::LoadByteZero { d: CHANNEL_ID, a: 4, offset: plan.channel_id_offset },
            Instruction::LoadByteZero { d: 4, a: 3, offset: leading.offset },
            Instruction::move_register(3, CHANNEL_ID),
        ]);
        emit_call(generator, leading.call);
        generator.output.instructions.extend([
            Instruction::load_immediate(INDEX, 0),
            Instruction::load_immediate(BYTE_OFFSET, 0),
        ]);
    } else {
        generator.output.instructions.extend([
            Instruction::AddImmediate { d: OBJECT, a: 3, immediate: 0 },
            Instruction::load_immediate(INDEX, 0),
            Instruction::load_immediate(BYTE_OFFSET, 0),
            Instruction::LoadWord { d: 3, a: 3, offset: plan.channel_pointer_offset },
            Instruction::LoadByteZero { d: CHANNEL_ID, a: 3, offset: plan.channel_id_offset },
        ]);
    }

    let loop_start = generator.output.instructions.len();
    generator.output.instructions.extend([
        Instruction::LoadWord { d: 3, a: OBJECT, offset: plan.manager_offset },
        Instruction::AddImmediate { d: 4, a: BYTE_OFFSET, immediate: plan.lane_values_offset },
        Instruction::AddImmediate { d: 0, a: INDEX, immediate: plan.lane_modes_offset },
        Instruction::LoadHalfwordAlgebraicIndexed { d: 5, a: OBJECT, b: 4 },
        Instruction::LoadByteZeroIndexed { d: 6, a: 3, b: 0 },
        Instruction::AddImmediate { d: 3, a: CHANNEL_ID, immediate: 0 },
        Instruction::ClearLeftImmediate { a: 4, s: INDEX, clear: 24 },
    ]);
    emit_call(generator, plan.mixer);
    generator.output.instructions.extend([
        Instruction::AddImmediate { d: INDEX, a: INDEX, immediate: 1 },
        Instruction::AddImmediate { d: BYTE_OFFSET, a: BYTE_OFFSET, immediate: 2 },
        Instruction::CompareLogicalWordImmediate { a: INDEX, immediate: plan.lane_count },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 0,
            target: loop_start,
        },
        Instruction::move_register(3, CHANNEL_ID),
        Instruction::LoadHalfwordZero { d: 4, a: OBJECT, offset: plan.pitch_offset },
    ]);
    emit_call(generator, plan.pitch);

    emit_conditional_filter(
        generator,
        plan,
        32,
        plan.iir_offset,
        plan.iir,
    );
    emit_conditional_filter(
        generator,
        plan,
        31,
        plan.fir_offset,
        plan.fir,
    );

    generator.output.instructions.extend([
        Instruction::LoadWord { d: 4, a: OBJECT, offset: plan.manager_offset },
        Instruction::move_register(3, CHANNEL_ID),
        Instruction::LoadByteZero { d: 4, a: 4, offset: plan.filter_mode_offset },
    ]);
    emit_call(generator, plan.mode);
    if let Some(distance) = &plan.distance {
        generator.output.instructions.extend([
            Instruction::LoadWord { d: 4, a: OBJECT, offset: plan.manager_offset },
            Instruction::move_register(3, CHANNEL_ID),
            Instruction::LoadHalfwordAlgebraic { d: 4, a: 4, offset: distance.offset },
        ]);
        emit_call(generator, distance.call);
    }
    generator.output.instructions.extend([
        Instruction::move_register(3, CHANNEL_ID),
        Instruction::LoadByteZero { d: 4, a: OBJECT, offset: plan.pause_offset },
    ]);
    emit_call(generator, plan.pause);
    generator.output.instructions.extend([
        Instruction::LoadMultipleWord { d: OBJECT, a: 1, offset: 16 },
        Instruction::LoadWord { d: 0, a: 1, offset: 36 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ]);
}

fn emit_conditional_filter(
    generator: &mut Generator,
    plan: &DspChannelParameterUpdate<'_>,
    mask: u8,
    value_offset: i16,
    call: &str,
) {
    generator.output.instructions.extend([
        Instruction::LoadWord { d: 4, a: 28, offset: plan.manager_offset },
        Instruction::LoadByteZero { d: 0, a: 4, offset: plan.filter_mode_offset },
        if mask == 32 {
            Instruction::AndMaskRecord { a: 0, s: 0, begin: 26, end: 26 }
        } else {
            Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 27 }
        },
    ]);
    let branch = generator.output.instructions.len();
    generator.output.instructions.push(Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: 0,
    });
    generator.output.instructions.extend([
        Instruction::AddImmediate { d: 3, a: 30, immediate: 0 },
        Instruction::AddImmediate { d: 4, a: 4, immediate: value_offset },
    ]);
    emit_call(generator, call);
    let target = generator.output.instructions.len();
    let Instruction::BranchConditionalForward { target: branch_target, .. } =
        &mut generator.output.instructions[branch]
    else {
        unreachable!()
    };
    *branch_target = target;
}

fn emit_call(generator: &mut Generator, target: &str) {
    generator.record_relocation(RelocationKind::Rel24, target);
    generator.output.instructions.push(Instruction::BranchAndLink {
        target: target.to_owned(),
    });
}
