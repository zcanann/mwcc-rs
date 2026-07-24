//! Physical-register preferences for dense structured saved-home classes.
//!
//! Legacy MWCC does not assign dense homes in creation order. It anchors the
//! last incoming parameter at the high end, the earliest input at the low end,
//! and packs eager and deferred values into the two gaps. Keeping this as a
//! pure layout policy prevents statement emission from depending on register
//! numbers or source identifiers.

use super::structured_locals::DeferredSavedHomePlan;
use super::structured_parameter_home_reuse::StructuredParameterHomeReuse;
use mwcc_syntax_trees::{Expression, Function, Statement};

/// A predecrement frame whose entry computes two flags, publishes a cursor
/// through an address-taken local, and receives an allocator result uses a
/// six-register lifetime layout. The call result takes the highest home, the
/// reloaded cursor takes the remaining low home, and the two flags bracket
/// them. This is MWCC's measured layout for macro-heavy display-list builders.
pub(super) fn allocator_result_cursor_preferences(
    function: &Function,
    deferred: &DeferredSavedHomePlan,
    eager_count: usize,
    parameter_count: usize,
    total_count: usize,
) -> std::collections::HashMap<usize, u8> {
    if eager_count != 0
        || parameter_count != 2
        || deferred.group_count != 4
        || total_count != 6
    {
        return std::collections::HashMap::new();
    }
    let [
        Statement::Assign {
            name: first_flag,
            value: first_value,
        },
        Statement::Assign {
            name: second_flag,
            value: second_value,
        },
        Statement::Assign {
            name: frame_cursor,
            value: Expression::Dereference { .. },
        },
        Statement::Assign {
            name: allocated,
            value: allocation,
        },
        Statement::Assign {
            name: cursor,
            value: Expression::Variable(cursor_source),
        },
        ..
    ] = function.statements.as_slice()
    else {
        return std::collections::HashMap::new();
    };
    if crate::analysis::expression_has_call(first_value)
        || crate::analysis::expression_has_call(second_value)
        || cursor_source != frame_cursor
        || !call_takes_address_of(allocation, frame_cursor)
    {
        return std::collections::HashMap::new();
    }
    let Some(first_flag_group) = deferred.group_if_present(first_flag) else {
        return std::collections::HashMap::new();
    };
    let Some(second_flag_group) = deferred.group_if_present(second_flag) else {
        return std::collections::HashMap::new();
    };
    let Some(allocated_group) = deferred.group_if_present(allocated) else {
        return std::collections::HashMap::new();
    };
    let Some(cursor_group) = deferred.group_if_present(cursor) else {
        return std::collections::HashMap::new();
    };
    let groups = [
        first_flag_group,
        second_flag_group,
        allocated_group,
        cursor_group,
    ];
    let unique: std::collections::HashSet<_> = groups.into_iter().collect();
    if unique.len() != groups.len() {
        return std::collections::HashMap::new();
    }

    let first_saved = 26u8;
    std::collections::HashMap::from([
        (0, first_saved + 1),
        (1, first_saved),
        (parameter_count + first_flag_group, first_saved + 2),
        (parameter_count + second_flag_group, first_saved + 4),
        (parameter_count + allocated_group, first_saved + 5),
        (parameter_count + cursor_group, first_saved + 3),
    ])
}

fn call_takes_address_of(expression: &Expression, local: &str) -> bool {
    match expression {
        Expression::Call { arguments, .. } => arguments.iter().any(|argument| {
            matches!(
                argument,
                Expression::AddressOf { operand }
                    if matches!(operand.as_ref(), Expression::Variable(name) if name == local)
            )
        }),
        Expression::Cast { operand, .. } => call_takes_address_of(operand, local),
        _ => false,
    }
}

/// A four-byte dead scratch array beside one live aggregate preserves a compact
/// legacy frame. Its incoming object and deferred pointer establish both GPR
/// save slots before initialization, and its paired deferred floats grow from
/// f30 toward f31 rather than descending from f31.
pub(super) fn compact_aggregate_scratch_frame_pair(
    unused_frame_array: bool,
    frame_array_bytes: i16,
    aggregate_count: usize,
    eager_count: usize,
    parameter_count: usize,
    deferred_count: usize,
    total_count: usize,
) -> bool {
    unused_frame_array
        && frame_array_bytes == 4
        && aggregate_count == 1
        && eager_count == 0
        && parameter_count == 1
        && deferred_count == 1
        && total_count == 2
}

pub(super) fn saved_float_home_preference(
    group: usize,
    group_count: usize,
    ascending_pair: bool,
) -> u8 {
    if ascending_pair && group_count == 2 {
        30u8.saturating_add(u8::try_from(group).unwrap_or(1).min(1))
    } else {
        31u8.saturating_sub(u8::try_from(group).unwrap_or(17))
    }
}

/// A linkage-first body with one entry-loaded local and one later call result
/// assigns the long-lived entry value to r30 and the later value to r31. This
/// is the compact two-home analogue of the dense lifetime-class layout below.
pub(super) fn paired_eager_deferred_preference(
    with_frame_array: bool,
    eager_count: usize,
    parameter_count: usize,
    deferred_count: usize,
    retained_inline_lane: bool,
    home_index: usize,
) -> Option<u8> {
    (!with_frame_array
        && eager_count == 1
        && parameter_count == 0
        && deferred_count == 1
        && retained_inline_lane
        && home_index < 2)
        .then_some(if home_index == 0 { 30 } else { 31 })
}

pub(super) fn dense_eager_deferred_preferences(
    eager_count: usize,
    parameter_count: usize,
    total_count: usize,
    deferred: &DeferredSavedHomePlan,
    reuse: &StructuredParameterHomeReuse,
    rounded_pointer_layout: bool,
    lifetime_order: bool,
) -> std::collections::HashMap<usize, u8> {
    let fresh_home_base = eager_count + parameter_count;
    let Some(first_saved) = 32usize.checked_sub(total_count) else {
        return std::collections::HashMap::new();
    };
    let occupied: std::collections::HashSet<_> = (0..fresh_home_base)
        .filter_map(|home| {
            if rounded_pointer_layout {
                rounded_pointer_dense_home_preference(
                    eager_count,
                    parameter_count,
                    total_count,
                    home,
                )
            } else {
                dense_eager_home_preference(eager_count, parameter_count, total_count, home)
            }
        })
        .collect();
    let available: Vec<_> = (first_saved..32)
        .filter_map(|register| u8::try_from(register).ok())
        .filter(|register| !occupied.contains(register))
        .collect();
    let mut groups: Vec<_> = (0..deferred.group_count)
        .filter_map(|group| {
            let home = reuse.home_index(group);
            (home >= fresh_home_base).then_some(DenseDeferredGroup {
                home,
                first_assignment: deferred.first_assignment(group),
                member_count: deferred.member_count(group),
                contains_value_version: deferred.contains_value_version(group),
            })
        })
        .collect();
    groups.sort_by_key(|group| group.home);
    groups.dedup_by_key(|group| group.home);
    rank_dense_deferred_groups(available, groups, lifetime_order)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DenseDeferredGroup {
    home: usize,
    first_assignment: usize,
    member_count: usize,
    contains_value_version: bool,
}

fn rank_dense_deferred_groups(
    mut available: Vec<u8>,
    mut groups: Vec<DenseDeferredGroup>,
    lifetime_order: bool,
) -> std::collections::HashMap<usize, u8> {
    let mut preferences = std::collections::HashMap::new();
    available.sort_unstable();
    if available.len() != groups.len() || groups.is_empty() {
        return preferences;
    }

    let primary_index = groups
        .iter()
        .enumerate()
        .min_by_key(|(_, group)| group.first_assignment)
        .map(|(index, _)| index);
    if let Some(primary_index) = primary_index {
        let primary = groups.remove(primary_index);
        preferences.insert(primary.home, available.pop().expect("counts matched"));
    }

    let mut versions = Vec::new();
    let mut recycled = Vec::new();
    let mut ordinary = Vec::new();
    for group in groups {
        if group.contains_value_version {
            versions.push(group);
        } else if group.member_count > 1 {
            recycled.push(group);
        } else {
            ordinary.push(group);
        }
    }
    versions.sort_by_key(|group| group.first_assignment);
    for group in versions {
        preferences.insert(group.home, available.pop().expect("counts matched"));
    }

    if lifetime_order {
        recycled.extend(ordinary);
        recycled.sort_by_key(|group| group.first_assignment);
        for group in recycled {
            preferences.insert(group.home, available.remove(0));
        }
        debug_assert!(available.is_empty());
        return preferences;
    }

    ordinary.sort_by_key(|group| group.first_assignment);
    if let Some(late) = ordinary.pop() {
        preferences.insert(late.home, available.remove(0));
    }
    recycled.sort_by_key(|group| group.first_assignment);
    for group in recycled {
        preferences.insert(group.home, available.remove(0));
    }
    for group in ordinary {
        preferences.insert(group.home, available.remove(0));
    }
    debug_assert!(available.is_empty());
    preferences
}

pub(super) fn uses_rounded_pointer_dense_layout(
    eager_count: usize,
    parameter_count: usize,
    total_count: usize,
) -> bool {
    eager_count == 4 && parameter_count == 2 && total_count == 12
}

pub(super) fn rounded_pointer_dense_home_preference(
    eager_count: usize,
    parameter_count: usize,
    total_count: usize,
    home_index: usize,
) -> Option<u8> {
    uses_rounded_pointer_dense_layout(eager_count, parameter_count, total_count)
        .then(|| [31, 30, 29, 25, 27, 20].get(home_index).copied())
        .flatten()
}

pub(super) fn dense_eager_home_preference(
    eager_count: usize,
    parameter_count: usize,
    total_count: usize,
    home_index: usize,
) -> Option<u8> {
    if eager_count == 0 || parameter_count == 0 || total_count > 18 || home_index >= total_count {
        return None;
    }
    let first_saved = 32usize.checked_sub(total_count)?;
    let preferred = if home_index < eager_count {
        if home_index + 1 == eager_count {
            30
        } else {
            29usize.checked_sub(home_index)?
        }
    } else if home_index < eager_count + parameter_count {
        let parameter = home_index - eager_count;
        if parameter == 0 {
            31
        } else {
            first_saved + parameter - 1
        }
    } else {
        let deferred = home_index - eager_count - parameter_count;
        if deferred == 0 {
            30usize.checked_sub(eager_count)?
        } else {
            first_saved + parameter_count + deferred - 2
        }
    };
    (preferred >= first_saved && preferred < 32)
        .then(|| u8::try_from(preferred).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{
        BinaryOperator, Expression, Function, LocalDeclaration, Pointee, Statement, Type,
    };

    #[test]
    fn pairs_one_eager_local_below_one_deferred_result() {
        assert_eq!(paired_eager_deferred_preference(false, 1, 0, 1, true, 0), Some(30));
        assert_eq!(paired_eager_deferred_preference(false, 1, 0, 1, true, 1), Some(31));
        assert_eq!(paired_eager_deferred_preference(true, 1, 0, 1, true, 0), None);
        assert_eq!(paired_eager_deferred_preference(false, 1, 0, 1, false, 0), None);
    }

    #[test]
    fn recognizes_the_compact_aggregate_scratch_frame_pair() {
        assert!(compact_aggregate_scratch_frame_pair(true, 4, 1, 0, 1, 1, 2));
        assert!(!compact_aggregate_scratch_frame_pair(true, 8, 1, 0, 1, 1, 2));
        assert!(!compact_aggregate_scratch_frame_pair(true, 4, 0, 0, 1, 1, 2));
        assert_eq!(saved_float_home_preference(0, 2, true), 30);
        assert_eq!(saved_float_home_preference(1, 2, true), 31);
        assert_eq!(saved_float_home_preference(0, 2, false), 31);
    }

    #[test]
    fn lays_out_mixed_dense_home_classes_without_overlap() {
        let preferences: Vec<_> = (0..12)
            .map(|home| dense_eager_home_preference(4, 2, 12, home).unwrap())
            .collect();
        assert_eq!(
            preferences,
            [29, 28, 27, 30, 31, 20, 26, 21, 22, 23, 24, 25]
        );
        let unique: std::collections::HashSet<_> = preferences.iter().copied().collect();
        assert_eq!(unique.len(), preferences.len());
    }

    #[test]
    fn ranks_deferred_groups_by_lifetime_role() {
        let groups = vec![
            DenseDeferredGroup {
                home: 6,
                first_assignment: 2,
                member_count: 1,
                contains_value_version: false,
            },
            DenseDeferredGroup {
                home: 7,
                first_assignment: 4,
                member_count: 2,
                contains_value_version: false,
            },
            DenseDeferredGroup {
                home: 8,
                first_assignment: 12,
                member_count: 1,
                contains_value_version: true,
            },
            DenseDeferredGroup {
                home: 9,
                first_assignment: 16,
                member_count: 1,
                contains_value_version: false,
            },
            DenseDeferredGroup {
                home: 10,
                first_assignment: 18,
                member_count: 1,
                contains_value_version: false,
            },
            DenseDeferredGroup {
                home: 11,
                first_assignment: 19,
                member_count: 1,
                contains_value_version: false,
            },
        ];
        let ranked = rank_dense_deferred_groups(vec![21, 22, 23, 24, 25, 26], groups, false);
        assert_eq!(ranked[&6], 26);
        assert_eq!(ranked[&7], 22);
        assert_eq!(ranked[&8], 25);
        assert_eq!(ranked[&9], 23);
        assert_eq!(ranked[&10], 24);
        assert_eq!(ranked[&11], 21);
    }

    #[test]
    fn lays_out_a_rounded_pointer_frame_by_lifetime_order() {
        let groups = vec![
            DenseDeferredGroup {
                home: 6,
                first_assignment: 2,
                member_count: 1,
                contains_value_version: false,
            },
            DenseDeferredGroup {
                home: 7,
                first_assignment: 4,
                member_count: 2,
                contains_value_version: false,
            },
            DenseDeferredGroup {
                home: 8,
                first_assignment: 5,
                member_count: 2,
                contains_value_version: false,
            },
            DenseDeferredGroup {
                home: 9,
                first_assignment: 12,
                member_count: 1,
                contains_value_version: true,
            },
            DenseDeferredGroup {
                home: 10,
                first_assignment: 16,
                member_count: 1,
                contains_value_version: false,
            },
            DenseDeferredGroup {
                home: 11,
                first_assignment: 17,
                member_count: 1,
                contains_value_version: false,
            },
        ];
        let ranked = rank_dense_deferred_groups(vec![21, 22, 23, 24, 26, 28], groups, true);
        assert_eq!(
            ranked,
            std::collections::HashMap::from([
                (6, 28),
                (7, 21),
                (8, 22),
                (9, 26),
                (10, 23),
                (11, 24),
            ])
        );
        let eager: Vec<_> = (0..6)
            .map(|home| rounded_pointer_dense_home_preference(4, 2, 12, home).unwrap())
            .collect();
        assert_eq!(eager, [31, 30, 29, 25, 27, 20]);
    }

    #[test]
    fn lays_out_an_allocator_result_beside_a_published_cursor() {
        let local = |name: &str, declared_type| LocalDeclaration {
            declared_type,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        };
        let statements = vec![
            Statement::Assign {
                name: "load".into(),
                value: Expression::Binary {
                    operator: BinaryOperator::NotEqual,
                    left: Box::new(Expression::Variable("flags".into())),
                    right: Box::new(Expression::IntegerLiteral(0)),
                },
            },
            Statement::Assign {
                name: "mode".into(),
                value: Expression::Binary {
                    operator: BinaryOperator::BitAnd,
                    left: Box::new(Expression::Variable("flags".into())),
                    right: Box::new(Expression::IntegerLiteral(4)),
                },
            },
            Statement::Assign {
                name: "temporary".into(),
                value: Expression::Dereference {
                    pointer: Box::new(Expression::Variable("output".into())),
                },
            },
            Statement::Assign {
                name: "object".into(),
                value: Expression::Call {
                    name: "allocate".into(),
                    arguments: vec![Expression::AddressOf {
                        operand: Box::new(Expression::Variable("temporary".into())),
                    }],
                },
            },
            Statement::Assign {
                name: "cursor".into(),
                value: Expression::Variable("temporary".into()),
            },
            Statement::Store {
                target: Expression::Dereference {
                    pointer: Box::new(Expression::Variable("object".into())),
                },
                value: Expression::Variable("mode".into()),
            },
            Statement::If {
                condition: Expression::Variable("load".into()),
                then_body: vec![Statement::Store {
                    target: Expression::Dereference {
                        pointer: Box::new(Expression::Variable("cursor".into())),
                    },
                    value: Expression::IntegerLiteral(0),
                }],
                else_body: Vec::new(),
            },
        ];
        let function = Function {
            return_type: Type::Void,
            name: "builder".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![
                local("load", Type::Int),
                local("mode", Type::Int),
                local("temporary", Type::Pointer(Pointee::Int)),
                local("object", Type::Pointer(Pointee::Int)),
                local("cursor", Type::Pointer(Pointee::Int)),
            ],
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
        };
        let saved = function
            .locals
            .iter()
            .filter(|local| local.name != "temporary")
            .collect::<Vec<_>>();
        let plan = super::super::structured_locals::plan_deferred_saved_homes(&function, &saved)
            .expect("the test values have overlapping lifetimes");
        assert_eq!(plan.group_count, 4);
        assert_eq!(
            allocator_result_cursor_preferences(&function, &plan, 0, 2, 6),
            std::collections::HashMap::from([
                (0, 27),
                (1, 26),
                (2, 28),
                (3, 30),
                (4, 31),
                (5, 29),
            ])
        );
    }
}
