//! Parsing of types, functions, parameters, locals, and guarded returns.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, GlobalDeclaration, GuardedReturn, LocalDeclaration, Parameter, Pointee, Statement, SwitchArm, TranslationUnit, Type};
use mwcc_tokens::Token;

use crate::parser::{Parser, StructField, StructLayout};

/// `target` assigned `value`: a reassignment of a tracked local, or a memory
/// store to any other lvalue (`*p`, `p[i]`, a member, a global).
fn store_or_assign(target: Expression, value: Expression, local_names: &std::collections::HashSet<&str>) -> Statement {
    match &target {
        Expression::Variable(name) if local_names.contains(name.as_str()) => Statement::Assign { name: name.clone(), value },
        _ => Statement::Store { target, value },
    }
}

/// The `target +/- 1` expression an increment/decrement statement assigns back.
fn increment_value(operator: BinaryOperator, target: &Expression) -> Expression {
    Expression::Binary { operator, left: Box::new(target.clone()), right: Box::new(Expression::IntegerLiteral(1)) }
}

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
        Type::Double => Ok(Pointee::Double),
        // `void *` is a 4-byte opaque pointer — only passed, stored, or compared
        // (dereferencing or indexing it is not valid C), so the pointee width is
        // never used. Model it as a word pointer.
        Type::Void => Ok(Pointee::Int),
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
    /// Consume an identifier token if it matches `word` (used for the `long` and
    /// `signed`/`unsigned` specifier words that aren't dedicated keywords).
    fn eat_word(&mut self, word: &str) -> bool {
        if matches!(self.peek(), Token::Identifier(found) if found == word) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Consume `token` if it is next; report whether it was.
    fn eat_keyword(&mut self, token: Token) -> bool {
        if *self.peek() == token {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Parse a single integer constant: an integer literal, optionally negated.
    /// Parse an enum body `{ NAME [= value], … }` (cursor at the `{`), registering
    /// each enumerator's value (auto-incrementing from 0, or an explicit constant).
    fn parse_enum_body(&mut self) -> Compilation<()> {
        self.expect(Token::BraceOpen)?;
        let mut next = 0i64;
        while *self.peek() != Token::BraceClose {
            let name = self.parse_identifier()?;
            let value = if self.eat_keyword(Token::Equals) { self.parse_enum_value()? } else { next };
            self.enum_constants.insert(name, value);
            next = value + 1;
            if *self.peek() == Token::Comma {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(Token::BraceClose)?;
        Ok(())
    }

    /// Evaluate a constant enumerator expression — integer/char literals, prior
    /// enumerators, parentheses, and left-to-right `+ - * & | ^ << >>`.
    fn parse_enum_value(&mut self) -> Compilation<i64> {
        let mut value = self.parse_enum_primary()?;
        loop {
            value = match self.peek() {
                Token::Plus => { self.advance(); value + self.parse_enum_primary()? }
                Token::Minus => { self.advance(); value - self.parse_enum_primary()? }
                Token::Star => { self.advance(); value * self.parse_enum_primary()? }
                Token::Ampersand => { self.advance(); value & self.parse_enum_primary()? }
                Token::Pipe => { self.advance(); value | self.parse_enum_primary()? }
                Token::Caret => { self.advance(); value ^ self.parse_enum_primary()? }
                Token::ShiftLeft => { self.advance(); value << self.parse_enum_primary()? }
                Token::ShiftRight => { self.advance(); value >> self.parse_enum_primary()? }
                _ => break,
            };
        }
        Ok(value)
    }

    fn parse_enum_primary(&mut self) -> Compilation<i64> {
        let negative = self.eat_keyword(Token::Minus);
        let value = match self.advance() {
            Token::IntegerLiteral(value) => value,
            Token::Identifier(name) => *self
                .enum_constants
                .get(&name)
                .ok_or_else(|| Diagnostic::error(format!("non-constant enumerator value '{name}'")))?,
            Token::ParenOpen => {
                let value = self.parse_enum_value()?;
                self.expect(Token::ParenClose)?;
                value
            }
            other => return Err(Diagnostic::error(format!("expected an enumerator value, found {other}"))),
        };
        Ok(if negative { -value } else { value })
    }

    fn parse_integer_constant(&mut self) -> Compilation<i64> {
        let negative = self.eat_keyword(Token::Minus);
        match self.advance() {
            Token::IntegerLiteral(value) => Ok(if negative { -(value as i64) } else { value as i64 }),
            other => Err(Diagnostic::error(format!("only integer-constant global initializers are supported, found {other}"))),
        }
    }

    /// Parse `switch (scrutinee) { case <int>: return E; ... default: return E; }`.
    /// The subset requires every arm to be a single `return`; fall-through, blocks,
    /// and non-constant case labels are not supported yet.
    fn parse_switch(&mut self) -> Compilation<Statement> {
        self.eat_word("switch");
        self.expect(Token::ParenOpen)?;
        let scrutinee = self.expression()?;
        self.expect(Token::ParenClose)?;
        self.expect(Token::BraceOpen)?;
        let mut arms = Vec::new();
        let mut default = None;
        while *self.peek() != Token::BraceClose {
            if self.eat_word("case") {
                let value = self.parse_integer_constant()?;
                self.expect(Token::Colon)?;
                self.expect(Token::KeywordReturn)?;
                let result = self.expression()?;
                self.expect(Token::Semicolon)?;
                arms.push(SwitchArm { value, result });
            } else if self.eat_word("default") {
                self.expect(Token::Colon)?;
                self.expect(Token::KeywordReturn)?;
                default = Some(self.expression()?);
                self.expect(Token::Semicolon)?;
            } else {
                return Err(Diagnostic::error("a switch arm must be `case <int>: return …;` or `default: return …;` (roadmap)"));
            }
        }
        self.expect(Token::BraceClose)?;
        Ok(Statement::Switch { scrutinee, arms, default })
    }

    /// Parse a global's constant initializer: a scalar `<const>` (one element) or
    /// an aggregate `{ <const>, ... }` (several, with an optional trailing comma).
    fn parse_constant_initializer(&mut self) -> Compilation<Vec<i64>> {
        if self.eat_keyword(Token::BraceOpen) {
            let mut values = Vec::new();
            while *self.peek() != Token::BraceClose {
                values.push(self.parse_integer_constant()?);
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
            self.expect(Token::BraceClose)?;
            Ok(values)
        } else {
            Ok(vec![self.parse_integer_constant()?])
        }
    }

    pub(crate) fn parse_type(&mut self) -> Compilation<Type> {
        self.last_struct_tag = None;
        // Leading qualifiers: `const`/`register` are transparent to codegen (`const`
        // is noted for the global path, which defers a read-only global); `volatile`
        // changes access semantics (memory accesses can't be elided), so defer it.
        self.skip_type_qualifiers()?;
        // `enum [Tag] [{ … }]` — an `int` (`-enum int`); a `{ … }` body registers
        // its enumerators so a bare enumerator resolves to its value.
        if matches!(self.peek(), Token::Identifier(word) if word == "enum") {
            self.advance();
            if matches!(self.peek(), Token::Identifier(_)) {
                self.advance(); // the tag
            }
            if *self.peek() == Token::BraceOpen {
                self.parse_enum_body()?;
            }
            return Ok(Type::Int);
        }
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
        // A struct typedef (`FILE`) behaves like its `struct Tag`: `FILE *` is a
        // struct pointer carrying the layout's tag; a struct value isn't supported.
        if let Token::Identifier(name) = self.peek() {
            if let Some(tag) = self.struct_typedefs.get(name).cloned() {
                self.advance();
                if *self.peek() != Token::Star {
                    return Err(Diagnostic::error("struct values are not supported yet — use a struct pointer"));
                }
                self.advance();
                self.last_struct_tag = Some(tag);
                return Ok(Type::StructPointer);
            }
        }
        // A `typedef`-declared alias resolves to its underlying type.
        if let Token::Identifier(name) = self.peek() {
            if let Some(&aliased) = self.typedefs.get(name) {
                self.advance();
                if *self.peek() == Token::Star {
                    self.advance();
                    return Ok(Type::Pointer(pointee_of(aliased)?));
                }
                return Ok(aliased);
            }
        }
        let base = match self.advance() {
            Token::KeywordInt => Type::Int,
            Token::KeywordChar => Type::Char,
            // `short` / `short int`.
            Token::KeywordShort => {
                let _ = self.eat_keyword(Token::KeywordInt);
                Type::Short
            }
            // `unsigned` and its widths, including `unsigned long [long] [int]`.
            Token::KeywordUnsigned => match self.peek() {
                Token::KeywordChar => {
                    self.advance();
                    Type::UnsignedChar
                }
                Token::KeywordShort => {
                    self.advance();
                    let _ = self.eat_keyword(Token::KeywordInt);
                    Type::UnsignedShort
                }
                Token::KeywordInt => {
                    self.advance();
                    Type::UnsignedInt
                }
                // `unsigned long`, `unsigned long long`, `unsigned long int` — all
                // 32-bit unsigned on this target.
                Token::Identifier(word) if word == "long" => {
                    while self.eat_word("long") {}
                    let _ = self.eat_keyword(Token::KeywordInt);
                    Type::UnsignedInt
                }
                _ => Type::UnsignedInt,
            },
            Token::KeywordFloat => Type::Float,
            Token::KeywordVoid => Type::Void,
            // `double` (and `long double`, which is also 64-bit here).
            Token::Identifier(word) if word == "double" => Type::Double,
            // `long`, `long long`, `long int` — 32-bit signed; `long double` is a
            // double.
            Token::Identifier(word) if word == "long" => {
                while self.eat_word("long") {}
                if self.eat_word("double") {
                    Type::Double
                } else {
                    let _ = self.eat_keyword(Token::KeywordInt);
                    Type::Int
                }
            }
            // `signed [char|short|int|long]`.
            Token::Identifier(word) if word == "signed" => match self.peek() {
                Token::KeywordChar => {
                    self.advance();
                    Type::Char
                }
                Token::KeywordShort => {
                    self.advance();
                    let _ = self.eat_keyword(Token::KeywordInt);
                    Type::Short
                }
                // `signed`, `signed int`, `signed long [long] [int]` — all 32-bit
                // signed on this target.
                _ => {
                    while self.eat_word("long") {}
                    let _ = self.eat_keyword(Token::KeywordInt);
                    Type::Int
                }
            },
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
        let layout = self.parse_struct_body()?;
        self.expect(Token::Semicolon)?;
        self.structs.insert(tag, layout);
        Ok(())
    }

    /// Parse a struct body `{ field; … }` (the cursor is at the `{`), returning its
    /// layout. Does not consume any trailing `;` — the caller (a definition or a
    /// typedef) does.
    pub(crate) fn parse_struct_body(&mut self) -> Compilation<StructLayout> {
        self.expect(Token::BraceOpen)?;
        let mut layout = StructLayout::default();
        let mut offset: u16 = 0;
        while *self.peek() != Token::BraceClose {
            let field_type = self.parse_type()?;
            let struct_tag = self.last_struct_tag.take();
            let field_name = self.parse_identifier()?;
            // An array member `type name[N]` occupies `N` elements; its access
            // yields the array address rather than a loaded value.
            let mut array_element = None;
            let mut size = type_size(field_type);
            let element_size = size;
            if *self.peek() == Token::BracketOpen {
                self.advance();
                let count = match self.advance() {
                    Token::IntegerLiteral(value) => value as u16,
                    other => return Err(Diagnostic::error(format!("expected an array length, found {other}"))),
                };
                self.expect(Token::BracketClose)?;
                array_element = Some(pointee_of(field_type)?);
                size = count * element_size;
            }
            self.expect(Token::Semicolon)?;
            // Natural alignment: to the element size (for an array, that element).
            let alignment = element_size.max(1);
            offset = offset.div_ceil(alignment) * alignment;
            layout.fields.insert(field_name, StructField { member_type: field_type, offset, struct_tag, array_element });
            offset += size;
        }
        self.expect(Token::BraceClose)?;
        Ok(layout)
    }

    pub(crate) fn translation_unit(&mut self) -> Compilation<TranslationUnit> {
        // Walk the top level in source order: struct definitions register layouts,
        // `type name;` lines are globals, `type name(params);` are prototypes, and
        // `type name(params) { ... }` are function definitions. Each definition is
        // lowered to its own object symbol downstream, so they are collected in
        // order.
        let mut globals = Vec::new();
        let mut functions = Vec::new();
        let mut prototypes = Vec::new();
        while *self.peek() != Token::EndOfFile {
            let start = self.position;
            if let Err(error) = self.parse_top_level_item(&mut globals, &mut functions, &mut prototypes) {
                // A declaration we can't parse (a typedef/struct/extern prototype or
                // qualified type from a preprocessed header) is skipped so the
                // function definitions can still be compiled; a function definition we
                // are expected to compile is propagated, deferring the unit honestly.
                self.position = start;
                if self.item_is_function_definition() {
                    return Err(error);
                }
                // A skipped `static inline` function with an inline `asm {}` body
                // still contributes a local undefined symbol (mwcc cannot inline it).
                if let Some(name) = self.inline_asm_function_name() {
                    self.inline_asm_symbols.push(name);
                }
                self.skip_top_level_declaration();
            }
        }
        Ok(TranslationUnit { globals, functions, prototypes, inline_asm_symbols: std::mem::take(&mut self.inline_asm_symbols) })
    }

    /// If the item at the cursor is an `inline`/`static inline` function whose body
    /// contains an inline `asm` block, return its name (mwcc emits a local symbol
    /// for it). Pure lookahead — consumes nothing.
    fn inline_asm_function_name(&self) -> Option<String> {
        let mut index = self.position;
        let mut is_inline = false;
        let mut name: Option<String> = None;
        // Signature up to the first `(`: note `inline`, and the last identifier
        // before the `(` (the function name).
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Identifier(word) if word == "inline" || word == "__inline" => is_inline = true,
                Token::Identifier(word) => name = Some(word.clone()),
                Token::ParenOpen => break,
                Token::Semicolon | Token::BraceOpen | Token::EndOfFile => return None,
                _ => {}
            }
            index += 1;
        }
        if !is_inline {
            return None;
        }
        let name = name?;
        // Skip the (balanced) parameter list.
        let mut parens = 0;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::ParenOpen => parens += 1,
                Token::ParenClose => {
                    parens -= 1;
                    if parens == 0 {
                        index += 1;
                        break;
                    }
                }
                Token::EndOfFile => return None,
                _ => {}
            }
            index += 1;
        }
        // The body must be a `{...}` block; scan it for an `asm` token.
        if self.tokens.get(index) != Some(&Token::BraceOpen) {
            return None;
        }
        let mut braces = 0;
        let mut has_asm = false;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::BraceOpen => braces += 1,
                Token::BraceClose => {
                    braces -= 1;
                    if braces == 0 {
                        break;
                    }
                }
                Token::Identifier(word) if word == "asm" || word == "__asm" => has_asm = true,
                Token::EndOfFile => break,
                _ => {}
            }
            index += 1;
        }
        has_asm.then_some(name)
    }

    /// Parse one top-level item — a typedef, struct definition, global declaration,
    /// prototype, or function definition — recording it into the unit. Returns `Err`
    /// for any form outside the subset; the caller skips a failed declaration or
    /// propagates a failed function definition.
    fn parse_top_level_item(
        &mut self,
        globals: &mut Vec<GlobalDeclaration>,
        functions: &mut Vec<Function>,
        prototypes: &mut Vec<(String, Type)>,
    ) -> Compilation<()> {
        {
            // `extern`/`static` storage qualifiers: `extern` makes the declaration a
            // reference to a symbol defined elsewhere; `static` makes a definition
            // local. Both are recorded so the object can classify the symbol.
            let mut is_extern = false;
            let mut is_static = false;
            while let Token::Identifier(word) = self.peek() {
                match word.as_str() {
                    "extern" => is_extern = true,
                    "static" => is_static = true,
                    _ => break,
                }
                self.advance();
            }
            if *self.peek() == Token::EndOfFile {
                return Ok(());
            }
            // `typedef <type> <name>;` registers a type alias. (Function-pointer and
            // array typedefs are not in the subset yet.)
            if self.eat_word("typedef") {
                // `typedef struct [Tag] { … } Alias;` registers the layout and the
                // alias->tag mapping (an anonymous struct uses the alias as its tag).
                let tagged = *self.peek() == Token::KeywordStruct
                    && (self.tokens.get(self.position + 1) == Some(&Token::BraceOpen)
                        || self.tokens.get(self.position + 2) == Some(&Token::BraceOpen));
                if tagged {
                    self.advance(); // `struct`
                    let tag = if matches!(self.peek(), Token::Identifier(_)) { self.parse_identifier()? } else { String::new() };
                    let layout = self.parse_struct_body()?;
                    let alias = self.parse_identifier()?;
                    self.expect(Token::Semicolon)?;
                    let tag = if tag.is_empty() { alias.clone() } else { tag };
                    self.structs.insert(tag.clone(), layout);
                    self.struct_typedefs.insert(alias, tag);
                    return Ok(());
                }
                let aliased = self.parse_type()?;
                // Function-pointer typedef `typedef RET (*name)(params);` — the
                // alias is a 4-byte pointer (modeled as a word pointer).
                if *self.peek() == Token::ParenOpen && self.tokens.get(self.position + 1) == Some(&Token::Star) {
                    self.advance(); // `(`
                    self.advance(); // `*`
                    let alias = self.parse_identifier()?;
                    self.expect(Token::ParenClose)?;
                    self.expect(Token::ParenOpen)?;
                    let mut depth = 1;
                    while depth > 0 {
                        match self.advance() {
                            Token::ParenOpen => depth += 1,
                            Token::ParenClose => depth -= 1,
                            Token::EndOfFile => return Err(Diagnostic::error("unterminated function-pointer typedef")),
                            _ => {}
                        }
                    }
                    self.expect(Token::Semicolon)?;
                    self.typedefs.insert(alias, Type::Pointer(Pointee::Int));
                    return Ok(());
                }
                let name = self.parse_identifier()?;
                self.expect(Token::Semicolon)?;
                self.typedefs.insert(name, aliased);
                return Ok(());
            }
            // A `struct Name { ... };` definition registers a layout. A `struct
            // Name*` use (function return or parameter) falls through to parse_type.
            if *self.peek() == Token::KeywordStruct && self.tokens.get(self.position + 2) == Some(&Token::BraceOpen) {
                self.parse_struct_definition()?;
                return Ok(());
            }
            let return_type = self.parse_type()?;
            // A bare type with no declarator (`enum E { … };`, a forward decl) just
            // registers the type; there is nothing else to emit.
            if *self.peek() == Token::Semicolon {
                self.advance();
                return Ok(());
            }
            // Function-pointer declarator: `RET (*name)(params)` — a pointer-typed
            // global (a 4-byte address). The return/parameter types don't affect
            // codegen, so the signature is skipped.
            if *self.peek() == Token::ParenOpen {
                self.advance();
                self.expect(Token::Star)?;
                let pointer_name = self.parse_identifier()?;
                self.expect(Token::ParenClose)?;
                self.expect(Token::ParenOpen)?;
                let mut depth = 1;
                while depth > 0 {
                    match self.advance() {
                        Token::ParenOpen => depth += 1,
                        Token::ParenClose => depth -= 1,
                        Token::EndOfFile => return Err(Diagnostic::error("unterminated function-pointer declarator")),
                        _ => {}
                    }
                }
                self.expect(Token::Semicolon)?;
                globals.push(GlobalDeclaration { declared_type: Type::StructPointer, name: pointer_name, is_extern, is_static, array_length: None, initializer: None });
                return Ok(());
            }
            let name = self.parse_identifier()?;
            // `type name;`, `type name[N];`, or comma-separated declarators is a
            // global variable declaration. A `(` instead begins a function. (An
            // initialized global `type name = …;` is not in the subset yet and
            // falls through to the function path, which reports it.)
            if matches!(self.peek(), Token::Semicolon | Token::Comma | Token::BracketOpen | Token::Equals) {
                // A `const` file-scope global lands in a read-only section (and may
                // be folded into its readers), which isn't modeled — defer it.
                if self.last_type_was_const {
                    return Err(Diagnostic::error("const file-scope global (read-only section) is not supported yet (roadmap)"));
                }
                let mut declarator_name = name;
                loop {
                    // `[N]` (explicit length), `[]` (length inferred from the
                    // initializer), or no brackets (a scalar).
                    let brackets = if *self.peek() == Token::BracketOpen {
                        self.advance();
                        let count = if *self.peek() == Token::BracketClose {
                            None
                        } else {
                            Some(match self.advance() {
                                Token::IntegerLiteral(value) => value as u16,
                                other => return Err(Diagnostic::error(format!("expected an array length, found {other}"))),
                            })
                        };
                        self.expect(Token::BracketClose)?;
                        Some(count)
                    } else {
                        None
                    };
                    // `= <constant>` or `= { <constant>, ... }`.
                    let initializer = if self.eat_keyword(Token::Equals) {
                        Some(self.parse_constant_initializer()?)
                    } else {
                        None
                    };
                    let array_length = match brackets {
                        None => None,
                        Some(Some(count)) => Some(count),
                        Some(None) => match &initializer {
                            Some(values) => Some(values.len() as u16),
                            None => return Err(Diagnostic::error("an array with no length needs an initializer")),
                        },
                    };
                    globals.push(GlobalDeclaration { declared_type: return_type, name: declarator_name, is_extern, is_static, array_length, initializer });
                    if *self.peek() == Token::Comma {
                        self.advance();
                        declarator_name = self.parse_identifier()?;
                    } else {
                        break;
                    }
                }
                self.expect(Token::Semicolon)?;
                return Ok(());
            }
            self.expect(Token::ParenOpen)?;

            let mut parameters = Vec::new();
            // `(void)` is an empty parameter list — but only when the `void` is the
            // whole list; `void *p` / `void (*f)()` are real first parameters.
            if *self.peek() == Token::KeywordVoid && self.tokens.get(self.position + 1) == Some(&Token::ParenClose) {
                self.advance();
            } else if *self.peek() != Token::ParenClose {
                loop {
                    // A `...` varargs marker ends the parameter list. (A function
                    // that actually reads its varargs defers later in codegen.)
                    if *self.peek() == Token::Dot {
                        self.advance();
                        self.expect(Token::Dot)?;
                        self.expect(Token::Dot)?;
                        break;
                    }
                    let parameter_type = self.parse_type()?;
                    let struct_tag = self.last_struct_tag.take();
                    // A function-pointer parameter `RET (*name)(params)` is a 4-byte
                    // opaque pointer; consume its declarator and signature.
                    if *self.peek() == Token::ParenOpen && self.tokens.get(self.position + 1) == Some(&Token::Star) {
                        self.advance(); // `(`
                        self.advance(); // `*`
                        let name = if matches!(self.peek(), Token::Identifier(_)) { self.parse_identifier()? } else { String::new() };
                        self.expect(Token::ParenClose)?;
                        self.expect(Token::ParenOpen)?;
                        let mut depth = 1;
                        while depth > 0 {
                            match self.advance() {
                                Token::ParenOpen => depth += 1,
                                Token::ParenClose => depth -= 1,
                                Token::EndOfFile => return Err(Diagnostic::error("unterminated function-pointer parameter")),
                                _ => {}
                            }
                        }
                        parameters.push(Parameter { parameter_type: Type::StructPointer, name });
                    } else {
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
                    }
                    if *self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect(Token::ParenClose)?;

            if *self.peek() == Token::Semicolon {
                self.advance(); // a prototype — record its return type, keep looking
                prototypes.push((name, return_type));
                return Ok(());
            }
            functions.push(self.function_body(return_type, name, parameters)?);
        }
        Ok(())
    }

    /// Whether the item starting at the cursor is a function *definition* (a
    /// `(params) {` body) rather than a declaration. Used after a parse failure to
    /// decide whether the item can be skipped (a declaration) or must be propagated
    /// (a function we are expected to compile). Pure lookahead — consumes nothing.
    fn item_is_function_definition(&self) -> bool {
        let mut index = self.position;
        let mut paren_depth = 0i32;
        let mut saw_parameter_list = false;
        while let Some(token) = self.tokens.get(index) {
            match token {
                // A typedef is never a function definition. An `inline` definition
                // is an SDK header helper mwcc only emits when used — skip it rather
                // than compile it as a standalone symbol.
                Token::Identifier(word) if word == "typedef" || word == "inline" || word == "__inline" => return false,
                Token::ParenOpen => paren_depth += 1,
                Token::ParenClose => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        saw_parameter_list = true;
                    }
                }
                // The first top-level `;` ends a declaration.
                Token::Semicolon if paren_depth == 0 => return false,
                // A top-level `{` is a function body iff a `(params)` group preceded
                // it (otherwise it opens a struct/enum/union or an initializer).
                Token::BraceOpen if paren_depth == 0 => return saw_parameter_list,
                Token::EndOfFile => return false,
                _ => {}
            }
            index += 1;
        }
        false
    }

    /// Advance past an unparseable top-level declaration to its end: the `;` at
    /// brace depth zero, or the matching `}` of a struct/enum/union/initializer
    /// followed by an optional `;`.
    fn skip_top_level_declaration(&mut self) {
        let mut brace_depth = 0i32;
        loop {
            match self.advance() {
                Token::BraceOpen => brace_depth += 1,
                Token::BraceClose => {
                    brace_depth -= 1;
                    if brace_depth <= 0 {
                        if *self.peek() == Token::Semicolon {
                            self.advance();
                        }
                        return;
                    }
                }
                Token::Semicolon if brace_depth == 0 => return,
                Token::EndOfFile => return,
                _ => {}
            }
        }
    }

    /// Parse a function definition's body, given its already-parsed signature.
    /// `{` then zero or more local declarations, statements, `if (...) return ...;`
    /// guards, and an optional final `return <expression>;`.
    fn function_body(&mut self, return_type: Type, name: String, parameters: Vec<Parameter>) -> Compilation<Function> {
        self.expect(Token::BraceOpen)?;

        // Zero or more local declarations precede the return statement. A
        // statement that begins with a type keyword is a local declaration;
        // `return` ends the body.
        let mut locals = Vec::new();
        while self.peek_is_type() {
            let declared_type = self.parse_type()?;
            let struct_tag = self.last_struct_tag.take();
            // One or more comma-separated declarators, each optionally initialized.
            loop {
                let name = self.parse_identifier()?;
                if let Some(tag) = &struct_tag {
                    self.variable_structs.insert(name.clone(), tag.clone());
                }
                let initializer = if self.eat_keyword(Token::Equals) { Some(self.expression()?) } else { None };
                locals.push(LocalDeclaration { declared_type, name, initializer });
                if *self.peek() == Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(Token::Semicolon)?;
        }

        // Zero or more statements: a store `*p = v;` / `p[i] = v;`, or a bare
        // expression evaluated for effect like a call `g();`.
        let local_names: std::collections::HashSet<&str> = locals.iter().map(|local| local.name.as_str()).collect();
        let mut statements = Vec::new();
        while !matches!(self.peek(), Token::KeywordReturn | Token::BraceClose) {
            // `if (c) { ... }` is a conditional block statement; a trailing
            // `if (c) return ...` is a guard, handled after the statement list.
            if *self.peek() == Token::KeywordIf {
                if self.block_if_ahead() {
                    let statement = self.parse_if_statement(&local_names)?;
                    statements.push(statement);
                    continue;
                }
                break;
            }
            let statement = self.parse_simple_statement(&local_names)?;
            statements.push(statement);
        }

        // Zero or more guarded early returns: `if (condition) return value;`. An
        // `if (c) return x; else return y;` terminates the function as a single
        // conditional return (the ternary `c ? x : y`).
        let mut guards = Vec::new();
        let mut conditional_return = None;
        while *self.peek() == Token::KeywordIf {
            self.advance();
            self.expect(Token::ParenOpen)?;
            let condition = self.expression()?;
            self.expect(Token::ParenClose)?;
            let value = self.parse_guard_return()?;
            if self.eat_word("else") {
                let otherwise = self.parse_guard_return()?;
                conditional_return = Some(Expression::Conditional {
                    condition: Box::new(condition),
                    when_true: Box::new(value),
                    when_false: Box::new(otherwise),
                });
                break;
            }
            guards.push(GuardedReturn { condition, value });
        }

        // The final `return <expr>;` is optional — a `void` function may end after
        // its statements (or an `if/else` already supplied the return).
        let return_expression = if conditional_return.is_some() {
            conditional_return
        } else if *self.peek() == Token::KeywordReturn {
            self.advance();
            let value = self.expression()?;
            self.expect(Token::Semicolon)?;
            Some(value)
        } else {
            None
        };
        self.expect(Token::BraceClose)?;

        Ok(Function { return_type, name, parameters, locals, statements, guards, return_expression })
    }

    pub(crate) fn peek_is_type(&self) -> bool {
        match self.peek() {
            Token::KeywordInt
            | Token::KeywordChar
            | Token::KeywordShort
            | Token::KeywordUnsigned
            | Token::KeywordFloat
            | Token::KeywordVoid
            | Token::KeywordStruct => true,
            // The `long`/`signed`/`double` specifier words, the `const`/`volatile`/
            // `register` qualifiers, and any typedef name.
            Token::Identifier(word) => {
                matches!(word.as_str(), "long" | "signed" | "double" | "const" | "volatile" | "register" | "enum")
                    || self.typedefs.contains_key(word)
                    || self.struct_typedefs.contains_key(word)
            }
            _ => false,
        }
    }

    /// Consume a run of leading qualifier / storage-class words. `const` (noted in
    /// `last_type_was_const`) and `register` are ignored; `volatile` is deferred
    /// (its access semantics aren't modeled yet).
    pub(crate) fn skip_type_qualifiers(&mut self) -> Compilation<()> {
        self.last_type_was_const = false;
        loop {
            match self.peek() {
                Token::Identifier(word) if word == "const" => {
                    self.last_type_was_const = true;
                    self.advance();
                }
                Token::Identifier(word) if word == "register" => {
                    self.advance();
                }
                Token::Identifier(word) if word == "volatile" => {
                    return Err(Diagnostic::error("volatile is not supported yet (roadmap)"));
                }
                _ => return Ok(()),
            }
        }
    }
}

impl Parser {
    /// Parse one simple (non-control-flow) statement: a `switch`, an increment,
    /// an assignment / compound assignment / memory store, or a bare expression.
    fn parse_simple_statement(&mut self, local_names: &std::collections::HashSet<&str>) -> Compilation<Statement> {
        if matches!(self.peek(), Token::Identifier(word) if word == "switch") {
            return self.parse_switch();
        }
        // Prefix `++target;` / `--target;` — a value-free statement.
        if let Some(operator) = self.peek_increment() {
            self.advance();
            self.advance();
            let target = self.factor()?;
            self.expect(Token::Semicolon)?;
            let value = increment_value(operator, &target);
            return Ok(store_or_assign(target, value, local_names));
        }
        let first = self.factor()?;
        if let Some(operator) = self.peek_increment() {
            // Postfix `target++;` / `target--;`.
            self.advance();
            self.advance();
            self.expect(Token::Semicolon)?;
            let value = increment_value(operator, &first);
            Ok(store_or_assign(first, value, local_names))
        } else if let Some(operator) = self.peek_compound_assignment() {
            self.advance();
            self.advance();
            let rhs = self.expression()?;
            self.expect(Token::Semicolon)?;
            let value = Expression::Binary { operator, left: Box::new(first.clone()), right: Box::new(rhs) };
            Ok(store_or_assign(first, value, local_names))
        } else if *self.peek() == Token::Equals {
            self.advance();
            let value = self.expression()?;
            self.expect(Token::Semicolon)?;
            Ok(store_or_assign(first, value, local_names))
        } else {
            self.expect(Token::Semicolon)?;
            Ok(Statement::Expression(first))
        }
    }

    /// At a `KeywordIf`, whether it is a conditional block/statement (body is a
    /// `{ ... }` block or a non-`return` statement) rather than a guard
    /// (`if (c) return …`). Scans the balanced condition parentheses.
    fn block_if_ahead(&self) -> bool {
        if *self.peek_at(1) != Token::ParenOpen {
            return false;
        }
        let mut depth = 0i32;
        let mut index = 1;
        loop {
            match self.peek_at(index) {
                Token::ParenOpen => depth += 1,
                Token::ParenClose => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            index += 1;
            if index > 4096 {
                return false;
            }
        }
        // A `return` body — bare `return …` or a braced single-return block
        // `{ return …` — is a guard; anything else is an if-statement.
        let after = self.peek_at(index + 1);
        if *after == Token::KeywordReturn {
            return false;
        }
        if *after == Token::BraceOpen && *self.peek_at(index + 2) == Token::KeywordReturn {
            return false;
        }
        true
    }

    /// Parse a guard's return body: `return <expr>;`, optionally wrapped in a
    /// single-statement block `{ return <expr>; }`. The braces are syntactic — the
    /// guard codegen is identical either way.
    fn parse_guard_return(&mut self) -> Compilation<Expression> {
        let braced = self.eat_keyword(Token::BraceOpen);
        self.expect(Token::KeywordReturn)?;
        let value = self.expression()?;
        self.expect(Token::Semicolon)?;
        if braced {
            self.expect(Token::BraceClose)?;
        }
        Ok(value)
    }

    /// `if (condition) <block-or-statement> [else <block-or-statement> | else if]`.
    fn parse_if_statement(&mut self, local_names: &std::collections::HashSet<&str>) -> Compilation<Statement> {
        self.expect(Token::KeywordIf)?;
        self.expect(Token::ParenOpen)?;
        let condition = self.expression()?;
        self.expect(Token::ParenClose)?;
        let then_body = self.parse_block_or_statement(local_names)?;
        let else_body = if self.eat_word("else") {
            if *self.peek() == Token::KeywordIf {
                vec![self.parse_if_statement(local_names)?]
            } else {
                self.parse_block_or_statement(local_names)?
            }
        } else {
            Vec::new()
        };
        Ok(Statement::If { condition, then_body, else_body })
    }

    /// A `{ ... }` block, or a single (non-`return`) statement, as a conditional
    /// branch body.
    fn parse_block_or_statement(&mut self, local_names: &std::collections::HashSet<&str>) -> Compilation<Vec<Statement>> {
        if *self.peek() == Token::BraceOpen {
            return self.parse_block(local_names);
        }
        if *self.peek() == Token::KeywordIf {
            return Ok(vec![self.parse_if_statement(local_names)?]);
        }
        Ok(vec![self.parse_simple_statement(local_names)?])
    }

    /// A `{ ... }` block of simple statements (and nested if-blocks). A `return`
    /// inside a block is not modeled as a statement yet.
    fn parse_block(&mut self, local_names: &std::collections::HashSet<&str>) -> Compilation<Vec<Statement>> {
        self.expect(Token::BraceOpen)?;
        let mut statements = Vec::new();
        while *self.peek() != Token::BraceClose {
            if *self.peek() == Token::KeywordIf {
                if self.block_if_ahead() {
                    statements.push(self.parse_if_statement(local_names)?);
                    continue;
                }
                return Err(Diagnostic::error("an `if (c) return` inside a block is not supported yet (roadmap)"));
            }
            if *self.peek() == Token::KeywordReturn {
                return Err(Diagnostic::error("a `return` inside a block is not supported yet (roadmap)"));
            }
            statements.push(self.parse_simple_statement(local_names)?);
        }
        self.expect(Token::BraceClose)?;
        Ok(statements)
    }
}
