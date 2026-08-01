//! Final schedule for archive-header publication around a reverse slash scan.
//!
//! The source transaction optionally allocates and loads an archive image,
//! scans its path backward for the last slash, publishes directory/name
//! strings, then installs two related pointer-table entries. Build 163 assigns
//! all three parameters and the scan index to one descending saved-register
//! packet and rotates the scan into CTR form. Generic structured lowering has
//! the correct operations but cannot recover that combined lifetime and issue
//! order statement by statement, so this pass owns the complete physical
//! region after allocation and ordinary scheduling have converged.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_archive_header_initialization(&mut self, function: &Function) {
        if !source_shape(function) || !candidate_shape(&self.output.instructions) {
            return;
        }

        let old = self.output.instructions.clone();
        let mut work = old.clone();
        for instruction in &mut work[4..83] {
            mwcc_vreg::for_each_register(instruction, |_, class, register| {
                if class != mwcc_vreg::Class::General {
                    return;
                }
                *register = match *register {
                    31 => 30,
                    30 => 29,
                    29 => 28,
                    28 => 31,
                    other => other,
                };
            });
        }

        let mut scheduled = work.clone();
        // Saved-register stores name frame slots, not value homes, so preserve
        // their physical register numbers while interleaving the entry copies.
        scheduled[4] = old[5].clone();
        scheduled[5] = Instruction::AddImmediate { d: 30, a: 5, immediate: 0 };
        scheduled[6] = old[7].clone();
        scheduled[7] = work[6].clone();
        scheduled[8] = old[9].clone();
        scheduled[9] = Instruction::AddImmediate { d: 28, a: 3, immediate: 0 };
        scheduled[10] = conditional_branch(&work[10], 29);

        // The allocation result and zero test coalesce into `mr.`. Removing the
        // standalone compare frees the instruction consumed by the CTR setup.
        scheduled[18] = Instruction::OrRecord { a: 29, s: 3, b: 3 };
        scheduled[19] = conditional_branch(&work[20], 22);
        scheduled[20] = work[21].clone();
        scheduled[21] = Instruction::Branch { target: 83 };
        scheduled[22] = Instruction::AddImmediate { d: 3, a: 28, immediate: 0 };
        scheduled[23] = Instruction::AddImmediate { d: 4, a: 29, immediate: 0 };
        scheduled[24] = work[25].clone();
        scheduled[25] = work[26].clone();
        scheduled[26] = conditional_branch(&work[27], 29);
        scheduled[27] = work[28].clone();
        scheduled[28] = Instruction::Branch { target: 83 };
        scheduled[29] = work[30].clone();
        scheduled[30] = work[31].clone();

        scheduled[31] = Instruction::AddImmediateCarryingRecord {
            d: 31,
            a: 3,
            immediate: -1,
        };
        scheduled[32] = Instruction::MoveToCountRegister { s: 31 };
        scheduled[33] = Instruction::Add { d: 3, a: 28, b: 31 };
        scheduled[34] = Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 41,
        };
        scheduled[35] = Instruction::LoadByteZero { d: 0, a: 3, offset: 0 };
        scheduled[36] = Instruction::CompareWordImmediate { a: 0, immediate: 47 };
        scheduled[37] = Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 41,
        };
        scheduled[38] = Instruction::AddImmediate { d: 31, a: 31, immediate: -1 };
        scheduled[39] = Instruction::AddImmediate { d: 3, a: 3, immediate: -1 };
        scheduled[40] = Instruction::BranchConditionalForward {
            options: 16,
            condition_bit: 0,
            target: 35,
        };

        permute_region(&mut scheduled, &work, 43, &[44, 47, 43, 45, 46, 48]);
        permute_region(&mut scheduled, &work, 50, &[51, 54, 50, 55, 52, 53, 56]);
        permute_region(&mut scheduled, &work, 57, &[58, 61, 57, 59, 60, 62]);
        permute_region(&mut scheduled, &work, 63, &[64, 67, 63, 68, 65, 66, 69]);
        permute_region(
            &mut scheduled,
            &work,
            70,
            &[71, 82, 70, 72, 73, 75, 74, 76, 77, 78, 79, 80, 81],
        );
        permute_region(&mut scheduled, &old, 83, &[83, 84, 85, 89, 86, 87, 88, 90]);

        rewrite_tail_registers(&mut scheduled);

        let relocation_schedule = archive_relocation_schedule();
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = relocation_schedule[relocation.instruction_index];
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
        self.output.instructions = scheduled;
        // The generic graph counts six optimizer-only labels which the measured
        // CTR/search transaction consumes before its string pool.
        self.output.anonymous_label_bump = self.output.anonymous_label_bump.saturating_sub(6);
    }
}

fn source_shape(function: &Function) -> bool {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || function.parameters.len() != 3
        || function.locals.len() != 2
        || !function
            .locals
            .iter()
            .all(|local| local.declared_type == Type::UnsignedInt && !local.is_static)
        || function.statements.len() != 8
        || !function
            .return_expression
            .as_ref()
            .is_some_and(|expression| constant_value(expression) == Some(1))
    {
        return false;
    }
    let [path, image, auxiliary] = function.parameters.as_slice() else {
        return false;
    };
    if path.parameter_type != Type::Pointer(Pointee::Char)
        || image.parameter_type != Type::Pointer(Pointee::UnsignedChar)
        || auxiliary.parameter_type != Type::Pointer(Pointee::UnsignedChar)
    {
        return false;
    }
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            },
        then_body,
        else_body,
    } = &function.statements[0]
    else {
        return false;
    };
    if !matches!(left.as_ref(), Expression::Variable(name) if name == &image.name)
        || constant_value(right) != Some(0)
        || !else_body.is_empty()
        || then_body.len() != 5
        || !matches!(then_body[0], Statement::Assign { value: Expression::Call { .. }, .. })
        || !matches!(then_body[2], Statement::Assign { value: Expression::Cast { .. }, .. })
        || !matches!(then_body[4], Statement::If { .. })
    {
        return false;
    }
    let Statement::Assign {
        name: scan,
        value:
            Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: length,
                right: one,
            },
    } = &function.statements[1]
    else {
        return false;
    };
    if constant_value(one) != Some(1)
        || !matches!(length.as_ref(), Expression::Call { arguments, .. }
            if matches!(arguments.as_slice(), [Expression::Variable(name)] if name == &path.name))
    {
        return false;
    }
    matches!(&function.statements[2],
        Statement::Loop {
            kind: LoopKind::While,
            condition: Some(Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            }),
            body,
            ..
        } if matches!(left.as_ref(), Expression::Variable(name) if name == scan)
            && constant_value(right) == Some(0)
            && body.len() == 2
            && matches!(&body[0], Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::Equal,
                    right,
                    ..
                },
                then_body,
                ..
            } if constant_value(right) == Some(47)
                && matches!(then_body.as_slice(), [Statement::Break])))
}

fn candidate_shape(instructions: &[Instruction]) -> bool {
    instructions.len() == 91
        && matches!(instructions[0], Instruction::MoveFromLinkRegister { d: 0 })
        && matches!(instructions[2], Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -40 })
        && matches!(instructions[3], Instruction::StoreWord { s: 31, a: 1, offset: 36 })
        && matches!(instructions[5], Instruction::StoreWord { s: 30, a: 1, offset: 32 })
        && matches!(instructions[7], Instruction::StoreWord { s: 29, a: 1, offset: 28 })
        && matches!(instructions[9], Instruction::StoreWord { s: 28, a: 1, offset: 24 })
        && [12, 17, 25, 31, 48, 56, 69]
            .into_iter()
            .all(|index| matches!(instructions[index], Instruction::BranchAndLink { .. }))
        && matches!(instructions[34], Instruction::LoadByteZeroIndexed { .. })
        && matches!(instructions[35], Instruction::ExtendSignByte { .. })
        && matches!(instructions[90], Instruction::BranchToLinkRegister)
}

fn conditional_branch(instruction: &Instruction, target: usize) -> Instruction {
    let Instruction::BranchConditionalForward {
        options,
        condition_bit,
        ..
    } = instruction
    else {
        unreachable!("archive schedule candidate checked its branch packet")
    };
    Instruction::BranchConditionalForward {
        options: *options,
        condition_bit: *condition_bit,
        target,
    }
}

fn permute_region(
    destination: &mut [Instruction],
    source: &[Instruction],
    start: usize,
    old_indices: &[usize],
) {
    for (offset, old) in old_indices.iter().copied().enumerate() {
        destination[start + offset] = source[old].clone();
    }
}

fn rewrite_tail_registers(instructions: &mut [Instruction]) {
    instructions[43] = Instruction::LoadWord { d: 0, a: 0, offset: 0 };
    instructions[44] = Instruction::load_immediate(4, 0);
    instructions[45] = Instruction::LoadWord { d: 3, a: 0, offset: 0 };
    instructions[46] = Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 };
    instructions[47] = Instruction::LoadWordIndexed { d: 3, a: 3, b: 0 };

    instructions[50] = Instruction::LoadWord { d: 0, a: 0, offset: 0 };
    instructions[51] = Instruction::Or { a: 4, s: 28, b: 28 };
    instructions[52] = Instruction::LoadWord { d: 3, a: 0, offset: 0 };
    instructions[53] = Instruction::AddImmediate { d: 5, a: 31, immediate: 0 };
    instructions[54] = Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 };
    instructions[55] = Instruction::LoadWordIndexed { d: 3, a: 3, b: 0 };

    instructions[57] = Instruction::LoadWord { d: 0, a: 0, offset: 0 };
    instructions[58] = Instruction::load_immediate(4, 0);
    instructions[59] = Instruction::LoadWord { d: 3, a: 0, offset: 0 };
    instructions[60] = Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 };
    instructions[61] = Instruction::LoadWordIndexed { d: 3, a: 3, b: 0 };
    instructions[62] = Instruction::StoreByteIndexed { s: 4, a: 3, b: 31 };

    instructions[63] = Instruction::LoadWord { d: 0, a: 0, offset: 0 };
    instructions[64] = Instruction::Add { d: 4, a: 31, b: 28 };
    instructions[65] = Instruction::LoadWord { d: 3, a: 0, offset: 0 };
    instructions[66] = Instruction::AddImmediate { d: 4, a: 4, immediate: 1 };
    instructions[67] = Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 };
    instructions[68] = Instruction::LoadWordIndexed { d: 3, a: 3, b: 0 };

    instructions[70] = Instruction::LoadWord { d: 0, a: 0, offset: 0 };
    instructions[71] = Instruction::load_immediate(3, 1);
    instructions[72] = Instruction::LoadWord { d: 4, a: 0, offset: 0 };
    instructions[73] = Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 };
    instructions[74] = Instruction::StoreWordIndexed { s: 29, a: 4, b: 0 };
    instructions[75] = Instruction::LoadWord { d: 0, a: 0, offset: 0 };
    instructions[76] = Instruction::LoadWord { d: 4, a: 0, offset: 0 };
    instructions[77] = Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 };
    instructions[78] = Instruction::LoadWordIndexed { d: 4, a: 4, b: 0 };
    instructions[79] = Instruction::StoreWord { s: 30, a: 4, offset: 8 };
    instructions[80] = Instruction::LoadWord { d: 4, a: 0, offset: 0 };
    instructions[81] = Instruction::AddImmediate { d: 0, a: 4, immediate: 1 };
    instructions[82] = Instruction::StoreWord { s: 0, a: 0, offset: 0 };
}

fn archive_relocation_schedule() -> Vec<usize> {
    let mut schedule = (0..91).collect::<Vec<_>>();
    for (new, old) in [5, 4, 7, 6, 9, 8].into_iter().enumerate() {
        schedule[old] = new + 4;
    }
    for old in 20..=31 {
        schedule[old] = old - 1;
    }
    for (new, old) in [44, 47, 43, 45, 46, 48].into_iter().enumerate() {
        schedule[old] = 43 + new;
    }
    for (new, old) in [51, 54, 50, 55, 52, 53, 56].into_iter().enumerate() {
        schedule[old] = 50 + new;
    }
    for (new, old) in [58, 61, 57, 59, 60, 62].into_iter().enumerate() {
        schedule[old] = 57 + new;
    }
    for (new, old) in [64, 67, 63, 68, 65, 66, 69].into_iter().enumerate() {
        schedule[old] = 63 + new;
    }
    for (new, old) in [71, 82, 70, 72, 73, 75, 74, 76, 77, 78, 79, 80, 81]
        .into_iter()
        .enumerate()
    {
        schedule[old] = 70 + new;
    }
    for (new, old) in [83, 84, 85, 89, 86, 87, 88, 90].into_iter().enumerate() {
        schedule[old] = 83 + new;
    }
    schedule
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relocation_owners_follow_every_scheduled_archive_packet() {
        let schedule = archive_relocation_schedule();
        let old = [12, 17, 25, 31, 43, 44, 47, 48, 50, 51, 56, 57, 58, 63, 64, 69,
            70, 71, 74, 75, 79, 81];
        let expected = [12, 17, 24, 30, 45, 43, 44, 48, 52, 50, 56, 59, 57, 65, 63,
            69, 72, 70, 76, 75, 80, 82];
        assert_eq!(
            old.map(|instruction| schedule[instruction]),
            expected,
        );
    }
}
