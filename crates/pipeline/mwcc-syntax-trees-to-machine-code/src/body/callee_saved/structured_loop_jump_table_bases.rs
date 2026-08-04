//! Loop-invariant jump-table addresses in saved GPRs.
//!
//! Dense switches normally materialize their table address immediately before
//! dispatch. In a call-making counted loop, MWCC instead builds each address in
//! the preheader and retains it in a saved register. This scheduler discovers
//! that lifetime from relocations and the loop backedge after structured
//! lowering, while the enclosing home layout supplies the physical preferences.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::{MachineFunction, RelocationTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TableBase {
    high: usize,
    low: usize,
    load: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Plan {
    insertion: usize,
    tables: Vec<TableBase>,
}

impl Generator {
    pub(crate) fn hoist_structured_loop_jump_table_bases(
        &mut self,
        retained_homes: &[u8],
    ) -> bool {
        let Some(plan) = plan(&self.output, retained_homes.len()) else {
            return false;
        };

        for (table, retained) in plan.tables.iter().zip(retained_homes) {
            let old = match self.output.instructions[table.high] {
                Instruction::AddImmediateShifted { d, .. } => d,
                _ => unreachable!("jump-table high half changed after recognition"),
            };
            let Instruction::AddImmediateShifted { d, .. } =
                &mut self.output.instructions[table.high]
            else {
                unreachable!("jump-table high half changed after recognition")
            };
            *d = *retained;
            let Instruction::AddImmediate { d, a, .. } =
                &mut self.output.instructions[table.low]
            else {
                unreachable!("jump-table low half changed after recognition")
            };
            *d = *retained;
            *a = *retained;
            let Instruction::LoadWordIndexed { a, .. } =
                &mut self.output.instructions[table.load]
            else {
                unreachable!("jump-table indexed load changed after recognition")
            };
            debug_assert_eq!(*a, old);
            *a = *retained;
        }

        // Moving an earlier pair before the loop does not change the indices
        // of later pairs: removal and insertion occur entirely before them.
        let mut destination = plan.insertion;
        for table in &plan.tables {
            crate::move_instruction_before_retargeting(self, table.high, destination);
            destination += 1;
            crate::move_instruction_before_retargeting(self, table.low, destination);
            destination += 1;
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
        true
    }
}

pub(super) fn needs_retained_homes(function: &Function) -> bool {
    function.statements.iter().any(loop_needs_retained_homes)
}

fn loop_needs_retained_homes(statement: &Statement) -> bool {
    match statement {
        Statement::Loop { body, .. } => {
            dense_switch_count(body) >= 2
                && body.iter().any(crate::analysis::statement_has_call)
        }
        Statement::If {
            then_body,
            else_body,
            ..
        } => then_body.iter().any(loop_needs_retained_homes)
            || else_body.iter().any(loop_needs_retained_homes),
        _ => false,
    }
}

fn dense_switch_count(statements: &[Statement]) -> usize {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::Switch { arms, default, .. }
                if super::structured_switch_lowering::is_dense_structured_switch(arms) =>
            {
                1 + arms
                    .iter()
                    .map(|arm| match &arm.body {
                        mwcc_syntax_trees::ArmBody::Statements(body) => {
                            dense_switch_count(body)
                        }
                        mwcc_syntax_trees::ArmBody::Return(_) => 0,
                    })
                    .sum::<usize>()
                    + default.as_ref().map_or(0, |body| match body {
                        mwcc_syntax_trees::ArmBody::Statements(body) => {
                            dense_switch_count(body)
                        }
                        mwcc_syntax_trees::ArmBody::Return(_) => 0,
                    })
            }
            Statement::If {
                then_body,
                else_body,
                ..
            } => dense_switch_count(then_body) + dense_switch_count(else_body),
            Statement::Loop { body, .. } => dense_switch_count(body),
            _ => 0,
        })
        .sum()
}

fn plan(output: &MachineFunction, available_homes: usize) -> Option<Plan> {
    if available_homes < 2 {
        return None;
    }
    let bounds = super::structured_counted_call_loop_shape::find(output)?;
    loop_plan(
        output,
        bounds.preheader,
        bounds.head,
        bounds.backedge,
        available_homes,
    )
}

fn loop_plan(
    output: &MachineFunction,
    insertion: usize,
    loop_head: usize,
    backedge: usize,
    available_homes: usize,
) -> Option<Plan> {
    let mut highs: Vec<(usize, usize)> = output
        .relocations
        .iter()
        .filter_map(|relocation| {
            (relocation.kind == RelocationKind::Addr16Ha
                && (loop_head..backedge).contains(&relocation.instruction_index))
            .then(|| {
                jump_table_index(&relocation.target)
                    .map(|table| (relocation.instruction_index, table))
            })
            .flatten()
        })
        .collect();
    highs.sort_unstable_by_key(|(instruction, _)| *instruction);
    if highs.len() < 2 || highs.len() > available_homes {
        return None;
    }

    let mut tables = Vec::with_capacity(highs.len());
    for (high, table_index) in highs {
        if is_control_flow_target(output, high) {
            return None;
        }
        let old = match output.instructions[high] {
            Instruction::AddImmediateShifted {
                d,
                a: 0,
                immediate: 0,
            } => d,
            _ => return None,
        };
        let low = output.relocations.iter().find_map(|relocation| {
            (relocation.kind == RelocationKind::Addr16Lo
                && relocation.instruction_index > high
                && relocation.instruction_index <= high + 4
                && jump_table_index(&relocation.target) == Some(table_index))
            .then_some(relocation.instruction_index)
        })?;
        if is_control_flow_target(output, low)
            || !matches!(
                output.instructions[low],
                Instruction::AddImmediate {
                    d,
                    a,
                    immediate: 0,
                } if d == old && a == old
            )
        {
            return None;
        }
        let load = (low + 1..backedge).find(|index| {
            matches!(
                output.instructions[*index],
                Instruction::LoadWordIndexed { a, .. } if a == old
            )
        })?;
        tables.push(TableBase { high, low, load });
    }
    Some(Plan { insertion, tables })
}

fn jump_table_index(target: &RelocationTarget) -> Option<usize> {
    match target {
        RelocationTarget::JumpTable => Some(0),
        RelocationTarget::JumpTableAt(index) => Some(*index),
        _ => None,
    }
}

fn is_control_flow_target(output: &MachineFunction, index: usize) -> bool {
    output.instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                if *target == index
        )
    }) || output.jump_tables.iter().any(|table| {
        table
            .entries
            .iter()
            .any(|offset| *offset as usize == index.saturating_mul(4))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind};

    fn sample_output() -> MachineFunction {
        let mut output = MachineFunction::default();
        output.instructions = vec![
            Instruction::BranchToLinkRegister,
            Instruction::load_immediate(31, 0),
            Instruction::load_immediate(29, 0),
            Instruction::Add { d: 25, a: 30, b: 29 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 },
            Instruction::BranchAndLink { target: "first".into() },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 },
            Instruction::BranchAndLink { target: "second".into() },
            Instruction::AddImmediate { d: 31, a: 31, immediate: 1 },
            Instruction::AddImmediate { d: 29, a: 29, immediate: 2 },
            Instruction::CompareLogicalWordImmediate { a: 31, immediate: 6 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 3 },
            Instruction::BranchToLinkRegister,
        ];
        output.relocations = vec![
            Relocation { instruction_index: 4, kind: RelocationKind::Addr16Ha, target: RelocationTarget::JumpTable },
            Relocation { instruction_index: 5, kind: RelocationKind::Addr16Lo, target: RelocationTarget::JumpTable },
            Relocation { instruction_index: 8, kind: RelocationKind::Addr16Ha, target: RelocationTarget::JumpTableAt(1) },
            Relocation { instruction_index: 9, kind: RelocationKind::Addr16Lo, target: RelocationTarget::JumpTableAt(1) },
        ];
        output
    }

    #[test]
    fn recognizes_two_table_bases_inside_one_call_making_counted_loop() {
        assert_eq!(
            plan(&sample_output(), 2),
            Some(Plan {
                insertion: 1,
                tables: vec![
                    TableBase { high: 4, low: 5, load: 6 },
                    TableBase { high: 8, low: 9, load: 10 },
                ],
            })
        );
    }

    #[test]
    fn rejects_a_loop_without_enough_saved_homes() {
        assert!(plan(&sample_output(), 1).is_none());
    }
}
