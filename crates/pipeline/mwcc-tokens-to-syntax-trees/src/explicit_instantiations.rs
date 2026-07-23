//! Materialization of single-type-parameter explicit class instantiations.
//!
//! A primary-template member definition does not emit on its own.  A later
//! `template class C<char>;` emits weak concrete copies of every defined member.
//! Keeping that source transformation ahead of the ordinary parser lets member
//! bodies use the same type, expression, and C++ ABI machinery as handwritten
//! concrete definitions.

use mwcc_tokens::{LocatedToken, Token};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct MemberTemplate {
    parameter: String,
    class: String,
    tokens: Vec<LocatedToken>,
}

#[derive(Clone)]
struct ClassTemplate {
    parameter: String,
    class: String,
    tokens: Vec<LocatedToken>,
}

#[derive(Clone)]
struct Instantiation {
    end: usize,
    class: String,
    argument: LocatedToken,
}

pub(crate) struct Materialization {
    pub(crate) tokens: Vec<LocatedToken>,
    /// Source-written names on primary member-template definitions removed by
    /// concrete materialization. Generated copies do not replace this one-time
    /// front-end analysis cost.
    pub(crate) removed_member_parameter_names: usize,
}

pub(crate) fn materialize(tokens: Vec<LocatedToken>) -> Materialization {
    let instantiated_classes = (0..tokens.len())
        .filter_map(|index| instantiation_at(&tokens, index).map(|item| item.class))
        .collect::<std::collections::HashSet<_>>();
    let mut classes = HashMap::new();
    let mut members: HashMap<String, Vec<MemberTemplate>> = HashMap::new();
    let mut member_ranges = HashMap::new();
    let mut index = 0;
    while index < tokens.len() {
        if let Some((class, end)) = class_template_at(&tokens, index) {
            classes.insert(class.class.clone(), class);
            index = end;
        } else if let Some((member, end)) = member_template_at(&tokens, index) {
            if instantiated_classes.contains(&member.class) {
                members
                    .entry(member.class.clone())
                    .or_default()
                    .push(member);
                member_ranges.insert(index, end);
            }
            index = end;
        } else {
            index += 1;
        }
    }

    let removed_member_parameter_names = members
        .values()
        .flatten()
        .map(|member| member_parameter_name_count(&member.tokens))
        .sum();
    let mut output = Vec::with_capacity(tokens.len());
    index = 0;
    while index < tokens.len() {
        if let Some(&end) = member_ranges.get(&index) {
            index = end;
            continue;
        }
        if let Some(instantiation) = instantiation_at(&tokens, index) {
            if let Some(definitions) = members.get(&instantiation.class) {
                let mut emitted_classes = HashSet::from([instantiation.class.clone()]);
                if let Some(class) = classes.get(&instantiation.class) {
                    for dependency in dependent_templates(&class.tokens, &class.parameter, &classes)
                    {
                        emit_dependent_class(
                            &dependency,
                            &instantiation.argument,
                            &classes,
                            &mut emitted_classes,
                            &mut output,
                        );
                    }
                }
                for definition in definitions {
                    for dependency in
                        dependent_templates(&definition.tokens, &definition.parameter, &classes)
                    {
                        emit_dependent_class(
                            &dependency,
                            &instantiation.argument,
                            &classes,
                            &mut emitted_classes,
                            &mut output,
                        );
                    }
                }
                if let Some(class) = classes.get(&instantiation.class) {
                    if let Some(concrete) = instantiate_class(class, &instantiation.argument) {
                        output.extend(concrete);
                    }
                }
                for definition in definitions {
                    if let Some(concrete) = instantiate_member(definition, &instantiation.argument)
                    {
                        output.extend(concrete);
                    }
                }
                index = instantiation.end;
                continue;
            }
        }
        output.push(tokens[index].clone());
        index += 1;
    }
    Materialization {
        tokens: output,
        removed_member_parameter_names,
    }
}

fn member_parameter_name_count(tokens: &[LocatedToken]) -> usize {
    let body_start = tokens
        .iter()
        .position(|located| located.token == Token::BraceOpen)
        .unwrap_or(tokens.len());
    let plain = tokens
        .iter()
        .map(|located| located.token.clone())
        .collect::<Vec<_>>();
    let mut latest = Vec::new();
    for (position, token) in plain.iter().enumerate().take(body_start) {
        if token != &Token::ParenOpen {
            continue;
        }
        if let Some((close, names)) = crate::parameter_names::positions(&plain, position) {
            if close < body_start {
                latest = names;
            }
        }
    }
    latest.len()
}

fn emit_dependent_class(
    class: &str,
    argument: &LocatedToken,
    classes: &HashMap<String, ClassTemplate>,
    emitted: &mut HashSet<String>,
    output: &mut Vec<LocatedToken>,
) {
    if !emitted.insert(class.to_string()) {
        return;
    }
    let Some(definition) = classes.get(class) else {
        return;
    };
    for dependency in dependent_templates(&definition.tokens, &definition.parameter, classes) {
        emit_dependent_class(&dependency, argument, classes, emitted, output);
    }
    if let Some(concrete) = instantiate_class(definition, argument) {
        output.extend(concrete);
    }
}

fn dependent_templates(
    tokens: &[LocatedToken],
    parameter: &str,
    classes: &HashMap<String, ClassTemplate>,
) -> Vec<String> {
    let mut dependencies = Vec::new();
    for window in tokens.windows(4) {
        let Token::Identifier(class) = &window[0].token else {
            continue;
        };
        if window[1].token == Token::Less
            && matches!(&window[2].token, Token::Identifier(name) if name == parameter)
            && window[3].token == Token::Greater
            && classes.contains_key(class)
            && !dependencies.contains(class)
        {
            dependencies.push(class.clone());
        }
    }
    dependencies
}

fn class_template_at(tokens: &[LocatedToken], start: usize) -> Option<(ClassTemplate, usize)> {
    let parameter = template_parameter_at(tokens, start)?;
    if !(word(tokens.get(start + 5), "class")
        || token(tokens.get(start + 5)) == Some(&Token::KeywordStruct))
    {
        return None;
    }
    let Token::Identifier(class) = token(tokens.get(start + 6))? else {
        return None;
    };
    let body_start = (start + 7..tokens.len()).find(|&cursor| {
        matches!(
            token(tokens.get(cursor)),
            Some(Token::BraceOpen | Token::Semicolon)
        )
    })?;
    if token(tokens.get(body_start)) != Some(&Token::BraceOpen) {
        return None;
    }
    let end = balanced_body_end(tokens, body_start)?;
    Some((
        ClassTemplate {
            parameter: parameter.to_string(),
            class: class.clone(),
            tokens: tokens[start + 5..end].to_vec(),
        },
        end,
    ))
}

fn member_template_at(tokens: &[LocatedToken], start: usize) -> Option<(MemberTemplate, usize)> {
    let parameter = template_parameter_at(tokens, start)?;

    let body_start = (start + 5..tokens.len()).find(|&cursor| {
        matches!(
            token(tokens.get(cursor)),
            Some(Token::BraceOpen | Token::Semicolon)
        )
    })?;
    if token(tokens.get(body_start)) != Some(&Token::BraceOpen) {
        return None;
    }
    let class = (start + 5..body_start.saturating_sub(5)).find_map(|cursor| {
        let Token::Identifier(class) = token(tokens.get(cursor))? else {
            return None;
        };
        (token(tokens.get(cursor + 1)) == Some(&Token::Less)
            && matches!(token(tokens.get(cursor + 2)), Some(Token::Identifier(name)) if name == parameter)
            && token(tokens.get(cursor + 3)) == Some(&Token::Greater)
            && token(tokens.get(cursor + 4)) == Some(&Token::Colon)
            && token(tokens.get(cursor + 5)) == Some(&Token::Colon))
        .then(|| class.clone())
    })?;
    let end = balanced_body_end(tokens, body_start)?;
    Some((
        MemberTemplate {
            parameter: parameter.to_string(),
            class,
            tokens: tokens[start + 5..end].to_vec(),
        },
        end,
    ))
}

fn template_parameter_at(tokens: &[LocatedToken], start: usize) -> Option<&str> {
    if !word(tokens.get(start), "template")
        || token(tokens.get(start + 1)) != Some(&Token::Less)
        || !(word(tokens.get(start + 2), "typename") || word(tokens.get(start + 2), "class"))
        || token(tokens.get(start + 4)) != Some(&Token::Greater)
    {
        return None;
    }
    let Token::Identifier(parameter) = token(tokens.get(start + 3))? else {
        return None;
    };
    Some(parameter)
}

fn balanced_body_end(tokens: &[LocatedToken], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, located) in tokens[start..].iter().enumerate() {
        match located.token {
            Token::BraceOpen => depth += 1,
            Token::BraceClose => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    let mut end = start + offset + 1;
                    if token(tokens.get(end)) == Some(&Token::Semicolon) {
                        end += 1;
                    }
                    return Some(end);
                }
            }
            Token::EndOfFile => return None,
            _ => {}
        }
    }
    None
}

fn instantiation_at(tokens: &[LocatedToken], start: usize) -> Option<Instantiation> {
    if !word(tokens.get(start), "template") || !word(tokens.get(start + 1), "class") {
        return None;
    }
    let Token::Identifier(class) = token(tokens.get(start + 2))? else {
        return None;
    };
    if token(tokens.get(start + 3)) != Some(&Token::Less)
        || token(tokens.get(start + 5)) != Some(&Token::Greater)
        || token(tokens.get(start + 6)) != Some(&Token::Semicolon)
    {
        return None;
    }
    Some(Instantiation {
        end: start + 7,
        class: class.clone(),
        argument: tokens.get(start + 4)?.clone(),
    })
}

fn instantiate_member(
    definition: &MemberTemplate,
    argument: &LocatedToken,
) -> Option<Vec<LocatedToken>> {
    let argument_code = template_argument_code(&argument.token)?;
    let specialized_class = format!("{}<{argument_code}>", definition.class);
    let mut concrete = definition.tokens.clone();
    for located in &mut concrete {
        if matches!(&located.token, Token::Identifier(name) if name == &definition.parameter) {
            located.token = argument.token.clone();
        }
    }

    let scope = concrete.windows(6).position(|window| {
        matches!(&window[0].token, Token::Identifier(name) if name == &definition.class)
            && window[1].token == Token::Less
            && window[2].token == argument.token
            && window[3].token == Token::Greater
            && window[4].token == Token::Colon
            && window[5].token == Token::Colon
    })?;
    let location = concrete[scope].location;
    concrete.splice(
        scope..scope + 4,
        [LocatedToken {
            token: Token::Identifier(specialized_class.clone()),
            location,
        }],
    );
    let member = scope + 3;
    if matches!(&concrete[member].token, Token::Identifier(name) if name == &definition.class) {
        concrete[member].token = Token::Identifier(specialized_class);
    } else if concrete[member].token == Token::Tilde
        && matches!(&concrete[member + 1].token, Token::Identifier(name) if name == &definition.class)
    {
        concrete[member + 1].token = Token::Identifier(specialized_class);
    }

    let weak = [
        Token::Identifier("__declspec".into()),
        Token::ParenOpen,
        Token::Identifier("weak".into()),
        Token::ParenClose,
    ]
    .into_iter()
    .map(|token| LocatedToken { token, location });
    concrete.splice(0..0, weak);
    Some(concrete)
}

fn instantiate_class(
    definition: &ClassTemplate,
    argument: &LocatedToken,
) -> Option<Vec<LocatedToken>> {
    let argument_code = template_argument_code(&argument.token)?;
    let specialized_class = format!("{}<{argument_code}>", definition.class);
    let mut concrete = definition.tokens.clone();
    for located in &mut concrete {
        if matches!(&located.token, Token::Identifier(name) if name == &definition.parameter) {
            located.token = argument.token.clone();
        }
    }
    let mut index = 0;
    while index < concrete.len() {
        if matches!(&concrete[index].token, Token::Identifier(name) if name == &definition.class) {
            concrete[index].token = Token::Identifier(specialized_class.clone());
            if concrete.get(index + 1).map(|token| &token.token) == Some(&Token::Less)
                && concrete.get(index + 2).map(|token| &token.token) == Some(&argument.token)
                && concrete.get(index + 3).map(|token| &token.token) == Some(&Token::Greater)
            {
                concrete.drain(index + 1..index + 4);
            }
        }
        index += 1;
    }
    Some(concrete)
}

fn template_argument_code(argument: &Token) -> Option<&'static str> {
    Some(match argument {
        Token::KeywordChar => "c",
        Token::KeywordShort => "s",
        Token::KeywordInt => "i",
        Token::KeywordFloat => "f",
        Token::Identifier(name) if name == "double" => "d",
        Token::Identifier(name) if name == "wchar_t" => "w",
        Token::Identifier(name) if name == "bool" => "b",
        _ => return None,
    })
}

fn token(located: Option<&LocatedToken>) -> Option<&Token> {
    located.map(|located| &located.token)
}

fn word(located: Option<&LocatedToken>, expected: &str) -> bool {
    matches!(token(located), Some(Token::Identifier(word)) if word == expected)
}
