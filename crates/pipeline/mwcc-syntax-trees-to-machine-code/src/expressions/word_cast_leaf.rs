//! Transparent word casts used as ordinary general-register operands.
//!
//! A cast between a 32-bit pointer/address and a 32-bit integer changes the
//! arithmetic type but not the register value. Operand placement must retain
//! that distinction: pointer arithmetic scales an uncast pointer, while an
//! explicit `(u32)pointer` is a raw integer leaf and needs no materialized copy.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn transparent_word_cast_register(
        &self,
        expression: &Expression,
    ) -> Option<u8> {
        let variable = match expression {
            Expression::Variable(name) => name,
            Expression::Cast {
                target_type,
                operand,
            } if is_general_word_type(*target_type) => match operand.as_ref() {
                Expression::Variable(name) => name,
                _ => return None,
            },
            _ => return None,
        };
        let location = self.locations.get(variable)?;
        (location.class == ValueClass::General && location.width == 32)
            .then_some(location.register)
    }
}

fn is_general_word_type(value_type: Type) -> bool {
    matches!(
        value_type,
        Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use mwcc_machine_code::MachineFunction;
    use mwcc_syntax_trees::{
        Function, InlineExpansionFacts, Parameter, SourceFundamentalType,
    };
    use mwcc_versions::{CompilerConfig, GC_2_0};

    use super::*;
    use crate::{InlineBodySet, InlineSummaries, lower_function};

    fn lower(return_expression: Expression, parameters: Vec<Parameter>) -> MachineFunction {
        let function = Function {
            return_type: Type::UnsignedInt,
            name: "address".into(),
            is_static: false,
            is_weak: false,
            parameters,
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(return_expression),
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
        lower_function(
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
        .expect("word-cast expression should lower")
    }

    fn pointer_parameter() -> Parameter {
        Parameter {
            parameter_type: Type::StructPointer { element_size: 8 },
            name: "chunk".into(),
        }
    }

    fn cast_chunk() -> Expression {
        Expression::Cast {
            target_type: Type::UnsignedInt,
            operand: Box::new(Expression::Variable("chunk".into())),
        }
    }

    #[test]
    fn a_word_cast_pointer_stays_in_place_beside_its_member_load() {
        let machine = lower(
            Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(cast_chunk()),
                right: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable("chunk".into())),
                    offset: 4,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                }),
            },
            vec![pointer_parameter()],
        );

        assert_eq!(
            machine.instructions,
            vec![
                Instruction::LoadWord {
                    d: 0,
                    a: 3,
                    offset: 4,
                },
                Instruction::Add { d: 3, a: 3, b: 0 },
                Instruction::BranchToLinkRegister,
            ]
        );
    }

    #[test]
    fn a_word_cast_pointer_stays_in_place_beside_an_integer_leaf() {
        let machine = lower(
            Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(cast_chunk()),
                right: Box::new(Expression::Variable("offset".into())),
            },
            vec![
                pointer_parameter(),
                Parameter {
                    parameter_type: Type::UnsignedInt,
                    name: "offset".into(),
                },
            ],
        );

        assert_eq!(
            machine.instructions,
            vec![
                Instruction::Add { d: 3, a: 3, b: 4 },
                Instruction::BranchToLinkRegister,
            ]
        );
    }
}
