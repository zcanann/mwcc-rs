//! Size-only aggregate evidence recovered from compile-time assertions.
//!
//! Decompilation projects commonly describe a class whose irrelevant methods or
//! static members are outside the parser's current subset, then immediately
//! verify its ABI size with `STATIC_ASSERT(sizeof(T) == N)`.  A failed full
//! layout must not erase that independent fact: an enclosing class can still
//! place a by-value `T` member (or base) when only its storage extent matters.

use crate::parser::{Parser, StructLayout};
use mwcc_tokens::Token;
use std::collections::{HashMap, HashSet};

/// Collect unambiguous `STATIC_ASSERT(sizeof(T) == N)` expansions.
///
/// The project's macro expands to a typedef whose generated name begins with
/// `static_assertion_failed`.  Requiring that marker avoids treating an
/// ordinary `sizeof(variable)` comparison as type-layout evidence. Conflicting
/// assertions are discarded rather than allowing source order to choose.
pub(crate) fn collect(tokens: &[Token]) -> HashMap<String, u32> {
    let mut sizes = HashMap::new();
    let mut conflicts = HashSet::new();

    for (marker, token) in tokens.iter().enumerate() {
        let Token::Identifier(generated) = token else {
            continue;
        };
        if !generated.starts_with("static_assertion_failed") {
            continue;
        }
        let end = tokens[marker..]
            .iter()
            .position(|token| *token == Token::Semicolon)
            .map_or(tokens.len(), |distance| marker + distance);
        let mut index = marker + 1;
        while index < end {
            if matches!(tokens.get(index), Some(Token::Identifier(word)) if word == "sizeof") {
                if let Some((name, size)) = parse_size_equality(tokens, index, end) {
                    if conflicts.contains(&name) {
                        break;
                    }
                    match sizes.get(&name).copied() {
                        None => {
                            sizes.insert(name, size);
                        }
                        Some(previous) if previous == size => {}
                        Some(_) => {
                            sizes.remove(&name);
                            conflicts.insert(name);
                        }
                    }
                    break;
                }
            }
            index += 1;
        }
    }
    sizes
}

/// Build the terminal-name aliases needed by declarations that omit a
/// namespace qualifier. A terminal name shared by distinct qualified types is
/// deliberately left unresolved instead of depending on `HashMap` iteration
/// order.
pub(crate) fn unambiguous_aliases(sizes: &HashMap<String, u32>) -> HashMap<String, String> {
    let mut candidates: HashMap<String, Option<String>> = HashMap::new();
    for qualified in sizes.keys() {
        let terminal = qualified.rsplit("::").next().unwrap_or(qualified);
        candidates
            .entry(terminal.to_string())
            .and_modify(|candidate| {
                if candidate.as_deref() != Some(qualified.as_str()) {
                    *candidate = None;
                }
            })
            .or_insert_with(|| Some(qualified.clone()));
    }
    candidates
        .into_iter()
        .filter_map(|(terminal, qualified)| qualified.map(|qualified| (terminal, qualified)))
        .collect()
}

fn parse_size_equality(tokens: &[Token], sizeof_index: usize, end: usize) -> Option<(String, u32)> {
    let mut index = sizeof_index + 1;
    (tokens.get(index) == Some(&Token::ParenOpen)).then_some(())?;
    index += 1;
    if tokens.get(index) == Some(&Token::KeywordStruct)
        || matches!(tokens.get(index), Some(Token::Identifier(word)) if word == "class")
    {
        index += 1;
    }
    let Token::Identifier(first) = tokens.get(index)? else {
        return None;
    };
    let mut components = vec![first.clone()];
    index += 1;
    while tokens.get(index) == Some(&Token::Colon)
        && tokens.get(index + 1) == Some(&Token::Colon)
    {
        let Token::Identifier(component) = tokens.get(index + 2)? else {
            return None;
        };
        components.push(component.clone());
        index += 3;
    }
    (tokens.get(index) == Some(&Token::ParenClose)).then_some(())?;
    index += 1;
    (tokens.get(index) == Some(&Token::EqualEqual)).then_some(())?;
    let Token::IntegerLiteral(size) = tokens.get(index + 1)? else {
        return None;
    };
    let size = u32::try_from(*size).ok().filter(|size| *size > 0)?;
    (index + 1 < end).then(|| (components.join("::"), size))
}

impl Parser {
    /// A size assertion proves storage extent but not fields.  Use the largest
    /// natural PowerPC alignment that divides the size (capped at the ABI's
    /// eight-byte scalar maximum); every possible smaller natural alignment
    /// produces the same placement whenever the caller's cursor is aligned to
    /// this value.
    pub(crate) fn asserted_aggregate_layout(&self, tag: &str) -> Option<StructLayout> {
        self.asserted_aggregate_sizes.get(tag).copied().map(|size| {
            let align = (1u32 << size.trailing_zeros().min(3)) as u8;
            StructLayout {
                source_tag: Some(tag.to_string()),
                size,
                align,
                ..StructLayout::default()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{collect, unambiguous_aliases};
    use mwcc_source_to_tokens::tokenize;
    use std::collections::HashMap;

    #[test]
    fn collects_only_unambiguous_generated_size_assertions() {
        let tokens = tokenize(
            "typedef char static_assertion_failed7[(sizeof(A::B) == 12) ? 1 : -1];\n\
             typedef char static_assertion_failed8[(sizeof(C) == 4) ? 1 : -1];\n\
             typedef char static_assertion_failed9[(sizeof(C) == 8) ? 1 : -1];\n\
             int ordinary[(sizeof(value) == 16) ? 1 : -1];",
        )
        .unwrap();
        let sizes = collect(&tokens);
        assert_eq!(sizes.get("A::B"), Some(&12));
        assert!(!sizes.contains_key("C"));
        assert!(!sizes.contains_key("value"));
    }

    #[test]
    fn discards_ambiguous_terminal_aliases() {
        let sizes = HashMap::from([
            ("One::Item".to_string(), 4),
            ("Two::Item".to_string(), 8),
            ("Only::Unique".to_string(), 12),
        ]);
        let aliases = unambiguous_aliases(&sizes);
        assert!(!aliases.contains_key("Item"));
        assert_eq!(
            aliases.get("Unique").map(String::as_str),
            Some("Only::Unique")
        );
    }
}
