//! Versioned register and frame planning for pointer-punned select composites.

#[derive(Clone, Copy)]
pub(super) struct ModfLadderPlan {
    pub(super) frame_size: i16,
    pub(super) i0: u8,
    pub(super) i1: u8,
    pub(super) j0: u8,
    pub(super) temp: u8,
    pub(super) reload_spill: bool,
}

pub(super) fn modf_ladder_plan(
    convention: mwcc_versions::PunnedFloatFrameConvention,
) -> ModfLadderPlan {
    if convention == mwcc_versions::PunnedFloatFrameConvention::LegacyReloading {
        ModfLadderPlan {
            frame_size: 24,
            i0: 5,
            i1: 7,
            j0: 6,
            temp: 4,
            reload_spill: true,
        }
    } else {
        ModfLadderPlan {
            frame_size: 16,
            i0: 5,
            i1: 6,
            j0: 7,
            temp: 4,
            reload_spill: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_modf_plan_pads_and_separates_the_low_word() {
        let plan = modf_ladder_plan(mwcc_versions::PunnedFloatFrameConvention::LegacyReloading);
        assert_eq!(plan.frame_size, 24);
        assert_eq!((plan.i0, plan.i1, plan.j0, plan.temp), (5, 7, 6, 4));
        assert!(plan.reload_spill);
    }
}
