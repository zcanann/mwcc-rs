//! Complemented logical operands selected as native PowerPC instructions.
//!
//! `andc` and `orc` consume the uncomplemented source directly. Selecting the
//! complement as a standalone `not` first loses that relationship and leaves a
//! redundant instruction for a late peephole to rediscover. Keep the expression
//! identity here, including when the complemented operand is itself computed.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn try_emit_complement_logical(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if !matches!(operator, BinaryOperator::BitAnd | BinaryOperator::BitOr) {
            return Ok(false);
        }

        // Both operands complemented — De Morgan folds to a single op: `~a &
        // ~b` is `nor(a,b)` and `~a | ~b` is `nand(a,b)`.
        if let (Some(left_name), Some(right_name)) =
            (complemented_leaf_name(left), complemented_leaf_name(right))
        {
            if let (Some(left_register), Some(right_register)) = (
                self.lookup_general(left_name),
                self.lookup_general(right_name),
            ) {
                self.output.instructions.push(match operator {
                    BinaryOperator::BitAnd => Instruction::Nor {
                        a: destination,
                        s: left_register,
                        b: right_register,
                    },
                    BinaryOperator::BitOr => Instruction::Nand {
                        a: destination,
                        s: left_register,
                        b: right_register,
                    },
                    _ => unreachable!(),
                });
                return Ok(true);
            }
        }

        let Some((kept_expression, complemented_operand)) =
            split_complemented_operand(left, right)
        else {
            return Ok(false);
        };
        let Some(kept_register) = leaf_name(kept_expression)
            .and_then(|name| self.lookup_general(name))
            .filter(|register| *register != GENERAL_SCRATCH)
        else {
            return Ok(false);
        };

        let complemented_register = if let Some(name) = leaf_name(complemented_operand) {
            let Some(register) = self.lookup_general(name) else {
                return Ok(false);
            };
            register
        } else {
            // A call or assignment can invalidate the retained leaf and imposes
            // source sequencing beyond this commutative logical operation. Pure
            // arithmetic is safe to compute into r0 before the native combine.
            if expression_has_side_effect(complemented_operand) {
                return Ok(false);
            }
            self.evaluate_general(complemented_operand, GENERAL_SCRATCH)?;
            GENERAL_SCRATCH
        };

        self.output.instructions.push(match operator {
            BinaryOperator::BitAnd => Instruction::AndComplement {
                a: destination,
                s: kept_register,
                b: complemented_register,
            },
            BinaryOperator::BitOr => Instruction::OrComplement {
                a: destination,
                s: kept_register,
                b: complemented_register,
            },
            _ => unreachable!(),
        });
        Ok(true)
    }
}

fn split_complemented_operand<'a>(
    left: &'a Expression,
    right: &'a Expression,
) -> Option<(&'a Expression, &'a Expression)> {
    bit_not_operand(right)
        .map(|operand| (left, operand))
        .or_else(|| bit_not_operand(left).map(|operand| (right, operand)))
}

fn bit_not_operand(expression: &Expression) -> Option<&Expression> {
    match expression {
        Expression::Unary {
            operator: UnaryOperator::BitNot,
            operand,
        } => Some(operand),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use mwcc_syntax_trees::{
        Function, InlineExpansionFacts, Parameter, SourceFundamentalType, Type,
    };
    use mwcc_versions::{CompilerConfig, GC_2_0};

    use super::*;
    use crate::{InlineBodySet, InlineSummaries, lower_function};

    #[test]
    fn a_computed_complement_is_consumed_by_andc_without_a_not() {
        let function = Function {
            return_type: Type::UnsignedInt,
            name: "mask".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::UnsignedInt,
                    name: "value".into(),
                },
                Parameter {
                    parameter_type: Type::UnsignedInt,
                    name: "alignment".into(),
                },
            ],
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left: Box::new(Expression::Variable("value".into())),
                right: Box::new(Expression::Unary {
                    operator: UnaryOperator::BitNot,
                    operand: Box::new(Expression::Binary {
                        operator: BinaryOperator::Subtract,
                        left: Box::new(Expression::Variable("alignment".into())),
                        right: Box::new(Expression::IntegerLiteral(1)),
                    }),
                }),
            }),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let fundamentals = HashMap::from([(
            function.name.clone(),
            SourceFundamentalType::UnsignedInteger,
        )]);
        let mut config = CompilerConfig::new(GC_2_0);
        config.flags.cpp_exceptions = false;

        let machine = lower_function(
            &function,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &InlineBodySet::default(),
            &InlineSummaries::default(),
            InlineExpansionFacts::default(),
            &HashMap::new(),
            &fundamentals,
            config,
        )
        .expect("computed complement should lower");

        assert_eq!(
            machine.instructions,
            vec![
                Instruction::AddImmediate {
                    d: 0,
                    a: 4,
                    immediate: -1,
                },
                Instruction::AndComplement { a: 3, s: 3, b: 0 },
                Instruction::BranchToLinkRegister,
            ]
        );
    }
}
