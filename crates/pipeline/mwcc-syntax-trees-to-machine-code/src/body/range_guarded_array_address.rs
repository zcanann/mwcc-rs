//! Leaf lookups that return an indexed global-array address inside a range.
//!
//! Source-level pointer locals make this look like a general conditional local,
//! but optimized mwcc keeps the null fallback in r0 and forms the selected
//! address directly into that same result candidate.

#[allow(unused_imports)]
use super::*;

struct RangeGuardedArrayAddress<'a> {
    index: &'a str,
    result: &'a str,
    array: &'a str,
    bound: i16,
    stride: i16,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn null_pointer_constant(expression: &Expression) -> bool {
    match expression {
        Expression::Cast { operand, .. } => null_pointer_constant(operand),
        _ => constant_value(expression) == Some(0),
    }
}

fn classify<'a>(
    function: &'a Function,
    globals: &std::collections::HashMap<String, Type>,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> Option<RangeGuardedArrayAddress<'a>> {
    if !matches!(
        function.return_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || !function.guards.is_empty()
    {
        return None;
    }
    let [index] = function.parameters.as_slice() else {
        return None;
    };
    let [result] = function.locals.as_slice() else {
        return None;
    };
    if index.parameter_type != Type::Int
        || !matches!(
            result.declared_type,
            Type::Pointer(_) | Type::StructPointer { .. }
        )
        || !result
            .initializer
            .as_ref()
            .is_some_and(null_pointer_constant)
        || result.array_length.is_some()
        || result.is_static
        || result.is_volatile
        || !matches!(function.return_expression.as_ref(), Some(value) if variable(value, &result.name))
    {
        return None;
    }
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if !matches!(left.as_ref(), Expression::Binary {
        operator: BinaryOperator::GreaterEqual,
        left,
        right,
    } if variable(left, &index.name) && constant_value(right) == Some(0))
    {
        return None;
    }
    let bound = match right.as_ref() {
        Expression::Binary {
            operator: BinaryOperator::Less,
            left,
            right,
        } if variable(left, &index.name) => i16::try_from(constant_value(right)?).ok()?,
        _ => return None,
    };
    if bound <= 0 {
        return None;
    }
    let [Statement::Assign {
        name: assigned_result,
        value: Expression::AddressOf { operand: selected },
    }] = then_body.as_slice()
    else {
        return None;
    };
    let Expression::Index {
        base: selected_array,
        index: selected_index,
    } = selected.as_ref()
    else {
        return None;
    };
    let Expression::Variable(array) = selected_array.as_ref() else {
        return None;
    };
    if assigned_result != &result.name
        || !variable(selected_index, &index.name)
        || !global_array_sizes.contains_key(array)
    {
        return None;
    }
    let stride = match globals.get(array) {
        Some(Type::Struct { size, .. }) => i16::try_from(*size).ok()?,
        _ => return None,
    };
    if stride <= 0 {
        return None;
    }
    Some(RangeGuardedArrayAddress {
        index: &index.name,
        result: &result.name,
        array,
        bound,
        stride,
    })
}

impl Generator {
    /// Lower `p = 0; if (i >= 0 && i < N) p = &array[i]; return p;`.
    pub(crate) fn try_range_guarded_array_address(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function, &self.globals, &self.global_array_sizes) else {
            return Ok(false);
        };
        let index = self.general_register_of(shape.index)?;
        if index != Eabi::FIRST_GENERAL_ARGUMENT || !self.frame_slots.is_empty() {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        let failed_range = self.fresh_label();
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: index,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(GENERAL_SCRATCH, 0));
        self.emit_branch_conditional_to(12, 0, failed_range);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: index,
                immediate: shape.bound,
            });
        self.emit_branch_conditional_to(4, 0, failed_range);
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 4,
                a: index,
                immediate: shape.stride,
            });
        self.record_relocation(RelocationKind::Addr16Ha, shape.array);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: Eabi::general_result().number,
                a: 0,
                immediate: 0,
            });
        self.record_relocation(RelocationKind::Addr16Lo, shape.array);
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: Eabi::general_result().number,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::Add {
            d: GENERAL_SCRATCH,
            a: GENERAL_SCRATCH,
            b: 4,
        });
        self.bind_label(failed_range);
        self.output.instructions.push(Instruction::move_register(
            Eabi::general_result().number,
            GENERAL_SCRATCH,
        ));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        self.locations.insert(
            shape.result.to_string(),
            Location {
                class: ValueClass::General,
                register: GENERAL_SCRATCH,
                signed: false,
                width: 32,
                pointee: None,
                stride: Some(shape.stride as u32),
            },
        );
        Ok(true)
    }
}
