//! Scalar temporaries materialized while analyzing discarded C++ inlines.
//!
//! Legacy mwcceppc assigns anonymous ordinals to rvalues bound to scalar
//! `const T&` parameters, even when the containing inline body is later
//! discarded.  The parser retains both resolved call identities and their
//! source reference masks; this pass counts the semantic binding sites without
//! mixing compiler-version weights into parsing.

use mwcc_syntax_trees::{
    ArmBody, Expression, Function, GuardedReturn, LocalDeclaration, Statement, TranslationUnit,
    Type,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Default, PartialEq)]
pub struct Analysis {
    pub binding_count: usize,
    /// Verified scalar images which old mwcceppc retains in initialized data
    /// after discarding the inline body. Non-literal rvalues still contribute
    /// to ordinal accounting but cannot be serialized from the retained IR.
    pub materialized_float_words: Vec<u32>,
}

pub fn analyze(unit: &TranslationUnit) -> Analysis {
    let mut seen = HashSet::new();
    unit.skipped_inline_definitions
        .iter()
        .filter(|function| seen.insert(function.name.as_str()))
        .map(|function| analyze_function(function, &unit.cxx_const_reference_parameter_types))
        .sum()
}

impl std::ops::Add for Analysis {
    type Output = Self;

    fn add(mut self, other: Self) -> Self {
        self.binding_count += other.binding_count;
        self.materialized_float_words
            .extend(other.materialized_float_words);
        self
    }
}

impl std::iter::Sum for Analysis {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), std::ops::Add::add)
    }
}

fn analyze_function(
    function: &Function,
    bindings: &HashMap<String, Vec<Option<Type>>>,
) -> Analysis {
    function
        .locals
        .iter()
        .map(|local| analyze_local(local, bindings))
        .sum::<Analysis>()
        + function
            .statements
            .iter()
            .map(|statement| analyze_statement(statement, bindings))
            .sum::<Analysis>()
        + function
            .guards
            .iter()
            .map(|guard| analyze_guard(guard, bindings))
            .sum::<Analysis>()
        + function
            .return_expression
            .as_ref()
            .map_or_else(Analysis::default, |expression| {
                analyze_expression(expression, bindings)
            })
}

fn analyze_local(
    local: &LocalDeclaration,
    bindings: &HashMap<String, Vec<Option<Type>>>,
) -> Analysis {
    local
        .initializer
        .as_ref()
        .map_or_else(Analysis::default, |expression| {
            analyze_expression(expression, bindings)
        })
}

fn analyze_guard(guard: &GuardedReturn, bindings: &HashMap<String, Vec<Option<Type>>>) -> Analysis {
    analyze_expression(&guard.condition, bindings) + analyze_expression(&guard.value, bindings)
}

fn analyze_arm(arm: &ArmBody, bindings: &HashMap<String, Vec<Option<Type>>>) -> Analysis {
    match arm {
        ArmBody::Return(expression) => analyze_expression(expression, bindings),
        ArmBody::Statements(statements) => statements
            .iter()
            .map(|statement| analyze_statement(statement, bindings))
            .sum(),
    }
}

fn analyze_statement(
    statement: &Statement,
    bindings: &HashMap<String, Vec<Option<Type>>>,
) -> Analysis {
    match statement {
        Statement::Store { target, value } => {
            analyze_expression(target, bindings) + analyze_expression(value, bindings)
        }
        Statement::Assign { value, .. } | Statement::Expression(value) => {
            analyze_expression(value, bindings)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            analyze_expression(condition, bindings)
                + then_body
                    .iter()
                    .map(|statement| analyze_statement(statement, bindings))
                    .sum::<Analysis>()
                + else_body
                    .iter()
                    .map(|statement| analyze_statement(statement, bindings))
                    .sum::<Analysis>()
        }
        Statement::Return(expression) => expression
            .as_ref()
            .map_or_else(Analysis::default, |expression| {
                analyze_expression(expression, bindings)
            }),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            analyze_expression(scrutinee, bindings)
                + arms
                    .iter()
                    .map(|arm| analyze_arm(&arm.body, bindings))
                    .sum::<Analysis>()
                + default
                    .as_ref()
                    .map_or_else(Analysis::default, |arm| analyze_arm(arm, bindings))
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            initializer
                .as_ref()
                .map_or_else(Analysis::default, |expression| {
                    analyze_expression(expression, bindings)
                })
                + condition
                    .as_ref()
                    .map_or_else(Analysis::default, |expression| {
                        analyze_expression(expression, bindings)
                    })
                + step.as_ref().map_or_else(Analysis::default, |expression| {
                    analyze_expression(expression, bindings)
                })
                + body
                    .iter()
                    .map(|statement| analyze_statement(statement, bindings))
                    .sum::<Analysis>()
        }
        Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => {
            Analysis::default()
        }
    }
}

fn analyze_call_bindings(
    name: &str,
    arguments: &[Expression],
    bindings: &HashMap<String, Vec<Option<Type>>>,
) -> Analysis {
    let Some(mask) = bindings.get(name) else {
        return Analysis::default();
    };
    let mask = if mask.len() == arguments.len() + 1 && mask.first() == Some(&None) {
        &mask[1..]
    } else {
        mask.as_slice()
    };
    let mut analysis = Analysis::default();
    for (argument, temporary_type) in arguments.iter().zip(mask) {
        let Some(temporary_type) = temporary_type else {
            continue;
        };
        if is_lvalue(argument) {
            continue;
        }
        analysis.binding_count += 1;
        if *temporary_type == Type::Float {
            if let Expression::FloatLiteral(value) = argument {
                analysis
                    .materialized_float_words
                    .push((*value as f32).to_bits());
            }
        }
    }
    analysis
}

fn is_lvalue(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Variable(_)
            | Expression::Dereference { .. }
            | Expression::Index { .. }
            | Expression::Member { .. }
    )
}

fn analyze_expression(
    expression: &Expression,
    bindings: &HashMap<String, Vec<Option<Type>>>,
) -> Analysis {
    match expression {
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => Analysis::default(),
        Expression::AggregateLiteral(elements) => elements
            .iter()
            .map(|element| analyze_expression(element, bindings))
            .sum(),
        Expression::Binary { left, right, .. }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => {
            analyze_expression(left, bindings) + analyze_expression(right, bindings)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::PostStep {
            target: operand, ..
        } => analyze_expression(operand, bindings),
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            analyze_expression(condition, bindings)
                + analyze_expression(when_true, bindings)
                + analyze_expression(when_false, bindings)
        }
        Expression::BitFieldRead {
            extracted, storage, ..
        } => analyze_expression(extracted, bindings) + analyze_expression(storage, bindings),
        Expression::Index { base, index } => {
            analyze_expression(base, bindings) + analyze_expression(index, bindings)
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            analyze_expression(base, bindings)
        }
        Expression::CallThrough { target, arguments } => {
            analyze_expression(target, bindings)
                + arguments
                    .iter()
                    .map(|argument| analyze_expression(argument, bindings))
                    .sum::<Analysis>()
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            analyze_expression(object, bindings)
                + arguments
                    .iter()
                    .map(|argument| analyze_expression(argument, bindings))
                    .sum::<Analysis>()
        }
        Expression::ConstructedNew {
            allocation,
            constructor,
            arguments,
            ..
        } => {
            analyze_expression(allocation, bindings)
                + analyze_call_bindings(constructor, arguments, bindings)
                + arguments
                    .iter()
                    .map(|argument| analyze_expression(argument, bindings))
                    .sum::<Analysis>()
        }
        Expression::Call { name, arguments } => {
            analyze_call_bindings(name, arguments, bindings)
                + arguments
                    .iter()
                    .map(|argument| analyze_expression(argument, bindings))
                    .sum::<Analysis>()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::BinaryOperator;

    #[test]
    fn counts_only_rvalues_at_const_reference_positions() {
        let bindings = HashMap::from([(
            "set__1VFRCfRCf".to_string(),
            vec![None, Some(Type::Float), Some(Type::Float)],
        )]);
        let expression = Expression::Call {
            name: "set__1VFRCfRCf".to_string(),
            arguments: vec![
                Expression::Variable("this".to_string()),
                Expression::Variable("plain".to_string()),
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("left".to_string())),
                    right: Box::new(Expression::Variable("right".to_string())),
                },
            ],
        };

        let analysis = analyze_expression(&expression, &bindings);
        assert_eq!(analysis.binding_count, 1);
        assert!(analysis.materialized_float_words.is_empty());
    }

    #[test]
    fn retains_literal_float_images_in_argument_order() {
        let bindings = HashMap::from([(
            "set__1VFRCfRCf".to_string(),
            vec![None, Some(Type::Float), Some(Type::Float)],
        )]);
        let expression = Expression::Call {
            name: "set__1VFRCfRCf".to_string(),
            arguments: vec![
                Expression::Variable("this".to_string()),
                Expression::FloatLiteral(0.0),
                Expression::FloatLiteral(1.25),
            ],
        };

        let analysis = analyze_expression(&expression, &bindings);
        assert_eq!(analysis.binding_count, 2);
        assert_eq!(
            analysis.materialized_float_words,
            vec![0.0f32.to_bits(), 1.25f32.to_bits()]
        );
    }
}
