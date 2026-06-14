//! Compiler-build auto-detection.
//!
//! "Try them all, best match wins": given a reference object (the original game's
//! `.o`) and reconstructed source, compile the source with every supported build
//! and score each against the reference. The build whose output matches is the
//! one the object was compiled with — and because several builds emit identical
//! code, the honest answer is often a *set* of indistinguishable candidates that
//! a project narrows by intersecting detections across many translation units.
//!
//! Usage: `mwcc-detect <reference.o> <source.c>`
//! The objdump toolchain is taken from a decomp checkout; set `FFCC` to point at
//! it (defaults to the local FFCC-Decomp path).

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> std::process::ExitCode {
    let mut arguments = std::env::args().skip(1);
    let (Some(reference), Some(source)) = (arguments.next(), arguments.next()) else {
        eprintln!("usage: mwcc-detect <reference.o> <source.c>");
        return std::process::ExitCode::FAILURE;
    };

    let decomp = PathBuf::from(
        std::env::var("FFCC").unwrap_or_else(|_| "/Users/zcanann/Documents/projects/FFCC-Decomp".to_string()),
    );
    let objdump = decomp.join("build/binutils/powerpc-eabi-objdump");
    let our_compiler = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("mwcc")))
        .expect("cannot locate sibling mwcc binary");

    let reference_bytes = std::fs::read(&reference).unwrap_or_default();
    let reference_text = disassemble(&objdump, Path::new(&reference));
    let temporary = std::env::temp_dir().join("mwcc-detect-candidate.o");

    println!("detecting compiler build for {source} against {reference}\n");
    let mut scored = Vec::new();
    for build in mwcc_versions::SUPPORTED {
        let _ = std::fs::remove_file(&temporary);
        let built = Command::new(&our_compiler)
            .arg("--build").arg(build.label)
            .arg("-c").arg(&source)
            .arg("-o").arg(&temporary)
            .output();
        if built.is_err() || !temporary.exists() {
            println!("  {:<10} (could not compile this source)", build.label);
            continue;
        }
        let candidate_bytes = std::fs::read(&temporary).unwrap_or_default();
        let candidate_text = disassemble(&objdump, &temporary);
        let object_exact = !reference_bytes.is_empty() && candidate_bytes == reference_bytes;
        let text_score = instruction_match(&candidate_text, &reference_text);
        scored.push((build.label, text_score, object_exact));
    }

    // Best by code match, with whole-object byte-exactness as the tiebreak.
    let best = scored.iter().map(|(_, score, _)| *score).fold(0.0_f64, f64::max);
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then(b.2.cmp(&a.2)));
    for (label, text_score, object_exact) in &scored {
        let marker = if *object_exact { "  byte-exact object" } else { "" };
        let lead = if (*text_score - best).abs() < f64::EPSILON { "->" } else { "  " };
        println!("  {lead} {label:<10} code {:>5.1}%{marker}", text_score * 100.0);
    }

    // The best candidates match the code; whole-object byte-exactness (the
    // version-stamped .comment) narrows further when available, so the answer is
    // the intersection.
    let any_object_exact = scored.iter().any(|(_, _, object)| *object);
    let winners: Vec<&str> = scored
        .iter()
        .filter(|(_, score, object)| (*score - best).abs() < f64::EPSILON && (!any_object_exact || *object))
        .map(|(label, _, _)| *label)
        .collect();
    println!();
    match winners.as_slice() {
        [] => println!("no candidate could compile this source"),
        [only] => println!("best match: {only}"),
        many => println!("best match (indistinguishable on this unit): {}", many.join(", ")),
    }
    std::process::ExitCode::SUCCESS
}

/// The `.text` as one entry per instruction (encoded bytes + mnemonic), the way
/// the oracle compares it. The address column is dropped so two objects compare
/// by content.
fn disassemble(objdump: &Path, object: &Path) -> Vec<String> {
    let output = Command::new(objdump).arg("-d").arg("-j").arg(".text").arg(object).output();
    let Ok(output) = output else { return Vec::new() };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.splitn(3, '\t').collect();
        if fields.len() == 3 {
            let bytes = fields[1].split_whitespace().collect::<String>();
            let mnemonic = fields[2].trim();
            lines.push(format!("{bytes}  {mnemonic}"));
        }
    }
    lines
}

/// Fraction of instructions that match between two `.text` listings.
fn instruction_match(candidate: &[String], reference: &[String]) -> f64 {
    if reference.is_empty() {
        return 0.0;
    }
    let matching = candidate.iter().zip(reference).filter(|(a, b)| a == b).count();
    matching as f64 / candidate.len().max(reference.len()) as f64
}
