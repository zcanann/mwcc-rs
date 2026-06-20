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
mod switch;
mod symbol_order;
mod frame;

use generator::Generator;

/// Lower a parsed function to machine code for the given compiler configuration.
/// `call_return_types` maps callable names (prototypes and definitions) to their
/// return type, so a call's result type is known (e.g. a `double`-returning math
/// routine drives the `frsp` of `(float)cos(x)`).
pub fn lower_function(function: &Function, globals: &[GlobalDeclaration], call_return_types: &HashMap<String, mwcc_syntax_trees::Type>, config: CompilerConfig) -> Compilation<MachineFunction> {
    let mut generator = Generator {
        output: MachineFunction::new(function.name.clone()),
        locations: HashMap::new(),
        // A `const` global is read-only and mwcc *folds* its value into each reader
        // (`return K;` becomes `li r3, <value>`, not a load). That folding is not
        // modeled yet, so const globals are withheld from the operand map: any
        // reference then defers ("unknown variable") rather than emitting a wrong
        // memory load. The const global is still emitted as read-only data.
        globals: globals.iter().filter(|global| !global.is_const).map(|global| (global.name.clone(), global.declared_type)).collect(),
        // Subscriptable array globals (non-const) with their total byte size, so a
        // `g[i]` picks the right address mode (SDA21 vs ADDR16) by size. An EXTERN
        // array is included: mwcc addresses it identically to a defined one (verified
        // — the section is irrelevant to the SDA21/ADDR16 choice), referencing it
        // through a relocation to the undefined symbol.
        global_array_sizes: globals
            .iter()
            .filter(|global| !global.is_const)
            .filter_map(|global| {
                global.array_length.map(|length| {
                    // A struct array's element size is its laid-out struct size, not the
                    // word-default scalar width — so `struct S arr[N]` measures N*sizeof,
                    // picking the right address mode (SDA21 vs ADDR16) by true total size.
                    let element_size = match global.declared_type {
                        mwcc_syntax_trees::Type::Struct { size, .. } => size as u32,
                        other => other.width() as u32 / 8,
                    };
                    (global.name.clone(), element_size * length as u32)
                })
            })
            .collect(),
        reserved: HashSet::new(),
        frame_size: 0,
        behavior: Behavior::resolve(&config),
        constraints: mwcc_vreg::RegisterConstraints::gekko(),
        non_leaf: false,
        next_virtual: 0,
        register_avoid: HashMap::new(),
        stored_globals: HashMap::new(),
        frame_slots: HashMap::new(),
        reuse_scratch_constant: false,
        scratch_constant: None,
        prematerialized_constants: Vec::new(),
        callee_saved: Vec::new(),
        call_return_types: call_return_types.clone(),
    };
    generator.assign_parameters(function)?;
    generator.evaluate_body(function)?;
    // The names this function references, in mwcc's symbol-table order (an AST
    // traversal); the writer assigns its external/global symbols in this order.
    generator.output.symbol_order = symbol_order::referenced_names(function);
    // Schedule on the virtual-register stream, then allocate. Ordering matters:
    // scheduling first means physical-register reuse cannot create false
    // dependencies that block a hoist, and allocation then colors the scheduled
    // order — reproducing mwcc's interleaving of the two phases.
    schedule_instructions(&mut generator);
    allocate_registers(&mut generator)?;
    // Issue the epilogue's saved-LR reload right after the last call (ahead of the
    // post-call computation), as mwcc does — a final pass on the physical stream.
    hoist_link_register_reload(&mut generator);
    // Symmetrically, delay the prologue's saved-LR store past the first call's ready
    // argument materializations (mwcc fills the mflr->store latency gap).
    schedule_link_register_save(&mut generator);

    // A function with a stack frame carries unwind tables. The codegen does not
    // yet save callee registers, so the saved counts are zero today; the FPU flag
    // is set for a non-leaf function that touches the FPU.
    // The `extab`/`extabindex` unwind tables are emitted only with C++ exceptions
    // on (the default); `-Cpp_exceptions off` suppresses them (the frame itself is
    // unchanged). `frame` drives those sections, so leave it `None` when off.
    if generator.frame_size != 0 && config.flags.cpp_exceptions {
        // The extab FPU flag is set for a non-leaf that touches the FPU, and also
        // for a leaf-with-frame that uses single-precision float arithmetic (an
        // `int`->`float` conversion's `fsubs`) — but not a double-only or
        // convert-to-int frame (`fsub`/`fctiwz` leave it clear).
        let touches_fpu = generator.output.instructions.iter().any(|instruction| instruction.is_floating_point());
        let single_arithmetic = generator.output.instructions.iter().any(|instruction| instruction.is_single_precision_arithmetic());
        generator.output.frame = Some(FrameInfo {
            saved_gpr_count: generator.callee_saved.len() as u8,
            saved_fpr_count: 0,
            uses_fpu: (generator.non_leaf && touches_fpu) || single_arithmetic,
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

/// Move the epilogue's saved-LR reload up to right after the last call, remapping
/// relocation indices through the resulting permutation.
fn hoist_link_register_reload(generator: &mut Generator) {
    let permutation = mwcc_vreg::hoist_link_register_reload(&mut generator.output.instructions);
    for relocation in &mut generator.output.relocations {
        relocation.instruction_index = permutation[relocation.instruction_index];
    }
}

fn schedule_link_register_save(generator: &mut Generator) {
    let permutation = mwcc_vreg::schedule_link_register_save(&mut generator.output.instructions);
    for relocation in &mut generator.output.relocations {
        relocation.instruction_index = permutation[relocation.instruction_index];
    }
}
