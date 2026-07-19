//! Versioned frame and register-role planning for punned writebacks.

pub(super) struct GuardRegisterRoles {
    pub(super) homes: Vec<u8>,
    pub(super) loads: Vec<u8>,
    pub(super) guard: u8,
    pub(super) copied_source: Option<usize>,
}

pub(super) fn plan_guard_registers(
    legacy_reloading: bool,
    has_folded_compare: bool,
    local_count: usize,
    guard_source: Option<usize>,
    source_read_in_block: bool,
    scratch_taken: bool,
    has_guard_local: bool,
) -> GuardRegisterRoles {
    if legacy_reloading && has_folded_compare {
        let loads = if local_count == 1 {
            vec![4]
        } else {
            vec![3, 4]
        };
        let mut homes = loads.clone();
        let copied_source = (!source_read_in_block).then_some(guard_source.expect("guard source"));
        if let Some(source) = copied_source {
            homes[source] = 0;
        }
        return GuardRegisterRoles {
            homes,
            loads,
            guard: if local_count == 2 { 0 } else { 3 },
            copied_source,
        };
    }

    let mut next_general = if has_guard_local { 4u8 } else { 3u8 };
    let mut homes = Vec::new();
    let mut r0_used = scratch_taken;
    for _ in 0..local_count {
        if !r0_used {
            homes.push(0);
            r0_used = true;
        } else {
            homes.push(next_general);
            next_general += 1;
        }
    }
    GuardRegisterRoles {
        loads: homes.clone(),
        homes,
        guard: 3,
        copied_source: None,
    }
}

pub(super) fn guard_frame_size(
    legacy_reloading: bool,
    has_float_guard: bool,
    outer_laddered: bool,
) -> i16 {
    if legacy_reloading && !has_float_guard && !outer_laddered {
        24
    } else {
        16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_single_read_guard_separates_load_and_home() {
        let roles = plan_guard_registers(true, true, 2, Some(0), false, true, true);
        assert_eq!(roles.loads, [3, 4]);
        assert_eq!(roles.homes, [0, 4]);
        assert_eq!(roles.guard, 0);
        assert_eq!(roles.copied_source, Some(0));
    }

    #[test]
    fn nested_source_read_keeps_legacy_load_home() {
        let roles = plan_guard_registers(true, true, 2, Some(0), true, true, true);
        assert_eq!(roles.loads, [3, 4]);
        assert_eq!(roles.homes, [3, 4]);
        assert_eq!(roles.guard, 0);
        assert_eq!(roles.copied_source, None);
    }

    #[test]
    fn legacy_guard_padding_excludes_float_and_ladder_forms() {
        assert_eq!(guard_frame_size(true, false, false), 24);
        assert_eq!(guard_frame_size(true, true, false), 16);
        assert_eq!(guard_frame_size(true, false, true), 16);
        assert_eq!(guard_frame_size(false, false, false), 16);
    }
}
