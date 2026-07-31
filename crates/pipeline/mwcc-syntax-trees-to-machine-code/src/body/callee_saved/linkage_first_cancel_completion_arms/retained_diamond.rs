//! Retained-object scheduling for a cancellation Boolean diamond.
//!
//! The progress comparison and the cancellation transaction share one object.
//! MWCC keeps it in `r30`, then materializes the cancellation result as an
//! explicit Boolean join before the final ordinary completion arm.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RetainedCancelDiamond {
    start: usize,
}

fn external_target_at<'a>(
    relocations: &'a [mwcc_machine_code::Relocation],
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&'a str> {
    relocations.iter().find_map(|relocation| {
        (relocation.instruction_index == instruction_index && relocation.kind == kind)
            .then(|| match &relocation.target {
                mwcc_machine_code::RelocationTarget::External(target) => Some(target.as_str()),
                _ => None,
            })
            .flatten()
    })
}

fn recognize_at(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    displacements: &[mwcc_machine_code::DataSectionDisplacement],
    start: usize,
) -> Option<RetainedCancelDiamond> {
    let [Instruction::LoadWord {
        d: object, a: 0, ..
    }, Instruction::LoadWord {
        d: progress,
        a: progress_object,
        offset: 32,
    }, Instruction::LoadWord {
        d: expected,
        a: expected_object,
        offset: 20,
    }, Instruction::CompareLogicalWord {
        a: compared_progress,
        b: compared_expected,
    }, Instruction::BranchConditionalForward { target: error, .. }, Instruction::LoadWord {
        d: canceling, a: 0, ..
    }, Instruction::CompareLogicalWordImmediate {
        a: compared_canceling,
        immediate: 0,
    }, Instruction::BranchConditionalForward {
        target: final_completion,
        ..
    }] = instructions.get(start..start + 8)?
    else {
        return None;
    };
    if progress_object != object
        || expected_object != object
        || compared_progress != progress
        || compared_expected != expected
        || compared_canceling != canceling
        || error != &(start + 45)
        || final_completion != &(start + 32)
        || external_target_at(relocations, start, RelocationKind::EmbSda21) != Some("executing")
        || external_target_at(relocations, start + 5, RelocationKind::EmbSda21) != Some("Canceling")
    {
        return None;
    }

    let [Instruction::AddImmediate {
        d: zero,
        a: 0,
        immediate: 0,
    }, Instruction::StoreWord {
        s: resume_zero,
        a: 0,
        ..
    }, Instruction::LoadWord {
        d: retained, a: 0, ..
    }, Instruction::StoreWord {
        s: cancel_zero,
        a: 0,
        ..
    }, Instruction::AddImmediate {
        d: replacement,
        a: anchor,
        ..
    }, Instruction::StoreWord {
        s: published, a: 0, ..
    }, Instruction::AddImmediate {
        d: state,
        a: 0,
        immediate: 10,
    }, Instruction::StoreWord {
        s: stored_state,
        a: state_object,
        offset: 12,
    }, Instruction::LoadWord {
        d: callback,
        a: callback_object,
        offset: 40,
    }, Instruction::CompareLogicalWordImmediate {
        a: compared_callback,
        immediate: 0,
    }, Instruction::BranchConditionalForward {
        target: first_join, ..
    }, Instruction::MoveToLinkRegister { s: linked_callback }, Instruction::AddImmediate {
        d: first_argument,
        a: argument_object,
        immediate: 0,
    }, Instruction::AddImmediate {
        d: first_result,
        a: 0,
        immediate: -3,
    }, Instruction::BranchToLinkRegisterAndLink, Instruction::LoadWord {
        d: cancel_callback,
        a: 0,
        ..
    }, Instruction::CompareLogicalWordImmediate {
        a: compared_cancel_callback,
        immediate: 0,
    }, Instruction::BranchConditionalForward {
        target: second_join,
        ..
    }, Instruction::MoveToLinkRegister {
        s: linked_cancel_callback,
    }, Instruction::AddImmediate {
        d: second_argument,
        a: second_argument_object,
        immediate: 0,
    }, Instruction::AddImmediate {
        d: second_result,
        a: 0,
        immediate: 0,
    }, Instruction::BranchToLinkRegisterAndLink, Instruction::BranchAndLink { .. }, Instruction::Branch { target: true_exit }] =
        instructions.get(start + 8..start + 32)?
    else {
        return None;
    };
    if zero != resume_zero
        || zero != cancel_zero
        || anchor == &0
        || replacement != published
        || state != stored_state
        || retained != state_object
        || retained != callback_object
        || callback != compared_callback
        || callback != linked_callback
        || first_argument != &4
        || argument_object != retained
        || first_result != &3
        || cancel_callback != compared_cancel_callback
        || cancel_callback != linked_cancel_callback
        || second_argument != &4
        || second_argument_object != retained
        || second_result != &3
        || first_join != &(start + 23)
        || second_join != &(start + 30)
        || true_exit != &(start + 48)
        || external_target_at(relocations, start + 9, RelocationKind::EmbSda21)
            != Some("ResumeFromHere")
        || external_target_at(relocations, start + 10, RelocationKind::EmbSda21)
            != Some("executing")
        || external_target_at(relocations, start + 11, RelocationKind::EmbSda21)
            != Some("Canceling")
        || external_target_at(relocations, start + 13, RelocationKind::EmbSda21)
            != Some("executing")
        || external_target_at(relocations, start + 23, RelocationKind::EmbSda21)
            != Some("CancelCallback")
        || external_target_at(relocations, start + 30, RelocationKind::Rel24) != Some("stateReady")
        || !displacements
            .iter()
            .any(|displacement| displacement.instruction_index == start + 12)
    {
        return None;
    }

    let [Instruction::LoadWord {
        d: final_object,
        a: 0,
        ..
    }, Instruction::AddImmediate {
        d: final_replacement,
        a: final_anchor,
        ..
    }, Instruction::AddImmediate {
        d: final_zero,
        a: 0,
        immediate: 0,
    }, Instruction::StoreWord {
        s: final_published,
        a: 0,
        ..
    }, Instruction::StoreWord {
        s: final_stored_zero,
        a: final_state_object,
        offset: 12,
    }, Instruction::LoadWord {
        d: final_callback,
        a: final_callback_object,
        offset: 40,
    }, Instruction::CompareLogicalWordImmediate {
        a: final_compared_callback,
        immediate: 0,
    }, Instruction::BranchConditionalForward {
        target: final_callback_join,
        ..
    }, Instruction::MoveToLinkRegister {
        s: final_linked_callback,
    }, Instruction::LoadWord {
        d: final_argument,
        a: final_argument_object,
        offset: 32,
    }, Instruction::BranchToLinkRegisterAndLink, Instruction::BranchAndLink { .. }, Instruction::Branch { target: final_exit }, Instruction::AddImmediateShifted {
        d: error_high,
        a: 0,
        ..
    }, Instruction::AddImmediate {
        d: error_address,
        a: error_high_source,
        ..
    }, Instruction::BranchAndLink { .. }] = instructions.get(start + 32..start + 48)?
    else {
        return None;
    };
    if final_anchor == &0
        || final_replacement != final_published
        || final_zero != final_stored_zero
        || final_object != final_state_object
        || final_object != final_callback_object
        || final_callback != final_compared_callback
        || final_callback != final_linked_callback
        || final_argument != &3
        || final_argument_object != final_object
        || final_callback_join != &(start + 43)
        || final_exit != &(start + 48)
        || error_high != error_address
        || error_high != error_high_source
        || external_target_at(relocations, start + 32, RelocationKind::EmbSda21)
            != Some("executing")
        || external_target_at(relocations, start + 35, RelocationKind::EmbSda21)
            != Some("executing")
        || external_target_at(relocations, start + 43, RelocationKind::Rel24) != Some("stateReady")
        || external_target_at(relocations, start + 45, RelocationKind::Addr16Ha)
            != Some("cbForStateGettingError")
        || external_target_at(relocations, start + 46, RelocationKind::Addr16Lo)
            != Some("cbForStateGettingError")
        || external_target_at(relocations, start + 47, RelocationKind::Rel24)
            != Some("DVDLowRequestError")
        || !displacements
            .iter()
            .any(|displacement| displacement.instruction_index == start + 33)
    {
        return None;
    }

    Some(RetainedCancelDiamond { start })
}

fn apply(generator: &mut Generator, plan: RetainedCancelDiamond) {
    let base = plan.start;
    let Instruction::LoadWord { d, .. } = &mut generator.output.instructions[base] else {
        unreachable!("validated cancellation object load changed form")
    };
    *d = 30;
    for index in [base + 1, base + 2] {
        let Instruction::LoadWord { a, .. } = &mut generator.output.instructions[index] else {
            unreachable!("validated progress member load changed form")
        };
        *a = 30;
    }

    crate::remove_instruction_retargeting_to_next(generator, base + 10);
    crate::move_instruction_before_retargeting(generator, base + 11, base + 10);
    crate::move_instruction_before_retargeting(generator, base + 13, base + 11);
    let Instruction::AddImmediate { d, .. } = &mut generator.output.instructions[base + 8] else {
        unreachable!("validated cancellation zero changed form")
    };
    *d = 4;
    for index in [base + 9, base + 12] {
        let Instruction::StoreWord { s, .. } = &mut generator.output.instructions[index] else {
            unreachable!("validated cancellation zero store changed form")
        };
        *s = 4;
    }
    let Instruction::AddImmediate { d, .. } = &mut generator.output.instructions[base + 10] else {
        unreachable!("validated cancellation replacement changed form")
    };
    *d = 3;
    let Instruction::StoreWord { s, .. } = &mut generator.output.instructions[base + 13] else {
        unreachable!("validated cancellation publication changed form")
    };
    *s = 3;

    generator.output.instructions[base + 30] = Instruction::load_immediate(0, 1);
    crate::insert_instruction_retargeting(generator, base + 31, Instruction::Branch { target: 0 });
    crate::insert_instruction_retargeting(generator, base + 32, Instruction::load_immediate(0, 0));
    crate::insert_instruction_retargeting(
        generator,
        base + 33,
        Instruction::CompareWordImmediate { a: 0, immediate: 0 },
    );
    crate::insert_instruction_retargeting(
        generator,
        base + 34,
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: 0,
        },
    );
    let Instruction::BranchConditionalForward { target, .. } =
        &mut generator.output.instructions[base + 7]
    else {
        unreachable!("validated cancellation false edge changed form")
    };
    *target = base + 32;
    let Instruction::Branch { target } = &mut generator.output.instructions[base + 31] else {
        unreachable!("inserted cancellation Boolean join changed form")
    };
    *target = base + 33;
    let Instruction::BranchConditionalForward { target, .. } =
        &mut generator.output.instructions[base + 34]
    else {
        unreachable!("inserted cancellation Boolean exit changed form")
    };
    *target = base + 51;

    let Instruction::AddImmediate { d, .. } = &mut generator.output.instructions[base + 36] else {
        unreachable!("validated final replacement changed form")
    };
    *d = 3;
    let Instruction::StoreWord { s, .. } = &mut generator.output.instructions[base + 38] else {
        unreachable!("validated final publication changed form")
    };
    *s = 3;
    crate::move_instruction_before_retargeting(generator, base + 44, base + 43);

    debug_assert!(matches!(
        generator.output.instructions[base + 7],
        Instruction::BranchConditionalForward { target, .. } if target == base + 32
    ));
}

pub(super) fn schedule(generator: &mut Generator) {
    let mut start = 0;
    while start + 48 <= generator.output.instructions.len() {
        let Some(plan) = recognize_at(
            &generator.output.instructions,
            &generator.output.relocations,
            &generator.output.data_section_displacements,
            start,
        ) else {
            start += 1;
            continue;
        };
        apply(generator, plan);
        start += 51;
    }
}
