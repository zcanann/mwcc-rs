//! Two-way call dispatch through a member of a freshly loaded pointer local.
//!
//! The local is only a source-level name for one member load. MWCC copy
//! propagates that load into the condition while leaving the original object
//! argument in r3 for either call arm.

#[allow(unused_imports)]
use super::*;
use crate::expressions::displacement_load;

struct LocalMemberCallDispatch<'a> {
    object: &'a str,
    pointer_offset: i16,
    condition_offset: i16,
    condition_type: Type,
    constant: i16,
    comparison: BinaryOperator,
    then_callee: &'a str,
    else_callee: &'a str,
}

fn one_object_call<'a>(statements: &'a [Statement], object: &str) -> Option<&'a str> {
    let [Statement::Expression(Expression::Call { name, arguments })] = statements else {
        return None;
    };
    matches!(arguments.as_slice(), [Expression::Variable(argument)] if argument == object)
        .then_some(name.as_str())
}

fn classify(function: &Function) -> Option<LocalMemberCallDispatch<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
    {
        return None;
    }
    let [object] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        object.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) {
        return None;
    }
    let [local] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(local.declared_type, Type::Pointer(_) | Type::StructPointer { .. }) {
        return None;
    }
    let Some(Expression::Member {
        base: pointer_base,
        offset: pointer_offset,
        index_stride: None,
        ..
    }) = local.initializer.as_ref()
    else {
        return None;
    };
    if !matches!(pointer_base.as_ref(), Expression::Variable(name) if name == &object.name) {
        return None;
    }
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: comparison,
                left,
                right,
            },
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(
        comparison,
        BinaryOperator::Equal | BinaryOperator::NotEqual
    ) {
        return None;
    }
    let Expression::Member {
        base: condition_base,
        offset: condition_offset,
        member_type: condition_type,
        index_stride: None,
    } = left.as_ref()
    else {
        return None;
    };
    if !matches!(condition_base.as_ref(), Expression::Variable(name) if name == &local.name) {
        return None;
    }
    let constant = i16::try_from(constant_value(right)?).ok()?;
    Some(LocalMemberCallDispatch {
        object: &object.name,
        pointer_offset: i16::try_from(*pointer_offset).ok()?,
        condition_offset: i16::try_from(*condition_offset).ok()?,
        condition_type: *condition_type,
        constant,
        comparison: *comparison,
        then_callee: one_object_call(then_body, &object.name)?,
        else_callee: one_object_call(else_body, &object.name)?,
    })
}

impl Generator {
    pub(crate) fn try_local_member_call_dispatch(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let object = self.general_register_of(shape.object)?;
        if object != 3 || self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return Ok(false);
        }
        let condition_pointee = pointee_of_type(shape.condition_type).ok_or_else(|| {
            Diagnostic::error("call-dispatch condition member has no scalar load width")
        })?;
        if matches!(condition_pointee, Pointee::Float | Pointee::Double) {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.emit_plain_nonleaf_prologue();
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: object,
            offset: shape.pointer_offset,
        });
        self.output.instructions.push(displacement_load(
            condition_pointee,
            GENERAL_SCRATCH,
            4,
            shape.condition_offset,
        )?);
        if self.signed_of(shape.condition_type) {
            self.output
                .instructions
                .push(Instruction::CompareWordImmediate {
                    a: GENERAL_SCRATCH,
                    immediate: shape.constant,
                });
        } else {
            self.output
                .instructions
                .push(Instruction::CompareLogicalWordImmediate {
                    a: GENERAL_SCRATCH,
                    immediate: shape.constant as u16,
                });
        }

        let else_label = self.fresh_label();
        let done = self.fresh_label();
        let (options, condition_bit) = false_branch_bo_bi(shape.comparison)
            .expect("the classifier accepts equality comparisons");
        self.emit_branch_conditional_to(options, condition_bit, else_label);
        self.record_relocation(RelocationKind::Rel24, shape.then_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.then_callee.to_string(),
        });
        self.emit_branch_to(done);
        self.bind_label(else_label);
        self.record_relocation(RelocationKind::Rel24, shape.else_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.else_callee.to_string(),
        });
        self.bind_label(done);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
