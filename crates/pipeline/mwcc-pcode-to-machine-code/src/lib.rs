//! MWCC backend stages after INITIAL CODE (see docs/backend-pipeline-proposal.md).

pub mod coloring;
pub mod finish;

pub use finish::{finish, FinishOptions};
