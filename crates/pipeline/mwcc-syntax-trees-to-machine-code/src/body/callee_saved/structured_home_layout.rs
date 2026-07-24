//! Physical-register preferences for dense structured saved-home classes.
//!
//! Legacy MWCC does not assign dense homes in creation order. It anchors the
//! last incoming parameter at the high end, the earliest input at the low end,
//! and packs eager and deferred values into the two gaps. Keeping this as a
//! pure layout policy prevents statement emission from depending on register
//! numbers or source identifiers.

use super::structured_locals::DeferredSavedHomePlan;
use super::structured_parameter_home_reuse::StructuredParameterHomeReuse;

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
) -> std::collections::HashMap<usize, u8> {
    let fresh_home_base = eager_count + parameter_count;
    let Some(first_saved) = 32usize.checked_sub(total_count) else {
        return std::collections::HashMap::new();
    };
    let occupied: std::collections::HashSet<_> = (0..fresh_home_base)
        .filter_map(|home| {
            dense_eager_home_preference(eager_count, parameter_count, total_count, home)
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
    rank_dense_deferred_groups(available, groups)
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
        let ranked = rank_dense_deferred_groups(vec![21, 22, 23, 24, 25, 26], groups);
        assert_eq!(ranked[&6], 26);
        assert_eq!(ranked[&7], 22);
        assert_eq!(ranked[&8], 25);
        assert_eq!(ranked[&9], 23);
        assert_eq!(ranked[&10], 24);
        assert_eq!(ranked[&11], 21);
    }
}
