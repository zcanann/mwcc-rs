//! Pipeline: syntax trees -> machine code.
//!
//! Instruction selection and register assignment for the supported C subset,
//! reproducing mwcceppc's output byte-for-byte. `lib.rs` only wires the theme
//! modules together and exposes the entry point; the work lives in them.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{FrameInfo, Instruction, MachineFunction, RelocationTarget};
use mwcc_syntax_trees::{Function, GlobalDeclaration, LocalDataRelocationTarget};
use mwcc_versions::{Behavior, CompilerConfig, FrameConvention};
use std::collections::{HashMap, HashSet};

mod analysis;
mod allocation_debug;
mod allocation_diagnostics;
mod allocation_frame;
mod arithmetic;
mod asm;
mod automatic_rodata;
mod body;
mod branch_value_reuse;
mod branch_cleanup;
mod captures;
mod casts;
mod comparisons;
#[cfg(test)]
mod comparisons_tests;
mod conversion_frame;
mod condition_float_cache;
mod condition_global_cache;
mod condition_member_cache;
mod conversion_scratch_scope;
mod control_flow;
mod copy_convention;
mod copy_sign_frame;
mod cxx_abi;
mod cxx_temporary_arguments;
mod dag_emitter;
mod debug_provenance;
mod division;
mod expressions;
mod float;
mod float_abs_pair_condition;
mod float_abs_select;
mod float_compare_schedules;
mod float_call_result_promotion;
mod float_saved_leaf_call;
mod float_computed_loaded_condition;
mod float_damping_product;
mod float_fused_triplet;
mod float_integer_affine;
mod float_integer_fraction;
mod float_memory_conditional;
mod float_materialized_condition;
mod float_materialized_projection;
mod float_negated_add;
mod float_negated_product;
mod float_product_condition;
mod float_scaled_integer_product;
mod floats;
mod frame;
mod frexp_family;
mod generator;
mod global_memory_schedule;
mod inline_expansion;
mod inline_sqrtf;
mod inline_source_order;
mod inline_summaries;
mod intrinsics;
mod legacy_comparisons;
mod legacy_dual_float_condition;
mod narrow;
mod operands;
mod ordinal_accounting;
mod placement;
mod runtime_conversions;
mod shared_global_store_base;
mod shared_global_array_store_base;
mod switch;
mod symbol_order;
mod value_tracking;
mod vec3_product_temporaries;
mod wide_local_scalarization;

use generator::Generator;
pub use inline_expansion::{InlineBodySet, InlineNestingBudget};
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
    if std::env::var_os("MWCC_DIAGNOSTIC_ANONYMOUS_ORDINALS").is_some() {
        eprintln!(
            "anonymous-ordinals {}: front={} fragment={} constants={} gaps={:?} adjust={} strings={} frame={} rodata={:?} jumps={:?} post={} rollback={}",
            function.name,
            output.object_anonymous_bump(),
            output.fragmented_debug_anonymous_bump,
            output.constants.len(),
            output.constant_number_gaps,
            output.constant_number_adjust,
            output.string_literals.len(),
            output.frame.is_some(),
            output
                .anonymous_rodata
                .iter()
                .map(|blob| (
                    blob.bytes.len(),
                    blob.static_slot_prefix_bump,
                    blob.anonymous_offset,
                ))
                .collect::<Vec<_>>(),
            output
                .jump_tables
                .iter()
                .map(|table| table.anonymous_offset)
                .collect::<Vec<_>>(),
            output.post_constant_label_bump,
            output.post_function_counter_rollback,
        );
    }
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
    let materialized_temporaries = cxx_temporary_arguments::materialize(
        function,
        call_return_types,
        call_parameter_types,
    );
    let function = materialized_temporaries.as_ref().unwrap_or(function);
    let materialized_vec3_products = vec3_product_temporaries::materialize(function);
    let function = materialized_vec3_products.as_ref().unwrap_or(function);
    // A `static` local has STATIC storage — an anonymous `<name>$N` object in `.sdata`/`.sbss`,
    // codegen'd like a file-scope global, not a frame slot. That path (the `$N = @N-1` numbering, the
    // per-function symbol, global-style access) is not built yet, so defer rather than mis-treat it as
    // an automatic local (`register`/`auto` hints, in contrast, are ordinary automatics and proceed).
    // STATIC locals have static storage: they compile as GLOBAL references
    // (`name$K` LOCAL objects — the writer numbers them off the function's
    // @N sequence). Register each in the operand maps and record its datum;
    // the automatic-local machinery never sees it.
    let ordinal_source_function = function;
    let static_locals: Vec<mwcc_syntax_trees::LocalDeclaration> = function
        .locals
        .iter()
        .filter(|local| local.is_static)
        .cloned()
        .collect();
    let mut static_local_data: Vec<mwcc_machine_code::StaticLocal> = Vec::new();
    let mut static_local_strings: Vec<Vec<u8>> = Vec::new();
    let mut static_aggregate_strings: Vec<Vec<u8>> = Vec::new();
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
                        let source_positioned_aggregate = config.flags.string_literals_packed
                            && local.array_length.is_none()
                            && matches!(local.declared_type, mwcc_syntax_trees::Type::Struct { .. });
                        let strings = if source_positioned_aggregate {
                            &mut static_aggregate_strings
                        } else {
                            &mut static_local_strings
                        };
                        let index = strings
                            .iter()
                            .position(|existing| existing == bytes)
                            .unwrap_or_else(|| {
                                strings.push(bytes.clone());
                                strings.len() - 1
                            });
                        if source_positioned_aggregate {
                            format!("@@staticstr{index}")
                        } else {
                            format!("@@str{index}")
                        }
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
    let behavior = Behavior::resolve(&config);
    let initial_inline_expansion_frame_bytes = inline_expansion::legacy_frame_residue_bytes(
        function,
        inline_expansion_facts,
    );
    let mut generator = Generator {
        variadic_definition,
        variadic_callees: variadic_definitions.clone(),
        output: MachineFunction::new(function.name.clone()),
        compiler_generated_symbols: Vec::new(),
        labels: mwcc_vreg::Labels::default(),
        locations: HashMap::new(),
        parameter_names: function
            .parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .collect(),
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
        structured_global_base_cache: None,
        structured_global_member_address_caches: Vec::new(),
        data_section_anchor: body::plan_linkage_first_data_anchor(
            function,
            globals,
            behavior,
            inline_bodies,
        ),
        data_section_anchor_reuses_deferred_home: false,
        structured_array_pool_emitted: false,
        structured_recovered_parameter_copies: false,
        structured_object_collision_loop_entry: false,
        structured_sequenced_callback_wait_starter: None,
        structured_switch_dispatch_conditionals: HashSet::new(),
        structured_cfg_cleanup_owner: false,
        structured_loop_exit_parameter_home_reuse: false,
        preserve_terminal_return_branches: false,
        structured_repeated_call_poll_owner: false,
        structured_nonreturning: false,
        structured_global_byte_loop_layout_owner: false,
        structured_dense_counted_loop_entry_owner: false,
        structured_member_array_offset_owner: false,
        passive_frame_scalar_mirrors: HashSet::new(),
        structured_broad_global_base_layout_owner: false,
        structured_shared_switch_global_value: None,
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
        behavior,
        return_source_fundamental: call_return_fundamentals.get(&function.name).copied(),
        call_return_fundamentals: call_return_fundamentals.clone(),
        constraints: mwcc_vreg::RegisterConstraints::gekko(),
        non_leaf: false,
        artificial_structured_leaf_frame: false,
        structured_pointer_table_index_cursor: false,
        structured_prescaled_pointer_table_index: false,
        preceded_by_asm: function.preceded_by_asm,
        callee_saved_float: 0,
        unoptimized_inline_float_loop_homes: false,
        unoptimized_inline_float_transaction_homes: false,
        virtual_cursors: generator::VirtualCursors::default(),
        register_avoid: HashMap::new(),
        register_prefer: HashMap::new(),
        stored_globals: HashMap::new(),
        condition_global_values: HashMap::new(),
        condition_float_cache: Default::default(),
        condition_member_cache: Default::default(),
        wide_pair_mask_cache: Default::default(),
        const_address_bases: HashMap::new(),
        emitted_leaf_variable_index_store_since_scratch_barrier: false,
        packed_shift_mask_min_operations: 3,
        prematerialized_float_constants: Vec::new(),
        preloaded_float_compare_literals: Vec::new(),
        released_float_compare_literal_register: None,
        structured_float_handoff: None,
        retained_float_compare_value: None,
        transient_condition_float_call_results: Default::default(),
        structured_guarded_bitfield_value: None,
        frame_slots: HashMap::new(),
        structured_compact_narrow_scalar_frame: false,
        structured_guarded_scalar_output_frame: false,
        structured_shared_switch_scalar_frame: false,
        structured_packed_switch_scalar_frame: false,
        structured_memory_transfer_frame: false,
        structured_memory_write_frame: false,
        structured_aggregate_call_copy_plan: None,
        structured_by_value_aggregate_plan: None,
        written_slots: HashSet::new(),
        frame_feeding_local_pressure: None,
        callee_saved_conversion_bytes: 0,
        float_to_int_scratch_next: 0,
        float_to_int_scratch_end: 0,
        int_to_float_scratch_next: 0,
        int_to_float_scratch_end: 0,
        shared_numeric_conversion_scratch: None,
        preserve_guarded_named_local_values: false,
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
        legacy_inline_expansion_frame_bytes: initial_inline_expansion_frame_bytes,
        initial_inline_expansion_frame_bytes,
        linkage_first_inline_aggregate_frame: false,
        inline_statement_body_substitutions: 0,
        late_inline_statement_body_substitutions: 0,
        inline_source_call_survivors: HashSet::new(),
        unoptimized_frame_call_home_names: HashSet::new(),
        inline_global_transaction_result_homes: Vec::new(),
        forced_general_callee_saved: HashSet::new(),
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
        materialized_float_window: None,
        materialized_float_assignment_active: false,
        structured_unoptimized_leaf_source_homes: false,
        structured_branch_float_work_home: None,
        structured_constant_address_home: None,
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
    generator.output.static_local_string_literals = static_aggregate_strings;
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
        generator.structured_cfg_cleanup_owner |=
            body::owns_unreferenced_forwarding_branch_cleanup(&generator.output.instructions);
        if generator.structured_cfg_cleanup_owner
            || generator.structured_array_pool_emitted
        {
            branch_cleanup::thread_conditional_branch_targets(
                &mut generator.output.instructions,
            );
        }
        let preserve_three_branch_entry_chains = function.preceded_by_asm
            && generator.behavior.frame_convention == FrameConvention::LinkageFirst;
        branch_cleanup::collapse_forwarding_branch_blocks(
            &mut generator,
            preserve_three_branch_entry_chains,
        );
        if generator.structured_cfg_cleanup_owner {
            branch_cleanup::remove_fallthrough_branches(
                &mut generator,
                preserve_three_branch_entry_chains,
            );
        }
    }
    collapse_conditional_skip_to_backward_branch(&mut generator);
    // Most leaf guards collapse a conditional edge to the terminal `blr` into
    // `b<cc>lr`. A retained source switch can instead own that terminal return
    // as a shared label, in which case MWCC preserves every incoming edge.
    if !generator.preserve_terminal_return_branches {
        collapse_forward_branch_to_terminal_blr(&mut generator.output.instructions);
    }
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
            symbol_order::interleave_compiler_generated_calls(
                &mut generator.output.symbol_order,
                &generator.compiler_generated_symbols,
                &generator.output.relocations,
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
    if function.name.contains("@unnamed@") && !generator.output.static_locals.is_empty() {
        // C++ unnamed-namespace bodies create their static-local object symbol
        // at the declaration before closing the owner's function-symbol block.
        generator.output.static_locals_lead = true;
    }
    // Schedule on the virtual-register stream, then allocate. Ordering matters:
    // scheduling first means physical-register reuse cannot create false
    // dependencies that block a hoist, and allocation then colors the scheduled
    // order — reproducing mwcc's interleaving of the two phases.
    generator.separate_structured_array_pool_initial_table_address();
    generator.schedule_structured_array_pool_following_format_call();
    generator.schedule_structured_array_pool_zero_terminated_format_call();
    generator.prefer_structured_array_pool_parsed_hour();
    generator.schedule_leading_int_to_float_argument();
    generator.hoist_structured_loop_float_zero();
    schedule_instructions(&mut generator);
    generator.schedule_dense_counted_loop_entry();
    generator.schedule_dense_counted_loop_state();
    generator.schedule_dense_counted_loop_tail();
    generator.schedule_dense_counted_loop_epilogue();
    generator.schedule_materialized_fixed_bank_store();
    generator.fuse_adjacent_materialized_fixed_bank_stores();
    generator.fuse_linkage_first_fixed_bank_region();
    generator.split_linkage_first_fixed_bank_self_copies();
    generator.schedule_linkage_first_callback_state_arms();
    generator.schedule_linkage_first_callback_completion_arms();
    generator.retain_inlined_leading_store_guard_constant();
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
    coalesce_float_conversion_moves(&mut generator);
    if generator.structured_member_array_offset_owner {
        generator.schedule_member_array_offset_loop();
    }
    generator.schedule_periodic_float_normalization(function);
    generator.schedule_inlined_global_transaction_volatile_reuse();
    generator.share_leaf_constant_guard_epilogue();
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
    generator.schedule_saved_return_epilogue(function);
    generator.schedule_post_call_zero_global_publication();
    generator.schedule_scaled_global_allocation_publications();
    generator.strip_artificial_leaf_linkage()?;
    generator.schedule_unoptimized_inline_float_transaction_handoffs();
    generator.schedule_unoptimized_inline_float_loop_handoffs();
    // Issue the epilogue's saved-LR reload at the generation-specific point in
    // the physical post-call stream. Mainline follows a call-result chain that
    // consumes a preserved FPR; older 2.3.3 distributions issue LR first.
    hoist_link_register_reload(&mut generator, &allocated_float_saves);
    schedule_shared_epilogue_link_reload(&mut generator);
    // Symmetrically, delay the prologue's saved-LR store past the first call's ready
    // argument materializations (mwcc fills the mflr->store latency gap).
    schedule_link_register_save(&mut generator);
    generator.schedule_guarded_return_address_frame();
    generator.schedule_addressable_return_frame();
    generator.normalize_linkage_first_addressable_scalar_frame();
    // Build 163 lays out GPR homes and retained entry lanes before reserving
    // its compact 8-byte FPR save lanes. Newer builds add their 16-byte Gekko
    // lanes directly to the predecrement frame.
    generator.normalize_linkage_first_callee_saved_frame(!allocated_float_saves.is_empty());
    generator.schedule_linkage_first_inline_aggregate_frame();
    let paired_single_float_frame = generator.behavior.frame_convention
        == mwcc_versions::FrameConvention::Predecrement;
    // The retained inline sqrt transaction can allocate saved FPRs after the
    // structured return branches have already selected the ordinary GPR
    // epilogue. Those exits must enter the newly materialized FPR restore
    // packet just like the owners that planned saved floats up front.
    let retained_sqrtf_frame = generator.has_retained_sqrtf_spill_slot();
    generator.materialize_allocated_float_frame(
        &allocated_float_saves,
        paired_single_float_frame,
        body::restores_fprs_before_gpr_helper_setup(function) || retained_sqrtf_frame,
        body::branches_enter_float_restores(function)
            || generator.unoptimized_inline_float_transaction_homes
            || retained_sqrtf_frame,
        generator.behavior.saved_float_epilogue_style,
    )?;
    generator.schedule_allocated_float_helper_epilogue();
    generator.schedule_unoptimized_inline_float_restore_order();
    generator.finalize_unoptimized_leaf_source_homes();
    // Build 163 shares the selected body schedule, but wraps GPR survivors in a
    // larger linkage-first frame. Normalize only the verified allocator shape;
    // convention-aware owners already emitted their final frame and are skipped.
    generator.normalize_linkage_first_saved_register_order();
    generator.finalize_structured_object_collision_loop_entry();
    generator.finalize_structured_object_collision_loop_guard();
    generator.finalize_structured_object_collision_loop_delta();
    generator.finalize_structured_state_transfer_entry(function);
    generator.finalize_structured_state_transfer_bit_swap(function);
    generator.finalize_structured_state_transfer_hp_call(function);
    generator.finalize_structured_state_transfer_copy_schedule(function);
    generator.finalize_structured_state_transfer_guard_schedule(function);
    generator.finalize_structured_state_transfer_scale_schedule(function);
    generator.finalize_structured_state_transfer_status_schedule(function);
    generator.finalize_structured_state_transfer_pointer_schedule(function);
    generator.finalize_structured_state_transfer_conversion_schedule(function);
    generator.finalize_structured_state_transfer_tail_schedule(function);
    generator.compact_exclusive_inline_conversion_frame();
    generator.schedule_structured_conversion_following_call();
    generator.schedule_structured_argument_load_latency();
    generator.schedule_linkage_first_inline_aggregate_frame();
    generator.schedule_structured_conversion_member_stores();
    generator.normalize_linkage_first_plain_nonleaf_frame();
    generator.schedule_retained_guarded_constant();
    generator.schedule_retained_deferred_local_entry();
    generator.schedule_retained_guarded_local_publication();
    generator.schedule_retained_deferred_local_post_call();
    generator.schedule_linkage_first_address_constant_arguments();
    generator.normalize_linkage_first_indirect_call_schedule();
    generator.normalize_linkage_first_conversion_frame();
    // Allocation and late frame normalization expose physical incoming-register
    // cycles that are intentionally invisible to the body-time scheduler.
    generator.schedule_leading_member_store_call();
    generator.schedule_leaf_tail_append();
    generator.schedule_leaf_singly_linked_unlink();
    generator.hoist_normalized_linkage_first_arg_moves();
    generator.schedule_linkage_first_variadic_frame_entry();
    generator.schedule_linkage_first_variadic_leaf_call();
    generator.schedule_linkage_first_float_result_latency();
    generator.schedule_linkage_first_global_float_arguments();
    generator.reuse_linkage_first_narrow_conversion_value();
    generator.schedule_linkage_first_data_anchor_frame();
    generator.schedule_structured_entry_call_forwarding();
    generator.schedule_linkage_first_inlined_switch_entry();
    generator.reuse_guarded_call_pointer_loads();
    generator.schedule_nested_reset_callback();
    generator.coalesce_member_equality_branch_runs();
    generator.schedule_structured_queue_transaction(function);
    generator.normalize_scratch_copy_convention();
    generator.schedule_saved_base_call_argument();
    generator.normalize_guarded_callback_single_argument_receivers(function);
    generator.schedule_guarded_callback_nullable_member_chain(function);
    generator.schedule_guarded_callback_oscillator_loop(function);
    generator.finalize_exclusive_arm_copy_encodings();
    generator.schedule_linkage_first_function_address();
    generator.schedule_linkage_first_global_member_callback();
    generator.schedule_linkage_first_global_member_forward();
    generator.schedule_direct_callback_wait_entry(function);
    generator.schedule_sequenced_callback_wait();
    generator.schedule_callback_publication_call();
    generator.schedule_retained_eager_entry_argument();
    generator.schedule_retained_split_member_guard();
    generator.schedule_linkage_first_inline_zero();
    generator.schedule_unused_array_call_linkage(function);
    generator.schedule_guarded_mutating_inline_linkage(function);
    generator.schedule_terminal_wrapper_mutating_inline(function);
    generator.schedule_call_condition_live_in_arguments();
    generator.schedule_guarded_report_store(function);
    generator.schedule_guarded_member_call_entry();
    // Whole-body owners, generic scheduling, and physical allocation converge
    // here. Apply final cross-owner schedules only when their complete measured
    // physical instruction region is present.
    generator.schedule_forwarded_member_initialization();
    generator.schedule_mixed_scalar_initialization();
    generator.schedule_pod_constructor_initialization();
    generator.schedule_saved_receiver_array_release_frame();
    generator.schedule_structured_saved_member_entry();
    generator.schedule_structured_entry_member_guard_home();
    generator.schedule_structured_indexed_callback_lookup();
    generator.schedule_assertion_float_member_return();
    generator.schedule_guarded_member_classifier_chain();
    generator.schedule_guarded_float_argument();
    generator.schedule_guarded_float_member_updates();
    generator.schedule_guarded_item_attach(function);
    generator.schedule_guarded_scaled_item_calls(function);
    generator.schedule_shared_right_float_product_pair();
    generator.schedule_shared_float_store_literal(function);
    generator.schedule_frame_vector_accumulation();
    generator.schedule_shared_global_float_pairs();
    generator.schedule_guarded_bitfield_storage_cache();
    generator.schedule_inlined_sign_store();
    generator.schedule_inlined_acceleration_select();
    generator.schedule_inlined_symmetric_float_clamp();
    generator.schedule_structured_float_or_groups();
    generator.schedule_structured_float_clamp_scale();
    generator.schedule_structured_aggregate_normalize_frame();
    generator.schedule_symmetric_sum_clamp();
    generator.schedule_bounded_acceleration();
    generator.schedule_joystick_count_updates();
    generator.schedule_grab_mash_transaction();
    generator.schedule_mixed_member_zero_reset();
    generator.schedule_inlined_context_clear_transaction();
    generator.schedule_variadic_report_member_arguments();
    generator.schedule_variadic_float_conversion_reports();
    generator.schedule_variadic_report_loop_tails();
    generator.schedule_stack_trace_report_loop();
    generator.schedule_saved_character_formatter_arguments();
    generator.schedule_position_formatter_arguments();
    generator.schedule_temporary_buffer_format_copy();
    generator.schedule_guarded_formatter_member_cache();
    generator.schedule_global_struct_binary_search();
    generator.schedule_hierarchy_push_pop_traversal();
    generator.schedule_frame_row_string_append();
    generator.schedule_ground_knockback_projection();
    generator.schedule_guarded_member_alias_initialization();
    generator.schedule_entry_saved_zero_test();
    generator.schedule_saved_pointer_zero_test();
    generator.schedule_reciprocal_frame_fill();
    generator.schedule_bounded_vector_reciprocal();
    generator.schedule_adjacent_fighter_nudge();
    generator.schedule_guarded_item_charge();
    generator.schedule_damage_vector_transaction();
    generator.schedule_dual_status_switches();
    generator.schedule_retained_item_ratio();
    generator.reuse_absolute_pooled_float_literals();
    generator.reuse_small_data_pooled_float_literals();
    generator.schedule_linkage_first_inline_aggregate_frame();
    generator.finalize_linkage_first_inline_aggregate_homes();
    generator.forward_adjacent_pointer_global_copy();
    generator.schedule_linkage_first_pointer_publication();
    generator.reuse_linkage_first_guarded_global_member_base();
    generator.pack_linkage_first_disjoint_scratch_frame();
    generator.finalize_linkage_first_instruction_array_frame();
    generator.reuse_linkage_first_condition_member();
    generator.reuse_guarded_integer_constant();
    generator.reuse_repeated_integer_constants();
    generator.finalize_structured_noncopy_conversion_lanes();
    generator.schedule_mixed_conversion_entry();
    generator.finalize_structured_guarded_ucode_packet_registers();
    generator.finalize_structured_noncopy_packet_registers();
    generator.finalize_structured_noncopy_tail_packet_registers();
    generator.schedule_structured_frame_packet_call();
    generator.reuse_structured_loop_packet_setup();
    generator.schedule_structured_frame_preloop_packets();
    generator.schedule_structured_frame_sign_clamp_load();
    // Final issue order is deliberately after every adjacency-sensitive
    // physical peephole. Control-flow functions were left in selection order
    // through allocation; schedule their branch-bounded blocks now, then
    // normalize pooled-frame packets whose MWCC order is allocation-specific.
    schedule_allocated_structured_array_pool_control_flow(&mut generator);
    generator.schedule_allocated_structured_array_pool_parameter_copies();
    generator.schedule_allocated_recovered_parameter_copies();
    generator.schedule_allocated_compact_structured_array_pool_entry();
    generator.schedule_allocated_structured_array_pool_first_image();
    generator.schedule_structured_variadic_output_frame();
    generator.finalize_structured_complement_product_pair();
    generator.finalize_structured_member_bound_call_epilogue();
    generator.schedule_structured_inlined_interrupt_transaction();
    generator.schedule_structured_inlined_guarded_value_transaction();
    generator.schedule_structured_inlined_anchored_guarded_value_transaction();
    generator.schedule_structured_inlined_anchored_retained_guarded_value_transaction();
    generator.fold_fixed_bank_transformed_loads();
    generator.schedule_linkage_first_retained_object_completion_arm();
    generator.schedule_linkage_first_retained_member_completion_arm();
    generator.schedule_linkage_first_stateful_callback_completion_arm();
    generator.schedule_linkage_first_cancel_completion_arms();
    generator.schedule_structured_precomposition_entry();
    generator.schedule_structured_inlined_dynamic_guarded_value_diamond();
    generator.schedule_structured_inlined_guarded_value_diamond();
    generator.schedule_structured_precomposition_tail();
    generator.schedule_structured_state_validation_transaction();
    generator.schedule_structured_state_read_entry();
    generator.schedule_structured_stream_sync_entry();
    generator.schedule_structured_multi_member_sync_entry();
    generator.schedule_structured_loop_exit_poll_register();
    generator.schedule_archive_header_initialization(function);

    ordinal_accounting::relocate_inline_initializer_ordinals(
        &mut generator.output,
        generator.inline_expansion_facts,
        generator
            .behavior
            .inline_initializer_ordinals_follow_strings
            && generator.inline_statement_body_substitutions != 0,
    );
    ordinal_accounting::apply_with_behavior(
        ordinal_source_function,
        &mut generator.output,
        &generator.behavior,
    );
    generator.schedule_structured_global_member_address();
    generator.schedule_linkage_first_state_switch_layout();
    generator.schedule_linkage_first_global_indirect_callback_tail();
    generator.schedule_structured_repeated_call_poll_transaction();
    generator.normalize_structured_call_poll_zero_comparisons();
    generator.schedule_structured_call_poll_fixed_address_entry();
    let scale = global_memory_schedule::hoist_integer_scales_over_address_highs(
        &mut generator.output.instructions,
        &generator.output.relocations,
    );
    remap_instruction_indices(&mut generator, &scale);
    if generator.behavior.schedule_latency_slots {
        if !generator.structured_repeated_call_poll_owner {
            branch_cleanup::align_tight_polling_call_loops(&mut generator);
        }
        // Address-pair latency filling is safe on the final physical stream,
        // after adjacency-sensitive structured schedules have consumed their
        // selected forms.
        let address = global_memory_schedule::hoist_address_highs_over_stores(
            &mut generator.output.instructions,
            &generator.output.relocations,
        );
        remap_instruction_indices(&mut generator, &address);
    }
    // Word-load narrowing participates in the generic global-address latency
    // pass above. Reapply the idempotent assembly-barrier packet owner after
    // that pass so its final saved-buffer forwarding order remains canonical.
    generator.schedule_linkage_first_asm_barrier_status_calls();
    generator.schedule_structured_inlined_preloaded_retained_guarded_value_transaction();
    generator.schedule_global_queue_pointer_send();
    generator.schedule_structured_multi_member_cache_entry();
    generator.fold_structured_call_result_assignment_zero_tests();
    generator.reuse_structured_modulo_bound_loads();
    generator.reuse_structured_frame_pointer_updates();
    generator.schedule_structured_guarded_frame_pointer_updates();
    generator.schedule_structured_inlined_byte_appends();
    generator.schedule_structured_saved_value_inlined_byte_append();
    generator.schedule_structured_single_inlined_byte_append();
    generator.schedule_structured_global_base_entry();
    generator.schedule_structured_broad_global_base_loop();
    generator.schedule_structured_global_byte_loop();
    generator.finalize_structured_compact_narrow_scalar_frame();
    generator.finalize_structured_guarded_scalar_output_frame();
    generator.finalize_structured_shared_switch_scalar_frame();
    generator.finalize_structured_mixed_switch_scalar_frame();
    generator.finalize_structured_write_register_frame();
    generator.finalize_structured_memory_transfer_frame();
    generator.finalize_structured_memory_write_frame(function);
    generator.finalize_linkage_first_forwarded_context_frame(function);
    if generator.structured_nonreturning {
        generator.normalize_nonreturning_materialization_copies();
    }
    generator.normalize_patched_build159_pointer_difference_call();
    generator.schedule_legacy_member_constant_store_run();
    generator.schedule_patched_status_initialization_chain();
    generator.canonicalize_linkage_first_post_asm_linkage();
    generator.schedule_linkage_first_post_asm_function_address();
    generator.schedule_linkage_first_post_asm_variadic_store();
    generator.schedule_structured_repeated_value_inlined_byte_appends();
    generator.schedule_pointer_table_index_cursor_prologue();
    generator.schedule_indexed_allocation_pair();
    generator.schedule_pointer_table_index_cursor_publication();
    generator.schedule_pointer_table_index_cursor_lookup();
    generator.schedule_pointer_table_index_cursor_epilogue();
    generator.schedule_prescaled_pointer_table_index();
    generator.schedule_polymorphic_zero_constructor();
    generator.schedule_materialized_vec3_product(function);
    generator.schedule_structured_dense_destroy_loop();
    generator.schedule_structured_global_pointer_replacement();
    generator.schedule_copy_sign_frame(function);
    if !function.peephole_disabled {
        generator.fold_recorded_boolean_zero_tests();
    }
    generator.schedule_structured_heap_transactions();
    generator.normalize_float_to_int_scratch_images();
    generator.schedule_structured_global_base_epilogue();
    generator.normalize_nintendo_saved_gpr_epilogue();
    generator.fuse_retained_zero_saved_pair();
    generator.schedule_structured_effecter_preloop();
    generator.normalize_structured_effecter_loop_conversion_frame();
    // Allocation can coalesce the terminal result move that previously kept a
    // conditional exit from targeting the final `blr`. Canonicalize again on
    // the finished physical stream so returned loop accumulators use MWCC's
    // direct `b<cc>lr` form too.
    if !generator.preserve_terminal_return_branches {
        collapse_forward_branch_to_terminal_blr(&mut generator.output.instructions);
    }

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
    generator.select_dense_counted_loop_debug_variables(function);

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
        if generator
            .forced_general_callee_saved
            .contains(&interval.vreg)
        {
            interval
                .avoid
                .extend(generator.constraints.general_pool.iter().copied());
            interval.avoid.sort_unstable();
            interval.avoid.dedup();
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
    allocation_diagnostics::report_pressure(generator, &liveness, &allocation, &used);
    generator.reconcile_allocated_general_frame(&allocation, &used)?;
    allocation_debug::reconcile_variable_locations(&mut generator.locations, &allocation);
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
    let scheduling_enabled =
        !generator.output.pre_scheduled && generator.behavior.schedule_latency_slots;
    let permutation: Vec<usize> = if !scheduling_enabled {
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
    if scheduling_enabled {
        let memory = global_memory_schedule::hoist_independent_sda_loads(
            &mut generator.output.instructions,
            &generator.output.relocations,
        );
        remap_instruction_indices(generator, &memory);
    }
}

/// Move the epilogue's saved-LR reload up to right after the last call, remapping
/// relocation indices through the resulting permutation.
fn hoist_link_register_reload(generator: &mut Generator, saved_float_registers: &[u8]) {
    if generator.owns_link_register_schedule || !generator.behavior.schedule_latency_slots {
        return;
    }
    // GC/1.1p1 deliberately restores the caller stack pointer before loading LR through
    // `4(r1)`. That load is address-dependent on the stack restore and therefore is not an
    // epilogue latency candidate (`li result; addi r1,...; lwz r0,4(r1)`). The generic hoist
    // only understands the reload-through-current-frame convention and would incorrectly move
    // this load ahead of both operations.
    if generator
        .output
        .instructions
        .iter()
        .position(|instruction| {
            matches!(instruction, Instruction::LoadWord { d: 0, a: 1, offset: 4 })
        })
        .is_some_and(|reload| link_reload_uses_restored_stack(generator, reload))
    {
        return;
    }
    let follows_saved_float_result = generator.behavior.saved_float_epilogue_style
        != mwcc_versions::SavedFloatEpilogueStyle::LinkReloadBeforeResult;
    let permutation = mwcc_vreg::hoist_link_register_reload(
        &mut generator.output.instructions,
        saved_float_registers,
        follows_saved_float_result,
    );
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
    // GC/1.1p1's `lwz r0,4(r1)` reads through the caller stack pointer
    // established by the immediately preceding addi. It looks superficially
    // like an independent shared-epilogue result operation, but swapping the
    // pair changes the address and contradicts the build's frame convention.
    if link_reload_uses_restored_stack(generator, reload) {
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

fn link_reload_uses_restored_stack(generator: &Generator, reload: usize) -> bool {
    generator.behavior.frame_convention == mwcc_versions::FrameConvention::LinkageFirst
        && generator.behavior.plain_linkage_epilogue_style
            == mwcc_versions::PlainLinkageEpilogueStyle::StackRestoreBeforeReload
        && reload > 0
        && matches!(
            generator.output.instructions[reload],
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 4,
            }
        )
        && matches!(
            generator.output.instructions[reload - 1],
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate,
            } if immediate == generator.frame_size
        )
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
    let metadata_owners = self_move_metadata_owners(&generator.output);
    let permutation = mwcc_vreg::coalesce_self_moves_preserving(
        &mut generator.output.instructions,
        &metadata_owners,
    );
    remap_instruction_indices(generator, &permutation);
}

fn self_move_metadata_owners(output: &MachineFunction) -> Vec<usize> {
    output
        .relocations
        .iter()
        .map(|relocation| relocation.instruction_index)
        .chain(
            output
                .data_section_displacements
                .iter()
                .map(|displacement| displacement.instruction_index),
        )
        .collect()
}

/// Fold an immediately converted float copy into the conversion's source.
///
/// Allocation can color a call-result handoff as `fmr f0,f1; fctiwz f0,f0`.
/// The copy has no observable intermediate value and MWCC converts directly
/// from f1. Apply the same coalescing to every adjacent physical pair while
/// preserving all instruction-index owners.
fn coalesce_float_conversion_moves(generator: &mut Generator) {
    let mut index = 0;
    while index + 1 < generator.output.instructions.len() {
        let replacement = adjacent_float_conversion_move(&generator.output.instructions, index);
        let Some((d, b)) = replacement else {
            index += 1;
            continue;
        };
        let rounds_to_single = matches!(
            generator.output.instructions.get(index + 1),
            Some(Instruction::RoundToSingle { .. })
        );
        generator.output.instructions[index + 1] = if rounds_to_single {
            Instruction::RoundToSingle { d, b }
        } else {
            Instruction::ConvertToIntegerWordZero { d, b }
        };
        remove_instruction_retargeting_to_next(generator, index);
    }
}

fn adjacent_float_conversion_move(
    instructions: &[Instruction],
    index: usize,
) -> Option<(u8, u8)> {
    match (instructions.get(index)?, instructions.get(index + 1)?) {
        (
            Instruction::FloatMove {
                d: moved,
                b: source,
            },
            Instruction::ConvertToIntegerWordZero {
                d: converted,
                b: converted_source,
            },
        ) if moved == converted && moved == converted_source => Some((*converted, *source)),
        (
            Instruction::FloatMove {
                d: moved,
                b: source,
            },
            Instruction::RoundToSingle {
                d: converted,
                b: converted_source,
            },
        ) if moved == converted && moved == converted_source => Some((*converted, *source)),
        _ => None,
    }
}

/// Initialized-array pool functions retain their selection-order virtual stream
/// through allocation so scheduling cannot lengthen live ranges enough to make
/// a previously feasible function spill or fail. Once registers are physical,
/// schedule their branch-bounded blocks; other structured control-flow owners
/// keep their dedicated measured schedules.
fn schedule_allocated_structured_array_pool_control_flow(generator: &mut Generator) {
    if generator.output.pre_scheduled
        || !generator.behavior.schedule_latency_slots
        || !generator.structured_array_pool_emitted
        || !generator.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::BranchConditionalForward { .. } | Instruction::Branch { .. }
            )
        })
    {
        return;
    }
    let permutation =
        mwcc_vreg::schedule_branch_bounded(&mut generator.output.instructions);
    for relocation in &mut generator.output.relocations {
        relocation.instruction_index = permutation[relocation.instruction_index];
    }
    for displacement in &mut generator.output.data_section_displacements {
        displacement.instruction_index = permutation[displacement.instruction_index];
    }
}

/// Remap relocations, late data displacements, and internal branch destinations
/// after an instruction permutation. Each is owned by an instruction index;
/// leaving one stale after scheduling or deleting a self-move patches or enters
/// the wrong instruction.
pub(crate) fn remap_instruction_indices(generator: &mut Generator, permutation: &[usize]) {
    remap_machine_function_indices(&mut generator.output, permutation);
}

/// Remap owners stored directly by a machine function when a scheduler does
/// not otherwise need access to generator state.
pub(crate) fn remap_machine_function_indices(
    output: &mut mwcc_machine_code::MachineFunction,
    permutation: &[usize],
) {
    for relocation in &mut output.relocations {
        relocation.instruction_index = permutation[relocation.instruction_index];
    }
    for displacement in &mut output.data_section_displacements {
        displacement.instruction_index = permutation[displacement.instruction_index];
    }
    // Jump-table entries are byte offsets into the same instruction stream.
    // They are label owners just like branch destinations; allocator
    // coalescing and late scheduling must move them through the permutation.
    for table in &mut output.jump_tables {
        for entry in &mut table.entries {
            let old_index = *entry as usize / 4;
            let new_index = if old_index == permutation.len() {
                output.instructions.len()
            } else {
                permutation[old_index]
            };
            *entry = u32::try_from(new_index)
                .unwrap_or(u32::MAX)
                .saturating_mul(4);
        }
    }
    remap_branch_targets(&mut output.instructions, permutation);
}

/// Move one instruction earlier after labels have been resolved, preserving
/// every instruction-index owner and branch destination.
pub(crate) fn move_instruction_before_retargeting(
    generator: &mut Generator,
    from: usize,
    to: usize,
) {
    let old_len = generator.output.instructions.len();
    debug_assert!(to < from);
    debug_assert!(from < old_len);
    let instruction = generator.output.instructions.remove(from);
    generator.output.instructions.insert(to, instruction);
    generator.labels.moved_before(from, to);
    let permutation = instruction_move_before_permutation(old_len, from, to);
    remap_instruction_indices(generator, &permutation);
}

/// Hoist one instruction while leaving incoming control-flow edges on the
/// continuation that originally followed it. Relocations and late data
/// displacements remain owned by the hoisted instruction.
///
/// This differs from [`move_instruction_before_retargeting`], which preserves
/// the moved instruction as a branch destination. Loop-invariant code that is
/// removed from the head of a body block needs the block label to stay behind.
pub(crate) fn move_instruction_before_retargeting_source_to_next(
    generator: &mut Generator,
    from: usize,
    to: usize,
) {
    debug_assert!(to < from);
    debug_assert!(from + 1 < generator.output.instructions.len());
    let continuation = from + 1;
    retarget_instruction_destinations(generator, from, continuation);
    move_instruction_before_retargeting(generator, from, to);
}

/// Move every control-flow destination from one existing instruction to
/// another without changing instruction ownership. Resolved branches, jump
/// tables, and still-recorded label bindings are one semantic label and must
/// be updated together.
pub(crate) fn retarget_instruction_destinations(
    generator: &mut Generator,
    from: usize,
    to: usize,
) {
    retarget_exact_branch_destinations(&mut generator.output.instructions, from, to);
    retarget_exact_jump_table_destinations(&mut generator.output.jump_tables, from, to);
    generator.labels.retarget_bindings(from, to);
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

/// Insert one instruction after labels have been resolved, preserving every
/// instruction-index owner and the identity of existing branch destinations.
pub(crate) fn insert_instruction_retargeting(
    generator: &mut Generator,
    index: usize,
    instruction: Instruction,
) {
    let old_len = generator.output.instructions.len();
    debug_assert!(index <= old_len);
    generator.output.instructions.insert(index, instruction);
    generator.labels.inserted(index, 1);
    for relocation in &mut generator.output.relocations {
        if relocation.instruction_index >= index {
            relocation.instruction_index += 1;
        }
    }
    for displacement in &mut generator.output.data_section_displacements {
        if displacement.instruction_index >= index {
            displacement.instruction_index += 1;
        }
    }
    retarget_jump_table_entries_after_insertion(&mut generator.output.jump_tables, index);
    for instruction in &mut generator.output.instructions {
        let target = match instruction {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target } => target,
            _ => continue,
        };
        if *target >= index {
            *target += 1;
        }
    }
}

fn retarget_jump_table_entries_after_insertion(
    tables: &mut [mwcc_machine_code::JumpTable],
    index: usize,
) {
    for table in tables {
        for entry in &mut table.entries {
            if *entry as usize / 4 >= index {
                *entry = entry.saturating_add(4);
            }
        }
    }
}

fn instruction_removal_permutation(old_len: usize, index: usize) -> Vec<usize> {
    debug_assert!(index < old_len);
    (0..old_len)
        .map(|old| if old <= index { old } else { old - 1 })
        .collect()
}

fn instruction_move_before_permutation(old_len: usize, from: usize, to: usize) -> Vec<usize> {
    debug_assert!(to < from);
    debug_assert!(from < old_len);
    (0..old_len)
        .map(|old| {
            if old == from {
                to
            } else if (to..from).contains(&old) {
                old + 1
            } else {
                old
            }
        })
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
        } else if *target >= old_end {
            // Structured lowering temporarily encodes unresolved joins as
            // out-of-range sentinels. Instruction-removal passes must leave
            // those opaque placeholders for their owning resolver.
            *target
        } else {
            permutation[*target]
        };
    }
}

fn retarget_exact_branch_destinations(
    instructions: &mut [Instruction],
    from: usize,
    to: usize,
) {
    for instruction in instructions {
        let target = match instruction {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target } => target,
            _ => continue,
        };
        if *target == from {
            *target = to;
        }
    }
}

fn retarget_exact_jump_table_destinations(
    tables: &mut [mwcc_machine_code::JumpTable],
    from: usize,
    to: usize,
) {
    let from_offset = u32::try_from(from).unwrap_or(u32::MAX).saturating_mul(4);
    let to_offset = u32::try_from(to).unwrap_or(u32::MAX).saturating_mul(4);
    for table in tables {
        for entry in &mut table.entries {
            if *entry == from_offset {
                *entry = to_offset;
            }
        }
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
    fn a_data_displacement_owned_self_add_is_not_coalesced() {
        let mut output = MachineFunction::new("anchored");
        output.instructions = vec![
            Instruction::AddImmediate {
                d: 31,
                a: 31,
                immediate: 0,
            },
            Instruction::move_register(4, 4),
        ];
        output.data_section_displacements.push(
            mwcc_machine_code::DataSectionDisplacement {
                instruction_index: 0,
                target: mwcc_machine_code::DataSectionDisplacementTarget::Symbol(
                    "object".into(),
                ),
            },
        );

        let owners = self_move_metadata_owners(&output);
        mwcc_vreg::coalesce_self_moves_preserving(&mut output.instructions, &owners);

        assert_eq!(output.instructions.len(), 1);
        assert!(matches!(
            output.instructions[0],
            Instruction::AddImmediate {
                d: 31,
                a: 31,
                immediate: 0
            }
        ));
    }

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
    fn moving_an_instruction_preserves_branch_destinations() {
        let mut instructions = vec![
            Instruction::Branch { target: 3 },
            Instruction::load_immediate(3, 1),
            Instruction::Branch { target: 1 },
            Instruction::load_immediate(4, 2),
        ];
        let moved = instructions.remove(3);
        instructions.insert(1, moved);
        let permutation = instruction_move_before_permutation(4, 3, 1);
        remap_branch_targets(&mut instructions, &permutation);

        assert_eq!(instructions[0], Instruction::Branch { target: 1 });
        assert_eq!(instructions[3], Instruction::Branch { target: 2 });
    }

    #[test]
    fn hoisting_a_block_head_leaves_incoming_edges_on_its_continuation() {
        let mut instructions = vec![
            Instruction::Branch { target: 3 },
            Instruction::load_immediate(3, 1),
            Instruction::Branch { target: 1 },
            Instruction::load_immediate(4, 2),
            Instruction::load_immediate(5, 3),
        ];
        retarget_exact_branch_destinations(&mut instructions, 3, 4);
        let moved = instructions.remove(3);
        instructions.insert(1, moved);
        let permutation = instruction_move_before_permutation(5, 3, 1);
        remap_branch_targets(&mut instructions, &permutation);

        assert_eq!(instructions[0], Instruction::Branch { target: 4 });
        assert_eq!(instructions[3], Instruction::Branch { target: 2 });
    }

    #[test]
    fn inserting_an_instruction_preserves_jump_table_destinations() {
        let mut tables = [mwcc_machine_code::JumpTable {
            entries: vec![4, 12, 20],
            anonymous_offset: 7,
        }];

        retarget_jump_table_entries_after_insertion(&mut tables, 3);

        assert_eq!(tables[0].entries, [4, 16, 24]);
    }

    #[test]
    fn hoisting_a_jump_table_arm_leaves_the_entry_on_its_continuation() {
        let mut tables = [mwcc_machine_code::JumpTable {
            entries: vec![4, 12, 20],
            anonymous_offset: 7,
        }];

        retarget_exact_jump_table_destinations(&mut tables, 3, 4);

        assert_eq!(tables[0].entries, [4, 16, 20]);
    }

    #[test]
    fn instruction_remapping_preserves_unresolved_branch_placeholders() {
        let placeholder = usize::MAX / 4;
        let mut instructions = vec![
            Instruction::Branch {
                target: placeholder,
            },
            Instruction::BranchToLinkRegister,
        ];
        remap_branch_targets(&mut instructions, &[0, 1]);

        assert_eq!(
            instructions[0],
            Instruction::Branch {
                target: placeholder
            }
        );
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
    fn an_immediately_converted_float_copy_folds_into_the_conversion() {
        let instructions = vec![
            Instruction::FloatMove { d: 0, b: 1 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
        ];

        assert_eq!(
            adjacent_float_conversion_move(&instructions, 0),
            Some((0, 1))
        );
        assert_eq!(adjacent_float_conversion_move(&instructions, 1), None);
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

        let permutation =
            collapse_conditional_skip_to_backward_branch_once(&mut instructions)
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
