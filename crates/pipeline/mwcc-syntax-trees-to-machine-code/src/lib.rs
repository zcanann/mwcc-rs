//! Pipeline: syntax trees -> machine code.
//!
//! Every function body is lowered by the staged PCode pipeline
//! (`mwcc-pcode-path`, installed by the driver through
//! [`install_pcode_lowering`]); an `asm` function is assembled verbatim. This
//! crate keeps the unit-level products around those bodies: inline-body
//! analysis, function statics, ordinal accounting and C++ adjustor thunks.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::MachineFunction;
use mwcc_syntax_trees::{Function, GlobalDeclaration, LocalDataRelocationTarget};
use mwcc_versions::{Behavior, CompilerConfig};
use std::collections::{HashMap, HashSet};

mod analysis;
mod asm;
mod automatic_rodata;
mod cxx_abi;
mod inline_expansion;
mod inline_source_order;
mod inline_summaries;
mod intrinsics;
mod ordinal_accounting;
mod packet_publication;
mod pcode_path;
mod static_local_storage;
mod symbol_order;

pub use inline_expansion::{InlineBodySet, InlineNestingBudget};
pub use inline_summaries::InlineSummaries;
pub use pcode_path::{install_pcode_lowering, PcodeLowering, PcodeRequest};

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

/// Source facts whose identities are finer than the executable syntax tree.
/// Keeping them together lets lowering consume new facts without widening its
/// call boundary each time. Keys use emitted function names and source locals.
#[derive(Clone, Copy)]
pub struct SourceFunctionFacts<'a> {
    pub variable_function_types:
        &'a HashMap<(String, String), mwcc_syntax_trees::SourceFunctionType>,
    pub global_function_types: &'a HashMap<String, mwcc_syntax_trees::SourceFunctionType>,
    pub variable_reference_counts: &'a HashMap<String, HashMap<String, usize>>,
    /// Source language, independent of C++ name mangling or extern-C linkage.
    pub is_cxx: bool,
    pub nonvolatile_pointer_bindings: &'a HashSet<(String, String)>,
    /// Pointer parameters and locals declared `const T *`.
    pub parameter_pointee_const: &'a HashSet<(String, String)>,
    /// Functions defined under `#pragma dont_inline on`.
    pub dont_inline_functions: &'a HashSet<String>,
    /// Locals whose initializer substituted an inline call.
    pub inline_initialized_locals: &'a HashSet<(String, String)>,
    pub local_pointee_const: &'a HashSet<(String, String)>,
    pub parameter_fundamentals:
        &'a HashMap<(String, String), mwcc_syntax_trees::SourceFundamentalType>,
    pub local_fundamentals:
        &'a HashMap<(String, String), mwcc_syntax_trees::SourceFundamentalType>,
}

/// Lower with source-proven declaration facts. Missing facts disable reuse.
/// (Several inputs served the legacy owners; the driver still passes them.)
#[allow(clippy::too_many_arguments, unused_variables)]
pub fn lower_function_with_source_facts(
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
    source_facts: SourceFunctionFacts<'_>,
    config: CompilerConfig,
) -> Compilation<MachineFunction> {
    let mut output = lower_function_body(
        function,
        globals,
        fixed_address_arrays,
        call_return_types,
        call_parameter_types,
        prototyped_names,
        variadic_definitions,
        inline_bodies,
        call_return_fundamentals,
        source_facts,
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

/// The function's body: an `asm` function assembled verbatim, every other
/// through PCode.
#[allow(clippy::too_many_arguments)]
fn lower_function_body(
    function: &Function,
    globals: &[GlobalDeclaration],
    fixed_address_arrays: &HashMap<String, (i64, mwcc_syntax_trees::Type)>,
    call_return_types: &HashMap<String, mwcc_syntax_trees::Type>,
    call_parameter_types: &HashMap<String, Vec<mwcc_syntax_trees::Type>>,
    prototyped_names: &std::collections::HashSet<String>,
    variadic_definitions: &std::collections::HashSet<String>,
    inline_bodies: &InlineBodySet,
    call_return_fundamentals: &HashMap<String, mwcc_syntax_trees::SourceFundamentalType>,
    source_facts: SourceFunctionFacts<'_>,
    config: CompilerConfig,
) -> Compilation<MachineFunction> {
    if function.asm_body.is_some() {
        return asm::assemble_asm_function(function, Behavior::resolve(&config));
    }
    // A variadic definition needs its register save area and va_list
    // support, which are not modeled.
    if variadic_definitions.contains(&function.name) {
        return Err(Diagnostic::error("PCode lowering: a variadic function definition (not yet supported)"));
    }
    // Whether MWCC expands a call inline: None (an ordinary call), or
    // the body it expands (None inside when not modeled). Under
    // `-inline auto` a definition preceding the caller (any definition
    // when deferred or with `-ipa file`) within the size limit; a
    // source-declared inline within its larger limit.
    let inline_decision = |name: &str| -> Option<Option<&mwcc_syntax_trees::Function>> {
        // (`#pragma dont_inline` at the caller's or the callee's definition.)
        if (source_facts.dont_inline_functions.contains(name) || source_facts.dont_inline_functions.contains(&function.name))
            && std::env::var_os("MWCC_PCODE_IGNORE_DONT_INLINE").is_none()
        {
            return None;
        }
        let automatic = config.flags.inline_enabled && config.flags.automatic_inlining_enabled;
        let size_goal = config.flags.optimization_goal == mwcc_versions::OptimizationGoal::Size;
        let limit = pcode_path::automatic_inline_limit(config.build.label, size_goal);
        let ordinary = if config.flags.inline_deferred
            || config.flags.ipa_file
            || std::env::var_os("MWCC_PCODE_ANY_BODY_INLINES").is_some()
        {
            inline_bodies.definition_body(name)
        } else if automatic {
            inline_bodies.source_visible_definition(name, &function.name)
        } else {
            None
        };
        if let Some(body) = ordinary {
            if std::env::var_os("MWCC_PCODE_NO_INLINE_LIMIT").is_some() {
                return Some(None);
            }
            // Under `-ipa file` with `,s` a static callee with one call
            // site inlines whatever its size.
            if config.flags.ipa_file && size_goal && body.is_static && inline_bodies.definition_call_count(name) == 1 {
                return Some(Some(body));
            }
            if pcode_path::inline_statement_count(body) <= limit {
                return Some(Some(body));
            }
        }
        // (Only a source-declared inline, which is no ordinary definition,
        // takes the larger limit.)
        if inline_bodies.definition_body(name).is_some() && std::env::var_os("MWCC_PCODE_DEFINITIONS_AS_DECLARED").is_none() {
            return None;
        }
        let declared = inline_bodies.composable_body(name).or_else(|| inline_bodies.retained_body(name))?;
        Some((pcode_path::inline_statement_count(declared) <= 64).then_some(declared))
    };
    // Function statics address like globals: the body sees them
    // stripped from its locals, and their data rides the output. (Ones
    // whose initializers point at string literals stay legacy.)
    let statics = collect_static_locals(function, globals, config);
    let (static_locals, static_local_data, static_strings_free) = match statics {
        Ok((locals, data, strings, aggregate_strings)) => {
            (locals, data, strings.is_empty() && aggregate_strings.is_empty())
        }
        Err(_) => (Vec::new(), Vec::new(), false),
    };
    let stripped_statics;
    let pcode_function = if static_locals.is_empty() {
        function
    } else {
        stripped_statics = mwcc_syntax_trees::Function {
            locals: function.locals.iter().filter(|local| !local.is_static).cloned().collect(),
            ..function.clone()
        };
        &stripped_statics
    };
    // (Statics carrying relocations — pointer tables — stay legacy.)
    // (As do ones whose data would not fit their computed size.)
    // (Symbol relocations — tables of functions or objects — ride along.)
    let statics_supported = (static_strings_free
        && static_locals.iter().all(|local| {
            local.data_relocations.is_empty()
                || (local.data_relocations.iter().all(|relocation| matches!(relocation.target, LocalDataRelocationTarget::Symbol(_)))
                    && std::env::var_os("MWCC_PCODE_NO_STATIC_TABLES").is_none())
        })
        && static_local_data
            .iter()
            .all(|datum| datum.initial_bytes.as_ref().is_none_or(|bytes| bytes.len() as u32 <= datum.size)))
        || function.locals.iter().all(|local| !local.is_static);
    if !statics_supported {
        return Err(Diagnostic::error("PCode lowering: static locals pointing at strings (not yet supported)"));
    }
    let mut output = pcode_path::lower(&pcode_path::PcodeRequest {
        function: pcode_function,
        variadic: variadic_definitions.contains(&function.name),
        static_locals: &static_locals,
        globals,
        fixed_address_arrays,
        call_return_types,
        pointer_return_type: &|owner, name| {
            let signature = if owner.is_empty() {
                source_facts.global_function_types.get(name)
            } else {
                source_facts.variable_function_types.get(&(owner.to_owned(), name.to_owned()))
            };
            signature.map(|signature| signature.return_type.declared_type)
        },
        variadic_callees: variadic_definitions,
        prototyped: prototyped_names,
        call_parameter_types,
        config: &config,
        is_intrinsic: &intrinsics::is_intrinsic_call,
        returns_bool: call_return_fundamentals.get(&function.name)
            == Some(&mwcc_syntax_trees::SourceFundamentalType::Boolean),
        cxx: source_facts.is_cxx,
        nonvolatile_pointers: &source_facts
            .nonvolatile_pointer_bindings
            .iter()
            .filter(|(owner, _)| owner == &function.name)
            .map(|(_, name)| name.clone())
            .collect(),
        inline_initialized_locals: &source_facts
            .inline_initialized_locals
            .iter()
            .filter(|(owner, _)| owner == &function.name)
            .map(|(_, name)| name.clone())
            .collect(),
        const_pointers: &source_facts
            .parameter_pointee_const
            .iter()
            .chain(source_facts.local_pointee_const.iter())
            .filter(|(owner, _)| owner == &function.name)
            .map(|(_, name)| name.clone())
            .collect(),
        has_body: &|name| inline_decision(name).is_some(),
        inline_bodies: &call_return_types
            .keys()
            .filter_map(|name| Some((name.clone(), inline_decision(name).flatten()?)))
            .collect(),
    })?;
    output.static_locals = static_local_data;
    Ok(output)
}

/// Function statics (`name$K` LOCAL objects the writer numbers off the
/// function's @N sequence): their declarations, data, and the string
/// literals their initializers point at.
#[allow(clippy::type_complexity)]
fn collect_static_locals(
    function: &mwcc_syntax_trees::Function,
    globals: &[GlobalDeclaration],
    config: CompilerConfig,
) -> Compilation<(
    Vec<mwcc_syntax_trees::LocalDeclaration>,
    Vec<mwcc_machine_code::StaticLocal>,
    Vec<Vec<u8>>,
    Vec<Vec<u8>>,
)> {
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
        let element = static_local_storage::element_size(local.declared_type);
        let size = element * local.array_length.map_or(1, u32::from);
        // The byte image: a brace-list array, or a scalar literal folded here.
        let bytes = match (&local.data_bytes, &local.initializer) {
            // `char s[3] = "abc";`: an array exactly the string's length drops
            // the terminating NUL.
            (Some(bytes), _) if bytes.len() as u32 == size + 1 && bytes.last() == Some(&0) => {
                Some(bytes[..size as usize].to_vec())
            }
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
        let alignment = static_local_storage::alignment(
            local.declared_type,
            local.array_length,
            local.attribute_alignment,
            config,
        );
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
    Ok((static_locals, static_local_data, static_local_strings, static_aggregate_strings))
}
