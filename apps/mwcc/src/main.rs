//! `mwcc` — the compiler driver.
//!
//! The command line is intentionally compatible with `mwcceppc` so the oracle
//! harness can swap us in: `mwcc [flags...] -c <input.c> -o <output.o>`. Flags we
//! do not yet model are ignored. `--emit-artifacts <dir>` writes a per-phase
//! report for inspecting how a translation unit becomes bytes.

mod cxx_analysis_residues;
mod cxx_rtti_names;
mod function_order;
mod global_initializers;
mod inline_ordinal_positions;
mod reference_analysis;

use mwcc_core::{Compilation, Diagnostic};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceLanguage {
    C,
    Cxx,
}

impl SourceLanguage {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "c" => Some(Self::C),
            "c++" | "cpp" | "cxx" => Some(Self::Cxx),
            _ => None,
        }
    }
}

struct Invocation {
    input: Option<String>,
    output: Option<String>,
    build_label: Option<String>,
    artifacts_directory: Option<String>,
    /// Explicit `-lang` selection. When absent, the input extension selects the
    /// frontend; real project lines sometimes deliberately compile `.cpp` as C.
    source_language: Option<SourceLanguage>,
    /// Ordered filesystem access paths used to materialize the source graph.
    include_paths: Vec<PathBuf>,
    /// Object-like macro definitions visible while selecting conditional source.
    preprocessor_definitions: std::collections::HashMap<String, String>,
    /// Codegen-affecting flags parsed from the real build line.
    flags: mwcc_versions::Flags,
    /// Diagnostic parity mode: emit every independently lowerable function and
    /// skip backend-deferred definitions instead of aborting the whole object.
    parity_keep_going: bool,
}

fn parse_invocation(arguments: &[String]) -> Invocation {
    use mwcc_versions::{CharDefault, EnumStorage, GlobalAddressing, Optimization};
    let mut invocation = Invocation {
        input: None,
        output: None,
        build_label: None,
        artifacts_directory: None,
        source_language: None,
        include_paths: Vec::new(),
        preprocessor_definitions: std::collections::HashMap::new(),
        flags: mwcc_versions::Flags::default(),
        parity_keep_going: false,
    };
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--parity-keep-going" => invocation.parity_keep_going = true,
            "-c" => {
                index += 1;
                invocation.input = arguments.get(index).cloned();
            }
            "-o" => {
                index += 1;
                invocation.output = arguments.get(index).cloned();
            }
            "--build" => {
                index += 1;
                invocation.build_label = arguments.get(index).cloned();
            }
            "--emit-artifacts" => {
                index += 1;
                invocation.artifacts_directory = arguments.get(index).cloned();
            }
            "-lang" => {
                index += 1;
                if let Some(language) = arguments
                    .get(index)
                    .and_then(|value| SourceLanguage::parse(value))
                {
                    invocation.source_language = Some(language);
                }
            }
            "-i" | "-I" | "-ir" | "-isystem" => {
                index += 1;
                if let Some(path) = arguments.get(index) {
                    invocation.include_paths.push(PathBuf::from(path));
                }
            }
            "-D" | "-d" => {
                index += 1;
                if let Some(definition) = arguments.get(index) {
                    apply_macro_definition(&mut invocation.preprocessor_definitions, definition);
                }
            }
            "-U" => {
                index += 1;
                if let Some(name) = arguments.get(index) {
                    invocation.preprocessor_definitions.remove(name);
                }
            }
            // `-char signed`/`-char unsigned` overrides the build's `char` default.
            "-char" => {
                index += 1;
                invocation.flags.char_default = match arguments.get(index).map(String::as_str) {
                    Some("signed") => CharDefault::Signed,
                    Some("unsigned") => CharDefault::Unsigned,
                    _ => CharDefault::BuildDefault,
                };
            }
            // `-enum min` uses the narrowest storage that contains the declared
            // values; `-enum int` restores the four-byte default.
            "-enum" => {
                index += 1;
                invocation.flags.enum_storage = match arguments.get(index).map(String::as_str) {
                    Some("min") => EnumStorage::Minimum,
                    Some("int") => EnumStorage::Int,
                    _ => invocation.flags.enum_storage,
                };
            }
            // `-Cpp_exceptions off` suppresses the extab/extabindex unwind tables.
            "-Cpp_exceptions" => {
                index += 1;
                if arguments.get(index).map(String::as_str) == Some("off") {
                    invocation.flags.cpp_exceptions = false;
                }
            }
            // RTTI is a last-wins C++ ABI policy, independent of exception
            // tables. Project build lines commonly spell both off and on.
            "-RTTI" => {
                index += 1;
                invocation.flags.rtti = match arguments.get(index).map(String::as_str) {
                    Some("on") => true,
                    Some("off") => false,
                    _ => invocation.flags.rtti,
                };
            }
            // `-pragma "cats off"` suppresses the Code Address Table section.
            // Accept `on` as well so the last command-line pragma wins, as in mwcc.
            "-pragma" => {
                index += 1;
                invocation.flags.emit_mwcats = match arguments.get(index).map(String::as_str) {
                    Some("cats off") => false,
                    Some("cats on") => true,
                    _ => invocation.flags.emit_mwcats,
                };
            }
            // `-inline …`: a `deferred` setting emits functions in reverse order.
            "-inline" => {
                index += 1;
                if let Some(value) = arguments.get(index) {
                    invocation.flags.inline_enabled = value != "off";
                    let deferred = value.split(',').any(|part| part.trim() == "deferred");
                    let deferred_continuation = value.trim_end().ends_with(',')
                        && arguments
                            .get(index + 1)
                            .is_some_and(|part| part.trim() == "deferred");
                    if deferred_continuation {
                        index += 1;
                    }
                    invocation.flags.inline_deferred = deferred || deferred_continuation;
                }
            }
            // `-str reuse,readonly` pools string literals in read-only data.
            // A later `-str reuse` only restates the pooling policy; it does not
            // cancel a standalone `-rostr` (the GC 3.0 project lines use both).
            "-str" => {
                index += 1;
                if let Some(value) = arguments.get(index) {
                    for part in value.split(',') {
                        match part {
                            "readonly" => invocation.flags.string_literals_read_only = true,
                            "noreadonly" => invocation.flags.string_literals_read_only = false,
                            "pool" => invocation.flags.string_literals_packed = true,
                            "nopool" => invocation.flags.string_literals_packed = false,
                            _ => {}
                        }
                    }
                }
            }
            // Modern command lines spell the same read-only string-pool mode
            // as the standalone `-rostr` switch.
            "-rostr" => invocation.flags.string_literals_read_only = true,
            // `-pool off` disables compiler pooling and is stamped into the
            // object's `.comment` header. Accept `on` so the last flag wins.
            "-pool" => {
                index += 1;
                invocation.flags.pooling_enabled = match arguments.get(index).map(String::as_str) {
                    Some("off") => false,
                    Some("on") => true,
                    _ => invocation.flags.pooling_enabled,
                };
            }
            // `-use_lmw_stmw on` selects inline multiple-register saves and
            // restores. Accept `off` too so the last occurrence wins.
            "-use_lmw_stmw" => {
                index += 1;
                invocation.flags.use_lmw_stmw = match arguments.get(index).map(String::as_str) {
                    Some("on") => true,
                    Some("off") => false,
                    _ => invocation.flags.use_lmw_stmw,
                };
            }
            // `-schedule off` preserves dependency order instead of filling
            // latency slots with independent address materializations.
            "-schedule" => {
                index += 1;
                invocation.flags.scheduler_enabled = match arguments.get(index).map(String::as_str)
                {
                    Some("off") => false,
                    Some("on") => true,
                    _ => invocation.flags.scheduler_enabled,
                };
            }
            // Floating-point contraction is independent from the O-level and
            // follows the last command-line occurrence.
            "-fp_contract" => {
                index += 1;
                invocation.flags.fp_contract = match arguments.get(index).map(String::as_str) {
                    Some("off") => false,
                    Some("on") => true,
                    _ => invocation.flags.fp_contract,
                };
            }
            // `-sym on` emits CodeWarrior `.line` and `.debug` sections. Keep
            // last-wins behavior even while object-level debug emission is a
            // deliberate capability boundary.
            "-sym" => {
                index += 1;
                invocation.flags.debug_info = match arguments.get(index).map(String::as_str) {
                    Some("on") => true,
                    Some("off") => false,
                    _ => invocation.flags.debug_info,
                };
            }
            // `-ipa file` enables whole-file optimization. Treat each
            // occurrence as a complete mode so a later `-ipa off` wins.
            "-ipa" => {
                index += 1;
                invocation.flags.ipa_file = match arguments.get(index).map(String::as_str) {
                    Some("file") => true,
                    Some("off") => false,
                    _ => invocation.flags.ipa_file,
                };
            }
            // `-opt off` is the long spelling used by debug project variants
            // after an earlier `-O4,p`. It wins in command-line order and
            // selects the same unoptimized schedules as `-O0`.
            "-opt" => {
                index += 1;
                if arguments.get(index).map(String::as_str) == Some("off") {
                    invocation.flags.optimization = Optimization::O0;
                }
            }
            // `-func_align N` overrides the build-default code alignment. The
            // project configurations use byte alignments (currently 4 or 32).
            "-func_align" => {
                index += 1;
                if let Some(alignment) = arguments
                    .get(index)
                    .and_then(|value| value.parse::<u32>().ok())
                    .filter(|value| value.is_power_of_two())
                {
                    invocation.flags.function_alignment = Some(alignment);
                }
            }
            // `-sdata N`: zero disables writable SDA (r13); a later non-zero
            // threshold turns it back on. Keep it independent from `-sdata2`.
            "-sdata" => {
                index += 1;
                if let Some(threshold) = arguments
                    .get(index)
                    .and_then(|value| value.parse::<u32>().ok())
                {
                    invocation.flags.global_addressing = if threshold == 0 {
                        GlobalAddressing::Absolute
                    } else {
                        GlobalAddressing::SmallData
                    };
                }
            }
            // `-sdata2 N` is the corresponding independent read-only SDA2 (r2)
            // threshold. Model zero versus non-zero until exact numeric threshold
            // selection is needed by a measured object.
            "-sdata2" => {
                index += 1;
                if let Some(threshold) = arguments
                    .get(index)
                    .and_then(|value| value.parse::<u32>().ok())
                {
                    invocation.flags.read_only_global_addressing = if threshold == 0 {
                        GlobalAddressing::Absolute
                    } else {
                        GlobalAddressing::SmallData
                    };
                }
            }
            // `-O0,p` .. `-O4,s`: the level and the performance/size objective
            // are independently observable. A spelling without a suffix resets
            // the objective to mwcc's performance default.
            argument if argument.starts_with("-O") && argument.len() >= 3 => {
                invocation.flags.optimization = match argument.as_bytes()[2] {
                    b'0' => Optimization::O0,
                    b'1' => Optimization::O1,
                    b'2' => Optimization::O2,
                    b'3' => Optimization::O3,
                    _ => Optimization::O4,
                };
                invocation.flags.optimization_goal = match argument.split_once(',') {
                    Some((_, "s" | "space")) => mwcc_versions::OptimizationGoal::Size,
                    _ => mwcc_versions::OptimizationGoal::Performance,
                };
            }
            argument if argument.starts_with("-lang=") => {
                if let Some(language) = SourceLanguage::parse(&argument[6..]) {
                    invocation.source_language = Some(language);
                }
            }
            argument if argument.starts_with("-I") && argument.len() > 2 => {
                invocation.include_paths.push(PathBuf::from(&argument[2..]));
            }
            argument if argument.starts_with("-D") && argument.len() > 2 => {
                apply_macro_definition(
                    &mut invocation.preprocessor_definitions,
                    &argument[2..],
                );
            }
            argument if argument.starts_with("-U") && argument.len() > 2 => {
                invocation.preprocessor_definitions.remove(&argument[2..]);
            }
            argument if argument.ends_with(".c") && invocation.input.is_none() => {
                invocation.input = Some(argument.to_string());
            }
            _ => {} // ignore flags we do not yet model
        }
        index += 1;
    }
    invocation
}

fn apply_macro_definition(
    definitions: &mut std::collections::HashMap<String, String>,
    definition: &str,
) {
    let (name, value) = definition.split_once('=').unwrap_or((definition, "1"));
    if !name.is_empty() {
        definitions.insert(name.to_string(), value.to_string());
    }
}

fn source_is_cxx(source_name: &str, source_language: Option<SourceLanguage>) -> bool {
    match source_language {
        Some(SourceLanguage::C) => false,
        Some(SourceLanguage::Cxx) => true,
        None => matches!(
            std::path::Path::new(source_name)
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("cpp" | "cp" | "cxx" | "cc")
        ),
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let invocation = parse_invocation(&arguments);

    let Some(input) = invocation.input else {
        eprintln!("mwcc: no input file (expected -c <file.c>)");
        return ExitCode::FAILURE;
    };
    let output = invocation.output.unwrap_or_else(|| {
        let stem = input.strip_suffix(".c").unwrap_or(&input);
        format!("{stem}.o")
    });

    let build = match invocation.build_label {
        Some(ref label) => match mwcc_versions::by_label(label).or_else(|| {
            std::env::var_os("MWCC_EXPERIMENTAL_BUILDS")
                .is_some()
                .then(|| mwcc_versions::by_label_experimental(label))
                .flatten()
        }) {
            Some(build) => build,
            None => {
                eprintln!("mwcc: unknown compiler build '{label}'");
                return ExitCode::FAILURE;
            }
        },
        None => mwcc_versions::DEFAULT,
    };

    let mut source_loader = mwcc_source_loader::SourceLoader::new(invocation.include_paths);
    source_loader.define("__MWERKS__", "1");
    if source_is_cxx(&input, invocation.source_language) {
        source_loader.define("__cplusplus", "1");
    } else {
        source_loader.define("__STDC__", "1");
    }
    for (name, value) in invocation.preprocessor_definitions {
        source_loader.define(name, value);
    }
    let source = match source_loader.load(std::path::Path::new(&input)) {
        Ok(source) => source,
        Err(diagnostic) => {
            eprintln!("mwcc: {diagnostic}");
            return ExitCode::FAILURE;
        }
    };

    let source_name = std::path::Path::new(&input)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&input);

    let config = mwcc_versions::CompilerConfig {
        build,
        flags: invocation.flags,
    };
    match compile(
        &source,
        source_name,
        config,
        invocation.source_language,
        invocation.artifacts_directory.as_deref(),
        invocation.parity_keep_going,
    ) {
        Ok(object) => {
            if let Err(error) = std::fs::write(&output, object) {
                eprintln!("mwcc: cannot write {output}: {error}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(diagnostic) => {
            eprintln!("mwcc: {diagnostic}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GlobalAlignments {
    layout: u32,
    comment: u32,
}

/// Resolve the two alignment domains MWCC assigns to an aggregate. At O0,
/// scalar arrays are word-aligned in their section while `.comment` retains
/// the element alignment. Optimized builds record the storage alignment.
fn global_alignments(
    element_size: u32,
    struct_alignment: Option<u32>,
    is_array: bool,
    is_read_only: bool,
    requested_alignment: u32,
    unoptimized: bool,
    large_aggregate_comment_alignment: u32,
) -> GlobalAlignments {
    let layout = match struct_alignment {
        // Arrays of aggregate elements receive MWCC's minimum word object
        // alignment even when the aggregate itself contains only byte members.
        // A scalar packed/byte-only aggregate retains its natural alignment.
        Some(alignment) if is_array => alignment.max(4),
        // A struct object uses the alignment established by its layout. This is
        // not necessarily word alignment: packed/byte-only aggregates and
        // compiler-generated C++ type-name records can legitimately align 1.
        Some(alignment) => alignment,
        None if is_array => element_size.max(4),
        None => element_size,
    }
    .max(requested_alignment);
    let comment = if unoptimized && struct_alignment.is_none() {
        element_size.max(requested_alignment)
    } else if !is_read_only
        && struct_alignment.is_some_and(|alignment| alignment >= 4)
        && element_size > 8
    {
        // A build may give writable full-section aggregate symbols a metadata
        // alignment larger than their actual member layout. Read-only
        // aggregates retain their declared alignment even in `.rodata`.
        // Keep that convention in the build profile rather than inferring it
        // from source language here.
        layout.max(large_aggregate_comment_alignment)
    } else {
        layout
    };
    GlobalAlignments { layout, comment }
}

/// Run the full pipeline, optionally dumping a per-phase artifact report.
fn compile(
    source: &[u8],
    source_name: &str,
    config: mwcc_versions::CompilerConfig,
    source_language: Option<SourceLanguage>,
    artifacts: Option<&str>,
    parity_keep_going: bool,
) -> Compilation<Vec<u8>> {
    let located_tokens = mwcc_source_to_tokens::tokenize_bytes_located(source)?;
    let tokens: Vec<mwcc_tokens::Token> = located_tokens
        .iter()
        .map(|located| located.token.clone())
        .collect();
    if let Some(directory) = artifacts {
        write_token_artifacts(directory, config, &tokens);
    }
    let behavior = mwcc_versions::Behavior::resolve(&config);
    let is_cxx = source_is_cxx(source_name, source_language);
    let mut unit = mwcc_tokens_to_syntax_trees::parse_located_translation_unit_with_behavior(
        located_tokens,
        is_cxx,
        config.char_is_signed(),
        behavior.plain_inline_localstatic_base,
        behavior.skipped_static_inline_label_base,
        behavior.skipped_plain_inline_label_base,
        behavior.skipped_function_template_label_base,
        behavior.dropped_inline_parameter_label_weight,
        behavior.dropped_inline_local_declaration_label_weight,
        behavior.dropped_inline_const_local_declaration_label_weight,
        behavior.dropped_inline_class_automatic_label_base,
        behavior.dropped_inline_class_automatic_label_weight,
        behavior.anonymous_aggregate_definition_label_weight,
        behavior.nested_anonymous_aggregate_definition_label_weight,
        config.flags.enum_storage == mwcc_versions::EnumStorage::Minimum,
    )?;
    if is_cxx && config.flags.rtti {
        mwcc_tokens_to_syntax_trees::materialize_cxx_rtti(&mut unit);
    }
    let mut disabled_inline_materializations = std::collections::HashSet::new();
    if !config.flags.inline_enabled {
        let referenced = reference_analysis::referenced_disabled_inlines(&unit);
        let mut materialized = Vec::new();
        for definition in &unit.skipped_inline_definitions {
            if !referenced.contains(&definition.name) {
                continue;
            }
            let mut definition = definition.clone();
            if !definition.is_static {
                definition.is_weak = true;
                if !unit.weak_materialized.contains(&definition.name) {
                    unit.weak_materialized.push(definition.name.clone());
                }
            }
            unit.skipped_inline_names.remove(&definition.name);
            disabled_inline_materializations.insert(definition.name.clone());
            materialized.push(definition);
        }
        unit.functions.extend(materialized);
        // These fallback symbol lists describe helpers dropped by automatic
        // inlining. Disabled inlining instead emits every reachable recovered
        // definition above; retaining the fallbacks would duplicate or invent
        // undefined symbols.
        unit.inline_asm_symbols.clear();
        unit.plain_inline_asm_helpers.clear();
    }
    // Every callable's return type (prototypes + this unit's definitions) so a
    // call's result type is known during lowering.
    let call_return_types: std::collections::HashMap<String, mwcc_syntax_trees::Type> = unit
        .prototypes
        .iter()
        .map(|(name, return_type, _)| (name.clone(), *return_type))
        .chain(
            unit.functions
                .iter()
                .map(|function| (function.name.clone(), function.return_type)),
        )
        .chain(
            unit.skipped_inline_definitions
                .iter()
                .map(|function| (function.name.clone(), function.return_type)),
        )
        .chain(
            unit.skipped_inline_signatures
                .iter()
                .map(|(name, return_type, _)| (name.clone(), *return_type)),
        )
        .collect();
    // Every callable's parameter types (prototypes + definitions) so a call places
    // each argument in the register the parameter's type requires (int vs float).
    let call_parameter_types: std::collections::HashMap<String, Vec<mwcc_syntax_trees::Type>> =
        unit.prototypes
            .iter()
            .map(|(name, _, parameter_types)| (name.clone(), parameter_types.clone()))
            .chain(unit.functions.iter().map(|function| {
                (
                    function.name.clone(),
                    function
                        .parameters
                        .iter()
                        .map(|parameter| parameter.parameter_type)
                        .collect(),
                )
            }))
            .chain(unit.skipped_inline_definitions.iter().map(|function| {
                (
                    function.name.clone(),
                    function
                        .parameters
                        .iter()
                        .map(|parameter| parameter.parameter_type)
                        .collect(),
                )
            }))
            .chain(unit.skipped_inline_signatures.iter().map(
                |(name, _, parameter_types)| (name.clone(), parameter_types.clone()),
            ))
            .collect();
    // An IMPLICITLY-materialized inline (ww uart) was unknown at its call
    // sites: mwcc compiled those calls under the K&R implicit-int rule and
    // classified the callee as an implicit external (the UND ghost). Drop the
    // definition from the callable maps so the lowering sees what mwcc saw.
    let call_return_types: std::collections::HashMap<_, _> = call_return_types
        .into_iter()
        .filter(|(name, _)| {
            !unit
                .implicitly_materialized
                .iter()
                .any(|materialized| materialized == name)
        })
        .collect();
    let call_parameter_types: std::collections::HashMap<_, _> = call_parameter_types
        .into_iter()
        .filter(|(name, _)| {
            !unit
                .implicitly_materialized
                .iter()
                .any(|materialized| materialized == name)
        })
        .collect();
    // Prototype-only names (file-scope declarations, NOT definitions) — the
    // implicit-callee classifier keys on these: a call with no prototype is
    // implicit even when the unit defines the callee later.
    // Under automatic inlining, a weak materialization may still be re-inlined
    // at a native caller and needs measured call-site policy. Final `-inline off`
    // makes every such edge an ordinary direct call; retain the unit list for
    // weak-inline object flags, but do not route those callers through the
    // automatic-inlining safety guard.
    let weak_materialized_names: std::collections::HashSet<String> = if config
        .flags
        .inline_enabled
    {
        unit.weak_materialized.iter().cloned().collect()
    } else {
        std::collections::HashSet::new()
    };
    let prototyped_names: std::collections::HashSet<String> = unit
        .prototypes
        .iter()
        .map(|(name, _, _)| name.clone())
        .chain(unit.cxx_declared_function_names.iter().cloned())
        .collect();
    let inline_summaries = if config.flags.inline_enabled {
        mwcc_syntax_trees_to_machine_code::InlineSummaries::analyze_with_skipped(
            &unit.functions,
            &unit.skipped_inline_definitions,
        )
    } else {
        mwcc_syntax_trees_to_machine_code::InlineSummaries::default()
    };
    let inline_bodies = if config.flags.inline_enabled {
        mwcc_syntax_trees_to_machine_code::InlineBodySet::analyze_with_definitions(
            &unit.functions,
            &unit.skipped_inline_definitions,
        )
    } else {
        mwcc_syntax_trees_to_machine_code::InlineBodySet::default()
    };
    let materialized_inline_names: std::collections::HashSet<String> = unit
        .materialized_inline_candidates
        .iter()
        .cloned()
        .collect();
    let referenced_materialized_inlines =
        reference_analysis::referenced_function_candidates(&unit, &materialized_inline_names);
    // Lower every function definition in source order; they share one object.
    let diagnose_function = std::env::var_os("MWCC_DIAGNOSTIC_FUNCTION").is_some();
    let diagnose_syntax_tree = std::env::var_os("MWCC_DIAGNOSTIC_SYNTAX_TREE").is_some();
    if diagnose_syntax_tree {
        eprintln!(
            "cxx-inline-ordinal-facts {:#?}",
            unit.cxx_inline_ordinal_facts
        );
        eprintln!("skipped-inline-ordinal-total {}", unit.skipped_inline_functions);
        eprintln!(
            "materialized-inline-candidates {:#?}",
            unit.materialized_inline_candidates
        );
        eprintln!("referenced-materialized-inlines {referenced_materialized_inlines:#?}");
        eprintln!("skipped-inline-names {:#?}", unit.skipped_inline_names);
        for function in &unit.skipped_inline_definitions {
            eprintln!("skipped-inline {function:#?}");
        }
    }
    let mut machine_functions: Vec<mwcc_machine_code::MachineFunction> =
        Vec::with_capacity(unit.functions.len());
    for (function_index, function) in unit.functions.iter().enumerate() {
        if config.flags.whole_file_optimization_enabled()
            && function.is_static
            && inline_summaries.should_elide_ipa_function(&function.name)
        {
            continue;
        }
        // The call-count heuristic speculatively materializes a static inline
        // before later source items are known. A candidate outside the rooted
        // reference graph never exists in MWCC's object and need not lower.
        if function.text_deferred && !referenced_materialized_inlines.contains(&function.name) {
            continue;
        }
        // A referenced terminal candidate can still disappear when lowering
        // consumes every earlier call. Keep it if even one Rel24 survives.
        if function.text_deferred
            && function_index + 1 == unit.functions.len()
            && unit
                .materialized_inline_candidates
                .iter()
                .any(|name| name == &function.name)
            && function_order::terminal_implicit_inline_is_consumed(
                &function.name,
                &machine_functions,
            )
        {
            continue;
        }
        if diagnose_syntax_tree {
            eprintln!("{function:#?}");
        }
        let mut function_config = config;
        if let Some(enabled) = unit
            .function_cpp_exception_overrides
            .get(&function.name)
        {
            function_config.flags.cpp_exceptions = *enabled;
        }
        match mwcc_syntax_trees_to_machine_code::lower_function(
            function,
            &unit.globals,
            &unit.aggregate_definitions,
            &unit.function_return_aggregate_tags,
            &call_return_types,
            &call_parameter_types,
            &unit.skipped_inline_names,
            &weak_materialized_names,
            &prototyped_names,
            &unit.variadic_definitions,
            &unit.fixed_address_arrays,
            &unit.fixed_address_objects,
            &inline_bodies,
            &inline_summaries,
            unit.inline_expansion_facts
                .get(&function.name)
                .copied()
                .unwrap_or_default(),
            &unit.function_return_fundamentals,
            function_config,
        ) {
            Ok(machine_function) => machine_functions.push(machine_function),
            Err(mut diagnostic) => {
                // Whole-TU parity sweeps bucket the stable reason. Feature work can opt
                // into the failing function name without changing the default output.
                if diagnose_function {
                    diagnostic
                        .message
                        .push_str(&format!(" (while lowering '{}')", function.name));
                }
                if parity_keep_going {
                    eprintln!(
                        "mwcc: parity skipped function '{}': {}",
                        function.name, diagnostic
                    );
                    continue;
                }
                return Err(diagnostic);
            }
        }
    }
    mwcc_syntax_trees_to_machine_code::apply_unit_ordinal_accounting(
        &unit.functions,
        &mut machine_functions,
        config,
    );
    machine_functions.extend(
        mwcc_syntax_trees_to_machine_code::lower_vtable_adjustor_thunks(
            &unit.globals,
            &unit.cxx_class_declaration_order,
        )?,
    );
    function_order::interleave_disabled_inline_materializations(
        &mut machine_functions,
        &disabled_inline_materializations,
    );
    // Mixed code payloads and relocations are modeled below. Debug lowering
    // owns its own section-aware support boundary; mwcats still has only one
    // catalog payload, so retain that narrower byte-exact-or-defer boundary.
    let code_sections: std::collections::HashSet<&str> = machine_functions
        .iter()
        .map(|function| function.section.as_deref().unwrap_or(".text"))
        .collect();
    if code_sections.len() > 1 && config.flags.emit_mwcats {
        return Err(Diagnostic::error(
            "mwcats for mixed function code sections needs per-section catalogs (roadmap)",
        ));
    }
    // MWCC_DUMP_FIXTURES=<dir>: serialize every lowered function's register
    // structure (per-instruction define/use operands via the vreg machine
    // description) — the FIT CORPUS for the keystone allocator (#20). Each
    // byte-verified function is a known-good whole-function register map.
    if let Some(directory) = std::env::var_os("MWCC_DUMP_FIXTURES") {
        let directory = std::path::PathBuf::from(directory);
        let _ = std::fs::create_dir_all(&directory);
        for function in &machine_functions {
            let mut out = String::new();
            out.push_str(&format!(
                "fn {} instrs={}
",
                function.name,
                function.instructions.len()
            ));
            for (index, instruction) in function.instructions.iter().enumerate() {
                let mut clone = instruction.clone();
                let debug = format!("{instruction:?}");
                let mnemonic = debug.split([' ', '{']).next().unwrap_or("?");
                let mut operands = Vec::new();
                mwcc_vreg::for_each_register(&mut clone, |role, class, register| {
                    let role = match role {
                        mwcc_vreg::RegisterRole::Define => "D",
                        mwcc_vreg::RegisterRole::Use => "U",
                    };
                    let class = match class {
                        mwcc_vreg::Class::General => "G",
                        mwcc_vreg::Class::Float => "F",
                    };
                    operands.push(format!("{role} {class} {register}"));
                });
                let call = matches!(
                    instruction,
                    mwcc_machine_code::Instruction::BranchAndLink { .. }
                        | mwcc_machine_code::Instruction::BranchToCountRegisterAndLink
                );
                out.push_str(&format!(
                    "{index} {mnemonic}{} | {}
",
                    if call { " CALL" } else { "" },
                    operands.join(" | ")
                ));
            }
            // Same-named variants across projects carry different bodies —
            // key the file by a content hash so none clobber.
            let digest = {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                out.hash(&mut hasher);
                hasher.finish()
            };
            let file = directory.join(format!(
                "{}_{digest:016x}.fixture",
                function.name.replace(['/', ':'], "_")
            ));
            let _ = std::fs::write(file, out);
        }
    }
    // Each SKIPPED inline function definition advanced mwcc's `@N` counter by 3
    // (compiled, then dropped) before the real functions were numbered — pre-bump
    // the first function's block (measured: math.h's fabs helper shifts s_frexp's
    // pool constant from @11 to @14).
    // Real functions' STATIC LOCALS become LOCAL data objects keyed by their
    // raw names; the writer numbers each off its owner's @N sequence and
    // displays `name$K`.
    for function in machine_functions.iter_mut() {
        if unit
            .implicitly_materialized
            .iter()
            .any(|name| *name == function.name)
        {
            function.implicit_materialized = true;
        }
        if unit
            .weak_materialized
            .iter()
            .any(|name| *name == function.name)
        {
            function.weak_inline = true;
        }
    }
    let read_only_small_data =
        config.flags.read_only_global_addressing == mwcc_versions::GlobalAddressing::SmallData;
    let mut static_local_globals: Vec<mwcc_machine_code_to_object::DefinedGlobal> = Vec::new();
    let cxx_inline_facts = unit.cxx_inline_ordinal_facts;
    let cxx_inline_bump = cxx_inline_facts.class_definitions
        * usize::from(behavior.cxx_class_definition_label_bump)
        + cxx_inline_facts.inline_definitions
            * usize::from(behavior.cxx_inline_definition_label_bump)
        + cxx_inline_facts.inline_definitions
            * usize::from(behavior.deferred_cxx_inline_definition_label_bump)
        + cxx_inline_facts.inline_definition_parameters
            * usize::from(behavior.dropped_inline_parameter_label_weight)
        + cxx_inline_facts.inline_definition_local_declarators
            * usize::from(behavior.dropped_inline_local_declaration_label_weight)
        + cxx_inline_facts.control_flow_labels
            * usize::from(behavior.cxx_inline_control_flow_label_weight)
        + cxx_inline_facts.nonvirtual_destructors
            * usize::from(behavior.cxx_nonvirtual_destructor_label_bump)
        + cxx_inline_facts.nonvirtual_destructors
            * usize::from(behavior.deferred_cxx_nonvirtual_destructor_label_bump)
        + cxx_inline_facts.trivial_class_temporary_constructions
            * usize::from(behavior.cxx_trivial_class_temporary_label_bump)
        + cxx_inline_facts.nontrivial_class_temporary_constructions
            * usize::from(behavior.cxx_nontrivial_class_temporary_label_bump)
        + cxx_inline_facts.virtual_destructors
            * usize::from(behavior.cxx_virtual_destructor_label_bump)
        + cxx_inline_facts.direct_calls * usize::from(behavior.cxx_inline_ipa_call_label_bump);
    let cxx_analysis_residues = is_cxx
        .then(|| {
            cxx_analysis_residues::recognize(
                &unit,
                &machine_functions,
                config.build.label,
                config.flags.optimization,
            )
        })
        .flatten();
    let prototype_name_bump = if config
        .build
        .profile
        .prototype_parameter_names_consume_labels()
    {
        unit.named_prototype_parameters
    } else {
        0
    };
    let unit_declaration_bump = if cxx_analysis_residues.is_some() {
        // The capture carries the optimizer walk's observable sparse ordinals
        // directly. Reapplying the aggregate dropped-inline estimate would
        // charge the same analysis a second time before emitted functions.
        0
    } else {
        cxx_inline_bump + prototype_name_bump
    };
    let cxx_rtti_prior_declaration_bump = if cxx_analysis_residues.is_some() {
        0
    } else {
        unit.skipped_inline_functions
            + cxx_inline_facts.inline_definitions
                * usize::from(behavior.cxx_rtti_inline_definition_label_bump)
            + cxx_inline_facts.control_flow_labels
                * usize::from(behavior.cxx_inline_control_flow_label_weight)
            + cxx_inline_facts.inline_definition_local_declarators
                * usize::from(behavior.dropped_inline_local_declaration_label_weight)
            + cxx_inline_facts.nonvirtual_destructors
                * usize::from(behavior.cxx_nonvirtual_destructor_label_bump)
            + cxx_inline_facts.trivial_class_temporary_constructions
                * usize::from(behavior.cxx_trivial_class_temporary_label_bump)
            + cxx_inline_facts.nontrivial_class_temporary_constructions
                * usize::from(behavior.cxx_nontrivial_class_temporary_label_bump)
            + cxx_inline_facts.virtual_destructors
                * usize::from(behavior.cxx_virtual_destructor_label_bump)
            + cxx_inline_facts.direct_calls
                * usize::from(behavior.cxx_inline_ipa_call_label_bump)
            + prototype_name_bump
    };
    // Static-local positional samples currently track skipped-inline cost.
    // Prototype-name provenance is unit-wide but not yet sampled at each local
    // declaration, so do not fold it into this separate adjustment channel.
    let total_inline_bump = unit.skipped_inline_functions as i64;
    for (function_index, function) in machine_functions.iter().enumerate() {
        for local in &function.static_locals {
            // A static numbers at the counter AS OF ITS DECLARATION (the parser's
            // positional sample). The whole-unit pre-bump folds into the FIRST
            // function's block below, so a first-function static shifts by its
            // full prebump; a later owner's running counter already includes the
            // total, leaving only the (typically zero) difference.
            let anonymous_adjust = match unit.static_local_prebumps.get(&local.name) {
                Some(&prebump) if function_index == 0 => prebump as i64,
                Some(&prebump) => prebump as i64 - total_inline_bump,
                None => 0,
            } + function.static_local_adjust;
            static_local_globals.push(mwcc_machine_code_to_object::DefinedGlobal {
                anonymous_adjust,
                static_local_owner: Some(function_index),
                is_weak: false,
                functions_before: 0,
                non_static_functions_before: 0,
                name: local.name.clone(),
                size: local.size,
                alignment: local.alignment,
                comment_alignment: local.alignment,
                initial_bytes: local.initial_bytes.clone(),
                is_const: local.is_const,
                force_full_data_section: local.is_const && !read_only_small_data,
                is_static: true,
                is_explicit_zero: false,
                preassigned_anonymous_ordinal: None,
                relocations: local
                    .relocations
                    .iter()
                    .map(
                        |(offset, target, addend)| mwcc_machine_code_to_object::DataRelocation {
                            offset: *offset,
                            target: target.clone(),
                            addend: *addend,
                        },
                    )
                    .collect(),
                section: None,
            });
        }
    }
    let leading_source_ordinal_bump = if cxx_analysis_residues.is_none() {
        inline_ordinal_positions::distribute(
            &mut machine_functions,
            &unit.function_inline_prebumps,
            unit.skipped_inline_functions,
        )
    } else {
        0
    };
    if diagnose_syntax_tree {
        eprintln!("leading-source-ordinal-bump {leading_source_ordinal_bump}");
        for function in &machine_functions {
            eprintln!(
                "machine-ordinal-facts {}: front={}, source_prefix={}, post={:?}, framed={}",
                function.name,
                function.anonymous_label_bump,
                function.deferred_source_prefix_bump,
                function.post_function_anonymous_bump,
                function.frame.is_some()
            );
        }
    }
    // Deferred inlining has its own translation-unit emission schedule. Keep the
    // policy isolated from lowering and object layout: both consume its result.
    if config.flags.inline_deferred {
        function_order::apply_deferred_emission_order(
            &mut machine_functions,
            behavior.deferred_source_function_label_bump,
            behavior.deferred_post_function_label_bump,
            behavior.deferred_function_emission_style,
            &unit.immediate_weak_materializations,
        );
    }
    // `#pragma defer_codegen on` defers the covered functions the same way:
    // they emit LAST, in REVERSE definition order (measured: melee mem_funcs,
    // where the pragma precedes every function and the whole .text reverses).
    if !unit.deferred_function_names.is_empty() {
        let (kept, mut deferred): (Vec<_>, Vec<_>) =
            machine_functions.drain(..).partition(|function| {
                !unit
                    .deferred_function_names
                    .iter()
                    .any(|name| *name == function.name)
            });
        deferred.reverse();
        machine_functions = kept;
        machine_functions.extend(deferred);
    }
    if let Some(first) = machine_functions.first_mut() {
        first.anonymous_label_bump += leading_source_ordinal_bump;
    }
    if let Some(first) = machine_functions.first_mut() {
        // File-scope declarations advance the unit-wide ordinal stream before
        // the first EMITTED compiled body. Attach after emission scheduling so
        // deferred reverse order does not strand the provenance on the tail.
        if config.build.version == (4, 1, 0)
            && config.flags.debug_info
            && config.flags.rtti
            && cxx_rtti_names::is_single_fragmented_debug_class(cxx_inline_facts)
        {
            first.fragmented_debug_anonymous_bump += unit_declaration_bump as u32;
        } else {
            first.anonymous_label_bump += unit_declaration_bump as u32;
        }
    }
    // File-scope variables defined here (not `extern`/`static`). A writable global
    // lands in `.sdata` (initialized) or `.sbss` (zero); a `const` one is read-only
    // and lands in `.sdata2` (≤ 8 bytes) or `.rodata` (larger). Declaration order is
    // kept so the writer can lay each section out the way mwcc does.
    let serialize = |values: &[i64], element_size: u32, size: u32| -> Vec<u8> {
        let mut bytes = vec![0u8; size as usize];
        for (index, &value) in values.iter().enumerate() {
            let start = index * element_size as usize;
            // An initializer may overrun the object (a char array shorter than its
            // string literal, e.g. `char s[2] = "hi"` keeps "hi", drops the NUL).
            if start + element_size as usize > bytes.len() {
                break;
            }
            let encoded = (value as u64).to_be_bytes();
            bytes[start..start + element_size as usize]
                .copy_from_slice(&encoded[8 - element_size as usize..]);
        }
        bytes
    };
    let small_data = config.flags.global_addressing == mwcc_versions::GlobalAddressing::SmallData;
    // A large (> 8 byte) writable global shares `.data`/`.bss` with any dense-switch
    // jump table; the two layouts aren't reconciled yet, so a jump table forces such
    // globals to keep deferring (be dropped).
    let has_jump_table = machine_functions
        .iter()
        .any(|function| !function.jump_tables.is_empty());
    let analysis_counter_floor = cxx_analysis_residues
        .as_ref()
        .map_or(0, |capture| capture.next_anonymous_ordinal);
    let analysis_upfront_globals = cxx_analysis_residues
        .as_ref()
        .map_or(&[][..], |capture| capture.force_upfront_globals);
    let mut defined_globals: Vec<mwcc_machine_code_to_object::DefinedGlobal> =
        cxx_analysis_residues.map_or_else(Vec::new, |capture| capture.objects);
    // Distinct pooled string literals, by bytes, to their anonymous `@N` name, and
    // the running `@N` counter — deduplicated across the unit (mwcc `-str reuse`).
    let mut string_pool: std::collections::HashMap<Vec<u8>, String> =
        std::collections::HashMap::new();
    let mut string_counter: u32 = 0;
    // File-scope strings declared BETWEEN functions: (functions_before,
    // placeholder) — numbered in the resolver walk at their source position.
    let mut pending_file_strings: Vec<(usize, String)> = Vec::new();
    // Strings pooled from STRUCT-member relocations collect here per global (the
    // enclosing push borrows `defined_globals`), then append after it.
    let mut pooled_string_globals: Vec<mwcc_machine_code_to_object::DefinedGlobal> = Vec::new();
    // A static declared AFTER the last function still emits UP FRONT (measured:
    // bfbb's plain `static void* const __destroy_global_chain_reference` in
    // .sdata2 with no section attribute); only a declaration strictly BETWEEN
    // functions interleaves at its source position (strikers' `unused`).
    let source_function_count = unit.functions.len();
    for global in &unit.globals {
        // Only a PLAIN static (no section attribute) normalizes — the measured
        // case is bfbb's tail `static void* const` reference. Section-attributed
        // (.ctors/.dtors) and exported globals keep their source position (the
        // fire-678 interleave, canary 1150).
        let clamp_tail = global.is_static
            && global.section.is_none()
            && global.functions_before >= source_function_count;
        let force_upfront = analysis_upfront_globals.contains(&global.name.as_str());
        let global = &mwcc_syntax_trees::GlobalDeclaration {
            non_static_functions_before: if force_upfront {
                0
            } else {
                global.non_static_functions_before
            },
            functions_before: if clamp_tail || force_upfront {
                0
            } else {
                global.functions_before
            },
            ..global.clone()
        };
        if !global.is_data_definition()
            || matches!(global.declared_type, mwcc_syntax_trees::Type::Void)
        {
            continue;
        }
        let force_full_data_section = global.section.is_none()
            && ((behavior.inferred_array_uses_full_data_section && global.array_length_inferred)
                || (global.is_const && !read_only_small_data));
        // A `static const` SCALAR is folded into its readers (or elided when unused),
        // so keep dropping it. A `static const` ARRAY can't be folded into a register —
        // mwcc emits it to `.rodata` with a LOCAL symbol — so let it fall through to the
        // const-data path (which now binds it LOCAL via `global.is_static`).
        if global.is_static
            && global.is_const
            && global.array_length.is_none()
            && global.address_initializer.is_none()
        {
            let kept = machine_functions.iter().any(|function| {
                function
                    .keep_named_const_scalars
                    .iter()
                    .any(|name| name == &global.name)
            });
            if !kept {
                continue;
            }
        }
        // A pointer global initialized with addresses (`int *p = &g;`, a string
        // `char *s = "…"`, or a `{…}` table): four zero bytes per element in
        // `.sdata`, each non-null element an ADDR32 relocation. A string element is
        // pooled — its bytes (plus NUL) become an anonymous local `@N` object, emitted
        // just before the pointer that first uses it, deduplicated across the unit.
        if let Some(elements) = &global.address_initializer {
            use mwcc_syntax_trees::PointerElement;
            // A `static const` pointer-to-symbol global (`static void* const p = &f;`,
            // e.g. the runtime's global-destructor reference when __declspec is macro'd
            // away) binds LOCAL in `.sdata2` with an ADDR32 — handled by the const/static
            // routing below. A non-const, non-static writable pointer array is the
            // original `.sdata` case. A section override handles its own placement.
            // ... or a single STRING (`static const char* const unused = "…"` —
            // ansi_fp's strikers revision): the pointer routes `.sdata2` LOCAL like
            // the symbol case, the string pools as a file-scope `@N` (measured:
            // `unused` l O .sdata2 4B, its string @229 l O .data).
            let single_target = global.array_length.is_none()
                && matches!(
                    elements.as_slice(),
                    [PointerElement::Symbol(_)] | [PointerElement::Str(_)]
                );
            // A static pointer initialized to NULL (`static T* p = 0;` — fstload's
            // idTmp/bb2) is an all-zero object: it routes to `.sbss` like any zero
            // global, LOCAL, no relocation. Only relocated/valued static pointers
            // still defer.
            let all_null = global.array_length.is_none()
                && elements
                    .iter()
                    .all(|element| matches!(element, PointerElement::Null));
            // (A table forward-referencing unit functions is handled by the writer's
            // symbol-order hoist: the address-taken functions' GLOBAL FUNC symbols
            // emit at the data object's position, reverse-slot first-seen — measured
            // `{e1,e2}` -> tbl,e2,e1; shuffled `{e2,e1,e3}` -> tbl,e3,e1,e2; a
            // duplicated element hoists once by its LAST slot.)
            // A `static` (non-const) function-pointer ARRAY can target unit functions
            // or declared extern functions. The writer binds the table LOCAL and owns
            // both the hoisted defined-function ordering and the relocated UND-symbol
            // first-use ordering. CONST tables (.sdata2/.rodata) keep the defer.
            let static_function_table = global_initializers::private_function_table(
                global,
                elements,
                &machine_functions,
                &prototyped_names,
            );
            let static_unit_data_table =
                global_initializers::private_unit_data_table(global, elements, &unit.globals);
            let static_string_table =
                global_initializers::private_string_table(global, elements);
            if (global.is_static || global.is_const)
                && global.section.is_none()
                && !single_target
                && !all_null
                && !static_function_table
                && !static_unit_data_table
                && !static_string_table
            {
                if parity_keep_going {
                    eprintln!(
                        "mwcc: parity skipped global '{}': a static/const pointer-address global is not supported yet (roadmap)",
                        global.name
                    );
                    continue;
                }
                return Err(Diagnostic::error(
                    "a static/const pointer-address global is not supported yet (roadmap)",
                ));
            }
            // A struct-table initializer (declared type is a struct) has one element
            // per FIELD, so its slot count is the flattened length; a plain pointer
            // array's length is the (possibly partially initialized) array length.
            let count = if matches!(global.declared_type, mwcc_syntax_trees::Type::Struct { .. }) {
                elements.len() as u32
            } else {
                global
                    .array_length
                    .map(u32::from)
                    .unwrap_or(elements.len() as u32)
            };
            let size = count * 4;
            let mut bytes = vec![0u8; size as usize];
            let mut relocations = Vec::new();
            for (index, element) in elements.iter().enumerate() {
                let offset = index as u32 * 4;
                let target = match element {
                    PointerElement::Null => continue,
                    // A scalar field is literal bytes, not a relocation.
                    PointerElement::Scalar(value) => {
                        bytes[offset as usize..offset as usize + 4]
                            .copy_from_slice(&(*value as u32).to_be_bytes());
                        continue;
                    }
                    PointerElement::Symbol(name) => name.clone(),
                    PointerElement::Str(string_bytes) => {
                        string_pool
                            .get(string_bytes.as_slice())
                            .cloned()
                            .unwrap_or_else(|| {
                                // Declared BETWEEN functions: the string numbers
                                // IN-STREAM at its source position (assigned in the
                                // resolver walk below via a placeholder). Up-front
                                // declarations keep the eager number.
                                let name = if global.functions_before > 0 {
                                    let placeholder =
                                        format!("@@file{}", pending_file_strings.len());
                                    pending_file_strings
                                        .push((global.functions_before, placeholder.clone()));
                                    placeholder
                                } else {
                                    string_counter += 1;
                                    format!("@{string_counter}")
                                };
                                string_pool.insert(string_bytes.clone(), name.clone());
                                let mut object_bytes = string_bytes.clone();
                                object_bytes.push(0);
                                defined_globals.push(mwcc_machine_code_to_object::DefinedGlobal {
                                    section: None,
                                    anonymous_adjust: 0,
                                    static_local_owner: None,
                                    is_weak: false,
                                    non_static_functions_before: 0,
                                    functions_before: global.functions_before,
                                    name: name.clone(),
                                    size: object_bytes.len() as u32,
                                    alignment: 4,
                                    comment_alignment: 4,
                                    initial_bytes: Some(object_bytes),
                                    is_const: config.flags.string_literals_read_only,
                                    force_full_data_section: config.flags.string_literals_read_only
                                        && !read_only_small_data,
                                    is_static: true,
                                    is_explicit_zero: false,
                                    preassigned_anonymous_ordinal: None,
                                    relocations: Vec::new(),
                                });
                                name
                            })
                    }
                };
                relocations.push(mwcc_machine_code_to_object::DataRelocation {
                    offset,
                    target,
                    addend: 0,
                });
            }
            // Relocated or non-zero bytes are initialized data (`.sdata`/`.data`); an
            // all-zero, unrelocated object (only null pointers) belongs in `.sbss`/`.bss`.
            let initial_bytes =
                (!relocations.is_empty() || bytes.iter().any(|&byte| byte != 0)).then_some(bytes);
            // An address initializer that resolved to no bytes is an all-null pointer
            // (`int *p = 0;`) — an EXPLICIT zero, so it orders ahead of the uninitialized run.
            let is_explicit_zero = initial_bytes.is_none();
            defined_globals.push(mwcc_machine_code_to_object::DefinedGlobal {
                anonymous_adjust: 0,
                static_local_owner: None,
                is_weak: global.is_weak,
                non_static_functions_before: global.non_static_functions_before,
                functions_before: global.functions_before,
                name: global.name.clone(),
                size,
                alignment: 4,
                comment_alignment: 4,
                initial_bytes,
                // A `static const` fn-pointer reference routes to `.sdata2` (read-only)
                // as a LOCAL; the writable `int *p = &g;` case stays non-const in `.sdata`.
                // A section override handles its own placement, so const is irrelevant there.
                is_const: global.is_const && global.section.is_none(),
                force_full_data_section,
                // A section-attributed static (`.dtors`), a `static const` reference,
                // or a static function-pointer TABLE binds LOCAL; a plain writable
                // pointer global stays GLOBAL as before.
                is_static: global.is_static
                    && (global.section.is_some()
                        || global.is_const
                        || all_null
                        || static_function_table
                        || static_unit_data_table
                        || static_string_table),
                is_explicit_zero,
                preassigned_anonymous_ordinal: None,
                relocations,
                section: global.section.clone(),
            });
            continue;
        }
        use mwcc_syntax_trees::Type;
        // A scalar/array of an arithmetic type serializes to fixed bytes (an integer
        // value, or a float/double IEEE-754 pattern already encoded by the parser).
        // Structs, pointers, and the like are not serializable here.
        let serializable_scalar = matches!(
            global.declared_type,
            Type::Int
                | Type::UnsignedInt
                | Type::Char
                | Type::UnsignedChar
                | Type::Short
                | Type::UnsignedShort
                | Type::Float
                | Type::Double
                | Type::LongLong
                | Type::UnsignedLongLong
        );
        // A struct object's element size and alignment come from its laid-out layout,
        // not the (word-default) scalar width — a struct value `g` or array `arr[N]`
        // occupies `struct_size * count` bytes at the struct's alignment.
        let (element_size, struct_alignment) = match global.declared_type {
            Type::Struct { size, align } => (size as u32, Some(align as u32)),
            _ => ((global.declared_type.width() / 8) as u32, None),
        };
        let count = global.array_length.unwrap_or(1) as u32;
        let size = element_size * count;
        // mwcc aligns a scalar to its element alignment but any *array* object to at
        // least a word (4), so a `char[4]`/`short[2]` is 4-aligned, not 1/2-aligned. A
        // struct takes its own alignment (already the max of its members').
        let alignments = global_alignments(
            element_size,
            struct_alignment,
            global.array_length.is_some(),
            global.is_const,
            global.attribute_alignment.map_or(1, u32::from),
            config.flags.optimization == mwcc_versions::Optimization::O0,
            config.build.profile.large_aggregate_comment_alignment(),
        );
        let alignment = alignments.layout;
        let comment_alignment = alignments.comment;
        // A struct's constant initializer lists its individual fields, each at its own
        // offset, so it serializes with a 4-byte field stride even though the object is
        // `struct_size`. (Only all-word-field structs are supported; a sub-word field
        // would need its own stride — guarded at the use site.)
        let serialize_stride = if struct_alignment.is_some() {
            4
        } else {
            element_size
        };

        if global.is_const {
            // A const struct value/array carries its pre-serialized field bytes
            // directly into the read-only section.
            if let Some(bytes) = &global.data_bytes {
                defined_globals.push(mwcc_machine_code_to_object::DefinedGlobal {
                    section: global.section.clone(),
                    anonymous_adjust: 0,
                    static_local_owner: None,
                    is_weak: global.is_weak,
                    non_static_functions_before: global.non_static_functions_before,
                    functions_before: global.functions_before,
                    name: global.name.clone(),
                    size,
                    alignment,
                    comment_alignment,
                    initial_bytes: Some(bytes.clone()),
                    is_const: true,
                    force_full_data_section,
                    is_static: global.is_static,
                    is_explicit_zero: false,
                    preassigned_anonymous_ordinal: None,
                    relocations: Vec::new(),
                });
                continue;
            }
            // A const global is always materialized as read-only initialized bytes
            // (even an all-zero one stays in `.sdata2`/`.rodata`, not `.sbss`). Only
            // an arithmetic scalar/array with a constant initializer is serializable
            // today; defer structs/pointers, strings, and uninitialized const — each
            // a separate piece.
            if !serializable_scalar {
                return Err(Diagnostic::error(
                    "a const global of this type is not supported yet (roadmap)",
                ));
            }
            let values = global.initializer.as_ref().ok_or_else(|| {
                Diagnostic::error("an uninitialized const global is not supported yet (roadmap)")
            })?;
            let initial_bytes = serialize(values, element_size, size);
            defined_globals.push(mwcc_machine_code_to_object::DefinedGlobal {
                section: global.section.clone(),
                anonymous_adjust: 0,
                static_local_owner: None,
                is_weak: global.is_weak,
                non_static_functions_before: global.non_static_functions_before,
                functions_before: global.functions_before,
                name: global.name.clone(),
                size,
                alignment,
                comment_alignment,
                initial_bytes: Some(initial_bytes),
                is_const: true,
                force_full_data_section,
                is_static: global.is_static,
                is_explicit_zero: false,
                preassigned_anonymous_ordinal: None,
                relocations: Vec::new(),
            });
            continue;
        }

        // Writable global. Small (≤ 8 bytes) → `.sdata`/`.sbss`; large (> 8) →
        // `.data`/`.bss` (the writer routes by size). Absolute addressing changes
        // references, not whether the definition exists, so `-sdata 0` must retain
        // large objects. A dense-switch jump table still shares `.data` with these
        // objects without a reconciled layout; defer that combination honestly.
        if size > 8 && has_jump_table {
            return Err(Diagnostic::error(
                "a large writable global alongside a jump table needs shared .data layout (roadmap)",
            ));
        }
        // Materialize the initializer's bytes if there is one (a struct value/array uses the
        // parser's pre-serialized field bytes — exact for sub-word/nested/padded fields; a
        // scalar/array serializes its word-stride values). `None` means uninitialized.
        let materialized: Option<Vec<u8>> = if let Some(bytes) = &global.data_bytes {
            Some(bytes.clone())
        } else {
            global
                .initializer
                .as_ref()
                .map(|values| serialize(values, serialize_stride, size))
        };
        // Section routing for a writable global:
        //   * a NON-zero initializer is always initialized data (`.sdata`/`.data`);
        //   * an all-zero ARRAY initializer stays MATERIALIZED zero bytes in `.sdata`/`.data` — mwcc
        //     does NOT coalesce a zeroed array into `.sbss`/`.bss` (`int a[2]={0,0};` -> `.sdata`,
        //     `int a[3]={0,0,0};` -> `.data`), regardless of size;
        //   * an all-zero SCALAR initializer coalesces to `.sbss` with no file bytes — an EXPLICIT
        //     zero, laid out in declaration order ahead of the reversed uninitialized run
        //     (`int a=0;`, `double d=0;`);
        //   * no initializer at all is uninitialized (`.sbss`/`.bss`).
        let is_array = global.array_length.is_some();
        let (initial_bytes, is_explicit_zero) = match materialized {
            // A relocation is itself initialized content even when every
            // placeholder byte is zero. Vtables and address-only aggregate
            // images must therefore remain PROGBITS `.data`, never collapse
            // into NOBITS `.bss`.
            Some(bytes)
                if bytes.iter().any(|&value| value != 0) || !global.data_relocations.is_empty() =>
            {
                (Some(bytes), false)
            }
            Some(bytes) if is_array => (Some(bytes), false),
            Some(_) => (None, true),
            None => (None, false),
        };
        defined_globals.push(mwcc_machine_code_to_object::DefinedGlobal {
            section: global.section.clone(),
            anonymous_adjust: 0,
            static_local_owner: None,
            is_weak: global.is_weak,
            non_static_functions_before: global.non_static_functions_before,
            functions_before: global.functions_before,
            name: global.name.clone(),
            size,
            alignment,
            comment_alignment,
            initial_bytes,
            is_const: false,
            force_full_data_section,
            is_static: global.is_static,
            is_explicit_zero,
            preassigned_anonymous_ordinal: None,
            relocations: global
                .data_relocations
                .iter()
                .map(|(offset, target, addend)| {
                    // A STRING-LITERAL struct member arrives as a \u{1}-marked
                    // target from the parser: pool it like an address-initializer
                    // string (an anonymous `@N` `.sdata` object, first-appearance
                    // numbering, deduplicated under `-str reuse` — locale's lconv).
                    let target = match target.strip_prefix('\u{1}') {
                        Some(literal) => {
                            let string_bytes: Vec<u8> = literal.as_bytes().to_vec();
                            string_pool
                                .get(string_bytes.as_slice())
                                .cloned()
                                .unwrap_or_else(|| {
                                    string_counter += 1;
                                    let name = format!("@{string_counter}");
                                    string_pool.insert(string_bytes.clone(), name.clone());
                                    let mut object_bytes = string_bytes.clone();
                                    object_bytes.push(0);
                                    pooled_string_globals.push(
                                        mwcc_machine_code_to_object::DefinedGlobal {
                                            section: None,
                                            anonymous_adjust: 0,
                                            static_local_owner: None,
                                            is_weak: false,
                                            non_static_functions_before: 0,
                                            functions_before: 0,
                                            name: name.clone(),
                                            size: object_bytes.len() as u32,
                                            alignment: 4,
                                            comment_alignment: 4,
                                            initial_bytes: Some(object_bytes),
                                            is_const: config.flags.string_literals_read_only,
                                            force_full_data_section: config
                                                .flags
                                                .string_literals_read_only
                                                && !read_only_small_data,
                                            is_static: true,
                                            is_explicit_zero: false,
                                            preassigned_anonymous_ordinal: None,
                                            relocations: Vec::new(),
                                        },
                                    );
                                    name
                                })
                        }
                        None => target.clone(),
                    };
                    mwcc_machine_code_to_object::DataRelocation {
                        offset: *offset,
                        target,
                        addend: *addend,
                    }
                })
                .collect(),
        });
        defined_globals.extend(pooled_string_globals.drain(..));
    }
    // Resolve each function's pooled string literals to anonymous `@N` `.sdata` objects, numbered at
    // the FRONT of that function's per-function `@N` block (before its constants and unwind entries),
    // matching mwcc's per-function counter walk (see mwcc-object's writer). A string reuses an
    // identical earlier one (`-str reuse`); a new one advances the counter. The counter starts at
    // `5 + global_strings` and advances per function by [its new strings + its new deduped constants
    // + its unwind entries] plus a fixed +4 gap. A jump table interleaves its own `@N` here in a way
    // not yet modeled, so a unit that mixes a string with a jump table defers wholesale.
    let mut counter = (u32::from(config.build.initial_anonymous_counter) + string_counter)
        .max(analysis_counter_floor);
    let mut numbered_constant: std::collections::HashSet<(u64, u8)> =
        std::collections::HashSet::new();
    let mut function_string_objects: Vec<mwcc_machine_code_to_object::DefinedGlobal> = Vec::new();
    let mut packed_string_base_counter = 0u32;
    let mut file_string_renames: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for (function_index, machine_function) in machine_functions.iter_mut().enumerate() {
        // A file-scope string declared right before this function consumes its
        // number here (mwcc rides ONE counter in compilation order).
        for (functions_before, placeholder) in &pending_file_strings {
            if *functions_before == function_index {
                file_string_renames.insert(placeholder.clone(), format!("@{counter}"));
                counter += 1;
            }
        }
        let bump = u32::from(machine_function.has_conversion)
            + if machine_function.has_float_branch {
                3
            } else {
                0
            }
            + machine_function.anonymous_label_bump;
        let mut number = counter
            + bump
            + unit.functions.get(function_index).map_or(0, |source| {
                source.locals.iter().filter(|local| local.is_static).count() as u32
            });
        // Strings first, in the function's `@N` block. The NEW ones (a reuse points at an earlier
        // pool entry) are recorded by name so the writer emits their symbols at the FRONT of this
        // function's `@N` block, interleaved per-function with its constants/unwind entries.
        let mut new_string_names: Vec<String> = Vec::new();
        let mut resolved: Vec<String> = if machine_function.packed_string_literals {
            let name = format!("@stringBase{packed_string_base_counter}");
            packed_string_base_counter += 1;
            let mut object_bytes = Vec::new();
            let mut names = Vec::with_capacity(machine_function.string_literals.len());
            for bytes in &machine_function.string_literals {
                names.push(name.clone());
                object_bytes.extend_from_slice(bytes);
                object_bytes.push(0);
            }
            new_string_names.push(name.clone());
            function_string_objects.push(mwcc_machine_code_to_object::DefinedGlobal {
                section: None,
                anonymous_adjust: 0,
                static_local_owner: None,
                is_weak: false,
                non_static_functions_before: 0,
                functions_before: 0,
                name,
                size: object_bytes.len() as u32,
                alignment: 4,
                comment_alignment: 4,
                initial_bytes: Some(object_bytes),
                is_const: config.flags.string_literals_read_only,
                // A packed read-only string base is a `.rodata` blob even when
                // its payload fits the ordinary sdata2 size threshold.
                force_full_data_section: config.flags.string_literals_read_only,
                is_static: true,
                is_explicit_zero: false,
                preassigned_anonymous_ordinal: None,
                relocations: Vec::new(),
            });
            names
        } else {
            machine_function
                .string_literals
                .iter()
                .map(|bytes| {
                    if let Some(name) = string_pool.get(bytes) {
                        return name.clone();
                    }
                    let name = format!("@{number}");
                    number += 1;
                    new_string_names.push(name.clone());
                    string_pool.insert(bytes.clone(), name.clone());
                    let mut object_bytes = bytes.clone();
                    object_bytes.push(0);
                    function_string_objects.push(mwcc_machine_code_to_object::DefinedGlobal {
                        section: None,
                        anonymous_adjust: 0,
                        static_local_owner: None,
                        is_weak: false,
                        non_static_functions_before: 0,
                        functions_before: 0,
                        name: name.clone(),
                        size: object_bytes.len() as u32,
                        alignment: 4,
                        comment_alignment: 4,
                        initial_bytes: Some(object_bytes),
                        is_const: config.flags.string_literals_read_only
                            || machine_function.strings_are_const,
                        force_full_data_section: (config.flags.string_literals_read_only
                            || machine_function.strings_are_const)
                            && !read_only_small_data,
                        is_static: true,
                        is_explicit_zero: false,
                        preassigned_anonymous_ordinal: None,
                        relocations: Vec::new(),
                    });
                    name
                })
                .collect()
        };
        machine_function.new_string_count = new_string_names.len() as u32;
        machine_function.new_string_names = new_string_names;
        // Then the function's constants (deduped across the unit, with the same
        // per-index number gaps the writer applies), its jump table (the counter
        // JUMPS to the table's number and continues from it — mirroring the
        // writer), the post-table label bump, and its unwind entries, so the
        // next function's block starts at the right `@N`.
        // string_number_after_constants: the string numbers assigned above used
        // the block-front position; when the knob places them after the first K
        // constants instead, re-derive them from the walk here (the writer
        // mirrors the same split).
        let string_shift = machine_function.new_string_count;
        let mut deferred_string_base: Option<u32> = None;
        if machine_function.string_number_after_constants.is_some() {
            // The strings were numbered at the front above; pull the walk back
            // and inject them at position K instead.
            number -= string_shift;
        }
        // string_number_after_rodata: pull the front assignment back and walk
        // the blobs, injecting the gap + strings before blob K (the writer's
        // numbering walk mirrors this split).
        if let Some((position, gap)) = machine_function.string_number_after_rodata {
            number -= string_shift;
            for (blob_index, blob) in machine_function.anonymous_rodata.iter().enumerate() {
                if position == blob_index as u32 {
                    deferred_string_base = Some(number + gap);
                    number += gap + string_shift;
                }
                number = (number as i64 + blob.anonymous_offset as i64) as u32 + 1;
            }
            if position as usize >= machine_function.anonymous_rodata.len() {
                deferred_string_base = Some(number + gap);
                number += gap + string_shift;
            }
        }
        for (constant_index, constant) in machine_function.constants.iter().enumerate() {
            if machine_function.string_number_after_constants == Some(constant_index as u32) {
                deferred_string_base = Some(number);
                number += string_shift;
            }
            for (gap_index, gap) in &machine_function.constant_number_gaps {
                if *gap_index == constant_index {
                    number += gap;
                }
            }
            // A force-new constant always consumes a fresh number (the writer
            // never dedupes it against an equal earlier slot).
            if constant.force_new || numbered_constant.insert((constant.bits, constant.byte_width))
            {
                number += 1;
            }
        }
        if let Some(position) = machine_function.string_number_after_constants {
            if position as usize >= machine_function.constants.len() {
                deferred_string_base = Some(number);
                number += string_shift;
            }
        }
        {
            // Rename the front-assigned @Ns to the deferred position.
            if let Some(base) = deferred_string_base {
                let mut renumbered = std::collections::HashMap::new();
                for (offset, name) in machine_function.new_string_names.iter().enumerate() {
                    renumbered.insert(name.clone(), format!("@{}", base + offset as u32));
                }
                for name in &mut machine_function.new_string_names {
                    if let Some(new_name) = renumbered.get(name) {
                        *name = new_name.clone();
                    }
                }
                for (bytes, name) in string_pool.iter_mut() {
                    let _ = bytes;
                    if let Some(new_name) = renumbered.get(name) {
                        *name = new_name.clone();
                    }
                }
                for object in &mut function_string_objects {
                    if let Some(new_name) = renumbered.get(&object.name) {
                        object.name = new_name.clone();
                    }
                }
                for relocation in &mut machine_function.relocations {
                    if let mwcc_machine_code::RelocationTarget::External(name) = &relocation.target
                    {
                        if let Some(new_name) = renumbered.get(name) {
                            relocation.target =
                                mwcc_machine_code::RelocationTarget::External(new_name.clone());
                        }
                    }
                }
                // The @@str placeholder rewrite below installs `resolved` names —
                // point them at the deferred numbers too.
                for name in &mut resolved {
                    if let Some(new_name) = renumbered.get(name) {
                        *name = new_name.clone();
                    }
                }
            }
        }
        for table in &machine_function.jump_tables {
            number += table.anonymous_offset;
        }
        number += machine_function.post_constant_label_bump;
        if machine_function.frame.is_some() {
            number += 2;
        }
        let post_function_bump = machine_function
            .post_function_anonymous_bump
            .unwrap_or_else(|| {
                if machine_function.frame.is_some() {
                    config.build.post_framed_function_anonymous_bump
                } else {
                    config.build.post_leaf_function_anonymous_bump
                }
            });
        counter = number + u32::from(post_function_bump);
        for relocation in &mut machine_function.relocations {
            match &relocation.target {
                mwcc_machine_code::RelocationTarget::External(name) => {
                    if let Some(index) = name
                        .strip_prefix("@@str")
                        .and_then(|rest| rest.parse::<usize>().ok())
                    {
                        relocation.target =
                            mwcc_machine_code::RelocationTarget::External(resolved[index].clone());
                    }
                }
                mwcc_machine_code::RelocationTarget::ExternalWithAddend(name, addend) => {
                    if let Some(index) = name
                        .strip_prefix("@@str")
                        .and_then(|rest| rest.parse::<usize>().ok())
                    {
                        relocation.target = mwcc_machine_code::RelocationTarget::ExternalWithAddend(
                            resolved[index].clone(),
                            *addend,
                        );
                    }
                }
                _ => {}
            }
        }
    }
    if !file_string_renames.is_empty() {
        for global in &mut defined_globals {
            if let Some(name) = file_string_renames.get(&global.name) {
                global.name = name.clone();
            }
            for relocation in &mut global.relocations {
                if let Some(name) = file_string_renames.get(&relocation.target) {
                    relocation.target = name.clone();
                }
            }
        }
        for machine_function in &mut machine_functions {
            for relocation in &mut machine_function.relocations {
                if let mwcc_machine_code::RelocationTarget::External(name) = &relocation.target {
                    if let Some(resolved) = file_string_renames.get(name) {
                        relocation.target =
                            mwcc_machine_code::RelocationTarget::External(resolved.clone());
                    }
                }
            }
        }
    }
    if config.flags.rtti {
        // RTTI helper names are reserved by the class/declaration analysis
        // walk, before executable function bodies advance the ordinary pool
        // counter. Keep this timeline independent from function lowering.
        let ordinary_rtti_analysis_counter = cxx_rtti_names::analysis_counter(
            config.build.initial_anonymous_counter,
            string_counter,
            cxx_rtti_prior_declaration_bump,
            cxx_inline_facts,
            cxx_rtti_names::AnalysisWeights {
                virtual_method: behavior.cxx_rtti_virtual_method_label_weight,
                virtual_destructor: behavior.cxx_rtti_virtual_destructor_label_weight,
                inherited_virtual_destructor: behavior
                    .cxx_rtti_inherited_virtual_destructor_label_bump,
                initial_virtual_discount: behavior.cxx_rtti_initial_virtual_label_discount,
            },
            analysis_counter_floor,
        );
        let rtti_analysis_counter = if config.build.version.0 >= 4 && config.flags.debug_info {
            cxx_rtti_names::fragmented_debug_counter(
                ordinary_rtti_analysis_counter,
                cxx_inline_facts,
            )
            .unwrap_or(ordinary_rtti_analysis_counter)
        } else {
            ordinary_rtti_analysis_counter
        };
        cxx_rtti_names::resolve(&mut defined_globals, rtti_analysis_counter);
    }
    defined_globals.extend(function_string_objects);
    defined_globals.extend(static_local_globals);

    // A `static` function whose ADDRESS is taken by a data initializer gets its
    // LOCAL FUNC symbol created while that initializer is analyzed, before deferred
    // function code generation creates constants and unwind symbols. Data
    // initializers are visited in their relocation-emission order (often reverse
    // field order). Text-body address references only hoist a symbol when a
    // prototype made the function known before its definition; a static that is
    // only CALLED (REL24) keeps its symbol at the definition.
    //
    // Measured examples:
    // - Animal Crossing's iam_ef_kigae profile creates dw/mv/ct/init from its
    //   descending-offset `.data` relocations before any function-local constants.
    // - OSAlarm creates DecrementerExceptionHandler when its address is passed to
    //   __OSSetExceptionHandler in a text body.
    let early_static_function_symbols: Vec<String> = {
        let static_definition_index: std::collections::HashMap<&str, usize> = unit
            .functions
            .iter()
            .enumerate()
            .filter(|(_, function)| function.is_static)
            .map(|(index, function)| (function.name.as_str(), index))
            .collect();
        let text_address_taken: std::collections::HashSet<&str> = machine_functions
            .iter()
            .flat_map(|function| function.relocations.iter())
            .filter(|relocation| {
                !matches!(relocation.kind, mwcc_machine_code::RelocationKind::Rel24)
            })
            .filter_map(|relocation| match &relocation.target {
                mwcc_machine_code::RelocationTarget::External(name)
                | mwcc_machine_code::RelocationTarget::ExternalWithAddend(name, _) => {
                    Some(name.as_str())
                }
                _ => None,
            })
            .collect();
        let mut seen = std::collections::HashSet::new();
        let mut symbols = Vec::new();
        for global in &defined_globals {
            // Ordinary data relocation records are emitted in reverse element
            // order by the object writer; MWCC's symbol creation follows that
            // same traversal. Constructor chains are the one forward-ordered
            // section.
            let relocation_indices: Box<dyn Iterator<Item = usize>> =
                if global.section.as_deref() == Some(".ctors") {
                    Box::new(0..global.relocations.len())
                } else {
                    Box::new((0..global.relocations.len()).rev())
                };
            for relocation_index in relocation_indices {
                let relocation = &global.relocations[relocation_index];
                if !static_definition_index.contains_key(relocation.target.as_str()) {
                    continue;
                }
                if seen.insert(relocation.target.clone()) {
                    symbols.push(relocation.target.clone());
                }
            }
        }
        for (name, _, _) in &unit.prototypes {
            if static_definition_index.contains_key(name.as_str())
                && text_address_taken.contains(name.as_str())
                && seen.insert(name.clone())
            {
                symbols.push(name.clone());
            }
        }
        symbols
    };

    // C's plain-`inline` asm helpers (OSFastCast's `inline __OSf32tos16`) are
    // materialized as unused GLOBAL UND symbols ahead of the first function's
    // references. In C++ the same inline definitions disappear completely.
    // Captures may already pin a measured phantom list; otherwise the parsed
    // declaration order is the single source of truth for the general path.
    if !is_cxx
        && behavior.retain_unused_c_inline_asm_symbols
        && !config.flags.inline_deferred
        && config.flags.optimization != mwcc_versions::Optimization::O0
        && !unit.plain_inline_asm_helpers.is_empty()
        && machine_functions
            .iter()
            .all(|function| function.phantom_externals.is_empty())
    {
        let Some(first) = machine_functions.first_mut() else {
            return Err(Diagnostic::error(
                "a functionless C translation unit with plain-inline asm helpers needs a unit-level external-symbol run (roadmap)",
            ));
        };
        first.phantom_externals = unit.plain_inline_asm_helpers.clone();
    }

    // The C and C++ frontends have independent version boundaries for retaining
    // skipped static-inline asm helpers as LOCAL undefined symbols. Referenced
    // helpers always need their local UND binding in every generation.
    let referenced_targets: std::collections::HashSet<&str> = machine_functions
        .iter()
        .flat_map(|function| &function.relocations)
        .filter_map(|relocation| match &relocation.target {
            mwcc_machine_code::RelocationTarget::External(name)
            | mwcc_machine_code::RelocationTarget::ExternalWithAddend(name, _) => {
                Some(name.as_str())
            }
            _ => None,
        })
        .collect();
    let object_inline_asm_symbols: Vec<String> = unit
        .inline_asm_symbols
        .iter()
        .filter(|name| {
            ((is_cxx && behavior.retain_unused_cxx_inline_asm_symbols)
                || (!is_cxx
                    && behavior.retain_unused_c_inline_asm_symbols
                    && !config.flags.inline_deferred
                    && config.flags.optimization != mwcc_versions::Optimization::O0))
                || referenced_targets.contains(name.as_str())
        })
        .cloned()
        .collect();
    let early_undefined_externals: Vec<String> = if behavior.materialize_section_prototypes {
        unit.section_prototypes
            .iter()
            .filter(|name| {
                !unit
                    .functions
                    .iter()
                    .any(|function| function.name == **name)
                    // A prior ordinary prototype suppresses the otherwise
                    // retained section-only UND symbol (measured: CARDNet's
                    // plain `__start` declaration followed by an `.init`
                    // redeclaration). Repeated section declarations alone do
                    // not suppress it (OSSync in Strikers and TP).
                    && !unit
                        .section_prototypes_with_prior_plain_declaration
                        .contains(*name)
            })
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let section_externals: Vec<(String, usize)> = if behavior.materialize_section_prototypes {
        unit.globals
            .iter()
            .filter(|global| global.is_extern && global.section.is_some())
            .map(|global| (global.name.clone(), global.functions_before))
            .collect()
    } else {
        Vec::new()
    };

    let code_alignment = if machine_functions.is_empty() {
        // A data-only unit still carries an empty `.text` section. Later Wii
        // builds normally align real functions to 16, but MWCC gives this
        // placeholder section the EABI minimum word alignment.
        4
    } else {
        config
            .flags
            .function_alignment
            .unwrap_or(u32::from(config.build.code_alignment))
    };
    let object_format = mwcc_machine_code_to_object::ObjectFormat {
        comment: mwcc_machine_code_to_object::CommentFormat {
            marker: config.build.comment_marker,
            version: config.build.comment_version,
            pooling_enabled: config.flags.pooling_enabled,
        },
        emb_sda21_offset: config.build.emb_sda21_offset,
        code_alignment,
        sdata2_writable: config.build.sdata2_writable,
        function_symbol_order: if config.flags.whole_file_optimization_enabled() {
            // Whole-file IPA registers the optimized function before the
            // external target discovered while lowering its body.
            mwcc_machine_code_to_object::FunctionSymbolOrder::FunctionFirst
        } else if config.build.function_symbol_before_references {
            if config.flags.optimization == mwcc_versions::Optimization::O0 {
                mwcc_machine_code_to_object::FunctionSymbolOrder::FunctionFirstAtDefinition
            } else if config.flags.inline_deferred {
                mwcc_machine_code_to_object::FunctionSymbolOrder::LegacyDeferred
            } else {
                mwcc_machine_code_to_object::FunctionSymbolOrder::FunctionFirst
            }
        } else if config.flags.inline_deferred {
            mwcc_machine_code_to_object::FunctionSymbolOrder::Deferred
        } else {
            mwcc_machine_code_to_object::FunctionSymbolOrder::ReferencesFirst
        },
        initialized_globals_before_deferred_functions: config.flags.inline_deferred,
        local_data_symbols_in_declaration_order: behavior.local_data_symbol_order
            == mwcc_versions::LocalDataSymbolOrder::DeclarationOrder,
        small_zero_statics_in_declaration_order: behavior.small_zero_data_layout_style
            == mwcc_versions::SmallZeroDataLayoutStyle::LegacyStaticDeclarationOrderFirst,
        rodata_anchor_before_data_symbols: behavior.read_only_section_anchor_order
            == mwcc_versions::ReadOnlySectionAnchorOrder::BeforeDataObjects,
        rodata_anchor_comment_flags: behavior.read_only_section_anchor_comment_flags,
        data_relocations_use_section_anchors: behavior.data_section_relocation_style
            == mwcc_versions::DataSectionRelocationStyle::SectionAnchor,
        data_anchor_comment_flags: behavior.data_section_anchor_comment_flags,
        initial_anonymous_counter: config.build.initial_anonymous_counter,
        post_leaf_function_anonymous_bump: config.build.post_leaf_function_anonymous_bump,
        post_framed_function_anonymous_bump: config.build.post_framed_function_anonymous_bump,
    };
    // Debug lowering describes only source declarations that actually survived
    // data materialization. In particular, `extern T x = {...}` is a definition,
    // while an unused folded `static const` is not. Keep that object-emission
    // decision in the driver and pass the semantic debug stage a name set rather
    // than making it duplicate every data-elision rule above.
    let emitted_data_symbols: std::collections::HashSet<String> = defined_globals
        .iter()
        .map(|global| global.name.clone())
        .collect();
    let debug = if config.flags.debug_info {
        mwcc_syntax_trees_to_debug_info::lower_debug_info(
            &unit,
            &machine_functions,
            !defined_globals.is_empty(),
            &emitted_data_symbols,
            source_name,
            source,
            config.build,
            code_alignment,
        )?
    } else {
        None
    };
    let object = mwcc_machine_code_to_object::assemble_object(
        &machine_functions,
        &defined_globals,
        &object_inline_asm_symbols,
        &early_static_function_symbols,
        &early_undefined_externals,
        &unit.section_prototypes,
        &section_externals,
        source_name,
        object_format,
        small_data,
        config.flags.emit_mwcats,
        debug,
    );

    if let Some(directory) = artifacts {
        write_lowered_artifacts(
            directory,
            &unit.functions,
            &machine_functions,
            &object,
        );
    }
    Ok(object)
}

#[cfg(test)]
mod tests {
    use super::{compile, global_alignments, parse_invocation, GlobalAlignments, SourceLanguage};
    use mwcc_versions::{EnumStorage, GlobalAddressing};

    #[test]
    fn parity_keep_going_is_an_explicit_diagnostic_flag() {
        let ordinary = parse_invocation(&[]);
        let diagnostic = parse_invocation(&["--parity-keep-going".into()]);
        assert!(!ordinary.parity_keep_going);
        assert!(diagnostic.parity_keep_going);
    }

    #[test]
    fn parity_keep_going_emits_prior_functions_after_a_backend_defer() {
        let source = b"int good(void) { return 1; }\nint bad(void) { return missing; }\n";
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::DEFAULT,
            flags,
        };
        let ordinary = compile(source, "partial.c", config, Some(SourceLanguage::C), None, false);
        assert!(ordinary.is_err());
        let partial = compile(source, "partial.c", config, Some(SourceLanguage::C), None, true)
            .expect("diagnostic mode should serialize the lowerable prefix");
        assert!(!partial.is_empty());
    }

    #[test]
    fn lowers_fixed_halfword_mask_insert_inside_an_ordinary_function() {
        let source = br#"
            typedef volatile unsigned short vu16;
            vu16 regs[32] : (0xCC005000);
            void update(int unused) {
                regs[5] = ((unsigned short)regs[5] & ~0x28) | 0x80;
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::DEFAULT,
            flags,
        };
        let object = compile(
            source,
            "fixed-mask-insert.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("fixed-address RMW should share one absolute base");
        assert!(!object.is_empty());
    }

    #[test]
    fn records_bare_bit_field_conditions_during_extraction() {
        let source = br#"
            struct Flags { unsigned char enabled : 1; };
            extern void enabled(void);
            extern void disabled(void);
            void test(struct Flags* flags) {
                if (flags->enabled) {
                    enabled();
                }
            }
            void test_not(struct Flags* flags) {
                if (!flags->enabled) {
                    disabled();
                }
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "bit-field-condition.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("a bare bit-field condition should lower through record-form extraction");
        assert_eq!(
            object
                .windows(4)
                .filter(|instruction| *instruction == [0x54, 0x00, 0xcf, 0xff])
                .count(),
            2
        );
    }

    #[test]
    fn copies_a_bit_field_within_its_storage_unit_in_place() {
        let source = br#"
            struct Flags {
                unsigned char b0 : 1;
                unsigned char b1 : 1;
                unsigned char b2 : 1;
                unsigned char b3 : 1;
                unsigned char b4 : 1;
                unsigned char source : 1;
                unsigned char target : 1;
            };
            void copy(struct Flags* flags) {
                flags->target = flags->source;
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "in-place-bit-field-copy.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("fields in one storage unit should share their load");
        let expected = [
            0x88, 0x83, 0x00, 0x00, 0x50, 0x84, 0xff, 0xbc, 0x98, 0x83, 0x00, 0x00, 0x4e, 0x80,
            0x00, 0x20,
        ];
        assert!(object
            .windows(expected.len())
            .any(|bytes| bytes == expected));
    }

    #[test]
    fn schedules_in_place_float_updates_with_their_trailing_clamps() {
        let source = br#"
            struct Body { float velocity; };
            void fall(struct Body* body, float acceleration, float limit) {
                body->velocity -= acceleration;
                if (body->velocity < -limit) {
                    body->velocity = -limit;
                }
            }
            void ascend(struct Body* body, float acceleration, float limit) {
                body->velocity += acceleration;
                if (body->velocity > limit) {
                    body->velocity = limit;
                }
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "float-update-clamp.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("in-place float updates should share a schedule with their clamps");
        let fall = [
            0xc0, 0x03, 0x00, 0x00, 0xfc, 0x40, 0x10, 0x50, 0xec, 0x00, 0x08, 0x28, 0xd0, 0x03,
            0x00, 0x00, 0xc0, 0x03, 0x00, 0x00, 0xfc, 0x00, 0x10, 0x40, 0x4c, 0x80, 0x00, 0x20,
            0xd0, 0x43, 0x00, 0x00, 0x4e, 0x80, 0x00, 0x20,
        ];
        let ascend = [
            0xc0, 0x03, 0x00, 0x00, 0xec, 0x00, 0x08, 0x2a, 0xd0, 0x03, 0x00, 0x00, 0xc0, 0x03,
            0x00, 0x00, 0xfc, 0x00, 0x10, 0x40, 0x4c, 0x81, 0x00, 0x20, 0xd0, 0x43, 0x00, 0x00,
            0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object.windows(fall.len()).any(|bytes| bytes == fall));
        assert!(object.windows(ascend.len()).any(|bytes| bytes == ascend));
    }

    #[test]
    fn shares_the_conversion_frame_for_conditional_float_requantization() {
        let source = br#"
            struct Params {
                char padding[408];
                float factor;
                float addend;
                float conditional_factor;
            };
            extern struct Params* params;
            float quantize(int value, int selector, float multiplier) {
                int first = value * params->factor + params->addend;
                float result = (int) (first * multiplier);
                if ((unsigned) selector - 39 <= 1) {
                    result = (int) (result * params->conditional_factor);
                }
                return result;
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "conditional-float-requantize.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("all three float-to-int images should share one conversion frame");
        let expected = [
            0x94, 0x21, 0xff, 0xc0, 0x6c, 0x60, 0x80, 0x00, 0x3c, 0x60, 0x43, 0x30, 0x90, 0x01,
            0x00, 0x3c, 0x38, 0x04, 0xff, 0xd9, 0x80, 0xa0, 0x00, 0x00, 0x28, 0x00, 0x00, 0x01,
            0x90, 0x61, 0x00, 0x38, 0xc8, 0x80, 0x00, 0x00, 0xc8, 0x61, 0x00, 0x38, 0xc0, 0x45,
            0x01, 0x98, 0xec, 0x63, 0x20, 0x28, 0xc0, 0x05, 0x01, 0x9c, 0xec, 0x03, 0x00, 0xba,
            0xfc, 0x00, 0x00, 0x1e, 0xd8, 0x01, 0x00, 0x30, 0x80, 0x01, 0x00, 0x34, 0x6c, 0x00,
            0x80, 0x00, 0x90, 0x01, 0x00, 0x2c, 0x90, 0x61, 0x00, 0x28, 0xc8, 0x01, 0x00, 0x28,
            0xec, 0x00, 0x20, 0x28, 0xec, 0x00, 0x00, 0x72, 0xfc, 0x00, 0x00, 0x1e, 0xd8, 0x01,
            0x00, 0x20, 0x80, 0x01, 0x00, 0x24, 0x6c, 0x00, 0x80, 0x00, 0x90, 0x01, 0x00, 0x1c,
            0x90, 0x61, 0x00, 0x18, 0xc8, 0x01, 0x00, 0x18, 0xec, 0x20, 0x20, 0x28, 0x41, 0x81,
            0x00, 0x2c, 0xc0, 0x05, 0x01, 0xa0, 0xec, 0x01, 0x00, 0x32, 0xfc, 0x00, 0x00, 0x1e,
            0xd8, 0x01, 0x00, 0x18, 0x80, 0x01, 0x00, 0x1c, 0x6c, 0x00, 0x80, 0x00, 0x90, 0x01,
            0x00, 0x24, 0x90, 0x61, 0x00, 0x20, 0xc8, 0x01, 0x00, 0x20, 0xec, 0x20, 0x20, 0x28,
            0x38, 0x21, 0x00, 0x40, 0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object
            .windows(expected.len())
            .any(|bytes| bytes == expected));
    }

    #[test]
    fn schedules_symmetric_member_clamps_as_one_region() {
        let source = br#"
            struct Body { float velocity; };
            void direct(struct Body* body, float limit) {
                if (body->velocity < -limit) {
                    body->velocity = -limit;
                } else if (body->velocity > limit) {
                    body->velocity = limit;
                }
            }
            void through_local(struct Body* body, float limit) {
                float velocity = body->velocity;
                if (velocity < -limit) {
                    body->velocity = -limit;
                } else if (velocity > limit) {
                    body->velocity = limit;
                }
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "symmetric-member-clamp.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("both source spellings should retain one member load");
        let direct = [
            0xfc, 0x00, 0x08, 0x50, 0xc0, 0x43, 0x00, 0x00, 0xfc, 0x02, 0x00, 0x40, 0x40, 0x80,
            0x00, 0x0c, 0xd0, 0x03, 0x00, 0x00, 0x4e, 0x80, 0x00, 0x20, 0xfc, 0x02, 0x08, 0x40,
            0x4c, 0x81, 0x00, 0x20, 0xd0, 0x23, 0x00, 0x00, 0x4e, 0x80, 0x00, 0x20,
        ];
        let through_local = [
            0xfc, 0x40, 0x08, 0x50, 0xc0, 0x03, 0x00, 0x00, 0xfc, 0x00, 0x10, 0x40, 0x40, 0x80,
            0x00, 0x0c, 0xd0, 0x43, 0x00, 0x00, 0x4e, 0x80, 0x00, 0x20, 0xfc, 0x00, 0x08, 0x40,
            0x4c, 0x81, 0x00, 0x20, 0xd0, 0x23, 0x00, 0x00, 0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object
            .windows(direct.len())
            .any(|bytes| bytes == direct));
        assert!(object
            .windows(through_local.len())
            .any(|bytes| bytes == through_local));
    }

    #[test]
    fn materializes_member_arguments_for_automatic_float_helper_inlining() {
        let source = br#"
            struct Body {
                float horizontal_velocity;
                float vertical_velocity;
                float gravity;
                float terminal_velocity;
                float drift_limit;
            };
            void clamp(struct Body* body, float limit) {
                float velocity = body->horizontal_velocity;
                if (velocity < -limit) {
                    body->horizontal_velocity = -limit;
                } else if (velocity > limit) {
                    body->horizontal_velocity = limit;
                }
            }
            void clamp_wrapper(struct Body* body) {
                clamp(body, body->drift_limit);
            }
            void fall(struct Body* body, float gravity, float terminal_velocity) {
                body->vertical_velocity -= gravity;
                if (body->vertical_velocity < -terminal_velocity) {
                    body->vertical_velocity = -terminal_velocity;
                }
            }
            void fall_wrapper(struct Body* body) {
                fall(body, body->gravity, body->terminal_velocity);
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "automatic-member-float-helpers.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("member-valued helper arguments should be evaluated once and inlined");
        let clamp_wrapper = [
            0xc0, 0x23, 0x00, 0x10, 0xc0, 0x43, 0x00, 0x00, 0xfc, 0x00, 0x08, 0x50, 0xfc, 0x02,
            0x00, 0x40, 0x40, 0x80, 0x00, 0x0c, 0xd0, 0x03, 0x00, 0x00, 0x4e, 0x80, 0x00, 0x20,
            0xfc, 0x02, 0x08, 0x40, 0x4c, 0x81, 0x00, 0x20, 0xd0, 0x23, 0x00, 0x00, 0x4e, 0x80,
            0x00, 0x20,
        ];
        let fall_wrapper = [
            0xc0, 0x23, 0x00, 0x04, 0xc0, 0x03, 0x00, 0x08, 0xc0, 0x43, 0x00, 0x0c, 0xec, 0x01,
            0x00, 0x28, 0xfc, 0x20, 0x10, 0x50, 0xd0, 0x03, 0x00, 0x04, 0xc0, 0x03, 0x00, 0x04,
            0xfc, 0x00, 0x08, 0x40, 0x4c, 0x80, 0x00, 0x20, 0xd0, 0x23, 0x00, 0x04, 0x4e, 0x80,
            0x00, 0x20,
        ];
        assert!(object
            .windows(clamp_wrapper.len())
            .any(|bytes| bytes == clamp_wrapper));
        assert!(object
            .windows(fall_wrapper.len())
            .any(|bytes| bytes == fall_wrapper));
    }

    #[test]
    fn retains_an_inline_frame_lane_and_reorders_shared_member_arguments() {
        let source = br#"
            struct Fighter {
                unsigned char player_id;
                char padding[7];
                void* item;
                unsigned char rumble_id;
                unsigned char b0 : 1;
                unsigned char b1 : 1;
                unsigned char b2 : 1;
                unsigned char selected : 1;
            };
            struct Object {
                char padding[44];
                struct Fighter* fighter;
            };
            extern int active(unsigned char, int);
            extern void remove_rumble(unsigned char, int);
            extern void notify(unsigned char, int);
            inline void cleanup(struct Fighter* fighter, int kind) {
                if (active(fighter->player_id, fighter->selected)) {
                    remove_rumble(fighter->rumble_id, kind + kind);
                }
            }
            void release(struct Object* object) {
                struct Fighter* fighter = object->fighter;
                fighter->item = 0;
                cleanup(fighter, 2);
                notify(fighter->player_id, fighter->selected);
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "inline-shared-member-arguments.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the retained inline body should share the caller's saved alias");
        let entry = [
            0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x38, 0x00, 0x00, 0x00, 0x94, 0x21,
            0xff, 0xe8, 0x93, 0xe1, 0x00, 0x14, 0x83, 0xe3, 0x00, 0x2c, 0x90, 0x1f, 0x00, 0x08,
        ];
        let shared_arguments = [0x88, 0x9f, 0x00, 0x0d, 0x88, 0x7f, 0x00, 0x00];
        let exit = [
            0x80, 0x01, 0x00, 0x1c, 0x83, 0xe1, 0x00, 0x14, 0x38, 0x21, 0x00, 0x18, 0x7c, 0x08,
            0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object.windows(entry.len()).any(|bytes| bytes == entry));
        assert_eq!(
            object
                .windows(shared_arguments.len())
                .filter(|bytes| *bytes == shared_arguments)
                .count(),
            2
        );
        assert!(object.windows(exit.len()).any(|bytes| bytes == exit));
    }

    #[test]
    fn keeps_a_one_use_sign_selection_in_the_float_scratch() {
        let source = br#"
            struct Body { float facing; float input; };
            void update(struct Body* body) {
                float direction;
                if (body->input >= 0) {
                    direction = 1;
                } else {
                    direction = -1;
                }
                body->facing = direction;
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "sign-selected-member-store.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the one-use selected value should stay in f0");
        let expected = [
            0xc0, 0x23, 0x00, 0x04, 0xc0, 0x00, 0x00, 0x00, 0xfc, 0x01, 0x00, 0x40, 0x4c, 0x41,
            0x13, 0x82, 0x40, 0x82, 0x00, 0x0c, 0xc0, 0x00, 0x00, 0x00, 0x48, 0x00, 0x00, 0x08,
            0xc0, 0x00, 0x00, 0x00, 0xd0, 0x03, 0x00, 0x00, 0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object
            .windows(expected.len())
            .any(|bytes| bytes == expected));
    }

    #[test]
    fn preserves_a_float_call_argument_while_deriving_later_arguments() {
        let source = br#"
            struct Attributes {
                char padding[100];
                float scale;
                float base;
                float maximum;
                float friction;
                char tail[272];
            };
            struct Fighter {
                char padding[272];
                struct Attributes attributes;
                char middle[908];
                float stick;
            };
            extern void apply(struct Fighter*, float, float, float, float);
            void update(struct Fighter* fighter, float velocity) {
                float scaled;
                float flat;
                float stick = fighter->stick;
                struct Attributes* attributes = &fighter->attributes;
                scaled = stick * attributes->scale;
                if (stick > 0) {
                    flat = attributes->base;
                } else {
                    flat = -attributes->base;
                }
                apply(fighter, velocity, scaled + flat,
                      stick * attributes->maximum, attributes->friction);
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "derived-float-call-arguments.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the incoming f1 argument should survive the derived arguments");
        let expected = [
            0x7c, 0x08, 0x02, 0xa6, 0x38, 0x83, 0x01, 0x10, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21,
            0xff, 0xf8, 0xc0, 0x00, 0x00, 0x00, 0xc0, 0xa3, 0x06, 0x20, 0xc0, 0x43, 0x01, 0x74,
            0xfc, 0x05, 0x00, 0x40, 0xec, 0x45, 0x00, 0xb2, 0x40, 0x81, 0x00, 0x0c, 0xc0, 0x64,
            0x00, 0x68, 0x48, 0x00, 0x00, 0x0c, 0xc0, 0x04, 0x00, 0x68, 0xfc, 0x60, 0x00, 0x50,
            0xc0, 0x04, 0x00, 0x6c, 0xec, 0x42, 0x18, 0x2a, 0xc0, 0x84, 0x00, 0x70, 0xec, 0x65,
            0x00, 0x32, 0x48, 0x00, 0x00, 0x01, 0x80, 0x01, 0x00, 0x0c, 0x38, 0x21, 0x00, 0x08,
            0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object
            .windows(expected.len())
            .any(|bytes| bytes == expected));
    }

    #[test]
    fn dispatches_on_a_member_of_a_freshly_loaded_pointer_local() {
        let source = br#"
            struct State { int mode; };
            struct Object { int padding[11]; struct State* state; };
            extern void air(struct Object*);
            extern void ground(struct Object*);
            void dispatch(struct Object* object) {
                struct State* state = object->state;
                if (state->mode == 1) {
                    air(object);
                } else {
                    ground(object);
                }
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "local-member-call-dispatch.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("a fresh pointer local should fold into its call-dispatch condition");
        let expected = [
            0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21, 0xff, 0xf8, 0x80, 0x83,
            0x00, 0x2c, 0x80, 0x04, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x01, 0x40, 0x82, 0x00, 0x0c,
            0x48, 0x00, 0x00, 0x01, 0x48, 0x00, 0x00, 0x08, 0x48, 0x00, 0x00, 0x01, 0x80, 0x01,
            0x00, 0x0c, 0x38, 0x21, 0x00, 0x08, 0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object
            .windows(expected.len())
            .any(|bytes| bytes == expected));
    }

    #[test]
    fn preserves_an_owner_argument_across_an_alias_bitfield_clear() {
        let source = br#"
            struct State {
                char padding[8729];
                unsigned char active : 1;
            };
            struct Object {
                char padding[44];
                struct State* state;
            };
            extern void destroy(struct Object*);
            void release(struct Object* object) {
                struct State* state = object->state;
                state->active = 0;
                destroy(object);
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "alias-bitfield-clear.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the owner should remain in r3 for the trailing call");
        let expected = [
            0x7c, 0x08, 0x02, 0xa6, 0x38, 0xa0, 0x00, 0x00, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21,
            0xff, 0xf8, 0x80, 0x83, 0x00, 0x2c, 0x88, 0x04, 0x22, 0x19, 0x50, 0xa0, 0x3e, 0x30,
            0x98, 0x04, 0x22, 0x19, 0x48, 0x00, 0x00, 0x01, 0x80, 0x01, 0x00, 0x0c, 0x38, 0x21,
            0x00, 0x08, 0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object
            .windows(expected.len())
            .any(|bytes| bytes == expected));
    }

    #[test]
    fn schedules_two_guarded_callbacks_after_shared_alias_calls() {
        let source = br#"
            struct Object;
            typedef void (*Callback)(struct Object*);
            struct Fighter {
                char callback_padding[8668];
                Callback first_callback;
                Callback second_callback;
            };
            struct Object {
                char user_padding[44];
                struct Fighter* user_data;
            };
            extern void prepare(struct Fighter*);
            extern void update(struct Fighter*);
            void dispatch_callbacks(struct Object* object) {
                struct Fighter* fighter = object->user_data;
                prepare(fighter);
                update(fighter);
                if (fighter->first_callback != 0) {
                    fighter->first_callback(object);
                }
                if (fighter->second_callback != 0) {
                    fighter->second_callback(object);
                }
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "dual-conditional-member-callbacks.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the object and member alias should survive the complete call sequence");
        let expected = [
            0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21, 0xff, 0xe8, 0x93, 0xe1,
            0x00, 0x14, 0x93, 0xc1, 0x00, 0x10, 0x7c, 0x7e, 0x1b, 0x78, 0x83, 0xe3, 0x00, 0x2c,
            0x7f, 0xe3, 0xfb, 0x78, 0x48, 0x00, 0x00, 0x01, 0x7f, 0xe3, 0xfb, 0x78, 0x48, 0x00,
            0x00, 0x01, 0x81, 0x9f, 0x21, 0xdc, 0x28, 0x0c, 0x00, 0x00, 0x41, 0x82, 0x00, 0x10,
            0x7d, 0x88, 0x03, 0xa6, 0x38, 0x7e, 0x00, 0x00, 0x4e, 0x80, 0x00, 0x21, 0x81, 0x9f,
            0x21, 0xe0, 0x28, 0x0c, 0x00, 0x00, 0x41, 0x82, 0x00, 0x10, 0x7d, 0x88, 0x03, 0xa6,
            0x38, 0x7e, 0x00, 0x00, 0x4e, 0x80, 0x00, 0x21, 0x80, 0x01, 0x00, 0x1c, 0x83, 0xe1,
            0x00, 0x14, 0x83, 0xc1, 0x00, 0x10, 0x38, 0x21, 0x00, 0x18, 0x7c, 0x08, 0x03, 0xa6,
            0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object
            .windows(expected.len())
            .any(|bytes| bytes == expected));
    }

    #[test]
    fn schedules_a_guarded_report_before_a_call_result_member_store() {
        let source = br#"
            struct Holder {
                int padding[2];
                void* value;
            };
            extern void report(const char*, ...);
            extern void assertion(const char*, int, const char*);
            extern void* load(void*);
            void attach(struct Holder* holder, void* source) {
                if (holder->value != 0) {
                    report("value already exists\n");
                    assertion("fixture.c", 10, "0");
                }
                holder->value = load(source);
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "guarded-report-store.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the guarded report should retain both call-crossing parameters");
        let expected = [
            0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21, 0xff, 0xe8, 0x93, 0xe1,
            0x00, 0x14, 0x3b, 0xe4, 0x00, 0x00, 0x93, 0xc1, 0x00, 0x10, 0x7c, 0x7e, 0x1b, 0x78,
            0x80, 0x03, 0x00, 0x08, 0x28, 0x00, 0x00, 0x00, 0x41, 0x82, 0x00, 0x28, 0x3c, 0x60,
            0x00, 0x00, 0x4c, 0xc6, 0x31, 0x82, 0x38, 0x63, 0x00, 0x00, 0x48, 0x00, 0x00, 0x01,
            0x3c, 0x60, 0x00, 0x00, 0x38, 0x63, 0x00, 0x00, 0x38, 0x80, 0x00, 0x0a, 0x38, 0xa0,
            0x00, 0x00, 0x48, 0x00, 0x00, 0x01, 0x7f, 0xe3, 0xfb, 0x78, 0x48, 0x00, 0x00, 0x01,
            0x90, 0x7e, 0x00, 0x08, 0x80, 0x01, 0x00, 0x1c, 0x83, 0xe1, 0x00, 0x14, 0x83, 0xc1,
            0x00, 0x10, 0x38, 0x21, 0x00, 0x18, 0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object
            .windows(expected.len())
            .any(|bytes| bytes == expected));
    }

    #[test]
    fn retains_a_shared_zero_across_a_bit_field_update_and_guard() {
        let source = br#"
            struct CommonData { void* padding; int value; };
            extern struct CommonData* common;
            struct Fighter {
                char prefix[6732];
                float timer;
                char first;
                char second;
                unsigned char guarded_zero;
                unsigned char guarded_value;
                char middle[2000];
                unsigned char b0 : 1;
                unsigned char b1 : 1;
                unsigned char b2 : 1;
                unsigned char b3 : 1;
                unsigned char b4 : 1;
                unsigned char b5 : 1;
                unsigned char enabled : 1;
            };
            void initialize(struct Fighter* fighter, int enabled, float timer) {
                fighter->timer = timer;
                fighter->second = 0;
                fighter->first = 0;
                fighter->enabled = enabled;
                if (fighter->enabled) {
                    fighter->guarded_zero = 0;
                    fighter->guarded_value = common->value;
                }
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "shared-zero-bit-field-guard.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the leading zero should remain live through the bit-field guard");
        let expected = [
            0xd0, 0x23, 0x1a, 0x4c, 0x38, 0xa0, 0x00, 0x00, 0x98, 0xa3, 0x1a, 0x51, 0x98, 0xa3,
            0x1a, 0x50, 0x88, 0x03, 0x22, 0x24, 0x50, 0x80, 0x0f, 0xbc, 0x98, 0x03, 0x22, 0x24,
            0x88, 0x03, 0x22, 0x24, 0x54, 0x00, 0xff, 0xff, 0x4d, 0x82, 0x00, 0x20, 0x98, 0xa3,
            0x1a, 0x52, 0x80, 0x80, 0x00, 0x00, 0x80, 0x04, 0x00, 0x04, 0x98, 0x03, 0x1a, 0x53,
            0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object
            .windows(expected.len())
            .any(|bytes| bytes == expected));
    }

    #[test]
    fn lowers_discarded_assignments_introduced_by_inline_aggregate_scalarization() {
        let source = br#"
            class Vec {
            public:
                float x;
                float y;
                float z;
            };
            class Holder {
            public:
                Vec source;
                Vec target;
            };
            inline void copy(Holder* holder) { holder->target = holder->source; }
            void compiled(Holder* holder) { copy(holder); }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::DEFAULT,
            flags,
        };
        let object = compile(
            source,
            "inline-copy.cpp",
            config,
            Some(SourceLanguage::Cxx),
            None,
            false,
        )
        .expect("scalarized inline assignments should lower as stores");
        assert!(!object.is_empty());
    }

    #[test]
    fn binds_aggregate_reference_from_struct_pointer_member() {
        let source = br#"
            struct Writer { int value; };
            struct Context { Writer* writer; };
            extern void consume(Writer&);
            void run(Context* context) {
                Writer& writer = *context->writer;
                consume(writer);
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::DEFAULT,
            flags,
        };
        let object = compile(
            source,
            "aggregate-reference.cpp",
            config,
            Some(SourceLanguage::Cxx),
            None,
            false,
        )
        .expect("aggregate reference binding should preserve the pointee address");
        assert!(!object.is_empty());
    }

    #[test]
    fn lowers_wii_member_linefeed_with_saved_float_window() {
        let source = br#"
            struct Writer {
                float cursorY() const;
                float lineHeight() const;
                void setCursor(float, float);
            };
            struct Context {
                Writer* writer;
                int padding;
                float xOrigin;
            };
            struct Handler { void linefeed(Context*); };
            void Handler::linefeed(Context* context) {
                (void)0;
                Writer& writer = *context->writer;
                float x = context->xOrigin;
                float y = writer.cursorY() + writer.lineHeight();
                writer.setCursor(x, y);
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        flags.inline_enabled = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::WII_1_0,
            flags,
        };
        let object = compile(
            source,
            "member-linefeed.cpp",
            config,
            Some(SourceLanguage::Cxx),
            None,
            false,
        )
        .expect("the structural Wii linefeed schedule should lower");
        assert!(object
            .windows(4)
            .any(|instruction| instruction == [0x94, 0x21, 0xff, 0xc0]));
    }

    #[test]
    fn lowers_wii_member_tab_with_integer_conversion_frame() {
        let source = br#"
            struct Writer {
                int tabWidth() const;
                int widthFixed() const;
                float fixedWidth() const;
                float fontWidth() const;
                float cursorX() const;
                void setCursorX(float);
            };
            struct Context {
                Writer* writer;
                int padding;
                float xOrigin;
            };
            struct Handler { void tab(Context*); };
            void Handler::tab(Context* context) {
                (void)0;
                Writer& writer = *context->writer;
                int tabWidth = writer.tabWidth();
                if (tabWidth > 0) {
                    float charWidth = writer.widthFixed()
                        ? writer.fixedWidth()
                        : writer.fontWidth();
                    float dx = writer.cursorX() - context->xOrigin;
                    float tabPixels = (float)tabWidth * charWidth;
                    int tabCount = (int)(dx / tabPixels) + 1;
                    float cursorX = tabPixels * (float)tabCount + context->xOrigin;
                    writer.setCursorX(cursorX);
                }
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        flags.inline_enabled = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::WII_1_0,
            flags,
        };
        let object = compile(
            source,
            "member-tab.cpp",
            config,
            Some(SourceLanguage::Cxx),
            None,
            false,
        )
        .expect("the structural Wii tab schedule should lower");
        assert!(object
            .windows(4)
            .any(|instruction| instruction == [0x94, 0x21, 0xff, 0x70]));
        assert!(object
            .windows(4)
            .any(|instruction| instruction == [0xfc, 0x00, 0x00, 0x1e]));
    }

    #[test]
    fn lowers_wii_writer_control_rectangle_switch() {
        let source = br#"
            struct Rect {
                float left;
                float top;
                float right;
                float bottom;
                void normalize();
            };
            struct Writer {
                float cursorX() const;
                float cursorY() const;
                float fontHeight() const;
            };
            struct Context {
                Writer* writer;
                int padding[4];
            };
            struct Handler {
                void linefeed(Context*);
                void tab(Context*);
                int rectangle(Rect*, unsigned short, Context*);
            };
            int Handler::rectangle(Rect* rect, unsigned short code, Context* context) {
                (void)0;
                (void)0;
                (void)0;
                switch (code) {
                case 10: {
                    Writer& writer = *context->writer;
                    rect->right = writer.cursorX();
                    rect->top = writer.cursorY();
                    linefeed(context);
                    rect->left = writer.cursorX();
                    rect->bottom = writer.cursorY() + context->writer->fontHeight();
                    rect->normalize();
                }
                    return 3;
                case 9: {
                    Writer& writer = *context->writer;
                    rect->left = writer.cursorX();
                    tab(context);
                    rect->right = writer.cursorX();
                    rect->top = writer.cursorY();
                    rect->bottom = rect->top + writer.fontHeight();
                    rect->normalize();
                }
                    return 1;
                default:
                    return 0;
                }
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        flags.inline_enabled = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::WII_1_0,
            flags,
        };
        let object = compile(
            source,
            "member-rect-control.cpp",
            config,
            Some(SourceLanguage::Cxx),
            None,
            false,
        )
        .expect("the structural Wii control-character rectangle should lower");
        assert!(object
            .windows(4)
            .any(|instruction| instruction == [0x94, 0x21, 0xff, 0xc0]));
        assert!(object
            .windows(4)
            .any(|instruction| instruction == [0x41, 0x82, 0x00, 0x14]));
    }

    #[test]
    fn spills_the_o0_virtual_destructor_deleting_flag() {
        let source = br#"
            template <typename T>
            struct Empty {
                virtual ~Empty();
            };
            template <typename T>
            Empty<T>::~Empty() {}
            template class Empty<char>;
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        flags.inline_enabled = false;
        flags.optimization = mwcc_versions::Optimization::O0;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::WII_1_0,
            flags,
        };
        let object = compile(
            source,
            "o0-virtual-destructor.cpp",
            config,
            Some(SourceLanguage::Cxx),
            None,
            false,
        )
        .expect("the unoptimized virtual destructor should lower");
        assert!(object
            .windows(4)
            .any(|instruction| instruction == [0x94, 0x21, 0xff, 0xe0]));
        assert!(object
            .windows(4)
            .any(|instruction| instruction == [0xb0, 0x81, 0x00, 0x08]));
        assert!(object
            .windows(4)
            .any(|instruction| instruction == [0xa8, 0x01, 0x00, 0x08]));
    }

    #[test]
    fn preserves_a_later_member_address_while_materializing_a_global_receiver() {
        let source = br#"
            class Sink {
            public:
                int payload;
                void Set(int*);
            };
            class Globals {
            public:
                int prefix;
                Sink sink;
            };
            class Actor {
            public:
                int prefix;
                int value;
            };
            extern Globals globals;
            inline Sink* sink() { return &globals.sink; }
            void compiled(Actor* actor) { sink()->Set(&actor->value); }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::DEFAULT,
            flags,
        };
        let object = compile(
            source,
            "endangered-member-address.cpp",
            config,
            Some(SourceLanguage::Cxx),
            None,
            false,
        )
        .expect("the incoming actor pointer must survive global address materialization");
        assert!(object
            .windows(4)
            .any(|bytes| bytes == [0x7c, 0x65, 0x1b, 0x78])); // mr r5,r3
        assert!(object
            .windows(4)
            .any(|bytes| bytes == [0x38, 0x85, 0x00, 0x04])); // addi r4,r5,4
    }

    #[test]
    fn preserves_a_store_base_while_chasing_its_value_member_chain() {
        let source = br#"
            struct Fighter;
            struct Object {
                int padding[11];
                struct Fighter* user_data;
            };
            struct Fighter {
                float facing;
                char padding[40];
                struct Object* victim;
            };
            void compiled(struct Fighter* fighter) {
                fighter->facing = fighter->victim->user_data->facing;
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::DEFAULT,
            flags,
        };
        let object = compile(
            source,
            "member-chain-store.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the target object must survive evaluation of its value chain");
        let expected = [
            0x80, 0x83, 0x00, 0x2c, // lwz r4,44(r3): fighter->victim
            0x80, 0x84, 0x00, 0x2c, // lwz r4,44(r4): victim->user_data
            0xc0, 0x04, 0x00, 0x00, // lfs f0,0(r4): nested facing
            0xd0, 0x03, 0x00, 0x00, // stfs f0,0(r3): original fighter
        ];
        assert!(object
            .windows(expected.len())
            .any(|bytes| bytes == expected));
    }

    #[test]
    fn lowers_an_inlined_pointer_parameter_based_on_an_embedded_member_address() {
        let source = br#"
            class Status {
            public:
                int actor;
            };
            class Collider {
            public:
                Status* status;
                void SetStatus(Status* value) { status = value; }
            };
            class Actor {
            public:
                int prefix;
                Status status;
                Collider collider;
            };
            void compiled(Actor* actor) {
                actor->collider.SetStatus(&actor->status);
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::DEFAULT,
            flags,
        };
        let object = compile(
            source,
            "embedded-member-pointer.cpp",
            config,
            Some(SourceLanguage::Cxx),
            None,
            false,
        )
        .expect("the substituted pointer parameter should retain its lvalue address");
        assert!(!object.is_empty());
    }

    #[test]
    fn reuses_a_guard_member_and_splits_the_selected_arm_pointer() {
        let source = br#"
            class Vec { public: float x; float y; float z; };
            class Sphere { public: Vec center; };
            class Sink { public: int payload; void Set(Sphere*); };
            class Globals { public: int prefix; Sink sink; };
            class Actor {
            public:
                char prefix[504];
                Vec position;
                char middle[150];
                short timer;
                char gap[60];
                Sphere sphere;
            };
            extern Globals globals;
            inline Sink* sink() { return &globals.sink; }
            extern void remove(Actor*);
            int compiled(Actor* actor) {
                if (actor->timer != 0) {
                    actor->timer--;
                    actor->sphere.center.x = actor->position.x;
                    actor->sphere.center.y = actor->position.y;
                    actor->sphere.center.z = actor->position.z;
                    sink()->Set(&actor->sphere);
                } else {
                    remove(actor);
                }
                return 1;
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_3_2,
            flags,
        };
        let object = compile(
            source,
            "guarded-member.cpp",
            config,
            Some(SourceLanguage::Cxx),
            None,
            false,
        )
        .expect("the guarded member live range should lower");
        let expected_text = [
            0x94, 0x21, 0xff, 0xf0, 0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x14, 0x7c, 0x65,
            0x1b, 0x78, 0xa8, 0x83, 0x02, 0x9a, 0x2c, 0x04, 0x00, 0x00, 0x41, 0x82, 0x00, 0x38,
            0x38, 0x04, 0xff, 0xff, 0xb0, 0x05, 0x02, 0x9a, 0xc0, 0x05, 0x01, 0xf8, 0xd0, 0x05,
            0x02, 0xd8, 0xc0, 0x05, 0x01, 0xfc, 0xd0, 0x05, 0x02, 0xdc, 0xc0, 0x05, 0x02, 0x00,
            0xd0, 0x05, 0x02, 0xe0, 0x38, 0x60, 0x00, 0x00, 0x38, 0x63, 0x00, 0x04, 0x38, 0x85,
            0x02, 0xd8, 0x48, 0x00, 0x00, 0x01, 0x48, 0x00, 0x00, 0x08, 0x48, 0x00, 0x00, 0x01,
            0x38, 0x60, 0x00, 0x01, 0x80, 0x01, 0x00, 0x14, 0x7c, 0x08, 0x03, 0xa6, 0x38, 0x21,
            0x00, 0x10, 0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object
            .windows(expected_text.len())
            .any(|bytes| bytes == expected_text));
    }

    #[test]
    fn preserves_a_tested_call_result_until_the_function_return() {
        let source = br#"
            extern int load_result(void);
            extern void consume(void);
            int compiled(void) {
                int result = load_result();
                if (result == 4) {
                    consume();
                }
                return result;
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_3_2,
            flags,
        };
        let object = compile(
            source,
            "returned-call-result.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the call result must survive the conditional call");

        assert!(object
            .windows(4)
            .any(|bytes| bytes == [0x7c, 0x7f, 0x1b, 0x78])); // mr r31,r3
        assert!(object
            .windows(4)
            .any(|bytes| bytes == [0x7f, 0xe3, 0xfb, 0x78])); // mr r3,r31
    }

    #[test]
    fn tests_a_discarded_placement_address_through_the_r0_scratch() {
        let source = br#"
            class Base { public: Base(); };
            class Derived : public Base {};
            extern int consume(Derived*);
            int compiled(Derived* object) {
                new (object) Derived();
                return consume(object);
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_3_2,
            flags,
        };
        let object = compile(
            source,
            "placement-scratch.cpp",
            config,
            Some(SourceLanguage::Cxx),
            None,
            false,
        )
        .expect("discarded placement construction should retain its null guard");

        assert!(object
            .windows(4)
            .any(|bytes| bytes == [0x7f, 0xe0, 0xfb, 0x79])); // mr. r0,r31
    }

    #[test]
    fn scalar_array_layout_and_comment_alignment_are_independent() {
        assert_eq!(
            global_alignments(1, None, true, false, 1, true, 4),
            GlobalAlignments {
                layout: 4,
                comment: 1,
            }
        );
        assert_eq!(
            global_alignments(2, None, true, false, 1, true, 4),
            GlobalAlignments {
                layout: 4,
                comment: 2,
            }
        );
        assert_eq!(
            global_alignments(1, None, true, false, 32, true, 4),
            GlobalAlignments {
                layout: 32,
                comment: 32,
            }
        );
        assert_eq!(
            global_alignments(1, None, true, false, 1, false, 4),
            GlobalAlignments {
                layout: 4,
                comment: 4,
            }
        );
        assert_eq!(
            global_alignments(14, Some(1), false, false, 1, false, 4),
            GlobalAlignments {
                layout: 1,
                comment: 1,
            }
        );
        assert_eq!(
            global_alignments(4, Some(1), true, false, 1, false, 4),
            GlobalAlignments {
                layout: 4,
                comment: 4,
            }
        );
        assert_eq!(
            global_alignments(24, Some(4), false, false, 1, false, 8),
            GlobalAlignments {
                layout: 4,
                comment: 8,
            }
        );
        assert_eq!(
            global_alignments(12, Some(4), false, true, 1, true, 8),
            GlobalAlignments {
                layout: 4,
                comment: 4,
            }
        );
    }

    #[test]
    fn command_line_enum_storage_is_last_wins() {
        let minimum = parse_invocation(&["-enum".into(), "min".into()]);
        assert_eq!(minimum.flags.enum_storage, EnumStorage::Minimum);

        let integer =
            parse_invocation(&["-enum".into(), "min".into(), "-enum".into(), "int".into()]);
        assert_eq!(integer.flags.enum_storage, EnumStorage::Int);
    }

    #[test]
    fn command_line_rtti_is_last_wins() {
        let enabled = parse_invocation(&["-RTTI".into(), "on".into()]);
        assert!(enabled.flags.rtti);

        let disabled = parse_invocation(&[
            "-RTTI".into(),
            "on".into(),
            "-RTTI".into(),
            "off".into(),
        ]);
        assert!(!disabled.flags.rtti);
    }

    #[test]
    fn command_line_language_accepts_both_forms_and_is_last_wins() {
        let equals = parse_invocation(&["-lang=c".into()]);
        assert_eq!(equals.source_language, Some(SourceLanguage::C));

        let split = parse_invocation(&["-lang".into(), "c++".into()]);
        assert_eq!(split.source_language, Some(SourceLanguage::Cxx));

        let last_wins = parse_invocation(&["-lang=c++".into(), "-lang".into(), "c".into()]);
        assert_eq!(last_wins.source_language, Some(SourceLanguage::C));
    }

    #[test]
    fn command_line_include_paths_preserve_search_order_and_forms() {
        let parsed = parse_invocation(&[
            "-i".into(),
            "first".into(),
            "-Isecond".into(),
            "-ir".into(),
            "third".into(),
            "-isystem".into(),
            "fourth".into(),
            "-inline".into(),
            "auto".into(),
        ]);
        assert_eq!(
            parsed.include_paths,
            ["first", "second", "third", "fourth"].map(std::path::PathBuf::from)
        );
    }

    #[test]
    fn command_line_inline_mode_is_last_wins() {
        let parsed = parse_invocation(&[
            "-inline".into(),
            "auto,deferred".into(),
            "-inline".into(),
            "off".into(),
        ]);
        assert!(!parsed.flags.inline_enabled);
        assert!(!parsed.flags.inline_deferred);

        let parsed = parse_invocation(&[
            "-inline".into(),
            "off".into(),
            "-inline".into(),
            "auto".into(),
        ]);
        assert!(parsed.flags.inline_enabled);
        assert!(!parsed.flags.inline_deferred);

        let parsed = parse_invocation(&[
            "-inline".into(),
            "auto,".into(),
            "deferred".into(),
        ]);
        assert!(parsed.flags.inline_enabled);
        assert!(parsed.flags.inline_deferred);
    }

    #[test]
    fn command_line_macro_definitions_support_mwcc_and_standard_forms() {
        let parsed = parse_invocation(&[
            "-DVERSION=2".into(),
            "-d".into(),
            "FEATURE".into(),
            "-D".into(),
            "REMOVED=1".into(),
            "-UREMOVED".into(),
        ]);
        assert_eq!(
            parsed.preprocessor_definitions.get("VERSION"),
            Some(&String::from("2"))
        );
        assert_eq!(
            parsed.preprocessor_definitions.get("FEATURE"),
            Some(&String::from("1"))
        );
        assert!(!parsed.preprocessor_definitions.contains_key("REMOVED"));
    }

    #[test]
    fn command_line_cats_pragma_controls_object_catalogs() {
        let off = parse_invocation(&["-pragma".into(), "cats off".into()]);
        assert!(!off.flags.emit_mwcats);

        let last_wins = parse_invocation(&[
            "-pragma".into(),
            "cats off".into(),
            "-pragma".into(),
            "cats on".into(),
        ]);
        assert!(last_wins.flags.emit_mwcats);
    }

    #[test]
    fn command_line_string_mode_controls_read_only_literals() {
        let read_only = parse_invocation(&["-str".into(), "reuse,pool,readonly".into()]);
        assert!(read_only.flags.string_literals_read_only);
        assert!(read_only.flags.string_literals_packed);

        let restated_pooling = parse_invocation(&[
            "-str".into(),
            "reuse,readonly".into(),
            "-str".into(),
            "reuse".into(),
        ]);
        assert!(restated_pooling.flags.string_literals_read_only);

        let modern = parse_invocation(&["-rostr".into(), "-str".into(), "reuse".into()]);
        assert!(modern.flags.string_literals_read_only);

        let writable_override = parse_invocation(&[
            "-str".into(),
            "reuse,readonly".into(),
            "-str".into(),
            "noreadonly".into(),
        ]);
        assert!(!writable_override.flags.string_literals_read_only);

        let unpacked = parse_invocation(&[
            "-str".into(),
            "pool".into(),
            "-str".into(),
            "nopool".into(),
        ]);
        assert!(!unpacked.flags.string_literals_packed);
    }

    #[test]
    fn packed_string_call_schedules_member_address_last() {
        let source = br#"
            struct Item { int padding; int value; };
            extern void consume(int*, const char*);
            int run(struct Item* item) {
                consume(&item->value, "name");
                return 1;
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.cpp_exceptions = false;
        flags.debug_info = false;
        flags.emit_mwcats = false;
        flags.string_literals_read_only = true;
        flags.string_literals_packed = true;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::DEFAULT,
            flags,
        };
        let object = compile(source, "packed.c", config, Some(SourceLanguage::C), None, false)
            .expect("the packed-string call should compile");
        let word_position = |expected: [u8; 4]| {
            object
                .chunks_exact(4)
                .position(|word| word == expected)
                .expect("expected instruction")
        };
        let string_high = word_position([0x3c, 0x80, 0x00, 0x00]);
        let string_low = word_position([0x38, 0x84, 0x00, 0x00]);
        let member = word_position([0x38, 0x63, 0x00, 0x04]);
        assert!(string_high < string_low);
        assert_eq!(member, string_low + 1);
    }

    #[test]
    fn schedules_member_float_absolute_value_before_the_first_call_argument() {
        let source = br#"
            struct Stick { float x; float y; };
            struct Fighter { char padding[1568]; struct Stick stick; };
            extern float atan2f(float, float);
            float compiled(struct Fighter* fighter) {
                return atan2f(
                    fighter->stick.y,
                    fighter->stick.x < 0
                        ? -fighter->stick.x
                        : fighter->stick.x);
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        flags.emit_mwcats = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "float-abs-arguments.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the member-backed absolute value call should compile");
        let expected_text = [
            0x7c, 0x08, 0x02, 0xa6, // mflr r0
            0x90, 0x01, 0x00, 0x04, // stw r0,4(r1)
            0x94, 0x21, 0xff, 0xf8, // stwu r1,-8(r1)
            0xc0, 0x43, 0x06, 0x20, // lfs f2,1568(r3)
            0xc0, 0x00, 0x00, 0x00, // lfs f0,@zero
            0xfc, 0x02, 0x00, 0x40, // fcmpo cr0,f2,f0
            0x40, 0x80, 0x00, 0x08, // bge nonnegative
            0xfc, 0x40, 0x10, 0x50, // fneg f2,f2
            0xc0, 0x23, 0x06, 0x24, // lfs f1,1572(r3)
            0x48, 0x00, 0x00, 0x01, // bl atan2f
            0x80, 0x01, 0x00, 0x0c, // lwz r0,12(r1)
            0x38, 0x21, 0x00, 0x08, // addi r1,r1,8
            0x7c, 0x08, 0x03, 0xa6, // mtlr r0
            0x4e, 0x80, 0x00, 0x20, // blr
        ];
        assert!(object
            .windows(expected_text.len())
            .any(|bytes| bytes == expected_text));
    }

    #[test]
    fn folds_a_saved_receiver_early_return_and_schedules_bit_field_arguments() {
        let source = br#"
            typedef unsigned char u8;
            struct Fighter {
                char pad0[8];
                int spawn;
                u8 player;
                char pad1[163];
                float position_x;
                char pad2[44];
                int ground_or_air;
                char pad3[6080];
                float knockback;
                char pad4[192];
                u8 jumps_used;
                char pad5[2230];
                u8 flag_a0 : 1;
                u8 flag_a1 : 1;
                u8 flag_a2 : 1;
                u8 flag_a3 : 1;
                u8 flag_a4 : 1;
                u8 flag_a5 : 1;
                u8 flag_a6 : 1;
                u8 flag_a7 : 1;
                char pad6[7];
                u8 guard : 1;
            };
            extern int test_knockback(int, float, float);
            extern void record_jump(u8, int);
            void compiled(struct Fighter* fighter) {
                if (fighter->ground_or_air != 1) {
                    return;
                }
                if (test_knockback(fighter->spawn, fighter->position_x,
                                   fighter->knockback)) {
                    fighter->knockback = 0;
                }
                if (fighter->guard && fighter->jumps_used <= 1) {
                    record_jump(fighter->player, fighter->flag_a4);
                }
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        flags.emit_mwcats = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "structured-early-return.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the saved-receiver branch and calls should compile");
        let expected_text = [
            0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21, 0xff, 0xe8, 0x93,
            0xe1, 0x00, 0x14, 0x7c, 0x7f, 0x1b, 0x78, 0x80, 0x03, 0x00, 0xe0, 0x2c, 0x00,
            0x00, 0x01, 0x40, 0x82, 0x00, 0x4c, 0x80, 0x7f, 0x00, 0x08, 0xc0, 0x3f, 0x00,
            0xb0, 0xc0, 0x5f, 0x18, 0xa4, 0x48, 0x00, 0x00, 0x01, 0x2c, 0x03, 0x00, 0x00,
            0x41, 0x82, 0x00, 0x0c, 0xc0, 0x00, 0x00, 0x00, 0xd0, 0x1f, 0x18, 0xa4, 0x88,
            0x1f, 0x22, 0x27, 0x54, 0x00, 0xcf, 0xff, 0x41, 0x82, 0x00, 0x20, 0x88, 0x1f,
            0x19, 0x68, 0x28, 0x00, 0x00, 0x01, 0x41, 0x81, 0x00, 0x14, 0x88, 0x9f, 0x22,
            0x1f, 0x88, 0x7f, 0x00, 0x0c, 0x54, 0x84, 0xef, 0xfe, 0x48, 0x00, 0x00, 0x01,
            0x80, 0x01, 0x00, 0x1c, 0x83, 0xe1, 0x00, 0x14, 0x38, 0x21, 0x00, 0x18, 0x7c,
            0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object
            .windows(expected_text.len())
            .any(|bytes| bytes == expected_text));
    }

    #[test]
    fn schedules_a_zero_store_before_two_saved_receiver_calls() {
        let source = br#"
            struct Fighter {
                char pad0[56];
                float scale_y;
                char pad1[6464];
                void* target;
            };
            struct Object { char pad[44]; struct Fighter* user_data; };
            extern void first(struct Object*);
            extern void second(struct Object*, float);
            void compiled(struct Object* object) {
                struct Fighter* fighter = object->user_data;
                fighter->target = 0;
                first(object);
                second(object, fighter->scale_y);
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        flags.emit_mwcats = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "entry-zero-store.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the entry zero-store schedule should compile");
        let expected_text = [
            0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x38, 0x00, 0x00, 0x00, 0x94,
            0x21, 0xff, 0xe8, 0x93, 0xe1, 0x00, 0x14, 0x93, 0xc1, 0x00, 0x10, 0x7c, 0x7e,
            0x1b, 0x78, 0x83, 0xe3, 0x00, 0x2c, 0x90, 0x1f, 0x19, 0x7c, 0x48, 0x00, 0x00,
            0x01, 0x7f, 0xc3, 0xf3, 0x78, 0xc0, 0x3f, 0x00, 0x38, 0x48, 0x00, 0x00, 0x01,
            0x80, 0x01, 0x00, 0x1c, 0x83, 0xe1, 0x00, 0x14, 0x83, 0xc1, 0x00, 0x10, 0x38,
            0x21, 0x00, 0x18, 0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object
            .windows(expected_text.len())
            .any(|bytes| bytes == expected_text));
    }

    #[test]
    fn folds_an_immutable_embedded_member_pointer_into_scalar_accesses() {
        let source = br#"
            struct Vec2 { float x; float y; };
            struct CollData { char pad[340]; struct Vec2 normal; };
            struct Fighter {
                char pad0[152];
                struct Vec2 shield_knockback;
                char pad1[84];
                float ground_velocity;
                char pad2[1528];
                struct CollData collision;
            };
            struct Object { char pad[44]; struct Fighter* user_data; };
            struct Object* compiled(struct Object* object) {
                struct Fighter* fighter = object->user_data;
                struct CollData* collision = &fighter->collision;
                fighter->shield_knockback.x =
                    collision->normal.y * fighter->ground_velocity;
                fighter->shield_knockback.y =
                    -collision->normal.x * fighter->ground_velocity;
                return object;
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        flags.emit_mwcats = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "embedded-member-alias.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the immutable embedded-member pointer should fold");
        let expected_text = [
            0x80, 0x83, 0x00, 0x2c, 0xc0, 0x04, 0x00, 0xf4, 0xc0, 0x24, 0x08, 0x48, 0xec,
            0x01, 0x00, 0x32, 0xd0, 0x04, 0x00, 0x98, 0xc0, 0x24, 0x08, 0x44, 0xc0, 0x04,
            0x00, 0xf4, 0xfc, 0x20, 0x08, 0x50, 0xec, 0x01, 0x00, 0x32, 0xd0, 0x04, 0x00,
            0x9c, 0x4e, 0x80, 0x00, 0x20,
        ];
        assert!(object
            .windows(expected_text.len())
            .any(|bytes| bytes == expected_text));
    }

    #[test]
    fn classifies_integer_zero_from_a_member_call_by_the_float_prototype() {
        let source = br#"
            struct Node { int value; };
            struct Object { char pad[8352]; struct Node* node; };
            extern void consume(struct Node*, float);
            void compiled(struct Object* object) {
                consume(object->node, 0);
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        flags.emit_mwcats = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "member-float-zero-call.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the integer zero should use the prototype's floating argument class");
        let member_load_float_zero_call = [
            0x80, 0x63, 0x20, 0xa0, // lwz r3,8352(r3)
            0xc0, 0x20, 0x00, 0x00, // lfs f1,@zero@sda21
            0x48, 0x00, 0x00, 0x01, // bl consume
        ];
        assert!(object
            .windows(member_load_float_zero_call.len())
            .any(|bytes| bytes == member_load_float_zero_call));
    }

    #[test]
    fn retains_a_shared_zero_across_two_vector_product_groups() {
        let source = br#"
            struct Vec3 { float x; float y; float z; };
            struct CollData { char pad[340]; struct Vec3 normal; };
            struct Fighter {
                char pad0[116];
                struct Vec3 anim_velocity;
                struct Vec3 self_velocity;
                char pad1[88];
                float ground_acceleration;
                char pad2[4];
                float ground_velocity;
                char pad3[1536];
                struct CollData collision;
            };
            struct Object { char pad[44]; struct Fighter* user_data; };
            struct Object* compiled(struct Object* object) {
                struct Fighter* fighter = object->user_data;
                struct Vec3* normal = &fighter->collision.normal;
                fighter->anim_velocity.x = normal->y * fighter->ground_acceleration;
                fighter->anim_velocity.y = -normal->x * fighter->ground_acceleration;
                fighter->anim_velocity.z = 0;
                fighter->self_velocity.x = normal->y * fighter->ground_velocity;
                fighter->self_velocity.y = -normal->x * fighter->ground_velocity;
                fighter->self_velocity.z = 0;
                return object;
            }
        "#;
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.cpp_exceptions = false;
        flags.emit_mwcats = false;
        let config = mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        };
        let object = compile(
            source,
            "shared-vector-zero.c",
            config,
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("the repeated zero should stay live across the product group");
        let expected_text = [
            0x80, 0x83, 0x00, 0x2c, 0xc0, 0x04, 0x00, 0xe4, 0xc0, 0x24, 0x08, 0x48, 0xec,
            0x01, 0x00, 0x32, 0xd0, 0x04, 0x00, 0x74, 0xc0, 0x24, 0x08, 0x44, 0xc0, 0x04,
            0x00, 0xe4, 0xfc, 0x20, 0x08, 0x50, 0xec, 0x01, 0x00, 0x32, 0xd0, 0x04, 0x00,
            0x78, 0xc0, 0x40, 0x00, 0x00, 0xd0, 0x44, 0x00, 0x7c, 0xc0, 0x24, 0x08, 0x48,
            0xc0, 0x04, 0x00, 0xec, 0xec, 0x01, 0x00, 0x32, 0xd0, 0x04, 0x00, 0x80, 0xc0,
            0x24, 0x08, 0x44, 0xc0, 0x04, 0x00, 0xec, 0xfc, 0x20, 0x08, 0x50, 0xec, 0x01,
            0x00, 0x32, 0xd0, 0x04, 0x00, 0x84, 0xd0, 0x44, 0x00, 0x88, 0x4e, 0x80, 0x00,
            0x20,
        ];
        assert!(object
            .windows(expected_text.len())
            .any(|bytes| bytes == expected_text));
    }

    #[test]
    fn command_line_pool_mode_is_last_wins() {
        let off = parse_invocation(&["-pool".into(), "off".into()]);
        assert!(!off.flags.pooling_enabled);

        let last_wins =
            parse_invocation(&["-pool".into(), "off".into(), "-pool".into(), "on".into()]);
        assert!(last_wins.flags.pooling_enabled);
    }

    #[test]
    fn command_line_lmw_stmw_mode_is_last_wins() {
        let on = parse_invocation(&["-use_lmw_stmw".into(), "on".into()]);
        assert!(on.flags.use_lmw_stmw);

        let last_wins = parse_invocation(&[
            "-use_lmw_stmw".into(),
            "on".into(),
            "-use_lmw_stmw".into(),
            "off".into(),
        ]);
        assert!(!last_wins.flags.use_lmw_stmw);
    }

    #[test]
    fn command_line_debug_info_mode_is_last_wins() {
        let on = parse_invocation(&["-sym".into(), "on".into()]);
        assert!(on.flags.debug_info);

        let last_wins =
            parse_invocation(&["-sym".into(), "on".into(), "-sym".into(), "off".into()]);
        assert!(!last_wins.flags.debug_info);
    }

    #[test]
    fn command_line_scheduler_mode_is_last_wins() {
        let off = parse_invocation(&["-schedule".into(), "off".into()]);
        assert!(!off.flags.scheduler_enabled);

        let last_wins = parse_invocation(&[
            "-schedule".into(),
            "off".into(),
            "-schedule".into(),
            "on".into(),
        ]);
        assert!(last_wins.flags.scheduler_enabled);
    }

    #[test]
    fn command_line_fp_contract_mode_is_last_wins() {
        let off = parse_invocation(&["-fp_contract".into(), "off".into()]);
        assert!(!off.flags.fp_contract);

        let last_wins = parse_invocation(&[
            "-fp_contract".into(),
            "off".into(),
            "-fp_contract".into(),
            "on".into(),
        ]);
        assert!(last_wins.flags.fp_contract);
    }

    #[test]
    fn command_line_ipa_mode_is_last_wins() {
        let file = parse_invocation(&["-ipa".into(), "file".into()]);
        assert!(file.flags.ipa_file);

        let last_wins =
            parse_invocation(&["-ipa".into(), "file".into(), "-ipa".into(), "off".into()]);
        assert!(!last_wins.flags.ipa_file);
    }

    #[test]
    fn command_line_opt_off_overrides_an_earlier_level() {
        let parsed = parse_invocation(&[
            "-O4,p".into(),
            "-ipa".into(),
            "file".into(),
            "-opt".into(),
            "off".into(),
        ]);
        assert_eq!(parsed.flags.optimization, mwcc_versions::Optimization::O0);
        assert!(parsed.flags.ipa_file);
        assert!(!parsed.flags.whole_file_optimization_enabled());
    }

    #[test]
    fn command_line_optimization_goal_is_last_wins() {
        let size = parse_invocation(&["-O4,s".into()]);
        assert_eq!(
            size.flags.optimization_goal,
            mwcc_versions::OptimizationGoal::Size
        );

        let performance = parse_invocation(&["-O4,space".into(), "-O3,p".into()]);
        assert_eq!(
            performance.flags.optimization_goal,
            mwcc_versions::OptimizationGoal::Performance
        );

        let unsuffixed = parse_invocation(&["-O4,s".into(), "-O2".into()]);
        assert_eq!(
            unsuffixed.flags.optimization_goal,
            mwcc_versions::OptimizationGoal::Performance
        );
    }

    #[test]
    fn command_line_function_alignment_is_last_valid_value() {
        let parsed = parse_invocation(&[
            "-func_align".into(),
            "32".into(),
            "-func_align".into(),
            "4".into(),
        ]);
        assert_eq!(parsed.flags.function_alignment, Some(4));

        let invalid = parse_invocation(&["-func_align".into(), "3".into()]);
        assert_eq!(invalid.flags.function_alignment, None);
    }

    #[test]
    fn command_line_small_data_areas_are_independent_and_last_wins() {
        let split = parse_invocation(&["-sdata".into(), "8".into(), "-sdata2".into(), "0".into()]);
        assert_eq!(split.flags.global_addressing, GlobalAddressing::SmallData);
        assert_eq!(
            split.flags.read_only_global_addressing,
            GlobalAddressing::Absolute
        );

        let last_wins = parse_invocation(&[
            "-sdata".into(),
            "0".into(),
            "-sdata2".into(),
            "0".into(),
            "-sdata".into(),
            "8".into(),
            "-sdata2".into(),
            "8".into(),
        ]);
        assert_eq!(
            last_wins.flags.global_addressing,
            GlobalAddressing::SmallData
        );
        assert_eq!(
            last_wins.flags.read_only_global_addressing,
            GlobalAddressing::SmallData
        );
    }
}

fn artifact_dump(directory: &std::path::Path, name: &str, body: String) {
    let _ = std::fs::write(directory.join(name), body);
}

fn write_token_artifacts(
    directory: &str,
    config: mwcc_versions::CompilerConfig,
    tokens: &[mwcc_tokens::Token],
) {
    let directory = PathBuf::from(directory);
    let _ = std::fs::create_dir_all(&directory);

    // The build identity, then the resolved behavior's *active quirks* — exactly
    // what diverges from the 2.4.x mainline for this configuration, and why. A
    // plain mainline build lists none; a quirk-bearing one names each, tagged as
    // a deliberate version difference or a reproduced bug.
    let build = config.build;
    let behavior = mwcc_versions::Behavior::resolve(&config);
    let mut report = format!(
        "{} — {} (version {:?} build {})\n",
        build.label, build.product, build.version, build.build
    );
    let quirks = behavior.active_quirks();
    if quirks.is_empty() {
        report.push_str("active quirks: none (2.4.x mainline behavior)\n");
    } else {
        report.push_str("active quirks:\n");
        for active in quirks {
            report.push_str(&format!(
                "  - {:?} [{:?}]: {}\n",
                active.quirk, active.kind, active.summary
            ));
        }
    }
    artifact_dump(&directory, "00_build.txt", report);
    artifact_dump(
        &directory,
        "01_tokens.txt",
        tokens.iter().map(|token| format!("{token}\n")).collect(),
    );
}

fn write_lowered_artifacts(
    directory: &str,
    functions: &[mwcc_syntax_trees::Function],
    machine_functions: &[mwcc_machine_code::MachineFunction],
    object: &[u8],
) {
    let directory = PathBuf::from(directory);
    let _ = std::fs::create_dir_all(&directory);
    artifact_dump(&directory, "02_syntax_tree.txt", format!("{functions:#?}\n"));
    artifact_dump(
        &directory,
        "03_machine_code.txt",
        machine_functions
            .iter()
            .map(|machine_code| {
                let body: String = machine_code
                    .instructions
                    .iter()
                    .map(|instruction| format!("{:08x}  {instruction:?}\n", instruction.encode()))
                    .collect();
                format!("{}:\n{body}\n", machine_code.name)
            })
            .collect(),
    );
    artifact_dump(
        &directory,
        "04_object.txt",
        format!(
            "ELF32 BE PowerPC relocatable object, {} bytes\n",
            object.len()
        ),
    );
}
