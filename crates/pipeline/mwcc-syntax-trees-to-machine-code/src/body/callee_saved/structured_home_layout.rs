//! Physical-register preferences for dense structured saved-home classes.
//!
//! Legacy MWCC does not assign dense homes in creation order. It anchors the
//! last incoming parameter at the high end, the earliest input at the low end,
//! and packs eager and deferred values into the two gaps. Keeping this as a
//! pure layout policy prevents statement emission from depending on register
//! numbers or source identifiers.

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
}
