//! Constant-valued indexed stores through pointer members.
//!
//! A multi-byte indexed store consumes r0 for its scaled index, so its constant
//! value needs an allocator-backed lane of its own. Byte stores remain on the
//! ordinary path because they can use r0 directly without scaling.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn try_emit_member_pointer_constant_indexed_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Expression::Index { base, index } = target else {
            return Ok(false);
        };
        let Expression::Member {
            member_type: Type::Pointer(pointee),
            ..
        } = base.as_ref()
        else {
            return Ok(false);
        };
        let Some(constant) = constant_value(value) else {
            return Ok(false);
        };
        if pointee.size() <= 1
            || matches!(
                pointee,
                Pointee::Float
                    | Pointee::Double
                    | Pointee::LongLong
                    | Pointee::UnsignedLongLong
                    | Pointee::Pointer
                    | Pointee::WordPointer
            )
            || leaf_name(index).is_none()
        {
            return Ok(false);
        }

        let address = self.fresh_virtual_general_preferring(3);
        self.evaluate_general(base, address)?;
        let source = self.fresh_virtual_general_preferring(4);
        self.load_integer_constant(source, constant);
        let index = self.general_register_of_leaf(index)?;
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: GENERAL_SCRATCH,
                s: index,
                shift: pointee.size().trailing_zeros() as u8,
            });
        self.output.instructions.push(indexed_store(
            *pointee,
            source,
            address,
            GENERAL_SCRATCH,
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
    fn retains_a_word_constant_while_scaling_the_member_index() {
        let function = Function {
            return_type: Type::Void,
            name: "store_flag".into(),
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
            ],
            locals: Vec::new(),
            statements: vec![Statement::Store {
                target: Expression::Index {
                    base: Box::new(Expression::Member {
                        base: Box::new(Expression::Variable("this".into())),
                        offset: 28,
                        member_type: Type::Pointer(Pointee::Int),
                        index_stride: None,
                    }),
                    index: Box::new(Expression::Variable("node".into())),
                },
                value: Expression::IntegerLiteral(1),
            }],
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
        .expect("constant member-pointer store should lower");

        assert_eq!(
            machine.encode_text(),
            [
                0x8063_001c_u32,
                0x38a0_0001,
                0x5480_103a,
                0x7ca3_012e,
                0x4e80_0020,
            ]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>()
        );
    }
}
