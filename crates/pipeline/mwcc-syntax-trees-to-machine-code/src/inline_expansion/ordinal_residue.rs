//! Anonymous ordinal residue left by inline substitutions.
//!
//! The optimizer's eliminated value nodes remain visible through later `@N`
//! pool symbols even though neither the call nor those nodes survive codegen.

use mwcc_syntax_trees::{ArmBody, Expression, Function, InlineExpansionFacts, Statement};

pub(super) fn ordinal_residue(
    facts: InlineExpansionFacts,
    statement_body_substitutions: usize,
    value_body_substitutions: usize,
    statement_body_weight: u8,
) -> u32 {
    facts.leading_initializer_substitutions as u32
        + u32::from(statement_body_weight) * statement_body_substitutions as u32
        + 3 * value_body_substitutions as u32
}

/// Build 163 retains both sides of the value graph when an inlined conditional
/// value clears a bitfield inside a caller-owned scratch frame. The call is
/// gone before instruction selection, but its six optimizer nodes precede the
/// translation unit's first pooled literal.
pub(super) fn legacy_mutating_value_body_ordinal_residue(
    function: &Function,
    substitutions: usize,
) -> u32 {
    if substitutions == 0
        || !function
            .locals
            .iter()
            .any(|local| local.array_length.is_some())
    {
        return 0;
    }
    let mutating_substitutions = function
        .statements
        .iter()
        .map(statement_bitfield_store_count)
        .sum::<usize>()
        .min(substitutions);
    6 * mutating_substitutions as u32
}

fn statement_bitfield_store_count(statement: &Statement) -> usize {
    match statement {
        Statement::Store { target, .. } => {
            usize::from(matches!(target, Expression::BitFieldRead { .. }))
        }
        Statement::If {
            then_body,
            else_body,
            ..
        } => then_body
            .iter()
            .chain(else_body)
            .map(statement_bitfield_store_count)
            .sum(),
        Statement::Loop { body, .. } => body.iter().map(statement_bitfield_store_count).sum(),
        Statement::Switch { arms, default, .. } => {
            arms.iter()
                .map(|arm| arm_bitfield_store_count(&arm.body))
                .sum::<usize>()
                + default.as_ref().map_or(0, arm_bitfield_store_count)
        }
        _ => 0,
    }
}

fn arm_bitfield_store_count(arm: &ArmBody) -> usize {
    match arm {
        ArmBody::Return(_) => 0,
        ArmBody::Statements(body) => body.iter().map(statement_bitfield_store_count).sum(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{LocalDeclaration, Type};

    #[test]
    fn counts_each_inline_substitution_form() {
        assert_eq!(
            ordinal_residue(
                InlineExpansionFacts {
                    leading_initializer_substitutions: 1,
                    body_value_substitutions: 0,
                },
                2,
                1,
                2,
            ),
            8
        );
    }

    #[test]
    fn counts_a_mutating_conditional_value_inline_in_a_scratch_frame() {
        let function = Function {
            return_type: Type::Void,
            name: "caller".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![LocalDeclaration {
                declared_type: Type::UnsignedChar,
                name: "scratch".into(),
                initializer: None,
                is_volatile: false,
                array_length: Some(12),
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            }],
            statements: vec![Statement::If {
                condition: Expression::IntegerLiteral(1),
                then_body: vec![Statement::Store {
                    target: Expression::BitFieldRead {
                        extracted: Box::new(Expression::IntegerLiteral(1)),
                        promoted_type: Type::Int,
                        storage: Box::new(Expression::Variable("flags".into())),
                        shift: 6,
                        width: 1,
                    },
                    value: Expression::IntegerLiteral(0),
                }],
                else_body: Vec::new(),
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

        assert_eq!(legacy_mutating_value_body_ordinal_residue(&function, 1), 6);
    }
}
