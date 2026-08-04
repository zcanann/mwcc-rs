//! Linkage-first issue order for a leaf-shaped variadic call.
//!
//! A small non-leaf can preserve incoming arguments without allocating saved
//! registers. Build 163 fills the formatter-address latency slot with `crclr`
//! before pushing the frame; the ordinary selector emits the condition update
//! immediately before the call. Keep this schedule separate from the
//! saved-register variadic-frame family because its prologue deliberately has
//! an incoming-register copy between `mflr` and the linkage-area store.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_linkage_first_variadic_leaf_call(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let Some((from, to)) = linkage_first_variadic_leaf_move(
            &self.output.instructions,
            &self.variadic_callees,
        ) else {
            return;
        };
        self.move_instruction_before(from, to);
    }
}

fn linkage_first_variadic_leaf_move(
    instructions: &[Instruction],
    variadic_callees: &std::collections::HashSet<String>,
) -> Option<(usize, usize)> {
    if let [
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::AddImmediateShifted { d: 3, a: 0, .. },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        },
        Instruction::AddImmediate { d: 3, a: 3, .. },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset },
        Instruction::ConditionRegisterClear { d: 6 },
        Instruction::BranchAndLink { target },
        ..
    ] = instructions
    {
        if *offset < 0 && variadic_callees.contains(target) {
            return Some((5, 3));
        }
    }
    let [
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::AddImmediate {
            d: 3,
            a: 4,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        },
        Instruction::AddImmediate { d: 4, a: 0, .. },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset },
        Instruction::ConditionRegisterClear { d: 6 },
        Instruction::BranchAndLink { target },
        ..
    ] = instructions
    else {
        return None;
    };
    (*offset < 0 && variadic_callees.contains(target)).then_some((5, 3))
}

#[cfg(test)]
mod tests {
    use super::linkage_first_variadic_leaf_move;
    use mwcc_machine_code::Instruction;
    use std::collections::HashSet;

    fn leaf_prefix(target: &str) -> Vec<Instruction> {
        vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediate {
                d: 3,
                a: 4,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -8,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: target.into(),
            },
        ]
    }

    fn format_address_prefix(target: &str) -> Vec<Instruction> {
        vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -8,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: target.into(),
            },
        ]
    }

    #[test]
    fn fills_the_leaf_formatter_address_latency_slot() {
        let variadic = HashSet::from(["format".into()]);
        assert_eq!(
            linkage_first_variadic_leaf_move(&leaf_prefix("format"), &variadic),
            Some((5, 3))
        );
    }

    #[test]
    fn fills_the_direct_format_address_latency_slot() {
        let variadic = HashSet::from(["format".into()]);
        assert_eq!(
            linkage_first_variadic_leaf_move(&format_address_prefix("format"), &variadic),
            Some((5, 3))
        );
    }

    #[test]
    fn rejects_a_nonvariadic_call() {
        let variadic = HashSet::from(["format".into()]);
        assert_eq!(
            linkage_first_variadic_leaf_move(&leaf_prefix("copy"), &variadic),
            None
        );
    }
}
