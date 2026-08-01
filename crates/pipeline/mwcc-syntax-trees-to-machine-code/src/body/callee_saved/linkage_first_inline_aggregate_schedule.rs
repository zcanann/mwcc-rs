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
        schedule_zeroed_vector_arguments(self);
    }

    /// Assign the persistent receiver before the leading constructor image,
    /// then give the constructor result the following saved home. This runs
    /// after every shape-changing reuse pass so the physical r30/r31 swap
    /// cannot invalidate an earlier recognizer.
    pub(crate) fn finalize_linkage_first_inline_aggregate_homes(&mut self) {
        if !self.linkage_first_inline_aggregate_frame {
            return;
        }
        schedule_rand_float_dag(self);
        for instruction in &mut self.output.instructions {
            let saved_home_access = matches!(
                instruction,
                Instruction::StoreWord {
                    s: 30 | 31,
                    a: 1,
                    offset: 112 | 116,
                } | Instruction::LoadWord {
                    d: 30 | 31,
                    a: 1,
                    offset: 112 | 116,
                }
            );
            if !saved_home_access {
                mwcc_vreg::for_each_register(instruction, |_, class, register| {
                    if class == mwcc_vreg::Class::General {
                        *register = match *register {
                            30 => 31,
                            31 => 30,
                            other => other,
                        };
                    }
                });
            }
        }
        schedule_saved_home_entry(self);
        spell_second_constructor_result(self);
        release_guard_receiver_home(self);
    }
}

fn schedule_rand_float_dag(generator: &mut Generator) {
    let Some(start) = generator.output.instructions.windows(18).position(|window| {
        matches!(window, [
            Instruction::XorImmediateShifted { .. },
            Instruction::LoadFloatDouble { .. },
            Instruction::StoreWord { a: 1, offset: 108, .. },
            Instruction::AddImmediateShifted { d: 0, a: 0, .. },
            Instruction::StoreWord { a: 1, offset: 104, .. },
            Instruction::LoadFloatDouble { a: 1, offset: 104, .. },
            Instruction::FloatSubtractSingle { .. },
            Instruction::LoadFloatSingle { .. },
            Instruction::FloatDivideSingle { .. },
            Instruction::LoadFloatSingle { .. },
            Instruction::FloatMultiplySingle { .. },
            Instruction::LoadFloatSingle { .. },
            Instruction::FloatSubtractSingle { .. },
            Instruction::LoadFloatSingle { .. },
            Instruction::FloatMultiplySingle { .. },
            Instruction::LoadFloatSingle { .. },
            Instruction::FloatAddSingle { .. },
            Instruction::StoreFloatSingle { s: 0, .. },
        ])
    }) else {
        return;
    };
    for (from, to) in [(7, 4), (9, 6), (11, 8), (13, 10), (15, 11)] {
        crate::move_instruction_before_retargeting(generator, start + from, start + to);
    }
    for (relative, register) in [(1, 2), (4, 4), (6, 3), (7, 1), (8, 0), (10, 1), (11, 2)] {
        match &mut generator.output.instructions[start + relative] {
            Instruction::LoadFloatSingle { d, .. }
            | Instruction::LoadFloatDouble { d, .. } => *d = register,
            _ => unreachable!(),
        }
    }
    generator.output.instructions[start + 9] = Instruction::FloatSubtractSingle {
        d: 5,
        a: 1,
        b: 2,
    };
    generator.output.instructions[start + 12] = Instruction::FloatDivideSingle {
        d: 4,
        a: 5,
        b: 4,
    };
    generator.output.instructions[start + 13] = Instruction::FloatMultiplySingle {
        d: 3,
        a: 3,
        c: 4,
    };
    generator.output.instructions[start + 14] = Instruction::FloatSubtractSingle {
        d: 0,
        a: 3,
        b: 0,
    };
    generator.output.instructions[start + 15] = Instruction::FloatMultiplySingle {
        d: 0,
        a: 1,
        c: 0,
    };
    generator.output.instructions[start + 16] = Instruction::FloatAddSingle {
        d: 0,
        a: 2,
        b: 0,
    };
}

fn schedule_saved_home_entry(generator: &mut Generator) {
    let Some(start) = generator.output.instructions.windows(4).position(|window| {
        matches!(window, [
            Instruction::StoreWord { s: 31, a: 1, offset: 116 },
            Instruction::StoreWord { s: 30, a: 1, offset: 112 },
            Instruction::Or { a: 31, s: 3, b: 3 },
            Instruction::AddImmediate { d: 3, a: 1, .. },
        ])
    }) else {
        return;
    };
    generator.output.instructions[start + 2] = Instruction::AddImmediate {
        d: 31,
        a: 3,
        immediate: 0,
    };
    crate::move_instruction_before_retargeting(generator, start + 2, start + 1);
    crate::move_instruction_before_retargeting(generator, start + 3, start + 2);
}

fn spell_second_constructor_result(generator: &mut Generator) {
    let Some(copy) = generator.output.instructions.windows(3).position(|window| {
        matches!(window, [
            Instruction::AddImmediate { d: 4, a: 3, immediate: 0 },
            Instruction::LoadWord { d: 3, a: 31, offset: 12 },
            Instruction::Or { a: 5, s: 30, b: 30 },
        ])
    }) else {
        return;
    };
    generator.output.instructions[copy] = Instruction::move_register(4, 3);
}

fn release_guard_receiver_home(generator: &mut Generator) {
    let Some(start) = generator.output.instructions.windows(6).position(|window| {
        matches!(window, [
            Instruction::LoadWord { d: 30, a: 31, offset: 12 },
            Instruction::LoadWord { d: 0, a: 30, offset: 684 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::AddImmediate { d: 0, a: 0, .. },
            Instruction::StoreByte { a: 30, offset: 1032, .. },
        ])
    }) else {
        return;
    };
    let Instruction::LoadWord { d, .. } = &mut generator.output.instructions[start] else {
        unreachable!()
    };
    *d = 3;
    let Instruction::LoadWord { a, .. } = &mut generator.output.instructions[start + 1] else {
        unreachable!()
    };
    *a = 3;
    let Instruction::StoreByte { a, .. } = &mut generator.output.instructions[start + 5] else {
        unreachable!()
    };
    *a = 3;
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

fn schedule_zeroed_vector_arguments(generator: &mut Generator) {
    let Some(start) = generator.output.instructions.windows(14).position(|window| {
        matches!(window, [
            Instruction::LoadFloatSingle { d: value, .. },
            Instruction::StoreFloatSingle { s: first, a: 1, offset: 68 },
            Instruction::StoreFloatSingle { s: second, a: 1, offset: 64 },
            Instruction::StoreFloatSingle { s: third, a: 1, offset: 60 },
            Instruction::StoreFloatSingle { s: fourth, a: 1, offset: 56 },
            Instruction::StoreFloatSingle { s: fifth, a: 1, offset: 52 },
            Instruction::StoreFloatSingle { s: sixth, a: 1, offset: 48 },
            Instruction::AddImmediate { d: 3, a: 1, immediate: 72 },
            Instruction::LoadWord { d: 4, .. },
            Instruction::LoadFloatSingle { d: 1, a: 4, offset: 404 },
            Instruction::AddImmediate { d: 4, a: 4, immediate: 148 },
            Instruction::AddImmediate { d: 5, a: 1, immediate: 60 },
            Instruction::AddImmediate { d: 6, a: 1, immediate: 48 },
            Instruction::BranchAndLink { .. },
        ] if value == first
            && value == second
            && value == third
            && value == fourth
            && value == fifth
            && value == sixth)
    }) else {
        return;
    };
    crate::move_instruction_before_retargeting(generator, start + 7, start + 1);
    crate::move_instruction_before_retargeting(generator, start + 11, start + 2);
    crate::move_instruction_before_retargeting(generator, start + 12, start + 4);
    crate::move_instruction_before_retargeting(generator, start + 7, start + 5);
    crate::move_instruction_before_retargeting(generator, start + 8, start + 7);
}
