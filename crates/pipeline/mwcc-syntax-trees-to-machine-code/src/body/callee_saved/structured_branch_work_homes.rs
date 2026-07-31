//! Volatile work homes retained across mutually exclusive structured arms.

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StructuredBranchWorkHomes {
    pub(super) float: u8,
    pub(super) constant_address: u8,
}

impl StructuredBranchWorkHomes {
    pub(super) fn plan(generator: &Generator, function: &Function) -> Option<Self> {
        let Expression::Variable(returned) = function.return_expression.as_ref()? else {
            return None;
        };
        let returned_parameter = function.parameters.iter().find(|parameter| {
            parameter.name == *returned
                && matches!(parameter.parameter_type, Type::Float | Type::Double)
        })?;
        if !contains_loaded_float_sum_store(&function.statements) {
            return None;
        }
        let float = physical_parameter_register(
            generator,
            &returned_parameter.name,
            mwcc_vreg::Class::Float,
        )?
        .checked_add(1)?;
        let constant_address = function
            .parameters
            .iter()
            .filter_map(|parameter| {
                physical_parameter_register(generator, &parameter.name, mwcc_vreg::Class::General)
            })
            .max()?
            .checked_add(1)?;
        (float <= 13 && constant_address <= 12).then_some(Self {
            float,
            constant_address,
        })
    }
}

fn physical_parameter_register(
    generator: &Generator,
    name: &str,
    class: mwcc_vreg::Class,
) -> Option<u8> {
    let location = generator.locations.get(name)?;
    match mwcc_vreg::Reg::from_field(location.register, class) {
        mwcc_vreg::Reg::Physical(register) => Some(register),
        mwcc_vreg::Reg::Virtual(_) => None,
    }
}

fn contains_loaded_float_sum_store(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Store {
            value:
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    left,
                    right,
                },
            ..
        } => is_float_member(left) && is_float_member(right),
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            contains_loaded_float_sum_store(then_body) || contains_loaded_float_sum_store(else_body)
        }
        _ => false,
    })
}

fn is_float_member(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Member {
            member_type: Type::Float | Type::Double,
            ..
        }
    )
}
