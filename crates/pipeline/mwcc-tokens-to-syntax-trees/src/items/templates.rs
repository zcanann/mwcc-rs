//! Symbolic layout recovery for skipped C++ struct and class templates.
//!
//! The general C++ parser does not yet compile template definitions. We still
//! need their concrete instance layout when later non-template functions use a
//! typedef such as `Vector3<float>`. This module records only parameter-typed
//! instance fields; methods, nested bodies, and static members remain skipped.

use super::{type_alignment, type_size};
use crate::parser::{
    Parser, StructField, StructLayout, StructTemplate, TemplateField, TemplateFieldType,
    TemplateTypePattern,
};
use mwcc_syntax_trees::{Pointee, Type};
use mwcc_tokens::Token;
use std::collections::HashMap;

fn template_pointer_type(declared: Option<Type>) -> Type {
    match declared {
        Some(Type::Int) => Type::Pointer(Pointee::Int),
        Some(Type::UnsignedInt) => Type::Pointer(Pointee::UnsignedInt),
        Some(Type::Char) => Type::Pointer(Pointee::Char),
        Some(Type::UnsignedChar) => Type::Pointer(Pointee::UnsignedChar),
        Some(Type::Short) => Type::Pointer(Pointee::Short),
        Some(Type::UnsignedShort) => Type::Pointer(Pointee::UnsignedShort),
        Some(Type::Float) => Type::Pointer(Pointee::Float),
        Some(Type::Double) => Type::Pointer(Pointee::Double),
        Some(Type::LongLong) => Type::Pointer(Pointee::LongLong),
        Some(Type::UnsignedLongLong) => Type::Pointer(Pointee::UnsignedLongLong),
        Some(Type::Struct { size, .. }) => Type::StructPointer { element_size: size },
        Some(Type::Pointer(_) | Type::StructPointer { .. }) => Type::Pointer(Pointee::Pointer),
        Some(Type::Void) | None => Type::StructPointer { element_size: 0 },
    }
}

#[derive(Clone)]
struct ResolvedTemplateType {
    declared: Type,
    known: bool,
    tag: Option<String>,
    layout: Option<StructLayout>,
}

impl Parser {
    /// Consume the declaration-scope marker on an explicit specialization.
    ///
    /// The translation-unit loop calls this only after giving inline-template
    /// recovery a chance to inspect the marker. What follows is an ordinary
    /// concrete declaration or definition as far as parsing and mangling are
    /// concerned; primary templates retain their non-empty parameter list and
    /// continue through the existing recovery path.
    pub(crate) fn consume_explicit_specialization_prefix(&mut self) -> bool {
        let explicit_specialization = matches!(
            self.tokens.get(self.position..self.position + 3),
            Some([Token::Identifier(template), Token::Less, Token::Greater]) if template == "template"
        );
        if explicit_specialization {
            self.position += 3;
        }
        explicit_specialization
    }

    /// Whether the item after a consumed `template <>` prefix is a concrete
    /// data definition. Explicit class specializations are type declarations,
    /// and function specializations have a top-level parameter list; neither
    /// category necessarily emits an object merely by being present. A
    /// semicolon-terminated qualified object with no parameter list does.
    pub(crate) fn item_is_explicit_data_specialization(&self) -> bool {
        if matches!(self.tokens.get(self.position), Some(Token::KeywordStruct))
            || matches!(
                self.tokens.get(self.position),
                Some(Token::Identifier(word))
                    if matches!(word.as_str(), "class" | "union" | "enum")
            )
        {
            return false;
        }

        let mut index = self.position;
        let mut angle_depth = 0i32;
        let mut paren_depth = 0i32;
        let mut saw_parameter_list = false;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Less if paren_depth == 0 => angle_depth += 1,
                Token::Greater if paren_depth == 0 && angle_depth > 0 => angle_depth -= 1,
                Token::ParenOpen if angle_depth == 0 => paren_depth += 1,
                Token::ParenClose if paren_depth > 0 => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        saw_parameter_list = true;
                    }
                }
                Token::Semicolon if angle_depth == 0 && paren_depth == 0 => {
                    return !saw_parameter_list;
                }
                Token::BraceOpen if angle_depth == 0 && paren_depth == 0 => return false,
                Token::EndOfFile => return false,
                _ => {}
            }
            index += 1;
        }
        false
    }

    /// Parse a direct `[scope::]Template<Argument>` object type from a recovered
    /// template layout. This complements typedef instantiation: game headers
    /// commonly place concrete template objects directly in class layouts.
    pub(crate) fn parse_template_instance_type(&mut self) -> Option<Type> {
        let (_, tag, end) = self.parse_template_instance_at(self.position)?;
        let template_name = tag
            .split('<')
            .next()
            .and_then(|name| name.rsplit("::").next())?;
        let argument = self
            .template_argument_at(self.template_argument_start(self.position)?)?
            .0;
        let layout = self.instantiate_struct_template_layout(template_name, argument)?;
        let element_size = layout.size;
        let element_align = layout.align;
        self.structs.insert(tag.clone(), layout);
        self.position = end;
        self.last_struct_tag = Some(tag);
        if self.eat_keyword(Token::Star) {
            if self.eat_keyword(Token::Star) {
                return Some(Type::Pointer(Pointee::Pointer));
            }
            return Some(Type::StructPointer { element_size });
        }
        if *self.peek() == Token::Ampersand {
            self.last_type_was_aggregate_reference = true;
            return Some(Type::StructPointer { element_size });
        }
        Some(Type::Struct {
            size: element_size,
            align: element_align,
        })
    }

    /// Whether the current token begins a concrete template instance whose
    /// layout can be recovered. Declaration lookahead must use the same test as
    /// `parse_type`; otherwise `Box<T>* value` is misread as `Box < T > ...`.
    pub(crate) fn peek_is_template_instance_type(&self) -> bool {
        self.cplusplus && self.parse_template_instance_at(self.position).is_some()
    }

    fn template_argument_start(&self, start: usize) -> Option<usize> {
        let mut scan = start + 1;
        while self.tokens.get(scan) == Some(&Token::Colon)
            && self.tokens.get(scan + 1) == Some(&Token::Colon)
            && matches!(self.tokens.get(scan + 2), Some(Token::Identifier(_)))
        {
            scan += 3;
        }
        (self.tokens.get(scan) == Some(&Token::Less)).then_some(scan + 1)
    }

    fn parse_template_instance_at(&self, start: usize) -> Option<(Type, String, usize)> {
        let mut scan = start;
        let mut components = Vec::new();
        let Token::Identifier(first) = self.tokens.get(scan)? else {
            return None;
        };
        components.push(first.clone());
        scan += 1;
        while self.tokens.get(scan) == Some(&Token::Colon)
            && self.tokens.get(scan + 1) == Some(&Token::Colon)
        {
            let Some(Token::Identifier(component)) = self.tokens.get(scan + 2) else {
                break;
            };
            components.push(component.clone());
            scan += 3;
        }
        if self.tokens.get(scan) != Some(&Token::Less) {
            return None;
        }
        let argument = self.template_argument_at(scan + 1)?.0;
        let mut end = scan + 1;
        let mut depth = 1i32;
        while depth > 0 {
            match self.tokens.get(end) {
                Some(Token::Less) => depth += 1,
                Some(Token::Greater) => depth -= 1,
                Some(Token::EndOfFile) | None => return None,
                _ => {}
            }
            end += 1;
        }
        let template_name = components.last()?;
        let layout = self.instantiate_struct_template_layout(template_name, argument)?;
        Some((
            Type::Struct {
                size: layout.size,
                align: layout.align,
            },
            format!("{}<...>", components.join("::")),
            end,
        ))
    }

    pub(crate) fn template_argument_at(&self, start: usize) -> Option<(Option<Type>, usize)> {
        if let Some((instance, _, end)) = self.parse_template_instance_at(start) {
            return Some((Some(instance), end));
        }
        let token = self.tokens.get(start)?;
        let mut declared = self.template_argument_type(token).or_else(|| match token {
            Token::Identifier(name) => self.struct_value_type(name),
            _ => None,
        });
        if declared.is_some() || matches!(token, Token::Identifier(_)) {
            let mut end = start + 1;
            while self.tokens.get(end) == Some(&Token::Star) {
                declared = Some(template_pointer_type(declared));
                end += 1;
            }
            Some((declared, end))
        } else {
            None
        }
    }

    pub(crate) fn instantiate_struct_template_layout(
        &self,
        template_name: &str,
        argument: Option<Type>,
    ) -> Option<StructLayout> {
        let arguments = [ResolvedTemplateType {
            declared: argument.unwrap_or(Type::Void),
            known: argument.is_some(),
            tag: None,
            layout: None,
        }];
        self.instantiate_struct_template_layout_with_arguments(template_name, &arguments)
    }

    fn resolve_template_pattern(
        &self,
        pattern: &TemplateTypePattern,
        arguments: &[ResolvedTemplateType],
    ) -> Option<ResolvedTemplateType> {
        match pattern {
            TemplateTypePattern::Parameter(index) => arguments.get(*index).cloned(),
            TemplateTypePattern::Named(name) => {
                let layout = self.structs.get(name)?.clone();
                Some(ResolvedTemplateType {
                    declared: Type::Struct {
                        size: layout.size,
                        align: layout.align,
                    },
                    known: true,
                    tag: Some(name.clone()),
                    layout: Some(layout),
                })
            }
            TemplateTypePattern::Instance {
                name,
                arguments: patterns,
            } => {
                let resolved = patterns
                    .iter()
                    .map(|pattern| self.resolve_template_pattern(pattern, arguments))
                    .collect::<Option<Vec<_>>>()?;
                let layout =
                    self.instantiate_struct_template_layout_with_arguments(name, &resolved)?;
                Some(ResolvedTemplateType {
                    declared: Type::Struct {
                        size: layout.size,
                        align: layout.align,
                    },
                    known: true,
                    tag: Some(format!("{name}<...>")),
                    layout: Some(layout),
                })
            }
        }
    }

    fn template_pattern_pointer_identity(
        &self,
        pattern: &TemplateTypePattern,
        arguments: &[ResolvedTemplateType],
    ) -> (u32, Option<String>) {
        match pattern {
            TemplateTypePattern::Parameter(index) => arguments.get(*index).map_or(
                (0, None),
                |argument| (type_size(argument.declared), argument.tag.clone()),
            ),
            TemplateTypePattern::Named(name) => (
                self.structs.get(name).map_or(0, |layout| layout.size),
                Some(name.clone()),
            ),
            TemplateTypePattern::Instance { name, .. } => {
                // Do not instantiate here: a self-pointer (`Node<T>*`) would
                // recurse forever. The concrete instance layout is registered
                // by the containing type before any expression dereferences it.
                (0, Some(format!("{name}<...>")))
            }
        }
    }

    fn instantiate_struct_template_layout_with_arguments(
        &self,
        template_name: &str,
        arguments: &[ResolvedTemplateType],
    ) -> Option<StructLayout> {
        let template = self.struct_templates.get(template_name)?;
        if arguments.len() > template.parameters.len() {
            return None;
        }
        let mut offset = 0u32;
        let mut max_alignment = 1u32;
        let mut fields = HashMap::new();
        let mut field_order = Vec::new();
        let mut function_pointer_fields = std::collections::HashSet::new();
        if let Some(base_pattern) = &template.base {
            let base = self.resolve_template_pattern(base_pattern, arguments)?;
            let base_layout = base.layout?;
            max_alignment = max_alignment.max(u32::from(base_layout.align));
            for (name, field) in base_layout.fields_in_declaration_order() {
                field_order.push(name.clone());
                fields.insert(name.clone(), field.clone());
            }
            function_pointer_fields.extend(base_layout.function_pointer_fields);
            offset = base_layout.size;
        }
        for field in &template.fields {
            let (field_type, field_size, natural_alignment, struct_tag) = match &field.field_type {
                TemplateFieldType::Parameter(index) => {
                    let resolved = arguments.get(*index)?;
                    if !resolved.known {
                        return None;
                    }
                    let field_type = resolved.declared;
                    (
                        field_type,
                        type_size(field_type),
                        type_alignment(field_type),
                        resolved.tag.clone(),
                    )
                }
                TemplateFieldType::ParameterByteArray(index) => {
                    let resolved = arguments.get(*index)?;
                    if !resolved.known {
                        return None;
                    }
                    (Type::UnsignedChar, type_size(resolved.declared), 1, None)
                }
                TemplateFieldType::TemplateValue(pattern) => {
                    let resolved = self.resolve_template_pattern(pattern, arguments)?;
                    let field_type = resolved.declared;
                    (
                        field_type,
                        type_size(field_type),
                        type_alignment(field_type),
                        resolved.tag,
                    )
                }
                TemplateFieldType::TemplatePointer(pattern) => {
                    let (element_size, tag) =
                        self.template_pattern_pointer_identity(pattern, arguments);
                    (
                        Type::StructPointer { element_size },
                        4,
                        4,
                        tag,
                    )
                }
                TemplateFieldType::Concrete(field_type) => (
                    *field_type,
                    type_size(*field_type),
                    type_alignment(*field_type),
                    None,
                ),
            };
            let alignment = natural_alignment.max(1).max(field.alignment);
            max_alignment = max_alignment.max(alignment);
            offset = offset.div_ceil(alignment) * alignment;
            fields.insert(
                field.name.clone(),
                StructField {
                    member_type: field_type,
                    source_fundamental: None,
                    offset,
                    struct_tag,
                    array_element: None,
                    array_bytes: None,
                    array_stride: None,
                    bit_field: None,
                },
            );
            field_order.push(field.name.clone());
            offset += field_size;
        }
        let size = offset.div_ceil(max_alignment) * max_alignment;
        Some(StructLayout {
            source_tag: None,
            field_order,
            fields,
            is_union: false,
            function_pointer_fields,
            size,
            align: max_alignment as u8,
        })
    }

    /// A generic primary template (`template <typename T, ...>`), as opposed
    /// to an explicit specialization (`template <>`). Primary definitions do
    /// not emit code or data until instantiated, so recovery may skip them.
    pub(crate) fn item_is_primary_template_declaration(&self) -> bool {
        matches!(self.tokens.get(self.position), Some(Token::Identifier(word)) if word == "template")
            && self.tokens.get(self.position + 1) == Some(&Token::Less)
            && self.tokens.get(self.position + 2) != Some(&Token::Greater)
    }

    /// Recognize an out-of-class definition proven to retain inline semantics.
    /// This covers both a concrete template member (`T Table<8, T>::get(...)`)
    /// and an ordinary member whose earlier class declaration said `inline`.
    ///
    /// CodeWarrior treats these header specializations like inline template
    /// materializations: an unused definition emits no function. Recovery can
    /// therefore skip it, while the ordinary skipped-inline name tracking makes
    /// a later call defer until template instantiation is implemented.
    pub(crate) fn item_is_skippable_inline_member_definition(&self) -> bool {
        let explicit_specialization = matches!(
            self.tokens.get(self.position..self.position + 3),
            Some([Token::Identifier(template), Token::Less, Token::Greater]) if template == "template"
        );
        let mut index = self.position + if explicit_specialization { 3 } else { 0 };
        let mut angle_depth = 0i32;
        let mut parameter_depth = 0i32;
        let mut saw_template_arguments = false;
        let mut saw_qualified_member = false;
        let mut saw_parameter_list = false;
        let mut last_identifier: Option<&str> = None;
        let mut angle_qualified_name: Option<&str> = None;
        let mut class_name: Option<&str> = None;
        let mut member_name: Option<&str> = None;
        let mut awaiting_member = false;

        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Less if parameter_depth == 0 => {
                    if angle_depth == 0 {
                        angle_qualified_name = last_identifier;
                    }
                    angle_depth += 1;
                    saw_template_arguments = true;
                }
                Token::Greater if parameter_depth == 0 && angle_depth > 0 => angle_depth -= 1,
                Token::Colon
                    if parameter_depth == 0
                        && angle_depth == 0
                        && self.tokens.get(index + 1) == Some(&Token::Colon) =>
                {
                    // Keep the final qualifier/member pair. This naturally
                    // handles `N::C::f`: the second `::` replaces `N` with `C`.
                    class_name = angle_qualified_name.take().or(last_identifier);
                    member_name = None;
                    saw_qualified_member = true;
                    awaiting_member = true;
                    index += 1;
                }
                Token::Identifier(name) if parameter_depth == 0 && angle_depth == 0 => {
                    if awaiting_member && !saw_parameter_list {
                        member_name = Some(name);
                        awaiting_member = false;
                    }
                    last_identifier = Some(name);
                }
                Token::ParenOpen if angle_depth == 0 => parameter_depth += 1,
                Token::ParenClose if parameter_depth > 0 => {
                    parameter_depth -= 1;
                    if parameter_depth == 0 {
                        saw_parameter_list = true;
                    }
                }
                Token::BraceOpen if angle_depth == 0 && parameter_depth == 0 => {
                    if !(saw_qualified_member && saw_parameter_list) {
                        return false;
                    }
                    let Some((class, member)) = class_name.zip(member_name) else {
                        return false;
                    };
                    let qualified_class = self.qualify_cxx_class_name(class);
                    let ordinary_inline = self
                        .inline_cxx_members
                        .contains(&(qualified_class, member.to_string()));
                    let template_inline = (explicit_specialization
                        || saw_template_arguments
                        || self.template_aliases.contains_key(class))
                        && {
                            let primary = self
                                .template_aliases
                                .get(class)
                                .map_or(class, String::as_str);
                            self.inline_template_members
                                .contains(&(primary.to_string(), member.to_string()))
                        };
                    return ordinary_inline || template_inline;
                }
                Token::Semicolon if angle_depth == 0 && parameter_depth == 0 => return false,
                Token::EndOfFile => return false,
                _ => {}
            }
            index += 1;
        }
        false
    }

    /// Capture `template <typename T> struct Name { T a, b; ... };` at the
    /// current recovery position without advancing the main parser cursor.
    pub(crate) fn capture_skipped_struct_template(&mut self) {
        self.capture_inline_template_members();
        self.capture_mixed_struct_template();
    }

    fn template_type_pattern_at(
        &self,
        start: usize,
        parameters: &[String],
    ) -> Option<(TemplateTypePattern, usize)> {
        let Token::Identifier(first) = self.tokens.get(start)? else {
            return None;
        };
        if let Some(index) = parameters.iter().position(|parameter| parameter == first) {
            return Some((TemplateTypePattern::Parameter(index), start + 1));
        }
        let mut name = first.clone();
        let mut cursor = start + 1;
        while self.tokens.get(cursor) == Some(&Token::Colon)
            && self.tokens.get(cursor + 1) == Some(&Token::Colon)
        {
            let Some(Token::Identifier(component)) = self.tokens.get(cursor + 2) else {
                return None;
            };
            name.push_str("::");
            name.push_str(component);
            cursor += 3;
        }
        if self.tokens.get(cursor) != Some(&Token::Less) {
            return Some((TemplateTypePattern::Named(name), cursor));
        }
        cursor += 1;
        let mut arguments = Vec::new();
        loop {
            let (argument, next) = self.template_type_pattern_at(cursor, parameters)?;
            arguments.push(argument);
            cursor = next;
            while self.tokens.get(cursor) == Some(&Token::Star) {
                // Nested pointer arguments are word-sized for layout. Their
                // pointee identity is not needed until a field dereferences one.
                cursor += 1;
            }
            match self.tokens.get(cursor) {
                Some(Token::Comma) => cursor += 1,
                Some(Token::Greater) => {
                    cursor += 1;
                    break;
                }
                _ => return None,
            }
        }
        Some((TemplateTypePattern::Instance { name, arguments }, cursor))
    }

    /// Recover mixed-layout templates with multiple/defaulted parameters. This
    /// intentionally reads declarations only: parameter-valued fields remain
    /// symbolic, while scalar fields and every pointer field have concrete
    /// target storage independent of template arguments.
    fn capture_mixed_struct_template(&mut self) {
        let start = self.position;
        if !matches!(self.tokens.get(start), Some(Token::Identifier(word)) if word == "template")
            || self.tokens.get(start + 1) != Some(&Token::Less)
        {
            return;
        }
        let mut cursor = start + 2;
        let mut angle_depth = 1i32;
        let mut parameters = Vec::new();
        while angle_depth > 0 {
            match self.tokens.get(cursor) {
                Some(Token::Identifier(kind))
                    if angle_depth == 1 && matches!(kind.as_str(), "typename" | "class") =>
                {
                    if let Some(Token::Identifier(name)) = self.tokens.get(cursor + 1) {
                        parameters.push(name.clone());
                    }
                }
                Some(Token::Less) => angle_depth += 1,
                Some(Token::Greater) => angle_depth -= 1,
                Some(Token::EndOfFile) | None => return,
                _ => {}
            }
            cursor += 1;
        }
        if parameters.is_empty() {
            return;
        }
        if !matches!(self.tokens.get(cursor), Some(Token::KeywordStruct))
            && !matches!(self.tokens.get(cursor), Some(Token::Identifier(word)) if word == "class")
        {
            return;
        }
        let Some(Token::Identifier(name)) = self.tokens.get(cursor + 1) else {
            return;
        };
        let name = name.clone();
        cursor += 2;
        let mut base = None;
        if self.tokens.get(cursor) == Some(&Token::Colon) {
            cursor += 1;
            while matches!(self.tokens.get(cursor), Some(Token::Identifier(word)) if matches!(word.as_str(), "public" | "private" | "protected" | "virtual"))
            {
                cursor += 1;
            }
            if let Some((pattern, next)) = self.template_type_pattern_at(cursor, &parameters) {
                base = Some(pattern);
                cursor = next;
            }
        }
        while !matches!(
            self.tokens.get(cursor),
            Some(Token::BraceOpen | Token::EndOfFile) | None
        ) {
            cursor += 1;
        }
        if self.tokens.get(cursor) != Some(&Token::BraceOpen) {
            return;
        }
        cursor += 1;
        let mut depth = 1i32;
        let mut fields = Vec::new();
        while depth > 0 {
            match self.tokens.get(cursor) {
                Some(Token::BraceOpen) => {
                    depth += 1;
                    cursor += 1;
                }
                Some(Token::BraceClose) => {
                    depth -= 1;
                    cursor += 1;
                }
                Some(Token::EndOfFile) | None => return,
                _ if depth == 1 => {
                    if let Some((mut declaration, next)) =
                        self.capture_template_field_declaration(cursor, &parameters)
                    {
                        fields.append(&mut declaration);
                        cursor = next;
                    } else {
                        cursor += 1;
                    }
                }
                _ => cursor += 1,
            }
        }
        if !fields.is_empty() || base.is_some() {
            self.struct_templates.insert(
                name,
                StructTemplate {
                    parameters,
                    base,
                    fields,
                },
            );
        }
    }

    fn capture_template_field_declaration(
        &self,
        start: usize,
        parameters: &[String],
    ) -> Option<(Vec<TemplateField>, usize)> {
        let mut cursor = start;
        while matches!(self.tokens.get(cursor), Some(Token::Identifier(word)) if matches!(word.as_str(), "const" | "volatile" | "mutable"))
        {
            cursor += 1;
        }
        if matches!(self.tokens.get(cursor), Some(Token::Identifier(word)) if word == "static") {
            return None;
        }
        let (mut field_type, type_tokens) = match self.tokens.get(cursor)? {
            Token::Identifier(name) if parameters.iter().any(|parameter| parameter == name) => {
                let index = parameters.iter().position(|parameter| parameter == name)?;
                (TemplateFieldType::Parameter(index), 1)
            }
            Token::KeywordUnsigned if self.tokens.get(cursor + 1) == Some(&Token::KeywordChar) => {
                (TemplateFieldType::Concrete(Type::UnsignedChar), 2)
            }
            token if self.template_argument_type(token).is_some() => (
                TemplateFieldType::Concrete(self.template_argument_type(token)?), 1
            ),
            Token::Identifier(_) => {
                let (pattern, next) = self.template_type_pattern_at(cursor, parameters)?;
                (TemplateFieldType::TemplateValue(pattern), next - cursor)
            }
            _ => return None,
        };
        cursor += type_tokens;
        while matches!(self.tokens.get(cursor), Some(Token::Identifier(word)) if matches!(word.as_str(), "const" | "volatile"))
        {
            cursor += 1;
        }
        if self.tokens.get(cursor) == Some(&Token::Star) {
            field_type = match field_type {
                TemplateFieldType::Parameter(index) => {
                    TemplateFieldType::TemplatePointer(TemplateTypePattern::Parameter(index))
                }
                TemplateFieldType::TemplateValue(pattern) => {
                    TemplateFieldType::TemplatePointer(pattern)
                }
                _ => TemplateFieldType::Concrete(Type::Pointer(Pointee::Int)),
            };
            while self.tokens.get(cursor) == Some(&Token::Star) {
                cursor += 1;
            }
            while matches!(self.tokens.get(cursor), Some(Token::Identifier(word)) if matches!(word.as_str(), "const" | "volatile"))
            {
                cursor += 1;
            }
        }
        let mut fields = Vec::new();
        loop {
            let Some(Token::Identifier(name)) = self.tokens.get(cursor) else {
                return None;
            };
            fields.push(TemplateField {
                name: name.clone(),
                field_type: field_type.clone(),
                alignment: 1,
            });
            cursor += 1;
            if self.tokens.get(cursor) == Some(&Token::BracketOpen)
                && matches!(
                    self.tokens.get(cursor + 1..cursor + 6),
                    Some([
                        Token::Identifier(sizeof),
                        Token::ParenOpen,
                        Token::Identifier(sized),
                        Token::ParenClose,
                        Token::BracketClose,
                    ]) if sizeof == "sizeof" && parameters.iter().any(|parameter| parameter == sized)
                )
            {
                let Some(Token::Identifier(sized)) = self.tokens.get(cursor + 3) else {
                    return None;
                };
                let index = parameters.iter().position(|parameter| parameter == sized)?;
                fields.last_mut().unwrap().field_type =
                    TemplateFieldType::ParameterByteArray(index);
                cursor += 6;
            }
            if matches!(self.tokens.get(cursor), Some(Token::Identifier(attribute)) if attribute == "__attribute__")
            {
                let end = (cursor..self.tokens.len()).find(|&index| {
                    matches!(self.tokens[index], Token::Semicolon | Token::EndOfFile)
                })?;
                let alignment = self.tokens[cursor..end].windows(3).find_map(
                    |tokens| match tokens {
                        [Token::Identifier(aligned), Token::ParenOpen, Token::IntegerLiteral(value)]
                            if aligned == "aligned" =>
                        {
                            u32::try_from(*value).ok()
                        }
                        _ => None,
                    },
                );
                if let Some(alignment) = alignment {
                    fields.last_mut().unwrap().alignment = alignment;
                }
                cursor = end;
            }
            match self.tokens.get(cursor) {
                Some(Token::Comma) => cursor += 1,
                Some(Token::Semicolon) => return Some((fields, cursor + 1)),
                _ => return None,
            }
        }
    }

    /// Record methods defined directly inside any class-template body. This is
    /// deliberately independent of layout recovery, which substitutes only
    /// the first type parameter; Pikmin's trig templates use both an integer and
    /// a type parameter but still need correct specialization materialization.
    fn capture_inline_template_members(&mut self) {
        let start = self.position;
        if !self.item_is_primary_template_declaration() {
            return;
        }
        let mut index = start + 1;
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
        let is_class = matches!(self.tokens.get(index), Some(Token::KeywordStruct))
            || matches!(self.tokens.get(index), Some(Token::Identifier(word)) if word == "class");
        if !is_class {
            return;
        }
        index += 1;
        let Some(Token::Identifier(class_name)) = self.tokens.get(index) else {
            return;
        };
        let class_name = class_name.clone();
        while !matches!(
            self.tokens.get(index),
            Some(Token::BraceOpen | Token::EndOfFile) | None
        ) {
            index += 1;
        }
        if self.tokens.get(index) != Some(&Token::BraceOpen) {
            return;
        }
        index += 1;
        let mut brace_depth = 1i32;
        while let Some(token) = self.tokens.get(index) {
            if brace_depth == 1 {
                if token == &Token::KeywordStruct {
                    if let Some([Token::Identifier(nested), Token::BraceOpen, Token::BraceClose]) =
                        self.tokens.get(index + 1..index + 4)
                    {
                        self.empty_nested_template_types
                            .insert((class_name.clone(), nested.clone()));
                    }
                }
                if let Token::Identifier(member_name) = token {
                    if self.tokens.get(index + 1) == Some(&Token::ParenOpen) {
                        let mut cursor = index + 1;
                        let mut parens = 0i32;
                        while let Some(candidate) = self.tokens.get(cursor) {
                            match candidate {
                                Token::ParenOpen => parens += 1,
                                Token::ParenClose => {
                                    parens -= 1;
                                    if parens == 0 {
                                        cursor += 1;
                                        break;
                                    }
                                }
                                Token::EndOfFile => return,
                                _ => {}
                            }
                            cursor += 1;
                        }
                        while matches!(self.tokens.get(cursor), Some(Token::Identifier(_))) {
                            cursor += 1;
                        }
                        if self.tokens.get(cursor) == Some(&Token::BraceOpen) {
                            self.inline_template_members
                                .insert((class_name.clone(), member_name.clone()));
                        }
                    }
                }
            }
            match token {
                Token::BraceOpen => brace_depth += 1,
                Token::BraceClose => {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        return;
                    }
                }
                Token::EndOfFile => return,
                _ => {}
            }
            index += 1;
        }
    }

    /// Instantiate `typedef Template<Concrete> Alias;` from a recovered
    /// template. Returns true only when the complete declaration was consumed
    /// conceptually; the caller's recovery scanner still advances the cursor.
    pub(crate) fn capture_skipped_template_typedef(&mut self) -> bool {
        self.capture_template_alias();
        let start = self.position;
        let Some(
            [Token::Identifier(typedef), Token::Identifier(template_name), Token::Less, argument_token, Token::Greater, Token::Identifier(alias), Token::Semicolon],
        ) = self.tokens.get(start..start + 7)
        else {
            return false;
        };
        if typedef != "typedef" {
            return false;
        }
        let Some(argument) = self.template_argument_type(argument_token) else {
            return false;
        };
        let Some(layout) = self.instantiate_struct_template_layout(template_name, Some(argument))
        else {
            return false;
        };
        self.structs.insert(alias.clone(), layout);
        self.struct_typedefs.insert(alias.clone(), alias.clone());
        true
    }

    /// Capture `typedef [Scope::]Template<...> Alias;` even when the concrete
    /// argument list is too complex for layout recovery. The immediate name
    /// before the outer `<` is the primary template; the final top-level name
    /// after its matching `>` is the alias.
    pub(crate) fn capture_template_alias(&mut self) {
        let start = self.position;
        if !matches!(self.tokens.get(start), Some(Token::Identifier(word)) if word == "typedef") {
            return;
        }
        let mut index = start + 1;
        let mut previous_identifier: Option<String> = None;
        let mut primary: Option<String> = None;
        let mut angle_depth = 0i32;
        let mut closed_outer = false;
        let mut alias: Option<String> = None;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Identifier(name) if angle_depth == 0 => {
                    if closed_outer {
                        alias = Some(name.clone());
                    }
                    previous_identifier = Some(name.clone());
                }
                Token::Less => {
                    if angle_depth == 0 && primary.is_none() {
                        primary = previous_identifier.clone();
                    }
                    angle_depth += 1;
                }
                Token::Greater if angle_depth > 0 => {
                    angle_depth -= 1;
                    if angle_depth == 0 {
                        closed_outer = true;
                    }
                }
                Token::Semicolon if angle_depth == 0 => break,
                Token::EndOfFile => return,
                _ => {}
            }
            index += 1;
        }
        if let (Some(primary), Some(alias)) = (primary, alias) {
            self.template_aliases.insert(alias, primary);
        }
    }

    pub(crate) fn template_argument_type(&self, token: &Token) -> Option<Type> {
        match token {
            Token::KeywordInt => Some(Type::Int),
            Token::KeywordChar => Some(Type::Char),
            Token::KeywordShort => Some(Type::Short),
            Token::KeywordUnsigned => Some(Type::UnsignedInt),
            Token::KeywordFloat => Some(Type::Float),
            Token::Identifier(name) if self.cplusplus && name == "wchar_t" => {
                Some(Type::UnsignedShort)
            }
            Token::Identifier(name) if self.cplusplus && name == "bool" => Some(Type::UnsignedChar),
            Token::Identifier(name) => self.typedefs.get(name).copied(),
            _ => None,
        }
    }
}
