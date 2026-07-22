use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Macro {
    Object(Vec<u8>),
    Function {
        parameters: Vec<String>,
        replacement: Vec<u8>,
    },
}

const MAX_EXPANSION_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct LexicalState {
    in_block_comment: bool,
}

pub(super) fn expand_line(
    line: &[u8],
    definitions: &HashMap<String, Macro>,
    state: &mut LexicalState,
) -> Vec<u8> {
    let mut expanding = HashSet::new();
    expand(line, definitions, state, &mut expanding, 0)
}

fn expand(
    input: &[u8],
    definitions: &HashMap<String, Macro>,
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
            let Some(definition) = definitions.get(name) else {
                output.extend_from_slice(identifier);
                continue;
            };
            let (replacement, invocation_end, parameter_definitions) = match definition {
                Macro::Object(replacement) => (replacement.clone(), index, None),
                Macro::Function {
                    parameters,
                    replacement,
                } => {
                    let Some((arguments, invocation_end)) = parse_invocation(input, index) else {
                        output.extend_from_slice(identifier);
                        continue;
                    };
                    if arguments.len() != parameters.len() {
                        output.extend_from_slice(&input[start..invocation_end]);
                        index = invocation_end;
                        continue;
                    }
                    let mut parameter_definitions = definitions.clone();
                    for (parameter, argument) in parameters.iter().zip(arguments) {
                        let mut argument_state = LexicalState::default();
                        let expanded = expand(
                            argument,
                            definitions,
                            &mut argument_state,
                            expanding,
                            depth + 1,
                        );
                        parameter_definitions.insert(parameter.clone(), Macro::Object(expanded));
                    }
                    (
                        replacement.clone(),
                        invocation_end,
                        Some(parameter_definitions),
                    )
                }
            };
            if depth >= MAX_EXPANSION_DEPTH || !expanding.insert(name.to_string()) {
                output.extend_from_slice(&input[start..invocation_end]);
                index = invocation_end;
                continue;
            }
            let mut replacement_state = LexicalState::default();
            output.extend(expand(
                &replacement,
                parameter_definitions.as_ref().unwrap_or(definitions),
                &mut replacement_state,
                expanding,
                depth + 1,
            ));
            expanding.remove(name);
            index = invocation_end;
            continue;
        }
        output.push(input[index]);
        index += 1;
    }
    output
}

fn parse_invocation(input: &[u8], after_name: usize) -> Option<(Vec<&[u8]>, usize)> {
    let mut open = after_name;
    while input.get(open).is_some_and(u8::is_ascii_whitespace) {
        open += 1;
    }
    if input.get(open) != Some(&b'(') {
        return None;
    }
    let mut arguments = Vec::new();
    let mut argument_start = open + 1;
    let mut index = argument_start;
    let mut depth = 1usize;
    while index < input.len() {
        if matches!(input[index], b'\'' | b'"') {
            index = skip_quoted(input, index);
            continue;
        }
        if input[index..].starts_with(b"//") {
            return None;
        }
        if input[index..].starts_with(b"/*") {
            let close = input[index + 2..]
                .windows(2)
                .position(|bytes| bytes == b"*/")?;
            index += close + 4;
            continue;
        }
        match input[index] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    if input[argument_start..index]
                        .iter()
                        .any(|byte| !byte.is_ascii_whitespace())
                        || !arguments.is_empty()
                    {
                        arguments.push(&input[argument_start..index]);
                    }
                    return Some((arguments, index + 1));
                }
            }
            b',' if depth == 1 => {
                arguments.push(&input[argument_start..index]);
                argument_start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn copy_quoted(input: &[u8], start: usize, output: &mut Vec<u8>) -> usize {
    let end = skip_quoted(input, start);
    output.extend_from_slice(&input[start..end]);
    end
}

fn skip_quoted(input: &[u8], start: usize) -> usize {
    let quote = input[start];
    let mut index = start;
    while index < input.len() {
        let byte = input[index];
        index += 1;
        if byte == b'\\' && index < input.len() {
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
    use super::{expand_line, LexicalState, Macro};
    use std::collections::HashMap;

    #[test]
    fn expands_identifiers_outside_literals_and_comments() {
        let definitions = HashMap::from([("NULL".to_string(), Macro::Object(b"0L".to_vec()))]);
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
            ("A".to_string(), Macro::Object(b"B".to_vec())),
            ("B".to_string(), Macro::Object(b"3".to_vec())),
            ("X".to_string(), Macro::Object(b"Y".to_vec())),
            ("Y".to_string(), Macro::Object(b"X".to_vec())),
        ]);
        let mut state = LexicalState::default();
        assert_eq!(expand_line(b"A X\n", &definitions, &mut state), b"3 X\n");
    }

    #[test]
    fn expands_fixed_arity_functions_and_nested_arguments() {
        let definitions = HashMap::from([
            (
                "PROTO".to_string(),
                Macro::Function {
                    parameters: vec!["p".to_string()],
                    replacement: b"p".to_vec(),
                },
            ),
            (
                "PAIR".to_string(),
                Macro::Function {
                    parameters: vec!["a".to_string(), "b".to_string()],
                    replacement: b"a + b".to_vec(),
                },
            ),
            ("VALUE".to_string(), Macro::Object(b"3".to_vec())),
            (
                "EMPTY".to_string(),
                Macro::Function {
                    parameters: Vec::new(),
                    replacement: b"7".to_vec(),
                },
            ),
        ]);
        let mut state = LexicalState::default();
        assert_eq!(
            expand_line(
                b"double acos PROTO((double)); int x = PAIR(VALUE, call(1, 2)) + EMPTY( );\n",
                &definitions,
                &mut state,
            ),
            b"double acos (double); int x = 3 +  call(1, 2) + 7;\n"
        );
    }
}
