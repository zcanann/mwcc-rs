use super::*;

pub(super) fn emit(generator: &mut Generator, plan: &FixedListBankTransfer<'_>) {
    const SOURCE: u8 = 29;
    const DESTINATION: u8 = 30;
    const OBJECT: u8 = 31;
    generator.non_leaf = true;
    generator.frame_size = 40;
    generator.callee_saved = vec![OBJECT, DESTINATION, SOURCE];
    generator.output.pre_scheduled = true;
    generator.owns_link_register_schedule = true;

    generator.output.instructions.extend([
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 4 },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -40 },
        Instruction::StoreMultipleWord { s: SOURCE, a: 1, offset: 28 },
        Instruction::AddImmediate { d: SOURCE, a: 3, immediate: 0 },
        Instruction::AddImmediate { d: DESTINATION, a: 4, immediate: 0 },
    ]);

    for (bank, offset) in plan.list_offsets.into_iter().enumerate() {
        let start = 6 + bank * 9;
        let next = start + 9;
        generator.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: SOURCE,
            immediate: offset,
        });
        emit_call(generator, plan.take);
        generator.output.instructions.extend([
            Instruction::OrRecord { a: OBJECT, s: 3, b: 3 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: next,
            },
            Instruction::AddImmediate {
                d: 3,
                a: DESTINATION,
                immediate: offset,
            },
            Instruction::AddImmediate { d: 4, a: OBJECT, immediate: 0 },
        ]);
        emit_call(generator, plan.append);
        generator.output.instructions.extend([
            Instruction::StoreWord {
                s: DESTINATION,
                a: OBJECT,
                offset: plan.object_owner_offset,
            },
            Instruction::Branch { target: start },
        ]);
    }

    let [first_count, second_count] = plan.count_offsets;
    generator.output.instructions.extend([
        Instruction::LoadWord { d: 4, a: DESTINATION, offset: first_count },
        Instruction::load_immediate(5, 0),
        Instruction::LoadWord { d: 0, a: SOURCE, offset: first_count },
        Instruction::load_immediate(3, 0),
        Instruction::Add { d: 0, a: 4, b: 0 },
        Instruction::StoreWord { s: 0, a: DESTINATION, offset: first_count },
        Instruction::StoreWord { s: 5, a: SOURCE, offset: first_count },
        Instruction::LoadWord { d: 4, a: DESTINATION, offset: second_count },
        Instruction::LoadWord { d: 0, a: SOURCE, offset: second_count },
        Instruction::Add { d: 0, a: 4, b: 0 },
        Instruction::StoreWord { s: 0, a: DESTINATION, offset: second_count },
        Instruction::StoreWord { s: 5, a: SOURCE, offset: second_count },
        Instruction::LoadWord { d: 0, a: 1, offset: 44 },
        Instruction::LoadMultipleWord { d: SOURCE, a: 1, offset: 28 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 40 },
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
