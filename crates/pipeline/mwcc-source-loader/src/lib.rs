//! Filesystem source loading for a translation unit.
//!
//! This phase owns physical source discovery and selection: it resolves
//! `#include` directives against the including file and the driver's ordered
//! access paths, evaluates conditional directives and object-like macros, and
//! emits one byte-preserving source buffer. Filesystem policy never leaks into
//! the lexer or parser.

mod condition;
mod macro_expansion;

use mwcc_core::{Compilation, Diagnostic};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceLoader {
    access_paths: Vec<PathBuf>,
    definitions: HashMap<String, String>,
    macros: HashMap<String, macro_expansion::Macro>,
}

impl SourceLoader {
    pub fn new(access_paths: Vec<PathBuf>) -> Self {
        Self {
            access_paths,
            definitions: HashMap::new(),
            macros: HashMap::new(),
        }
    }

    pub fn define(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        self.macros.insert(
            name.clone(),
            macro_expansion::Macro::Object(value.as_bytes().to_vec()),
        );
        self.definitions.insert(name, value);
    }

    pub fn undefine(&mut self, name: &str) {
        self.definitions.remove(name);
        self.macros.remove(name);
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
            definitions: self.definitions.clone(),
            macros: self.macros.clone(),
        };
        context.load_file(&input)
    }
}

struct LoadContext<'a> {
    access_paths: &'a [PathBuf],
    loaded: HashSet<PathBuf>,
    definitions: HashMap<String, String>,
    macros: HashMap<String, macro_expansion::Macro>,
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
        let mut conditional = Vec::new();
        let mut directive_continuation = false;
        let mut lexical_state = macro_expansion::LexicalState::default();
        for line in physical_lines(&source) {
            if directive_continuation {
                directive_continuation = line_continues(line);
                preserve_line_ending(&mut output, line);
                continue;
            }
            if let Some(directive) = parse_directive(line) {
                directive_continuation = line_continues(line);
                if update_conditional_state(&mut conditional, directive, &self.definitions) {
                    continue;
                }
                if !is_active(&conditional) {
                    continue;
                }
                if let Some(definition) = parse_define(directive) {
                    self.definitions.insert(
                        definition.name.to_string(),
                        if directive_continuation {
                            "1".to_string()
                        } else {
                            definition.conditional_value.to_string()
                        },
                    );
                    // A continued replacement needs logical-line assembly. Do
                    // not publish a truncated value ending in `\`; it is safer
                    // to leave those tokens for the later frontend until that
                    // distinct preprocessing feature is implemented.
                    self.macros.remove(definition.name);
                    if !directive_continuation {
                        if let Some(expansion) = definition.expansion {
                            self.macros.insert(definition.name.to_string(), expansion);
                        }
                    }
                    output.extend_from_slice(line);
                    continue;
                }
                if let Some(name) = directive
                    .strip_prefix("undef")
                    .and_then(directive_argument)
                    .and_then(directive_name)
                {
                    self.definitions.remove(name);
                    self.macros.remove(name);
                    output.extend_from_slice(line);
                    continue;
                }
            } else if !is_active(&conditional) {
                continue;
            }
            let Some(include) = parse_include(line) else {
                output.extend(macro_expansion::expand_line(
                    line,
                    &self.macros,
                    &mut lexical_state,
                ));
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

#[derive(Clone, Copy, Debug)]
struct ConditionalBranch {
    parent_active: bool,
    branch_taken: bool,
    active: bool,
}

fn is_active(stack: &[ConditionalBranch]) -> bool {
    stack.last().map_or(true, |branch| branch.active)
}

fn update_conditional_state(
    stack: &mut Vec<ConditionalBranch>,
    directive: &str,
    definitions: &HashMap<String, String>,
) -> bool {
    let parent_active = is_active(stack);
    let condition = if let Some(name) = directive
        .strip_prefix("ifdef")
        .and_then(directive_argument)
        .and_then(directive_name)
    {
        Some(definitions.contains_key(name))
    } else if let Some(name) = directive
        .strip_prefix("ifndef")
        .and_then(directive_argument)
        .and_then(directive_name)
    {
        Some(!definitions.contains_key(name))
    } else {
        directive
            .strip_prefix("if")
            .and_then(conditional_argument)
            .map(|expression| condition::evaluate(expression, definitions))
    };
    if let Some(condition) = condition {
        let active = parent_active && condition;
        stack.push(ConditionalBranch {
            parent_active,
            branch_taken: active,
            active,
        });
        return true;
    }
    if let Some(expression) = directive
        .strip_prefix("elif")
        .and_then(conditional_argument)
    {
        if let Some(branch) = stack.last_mut() {
            let active = branch.parent_active
                && !branch.branch_taken
                && condition::evaluate(expression, definitions);
            branch.active = active;
            branch.branch_taken |= active;
        }
        return true;
    }
    if directive == "else" || directive.starts_with("else ") {
        if let Some(branch) = stack.last_mut() {
            branch.active = branch.parent_active && !branch.branch_taken;
            branch.branch_taken = true;
        }
        return true;
    }
    if directive == "endif" || directive.starts_with("endif ") {
        stack.pop();
        return true;
    }
    false
}

fn parse_directive(line: &[u8]) -> Option<&str> {
    std::str::from_utf8(line)
        .ok()?
        .trim()
        .strip_prefix('#')
        .map(str::trim_start)
}

fn directive_argument(text: &str) -> Option<&str> {
    text.as_bytes()
        .first()
        .is_some_and(u8::is_ascii_whitespace)
        .then(|| text.trim())
        .filter(|argument| !argument.is_empty())
}

fn conditional_argument(text: &str) -> Option<&str> {
    text.as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'(')
        .then(|| text.trim())
        .filter(|argument| !argument.is_empty())
}

fn directive_name(text: &str) -> Option<&str> {
    text.trim()
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .next()
        .filter(|name| !name.is_empty())
}

struct MacroDefinition<'a> {
    name: &'a str,
    conditional_value: &'a str,
    expansion: Option<macro_expansion::Macro>,
}

fn parse_define(directive: &str) -> Option<MacroDefinition<'_>> {
    let argument = directive
        .strip_prefix("define")
        .and_then(directive_argument)?;
    let name_end = argument
        .bytes()
        .position(|byte| !byte.is_ascii_alphanumeric() && byte != b'_')
        .unwrap_or(argument.len());
    let name = &argument[..name_end];
    if name.is_empty() {
        return None;
    }
    let rest = &argument[name_end..];
    // Function-like macros count as defined but are neither integer-valued in
    // `#if` nor eligible for object-like expansion.
    if rest.starts_with('(') {
        let close = rest.find(')')?;
        let parameter_text = &rest[1..close];
        let parameters = if parameter_text.trim().is_empty() {
            Vec::new()
        } else {
            parameter_text
                .split(',')
                .map(str::trim)
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        let valid_parameters = parameters.iter().all(|parameter| {
            parameter
                .as_bytes()
                .first()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
                && parameter
                    .as_bytes()
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        });
        return Some(MacroDefinition {
            name,
            conditional_value: "1",
            expansion: valid_parameters.then(|| macro_expansion::Macro::Function {
                parameters,
                replacement: rest[close + 1..]
                    .trim()
                    .split("//")
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .as_bytes()
                    .to_vec(),
            }),
        });
    }
    let replacement = rest.trim().split("//").next().unwrap_or_default().trim();
    Some(MacroDefinition {
        name,
        conditional_value: if replacement.is_empty() {
            "1"
        } else {
            replacement
        },
        expansion: Some(macro_expansion::Macro::Object(
            replacement.as_bytes().to_vec(),
        )),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Include<'a> {
    path: &'a str,
    quoted: bool,
}

fn parse_include(line: &[u8]) -> Option<Include<'_>> {
    let directive = parse_directive(line)?;
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

fn line_continues(line: &[u8]) -> bool {
    line.iter()
        .rev()
        .copied()
        .find(|byte| !matches!(byte, b'\n' | b'\r' | b' ' | b'\t'))
        == Some(b'\\')
}

fn preserve_line_ending(output: &mut Vec<u8>, line: &[u8]) {
    if line.ends_with(b"\n") {
        output.push(b'\n');
    }
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
    fn inactive_includes_are_ignored_and_active_branches_are_selected() {
        let scratch = Scratch::new();
        std::fs::write(scratch.0.join("present.h"), b"int selected;\n").unwrap();
        std::fs::write(
            scratch.0.join("unit.c"),
            b"#ifdef NEVER\n#include <absent.h>\n#else\n#include \"present.h\"\n#endif\nint f(void);\n",
        )
        .unwrap();

        let loaded = SourceLoader::default()
            .load(&scratch.0.join("unit.c"))
            .unwrap();
        assert_eq!(loaded, b"int selected;\nint f(void);\n");
    }

    #[test]
    fn source_and_driver_definitions_control_nested_branches() {
        let scratch = Scratch::new();
        std::fs::write(
            scratch.0.join("unit.c"),
            b"#define LOCAL 3\n#if defined(ENABLED) && LOCAL >= 3\nint yes;\n#if 0\nint never;\n#endif\n#else\nint no;\n#endif\n",
        )
        .unwrap();

        let mut loader = SourceLoader::default();
        loader.define("ENABLED", "1");
        let loaded = loader.load(&scratch.0.join("unit.c")).unwrap();
        assert_eq!(loaded, b"#define LOCAL 3\nint yes;\n");
    }

    #[test]
    fn multiline_macro_bodies_do_not_leak_into_language_tokens() {
        let scratch = Scratch::new();
        std::fs::write(
            scratch.0.join("unit.c"),
            concat!(
                "#define BODY(x) do { \\\n",
                "  x += 1; \\\n",
                "} while (0)\n",
                "int f(void);\n"
            ),
        )
        .unwrap();

        let loaded = SourceLoader::default()
            .load(&scratch.0.join("unit.c"))
            .unwrap();
        assert_eq!(loaded, b"#define BODY(x) do { \\\n\n\nint f(void);\n");
    }

    #[test]
    fn continued_object_macros_are_not_partially_substituted() {
        let scratch = Scratch::new();
        std::fs::write(
            scratch.0.join("unit.c"),
            b"#define PAIR 1, \\\n2\nint values[] = { PAIR };\n",
        )
        .unwrap();

        let loaded = SourceLoader::default()
            .load(&scratch.0.join("unit.c"))
            .unwrap();
        assert_eq!(loaded, b"#define PAIR 1, \\\n\nint values[] = { PAIR };\n");
    }

    #[test]
    fn source_macros_expand_and_undef_removes_definitions() {
        let scratch = Scratch::new();
        std::fs::write(
            scratch.0.join("unit.c"),
            concat!(
                "#define NULL ((void*)0)\n",
                "#define CALL(x) use(x)\n",
                "void *first = NULL;\n",
                "void *second = CALL(NULL);\n",
                "#undef NULL\n",
                "void *third = NULL;\n"
            ),
        )
        .unwrap();

        let loaded = SourceLoader::default()
            .load(&scratch.0.join("unit.c"))
            .unwrap();
        assert_eq!(
            loaded,
            concat!(
                "#define NULL ((void*)0)\n",
                "#define CALL(x) use(x)\n",
                "void *first = ((void*)0);\n",
                "void *second = use(((void*)0));\n",
                "#undef NULL\n",
                "void *third = NULL;\n"
            )
            .as_bytes()
        );
    }
}
