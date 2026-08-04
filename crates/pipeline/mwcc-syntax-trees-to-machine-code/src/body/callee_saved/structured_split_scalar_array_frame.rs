//! Linkage-first frames whose addressable scalar table straddles an array.
//!
//! Early MWCC lays out source declarations before eliminating passive pointer
//! aliases. In a dense call-bearing loop this can leave an unread scalar prefix
//! below an automatic array while addressable scalar values are placed above
//! that array. This plan owns only those byte boundaries; saved-home selection
//! and scalar instruction emission remain with the structured generator.

use mwcc_syntax_trees::{Function, LocalDeclaration};
use mwcc_versions::FrameConvention;

use super::structured_unobserved_scalar_table::UnobservedScalarTable;

pub(super) struct Layout {
    prefix_bytes: i16,
    trailing_scalar_bytes: i16,
    scalar_count: usize,
}

impl Layout {
    pub(super) fn plan(
        function: &Function,
        materialized_loop_member_addresses: bool,
        frame_convention: FrameConvention,
        frame_scalar_parameter_count: usize,
        frame_scalar_locals: &[&LocalDeclaration],
    ) -> Option<Self> {
        if !materialized_loop_member_addresses
            || frame_convention != FrameConvention::LinkageFirst
            || frame_scalar_parameter_count == 0
            || frame_scalar_locals.is_empty()
            || !function
                .locals
                .iter()
                .any(|local| local.array_length.is_some())
        {
            return None;
        }
        let prefix = UnobservedScalarTable::plan_before_first_array(function)?;
        let trailing_scalar_bytes = i16::try_from(frame_scalar_locals.len().checked_mul(4)?).ok()?;
        Some(Self {
            prefix_bytes: prefix.bytes,
            trailing_scalar_bytes,
            scalar_count: frame_scalar_parameter_count.checked_add(frame_scalar_locals.len())?,
        })
    }

    pub(super) fn array_offset(&self, ordinary_offset: i16) -> Option<i16> {
        ordinary_offset.checked_add(self.prefix_bytes)
    }

    pub(super) fn local_region_bytes(&self, ordinary_bytes: i16) -> Option<i16> {
        ordinary_bytes.checked_add(self.trailing_scalar_bytes)
    }

    pub(super) fn parameter_scalar_offset(&self, array_offset: i16) -> Option<i16> {
        array_offset
            .checked_sub(self.prefix_bytes)?
            .checked_sub(i16::try_from(self.scalar_count.checked_mul(4)?).ok()?)
    }

    pub(super) fn local_scalar_offset(
        &self,
        array_offset: i16,
        array_bytes: i16,
    ) -> Option<i16> {
        array_offset.checked_add(array_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{Expression, LocalDeclaration, Pointee, Statement, Type};

    fn local(name: &str, initializer: Option<Expression>) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Pointer(Pointee::Pointer),
            name: name.into(),
            initializer,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    #[test]
    fn splits_parameters_below_and_locals_above_the_array() {
        let mut array = local("pad", None);
        array.declared_type = Type::Int;
        array.array_length = Some(10);
        let function = Function {
            return_type: Type::Void,
            name: "caller".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![
                local("live", None),
                local(
                    "alias_a",
                    Some(Expression::AddressOf {
                        operand: Box::new(Expression::Variable("owner".into())),
                    }),
                ),
                local(
                    "alias_b",
                    Some(Expression::AddressOf {
                        operand: Box::new(Expression::Variable("value".into())),
                    }),
                ),
                array,
            ],
            statements: vec![Statement::Expression(Expression::Variable("live".into()))],
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
        let frame_locals = [&function.locals[0], &function.locals[0]];
        let layout = Layout::plan(
            &function,
            true,
            FrameConvention::LinkageFirst,
            1,
            &frame_locals,
        )
        .expect("the split frame should be recognized");

        let array_offset = layout.array_offset(20).unwrap();
        assert_eq!(array_offset, 28);
        assert_eq!(layout.parameter_scalar_offset(array_offset), Some(8));
        assert_eq!(layout.local_scalar_offset(array_offset, 40), Some(68));
        assert_eq!(layout.local_region_bytes(40), Some(48));
    }
}
