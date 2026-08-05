//! Predecrement emission for a semantically verified bounded byte-buffer read.

#[allow(unused_imports)]
use super::super::*;
use super::ReadPlan;

pub(super) fn emit(generator: &mut Generator, plan: &ReadPlan<'_>) {
    const ERROR: u8 = 31;
    const REQUESTED: u8 = 30;
    const BUFFER: u8 = 29;
    let nonempty = generator.fresh_label();
    let unclamped = generator.fresh_label();
    let epilogue = generator.fresh_label();

    generator.non_leaf = true;
    generator.frame_size = 32;
    generator.callee_saved = vec![ERROR, REQUESTED, BUFFER];
    generator.output.pre_scheduled = true;
    generator.owns_link_register_schedule = true;
    generator.output.instructions.extend([
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 36 },
        Instruction::StoreWord { s: ERROR, a: 1, offset: 28 },
        Instruction::load_immediate(ERROR, 0),
        Instruction::StoreWord { s: REQUESTED, a: 1, offset: 24 },
        Instruction::OrRecord { a: REQUESTED, s: 5, b: 5 },
        Instruction::StoreWord { s: BUFFER, a: 1, offset: 20 },
        Instruction::move_register(BUFFER, 3),
        Instruction::move_register(3, 4),
    ]);
    generator.emit_branch_conditional_to(4, 2, nonempty);
    generator.output.instructions.push(Instruction::load_immediate(3, 0));
    generator.emit_branch_to(epilogue);

    generator.bind_label(nonempty);
    generator.output.instructions.extend([
        Instruction::LoadWord { d: 4, a: BUFFER, offset: plan.position_offset },
        Instruction::LoadWord { d: 0, a: BUFFER, offset: plan.length_offset },
        Instruction::SubtractFrom { d: 0, a: 4, b: 0 },
        Instruction::CompareLogicalWord { a: REQUESTED, b: 0 },
    ]);
    generator.emit_branch_conditional_to(4, 1, unclamped);
    generator.output.instructions.extend([
        Instruction::load_immediate(ERROR, plan.overflow),
        Instruction::move_register(REQUESTED, 0),
    ]);

    generator.bind_label(unclamped);
    generator.output.instructions.extend([
        Instruction::AddImmediate { d: 4, a: 4, immediate: plan.data_offset },
        Instruction::move_register(5, REQUESTED),
        Instruction::Add { d: 4, a: BUFFER, b: 4 },
    ]);
    generator.record_relocation(RelocationKind::Rel24, plan.callee);
    generator.output.instructions.push(Instruction::BranchAndLink {
        target: plan.callee.to_owned(),
    });
    generator.output.instructions.extend([
        Instruction::LoadWord { d: 0, a: BUFFER, offset: plan.position_offset },
        Instruction::move_register(3, ERROR),
        Instruction::Add { d: 0, a: 0, b: REQUESTED },
        Instruction::StoreWord { s: 0, a: BUFFER, offset: plan.position_offset },
    ]);

    generator.bind_label(epilogue);
    generator.output.instructions.extend([
        Instruction::LoadWord { d: 0, a: 1, offset: 36 },
        Instruction::LoadWord { d: ERROR, a: 1, offset: 28 },
        Instruction::LoadWord { d: REQUESTED, a: 1, offset: 24 },
        Instruction::LoadWord { d: BUFFER, a: 1, offset: 20 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
        Instruction::BranchToLinkRegister,
    ]);
}
