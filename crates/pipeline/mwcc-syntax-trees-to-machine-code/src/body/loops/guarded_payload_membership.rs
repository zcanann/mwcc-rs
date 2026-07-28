//! Test an object payload against a registry and its owning type.
//!
//! Legacy MWCC folds the payload-header lookup into one negative-displacement
//! load, keeps the requested type in a saved register across the registry
//! call, and short-circuits both identity tests through one failure block.

#[allow(unused_imports)]
use super::*;

struct GuardedPayloadMembership {
    registry: String,
    test_callee: String,
    payload_header_size: i16,
    type_offset: i16,
}

fn var(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn casted_var(expression: &Expression, expected: &str) -> bool {
    match expression {
        Expression::Cast { operand, .. } => casted_var(operand, expected),
        _ => var(expression, expected),
    }
}

fn is_constant(expression: &Expression, expected: i64) -> bool {
    constant_value(expression) == Some(expected)
}

fn classify(function: &Function) -> Option<GuardedPayloadMembership> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || !is_constant(function.return_expression.as_ref()?, 0)
    {
        return None;
    }
    let [object, requested_type] = function.parameters.as_slice() else {
        return None;
    };
    let [payload] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(object.parameter_type, Type::Pointer(_))
        || !matches!(requested_type.parameter_type, Type::StructPointer { .. })
        || !matches!(payload.declared_type, Type::StructPointer { .. })
        || payload.initializer.is_some()
        || payload.is_static
        || payload.is_volatile
        || payload.array_length.is_some()
    {
        return None;
    }

    let [outer] = function.statements.as_slice() else {
        return None;
    };
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left: tested_object,
                right: null_object,
            },
        then_body,
        else_body,
    } = outer
    else {
        return None;
    };
    if !var(tested_object, &object.name) || !is_constant(null_object, 0) || !else_body.is_empty() {
        return None;
    }

    let [load_payload, membership_if] = then_body.as_slice() else {
        return None;
    };
    let Statement::Assign {
        name: payload_name,
        value: Expression::Dereference { pointer },
    } = load_payload
    else {
        return None;
    };
    let Expression::Cast {
        operand: header_address,
        ..
    } = pointer.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left: object_bytes,
        right: payload_header_size,
    } = header_address.as_ref()
    else {
        return None;
    };
    let payload_header_size = i16::try_from(constant_value(payload_header_size)?).ok()?;
    if payload_name != &payload.name
        || !casted_var(object_bytes, &object.name)
        || payload_header_size <= 0
    {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: membership_call,
                right: type_match,
            },
        then_body: membership_success,
        else_body: membership_else,
    } = membership_if
    else {
        return None;
    };
    let Expression::Call {
        name: test_callee,
        arguments,
    } = membership_call.as_ref()
    else {
        return None;
    };
    let [Expression::Variable(registry), tested_payload] = arguments.as_slice() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left: payload_type,
        right: compared_type,
    } = type_match.as_ref()
    else {
        return None;
    };
    let Expression::Member {
        base,
        offset: type_offset,
        index_stride: None,
        ..
    } = payload_type.as_ref()
    else {
        return None;
    };
    if !var(tested_payload, &payload.name)
        || !var(base, &payload.name)
        || !var(compared_type, &requested_type.name)
        || !membership_else.is_empty()
        || !matches!(
            membership_success.as_slice(),
            [Statement::Return(Some(value))] if is_constant(value, 1)
        )
    {
        return None;
    }

    Some(GuardedPayloadMembership {
        registry: registry.clone(),
        test_callee: test_callee.clone(),
        payload_header_size,
        type_offset: i16::try_from(*type_offset).ok()?,
    })
}

impl Generator {
    pub(crate) fn try_guarded_payload_membership(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if !self.globals.contains_key(&shape.registry)
            || !self.frame_slots.is_empty()
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.saved_gpr_epilogue_style
                != mwcc_versions::SavedGprEpilogueStyle::LinkRegisterBeforeFinalSaved
        {
            return Ok(false);
        }
        self.emit_guarded_payload_membership(&shape);
        Ok(true)
    }

    fn emit_guarded_payload_membership(&mut self, shape: &GuardedPayloadMembership) {
        const PAYLOAD: u8 = 31;
        const REQUESTED_TYPE: u8 = 30;
        let failure = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved = vec![PAYLOAD, REQUESTED_TYPE];
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
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
                s: PAYLOAD,
                a: 1,
                offset: 20,
            },
            Instruction::StoreWord {
                s: REQUESTED_TYPE,
                a: 1,
                offset: 16,
            },
            Instruction::AddImmediate {
                d: REQUESTED_TYPE,
                a: 4,
                immediate: 0,
            },
        ]);
        self.emit_branch_conditional_to(12, 2, failure);
        self.output.instructions.push(Instruction::LoadWord {
            d: PAYLOAD,
            a: 3,
            offset: -shape.payload_header_size,
        });
        self.record_relocation(RelocationKind::EmbSda21, &shape.registry);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::move_register(4, PAYLOAD),
        ]);
        self.record_relocation(RelocationKind::Rel24, &shape.test_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.test_callee.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, failure);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: PAYLOAD,
                offset: shape.type_offset,
            },
            Instruction::CompareLogicalWord {
                a: 0,
                b: REQUESTED_TYPE,
            },
        ]);
        self.emit_branch_conditional_to(4, 2, failure);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.emit_branch_to(epilogue);
        self.bind_label(failure);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));

        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: PAYLOAD,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: REQUESTED_TYPE,
                a: 1,
                offset: 16,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 24,
            },
            Instruction::BranchToLinkRegister,
        ]);
    }
}
