//! Opt-in diagnostics for physical register pressure after allocation.

use crate::generator::Generator;
use mwcc_vreg::{Allocation, Liveness};

pub(crate) fn report_pressure(
    generator: &Generator,
    liveness: &Liveness,
    allocation: &Allocation,
    used: &[u8],
) {
    if std::env::var_os("MWCC_DIAGNOSTIC_ALLOCATION").is_none()
        || used.len() <= generator.callee_saved.len()
    {
        return;
    }

    eprintln!(
        "allocation pressure: declared={:?} used={used:?}",
        generator.callee_saved
    );
    for interval in &liveness.intervals {
        eprintln!(
            "  {:?} start={} end={} prefer={:?} physical={:?} slots={}",
            interval.vreg,
            interval.start,
            interval.end,
            interval.prefer,
            allocation.physical(interval.vreg),
            interval.live_slots.as_ref().map_or(0, Vec::len),
        );
    }
    for (index, instruction) in generator.output.instructions.iter().enumerate() {
        eprintln!("  {index:04}: {instruction:?}");
    }
}
