//! Source scalar declarations retained after their values disappear.
//!
//! GC/1.2.5n assigns stack-table space before dead-value elimination. A scalar
//! which is never read therefore emits no load or store, but can still move the
//! addressable locals and saved-register range above it. This plan records only
//! that source-layout residue; the structured frame owner decides when the
//! linkage-first layout exposes it.

use mwcc_syntax_trees::{Function, LocalDeclaration, Type};

use super::structured_locals::body_reads_local;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct UnobservedScalarTable {
    pub(super) bytes: i16,
    pub(super) count: usize,
}

impl UnobservedScalarTable {
    pub(super) fn plan(function: &Function) -> Option<Self> {
        Self::measure(function, function.locals.iter())
    }

    /// Measure source scalar residue which precedes the first automatic array.
    /// Linkage-first frames keep this declaration-order prefix below the array
    /// even when the values themselves disappear during optimization.
    pub(super) fn plan_before_first_array(function: &Function) -> Option<Self> {
        Self::measure(
            function,
            function
                .locals
                .iter()
                .take_while(|local| local.array_length.is_none()),
        )
    }

    fn measure<'a>(
        function: &Function,
        locals: impl Iterator<Item = &'a LocalDeclaration>,
    ) -> Option<Self> {
        let mut end = 0u32;
        let mut count = 0usize;
        for local in locals.filter(|local| {
            !local.is_static
                && local.array_length.is_none()
                && !body_reads_local(&function.statements, &local.name)
        }) {
            let Some((size, alignment)) = scalar_layout(local.declared_type) else {
                continue;
            };
            end = end.div_ceil(alignment) * alignment;
            end = end.checked_add(size)?;
            count += 1;
        }
        (end != 0).then_some(Self {
            bytes: i16::try_from(end).ok()?,
            count,
        })
    }
}

fn scalar_layout(value_type: Type) -> Option<(u32, u32)> {
    match value_type {
        Type::Void | Type::Struct { .. } => None,
        Type::Char | Type::UnsignedChar => Some((1, 1)),
        Type::Short | Type::UnsignedShort => Some((2, 2)),
        Type::Double | Type::LongLong | Type::UnsignedLongLong => Some((8, 8)),
        Type::Int
        | Type::UnsignedInt
        | Type::Float
        | Type::Pointer(_)
        | Type::StructPointer { .. } => Some((4, 4)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{Expression, LocalDeclaration, Statement};

    fn local(name: &str, declared_type: Type) -> LocalDeclaration {
        LocalDeclaration {
            declared_type,
            name: name.into(),
            initializer: None,
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

    fn function(locals: Vec<LocalDeclaration>, statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "caller".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals,
            statements,
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn measures_unread_scalars_with_their_natural_layout() {
        let function = function(
            vec![
                local("byte", Type::UnsignedChar),
                local("half", Type::UnsignedShort),
                local("wide", Type::UnsignedLongLong),
            ],
            Vec::new(),
        );
        assert_eq!(
            UnobservedScalarTable::plan(&function),
            Some(UnobservedScalarTable {
                bytes: 16,
                count: 3,
            })
        );
    }

    #[test]
    fn excludes_a_scalar_whose_value_is_read() {
        let function = function(
            vec![local("dead", Type::Int), local("live", Type::Int)],
            vec![Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![Expression::Variable("live".into())],
            })],
        );
        assert_eq!(
            UnobservedScalarTable::plan(&function),
            Some(UnobservedScalarTable { bytes: 4, count: 1 })
        );
    }

    #[test]
    fn stops_the_prefix_measurement_at_the_first_array() {
        let mut array = local("array", Type::Int);
        array.array_length = Some(4);
        let function = function(
            vec![
                local("prefix", Type::Pointer(mwcc_syntax_trees::Pointee::Int)),
                array,
                local("suffix", Type::Int),
            ],
            Vec::new(),
        );
        assert_eq!(
            UnobservedScalarTable::plan_before_first_array(&function),
            Some(UnobservedScalarTable { bytes: 4, count: 1 })
        );
        assert_eq!(
            UnobservedScalarTable::plan(&function),
            Some(UnobservedScalarTable { bytes: 8, count: 2 })
        );
    }
}
