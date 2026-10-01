//! Unit-level inline summaries.
//!
//! The legacy owners summarized callee shapes (pointer walkers, queue
//! services, ...) to compose them at their callers; with PCode as the only
//! code generator the summaries carry no facts yet. In particular no function
//! is elided as an IPA walker: PCode expands inline calls itself.

use mwcc_syntax_trees::Function;

/// Facts about the unit's definitions the driver consults.
#[derive(Clone, Debug, Default)]
pub struct InlineSummaries;

impl InlineSummaries {
    /// Summarize a unit's definitions (none are modeled).
    pub fn analyze(_functions: &[Function]) -> Self {
        Self
    }

    /// Summarize a unit's definitions, including skipped inline bodies.
    pub fn analyze_with_skipped(_functions: &[Function], _skipped: &[Function]) -> Self {
        Self
    }

    /// Whether `-ipa file` drops a walker absorbed by its callers.
    pub fn should_elide_ipa_function(&self, _name: &str) -> bool {
        false
    }
}
