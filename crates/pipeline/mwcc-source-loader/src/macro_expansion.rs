use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Macro {
    Object(Vec<u8>),
    Function {
        parameters: Vec<String>,
        variadic: bool,
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
                    variadic,
                    replacement,
                } => {
                    let Some((arguments, invocation_end)) = parse_invocation(input, index) else {
                        output.extend_from_slice(identifier);
                        continue;
                    };
                    let fixed_count = parameters.len() - usize::from(*variadic);
                    if (!variadic && arguments.len() != fixed_count)
                        || (*variadic && arguments.len() < fixed_count)
                    {
                        output.extend_from_slice(&input[start..invocation_end]);
                        index = invocation_end;
                        continue;
                    }
                    let arguments = if *variadic {
                        let mut normalized = arguments[..fixed_count].to_vec();
                        let mut trailing = Vec::new();
                        for (position, argument) in arguments[fixed_count..].iter().enumerate() {
                            if position != 0 {
                                trailing.push(b',');
                            }
                            trailing.extend_from_slice(argument);
                        }
                        normalized.push(trailing);
                        normalized
                    } else {
                        arguments
                    };
                    // C/C++ stringification uses the original (unexpanded)
                    // argument spelling. Resolve those `#parameter` operators
                    // before installing the expanded argument macros below;
                    // otherwise the lexer sees the surviving `#` in the middle
                    // of a source line as a directive and can discard the rest
                    // of a multi-statement macro body.
                    let replacement = stringify_parameter_uses(
                        replacement,
                        parameters,
                        &arguments,
                    );
                    let mut parameter_definitions = definitions.clone();
                    for (parameter, argument) in parameters.iter().zip(&arguments) {
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
                        replacement,
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
            let expanded_replacement = expand(
                &replacement,
                parameter_definitions.as_ref().unwrap_or(definitions),
                &mut replacement_state,
                expanding,
                depth + 1,
            );
            let pasted_replacement = paste_tokens(&expanded_replacement);
            if pasted_replacement == expanded_replacement {
                output.extend(expanded_replacement);
            } else {
                let mut rescan_state = LexicalState::default();
                output.extend(expand(
                    &pasted_replacement,
                    definitions,
                    &mut rescan_state,
                    expanding,
                    depth + 1,
                ));
            }
            expanding.remove(name);
            index = invocation_end;
            continue;
        }
        output.push(input[index]);
        index += 1;
    }
    output
}

/// Replace function-macro `#parameter` operators with C string literals.
///
/// Keep this separate from ordinary identifier expansion: stringification is
/// deliberately based on the call-site spelling, while a normal parameter use
/// receives the recursively expanded argument.
fn stringify_parameter_uses(
    replacement: &[u8],
    parameters: &[String],
    arguments: &[Vec<u8>],
) -> Vec<u8> {
    let mut output = Vec::with_capacity(replacement.len());
    let mut index = 0;
    while index < replacement.len() {
        if matches!(replacement[index], b'\'' | b'"') {
            let end = skip_quoted(replacement, index);
            output.extend_from_slice(&replacement[index..end]);
            index = end;
            continue;
        }
        if replacement[index..].starts_with(b"//") {
            output.extend_from_slice(&replacement[index..]);
            break;
        }
        if replacement[index..].starts_with(b"/*") {
            let end = replacement[index + 2..]
                .windows(2)
                .position(|bytes| bytes == b"*/")
                .map_or(replacement.len(), |close| index + close + 4);
            output.extend_from_slice(&replacement[index..end]);
            index = end;
            continue;
        }
        if replacement[index] != b'#'
            || replacement.get(index + 1) == Some(&b'#')
            || (index > 0 && replacement[index - 1] == b'#')
        {
            output.push(replacement[index]);
            index += 1;
            continue;
        }

        let mut name_start = index + 1;
        while replacement
            .get(name_start)
            .is_some_and(u8::is_ascii_whitespace)
        {
            name_start += 1;
        }
        if !replacement
            .get(name_start)
            .copied()
            .is_some_and(is_identifier_start)
        {
            output.push(b'#');
            index += 1;
            continue;
        }
        let mut name_end = name_start + 1;
        while replacement
            .get(name_end)
            .copied()
            .is_some_and(is_identifier_continue)
        {
            name_end += 1;
        }
        let name = &replacement[name_start..name_end];
        let Some(argument) = parameters
            .iter()
            .position(|parameter| parameter.as_bytes() == name)
            .and_then(|position| arguments.get(position))
        else {
            output.push(b'#');
            index += 1;
            continue;
        };
        output.extend(stringify_argument(argument));
        index = name_end;
    }
    output
}

fn stringify_argument(argument: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(argument.len() + 2);
    output.push(b'"');
    let mut pending_space = false;
    for byte in argument
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
    {
        if byte.is_ascii_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && output.len() > 1 {
            output.push(b' ');
        }
        pending_space = false;
        if matches!(byte, b'"' | b'\\') {
            output.push(b'\\');
        }
        output.push(byte);
    }
    output.push(b'"');
    output
}

fn paste_tokens(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if matches!(input[index], b'\'' | b'"') {
            let end = skip_quoted(input, index);
            output.extend_from_slice(&input[index..end]);
            index = end;
            continue;
        }
        if input[index..].starts_with(b"//") {
            output.extend_from_slice(&input[index..]);
            break;
        }
        if input[index..].starts_with(b"/*") {
            let end = input[index + 2..]
                .windows(2)
                .position(|bytes| bytes == b"*/")
                .map_or(input.len(), |close| index + close + 4);
            output.extend_from_slice(&input[index..end]);
            index = end;
            continue;
        }
        if input[index..].starts_with(b"##") {
            while output.last().is_some_and(u8::is_ascii_whitespace) {
                output.pop();
            }
            index += 2;
            while input.get(index).is_some_and(u8::is_ascii_whitespace) {
                index += 1;
            }
            continue;
        }
        output.push(input[index]);
        index += 1;
    }
    output
}

fn parse_invocation(input: &[u8], after_name: usize) -> Option<(Vec<Vec<u8>>, usize)> {
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
                        arguments.push(input[argument_start..index].to_vec());
                    }
                    return Some((arguments, index + 1));
                }
            }
            b',' if depth == 1 => {
                arguments.push(input[argument_start..index].to_vec());
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
                    variadic: false,
                    replacement: b"p".to_vec(),
                },
            ),
            (
                "PAIR".to_string(),
                Macro::Function {
                    parameters: vec!["a".to_string(), "b".to_string()],
                    variadic: false,
                    replacement: b"a + b".to_vec(),
                },
            ),
            ("VALUE".to_string(), Macro::Object(b"3".to_vec())),
            (
                "EMPTY".to_string(),
                Macro::Function {
                    parameters: Vec::new(),
                    variadic: false,
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

    #[test]
    fn stringifies_original_function_macro_arguments() {
        let definitions = HashMap::from([
            (
                "ASSERT".to_string(),
                Macro::Function {
                    parameters: vec!["condition".to_string()],
                    variadic: false,
                    replacement: b"show(#condition); if (condition) { pass(); }".to_vec(),
                },
            ),
            ("VALUE".to_string(), Macro::Object(b"3".to_vec())),
        ]);
        let mut state = LexicalState::default();
        assert_eq!(
            expand_line(b"ASSERT(VALUE == 3)\n", &definitions, &mut state),
            b"show(\"VALUE == 3\"); if (3 == 3) { pass(); }\n"
        );
    }

    #[test]
    fn stringification_collapses_space_and_escapes_literal_spelling() {
        let definitions = HashMap::from([(
            "TEXT".to_string(),
            Macro::Function {
                parameters: vec!["value".to_string()],
                variadic: false,
                replacement: b"# value".to_vec(),
            },
        )]);
        let mut state = LexicalState::default();
        assert_eq!(
            expand_line(b"TEXT(  left   + \"right\"  )\n", &definitions, &mut state),
            b"\"left + \\\"right\\\"\"\n"
        );
    }

    #[test]
    fn pastes_function_macro_tokens_and_rescans_the_result() {
        let definitions = HashMap::from([
            (
                "DECLARE".to_string(),
                Macro::Function {
                    parameters: vec!["name".to_string(), "suffix".to_string()],
                    variadic: false,
                    replacement: b"int name ## 1 ## suffix;".to_vec(),
                },
            ),
            ("VALUE".to_string(), Macro::Object(b"renamed".to_vec())),
            ("prefix1u8".to_string(), Macro::Object(b"VALUE".to_vec())),
        ]);
        let mut state = LexicalState::default();
        assert_eq!(
            expand_line(b"DECLARE(prefix, u8)\n", &definitions, &mut state),
            b"int renamed;\n"
        );
    }

    #[test]
    fn token_pasting_does_not_modify_literals_or_comments() {
        let definitions = HashMap::from([(
            "TEXT".to_string(),
            Macro::Function {
                parameters: Vec::new(),
                variadic: false,
                replacement: b"\"a ## b\" /* c ## d */ value ## 2".to_vec(),
            },
        )]);
        let mut state = LexicalState::default();
        assert_eq!(
            expand_line(b"TEXT()\n", &definitions, &mut state),
            b"\"a ## b\" /* c ## d */ value2\n"
        );
    }
}
