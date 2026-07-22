//! Filesystem source loading for a translation unit.
//!
//! This phase owns physical source discovery: it resolves `#include` directives
//! against the including file and the driver's ordered access paths, then emits
//! one byte-preserving source buffer. Macro expansion and conditional directives
//! deliberately remain for the preprocessor proper; keeping filesystem policy
//! here prevents it from leaking into the lexer or parser.

use mwcc_core::{Compilation, Diagnostic};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceLoader {
    access_paths: Vec<PathBuf>,
}

impl SourceLoader {
    pub fn new(access_paths: Vec<PathBuf>) -> Self {
        Self { access_paths }
    }

    /// Load `input` and recursively materialize every resolvable include.
    ///
    /// Headers are materialized once per translation unit. Real projects use
    /// include guards pervasively; enforcing that invariant here also prevents
    /// cycles while conditional evaluation is still a later frontend phase.
    /// An unresolved include is retained verbatim. It may belong to an inactive
    /// conditional branch, and the later preprocessor is the component that can
    /// diagnose that distinction accurately.
    pub fn load(&self, input: &Path) -> Compilation<Vec<u8>> {
        let input = normalize_existing(input).map_err(|error| {
            Diagnostic::error(format!("cannot read {}: {error}", input.display()))
        })?;
        let access_paths = self
            .access_paths
            .iter()
            .map(|path| normalize_relative(path))
            .collect::<Vec<_>>();
        let mut context = LoadContext {
            access_paths: &access_paths,
            loaded: HashSet::new(),
        };
        context.load_file(&input)
    }
}

struct LoadContext<'a> {
    access_paths: &'a [PathBuf],
    loaded: HashSet<PathBuf>,
}

impl LoadContext<'_> {
    fn load_file(&mut self, path: &Path) -> Compilation<Vec<u8>> {
        let canonical = normalize_existing(path).map_err(|error| {
            Diagnostic::error(format!("cannot read {}: {error}", path.display()))
        })?;
        if !self.loaded.insert(canonical.clone()) {
            return Ok(Vec::new());
        }
        let source = std::fs::read(&canonical).map_err(|error| {
            Diagnostic::error(format!("cannot read {}: {error}", canonical.display()))
        })?;
        let mut output = Vec::with_capacity(source.len());
        for line in physical_lines(&source) {
            let Some(include) = parse_include(line) else {
                output.extend_from_slice(line);
                continue;
            };
            let Some(included_path) = self.resolve_include(&canonical, &include) else {
                output.extend_from_slice(line);
                continue;
            };
            let included = self.load_file(&included_path)?;
            output.extend_from_slice(&included);
            if !included.is_empty() && !included.ends_with(b"\n") && line.ends_with(b"\n") {
                output.push(b'\n');
            }
        }
        Ok(output)
    }

    fn resolve_include(&self, including_file: &Path, include: &Include<'_>) -> Option<PathBuf> {
        let requested = Path::new(include.path);
        if requested.is_absolute() && requested.is_file() {
            return Some(requested.to_path_buf());
        }
        if include.quoted {
            if let Some(parent) = including_file.parent() {
                let candidate = parent.join(requested);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        self.access_paths
            .iter()
            .map(|root| root.join(requested))
            .find(|candidate| candidate.is_file())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Include<'a> {
    path: &'a str,
    quoted: bool,
}

fn parse_include(line: &[u8]) -> Option<Include<'_>> {
    let text = std::str::from_utf8(line).ok()?.trim();
    let directive = text.strip_prefix('#')?.trim_start();
    let argument = directive.strip_prefix("include")?;
    if argument
        .as_bytes()
        .first()
        .is_some_and(|byte| !byte.is_ascii_whitespace())
    {
        return None;
    }
    let argument = argument.trim_start();
    let (quoted, close) = match argument.as_bytes().first()? {
        b'"' => (true, '"'),
        b'<' => (false, '>'),
        _ => return None,
    };
    let rest = &argument[1..];
    let end = rest.find(close)?;
    Some(Include {
        path: &rest[..end],
        quoted,
    })
}

fn physical_lines(source: &[u8]) -> impl Iterator<Item = &[u8]> {
    source.split_inclusive(|byte| *byte == b'\n')
}

fn normalize_existing(path: &Path) -> std::io::Result<PathBuf> {
    path.canonicalize()
}

fn normalize_relative(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_include, SourceLoader};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "mwcc-source-loader-{}-{serial}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("create scratch directory");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn recognizes_literal_include_directives_only() {
        assert_eq!(
            parse_include(b"  # include \"local.h\" // note\n")
                .map(|include| (include.path, include.quoted)),
            Some(("local.h", true))
        );
        assert_eq!(
            parse_include(b"#include <system.h>\n").map(|include| (include.path, include.quoted)),
            Some(("system.h", false))
        );
        assert!(parse_include(b"#include HEADER\n").is_none());
        assert!(parse_include(b"#include_next <x.h>\n").is_none());
    }

    #[test]
    fn quoted_includes_prefer_the_including_directory() {
        let scratch = Scratch::new();
        let source = scratch.0.join("source");
        let access = scratch.0.join("access");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&access).unwrap();
        std::fs::write(source.join("same.h"), b"int sibling;\n").unwrap();
        std::fs::write(access.join("same.h"), b"int access;\n").unwrap();
        std::fs::write(
            source.join("unit.c"),
            b"#include \"same.h\"\n#include <same.h>\n",
        )
        .unwrap();

        let loaded = SourceLoader::new(vec![access])
            .load(&source.join("unit.c"))
            .unwrap();
        assert_eq!(loaded, b"int sibling;\nint access;\n");
    }

    #[test]
    fn nested_headers_are_loaded_once_and_non_utf8_bytes_survive() {
        let scratch = Scratch::new();
        std::fs::write(scratch.0.join("leaf.h"), b"char *s = \"\x82\xa0\";\n").unwrap();
        std::fs::write(
            scratch.0.join("middle.h"),
            b"#include \"leaf.h\"\n#include \"leaf.h\"\n",
        )
        .unwrap();
        std::fs::write(
            scratch.0.join("unit.c"),
            b"#include \"middle.h\"\nint f(void);\n",
        )
        .unwrap();

        let loaded = SourceLoader::default()
            .load(&scratch.0.join("unit.c"))
            .unwrap();
        assert_eq!(loaded, b"char *s = \"\x82\xa0\";\nint f(void);\n");
    }

    #[test]
    fn unresolved_includes_remain_for_conditional_preprocessing() {
        let scratch = Scratch::new();
        std::fs::write(
            scratch.0.join("unit.c"),
            b"#ifdef NEVER\n#include <absent.h>\n#endif\nint f(void);\n",
        )
        .unwrap();

        let loaded = SourceLoader::default()
            .load(&scratch.0.join("unit.c"))
            .unwrap();
        assert_eq!(
            loaded,
            b"#ifdef NEVER\n#include <absent.h>\n#endif\nint f(void);\n"
        );
    }
}
