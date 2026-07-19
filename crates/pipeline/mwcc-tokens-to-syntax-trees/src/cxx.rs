//! Metrowerks C++ surface syntax kept out of the general C item parser.
//!
//! Linkage specifications are declaration wrappers, not declarations themselves;
//! normalization removes those wrappers before recursive descent. Symbol names
//! use CodeWarrior's own mangling rather than the Itanium ABI.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Expression, Pointee, Statement, Type};
use mwcc_tokens::Token;

use crate::items::{type_alignment, type_size};
use crate::parser::{Parser, StructField, StructLayout};

/// The C++-only information that a plain C struct layout cannot retain.
/// Declaration order controls constructor initialization order, while base
/// names distinguish a base initializer from an identically shaped member.
#[derive(Default)]
pub(crate) struct ClassLayout {
    pub(crate) bases: Vec<BaseClass>,
    pub(crate) fields: Vec<String>,
    pub(crate) constructors: Vec<Vec<Type>>,
    pub(crate) methods: std::collections::HashMap<String, Vec<MemberMethod>>,
    /// The class has a virtual dispatch table pointer. This is layout state,
    /// not merely syntax: a polymorphic primary base already supplies the slot.
    pub(crate) is_polymorphic: bool,
}

pub(crate) struct MemberMethod {
    pub(crate) parameters: Vec<Type>,
    pub(crate) is_inline: bool,
}

pub(crate) struct BaseClass {
    pub(crate) name: String,
}

/// Remove C++ linkage-specification syntax while retaining every enclosed token
/// in source order. `extern "C" { declarations }` becomes `declarations`, and
/// `extern "C" declaration` keeps the `extern` storage class but drops the
/// language string. The latter distinction matters for data declarations.
pub(crate) fn normalize_linkage_specifications(mut tokens: Vec<Token>) -> Vec<Token> {
    let mut index = 0usize;
    while index + 1 < tokens.len() {
        let starts_linkage = matches!(&tokens[index], Token::Identifier(word) if word == "extern")
            && matches!(&tokens[index + 1], Token::StringLiteral(language) if language == b"C" || language == b"C++");
        if !starts_linkage {
            index += 1;
            continue;
        }

        if tokens.get(index + 2) == Some(&Token::BraceOpen) {
            let mut cursor = index + 2;
            let mut depth = 0i32;
            let mut close = None;
            while cursor < tokens.len() {
                match tokens[cursor] {
                    Token::BraceOpen => depth += 1,
                    Token::BraceClose => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(cursor);
                            break;
                        }
                    }
                    Token::EndOfFile => break,
                    _ => {}
                }
                cursor += 1;
            }
            if let Some(close) = close {
                tokens.remove(close);
                tokens.drain(index..index + 3);
                continue;
            }
        } else {
            // Keep `extern` so an object declaration remains a declaration rather
            // than becoming a tentative definition.
            tokens.remove(index + 1);
        }
        index += 1;
    }
    tokens
}

/// C++ constructors have no written return type. Insert the parser-internal
/// `void` only for a top-level `Class::Class(` declarator, leaving class-body
/// prototypes and expression-level qualified names untouched.
pub(crate) fn normalize_constructor_declarators(mut tokens: Vec<Token>) -> Vec<Token> {
    let mut index = 0usize;
    let mut brace_depth = 0usize;
    while index + 4 < tokens.len() {
        match tokens[index] {
            Token::BraceOpen => brace_depth += 1,
            Token::BraceClose => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
        let constructor = brace_depth == 0
            && matches!((&tokens[index], &tokens[index + 3]),
                (Token::Identifier(scope), Token::Identifier(name)) if scope == name)
            && tokens[index + 1] == Token::Colon
            && tokens[index + 2] == Token::Colon
            && tokens[index + 4] == Token::ParenOpen;
        if constructor {
            tokens.insert(index, Token::KeywordVoid);
            index += 6;
        } else {
            index += 1;
        }
    }
    tokens
}

impl Parser {
    /// Mangle a member declared in the active namespace scope. Class layouts
    /// remain keyed by their local source name; namespace qualification is an
    /// ABI concern applied only at symbol boundaries.
    pub(crate) fn mangle_member_in_current_namespace(
        &self,
        class: &str,
        function: &str,
        explicit_parameters: &[Type],
    ) -> Compilation<String> {
        let mut scopes: Vec<&str> = self.namespace_stack.iter().map(String::as_str).collect();
        scopes.push(class);
        mangle_qualified_member_function(&scopes, function, explicit_parameters)
    }

    /// Resolve an unqualified call inside a member body. Arity is enough for the
    /// currently modeled overload set; ambiguous same-arity overloads defer.
    pub(crate) fn resolve_implicit_member_call(
        &self,
        function: &str,
        argument_count: usize,
    ) -> Compilation<Option<(String, bool)>> {
        let Some(class_name) = self.current_member_scope.as_deref() else {
            return Ok(None);
        };
        let Some(methods) = self
            .cxx_classes
            .get(class_name)
            .and_then(|class| class.methods.get(function))
        else {
            return Ok(None);
        };
        let candidates: Vec<&MemberMethod> = methods
            .iter()
            .filter(|method| method.parameters.len() == argument_count)
            .collect();
        if candidates.len() != 1 {
            return Err(Diagnostic::error(format!(
                "member overload resolution for '{class_name}::{function}' is ambiguous or unavailable (roadmap)"
            )));
        }
        let method = candidates[0];
        Ok(Some((
            self.mangle_member_in_current_namespace(class_name, function, &method.parameters)?,
            method.is_inline,
        )))
    }

    /// Parse one class definition and recover its object layout.
    /// Method declarations do not occupy storage and are skipped after recording
    /// constructor signatures. A single non-virtual base is laid out first;
    /// simple polymorphic classes reserve their implicit vptr at offset zero.
    pub(crate) fn parse_class_definition(
        &mut self,
    ) -> Compilation<(String, StructLayout, ClassLayout)> {
        if !self.eat_word("class") {
            return Err(Diagnostic::error("expected a C++ class definition"));
        }
        let name = self.parse_identifier()?;
        let mut class = ClassLayout::default();
        let mut layout = StructLayout::default();
        let mut offset = 0u16;
        let mut max_align = 1u16;

        if self.eat_keyword(Token::Colon) {
            loop {
                while matches!(self.peek(), Token::Identifier(word)
                    if matches!(word.as_str(), "public" | "private" | "protected"))
                {
                    self.advance();
                }
                if self.eat_word("virtual") {
                    return Err(Diagnostic::error(
                        "virtual base-class layout is not supported yet (roadmap)",
                    ));
                }
                let base_name = self.parse_identifier()?;
                let base_is_polymorphic = self
                    .cxx_classes
                    .get(&base_name)
                    .is_some_and(|base| base.is_polymorphic);
                let base = self.structs.get(&base_name).ok_or_else(|| {
                    Diagnostic::error(format!(
                        "base class '{base_name}' must be defined before '{name}'"
                    ))
                })?;
                let base_align = (base.align as u16).max(1);
                offset = offset.div_ceil(base_align) * base_align;
                let base_offset = offset;
                for (field_name, field) in &base.fields {
                    layout.fields.insert(
                        field_name.clone(),
                        StructField {
                            member_type: field.member_type,
                            offset: base_offset + field.offset,
                            struct_tag: field.struct_tag.clone(),
                            array_element: field.array_element,
                            array_bytes: field.array_bytes,
                            bit_field: field.bit_field,
                        },
                    );
                }
                class.bases.push(BaseClass { name: base_name });
                class.is_polymorphic |= base_is_polymorphic;
                offset += base.size;
                max_align = max_align.max(base_align);
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
                return Err(Diagnostic::error(
                    "multiple inheritance is not supported yet (roadmap)",
                ));
            }
        }

        self.expect(Token::BraceOpen)?;
        while *self.peek() != Token::BraceClose {
            if matches!(self.peek(), Token::Identifier(word)
                if matches!(word.as_str(), "public" | "private" | "protected"))
                && *self.peek_at(1) == Token::Colon
            {
                self.advance();
                self.advance();
                continue;
            }
            if self.eat_word("virtual") {
                if !class.is_polymorphic {
                    // A new vptr is the primary object component. We can place it
                    // exactly while no base data has already occupied the prefix.
                    // Polymorphic derivation reuses the primary base's vptr above.
                    if offset != 0 {
                        return Err(Diagnostic::error(
                            "a polymorphic class with a non-polymorphic base is not supported yet (roadmap)",
                        ));
                    }
                    offset = 4;
                    max_align = max_align.max(4);
                    class.is_polymorphic = true;
                }
            }
            if self.eat_word("static") {
                self.skip_class_member()?;
                continue;
            }
            if matches!(self.peek(), Token::Identifier(word) if word == &name)
                && *self.peek_at(1) == Token::ParenOpen
            {
                self.advance();
                class.constructors.push(self.parse_class_parameter_types()?);
                self.skip_class_method_tail()?;
                continue;
            }
            if *self.peek() == Token::Tilde {
                self.skip_class_member()?;
                continue;
            }

            let field_type = self.parse_type()?;
            if self.last_array_typedef.take().is_some() {
                return Err(Diagnostic::error(
                    "an array-typedef class member is not supported yet (roadmap)",
                ));
            }
            let struct_tag = self.last_struct_tag.take();
            let attribute_align = self.skip_attributes()?.unwrap_or(1);
            let field_name = self.parse_identifier()?;
            if *self.peek() == Token::ParenOpen {
                let parameters = self.parse_class_parameter_types()?;
                let is_inline = self.skip_class_method_tail()?;
                class
                    .methods
                    .entry(field_name)
                    .or_default()
                    .push(MemberMethod {
                        parameters,
                        is_inline,
                    });
                continue;
            }
            if matches!(self.peek(), Token::Colon) {
                return Err(Diagnostic::error(
                    "a C++ bit-field member is not supported yet (roadmap)",
                ));
            }
            if matches!(self.peek(), Token::BracketOpen) {
                return Err(Diagnostic::error(
                    "a C++ array member is not supported yet (roadmap)",
                ));
            }
            if *self.peek() != Token::Semicolon {
                return Err(Diagnostic::error(
                    "a multi-declarator class member is not supported yet (roadmap)",
                ));
            }
            self.advance();
            let align = type_alignment(field_type).max(attribute_align).max(1);
            offset = offset.div_ceil(align) * align;
            layout.fields.insert(
                field_name.clone(),
                StructField {
                    member_type: field_type,
                    offset,
                    struct_tag,
                    array_element: None,
                    array_bytes: None,
                    bit_field: None,
                },
            );
            class.fields.push(field_name);
            offset += type_size(field_type);
            max_align = max_align.max(align);
        }
        self.expect(Token::BraceClose)?;
        self.expect(Token::Semicolon)?;
        // C++ gives an otherwise empty class size one. Empty-base optimization is
        // deliberately outside this subset.
        layout.size = offset.max(1).div_ceil(max_align) * max_align;
        layout.align = max_align as u8;
        Ok((name, layout, class))
    }

    fn parse_class_parameter_types(&mut self) -> Compilation<Vec<Type>> {
        self.expect(Token::ParenOpen)?;
        let mut parameters = Vec::new();
        if *self.peek() == Token::KeywordVoid && *self.peek_at(1) == Token::ParenClose {
            self.advance();
        } else {
            while *self.peek() != Token::ParenClose {
                let parameter_type = self.parse_type()?;
                self.last_array_typedef.take();
                self.last_struct_tag.take();
                if matches!(self.peek(), Token::Identifier(_)) {
                    self.advance();
                }
                parameters.push(parameter_type);
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
        }
        self.expect(Token::ParenClose)?;
        Ok(parameters)
    }

    fn skip_class_method_tail(&mut self) -> Compilation<bool> {
        while matches!(self.peek(), Token::Identifier(word)
            if matches!(word.as_str(), "const" | "override" | "final"))
        {
            self.advance();
        }
        if self.eat_keyword(Token::Equals) {
            self.advance();
        }
        if self.eat_keyword(Token::Semicolon) {
            return Ok(false);
        }
        if *self.peek() == Token::BraceOpen {
            self.skip_balanced(Token::BraceOpen, Token::BraceClose)?;
            return Ok(true);
        }
        Err(Diagnostic::error(format!(
            "unsupported C++ member declaration tail: {}",
            self.peek()
        )))
    }

    fn skip_class_member(&mut self) -> Compilation<()> {
        let mut parens = 0usize;
        while *self.peek() != Token::EndOfFile {
            match self.advance() {
                Token::ParenOpen => parens += 1,
                Token::ParenClose => parens = parens.saturating_sub(1),
                Token::Semicolon if parens == 0 => return Ok(()),
                Token::BraceOpen if parens == 0 => {
                    self.position -= 1;
                    return self.skip_balanced(Token::BraceOpen, Token::BraceClose);
                }
                _ => {}
            }
        }
        Err(Diagnostic::error("unterminated C++ member declaration"))
    }

    fn skip_balanced(&mut self, open: Token, close: Token) -> Compilation<()> {
        self.expect(open.clone())?;
        let mut depth = 1usize;
        while depth > 0 {
            let token = self.advance();
            if token == open {
                depth += 1;
            } else if token == close {
                depth -= 1;
            } else if token == Token::EndOfFile {
                return Err(Diagnostic::error("unterminated C++ declaration"));
            }
        }
        Ok(())
    }

    /// Consume a constructor initializer list and lower it to ordinary IR in
    /// language-mandated order: bases first, then members in declaration order.
    pub(crate) fn parse_constructor_initializers(
        &mut self,
        scope: &str,
    ) -> Compilation<Vec<Statement>> {
        if !self.eat_keyword(Token::Colon) {
            return Ok(Vec::new());
        }
        let class = self.cxx_classes.get(scope).ok_or_else(|| {
            Diagnostic::error(format!(
                "class layout for constructor '{scope}' was not recovered"
            ))
        })?;
        let base_names: Vec<String> = class.bases.iter().map(|base| base.name.clone()).collect();
        let field_names = class.fields.clone();
        let mut initializers = std::collections::HashMap::new();
        loop {
            let target = self.parse_identifier()?;
            self.expect(Token::ParenOpen)?;
            let mut arguments = Vec::new();
            if *self.peek() != Token::ParenClose {
                loop {
                    arguments.push(self.expression()?);
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                }
            }
            self.expect(Token::ParenClose)?;
            if initializers.insert(target.clone(), arguments).is_some() {
                return Err(Diagnostic::error(format!(
                    "duplicate constructor initializer for '{target}'"
                )));
            }
            if !self.eat_keyword(Token::Comma) {
                break;
            }
        }

        let mut statements = Vec::new();
        for base_name in base_names {
            let Some(arguments) = initializers.remove(&base_name) else {
                return Err(Diagnostic::error(format!(
                    "implicit base construction for '{base_name}' is not supported yet (roadmap)"
                )));
            };
            let signatures = &self
                .cxx_classes
                .get(&base_name)
                .ok_or_else(|| {
                    Diagnostic::error(format!(
                        "base class layout for '{base_name}' was not recovered"
                    ))
                })?
                .constructors;
            let candidates: Vec<&Vec<Type>> = signatures
                .iter()
                .filter(|signature| signature.len() == arguments.len())
                .collect();
            if candidates.len() != 1 {
                return Err(Diagnostic::error(format!(
                    "constructor overload resolution for '{base_name}' is ambiguous or unavailable (roadmap)"
                )));
            }
            let name =
                self.mangle_member_in_current_namespace(base_name.as_str(), "__ct", candidates[0])?;
            let mut call_arguments = vec![Expression::Variable("this".to_string())];
            call_arguments.extend(arguments);
            statements.push(Statement::Expression(Expression::Call {
                name,
                arguments: call_arguments,
            }));
        }
        let layout = self.structs.get(scope).ok_or_else(|| {
            Diagnostic::error(format!(
                "class layout for constructor '{scope}' was not recovered"
            ))
        })?;
        for field_name in field_names {
            let Some(mut arguments) = initializers.remove(&field_name) else {
                continue;
            };
            let field = layout.fields.get(&field_name).ok_or_else(|| {
                Diagnostic::error(format!(
                    "member '{field_name}' is absent from class '{scope}'"
                ))
            })?;
            if field.struct_tag.is_some() || arguments.len() != 1 {
                return Err(Diagnostic::error(format!(
                    "non-scalar constructor initialization for '{field_name}' is not supported yet (roadmap)"
                )));
            }
            statements.push(Statement::Store {
                target: Expression::Member {
                    base: Box::new(Expression::Variable("this".to_string())),
                    offset: field.offset,
                    member_type: field.member_type,
                    index_stride: None,
                },
                value: arguments.remove(0),
            });
        }
        if let Some(unknown) = initializers.keys().next() {
            return Err(Diagnostic::error(format!(
                "unknown constructor initializer '{unknown}' in class '{scope}'"
            )));
        }
        Ok(statements)
    }
}

/// Mangle an ordinary, singly-qualified member function.
///
/// Examples measured from mwcceppc:
/// `void KartCannon::Init(int)` -> `Init__10KartCannonFi`
/// `void KartCannon::DoKeep()` -> `DoKeep__10KartCannonFv`
pub(crate) fn mangle_member_function(
    scope: &str,
    function: &str,
    explicit_parameters: &[Type],
) -> Compilation<String> {
    mangle_qualified_member_function(&[scope], function, explicit_parameters)
}

/// Mangle a class member qualified by one or more scopes. CodeWarrior encodes
/// one class directly (`7Counter`) and nested namespace/class names as
/// `Q<count><length><name>...` (`Q26sample7Counter`).
pub(crate) fn mangle_qualified_member_function(
    scopes: &[&str],
    function: &str,
    explicit_parameters: &[Type],
) -> Compilation<String> {
    if scopes.is_empty() || scopes.iter().any(|scope| scope.is_empty()) || function.is_empty() {
        return Err(Diagnostic::error("an empty C++ member name is invalid"));
    }
    let arguments = if explicit_parameters.is_empty() {
        "v".to_string()
    } else {
        explicit_parameters
            .iter()
            .copied()
            .map(encode_type)
            .collect::<Compilation<Vec<_>>>()?
            .concat()
    };
    let qualified_scope = if scopes.len() == 1 {
        format!("{}{}", scopes[0].len(), scopes[0])
    } else {
        let components = scopes
            .iter()
            .map(|scope| format!("{}{scope}", scope.len()))
            .collect::<String>();
        format!("Q{}{components}", scopes.len())
    };
    Ok(format!("{function}__{qualified_scope}F{arguments}"))
}

fn encode_type(parameter: Type) -> Compilation<String> {
    let code = match parameter {
        Type::Int => "i".to_string(),
        Type::UnsignedInt => "Ui".to_string(),
        Type::Char => "c".to_string(),
        Type::UnsignedChar => "Uc".to_string(),
        Type::Short => "s".to_string(),
        Type::UnsignedShort => "Us".to_string(),
        Type::Float => "f".to_string(),
        Type::Double => "d".to_string(),
        Type::LongLong => "x".to_string(),
        Type::UnsignedLongLong => "Ux".to_string(),
        Type::Pointer(pointee) => format!("P{}", encode_pointee(pointee)?),
        Type::Void => {
            return Err(Diagnostic::error(
                "a named void C++ parameter is not supported",
            ))
        }
        Type::StructPointer { .. } | Type::Struct { .. } => {
            return Err(Diagnostic::error(
                "a struct-valued C++ member parameter needs qualified type mangling (roadmap)",
            ))
        }
    };
    Ok(code)
}

fn encode_pointee(pointee: Pointee) -> Compilation<&'static str> {
    match pointee {
        Pointee::Int => Ok("i"),
        Pointee::UnsignedInt => Ok("Ui"),
        Pointee::Char => Ok("c"),
        Pointee::UnsignedChar => Ok("Uc"),
        Pointee::Short => Ok("s"),
        Pointee::UnsignedShort => Ok("Us"),
        Pointee::Float => Ok("f"),
        Pointee::Double => Ok("d"),
        Pointee::LongLong => Ok("x"),
        Pointee::UnsignedLongLong => Ok("Ux"),
        Pointee::Pointer | Pointee::WordPointer => Err(Diagnostic::error(
            "a pointer-to-pointer C++ member parameter needs exact pointee mangling (roadmap)",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_block_linkage_without_losing_declarations() {
        let tokens = vec![
            Token::Identifier("extern".to_string()),
            Token::StringLiteral(b"C".to_vec()),
            Token::BraceOpen,
            Token::KeywordInt,
            Token::Identifier("value".to_string()),
            Token::Semicolon,
            Token::BraceClose,
            Token::EndOfFile,
        ];
        assert_eq!(
            normalize_linkage_specifications(tokens),
            vec![
                Token::KeywordInt,
                Token::Identifier("value".to_string()),
                Token::Semicolon,
                Token::EndOfFile,
            ]
        );
    }

    #[test]
    fn mangles_measured_member_shapes() {
        assert_eq!(
            mangle_member_function("KartCannon", "Init", &[Type::Int]).unwrap(),
            "Init__10KartCannonFi"
        );
        assert_eq!(
            mangle_member_function("KartCannon", "DoKeep", &[]).unwrap(),
            "DoKeep__10KartCannonFv"
        );
        assert_eq!(
            mangle_member_function("Counter", "__ct", &[Type::Int, Type::Short]).unwrap(),
            "__ct__7CounterFis"
        );
        assert_eq!(
            mangle_qualified_member_function(
                &["homebutton", "FrameController"],
                "init",
                &[Type::Int, Type::Float, Type::Float, Type::Float],
            )
            .unwrap(),
            "init__Q210homebutton15FrameControllerFifff"
        );
    }

    #[test]
    fn adds_internal_return_type_only_to_out_of_class_constructors() {
        let tokens = vec![
            Token::Identifier("class".to_string()),
            Token::Identifier("Counter".to_string()),
            Token::BraceOpen,
            Token::Identifier("Counter".to_string()),
            Token::ParenOpen,
            Token::KeywordInt,
            Token::ParenClose,
            Token::Semicolon,
            Token::BraceClose,
            Token::Semicolon,
            Token::Identifier("Counter".to_string()),
            Token::Colon,
            Token::Colon,
            Token::Identifier("Counter".to_string()),
            Token::ParenOpen,
            Token::KeywordInt,
            Token::ParenClose,
            Token::BraceOpen,
            Token::BraceClose,
            Token::EndOfFile,
        ];
        let normalized = normalize_constructor_declarators(tokens);
        assert_eq!(
            normalized
                .iter()
                .filter(|token| **token == Token::KeywordVoid)
                .count(),
            1
        );
        let constructor = normalized
            .windows(6)
            .find(|window| {
                window[0] == Token::KeywordVoid
                    && matches!(&window[1], Token::Identifier(name) if name == "Counter")
            })
            .unwrap();
        assert_eq!(constructor[2], Token::Colon);
        assert_eq!(constructor[3], Token::Colon);
    }
}
