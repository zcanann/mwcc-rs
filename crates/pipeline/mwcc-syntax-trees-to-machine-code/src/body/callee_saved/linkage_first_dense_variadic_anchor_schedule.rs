//! Entry scheduling for dense writable-section frames whose first call is variadic.
//!
//! A retained incoming context and the section anchor both cross that call.
//! Selection creates independent save, anchor, and argument packets; build 163
//! copies the context before reusing incoming r3 to stage the anchor, then fills
//! the anchor-pair latency slot with `crclr`.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DenseVariadicAnchorPrefix {
    anchor_home: u8,
    context_home: u8,
}

pub(super) fn schedule_dense_variadic_anchor_prefix(generator: &mut Generator) -> bool {
    let Some(plan) = dense_variadic_anchor_prefix(&generator.output.instructions) else {
        return false;
    };

    // Initial packet:
    //   lis; addi anchor; copy context; arg3; arg4; crclr; bl
    // Measured packet:
    //   copy context; lis; crclr; addi anchor; arg3; arg4; bl
    generator.move_instruction_before(6, 4);
    generator.move_instruction_before(9, 6);

    generator.output.instructions[4] =
        Instruction::move_register(plan.context_home, Eabi::FIRST_GENERAL_ARGUMENT);
    let Instruction::AddImmediateShifted { d, .. } = &mut generator.output.instructions[5]
    else {
        unreachable!("the dense variadic anchor high half was matched")
    };
    *d = Eabi::FIRST_GENERAL_ARGUMENT;
    let Instruction::AddImmediate { a, .. } = &mut generator.output.instructions[7] else {
        unreachable!("the dense variadic anchor low half was matched")
    };
    *a = Eabi::FIRST_GENERAL_ARGUMENT;
    generator.output.instructions[8] =
        Instruction::move_register(Eabi::FIRST_GENERAL_ARGUMENT, plan.anchor_home);
    generator.output.instructions[9] = Instruction::move_register(4, plan.context_home);
    true
}

fn dense_variadic_anchor_prefix(
    instructions: &[Instruction],
) -> Option<DenseVariadicAnchorPrefix> {
    let [
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
        Instruction::StoreMultipleWord {
            s: first_saved,
            a: 1,
            ..
        },
        Instruction::AddImmediateShifted {
            d: anchor_stage,
            a: 0,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: anchor_home,
            a: anchor_low_base,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: context_home,
            a: 3,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 3,
            a: first_argument,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 4,
            a: second_argument,
            immediate: 0,
        },
        Instruction::ConditionRegisterClear { .. },
        Instruction::BranchAndLink { .. },
        ..
    ] = instructions
    else {
        return None;
    };
    (*anchor_low_base == *anchor_stage
        && *first_argument == *anchor_home
        && *second_argument == *context_home
        && anchor_home != context_home
        && *first_saved <= *anchor_home
        && *first_saved <= *context_home)
        .then_some(DenseVariadicAnchorPrefix {
            anchor_home: *anchor_home,
            context_home: *context_home,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_dense_anchor_and_context_feeding_the_first_variadic_call() {
        let instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -760,
            },
            Instruction::StoreMultipleWord {
                s: 25,
                a: 1,
                offset: 732,
            },
            Instruction::load_immediate_shifted(5, 0),
            Instruction::AddImmediate {
                d: 31,
                a: 5,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 28,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 28,
                immediate: 0,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: "report".into(),
            },
        ];

        assert_eq!(
            dense_variadic_anchor_prefix(&instructions),
            Some(DenseVariadicAnchorPrefix {
                anchor_home: 31,
                context_home: 28,
            })
        );
    }

    #[test]
    fn rejects_a_non_dense_anchor_prefix() {
        let instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 20,
            },
        ];
        assert!(dense_variadic_anchor_prefix(&instructions).is_none());
    }
}
