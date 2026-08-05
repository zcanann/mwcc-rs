use super::*;

const MESSAGE_ID: u8 = 27;
const OUTPUT: u8 = 28;
const INDEX: u8 = 29;
const ERROR: u8 = 30;
const BUFFER: u8 = 31;
// GC/1.3 build 163 numbers the eliminated lookup, reset, and setter value
// graphs before the caller's diagnostic string. The composed machine body no
// longer visits those graphs, so retain their measured source-analysis cost.
const COMPOSED_HELPER_ORDINAL_RESIDUE: u32 = 217;

fn emit_call(generator: &mut Generator, target: &str) {
    generator.record_relocation(RelocationKind::Rel24, target);
    generator.output.instructions.push(Instruction::BranchAndLink {
        target: target.to_owned(),
    });
}

pub(super) fn emit(generator: &mut Generator, plan: &Plan) -> Compilation<()> {
    let loop_body = generator.fresh_label();
    let range_failed = generator.fresh_label();
    let release = generator.fresh_label();
    let loop_test = generator.fresh_label();
    let done = generator.fresh_label();

    generator.non_leaf = true;
    generator.frame_size = 32;
    generator.callee_saved = vec![BUFFER, ERROR, INDEX, OUTPUT, MESSAGE_ID];
    generator.output.pre_scheduled = true;
    generator.owns_link_register_schedule = true;
    generator.output.anonymous_label_bump += COMPOSED_HELPER_ORDINAL_RESIDUE;
    generator.output.instructions.extend([
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 36 },
        Instruction::load_immediate(0, 0),
        Instruction::StoreMultipleWord { s: MESSAGE_ID, a: 1, offset: 12 },
        Instruction::move_register(OUTPUT, 4),
        Instruction::move_register(MESSAGE_ID, 3),
        Instruction::load_immediate(ERROR, plan.unavailable),
        Instruction::load_immediate(INDEX, 0),
        Instruction::StoreWord { s: 0, a: 4, offset: 0 },
    ]);
    generator.emit_branch_to(loop_test);

    generator.bind_label(loop_body);
    generator.output.instructions.extend([
        Instruction::CompareWordImmediate { a: INDEX, immediate: 0 },
        Instruction::load_immediate(BUFFER, 0),
    ]);
    generator.emit_branch_conditional_to(12, 0, range_failed);
    generator.output.instructions.push(Instruction::CompareWordImmediate {
        a: INDEX,
        immediate: plan.bound,
    });
    generator.emit_branch_conditional_to(4, 0, range_failed);
    generator.output.instructions.push(Instruction::MultiplyImmediate {
        d: 4,
        a: INDEX,
        immediate: plan.stride,
    });
    generator.record_relocation(RelocationKind::Addr16Ha, &plan.array);
    generator.output.instructions.push(Instruction::AddImmediateShifted {
        d: 3,
        a: 0,
        immediate: 0,
    });
    generator.record_relocation(RelocationKind::Addr16Lo, &plan.array);
    generator.output.instructions.extend([
        Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
        Instruction::Add { d: BUFFER, a: 0, b: 4 },
    ]);

    generator.bind_label(range_failed);
    generator.output.instructions.push(Instruction::move_register(3, BUFFER));
    emit_call(generator, &plan.acquire);
    generator.output.instructions.extend([
        Instruction::LoadWord { d: 0, a: BUFFER, offset: plan.used_offset },
        Instruction::CompareWordImmediate { a: 0, immediate: 0 },
    ]);
    generator.emit_branch_conditional_to(4, 2, release);
    generator.output.instructions.extend([
        Instruction::load_immediate(3, 0),
        Instruction::load_immediate(0, 1),
        Instruction::StoreWord { s: 3, a: BUFFER, offset: plan.length_offset },
        Instruction::load_immediate(ERROR, plan.success),
        Instruction::StoreWord { s: 3, a: BUFFER, offset: plan.position_offset },
        Instruction::StoreWord { s: 0, a: BUFFER, offset: plan.used_offset },
        Instruction::StoreWord { s: BUFFER, a: OUTPUT, offset: 0 },
        Instruction::StoreWord { s: INDEX, a: MESSAGE_ID, offset: 0 },
        Instruction::load_immediate(INDEX, plan.bound),
    ]);

    generator.bind_label(release);
    generator.output.instructions.push(Instruction::move_register(3, BUFFER));
    emit_call(generator, &plan.release);
    generator.output.instructions.push(Instruction::AddImmediate {
        d: INDEX,
        a: INDEX,
        immediate: 1,
    });

    generator.bind_label(loop_test);
    generator.output.instructions.push(Instruction::CompareWordImmediate {
        a: INDEX,
        immediate: plan.bound,
    });
    generator.emit_branch_conditional_to(12, 0, loop_body);
    generator.output.instructions.push(Instruction::CompareWordImmediate {
        a: ERROR,
        immediate: plan.unavailable,
    });
    generator.emit_branch_conditional_to(4, 2, done);
    generator.emit_string_literal(&plan.report_text, 3)?;
    emit_call(generator, &plan.report);

    generator.bind_label(done);
    generator.output.instructions.extend([
        Instruction::move_register(3, ERROR),
        Instruction::LoadMultipleWord { d: MESSAGE_ID, a: 1, offset: 12 },
        Instruction::LoadWord { d: 0, a: 1, offset: 36 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
        Instruction::BranchToLinkRegister,
    ]);
    Ok(())
}
