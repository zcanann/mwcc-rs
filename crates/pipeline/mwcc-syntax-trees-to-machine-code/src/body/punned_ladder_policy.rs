//! Version-resolved frame, placement, and store plans for punned ladder families.

use mwcc_versions::{ComputedStoreIssueStyle, PunnedFloatFrameConvention};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PairLadderPlan {
    pub(crate) frame_size: i16,
    pub(crate) frame_delta: i16,
    pub(crate) high_x: u8,
    pub(crate) high_y: u8,
    pub(crate) low_y: u8,
}

pub(crate) fn pair_ladder_plan(convention: PunnedFloatFrameConvention) -> PairLadderPlan {
    match convention {
        PunnedFloatFrameConvention::CompactLiveParameter => PairLadderPlan {
            frame_size: 32,
            frame_delta: -32,
            high_x: 0,
            high_y: 3,
            low_y: 0,
        },
        PunnedFloatFrameConvention::LegacyReloading => PairLadderPlan {
            frame_size: 24,
            frame_delta: -24,
            high_x: 3,
            high_y: 0,
            low_y: 0,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NormWritebackPlan {
    pub(crate) frame_size: i16,
    pub(crate) frame_delta: i16,
    pub(crate) high_store_first: bool,
}

pub(crate) fn norm_writeback_plan(
    convention: PunnedFloatFrameConvention,
    store_issue: ComputedStoreIssueStyle,
) -> NormWritebackPlan {
    let (frame_size, frame_delta) = match convention {
        PunnedFloatFrameConvention::CompactLiveParameter => (16, -16),
        PunnedFloatFrameConvention::LegacyReloading => (32, -32),
    };
    NormWritebackPlan {
        frame_size,
        frame_delta,
        high_store_first: store_issue == ComputedStoreIssueStyle::EvaluationOrder,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_pair_uses_the_unpadded_two_double_frame() {
        assert_eq!(
            pair_ladder_plan(PunnedFloatFrameConvention::LegacyReloading),
            PairLadderPlan {
                frame_size: 24,
                frame_delta: -24,
                high_x: 3,
                high_y: 0,
                low_y: 0,
            }
        );
    }

    #[test]
    fn legacy_norm_uses_padded_frame_and_evaluation_order() {
        assert_eq!(
            norm_writeback_plan(
                PunnedFloatFrameConvention::LegacyReloading,
                ComputedStoreIssueStyle::EvaluationOrder,
            ),
            NormWritebackPlan {
                frame_size: 32,
                frame_delta: -32,
                high_store_first: true,
            }
        );
    }
}
