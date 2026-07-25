//! Semantic lowering for aggregate results copied from a local object.
//!
//! The source tree deliberately omits the EABI hidden-result parameter.  Make
//! it explicit before structured-body planning so the ordinary allocator can
//! preserve it across calls and the ordinary frame owner can place the source
//! aggregate.  This is not a fixed instruction schedule: it only exposes the
//! ABI data flow which was implicit in the source-level function.

use super::*;

pub(super) const HIDDEN_RESULT_NAME: &str = "__mwcc_hidden_aggregate_result";

pub(super) fn lower_local_aggregate_return(function: &Function) -> Option<Function> {
    let Type::Struct { size, align } = function.return_type else {
        return None;
    };
    let Expression::Variable(result) = function.return_expression.as_ref()? else {
        return None;
    };
    let local = function.locals.iter().find(|local| {
        local.name == *result
            && !local.is_static
            && local.array_length.is_none()
            && local.declared_type == function.return_type
    })?;
    if local.is_volatile
        || size == 0
        || size % 4 != 0
        || align < 4
        || function
            .parameters
            .iter()
            .any(|parameter| parameter.name == HIDDEN_RESULT_NAME)
        || function
            .locals
            .iter()
            .any(|local| local.name == HIDDEN_RESULT_NAME)
    {
        return None;
    }

    let mut lowered = function.clone();
    lowered.return_type = Type::Void;
    lowered.return_expression = None;
    lowered.parameters.insert(
        0,
        mwcc_syntax_trees::Parameter {
            parameter_type: Type::Pointer(Pointee::UnsignedInt),
            name: HIDDEN_RESULT_NAME.into(),
        },
    );
    for offset in (0..size).step_by(4) {
        lowered.statements.push(Statement::Store {
            target: Expression::Member {
                base: Box::new(Expression::Variable(HIDDEN_RESULT_NAME.into())),
                offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
            value: Expression::Member {
                base: Box::new(Expression::Variable(result.clone())),
                offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
        });
    }
    Some(lowered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_a_word_aligned_local_result_as_tail_copies() {
        let function = Function {
            return_type: Type::Struct { size: 12, align: 4 },
            name: "make_vector".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![LocalDeclaration {
                declared_type: Type::Struct { size: 12, align: 4 },
                name: "result".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            }],
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("result".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        let lowered = lower_local_aggregate_return(&function).expect("local aggregate result");
        assert_eq!(lowered.return_type, Type::Void);
        assert_eq!(lowered.parameters[0].name, HIDDEN_RESULT_NAME);
        assert_eq!(lowered.statements.len(), 3);
        assert!(lowered.return_expression.is_none());
    }
}
