//! Expose ABI-backed casts before structured call liveness and frame planning.

use super::structured_expression_visit::{
    rewrite_expression, rewrite_statement, visit_statement_nodes_mut,
};
use crate::generator::Generator;
use crate::runtime_conversions::FLOAT_TO_UNSIGNED;
use mwcc_syntax_trees::{
    ArmBody, BinaryOperator, Expression, Function, Pointee, Statement, Type, UnaryOperator,
};
use std::collections::HashMap;

/// Type evidence for scalar conversion operands, independent of register homes.
/// Structured locals have declarations before their locations are allocated.
struct ConversionTypes<'a> {
    values: HashMap<String, Type>,
    returns: &'a HashMap<String, Type>,
}

impl ConversionTypes<'_> {
    fn value_type(&self, expression: &Expression) -> Option<Type> {
        match expression {
            Expression::Variable(name) => self.values.get(name).copied(),
            Expression::FloatLiteral(_) => Some(Type::Double),
            Expression::IntegerLiteral(_) => Some(Type::Int),
            Expression::Cast { target_type, .. }
            | Expression::Member {
                member_type: target_type,
                ..
            }
            | Expression::VirtualCall {
                return_type: target_type,
                ..
            } => Some(*target_type),
            Expression::Call { name, .. } => self.returns.get(name).copied(),
            Expression::Dereference { pointer } | Expression::Index { base: pointer, .. } => {
                match self.value_type(pointer)? {
                    Type::Pointer(Pointee::Float) => Some(Type::Float),
                    Type::Pointer(Pointee::Double) => Some(Type::Double),
                    Type::Pointer(Pointee::UnsignedInt) => Some(Type::UnsignedInt),
                    _ => None,
                }
            }
            Expression::Unary {
                operator: UnaryOperator::Negate,
                operand,
            }
            | Expression::IndexedUpdateValue { value: operand }
            | Expression::PostStep {
                target: operand, ..
            } => self.value_type(operand),
            Expression::Unary { .. } => Some(Type::Int),
            Expression::Binary {
                operator,
                left,
                right,
            } if matches!(
                operator,
                BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
            ) =>
            {
                let left = self.value_type(left);
                let right = self.value_type(right);
                if matches!(left, Some(Type::Pointer(_))) {
                    return left;
                }
                if matches!(right, Some(Type::Pointer(_))) {
                    return right;
                }
                floating_common_type(left, right)
            }
            Expression::Binary { .. } => Some(Type::Int),
            Expression::Conditional {
                when_true,
                when_false,
                ..
            } => floating_common_type(self.value_type(when_true), self.value_type(when_false)),
            Expression::Comma { right, .. } => self.value_type(right),
            Expression::Assign { target, .. } => self.value_type(target),
            _ => None,
        }
    }

    fn implicit(
        &self,
        value: &Expression,
        target: Option<Type>,
        changed: &mut bool,
    ) -> Option<Expression> {
        if target != Some(Type::UnsignedInt) {
            return None;
        }
        self.convert(
            &Expression::Cast {
                target_type: Type::UnsignedInt,
                operand: Box::new(value.clone()),
            },
            changed,
        )
    }

    fn convert(&self, expression: &Expression, changed: &mut bool) -> Option<Expression> {
        if let Expression::Assign { target, value } = expression {
            let converted = self.implicit(value, self.value_type(target), changed)?;
            return Some(Expression::Assign {
                target: Box::new(rewrite_expression(target, &mut |value| {
                    self.convert(value, changed)
                })),
                value: Box::new(converted),
            });
        }
        let Expression::Cast {
            target_type: Type::UnsignedInt,
            operand,
        } = expression
        else {
            return None;
        };
        if !matches!(self.value_type(operand), Some(Type::Float | Type::Double)) {
            return None;
        }
        *changed = true;
        // Recurse into the argument as well: replacing an outer cast must not
        // hide a second ABI call inside a nested floating expression.
        let argument = rewrite_expression(operand, &mut |value| self.convert(value, changed));
        Some(Expression::Call {
            name: FLOAT_TO_UNSIGNED.into(),
            arguments: vec![argument],
        })
    }
}

fn floating_common_type(left: Option<Type>, right: Option<Type>) -> Option<Type> {
    if left == Some(Type::Double) || right == Some(Type::Double) {
        Some(Type::Double)
    } else if left == Some(Type::Float) || right == Some(Type::Float) {
        Some(Type::Float)
    } else {
        None
    }
}

fn expose(function: &Function, types: &ConversionTypes<'_>) -> Option<Function> {
    let mut changed = false;
    let mut rewrite = |expression: &Expression| types.convert(expression, &mut changed);
    let mut lowered = function.clone();
    for local in &mut lowered.locals {
        local.initializer = local
            .initializer
            .as_ref()
            .map(|value| rewrite_expression(value, &mut rewrite));
    }
    lowered.statements = function
        .statements
        .iter()
        .map(|statement| rewrite_statement(statement, &mut rewrite))
        .collect();
    for guard in &mut lowered.guards {
        guard.condition = rewrite_expression(&guard.condition, &mut rewrite);
        guard.value = rewrite_expression(&guard.value, &mut rewrite);
    }
    lowered.return_expression = function
        .return_expression
        .as_ref()
        .map(|value| rewrite_expression(value, &mut rewrite));
    // Statement assignment targets and return types live outside expression
    // trees. Apply their contextual conversions after the shared expression walk.
    let mut convert_value = |value: &mut Expression, target: Option<Type>| {
        if let Some(converted) = types.implicit(value, target, &mut changed) {
            *value = converted;
        }
    };
    for local in &mut lowered.locals {
        if local.array_length.is_none() && !local.is_static {
            if let Some(value) = &mut local.initializer {
                convert_value(value, Some(local.declared_type));
            }
        }
    }
    for statement in &mut lowered.statements {
        visit_statement_nodes_mut(statement, &mut |statement| match statement {
            Statement::Assign { name, value } => {
                convert_value(value, types.values.get(name).copied())
            }
            Statement::Store { target, value } => convert_value(value, types.value_type(target)),
            Statement::Return(Some(value)) => convert_value(value, Some(function.return_type)),
            Statement::Switch { arms, default, .. } => {
                for arm in arms.iter_mut().map(|arm| &mut arm.body).chain(default) {
                    if let ArmBody::Return(value) = arm {
                        convert_value(value, Some(function.return_type));
                    }
                }
            }
            _ => {}
        });
    }
    for guard in &mut lowered.guards {
        convert_value(&mut guard.value, Some(function.return_type));
    }
    if let Some(value) = &mut lowered.return_expression {
        convert_value(value, Some(function.return_type));
    }
    changed.then_some(lowered)
}

impl Generator {
    pub(crate) fn expose_structured_runtime_conversions(
        &mut self,
        function: &Function,
    ) -> Option<Function> {
        let mut values = self.globals.clone();
        for name in self.global_array_sizes.keys() {
            if let Some(ty) = values.get_mut(name) {
                *ty = array_decay(*ty);
            }
        }
        values.extend(
            function
                .parameters
                .iter()
                .map(|parameter| (parameter.name.clone(), parameter.parameter_type)),
        );
        values.extend(function.locals.iter().map(|local| {
            (
                local.name.clone(),
                if local.array_length.is_some() {
                    array_decay(local.declared_type)
                } else {
                    local.declared_type
                },
            )
        }));
        let lowered = expose(
            function,
            &ConversionTypes {
                values,
                returns: &self.call_return_types,
            },
        )?;
        self.call_return_types
            .insert(FLOAT_TO_UNSIGNED.into(), Type::UnsignedInt);
        self.call_parameter_types
            .insert(FLOAT_TO_UNSIGNED.into(), vec![Type::Double]);
        if !self
            .compiler_generated_symbols
            .iter()
            .any(|name| name == FLOAT_TO_UNSIGNED)
        {
            self.compiler_generated_symbols
                .push(FLOAT_TO_UNSIGNED.into());
        }
        Some(lowered)
    }
}

fn array_decay(ty: Type) -> Type {
    match ty {
        Type::Float => Type::Pointer(Pointee::Float),
        Type::Double => Type::Pointer(Pointee::Double),
        _ => ty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cast(operand: Expression) -> Expression {
        Expression::Cast {
            target_type: Type::UnsignedInt,
            operand: Box::new(operand),
        }
    }
    #[test]
    fn comparisons_and_logical_not_do_not_introduce_runtime_calls() {
        let returns = HashMap::new();
        let types = ConversionTypes {
            values: HashMap::from([("x".into(), Type::Float)]),
            returns: &returns,
        };
        for operand in [
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand: Box::new(Expression::Variable("x".into())),
            },
            Expression::Binary {
                operator: BinaryOperator::Less,
                left: Box::new(Expression::Variable("x".into())),
                right: Box::new(Expression::FloatLiteral(1.0)),
            },
        ] {
            let mut changed = false;
            assert!(types.convert(&cast(operand), &mut changed).is_none());
            assert!(!changed);
        }
    }
    #[test]
    fn declaration_types_cover_float_locals_and_double_members() {
        let returns = HashMap::new();
        let types = ConversionTypes {
            values: HashMap::from([
                ("x".into(), Type::Float),
                ("p".into(), Type::Pointer(Pointee::Double)),
            ]),
            returns: &returns,
        };
        for operand in [
            Expression::Variable("x".into()),
            Expression::Dereference {
                pointer: Box::new(Expression::Variable("p".into())),
            },
            Expression::Member {
                base: Box::new(Expression::Variable("record".into())),
                offset: 8,
                member_type: Type::Double,
                index_stride: None,
            },
        ] {
            assert!(
                matches!(types.convert(&cast(operand), &mut false), Some(Expression::Call { name, .. }) if name == FLOAT_TO_UNSIGNED)
            );
        }
    }
    #[test]
    fn nested_conversions_are_exposed_in_the_same_walk() {
        let returns = HashMap::new();
        let types = ConversionTypes {
            values: HashMap::from([("x".into(), Type::Float)]),
            returns: &returns,
        };
        let expression = cast(Expression::Cast {
            target_type: Type::Double,
            operand: Box::new(cast(Expression::Variable("x".into()))),
        });
        let converted = types.convert(&expression, &mut false).unwrap();
        let Expression::Call { arguments, .. } = &converted else {
            panic!("outer call");
        };
        assert!(
            matches!(&arguments[0], Expression::Cast { operand, .. } if matches!(operand.as_ref(), Expression::Call { name, .. } if name == FLOAT_TO_UNSIGNED))
        );
        assert!(!types.convert(&converted, &mut false).is_some());
    }
    #[test]
    fn implicit_conversion_uses_destination_type() {
        let returns = HashMap::new();
        let types = ConversionTypes {
            values: HashMap::from([("x".into(), Type::Float), ("y".into(), Type::UnsignedInt)]),
            returns: &returns,
        };
        let value = Expression::Variable("x".into());
        for target in [
            None,
            Some(Type::Int),
            Some(Type::Float),
            Some(Type::UnsignedChar),
        ] {
            assert!(types.implicit(&value, target, &mut false).is_none());
        }
        assert!(matches!(
            types.implicit(&value, Some(Type::UnsignedInt), &mut false),
            Some(Expression::Call { .. })
        ));
        let assigned = Expression::Assign {
            target: Box::new(Expression::Variable("y".into())),
            value: Box::new(value),
        };
        assert!(
            matches!(types.convert(&assigned, &mut false), Some(Expression::Assign { value, .. }) if matches!(value.as_ref(), Expression::Call { .. }))
        );
    }
}
