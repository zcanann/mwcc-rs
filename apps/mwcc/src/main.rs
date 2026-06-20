//! `mwcc` — the compiler driver.
//!
//! The command line is intentionally compatible with `mwcceppc` so the oracle
//! harness can swap us in: `mwcc [flags...] -c <input.c> -o <output.o>`. Flags we
//! do not yet model are ignored. `--emit-artifacts <dir>` writes a per-phase
//! report for inspecting how a translation unit becomes bytes.

use mwcc_core::{Compilation, Diagnostic};
use std::path::PathBuf;
use std::process::ExitCode;

struct Invocation {
    input: Option<String>,
    output: Option<String>,
    build_label: Option<String>,
    artifacts_directory: Option<String>,
    /// Codegen-affecting flags parsed from the real build line; the rest are
    /// ignored (they are the preprocessor's or diagnostics' concern).
    flags: mwcc_versions::Flags,
}

fn parse_invocation(arguments: &[String]) -> Invocation {
    use mwcc_versions::{CharDefault, GlobalAddressing, Optimization};
    let mut invocation = Invocation {
        input: None,
        output: None,
        build_label: None,
        artifacts_directory: None,
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
            // `-char signed`/`-char unsigned` overrides the build's `char` default.
            "-char" => {
                index += 1;
                invocation.flags.char_default = match arguments.get(index).map(String::as_str) {
                    Some("signed") => CharDefault::Signed,
                    Some("unsigned") => CharDefault::Unsigned,
                    _ => CharDefault::BuildDefault,
                };
            }
            // `-Cpp_exceptions off` suppresses the extab/extabindex unwind tables.
            "-Cpp_exceptions" => {
                index += 1;
                if arguments.get(index).map(String::as_str) == Some("off") {
                    invocation.flags.cpp_exceptions = false;
                }
            }
            // `-inline …`: a `deferred` setting emits functions in reverse order.
            "-inline" => {
                index += 1;
                if arguments.get(index).is_some_and(|value| value.split(',').any(|part| part == "deferred")) {
                    invocation.flags.inline_deferred = true;
                }
            }
            // `-sdata N`: a threshold of zero addresses globals absolutely.
            "-sdata" => {
                index += 1;
                if arguments.get(index).map(String::as_str) == Some("0") {
                    invocation.flags.global_addressing = GlobalAddressing::Absolute;
                }
            }
            // `-O0,p` .. `-O4,s` — only the level affects what we model so far.
            argument if argument.starts_with("-O") && argument.len() >= 3 => {
                invocation.flags.optimization = match argument.as_bytes()[2] {
                    b'0' => Optimization::O0,
                    b'1' => Optimization::O1,
                    b'2' => Optimization::O2,
                    b'3' => Optimization::O3,
                    _ => Optimization::O4,
                };
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
        Some(ref label) => match mwcc_versions::by_label(label) {
            Some(build) => build,
            None => {
                eprintln!("mwcc: unknown compiler build '{label}'");
                return ExitCode::FAILURE;
            }
        },
        None => mwcc_versions::DEFAULT,
    };

    let source = match std::fs::read_to_string(&input) {
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

    let config = mwcc_versions::CompilerConfig { build, flags: invocation.flags };
    match compile(&source, source_name, config, invocation.artifacts_directory.as_deref()) {
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

/// Run the full pipeline, optionally dumping a per-phase artifact report.
fn compile(source: &str, source_name: &str, config: mwcc_versions::CompilerConfig, artifacts: Option<&str>) -> Compilation<Vec<u8>> {
    let tokens = mwcc_source_to_tokens::tokenize(source)?;
    let unit = mwcc_tokens_to_syntax_trees::parse_translation_unit(tokens.clone())?;
    // Every callable's return type (prototypes + this unit's definitions) so a
    // call's result type is known during lowering.
    let call_return_types: std::collections::HashMap<String, mwcc_syntax_trees::Type> = unit
        .prototypes
        .iter()
        .cloned()
        .chain(unit.functions.iter().map(|function| (function.name.clone(), function.return_type)))
        .collect();
    // Lower every function definition in source order; they share one object.
    let mut machine_functions: Vec<mwcc_machine_code::MachineFunction> = unit
        .functions
        .iter()
        .map(|function| mwcc_syntax_trees_to_machine_code::lower_function(function, &unit.globals, &call_return_types, config))
        .collect::<Compilation<_>>()?;
    // Deferred inlining (`-inline …,deferred`) emits the object's functions — and
    // hence their `.text`, symbols, and metadata records — in reverse order.
    if config.flags.inline_deferred {
        machine_functions.reverse();
    }
    // File-scope variables defined here (not `extern`/`static`). A writable global
    // lands in `.sdata` (initialized) or `.sbss` (zero); a `const` one is read-only
    // and lands in `.sdata2` (≤ 8 bytes) or `.rodata` (larger). Declaration order is
    // kept so the writer can lay each section out the way mwcc does.
    let serialize = |values: &[i64], element_size: u32, size: u32| -> Vec<u8> {
        let mut bytes = vec![0u8; size as usize];
        for (index, &value) in values.iter().enumerate() {
            let start = index * element_size as usize;
            let encoded = (value as u64).to_be_bytes();
            bytes[start..start + element_size as usize].copy_from_slice(&encoded[8 - element_size as usize..]);
        }
        bytes
    };
    let small_data = config.flags.global_addressing == mwcc_versions::GlobalAddressing::SmallData;
    // A large (> 8 byte) writable global shares `.data`/`.bss` with any dense-switch
    // jump table; the two layouts aren't reconciled yet, so a jump table forces such
    // globals to keep deferring (be dropped).
    let has_jump_table = machine_functions.iter().any(|function| function.jump_table.is_some());
    let mut defined_globals: Vec<mwcc_machine_code_to_object::DefinedGlobal> = Vec::new();
    for global in &unit.globals {
        if global.is_extern || global.is_static || matches!(global.declared_type, mwcc_syntax_trees::Type::Void) {
            continue;
        }
        use mwcc_syntax_trees::Type;
        // A scalar/array of an arithmetic type serializes to fixed bytes (an integer
        // value, or a float/double IEEE-754 pattern already encoded by the parser).
        // Structs, pointers, and the like are not serializable here.
        let serializable_scalar = matches!(
            global.declared_type,
            Type::Int | Type::UnsignedInt | Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort | Type::Float | Type::Double
        );
        let element_size = (global.declared_type.width() / 8) as u32;
        let count = global.array_length.unwrap_or(1) as u32;
        let size = element_size * count;
        // mwcc aligns a scalar to its element alignment but any *array* object to at
        // least a word (4), so a `char[4]`/`short[2]` is 4-aligned, not 1/2-aligned.
        let alignment = if global.array_length.is_some() { element_size.max(4) } else { element_size };

        if global.is_const {
            // A const global is always materialized as read-only initialized bytes
            // (even an all-zero one stays in `.sdata2`/`.rodata`, not `.sbss`). Only
            // an arithmetic scalar/array with a constant initializer is serializable
            // today; defer structs/pointers, strings, and uninitialized const — each
            // a separate piece.
            if !serializable_scalar {
                return Err(Diagnostic::error("a const global of this type is not supported yet (roadmap)"));
            }
            let values = global
                .initializer
                .as_ref()
                .ok_or_else(|| Diagnostic::error("an uninitialized const global is not supported yet (roadmap)"))?;
            let initial_bytes = serialize(values, element_size, size);
            defined_globals.push(mwcc_machine_code_to_object::DefinedGlobal {
                name: global.name.clone(),
                size,
                alignment,
                initial_bytes: Some(initial_bytes),
                is_const: true,
            });
            continue;
        }

        // Writable global. Small (≤ 8 bytes) → `.sdata`/`.sbss`; large (> 8) →
        // `.data`/`.bss` (the writer routes by size). Large data is only emitted
        // under small-data addressing and when no jump table shares `.data`;
        // otherwise it is still dropped (the prior behavior — never a wrong object).
        if size > 8 && (!small_data || has_jump_table) {
            continue;
        }
        // A non-zero initializer becomes initialized `.sdata`/`.data` bytes;
        // zero/absent stays in `.sbss`/`.bss`.
        let initial_bytes = global
            .initializer
            .as_ref()
            .filter(|values| values.iter().any(|&value| value != 0))
            .map(|values| serialize(values, element_size, size));
        defined_globals.push(mwcc_machine_code_to_object::DefinedGlobal {
            name: global.name.clone(),
            size,
            alignment,
            initial_bytes,
            is_const: false,
        });
    }
    let object = mwcc_machine_code_to_object::assemble_object(&machine_functions, &defined_globals, &unit.inline_asm_symbols, source_name, config.build.version, config.build.build, small_data);

    if let Some(directory) = artifacts {
        write_artifacts(directory, config, &tokens, &unit.functions, &machine_functions, &object);
    }
    Ok(object)
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
    let mut report = format!("{} — {} (version {:?} build {})\n", build.label, build.product, build.version, build.build);
    let quirks = behavior.active_quirks();
    if quirks.is_empty() {
        report.push_str("active quirks: none (2.4.x mainline behavior)\n");
    } else {
        report.push_str("active quirks:\n");
        for active in quirks {
            report.push_str(&format!("  - {:?} [{:?}]: {}\n", active.quirk, active.kind, active.summary));
        }
    }
    dump("00_build.txt", report);
    dump("01_tokens.txt", tokens.iter().map(|token| format!("{token}\n")).collect());
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
    dump("04_object.txt", format!("ELF32 BE PowerPC relocatable object, {} bytes\n", object.len()));
}
