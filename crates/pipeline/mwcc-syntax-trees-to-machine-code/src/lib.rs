//! Pipeline: syntax trees -> machine code.
//!
//! Instruction selection and register assignment for the supported C subset,
//! reproducing mwcceppc's output byte-for-byte. `lib.rs` only wires the theme
//! modules together and exposes the entry point; the work lives in them.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{FrameInfo, Instruction, MachineFunction};
use mwcc_syntax_trees::{Function, GlobalDeclaration};
use mwcc_versions::{Behavior, CompilerConfig};
use std::collections::{HashMap, HashSet};

mod analysis;
mod arithmetic;
mod asm;
mod body;
mod captures;
mod casts;
mod comparisons;
mod control_flow;
mod copy_convention;
mod cxx_abi;
mod dag_emitter;
mod division;
mod expressions;
mod float;
mod floats;
mod frame;
mod frexp_family;
mod generator;
mod inline_expansion;
mod inline_summaries;
mod legacy_comparisons;
mod narrow;
mod operands;
mod ordinal_accounting;
mod placement;
mod switch;
mod symbol_order;
mod value_tracking;

use generator::Generator;
pub use inline_expansion::InlineBodySet;
pub use inline_summaries::InlineSummaries;

/// Apply optimizer bookkeeping that is observable only after every function in
/// the translation unit is known. File IPA can move labels from later functions
/// ahead of the first pool constant, so this cannot be modeled honestly inside
/// [`lower_function`].
pub fn apply_unit_ordinal_accounting(
    functions: &[Function],
    machine_functions: &mut [MachineFunction],
    config: CompilerConfig,
) {
    ordinal_accounting::apply_unit(
        functions,
        machine_functions,
        Behavior::resolve(&config).function_ordinal_accounting_style,
    );
}

/// Materialize weak C++ `this`-adjustor functions demanded by secondary
/// vtable relocation targets. These are unit-level compiler products and have
/// no source [`Function`] to pass through [`lower_function`].
pub fn lower_vtable_adjustor_thunks(
    globals: &[GlobalDeclaration],
    class_declaration_order: &[String],
) -> Compilation<Vec<MachineFunction>> {
    cxx_abi::lower_vtable_adjustor_thunks(globals, class_declaration_order)
}

/// Lower a parsed function to machine code for the given compiler configuration.
/// `call_return_types` maps callable names (prototypes and definitions) to their
/// return type, so a call's result type is known (e.g. a `double`-returning math
/// routine drives the `frsp` of `(float)cos(x)`).
pub fn lower_function(
    function: &Function,
    globals: &[GlobalDeclaration],
    call_return_types: &HashMap<String, mwcc_syntax_trees::Type>,
    call_parameter_types: &HashMap<String, Vec<mwcc_syntax_trees::Type>>,
    skipped_inline_names: &std::collections::HashSet<String>,
    weak_materialized_names: &std::collections::HashSet<String>,
    prototyped_names: &std::collections::HashSet<String>,
    variadic_definitions: &std::collections::HashSet<String>,
    fixed_address_arrays: &HashMap<String, (i64, mwcc_syntax_trees::Type)>,
    fixed_address_objects: &HashMap<String, i64>,
    inline_bodies: &InlineBodySet,
    inline_summaries: &InlineSummaries,
    config: CompilerConfig,
) -> Compilation<MachineFunction> {
    // An inline-`asm` function is emitted verbatim — no register allocation,
    // scheduling, or optimizer — so it bypasses the ordinary codegen path entirely.
    if function.asm_body.is_some() {
        return asm::assemble_asm_function(function, Behavior::resolve(&config));
    }
    let expanded_constructor = function
        .name
        .starts_with("__ct__")
        .then(|| inline_bodies.expand_calls(function))
        .flatten();
    if let Some(output) = cxx_abi::lower_composed_constructor(
        expanded_constructor.as_ref().unwrap_or(function),
        globals,
        config.clone(),
    ) {
        return Ok(output);
    }
    if let Some(output) = cxx_abi::lower_virtual_constructor(function, globals) {
        return Ok(output);
    }
    if let Some(output) =
        cxx_abi::lower_composed_destructor(function, inline_summaries, config.clone())
    {
        return Ok(output);
    }
    if let Some(output) = cxx_abi::lower_virtual_destructor(function, globals, config.clone()) {
        return Ok(output);
    }
    // A STATIC CONST float/double global is DE-NAMED by mwcc: every read compiles
    // as the literal value, pooled anonymously (@N in .sdata2) with no named
    // symbol — measured: `static const double two54 = C; x * two54` emits the
    // exact bytes of the inline literal. Substitute before lowering (a name
    // shadowed by a parameter or local is left alone).
    let substituted = body::substitute_const_float_globals(function, globals);
    let function = substituted.as_ref().unwrap_or(function);
    // A `static` local has STATIC storage — an anonymous `<name>$N` object in `.sdata`/`.sbss`,
    // codegen'd like a file-scope global, not a frame slot. That path (the `$N = @N-1` numbering, the
    // per-function symbol, global-style access) is not built yet, so defer rather than mis-treat it as
    // an automatic local (`register`/`auto` hints, in contrast, are ordinary automatics and proceed).
    // STATIC locals have static storage: they compile as GLOBAL references
    // (`name$K` LOCAL objects — the writer numbers them off the function's
    // @N sequence). Register each in the operand maps and record its datum;
    // the automatic-local machinery never sees it.
    let static_locals: Vec<mwcc_syntax_trees::LocalDeclaration> = function
        .locals
        .iter()
        .filter(|local| local.is_static)
        .cloned()
        .collect();
    let mut static_local_data: Vec<mwcc_machine_code::StaticLocal> = Vec::new();
    for local in &static_locals {
        if globals.iter().any(|global| global.name == local.name) {
            return Err(Diagnostic::error(
                "a static local shadowing a global is not supported yet (roadmap)",
            ));
        }
        // A struct-typed static (`static __mem_pool protopool;`) carries its
        // own byte size; scalars derive from the type width.
        let element = match local.declared_type {
            mwcc_syntax_trees::Type::Struct { size, .. } => size as u32,
            other => other.width() as u32 / 8,
        };
        let size = element * local.array_length.map_or(1, u32::from);
        // The byte image: a brace-list array, or a scalar literal folded here.
        let bytes = match (&local.data_bytes, &local.initializer) {
            (Some(bytes), _) => Some(bytes.clone()),
            (None, Some(mwcc_syntax_trees::Expression::IntegerLiteral(value))) => (*value != 0)
                .then(|| match local.declared_type {
                    mwcc_syntax_trees::Type::Double => (*value as f64).to_be_bytes().to_vec(),
                    mwcc_syntax_trees::Type::Float => (*value as f32).to_be_bytes().to_vec(),
                    _ => (*value as i32).to_be_bytes().to_vec(),
                }),
            (None, Some(mwcc_syntax_trees::Expression::FloatLiteral(value))) => {
                Some(match local.declared_type {
                    mwcc_syntax_trees::Type::Float => (*value as f32).to_be_bytes().to_vec(),
                    _ => value.to_be_bytes().to_vec(),
                })
            }
            (None, Some(_)) => {
                return Err(Diagnostic::error(
                    "a non-constant static local initializer is not supported yet (roadmap)",
                ));
            }
            (None, None) => None,
        };
        let alignment = match local.declared_type {
            mwcc_syntax_trees::Type::Struct { align, .. } => (align as u32).max(4),
            // A char static records its natural alignment 1 (measured: mp4
            // alloc's init$130 comment record).
            mwcc_syntax_trees::Type::Char | mwcc_syntax_trees::Type::UnsignedChar
                if local.array_length.is_none() =>
            {
                1
            }
            _ => element.max(4),
        };
        static_local_data.push(mwcc_machine_code::StaticLocal {
            name: local.name.clone(),
            initial_bytes: bytes,
            size,
            alignment,
            is_const: local.is_const,
            relocations: local.data_relocations.clone(),
        });
    }
    // The body machinery never sees the statics as automatic locals.
    let stripped;
    let function = if static_locals.is_empty() {
        function
    } else {
        stripped = mwcc_syntax_trees::Function {
            locals: function
                .locals
                .iter()
                .filter(|local| !local.is_static)
                .cloned()
                .collect(),
            ..function.clone()
        };
        &stripped
    };
    let variadic_definition = variadic_definitions.contains(&function.name);
    let mut generator = Generator {
        variadic_definition,
        variadic_callees: variadic_definitions.clone(),
        output: MachineFunction::new(function.name.clone()),
        labels: mwcc_vreg::Labels::default(),
        locations: HashMap::new(),
        // A `const` global is read-only and mwcc *folds* its value into each reader
        // (`return K;` becomes `li r3, <value>`, not a load). That folding is not
        // modeled yet, so const globals are withheld from the operand map: any
        // reference then defers ("unknown variable") rather than emitting a wrong
        // memory load. The const global is still emitted as read-only data.
        // Const ARRAYS (the .rodata ctype tables) stay visible — their reads
        // address like any large array; const SCALARS keep deferring (float ones
        // de-name above, int ones fold differently).
        globals: globals
            .iter()
            .filter(|global| !global.is_const || global.array_length.is_some())
            .map(|global| (global.name.clone(), global.declared_type))
            .chain(
                // Static locals address like globals (const scalars stay
                // visible too: their `name$K` datum is always materialized,
                // never value-folded — measured).
                static_locals
                    .iter()
                    .map(|local| (local.name.clone(), local.declared_type)),
            )
            .collect(),
        volatile_globals: globals
            .iter()
            .filter(|global| global.is_volatile)
            .map(|global| global.name.clone())
            .collect(),
        // Subscriptable array globals (non-const) with their total byte size, so a
        // `g[i]` picks the right address mode (SDA21 vs ADDR16) by size. An EXTERN
        // array is included: mwcc addresses it identically to a defined one (verified
        // — the section is irrelevant to the SDA21/ADDR16 choice), referencing it
        // through a relocation to the undefined symbol.
        global_array_sizes: static_locals
            .iter()
            .filter_map(|local| {
                local.array_length.map(|length| {
                    let element = local.declared_type.width() as u32 / 8;
                    (local.name.clone(), element * length as u32)
                })
            })
            .chain(
                globals
                    .iter()
                    .filter(|global| !global.is_const || global.array_length.is_some())
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
                    }),
            )
            .collect(),
        reserved: HashSet::new(),
        frame_size: 0,
        float: generator::FloatContext::default(),
        double_tables: globals
            .iter()
            .filter(|global| {
                global.is_static
                    && global.is_const
                    && global.declared_type == mwcc_syntax_trees::Type::Double
                    && global.array_length.is_some()
            })
            .map(|global| global.name.clone())
            .collect(),
        behavior: Behavior::resolve(&config),
        constraints: mwcc_vreg::RegisterConstraints::gekko(),
        non_leaf: false,
        callee_saved_float: 0,
        next_virtual: 0,
        register_avoid: HashMap::new(),
        register_prefer: HashMap::new(),
        stored_globals: HashMap::new(),
        const_address_bases: HashSet::new(),
        emitted_variable_index_store: false,
        prematerialized_float_constants: Vec::new(),
        frame_slots: HashMap::new(),
        written_slots: HashSet::new(),
        frame_feeding_local_pressure: None,
        reuse_scratch_constant: false,
        scratch_constant: None,
        prematerialized_constants: Vec::new(),
        callee_saved: Vec::new(),
        entry_parameter_words: function
            .parameters
            .iter()
            .map(|parameter| usize::from(parameter.parameter_type.width()).div_ceil(32).max(1))
            .sum(),
        legacy_callee_saved_frame_layout:
            generator::LegacyCalleeSavedFrameLayout::InferFromValueOrigin,
        legacy_discarded_call_locals: 0,
        epilogue_lr_first: false,
        epilogue_lr_before_gprs: false,
        narrow_truncation_context: false,
        known_locals: std::collections::HashSet::new(),
        call_return_types: call_return_types.clone(),
        fixed_address_arrays: fixed_address_arrays
            .iter()
            .map(|(name, (address, element))| (name.clone(), (*address as u32, *element)))
            .collect(),
        fixed_address_objects: fixed_address_objects
            .iter()
            .map(|(name, address)| (name.clone(), *address as u32))
            .collect(),
        frame_row_bytes: function
            .locals
            .iter()
            .filter_map(|local| local.row_bytes.map(|row| (local.name.clone(), row)))
            .collect(),
        descending_allocation_top: None,
        skipped_inline_names: skipped_inline_names.clone(),
        prototyped_names: prototyped_names.clone(),
        weak_materialized_names: weak_materialized_names.clone(),
        call_parameter_types: call_parameter_types.clone(),
        inline_bodies: inline_bodies.clone(),
        inline_summaries: inline_summaries.clone(),
    };
    generator.assign_parameters(function)?;
    generator.evaluate_body(function).map_err(|mut diagnostic| {
        let context = format!("function '{}'", function.name);
        if !diagnostic.message.contains(&context) {
            diagnostic.message.push_str(&format!(" (in {context})"));
        }
        diagnostic
    })?;
    // Resolve label-addressed branch targets now that emission is complete (and
    // before any stream-shortening pass could shift instruction indices).
    if generator
        .labels
        .resolve(&mut generator.output.instructions)
        .is_err()
    {
        return Err(mwcc_core::Diagnostic::error(
            "internal: a branch label was used but never bound",
        ));
    }
    // Peephole: a conditional forward branch whose target is the function's TERMINAL
    // `blr` is byte-identical to `b<cc>lr` — mwcc always emits the branch-to-link form
    // (`if(c) *p=x; return a;` -> `cmpwi;blelr;stw;blr`, never `ble .Lend`). Collapse it
    // so any guarded tail matches, whichever handler emitted the forward branch. Safe
    // ONLY for the terminal blr (a leaf epilogue is a bare `blr`): the fall-through always
    // reaches it, so nothing is left dead; a mid-function blr or framed epilogue (whose
    // target is the teardown, not a bare blr) is untouched. The forward branch's
    // (options, condition_bit) already encode the same BO/BI, so reusing them yields the
    // exact `b<cc>lr` mwcc emits.
    collapse_forward_branch_to_terminal_blr(&mut generator.output.instructions);
    // The names this function references, in mwcc's symbol-table order (an AST
    // traversal); the writer assigns its external/global symbols in this order.
    if generator.output.symbol_order.is_empty() {
        // A capture template may pin its own measured order (atof, pikmin
        // s_ldexp) — only derive from the AST when it didn't.
        if generator.output.symbol_order.is_empty() {
            generator.output.symbol_order = symbol_order::referenced_names(
                function,
                &generator.call_return_types,
                generator.behavior.symbol_traversal_style,
            );
        }
    }
    generator.output.referenced_function_symbols = generator
        .output
        .symbol_order
        .iter()
        .filter(|name| generator.call_return_types.contains_key(name.as_str()))
        .cloned()
        .collect();
    // A call target with no prototype/definition (absent from `call_return_types`) was
    // IMPLICITLY declared — K&R first-use. mwcc creates its symbol at the call site inside
    // the body, so the writer emits it AFTER the function symbol (a prototyped external,
    // created at its file-scope declaration, precedes the function). Collected from the
    // call (Rel24) relocations, in first-call order, deduplicated.
    {
        use mwcc_machine_code::{RelocationKind, RelocationTarget};
        let mut seen = HashSet::new();
        for relocation in &generator.output.relocations {
            if let (RelocationKind::Rel24, RelocationTarget::External(name)) =
                (&relocation.kind, &relocation.target)
            {
                // Implicit means NO PROTOTYPE at the call — a unit-DEFINED but
                // unprototyped callee is still implicit (mwcc creates its
                // symbol at the call site; measured: AC file_io's fclose ->
                // fflush keeps plain [fclose, fflush] order, no hoist).
                if !prototyped_names.contains(name.as_str()) && seen.insert(name.clone()) {
                    generator
                        .output
                        .implicit_external_callees
                        .push(name.clone());
                }
            }
        }
    }
    generator.output.is_static = function.is_static;
    generator.output.is_weak = function.is_weak;
    generator.output.text_deferred = function.text_deferred;
    generator.output.section = function.section.clone();
    generator.output.force_active = function.force_active;
    if generator.output.static_locals.is_empty() {
        generator.output.static_locals = static_local_data;
    }
    // Schedule on the virtual-register stream, then allocate. Ordering matters:
    // scheduling first means physical-register reuse cannot create false
    // dependencies that block a hoist, and allocation then colors the scheduled
    // order — reproducing mwcc's interleaving of the two phases.
    schedule_instructions(&mut generator);
    allocate_registers(&mut generator)?;
    // Coalesce away `mr rX,rX` self-moves the allocator leaves when it colors a value's
    // virtual home to the register the value already holds (mwcc coalesces them).
    coalesce_self_moves(&mut generator);
    // Issue the epilogue's saved-LR reload right after the last call (ahead of the
    // post-call computation), as mwcc does — a final pass on the physical stream.
    hoist_link_register_reload(&mut generator);
    // Symmetrically, delay the prologue's saved-LR store past the first call's ready
    // argument materializations (mwcc fills the mflr->store latency gap).
    schedule_link_register_save(&mut generator);
    // Build 163 shares the selected body schedule, but wraps GPR survivors in a
    // larger linkage-first frame. Normalize only the verified allocator shape;
    // convention-aware owners already emitted their final frame and are skipped.
    generator.normalize_linkage_first_callee_saved_frame();
    generator.normalize_linkage_first_plain_nonleaf_frame();
    generator.normalize_linkage_first_indirect_call_schedule();
    generator.normalize_linkage_first_conversion_frame();
    generator.normalize_scratch_copy_convention();

    ordinal_accounting::apply(
        function,
        &mut generator.output,
        generator.behavior.function_ordinal_accounting_style,
    );

    // A function with a stack frame carries unwind tables. The codegen does not
    // yet save callee registers, so the saved counts are zero today; the FPU flag
    // is set for a non-leaf function that touches the FPU.
    // The `extab`/`extabindex` unwind tables are emitted only with C++ exceptions
    // on (the default); `-Cpp_exceptions off` suppresses them (the frame itself is
    // unchanged). `frame` drives those sections, so leave it `None` when off.
    if generator.frame_size != 0
        && config.flags.cpp_exceptions
        && (generator.non_leaf || generator.behavior.emit_leaf_frame_unwind)
    {
        // The extab FPU flag is keyed on *single-precision* float usage: a non-leaf
        // that uses a single-precision load/store/arith sets it, and so does any
        // leaf-with-frame that does single-precision arithmetic (an `int`->`float`
        // conversion's `fsubs`). Double-only work — `lfd`/`fadd`/`fctiwz`, or a bare
        // `fcmpo` against a double constant — leaves it clear (`if (d > 0.0)` carries
        // no flag, `if (f > 0.0f)` does). Counting *any* FP here over-set it for
        // double-only non-leaves such as a double comparison against a constant.
        let touches_fpu = generator
            .output
            .instructions
            .iter()
            .any(|instruction| instruction.is_single_precision_floating_point());
        let single_arithmetic = generator
            .output
            .instructions
            .iter()
            .any(|instruction| instruction.is_single_precision_arithmetic());
        generator.output.frame = Some(FrameInfo {
            saved_gpr_count: generator.callee_saved.len() as u8,
            saved_fpr_count: generator.callee_saved_float,
            uses_fpu: generator.behavior.mark_single_precision_extab
                && ((generator.non_leaf && touches_fpu) || single_arithmetic),
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
    // Apply selection's placement hints: registers a given virtual must avoid,
    // and the consumer-tree preference it should take when free (policy #1).
    for interval in &mut liveness.intervals {
        if let Some(avoid) = generator.register_avoid.get(&interval.vreg.id) {
            interval.avoid = avoid.clone();
        }
        if let Some(&prefer) = generator.register_prefer.get(&interval.vreg.id) {
            interval.prefer = Some(prefer);
        }
    }
    // PASS-ARC STEP 2: a whole-body fill that emitted its values as virtuals
    // selects the DESCENDING policy (the measured store-fill assignment);
    // everything else keeps lowest-free LinearScan.
    let allocation = match generator.descending_allocation_top {
        Some(top) => mwcc_vreg::Allocator::allocate(
            &mwcc_vreg::DescendingScan { top },
            &liveness.intervals,
            &liveness.pinned,
            &liveness.calls,
            &generator.constraints,
        ),
        None => mwcc_vreg::Allocator::allocate(
            &mwcc_vreg::LinearScan,
            &liveness.intervals,
            &liveness.pinned,
            &liveness.calls,
            &generator.constraints,
        ),
    }
    .map_err(|error| {
        mwcc_core::Diagnostic::error(format!("register allocation failed: {error:?}"))
    })?;
    mwcc_vreg::apply(&mut generator.output.instructions, &allocation);
    // FRAME-METADATA CONSISTENCY: every callee-saved register the allocation used
    // must correspond to a save slot the arm declared (generator.callee_saved, one
    // entry per prologue save). A mismatch would emit unwind metadata that disagrees
    // with the actual saves — defer instead of shipping a wrong extab.
    let used = allocation.assigned_callee_saved(&generator.constraints);
    if used.len() > generator.callee_saved.len() {
        return Err(mwcc_core::Diagnostic::error(format!(
            "allocation used {} callee-saved register(s) but the frame declares {} save slot(s) (frame builder needed)",
            used.len(),
            generator.callee_saved.len()
        )));
    }
    Ok(())
}

/// The instruction-scheduling pass (Phase E): reorder instructions within the
/// block to mwcc's pipeline schedule, then remap any relocation's instruction
/// index through the permutation so it still points at its instruction. With the
/// scheduler's identity policy this is a no-op; it becomes active as the policy
/// is tuned against the oracle.
fn schedule_instructions(generator: &mut Generator) {
    let permutation: Vec<usize> = if generator.output.pre_scheduled
        || !generator.behavior.schedule_latency_slots
    {
        (0..generator.output.instructions.len()).collect()
    } else {
        // The `lis -> addi` latency-slot fill runs first (mwcc issues a ready
        // `li` into the stall slot), then the list scheduler; the relocation
        // remap composes the two permutations (old -> filled -> scheduled).
        let slot_fill = mwcc_vreg::fill_address_latency_slots(&mut generator.output.instructions);
        let list = mwcc_vreg::schedule(&mut generator.output.instructions);
        slot_fill.into_iter().map(|filled| list[filled]).collect()
    };
    for relocation in &mut generator.output.relocations {
        relocation.instruction_index = permutation[relocation.instruction_index];
    }
}

/// Move the epilogue's saved-LR reload up to right after the last call, remapping
/// relocation indices through the resulting permutation.
fn hoist_link_register_reload(generator: &mut Generator) {
    if !generator.behavior.schedule_latency_slots {
        return;
    }
    // GC/1.1p1 deliberately restores the caller stack pointer before loading LR through
    // `4(r1)`. That load is address-dependent on the stack restore and therefore is not an
    // epilogue latency candidate (`li result; addi r1,...; lwz r0,4(r1)`). The generic hoist
    // only understands the reload-through-current-frame convention and would incorrectly move
    // this load ahead of both operations.
    if generator.behavior.frame_convention == mwcc_versions::FrameConvention::LinkageFirst
        && generator.behavior.plain_linkage_epilogue_style
            == mwcc_versions::PlainLinkageEpilogueStyle::StackRestoreBeforeReload
    {
        let stack_restore = generator.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::AddImmediate { d: 1, a: 1, immediate }
                if *immediate == generator.frame_size)
        });
        let restored_stack_link_load = generator.output.instructions.iter().position(
            |instruction| {
                matches!(instruction, Instruction::LoadWord { d: 0, a: 1, offset: 4 })
            },
        );
        if matches!((stack_restore, restored_stack_link_load), (Some(restore), Some(load)) if restore < load)
        {
            return;
        }
    }
    let permutation = mwcc_vreg::hoist_link_register_reload(&mut generator.output.instructions);
    for relocation in &mut generator.output.relocations {
        relocation.instruction_index = permutation[relocation.instruction_index];
    }
}

fn schedule_link_register_save(generator: &mut Generator) {
    if !generator.behavior.schedule_latency_slots {
        return;
    }
    let permutation = mwcc_vreg::schedule_link_register_save(&mut generator.output.instructions);
    for relocation in &mut generator.output.relocations {
        relocation.instruction_index = permutation[relocation.instruction_index];
    }
}

/// Coalesce allocator self-moves (`mr rX,rX`) on the physical stream, remapping relocation
/// indices through the resulting removal.
fn coalesce_self_moves(generator: &mut Generator) {
    let permutation = mwcc_vreg::coalesce_self_moves(&mut generator.output.instructions);
    for relocation in &mut generator.output.relocations {
        relocation.instruction_index = permutation[relocation.instruction_index];
    }
}

/// Rewrite any conditional forward branch whose target is the function's TERMINAL `blr`
/// into the equivalent `b<cc>lr` (branch-conditional-to-link-register), matching mwcc,
/// which never emits `b<cc> .Lend` when the destination is the final return. In place —
/// same instruction count, so no relocation/index remap. Restricted to the terminal blr
/// (the last instruction): its fall-through is always live, so the collapse leaves no dead
/// code, and a framed epilogue (whose branch target is the teardown, not a bare `blr`) is
/// never matched.
fn collapse_forward_branch_to_terminal_blr(instructions: &mut [Instruction]) {
    let Some(last) = instructions.len().checked_sub(1) else {
        return;
    };
    if !matches!(instructions[last], Instruction::BranchToLinkRegister) {
        return;
    }
    for index in 0..last {
        if let Instruction::BranchConditionalForward {
            options,
            condition_bit,
            target,
        } = instructions[index]
        {
            if target == last {
                instructions[index] = Instruction::BranchConditionalToLinkRegister {
                    options,
                    condition_bit,
                };
            }
        }
    }
}
