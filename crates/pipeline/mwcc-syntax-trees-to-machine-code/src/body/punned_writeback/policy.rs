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

pub(super) struct ShiftWritebackPlan {
    pub(super) frame_size: i16,
    pub(super) reload_early_return: bool,
    pub(super) reload_before_float_guard: bool,
    pub(super) constant_label_bump: u32,
}

pub(super) fn shift_writeback_plan(
    style: mwcc_versions::PunnedShiftWritebackStyle,
    has_sign_block: bool,
    has_float_guard: bool,
) -> ShiftWritebackPlan {
    let legacy = style == mwcc_versions::PunnedShiftWritebackStyle::LegacyReloading;
    ShiftWritebackPlan {
        frame_size: if legacy && !has_sign_block { 24 } else { 16 },
        reload_early_return: legacy,
        reload_before_float_guard: legacy,
        // The flat guarded form in build 163 retains twelve internal label
        // slots before its first pooled double; nested sign forms reuse it.
        constant_label_bump: if legacy && has_float_guard && !has_sign_block {
            12
        } else {
            0
        },
    }
}

pub(super) fn allocate_shift_registers(
    style: mwcc_versions::PunnedShiftWritebackStyle,
    values: &[mwcc_vreg::int_alloc::Value],
) -> Vec<u8> {
    use mwcc_vreg::int_alloc::{assign, model_order, Class};

    if style != mwcc_versions::PunnedShiftWritebackStyle::LegacyReloading {
        return assign(&model_order(values), values);
    }

    let crossers = values.iter().any(|value| {
        matches!(
            value.class,
            Class::LoadSurviving | Class::Shift | Class::ArmShift | Class::Scrutinee
        )
    });
    let mut order = Vec::new();
    let push_class = |order: &mut Vec<usize>, class: Class, descending: bool| {
        let mut members: Vec<usize> = (0..values.len())
            .filter(|&index| values[index].class == class)
            .collect();
        members.sort_by_key(|&index| {
            let definition = i64::from(values[index].def);
            if descending {
                -definition
            } else {
                definition
            }
        });
        order.extend(members);
    };
    push_class(&mut order, Class::Temp, false);
    if crossers {
        push_class(&mut order, Class::LoadDiscarded, false);
    }
    let mut computed: Vec<usize> = (0..values.len())
        .filter(|&index| {
            matches!(
                values[index].class,
                Class::Computed | Class::Mask | Class::ArmShift
            )
        })
        .collect();
    computed.sort_by_key(|&index| values[index].last);
    order.extend(computed);
    push_class(&mut order, Class::Shift, false);
    push_class(&mut order, Class::LoadSurviving, true);
    push_class(&mut order, Class::Scrutinee, false);
    if !crossers {
        push_class(&mut order, Class::LoadDiscarded, true);
    }
    assign(&order, values)
}

pub(super) struct LegacyShiftCarryRegisters {
    pub(super) mask: u8,
    pub(super) source: u8,
    pub(super) guard: u8,
    pub(super) shift: u8,
    pub(super) other: u8,
    pub(super) carry_one: u8,
}

pub(super) fn legacy_shift_carry_registers(
    style: mwcc_versions::PunnedShiftWritebackStyle,
) -> Option<LegacyShiftCarryRegisters> {
    (style == mwcc_versions::PunnedShiftWritebackStyle::LegacyReloading).then_some(
        LegacyShiftCarryRegisters {
            mask: 3,
            source: 4,
            guard: 5,
            shift: 6,
            other: 7,
            carry_one: 3,
        },
    )
}

pub(super) struct LegacyLadderRegisters {
    pub(super) extract: u8,
    pub(super) scrutinee: u8,
    pub(super) source_load: u8,
    pub(super) source_home: u8,
    pub(super) other: u8,
    pub(super) arm_temp: u8,
    pub(super) arm_shift: u8,
    pub(super) arm_mask: u8,
    pub(super) carry_one: u8,
    pub(super) constant_label_bump: u32,
}

pub(super) fn legacy_ladder_registers(
    style: mwcc_versions::PunnedShiftWritebackStyle,
) -> Option<LegacyLadderRegisters> {
    (style == mwcc_versions::PunnedShiftWritebackStyle::LegacyReloading).then_some(
        LegacyLadderRegisters {
            extract: 3,
            scrutinee: 4,
            source_load: 0,
            source_home: 6,
            other: 7,
            arm_temp: 3,
            arm_shift: 5,
            arm_mask: 3,
            carry_one: 3,
            constant_label_bump: 4,
        },
    )
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

    #[test]
    fn legacy_shift_plan_reloads_and_pads_non_sign_forms() {
        let flat = shift_writeback_plan(
            mwcc_versions::PunnedShiftWritebackStyle::LegacyReloading,
            false,
            false,
        );
        assert_eq!(flat.frame_size, 24);
        assert!(flat.reload_early_return);
        assert!(flat.reload_before_float_guard);
        let nested = shift_writeback_plan(
            mwcc_versions::PunnedShiftWritebackStyle::LegacyReloading,
            true,
            true,
        );
        assert_eq!(nested.frame_size, 16);
    }
}
