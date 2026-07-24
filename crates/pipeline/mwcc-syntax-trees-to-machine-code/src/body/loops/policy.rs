//! Version-resolved policy for specialized integer-loop emission.

use mwcc_versions::IntegerLoopStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IntegerLoopPolicy {
    pub(crate) compare_before_ctr: bool,
    pub(crate) scaffold_after_ctr: bool,
    pub(crate) dependency_first: bool,
    pub(crate) counter_fills_result_latency: bool,
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
                counter_fills_result_latency: false,
                recompute_recorded_immediate: false,
                sign_extract_through_scratch: false,
                reuse_count_home: true,
            },
            IntegerLoopStyle::LegacyDependencyFirst => Self {
                compare_before_ctr: true,
                scaffold_after_ctr: true,
                dependency_first: true,
                counter_fills_result_latency: false,
                recompute_recorded_immediate: true,
                sign_extract_through_scratch: true,
                reuse_count_home: false,
            },
        }
    }

    /// Overlay the issue-order decisions made by a processor latency model
    /// without changing the build family's register-allocation policy.
    pub(crate) fn with_latency_interleaving(mut self, enabled: bool) -> Self {
        if enabled {
            self.scaffold_after_ctr = false;
            self.dependency_first = false;
            self.counter_fills_result_latency = true;
        }
        self
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

    #[test]
    fn latency_interleaving_preserves_legacy_allocation_traits() {
        let legacy = IntegerLoopPolicy::resolve(IntegerLoopStyle::LegacyDependencyFirst);
        let interleaved = legacy.with_latency_interleaving(true);

        assert!(!interleaved.scaffold_after_ctr);
        assert!(!interleaved.dependency_first);
        assert!(interleaved.counter_fills_result_latency);
        assert_eq!(interleaved.compare_before_ctr, legacy.compare_before_ctr);
        assert_eq!(
            interleaved.recompute_recorded_immediate,
            legacy.recompute_recorded_immediate
        );
        assert_eq!(
            interleaved.sign_extract_through_scratch,
            legacy.sign_extract_through_scratch
        );
        assert_eq!(interleaved.reuse_count_home, legacy.reuse_count_home);
    }
}
