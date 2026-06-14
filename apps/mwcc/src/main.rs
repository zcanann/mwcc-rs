//! `mwcc` — the compiler driver.
//!
//! The command line is intentionally compatible with `mwcceppc` so the oracle
//! harness can swap us in: `mwcc [flags...] -c <input.c> -o <output.o>`. Flags we
//! do not yet model are ignored. `--emit-artifacts <dir>` writes a per-phase
//! report for inspecting how a translation unit becomes bytes.

use mwcc_core::Compilation;
use std::path::PathBuf;
use std::process::ExitCode;

struct Invocation {
    input: Option<String>,
    output: Option<String>,
    build_label: Option<String>,
    artifacts_directory: Option<String>,
}

fn parse_invocation(arguments: &[String]) -> Invocation {
    let mut invocation = Invocation { input: None, output: None, build_label: None, artifacts_directory: None };
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

    match compile(&source, source_name, build, invocation.artifacts_directory.as_deref()) {
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
fn compile(source: &str, source_name: &str, build: mwcc_versions::CompilerBuild, artifacts: Option<&str>) -> Compilation<Vec<u8>> {
    let tokens = mwcc_source_to_tokens::tokenize(source)?;
    let function = mwcc_tokens_to_syntax_trees::parse_function(tokens.clone())?;
    let machine_code = mwcc_syntax_trees_to_machine_code::lower_function(&function, build)?;
    let object = mwcc_machine_code_to_object::assemble_object(&machine_code, source_name, build.version, build.build);

    if let Some(directory) = artifacts {
        write_artifacts(directory, build, &tokens, &function, &machine_code, &object);
    }
    Ok(object)
}

fn write_artifacts(
    directory: &str,
    build: mwcc_versions::CompilerBuild,
    tokens: &[mwcc_tokens::Token],
    function: &mwcc_syntax_trees::Function,
    machine_code: &mwcc_machine_code::MachineFunction,
    object: &[u8],
) {
    let directory = PathBuf::from(directory);
    let _ = std::fs::create_dir_all(&directory);
    let dump = |name: &str, body: String| {
        let _ = std::fs::write(directory.join(name), body);
    };

    dump("00_build.txt", format!("{} — {} (version {:?} build {})\n", build.label, build.product, build.version, build.build));
    dump("01_tokens.txt", tokens.iter().map(|token| format!("{token}\n")).collect());
    dump("02_syntax_tree.txt", format!("{function:#?}\n"));
    dump(
        "03_machine_code.txt",
        machine_code
            .instructions
            .iter()
            .map(|instruction| format!("{:08x}  {instruction:?}\n", instruction.encode()))
            .collect(),
    );
    dump("04_object.txt", format!("ELF32 BE PowerPC relocatable object, {} bytes\n", object.len()));
}
