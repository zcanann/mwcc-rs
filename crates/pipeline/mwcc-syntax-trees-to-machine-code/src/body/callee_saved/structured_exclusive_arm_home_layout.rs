//! Saved-register ordering for dense, path-colored conditional bodies.
//!
//! MWCC colors mutually exclusive arm values before assigning physical homes.
//! In the measured five-home shape, this leaves one eager entry value, one
//! recycled parameter home, and three deferred lifetime classes. Their physical
//! order is distinct from the ordinary dense creation-order layout.

use super::structured_locals::DeferredSavedHomePlan;
use super::structured_parameter_home_reuse::StructuredParameterHomeReuse;

pub(super) struct ExclusiveArmHomeLayout {
    preferences: std::collections::HashMap<usize, u8>,
}

impl ExclusiveArmHomeLayout {
    pub(super) fn plan(
        with_frame_array: bool,
        standalone_data_anchor: bool,
        eager_count: usize,
        parameter_count: usize,
        total_count: usize,
        deferred: &DeferredSavedHomePlan,
        reuse: &StructuredParameterHomeReuse,
    ) -> Option<Self> {
        if !with_frame_array
            || !standalone_data_anchor
            || eager_count != 1
            || parameter_count != 1
            || total_count != 5
            || deferred.group_count != 4
            || deferred.path_reuse_count != 2
        {
            return None;
        }
        let fresh_home_base = eager_count + parameter_count;
        let mut recycled_parameter_groups = 0;
        let mut fresh = Vec::new();
        for group in 0..deferred.group_count {
            let home = reuse.home_index(group);
            if home == eager_count {
                recycled_parameter_groups += 1;
            } else if home >= fresh_home_base {
                fresh.push((deferred.first_assignment(group), home));
            } else {
                return None;
            }
        }
        fresh.sort_unstable();
        fresh.dedup_by_key(|(_, home)| *home);
        if recycled_parameter_groups != 1 || fresh.len() != 3 {
            return None;
        }
        let preferences = preferences_for_fresh_homes(&fresh);
        Some(Self { preferences })
    }

    pub(super) fn preference(&self, home: usize) -> Option<u8> {
        self.preferences.get(&home).copied()
    }

    pub(super) fn data_anchor_preference(&self) -> u8 {
        29
    }
}

fn preferences_for_fresh_homes(
    fresh: &[(usize, usize)],
) -> std::collections::HashMap<usize, u8> {
    std::collections::HashMap::from([
        (0, 28),
        (1, 26),
        (fresh[0].1, 31),
        (fresh[1].1, 30),
        (fresh[2].1, 27),
    ])
}

#[cfg(test)]
mod tests {
    use super::preferences_for_fresh_homes;

    #[test]
    fn orders_dense_path_colored_lifetime_classes() {
        let preferences = preferences_for_fresh_homes(&[(2, 4), (5, 2), (9, 3)]);

        assert_eq!(preferences[&0], 28);
        assert_eq!(preferences[&1], 26);
        assert_eq!(preferences[&4], 31);
        assert_eq!(preferences[&2], 30);
        assert_eq!(preferences[&3], 27);
    }
}
