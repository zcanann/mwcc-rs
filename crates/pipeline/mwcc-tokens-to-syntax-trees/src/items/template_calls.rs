//! Semantic recovery for explicit C++ member-template calls.
//!
//! This deliberately models only proven forwarding chains: an in-class member
//! template calls a free helper with `*this`, and an explicit specialization of
//! that helper calls one concrete instance method.

use crate::parser::Parser;
use mwcc_syntax_trees::Type;
use mwcc_tokens::Token;

impl Parser {
    /// Recover the narrow but common C++ header idiom
    /// `template <class T> T get(defaulted_tag<T>) {
    /// return helper(tag<T>(), *this); }`.
    ///
    /// The record is useful only together with a concrete specialization of
    /// `helper`, captured below. Keeping the two halves separate prevents the
    /// parser from guessing a template instantiation from spelling alone.
    pub(crate) fn capture_cxx_member_template_forwarder(
        &mut self,
        declaration_start: usize,
        class: &str,
    ) {
        if !matches!(
            self.tokens.get(declaration_start..declaration_start + 2),
            Some([Token::Identifier(template), Token::Less]) if template == "template"
        ) {
            return;
        }

        let mut index = declaration_start + 1;
        let mut angle_depth = 0i32;
        loop {
            match self.tokens.get(index) {
                Some(Token::Less) => angle_depth += 1,
                Some(Token::Greater) => {
                    angle_depth -= 1;
                    if angle_depth == 0 {
                        index += 1;
                        break;
                    }
                }
                Some(Token::EndOfFile) | None => return,
                _ => {}
            }
            index += 1;
        }

        let Some(parameter_open) = (index..self.tokens.len()).find(|&cursor| {
            matches!(
                self.tokens[cursor],
                Token::ParenOpen | Token::Semicolon | Token::BraceOpen
            )
        }) else {
            return;
        };
        if self.tokens[parameter_open] != Token::ParenOpen {
            return;
        }
        let Some(Token::Identifier(member)) = self.tokens.get(parameter_open.wrapping_sub(1))
        else {
            return;
        };
        let member = member.clone();

        let mut cursor = parameter_open;
        let mut paren_depth = 0i32;
        let parameter_close = loop {
            match self.tokens.get(cursor) {
                Some(Token::ParenOpen) => paren_depth += 1,
                Some(Token::ParenClose) => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        break cursor;
                    }
                }
                Some(Token::EndOfFile) | None => return,
                _ => {}
            }
            cursor += 1;
        };
        let parameters = &self.tokens[parameter_open + 1..parameter_close];
        let accepts_no_runtime_arguments = parameters.is_empty()
            || matches!(parameters, [Token::KeywordVoid])
            || parameters.iter().any(|token| token == &Token::Equals);
        if !accepts_no_runtime_arguments {
            return;
        }

        let Some(body_open) = (parameter_close + 1..self.tokens.len())
            .find(|&cursor| matches!(self.tokens[cursor], Token::BraceOpen | Token::Semicolon))
        else {
            return;
        };
        if self.tokens[body_open] != Token::BraceOpen
            || self.tokens.get(body_open + 1) != Some(&Token::KeywordReturn)
        {
            return;
        }
        let Some(Token::Identifier(helper)) = self.tokens.get(body_open + 2) else {
            return;
        };
        if self.tokens.get(body_open + 3) != Some(&Token::ParenOpen) {
            return;
        }

        cursor = body_open + 3;
        paren_depth = 0;
        let mut passes_this = false;
        let call_close = loop {
            match self.tokens.get(cursor) {
                Some(Token::ParenOpen) => paren_depth += 1,
                Some(Token::ParenClose) => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        break cursor;
                    }
                }
                Some(Token::Star)
                    if self.tokens.get(cursor + 1)
                        == Some(&Token::Identifier("this".to_string())) =>
                {
                    passes_this = true;
                }
                Some(Token::EndOfFile) | None => return,
                _ => {}
            }
            cursor += 1;
        };
        if !passes_this
            || self.tokens.get(call_close + 1) != Some(&Token::Semicolon)
            || self.tokens.get(call_close + 2) != Some(&Token::BraceClose)
        {
            return;
        }
        self.cxx_member_template_forwarders
            .insert((class.to_string(), member), helper.clone());
    }

    /// Recover an explicit free-helper specialization whose complete body is
    /// `return object.member();`. This supplies the concrete half of a member
    /// template forwarder without pretending to instantiate arbitrary C++.
    pub(crate) fn capture_explicit_template_forwarder_specialization(&mut self) {
        let start = self.position;
        if !matches!(
            self.tokens.get(start..start + 3),
            Some([Token::Identifier(template), Token::Less, Token::Greater]) if template == "template"
        ) {
            return;
        }

        let Some(parameter_open) = (start + 3..self.tokens.len()).find(|&cursor| {
            matches!(
                self.tokens[cursor],
                Token::ParenOpen | Token::Semicolon | Token::BraceOpen
            )
        }) else {
            return;
        };
        if self.tokens[parameter_open] != Token::ParenOpen {
            return;
        }
        let Some(Token::Identifier(helper)) = self.tokens.get(parameter_open.wrapping_sub(1))
        else {
            return;
        };
        let helper = helper.clone();

        let mut cursor = parameter_open + 1;
        let mut paren_depth = 1i32;
        let mut argument = None;
        while let Some(token) = self.tokens.get(cursor) {
            match token {
                Token::Less if paren_depth == 1 => {
                    if let Some(candidate) = self
                        .tokens
                        .get(cursor + 1)
                        .and_then(|token| self.template_argument_type(token))
                    {
                        if self.tokens.get(cursor + 2) == Some(&Token::Greater) {
                            argument = Some(candidate);
                        }
                    }
                }
                Token::ParenOpen => paren_depth += 1,
                Token::ParenClose => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        break;
                    }
                }
                Token::EndOfFile => return,
                _ => {}
            }
            cursor += 1;
        }
        let parameter_close = cursor;
        let Some(argument) = argument else {
            return;
        };
        let Some(body_open) = (parameter_close + 1..self.tokens.len())
            .find(|&cursor| matches!(self.tokens[cursor], Token::BraceOpen | Token::Semicolon))
        else {
            return;
        };
        let Some(
            [Token::BraceOpen, Token::KeywordReturn, Token::Identifier(object), Token::Dot | Token::Arrow, Token::Identifier(member), Token::ParenOpen, Token::ParenClose, Token::Semicolon, Token::BraceClose],
        ) = self.tokens.get(body_open..body_open + 9)
        else {
            return;
        };

        let owner = (parameter_open + 1..parameter_close).find_map(|index| {
            match self.tokens.get(index..index + 3) {
                Some(
                    [Token::Identifier(owner), Token::Ampersand, Token::Identifier(parameter)],
                ) if parameter == object => Some(self.qualify_cxx_class_name(owner)),
                _ => None,
            }
        });
        let Some(owner) = owner else {
            return;
        };
        let specialization = (argument, owner, member.clone());
        let specializations = self
            .cxx_template_forwarder_specializations
            .entry(helper)
            .or_default();
        if !specializations.contains(&specialization) {
            specializations.push(specialization);
        }
    }

    /// Parse one concrete type argument only when it is immediately followed
    /// by `>(`. Failure restores the cursor so ordinary `<` expressions retain
    /// their existing meaning.
    pub(crate) fn try_explicit_member_template_argument(&mut self) -> Option<Type> {
        let saved = self.position;
        if self.tokens.get(self.position) != Some(&Token::Less) {
            return None;
        }
        self.position += 1;
        let Some((argument, end)) = self.template_argument_at(self.position) else {
            self.position = saved;
            return None;
        };
        let Some(argument) = argument else {
            self.position = saved;
            return None;
        };
        self.position = end;
        if self.tokens.get(self.position) != Some(&Token::Greater)
            || self.tokens.get(self.position + 1) != Some(&Token::ParenOpen)
        {
            self.position = saved;
            return None;
        }
        self.position += 1;
        Some(argument)
    }

    /// Resolve a recovered member-template/helper-specialization chain to the
    /// ordinary ABI symbol of its concrete zero-argument instance method.
    pub(crate) fn resolve_member_template_forwarder(
        &self,
        class: &str,
        member: &str,
        argument: Type,
        runtime_argument_count: usize,
    ) -> mwcc_core::Compilation<Option<String>> {
        if runtime_argument_count != 0 {
            return Ok(None);
        }
        let qualified_class = self.qualify_cxx_class_name(class);
        let helper = self
            .cxx_member_template_forwarders
            .get(&(qualified_class.clone(), member.to_string()))
            .or_else(|| {
                self.cxx_member_template_forwarders
                    .get(&(class.to_string(), member.to_string()))
            });
        let Some(helper) = helper else {
            return Ok(None);
        };
        let mut resolved = Vec::new();
        for (candidate_argument, owner, target_member) in self
            .cxx_template_forwarder_specializations
            .get(helper)
            .into_iter()
            .flatten()
        {
            if *candidate_argument != argument || (owner != &qualified_class && owner != class) {
                continue;
            }
            if let Some(name) = self.resolve_instance_member_call(class, target_member, 0)? {
                if !resolved.contains(&name) {
                    resolved.push(name);
                }
            }
        }
        match resolved.as_slice() {
            [] => Ok(None),
            [name] => Ok(Some(name.clone())),
            _ => Err(mwcc_core::Diagnostic::error(format!(
                "C++ member-template call '{qualified_class}::{member}' is ambiguous (roadmap)"
            ))),
        }
    }
}
