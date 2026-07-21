//! Tokenizers backed by a 256-bit byte-class map.
//!
//! The map construction, leading-delimiter skip, token scan, continuation
//! writeback, and empty-token result share one frame and register schedule in
//! MWCC. Keep recognition and emission under one semantic owner rather than
//! teaching the generic loop path several mutually dependent special cases.

#[allow(unused_imports)]
use super::*;

struct TokenizerPlan<'a> {
    string: &'a str,
    control: &'a str,
    next_token: &'a str,
}

mod emit;
mod recognize;

#[cfg(test)]
mod tests;
