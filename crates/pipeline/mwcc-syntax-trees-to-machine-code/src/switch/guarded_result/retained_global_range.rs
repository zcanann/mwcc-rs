//! Guarded result switches that retain two global aggregate bases.
//!
//! Debug-monitor state machines reuse status and CPU-state members throughout
//! their guard, dispatch, and range checks. Their address materializations and
//! shared terminal return are one scheduling transaction.

use super::ResultArm;
use crate::{analysis::constant_value, generator::*};
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{BinaryOperator, Expression, Type};
use mwcc_versions::{GlobalAddressing, Optimization};

struct RetainedGlobalRange<'a> {
    status: &'a str,
    active_offset: i16,
    kind_offset: i16,
    count_offset: i16,
    range_start_offset: i16,
    range_end_offset: i16,
    cpu: &'a str,
    exception_offset: i16,
    pc_offset: i16,
    exception: u16,
}

fn global_member(expression: &Expression, member_type: Type) -> Option<(&str, i16)> {
    let Expression::Member {
        base,
        offset,
        member_type: actual_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    let Expression::Variable(name) = base.as_ref() else {
        return None;
    };
    if *actual_type != member_type {
        return None;
    }
    Some((name.as_str(), i16::try_from(*offset).ok()?))
}

fn recognize<'a>(
    outer: &'a Expression,
    scrutinee: &'a Expression,
    arms: &[ResultArm<'a>],
    initial: i64,
) -> Option<RetainedGlobalRange<'a>> {
    if initial != 1 || arms.len() != 2 {
        return None;
    }
    let count_arm = arms
        .iter()
        .find(|arm| arm.value == 0 && arm.replacement == 0)?;
    let range_arm = arms
        .iter()
        .find(|arm| arm.value == 1 && arm.replacement == 0)?;

    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left: active,
        right: exception_test,
    } = outer
    else {
        return None;
    };
    let (status, active_offset) = global_member(active, Type::Int)?;
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left: exception_value,
        right: exception_literal,
    } = exception_test.as_ref()
    else {
        return None;
    };
    let Expression::Cast {
        target_type: Type::UnsignedShort,
        operand: exception_member,
    } = exception_value.as_ref()
    else {
        return None;
    };
    let (cpu, exception_offset) = global_member(exception_member, Type::UnsignedInt)?;
    let exception = u16::try_from(constant_value(exception_literal)?).ok()?;

    let (switch_status, kind_offset) = global_member(scrutinee, Type::UnsignedChar)?;
    if switch_status != status {
        return None;
    }

    let Expression::Binary {
        operator: BinaryOperator::Greater,
        left: count,
        right: zero,
    } = count_arm.condition
    else {
        return None;
    };
    let (count_status, count_offset) = global_member(count, Type::UnsignedInt)?;
    if count_status != status || constant_value(zero) != Some(0) {
        return None;
    }

    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left: lower_test,
        right: upper_test,
    } = range_arm.condition
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::GreaterEqual,
        left: lower_pc,
        right: range_start,
    } = lower_test.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::LessEqual,
        left: upper_pc,
        right: range_end,
    } = upper_test.as_ref()
    else {
        return None;
    };
    let (lower_cpu, pc_offset) = global_member(lower_pc, Type::UnsignedInt)?;
    let (upper_cpu, upper_pc_offset) = global_member(upper_pc, Type::UnsignedInt)?;
    let (start_status, range_start_offset) = global_member(range_start, Type::UnsignedInt)?;
    let (end_status, range_end_offset) = global_member(range_end, Type::UnsignedInt)?;
    if lower_cpu != cpu
        || upper_cpu != cpu
        || upper_pc_offset != pc_offset
        || start_status != status
        || end_status != status
    {
        return None;
    }

    Some(RetainedGlobalRange {
        status,
        active_offset,
        kind_offset,
        count_offset,
        range_start_offset,
        range_end_offset,
        cpu,
        exception_offset,
        pc_offset,
        exception,
    })
}

impl Generator {
    pub(super) fn try_emit_retained_global_range(
        &mut self,
        outer: &Expression,
        scrutinee: &Expression,
        arms: &[ResultArm<'_>],
        initial: i64,
    ) -> bool {
        if self.behavior.global_addressing != GlobalAddressing::Absolute
            || self.behavior.optimization != Optimization::O4
        {
            return false;
        }
        let Some(range) = recognize(outer, scrutinee, arms, initial) else {
            return false;
        };
        self.emit_retained_global_range(range);
        true
    }

    fn emit_retained_global_range(&mut self, range: RetainedGlobalRange<'_>) {
        self.preserve_terminal_return_branches = true;
        let count = self.fresh_label();
        let range_body = self.fresh_label();
        let done = self.fresh_label();

        self.record_relocation(RelocationKind::Addr16Ha, range.status);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            });
        self.record_relocation(RelocationKind::Addr16Lo, range.status);
        self.output
            .instructions
            .push(Instruction::AddImmediate {
                d: 5,
                a: 3,
                immediate: 0,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: range.active_offset,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, done);

        self.record_relocation(RelocationKind::Addr16Ha, range.cpu);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            });
        self.record_relocation(RelocationKind::Addr16Lo, range.cpu);
        self.output
            .instructions
            .push(Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 0,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: range.exception_offset,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 0,
                clear: 16,
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: range.exception,
            });
        self.emit_branch_conditional_to(4, 2, done);

        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: range.kind_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, range_body);
        self.emit_branch_conditional_to(4, 0, done);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, count);
        self.emit_branch_to(done);

        self.bind_label(count);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: range.count_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, done);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(done);

        self.bind_label(range_body);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 4,
            offset: range.pc_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: range.range_start_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 0, done);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: range.range_end_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 1, done);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));

        self.bind_label(done);
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 12;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(global: &str, offset: u32, member_type: Type) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(global.into())),
            offset,
            member_type,
            index_stride: None,
        }
    }

    fn recognized_inputs() -> (Expression, Expression, Expression, Expression) {
        let outer = Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: Box::new(member("status", 0, Type::Int)),
            right: Box::new(Expression::Binary {
                operator: BinaryOperator::Equal,
                left: Box::new(Expression::Cast {
                    target_type: Type::UnsignedShort,
                    operand: Box::new(member("cpu", 760, Type::UnsignedInt)),
                }),
                right: Box::new(Expression::IntegerLiteral(3328)),
            }),
        };
        let scrutinee = member("status", 4, Type::UnsignedChar);
        let count = Expression::Binary {
            operator: BinaryOperator::Greater,
            left: Box::new(member("status", 8, Type::UnsignedInt)),
            right: Box::new(Expression::IntegerLiteral(0)),
        };
        let range = Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: Box::new(Expression::Binary {
                operator: BinaryOperator::GreaterEqual,
                left: Box::new(member("cpu", 128, Type::UnsignedInt)),
                right: Box::new(member("status", 12, Type::UnsignedInt)),
            }),
            right: Box::new(Expression::Binary {
                operator: BinaryOperator::LessEqual,
                left: Box::new(member("cpu", 128, Type::UnsignedInt)),
                right: Box::new(member("status", 16, Type::UnsignedInt)),
            }),
        };
        (outer, scrutinee, count, range)
    }

    #[test]
    fn recognizes_a_retained_global_range_state_machine() {
        let (outer, scrutinee, count, range) = recognized_inputs();
        let arms = [
            ResultArm {
                value: 0,
                condition: &count,
                replacement: 0,
            },
            ResultArm {
                value: 1,
                condition: &range,
                replacement: 0,
            },
        ];

        let plan = recognize(&outer, &scrutinee, &arms, 1).unwrap();
        assert_eq!(plan.status, "status");
        assert_eq!(plan.cpu, "cpu");
        assert_eq!(plan.exception_offset, 760);
        assert_eq!(plan.pc_offset, 128);
        assert_eq!(plan.exception, 3328);
    }

    #[test]
    fn rejects_a_range_using_a_different_status_object() {
        let (outer, scrutinee, _, range) = recognized_inputs();
        let other_count = Expression::Binary {
            operator: BinaryOperator::Greater,
            left: Box::new(member("other_status", 8, Type::UnsignedInt)),
            right: Box::new(Expression::IntegerLiteral(0)),
        };
        let arms = [
            ResultArm {
                value: 0,
                condition: &other_count,
                replacement: 0,
            },
            ResultArm {
                value: 1,
                condition: &range,
                replacement: 0,
            },
        ];

        assert!(recognize(&outer, &scrutinee, &arms, 1).is_none());
    }
}
