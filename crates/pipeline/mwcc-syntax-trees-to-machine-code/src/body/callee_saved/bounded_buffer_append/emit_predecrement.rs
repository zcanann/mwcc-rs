//! Predecrement emission for a semantically verified bounded byte-buffer append.

#[allow(unused_imports)]
use super::super::*;
use super::AppendPlan;

pub(super) fn emit(generator: &mut Generator, plan: &AppendPlan<'_>) {
    const ERROR: u8 = 31;
    const LENGTH: u8 = 30;
    const BUFFER: u8 = 29;
    let nonempty = generator.fresh_label();
    let unclamped = generator.fresh_label();
    let bulk = generator.fresh_label();
    let copied = generator.fresh_label();
    let epilogue = generator.fresh_label();

    generator.non_leaf = true;
    generator.frame_size = 32;
    generator.callee_saved = vec![ERROR, LENGTH, BUFFER];
    generator.output.pre_scheduled = true;
    generator.owns_link_register_schedule = true;
    generator.output.instructions.extend([
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 36 },
        Instruction::StoreWord { s: ERROR, a: 1, offset: 28 },
        Instruction::load_immediate(ERROR, 0),
        Instruction::StoreWord { s: LENGTH, a: 1, offset: 24 },
        Instruction::OrRecord { a: LENGTH, s: 5, b: 5 },
        Instruction::StoreWord { s: BUFFER, a: 1, offset: 20 },
        Instruction::move_register(BUFFER, 3),
    ]);
    generator.emit_branch_conditional_to(4, 2, nonempty);
    generator.output.instructions.push(Instruction::load_immediate(3, 0));
    generator.emit_branch_to(epilogue);

    generator.bind_label(nonempty);
    generator.output.instructions.extend([
        Instruction::LoadWord { d: 3, a: BUFFER, offset: plan.position_offset },
        Instruction::SubtractFromImmediate { d: 0, a: 3, immediate: plan.capacity },
        Instruction::CompareLogicalWord { a: 0, b: LENGTH },
    ]);
    generator.emit_branch_conditional_to(4, 0, unclamped);
    generator.output.instructions.extend([
        Instruction::load_immediate(ERROR, plan.overflow),
        Instruction::move_register(LENGTH, 0),
    ]);

    generator.bind_label(unclamped);
    generator.output.instructions.push(Instruction::CompareLogicalWordImmediate {
        a: LENGTH,
        immediate: 1,
    });
    generator.emit_branch_conditional_to(4, 2, bulk);
    generator.output.instructions.extend([
        Instruction::LoadByteZero { d: 0, a: 4, offset: 0 },
        Instruction::Add { d: 3, a: BUFFER, b: 3 },
        Instruction::StoreByte { s: 0, a: 3, offset: plan.data_offset },
    ]);
    generator.emit_branch_to(copied);

    generator.bind_label(bulk);
    generator.output.instructions.extend([
        Instruction::AddImmediate { d: 3, a: 3, immediate: plan.data_offset },
        Instruction::move_register(5, LENGTH),
        Instruction::Add { d: 3, a: BUFFER, b: 3 },
    ]);
    generator.record_relocation(RelocationKind::Rel24, plan.callee);
    generator.output.instructions.push(Instruction::BranchAndLink {
        target: plan.callee.to_owned(),
    });

    generator.bind_label(copied);
    generator.output.instructions.extend([
        Instruction::LoadWord { d: 0, a: BUFFER, offset: plan.position_offset },
        Instruction::move_register(3, ERROR),
        Instruction::Add { d: 0, a: 0, b: LENGTH },
        Instruction::StoreWord { s: 0, a: BUFFER, offset: plan.position_offset },
        Instruction::LoadWord { d: 0, a: BUFFER, offset: plan.position_offset },
        Instruction::StoreWord { s: 0, a: BUFFER, offset: plan.length_offset },
    ]);

    generator.bind_label(epilogue);
    generator.output.instructions.extend([
        Instruction::LoadWord { d: 0, a: 1, offset: 36 },
        Instruction::LoadWord { d: ERROR, a: 1, offset: 28 },
        Instruction::LoadWord { d: LENGTH, a: 1, offset: 24 },
        Instruction::LoadWord { d: BUFFER, a: 1, offset: 20 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
        Instruction::BranchToLinkRegister,
    ]);
}
