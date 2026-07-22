//! Conditional extraction from a call-filled aggregate frame.
//!
//! A producer fills an address-taken aggregate, returns a status in `r3`, and
//! the success edge copies one narrow member through an output pointer.  The
//! status remains the function result.  This module owns the measured legacy
//! frame layout and branch/store schedule for that transaction.

use super::*;

struct MemberCopyPlan<'a> {
    output: &'a str,
    aggregate: &'a str,
    result: &'a str,
    producer: &'a str,
    aggregate_size: u32,
    aggregate_align: u8,
    member_offset: i16,
    success_value: i16,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn ordinary(local: &LocalDeclaration) -> bool {
    local.initializer.is_none()
        && !local.is_volatile
        && !local.is_static
        && local.array_length.is_none()
}

fn classify(function: &Function) -> Option<MemberCopyPlan<'_>> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || function.parameters.len() != 3
        || function.locals.len() != 2
    {
        return None;
    }
    let [first, second, output] = function.parameters.as_slice() else {
        return None;
    };
    if first.parameter_type != Type::Int
        || second.parameter_type != Type::Int
        || output.parameter_type != Type::Pointer(Pointee::UnsignedChar)
    {
        return None;
    }
    let [aggregate, result] = function.locals.as_slice() else {
        return None;
    };
    let Type::Struct {
        size: aggregate_size,
        align: aggregate_align,
    } = aggregate.declared_type
    else {
        return None;
    };
    if result.declared_type != Type::Int || !ordinary(aggregate) || !ordinary(result) {
        return None;
    }

    let [Statement::Assign {
        name: result_target,
        value: Expression::Call {
            name: producer,
            arguments,
        },
    }, Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if result_target != &result.name
        || !matches!(arguments.as_slice(), [a, b, Expression::AddressOf { operand }]
            if variable(a, &first.name)
                && variable(b, &second.name)
                && variable(operand, &aggregate.name))
        || !else_body.is_empty()
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left: tested_result,
        right: success,
    } = condition
    else {
        return None;
    };
    let success_value = i16::try_from(constant_value(success)?).ok()?;
    let [Statement::Store {
        target: Expression::Dereference { pointer },
        value:
            Expression::Member {
                base,
                offset: member_offset,
                member_type: Type::UnsignedChar,
                index_stride: None,
            },
    }] = then_body.as_slice()
    else {
        return None;
    };
    let member_offset = i16::try_from(*member_offset).ok()?;
    if !variable(tested_result, &result.name)
        || !variable(pointer, &output.name)
        || !variable(base, &aggregate.name)
        || !matches!(function.return_expression.as_ref(), Some(value) if variable(value, &result.name))
        || member_offset < 0
        || member_offset >= i16::try_from(aggregate_size).ok()?
    {
        return None;
    }

    Some(MemberCopyPlan {
        output: &output.name,
        aggregate: &aggregate.name,
        result: &result.name,
        producer,
        aggregate_size,
        aggregate_align,
        member_offset,
        success_value,
    })
}

impl Generator {
    pub(crate) fn try_conditional_member_copy(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        // Only the measured CARDDir-class legacy layout is selected.  Other
        // aggregate sizes need their own observed linkage/local padding rule.
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || plan.aggregate_size != 64
            || plan.aggregate_align != 4
            || self.lookup_general(plan.output) != Some(5)
        {
            return Ok(false);
        }
        let _ = (plan.aggregate, plan.result);

        self.output.pre_scheduled = true;
        self.output.anonymous_label_bump += 2;
        self.frame_size = 96;
        self.non_leaf = true;
        self.callee_saved = vec![31];
        let aggregate_offset = 20i16;
        let done = self.fresh_label();

        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -96,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 92,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 5,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: aggregate_offset,
        });
        self.record_relocation(RelocationKind::Rel24, plan.producer);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.producer.into(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 3,
                immediate: plan.success_value,
            });
        self.emit_branch_conditional_to(4, 2, done); // bne
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: aggregate_offset + plan.member_offset,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 31,
            offset: 0,
        });

        self.bind_label(done);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
