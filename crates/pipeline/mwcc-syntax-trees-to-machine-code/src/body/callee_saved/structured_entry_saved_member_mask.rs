//! O0 saved-home scheduling for a leading member-mask guard.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Restore MWCC's direct saved-home definition and explicit zero compare
    /// for `saved = &entry->member; if ((saved->flags & mask) == 0)`.
    pub(super) fn restore_explicit_entry_saved_member_mask(
        &mut self,
        function: &Function,
    ) -> bool {
        if !explicit_entry_saved_member_mask(&function.statements) {
            return false;
        }
        let Some((initializer, saved)) =
            find_delayed_entry_saved_member_mask(&self.output.instructions)
        else {
            return false;
        };
        let (a, s, begin, end) = match self.output.instructions[initializer + 3] {
            Instruction::AndMaskRecord { a, s, begin, end } => (a, s, begin, end),
            _ => return false,
        };

        match &mut self.output.instructions[initializer] {
            Instruction::LoadWord { d, .. } => *d = saved,
            _ => unreachable!(),
        }
        self.remove_structured_condition_instruction(initializer + 1);
        self.output.instructions[initializer + 2] = Instruction::RotateAndMask {
            a,
            s,
            shift: 0,
            begin,
            end,
        };
        crate::insert_instruction_retargeting(
            self,
            initializer + 3,
            Instruction::CompareLogicalWordImmediate { a, immediate: 0 },
        );
        true
    }
}

fn find_delayed_entry_saved_member_mask(instructions: &[Instruction]) -> Option<(usize, u8)> {
    for initializer in 0..instructions.len().saturating_sub(4) {
        let [
            Instruction::LoadWord {
                d: retained,
                a: entry,
                ..
            },
            Instruction::AddImmediate {
                d: saved,
                a: copied,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: tested,
                a: member_base,
                ..
            },
            Instruction::AndMaskRecord {
                a: result,
                s: masked,
                ..
            },
            Instruction::BranchConditionalForward { .. },
            ..
        ] = &instructions[initializer..]
        else {
            continue;
        };
        if retained != copied
            || saved != member_base
            || tested != masked
            || tested != result
            || entry == saved
            || !mwcc_vreg::Reg::is_virtual_field(*saved)
        {
            continue;
        }
        let Some(first_call) = instructions[initializer + 5..]
            .iter()
            .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
            .map(|offset| initializer + 5 + offset)
        else {
            continue;
        };
        let used_after_call = instructions[first_call + 1..].iter().any(|instruction| {
            mwcc_vreg::register_operands(instruction)
                .into_iter()
                .any(|operand| {
                    operand.class == mwcc_vreg::Class::General
                        && operand.role == mwcc_vreg::RegisterRole::Use
                        && operand.register == *saved
                })
        });
        if used_after_call {
            return Some((initializer, *saved));
        }
    }
    None
}

/// Keep source classification separate from the instruction recognizer. The
/// latter proves the selected saved home really survives a call.
fn explicit_entry_saved_member_mask(statements: &[Statement]) -> bool {
    let (assigned_saved, condition) = match statements {
        [
            Statement::Assign {
                name: saved,
                value: Expression::AddressOf { .. },
            },
            Statement::If { condition, .. },
            ..
        ] => (Some(saved.as_str()), condition),
        // Definite-assignment recovery may promote the leading pointer
        // assignment into the local declaration before structured lowering.
        [Statement::If { condition, .. }, ..] => (None, condition),
        _ => return false,
    };
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = condition
    else {
        return false;
    };
    let masked = if constant_value(right) == Some(0) {
        left.as_ref()
    } else if constant_value(left) == Some(0) {
        right.as_ref()
    } else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: value,
        right: mask,
    } = masked
    else {
        return false;
    };
    let Some(mask) = constant_value(mask).and_then(|mask| u32::try_from(mask).ok()) else {
        return false;
    };
    if mask_to_run(mask).is_none() {
        return false;
    }
    matches!(value.as_ref(), Expression::Member { base, .. }
        if matches!(base.as_ref(), Expression::Variable(name)
            if assigned_saved.is_none_or(|saved| name == saved)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_an_explicit_mask_on_an_entry_initialized_saved_member() {
        let statements = vec![
            Statement::Assign {
                name: "saved".into(),
                value: Expression::AddressOf {
                    operand: Box::new(Expression::Variable("entry".into())),
                },
            },
            Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left: Box::new(Expression::Binary {
                        operator: BinaryOperator::BitAnd,
                        left: Box::new(Expression::Member {
                            base: Box::new(Expression::Variable("saved".into())),
                            offset: 52,
                            member_type: Type::UnsignedInt,
                            index_stride: None,
                        }),
                        right: Box::new(Expression::IntegerLiteral(4)),
                    }),
                    right: Box::new(Expression::IntegerLiteral(0)),
                },
                then_body: Vec::new(),
                else_body: Vec::new(),
            },
        ];

        assert!(explicit_entry_saved_member_mask(&statements));
    }

    #[test]
    fn recognizes_a_delayed_saved_home_used_after_a_call() {
        let retained = mwcc_vreg::Reg::general(0).to_field();
        let saved = mwcc_vreg::Reg::general(1).to_field();
        let instructions = [
            Instruction::LoadWord {
                d: retained,
                a: 26,
                offset: 60,
            },
            Instruction::AddImmediate {
                d: saved,
                a: retained,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: saved,
                offset: 52,
            },
            Instruction::AndMaskRecord {
                a: 0,
                s: 0,
                begin: 29,
                end: 29,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 7,
            },
            Instruction::BranchAndLink {
                target: "predicate".into(),
            },
            Instruction::Or {
                a: 3,
                s: saved,
                b: saved,
            },
        ];

        assert_eq!(
            find_delayed_entry_saved_member_mask(&instructions),
            Some((0, saved))
        );
    }
}
