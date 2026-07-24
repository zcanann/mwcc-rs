//! Frame placement for aggregate locals in structured bodies.
//!
//! MWCC gives aggregate-return destinations the low frame prefix in declaration
//! order. Ordinary address-taken aggregates follow in reverse declaration
//! order. Frame capacity remains based on every source aggregate, independently
//! of this placement policy.

#[allow(unused_imports)]
use super::*;

pub(super) fn plan_aggregate_frame_slots(
    locals: &[&LocalDeclaration],
    statements: &[Statement],
) -> Compilation<std::collections::HashMap<String, i16>> {
    plan_aggregate_frame_slots_from(locals, statements, 8)
}

/// Place aggregates after an already reserved low-frame prefix, such as a
/// retained entry lane plus an unused source array. Keeping the base explicit
/// prevents independently planned frame-local families from overlapping.
pub(super) fn plan_aggregate_frame_slots_from(
    locals: &[&LocalDeclaration],
    statements: &[Statement],
    base_offset: u32,
) -> Compilation<std::collections::HashMap<String, i16>> {
    let mut result_locals = Vec::new();
    let mut ordinary_locals = Vec::new();
    for local in locals {
        if is_aggregate_call_result(statements, &local.name) {
            result_locals.push(*local);
        } else {
            ordinary_locals.push(*local);
        }
    }

    let mut placements = std::collections::HashMap::new();
    let mut offset = base_offset;
    for local in result_locals
        .into_iter()
        .chain(ordinary_locals.into_iter().rev())
    {
        let Type::Struct { size, align } = local.declared_type else {
            return Err(Diagnostic::error(
                "structured aggregate slot planning received a scalar local",
            ));
        };
        let alignment = u32::from(align.max(1));
        offset = offset.div_ceil(alignment) * alignment;
        let slot_offset = i16::try_from(offset)
            .map_err(|_| Diagnostic::error("structured aggregate slot is out of range"))?;
        placements.insert(local.name.clone(), slot_offset);
        offset = offset
            .checked_add(size)
            .ok_or_else(|| Diagnostic::error("structured aggregate frame is too large"))?;
    }
    Ok(placements)
}

fn is_aggregate_call_result(statements: &[Statement], candidate: &str) -> bool {
    statements.iter().any(|statement| {
        matches!(statement,
            Statement::Assign {
                name,
                value: Expression::Call { .. } | Expression::VirtualCall { .. },
            } if name == candidate)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aggregate(name: &str, size: u32) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Struct { size, align: 4 },
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }
    }

    #[test]
    fn places_call_results_before_reverse_order_ordinary_aggregates() {
        let position = aggregate("position", 12);
        let argument = aggregate("argument", 16);
        let effect = aggregate("effect", 12);
        let locals = vec![&position, &argument, &effect];
        let statements = vec![Statement::Assign {
            name: "position".into(),
            value: Expression::VirtualCall {
                object: Box::new(Expression::Variable("object".into())),
                vptr_offset: 0,
                slot_offset: 8,
                return_type: Type::Struct { size: 12, align: 4 },
                variadic: false,
                arguments: Vec::new(),
            },
        }];

        let slots = plan_aggregate_frame_slots(&locals, &statements).unwrap();
        assert_eq!(slots["position"], 8);
        assert_eq!(slots["effect"], 20);
        assert_eq!(slots["argument"], 32);
    }

    #[test]
    fn places_aggregates_after_a_reserved_array_prefix() {
        let vector = aggregate("vector", 12);
        let slots = plan_aggregate_frame_slots_from(&[&vector], &[], 16).unwrap();

        assert_eq!(slots["vector"], 16);
    }
}
