//! Lifetime-aware frame placement for structured aggregate locals.
//!
//! MWCC reserves frame capacity for every source aggregate, but may give two
//! dead-disjoint values the same offset. Keeping capacity and placement as
//! separate decisions preserves its frame-size rounding while allowing exact
//! stack-slot reuse.

#[allow(unused_imports)]
use super::*;
use super::structured_locals::body_uses_local;

#[derive(Clone, Copy)]
struct Lifetime {
    first: usize,
    last: usize,
}

impl Lifetime {
    fn overlaps(self, other: Self) -> bool {
        self.first <= other.last && other.first <= self.last
    }
}

struct Slot {
    offset: i16,
    size: u32,
    lifetimes: Vec<Lifetime>,
}

pub(super) fn plan_aggregate_frame_slots(
    locals: &[&LocalDeclaration],
    statements: &[Statement],
) -> Compilation<std::collections::HashMap<String, i16>> {
    let mut slots: Vec<Slot> = Vec::new();
    let mut placements = std::collections::HashMap::new();
    let mut end = 8u32;

    for local in locals.iter().rev() {
        let Type::Struct { size, align } = local.declared_type else {
            return Err(Diagnostic::error(
                "structured aggregate slot planning received a scalar local",
            ));
        };
        let lifetime = aggregate_lifetime(statements, &local.name);
        let alignment = u32::from(align.max(1));
        let reusable = slots.iter_mut().find(|slot| {
            slot.size >= size
                && u32::try_from(slot.offset)
                    .is_ok_and(|offset| offset % alignment == 0)
                && slot
                    .lifetimes
                    .iter()
                    .all(|other| !lifetime.overlaps(*other))
        });
        let offset = if let Some(slot) = reusable {
            slot.lifetimes.push(lifetime);
            slot.offset
        } else {
            end = end.div_ceil(alignment) * alignment;
            let offset = i16::try_from(end)
                .map_err(|_| Diagnostic::error("structured aggregate slot is out of range"))?;
            slots.push(Slot {
                offset,
                size,
                lifetimes: vec![lifetime],
            });
            end = end
                .checked_add(size)
                .ok_or_else(|| Diagnostic::error("structured aggregate frame is too large"))?;
            offset
        };
        placements.insert(local.name.clone(), offset);
    }
    Ok(placements)
}

fn aggregate_lifetime(statements: &[Statement], name: &str) -> Lifetime {
    let mut uses = statements
        .iter()
        .enumerate()
        .filter_map(|(index, statement)| {
            body_uses_local(std::slice::from_ref(statement), name).then_some(index)
        });
    let Some(first) = uses.next() else {
        return Lifetime {
            first: 0,
            last: usize::MAX,
        };
    };
    Lifetime {
        first,
        last: uses.last().unwrap_or(first),
    }
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
    fn reuses_a_dead_aggregate_slot_without_reducing_frame_capacity() {
        let position = aggregate("position", 12);
        let argument = aggregate("argument", 16);
        let effect = aggregate("effect", 12);
        let locals = vec![&position, &argument, &effect];
        let statements = vec![
            Statement::Assign {
                name: "position".into(),
                value: Expression::IntegerLiteral(0),
            },
            Statement::Assign {
                name: "argument".into(),
                value: Expression::Variable("position".into()),
            },
            Statement::Assign {
                name: "effect".into(),
                value: Expression::IntegerLiteral(0),
            },
            Statement::Expression(Expression::Call {
                name: "create".into(),
                arguments: vec![
                    Expression::Variable("effect".into()),
                    Expression::Variable("argument".into()),
                ],
            }),
        ];

        let slots = plan_aggregate_frame_slots(&locals, &statements).unwrap();
        assert_eq!(slots["effect"], 8);
        assert_eq!(slots["argument"], 20);
        assert_eq!(slots["position"], 8);
    }
}
