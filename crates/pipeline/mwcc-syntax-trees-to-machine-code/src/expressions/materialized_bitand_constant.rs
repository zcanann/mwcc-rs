//! Non-contiguous signed-immediate masks materialized through the scratch.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Emit `leaf & C` when `C` is a signed 16-bit mask that cannot be represented
    /// by one `rlwinm`.
    ///
    /// PowerPC has no non-recording `andi`, and a negative mask such as
    /// `~0x28` must retain its high bits after integer promotion. MWCC therefore
    /// materializes it in r0 and uses a register AND:
    ///
    /// ```text
    /// li  r0,-41
    /// and result,leaf,r0
    /// ```
    ///
    /// Keep this separate from the immediate-form selector: this is a
    /// two-instruction register operation whose scratch lifetime is visible to
    /// allocation and scheduling.
    pub(crate) fn try_emit_materialized_bitand_constant(
        &mut self,
        operator: BinaryOperator,
        variable: &Expression,
        constant: i64,
        destination: u8,
    ) -> Compilation<bool> {
        if operator != BinaryOperator::BitAnd
            || i16::try_from(constant).is_err()
            || constant >= 0
            || rlwinm_mask(constant).is_some()
        {
            return Ok(false);
        }
        let Ok((source, _, _)) = self.leaf_info(variable) else {
            return Ok(false);
        };
        if source == GENERAL_SCRATCH {
            return Ok(false);
        }

        self.load_integer_constant(GENERAL_SCRATCH, constant);
        self.output.instructions.push(Instruction::And {
            a: destination,
            s: source,
            b: GENERAL_SCRATCH,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use mwcc_syntax_trees::{
        Function, InlineExpansionFacts, Parameter, SourceFundamentalType, Type,
    };
    use mwcc_versions::{CompilerConfig, GC_3_0A3};

    use super::*;
    use crate::{lower_function, InlineBodySet, InlineSummaries};

    #[test]
    fn a_non_contiguous_negative_mask_is_materialized_in_r0() {
        let function = Function {
            return_type: Type::Int,
            name: "f".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Int,
                name: "x".into(),
            }],
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left: Box::new(Expression::Variable("x".into())),
                right: Box::new(Expression::IntegerLiteral(-41)),
            }),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let fundamentals =
            HashMap::from([(function.name.clone(), SourceFundamentalType::SignedInteger)]);
        let mut config = CompilerConfig::new(GC_3_0A3);
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
        .expect("materialized mask should lower");

        assert_eq!(
            machine.encode_text(),
            [0x3800_ffd7_u32, 0x7c63_0038, 0x4e80_0020,]
                .into_iter()
                .flat_map(u32::to_be_bytes)
                .collect::<Vec<_>>()
        );
    }
}
