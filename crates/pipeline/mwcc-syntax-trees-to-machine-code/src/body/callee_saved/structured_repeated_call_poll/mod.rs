//! Repeated direct-send / empty-call-poll transactions.
//!
//! Recognition, ordinary scheduling, and retained-string-anchor scheduling are
//! separate responsibilities so additional protocol variants do not bloat the
//! structured body planner.

mod recognize;
mod schedule;

pub(super) use recognize::{is_repeated_call_poll_transaction, owns_long_string_data_anchor};
