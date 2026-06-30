//! Parsing of types, functions, parameters, locals, and guarded returns.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Expression, Function, GlobalDeclaration, GuardedReturn, LocalDeclaration, LoopKind, Parameter, Pointee, PointerElement, Statement, SwitchArm, TranslationUnit, Type};
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
        Type::Pointer(_) | Type::StructPointer { .. } => 4,
        Type::Struct { size, .. } => size,
        other => (other.width() / 8) as u16,
    }
}

/// A type's alignment for laying out a struct member: a struct value aligns to its
/// own alignment (not its size), every other type to its size.
fn type_alignment(declared: Type) -> u16 {
    match declared {
        Type::Struct { align, .. } => align as u16,
        other => type_size(other),
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

    /// A constant integer in statement position — a `switch` case label. Parsed as a
    /// full constant expression so an enum constant (`case GX_MODULATE:`) or a folded
    /// expression (`case A | B:`) resolves, not just a bare integer literal.
    fn parse_integer_constant(&mut self) -> Compilation<i64> {
        let expression = self.expression()?;
        crate::expressions::fold_constant_expression(&expression)
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
    /// A pointer global's initializer: a single address (`int *p = &g;`) or a brace
    /// list of them (`int *t[] = {&a, &b};`), each element a target symbol, string,
    /// or null.
    fn parse_address_initializer(&mut self) -> Compilation<Vec<PointerElement>> {
        if self.eat_keyword(Token::BraceOpen) {
            let mut elements = Vec::new();
            while *self.peek() != Token::BraceClose {
                elements.push(self.parse_pointer_init_element()?);
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
            self.expect(Token::BraceClose)?;
            Ok(elements)
        } else {
            Ok(vec![self.parse_pointer_init_element()?])
        }
    }

    /// A struct `tag`'s fields in layout (offset) order, but only when every field is
    /// exactly 4 bytes (a pointer, int, or float) and at least one is a pointer — the
    /// shape of a `{ "name", id }` string-table entry. Returns `None` otherwise, so
    /// the caller falls through to the scalar/defer paths (sub-word or 8-byte fields
    /// break the flat 4-byte-slot model).
    fn struct_pointer_table_fields(&self, tag: &str) -> Option<Vec<Type>> {
        let layout = self.structs.get(tag)?;
        let mut fields: Vec<&crate::parser::StructField> = layout.fields.values().collect();
        fields.sort_by_key(|field| field.offset);
        let types: Vec<Type> = fields.iter().map(|field| field.member_type).collect();
        let all_word = types.iter().all(|field_type| field_type.width() == 32);
        let any_pointer = types.iter().any(|field_type| matches!(field_type, Type::Pointer(_) | Type::StructPointer { .. }));
        (all_word && any_pointer).then_some(types)
    }

    /// Parse `{ { field0, field1, … }, … }` for an array of word-field structs: each
    /// element's fields are flattened, in order, to a `PointerElement` sequence — a
    /// pointer field to `Str`/`Symbol`/`Null`, a scalar field to `Scalar`. The shape
    /// of a `{ "path", id }` overlay/string table.
    fn parse_struct_pointer_table(&mut self, field_types: &[Type]) -> Compilation<Vec<PointerElement>> {
        self.expect(Token::BraceOpen)?;
        let mut elements = Vec::new();
        while *self.peek() != Token::BraceClose {
            self.expect(Token::BraceOpen)?;
            for (index, field_type) in field_types.iter().enumerate() {
                if matches!(field_type, Type::Pointer(_) | Type::StructPointer { .. }) {
                    let element = self.parse_pointer_init_element()?;
                    // A string element pools an anonymous `@N` object, whose NUMBER in a
                    // real translation unit is offset by phantom `@N` mwcc consumes while
                    // processing preceding (header inline) functions — not modeled yet, so
                    // defer string tables. `&symbol`/`&global`/`0`/scalar tables have no
                    // `@N` and stay byte-exact.
                    if matches!(element, PointerElement::Str(_)) {
                        return Err(Diagnostic::error("a struct-table with string literals needs the anonymous @N base (roadmap)"));
                    }
                    elements.push(element);
                } else {
                    elements.push(PointerElement::Scalar(self.parse_integer_constant()?));
                }
                if index + 1 < field_types.len() {
                    self.expect(Token::Comma)?;
                }
            }
            self.eat_keyword(Token::Comma); // optional trailing comma inside the element
            self.expect(Token::BraceClose)?;
            if !self.eat_keyword(Token::Comma) {
                break;
            }
        }
        self.expect(Token::BraceClose)?;
        Ok(elements)
    }

    /// One element of a pointer global's address initializer: a string literal
    /// (pooled), `&name` or a bare `name` (a function pointer) is that symbol; `0` is
    /// a null pointer. `&a[i]`, `&s.f`, casts, and arithmetic defer (they need an
    /// addend not yet modeled).
    fn parse_pointer_init_element(&mut self) -> Compilation<PointerElement> {
        // A cast is transparent for an address: `(SomeType *)&x` is just `&x`. Skip
        // the parenthesised type and parse the operand after it.
        if *self.peek() == Token::ParenOpen && self.token_starts_type(self.peek_at(1)) {
            self.advance();
            let mut depth = 1;
            while depth > 0 {
                match self.advance() {
                    Token::ParenOpen => depth += 1,
                    Token::ParenClose => depth -= 1,
                    Token::EndOfFile => return Err(Diagnostic::error("unterminated cast in a pointer initializer")),
                    _ => {}
                }
            }
            return self.parse_pointer_init_element();
        }
        // A grouping paren that is not a cast: `((void *)0)` (the common `NULL` macro
        // expansion) or `(&x)`. Parse the inner element and consume the closing paren.
        if *self.peek() == Token::ParenOpen {
            self.advance();
            let element = self.parse_pointer_init_element()?;
            self.expect(Token::ParenClose)?;
            return Ok(element);
        }
        if let Token::StringLiteral(bytes) = self.peek() {
            let bytes = bytes.clone();
            self.advance();
            return Ok(PointerElement::Str(bytes));
        }
        if *self.peek() == Token::Ampersand {
            self.advance();
            let name = self.parse_identifier()?;
            if matches!(self.peek(), Token::BracketOpen | Token::Dot | Token::Arrow) {
                return Err(Diagnostic::error("a pointer initializer with an offset is not supported yet (roadmap)"));
            }
            return Ok(PointerElement::Symbol(name));
        }
        if matches!(self.peek(), Token::IntegerLiteral(0)) {
            self.advance();
            return Ok(PointerElement::Null);
        }
        if let Token::Identifier(name) = self.peek() {
            let name = name.clone();
            self.advance();
            return Ok(PointerElement::Symbol(name));
        }
        Err(Diagnostic::error("a pointer global initializer must be a string, &symbol, a symbol, or 0 (roadmap)"))
    }

    fn parse_constant_initializer(&mut self, element_type: Type) -> Compilation<Vec<i64>> {
        // A string literal initializes a `char` array with its bytes plus a NUL
        // terminator (the store truncates if the array is shorter). A string for a
        // non-char type is a char-pointer initializer that needs a data relocation —
        // deferred. (A bare string, no braces; an array of strings would brace it.)
        if let Token::StringLiteral(bytes) = self.peek() {
            if !matches!(element_type, Type::Char | Type::UnsignedChar) {
                return Err(Diagnostic::error("a string initializer is only supported for a char array yet (roadmap)"));
            }
            let mut values: Vec<i64> = bytes.iter().map(|&byte| byte as i64).collect();
            self.advance();
            values.push(0);
            return Ok(values);
        }
        if self.eat_keyword(Token::BraceOpen) {
            let mut values = Vec::new();
            while *self.peek() != Token::BraceClose {
                // A nested brace (`{{1,2},{3,4}}` for a multi-dimensional array)
                // flattens in row-major order into the same flat element list.
                if *self.peek() == Token::BraceOpen {
                    values.extend(self.parse_constant_initializer(element_type)?);
                } else {
                    values.push(self.parse_scalar_constant(element_type)?);
                }
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
            self.expect(Token::BraceClose)?;
            Ok(values)
        } else {
            Ok(vec![self.parse_scalar_constant(element_type)?])
        }
    }

    /// One scalar element of a global initializer, encoded to the raw bits the
    /// object stores for `element_type`. An integer element is a full constant
    /// *expression* (`((dir)+(file))`, `1 << 3`, `(u8)0xFF`), folded to its value;
    /// the store later truncates to the element width. A `float`/`double` element
    /// is a single literal (or integer, converted) as its IEEE-754 bit pattern.
    fn parse_scalar_constant(&mut self, element_type: Type) -> Compilation<i64> {
        match element_type {
            Type::Float | Type::Double => {
                let negative = self.eat_keyword(Token::Minus);
                match self.advance() {
                    Token::IntegerLiteral(value) => {
                        let value = if negative { -(value as i64) } else { value as i64 };
                        Ok(if element_type == Type::Float { (value as f32).to_bits() as i64 } else { (value as f64).to_bits() as i64 })
                    }
                    Token::FloatLiteral(value) => {
                        let value = if negative { -value } else { value };
                        Ok(if element_type == Type::Float { (value as f32).to_bits() as i64 } else { value.to_bits() as i64 })
                    }
                    other => Err(Diagnostic::error(format!("a float global initializer must be a literal, found {other}"))),
                }
            }
            _ => {
                let expression = self.expression()?;
                crate::expressions::fold_constant_expression(&expression)
            }
        }
    }

    /// Parse one struct value `{ f0, f1, ... }` for the layout `tag`, folding each
    /// field with its own type (the flat `parse_constant_initializer` cannot, since a
    /// struct mixes field types — notably `float`). Fields are taken in offset order;
    /// a nested struct field recurses. Every field must be word-width (4 bytes) so the
    /// flat value list lays out contiguously with no padding — a sub-word/`double`
    /// field, or an array field, defers (those need a width-aware data emitter).
    fn parse_one_struct(&mut self, tag: &str) -> Compilation<Vec<u8>> {
        let (struct_size, fields) = {
            let layout = self.structs.get(tag).ok_or_else(|| Diagnostic::error(format!("struct '{tag}' is not declared")))?;
            let mut ordered: Vec<(u16, Type, Option<String>, Option<Pointee>)> =
                layout.fields.values().map(|field| (field.offset, field.member_type, field.struct_tag.clone(), field.array_element)).collect();
            ordered.sort_by_key(|(offset, ..)| *offset);
            (layout.size, ordered)
        };
        for (_, _, _, array_element) in &fields {
            if array_element.is_some() {
                return Err(Diagnostic::error("a struct initializer with an array field is not supported yet (roadmap)"));
            }
        }
        self.expect(Token::BraceOpen)?;
        // Each field is written into the struct's byte image at its own offset and
        // width (big-endian); gaps and trailing padding stay zero. A nested struct
        // field copies its own image in.
        let mut bytes = vec![0u8; struct_size as usize];
        for (index, (offset, member_type, struct_tag, _)) in fields.iter().enumerate() {
            let offset = *offset as usize;
            if let Some(nested) = struct_tag {
                let nested_bytes = self.parse_one_struct(nested)?;
                bytes[offset..offset + nested_bytes.len()].copy_from_slice(&nested_bytes);
            } else {
                let value = self.parse_scalar_constant(*member_type)?;
                let width = type_size(*member_type) as usize;
                let encoded = (value as u64).to_be_bytes();
                bytes[offset..offset + width].copy_from_slice(&encoded[8 - width..]);
            }
            if index + 1 < fields.len() && !self.eat_keyword(Token::Comma) {
                break;
            }
        }
        self.eat_keyword(Token::Comma);
        self.expect(Token::BraceClose)?;
        Ok(bytes)
    }

    /// Parse a `{ s0, s1, ... }` array of struct values for the layout `tag`, each
    /// element parsed by [`Self::parse_one_struct`] and concatenated (the array stride
    /// is the struct size, which each element's image already fills).
    fn parse_struct_array_initializer(&mut self, tag: &str) -> Compilation<Vec<u8>> {
        self.expect(Token::BraceOpen)?;
        let mut bytes = Vec::new();
        while *self.peek() != Token::BraceClose {
            bytes.extend(self.parse_one_struct(tag)?);
            if !self.eat_keyword(Token::Comma) {
                break;
            }
        }
        self.expect(Token::BraceClose)?;
        Ok(bytes)
    }

    /// The struct-value [`Type`] for a known struct layout (size + alignment), or
    /// `None` for an opaque/undeclared struct (whose value cannot be laid out).
    /// Drives `struct S v;` value support.
    fn struct_value_type(&self, tag: &str) -> Option<Type> {
        self.structs
            .get(tag)
            .filter(|layout| layout.size > 0)
            .map(|layout| Type::Struct { size: layout.size, align: layout.align.max(1) })
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
                // A struct *value*: a known layout becomes a sized struct value
                // (a frame-resident local); an opaque/unknown struct still defers.
                return match self.struct_value_type(&tag) {
                    Some(struct_type) => {
                        self.last_struct_tag = Some(tag);
                        Ok(struct_type)
                    }
                    None => Err(Diagnostic::error("struct values are not supported yet — use a struct pointer")),
                };
            }
            self.advance();
            let element_size = self.structs.get(&tag).map_or(0, |layout| layout.size);
            self.last_struct_tag = Some(tag);
            return Ok(Type::StructPointer { element_size });
        }
        // `union Name*` / `union Name` — a union is laid out like a struct with every member
        // at offset 0 (overlapping storage), so it reuses the struct machinery once the layout
        // is registered. `union` is lexed as a plain identifier, not a keyword.
        if matches!(self.peek(), Token::Identifier(word) if word == "union") {
            self.advance();
            let tag = self.parse_identifier()?;
            if *self.peek() != Token::Star {
                return match self.struct_value_type(&tag) {
                    Some(union_type) => {
                        self.last_struct_tag = Some(tag);
                        Ok(union_type)
                    }
                    None => Err(Diagnostic::error("union values are not supported yet — use a union pointer")),
                };
            }
            self.advance();
            let element_size = self.structs.get(&tag).map_or(0, |layout| layout.size);
            self.last_struct_tag = Some(tag);
            return Ok(Type::StructPointer { element_size });
        }
        // A struct-pointer typedef (`VecPtr`) is itself a pointer to the struct —
        // no trailing `*` — carrying the layout's tag.
        if let Token::Identifier(name) = self.peek() {
            if let Some(tag) = self.struct_pointer_typedefs.get(name).cloned() {
                self.advance();
                let element_size = self.structs.get(&tag).map_or(0, |layout| layout.size);
                self.last_struct_tag = Some(tag);
                return Ok(Type::StructPointer { element_size });
            }
        }
        // A struct typedef (`FILE`) behaves like its `struct Tag`: `FILE *` is a
        // struct pointer carrying the layout's tag; a struct value isn't supported.
        if let Token::Identifier(name) = self.peek() {
            if let Some(tag) = self.struct_typedefs.get(name).cloned() {
                self.advance();
                if *self.peek() != Token::Star {
                    return match self.struct_value_type(&tag) {
                    Some(struct_type) => {
                        self.last_struct_tag = Some(tag);
                        Ok(struct_type)
                    }
                        None => Err(Diagnostic::error("struct values are not supported yet — use a struct pointer")),
                    };
                }
                self.advance();
                let element_size = self.structs.get(&tag).map_or(0, |layout| layout.size);
                self.last_struct_tag = Some(tag);
                return Ok(Type::StructPointer { element_size });
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
                // `unsigned long` / `unsigned long int` — 32-bit unsigned. `unsigned long
                // long` is the 64-bit register-pair type.
                Token::Identifier(word) if word == "long" => {
                    let mut long_count = 0;
                    while self.eat_word("long") {
                        long_count += 1;
                    }
                    let _ = self.eat_keyword(Token::KeywordInt);
                    if long_count >= 2 { Type::UnsignedLongLong } else { Type::UnsignedInt }
                }
                _ => Type::UnsignedInt,
            },
            Token::KeywordFloat => Type::Float,
            Token::KeywordVoid => Type::Void,
            // `double` (and `long double`, which is also 64-bit here).
            Token::Identifier(word) if word == "double" => Type::Double,
            // `long` / `long int` — 32-bit signed on this target; `long double` is a
            // double. `long long` is the 64-bit register-pair type.
            Token::Identifier(word) if word == "long" => {
                // The outer `match self.advance()` already consumed the first `long`, so
                // seed the count at 1; the loop adds any further `long`s.
                let mut long_count = 1;
                while self.eat_word("long") {
                    long_count += 1;
                }
                if self.eat_word("double") {
                    Type::Double
                } else if long_count >= 2 {
                    let _ = self.eat_keyword(Token::KeywordInt);
                    Type::LongLong
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
    /// Parse a struct body `{ field; … }` (the cursor is at the `{`), returning its
    /// layout. Does not consume any trailing `;` — the caller (a definition or a
    /// typedef) does.
    /// Skip any `__attribute__((...))` specifiers at the cursor — GCC/CodeWarrior
    /// attributes that survive preprocessing (e.g. `ATTRIBUTE_ALIGN(n)` expands to
    /// `__attribute__((aligned(n)))`). Returns the largest `aligned(n)` requested,
    /// if any, so the declarator that follows is laid out with that alignment.
    pub(crate) fn skip_attributes(&mut self) -> Compilation<Option<u16>> {
        let mut align: Option<u16> = None;
        while matches!(self.peek(), Token::Identifier(name) if name == "__attribute__") {
            self.advance();
            self.expect(Token::ParenOpen)?;
            self.expect(Token::ParenOpen)?;
            let mut depth = 2;
            while depth > 0 {
                match self.advance() {
                    Token::ParenOpen => depth += 1,
                    Token::ParenClose => depth -= 1,
                    Token::Identifier(name) if name == "aligned" => {
                        if *self.peek() == Token::ParenOpen {
                            self.advance();
                            depth += 1;
                            let requested = self.parse_integer_constant()? as u16;
                            align = Some(align.unwrap_or(1).max(requested));
                        }
                    }
                    Token::EndOfFile => return Err(Diagnostic::error("unterminated __attribute__")),
                    _ => {}
                }
            }
        }
        Ok(align)
    }

    pub(crate) fn parse_struct_body(&mut self) -> Compilation<StructLayout> {
        self.expect(Token::BraceOpen)?;
        let mut layout = StructLayout::default();
        let mut offset: u16 = 0;
        let mut alignment_max: u16 = 1;
        // The open bit-field allocation unit (its type, byte offset, bits used so
        // far); an ordinary member or a different-typed bit-field closes it.
        let mut bit_unit: Option<(Type, u16, u8)> = None;
        while *self.peek() != Token::BraceClose {
            // An inline struct definition as a member: `struct [Tag] { … } [name];`. An
            // ANONYMOUS one with no member name promotes (flattens) its fields into this
            // struct — C anonymous-struct semantics, and how the game-state structs wrap
            // their bit-fields. A named-tag form registers the tag (and adds a nested
            // struct-value member if a name follows).
            if *self.peek() == Token::KeywordStruct
                && (self.tokens.get(self.position + 1) == Some(&Token::BraceOpen)
                    || self.tokens.get(self.position + 2) == Some(&Token::BraceOpen))
            {
                self.advance(); // `struct`
                let tag = if matches!(self.peek(), Token::Identifier(_)) { Some(self.parse_identifier()?) } else { None };
                let inner = self.parse_struct_body()?;
                let inner_size = inner.size;
                let inner_align = (inner.align as u16).max(1);
                let member_name = if matches!(self.peek(), Token::Identifier(_)) { Some(self.parse_identifier()?) } else { None };
                match (tag, member_name) {
                    (Some(tag), Some(name)) => {
                        self.structs.insert(tag.clone(), inner);
                        alignment_max = alignment_max.max(inner_align);
                        offset = offset.div_ceil(inner_align) * inner_align;
                        layout.fields.insert(name, StructField { member_type: Type::Struct { size: inner_size, align: inner_align as u8 }, offset, struct_tag: Some(tag), array_element: None, bit_field: None });
                        offset += inner_size;
                    }
                    (Some(tag), None) => {
                        // A named struct type registered inside this one (no member).
                        self.structs.insert(tag, inner);
                    }
                    (None, Some(name)) => {
                        // An anonymous inline struct *named* as a member (`struct {…}
                        // mesh;`). Register its layout under a synthetic tag (unique —
                        // generated after the inner parse, so nested anon structs don't
                        // collide) so `parent.mesh.field` chains, then add it as an
                        // ordinary struct-value member.
                        let synthetic = format!("@anon{}", self.structs.len());
                        self.structs.insert(synthetic.clone(), inner);
                        alignment_max = alignment_max.max(inner_align);
                        offset = offset.div_ceil(inner_align) * inner_align;
                        layout.fields.insert(name, StructField { member_type: Type::Struct { size: inner_size, align: inner_align as u8 }, offset, struct_tag: Some(synthetic), array_element: None, bit_field: None });
                        offset += inner_size;
                    }
                    (None, None) => {
                        alignment_max = alignment_max.max(inner_align);
                        offset = offset.div_ceil(inner_align) * inner_align;
                        for (field_name, field) in &inner.fields {
                            layout.fields.insert(field_name.clone(), StructField {
                                member_type: field.member_type,
                                offset: offset + field.offset,
                                struct_tag: field.struct_tag.clone(),
                                array_element: field.array_element,
                                bit_field: field.bit_field,
                            });
                        }
                        offset += inner_size;
                    }
                }
                self.expect(Token::Semicolon)?;
                bit_unit = None;
                continue;
            }
            // An inline union member `union [Tag] { … } [name];`. An ANONYMOUS one
            // with no member name flattens its members into this struct — every
            // union member shares the union's offset (overlapping storage), and the
            // union occupies its largest member. This is how the game's model
            // structs overlay variant payloads (e.g. HsfObject's data/camera/light).
            if matches!(self.peek(), Token::Identifier(word) if word == "union")
                && (self.tokens.get(self.position + 1) == Some(&Token::BraceOpen)
                    || self.tokens.get(self.position + 2) == Some(&Token::BraceOpen))
            {
                self.advance(); // `union`
                let tag = if matches!(self.peek(), Token::Identifier(_)) { Some(self.parse_identifier()?) } else { None };
                let inner = self.parse_union_body()?;
                let inner_size = inner.size;
                let inner_align = (inner.align as u16).max(1);
                let member_name = if matches!(self.peek(), Token::Identifier(_)) { Some(self.parse_identifier()?) } else { None };
                self.expect(Token::Semicolon)?;
                bit_unit = None;
                match (tag, member_name) {
                    // A named union *value* member (`union {…} u;`) needs union-value
                    // access — defer rather than mis-place it.
                    (_, Some(_)) => return Err(Diagnostic::error("a named union member is not supported yet (roadmap)")),
                    // `union Tag { … };` — register the tag, no member contributed.
                    (Some(tag), None) => { self.structs.insert(tag, inner); }
                    // `union { … };` — flatten every member at the union's offset.
                    (None, None) => {
                        alignment_max = alignment_max.max(inner_align);
                        offset = offset.div_ceil(inner_align) * inner_align;
                        for (field_name, field) in &inner.fields {
                            layout.fields.insert(field_name.clone(), StructField {
                                member_type: field.member_type,
                                offset: offset + field.offset,
                                struct_tag: field.struct_tag.clone(),
                                array_element: field.array_element,
                                bit_field: field.bit_field,
                            });
                        }
                        offset += inner_size;
                    }
                }
                continue;
            }
            // An array-typedef member (`Mtx unk_F0;` where `Mtx` is `typedef float
            // Mtx[3][4]`) — the typedef gives the element type and base length; a
            // trailing `[N]` multiplies it. Recorded as a flat element array so the
            // member (and those after it) lay out correctly. A 2D element access still
            // defers in codegen — the flat array stops the second `[]` resolving.
            let array_typedef = match self.peek() {
                Token::Identifier(word) => self.array_typedefs.get(word).copied(),
                _ => None,
            };
            if let Some((element, base_len)) = array_typedef {
                self.advance(); // the array-typedef name
                let attr_align = self.skip_attributes()?;
                let element_size = type_size(element);
                let alignment = type_alignment(element).max(1).max(attr_align.unwrap_or(1));
                loop {
                    let field_name = self.parse_identifier()?;
                    let mut count = base_len;
                    while *self.peek() == Token::BracketOpen {
                        self.advance();
                        let extra = self.parse_integer_constant()? as u16;
                        self.expect(Token::BracketClose)?;
                        count = count.saturating_mul(extra);
                    }
                    alignment_max = alignment_max.max(alignment);
                    offset = offset.div_ceil(alignment) * alignment;
                    bit_unit = None;
                    layout.fields.insert(field_name, StructField { member_type: element, offset, struct_tag: None, array_element: Some(pointee_of(element)?), bit_field: None });
                    offset += count.saturating_mul(element_size);
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::Semicolon)?;
                continue;
            }
            let field_type = self.parse_type()?;
            let struct_tag = self.last_struct_tag.take();
            // A declarator may carry `__attribute__((aligned(n)))` between the type
            // and the name (e.g. `u8 ATTRIBUTE_ALIGN(4) board_data[32];`); skip it,
            // honouring any requested alignment so subsequent offsets stay exact.
            let attr_align = self.skip_attributes()?;
            // One or more comma-separated declarators share the field type, e.g.
            // `f32 x, y, z;`. Each gets its own naturally-aligned offset.
            loop {
                // A function-pointer member `RET (*name)(params)` is a 4-byte pointer
                // (the `field_type` parsed above is just the return type). Consume the
                // declarator and record a pointer-typed member so `p->name` resolves.
                if *self.peek() == Token::ParenOpen && self.tokens.get(self.position + 1) == Some(&Token::Star) {
                    self.advance(); // `(`
                    self.advance(); // `*`
                    let pointer_name = self.parse_identifier()?;
                    self.expect(Token::ParenClose)?;
                    self.expect(Token::ParenOpen)?;
                    let mut depth = 1;
                    while depth > 0 {
                        match self.advance() {
                            Token::ParenOpen => depth += 1,
                            Token::ParenClose => depth -= 1,
                            Token::EndOfFile => return Err(Diagnostic::error("unterminated function-pointer member")),
                            _ => {}
                        }
                    }
                    bit_unit = None;
                    let alignment = 4u16;
                    alignment_max = alignment_max.max(alignment);
                    offset = offset.div_ceil(alignment) * alignment;
                    layout.fields.insert(pointer_name, StructField { member_type: Type::StructPointer { element_size: 0 }, offset, struct_tag: None, array_element: None, bit_field: None });
                    offset += 4;
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                    continue;
                }
                // An anonymous bit-field `type : width;` is padding with no member: a
                // positive width advances the current allocation unit (opening a fresh
                // one if it overflows, same packing as a named bit-field), and a zero
                // width (`int : 0;`) closes the open unit so the next bit-field starts a
                // new one at the next boundary.
                if *self.peek() == Token::Colon {
                    self.advance();
                    let width = self.parse_integer_constant()? as u8;
                    let unit_bits = (type_size(field_type) * 8) as u8;
                    if width == 0 {
                        bit_unit = None;
                    } else if width > unit_bits {
                        return Err(Diagnostic::error("an unsupported anonymous bit-field width (roadmap)"));
                    } else {
                        match bit_unit {
                            Some((unit_type, unit_offset, bits_used)) if unit_type == field_type && bits_used + width <= unit_bits => {
                                bit_unit = Some((field_type, unit_offset, bits_used + width));
                            }
                            Some((unit_type, ..)) if unit_type != field_type => {
                                return Err(Diagnostic::error("a struct mixing adjacent bit-field types is not supported yet (roadmap)"));
                            }
                            _ => {
                                let alignment = type_alignment(field_type).max(1).max(attr_align.unwrap_or(1));
                                let unit_offset = offset.div_ceil(alignment) * alignment;
                                offset = unit_offset + type_size(field_type);
                                alignment_max = alignment_max.max(alignment);
                                bit_unit = Some((field_type, unit_offset, width));
                            }
                        }
                    }
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                    continue;
                }
                let field_name = self.parse_identifier()?;
                // A bit-field `type name : width` packs `width` bits (MSB-first) into a
                // `sizeof(type)` storage unit shared by adjacent same-typed bit-fields;
                // it overflows into a fresh unit. (Mixed-type adjacent bit-fields follow
                // CodeWarrior's irregular packing and defer; member access defers too.)
                if self.eat_keyword(Token::Colon) {
                    let width = self.parse_integer_constant()? as u8;
                    let unit_bits = (type_size(field_type) * 8) as u8;
                    if width == 0 || width > unit_bits {
                        return Err(Diagnostic::error("an unsupported bit-field width (roadmap)"));
                    }
                    let (unit_offset, bit_offset) = match bit_unit {
                        Some((unit_type, unit_offset, bits_used)) if unit_type == field_type && bits_used + width <= unit_bits => {
                            bit_unit = Some((field_type, unit_offset, bits_used + width));
                            (unit_offset, bits_used)
                        }
                        Some((unit_type, ..)) if unit_type != field_type => {
                            return Err(Diagnostic::error("a struct mixing adjacent bit-field types is not supported yet (roadmap)"));
                        }
                        _ => {
                            let alignment = type_alignment(field_type).max(1).max(attr_align.unwrap_or(1));
                            let unit_offset = offset.div_ceil(alignment) * alignment;
                            offset = unit_offset + type_size(field_type);
                            alignment_max = alignment_max.max(alignment);
                            bit_unit = Some((field_type, unit_offset, width));
                            (unit_offset, 0)
                        }
                    };
                    layout.fields.insert(field_name, StructField { member_type: field_type, offset: unit_offset, struct_tag: None, array_element: None, bit_field: Some((bit_offset, width)) });
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                    continue;
                }
                // An ordinary member closes any open bit-field unit.
                bit_unit = None;
                // An array member `type name[N]` occupies `N` elements; its access
                // yields the array address rather than a loaded value.
                let mut array_element = None;
                let mut size = type_size(field_type);
                let element_size = size;
                if *self.peek() == Token::BracketOpen {
                    // A scalar array records its element type for indexed access. A
                    // struct-value array (`GXTexRegion TexRegions[8];`) or a pointer
                    // array (`u8 *mess_stack[8];`) has no scalar pointee — its element
                    // size still lays the array out correctly (4 bytes per pointer, the
                    // struct width per struct), so later members resolve; indexed
                    // element access defers in codegen rather than miscomputing.
                    if !matches!(field_type, Type::Struct { .. } | Type::Pointer(_) | Type::StructPointer { .. }) {
                        array_element = Some(pointee_of(field_type)?);
                    }
                    // One or more dimensions — `field[N]`, `field[R][C]`, … — occupy the
                    // product of the (constant-expression) lengths times the element
                    // size. (Member *access* of a multi-dimensional field still defers in
                    // codegen; the layout is needed so the rest of the struct registers.)
                    let mut total: u16 = 1;
                    while *self.peek() == Token::BracketOpen {
                        self.advance();
                        let count = self.parse_integer_constant()? as u16;
                        self.expect(Token::BracketClose)?;
                        total = total.saturating_mul(count);
                    }
                    size = total * element_size;
                }
                // Natural alignment: to the element's alignment (a struct value to its
                // own alignment, every other type to its size — for an array, that
                // element's).
                let alignment = type_alignment(field_type).max(1).max(attr_align.unwrap_or(1));
                alignment_max = alignment_max.max(alignment);
                offset = offset.div_ceil(alignment) * alignment;
                layout.fields.insert(field_name, StructField { member_type: field_type, offset, struct_tag: struct_tag.clone(), array_element, bit_field: None });
                offset += size;
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
            self.expect(Token::Semicolon)?;
        }
        self.expect(Token::BraceClose)?;
        // The struct size includes trailing padding to its own alignment.
        layout.size = offset.div_ceil(alignment_max) * alignment_max;
        layout.align = alignment_max as u8;
        Ok(layout)
    }

    /// Parse a `union { … }` body: every member starts at offset 0, so the union's
    /// size is its largest member and its alignment the strictest. Supports the
    /// common shape — a scalar, pointer, or struct-value member per line (each
    /// keeps its `struct_tag` so `u.member.field` still chains). The irregular
    /// cases — bit-fields, arrays, multiple declarators, nested inline aggregates —
    /// defer rather than risk a wrong offset.
    pub(crate) fn parse_union_body(&mut self) -> Compilation<StructLayout> {
        self.expect(Token::BraceOpen)?;
        let mut layout = StructLayout::default();
        let mut max_size: u16 = 0;
        let mut max_align: u16 = 1;
        while *self.peek() != Token::BraceClose {
            // An inline struct *variant* of the union (`struct [Tag] { … } name;`),
            // e.g. HsfObjectData's `mesh`. Register its layout under a tag so
            // `u.name.field` chains, then add it as a struct-value variant at offset 0.
            if *self.peek() == Token::KeywordStruct
                && (self.tokens.get(self.position + 1) == Some(&Token::BraceOpen)
                    || self.tokens.get(self.position + 2) == Some(&Token::BraceOpen))
            {
                self.advance(); // `struct`
                let tag = if matches!(self.peek(), Token::Identifier(_)) { Some(self.parse_identifier()?) } else { None };
                let inner = self.parse_struct_body()?;
                let inner_size = inner.size;
                let inner_align = (inner.align as u16).max(1);
                if !matches!(self.peek(), Token::Identifier(_)) {
                    return Err(Diagnostic::error("an anonymous inline struct variant in a union is not supported yet (roadmap)"));
                }
                let name = self.parse_identifier()?;
                let variant_tag = tag.unwrap_or_else(|| format!("@anon{}", self.structs.len()));
                self.structs.insert(variant_tag.clone(), inner);
                layout.fields.insert(name, StructField { member_type: Type::Struct { size: inner_size, align: inner_align as u8 }, offset: 0, struct_tag: Some(variant_tag), array_element: None, bit_field: None });
                max_size = max_size.max(inner_size);
                max_align = max_align.max(inner_align);
                self.expect(Token::Semicolon)?;
                continue;
            }
            let field_type = self.parse_type()?;
            let struct_tag = self.last_struct_tag.take();
            let attr_align = self.skip_attributes()?;
            let name = self.parse_identifier()?;
            // Bit-fields and multiple declarators in a union are uncommon and defer.
            if matches!(self.peek(), Token::Colon | Token::Comma) {
                return Err(Diagnostic::error("an irregular union member shape is not supported yet (roadmap)"));
            }
            // An array member occupies the product of its dimensions; it still
            // starts at offset 0, so it only widens the union.
            let mut array_element = None;
            let mut size = type_size(field_type);
            if *self.peek() == Token::BracketOpen {
                array_element = Some(pointee_of(field_type)?);
                let mut total: u16 = 1;
                while *self.peek() == Token::BracketOpen {
                    self.advance();
                    total = total.saturating_mul(self.parse_integer_constant()? as u16);
                    self.expect(Token::BracketClose)?;
                }
                size = total * type_size(field_type);
            }
            let align = type_alignment(field_type).max(1).max(attr_align.unwrap_or(1));
            layout.fields.insert(name, StructField { member_type: field_type, offset: 0, struct_tag, array_element, bit_field: None });
            max_size = max_size.max(size);
            max_align = max_align.max(align);
            self.expect(Token::Semicolon)?;
        }
        self.expect(Token::BraceClose)?;
        layout.size = max_size.div_ceil(max_align) * max_align;
        layout.align = max_align as u8;
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
        // A `static` (file-local) global's symbol is emitted among the locals, in
        // source order interleaved with each function's anonymous `@N` entries. Only
        // the common shape — all such data declared before any function — is modeled,
        // so defer the unit if an emittable static global follows a function.
        let mut seen_function = false;
        while *self.peek() != Token::EndOfFile {
            let start = self.position;
            let functions_before = functions.len();
            let globals_before = globals.len();
            if let Err(error) = self.parse_top_level_item(&mut globals, &mut functions, &mut prototypes) {
                // A declaration we can't parse (a typedef/struct/extern prototype or
                // qualified type from a preprocessed header) is skipped so the
                // function definitions can still be compiled; a function definition we
                // are expected to compile is propagated, deferring the unit honestly.
                self.position = start;
                if self.item_is_function_definition() {
                    return Err(error);
                }
                // An initialized data definition we cannot parse emits `.data` we
                // would otherwise drop — defer the unit rather than leave a partial
                // object (a silent DIFF).
                if self.item_is_initialized_definition() {
                    return Err(error);
                }
                // A skipped `static inline` function with an inline `asm {}` body
                // still contributes a local undefined symbol (mwcc cannot inline it).
                if let Some(name) = self.inline_asm_function_name() {
                    self.inline_asm_symbols.push(name);
                }
                // A skipped `typedef` still registers its alias name, so function
                // bodies that use the type as a pointer (`FILE *fp`) still parse.
                self.capture_skipped_typedef();
                self.skip_top_level_declaration();
            }
            if functions.len() > functions_before {
                seen_function = true;
            }
            // An emittable (non-`extern`, non-`const`) `static` global declared after
            // a function would need its local symbol interleaved among the functions'
            // `@N` entries — not yet modeled, so defer the unit honestly.
            if seen_function && globals[globals_before..].iter().any(|global| global.is_static && !global.is_const && !global.is_extern) {
                return Err(Diagnostic::error("a static global declared after a function is not supported yet (local-symbol ordering)"));
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
        prototypes: &mut Vec<(String, Type, Vec<Type>)>,
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
                // `typedef struct/union [Tag] { … } Alias;` registers the layout and the
                // alias->tag mapping (an anonymous one uses the alias as its tag). A union is
                // laid out like a struct — every member at offset 0 — so both share this path
                // and member access resolves identically. `union` is lexed as a plain identifier,
                // not a keyword. (A bodyless `typedef union Tag Alias;` falls through to parse_type.)
                let is_union_kw = matches!(self.peek(), Token::Identifier(word) if word == "union");
                let tagged = (*self.peek() == Token::KeywordStruct || is_union_kw)
                    && (self.tokens.get(self.position + 1) == Some(&Token::BraceOpen)
                        || self.tokens.get(self.position + 2) == Some(&Token::BraceOpen));
                if tagged {
                    self.advance(); // `struct` or `union`
                    let tag = if matches!(self.peek(), Token::Identifier(_)) { self.parse_identifier()? } else { String::new() };
                    let layout = if is_union_kw { self.parse_union_body()? } else { self.parse_struct_body()? };
                    // One or more comma-separated declarators: a value alias `Vec`
                    // or a pointer alias `*VecPtr`. The first value alias names an
                    // anonymous struct's tag.
                    let mut is_pointer = self.eat_keyword(Token::Star);
                    let mut alias = self.parse_identifier()?;
                    let tag = if tag.is_empty() { alias.clone() } else { tag };
                    self.structs.insert(tag.clone(), layout);
                    loop {
                        if is_pointer {
                            self.struct_pointer_typedefs.insert(alias, tag.clone());
                        } else {
                            self.struct_typedefs.insert(alias, tag.clone());
                        }
                        if !self.eat_keyword(Token::Comma) {
                            break;
                        }
                        is_pointer = self.eat_keyword(Token::Star);
                        alias = self.parse_identifier()?;
                    }
                    self.expect(Token::Semicolon)?;
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
                // An array typedef (`typedef float Mtx[3][4];`) — record the element
                // type and total element count so a member of this type lays out with
                // the right size (the `Type` model has no array variant).
                if *self.peek() == Token::BracketOpen {
                    let mut total: u16 = 1;
                    while *self.peek() == Token::BracketOpen {
                        self.advance();
                        let count = self.parse_integer_constant()? as u16;
                        self.expect(Token::BracketClose)?;
                        total = total.saturating_mul(count);
                    }
                    self.expect(Token::Semicolon)?;
                    self.array_typedefs.insert(name, (aliased, total));
                    return Ok(());
                }
                self.expect(Token::Semicolon)?;
                self.typedefs.insert(name, aliased);
                return Ok(());
            }
            // A `struct Name { ... }` definition registers a layout. A bare `;` ends
            // it; trailing declarators (`} var, var2;`) are struct-valued globals that
            // carry the tag so `var.field` resolves — the `static struct OSAlarmQueue
            // { ... } AlarmQueue;` shape. A `struct Name*` use (function return or
            // parameter) falls through to parse_type.
            // `union Tag { … };` — a top-level union declaration. A union is laid out like a
            // struct with every member at offset 0; register the layout under the tag so a
            // later `union Tag*` use resolves. A trailing union-value declarator is rare and
            // defers.
            if matches!(self.peek(), Token::Identifier(word) if word == "union") && self.tokens.get(self.position + 2) == Some(&Token::BraceOpen) {
                self.advance(); // `union`
                let tag = self.parse_identifier()?;
                let layout = self.parse_union_body()?;
                self.structs.insert(tag, layout);
                if *self.peek() == Token::Semicolon {
                    self.advance();
                    return Ok(());
                }
                return Err(Diagnostic::error("a union-definition global value is not supported yet (roadmap)"));
            }
            if *self.peek() == Token::KeywordStruct && self.tokens.get(self.position + 2) == Some(&Token::BraceOpen) {
                self.expect(Token::KeywordStruct)?;
                let tag = self.parse_identifier()?;
                let layout = self.parse_struct_body()?;
                self.structs.insert(tag.clone(), layout);
                if *self.peek() == Token::Semicolon {
                    self.advance();
                    return Ok(());
                }
                let struct_type = self
                    .struct_value_type(&tag)
                    .ok_or_else(|| Diagnostic::error("struct values are not supported yet — use a struct pointer"))?;
                loop {
                    let name = self.parse_identifier()?;
                    // Only a scalar, uninitialized struct global is in the subset; an
                    // array or initializer defers honestly (no miscompile).
                    if !matches!(self.peek(), Token::Semicolon | Token::Comma) {
                        return Err(Diagnostic::error("an initialized or array struct-definition global is not supported yet (roadmap)"));
                    }
                    self.variable_structs.insert(name.clone(), tag.clone());
                    globals.push(GlobalDeclaration { declared_type: struct_type, name, is_extern, is_static, array_length: None, initializer: None, is_const: false, address_initializer: None, data_bytes: None });
                    if *self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(Token::Semicolon)?;
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
                globals.push(GlobalDeclaration { declared_type: Type::StructPointer { element_size: 0 }, name: pointer_name, is_extern, is_static, array_length: None, initializer: None, is_const: false, address_initializer: None, data_bytes: None });
                return Ok(());
            }
            let name = self.parse_identifier()?;
            // `type name;`, `type name[N];`, or comma-separated declarators is a
            // global variable declaration. A `(` instead begins a function. (An
            // initialized global `type name = …;` is not in the subset yet and
            // falls through to the function path, which reports it.)
            if matches!(self.peek(), Token::Semicolon | Token::Comma | Token::BracketOpen | Token::Equals) {
                // A `const` file-scope global lands in a *read-only* section
                // (`.sdata2` if small, `.rodata` if large). Record it; the lowering
                // routes the supported shapes and defers the rest. `parse_type` set
                // this for the declared type and nothing since has reset it.
                let is_const = self.last_type_was_const;
                // A struct-typed global (pointer, value, or array) carries the struct
                // tag `parse_type` stashed, so `gp->field` / `g.field` / `arr[i].field`
                // resolve the member layout. Codegen handles the struct-pointer base
                // and defers the value/array bases (no miscompile).
                let global_struct_tag = self.last_struct_tag.clone();
                let mut declarator_name = name;
                loop {
                    // Array dimensions `[A][B]…`: each `[N]` is an explicit length,
                    // `[]` (only the first dimension) is inferred from the
                    // initializer; no brackets is a scalar. A multi-dimensional array
                    // flattens row-major to one element list of the dimensions' product.
                    let mut dimensions: Vec<Option<u16>> = Vec::new();
                    while *self.peek() == Token::BracketOpen {
                        self.advance();
                        let count = if *self.peek() == Token::BracketClose {
                            None
                        } else {
                            Some(self.parse_integer_constant()? as u16)
                        };
                        self.expect(Token::BracketClose)?;
                        dimensions.push(count);
                    }
                    // A pointer global initialized with addresses (`int *p = &g;` or
                    // a `{&a, &b}` array) is a set of data relocations, not constants.
                    // An array of word-field structs with a pointer field (a
                    // `{ "name", id }` table) flattens to the same address-initializer
                    // (pointer slots relocate, scalar slots are literal bytes).
                    let table_fields = if !dimensions.is_empty() && matches!(return_type, Type::Struct { .. }) {
                        global_struct_tag.as_deref().and_then(|tag| self.struct_pointer_table_fields(tag))
                    } else {
                        None
                    };
                    let mut address_initializer = None;
                    let mut initializer = None;
                    let mut data_bytes: Option<Vec<u8>> = None;
                    if matches!(return_type, Type::Pointer(_) | Type::StructPointer { .. }) && *self.peek() == Token::Equals {
                        self.advance();
                        address_initializer = Some(self.parse_address_initializer()?);
                    } else if table_fields.is_some() && *self.peek() == Token::Equals {
                        self.advance();
                        address_initializer = Some(self.parse_struct_pointer_table(table_fields.as_ref().unwrap())?);
                    } else if matches!(return_type, Type::Struct { .. }) && global_struct_tag.is_some() && *self.peek() == Token::Equals {
                        // A struct value/array initializer serializes each field at its
                        // own offset/width into the object's byte image — float, sub-word,
                        // and nested-struct fields all land correctly.
                        self.advance();
                        let tag = global_struct_tag.clone().unwrap();
                        data_bytes = Some(if dimensions.is_empty() {
                            self.parse_one_struct(&tag)?
                        } else {
                            self.parse_struct_array_initializer(&tag)?
                        });
                    } else if self.eat_keyword(Token::Equals) {
                        // `= <constant>` or `= { <constant>, ... }` (nested braces flatten).
                        initializer = Some(self.parse_constant_initializer(return_type)?);
                    }
                    let array_length = if dimensions.is_empty() {
                        None
                    } else if let Some(explicit) = dimensions.iter().copied().collect::<Option<Vec<u16>>>() {
                        // Every dimension is explicit: the length is their product.
                        Some(explicit.iter().map(|&dimension| dimension as u32).product::<u32>() as u16)
                    } else if let Some(bytes) = &data_bytes {
                        // A struct array's inferred length is its byte image divided by
                        // the element (struct) size.
                        let struct_size = match return_type {
                            Type::Struct { size, .. } => size.max(1) as usize,
                            _ => 1,
                        };
                        Some((bytes.len() / struct_size) as u16)
                    } else {
                        // An inferred dimension takes its length from the flat
                        // initializer (constant values or address elements).
                        match initializer.as_ref().map(Vec::len).or(address_initializer.as_ref().map(Vec::len)) {
                            Some(length) => Some(length as u16),
                            None => return Err(Diagnostic::error("an array with no length needs an initializer")),
                        }
                    };
                    if let Some(tag) = &global_struct_tag {
                        self.variable_structs.insert(declarator_name.clone(), tag.clone());
                    }
                    globals.push(GlobalDeclaration { declared_type: return_type, name: declarator_name, is_extern, is_static, array_length, initializer, is_const, address_initializer, data_bytes });
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
            let mut is_variadic = false;
            // `(void)` is an empty parameter list — but only when the `void` is the
            // whole list; `void *p` / `void (*f)()` are real first parameters.
            if *self.peek() == Token::KeywordVoid && self.tokens.get(self.position + 1) == Some(&Token::ParenClose) {
                self.advance();
            } else if *self.peek() != Token::ParenClose {
                loop {
                    // A `...` varargs marker ends the parameter list.
                    if *self.peek() == Token::Dot {
                        self.advance();
                        self.expect(Token::Dot)?;
                        self.expect(Token::Dot)?;
                        is_variadic = true;
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
                        parameters.push(Parameter { parameter_type: Type::StructPointer { element_size: 0 }, name });
                    } else {
                        // The name is optional (a prototype may write just the type).
                        let name = if matches!(self.peek(), Token::Identifier(_)) {
                            self.parse_identifier()?
                        } else {
                            String::new()
                        };
                        // `T a[]` / `T a[N]` is exactly `T* a` — C array-to-pointer parameter
                        // decay. Consume the `[...]` (the size is irrelevant for a parameter)
                        // and make the parameter a pointer to the element type.
                        let parameter_type = if *self.peek() == Token::BracketOpen {
                            self.advance(); // `[`
                            while !matches!(self.peek(), Token::BracketClose | Token::EndOfFile) {
                                self.advance(); // skip the optional size expression
                            }
                            self.expect(Token::BracketClose)?;
                            match parameter_type {
                                Type::Struct { size, .. } => Type::StructPointer { element_size: size },
                                scalar => Type::Pointer(pointee_of(scalar)?),
                            }
                        } else {
                            parameter_type
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
                self.advance(); // a prototype — record its return + parameter types, keep looking
                let parameter_types = parameters.iter().map(|parameter| parameter.parameter_type).collect();
                prototypes.push((name, return_type, parameter_types));
                return Ok(());
            }
            // A variadic function DEFINITION needs the variadic-register save
            // prologue (`stwu; bne cr1; stfd f1-f8; stw r3-r10; …`), which is not
            // modeled — defer rather than emit an empty body. (A variadic prototype
            // above is fine; only a definition reaches here.)
            if is_variadic {
                return Err(Diagnostic::error("a variadic function definition is not supported yet (the variadic-register save prologue)"));
            }
            functions.push(self.function_body(return_type, name, is_static, parameters)?);
        }
        Ok(())
    }

    /// Whether the item at the cursor is an initialized data *definition* — a
    /// top-level `= …` initializer before the `;` (e.g. `OvlInfo list[] = {…};`).
    /// Such a definition emits `.data`/`.sdata` bytes; if its initializer is outside
    /// the subset, skipping it would leave an incomplete object (a silent
    /// whole-object DIFF), so it must instead DEFER the unit like a function we
    /// cannot compile. Pure lookahead — consumes nothing.
    fn item_is_initialized_definition(&self) -> bool {
        let mut index = self.position;
        let (mut brace, mut paren, mut bracket) = (0i32, 0i32, 0i32);
        while let Some(token) = self.tokens.get(index) {
            let top_level = brace == 0 && paren == 0 && bracket == 0;
            match token {
                // A typedef never defines data, even with an `=` (none occur).
                Token::Identifier(word) if index == self.position && word == "typedef" => return false,
                // A top-level `=` before any body brace is an initializer: data.
                Token::Equals if top_level => return true,
                // A top-level `{` reached first is a function or aggregate body (no
                // preceding `= …`), not an initialized data definition — stop here so
                // the scan never runs past this item into the next one's initializer.
                Token::BraceOpen if top_level => return false,
                Token::BraceOpen => brace += 1,
                Token::BraceClose => brace -= 1,
                Token::ParenOpen => paren += 1,
                Token::ParenClose => paren -= 1,
                Token::BracketOpen => bracket += 1,
                Token::BracketClose => bracket -= 1,
                Token::Semicolon if brace == 0 && paren == 0 => return false,
                Token::EndOfFile => return false,
                _ => {}
            }
            index += 1;
        }
        false
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
    /// When a top-level `typedef` failed to parse (an unsupported struct/enum body,
    /// a qualified or aggregate base), still register its alias name as an opaque
    /// struct typedef. The alias is the last identifier at brace/paren/bracket depth
    /// zero before the terminating `;` — the shape of an aggregate or basic typedef
    /// (`typedef struct {…} FILE;`, `typedef … OSThread;`). A function-pointer
    /// typedef's name sits inside parens, so it is left alone. This lets function
    /// bodies that use the type as a pointer (`FILE *fp`, `OSThread *t`) parse
    /// instead of failing the whole translation unit on an "unknown type".
    fn capture_skipped_typedef(&mut self) {
        if !matches!(self.tokens.get(self.position), Some(Token::Identifier(word)) if word == "typedef") {
            return;
        }
        let mut index = self.position + 1;
        let (mut brace, mut paren, mut bracket) = (0i32, 0i32, 0i32);
        let mut alias: Option<String> = None;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::BraceOpen => brace += 1,
                Token::BraceClose => brace -= 1,
                Token::ParenOpen => paren += 1,
                Token::ParenClose => paren -= 1,
                Token::BracketOpen => bracket += 1,
                Token::BracketClose => bracket -= 1,
                Token::Semicolon if brace == 0 && paren == 0 => break,
                Token::Identifier(word) if brace == 0 && paren == 0 && bracket == 0 => alias = Some(word.clone()),
                Token::EndOfFile => break,
                _ => {}
            }
            index += 1;
        }
        if let Some(name) = alias {
            // `typedef` is itself an identifier here; never register it, and never
            // shadow a type the parser already knows.
            if name != "typedef"
                && !self.struct_typedefs.contains_key(&name)
                && !self.struct_pointer_typedefs.contains_key(&name)
                && !self.typedefs.contains_key(&name)
            {
                self.struct_typedefs.insert(name.clone(), name);
            }
        }
    }

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
    fn function_body(&mut self, return_type: Type, name: String, is_static: bool, parameters: Vec<Parameter>) -> Compilation<Function> {
        self.expect(Token::BraceOpen)?;

        // Zero or more local declarations precede the return statement. A
        // statement that begins with a type keyword is a local declaration;
        // `return` ends the body.
        let mut locals = Vec::new();
        while self.peek_is_type() {
            let declared_type = self.parse_type()?;
            // A volatile local's accesses must not be elided or folded (the straight-
            // line/value-tracking paths would, e.g. `volatile int x = 5; return x;` ->
            // `li r3,5` instead of mwcc's store-then-load). Defer until that is modeled.
            if self.last_type_was_volatile {
                return Err(Diagnostic::error("a volatile local is not supported yet (roadmap)"));
            }
            let struct_tag = self.last_struct_tag.take();
            // One or more comma-separated declarators, each optionally initialized.
            loop {
                // `T *p, *q;` — the first declarator's `*` was consumed into the declared
                // type; a later pointer declarator carries its own `*`, which mirrors it.
                // A mixed list (`int *p, q;`) or multi-level (`int *p, **q;`) would need a
                // per-declarator type, so defer those rather than mis-type a declarator.
                if *self.peek() == Token::Star {
                    if !matches!(declared_type, Type::Pointer(_) | Type::StructPointer { .. }) {
                        return Err(Diagnostic::error("a mixed pointer/non-pointer declarator list is not supported yet (roadmap)"));
                    }
                    self.advance();
                    if *self.peek() == Token::Star {
                        return Err(Diagnostic::error("a multi-level pointer declarator list is not supported yet (roadmap)"));
                    }
                }
                let name = self.parse_identifier()?;
                if let Some(tag) = &struct_tag {
                    self.variable_structs.insert(name.clone(), tag.clone());
                }
                // A local array `type buf[N];` — a frame slot of `N` elements. (A
                // multi-dimensional or initialized local array defers for now.)
                let array_length = if *self.peek() == Token::BracketOpen {
                    self.advance();
                    let length = self.parse_integer_constant()? as u16;
                    self.expect(Token::BracketClose)?;
                    if *self.peek() == Token::BracketOpen {
                        return Err(Diagnostic::error("a multi-dimensional local array is not supported yet (roadmap)"));
                    }
                    if *self.peek() == Token::Equals {
                        return Err(Diagnostic::error("an initialized local array is not supported yet (roadmap)"));
                    }
                    Some(length)
                } else {
                    None
                };
                let initializer = if array_length.is_none() && self.eat_keyword(Token::Equals) { Some(self.expression()?) } else { None };
                locals.push(LocalDeclaration { declared_type, name, initializer, array_length });
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
        // Parameters are register-resident variables just like locals: `a = expr` for a
        // parameter `a` is a reassignment (an Assign the value tracker can inline), NOT a memory
        // store. Without this, `int f(int a){ a += 5; return a; }` lowered to a Store{Variable(a)}
        // the codegen rejected. Globals are not in this set, so they stay Stores (observable).
        let mut local_names: std::collections::HashSet<&str> = locals.iter().map(|local| local.name.as_str()).collect();
        local_names.extend(parameters.iter().map(|parameter| parameter.name.as_str()));
        let mut statements = Vec::new();
        while !matches!(self.peek(), Token::KeywordReturn | Token::BraceClose) {
            // An empty statement (a lone `;`) produces no code — skip it.
            if *self.peek() == Token::Semicolon {
                self.advance();
                continue;
            }
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
            if matches!(self.peek(), Token::KeywordWhile | Token::KeywordDo | Token::KeywordFor) {
                statements.push(self.parse_loop_statement(&local_names)?);
                continue;
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
                // `else if (…)` chains another guard — since each branch returns, the
                // `else` is implied, so the loop's next turn parses it as the next
                // guard. A plain `else return w;` is the chain's default: a lone
                // if/else is the ternary select; an else ending an else-if chain
                // supplies the trailing return after the collected guards.
                if *self.peek() == Token::KeywordIf {
                    guards.push(GuardedReturn { condition, value });
                    continue;
                }
                // `if (c) return v; else return d;` is the guard `if (c) return v;`
                // with fall-through `d` — routed through the guard codegen (which
                // normalizes a negated `!c` to keep `v` as the in-place default, as
                // mwcc does) rather than emitted as a bare `(c) ? v : d` ternary.
                let otherwise = self.parse_guard_return()?;
                guards.push(GuardedReturn { condition, value });
                conditional_return = Some(otherwise);
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
            // A bare `return;` ends a `void` function with no value — like reaching the
            // closing brace, it produces no return value (the epilogue is the whole tail).
            if *self.peek() == Token::Semicolon {
                self.advance();
                None
            } else {
                let value = self.expression()?;
                self.expect(Token::Semicolon)?;
                Some(value)
            }
        } else {
            None
        };
        self.expect(Token::BraceClose)?;

        Ok(Function { return_type, name, is_static, parameters, locals, statements, guards, return_expression })
    }

    pub(crate) fn peek_is_type(&self) -> bool {
        self.token_starts_type(self.peek())
    }

    /// Whether `token` can begin a type name (a keyword, a specifier word, a
    /// qualifier, or a declared typedef) — used for both the current token and a
    /// one-token lookahead (e.g. the type inside a `(T*)` cast).
    pub(crate) fn token_starts_type(&self, token: &Token) -> bool {
        match token {
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
                    || self.struct_pointer_typedefs.contains_key(word)
            }
            _ => false,
        }
    }

    /// Consume a run of leading qualifier / storage-class words. `const` (noted in
    /// `last_type_was_const`) and `register` are ignored; `volatile` is deferred
    /// (its access semantics aren't modeled yet).
    pub(crate) fn skip_type_qualifiers(&mut self) -> Compilation<()> {
        self.last_type_was_const = false;
        self.last_type_was_volatile = false;
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
                    // `volatile` is transparent to layout and to a simple (un-elided)
                    // access — skip it so a struct with a volatile member (e.g.
                    // `vu32 mode;` in CARDControl) records its layout. A context that
                    // could mis-optimize a volatile access (a value-tracked local)
                    // guards on `last_type_was_volatile` and defers.
                    self.last_type_was_volatile = true;
                    self.advance();
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
        let first = self.factor()?;
        // `factor` lowers an `++`/`--` (prefix or postfix) to `target = target ± 1`;
        // as a value-free statement that routes to a store or local assignment.
        if let Expression::Assign { target, value } = first {
            self.expect(Token::Semicolon)?;
            return Ok(store_or_assign(*target, *value, local_names));
        }
        if let Some(operator) = self.peek_compound_assignment() {
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

    /// `return [value];` as a body statement (an early return), with an optional
    /// value (absent for `return;` in a void function).
    fn parse_return_statement(&mut self) -> Compilation<Statement> {
        self.expect(Token::KeywordReturn)?;
        let value = if *self.peek() == Token::Semicolon { None } else { Some(self.expression()?) };
        self.expect(Token::Semicolon)?;
        Ok(Statement::Return(value))
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

    /// A `while`, `do … while`, or `for` loop. The body is a `{ … }` block or a
    /// single statement; the for-clause `init`/`step` are expressions (an `i = 0`
    /// assignment, an `i++` increment), any of which may be empty.
    fn parse_loop_statement(&mut self, local_names: &std::collections::HashSet<&str>) -> Compilation<Statement> {
        match self.peek() {
            Token::KeywordWhile => {
                self.advance();
                self.expect(Token::ParenOpen)?;
                let condition = Some(self.expression()?);
                self.expect(Token::ParenClose)?;
                let body = self.parse_block_or_statement(local_names)?;
                Ok(Statement::Loop { kind: LoopKind::While, initializer: None, condition, step: None, body })
            }
            Token::KeywordDo => {
                self.advance();
                let body = self.parse_block_or_statement(local_names)?;
                self.expect(Token::KeywordWhile)?;
                self.expect(Token::ParenOpen)?;
                let condition = Some(self.expression()?);
                self.expect(Token::ParenClose)?;
                self.expect(Token::Semicolon)?;
                Ok(Statement::Loop { kind: LoopKind::DoWhile, initializer: None, condition, step: None, body })
            }
            Token::KeywordFor => {
                self.advance();
                self.expect(Token::ParenOpen)?;
                let initializer = (*self.peek() != Token::Semicolon).then(|| self.expression()).transpose()?;
                self.expect(Token::Semicolon)?;
                let condition = (*self.peek() != Token::Semicolon).then(|| self.expression()).transpose()?;
                self.expect(Token::Semicolon)?;
                let step = (*self.peek() != Token::ParenClose).then(|| self.expression()).transpose()?;
                self.expect(Token::ParenClose)?;
                let body = self.parse_block_or_statement(local_names)?;
                Ok(Statement::Loop { kind: LoopKind::For, initializer, condition, step, body })
            }
            other => Err(Diagnostic::error(format!("expected a loop keyword, found {other}"))),
        }
    }

    /// A `{ ... }` block, or a single (non-`return`) statement, as a conditional
    /// branch body.
    fn parse_block_or_statement(&mut self, local_names: &std::collections::HashSet<&str>) -> Compilation<Vec<Statement>> {
        if *self.peek() == Token::BraceOpen {
            return self.parse_block(local_names);
        }
        // An empty body — `while (c) ;` / `if (c) ;` — is no statements.
        if *self.peek() == Token::Semicolon {
            self.advance();
            return Ok(Vec::new());
        }
        if *self.peek() == Token::KeywordIf {
            return Ok(vec![self.parse_if_statement(local_names)?]);
        }
        if *self.peek() == Token::KeywordReturn {
            return Ok(vec![self.parse_return_statement()?]);
        }
        if matches!(self.peek(), Token::KeywordWhile | Token::KeywordDo | Token::KeywordFor) {
            return Ok(vec![self.parse_loop_statement(local_names)?]);
        }
        Ok(vec![self.parse_simple_statement(local_names)?])
    }

    /// A `{ ... }` block of simple statements, nested if-blocks, and `return`s. A
    /// trailing `if (c) { return X; } return Y;` collapses to `return (c ? X : Y)`
    /// (mwcc lowers an if-return followed by a return to a select), which also
    /// makes nested if-return chains fold into nested ternaries.
    fn parse_block(&mut self, local_names: &std::collections::HashSet<&str>) -> Compilation<Vec<Statement>> {
        self.expect(Token::BraceOpen)?;
        let mut statements = Vec::new();
        while *self.peek() != Token::BraceClose {
            // An empty statement (a lone `;`) produces no code — skip it.
            if *self.peek() == Token::Semicolon {
                self.advance();
                continue;
            }
            if *self.peek() == Token::KeywordIf {
                statements.push(self.parse_if_statement(local_names)?);
                continue;
            }
            if *self.peek() == Token::KeywordReturn {
                statements.push(self.parse_return_statement()?);
                continue;
            }
            if matches!(self.peek(), Token::KeywordWhile | Token::KeywordDo | Token::KeywordFor) {
                statements.push(self.parse_loop_statement(local_names)?);
                continue;
            }
            statements.push(self.parse_simple_statement(local_names)?);
        }
        collapse_if_return_chain(&mut statements);
        self.expect(Token::BraceClose)?;
        Ok(statements)
    }
}

/// Collapse a trailing `if (c) { return X; } return Y;` into `return (c ? X : Y)`,
/// repeatedly, so nested if-return chains fold into nested ternaries — matching
/// mwcc, which lowers an if-return immediately followed by a return to a select.
fn collapse_if_return_chain(statements: &mut Vec<Statement>) {
    while statements.len() >= 2 {
        let n = statements.len();
        let collapsible = matches!(&statements[n - 2],
            Statement::If { then_body, else_body, .. }
                if else_body.is_empty()
                    && matches!(then_body.as_slice(), [Statement::Return(Some(_))]))
            && matches!(&statements[n - 1], Statement::Return(Some(_)));
        if !collapsible {
            break;
        }
        let Some(Statement::Return(Some(when_false))) = statements.pop() else { unreachable!() };
        let Some(Statement::If { condition, then_body, .. }) = statements.pop() else { unreachable!() };
        let Some(Statement::Return(Some(when_true))) = then_body.into_iter().next() else { unreachable!() };
        statements.push(Statement::Return(Some(Expression::Conditional {
            condition: Box::new(condition),
            when_true: Box::new(when_true),
            when_false: Box::new(when_false),
        })));
    }
}
