//! Affine indexed stores through pointer-valued aggregate members.
//!
//! MWCC treats `object->words[index + constant] = left +/- right` as one
//! address/value transaction: it loads the member pointer, pre-scales the
//! variable index into r0, computes the leaf arithmetic in a separate value
//! lane, adds the scaled index to the pointer, and folds the constant term into
//! the final store displacement.

#[allow(unused_imports)]
use super::*;

fn affine_leaf_index(index: &Expression) -> Option<(&Expression, i64)> {
    let Expression::Binary {
        operator,
        left,
        right,
    } = index
    else {
        return None;
    };
    match operator {
        BinaryOperator::Add => {
            if let Some(constant) = constant_value(right) {
                leaf_name(left).map(|_| (left.as_ref(), constant))
            } else if let Some(constant) = constant_value(left) {
                leaf_name(right).map(|_| (right.as_ref(), constant))
            } else {
                None
            }
        }
        BinaryOperator::Subtract => constant_value(right)
            .and_then(|constant| leaf_name(left).map(|_| (left.as_ref(), -constant))),
        _ => None,
    }
}

impl Generator {
    pub(crate) fn try_emit_affine_member_pointer_leaf_arithmetic_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Expression::Index { base, index } = target else {
            return Ok(false);
        };
        let Expression::Member {
            member_type: Type::Pointer(pointee @ (Pointee::Int | Pointee::UnsignedInt)),
            ..
        } = base.as_ref()
        else {
            return Ok(false);
        };
        let Some((index, constant)) = affine_leaf_index(index) else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: operator @ (BinaryOperator::Add | BinaryOperator::Subtract),
            left,
            right,
        } = value
        else {
            return Ok(false);
        };
        let (Some(left), Some(right)) = (
            leaf_name(left).and_then(|name| self.lookup_general(name)),
            leaf_name(right).and_then(|name| self.lookup_general(name)),
        ) else {
            return Ok(false);
        };

        let address = self.fresh_virtual_general_preferring(3);
        self.evaluate_general(base, address)?;
        let index = self.general_register_of_leaf(index)?;
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: GENERAL_SCRATCH,
                s: index,
                shift: pointee.size().trailing_zeros() as u8,
            });

        let source = self.fresh_virtual_general_preferring(6);
        self.output.instructions.push(match operator {
            BinaryOperator::Add => Instruction::Add {
                d: source,
                a: left,
                b: right,
            },
            BinaryOperator::Subtract => Instruction::SubtractFrom {
                d: source,
                a: right,
                b: left,
            },
            _ => unreachable!("pattern restricts affine store arithmetic"),
        });
        self.output.instructions.push(Instruction::Add {
            d: address,
            a: address,
            b: GENERAL_SCRATCH,
        });
        let displacement = constant
            .checked_mul(i64::from(pointee.size()))
            .and_then(|offset| i16::try_from(offset).ok())
            .ok_or_else(|| Diagnostic::error("affine pointer-store displacement is out of range"))?;
        self.output.instructions.push(displacement_store(
            *pointee,
            source,
            address,
            displacement,
        )?);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use mwcc_syntax_trees::{
        Function, InlineExpansionFacts, Parameter, SourceFundamentalType, Statement,
    };
    use mwcc_versions::{CompilerConfig, GC_2_0};

    use super::*;
    use crate::{lower_function, InlineBodySet, InlineSummaries};

    #[test]
    fn folds_an_affine_index_around_a_leaf_difference() {
        let target = Expression::Index {
            base: Box::new(Expression::Member {
                base: Box::new(Expression::Variable("this".into())),
                offset: 28,
                member_type: Type::Pointer(Pointee::Int),
                index_stride: None,
            }),
            index: Box::new(Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: Box::new(Expression::Variable("node".into())),
                right: Box::new(Expression::IntegerLiteral(1)),
            }),
        };
        let value = Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: Box::new(Expression::Variable("depth".into())),
            right: Box::new(Expression::Variable("stack".into())),
        };
        let function = Function {
            return_type: Type::Void,
            name: "store_depth".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 52 },
                    name: "this".into(),
                },
                Parameter {
                    parameter_type: Type::Int,
                    name: "node".into(),
                },
                Parameter {
                    parameter_type: Type::Int,
                    name: "depth".into(),
                },
                Parameter {
                    parameter_type: Type::Int,
                    name: "stack".into(),
                },
            ],
            locals: Vec::new(),
            statements: vec![Statement::Store { target, value }],
            guards: Vec::new(),
            return_expression: None,
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
            SourceFundamentalType::Void,
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
            &Default::default(),
            &Default::default(),
            &Default::default(),
            &Default::default(),
            &HashMap::new(),
            &HashMap::new(),
            &InlineBodySet::default(),
            &InlineSummaries::default(),
            InlineExpansionFacts::default(),
            &HashMap::new(),
            &fundamentals,
            config,
        )
        .expect("affine member-pointer store should lower");

        assert_eq!(
            machine.encode_text(),
            [
                0x8063_001c_u32,
                0x5480_103a,
                0x7cc6_2850,
                0x7c63_0214,
                0x90c3_fffc,
                0x4e80_0020,
            ]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>()
        );
    }
}
