use std::collections::{HashMap, HashSet};

const MAX_EXPANSION_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct LexicalState {
    in_block_comment: bool,
}

pub(super) fn expand_line(
    line: &[u8],
    definitions: &HashMap<String, Vec<u8>>,
    state: &mut LexicalState,
) -> Vec<u8> {
    let mut expanding = HashSet::new();
    expand(line, definitions, state, &mut expanding, 0)
}

fn expand(
    input: &[u8],
    definitions: &HashMap<String, Vec<u8>>,
    state: &mut LexicalState,
    expanding: &mut HashSet<String>,
    depth: usize,
) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if state.in_block_comment {
            if input[index..].starts_with(b"*/") {
                output.extend_from_slice(b"*/");
                index += 2;
                state.in_block_comment = false;
            } else {
                output.push(input[index]);
                index += 1;
            }
            continue;
        }
        if input[index..].starts_with(b"//") {
            output.extend_from_slice(&input[index..]);
            break;
        }
        if input[index..].starts_with(b"/*") {
            output.extend_from_slice(b"/*");
            index += 2;
            state.in_block_comment = true;
            continue;
        }
        if matches!(input[index], b'\'' | b'"') {
            index = copy_quoted(input, index, &mut output);
            continue;
        }
        if is_identifier_start(input[index]) {
            let start = index;
            index += 1;
            while index < input.len() && is_identifier_continue(input[index]) {
                index += 1;
            }
            let identifier = &input[start..index];
            let Some(name) = std::str::from_utf8(identifier).ok() else {
                output.extend_from_slice(identifier);
                continue;
            };
            let Some(replacement) = definitions.get(name) else {
                output.extend_from_slice(identifier);
                continue;
            };
            if depth >= MAX_EXPANSION_DEPTH || !expanding.insert(name.to_string()) {
                output.extend_from_slice(identifier);
                continue;
            }
            let mut replacement_state = LexicalState::default();
            output.extend(expand(
                replacement,
                definitions,
                &mut replacement_state,
                expanding,
                depth + 1,
            ));
            expanding.remove(name);
            continue;
        }
        output.push(input[index]);
        index += 1;
    }
    output
}

fn copy_quoted(input: &[u8], start: usize, output: &mut Vec<u8>) -> usize {
    let quote = input[start];
    let mut index = start;
    while index < input.len() {
        let byte = input[index];
        output.push(byte);
        index += 1;
        if byte == b'\\' && index < input.len() {
            output.push(input[index]);
            index += 1;
        } else if byte == quote && index > start + 1 {
            break;
        }
    }
    index
}

fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_identifier_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests {
    use super::{expand_line, LexicalState};
    use std::collections::HashMap;

    #[test]
    fn expands_identifiers_outside_literals_and_comments() {
        let definitions = HashMap::from([("NULL".to_string(), b"0L".to_vec())]);
        let mut state = LexicalState::default();
        let expanded = expand_line(
            b"NULL NULLISH \"NULL\" 'N' /* NULL\n",
            &definitions,
            &mut state,
        );
        assert_eq!(expanded, b"0L NULLISH \"NULL\" 'N' /* NULL\n");
        assert_eq!(
            expand_line(b"NULL */ NULL // NULL\n", &definitions, &mut state),
            b"NULL */ 0L // NULL\n"
        );
    }

    #[test]
    fn recursively_expands_without_looping_on_cycles() {
        let definitions = HashMap::from([
            ("A".to_string(), b"B".to_vec()),
            ("B".to_string(), b"3".to_vec()),
            ("X".to_string(), b"Y".to_vec()),
            ("Y".to_string(), b"X".to_vec()),
        ]);
        let mut state = LexicalState::default();
        assert_eq!(expand_line(b"A X\n", &definitions, &mut state), b"3 X\n");
    }
}
