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

/// Plan the measured EABI slice where two leading four-byte aggregate
/// arguments are copied into distinct outgoing stack objects before a terminal
/// direct call. The source objects live above this reserved prefix.
///
/// This deliberately owns only the dependency-complete two-word form. Wider
/// aggregates and interspersed scalar arguments need a general multiword copy
/// scheduler rather than an accidental extension of this schedule.
pub(super) fn plan_terminal_one_word_aggregate_call_copies(
    locals: &[&LocalDeclaration],
    all_locals: &[LocalDeclaration],
    statements: &[Statement],
    call_parameter_types: &std::collections::HashMap<String, Vec<Type>>,
) -> Option<StructuredAggregateCallCopyPlan> {
    let Statement::Expression(Expression::Call { name, arguments }) = statements.last()? else {
        return None;
    };
    if locals.len() != 2
        || arguments.len() < 2
    {
        return None;
    }
    if let Some(parameter_types) = call_parameter_types.get(name) {
        if parameter_types.len() != arguments.len()
            || !matches!(
                parameter_types.as_slice(),
                [
                    Type::Struct { size: 4, .. },
                    Type::Struct { size: 4, .. },
                    rest @ ..
                ] if rest
                    .iter()
                    .all(|parameter| !matches!(parameter, Type::Struct { .. } | Type::Float | Type::Double))
            )
        {
            return None;
        }
    } else {
        // Old C permits a call without a visible prototype. MWCC still knows
        // the source argument types and applies the aggregate-copy ABI. Keep
        // this inference deliberately narrow: the measured form has exactly
        // one trailing scalar local, whose ordinary register marshalling is
        // dependency-complete.
        let [_, _, Expression::Variable(trailing)] = arguments.as_slice() else {
            return None;
        };
        let trailing_type = all_locals
            .iter()
            .find(|local| local.name == *trailing)?
            .declared_type;
        if matches!(
            trailing_type,
            Type::Struct { .. } | Type::Float | Type::Double
        ) {
            return None;
        }
    }

    let mut copies = Vec::with_capacity(2);
    for (argument_index, copy_offset) in [(0, 12), (1, 8)] {
        let Expression::Variable(local_name) = &arguments[argument_index] else {
            return None;
        };
        let local = locals.iter().find(|local| local.name == *local_name)?;
        if local.is_static
            || local.is_volatile
            || local.array_length.is_some()
            || local.initializer.is_some()
            || !local.data_relocations.is_empty()
            || !matches!(local.declared_type, Type::Struct { size: 4, .. })
            || local.data_bytes.as_ref().is_none_or(|image| image.len() != 4)
        {
            return None;
        }
        copies.push(StructuredAggregateArgumentCopy {
            argument_index,
            local: local_name.clone(),
            copy_offset,
        });
    }
    if copies[0].local == copies[1].local {
        return None;
    }

    Some(StructuredAggregateCallCopyPlan {
        callee: name.clone(),
        copies,
        total_bytes: 8,
    })
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
            attribute_alignment: None,
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

    #[test]
    fn reserves_reverse_argument_order_copies_below_source_objects() {
        let mut foreground = aggregate("foreground", 4);
        foreground.data_bytes = Some(vec![0xff, 0xff, 0xff, 0]);
        let mut background = aggregate("background", 4);
        background.data_bytes = Some(vec![0, 0, 0, 0]);
        let locals = vec![&background, &foreground];
        let statements = vec![Statement::Expression(Expression::Call {
            name: "fatal".into(),
            arguments: vec![
                Expression::Variable("foreground".into()),
                Expression::Variable("background".into()),
                Expression::Variable("message".into()),
            ],
        })];
        let parameter_types = std::collections::HashMap::from([(
            "fatal".into(),
            vec![
                Type::Struct { size: 4, align: 1 },
                Type::Struct { size: 4, align: 1 },
                Type::Pointer(Pointee::Char),
            ],
        )]);

        let plan = plan_terminal_one_word_aggregate_call_copies(
            &locals,
            &[],
            &statements,
            &parameter_types,
        )
        .unwrap();
        assert_eq!(plan.total_bytes, 8);
        assert_eq!(plan.copies[0].local, "foreground");
        assert_eq!(plan.copies[0].copy_offset, 12);
        assert_eq!(plan.copies[1].local, "background");
        assert_eq!(plan.copies[1].copy_offset, 8);

        let source_slots =
            plan_aggregate_frame_slots_from(&locals, &statements, 16).unwrap();
        assert_eq!(source_slots["foreground"], 16);
        assert_eq!(source_slots["background"], 20);
    }
}
