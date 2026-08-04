//! Three-element float accumulation selected by a global byte table.
//!
//! Build 163 retains the byte-table cursor and float byte offset in separate
//! registers, then lowers the fixed trip count through CTR. This owner keeps
//! the inferred global array as an address value and emits the complete switch
//! schedule without manufacturing a stack frame for the cursor local.

#[allow(unused_imports)]
use super::*;

struct FixedTripletFloatDispatch<'a> {
    table: &'a str,
}

impl Generator {
    pub(crate) fn try_fixed_triplet_float_dispatch(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.integer_loop_style
            != mwcc_versions::IntegerLoopStyle::LegacyDependencyFirst
            || !self.behavior.schedule_latency_slots
            || self.global_array_sizes.get(plan.table).copied().is_none_or(|size| size < 9)
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 5,
            shift: 0,
            begin: 24,
            end: 31,
        });
        self.emit_address_high(5, plan.table);
        self.output.instructions.push(Instruction::MultiplyImmediate {
            d: 7,
            a: 0,
            immediate: 3,
        });
        self.record_relocation(RelocationKind::Addr16Lo, plan.table);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 6, a: 5, immediate: 0 },
            Instruction::load_immediate(0, 3),
        ]);
        self.load_float_constant(1, 0.0);
        self.output.instructions.extend([
            Instruction::load_immediate(5, 0),
            Instruction::Add { d: 6, a: 6, b: 7 },
            Instruction::MoveToCountRegister { s: 0 },
            Instruction::LoadByteZero { d: 0, a: 6, offset: 0 },
            Instruction::CompareWordImmediate { a: 0, immediate: 1 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 17,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: 14,
            },
            Instruction::Branch { target: 24 },
            Instruction::CompareWordImmediate { a: 0, immediate: 3 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: 24,
            },
            Instruction::Branch { target: 20 },
            Instruction::LoadFloatSingleIndexed { d: 0, a: 3, b: 5 },
            Instruction::FloatAddSingle { d: 1, a: 1, b: 0 },
            Instruction::Branch { target: 24 },
            Instruction::LoadFloatSingleIndexed { d: 2, a: 3, b: 5 },
            Instruction::LoadFloatSingleIndexed { d: 0, a: 4, b: 5 },
            Instruction::FloatMultiplySingle { d: 0, a: 2, c: 0 },
            Instruction::FloatAddSingle { d: 1, a: 1, b: 0 },
            Instruction::AddImmediate { d: 5, a: 5, immediate: 4 },
            Instruction::AddImmediate { d: 6, a: 6, immediate: 1 },
            Instruction::BranchConditionalForward {
                options: 16,
                condition_bit: 0,
                target: 9,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

fn classify(function: &Function) -> Option<FixedTripletFloatDispatch<'_>> {
    let [left_matrix, right_matrix, selector] = function.parameters.as_slice() else {
        return None;
    };
    let [accumulator, values, index] = function.locals.as_slice() else {
        return None;
    };
    if function.return_type != Type::Float
        || !matches!(left_matrix.parameter_type, Type::StructPointer { .. })
        || !matches!(right_matrix.parameter_type, Type::StructPointer { .. })
        || selector.parameter_type != Type::UnsignedChar
        || accumulator.declared_type != Type::Float
        || !matches!(accumulator.initializer, Some(Expression::FloatLiteral(value)) if value == 0.0)
        || !matches!(values.declared_type, Type::Pointer(Pointee::UnsignedChar))
        || index.declared_type != Type::Int
        || index.initializer.is_some()
        || !function.guards.is_empty()
        || variable(function.return_expression.as_ref()?) != Some(accumulator.name.as_str())
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: table,
        right: selector_scale,
    } = values.initializer.as_ref()?
    else {
        return None;
    };
    let table = variable(table)?;
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: scaled_selector,
        right: scale,
    } = selector_scale.as_ref()
    else {
        return None;
    };
    if variable(scaled_selector) != Some(selector.name.as_str()) || constant_value(scale) != Some(3)
    {
        return None;
    }

    let [Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !assigns_constant(initializer, &index.name, 0)
        || !increments_by_one(step, &index.name)
        || !matches!(condition, Expression::Binary {
            operator: BinaryOperator::Less,
            left,
            right,
        } if variable(left) == Some(index.name.as_str()) && constant_value(right) == Some(3))
    {
        return None;
    }
    let [Statement::Switch {
        scrutinee,
        arms,
        default: None,
    }] = body.as_slice()
    else {
        return None;
    };
    let (scrutinee_base, scrutinee_index) = indexed_variable(scrutinee)?;
    if scrutinee_base != values.name || variable(scrutinee_index) != Some(index.name.as_str()) {
        return None;
    }
    let [zero, one, two] = arms.as_slice() else {
        return None;
    };
    if zero.value != 0
        || zero.falls_through
        || !matches!(&zero.body, mwcc_syntax_trees::ArmBody::Statements(statements) if statements.is_empty())
        || one.value != 1
        || one.falls_through
        || two.value != 2
        || two.falls_through
    {
        return None;
    }
    let mwcc_syntax_trees::ArmBody::Statements(one_body) = &one.body else {
        return None;
    };
    let mwcc_syntax_trees::ArmBody::Statements(two_body) = &two.body else {
        return None;
    };
    if !matches!(one_body.as_slice(), [Statement::Assign { name, value }]
        if name == &accumulator.name
            && accumulation(value, &accumulator.name).is_some_and(|term| {
                matrix_element(term, &left_matrix.name, &index.name)
            }))
        || !matches!(two_body.as_slice(), [Statement::Assign { name, value }]
            if name == &accumulator.name
                && accumulation(value, &accumulator.name).is_some_and(|term| {
                    matches!(term, Expression::Binary {
                        operator: BinaryOperator::Multiply,
                        left,
                        right,
                    } if matrix_element(left, &left_matrix.name, &index.name)
                        && matrix_element(right, &right_matrix.name, &index.name))
                }))
    {
        return None;
    }

    Some(FixedTripletFloatDispatch { table })
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

fn indexed_variable(expression: &Expression) -> Option<(&str, &Expression)> {
    let Expression::Index { base, index } = expression else {
        return None;
    };
    Some((variable(base)?, index))
}

fn accumulation<'a>(expression: &'a Expression, accumulator: &str) -> Option<&'a Expression> {
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = expression
    else {
        return None;
    };
    (variable(left) == Some(accumulator)).then_some(right)
}

fn matrix_element(expression: &Expression, matrix: &str, index: &str) -> bool {
    let Expression::Index { base, index: element_index } = expression else {
        return false;
    };
    matches!(base.as_ref(), Expression::MemberAddress {
        base,
        offset: 0,
        element: Pointee::Float,
        index_stride: None,
    } if variable(base) == Some(matrix) && variable(element_index) == Some(index))
}

fn assigns_constant(expression: &Expression, name: &str, expected: i64) -> bool {
    matches!(expression, Expression::Assign { target, value }
        if variable(target) == Some(name) && constant_value(value) == Some(expected))
}

fn increments_by_one(expression: &Expression, name: &str) -> bool {
    let Expression::Assign { target, value } = expression else {
        return false;
    };
    matches!(value.as_ref(), Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } if variable(target) == Some(name)
        && variable(left) == Some(name)
        && constant_value(right) == Some(1))
}
