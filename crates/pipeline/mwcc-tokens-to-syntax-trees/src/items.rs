//! Parsing of types, functions, parameters, locals, and guarded returns.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Function, GlobalDeclaration, GuardedReturn, LocalDeclaration, Parameter, Pointee, Statement, TranslationUnit, Type};
use mwcc_tokens::Token;

use crate::parser::{Parser, StructField, StructLayout};

/// The pointee kind for `<scalar>*`. Pointer-to-pointer and pointer-to-aggregate
/// are not in the subset yet.
fn pointee_of(base: Type) -> Compilation<Pointee> {
    match base {
        Type::Int => Ok(Pointee::Int),
        Type::UnsignedInt => Ok(Pointee::UnsignedInt),
        Type::Char => Ok(Pointee::Char),
        Type::UnsignedChar => Ok(Pointee::UnsignedChar),
        Type::Short => Ok(Pointee::Short),
        Type::UnsignedShort => Ok(Pointee::UnsignedShort),
        Type::Float => Ok(Pointee::Float),
        other => Err(Diagnostic::error(format!("pointer to {other:?} is not supported yet"))),
    }
}

/// Size in bytes of a scalar or pointer type, for laying out struct members.
fn type_size(declared: Type) -> u16 {
    match declared {
        Type::Pointer(_) | Type::StructPointer => 4,
        other => (other.width() / 8) as u16,
    }
}

impl Parser {
    pub(crate) fn parse_type(&mut self) -> Compilation<Type> {
        self.last_struct_tag = None;
        // `struct Name*` — a pointer to a (already declared) struct. The tag is
        // stashed in `last_struct_tag` for the declarator parser to record.
        if *self.peek() == Token::KeywordStruct {
            self.advance();
            let tag = self.parse_identifier()?;
            if *self.peek() != Token::Star {
                return Err(Diagnostic::error("struct values are not supported yet — use a struct pointer"));
            }
            self.advance();
            self.last_struct_tag = Some(tag);
            return Ok(Type::StructPointer);
        }
        let base = match self.advance() {
            Token::KeywordInt => Type::Int,
            Token::KeywordChar => Type::Char,
            Token::KeywordShort => Type::Short,
            // `unsigned` may be followed by char/short/int.
            Token::KeywordUnsigned => match self.peek() {
                Token::KeywordChar => {
                    self.advance();
                    Type::UnsignedChar
                }
                Token::KeywordShort => {
                    self.advance();
                    Type::UnsignedShort
                }
                Token::KeywordInt => {
                    self.advance();
                    Type::UnsignedInt
                }
                _ => Type::UnsignedInt,
            },
            Token::KeywordFloat => Type::Float,
            Token::KeywordVoid => Type::Void,
            other => return Err(Diagnostic::error(format!("expected a type, found {other}"))),
        };
        // A trailing `*` makes it a pointer to that scalar.
        if *self.peek() == Token::Star {
            self.advance();
            return Ok(Type::Pointer(pointee_of(base)?));
        }
        Ok(base)
    }

    /// Parse `struct Name { type field; ... };`, laying members out with natural
    /// alignment (the `-align powerpc` default) and registering the layout.
    pub(crate) fn parse_struct_definition(&mut self) -> Compilation<()> {
        self.expect(Token::KeywordStruct)?;
        let tag = self.parse_identifier()?;
        self.expect(Token::BraceOpen)?;
        let mut layout = StructLayout::default();
        let mut offset: u16 = 0;
        while *self.peek() != Token::BraceClose {
            let field_type = self.parse_type()?;
            let field_name = self.parse_identifier()?;
            self.expect(Token::Semicolon)?;
            let size = type_size(field_type);
            // Natural alignment: a member starts at the next multiple of its size.
            let alignment = size.max(1);
            offset = offset.div_ceil(alignment) * alignment;
            layout.fields.insert(field_name, StructField { member_type: field_type, offset });
            offset += size;
        }
        self.expect(Token::BraceClose)?;
        self.expect(Token::Semicolon)?;
        self.structs.insert(tag, layout);
        Ok(())
    }

    pub(crate) fn translation_unit(&mut self) -> Compilation<TranslationUnit> {
        // Collect file-scope globals and skip prototype declarations until the
        // function *definition* (the signature followed by `{`).
        let mut globals = Vec::new();
        let (return_type, name, parameters) = loop {
            // `extern`/`static` storage qualifiers don't change codegen here.
            while matches!(self.peek(), Token::Identifier(word) if word == "extern" || word == "static") {
                self.advance();
            }
            // A `struct Name { ... };` definition registers a layout. A `struct
            // Name*` use (function return or parameter) falls through to parse_type.
            if *self.peek() == Token::KeywordStruct && self.tokens.get(self.position + 2) == Some(&Token::BraceOpen) {
                self.parse_struct_definition()?;
                continue;
            }
            let return_type = self.parse_type()?;
            let name = self.parse_identifier()?;
            // `type name;` or `type a, b, c;` is a global variable declaration.
            if matches!(self.peek(), Token::Semicolon | Token::Comma) {
                globals.push(GlobalDeclaration { declared_type: return_type, name });
                while *self.peek() == Token::Comma {
                    self.advance();
                    let next = self.parse_identifier()?;
                    globals.push(GlobalDeclaration { declared_type: return_type, name: next });
                }
                self.expect(Token::Semicolon)?;
                continue;
            }
            self.expect(Token::ParenOpen)?;

            let mut parameters = Vec::new();
            if *self.peek() == Token::KeywordVoid {
                self.advance();
            } else if *self.peek() != Token::ParenClose {
                loop {
                    let parameter_type = self.parse_type()?;
                    let struct_tag = self.last_struct_tag.take();
                    // The name is optional (a prototype may write just the type).
                    let name = if matches!(self.peek(), Token::Identifier(_)) {
                        self.parse_identifier()?
                    } else {
                        String::new()
                    };
                    if let Some(tag) = struct_tag {
                        if !name.is_empty() {
                            self.variable_structs.insert(name.clone(), tag);
                        }
                    }
                    parameters.push(Parameter { parameter_type, name });
                    if *self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect(Token::ParenClose)?;

            if *self.peek() == Token::Semicolon {
                self.advance(); // a prototype — keep looking for the definition
                continue;
            }
            break (return_type, name, parameters);
        };

        self.expect(Token::BraceOpen)?;

        // Zero or more local declarations precede the return statement. A
        // statement that begins with a type keyword is a local declaration;
        // `return` ends the body.
        let mut locals = Vec::new();
        while self.peek_is_type() {
            let declared_type = self.parse_type()?;
            let struct_tag = self.last_struct_tag.take();
            let name = self.parse_identifier()?;
            if let Some(tag) = struct_tag {
                self.variable_structs.insert(name.clone(), tag);
            }
            self.expect(Token::Equals)?;
            let initializer = self.expression()?;
            self.expect(Token::Semicolon)?;
            locals.push(LocalDeclaration { declared_type, name, initializer });
        }

        // Zero or more statements: a store `*p = v;` / `p[i] = v;`, or a bare
        // expression evaluated for effect like a call `g();`.
        let mut statements = Vec::new();
        while !matches!(self.peek(), Token::KeywordReturn | Token::KeywordIf | Token::BraceClose) {
            let first = self.factor()?;
            if *self.peek() == Token::Equals {
                self.advance();
                let value = self.expression()?;
                self.expect(Token::Semicolon)?;
                statements.push(Statement::Store { target: first, value });
            } else {
                self.expect(Token::Semicolon)?;
                statements.push(Statement::Expression(first));
            }
        }

        // Zero or more guarded early returns: `if (condition) return value;`.
        let mut guards = Vec::new();
        while *self.peek() == Token::KeywordIf {
            self.advance();
            self.expect(Token::ParenOpen)?;
            let condition = self.expression()?;
            self.expect(Token::ParenClose)?;
            self.expect(Token::KeywordReturn)?;
            let value = self.expression()?;
            self.expect(Token::Semicolon)?;
            guards.push(GuardedReturn { condition, value });
        }

        // The final `return <expr>;` is optional — a `void` function may end after
        // its statements.
        let return_expression = if *self.peek() == Token::KeywordReturn {
            self.advance();
            let value = self.expression()?;
            self.expect(Token::Semicolon)?;
            Some(value)
        } else {
            None
        };
        self.expect(Token::BraceClose)?;

        let function = Function { return_type, name, parameters, locals, statements, guards, return_expression };
        Ok(TranslationUnit { globals, function })
    }

    pub(crate) fn peek_is_type(&self) -> bool {
        matches!(
            self.peek(),
            Token::KeywordInt
                | Token::KeywordChar
                | Token::KeywordShort
                | Token::KeywordUnsigned
                | Token::KeywordFloat
                | Token::KeywordVoid
                | Token::KeywordStruct
        )
    }
}
