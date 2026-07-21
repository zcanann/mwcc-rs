//! Shared member-store runs followed by a guarded call.
//!
//! Legacy mwcc treats the stores, the narrow guard test, and the call arguments
//! as one scheduling region: the shared store value fills the `mflr` latency,
//! the guard test precedes frame allocation, and that value register is later
//! reused for the call's third constant argument.

#[allow(unused_imports)]
use super::*;

struct MemberStore {
    offset: i16,
    member_type: Type,
}

struct LeadingStoreGuardedCall<'a> {
    base: &'a str,
    condition: &'a str,
    value: i16,
    stores: [MemberStore; 2],
    callee: &'a str,
    member_address_offset: i16,
    second_argument: i16,
    third_argument: i16,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn classify(function: &Function) -> Option<LeadingStoreGuardedCall<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.locals.is_empty()
        || !function.guards.is_empty()
    {
        return None;
    }
    let [base, condition_parameter] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        base.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || !matches!(
        condition_parameter.parameter_type,
        Type::Char | Type::UnsignedChar
    ) {
        return None;
    }
    let [first, second, Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty()
        || !matches!(condition,
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand,
            } if variable(operand, &condition_parameter.name))
    {
        return None;
    }
    let parse_store = |statement: &Statement| -> Option<(MemberStore, i16)> {
        let Statement::Store {
            target:
                Expression::Member {
                    base: store_base,
                    offset,
                    member_type,
                    index_stride: None,
                },
            value,
        } = statement
        else {
            return None;
        };
        if !variable(store_base, &base.name) {
            return None;
        }
        Some((
            MemberStore {
                offset: i16::try_from(*offset).ok()?,
                member_type: *member_type,
            },
            i16::try_from(constant_value(value)?).ok()?,
        ))
    };
    let (first, value) = parse_store(first)?;
    let (second, second_value) = parse_store(second)?;
    if second_value != value {
        return None;
    }

    let [Statement::Expression(Expression::Call { name, arguments })] = then_body.as_slice() else {
        return None;
    };
    let [Expression::MemberAddress {
        base: call_base,
        offset,
        index_stride: None,
        ..
    }, second_argument, third_argument] = arguments.as_slice()
    else {
        return None;
    };
    if !variable(call_base, &base.name) {
        return None;
    }
    Some(LeadingStoreGuardedCall {
        base: &base.name,
        condition: &condition_parameter.name,
        value,
        stores: [first, second],
        callee: name,
        member_address_offset: i16::try_from(*offset).ok()?,
        second_argument: i16::try_from(constant_value(second_argument)?).ok()?,
        third_argument: i16::try_from(constant_value(third_argument)?).ok()?,
    })
}

impl Generator {
    /// Lower `p->a=C; p->b=C; if (!flag) call(p->bytes, X, Y);` as one
    /// cross-statement schedule.
    pub(crate) fn try_leading_store_guarded_call(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.plain_linkage_epilogue_style
                != PlainLinkageEpilogueStyle::StackRestoreBeforeReload
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let base = self.general_register_of(shape.base)?;
        let condition = self.general_register_of(shape.condition)?;
        if base != 3 || condition != 4 {
            return Ok(false);
        }
        let value = 5;
        let done = self.fresh_label();
        self.non_leaf = true;
        self.frame_size = 8;
        self.output.pre_scheduled = true;

        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output
            .instructions
            .push(Instruction::load_immediate(value, shape.value));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: GENERAL_SCRATCH,
                s: condition,
                clear: 24,
            });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -8,
            });
        for store in &shape.stores {
            let pointee = pointee_of_type(store.member_type).ok_or_else(|| {
                Diagnostic::error("guarded-call member store has no scalar width")
            })?;
            self.output
                .instructions
                .push(displacement_store(pointee, value, base, store.offset)?);
        }
        self.emit_branch_conditional_to(4, 2, done);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: base,
            immediate: shape.member_address_offset,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, shape.second_argument));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, shape.third_argument));
        self.record_relocation(RelocationKind::Rel24, shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.to_string(),
        });

        self.bind_label(done);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
