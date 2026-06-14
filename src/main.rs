//! mwcc-rs: a byte-matching reimplementation of Metrowerks CodeWarrior for
//! Embedded PowerPC (mwcceppc), targeting GameCube/Wii decompilation.
//!
//! CLI is intentionally mwcceppc-compatible-ish so the A/B harness can swap us
//! in: `mwcc [flags...] -c <input.c> -o <output.o>`. Unknown flags are ignored
//! for now (we only act on -c and -o).

mod codegen;
mod elf;
mod lexer;
mod parser;
mod ppc;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-c" => {
                i += 1;
                if i < args.len() {
                    input = Some(args[i].clone());
                }
            }
            "-o" => {
                i += 1;
                if i < args.len() {
                    output = Some(args[i].clone());
                }
            }
            // a bare .c argument is also accepted as the input
            a if a.ends_with(".c") && input.is_none() => input = Some(a.to_string()),
            _ => {} // ignore other flags (v0)
        }
        i += 1;
    }

    let input = match input {
        Some(p) => p,
        None => {
            eprintln!("mwcc-rs: no input file (expected -c <file.c>)");
            return ExitCode::FAILURE;
        }
    };
    let output = output.unwrap_or_else(|| {
        let stem = input.strip_suffix(".c").unwrap_or(&input);
        format!("{stem}.o")
    });

    let src = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("mwcc-rs: cannot read {input}: {e}");
            return ExitCode::FAILURE;
        }
    };

    match compile(&src) {
        Ok((name, text)) => {
            let obj = elf::write_object(&name, &text);
            if let Err(e) = std::fs::write(&output, obj) {
                eprintln!("mwcc-rs: cannot write {output}: {e}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("mwcc-rs: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Compile one translation unit (v0: a single function) -> (symbol, text bytes).
fn compile(src: &str) -> Result<(String, Vec<u8>), String> {
    let toks = lexer::lex(src)?;
    let mut p = parser::Parser::new(toks);
    let func = p.func()?;
    let text = codegen::Gen::new().gen(&func)?;
    Ok((func.name, text))
}
