//! Leaf member initialization from a narrow indirect input plus a constant sibling field.

#[allow(unused_imports)]
use super::*;

struct NarrowMemberInitialization {
    target_register: u8,
    source_register: u8,
    computed_offset: i16,
    increment: i16,
    constant_offset: i16,
    constant: i16,
}

fn variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(candidate) if candidate == name)
}

fn narrow_load(expression: &Expression, source: &str) -> bool {
    matches!(expression,
        Expression::Dereference { pointer }
            if matches!(pointer.as_ref(),
                Expression::Cast {
                    target_type: Type::Pointer(Pointee::Short),
                    operand,
                } if variable(operand, source)))
}

fn narrow_member_target(expression: &Expression) -> Option<(&Expression, u32)> {
    match expression {
        Expression::Member {
            base,
            offset,
            member_type: Type::Short | Type::UnsignedShort,
            index_stride: None,
        } => Some((base, *offset)),
        Expression::Index { base, index }
            if matches!(index.as_ref(), Expression::IntegerLiteral(0)) =>
        {
            match base.as_ref() {
                Expression::MemberAddress {
                    base,
                    offset,
                    element: Pointee::Short | Pointee::UnsignedShort,
                } => Some((base, *offset)),
                _ => None,
            }
        }
        _ => None,
    }
}

fn immediate(expression: &Expression) -> Option<i16> {
    match expression {
        Expression::IntegerLiteral(value) => i16::try_from(*value).ok(),
        Expression::Cast {
            target_type: Type::Int | Type::UnsignedInt,
            operand,
        } => match operand.as_ref() {
            Expression::FloatLiteral(value)
                if value.is_finite() && value.fract() == 0.0 && *value >= i16::MIN as f64
                    && *value <= i16::MAX as f64 =>
            {
                Some(*value as i16)
            }
            _ => None,
        },
        _ => None,
    }
}

impl Generator {
    /// Lower two narrow stores through one struct parameter when the first value is a signed-short
    /// indirect load plus an immediate and the second is a literal. mwcc fills the load latency
    /// with the sibling literal, then completes the add and emits both stores in source order.
    pub(crate) fn try_narrow_member_initialization(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || function.parameters.len() < 2
        {
            return Ok(false);
        }
        let [
            Statement::Store {
                target: computed_target,
                value:
                    Expression::Binary {
                        operator: BinaryOperator::Add,
                        left,
                        right,
                    },
            },
            Statement::Store {
                target:
                    Expression::Member {
                        base: constant_base,
                        offset: constant_offset,
                        member_type: Type::Short | Type::UnsignedShort,
                        index_stride: None,
                    },
                value: Expression::IntegerLiteral(constant),
            },
        ] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Some((computed_base, computed_offset)) = narrow_member_target(computed_target) else {
            return Ok(false);
        };
        let Expression::Variable(target_name) = computed_base else {
            return Ok(false);
        };
        if !variable(constant_base, target_name) {
            return Ok(false);
        }
        let (load, increment) = if let Some(increment) = immediate(right) {
            (left.as_ref(), increment)
        } else if let Some(increment) = immediate(left) {
            (right.as_ref(), increment)
        } else {
            return Ok(false);
        };
        let Some(source_name) = function
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .find(|name| narrow_load(load, name))
        else {
            return Ok(false);
        };
        let Some(target) = self.locations.get(target_name) else {
            return Ok(false);
        };
        let Some(source) = self.locations.get(source_name) else {
            return Ok(false);
        };
        if target.class != ValueClass::General
            || source.class != ValueClass::General
            || !matches!(
                function
                    .parameters
                    .iter()
                    .find(|parameter| parameter.name == *target_name)
                    .map(|parameter| parameter.parameter_type),
                Some(Type::StructPointer { .. })
            )
        {
            return Ok(false);
        }
        let plan = NarrowMemberInitialization {
            target_register: target.register,
            source_register: source.register,
            computed_offset: match i16::try_from(computed_offset) {
                Ok(offset) => offset,
                Err(_) => return Ok(false),
            },
            increment,
            constant_offset: match i16::try_from(*constant_offset) {
                Ok(offset) => offset,
                Err(_) => return Ok(false),
            },
            constant: match i16::try_from(*constant) {
                Ok(value) => value,
                Err(_) => return Ok(false),
            },
        };
        // r4 is the measured lowest free home after reserving the r3 target and the later source
        // parameter. Restrict the claim until the general allocator owns this choice.
        if plan.target_register != 3 || plan.source_register <= 4 {
            return Ok(false);
        }

        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 4,
                a: plan.source_register,
                offset: 0,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: plan.constant,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: plan.increment,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 4,
            a: plan.target_register,
            offset: plan.computed_offset,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: plan.target_register,
            offset: plan.constant_offset,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
