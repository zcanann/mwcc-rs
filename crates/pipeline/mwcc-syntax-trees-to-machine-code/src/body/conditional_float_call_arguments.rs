//! Derived floating call arguments with a live incoming floating argument.
//!
//! The incoming float remains in f1 for the trailing call.  A loaded selector,
//! its scaled value, a sign-selected addend, and two later member arguments
//! occupy f5/f2/f3/f4.  This body owns the complete region because allocating
//! the selector as an ordinary local in f1 silently changes the call's first
//! floating argument.

#[allow(unused_imports)]
use super::*;

struct FloatMember<'a> {
    base: &'a str,
    offset: i16,
}

struct ConditionalFloatCallArguments<'a> {
    object: &'a str,
    passthrough: &'a str,
    attrs_offset: i16,
    selector_offset: i16,
    scale_offset: i16,
    selected_offset: i16,
    product_offset: i16,
    trailing_offset: i16,
    callee: &'a str,
}

fn float_member(expression: &Expression) -> Option<FloatMember<'_>> {
    let Expression::Member {
        base,
        offset,
        member_type: Type::Float,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    let Expression::Variable(base) = base.as_ref() else {
        return None;
    };
    Some(FloatMember {
        base,
        offset: i16::try_from(*offset).ok()?,
    })
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn product_with_member<'a>(
    expression: &'a Expression,
    value: &str,
    member_base: &str,
) -> Option<FloatMember<'a>> {
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if !variable(left, value) {
        return None;
    }
    let member = float_member(right)?;
    (member.base == member_base).then_some(member)
}

fn assigned_member<'a>(
    statements: &'a [Statement],
    local: &str,
    member_base: &str,
) -> Option<(FloatMember<'a>, bool)> {
    let [Statement::Assign { name, value }] = statements else {
        return None;
    };
    if name != local {
        return None;
    }
    match value {
        Expression::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } => {
            let member = float_member(operand)?;
            (member.base == member_base).then_some((member, true))
        }
        _ => {
            let member = float_member(value)?;
            (member.base == member_base).then_some((member, false))
        }
    }
}

fn classify(function: &Function) -> Option<ConditionalFloatCallArguments<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
    {
        return None;
    }
    let [object, passthrough] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        object.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || passthrough.parameter_type != Type::Float
    {
        return None;
    }
    let [scaled, selected, selector, attrs] = function.locals.as_slice() else {
        return None;
    };
    if function
        .locals
        .iter()
        .any(|local| local.is_volatile || local.is_static || local.array_length.is_some())
        || scaled.declared_type != Type::Float
        || scaled.initializer.is_some()
        || selected.declared_type != Type::Float
        || selected.initializer.is_some()
        || selector.declared_type != Type::Float
        || !matches!(attrs.declared_type, Type::Pointer(_) | Type::StructPointer { .. })
    {
        return None;
    }
    let selector_member = float_member(selector.initializer.as_ref()?)?;
    if selector_member.base != object.name {
        return None;
    }
    let Expression::AddressOf { operand } = attrs.initializer.as_ref()? else {
        return None;
    };
    let Expression::Member {
        base: attrs_base,
        offset: attrs_offset,
        index_stride: None,
        ..
    } = operand.as_ref()
    else {
        return None;
    };
    if !matches!(attrs_base.as_ref(), Expression::Variable(name) if name == &object.name) {
        return None;
    }
    let [
        Statement::Assign {
            name: scaled_name,
            value: scaled_value,
        },
        Statement::If {
            condition:
                Expression::Binary {
                    operator: BinaryOperator::Greater,
                    left: condition_value,
                    right: condition_zero,
                },
            then_body,
            else_body,
        },
        Statement::Expression(Expression::Call { name: callee, arguments }),
    ] = function.statements.as_slice()
    else {
        return None;
    };
    if scaled_name != &scaled.name
        || constant_value(condition_zero) != Some(0)
        || !variable(condition_value, &selector.name)
    {
        return None;
    }
    let scale = product_with_member(scaled_value, &selector.name, &attrs.name)?;
    let (then_member, then_negated) = assigned_member(then_body, &selected.name, &attrs.name)?;
    let (else_member, else_negated) = assigned_member(else_body, &selected.name, &attrs.name)?;
    if then_negated
        || !else_negated
        || then_member.offset != else_member.offset
        || arguments.len() != 5
        || !variable(&arguments[0], &object.name)
        || !variable(&arguments[1], &passthrough.name)
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: added_scaled,
        right: added_selected,
    } = &arguments[2]
    else {
        return None;
    };
    if !variable(added_scaled, &scaled.name) || !variable(added_selected, &selected.name) {
        return None;
    }
    let product = product_with_member(&arguments[3], &selector.name, &attrs.name)?;
    let trailing = float_member(&arguments[4])?;
    if trailing.base != attrs.name {
        return None;
    }
    Some(ConditionalFloatCallArguments {
        object: &object.name,
        passthrough: &passthrough.name,
        attrs_offset: i16::try_from(*attrs_offset).ok()?,
        selector_offset: selector_member.offset,
        scale_offset: scale.offset,
        selected_offset: then_member.offset,
        product_offset: product.offset,
        trailing_offset: trailing.offset,
        callee,
    })
}

impl Generator {
    pub(crate) fn try_conditional_float_call_arguments(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let object = self.general_register_of(shape.object)?;
        let passthrough = self.float_register_of(shape.passthrough)?;
        if object != 3
            || passthrough != 1
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
        {
            return Ok(false);
        }
        let folded_scale_offset = shape
            .attrs_offset
            .checked_add(shape.scale_offset)
            .ok_or_else(|| Diagnostic::error("derived float member offset overflowed"))?;

        self.output.pre_scheduled = true;
        self.emit_plain_nonleaf_prologue();
        let attrs = Instruction::AddImmediate {
            d: 4,
            a: object,
            immediate: shape.attrs_offset,
        };
        self.output.instructions.insert(1, attrs);
        self.load_float_constant(0, 0.0);
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 5,
            a: object,
            offset: shape.selector_offset,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 2,
            a: object,
            offset: folded_scale_offset,
        });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 5, b: 0 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 2, a: 5, c: 2 });
        let else_branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 0,
            });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 3,
            a: 4,
            offset: shape.selected_offset,
        });
        let join_branch = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });
        let else_label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[else_branch]
        {
            *target = else_label;
        }
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 4,
            offset: shape.selected_offset,
        });
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 3, b: 0 });
        let join = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[join_branch] {
            *target = join;
        }
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 4,
            offset: shape.product_offset,
        });
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 2, a: 2, b: 3 });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 4,
            a: 4,
            offset: shape.trailing_offset,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 3, a: 5, c: 0 });
        self.record_relocation(RelocationKind::Rel24, shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.to_string(),
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
