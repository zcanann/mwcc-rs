//! Punned-double int-local writeback codegen. Split from the former single
//! punned_writeback.rs (fire 615) into per-family submodules; behavior-identical.

mod ladder;
mod shift;
mod guard;
mod block;

#[allow(unused_imports)]
use super::*;
