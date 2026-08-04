//! Canonical two-case control flow for optimized structured switches.
//!
//! For a `0`/`1` switch with no default body, build 163 spells the final range
//! check as `bge case0; b join`. The generic lowering uses the equivalent
//! inverted `blt join`; expanding the explicit edge keeps each case entry at
//! MWCC's measured block boundary.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn expand_structured_small_switch_control(&mut self) -> bool {
        if !self.behavior.schedule_latency_slots {
            return false;
        }
        let Some(plan) = plan(&self.output.instructions) else {
            return false;
        };
        self.output.instructions[plan.conditional] = Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target: plan.case_start,
        };
        crate::insert_instruction_retargeting(
            self,
            plan.case_start,
            Instruction::Branch { target: plan.join },
        );
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    conditional: usize,
    case_start: usize,
    join: usize,
}

fn plan(instructions: &[Instruction]) -> Option<Plan> {
    instructions.windows(5).enumerate().find_map(|(start, window)| {
        let [
            Instruction::CompareWordImmediate { a: selector, immediate: 1 },
            Instruction::BranchConditionalForward { .. },
            Instruction::BranchConditionalForward { .. },
            Instruction::CompareWordImmediate { a: zero_selector, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: join,
            },
        ] = window
        else {
            return None;
        };
        let body = instructions.get(start + 5..*join)?;
        (*selector == *zero_selector
            && body
                .iter()
                .filter(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
                .count()
                == 4
            && body
                .iter()
                .any(|instruction| matches!(instruction, Instruction::LoadByteZero { .. })))
        .then_some(Plan {
            conditional: start + 4,
            case_start: start + 5,
            join: *join,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_inverted_zero_case_range_check() {
        let mut instructions = vec![
            Instruction::CompareWordImmediate { a: 0, immediate: 1 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 12 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 20 },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 20 },
        ];
        instructions.push(Instruction::LoadByteZero { d: 5, a: 30, offset: 184 });
        for callee in ["first", "second", "third", "fourth"] {
            instructions.push(Instruction::BranchAndLink { target: callee.into() });
        }
        instructions.resize(
            20,
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
        );
        assert_eq!(
            plan(&instructions),
            Some(Plan { conditional: 4, case_start: 5, join: 20 })
        );
    }
}
