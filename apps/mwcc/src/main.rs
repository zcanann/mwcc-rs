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
        .map(|(name, return_type, _)| (name.clone(), *return_type))
        .chain(unit.functions.iter().map(|function| (function.name.clone(), function.return_type)))
        .collect();
    // Every callable's parameter types (prototypes + definitions) so a call places
    // each argument in the register the parameter's type requires (int vs float).
    let call_parameter_types: std::collections::HashMap<String, Vec<mwcc_syntax_trees::Type>> = unit
        .prototypes
        .iter()
        .map(|(name, _, parameter_types)| (name.clone(), parameter_types.clone()))
        .chain(unit.functions.iter().map(|function| {
            (function.name.clone(), function.parameters.iter().map(|parameter| parameter.parameter_type).collect())
        }))
        .collect();
    // Lower every function definition in source order; they share one object.
    let mut machine_functions: Vec<mwcc_machine_code::MachineFunction> = unit
        .functions
        .iter()
        .map(|function| mwcc_syntax_trees_to_machine_code::lower_function(function, &unit.globals, &call_return_types, &call_parameter_types, config))
        .collect::<Compilation<_>>()?;
    // Each SKIPPED inline function definition advanced mwcc's `@N` counter by 3
    // (compiled, then dropped) before the real functions were numbered — pre-bump
    // the first function's block (measured: math.h's fabs helper shifts s_frexp's
    // pool constant from @11 to @14).
    if let Some(first) = machine_functions.first_mut() {
        // The parser accumulates the measured PER-BODY label bump directly.
        first.anonymous_label_bump += unit.skipped_inline_functions as u32;
    }
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
            // An initializer may overrun the object (a char array shorter than its
            // string literal, e.g. `char s[2] = "hi"` keeps "hi", drops the NUL).
            if start + element_size as usize > bytes.len() {
                break;
            }
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
    // Distinct pooled string literals, by bytes, to their anonymous `@N` name, and
    // the running `@N` counter — deduplicated across the unit (mwcc `-str reuse`).
    let mut string_pool: std::collections::HashMap<Vec<u8>, String> = std::collections::HashMap::new();
    let mut string_counter: u32 = 0;
    for global in &unit.globals {
        if global.is_extern || matches!(global.declared_type, mwcc_syntax_trees::Type::Void) {
            continue;
        }
        // A `static const` SCALAR is folded into its readers (or elided when unused),
        // so keep dropping it. A `static const` ARRAY can't be folded into a register —
        // mwcc emits it to `.rodata` with a LOCAL symbol — so let it fall through to the
        // const-data path (which now binds it LOCAL via `global.is_static`).
        if global.is_static && global.is_const && global.array_length.is_none() {
            continue;
        }
        // A pointer global initialized with addresses (`int *p = &g;`, a string
        // `char *s = "…"`, or a `{…}` table): four zero bytes per element in
        // `.sdata`, each non-null element an ADDR32 relocation. A string element is
        // pooled — its bytes (plus NUL) become an anonymous local `@N` object, emitted
        // just before the pointer that first uses it, deduplicated across the unit.
        if let Some(elements) = &global.address_initializer {
            use mwcc_syntax_trees::PointerElement;
            if global.is_static || global.is_const {
                return Err(Diagnostic::error("a static/const pointer-address global is not supported yet (roadmap)"));
            }
            // A struct-table initializer (declared type is a struct) has one element
            // per FIELD, so its slot count is the flattened length; a plain pointer
            // array's length is the (possibly partially initialized) array length.
            let count = if matches!(global.declared_type, mwcc_syntax_trees::Type::Struct { .. }) {
                elements.len() as u32
            } else {
                global.array_length.map(u32::from).unwrap_or(elements.len() as u32)
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
                        bytes[offset as usize..offset as usize + 4].copy_from_slice(&(*value as u32).to_be_bytes());
                        continue;
                    }
                    PointerElement::Symbol(name) => name.clone(),
                    PointerElement::Str(string_bytes) => {
                        string_pool.get(string_bytes.as_slice()).cloned().unwrap_or_else(|| {
                            string_counter += 1;
                            let name = format!("@{string_counter}");
                            string_pool.insert(string_bytes.clone(), name.clone());
                            let mut object_bytes = string_bytes.clone();
                            object_bytes.push(0);
                            defined_globals.push(mwcc_machine_code_to_object::DefinedGlobal {
                                name: name.clone(),
                                size: object_bytes.len() as u32,
                                alignment: 4,
                                initial_bytes: Some(object_bytes),
                                is_const: false,
                                is_static: true,
                                is_explicit_zero: false,
                                relocations: Vec::new(),
                            });
                            name
                        })
                    }
                };
                relocations.push(mwcc_machine_code_to_object::DataRelocation { offset, target, addend: 0 });
            }
            // Relocated or non-zero bytes are initialized data (`.sdata`/`.data`); an
            // all-zero, unrelocated object (only null pointers) belongs in `.sbss`/`.bss`.
            let initial_bytes = (!relocations.is_empty() || bytes.iter().any(|&byte| byte != 0)).then_some(bytes);
            // An address initializer that resolved to no bytes is an all-null pointer
            // (`int *p = 0;`) — an EXPLICIT zero, so it orders ahead of the uninitialized run.
            let is_explicit_zero = initial_bytes.is_none();
            defined_globals.push(mwcc_machine_code_to_object::DefinedGlobal {
                name: global.name.clone(),
                size,
                alignment: 4,
                initial_bytes,
                is_const: false,
                is_static: false,
                is_explicit_zero,
                relocations,
            });
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
        let alignment = match struct_alignment {
            // A struct global is word-aligned at minimum (mwcc records 4 even for an
            // all-`char` struct whose natural alignment is 1), like an array object.
            Some(align) => align.max(4),
            None if global.array_length.is_some() => element_size.max(4),
            None => element_size,
        };
        // A struct's constant initializer lists its individual fields, each at its own
        // offset, so it serializes with a 4-byte field stride even though the object is
        // `struct_size`. (Only all-word-field structs are supported; a sub-word field
        // would need its own stride — guarded at the use site.)
        let serialize_stride = if struct_alignment.is_some() { 4 } else { element_size };

        if global.is_const {
            // A const struct value/array carries its pre-serialized field bytes
            // directly into the read-only section.
            if let Some(bytes) = &global.data_bytes {
                defined_globals.push(mwcc_machine_code_to_object::DefinedGlobal {
                    name: global.name.clone(),
                    size,
                    alignment,
                    initial_bytes: Some(bytes.clone()),
                    is_const: true,
                    is_static: global.is_static,
                    is_explicit_zero: false,
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
                is_static: global.is_static,
                is_explicit_zero: false,
                relocations: Vec::new(),
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
        // Materialize the initializer's bytes if there is one (a struct value/array uses the
        // parser's pre-serialized field bytes — exact for sub-word/nested/padded fields; a
        // scalar/array serializes its word-stride values). `None` means uninitialized.
        let materialized: Option<Vec<u8>> = if let Some(bytes) = &global.data_bytes {
            Some(bytes.clone())
        } else {
            global.initializer.as_ref().map(|values| serialize(values, serialize_stride, size))
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
            Some(bytes) if bytes.iter().any(|&value| value != 0) => (Some(bytes), false),
            Some(bytes) if is_array => (Some(bytes), false),
            Some(_) => (None, true),
            None => (None, false),
        };
        defined_globals.push(mwcc_machine_code_to_object::DefinedGlobal {
            name: global.name.clone(),
            size,
            alignment,
            initial_bytes,
            is_const: false,
            is_static: global.is_static,
            is_explicit_zero,
            relocations: Vec::new(),
        });
    }
    // Resolve each function's pooled string literals to anonymous `@N` `.sdata` objects, numbered at
    // the FRONT of that function's per-function `@N` block (before its constants and unwind entries),
    // matching mwcc's per-function counter walk (see mwcc-object's writer). A string reuses an
    // identical earlier one (`-str reuse`); a new one advances the counter. The counter starts at
    // `5 + global_strings` and advances per function by [its new strings + its new deduped constants
    // + its unwind entries] plus a fixed +4 gap. A jump table interleaves its own `@N` here in a way
    // not yet modeled, so a unit that mixes a string with a jump table defers wholesale.
    let has_string = machine_functions.iter().any(|function| !function.string_literals.is_empty());
    let has_jump_table = machine_functions.iter().any(|function| function.jump_table.is_some());
    if has_string && has_jump_table {
        return Err(Diagnostic::error("a string literal alongside a jump table is not supported yet (roadmap)"));
    }
    let mut counter = 5u32 + string_counter;
    let mut numbered_constant: std::collections::HashSet<(u64, u8)> = std::collections::HashSet::new();
    let mut function_string_objects: Vec<mwcc_machine_code_to_object::DefinedGlobal> = Vec::new();
    for machine_function in &mut machine_functions {
        let bump = u32::from(machine_function.has_conversion)
            + if machine_function.has_float_branch { 3 } else { 0 }
            + machine_function.anonymous_label_bump;
        let mut number = counter + bump;
        // Strings first, in the function's `@N` block. The NEW ones (a reuse points at an earlier
        // pool entry) are recorded by name so the writer emits their symbols at the FRONT of this
        // function's `@N` block, interleaved per-function with its constants/unwind entries.
        let mut new_string_names: Vec<String> = Vec::new();
        let resolved: Vec<String> = machine_function
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
                    name: name.clone(),
                    size: object_bytes.len() as u32,
                    alignment: 4,
                    initial_bytes: Some(object_bytes),
                    is_const: false,
                    is_static: true,
                    is_explicit_zero: false,
                    relocations: Vec::new(),
                });
                name
            })
            .collect();
        machine_function.new_string_count = new_string_names.len() as u32;
        machine_function.new_string_names = new_string_names;
        // Then the function's constants (deduped across the unit) and its unwind entries, so the next
        // function's block starts at the right `@N`.
        for constant in &machine_function.constants {
            if numbered_constant.insert((constant.bits, constant.byte_width)) {
                number += 1;
            }
        }
        if machine_function.frame.is_some() {
            number += 2;
        }
        counter = number + 4;
        for relocation in &mut machine_function.relocations {
            let resolved_target = if let mwcc_machine_code::RelocationTarget::External(name) = &relocation.target {
                name.strip_prefix("@@str").and_then(|rest| rest.parse::<usize>().ok()).map(|index| resolved[index].clone())
            } else {
                None
            };
            if let Some(name) = resolved_target {
                relocation.target = mwcc_machine_code::RelocationTarget::External(name);
            }
        }
    }
    defined_globals.extend(function_string_objects);

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
