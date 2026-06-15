//! Pipeline: syntax trees -> machine code.
//!
//! Instruction selection and register assignment for the supported C subset,
//! reproducing mwcceppc's output byte-for-byte. `lib.rs` only wires the theme
//! modules together and exposes the entry point; the work lives in them.

use mwcc_core::Compilation;
use mwcc_machine_code::{FrameInfo, MachineFunction};
use mwcc_syntax_trees::{Function, GlobalDeclaration};
use mwcc_versions::{Behavior, CompilerConfig};
use std::collections::{HashMap, HashSet};

mod analysis;
mod generator;
mod operands;
mod body;
mod expressions;
mod arithmetic;
mod division;
mod comparisons;
mod control_flow;
mod narrow;
mod casts;
mod placement;
mod floats;
mod value_tracking;

use generator::Generator;

/// Lower a parsed function to machine code for the given compiler configuration.
pub fn lower_function(function: &Function, globals: &[GlobalDeclaration], config: CompilerConfig) -> Compilation<MachineFunction> {
    let mut generator = Generator {
        output: MachineFunction::new(function.name.clone()),
        locations: HashMap::new(),
        globals: globals.iter().map(|global| (global.name.clone(), global.declared_type)).collect(),
        reserved: HashSet::new(),
        frame_size: 0,
        behavior: Behavior::resolve(&config),
        constraints: mwcc_vreg::RegisterConstraints::gekko(),
        non_leaf: false,
        next_virtual: 0,
        register_avoid: HashMap::new(),
    };
    generator.assign_parameters(function)?;
    generator.evaluate_body(function)?;
    // Schedule on the virtual-register stream, then allocate. Ordering matters:
    // scheduling first means physical-register reuse cannot create false
    // dependencies that block a hoist, and allocation then colors the scheduled
    // order — reproducing mwcc's interleaving of the two phases.
    schedule_instructions(&mut generator);
    allocate_registers(&mut generator)?;

    // A function with a stack frame carries unwind tables. The codegen does not
    // yet save callee registers, so the saved counts are zero today; the FPU flag
    // is set for a non-leaf function that touches the FPU.
    if generator.frame_size != 0 {
        let uses_fpu = generator.output.instructions.iter().any(|instruction| instruction.is_floating_point());
        generator.output.frame = Some(FrameInfo {
            saved_gpr_count: 0,
            saved_fpr_count: 0,
            fpu_in_non_leaf: generator.non_leaf && uses_fpu,
        });
    }
    Ok(generator.output)
}

/// The register-allocation pass: resolve any virtual registers the selection
/// emitted to physical homes, honoring liveness and the target constraints.
///
/// Selection currently emits mostly physical registers inline; for those this is
/// a no-op (no virtual fields, nothing to assign). As selection is migrated to
/// emit virtuals, this pass becomes where their physical registers are decided —
/// each migration step verified byte-exact against the oracle. Running it
/// unconditionally keeps one pipeline (no fork between a legacy and a vreg path).
fn allocate_registers(generator: &mut Generator) -> Compilation<()> {
    let mut liveness = mwcc_vreg::analyze(&generator.output.instructions);
    if liveness.intervals.is_empty() {
        return Ok(()); // no virtuals — selection chose physical registers directly
    }
    // Apply selection's placement hints: registers a given virtual must avoid.
    for interval in &mut liveness.intervals {
        if let Some(avoid) = generator.register_avoid.get(&interval.vreg.id) {
            interval.avoid = avoid.clone();
        }
    }
    let allocation = mwcc_vreg::Allocator::allocate(
        &mwcc_vreg::LinearScan,
        &liveness.intervals,
        &liveness.pinned,
        &generator.constraints,
    )
        .map_err(|error| mwcc_core::Diagnostic::error(format!("register allocation failed: {error:?}")))?;
    mwcc_vreg::apply(&mut generator.output.instructions, &allocation);
    Ok(())
}

/// The instruction-scheduling pass (Phase E): reorder instructions within the
/// block to mwcc's pipeline schedule, then remap any relocation's instruction
/// index through the permutation so it still points at its instruction. With the
/// scheduler's identity policy this is a no-op; it becomes active as the policy
/// is tuned against the oracle.
fn schedule_instructions(generator: &mut Generator) {
    let permutation = mwcc_vreg::schedule(&mut generator.output.instructions);
    for relocation in &mut generator.output.relocations {
        relocation.instruction_index = permutation[relocation.instruction_index];
    }
}
