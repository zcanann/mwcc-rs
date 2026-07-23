//! Typed scalarization of aggregate value assignments.
//!
//! Aggregate tags and field declarations deliberately do not enter machine IR.
//! This pass expands copies while the parser still knows each field's real load
//! and store class, avoiding size-based guesses in instruction selection.

use crate::parser::Parser;
use mwcc_syntax_trees::{Expression, Type};

impl Parser {
    /// Copy declared scalar fields in source order and recurse through embedded
    /// aggregates. Padding is not an object value and is therefore untouched.
    pub(super) fn lower_typed_aggregate_assignment(
        &self,
        target: &Expression,
        value: &Expression,
        target_tag: Option<&str>,
        value_tag: Option<&str>,
    ) -> Option<Expression> {
        // Ordinary C++ assignment may dispatch a user-defined operator=. This
        // scalarizer serves recovered skipped-inline bodies, whose raw field
        // assignment must be composed at their call sites; leave ordinary
        // function bodies unchanged until overload resolution owns that choice.
        if !self.recover_skipped_inline_definition {
            return None;
        }
        // A pointer/reference expression carries its pointee's struct tag for
        // overload resolution, but that does not make the pointer itself an
        // aggregate value. Require aggregate storage on both sides before
        // expanding fields; otherwise `member_ptr = parameter_ptr` would be
        // miscompiled as `*member_ptr = *parameter_ptr`.
        let is_aggregate_value = |expression: &Expression| {
            matches!(self.cxx_expression_type(expression), Some(Type::Struct { .. }))
                || matches!(expression, Expression::Variable(name)
                    if self.cxx_reference_variables.contains(name))
        };
        if !is_aggregate_value(target) || !is_aggregate_value(value)
        {
            return None;
        }
        let target_tag = target_tag?;
        let value_tag = value_tag?;
        if target_tag != value_tag {
            return None;
        }
        let mut active = std::collections::HashSet::new();
        let assignments =
            self.scalar_aggregate_copy_fields(target_tag, target, value, &mut active)?;
        let mut assignments = assignments.into_iter();
        let first = assignments.next()?;
        Some(assignments.fold(first, |left, right| Expression::Comma {
            left: Box::new(left),
            right: Box::new(right),
        }))
    }

    fn scalar_aggregate_copy_fields(
        &self,
        tag: &str,
        target: &Expression,
        value: &Expression,
        active: &mut std::collections::HashSet<String>,
    ) -> Option<Vec<Expression>> {
        if !active.insert(tag.to_owned()) {
            return None;
        }
        let layout = self.structs.get(tag)?;
        if layout.is_union {
            active.remove(tag);
            return None;
        }
        let fields = layout
            .fields_in_declaration_order()
            .into_iter()
            .map(|(_, field)| field.clone())
            .collect::<Vec<_>>();
        let mut assignments = Vec::new();
        for field in fields {
            if field.array_element.is_some()
                || field.array_bytes.is_some()
                || field.bit_field.is_some()
            {
                active.remove(tag);
                return None;
            }
            let target_field = Expression::Member {
                base: Box::new(target.clone()),
                offset: field.offset,
                member_type: field.member_type,
                index_stride: None,
            };
            let value_field = Expression::Member {
                base: Box::new(value.clone()),
                offset: field.offset,
                member_type: field.member_type,
                index_stride: None,
            };
            if matches!(field.member_type, Type::Struct { .. }) {
                let nested = field.struct_tag.as_deref()?;
                assignments.extend(self.scalar_aggregate_copy_fields(
                    nested,
                    &target_field,
                    &value_field,
                    active,
                )?);
            } else if matches!(
                field.member_type,
                Type::Int
                    | Type::UnsignedInt
                    | Type::Char
                    | Type::UnsignedChar
                    | Type::Short
                    | Type::UnsignedShort
                    | Type::Float
                    | Type::Double
                    | Type::Pointer(_)
                    | Type::StructPointer { .. }
            ) {
                assignments.push(Expression::Assign {
                    target: Box::new(target_field),
                    value: Box::new(value_field),
                });
            } else {
                active.remove(tag);
                return None;
            }
        }
        active.remove(tag);
        Some(assignments)
    }
}
