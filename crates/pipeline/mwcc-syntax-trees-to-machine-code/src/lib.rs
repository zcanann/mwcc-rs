//! Pipeline: syntax trees -> machine code.
//!
//! Instruction selection and register assignment for the supported C subset,
//! reproducing mwcceppc's output byte-for-byte. `lib.rs` only wires the theme
//! modules together and exposes the entry point; the work lives in them.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{FrameInfo, Instruction, MachineFunction, RelocationTarget};
use mwcc_syntax_trees::{Function, GlobalDeclaration, LocalDataRelocationTarget};
use mwcc_versions::{Behavior, CompilerConfig};
use std::collections::{HashMap, HashSet};

mod analysis;
mod allocation_frame;
mod arithmetic;
mod asm;
mod automatic_rodata;
mod body;
mod branch_cleanup;
mod captures;
mod casts;
mod comparisons;
mod conversion_frame;
mod condition_float_cache;
mod condition_global_cache;
mod control_flow;
mod copy_convention;
mod cxx_abi;
mod dag_emitter;
mod division;
mod expressions;
mod float;
mod float_abs_pair_condition;
mod float_abs_select;
mod float_compare_schedules;
mod float_computed_loaded_condition;
mod float_fused_triplet;
mod float_negated_add;
mod float_negated_product;
mod float_product_condition;
mod floats;
mod frame;
mod frexp_family;
mod generator;
mod inline_expansion;
mod inline_source_order;
mod inline_summaries;
mod legacy_comparisons;
mod legacy_dual_float_condition;
mod narrow;
mod operands;
mod ordinal_accounting;
mod placement;
mod runtime_conversions;
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
/// `call_return_fundamentals` retains source identities that the compact type IR
/// merges, so same-type forwarding remains distinct from a required conversion.
pub fn lower_function(
    function: &Function,
    globals: &[GlobalDeclaration],
    aggregate_definitions: &HashMap<String, mwcc_syntax_trees::AggregateDefinition>,
    function_return_aggregate_tags: &HashMap<String, String>,
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
    inline_expansion_facts: mwcc_syntax_trees::InlineExpansionFacts,
    source_inline_string_symbols: &HashMap<Vec<u8>, String>,
    call_return_fundamentals: &HashMap<String, mwcc_syntax_trees::SourceFundamentalType>,
    config: CompilerConfig,
) -> Compilation<MachineFunction> {
    let mut output = lower_function_body(
        function,
        globals,
        aggregate_definitions,
        function_return_aggregate_tags,
        call_return_types,
        call_parameter_types,
        skipped_inline_names,
        weak_materialized_names,
        prototyped_names,
        variadic_definitions,
        fixed_address_arrays,
        fixed_address_objects,
        inline_bodies,
        inline_summaries,
        inline_expansion_facts,
        source_inline_string_symbols,
        call_return_fundamentals,
        config,
    )?;
    automatic_rodata::retain_unused_array_images(
        function,
        &mut output,
        Behavior::resolve(&config),
    );
    Ok(output)
}

/// Select and schedule the executable body. Object-only products derived from
/// optimized-away source declarations are attached by [`lower_function`] after
/// this returns, so specialized early-return lowerers cannot bypass them.
#[allow(clippy::too_many_arguments)]
fn lower_function_body(
    function: &Function,
    globals: &[GlobalDeclaration],
    aggregate_definitions: &HashMap<String, mwcc_syntax_trees::AggregateDefinition>,
    function_return_aggregate_tags: &HashMap<String, String>,
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
    inline_expansion_facts: mwcc_syntax_trees::InlineExpansionFacts,
    source_inline_string_symbols: &HashMap<Vec<u8>, String>,
    call_return_fundamentals: &HashMap<String, mwcc_syntax_trees::SourceFundamentalType>,
    config: CompilerConfig,
) -> Compilation<MachineFunction> {
    if let Some(output) = body::lower_register_inline_asm_wrapper(
        function,
        &Behavior::resolve(&config),
        config.flags.cpp_exceptions,
    ) {
        return Ok(output);
    }
    if let Some(output) = body::lower_member_float_normalize(
        function,
        &Behavior::resolve(&config),
        config.flags.cpp_exceptions,
    ) {
        return Ok(output);
    }
    if let Some(output) = body::lower_member_linefeed(
        function,
        &Behavior::resolve(&config),
        config.flags.cpp_exceptions,
        config.build.version >= (4, 3, 0),
    ) {
        return Ok(output);
    }
    if let Some(output) = body::lower_member_tab(
        function,
        &Behavior::resolve(&config),
        config.flags.cpp_exceptions,
        config.build.version >= (4, 3, 0),
    ) {
        return Ok(output);
    }
    if let Some(output) = body::lower_member_rect_control(
        function,
        &Behavior::resolve(&config),
        config.flags.cpp_exceptions,
        config.build.version >= (4, 3, 0),
    ) {
        return Ok(output);
    }
    // An inline-`asm` function is emitted verbatim — no register allocation,
    // scheduling, or optimizer — so it bypasses the ordinary codegen path entirely.
    if function.asm_body.is_some() {
        return asm::assemble_asm_function(function, Behavior::resolve(&config));
    }
    if let Some(output) = cxx_abi::lower_aggregate_member_return(
        function,
        aggregate_definitions,
        function_return_aggregate_tags,
    ) {
        return Ok(output);
    }
    let expanded_constructor = function
        .name
        .starts_with("__ct__")
        .then(|| inline_bodies.expand_calls_with_facts(function))
        .flatten();
    if std::env::var_os("MWCC_DIAGNOSTIC_SYNTAX_TREE").is_some() {
        if let Some(expanded) = &expanded_constructor {
            eprintln!(
                "constructor-inline-expansion {}: front={}, statement={}, value={}",
                function.name,
                inline_expansion_facts.leading_initializer_substitutions,
                expanded.statement_body_substitutions,
                expanded.value_body_substitutions,
            );
        }
    }
    let constructor_inline_ordinal_residue =
        expanded_constructor.as_ref().map_or(0, |expanded| {
            let behavior = Behavior::resolve(&config);
            if let Some(weights) = behavior.cxx_constructor_inline_ordinal_weights {
                u32::from(weights.base)
                    + u32::from(weights.leading_initializer)
                        * inline_expansion_facts.leading_initializer_substitutions as u32
                    + u32::from(weights.statement_body)
                        * expanded.statement_body_substitutions as u32
                    + u32::from(weights.value_body)
                        * expanded.value_body_substitutions as u32
            } else {
                inline_expansion::ordinal_residue(
                    inline_expansion_facts,
                    expanded.statement_body_substitutions,
                    expanded.value_body_substitutions,
                    behavior.inline_statement_substitution_label_weight,
                )
            }
        });
    if let Some(mut output) = cxx_abi::lower_link_node_constructor(
        expanded_constructor
            .as_ref()
            .map(|expanded| &expanded.function)
            .unwrap_or(function),
        config.clone(),
    ) {
        output.anonymous_label_bump += constructor_inline_ordinal_residue;
        return Ok(output);
    }
    if let Some(mut output) = cxx_abi::lower_partial_constructor_chain(
        expanded_constructor
            .as_ref()
            .map(|expanded| &expanded.function)
            .unwrap_or(function),
        source_inline_string_symbols,
        config.clone(),
    ) {
        output.anonymous_label_bump += constructor_inline_ordinal_residue;
        return Ok(output);
    }
    if let Some(mut output) = cxx_abi::lower_inlined_constructor_chain(
        expanded_constructor
            .as_ref()
            .map(|expanded| &expanded.function)
            .unwrap_or(function),
        source_inline_string_symbols,
        config.clone(),
    ) {
        output.anonymous_label_bump += constructor_inline_ordinal_residue;
        return Ok(output);
    }
    if let Some(mut output) = cxx_abi::lower_composed_constructor(
        expanded_constructor
            .as_ref()
            .map(|expanded| &expanded.function)
            .unwrap_or(function),
        globals,
        config.clone(),
    ) {
        output.anonymous_label_bump += constructor_inline_ordinal_residue;
        return Ok(output);
    }
    if let Some(mut output) = cxx_abi::lower_virtual_constructor(
        expanded_constructor
            .as_ref()
            .map(|expanded| &expanded.function)
            .unwrap_or(function),
        globals,
        config.clone(),
    ) {
        output.anonymous_label_bump += constructor_inline_ordinal_residue;
        return Ok(output);
    }
    if let Some(output) = cxx_abi::lower_optional_destructor(function, config.clone()) {
        return Ok(output);
    }
    if let Some(output) = cxx_abi::lower_array_destructor(function, config.clone()) {
        return Ok(output);
    }
    if let Some(output) =
        cxx_abi::lower_array_member_destructor(function, inline_summaries, config.clone())
    {
        return Ok(classify_specialized_call_declarations(
            output,
            prototyped_names,
        ));
    }
    if let Some(output) =
        cxx_abi::lower_composed_destructor(function, inline_summaries, config.clone())
    {
        return Ok(classify_specialized_call_declarations(
            output,
            prototyped_names,
        ));
    }
    if let Some(output) = cxx_abi::lower_trivial_destructor(function, config.clone()) {
        return Ok(output);
    }
    if let Some(output) = cxx_abi::lower_virtual_destructor(function, globals, config.clone()) {
        return Ok(classify_specialized_call_declarations(
            output,
            prototyped_names,
        ));
    }
    // A defined CONST float/double global is DE-NAMED by mwcc: every read compiles
    // as the literal value, pooled anonymously (@N in .sdata2) with no named
    // reference — measured for both `static const double two54 = C` and
    // external-linkage `const float NMathF::pi`. Substitute before lowering (a
    // name shadowed by a parameter or local is left alone).
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
    let mut static_local_strings: Vec<Vec<u8>> = Vec::new();
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
        let relocations = local
            .data_relocations
            .iter()
            .map(|relocation| {
                let target = match &relocation.target {
                    LocalDataRelocationTarget::Symbol(target) => target.clone(),
                    LocalDataRelocationTarget::StringLiteral(bytes) => {
                        let index = static_local_strings
                            .iter()
                            .position(|existing| existing == bytes)
                            .unwrap_or_else(|| {
                                static_local_strings.push(bytes.clone());
                                static_local_strings.len() - 1
                            });
                        format!("@@str{index}")
                    }
                };
                (relocation.offset, target, relocation.addend)
            })
            .collect();
        static_local_data.push(mwcc_machine_code::StaticLocal {
            name: local.name.clone(),
            initial_bytes: bytes,
            size,
            alignment,
            is_const: local.is_const,
            relocations,
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
        addressable_globals: globals
            .iter()
            .map(|global| (global.name.clone(), global.declared_type))
            .chain(
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
        global_arrays: static_locals
            .iter()
            .filter(|local| local.array_length.is_some())
            .map(|local| local.name.clone())
            .chain(
                globals
                    .iter()
                    .filter(|global| {
                        global.array_length.is_some() || global.array_length_inferred
                    })
                    .map(|global| global.name.clone()),
            )
            .collect(),
        structured_global_index_cache: None,
        transient_global_index_base: None,
        full_bss_globals: globals
            .iter()
            .filter(|global| {
                global.is_data_definition()
                    && !global.is_const
                    && global.section.is_none()
                    && global.initializer.is_none()
                    && global.data_bytes.is_none()
                    && global.address_initializer.is_none()
                    && match (global.declared_type, global.array_length) {
                        (mwcc_syntax_trees::Type::Struct { size, .. }, Some(length)) => {
                            size.saturating_mul(u32::from(length)) > 8
                        }
                        (mwcc_syntax_trees::Type::Struct { size, .. }, None) => size > 8,
                        (other, Some(length)) => {
                            u32::from(other.width()).saturating_mul(u32::from(length)) / 8 > 8
                        }
                        (_, None) => false,
                    }
            })
            .map(|global| global.name.clone())
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
        return_source_fundamental: call_return_fundamentals.get(&function.name).copied(),
        call_return_fundamentals: call_return_fundamentals.clone(),
        constraints: mwcc_vreg::RegisterConstraints::gekko(),
        non_leaf: false,
        callee_saved_float: 0,
        virtual_cursors: generator::VirtualCursors::default(),
        register_avoid: HashMap::new(),
        register_prefer: HashMap::new(),
        stored_globals: HashMap::new(),
        condition_global_values: HashMap::new(),
        condition_float_cache: Default::default(),
        const_address_bases: HashMap::new(),
        emitted_variable_index_store: false,
        packed_shift_mask_min_operations: 3,
        prematerialized_float_constants: Vec::new(),
        preloaded_float_compare_literal: None,
        structured_float_handoff: None,
        retained_float_compare_value: None,
        frame_slots: HashMap::new(),
        structured_aggregate_call_copy_plan: None,
        written_slots: HashSet::new(),
        frame_feeding_local_pressure: None,
        callee_saved_conversion_bytes: 0,
        float_to_int_scratch_next: 0,
        float_to_int_scratch_end: 0,
        int_to_float_scratch_next: 0,
        int_to_float_scratch_end: 0,
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
        legacy_inline_expansion_frame_bytes: inline_expansion::legacy_frame_residue_bytes(
            function,
            inline_expansion_facts,
        ),
        inline_expansion_facts,
        epilogue_lr_first: false,
        epilogue_lr_before_gprs: false,
        owns_link_register_schedule: false,
        narrow_truncation_context: false,
        known_locals: std::collections::HashSet::new(),
        one_word_aggregate_locals: std::collections::HashSet::new(),
        canonical_boolean_locals: std::collections::HashSet::new(),
        loop_assertion_string_highs: Vec::new(),
        loop_assertion_string_highs_emitted: false,
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
        frame_row_pointees: function
            .locals
            .iter()
            .filter_map(|local| {
                local
                    .row_bytes
                    .and_then(|_| expressions::pointee_of_type(local.declared_type))
                    .map(|pointee| (local.name.clone(), pointee))
            })
            .collect(),
        descending_allocation_top: None,
        skipped_inline_names: skipped_inline_names.clone(),
        // Allocation operators and the standard block-copy intrinsic are
        // compiler-known runtime entry points even when the preprocessed source
        // does not retain an explicit declaration. Treat them as prototyped so
        // their undefined symbols are created in the declaration/reference run,
        // as real mwcc does.
        prototyped_names: prototyped_names
            .iter()
            .cloned()
            .chain([
                "__nw__FUl".to_owned(),
                "__nwa__FUl".to_owned(),
                "__nwa__FUli".to_owned(),
                "__nwa__FUlP7JKRHeapi".to_owned(),
                "__destroy_arr".to_owned(),
                "memcpy".to_owned(),
            ])
            .collect(),
        weak_materialized_names: weak_materialized_names.clone(),
        call_parameter_types: call_parameter_types.clone(),
        inline_bodies: inline_bodies.clone(),
        inline_string_symbols: source_inline_string_symbols.clone(),
        inline_summaries: inline_summaries.clone(),
    };
    // Static-local pointer tables are declared before executable statements,
    // so their literal targets lead the function's ordinary string pool.
    generator.output.string_literals = static_local_strings;
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
    if generator.behavior.schedule_latency_slots {
        branch_cleanup::collapse_forwarding_branch_blocks(&mut generator);
    }
    collapse_conditional_skip_to_backward_branch(&mut generator);
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
    // The names this function references, in mwcc's symbol-table discovery
    // order; the writer assigns its external/global symbols in this order.
    if generator.output.symbol_order.is_empty() {
        // GC 3/Wii create referenced symbols as their instruction relocations
        // are emitted, preserving order across data and function kinds. Keep
        // this separate from the older AST traversals: their grouping and
        // assignment visitation rules remain independently versioned.
        if generator.behavior.symbol_traversal_style
            == mwcc_versions::SymbolTraversalStyle::RelocationOrder
        {
            let mut seen = HashSet::new();
            generator.output.symbol_order = generator
                .output
                .relocations
                .iter()
                .filter_map(|relocation| match &relocation.target {
                    RelocationTarget::External(name)
                    | RelocationTarget::ExternalWithAddend(name, _) => Some(name.clone()),
                    _ => None,
                })
                .filter(|name| seen.insert(name.clone()))
                .collect();
        }
        // A capture template may pin its own measured order (atof, pikmin
        // s_ldexp) — only derive from the AST when it didn't.
        if generator.output.symbol_order.is_empty() {
            // A skipped inline's callees enter the symbol stream at the expanded
            // call site, not after every name visible in the caller's original
            // AST. Reconstruct the same expanded tree used by body lowering so
            // declaration-order symbols preserve that source position.
            let expanded_symbol_source = body::function_calls_any(
                function,
                &generator.skipped_inline_names,
            )
            .then(|| generator.inline_bodies.expand_calls(function))
            .flatten();
            generator.output.symbol_order = symbol_order::referenced_names(
                expanded_symbol_source.as_ref().unwrap_or(function),
                &generator.call_return_types,
                generator.behavior.symbol_traversal_style,
            );
        }
    }
    generator.output.referenced_function_symbols = generator
        .output
        .symbol_order
        .iter()
        .filter(|name| {
            generator.call_return_types.contains_key(name.as_str())
                || allocation_operator_returns_pointer(name)
        })
        .cloned()
        .collect();
    // A call target with no prototype/definition (absent from `call_return_types`) was
    // IMPLICITLY declared — K&R first-use. mwcc creates its symbol at the call site inside
    // the body, so the older writers emit it AFTER the function symbol (a prototyped
    // external, created at its file-scope declaration, precedes the function). GC 3/Wii's
    // relocation-order policy deliberately preserves one stream across both categories;
    // do not re-partition that stream by prototype status after deriving it above.
    if generator.behavior.symbol_traversal_style
        != mwcc_versions::SymbolTraversalStyle::RelocationOrder
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
                if !generator.prototyped_names.contains(name.as_str()) && seen.insert(name.clone()) {
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
    generator.schedule_leading_int_to_float_argument();
    schedule_instructions(&mut generator);
    let allocated_float_saves = allocate_registers(&mut generator).map_err(|mut diagnostic| {
        let context = format!("function '{}'", function.name);
        if !diagnostic.message.contains(&context) {
            diagnostic.message.push_str(&format!(" (in {context})"));
        }
        diagnostic
    })?;
    // Coalesce away `mr rX,rX` self-moves the allocator leaves when it colors a value's
    // virtual home to the register the value already holds (mwcc coalesces them).
    coalesce_self_moves(&mut generator);
    generator.schedule_allocated_structured_array_pool_parameter_copies();
    // Allocation can coalesce a just-published frame value and its immediate
    // reload to the same physical register even when their virtual lanes were
    // distinct during selection. Remove that newly visible reload only for
    // non-volatile source scalars; branch-entry and wider forwarding regions
    // remain guarded by the dedicated frame-value pass.
    let forwardable_frame_scalar_offsets = function
        .locals
        .iter()
        .filter(|local| !local.is_volatile)
        .filter_map(|local| generator.frame_slots.get(&local.name).map(|slot| slot.offset))
        .collect();
    generator.forward_adjacent_frame_scalar_values(&forwardable_frame_scalar_offsets);
    generator.schedule_structured_frame_publication_entry();
    // Revisit the narrow saved-result epilogue on the physical stream. A
    // source-level return branch can hide the move from the structured
    // pre-allocation pass until generic scheduling removes that branch.
    generator.schedule_saved_return_epilogue();
    // Issue the epilogue's saved-LR reload right after the last call (ahead of the
    // post-call computation), as mwcc does — a final pass on the physical stream.
    hoist_link_register_reload(&mut generator);
    schedule_shared_epilogue_link_reload(&mut generator);
    // Symmetrically, delay the prologue's saved-LR store past the first call's ready
    // argument materializations (mwcc fills the mflr->store latency gap).
    schedule_link_register_save(&mut generator);
    // Build 163 lays out GPR homes and retained entry lanes before reserving
    // its compact 8-byte FPR save lanes. Newer builds add their 16-byte Gekko
    // lanes directly to the predecrement frame.
    generator.normalize_linkage_first_callee_saved_frame(!allocated_float_saves.is_empty());
    generator.materialize_allocated_float_frame(
        &allocated_float_saves,
        config.build.version >= (4, 3, 0),
    )?;
    // Build 163 shares the selected body schedule, but wraps GPR survivors in a
    // larger linkage-first frame. Normalize only the verified allocator shape;
    // convention-aware owners already emitted their final frame and are skipped.
    generator.normalize_linkage_first_saved_register_order();
    generator.normalize_linkage_first_plain_nonleaf_frame();
    generator.normalize_linkage_first_indirect_call_schedule();
    generator.normalize_linkage_first_conversion_frame();
    generator.hoist_normalized_linkage_first_arg_moves();
    generator.normalize_scratch_copy_convention();
    generator.schedule_saved_base_call_argument();
    generator.schedule_linkage_first_function_address();
    generator.schedule_retained_eager_entry_argument();
    generator.schedule_retained_split_member_guard();
    generator.schedule_linkage_first_inline_zero();
    generator.schedule_call_condition_live_in_arguments();
    generator.schedule_guarded_report_store(function);
    // Whole-body owners, generic scheduling, and physical allocation converge
    // here. Apply final cross-owner schedules only when their complete measured
    // physical instruction region is present.
    generator.schedule_forwarded_member_initialization();
    generator.schedule_mixed_scalar_initialization();
    generator.schedule_pod_constructor_initialization();
    generator.schedule_saved_receiver_array_release_frame();
    generator.schedule_assertion_float_member_return();
    generator.schedule_guarded_member_classifier_chain();
    generator.schedule_shared_right_float_product_pair();
    generator.schedule_shared_float_store_literal(function);
    generator.schedule_frame_vector_accumulation();
    generator.schedule_shared_global_float_pairs();
    generator.schedule_guarded_bitfield_storage_cache();
    generator.schedule_inlined_sign_store();
    generator.schedule_inlined_acceleration_select();
    generator.schedule_inlined_symmetric_float_clamp();
    generator.schedule_structured_float_or_groups();
    generator.schedule_symmetric_sum_clamp();
    generator.schedule_bounded_acceleration();
    generator.schedule_joystick_count_updates();
    generator.schedule_grab_mash_transaction();
    generator.schedule_mixed_member_zero_reset();
    generator.schedule_variadic_report_member_arguments();
    generator.schedule_saved_character_formatter_arguments();
    generator.schedule_position_formatter_arguments();
    generator.schedule_temporary_buffer_format_copy();
    generator.schedule_guarded_formatter_member_cache();
    generator.schedule_global_struct_binary_search();
    generator.schedule_frame_row_string_append();
    generator.schedule_ground_knockback_projection();
    generator.schedule_guarded_member_alias_initialization();
    generator.schedule_entry_saved_zero_test();
    generator.schedule_saved_pointer_zero_test();
    generator.schedule_reciprocal_frame_fill();
    generator.reuse_absolute_pooled_float_literals();
    generator.finalize_structured_noncopy_conversion_lanes();
    generator.finalize_structured_guarded_ucode_packet_registers();
    generator.finalize_structured_noncopy_packet_registers();
    generator.finalize_structured_noncopy_tail_packet_registers();
    generator.schedule_structured_frame_packet_call();
    generator.reuse_structured_loop_packet_setup();
    generator.schedule_structured_frame_preloop_packets();
    generator.schedule_structured_frame_sign_clamp_load();

    ordinal_accounting::apply(
        function,
        &mut generator.output,
        generator.behavior.function_ordinal_accounting_style,
    );

    // Debug lowering consumes final physical allocation, not the frontend's
    // provisional variable table. Frame slots are authoritative for
    // address-taken/aggregate locals; remaining allocated names retain their
    // general/FPR home when it is a physical target register.
    generator.output.debug_variables = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .chain(function.locals.iter().map(|local| local.name.as_str()))
        .filter_map(|name| {
            let location = if let Some(slot) = generator.frame_slots.get(name) {
                mwcc_machine_code::DebugVariableLocation::FrameOffset(slot.offset)
            } else {
                let location = generator.locations.get(name)?;
                if location.register > 31 {
                    return None;
                }
                match location.class {
                    generator::ValueClass::General => {
                        mwcc_machine_code::DebugVariableLocation::GeneralRegister(
                            location.register,
                        )
                    }
                    generator::ValueClass::Float => {
                        mwcc_machine_code::DebugVariableLocation::FloatRegister(location.register)
                    }
                }
            };
            Some(mwcc_machine_code::DebugVariable {
                name: name.to_owned(),
                location,
            })
        })
        .collect();

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

/// Specialized ABI lowerers bypass the ordinary `Generator` finalization pass.
/// Reconcile their call-site metadata with declarations recovered by the
/// frontend so the object writer does not treat a declared member destructor
/// or class-specific delete as an implicit K&R-era call.
fn classify_specialized_call_declarations(
    mut output: MachineFunction,
    prototyped_names: &HashSet<String>,
) -> MachineFunction {
    output
        .implicit_external_callees
        .retain(|name| !prototyped_names.contains(name));
    output
}

pub(crate) fn allocation_operator_returns_pointer(name: &str) -> bool {
    name.starts_with("__nw__F") || name.starts_with("__nwa__F")
}

/// The register-allocation pass: resolve any virtual registers the selection
/// emitted to physical homes, honoring liveness and the target constraints.
///
/// Selection currently emits mostly physical registers inline; for those this is
/// a no-op (no virtual fields, nothing to assign). As selection is migrated to
/// emit virtuals, this pass becomes where their physical registers are decided —
/// each migration step verified byte-exact against the oracle. Running it
/// unconditionally keeps one pipeline (no fork between a legacy and a vreg path).
fn allocate_registers(generator: &mut Generator) -> Compilation<Vec<u8>> {
    let mut liveness = mwcc_vreg::analyze(&generator.output.instructions);
    if liveness.intervals.is_empty() {
        return Ok(Vec::new()); // no virtuals — selection chose physical registers directly
    }
    // Apply selection's placement hints: registers a given virtual must avoid,
    // and the consumer-tree preference it should take when free (policy #1).
    for interval in &mut liveness.intervals {
        if let Some(avoid) = generator.register_avoid.get(&interval.vreg) {
            interval.avoid = avoid.clone();
        }
        if let Some(&prefer) = generator.register_prefer.get(&interval.vreg) {
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
    let used_float = allocation.assigned_float_callee_saved(&generator.constraints);
    let used = allocation.assigned_callee_saved(&generator.constraints);
    generator.reconcile_allocated_general_frame(&allocation, &used)?;
    mwcc_vreg::apply(&mut generator.output.instructions, &allocation);
    // FRAME-METADATA CONSISTENCY: every callee-saved register the allocation used
    // must correspond to a save slot the arm declared (generator.callee_saved, one
    // entry per prologue save). A mismatch would emit unwind metadata that disagrees
    // with the actual saves — defer instead of shipping a wrong extab.
    if used.len() > generator.callee_saved.len() {
        return Err(mwcc_core::Diagnostic::error(format!(
            "allocation used {} callee-saved register(s) but the frame declares {} save slot(s) (frame builder needed)",
            used.len(),
            generator.callee_saved.len()
        )));
    }
    Ok(used_float)
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
        // Call arguments are arranged while their values still have distinct
        // virtual identities. Address latency filling and list scheduling then
        // operate on that stream; relocation remaps compose in the same order.
        let call_arguments =
            mwcc_vreg::hoist_simple_later_call_argument(&mut generator.output.instructions);
        let slot_fill = mwcc_vreg::fill_address_latency_slots(&mut generator.output.instructions);
        let list = mwcc_vreg::schedule(&mut generator.output.instructions);
        call_arguments
            .into_iter()
            .map(|argument| list[slot_fill[argument]])
            .collect()
    };
    remap_instruction_indices(generator, &permutation);
}

/// Move the epilogue's saved-LR reload up to right after the last call, remapping
/// relocation indices through the resulting permutation.
fn hoist_link_register_reload(generator: &mut Generator) {
    if generator.owns_link_register_schedule || !generator.behavior.schedule_latency_slots {
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
    remap_instruction_indices(generator, &permutation);
}

/// At a shared framed epilogue, MWCC issues the saved-LR load before an
/// independent return-value operation. Incoming branches must land on the
/// moved LR load, while relocations remain attached to the return operation.
/// Those are different mappings, so this join-aware two-instruction schedule
/// lives above the generic permutation helper.
fn schedule_shared_epilogue_link_reload(generator: &mut Generator) {
    if !generator.behavior.schedule_latency_slots {
        return;
    }
    let Some(mtlr) = generator
        .output
        .instructions
        .iter()
        .position(|instruction| matches!(instruction, Instruction::MoveToLinkRegister { s: 0 }))
    else {
        return;
    };
    if mtlr < 2 {
        return;
    }
    let reload = mtlr - 1;
    let result_operation = reload - 1;
    if !matches!(
        generator.output.instructions[reload],
        Instruction::LoadWord { d: 0, a: 1, .. }
    ) {
        return;
    }

    // A shared continuation that returns a static/global address has a two-op
    // `lis`/`addi` result chain. MWCC starts that block with the LR reload, then
    // overlaps the load latency with both address instructions. Incoming case
    // branches must follow the reload to the moved address-high instruction.
    if reload >= 2 {
        let address_high = reload - 2;
        let address_low = reload - 1;
        let address_register = match generator.output.instructions[address_high] {
            Instruction::AddImmediateShifted { d, a: 0, .. } if d != 0 => Some(d),
            _ => None,
        };
        let address_pair = address_register.is_some_and(|register| {
            matches!(
                generator.output.instructions[address_low],
                Instruction::AddImmediate { d, a, .. } if d == register && a == register
            ) && generator.output.relocations.iter().any(|relocation| {
                relocation.instruction_index == address_high
                    && relocation.kind == mwcc_machine_code::RelocationKind::Addr16Ha
            }) && generator.output.relocations.iter().any(|relocation| {
                relocation.instruction_index == address_low
                    && relocation.kind == mwcc_machine_code::RelocationKind::Addr16Lo
            })
        });
        let incoming: Vec<usize> = generator
            .output
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(index, instruction)| match instruction {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                    if *target == address_high =>
                {
                    Some(index)
                }
                _ => None,
            })
            .collect();
        if address_pair && !incoming.is_empty() {
            generator.output.instructions[address_high..=reload].rotate_right(1);
            let mut permutation: Vec<usize> =
                (0..generator.output.instructions.len()).collect();
            permutation[address_high] = address_high + 1;
            permutation[address_low] = address_low + 1;
            permutation[reload] = address_high;
            remap_instruction_indices(generator, &permutation);
            for old_branch in incoming {
                let branch = permutation[old_branch];
                match &mut generator.output.instructions[branch] {
                    Instruction::Branch { target }
                    | Instruction::BranchConditionalForward { target, .. } => {
                        *target = address_high;
                    }
                    _ => unreachable!("recorded branch changed kind during address rotation"),
                }
            }
            return;
        }
    }

    let relocated_small_data = generator.output.relocations.iter().any(|relocation| {
        relocation.instruction_index == result_operation
            && relocation.kind == mwcc_machine_code::RelocationKind::EmbSda21
    });
    let independent_result = match generator.output.instructions[result_operation] {
        Instruction::LoadWord { d, a, .. }
        | Instruction::LoadByteZero { d, a, .. }
        | Instruction::LoadHalfwordZero { d, a, .. }
        | Instruction::LoadHalfwordAlgebraic { d, a, .. } => {
            d != 0 && (a != 0 || relocated_small_data)
        }
        Instruction::LoadFloatSingle { d: _, a, .. }
        | Instruction::LoadFloatDouble { d: _, a, .. } => a != 0 || relocated_small_data,
        Instruction::AddImmediate { d, a, .. } => d != 0 && d == a,
        _ => false,
    };
    if !independent_result {
        return;
    }
    let incoming: Vec<usize> = generator
        .output
        .instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| match instruction {
            Instruction::Branch { target }
            | Instruction::BranchConditionalForward { target, .. }
                if *target == result_operation =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect();
    if incoming.is_empty() {
        return;
    }

    generator
        .output
        .instructions
        .swap(result_operation, reload);
    let mut permutation: Vec<usize> = (0..generator.output.instructions.len()).collect();
    permutation[result_operation] = reload;
    permutation[reload] = result_operation;
    remap_instruction_indices(generator, &permutation);
    for old_branch in incoming {
        let branch = permutation[old_branch];
        match &mut generator.output.instructions[branch] {
            Instruction::Branch { target }
            | Instruction::BranchConditionalForward { target, .. } => {
                *target = result_operation;
            }
            _ => unreachable!("recorded branch changed kind during adjacent swap"),
        }
    }
}

fn schedule_link_register_save(generator: &mut Generator) {
    if generator.owns_link_register_schedule || !generator.behavior.schedule_latency_slots {
        return;
    }
    let permutation = mwcc_vreg::schedule_link_register_save(&mut generator.output.instructions);
    remap_instruction_indices(generator, &permutation);
}

/// Coalesce allocator self-moves on the physical stream, remapping every
/// instruction-index owner through the resulting removal.
fn coalesce_self_moves(generator: &mut Generator) {
    let relocation_owners: Vec<usize> = generator
        .output
        .relocations
        .iter()
        .map(|relocation| relocation.instruction_index)
        .collect();
    let permutation = mwcc_vreg::coalesce_self_moves_preserving(
        &mut generator.output.instructions,
        &relocation_owners,
    );
    remap_instruction_indices(generator, &permutation);
}

/// Remap relocations and internal branch destinations after an instruction
/// permutation. Branch destinations are instruction indices just like
/// relocation owners; leaving them stale after deleting a self-move can skip
/// the first instruction of a guarded continuation.
pub(crate) fn remap_instruction_indices(generator: &mut Generator, permutation: &[usize]) {
    for relocation in &mut generator.output.relocations {
        relocation.instruction_index = permutation[relocation.instruction_index];
    }
    remap_branch_targets(&mut generator.output.instructions, permutation);
}

/// Remove one instruction after labels have been resolved, preserving every
/// instruction-index owner. A branch to the erased instruction denotes the
/// continuation that followed it, so it must target the survivor now occupying
/// the same slot rather than the preceding instruction.
pub(crate) fn remove_instruction_retargeting_to_next(
    generator: &mut Generator,
    index: usize,
) {
    let old_len = generator.output.instructions.len();
    debug_assert!(index < old_len);
    generator.output.instructions.remove(index);
    generator
        .output
        .relocations
        .retain(|relocation| relocation.instruction_index != index);
    generator.labels.removed_retargeting_to_next(index, 1);
    let permutation = instruction_removal_permutation(old_len, index);
    remap_instruction_indices(generator, &permutation);
}

fn instruction_removal_permutation(old_len: usize, index: usize) -> Vec<usize> {
    debug_assert!(index < old_len);
    (0..old_len)
        .map(|old| if old <= index { old } else { old - 1 })
        .collect()
}

fn remap_branch_targets(instructions: &mut [Instruction], permutation: &[usize]) {
    let old_end = permutation.len();
    let new_end = instructions.len();
    for instruction in instructions {
        let target = match instruction {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target } => target,
            _ => continue,
        };
        *target = if *target == old_end {
            new_end
        } else {
            permutation[*target]
        };
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

/// `b<cc> next; b loop; next:` is the unoptimized spelling of an inverted
/// conditional backedge. MWCC emits the single `b<!cc> loop`; collapse only
/// the exact one-instruction skip shape and remap all instruction-index owners.
fn collapse_conditional_skip_to_backward_branch(generator: &mut Generator) {
    while let Some(permutation) =
        collapse_conditional_skip_to_backward_branch_once(&mut generator.output.instructions)
    {
        remap_instruction_indices(generator, &permutation);
    }
}

fn collapse_conditional_skip_to_backward_branch_once(
    instructions: &mut Vec<Instruction>,
) -> Option<Vec<usize>> {
        let index = (0..instructions.len().saturating_sub(1)).find(
            |&index| {
                matches!(
                    (
                        &instructions[index],
                        &instructions[index + 1],
                    ),
                    (
                        Instruction::BranchConditionalForward { target, .. },
                        Instruction::Branch { target: backward },
                    ) if *target == index + 2
                        && *backward < index
                        && !instructions.iter().enumerate().any(
                            |(owner, instruction)| owner != index
                                && matches!(
                                    instruction,
                                    Instruction::BranchConditionalForward { target, .. }
                                        | Instruction::Branch { target }
                                        if *target == index + 1
                                )
                        )
                )
            },
        )?;
        let Instruction::BranchConditionalForward {
            options,
            condition_bit,
            ..
        } = instructions[index]
        else {
            unreachable!()
        };
        let Instruction::Branch { target } = instructions[index + 1] else {
            unreachable!()
        };
        instructions[index] = Instruction::BranchConditionalForward {
            options: options ^ 8,
            condition_bit,
            target,
        };
        instructions.remove(index + 1);
        let old_len = instructions.len() + 1;
        let permutation: Vec<usize> = (0..old_len)
            .map(|old| {
                if old <= index {
                    old
                } else if old == index + 1 {
                    index
                } else {
                    old - 1
                }
            })
            .collect();
        Some(permutation)
}

#[cfg(test)]
mod instruction_index_tests {
    use super::*;

    #[test]
    fn a_branch_to_a_removed_instruction_retargets_to_the_next_survivor() {
        let mut instructions = vec![
            Instruction::Branch {
                target: 2,
            },
            Instruction::load_immediate(3, 1),
            Instruction::load_immediate(4, 2),
            Instruction::load_immediate(5, 3),
        ];
        instructions.remove(2);
        let permutation = instruction_removal_permutation(4, 2);
        remap_branch_targets(&mut instructions, &permutation);

        assert_eq!(instructions[0], Instruction::Branch { target: 2 });
        assert_eq!(instructions[2], Instruction::load_immediate(5, 3));
    }

    #[test]
    fn a_removed_self_move_remaps_the_guarded_continuation() {
        let mut instructions = vec![
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: 3,
            },
            Instruction::FloatMove { d: 1, b: 1 },
            Instruction::BranchAndLink {
                target: "guarded".into(),
            },
            Instruction::move_register(3, 31),
            Instruction::BranchAndLink {
                target: "continuation".into(),
            },
        ];
        let permutation = mwcc_vreg::coalesce_self_moves(&mut instructions);
        remap_branch_targets(&mut instructions, &permutation);
        let Instruction::BranchConditionalForward { target, .. } = instructions[0] else {
            panic!("expected guarded branch");
        };

        assert_eq!(target, 2);
        assert!(matches!(instructions[target], Instruction::Or { a: 3, .. }));
    }

    #[test]
    fn a_conditional_skip_and_backedge_collapse_to_one_inverted_branch() {
        let mut instructions = vec![
            Instruction::load_immediate(3, 1),
            Instruction::CompareWordImmediate {
                a: 3,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 4,
            },
            Instruction::Branch { target: 1 },
            Instruction::BranchToLinkRegister,
        ];

        let permutation = collapse_conditional_skip_to_backward_branch_once(&mut instructions)
            .expect("skip around a backward branch should collapse");
        remap_branch_targets(&mut instructions, &permutation);

        assert_eq!(instructions.len(), 4);
        assert!(matches!(
            instructions[2],
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 1,
            }
        ));
    }

    #[test]
    fn specialized_abi_calls_honor_recovered_declarations() {
        let mut function = MachineFunction::new("__dt__5OwnerFv");
        function.implicit_external_callees = vec![
            "__dt__4ItemFv".into(),
            "undeclared_helper".into(),
        ];
        let declared = HashSet::from(["__dt__4ItemFv".to_string()]);

        let function = classify_specialized_call_declarations(function, &declared);

        assert_eq!(function.implicit_external_callees, ["undeclared_helper"]);
    }
}
