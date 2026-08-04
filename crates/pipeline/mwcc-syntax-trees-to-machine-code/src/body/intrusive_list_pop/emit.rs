use super::*;

pub(super) fn emit(generator: &mut Generator, plan: &IntrusiveListPop) {
    const HEAD: u8 = 3;
    const NEXT: u8 = 4;
    const NODE: u8 = 5;

    generator.output.pre_scheduled = true;
    generator.output.instructions.extend([
        Instruction::LoadWord {
            d: 0,
            a: HEAD,
            offset: 0,
        },
        Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        Instruction::move_register(NODE, 0),
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: 6,
        },
        Instruction::load_immediate(HEAD, 0),
        Instruction::BranchToLinkRegister,
        Instruction::LoadWord {
            d: NEXT,
            a: NODE,
            offset: plan.next_offset,
        },
        Instruction::load_immediate(0, 0),
        Instruction::StoreWord {
            s: NEXT,
            a: HEAD,
            offset: 0,
        },
        Instruction::move_register(HEAD, NODE),
        Instruction::StoreWord {
            s: 0,
            a: NODE,
            offset: plan.owner_offset,
        },
        Instruction::BranchToLinkRegister,
    ]);
}
