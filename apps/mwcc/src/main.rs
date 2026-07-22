//! `mwcc` — the compiler driver.
//!
//! The command line is intentionally compatible with `mwcceppc` so the oracle
//! harness can swap us in: `mwcc [flags...] -c <input.c> -o <output.o>`. Flags we
//! do not yet model are ignored. `--emit-artifacts <dir>` writes a per-phase
//! report for inspecting how a translation unit becomes bytes.

mod cxx_analysis_residues;
mod cxx_rtti_names;
mod function_order;
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
    /// Codegen-affecting flags parsed from the real build line; the rest are
    /// ignored (they are the preprocessor's or diagnostics' concern).
    flags: mwcc_versions::Flags,
}

fn parse_invocation(arguments: &[String]) -> Invocation {
    use mwcc_versions::{CharDefault, EnumStorage, GlobalAddressing, Optimization};
    let mut invocation = Invocation {
        input: None,
        output: None,
        build_label: None,
        artifacts_directory: None,
        source_language: None,
        flags: mwcc_versions::Flags::default(),
    };
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
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
                if arguments
                    .get(index)
                    .is_some_and(|value| value.split(',').any(|part| part == "deferred"))
                {
                    invocation.flags.inline_deferred = true;
                }
            }
            // `-str reuse,readonly` pools string literals in read-only data.
            // A later `-str reuse` only restates the pooling policy; it does not
            // cancel a standalone `-rostr` (the GC 3.0 project lines use both).
            "-str" => {
                index += 1;
                if arguments
                    .get(index)
                    .is_some_and(|value| value.split(',').any(|part| part == "readonly"))
                {
                    invocation.flags.string_literals_read_only = true;
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
            argument if argument.ends_with(".c") && invocation.input.is_none() => {
                invocation.input = Some(argument.to_string());
            }
            _ => {} // ignore flags we do not yet model
        }
        index += 1;
    }
    invocation
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

    let source = match std::fs::read(&input) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("mwcc: cannot read {input}: {error}");
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
    requested_alignment: u32,
    unoptimized: bool,
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
) -> Compilation<Vec<u8>> {
    let located_tokens = mwcc_source_to_tokens::tokenize_bytes_located(source)?;
    let tokens: Vec<mwcc_tokens::Token> = located_tokens
        .iter()
        .map(|located| located.token.clone())
        .collect();
    let behavior = mwcc_versions::Behavior::resolve(&config);
    let is_cxx = match source_language {
        Some(SourceLanguage::C) => false,
        Some(SourceLanguage::Cxx) => true,
        None => matches!(
            std::path::Path::new(source_name)
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("cpp" | "cp" | "cxx" | "cc")
        ),
    };
    let mut unit = mwcc_tokens_to_syntax_trees::parse_located_translation_unit_with_enum_min(
        located_tokens,
        is_cxx,
        config.char_is_signed(),
        behavior.plain_inline_localstatic_base,
        behavior.skipped_static_inline_label_base,
        config.flags.enum_storage == mwcc_versions::EnumStorage::Minimum,
    )?;
    if is_cxx && config.flags.rtti {
        mwcc_tokens_to_syntax_trees::materialize_cxx_rtti(&mut unit);
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
    let weak_materialized_names: std::collections::HashSet<String> =
        unit.weak_materialized.iter().cloned().collect();
    let prototyped_names: std::collections::HashSet<String> = unit
        .prototypes
        .iter()
        .map(|(name, _, _)| name.clone())
        .collect();
    let inline_summaries = mwcc_syntax_trees_to_machine_code::InlineSummaries::analyze_with_skipped(
        &unit.functions,
        &unit.skipped_inline_definitions,
    );
    let inline_bodies = mwcc_syntax_trees_to_machine_code::InlineBodySet::analyze_with_definitions(
        &unit.functions,
        &unit.skipped_inline_definitions,
    );
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
        match mwcc_syntax_trees_to_machine_code::lower_function(
            function,
            &unit.globals,
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
            config,
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
        + cxx_inline_facts.control_flow_labels
            * usize::from(behavior.cxx_inline_control_flow_label_weight)
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
    // Deferred inlining has its own translation-unit emission schedule. Keep the
    // policy isolated from lowering and object layout: both consume its result.
    if config.flags.inline_deferred {
        function_order::apply_deferred_emission_order(
            &mut machine_functions,
            behavior.deferred_transparent_leaf_bump,
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
            // A `static` (non-const) symbol ARRAY whose targets are ALL unit functions
            // (`static void (*tbl[])(void) = { e1, e2 };` — item.c's dispatch tables):
            // measured layout is the table LOCAL in the local-statics run and the
            // hoisted callee FUNC symbols at the table's source position in the GLOBAL
            // run — both handled by the writer now, so it passes through. A CONST
            // table (.sdata2/.rodata) and a static table with EXTERN targets (their
            // undef-symbol placement is unmeasured) keep the defer.
            let static_unit_function_table = global.is_static
                && !global.is_const
                && global.array_length.is_some()
                && elements.iter().all(|element| {
                    matches!(element, PointerElement::Symbol(name)
                        if machine_functions.iter().any(|function| &function.name == name))
                        || matches!(element, PointerElement::Null)
                });
            if (global.is_static || global.is_const)
                && global.section.is_none()
                && !single_target
                && !all_null
                && !static_unit_function_table
            {
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
                // or a static unit-function TABLE binds LOCAL; a plain writable
                // pointer global stays GLOBAL as before.
                is_static: global.is_static
                    && (global.section.is_some() || global.is_const || static_unit_function_table),
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
            global.attribute_alignment.map_or(1, u32::from),
            config.flags.optimization == mwcc_versions::Optimization::O0,
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
                    section: None,
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
                section: None,
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
            section: None,
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
                force_full_data_section: config.flags.string_literals_read_only
                    && !read_only_small_data,
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

    // A `static` function whose ADDRESS is taken before its definition gets its
    // LOCAL FUNC symbol created at that reference. Data initializers are visited in
    // their relocation-emission order (often reverse field order), while text-body
    // address references follow prototype order. A forward-declared static that is
    // only CALLED (REL24) keeps its symbol at the definition.
    //
    // Measured examples:
    // - Animal Crossing's iam_ef_kigae profile creates dw/mv/ct/init from its
    //   descending-offset `.data` relocations before any function-local constants.
    // - OSAlarm creates DecrementerExceptionHandler when its address is passed to
    //   __OSSetExceptionHandler in a text body.
    let forward_declared_statics: Vec<String> = {
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
                let Some(&definition_index) =
                    static_definition_index.get(relocation.target.as_str())
                else {
                    continue;
                };
                if definition_index >= global.functions_before
                    && seen.insert(relocation.target.clone())
                {
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

    let code_alignment = config
        .flags
        .function_alignment
        .unwrap_or(u32::from(config.build.code_alignment));
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
        &forward_declared_statics,
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
        write_artifacts(
            directory,
            config,
            &tokens,
            &unit.functions,
            &machine_functions,
            &object,
        );
    }
    Ok(object)
}

#[cfg(test)]
mod tests {
    use super::{global_alignments, parse_invocation, GlobalAlignments, SourceLanguage};
    use mwcc_versions::{EnumStorage, GlobalAddressing};

    #[test]
    fn scalar_array_layout_and_comment_alignment_are_independent() {
        assert_eq!(
            global_alignments(1, None, true, 1, true),
            GlobalAlignments {
                layout: 4,
                comment: 1,
            }
        );
        assert_eq!(
            global_alignments(2, None, true, 1, true),
            GlobalAlignments {
                layout: 4,
                comment: 2,
            }
        );
        assert_eq!(
            global_alignments(1, None, true, 32, true),
            GlobalAlignments {
                layout: 32,
                comment: 32,
            }
        );
        assert_eq!(
            global_alignments(1, None, true, 1, false),
            GlobalAlignments {
                layout: 4,
                comment: 4,
            }
        );
        assert_eq!(
            global_alignments(14, Some(1), false, 1, false),
            GlobalAlignments {
                layout: 1,
                comment: 1,
            }
        );
        assert_eq!(
            global_alignments(4, Some(1), true, 1, false),
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
        let read_only = parse_invocation(&["-str".into(), "reuse,readonly".into()]);
        assert!(read_only.flags.string_literals_read_only);

        let restated_pooling = parse_invocation(&[
            "-str".into(),
            "reuse,readonly".into(),
            "-str".into(),
            "reuse".into(),
        ]);
        assert!(restated_pooling.flags.string_literals_read_only);

        let modern = parse_invocation(&["-rostr".into(), "-str".into(), "reuse".into()]);
        assert!(modern.flags.string_literals_read_only);
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

fn write_artifacts(
    directory: &str,
    config: mwcc_versions::CompilerConfig,
    tokens: &[mwcc_tokens::Token],
    functions: &[mwcc_syntax_trees::Function],
    machine_functions: &[mwcc_machine_code::MachineFunction],
    object: &[u8],
) {
    let directory = PathBuf::from(directory);
    let _ = std::fs::create_dir_all(&directory);
    let dump = |name: &str, body: String| {
        let _ = std::fs::write(directory.join(name), body);
    };

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
    dump("00_build.txt", report);
    dump(
        "01_tokens.txt",
        tokens.iter().map(|token| format!("{token}\n")).collect(),
    );
    dump("02_syntax_tree.txt", format!("{functions:#?}\n"));
    dump(
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
    dump(
        "04_object.txt",
        format!(
            "ELF32 BE PowerPC relocatable object, {} bytes\n",
            object.len()
        ),
    );
}
