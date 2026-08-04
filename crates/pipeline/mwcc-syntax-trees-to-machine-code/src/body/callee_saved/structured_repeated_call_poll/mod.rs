//! Repeated direct-send / empty-call-poll transactions.
//!
//! Recognition, ordinary scheduling, and retained-string-anchor scheduling are
//! separate responsibilities so additional protocol variants do not bloat the
//! structured body planner.

mod handler;
mod recognize;
mod schedule;

pub(super) use recognize::is_repeated_call_poll_transaction;
