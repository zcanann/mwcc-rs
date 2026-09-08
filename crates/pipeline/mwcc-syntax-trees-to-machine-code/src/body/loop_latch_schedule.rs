//! Place a constant-bound induction comparison among independent loop steps.
//!
//! O4 loop canonicalization moves the tested induction update ahead of the
//! other post-call steps. Early schedulers compare immediately; later ones
//! issue another step first and leave the first independent step for the end.

use super::*;
use std::collections::HashSet;

impl Generator {
    pub(crate) fn schedule_loop_latch_steps(&mut self) {
        if self.behavior.optimization != mwcc_versions::Optimization::O4 {
            return;
        }
        for branch in 0..self.output.instructions.len() {
            let Some((start, order)) = latch_plan(
                &self.output.instructions,
                branch,
                self.behavior.interleave_loop_latch_steps,
            ) else {
                continue;
            };
            let inside = |index| (start..branch).contains(&index);
            // A relocation, table entry, or deferred address owns an exact
            // instruction. Do not move a packet containing any such owner.
            if self
                .output
                .relocations
                .iter()
                .any(|r| inside(r.instruction_index))
                || self
                    .output
                    .deferred_displacements
                    .iter()
                    .any(|d| inside(d.instruction_index))
                || self
                    .output
                    .jump_tables
                    .iter()
                    .any(|t| t.entries.iter().any(|e| inside(*e as usize / 4)))
            {
                continue;
            }
            crate::permute_machine_function_region(&mut self.output, start, &order);
        }
    }
}

fn latch_plan(
    instructions: &[Instruction],
    branch: usize,
    interleave: bool,
) -> Option<(usize, Vec<usize>)> {
    let Instruction::BranchConditionalForward { target, .. } = *instructions.get(branch)? else {
        return None;
    };
    let comparison = branch.checked_sub(1)?;
    let index = match instructions[comparison] {
        Instruction::CompareWordImmediate { a, .. }
        | Instruction::CompareLogicalWordImmediate { a, .. } => a,
        _ => return None,
    };
    let mut start = comparison;
    let mut registers = HashSet::new();
    let mut induction = None;
    while start > 0 {
        let Instruction::AddImmediate { d, a, immediate } = instructions[start - 1] else {
            break;
        };
        if d != a || a == 0 || immediate == 0 || !registers.insert(d) {
            return None;
        }
        start -= 1;
        if d == index {
            // Descending loops have a separate recorded-decrement owner.
            if immediate <= 0 {
                return None;
            }
            induction = Some(start);
        }
    }
    let induction = induction?;
    if registers.len() < 2
        || target >= start
        || start == 0
        || !matches!(instructions[start - 1], Instruction::BranchAndLink { .. })
    {
        return None;
    }
    // An edge into any part of the packet would bypass newly hoisted work.
    if instructions.iter().any(|instruction| match instruction {
        Instruction::Branch { target } | Instruction::BranchConditionalForward { target, .. } => {
            (start..branch).contains(target)
        }
        _ => false,
    }) {
        return None;
    }
    let mut steps: Vec<_> = (start..comparison).filter(|i| *i != induction).collect();
    let mut order = vec![induction];
    if interleave {
        steps.rotate_left(1);
        order.push(steps.remove(0));
    }
    order.push(comparison);
    order.extend(steps);
    Some((start, order.into_iter().map(|i| i - start).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet() -> Vec<Instruction> {
        vec![
            Instruction::BranchAndLink {
                target: "observe".into(),
            },
            Instruction::AddImmediate {
                d: 30,
                a: 30,
                immediate: 4,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 31,
                immediate: 8,
            },
            Instruction::AddImmediate {
                d: 29,
                a: 29,
                immediate: 1,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 29,
                immediate: 64,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 0,
            },
        ]
    }

    #[test]
    fn schedules_the_induction_and_comparison_by_issue_policy() {
        assert_eq!(latch_plan(&packet(), 5, false), Some((1, vec![2, 3, 0, 1])));
        assert_eq!(latch_plan(&packet(), 5, true), Some((1, vec![2, 1, 3, 0])));
    }

    #[test]
    fn rejects_dependent_steps_and_alternate_packet_entries() {
        let mut code = packet();
        code[2] = Instruction::AddImmediate {
            d: 31,
            a: 30,
            immediate: 8,
        };
        assert!(latch_plan(&code, 5, true).is_none());
        let mut code = packet();
        code.push(Instruction::Branch { target: 2 });
        assert!(latch_plan(&code, 5, false).is_none());
        let mut code = packet();
        code[2] = code[1].clone();
        assert!(latch_plan(&code, 5, false).is_none());
    }
}
