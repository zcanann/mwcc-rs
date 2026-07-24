//! Straight-line bitfield packet accumulation followed by a fixed-port write.
//!
//! The GX SDK builds BP packets in one local word. Build 163 keeps the first
//! eight arguments in their incoming registers, pulls the remaining fields
//! from their ABI stack lanes, and interleaves those loads with bit inserts.

mod emit;
mod recognize;
