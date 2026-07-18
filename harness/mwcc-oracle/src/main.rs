//! The differential oracle.
//!
//! The real `mwcceppc` is the source of truth. For every canary, this compiles
//! the same source with both the real compiler (run via `wibo`) and our `mwcc`,
//! then compares the `.text` disassembly. We are correct for a canary if and
//! only if the two match exactly.
//!
//! Usage: `mwcc-oracle [GC_VERSION]`   (default 1.3.2)
//! Set `MWCC_ORACLE_FILTER` to a filename substring for a focused probe.
//! The real toolchain (wibo, the compiler set, powerpc-eabi-objdump) is taken
//! from a decomp checkout; set `FFCC` to point at it.

use std::path::{Path, PathBuf};
use std::process::Command;

const COMPILE_FLAGS: &[&str] = &[
    "-nodefaults", "-proc", "gekko", "-align", "powerpc", "-enum", "int", "-fp", "hardware",
    "-O4,p", "-inline", "auto", "-maxerrors", "1", "-nosyspath", "-RTTI", "off",
    "-fp_contract", "on", "-str", "reuse", "-lang=c",
];

fn main() -> std::process::ExitCode {
    let version = std::env::args().nth(1).unwrap_or_else(|| "1.3.2".to_string());
    let decomp = PathBuf::from(std::env::var("FFCC").unwrap_or_else(|_| {
        "/Users/zcanann/Documents/projects/FFCC-Decomp".to_string()
    }));

    let wibo = decomp.join("build/tools/wibo");
    let sjis = decomp.join("build/tools/sjiswrap.exe");
    let real_compiler = decomp.join(format!("build/compilers/GC/{version}/mwcceppc.exe"));
    let objdump = decomp.join("build/binutils/powerpc-eabi-objdump");
    let our_compiler = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("mwcc")))
        .expect("cannot locate sibling mwcc binary");

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf();
    let canaries = workspace.join("canaries");
    let temporary = std::env::temp_dir().join("mwcc-oracle");
    let _ = std::fs::create_dir_all(&temporary);

    println!("== differential oracle vs mwcceppc GC/{version} ==");
    let mut passed = 0u32;
    let mut failed = 0u32;
    // Whole-object byte-exactness, tracked alongside .text. Pass/fail keys on
    // .text; this reports how many objects match mwcceppc byte-for-byte.
    let mut object_exact = 0u32;
    // Relocation correctness — the objdiff contract. `.comment` keeps reloc'd
    // objects from being byte-exact, but their relocations/symbols must match.
    let mut reloc_exact = 0u32;

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&canaries)
        .expect("cannot read canaries/")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "c"))
        .collect();
    entries.sort();
    if let Ok(filter) = std::env::var("MWCC_ORACLE_FILTER") {
        entries.retain(|path| path.file_name().is_some_and(|name| name.to_string_lossy().contains(&filter)));
    }

    for source in entries {
        let name = source.file_stem().unwrap().to_string_lossy().to_string();
        let reference_object = temporary.join("reference.o");
        let our_object = temporary.join("ours.o");
        let _ = std::fs::remove_file(&reference_object);
        let _ = std::fs::remove_file(&our_object);

        // A canary may pin extra build-line flags (e.g. `-sdata 0`, `-char
        // unsigned`, `-O1`) via a `// flags:` directive. The same flags are
        // handed to both compilers — the real one models all of them, ours
        // parses the codegen-affecting subset — so the matrix beyond the default
        // -O4 small-data configuration is verifiable from a canary file alone.
        let extra_flags = flag_directive(&source);

        // Oracle: wibo sjiswrap mwcceppc FLAGS [extra] -c source -o reference.o
        let mut oracle = Command::new(&wibo);
        oracle.arg(&sjis).arg(&real_compiler).args(COMPILE_FLAGS).args(&extra_flags)
            .arg("-c").arg(&source).arg("-o").arg(&reference_object);
        let _ = oracle.output();
        if !reference_object.exists() {
            println!("  SKIP {name} (oracle rejected the source)");
            continue;
        }

        // Ours — pin codegen to the same build the oracle is running, plus the
        // canary's own flags.
        let ours = Command::new(&our_compiler)
            .arg("--build").arg(format!("GC/{version}"))
            .args(&extra_flags)
            .arg("-c").arg(&source).arg("-o").arg(&our_object).output();
        match ours {
            Ok(result) if our_object.exists() => {
                let reference_text = disassemble(&objdump, &reference_object);
                let our_text = disassemble(&objdump, &our_object);
                if reference_text == our_text {
                    let reference_relocs = relocations(&objdump, &reference_object);
                    let our_relocs = relocations(&objdump, &our_object);
                    let relocs_match = reference_relocs == our_relocs;
                    if relocs_match {
                        reloc_exact += 1;
                    }
                    let whole_object_matches = std::fs::read(&reference_object).ok() == std::fs::read(&our_object).ok();
                    if whole_object_matches {
                        object_exact += 1;
                        println!("  PASS {name}");
                        passed += 1;
                    } else {
                        // BYTE-EXACT OR DEFER: a .text match with any other
                        // section drifting (pool order, symbols, .comment)
                        // is a FAILURE, not a softer pass (the fire-366 pool
                        // reorder hid behind the old PASS* marker).
                        println!(
                            "  FAIL {name} — .text matches but the WHOLE OBJECT differs{}",
                            if relocs_match { " (sections/symbols)" } else { " (relocations!)" }
                        );
                        if !relocs_match {
                            print_difference(&our_relocs, &reference_relocs);
                        }
                        failed += 1;
                    }
                } else {
                    println!("  FAIL {name} — .text differs (ours | oracle):");
                    print_difference(&our_text, &reference_text);
                    failed += 1;
                }
            }
            Ok(result) => {
                let message = String::from_utf8_lossy(&result.stderr);
                println!("  FAIL {name} (ours: {})", message.trim());
                failed += 1;
            }
            Err(error) => {
                println!("  FAIL {name} (cannot run mwcc: {error})");
                failed += 1;
            }
        }
    }

    println!("== {passed} passed, {failed} failed ({object_exact} byte-exact, {reloc_exact} reloc-exact objects) ==");
    if failed == 0 { std::process::ExitCode::SUCCESS } else { std::process::ExitCode::FAILURE }
}

/// Extra build-line flags a canary pins for itself, from a `// flags: ...` line
/// (anywhere in the file). The remainder of the line is split on whitespace and
/// passed verbatim to both compilers. Absent directive -> no extra flags.
fn flag_directive(source: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(source) else { return Vec::new() };
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("// flags:").or_else(|| trimmed.strip_prefix("//flags:")) {
            return rest.split_whitespace().map(str::to_string).collect();
        }
    }
    Vec::new()
}

/// Parse `objdump -r` into normalized `[section] offset type symbol` lines,
/// sorted so two objects compare by content. This is the objdiff contract for
/// relocations: same offsets, same types, same target symbols.
fn relocations(objdump: &Path, object: &Path) -> Vec<String> {
    let output = Command::new(objdump).arg("-r").arg(object).output();
    let Ok(output) = output else { return Vec::new() };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut section = String::new();
    let mut lines = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("RELOCATION RECORDS FOR [") {
            section = rest.trim_end_matches("]:").to_string();
            continue;
        }
        // Data rows: "OFFSET   TYPE   VALUE"; skip the "OFFSET TYPE VALUE" header.
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 3 && fields[0] != "OFFSET" && fields[0].chars().all(|c| c.is_ascii_hexdigit()) {
            lines.push(format!("[{section}] {} {} {}", fields[0], fields[1], fields[2]));
        }
    }
    lines.sort();
    lines
}

/// Disassemble `.text` into one entry per instruction, each carrying the raw
/// encoded bytes *and* the mnemonic. Comparing these makes the oracle authoritative
/// on bytes — the project's actual contract — while the mnemonic keeps failure
/// diffs readable. The address column is dropped so two objects compare by content.
fn disassemble(objdump: &Path, object: &Path) -> Vec<String> {
    let output = Command::new(objdump).arg("-d").arg("-j").arg(".text").arg(object).output();
    let Ok(output) = output else { return Vec::new() };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = Vec::new();
    for line in text.lines() {
        // Instruction lines look like: "   0:\t38 60 00 00 \tli      r3,0"
        // fields: [address] [encoded bytes] [mnemonic]
        let fields: Vec<&str> = line.splitn(3, '\t').collect();
        if fields.len() == 3 {
            let bytes = fields[1].split_whitespace().collect::<String>();
            let mnemonic = fields[2].trim();
            lines.push(format!("{bytes}  {mnemonic}"));
        }
    }
    lines
}

fn print_difference(ours: &[String], oracle: &[String]) {
    let count = ours.len().max(oracle.len());
    for index in 0..count {
        let left = ours.get(index).map(String::as_str).unwrap_or("");
        let right = oracle.get(index).map(String::as_str).unwrap_or("");
        if left != right {
            println!("      {left:<28} | {right}");
        }
    }
}
