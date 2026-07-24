//! Terminal compound-float updates followed by a clamp or wrap.
//!
//! State-machine arms commonly use `(member += delta) >= bound` and then
//! either clamp to `bound` or wrap by a member span. MWCC schedules the entire
//! region together: all live float inputs are loaded before arithmetic, the
//! updated member is stored after the compare, and the compared bound remains
//! live for the selected arm.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
struct FloatMember<'a> {
    base: &'a str,
    offset: i16,
}

#[derive(Clone, Copy)]
struct ScalarMember<'a> {
    base: &'a str,
    offset: i16,
    member_type: Type,
}

enum Bound<'a> {
    Direct(FloatMember<'a>),
    DifferenceLiteral {
        member: FloatMember<'a>,
        literal: f32,
    },
}

enum SelectedAction<'a> {
    Clamp {
        scalar: Option<(ScalarMember<'a>, i64)>,
    },
    Wrap {
        lower: FloatMember<'a>,
    },
}

struct TerminalFloatUpdate<'a> {
    target: FloatMember<'a>,
    adjustment: FloatMember<'a>,
    bound: Bound<'a>,
    update: BinaryOperator,
    comparison: BinaryOperator,
    selected: SelectedAction<'a>,
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

fn scalar_member(expression: &Expression) -> Option<ScalarMember<'_>> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    if !matches!(member_type, Type::Int | Type::UnsignedChar) {
        return None;
    }
    let Expression::Variable(base) = base.as_ref() else {
        return None;
    };
    Some(ScalarMember {
        base,
        offset: i16::try_from(*offset).ok()?,
        member_type: *member_type,
    })
}

fn same_float_member(expression: &Expression, member: FloatMember<'_>) -> bool {
    float_member(expression)
        .is_some_and(|candidate| candidate.base == member.base && candidate.offset == member.offset)
}

fn bound(expression: &Expression) -> Option<Bound<'_>> {
    if let Some(member) = float_member(expression) {
        return Some(Bound::Direct(member));
    }
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left,
        right,
    } = expression
    else {
        return None;
    };
    let member = float_member(left)?;
    let Expression::FloatLiteral(literal) = right.as_ref() else {
        return None;
    };
    Some(Bound::DifferenceLiteral {
        member,
        literal: *literal as f32,
    })
}

fn scalar_tail(statement: &Statement) -> Option<(ScalarMember<'_>, i64)> {
    let (target, value) = match statement {
        Statement::Store { target, value } => (target, value),
        Statement::Expression(Expression::Comma { left, right })
            if matches!(right.as_ref(), Expression::IntegerLiteral(0)) =>
        {
            let Expression::Assign { target, value } = left.as_ref() else {
                return None;
            };
            (target.as_ref(), value.as_ref())
        }
        _ => return None,
    };
    let Expression::IntegerLiteral(value) = value else {
        return None;
    };
    Some((scalar_member(target)?, *value))
}

fn classify(statements: &[Statement]) -> Option<TerminalFloatUpdate<'_>> {
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: comparison,
                left,
                right: compared_bound,
            },
        then_body,
        else_body,
    }] = statements
    else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    let Expression::Assign {
        target,
        value:
            update_value,
    } = left.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: update,
        left: previous_value,
        right: adjustment,
    } = update_value.as_ref()
    else {
        return None;
    };
    if !matches!(
        (*update, *comparison),
        (BinaryOperator::Add, BinaryOperator::GreaterEqual)
            | (BinaryOperator::Subtract, BinaryOperator::LessEqual)
    ) {
        return None;
    }
    let target_member = float_member(target)?;
    let adjustment = float_member(adjustment)?;
    if !same_float_member(previous_value, target_member)
        || adjustment.base != target_member.base
    {
        return None;
    }
    let bound = bound(compared_bound)?;
    let bound_base = match bound {
        Bound::Direct(member) | Bound::DifferenceLiteral { member, .. } => member.base,
    };
    if bound_base != target_member.base {
        return None;
    }

    let selected = match then_body.as_slice() {
        [Statement::Store {
            target: clamped_target,
            value: clamped_value,
        }] if same_float_member(clamped_target, target_member)
            && crate::analysis::structurally_equal(clamped_value, compared_bound) =>
        {
            SelectedAction::Clamp { scalar: None }
        }
        [Statement::Store {
            target: clamped_target,
            value: clamped_value,
        }, tail]
            if same_float_member(clamped_target, target_member)
                && crate::analysis::structurally_equal(clamped_value, compared_bound) =>
        {
            let (member, value) = scalar_tail(tail)?;
            if member.base != target_member.base {
                return None;
            }
            SelectedAction::Clamp {
                scalar: Some((member, value)),
            }
        }
        [Statement::Store {
            target: wrapped_target,
            value:
                Expression::Binary {
                    operator: BinaryOperator::Subtract,
                    left: wrapped_value,
                    right: span,
                },
        }] if same_float_member(wrapped_target, target_member)
            && same_float_member(wrapped_value, target_member) =>
        {
            let Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: upper,
                right: lower,
            } = span.as_ref()
            else {
                return None;
            };
            let lower = float_member(lower)?;
            if !crate::analysis::structurally_equal(upper, compared_bound)
                || lower.base != target_member.base
            {
                return None;
            }
            SelectedAction::Wrap { lower }
        }
        _ => return None,
    };
    if matches!(bound, Bound::DifferenceLiteral { .. })
        && *update != BinaryOperator::Add
    {
        return None;
    }
    if matches!(selected, SelectedAction::Wrap { .. })
        && (!matches!(bound, Bound::Direct(_)) || *update != BinaryOperator::Add)
    {
        return None;
    }

    Some(TerminalFloatUpdate {
        target: target_member,
        adjustment,
        bound,
        update: *update,
        comparison: *comparison,
        selected,
    })
}

impl Generator {
    /// Emit a byte direction flag selecting between two terminal float-update
    /// regions. C++ `bool` currently shares unsigned-char storage in the syntax
    /// tree, but MWCC tests this state-machine flag with signed `cmpwi`.
    pub(crate) fn try_terminal_float_direction(
        &mut self,
        statements: &[Statement],
    ) -> Compilation<bool> {
        let [Statement::If {
            condition:
                Expression::Unary {
                    operator: UnaryOperator::LogicalNot,
                    operand,
                },
            then_body,
            else_body,
        }] = statements
        else {
            return Ok(false);
        };
        let Some(then_shape) = classify(then_body) else {
            return Ok(false);
        };
        let Some(else_shape) = classify(else_body) else {
            return Ok(false);
        };
        if else_body.is_empty() {
            return Ok(false);
        }
        let Expression::Member {
            base,
            offset,
            member_type: Type::UnsignedChar,
            index_stride: None,
        } = operand.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Variable(base) = base.as_ref() else {
            return Ok(false);
        };
        if then_shape.target.base != base || else_shape.target.base != base {
            return Ok(false);
        }
        let base = self.general_register_of(base)?;
        if base != 3 {
            return Ok(false);
        }
        let offset = i16::try_from(*offset)
            .map_err(|_| Diagnostic::error("direction flag offset is out of range"))?;
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: base,
            offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 0,
            });
        let branch_to_else = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });
        if !self.try_terminal_float_update(then_body)? {
            return Ok(false);
        }
        let else_start = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[branch_to_else]
        {
            *target = else_start;
        }
        if !self.try_terminal_float_update(else_body)? {
            return Ok(false);
        }
        Ok(true)
    }

    pub(crate) fn try_terminal_float_update(
        &mut self,
        statements: &[Statement],
    ) -> Compilation<bool> {
        let Some(shape) = classify(statements) else {
            return Ok(false);
        };
        let base = self.general_register_of(shape.target.base)?;
        if base != 3 {
            return Ok(false);
        }
        self.output.pre_scheduled = true;

        let (updated, bound) = match (shape.update, shape.bound) {
            (
                BinaryOperator::Add,
                Bound::DifferenceLiteral { member, literal },
            ) => {
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 3,
                    a: base,
                    offset: member.offset,
                });
                self.load_float_constant(2, literal);
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 1,
                    a: base,
                    offset: shape.target.offset,
                });
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 0,
                    a: base,
                    offset: shape.adjustment.offset,
                });
                self.output.instructions.push(Instruction::FloatSubtractSingle {
                    d: 2,
                    a: 3,
                    b: 2,
                });
                self.output.instructions.push(Instruction::FloatAddSingle {
                    d: 0,
                    a: 1,
                    b: 0,
                });
                (0, 2)
            }
            (BinaryOperator::Add, Bound::Direct(member)) => {
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 2,
                    a: base,
                    offset: shape.target.offset,
                });
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 0,
                    a: base,
                    offset: shape.adjustment.offset,
                });
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 1,
                    a: base,
                    offset: member.offset,
                });
                self.output.instructions.push(Instruction::FloatAddSingle {
                    d: 2,
                    a: 2,
                    b: 0,
                });
                (2, 1)
            }
            (BinaryOperator::Subtract, Bound::Direct(member)) => {
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 2,
                    a: base,
                    offset: shape.target.offset,
                });
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 1,
                    a: base,
                    offset: shape.adjustment.offset,
                });
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 0,
                    a: base,
                    offset: member.offset,
                });
                self.output.instructions.push(Instruction::FloatSubtractSingle {
                    d: 1,
                    a: 2,
                    b: 1,
                });
                (1, 0)
            }
            _ => return Ok(false),
        };

        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered {
                a: updated,
                b: bound,
            });
        self.output.instructions.push(Instruction::StoreFloatSingle {
            s: updated,
            a: base,
            offset: shape.target.offset,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterOr {
                d: 2,
                a: if shape.comparison == BinaryOperator::GreaterEqual {
                    1
                } else {
                    0
                },
                b: 2,
            });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 2,
            });

        match shape.selected {
            SelectedAction::Clamp { scalar } => {
                if let Some((_, value)) = scalar {
                    self.load_integer_constant(0, value);
                }
                self.output.instructions.push(Instruction::StoreFloatSingle {
                    s: bound,
                    a: base,
                    offset: shape.target.offset,
                });
                if let Some((member, _)) = scalar {
                    self.output.instructions.push(match member.member_type {
                        Type::Int => Instruction::StoreWord {
                            s: 0,
                            a: base,
                            offset: member.offset,
                        },
                        Type::UnsignedChar => Instruction::StoreByte {
                            s: 0,
                            a: base,
                            offset: member.offset,
                        },
                        _ => unreachable!("the classifier accepts word and byte scalar tails"),
                    });
                }
            }
            SelectedAction::Wrap { lower } => {
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 0,
                    a: base,
                    offset: lower.offset,
                });
                self.output.instructions.push(Instruction::FloatSubtractSingle {
                    d: 0,
                    a: bound,
                    b: 0,
                });
                self.output.instructions.push(Instruction::FloatSubtractSingle {
                    d: 0,
                    a: updated,
                    b: 0,
                });
                self.output.instructions.push(Instruction::StoreFloatSingle {
                    s: 0,
                    a: base,
                    offset: shape.target.offset,
                });
            }
        }
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
