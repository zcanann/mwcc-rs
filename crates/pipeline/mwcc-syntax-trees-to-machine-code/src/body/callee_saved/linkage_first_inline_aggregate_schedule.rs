//! Physical reuse schedule for linkage-first aggregate-inline frames.
//!
//! Once the frame planner proves that leading aggregate images and later
//! inline values share one retained lane, several address temporaries become
//! redundant. This pass consumes only allocation-visible instruction shapes:
//! a retained guard receiver, a split aggregate zero initializer, and a member
//! pair feeding one call.

use super::*;

impl Generator {
    pub(crate) fn schedule_linkage_first_inline_aggregate_frame(&mut self) {
        if !self.linkage_first_inline_aggregate_frame {
            return;
        }
        reuse_guard_receiver(self);
        fold_split_aggregate_initializer(self);
        reuse_call_member_base(self);
    }
}

fn reuse_guard_receiver(generator: &mut Generator) {
    let Some(start) = generator.output.instructions.windows(7).position(|window| {
        matches!(window, [
            Instruction::LoadWord { d: receiver, a: state, offset: 12 },
            Instruction::LoadWord { d: 0, a: member_receiver, .. },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadWord { d: 3, a: reload_state, offset: 12 },
            Instruction::AddImmediate { d: 0, a: 0, .. },
            Instruction::StoreByte { a: 3, .. },
        ] if receiver == member_receiver && state == reload_state)
    }) else {
        return;
    };
    let Instruction::LoadWord { d: receiver, .. } = generator.output.instructions[start] else {
        unreachable!()
    };
    let Instruction::StoreByte { a, .. } = &mut generator.output.instructions[start + 6] else {
        unreachable!()
    };
    *a = receiver;
    crate::remove_instruction_retargeting_to_next(generator, start + 4);
}

fn fold_split_aggregate_initializer(generator: &mut Generator) {
    let start = generator.output.instructions.windows(13).position(|window| {
        matches!(window, [
            Instruction::LoadFloatSingle { d: value, .. },
            Instruction::StoreFloatSingle { s: first, a: 1, .. },
            Instruction::StoreFloatSingle { s: second, a: 1, .. },
            Instruction::StoreFloatSingle { s: third, a: 1, offset: aggregate },
            Instruction::AddImmediate { d: address, a: 1, immediate },
            Instruction::AddImmediate { d: nested, a: address_use, immediate: 12 },
            Instruction::StoreFloatSingle { s: fourth, a: nested_first, offset: 8 },
            Instruction::StoreFloatSingle { s: fifth, a: nested_second, offset: 4 },
            Instruction::StoreFloatSingle { s: sixth, a: nested_third, offset: 0 },
            Instruction::LoadWord { .. },
            Instruction::LoadWord { .. },
            Instruction::AddImmediate { a: 1, immediate: argument, .. },
            Instruction::BranchAndLink { .. },
        ] if value == first
            && value == second
            && value == third
            && value == fourth
            && value == fifth
            && value == sixth
            && aggregate == immediate
            && address == address_use
            && nested == nested_first
            && nested == nested_second
            && nested == nested_third
            && aggregate == argument)
    });
    let Some(start) = start else {
        return;
    };
    let Instruction::AddImmediate {
        immediate: aggregate,
        ..
    } = generator.output.instructions[start + 4]
    else {
        unreachable!()
    };
    for (relative, offset) in [(6, 20), (7, 16), (8, 12)] {
        let Instruction::StoreFloatSingle {
            a,
            offset: displacement,
            ..
        } = &mut generator.output.instructions[start + relative]
        else {
            unreachable!()
        };
        *a = 1;
        *displacement = aggregate.saturating_add(offset);
    }
    crate::remove_instruction_retargeting_to_next(generator, start + 5);
    crate::remove_instruction_retargeting_to_next(generator, start + 4);
    crate::move_instruction_before_retargeting(generator, start + 9, start + 1);
}

fn reuse_call_member_base(generator: &mut Generator) {
    let Some(start) = generator.output.instructions.windows(4).position(|window| {
        matches!(window, [
            Instruction::LoadWord { d: base, a: state, offset: 12 },
            Instruction::AddImmediate { d: argument, a: base_use, .. },
            Instruction::LoadWord { d: duplicate, a: duplicate_state, offset: 12 },
            Instruction::LoadFloatSingle { a: float_base, .. },
        ] if base == argument
            && base == base_use
            && state == duplicate_state
            && duplicate == float_base)
    }) else {
        return;
    };
    let Instruction::LoadWord { d: base, .. } = generator.output.instructions[start] else {
        unreachable!()
    };
    let Instruction::LoadFloatSingle { a, .. } = &mut generator.output.instructions[start + 3]
    else {
        unreachable!()
    };
    *a = base;
    crate::remove_instruction_retargeting_to_next(generator, start + 2);
    crate::move_instruction_before_retargeting(generator, start + 2, start + 1);
}
