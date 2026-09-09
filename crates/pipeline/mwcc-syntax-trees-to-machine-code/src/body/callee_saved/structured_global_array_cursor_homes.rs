//! Saved-register priorities for strength-reduced global-array cursors.
//!
//! Cursor values rank in leading source-binding order, before eager scalars,
//! the logical index, and surviving parameters. Liveness and home sharing stay
//! with the ordinary deferred-home planner; this module supplies preferences.

use super::structured_global_array_cursors::Reduction;
use super::structured_locals::DeferredSavedHomePlan;
use super::structured_parameter_home_reuse::StructuredParameterHomeReuse;

pub(super) fn plan(
    reduction: &Reduction,
    eager_count: usize,
    parameter_count: usize,
    deferred: &DeferredSavedHomePlan,
    reuse: &StructuredParameterHomeReuse,
    count: usize,
    prefix_count: usize,
) -> Option<Vec<u32>> {
    let [group] = reduction.groups.as_slice() else {
        return None;
    };
    let role_count = group.cursors.len() + 1;
    if deferred.group_count != role_count
        || reuse.fresh_group_count != role_count
        || count != eager_count + parameter_count + role_count
        || count + prefix_count > 18
    {
        return None;
    }
    let home = |name: &str| {
        let group = deferred.group_if_present(name)?;
        (deferred.member_count(group) == 1).then(|| reuse.home_index(group))
    };
    let cursor_homes = group
        .cursors
        .iter()
        .map(|name| home(name))
        .collect::<Option<Vec<_>>>()?;
    let index_home = home(&group.index)?;
    rank(
        cursor_homes,
        index_home,
        eager_count,
        parameter_count,
        count,
        prefix_count,
    )
}

fn rank(
    mut order: Vec<usize>,
    index_home: usize,
    eager_count: usize,
    parameter_count: usize,
    count: usize,
    prefix_count: usize,
) -> Option<Vec<u32>> {
    order.extend(0..eager_count);
    order.push(index_home);
    order.extend(eager_count..eager_count + parameter_count);
    let mut distinct = order.clone();
    distinct.sort_unstable();
    if distinct != (0..count).collect::<Vec<_>>() || count + prefix_count > 18 {
        return None;
    }
    let mut preferences = vec![0; count];
    for (priority, home) in order.into_iter().enumerate() {
        preferences[home] = (31 - prefix_count - priority) as u32;
    }
    Some(preferences)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_cursor_roles_below_retained_bases_and_before_the_index() {
        assert_eq!(
            rank(vec![1, 2, 3, 4], 0, 0, 0, 5, 1),
            Some(vec![26, 30, 29, 28, 27])
        );
        assert_eq!(rank(vec![2, 3], 1, 0, 1, 4, 0), Some(vec![28, 29, 31, 30]));
        assert_eq!(rank(vec![2, 3], 1, 1, 0, 4, 0), Some(vec![29, 28, 31, 30]));
    }

    #[test]
    fn rejects_shared_or_missing_roles_and_register_overflow() {
        assert!(rank(vec![1, 1], 0, 0, 0, 3, 0).is_none());
        assert!(rank(vec![1], 0, 0, 0, 3, 0).is_none());
        assert!(rank(vec![1], 0, 0, 0, 2, 17).is_none());
    }
}
