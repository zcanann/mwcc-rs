//! Version-resolved policy for specialized integer-loop emission.

use mwcc_versions::IntegerLoopStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IntegerLoopPolicy {
    pub(crate) compare_before_ctr: bool,
    pub(crate) scaffold_after_ctr: bool,
    pub(crate) dependency_first: bool,
    pub(crate) recompute_recorded_immediate: bool,
    pub(crate) sign_extract_through_scratch: bool,
    pub(crate) reuse_count_home: bool,
}

impl IntegerLoopPolicy {
    pub(crate) fn resolve(style: IntegerLoopStyle) -> Self {
        match style {
            IntegerLoopStyle::ModernLatencyInterleaved => Self {
                compare_before_ctr: false,
                scaffold_after_ctr: false,
                dependency_first: false,
                recompute_recorded_immediate: false,
                sign_extract_through_scratch: false,
                reuse_count_home: true,
            },
            IntegerLoopStyle::LegacyDependencyFirst => Self {
                compare_before_ctr: true,
                scaffold_after_ctr: true,
                dependency_first: true,
                recompute_recorded_immediate: true,
                sign_extract_through_scratch: true,
                reuse_count_home: false,
            },
        }
    }

    pub(crate) fn pair_temporaries(
        self,
        count_register: u8,
        parameter_count: usize,
        has_sign: bool,
    ) -> PairTemporaries {
        let first_free = 3 + parameter_count as u8;
        if self.reuse_count_home {
            PairTemporaries {
                hz: count_register,
                sign: has_sign.then_some(first_free),
                lz: first_free + u8::from(has_sign),
            }
        } else {
            PairTemporaries {
                hz: first_free,
                sign: has_sign.then_some(first_free + 1),
                lz: first_free + 1 + u8::from(has_sign),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PairTemporaries {
    pub(crate) hz: u8,
    pub(crate) lz: u8,
    pub(crate) sign: Option<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_pair_reuses_count_home() {
        let policy = IntegerLoopPolicy::resolve(IntegerLoopStyle::ModernLatencyInterleaved);
        assert_eq!(
            policy.pair_temporaries(7, 5, true),
            PairTemporaries {
                hz: 7,
                lz: 9,
                sign: Some(8),
            }
        );
    }

    #[test]
    fn legacy_pair_keeps_temporaries_above_parameters() {
        let policy = IntegerLoopPolicy::resolve(IntegerLoopStyle::LegacyDependencyFirst);
        assert_eq!(
            policy.pair_temporaries(7, 5, true),
            PairTemporaries {
                hz: 8,
                lz: 10,
                sign: Some(9),
            }
        );
    }
}
